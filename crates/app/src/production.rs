//! Concrete production composition for canonical Agent runs.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use codewhale_config::PromptPreferences;
use codewhale_context::{InstructionSource, ProductionPromptRequest, production_system_prompt};
use codewhale_deepseek::{
    DeepSeekAutoRouteFallback, DeepSeekAutoRouteInput, DeepSeekConnectionConfig,
    DeepSeekCredential, DeepSeekEndpoint, DeepSeekModelPort, DeepSeekTransport,
    SharedApiRequestBudget, TransportRetryPolicy, model_accounting_snapshot,
    official_model_capabilities, resolve_deepseek_auto_route, resume_api_request_budget,
};
use codewhale_protocol::agent_runtime::{
    AgentActor, CanonicalTranscript, ReasoningEffort, RunEnvironment, RunId, RunRequest,
    ToolDefinition,
};
use codewhale_protocol::run_api::{
    RunApiError, RunApiErrorCode, RunProductControls, StartRunCommand,
};
use codewhale_runtime::{
    AgentRuntime, ModelPort, RunReplay, RunStore, RuntimeEventSink, RuntimeRun, ToolExecutor,
};
use codewhale_state::StateStore;
use codewhale_tools::sandbox::SandboxPolicy;
use codewhale_tools::shell::ShellPolicy;
use codewhale_tools::{
    ProductionToolConfig, ProductionToolExecutionIdentity, ProductionToolExecutor,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    AgentApplication, ReplayOnlyModelPort, RunComposition, api_error, resume_needs_live_model,
};

const DEEPSEEK_PROVIDER: &str = "deepseek";
pub const DEFAULT_MAX_API_REQUESTS: u32 = 64;

/// Prompt inputs shared by every production root run.
#[derive(Debug, Clone)]
pub struct ProductionPromptConfig {
    pub preferences: PromptPreferences,
    pub instructions: Vec<InstructionSource>,
    pub skills_dir: Option<PathBuf>,
    pub project_context_pack_enabled: bool,
    pub verbosity: Option<String>,
    pub skills_scan_codewhale_only: bool,
    pub shell_binary: String,
}

impl Default for ProductionPromptConfig {
    fn default() -> Self {
        Self {
            preferences: PromptPreferences::default(),
            instructions: Vec::new(),
            skills_dir: None,
            project_context_pack_enabled: true,
            verbosity: None,
            skills_scan_codewhale_only: false,
            shell_binary: if cfg!(windows) { "powershell" } else { "sh" }.to_owned(),
        }
    }
}

/// Concrete, credential-optional configuration for the sole production app.
///
/// The connection settings, prompt settings, and tool policy are inert values.
/// `AgentApplication::production` opens the Store but does not bind the
/// credential or create a model transport. That happens only for a live
/// start/resume.
#[derive(Clone)]
pub struct ProductionApplicationConfig {
    state_db_path: Option<PathBuf>,
    deepseek: DeepSeekConnectionConfig,
    credential: Option<DeepSeekCredential>,
    http_client: Option<reqwest::Client>,
    tools: ProductionToolConfig,
    prompt: ProductionPromptConfig,
    composition_build_revision: String,
    default_max_api_requests: NonZeroU32,
}

impl ProductionApplicationConfig {
    fn new(deepseek: DeepSeekConnectionConfig, tools: ProductionToolConfig) -> Self {
        Self {
            state_db_path: None,
            deepseek,
            credential: None,
            http_client: None,
            tools,
            prompt: ProductionPromptConfig::default(),
            composition_build_revision: env!("DEEPSEEK_BUILD_VERSION").to_owned(),
            default_max_api_requests: NonZeroU32::new(DEFAULT_MAX_API_REQUESTS)
                .expect("production request budget default is non-zero"),
        }
    }

