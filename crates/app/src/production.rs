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
use codewhale_orchestrator::ProductionAgentOrchestrator;
use codewhale_protocol::agent_runtime::{
    ActorRequestAccounting, AgentActor, CanonicalTranscript, ContextPolicy, InheritedRunFacts,
    ModelAccounting, ReasoningEffort, RunEnvironment, RunId, RunRequest, SurfaceUsage,
    TranscriptEntry, Usage,
};
use codewhale_protocol::run_api::{
    RunApiError, RunApiErrorCode, RunProductControls, StartRunCommand,
};
use codewhale_protocol::task::{TaskAcceptance, TaskContract, TaskDefinition, TaskGenerationId};
use codewhale_runtime::{
    AgentRuntime, ModelPort, ModelToolAuthority, RunReplay, RunStore, RuntimeEventSink, RuntimeRun,
    ToolExecutor, canonical_tool_catalog_sha256,
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
    AgentApplication, ReplayOnlyModelPort, RunComposition, api_error, recoverable_writer_task,
    resume_needs_live_model, store_error,
};

const DEEPSEEK_PROVIDER: &str = "deepseek";
const CONTEXT_INPUT_SAFETY_TOKENS: u32 = 32_000;
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

    /// Override only the transport retry admission bound.
    ///
    /// The resolved value is part of the production execution fingerprint, so
    /// a resumed run cannot silently change this resource contract.
    #[must_use]
    pub fn with_transport_max_retries(mut self, max_retries: u32) -> Self {
        self.deepseek.retry.max_retries = max_retries;
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
    #[error("初始化受管 writer 工作区根目录失败：{0}")]
    ManagedWorktreeRoot(String),
}

struct ProductionComposition {
    deepseek: DeepSeekConnectionConfig,
    credential: Option<DeepSeekCredential>,
    http_client: Option<reqwest::Client>,
    tools: ProductionToolConfig,
    prompt: ProductionPromptConfig,
    managed_worktree_root: PathBuf,
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
        let managed_worktree_root = store
            .db_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("worktrees");
        std::fs::create_dir_all(&managed_worktree_root).map_err(|error| {
            ProductionApplicationError::ManagedWorktreeRoot(format!(
                "无法创建 {}：{error}",
                managed_worktree_root.display()
            ))
        })?;
        let managed_worktree_root = managed_worktree_root.canonicalize().map_err(|error| {
            ProductionApplicationError::ManagedWorktreeRoot(format!(
                "无法规范化 {}：{error}",
                managed_worktree_root.display()
            ))
        })?;
        let composition = Arc::new(ProductionComposition {
            deepseek: config.deepseek,
            credential: config.credential,
            http_client: config.http_client,
            tools: config.tools,
            prompt: config.prompt,
            managed_worktree_root,
            composition_build_revision: config.composition_build_revision,
            default_max_api_requests: config.default_max_api_requests,
        });
        Ok(Self::from_parts(store, composition))
    }
}

#[async_trait]
impl RunComposition for ProductionComposition {
    fn prepare_start_command(
        &self,
        command: StartRunCommand,
    ) -> Result<StartRunCommand, RunApiError> {
        prepare_production_start_command(command, &self.tools)
    }

    fn prepare_continue_task(
        &self,
        source: &RunReplay,
        task: TaskDefinition,
    ) -> Result<TaskDefinition, RunApiError> {
        let source_run_id = source.snapshot.request.run_id.as_ref().ok_or_else(|| {
            invalid_request("run_continue_source_id_missing：source run has no durable id")
        })?;
        let request = &source.snapshot.request;
        if request.environment.provider != DEEPSEEK_PROVIDER {
            return Err(environment_mismatch(
                source_run_id,
                "run_continue_provider_mismatch：source run is not bound to the official DeepSeek provider",
            ));
        }
        let workspace = canonical_resume_workspace(source_run_id, &request.environment.workspace)?;
        let controls = controls_from_environment(&request.environment);
        let tool_config = tool_config_for_run(&self.tools, &workspace, &controls)?;
        resolve_task_verifiers(task, &ProductionToolExecutor::new(tool_config))
            .map_err(|error| invalid_request(format!("verifier_contract_invalid：{error}")))
    }