    /// Construct the official DeepSeek application defaults.
    ///
    /// The workspace placeholder is rebound for every run. Shell remains
    /// useful under the Full capability policy while dangerous commands stay
    /// blocked unless the run explicitly enables auto approval. The per-run
    /// workspace-write sandbox is applied by the composition.
    #[must_use]
    pub fn official() -> Self {
        Self::new(
            DeepSeekConnectionConfig {
                endpoint: DeepSeekEndpoint::Official,
                strict_tools: false,
                response_header_timeout: Duration::from_secs(45),
                stream_idle_timeout: Duration::from_secs(900),
                retry: TransportRetryPolicy {
                    max_retries: 3,
                    initial_delay: Duration::from_secs(1),
                    max_delay: Duration::from_secs(60),
                    exponential_base: 2.0,
                },
            },
            ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full),
        )
    }

    /// Replace the official connection only for resolved host policy or an
    /// explicit loopback fixture. This does not bind a credential.
    #[must_use]
    pub fn with_deepseek_connection(mut self, connection: DeepSeekConnectionConfig) -> Self {
        self.deepseek = connection;
        self
    }

    /// Replace the app-owned tool defaults with one resolved host snapshot.
    #[must_use]
    pub fn with_tool_config(mut self, tools: ProductionToolConfig) -> Self {
        self.tools = tools;
        self
    }

    #[must_use]
    pub fn with_state_db_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.state_db_path = Some(path.into());
        self
    }

    #[must_use]
    pub fn with_credential(mut self, credential: DeepSeekCredential) -> Self {
        self.credential = Some(credential);
        self
    }

    pub fn with_api_key(
        self,
        api_key: impl Into<String>,
    ) -> Result<Self, ProductionApplicationError> {
        let credential = DeepSeekCredential::new(api_key).map_err(|error| {
            ProductionApplicationError::InvalidDeepSeekConfig(error.to_string())
        })?;
        Ok(self.with_credential(credential))
    }

    #[must_use]
    pub fn with_prompt(mut self, prompt: ProductionPromptConfig) -> Self {
        self.prompt = prompt;
        self
    }

    /// Replace the HTTP client without changing DeepSeek transport ownership.
    /// This is useful for host TLS/proxy policy and deterministic loopback tests.
    #[must_use]
    pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Override only the non-secret composition revision identity.
    #[must_use]
    pub fn with_composition_build_revision(mut self, revision: impl Into<String>) -> Self {
        self.composition_build_revision = revision.into();
        self
    }

    #[must_use]
    pub fn with_default_max_api_requests(mut self, limit: NonZeroU32) -> Self {
        self.default_max_api_requests = limit;
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProductionApplicationError {
    #[error("DeepSeek production connection configuration is invalid: {0}")]
    InvalidDeepSeekConfig(String),
    #[error("failed to open the canonical StateStore: {0}")]
    StateStore(String),
}

struct ProductionComposition {
    deepseek: DeepSeekConnectionConfig,
    credential: Option<DeepSeekCredential>,
    http_client: Option<reqwest::Client>,
    tools: ProductionToolConfig,
    prompt: ProductionPromptConfig,
    composition_build_revision: String,
    default_max_api_requests: NonZeroU32,
}

impl AgentApplication {
    /// Construct the sole production composition root without requiring a Key.
    pub fn production(
        config: ProductionApplicationConfig,
    ) -> Result<Self, ProductionApplicationError> {
        config.deepseek.validate().map_err(|error| {
            ProductionApplicationError::InvalidDeepSeekConfig(error.to_string())
        })?;
        let store = Arc::new(
            StateStore::open(config.state_db_path)
                .map_err(|error| ProductionApplicationError::StateStore(error.to_string()))?,
        );
        let composition = Arc::new(ProductionComposition {
            deepseek: config.deepseek,
            credential: config.credential,
            http_client: config.http_client,
            tools: config.tools,
            prompt: config.prompt,
            composition_build_revision: config.composition_build_revision,
            default_max_api_requests: config.default_max_api_requests,
        });
        Ok(Self::from_parts(store, composition))
    }
}

#[async_trait]
impl RunComposition for ProductionComposition {
    async fn start(
        &self,
        command: StartRunCommand,
        store: Arc<dyn RunStore>,
        sink: Arc<dyn RuntimeEventSink>,
    ) -> Result<RuntimeRun, RunApiError> {
        let deadline_unix_ms = command
            .limits
            .wall_time_ms
            .map(|duration| unix_ms_now().saturating_add(duration));
        let workspace = canonical_start_workspace(&command.workspace)?;
        let explicit_capability = command
            .model
            .as_deref()
            .map(official_model_capabilities)
            .transpose()
            .map_err(|error| invalid_request(error.to_string()))?;
        let tool_config = tool_config_for_run(&self.tools, &workspace, &command.controls)?;
        let tool_identity = tool_config.execution_identity();

        let request_budget = SharedApiRequestBudget::new(
            command
                .max_api_requests
                .unwrap_or(self.default_max_api_requests),
        );
        let transport = self.bind_live_transport(request_budget.clone())?;
        let (model, reasoning_effort) = if let Some(capability) = explicit_capability {
            (capability.model.to_owned(), command.reasoning_effort)
        } else {
            let fallback = DeepSeekAutoRouteFallback::for_request(
                &command.input,
                Some(command.reasoning_effort),
            );
            let selected = resolve_deepseek_auto_route(
                &transport,
                DeepSeekAutoRouteInput {
                    latest_request: &command.input,
                    recent_context: "",
                    session_mode: "agent",
                    selected_model_mode: "auto",
                    selected_thinking_mode: reasoning_effort_label(command.reasoning_effort),
                    fallback,
                },
            )
            .await
            .map_err(|error| invalid_request(error.to_string()))?;
            (
                selected.model().to_owned(),
                selected
                    .reasoning_effort()
                    .unwrap_or(command.reasoning_effort),
            )
        };

        let capability = official_model_capabilities(&model)
            .map_err(|error| invalid_request(error.to_string()))?;
        let max_output_tokens = capability
            .resolve_output_tokens(command.max_output_tokens)
            .map_err(|error| invalid_request(error.to_string()))?;
        let tool_executor: Arc<dyn ToolExecutor> =
            Arc::new(ProductionToolExecutor::new(tool_config));
        let model_port: Arc<dyn ModelPort> =
            Arc::new(DeepSeekModelPort::new(transport, request_budget.clone()));
        let runtime = Arc::new(AgentRuntime::new(model_port, tool_executor, sink, store));
        let tool_catalog =
            runtime.tool_definitions(&command.tool_policy, 0, command.limits.max_depth);
        let tool_catalog_sha256 = tool_catalog_sha256(&tool_catalog);
        let execution_fingerprint_sha256 =
            self.execution_fingerprint_sha256(&model, &tool_identity, &tool_catalog_sha256);
        let system_prompt = self.system_prompt(&workspace, &model, !tool_catalog.is_empty());
        let accounting_baseline = model_accounting_snapshot(&request_budget);
        let request = RunRequest {
            run_id: None,
            parent_run_id: None,
            model,
            input: command.input,
            system_prompt,
            transcript: CanonicalTranscript::default(),
            reasoning_effort,
            max_output_tokens: Some(max_output_tokens),
            streaming: command.streaming,
            actor: AgentActor::default(),
            deadline_unix_ms,
            tool_policy: command.tool_policy,
            limits: command.limits,
            environment: RunEnvironment {
                workspace: stable_path(&workspace),
                provider: DEEPSEEK_PROVIDER.to_owned(),
                tool_catalog_sha256: Some(tool_catalog_sha256),
                execution_fingerprint_sha256: Some(execution_fingerprint_sha256),
                auto_approve: command.controls.auto_approve,
                trust_mode: command.controls.trust_mode,
                allow_sandbox_elevation: command.controls.allow_sandbox_elevation,
                sandbox: command.controls.sandbox,
            },
            accounting_baseline,
        };
        Ok(runtime.start(request))
    }

    async fn resume(
        &self,
        run_id: RunId,
        replay: RunReplay,
        store: Arc<dyn RunStore>,
        sink: Arc<dyn RuntimeEventSink>,
    ) -> Result<RuntimeRun, RunApiError> {
        let request = &replay.snapshot.request;
        if request.run_id.as_ref() != Some(&run_id) {
            return Err(environment_mismatch(
                &run_id,
                "persisted RunRequest id does not match the requested run",
            ));
        }
        if request.environment.provider != DEEPSEEK_PROVIDER {
            return Err(environment_mismatch(
                &run_id,
                "run_resume_provider_mismatch：persisted run provider is not the official DeepSeek production provider",
            ));
        }
        let workspace = canonical_resume_workspace(&run_id, &request.environment.workspace)?;
        let capability = official_model_capabilities(&request.model)
            .map_err(|error| environment_mismatch(&run_id, error.to_string()))?;
        let expected_max_output = capability
            .resolve_output_tokens(request.max_output_tokens)
            .map_err(|error| environment_mismatch(&run_id, error.to_string()))?;
        if request.max_output_tokens != Some(expected_max_output) {
            return Err(environment_mismatch(
                &run_id,
                "persisted run does not contain an exact resolved max_output_tokens value",
            ));
        }

        let controls = RunProductControls {
            auto_approve: request.environment.auto_approve,
            trust_mode: request.environment.trust_mode,
            allow_sandbox_elevation: request.environment.allow_sandbox_elevation,
            sandbox: request.environment.sandbox.clone(),
        };
        let tool_config = tool_config_for_run(&self.tools, &workspace, &controls)?;
        let tool_identity = tool_config.execution_identity();
        let tool_executor: Arc<dyn ToolExecutor> =
            Arc::new(ProductionToolExecutor::new(tool_config));
        let (request_budget, exhausted) = resume_api_request_budget(&replay.snapshot.accounting);
        let model_port: Arc<dyn ModelPort> = if !exhausted && resume_needs_live_model(&replay) {
            let transport = self.bind_live_transport(request_budget.clone())?;
            Arc::new(DeepSeekModelPort::new(transport, request_budget))
        } else {
            Arc::new(ReplayOnlyModelPort)
        };
        let runtime = Arc::new(AgentRuntime::new(model_port, tool_executor, sink, store));
        let tool_catalog =
            runtime.tool_definitions(&request.tool_policy, 0, request.limits.max_depth);
        let current_catalog_sha256 = tool_catalog_sha256(&tool_catalog);
        if request.environment.tool_catalog_sha256.as_deref()
            != Some(current_catalog_sha256.as_str())
        {
            return Err(environment_mismatch(
                &run_id,
                "run_resume_tool_catalog_mismatch：model-visible tool catalog does not match the persisted run",
            ));
        }
        let persisted_fingerprint = request
            .environment
            .execution_fingerprint_sha256
            .as_deref()
            .ok_or_else(|| {
                environment_mismatch(
                    &run_id,
                    "run_resume_fingerprint_missing：persisted run has no production execution fingerprint",
                )
            })?;
        let current_fingerprint = self.execution_fingerprint_sha256(
            &request.model,
            &tool_identity,
            &current_catalog_sha256,
        );
        if persisted_fingerprint != current_fingerprint {
            return Err(environment_mismatch(
                &run_id,
                "run_resume_fingerprint_mismatch：production execution fingerprint does not match the persisted run",
            ));
        }
        Ok(runtime.resume(run_id))
    }
}

impl ProductionComposition {
    fn bind_live_transport(
        &self,
        request_budget: SharedApiRequestBudget,
    ) -> Result<DeepSeekTransport, RunApiError> {
        let credential = self.credential.clone().ok_or_else(|| {
            invalid_request(
                "deepseek_credential_missing：该运行需要访问官方 DeepSeek API，但未配置测试或生产 Key",
            )
        })?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http_client = match &self.http_client {
            Some(client) => client.clone(),
            None => reqwest::Client::builder()
                .build()
                .map_err(|error| invalid_request(format!("http_client_invalid：{error}")))?,
        };
        self.deepseek
            .clone()
            .bind(http_client, credential, request_budget)
            .map_err(|error| invalid_request(error.to_string()))
    }

    fn system_prompt(
        &self,
        workspace: &Path,
        model: &str,
        tool_mode: bool,
    ) -> codewhale_protocol::agent_runtime::SystemPrompt {
        production_system_prompt(ProductionPromptRequest {
            workspace,
            model,
            preferences: &self.prompt.preferences,
            instructions: &self.prompt.instructions,
            skills_dir: self.prompt.skills_dir.as_deref(),
            project_context_pack_enabled: self.prompt.project_context_pack_enabled,
            verbosity: self.prompt.verbosity.as_deref(),
            skills_scan_codewhale_only: self.prompt.skills_scan_codewhale_only,
            shell_binary: &self.prompt.shell_binary,
            tool_mode,
        })
    }

    fn execution_fingerprint_sha256(
        &self,
        model: &str,
        tool_identity: &ProductionToolExecutionIdentity,
        tool_catalog_sha256: &str,
    ) -> String {
        let value = ProductionExecutionFingerprint {
            schema: 1,
            composition_build_revision: &self.composition_build_revision,
            provider: DEEPSEEK_PROVIDER,
            model,
            endpoint_root_sha256: sha256(self.deepseek.endpoint.root().as_bytes()),
            strict_tools: self.deepseek.strict_tools,
            response_header_timeout_ms: duration_millis(self.deepseek.response_header_timeout),
            stream_idle_timeout_ms: duration_millis(self.deepseek.stream_idle_timeout),
            retry: ProductionRetryIdentity {
                max_retries: self.deepseek.retry.max_retries,
                initial_delay_ms: duration_millis(self.deepseek.retry.initial_delay),
                max_delay_ms: duration_millis(self.deepseek.retry.max_delay),
                exponential_base: self.deepseek.retry.exponential_base,
            },
            tool_identity,
            tool_catalog_sha256,
        };
        let bytes = serde_json::to_vec(&value).expect("production fingerprint is serializable");
        sha256(&bytes)
    }
}

#[derive(Serialize)]
struct ProductionExecutionFingerprint<'a> {
    schema: u32,
    composition_build_revision: &'a str,
    provider: &'a str,
    model: &'a str,
    endpoint_root_sha256: String,
    strict_tools: bool,
    response_header_timeout_ms: u64,
    stream_idle_timeout_ms: u64,
    retry: ProductionRetryIdentity,
    tool_identity: &'a ProductionToolExecutionIdentity,
    tool_catalog_sha256: &'a str,
}