    async fn start(
        &self,
        run_id: RunId,
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
        let concrete_tool_executor = ProductionToolExecutor::new(tool_config.clone());
        ensure_task_verifiers_exact(&command.task, &concrete_tool_executor).map_err(|error| {
            environment_mismatch(
                &run_id,
                format!("run_start_verifier_contract_mismatch：{error}"),
            )
        })?;

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
                &command.task.objective,
                Some(command.reasoning_effort),
            );
            let selected = resolve_deepseek_auto_route(
                &transport,
                DeepSeekAutoRouteInput {
                    latest_request: &command.task.objective,
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
        let context_policy = production_context_policy(capability, max_output_tokens);
        let orchestrator = self.production_orchestrator(&workspace, tool_config.clone())?;
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(concrete_tool_executor);
        let model_port: Arc<dyn ModelPort> =
            Arc::new(DeepSeekModelPort::new(transport, request_budget.clone()));
        let runtime = Arc::new(
            AgentRuntime::new(model_port, tool_executor, sink, store)
                .with_orchestrator(orchestrator),
        );
        let tool_catalog = runtime.tool_definitions(
            &command.tool_policy,
            Some(&command.task),
            ModelToolAuthority::root(command.controls.write_execution_mode),
            0,
            command.limits.max_depth,
            command.controls.interactive,
        );
        let tool_catalog_sha256 = canonical_tool_catalog_sha256(&tool_catalog);
        let execution_fingerprint_sha256 =
            self.execution_fingerprint_sha256(&model, &tool_identity, &tool_catalog_sha256);
        let system_prompt = self.system_prompt(&workspace, &model, !tool_catalog.is_empty());
        let accounting_baseline = model_accounting_snapshot(&request_budget);
        let request = RunRequest {
            run_id: Some(run_id.clone()),
            parent_run_id: None,
            continued_from_run_id: None,
            model,
            task_contract: Some(TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: command.task,
            }),
            system_prompt,
            transcript: CanonicalTranscript::default(),
            reasoning_effort,
            max_output_tokens: Some(max_output_tokens),
            streaming: command.streaming,
            actor: AgentActor::default(),
            agent_task: None,
            deadline_unix_ms,
            tool_policy: command.tool_policy,
            limits: command.limits,
            environment: RunEnvironment {
                workspace: stable_path(&workspace),
                provider: DEEPSEEK_PROVIDER.to_owned(),
                tool_catalog_sha256: Some(tool_catalog_sha256),
                execution_fingerprint_sha256: Some(execution_fingerprint_sha256),
                write_execution_mode: command.controls.write_execution_mode,
                auto_approve: command.controls.auto_approve,
                trust_mode: command.controls.trust_mode,
                allow_sandbox_elevation: command.controls.allow_sandbox_elevation,
                interactive: command.controls.interactive,
                sandbox: command.controls.sandbox,
            },
            context_policy,
            context_projection: None,
            inherited_facts: None,
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
        let controls = controls_from_environment(&request.environment);
        let tool_config = tool_config_for_run(&self.tools, &workspace, &controls)?;
        let tool_identity = tool_config.execution_identity();
        let concrete_tool_executor = ProductionToolExecutor::new(tool_config.clone());
        if let Some(contract) = request.task_contract.as_ref() {
            ensure_task_verifiers_exact(&contract.definition, &concrete_tool_executor).map_err(
                |error| {
                    environment_mismatch(
                        &run_id,
                        format!("run_resume_verifier_contract_mismatch：{error}"),
                    )
                },
            )?;
        }
        let orchestrator = self.production_orchestrator(&workspace, tool_config.clone())?;
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(concrete_tool_executor);
        let accounting = recover_writer_accounting(&run_id, &replay, store.as_ref()).await?;
        let (request_budget, exhausted) = resume_api_request_budget(&accounting);
        let model_port: Arc<dyn ModelPort> = if !exhausted && resume_needs_live_model(&replay) {
            let transport = self.bind_live_transport(request_budget.clone())?;
            Arc::new(DeepSeekModelPort::new(transport, request_budget))
        } else {
            Arc::new(ReplayOnlyModelPort)
        };
        let runtime = Arc::new(
            AgentRuntime::new(model_port, tool_executor, sink, store)
                .with_orchestrator(orchestrator),
        );
        let tool_catalog = runtime.tool_definitions(
            &request.tool_policy,
            request
                .task_contract
                .as_ref()
                .map(|contract| &contract.definition),
            ModelToolAuthority::root(request.environment.write_execution_mode),
            0,
            request.limits.max_depth,
            request.environment.interactive,
        );
        let current_catalog_sha256 = canonical_tool_catalog_sha256(&tool_catalog);
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
        let expected_context_policy = production_context_policy(capability, expected_max_output);
        if request.context_policy != expected_context_policy {
            return Err(environment_mismatch(
                &run_id,
                "run_resume_context_policy_mismatch：persisted context policy does not match the current official DeepSeek capability",
            ));
        }
        Ok(runtime.resume_with_accounting_baseline(
            run_id,
            replay.snapshot.last_sequence,
            accounting,
        ))
    }

    async fn continue_run(
        &self,
        run_id: RunId,
        source: RunReplay,
        task: TaskDefinition,
        store: Arc<dyn RunStore>,
        sink: Arc<dyn RuntimeEventSink>,
    ) -> Result<RuntimeRun, RunApiError> {
        let source_request = &source.snapshot.request;
        let source_run_id = source_request.run_id.clone().ok_or_else(|| {
            invalid_request("run_continue_source_id_missing：source run has no durable id")
        })?;
        if source_request.environment.provider != DEEPSEEK_PROVIDER {
            return Err(environment_mismatch(
                &source_run_id,
                "run_continue_provider_mismatch：source run provider is not the official DeepSeek production provider",
            ));
        }
        if source_request
            .environment
            .execution_fingerprint_sha256
            .is_none()
        {
            return Err(environment_mismatch(
                &source_run_id,
                "run_continue_fingerprint_missing：source run has no production execution fingerprint",
            ));
        }
        let workspace =
            canonical_resume_workspace(&source_run_id, &source_request.environment.workspace)?;
        let controls = controls_from_environment(&source_request.environment);
        let tool_config = tool_config_for_run(&self.tools, &workspace, &controls)?;
        let tool_identity = tool_config.execution_identity();
        let concrete_tool_executor = ProductionToolExecutor::new(tool_config.clone());
        ensure_task_verifiers_exact(&task, &concrete_tool_executor).map_err(|error| {
            environment_mismatch(
                &source_run_id,
                format!("run_continue_verifier_contract_mismatch：{error}"),
            )
        })?;
        let request_limit = source
            .snapshot
            .accounting
            .hard_request_limit
            .and_then(NonZeroU32::new)
            .unwrap_or(self.default_max_api_requests);
        let request_budget = SharedApiRequestBudget::new(request_limit);
        let transport = self.bind_live_transport(request_budget.clone())?;
        let model = source_request.model.clone();
        let capability = official_model_capabilities(&model)
            .map_err(|error| environment_mismatch(&source_run_id, error.to_string()))?;
        let max_output_tokens = capability
            .resolve_output_tokens(source_request.max_output_tokens)
            .map_err(|error| environment_mismatch(&source_run_id, error.to_string()))?;
        let context_policy = production_context_policy(capability, max_output_tokens);
        let orchestrator = self.production_orchestrator(&workspace, tool_config.clone())?;
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(concrete_tool_executor);
        let model_port: Arc<dyn ModelPort> =
            Arc::new(DeepSeekModelPort::new(transport, request_budget.clone()));
        let runtime = Arc::new(
            AgentRuntime::new(model_port, tool_executor, sink, store)
                .with_orchestrator(orchestrator),
        );
        let tool_catalog = runtime.tool_definitions(
            &source_request.tool_policy,
            Some(&task),
            ModelToolAuthority::root(source_request.environment.write_execution_mode),
            0,
            source_request.limits.max_depth,
            source_request.environment.interactive,
        );
        let tool_catalog_sha256 = canonical_tool_catalog_sha256(&tool_catalog);
        let execution_fingerprint_sha256 =
            self.execution_fingerprint_sha256(&model, &tool_identity, &tool_catalog_sha256);
        let system_prompt = self.system_prompt(&workspace, &model, !tool_catalog.is_empty());
        let mut transcript = source.snapshot.transcript.clone();
        match transcript.entries.first_mut() {
            Some(TranscriptEntry::System { prompt }) => *prompt = system_prompt.clone(),
            _ => transcript.entries.insert(
                0,
                TranscriptEntry::System {
                    prompt: system_prompt.clone(),
                },
            ),
        }
        let deadline_unix_ms = source_request
            .limits
            .wall_time_ms
            .map(|duration| unix_ms_now().saturating_add(duration));
        let request = RunRequest {
            run_id: Some(run_id.clone()),
            parent_run_id: None,
            continued_from_run_id: Some(source_run_id),
            model,
            task_contract: Some(TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: task,
            }),
            system_prompt,
            transcript,
            reasoning_effort: source_request.reasoning_effort,
            max_output_tokens: Some(max_output_tokens),
            streaming: source_request.streaming,
            actor: AgentActor::default(),
            agent_task: None,
            deadline_unix_ms,
            tool_policy: source_request.tool_policy.clone(),
            limits: source_request.limits,
            environment: RunEnvironment {
                workspace: stable_path(&workspace),
                provider: DEEPSEEK_PROVIDER.to_owned(),
                tool_catalog_sha256: Some(tool_catalog_sha256),
                execution_fingerprint_sha256: Some(execution_fingerprint_sha256),
                write_execution_mode: controls.write_execution_mode,
                auto_approve: controls.auto_approve,
                trust_mode: controls.trust_mode,
                allow_sandbox_elevation: controls.allow_sandbox_elevation,
                interactive: controls.interactive,
                sandbox: controls.sandbox,
            },
            context_policy,
            context_projection: source.snapshot.context_projection.clone(),
            inherited_facts: Some(InheritedRunFacts {
                workspace_state: source.snapshot.workspace_state.clone(),
                last_completion_rejection: source.snapshot.last_completion_rejection.clone(),
                last_host_verification_failure: source
                    .snapshot
                    .last_host_verification_failure
                    .clone(),
            }),
            accounting_baseline: model_accounting_snapshot(&request_budget),
        };
        Ok(runtime.start(request))
    }
}

impl ProductionComposition {
    fn production_orchestrator(
        &self,
        workspace: &Path,
        root_tools: ProductionToolConfig,
    ) -> Result<Arc<ProductionAgentOrchestrator>, RunApiError> {
        ProductionAgentOrchestrator::new(workspace, &self.managed_worktree_root, root_tools)
            .map(Arc::new)
            .map_err(|error| {
                invalid_request(format!(
                    "writer_orchestrator_invalid：无法绑定 production writer 编排器：{error}"
                ))
            })
    }

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

async fn recover_writer_accounting(
    root_run_id: &RunId,
    replay: &RunReplay,
    store: &dyn RunStore,
) -> Result<ModelAccounting, RunApiError> {
    let parent = &replay.snapshot.accounting;
    let Some(task) = recoverable_writer_task(replay) else {
        return Ok(parent.clone());
    };
    let Some(child) = store.load(&task.child_run_id).await.map_err(store_error)? else {
        // TaskPrepared and workspace-create side-effect recovery can precede
        // durable child creation. No child ledger exists at those checkpoints.
        return Ok(parent.clone());
    };
    let request = &child.snapshot.request;
    if request.run_id.as_ref() != Some(&task.child_run_id)
        || request.parent_run_id.as_ref() != Some(root_run_id)
        || request.agent_task.as_ref() != Some(task)
    {
        return Err(environment_mismatch(
            root_run_id,
            "run_resume_writer_accounting_lineage_mismatch：writer child replay does not match the persisted parent task",
        ));
    }
    if child.snapshot.accounting == ModelAccounting::default()
        && request.accounting_baseline == ModelAccounting::default()
    {
        // A crash immediately after child creation has not emitted a child
        // accounting observation yet. The parent is still the newest ledger;
        // treating the child's intentionally empty baseline as a regression
        // would make this valid checkpoint unrecoverable.
        return Ok(parent.clone());
    }
    if !accounting_dominates(&child.snapshot.accounting, parent) {
        return Err(environment_mismatch(
            root_run_id,
            "run_resume_writer_accounting_diverged：writer child physical ledger does not dominate its parent snapshot",
        ));
    }
    let mut accounting = child.snapshot.accounting;
    // Runtime retries are owned by each Runtime run, not the shared DeepSeek
    // physical ledger. ChildFinished will fold child runtime retries into the
    // parent exactly once; carrying them here would double-count that field.
    accounting.runtime_retries = parent.runtime_retries;
    Ok(accounting)
}

fn accounting_dominates(candidate: &ModelAccounting, parent: &ModelAccounting) -> bool {
    candidate.hard_request_limit == parent.hard_request_limit
        && actor_accounting_is_valid(&candidate.root)
        && actor_accounting_is_valid(&candidate.child)
        // While a writer is unsettled the root waits for that exact tool call,
        // so only child-attributed physical requests may advance.
        && candidate.root == parent.root
        && actor_accounting_dominates(&candidate.child, &parent.child)
        && candidate.transport_retries >= parent.transport_retries
        && candidate.sealed_denied >= parent.sealed_denied
        && candidate.exhausted_denied >= parent.exhausted_denied
        && (!parent.budget_exhausted || candidate.budget_exhausted)
        && !candidate.sealed
        && candidate.usage_responses >= parent.usage_responses
        && candidate.usage_missing_responses >= parent.usage_missing_responses
        && candidate.incomplete_responses >= parent.incomplete_responses
        && candidate.billing_unknown_attempts >= parent.billing_unknown_attempts
        && candidate.unpriced_usage_responses >= parent.unpriced_usage_responses
        && candidate.records_after_seal >= parent.records_after_seal
        && usage_dominates(candidate.usage, parent.usage)
        && candidate.cost_nanousd >= parent.cost_nanousd
        && candidate.cost_nanocny >= parent.cost_nanocny
        && parent
            .surface_usage
            .iter()
            .all(|persisted| surface_usage_is_dominated(persisted, &candidate.surface_usage))
        && candidate
            .hard_request_limit
            .is_none_or(|limit| candidate.total_started() <= u64::from(limit))
}

fn actor_accounting_is_valid(accounting: &ActorRequestAccounting) -> bool {
    accounting.completed <= accounting.started
        && accounting.in_flight <= accounting.started.saturating_sub(accounting.completed)
}

fn actor_accounting_dominates(
    candidate: &ActorRequestAccounting,
    parent: &ActorRequestAccounting,
) -> bool {
    candidate.started >= parent.started
        && candidate.completed >= parent.completed
        && candidate.retries >= parent.retries
}

fn usage_dominates(candidate: Usage, parent: Usage) -> bool {
    candidate.input_tokens >= parent.input_tokens
        && candidate.output_tokens >= parent.output_tokens
        && candidate.cache_hit_tokens >= parent.cache_hit_tokens
        && candidate.cache_miss_tokens >= parent.cache_miss_tokens
        && candidate.cache_write_tokens >= parent.cache_write_tokens
        && candidate.reasoning_tokens >= parent.reasoning_tokens
        && candidate.reasoning_replay_tokens >= parent.reasoning_replay_tokens
}

fn surface_usage_is_dominated(parent: &SurfaceUsage, candidate: &[SurfaceUsage]) -> bool {
    candidate.iter().any(|candidate| {
        candidate.surface == parent.surface
            && candidate.model == parent.model
            && candidate.response_count >= parent.response_count
            && candidate.usage_response_count >= parent.usage_response_count
            && usage_dominates(candidate.usage, parent.usage)
            && candidate.cost_nanousd >= parent.cost_nanousd
            && candidate.cost_nanocny >= parent.cost_nanocny
    })
}

fn prepare_production_start_command(
    mut command: StartRunCommand,
    base_tools: &ProductionToolConfig,
) -> Result<StartRunCommand, RunApiError> {
    if let Some(model) = command.model.as_deref() {
        official_model_capabilities(model).map_err(|error| invalid_request(error.to_string()))?;
    }
    let workspace = canonical_start_workspace(&command.workspace)?;
    let tool_config = tool_config_for_run(base_tools, &workspace, &command.controls)?;
    command.task = resolve_task_verifiers(command.task, &ProductionToolExecutor::new(tool_config))
        .map_err(|error| invalid_request(format!("verifier_contract_invalid：{error}")))?;
    command.workspace = stable_path(&workspace);
    Ok(command)
}

fn controls_from_environment(environment: &RunEnvironment) -> RunProductControls {
    RunProductControls {
        write_execution_mode: environment.write_execution_mode,
        auto_approve: environment.auto_approve,
        trust_mode: environment.trust_mode,
        allow_sandbox_elevation: environment.allow_sandbox_elevation,
        interactive: environment.interactive,
        sandbox: environment.sandbox.clone(),
    }
}

fn resolve_task_verifiers(
    mut task: TaskDefinition,
    tools: &ProductionToolExecutor,
) -> Result<TaskDefinition, codewhale_tools::ToolError> {
    for acceptance in &mut task.acceptance {
        let TaskAcceptance::Verifier { verifier, .. } = acceptance else {
            continue;
        };
        *verifier =
            tools.resolve_verifier_spec(&verifier.verifier_id, verifier.parameters.clone())?;
    }
    Ok(task)
}

fn ensure_task_verifiers_exact(
    task: &TaskDefinition,
    tools: &ProductionToolExecutor,
) -> Result<(), codewhale_tools::ToolError> {
    let resolved = resolve_task_verifiers(task.clone(), tools)?;
    if resolved == *task {
        Ok(())
    } else {
        Err(codewhale_tools::ToolError::invalid_input(
            "persisted verifier specification differs from the production resolver",
        ))
    }
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

fn production_context_policy(
    capability: codewhale_deepseek::OfficialModelCapabilities,
    max_output_tokens: u32,
) -> ContextPolicy {
    let hard_input_tokens = capability
        .context_window_tokens
        .saturating_sub(max_output_tokens)
        .saturating_sub(CONTEXT_INPUT_SAFETY_TOKENS)
        .max(1);
    ContextPolicy { hard_input_tokens }
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
    use std::process::Command as ProcessCommand;
    use std::sync::{Arc, Mutex as StdMutex};

    use codewhale_context::compaction::{ContextInput, effective_context};
    use codewhale_deepseek::{
        ApiSurface, OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS, OFFICIAL_V4_CONTEXT_WINDOW_TOKENS,
        OFFICIAL_V4_MAX_OUTPUT_TOKENS, RuntimeChatPlanInput, StrictSchemaIssue, ToolSurfaceReason,
        plan_runtime_chat,
    };
    use codewhale_protocol::agent_runtime::{
        AGENT_TOOL_NAME, AgentActorKind, AgentOutcome, AgentResultDetails, AgentTask, AgentTaskId,
        AgentWorkspaceAccess, AgentWorkspaceAssignment, AttemptId, ModelAccounting,
        ModelFinishReason, ModelOutput, ModelRequest, ModelStreamEvent, ModelToolCall, OperationId,
        PendingRuntimeEvent, RecoveryAmbiguity, RecoveryAmbiguityPhase, RunLimits,
        RuntimeEventKind, TerminalState, ToolArguments, ToolDefinition, ToolFailureCode,
        ToolInvocation, ToolOutcome, ToolPolicy, Usage, WorkspaceAccess, WriteExecutionMode,
    };
    use codewhale_protocol::run_api::{
        RUN_API_SCHEMA_VERSION, RunCommand, RunCommandEnvelope, RunCommandResponse,
        RunCommandResult, RunView,
    };
    use codewhale_protocol::task::{
        CompletionCandidateId, CompletionDecision, WorkspaceRevision, WorkspaceState,
    };
    use codewhale_runtime::{
        AgentChildFinishedFact, CancellationToken, InMemoryRunStore, ModelPortError, ModelStream,
        NullEventSink, RunLease, ToolExecutionError,
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

    #[test]
    fn transport_retry_override_changes_only_the_connection_bound() {
        let baseline = ProductionApplicationConfig::official();
        let limited = baseline.clone().with_transport_max_retries(1);
        assert_eq!(baseline.deepseek.retry.max_retries, 3);
        assert_eq!(limited.deepseek.retry.max_retries, 1);
        assert_eq!(
            baseline.deepseek.retry.initial_delay,
            limited.deepseek.retry.initial_delay
        );
        assert_eq!(
            baseline.deepseek.retry.max_delay,
            limited.deepseek.retry.max_delay
        );
        assert_eq!(
            baseline.deepseek.retry.exponential_base,
            limited.deepseek.retry.exponential_base,
        );
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
            task: TaskDefinition::host("修复真实边界问题"),
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
                write_execution_mode: Default::default(),
                auto_approve: true,
                trust_mode: false,
                allow_sandbox_elevation: false,
                interactive: false,
                sandbox: Some("workspace-write".to_owned()),
            },
        }
    }

    fn caller_authored_verifier_task() -> TaskDefinition {
        serde_json::from_value(json!({
            "objective": "验证 canonical verifier contract",
            "constraints": [],
            "non_goals": [],
            "acceptance": [{
                "kind": "verifier",
                "id": "exact-check",
                "description": "确定性检查通过",
                "evidence_policy": "latest_pass",
                "verifier": {
                    "verifier_id": "run_verifiers",
                    "parameters": {
                        "profile": "exact",
                        "commands": [{
                            "name": "exact-check",
                            "program": "/usr/bin/python3",
                            "args": ["-I", "-B", "verify.py", "."],
                            "cwd": ""
                        }]
                    },
                    "plan": {"steps": [{
                        "id": "caller-guess",
                        "program": "false",
                        "args": [],
                        "cwd": "",
                        "env": {},
                        "timeout_ms": 1
                    }]}
                }
            }]
        }))
        .expect("caller verifier task")
    }

    fn test_run_request(
        run_id: RunId,
        objective: impl Into<String>,
        system_prompt: &str,
    ) -> RunRequest {
        let mut request = RunRequest::new(
            TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: TaskDefinition::host(objective),
            },
            system_prompt,
        );
        request.run_id = Some(run_id);
        request
    }