#[derive(Serialize)]
struct ProductionRetryIdentity {
    max_retries: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    exponential_base: f64,
}

fn canonical_start_workspace(raw: &str) -> Result<PathBuf, RunApiError> {
    let path = Path::new(raw);
    let canonical = path.canonicalize().map_err(|error| {
        invalid_request(format!(
            "workspace_invalid：无法规范化工作区 {}：{error}",
            path.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(invalid_request(format!(
            "workspace_invalid：工作区不是目录：{}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn canonical_resume_workspace(run_id: &RunId, raw: &str) -> Result<PathBuf, RunApiError> {
    let path = Path::new(raw);
    let canonical = path.canonicalize().map_err(|error| {
        environment_mismatch(
            run_id,
            format!(
                "run_resume_workspace_mismatch：persisted workspace {} cannot be canonicalized: {error}",
                path.display()
            ),
        )
    })?;
    if !canonical.is_dir() || stable_path(&canonical) != raw {
        return Err(environment_mismatch(
            run_id,
            "run_resume_workspace_mismatch：persisted workspace is no longer the same canonical directory",
        ));
    }
    Ok(canonical)
}

fn tool_config_for_run(
    base: &ProductionToolConfig,
    workspace: &Path,
    controls: &RunProductControls,
) -> Result<ProductionToolConfig, RunApiError> {
    let mut config = base
        .clone()
        .with_workspace(workspace.to_path_buf())
        .with_trust_mode(controls.trust_mode)
        .with_auto_approve(controls.auto_approve);
    if controls.allow_sandbox_elevation {
        config = config.with_elevated_sandbox_policy(SandboxPolicy::DangerFullAccess);
    } else {
        let policy = match controls.sandbox.as_deref() {
            None => SandboxPolicy::WorkspaceWrite {
                writable_roots: vec![workspace.to_path_buf()],
                network_access: true,
                exclude_tmpdir: false,
                exclude_slash_tmp: false,
            },
            Some(sandbox) => match sandbox {
                "read-only" => SandboxPolicy::ReadOnly,
                "workspace-write" => SandboxPolicy::WorkspaceWrite {
                    writable_roots: vec![workspace.to_path_buf()],
                    network_access: true,
                    exclude_tmpdir: false,
                    exclude_slash_tmp: false,
                },
                "danger-full-access" => SandboxPolicy::DangerFullAccess,
                "external-sandbox" => SandboxPolicy::ExternalSandbox {
                    network_access: true,
                },
                other => {
                    return Err(invalid_request(format!(
                        "sandbox_invalid：不支持的 sandbox '{other}'"
                    )));
                }
            },
        };
        config = config.with_elevated_sandbox_policy(policy);
    }
    Ok(config)
}

fn tool_catalog_sha256(catalog: &[ToolDefinition]) -> String {
    let bytes = serde_json::to_vec(catalog).expect("canonical tool catalog is serializable");
    sha256(&bytes)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn stable_path(path: &Path) -> String {
    path.display().to_string()
}

fn reasoning_effort_label(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Auto => "auto",
        ReasoningEffort::Max => "max",
    }
}

fn invalid_request(message: impl Into<String>) -> RunApiError {
    api_error(RunApiErrorCode::InvalidRequest, message, None, None)
}

fn environment_mismatch(run_id: &RunId, message: impl Into<String>) -> RunApiError {
    api_error(
        RunApiErrorCode::RunEnvironmentMismatch,
        message,
        Some(run_id.clone()),
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use codewhale_deepseek::OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS;
    use codewhale_protocol::agent_runtime::{
        ModelAccounting, ModelFinishReason, ModelOutput, ModelRequest, ModelStreamEvent, RunLimits,
        ToolArguments, ToolInvocation, ToolOutcome, ToolPolicy, Usage,
    };
    use codewhale_protocol::run_api::{
        RUN_API_SCHEMA_VERSION, RunCommand, RunCommandEnvelope, RunCommandResponse,
        RunCommandResult, RunView,
    };
    use codewhale_runtime::{
        CancellationToken, ModelPortError, ModelStream, NullEventSink, ToolExecutionError,
    };
    use codewhale_tools::PRODUCTION_TOOL_NAMES;
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        path: String,
        body: Value,
    }

    struct MockDeepSeekServer {
        root: String,
        requests: Arc<StdMutex<Vec<CapturedRequest>>>,
        task: JoinHandle<()>,
    }

    impl MockDeepSeekServer {
        async fn start(responses: Vec<Value>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind DeepSeek fixture");
            let address = listener.local_addr().expect("fixture address");
            let requests = Arc::new(StdMutex::new(Vec::new()));
            let captured = requests.clone();
            let task = tokio::spawn(async move {
                for response in responses {
                    let (mut socket, _) =
                        tokio::time::timeout(Duration::from_secs(5), listener.accept())
                            .await
                            .expect("fixture request timeout")
                            .expect("accept fixture request");
                    let request = read_request(&mut socket).await;
                    captured.lock().expect("capture lock").push(request);
                    let body = serde_json::to_vec(&response).expect("serialize fixture response");
                    let header = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    socket
                        .write_all(header.as_bytes())
                        .await
                        .expect("write fixture header");
                    socket.write_all(&body).await.expect("write fixture body");
                }
            });
            Self {
                root: format!("http://{address}/v1"),
                requests,
                task,
            }
        }

        async fn finish(self) -> Vec<CapturedRequest> {
            tokio::time::timeout(Duration::from_secs(5), self.task)
                .await
                .expect("fixture server completes")
                .expect("fixture server task");
            Arc::try_unwrap(self.requests)
                .expect("fixture request owner")
                .into_inner()
                .expect("fixture request lock")
        }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> CapturedRequest {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("read fixture request");
            assert!(read > 0, "fixture request ended before headers");
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let request_line = headers.lines().next().expect("request line");
        let path = request_line
            .split_whitespace()
            .nth(1)
            .expect("request path")
            .to_owned();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
            .expect("fixture content length");
        while bytes.len() < header_end + content_length {
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("read fixture body");
            assert!(read > 0, "fixture request ended before body");
            bytes.extend_from_slice(&chunk[..read]);
        }
        let body = serde_json::from_slice(&bytes[header_end..header_end + content_length])
            .expect("fixture JSON request");
        CapturedRequest { path, body }
    }

    async fn quiet_loopback() -> (String, JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind zero-request fixture");
        let root = format!(
            "http://{}/v1",
            listener.local_addr().expect("zero-request address")
        );
        let task = tokio::spawn(async move {
            let mut accepted = 0;
            while let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_millis(150), listener.accept()).await
            {
                accepted += 1;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    )
                    .await;
            }
            accepted
        });
        (root, task)
    }

    fn connection(root: &str, strict_tools: bool) -> DeepSeekConnectionConfig {
        DeepSeekConnectionConfig {
            endpoint: DeepSeekEndpoint::loopback_fixture(root).expect("loopback endpoint"),
            strict_tools,
            response_header_timeout: Duration::from_secs(2),
            stream_idle_timeout: Duration::from_secs(2),
            retry: TransportRetryPolicy::disabled(),
        }
    }

    fn config(
        state_path: &Path,
        connection: DeepSeekConnectionConfig,
        credential: bool,
    ) -> ProductionApplicationConfig {
        let config = ProductionApplicationConfig::official()
            .with_state_db_path(state_path)
            .with_deepseek_connection(connection)
            .with_composition_build_revision("test-composition");
        if credential {
            config
                .with_api_key("test-deepseek-key")
                .expect("test credential")
        } else {
            config
        }
    }

    fn start_command(workspace: &Path, model: Option<&str>) -> StartRunCommand {
        StartRunCommand {
            input: "修复真实边界问题".to_owned(),
            workspace: workspace.display().to_string(),
            model: model.map(str::to_owned),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: None,
            max_api_requests: None,
            streaming: false,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits {
                max_depth: 0,
                model_event_idle_ms: None,
                ..RunLimits::default()
            },
            controls: RunProductControls {
                auto_approve: true,
                trust_mode: false,
                allow_sandbox_elevation: false,
                sandbox: Some("workspace-write".to_owned()),
            },
        }
    }

    fn envelope(request_id: &str, command: RunCommand) -> RunCommandEnvelope {
        RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: request_id.to_owned(),
            command,
        }
    }

    fn run_result(response: RunCommandResponse) -> RunView {
        match response.result {
            RunCommandResult::Run { run } => *run,
            other => panic!("expected run result, got {other:?}"),
        }
    }

    fn error_result(response: RunCommandResponse) -> RunApiError {
        match response.result {
            RunCommandResult::Error { error } => error,
            other => panic!("expected error result, got {other:?}"),
        }
    }

    fn response(model: &str, content: &str, input_tokens: u64, output_tokens: u64) -> Value {
        json!({
            "id": format!("fixture-{input_tokens}"),
            "model": model,
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": content},
            }],
            "usage": {
                "prompt_tokens": input_tokens,
                "completion_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            },
        })
    }

    async fn wait_terminal(store: &dyn RunStore, run_id: &RunId) -> RunReplay {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let replay = store
                    .load(run_id)
                    .await
                    .expect("load run")
                    .expect("run exists");
                if replay.snapshot.terminal.is_some() {
                    return replay;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("run reaches terminal")
    }

    struct OneShotModel;

    #[async_trait]
    impl ModelPort for OneShotModel {
        async fn stream(
            &self,
            _request: ModelRequest,
        ) -> Result<Box<dyn ModelStream>, ModelPortError> {
            Ok(Box::new(OneShotStream { emitted: false }))
        }

        async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
            Ok(ModelAccounting {
                sealed: seal,
                complete: true,
                usage_complete: true,
                ..ModelAccounting::default()
            })
        }
    }

    struct OneShotStream {
        emitted: bool,
    }

    #[async_trait]
    impl ModelStream for OneShotStream {
        async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
            if self.emitted {
                return None;
            }
            self.emitted = true;
            Some(Ok(ModelStreamEvent::Completed {
                output: ModelOutput {
                    content: "本地完成".to_owned(),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: Usage::default(),
                },
            }))
        }
    }

    struct NoTools;

    #[async_trait]
    impl ToolExecutor for NoTools {
        fn definitions(&self) -> Vec<ToolDefinition> {
            Vec::new()
        }

        async fn execute(
            &self,
            _invocation: ToolInvocation,
            _cancellation: CancellationToken,
        ) -> Result<ToolOutcome, ToolExecutionError> {
            unreachable!("no tool definitions")
        }
    }

    #[tokio::test]
    async fn production_constructs_reads_and_replays_terminal_without_key() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let state_path = temp.path().join("state.db");
        let app = AgentApplication::production(
            ProductionApplicationConfig::official().with_state_db_path(&state_path),
        )
        .expect("construct without credential");
        let store = app.store.clone();
        let runtime = Arc::new(AgentRuntime::new(
            Arc::new(OneShotModel),
            Arc::new(NoTools),
            Arc::new(NullEventSink),
            store.clone(),
        ));
        let mut request = RunRequest::new("本地终态", "本地测试 prompt");
        request.model = "deepseek-v4-flash".to_owned();
        request.max_output_tokens = Some(OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS);
        request.tool_policy.enabled = false;
        request.environment.workspace = temp
            .path()
            .canonicalize()
            .expect("canonical workspace")
            .display()
            .to_string();
        request.environment.provider = DEEPSEEK_PROVIDER.to_owned();
        let run = runtime.start(request);
        let run_id = run.run_id.clone();
        run.wait().await.expect("local terminal");
        let frozen = store
            .load(&run_id)
            .await
            .expect("load terminal")
            .expect("terminal run")
            .events;

        let get = run_result(
            app.execute(envelope(
                "get-no-key",
                RunCommand::Get {
                    run_id: run_id.clone(),
                },
            ))
            .await,
        );
        assert!(get.terminal.is_some());
        let events = app
            .execute(envelope(
                "events-no-key",
                RunCommand::Events {
                    run_id: run_id.clone(),
                    after_sequence: 0,
                },
            ))
            .await;
        assert!(matches!(
            events.result,
            RunCommandResult::Events { ref events, .. } if events == &frozen
        ));
        let resumed = run_result(
            app.execute(envelope(
                "resume-no-key",
                RunCommand::Resume {
                    run_id: run_id.clone(),
                    expected_workspace: None,
                },
            ))
            .await,
        );
        assert_eq!(resumed.terminal, get.terminal);
        assert_eq!(
            store
                .load(&run_id)
                .await
                .expect("reload terminal")
                .expect("terminal remains")
                .events,
            frozen
        );
    }

    #[tokio::test]
    async fn live_start_without_key_is_typed_and_creates_no_run() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let app = AgentApplication::production(
            ProductionApplicationConfig::official()
                .with_state_db_path(temp.path().join("state.db")),
        )
        .expect("construct without key");
        let error = error_result(
            app.execute(envelope(
                "missing-key",
                RunCommand::Start(start_command(temp.path(), Some("deepseek-v4-flash"))),
            ))
            .await,
        );
        assert_eq!(error.code, RunApiErrorCode::InvalidRequest);
        assert!(error.message.contains("deepseek_credential_missing"));
        assert!(
            app.store
                .latest_resumable_run(
                    &temp
                        .path()
                        .canonicalize()
                        .expect("canonical workspace")
                        .display()
                        .to_string()
                )
                .await
                .expect("query runs")
                .is_none()
        );
    }

    #[tokio::test]
    async fn unsupported_model_alias_fails_before_any_http_request() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let (root, accepted) = quiet_loopback().await;
        let app = AgentApplication::production(config(
            &temp.path().join("state.db"),
            connection(&root, true),
            true,
        ))
        .expect("production app");
        let error = error_result(
            app.execute(envelope(
                "alias",
                RunCommand::Start(start_command(temp.path(), Some("deepseek-chat"))),
            ))
            .await,
        );
        assert_eq!(error.code, RunApiErrorCode::InvalidRequest);
        assert!(
            error
                .message
                .contains("unsupported official DeepSeek model")
        );
        assert_eq!(accepted.await.expect("zero request fixture"), 0);
    }

    #[tokio::test]
    async fn auto_route_and_root_share_one_hard_ledger_and_persist_exact_host_fields() {
        let server = MockDeepSeekServer::start(vec![
            response(
                "deepseek-v4-flash",
                r#"{"provider":"deepseek","model":"deepseek-v4-pro","thinking":"max"}"#,
                7,
                2,
            ),
            response("deepseek-v4-pro", "完成", 11, 3),
        ])
        .await;
        let temp = tempfile::tempdir().expect("temp workspace");
        let mut command = start_command(temp.path(), None);
        command.tool_policy.allowed = Some(vec!["read_file".to_owned()]);
        command.controls.trust_mode = true;
        command.controls.sandbox = Some("workspace-write".to_owned());
        command.limits.wall_time_ms = Some(60_000);
        let deadline_floor = unix_ms_now().saturating_add(59_000);
        let app = AgentApplication::production(config(
            &temp.path().join("state.db"),
            connection(&server.root, false),
            true,
        ))
        .expect("production app");
        let run = run_result(
            app.execute(envelope("auto", RunCommand::Start(command.clone())))
                .await,
        );
        let replay = wait_terminal(app.store.as_ref(), &run.run_id).await;
        let requests = server.finish().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body["model"], "deepseek-v4-flash");
        assert!(requests[0].body.get("tools").is_none());
        assert_eq!(requests[1].body["model"], "deepseek-v4-pro");

        let persisted = &replay.snapshot.request;
        assert_eq!(persisted.model, "deepseek-v4-pro");
        assert_eq!(persisted.input, command.input);
        assert_eq!(persisted.reasoning_effort, ReasoningEffort::Max);
        assert_eq!(
            persisted.max_output_tokens,
            Some(OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS)
        );
        assert!(!persisted.streaming);
        assert_eq!(persisted.tool_policy, command.tool_policy);
        assert_eq!(persisted.limits, command.limits);
        assert!(
            persisted
                .deadline_unix_ms
                .is_some_and(|deadline| deadline >= deadline_floor),
            "route and runtime must share the deadline computed at composition entry"
        );
        assert_eq!(
            persisted.environment.workspace,
            temp.path()
                .canonicalize()
                .expect("canonical workspace")
                .display()
                .to_string()
        );
        assert_eq!(persisted.environment.provider, DEEPSEEK_PROVIDER);
        assert!(persisted.environment.tool_catalog_sha256.is_some());
        assert!(persisted.environment.execution_fingerprint_sha256.is_some());
        assert!(persisted.environment.auto_approve);
        assert!(persisted.environment.trust_mode);
        assert_eq!(
            persisted.environment.sandbox.as_deref(),
            Some("workspace-write")
        );
        assert_eq!(persisted.accounting_baseline.hard_request_limit, Some(64));
        assert_eq!(persisted.accounting_baseline.root.started, 1);
        assert_eq!(replay.snapshot.accounting.root.started, 2);
        assert_eq!(replay.snapshot.runtime_model_requests, 1);
        assert!(
            persisted
                .system_prompt
                .blocks
                .last()
                .expect("runtime prompt block")
                .text
                .contains("唯一 AgentRuntime")
        );
    }

    #[tokio::test]
    async fn strict_incompatible_catalog_falls_back_atomically_without_losing_eleven_tools() {
        let server =
            MockDeepSeekServer::start(vec![response("deepseek-v4-pro", "完成", 10, 2)]).await;
        let temp = tempfile::tempdir().expect("temp workspace");
        let mut command = start_command(temp.path(), Some("deepseek-v4-pro"));
        command.tool_policy.allowed = Some(
            PRODUCTION_TOOL_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        );
        let app = AgentApplication::production(config(
            &temp.path().join("state.db"),
            connection(&server.root, true),
            true,
        ))
        .expect("production app");
        let run = run_result(
            app.execute(envelope("strict", RunCommand::Start(command)))
                .await,
        );
        wait_terminal(app.store.as_ref(), &run.run_id).await;
        let requests = server.finish().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/chat/completions");
        let tools = requests[0].body["tools"]
            .as_array()
            .expect("ordinary tool catalog");
        assert_eq!(tools.len(), PRODUCTION_TOOL_NAMES.len());
        let names = tools
            .iter()
            .map(|tool| tool["function"]["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        assert_eq!(names, PRODUCTION_TOOL_NAMES);
    }

    async fn seed_resume_mismatch(
        store: &dyn RunStore,
        workspace: &Path,
        catalog: String,
        fingerprint: Option<String>,
        suffix: &str,
    ) -> RunId {
        let mut request = RunRequest::new(format!("恢复 {suffix}"), "persisted prompt");
        let run_id = RunId::from(format!("mismatch-{suffix}"));
        request.run_id = Some(run_id.clone());
        request.model = "deepseek-v4-pro".to_owned();
        request.max_output_tokens = Some(OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS);
        request.limits.max_depth = 0;
        request.environment = RunEnvironment {
            workspace: workspace
                .canonicalize()
                .expect("canonical workspace")
                .display()
                .to_string(),
            provider: DEEPSEEK_PROVIDER.to_owned(),
            tool_catalog_sha256: Some(catalog),
            execution_fingerprint_sha256: fingerprint,
            sandbox: Some("workspace-write".to_owned()),
            ..RunEnvironment::default()
        };
        let created = store.create(request).await.expect("seed resumable");
        store.release(&created.lease).await.expect("release seed");
        run_id
    }

    #[tokio::test]
    async fn catalog_and_fingerprint_mismatch_fail_before_any_http_request() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let (root, accepted) = quiet_loopback().await;
        let connection = connection(&root, true);
        let app = AgentApplication::production(config(
            &temp.path().join("state.db"),
            connection.clone(),
            true,
        ))
        .expect("production app");
        let catalog_run = seed_resume_mismatch(
            app.store.as_ref(),
            temp.path(),
            "sha256:wrong-catalog".to_owned(),
            Some("sha256:wrong-fingerprint".to_owned()),
            "catalog",
        )
        .await;
        let catalog_error = error_result(
            app.execute(envelope(
                "catalog-mismatch",
                RunCommand::Resume {
                    run_id: catalog_run,
                    expected_workspace: None,
                },
            ))
            .await,
        );
        assert_eq!(catalog_error.code, RunApiErrorCode::RunEnvironmentMismatch);
        assert!(
            catalog_error
                .message
                .starts_with("run_resume_tool_catalog_mismatch：")
        );

        let tool_config = tool_config_for_run(
            &ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full),
            &temp.path().canonicalize().expect("canonical workspace"),
            &RunProductControls {
                sandbox: Some("workspace-write".to_owned()),
                ..RunProductControls::default()
            },
        )
        .expect("tool config");
        let executor: Arc<dyn ToolExecutor> = Arc::new(ProductionToolExecutor::new(tool_config));
        let catalog_runtime = AgentRuntime::new(
            Arc::new(ReplayOnlyModelPort),
            executor,
            Arc::new(NullEventSink),
            app.store.clone(),
        );
        let current_catalog =
            tool_catalog_sha256(&catalog_runtime.tool_definitions(&ToolPolicy::default(), 0, 0));
        let missing_fingerprint_run = seed_resume_mismatch(
            app.store.as_ref(),
            temp.path(),
            current_catalog.clone(),
            None,
            "fingerprint-missing",
        )
        .await;
        let missing_fingerprint_error = error_result(
            app.execute(envelope(
                "fingerprint-missing",
                RunCommand::Resume {
                    run_id: missing_fingerprint_run,
                    expected_workspace: None,
                },
            ))
            .await,
        );
        assert_eq!(
            missing_fingerprint_error.code,
            RunApiErrorCode::RunEnvironmentMismatch
        );
        assert!(
            missing_fingerprint_error
                .message
                .starts_with("run_resume_fingerprint_missing：")
        );

        let fingerprint_run = seed_resume_mismatch(
            app.store.as_ref(),
            temp.path(),
            current_catalog,
            Some("sha256:wrong-fingerprint".to_owned()),
            "fingerprint",
        )
        .await;
        let fingerprint_error = error_result(
            app.execute(envelope(
                "fingerprint-mismatch",
                RunCommand::Resume {
                    run_id: fingerprint_run,
                    expected_workspace: None,
                },
            ))
            .await,
        );
        assert_eq!(
            fingerprint_error.code,
            RunApiErrorCode::RunEnvironmentMismatch
        );
        assert!(
            fingerprint_error
                .message
                .starts_with("run_resume_fingerprint_mismatch：")
        );
        assert_eq!(accepted.await.expect("zero request fixture"), 0);
    }

    fn invocation(name: &str, value: Value) -> ToolInvocation {
        ToolInvocation {
            run_id: RunId::from("tool-state-test"),
            call_id: format!("call-{name}"),
            name: name.to_owned(),
            arguments: ToolArguments::from_value(value),
        }
    }

    #[tokio::test]
    async fn every_run_gets_isolated_read_and_process_state() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "old value\n").expect("write sample");
        let controls = RunProductControls {
            auto_approve: true,
            sandbox: Some("workspace-write".to_owned()),
            ..RunProductControls::default()
        };
        let base = ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full);
        let first = ProductionToolExecutor::new(
            tool_config_for_run(&base, temp.path(), &controls).expect("first run config"),
        );
        let second = ProductionToolExecutor::new(
            tool_config_for_run(&base, temp.path(), &controls).expect("second run config"),
        );
        let read = first
            .execute(
                invocation("read_file", json!({"path": "sample.txt"})),
                CancellationToken::default(),
            )
            .await
            .expect("read invocation");
        assert!(read.is_success());
        let isolated_edit = second
            .execute(
                invocation(
                    "edit_file",
                    json!({"path": "sample.txt", "search": "old value", "replace": "new value"}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("isolated edit invocation");
        assert!(!isolated_edit.is_success());
        assert!(isolated_edit.content.contains("has not been read"));
        assert_eq!(
            std::fs::read_to_string(&file).expect("unchanged"),
            "old value\n"
        );

        let same_run_edit = first
            .execute(
                invocation(
                    "edit_file",
                    json!({"path": "sample.txt", "search": "old value", "replace": "new value"}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("same-run edit invocation");
        assert!(same_run_edit.is_success());
        assert_eq!(
            std::fs::read_to_string(file).expect("edited"),
            "new value\n"
        );
    }
}