    fn test_production_composition(
        temp: &Path,
        connection: DeepSeekConnectionConfig,
        credential: bool,
    ) -> ProductionComposition {
        let managed_worktree_root = temp.join("worktrees");
        std::fs::create_dir_all(&managed_worktree_root).expect("managed worktree root");
        let managed_worktree_root = managed_worktree_root
            .canonicalize()
            .expect("canonical managed worktree root");
        let config = config(&temp.join("state.db"), connection, credential);
        ProductionComposition {
            deepseek: config.deepseek,
            credential: config.credential,
            http_client: config.http_client,
            tools: config.tools,
            prompt: config.prompt,
            managed_worktree_root,
            composition_build_revision: config.composition_build_revision,
            default_max_api_requests: config.default_max_api_requests,
        }
    }

    fn resume_accounting(
        root_started: u64,
        child_started: u64,
        input_tokens: u64,
    ) -> ModelAccounting {
        ModelAccounting {
            hard_request_limit: Some(5),
            root: ActorRequestAccounting {
                started: root_started,
                completed: root_started,
                ..ActorRequestAccounting::default()
            },
            child: ActorRequestAccounting {
                started: child_started,
                completed: child_started,
                ..ActorRequestAccounting::default()
            },
            complete: true,
            usage_complete: true,
            usage_responses: root_started.saturating_add(child_started),
            usage: Usage {
                input_tokens,
                output_tokens: root_started.saturating_add(child_started),
                ..Usage::default()
            },
            ..ModelAccounting::default()
        }
    }

    fn exact_resume_request(
        composition: &ProductionComposition,
        workspace: &Path,
        run_id: RunId,
        accounting: ModelAccounting,
        write_execution_mode: WriteExecutionMode,
    ) -> RunRequest {
        let workspace = workspace.canonicalize().expect("canonical workspace");
        let controls = RunProductControls {
            write_execution_mode,
            auto_approve: true,
            trust_mode: false,
            allow_sandbox_elevation: false,
            interactive: false,
            sandbox: Some("workspace-write".to_owned()),
        };
        let mut request = test_run_request(run_id, "恢复 production Agent", "persisted prompt");
        request.model = "deepseek-v4-pro".to_owned();
        request.max_output_tokens = Some(OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS);
        request.limits.max_depth = 1;
        request.context_policy = production_context_policy(
            official_model_capabilities(&request.model).expect("official model"),
            request.max_output_tokens.expect("resolved output limit"),
        );
        request.environment = RunEnvironment {
            workspace: stable_path(&workspace),
            provider: DEEPSEEK_PROVIDER.to_owned(),
            write_execution_mode: controls.write_execution_mode,
            auto_approve: controls.auto_approve,
            trust_mode: controls.trust_mode,
            allow_sandbox_elevation: controls.allow_sandbox_elevation,
            interactive: controls.interactive,
            sandbox: controls.sandbox.clone(),
            ..RunEnvironment::default()
        };
        request.accounting_baseline = accounting;

        let tool_config =
            tool_config_for_run(&composition.tools, &workspace, &controls).expect("tool config");
        let tool_identity = tool_config.execution_identity();
        let catalog_runtime = AgentRuntime::new(
            Arc::new(ReplayOnlyModelPort),
            Arc::new(ProductionToolExecutor::new(tool_config)),
            Arc::new(NullEventSink),
            Arc::new(InMemoryRunStore::default()),
        );
        let catalog = catalog_runtime.tool_definitions(
            &request.tool_policy,
            request
                .task_contract
                .as_ref()
                .map(|contract| &contract.definition),
            ModelToolAuthority::root(request.environment.write_execution_mode),
            0,
            request.limits.max_depth,
            request.environment.interactive,
        );
        let catalog_sha256 = canonical_tool_catalog_sha256(&catalog);
        request.environment.tool_catalog_sha256 = Some(catalog_sha256.clone());
        request.environment.execution_fingerprint_sha256 =
            Some(composition.execution_fingerprint_sha256(
                &request.model,
                &tool_identity,
                &catalog_sha256,
            ));
        request
    }

    fn writer_task(request: &RunRequest) -> AgentTask {
        let root_run_id = request.run_id.clone().expect("root run id");
        let child_run_id = RunId::from(format!("{}-writer", root_run_id.0));
        let mut tool_policy = request.tool_policy.clone();
        tool_policy.denied.push(AGENT_TOOL_NAME.to_owned());
        AgentTask {
            task_id: AgentTaskId::from(format!("{}-task", root_run_id.0)),
            root_run_id: root_run_id.clone(),
            parent_run_id: root_run_id,
            child_run_id: child_run_id.clone(),
            call_id: format!("{}-call", child_run_id.0),
            role: "implementer".to_owned(),
            task_contract: TaskContract {
                generation_id: TaskGenerationId::from(child_run_id.0.clone()),
                definition: TaskDefinition::host("修改唯一允许的文件并给出证据"),
            },
            workspace: AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::IsolatedWrite,
                root_workspace: request.environment.workspace.clone(),
                base_commit: "a".repeat(40),
                worktree_path: Some(
                    Path::new(&request.environment.workspace)
                        .join(format!(".writer-{}", child_run_id.0))
                        .display()
                        .to_string(),
                ),
                root_branch: Some("main".to_owned()),
                branch: Some(format!("codewhale/writer/{}", child_run_id.0)),
                allowed_paths: vec!["src/lib.rs".to_owned()],
                owner_token: Some(format!("owner-{}", child_run_id.0)),
            },
            tool_policy,
            limits: RunLimits {
                max_depth: 0,
                ..request.limits
            },
            deadline_unix_ms: request.deadline_unix_ms,
            expected_artifact: "一个 Host seal 的提交".to_owned(),
        }
    }

    async fn append_test_event(store: &dyn RunStore, lease: &RunLease, event: RuntimeEventKind) {
        store
            .append(lease, PendingRuntimeEvent::new(event))
            .await
            .expect("append fixture event");
    }

    async fn seed_in_flight_tool(
        store: &dyn RunStore,
        request: RunRequest,
        writer_checkpoint: Option<bool>,
    ) -> (RunReplay, Option<AgentTask>) {
        let run_id = request.run_id.clone().expect("root run id");
        let task = writer_checkpoint.map(|_| writer_task(&request));
        let call_id = task
            .as_ref()
            .map_or_else(|| "ordinary-call".to_owned(), |task| task.call_id.clone());
        let name = task
            .as_ref()
            .map_or_else(|| "read_file".to_owned(), |_| AGENT_TOOL_NAME.to_owned());
        let created = store.create(request).await.expect("create fixture root");
        let arguments = task.as_ref().map_or_else(
            || ToolArguments::from_value(json!({"path": "src/lib.rs"})),
            |_| {
                ToolArguments::from_value(json!({
                    "prompt": "修改唯一允许的文件并给出证据",
                    "type": "implementer",
                    "workspace_access": "isolated_write",
                    "allowed_paths": ["src/lib.rs"],
                    "expected_artifact": "一个 Host seal 的提交"
                }))
            },
        );
        let tool_call = ModelToolCall {
            id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        };
        let tools = vec![ToolDefinition {
            name: name.clone(),
            description: format!("{name} 测试夹具"),
            input_schema: json!({"type": "object"}),
        }];
        let snapshot = &created.replay.snapshot;
        let context = effective_context(ContextInput {
            transcript: &snapshot.transcript,
            projection: snapshot.context_projection.as_ref(),
            task_contract: snapshot.request.task_contract.as_ref(),
            workspace_state: &snapshot.workspace_state,
            evidence_receipts: &snapshot.evidence_receipts,
            last_completion_rejection: snapshot.last_completion_rejection.as_ref(),
            last_verifier_failure: snapshot
                .last_host_verification_failure
                .as_ref()
                .map(|failure| &failure.outcome),
            last_verifier_failure_workspace: snapshot
                .last_host_verification_failure
                .as_ref()
                .map(|failure| &failure.workspace_state),
            tools: &tools,
        })
        .expect("fixture model context");
        let attempt_id = AttemptId(format!("fixture-attempt-{}", run_id.0));
        append_test_event(
            store,
            &created.lease,
            RuntimeEventKind::ModelRequestPrepared {
                attempt_id: attempt_id.clone(),
                request: Box::new(ModelRequest {
                    run_id: run_id.clone(),
                    parent_run_id: snapshot.request.parent_run_id.clone(),
                    actor: snapshot.request.actor,
                    model: snapshot.request.model.clone(),
                    system_prompt: context.system_prompt,
                    messages: context.messages,
                    tools,
                    reasoning_effort: snapshot.request.reasoning_effort,
                    max_output_tokens: snapshot.request.max_output_tokens,
                    streaming: snapshot.request.streaming,
                    request_number: snapshot.local_turns.saturating_add(1),
                    attempt: 0,
                }),
            },
        )
        .await;
        append_test_event(
            store,
            &created.lease,
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: attempt_id.clone(),
            },
        )
        .await;
        append_test_event(
            store,
            &created.lease,
            RuntimeEventKind::ModelResponseCommitted {
                attempt_id,
                output: Box::new(ModelOutput {
                    content: String::new(),
                    reasoning_content: None,
                    tool_calls: vec![tool_call],
                    finish_reason: ModelFinishReason::ToolCalls,
                    usage: Usage::default(),
                }),
                accounting: Box::new(snapshot.accounting.clone()),
            },
        )
        .await;
        let operation_id = OperationId::from(format!("operation-{}", run_id.0));
        append_test_event(
            store,
            &created.lease,
            RuntimeEventKind::ToolPrepared {
                operation_id: operation_id.clone(),
                invocation: ToolInvocation {
                    run_id: run_id.clone(),
                    call_id,
                    name,
                    arguments,
                },
                workspace_access: if task.is_some() {
                    WorkspaceAccess::MayWrite
                } else {
                    WorkspaceAccess::ReadOnly
                },
            },
        )
        .await;
        append_test_event(
            store,
            &created.lease,
            RuntimeEventKind::ToolExecutionStarted { operation_id },
        )
        .await;
        if let Some(task) = &task {
            append_test_event(
                store,
                &created.lease,
                RuntimeEventKind::AgentTaskPrepared {
                    task: Box::new(task.clone()),
                },
            )
            .await;
            if writer_checkpoint == Some(true) {
                append_test_event(
                    store,
                    &created.lease,
                    RuntimeEventKind::AgentWorkspaceCreated {
                        task_id: task.task_id.clone(),
                        assignment: task.workspace.clone(),
                        writer_workspace_state: WorkspaceState {
                            generation: 0,
                            revision: WorkspaceRevision::Known {
                                sha256: task.workspace.base_commit.clone(),
                            },
                        },
                    },
                )
                .await;
            }
        }
        store
            .release(&created.lease)
            .await
            .expect("release fixture root");
        let replay = store
            .load(&run_id)
            .await
            .expect("load fixture root")
            .expect("fixture root exists");
        (replay, task)
    }

    fn mark_writer_finished(replay: &mut RunReplay, task: &AgentTask, terminal: TerminalState) {
        let accounting = replay.snapshot.accounting.clone();
        replay
            .snapshot
            .agent_tasks
            .iter_mut()
            .find(|lifecycle| lifecycle.task.task_id == task.task_id)
            .expect("writer lifecycle")
            .finished = Some(AgentChildFinishedFact {
            call_id: task.call_id.clone(),
            outcome: AgentOutcome {
                run_id: task.child_run_id.clone(),
                parent_run_id: Some(task.parent_run_id.clone()),
                terminal,
                accounting: accounting.clone(),
                runtime_model_requests: 1,
                runtime_retries: 0,
                tool_calls: 1,
                details: AgentResultDetails::default(),
            },
            accounting,
            handoff_content: "writer lifecycle 已闭合，等待提交 agent 工具结果".to_owned(),
        });
    }

    fn child_request(
        parent: &RunRequest,
        task: &AgentTask,
        accounting: ModelAccounting,
    ) -> RunRequest {
        let mut request = RunRequest::new(task.task_contract.clone(), "writer child prompt");
        request.run_id = Some(task.child_run_id.clone());
        request.parent_run_id = Some(task.parent_run_id.clone());
        request.model = parent.model.clone();
        request.reasoning_effort = parent.reasoning_effort;
        request.max_output_tokens = parent.max_output_tokens;
        request.streaming = false;
        request.actor = AgentActor {
            kind: AgentActorKind::Child,
            depth: 1,
        };
        request.agent_task = Some(task.clone());
        request.deadline_unix_ms = parent.deadline_unix_ms;
        request.tool_policy = task.tool_policy.clone();
        request.limits = task.limits;
        request.environment = parent.environment.clone();
        request.environment.workspace = task.workspace.execution_workspace().to_owned();
        request.environment.interactive = false;
        request.environment.sandbox = Some("isolated_writer".to_owned());
        request.context_policy = parent.context_policy;
        request.accounting_baseline = accounting;
        request
    }

    #[tokio::test]
    async fn unfinished_writer_resume_checkpoints_require_live_model_binding() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let (root, accepted) = quiet_loopback().await;
        let composition = test_production_composition(temp.path(), connection(&root, false), false);
        let store = Arc::new(InMemoryRunStore::default());

        for (suffix, workspace_created) in [("task-prepared", false), ("workspace-created", true)] {
            let run_id = RunId::from(format!("writer-live-{suffix}"));
            let request = exact_resume_request(
                &composition,
                temp.path(),
                run_id.clone(),
                resume_accounting(1, 0, 10),
                WriteExecutionMode::IsolatedWriter,
            );
            let (replay, _) =
                seed_in_flight_tool(store.as_ref(), request, Some(workspace_created)).await;
            assert!(resume_needs_live_model(&replay));
            let error = match composition
                .resume(run_id, replay, store.clone(), Arc::new(NullEventSink))
                .await
            {
                Ok(_) => panic!("unfinished writer must bind a live DeepSeek model"),
                Err(error) => error,
            };
            assert_eq!(error.code, RunApiErrorCode::InvalidRequest);
            assert!(error.message.contains("deepseek_credential_missing"));
        }
        assert_eq!(accepted.await.expect("zero request fixture"), 0);
    }

    #[tokio::test]
    async fn finished_writer_pending_tool_result_requires_live_model_unless_recovery_required() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let (root, accepted) = quiet_loopback().await;
        let composition = test_production_composition(temp.path(), connection(&root, false), false);
        let store = Arc::new(InMemoryRunStore::default());
        let run_id = RunId::from("writer-finished-before-tool-result");
        let request = exact_resume_request(
            &composition,
            temp.path(),
            run_id.clone(),
            resume_accounting(1, 0, 10),
            WriteExecutionMode::IsolatedWriter,
        );
        let (mut replay, task) = seed_in_flight_tool(store.as_ref(), request, Some(true)).await;
        let task = task.expect("writer task");
        mark_writer_finished(
            &mut replay,
            &task,
            TerminalState::Completed {
                message: "writer 已完成并集成".to_owned(),
                decision: CompletionDecision {
                    candidate_id: CompletionCandidateId::from("writer-finished"),
                    generation_id: task.task_contract.generation_id.clone(),
                    workspace_state: WorkspaceState {
                        generation: 1,
                        revision: WorkspaceRevision::Known {
                            sha256: task.workspace.base_commit.clone(),
                        },
                    },
                    satisfied: Vec::new(),
                },
            },
        );
        assert!(
            resume_needs_live_model(&replay),
            "ChildFinished 后仍须先提交 agent 工具结果，再让 root 继续模型回合"
        );
        let error = match composition
            .resume(
                run_id.clone(),
                replay.clone(),
                store.clone(),
                Arc::new(NullEventSink),
            )
            .await
        {
            Ok(_) => panic!("finished writer tool replay must bind live DeepSeek"),
            Err(error) => error,
        };
        assert!(error.message.contains("deepseek_credential_missing"));

        mark_writer_finished(
            &mut replay,
            &task,
            TerminalState::RecoveryRequired {
                ambiguity: RecoveryAmbiguity {
                    phase: RecoveryAmbiguityPhase::ChildRun,
                    action_id: "writer-integration".to_owned(),
                    message: "集成结果无法精确证明".to_owned(),
                },
            },
        );
        assert!(
            !resume_needs_live_model(&replay),
            "RecoveryRequired writer must close locally without a Key"
        );
        let run = composition
            .resume(run_id, replay, store, Arc::new(NullEventSink))
            .await
            .expect("RecoveryRequired writer composes without a Key")
            .ready()
            .await
            .expect("resume acquires canonical root");
        let _ = run.wait().await.expect("recovery path settles locally");
        assert_eq!(accepted.await.expect("zero request fixture"), 0);
    }

    #[tokio::test]
    async fn ordinary_in_flight_tool_resume_needs_no_key_and_never_replays_side_effect() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let (root, accepted) = quiet_loopback().await;
        let composition = test_production_composition(temp.path(), connection(&root, false), false);
        let store = Arc::new(InMemoryRunStore::default());
        let run_id = RunId::from("ordinary-in-flight");
        let request = exact_resume_request(
            &composition,
            temp.path(),
            run_id.clone(),
            resume_accounting(1, 0, 10),
            WriteExecutionMode::Root,
        );
        let (replay, _) = seed_in_flight_tool(store.as_ref(), request, None).await;
        assert!(!resume_needs_live_model(&replay));

        let run = composition
            .resume(run_id, replay, store.clone(), Arc::new(NullEventSink))
            .await
            .expect("ordinary ambiguous tool composes without a Key")
            .ready()
            .await
            .expect("resume acquires the run");
        let outcome = run.wait().await.expect("runtime settles ambiguity");
        assert!(matches!(
            outcome.terminal,
            TerminalState::RecoveryRequired { .. }
        ));
        assert_eq!(accepted.await.expect("zero request fixture"), 0);
    }

    #[tokio::test]
    async fn writer_resume_recovers_shared_child_ledger_without_reopening_limit_or_losing_usage() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let composition = test_production_composition(
            temp.path(),
            connection("http://127.0.0.1:9/v1", false),
            false,
        );
        let store = Arc::new(InMemoryRunStore::default());
        let run_id = RunId::from("writer-accounting");
        let parent_accounting = resume_accounting(1, 0, 10);
        let request = exact_resume_request(
            &composition,
            temp.path(),
            run_id.clone(),
            parent_accounting.clone(),
            WriteExecutionMode::IsolatedWriter,
        );
        let (replay, task) = seed_in_flight_tool(store.as_ref(), request, Some(true)).await;
        let task = task.expect("writer task");
        let acquired = store.acquire(&run_id).await.expect("acquire root");
        let lease = acquired.lease.expect("root lease");
        append_test_event(
            store.as_ref(),
            &lease,
            RuntimeEventKind::ChildStarted {
                task_id: task.task_id.clone(),
                call_id: task.call_id.clone(),
                child_run_id: task.child_run_id.clone(),
                depth: 1,
            },
        )
        .await;
        store.release(&lease).await.expect("release root");

        let child_accounting = resume_accounting(1, 3, 40);
        let child = store
            .create(child_request(
                &replay.snapshot.request,
                &task,
                child_accounting.clone(),
            ))
            .await
            .expect("create child replay");
        store.release(&child.lease).await.expect("release child");
        let replay = store
            .load(&run_id)
            .await
            .expect("load root")
            .expect("root exists");

        let recovered = recover_writer_accounting(&run_id, &replay, store.as_ref())
            .await
            .expect("recover shared ledger");
        assert_eq!(recovered.root.started, 1);
        assert_eq!(recovered.child.started, 3);
        assert_eq!(recovered.total_started(), 4);
        assert_eq!(recovered.usage, child_accounting.usage);

        let (budget, exhausted) = resume_api_request_budget(&recovered);
        assert!(!exhausted);
        assert_eq!(budget.accounting_snapshot().0.limit, 1);
        assert_eq!(
            store
                .load(&run_id)
                .await
                .expect("load canonical root")
                .expect("root exists")
                .snapshot
                .accounting,
            parent_accounting,
            "accounting recovery must not invent an unlogged canonical Store mutation"
        );
    }

    #[tokio::test]
    async fn writer_resume_fails_closed_when_child_shared_ledger_regresses_parent() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let composition = test_production_composition(
            temp.path(),
            connection("http://127.0.0.1:9/v1", false),
            false,
        );
        let store = Arc::new(InMemoryRunStore::default());
        let run_id = RunId::from("writer-accounting-diverged");
        let request = exact_resume_request(
            &composition,
            temp.path(),
            run_id.clone(),
            resume_accounting(1, 0, 10),
            WriteExecutionMode::IsolatedWriter,
        );
        let (replay, task) = seed_in_flight_tool(store.as_ref(), request, Some(false)).await;
        let task = task.expect("writer task");
        let child = store
            .create(child_request(
                &replay.snapshot.request,
                &task,
                resume_accounting(0, 1, 5),
            ))
            .await
            .expect("create divergent child replay");
        store.release(&child.lease).await.expect("release child");

        let error = recover_writer_accounting(&run_id, &replay, store.as_ref())
            .await
            .expect_err("regressed root counters must fail closed");
        assert_eq!(error.code, RunApiErrorCode::RunEnvironmentMismatch);
        assert!(
            error
                .message
                .starts_with("run_resume_writer_accounting_diverged：")
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_start_command_is_canonical_before_creation_reservation() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let workspace = parent.path().join("workspace");
        let alias = parent.path().join("workspace-alias");
        std::fs::create_dir(&workspace).expect("create workspace");
        std::os::unix::fs::symlink(&workspace, &alias).expect("create workspace symlink");

        let prepared = prepare_production_start_command(
            start_command(&alias, Some("deepseek-v4-flash")),
            &ProductionToolConfig::new(&workspace),
        )
        .expect("prepare canonical start command");
        assert_eq!(
            prepared.workspace,
            stable_path(&workspace.canonicalize().expect("canonical workspace"))
        );
    }

    #[test]
    fn production_start_preparation_accepts_only_auto_or_official_models() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let tools = ProductionToolConfig::new(workspace.path());

        for model in [None, Some("deepseek-v4-pro"), Some("deepseek-v4-flash")] {
            let prepared =
                prepare_production_start_command(start_command(workspace.path(), model), &tools)
                    .expect("supported model selection");
            assert_eq!(prepared.model.as_deref(), model);
        }

        for model in ["deepseek-chat", "deepseek-reasoner", "gpt-5.5-codex"] {
            let error = prepare_production_start_command(
                start_command(workspace.path(), Some(model)),
                &tools,
            )
            .expect_err("unsupported model must fail before creation reservation");
            assert_eq!(error.code, RunApiErrorCode::InvalidRequest);
            assert!(
                error
                    .message
                    .contains("unsupported official DeepSeek model")
            );
        }
    }

    #[test]
    fn production_start_replaces_the_caller_authored_verifier_plan() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let mut command = start_command(workspace.path(), Some("deepseek-v4-flash"));
        command.task = caller_authored_verifier_task();
        let prepared =
            prepare_production_start_command(command, &ProductionToolConfig::new(workspace.path()))
                .expect("resolve verifier before reservation");
        let TaskAcceptance::Verifier { verifier, .. } = &prepared.task.acceptance[0] else {
            panic!("expected verifier acceptance");
        };
        assert_eq!(verifier.plan.steps[0].id, "exact-check");
        assert_eq!(verifier.plan.steps[0].program, "/usr/bin/python3");
        assert_eq!(verifier.plan.steps[0].timeout_ms, 600_000);
        assert_eq!(
            verifier.plan.steps[0].env.get("PYTHONDONTWRITEBYTECODE"),
            Some(&"1".to_owned())
        );
    }

    #[test]
    fn official_v4_context_policy_reserves_output_and_fixed_catalog_uncertainty() {
        for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
            let capability = official_model_capabilities(model).expect("official V4 model");
            assert_eq!(
                capability.context_window_tokens,
                OFFICIAL_V4_CONTEXT_WINDOW_TOKENS
            );
            assert_eq!(capability.max_output_tokens, OFFICIAL_V4_MAX_OUTPUT_TOKENS);
            for output in [
                OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS,
                OFFICIAL_V4_MAX_OUTPUT_TOKENS,
            ] {
                let policy = production_context_policy(capability, output);
                let expected_hard = OFFICIAL_V4_CONTEXT_WINDOW_TOKENS
                    .saturating_sub(output)
                    .saturating_sub(CONTEXT_INPUT_SAFETY_TOKENS);
                assert_eq!(policy.hard_input_tokens, expected_hard);
            }
        }
    }

    #[test]
    fn fixed_model_visible_tool_catalog_fits_the_context_safety_reserve() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let tool_config = tool_config_for_run(
            &ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full),
            temp.path(),
            &RunProductControls {
                interactive: true,
                ..RunProductControls::default()
            },
        )
        .expect("tool config");
        let runtime = AgentRuntime::new(
            Arc::new(ReplayOnlyModelPort),
            Arc::new(ProductionToolExecutor::new(tool_config)),
            Arc::new(NullEventSink),
            Arc::new(codewhale_runtime::InMemoryRunStore::default()),
        );
        let catalog = runtime.tool_definitions(
            &ToolPolicy::default(),
            None,
            ModelToolAuthority::RootWrite,
            0,
            4,
            true,
        );
        let coordinator_catalog = runtime.tool_definitions(
            &ToolPolicy::default(),
            None,
            ModelToolAuthority::Coordinator,
            0,
            4,
            true,
        );
        let coordinator_names = coordinator_catalog
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            coordinator_names,
            [
                "agent",
                "file_search",
                "git_diff",
                "git_status",
                "grep_files",
                "list_dir",
                "read_file",
                "request_user_input",
            ]
        );
        let coordinator_agent = coordinator_catalog
            .iter()
            .find(|definition| definition.name == AGENT_TOOL_NAME)
            .expect("coordinator agent definition");
        assert_eq!(
            coordinator_agent.input_schema["properties"]["workspace_access"]["enum"],
            json!(["read_only", "isolated_write"])
        );
        let root_agent = catalog
            .iter()
            .find(|definition| definition.name == AGENT_TOOL_NAME)
            .expect("root agent definition");
        assert_eq!(
            root_agent.input_schema["properties"]["workspace_access"]["enum"],
            json!(["read_only"])
        );
        let conservative_tokens = serde_json::to_vec(&catalog)
            .expect("serialize fixed catalog")
            .len()
            .div_ceil(3);
        assert!(
            conservative_tokens < CONTEXT_INPUT_SAFETY_TOKENS as usize,
            "fixed catalog must fit the production safety reserve"
        );
    }

    #[test]
    fn actual_actor_catalogs_freeze_strict_fallback_without_touching_historical_manifests() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let tool_config = tool_config_for_run(
            &ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full),
            temp.path(),
            &RunProductControls::default(),
        )
        .expect("tool config");
        let runtime = AgentRuntime::new(
            Arc::new(ReplayOnlyModelPort),
            Arc::new(ProductionToolExecutor::new(tool_config)),
            Arc::new(NullEventSink),
            Arc::new(InMemoryRunStore::default()),
        );
        let policy = ToolPolicy::default();
        let catalogs = [
            (
                "root_headless",
                runtime.tool_definitions(&policy, None, ModelToolAuthority::RootWrite, 0, 4, false),
                Some(("agent", "$/required", "all_properties_required")),
            ),
            (
                "root_interactive",
                runtime.tool_definitions(&policy, None, ModelToolAuthority::RootWrite, 0, 4, true),
                Some(("agent", "$/required", "all_properties_required")),
            ),
            (
                "coordinator",
                runtime.tool_definitions(
                    &policy,
                    None,
                    ModelToolAuthority::Coordinator,
                    0,
                    4,
                    false,
                ),
                Some(("agent", "$/required", "all_properties_required")),
            ),
            (
                "read_only_child",
                runtime.tool_definitions(&policy, None, ModelToolAuthority::ReadOnly, 1, 4, false),
                Some(("agent", "$/required", "all_properties_required")),
            ),
            (
                "read_only_depth_limit",
                runtime.tool_definitions(&policy, None, ModelToolAuthority::ReadOnly, 4, 4, false),
                Some(("file_search", "$/required", "all_properties_required")),
            ),
            (
                "isolated_writer",
                runtime.tool_definitions(
                    &policy,
                    None,
                    ModelToolAuthority::IsolatedWriter,
                    1,
                    1,
                    false,
                ),
                Some(("apply_patch", "$/oneOf", "unsupported_keyword")),
            ),
            ("terminal_empty", Vec::new(), None),
        ];

        for (label, catalog, expected) in catalogs {
            let request = ModelRequest {
                run_id: RunId::from(format!("strict-matrix-{label}")),
                parent_run_id: None,
                actor: AgentActor::default(),
                model: "deepseek-v4-pro".to_owned(),
                system_prompt: codewhale_protocol::agent_runtime::SystemPrompt::from_text(
                    "冻结实际工具目录",
                ),
                messages: Vec::new(),
                tools: catalog.clone(),
                reasoning_effort: ReasoningEffort::Off,
                max_output_tokens: Some(64),
                streaming: false,
                request_number: 1,
                attempt: 0,
            };
            let plan = plan_runtime_chat(
                RuntimeChatPlanInput {
                    root: "https://api.deepseek.com",
                    strict_enabled: true,
                    wire_model: request.model.clone(),
                    max_tokens: 64,
                },
                &request,
            )
            .expect("actual actor catalog has a deterministic request plan");
            assert_eq!(plan.surface, ApiSurface::StandardChat, "{label}");
            let decision = plan.tool_surface.expect("Chat decision");
            match expected {
                Some((tool_name, path, code)) => assert_eq!(
                    decision.reason,
                    ToolSurfaceReason::IncompatibleCatalog {
                        tool_name: tool_name.to_owned(),
                        issue: StrictSchemaIssue {
                            path: path.to_owned(),
                            code: code.to_owned(),
                        },
                    },
                    "{label}"
                ),
                None => assert_eq!(decision.reason, ToolSurfaceReason::NoTools, "{label}"),
            }

            let wire_tools = plan.body.get("tools").and_then(Value::as_array);
            if catalog.is_empty() {
                assert!(wire_tools.is_none(), "{label}");
                continue;
            }
            let wire_tools = wire_tools.expect("non-empty actor catalog reaches Chat");
            assert_eq!(wire_tools.len(), catalog.len(), "{label}");
            for (wire, canonical) in wire_tools.iter().zip(&catalog) {
                assert_eq!(wire["function"]["name"], canonical.name, "{label}");
                assert_eq!(
                    wire["function"]["description"], canonical.description,
                    "{label}"
                );
                assert_eq!(
                    wire["function"]["parameters"], canonical.input_schema,
                    "{label}"
                );
                assert!(wire["function"].get("strict").is_none(), "{label}");
            }
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
        assert!(
            temp.path().join("worktrees").is_dir(),
            "managed writer root must be a state-db sibling"
        );
        let store = app.store.clone();
        let runtime = Arc::new(AgentRuntime::new(
            Arc::new(OneShotModel),
            Arc::new(NoTools),
            Arc::new(NullEventSink),
            store.clone(),
        ));
        let mut request = test_run_request(RunId::new(), "本地终态", "本地测试 prompt");
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
                .list_root_runs(
                    &temp
                        .path()
                        .canonicalize()
                        .expect("canonical workspace")
                        .display()
                        .to_string(),
                    1,
                )
                .await
                .expect("query runs")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn ordinary_start_does_not_require_writer_git_admission() {
        let server =
            MockDeepSeekServer::start(vec![response("deepseek-v4-flash", "完成", 5, 2)]).await;
        let temp = tempfile::tempdir().expect("temporary parent");
        let workspace = temp.path().join("repo");
        let state_dir = temp.path().join("state");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::create_dir(&state_dir).expect("state directory");
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "CodeWhale Test"],
            vec!["config", "user.email", "test@codewhale.local"],
        ] {
            let output = ProcessCommand::new("git")
                .current_dir(&workspace)
                .args(args)
                .output()
                .expect("launch git");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::write(workspace.join("tracked.txt"), "base\n").expect("tracked file");
        for args in [&["add", "tracked.txt"][..], &["commit", "-m", "base"][..]] {
            let output = ProcessCommand::new("git")
                .current_dir(&workspace)
                .args(args)
                .output()
                .expect("launch git");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::write(workspace.join("dirty.txt"), "dirty\n").expect("dirty file");

        let app = AgentApplication::production(config(
            &state_dir.join("state.db"),
            connection(&server.root, false),
            true,
        ))
        .expect("production app");
        let run = run_result(
            app.execute(envelope(
                "dirty-root-no-writer",
                RunCommand::Start(start_command(&workspace, Some("deepseek-v4-flash"))),
            ))
            .await,
        );
        let replay = wait_terminal(app.store.as_ref(), &run.run_id).await;
        assert!(matches!(
            replay.snapshot.terminal,
            Some(AgentOutcome {
                terminal: TerminalState::Completed { .. },
                ..
            })
        ));
        assert_eq!(server.finish().await.len(), 1);
        assert!(workspace.join("dirty.txt").is_file());
    }

    #[tokio::test]
    async fn production_resolves_verifier_then_seals_the_exact_host_receipt() {
        let server =
            MockDeepSeekServer::start(vec![response("deepseek-v4-flash", "完成", 5, 2)]).await;
        let temp = tempfile::tempdir().expect("temporary parent");
        let workspace = temp.path().join("repo");
        let state_dir = temp.path().join("state");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::create_dir(&state_dir).expect("state directory");
        std::fs::write(workspace.join("verify.py"), "import sys\nsys.exit(0)\n")
            .expect("verifier fixture");
        for args in [
            &["init", "-b", "main"][..],
            &["config", "user.name", "CodeWhale Test"][..],
            &["config", "user.email", "test@codewhale.local"][..],
            &["add", "verify.py"][..],
            &["commit", "-m", "fixture"][..],
        ] {
            let output = ProcessCommand::new("git")
                .current_dir(&workspace)
                .args(args)
                .output()
                .expect("launch git");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let app = AgentApplication::production(config(
            &state_dir.join("state.db"),
            connection(&server.root, false),
            true,
        ))
        .expect("production app");
        let mut command = start_command(&workspace, Some("deepseek-v4-flash"));
        command.task = caller_authored_verifier_task();
        let run = run_result(
            app.execute(envelope(
                "resolved-verifier-receipt",
                RunCommand::Start(command),
            ))
            .await,
        );
        let replay = wait_terminal(app.store.as_ref(), &run.run_id).await;
        assert!(matches!(
            replay.snapshot.terminal,
            Some(AgentOutcome {
                terminal: TerminalState::Completed { .. },
                ..
            })
        ));
        let frozen = replay
            .events
            .iter()
            .find_map(|stored| match &stored.event {
                RuntimeEventKind::RunCreated { request } => request
                    .task_contract
                    .as_ref()
                    .and_then(|contract| match &contract.definition.acceptance[0] {
                        TaskAcceptance::Verifier { verifier, .. } => Some(verifier.clone()),
                        TaskAcceptance::Host { .. } => None,
                    }),
                _ => None,
            })
            .expect("frozen resolved verifier");
        let (observation, receipt) = replay
            .events
            .iter()
            .find_map(|stored| match &stored.event {
                RuntimeEventKind::HostVerificationCommitted {
                    outcome, receipt, ..
                } => Some((
                    outcome
                        .verifier_observation
                        .as_ref()
                        .expect("Host verifier observation")
                        .spec
                        .clone(),
                    receipt.as_deref().expect("canonical receipt").clone(),
                )),
                _ => None,
            })
            .expect("Host verification commit");
        assert_eq!(frozen, observation);
        assert_eq!(frozen, receipt.verifier);
        assert_eq!(
            frozen.plan.steps[0].env.get("PYTHONDONTWRITEBYTECODE"),
            Some(&"1".to_owned())
        );
        assert_eq!(server.finish().await.len(), 1);
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
        let workspace = temp
            .path()
            .canonicalize()
            .expect("canonical workspace")
            .display()
            .to_string();
        assert!(
            app.store
                .list_root_runs(&workspace, 10)
                .await
                .expect("query runs")
                .is_empty()
        );
        assert!(
            app.store
                .list_pending_creations(&workspace, 10)
                .await
                .expect("query pending creations")
                .is_empty()
        );
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
        command.controls.write_execution_mode =
            codewhale_protocol::agent_runtime::WriteExecutionMode::IsolatedWriter;
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
        assert_eq!(
            persisted
                .task_contract
                .as_ref()
                .expect("persisted task contract")
                .definition,
            command.task
        );
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
        assert_eq!(
            persisted.environment.write_execution_mode,
            command.controls.write_execution_mode
        );
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
        let run_id = RunId::from(format!("mismatch-{suffix}"));
        let mut request =
            test_run_request(run_id.clone(), format!("恢复 {suffix}"), "persisted prompt");
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
        let current_catalog = canonical_tool_catalog_sha256(&catalog_runtime.tool_definitions(
            &ToolPolicy::default(),
            None,
            ModelToolAuthority::RootWrite,
            0,
            0,
            false,
        ));
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

    #[tokio::test]
    async fn stale_context_policy_fails_resume_before_any_http_request() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = temp.path().canonicalize().expect("canonical workspace");
        let (root, accepted) = quiet_loopback().await;
        let connection = connection(&root, false);
        let tools = ProductionToolConfig::new(".").with_shell_policy(ShellPolicy::Full);
        let controls = RunProductControls {
            sandbox: Some("workspace-write".to_owned()),
            ..RunProductControls::default()
        };
        let bound_tools =
            tool_config_for_run(&tools, &workspace, &controls).expect("bound tool config");
        let tool_identity = bound_tools.execution_identity();
        let store =
            Arc::new(StateStore::open(Some(temp.path().join("state.db"))).expect("state store"));
        let catalog_runtime = AgentRuntime::new(
            Arc::new(ReplayOnlyModelPort),
            Arc::new(ProductionToolExecutor::new(bound_tools)),
            Arc::new(NullEventSink),
            store.clone(),
        );
        let catalog = catalog_runtime.tool_definitions(
            &ToolPolicy::default(),
            None,
            ModelToolAuthority::RootWrite,
            0,
            0,
            false,
        );
        let catalog_sha256 = canonical_tool_catalog_sha256(&catalog);
        let composition = Arc::new(ProductionComposition {
            deepseek: connection,
            credential: Some(
                DeepSeekCredential::new("test-deepseek-key").expect("test credential"),
            ),
            http_client: None,
            tools,
            prompt: ProductionPromptConfig::default(),
            managed_worktree_root: {
                let path = temp.path().join("worktrees");
                std::fs::create_dir(&path).expect("managed worktree root");
                path
            },
            composition_build_revision: "test-composition".to_owned(),
            default_max_api_requests: NonZeroU32::new(DEFAULT_MAX_API_REQUESTS)
                .expect("non-zero default"),
        });
        let fingerprint = composition.execution_fingerprint_sha256(
            "deepseek-v4-pro",
            &tool_identity,
            &catalog_sha256,
        );
        let run_id = RunId::from("stale-context-policy");
        let mut request =
            test_run_request(run_id.clone(), "恢复陈旧上下文策略", "persisted prompt");
        request.model = "deepseek-v4-pro".to_owned();
        request.max_output_tokens = Some(OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS);
        request.limits.max_depth = 0;
        request.context_policy = ContextPolicy::default();
        request.environment = RunEnvironment {
            workspace: stable_path(&workspace),
            provider: DEEPSEEK_PROVIDER.to_owned(),
            tool_catalog_sha256: Some(catalog_sha256),
            execution_fingerprint_sha256: Some(fingerprint),
            sandbox: controls.sandbox,
            ..RunEnvironment::default()
        };
        let created = store.create(request).await.expect("seed resumable run");
        store
            .release(&created.lease)
            .await
            .expect("release seeded run");
        let app = AgentApplication::from_parts(store, composition);
        let error = error_result(
            app.execute(envelope(
                "stale-context-policy",
                RunCommand::Resume {
                    run_id,
                    expected_workspace: None,
                },
            ))
            .await,
        );
        assert_eq!(error.code, RunApiErrorCode::RunEnvironmentMismatch);
        assert!(
            error
                .message
                .starts_with("run_resume_context_policy_mismatch：")
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
        assert_eq!(
            isolated_edit.failure_code,
            Some(ToolFailureCode::WorkspacePrecondition)
        );
        assert!(isolated_edit.content.contains("尚未读取"));
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
