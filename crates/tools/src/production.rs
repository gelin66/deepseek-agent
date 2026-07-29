//! Fixed production tool catalog and direct runtime executor.
//!
//! This is the sole model-visible tool owner. It intentionally uses direct
//! exact-name dispatch instead of a second registry or handler abstraction.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use dse_context::skills::SkillRegistry;
use dse_execpolicy::{ExecPolicy, ExecPolicyDisposition};
use dse_protocol::agent_runtime::{
    ApprovalRisk, RunPermissionMode, ToolApprovalPrompt, ToolAuthorizationDecision,
    ToolAuthorizationDisposition, ToolDefinition, ToolExecutionGrant, ToolFailureCode,
    ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus, WorkspaceAccess,
};
use dse_protocol::task::{VerifierSpec, WorkspaceState};
use dse_runtime::{CancellationToken, ToolExecutionError, ToolExecutor, ToolInvocation};
use dse_secrets::Secrets;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken as TokioCancellationToken;

use crate::command_safety::{SafetyLevel, analyze_command, command_is_high_impact};
use crate::sandbox::SandboxPolicy as ExecutionSandboxPolicy;
use crate::sandbox::backend::{SandboxBackend, SandboxBackendIdentity};
use crate::semantic_browser::{
    BrowserCredentialGrant, BrowserInteractRequest, SemanticBrowserHarness,
    SystemSemanticBrowserHarness, browser_credential_grants_sha256, browser_harness_identity,
    default_browser_state_root, execute_browser_interact, execute_browser_navigate,
    parse_browser_interact_for_authorization, preflight_browser_interact,
    preflight_browser_navigate,
};
use crate::shell::{
    ExecShellHost, ExecShellOptions, ExecShellPolicyDecision, ShellPolicy,
    command_likely_needs_network, execute_exec_shell, new_shared_shell_manager,
    preflight_exec_shell,
};
use crate::web_fetch::{
    SystemWebFetchNetwork, WebFetchNetwork, execute_web_fetch, preflight_web_fetch,
};
use crate::web_search::{
    SystemWebSearchNetwork, WebSearchNetwork, execute_web_search_cancellable, preflight_web_search,
};
use crate::{
    APPLICATION_PROBE_VERIFIER_ID, ApplicationProbeRecovery, ProductionToolContext, ToolError,
    ToolOutcome, capture_workspace_revision, execute_application_probe, execute_apply_patch,
    execute_edit_file, execute_file_search, execute_git_diff, execute_git_status,
    execute_grep_files, execute_list_dir, execute_load_skill, execute_read_file, execute_run_tests,
    execute_run_verifiers, preflight_apply_patch, preflight_load_skill, recover_application_probe,
    resolve_application_probe_spec, resolve_run_tests_spec, resolve_run_verifiers_spec,
    validate_application_probe_spec,
};

pub const PRODUCTION_TOOL_NAMES: [&str; 16] = [
    "apply_patch",
    "browser_interact",
    "browser_navigate",
    "edit_file",
    "exec_shell",
    "file_search",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "load_skill",
    "read_file",
    "run_tests",
    "run_verifiers",
    "web_fetch",
    "web_search",
];

/// Immutable per-run configuration. It never contains live shell jobs,
/// cancellation state or read-before-edit freshness.
#[derive(Clone)]
pub struct ProductionToolConfig {
    workspace: PathBuf,
    allow_external_paths: bool,
    follow_symlinks: bool,
    permission_mode: RunPermissionMode,
    shell_policy: ShellPolicy,
    elevated_sandbox_policy: Option<ExecutionSandboxPolicy>,
    shell_network_denied_hint: Option<String>,
    sandbox_backend: Option<Arc<dyn SandboxBackend>>,
    prefer_external_pdftotext: bool,
    exec_policy: Option<ExecPolicy>,
    skill_registry: Arc<SkillRegistry>,
    web_fetch_network: Arc<dyn WebFetchNetwork>,
    web_search_network: Arc<dyn WebSearchNetwork>,
    semantic_browser_harness: Option<Arc<dyn SemanticBrowserHarness>>,
    browser_local_origin: Option<String>,
    browser_state_root: PathBuf,
    browser_credential_grants: Vec<BrowserCredentialGrant>,
    browser_secrets: Secrets,
}

#[derive(Clone)]
struct ProductionExecShellHost {
    exec_policy: Option<ExecPolicy>,
}

impl ExecShellHost for ProductionExecShellHost {
    fn evaluate_exec_policy(
        &self,
        command: &str,
    ) -> Result<Option<ExecShellPolicyDecision>, ToolError> {
        Ok(self.exec_policy.as_ref().and_then(|policy| {
            policy.evaluate(command).map(|matched| {
                let label = matched.rule_label();
                match matched.disposition {
                    ExecPolicyDisposition::Allow => ExecShellPolicyDecision::Allow(label),
                    ExecPolicyDisposition::Deny => ExecShellPolicyDecision::Deny(label),
                }
            })
        }))
    }
}

/// Stable non-secret identity material for resume fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionToolExecutionIdentity {
    pub schema: u32,
    pub workspace: String,
    pub allow_external_paths: bool,
    pub follow_symlinks: bool,
    pub permission_mode: RunPermissionMode,
    pub shell_policy: ShellPolicy,
    pub elevated_sandbox_policy: Option<ExecutionSandboxPolicy>,
    pub sandbox_backend: Option<SandboxBackendIdentity>,
    pub prefer_external_pdftotext: bool,
    pub shell_network_denied_hint_sha256: Option<String>,
    pub exec_policy_sha256: Option<String>,
    pub skills_snapshot_sha256: String,
    pub web_fetch_network_sha256: String,
    pub web_search_network_sha256: String,
    pub semantic_browser_harness_sha256: String,
    pub browser_local_origin_sha256: Option<String>,
    pub browser_state_root_sha256: String,
    pub browser_credential_grants_sha256: String,
    pub browser_secret_backend: String,
}

impl ProductionToolConfig {
    /// Create restrictive defaults for one workspace.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            allow_external_paths: false,
            follow_symlinks: false,
            permission_mode: RunPermissionMode::Ask,
            shell_policy: ShellPolicy::None,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            sandbox_backend: None,
            prefer_external_pdftotext: false,
            exec_policy: None,
            skill_registry: Arc::new(SkillRegistry::default()),
            web_fetch_network: Arc::new(SystemWebFetchNetwork),
            web_search_network: Arc::new(SystemWebSearchNetwork::default()),
            semantic_browser_harness: None,
            browser_local_origin: None,
            browser_state_root: default_browser_state_root(),
            browser_credential_grants: Vec::new(),
            browser_secrets: Secrets::auto_detect(),
        }
    }

    /// Clone-friendly per-run workspace binding. A later executor construction
    /// always creates fresh process and read-tracking state.
    #[must_use]
    pub fn with_workspace(mut self, workspace: impl Into<PathBuf>) -> Self {
        self.workspace = workspace.into();
        self
    }

    /// Clone this production configuration for one isolated writer worktree.
    ///
    /// This is intentionally a fresh per-run binding: it discards every path
    /// escape and remote execution capability, enables Host-approved tool
    /// execution, and requires the mandatory local writer sandbox. Constructing
    /// a new executor from the returned value also creates fresh shell-process
    /// and read-before-edit state.
    #[must_use]
    pub fn rebind_isolated_writer_workspace(&self, workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let mut rebound = self.clone();
        rebound.workspace = workspace.clone();
        rebound.allow_external_paths = false;
        rebound.follow_symlinks = false;
        rebound.permission_mode = self.permission_mode;
        rebound.elevated_sandbox_policy = Some(ExecutionSandboxPolicy::isolated_writer(workspace));
        rebound.shell_network_denied_hint = Some("隔离 Writer Agent 禁止访问网络".to_owned());
        rebound.sandbox_backend = None;
        rebound.browser_local_origin = None;
        rebound.browser_credential_grants.clear();
        rebound
    }

    #[must_use]
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: RunPermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self.allow_external_paths = matches!(
            permission_mode,
            RunPermissionMode::Agent | RunPermissionMode::FullAccess
        );
        self
    }

    #[must_use]
    pub fn with_shell_policy(mut self, shell_policy: ShellPolicy) -> Self {
        self.shell_policy = shell_policy;
        self
    }

    #[must_use]
    pub fn with_elevated_sandbox_policy(mut self, policy: ExecutionSandboxPolicy) -> Self {
        self.elevated_sandbox_policy = Some(policy);
        self
    }

    #[must_use]
    pub fn with_shell_network_denied_hint(mut self, hint: Option<String>) -> Self {
        self.shell_network_denied_hint = hint;
        self
    }

    #[must_use]
    pub fn with_sandbox_backend(mut self, backend: Arc<dyn SandboxBackend>) -> Self {
        self.sandbox_backend = Some(backend);
        self
    }

    #[must_use]
    pub fn with_prefer_external_pdftotext(mut self, prefer: bool) -> Self {
        self.prefer_external_pdftotext = prefer;
        self
    }

    #[must_use]
    pub fn with_exec_policy(mut self, policy: Option<ExecPolicy>) -> Self {
        self.exec_policy = policy;
        self
    }

    /// Bind the exact immutable Skill discovery snapshot shared with the
    /// canonical prompt. Rebinding a Writer workspace intentionally preserves
    /// this run-scoped grant instead of rediscovering filesystem state.
    #[must_use]
    pub fn with_skill_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
        self.skill_registry = registry;
        self
    }

    /// Replace only the stateless DNS/HTTP seam used by `web_fetch`.
    /// Production callers keep the pinned Rustls implementation; deterministic
    /// vertical tests use this to avoid depending on public network state.
    #[must_use]
    pub fn with_web_fetch_network(mut self, network: Arc<dyn WebFetchNetwork>) -> Self {
        self.web_fetch_network = network;
        self
    }

    /// Replace only the fixed canonical Web Search transport. Production uses
    /// the Host-owned Tavily Basic adapter; tests inject bounded responses.
    #[must_use]
    pub fn with_web_search_network(mut self, network: Arc<dyn WebSearchNetwork>) -> Self {
        self.web_search_network = network;
        self
    }

    /// Replace the bounded live-session semantic browser seam for deterministic vertical
    /// tests. Production uses the pinned Chrome for Testing implementation.
    #[must_use]
    pub fn with_semantic_browser_harness(
        mut self,
        harness: Arc<dyn SemanticBrowserHarness>,
    ) -> Self {
        self.semantic_browser_harness = Some(harness);
        self
    }

    /// Bind the one exact loopback application origin assigned by the Host.
    /// This authority is not model-visible and is dropped for Writer rebinds.
    #[must_use]
    pub fn with_browser_local_origin(mut self, origin: Option<String>) -> Self {
        self.browser_local_origin = origin;
        self
    }

    /// Bind the Host-owned browser data root. Managed profiles and download
    /// quarantine remain project-isolated beneath this directory.
    #[must_use]
    pub fn with_browser_state_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.browser_state_root = root.into();
        self
    }

    /// Add one exact non-secret login grant. Secret values remain in the
    /// existing DSE secret owner and are resolved only after authorization.
    #[must_use]
    pub fn with_browser_credential_grant(mut self, grant: BrowserCredentialGrant) -> Self {
        self.browser_credential_grants.push(grant);
        self
    }

    /// Replace only the existing DSE secret facade, primarily for isolated
    /// application composition and deterministic tests.
    #[must_use]
    pub fn with_browser_secrets(mut self, secrets: Secrets) -> Self {
        self.browser_secrets = secrets;
        self
    }

    /// Project this config into deterministic, serializable and non-secret
    /// fingerprint material. Live executor state is intentionally absent.
    #[must_use]
    pub fn execution_identity(&self) -> ProductionToolExecutionIdentity {
        ProductionToolExecutionIdentity {
            schema: 7,
            workspace: stable_path_identity(&self.workspace),
            allow_external_paths: self.allow_external_paths,
            follow_symlinks: self.follow_symlinks,
            permission_mode: self.permission_mode,
            shell_policy: self.shell_policy,
            elevated_sandbox_policy: self.elevated_sandbox_policy.clone(),
            sandbox_backend: self
                .sandbox_backend
                .as_ref()
                .map(|backend| backend.identity()),
            prefer_external_pdftotext: self.prefer_external_pdftotext,
            shell_network_denied_hint_sha256: self
                .shell_network_denied_hint
                .as_deref()
                .map(non_secret_sha256),
            exec_policy_sha256: self.exec_policy.as_ref().map(|policy| {
                let bytes = serde_json::to_vec(policy)
                    .expect("production exec policy snapshot is serializable");
                non_secret_sha256_bytes(&bytes)
            }),
            skills_snapshot_sha256: self.skill_registry.snapshot_sha256(),
            web_fetch_network_sha256: non_secret_sha256(self.web_fetch_network.identity()),
            web_search_network_sha256: non_secret_sha256(&self.web_search_network.identity()),
            semantic_browser_harness_sha256: non_secret_sha256(&browser_harness_identity(
                self.semantic_browser_harness.as_deref(),
                self.web_fetch_network.as_ref(),
            )),
            browser_local_origin_sha256: self
                .browser_local_origin
                .as_deref()
                .map(non_secret_sha256),
            browser_state_root_sha256: non_secret_sha256(&stable_path_identity(
                &self.browser_state_root,
            )),
            browser_credential_grants_sha256: browser_credential_grants_sha256(
                &self.browser_credential_grants,
            ),
            browser_secret_backend: self.browser_secrets.backend_name().to_owned(),
        }
    }

    /// Hash of [`Self::execution_identity`]. This is the tools portion of a
    /// run fingerprint, not the policy-filtered model catalog hash.
    #[must_use]
    pub fn execution_identity_sha256(&self) -> String {
        let bytes = serde_json::to_vec(&self.execution_identity())
            .expect("production tool identity is serializable");
        non_secret_sha256_bytes(&bytes)
    }
}

fn stable_path_identity(path: &std::path::Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn non_secret_sha256(value: &str) -> String {
    non_secret_sha256_bytes(value.as_bytes())
}

fn non_secret_sha256_bytes(value: &[u8]) -> String {
    let digest = Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

struct AuthorizationDecisionBuilder<'a> {
    mode: RunPermissionMode,
    execution_grant: &'a ToolExecutionGrant,
    invocation: &'a ToolInvocation,
    workspace_state: &'a WorkspaceState,
}

impl AuthorizationDecisionBuilder<'_> {
    fn build(
        &self,
        disposition: ToolAuthorizationDisposition,
        risk: ApprovalRisk,
        matched_rule: Option<String>,
        reason: impl Into<String>,
        prompt: Option<ToolApprovalPrompt>,
    ) -> ToolAuthorizationDecision {
        ToolAuthorizationDecision {
            mode: self.mode,
            execution_grant: self.execution_grant.clone(),
            tool_name: self.invocation.name.clone(),
            arguments_sha256: self.invocation.arguments_sha256(),
            workspace_state: self.workspace_state.clone(),
            disposition,
            risk,
            matched_rule,
            reason: reason.into(),
            prompt,
        }
    }
}

fn invocation_has_external_path(
    tool_name: &str,
    input: &Value,
    context: &ProductionToolContext,
    allow_external_verifier_program: bool,
) -> bool {
    let direct = ["path", "cwd"]
        .into_iter()
        .filter_map(|key| input.get(key).and_then(Value::as_str));
    let changes = input
        .get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|change| change.get("path").and_then(Value::as_str));
    let command_cwds = input
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| command.get("cwd").and_then(Value::as_str))
        .filter(|cwd| !cwd.is_empty());
    let command_programs = input
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| command.get("program").and_then(Value::as_str))
        .filter(|program| !program.is_empty())
        .filter(|_| !allow_external_verifier_program);
    let mut patch_paths = (tool_name == "apply_patch")
        .then(|| preflight_apply_patch(input).ok())
        .flatten()
        .into_iter()
        .flat_map(|preflight| preflight.touched_files);
    direct
        .chain(changes)
        .chain(command_cwds)
        .chain(command_programs)
        .any(|path| context.path_is_external(path))
        || patch_paths.any(|path| context.path_is_external(&path))
}

/// Direct executor for the fixed sixteen-tool production surface.
pub struct ProductionToolExecutor {
    context: ProductionToolContext,
    shell: ExecShellOptions,
    prefer_external_pdftotext: bool,
    shell_host: ProductionExecShellHost,
    skill_registry: Arc<SkillRegistry>,
    web_fetch_network: Arc<dyn WebFetchNetwork>,
    web_search_network: Arc<dyn WebSearchNetwork>,
    controlled_network_allowed: bool,
    semantic_browser_harness: Arc<dyn SemanticBrowserHarness>,
    browser_local_origin: Option<String>,
}

impl Drop for ProductionToolExecutor {
    fn drop(&mut self) {
        self.semantic_browser_harness.shutdown();
    }
}

impl ProductionToolExecutor {
    /// Construct one isolated run executor. The concrete shell host preserves
    /// the resolved exec-policy snapshot; built-in command safety, shell
    /// policy, frozen permission mode and sandbox enforcement remain inside
    /// `execute_exec_shell`. A no-op host is therefore never used at the
    /// production cutover boundary.
    #[must_use]
    pub fn new(config: ProductionToolConfig) -> Self {
        let isolated_writer_workspace = match config.elevated_sandbox_policy.as_ref() {
            Some(ExecutionSandboxPolicy::IsolatedWriter { workspace, .. }) => {
                Some(workspace.clone())
            }
            _ => None,
        };
        let controlled_network_allowed = isolated_writer_workspace.is_none();
        let mut context = ProductionToolContext::new(config.workspace.clone())
            .with_external_path_authority(config.allow_external_paths)
            .with_follow_symlinks(config.follow_symlinks)
            .with_permission_mode(config.permission_mode);
        if let Some(workspace) = isolated_writer_workspace {
            context = context.with_isolated_writer_write_guard(workspace);
        }
        let mut shell = ExecShellOptions::new(
            new_shared_shell_manager(config.workspace.clone()),
            config.shell_policy,
        );
        shell.elevated_sandbox_policy = config.elevated_sandbox_policy;
        shell.shell_network_denied_hint = config.shell_network_denied_hint;
        shell.sandbox_backend = config.sandbox_backend;
        let semantic_browser_harness = config.semantic_browser_harness.unwrap_or_else(|| {
            Arc::new(SystemSemanticBrowserHarness::new_managed(
                Arc::clone(&config.web_fetch_network),
                config.workspace.clone(),
                config.browser_state_root.clone(),
                config.browser_credential_grants.clone(),
                config.browser_secrets.clone(),
            ))
        });
        Self {
            context,
            shell,
            prefer_external_pdftotext: config.prefer_external_pdftotext,
            shell_host: ProductionExecShellHost {
                exec_policy: config.exec_policy,
            },
            skill_registry: config.skill_registry,
            web_fetch_network: config.web_fetch_network,
            web_search_network: config.web_search_network,
            controlled_network_allowed,
            semantic_browser_harness,
            browser_local_origin: config.browser_local_origin,
        }
    }

    /// Resolve one Host verifier request into the exact plan used by the
    /// production implementation. Callers must persist this result instead of
    /// trusting a duplicated, caller-authored execution plan.
    pub fn resolve_verifier_spec(
        &self,
        verifier_id: &str,
        parameters: Value,
    ) -> Result<VerifierSpec, ToolError> {
        if verifier_id == APPLICATION_PROBE_VERIFIER_ID {
            return resolve_application_probe_spec(parameters, &self.context);
        }
        validate_production_input(verifier_id, &parameters)?;
        match verifier_id {
            "run_tests" => resolve_run_tests_spec(parameters),
            "run_verifiers" => {
                let spec = resolve_run_verifiers_spec(parameters, &self.context)?;
                if spec.parameters.get("profile").and_then(Value::as_str) != Some("exact") {
                    return Err(ToolError::invalid_input(
                        "Host acceptance requires run_verifiers profile 'exact'; workspace-detected profiles remain advisory tools because their plan can change after edits",
                    ));
                }
                Ok(spec)
            }
            _ => Err(ToolError::invalid_input(format!(
                "tool '{verifier_id}' cannot produce canonical Host verification evidence"
            ))),
        }
    }

    /// Validate one already persisted Host verifier without regenerating its
    /// durable process identity.
    pub fn validate_verifier_spec_exact(&self, verifier: &VerifierSpec) -> Result<(), ToolError> {
        if verifier.verifier_id == APPLICATION_PROBE_VERIFIER_ID {
            validate_application_probe_spec(verifier, &self.context)
        } else {
            let resolved =
                self.resolve_verifier_spec(&verifier.verifier_id, verifier.parameters.clone())?;
            if resolved == *verifier {
                Ok(())
            } else {
                Err(ToolError::invalid_input(
                    "persisted verifier specification differs from the production resolver",
                ))
            }
        }
    }

    /// Recover an in-flight one-shot verifier from facts already committed in
    /// `HostVerificationPrepared`. Ordinary verifiers have no retained
    /// process identity and therefore perform no cleanup here.
    pub async fn recover_inflight_verifier(
        &self,
        verifier: &VerifierSpec,
    ) -> Result<Option<ApplicationProbeRecovery>, ToolError> {
        if verifier.verifier_id == APPLICATION_PROBE_VERIFIER_ID {
            recover_application_probe(verifier, &self.context)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn unavailable(name: &str) -> ToolOutcome {
        ToolOutcome::rejected(
            format!("工具 '{name}' 未在生产 AgentRuntime 工具目录中提供"),
            ToolRetryDisposition::NotRetryable,
        )
        .with_failure_code(ToolFailureCode::UnknownTool)
    }

    fn preflight_error_outcome(error: ToolError) -> ToolOutcome {
        let code = match error {
            ToolError::SchemaValidation { .. } => ToolFailureCode::SchemaValidation,
            ToolError::MissingField { .. } => ToolFailureCode::MissingField,
            ToolError::PatchParse { .. } => ToolFailureCode::PatchParse,
            ToolError::NotAvailable { .. } => ToolFailureCode::UnknownTool,
            ToolError::InvalidInput { .. } | ToolError::PathEscape { .. } => {
                ToolFailureCode::InvalidField
            }
            ToolError::PermissionDenied { .. } => ToolFailureCode::InvocationRejected,
            ToolError::WorkspacePrecondition { .. }
            | ToolError::StaleRead { .. }
            | ToolError::AmbiguousEdit { .. }
            | ToolError::Timeout { .. }
            | ToolError::ExecutionFailed { .. } => ToolFailureCode::OperationFailed,
        };
        ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::AfterCorrection)
            .with_failure_code(code)
    }

    /// Convert an error observed after `ToolExecutionStarted`. Even a
    /// correctable input-semantic failure is now an accepted operation that
    /// failed without applying a side effect; it can no longer claim the
    /// preflight-only Rejected/NotStarted lifecycle.
    fn execution_error_outcome(error: ToolError, workspace_access: WorkspaceAccess) -> ToolOutcome {
        let outcome = match &error {
            ToolError::SchemaValidation { .. }
            | ToolError::InvalidInput { .. }
            | ToolError::MissingField { .. }
            | ToolError::PathEscape { .. }
            | ToolError::PatchParse { .. } => {
                // PatchParse is emitted only by preflight. A caller that
                // skips preflight receives an observed invalid operation,
                // never the preflight-only PatchParse lifecycle.
                let mut outcome = ToolOutcome::error(error.to_string())
                    .with_failure_code(ToolFailureCode::InvalidField);
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
                outcome.retry = ToolRetryDisposition::AfterCorrection;
                outcome
            }
            ToolError::WorkspacePrecondition { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string())
                    .with_failure_code(ToolFailureCode::WorkspacePrecondition);
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
                outcome.retry = ToolRetryDisposition::AfterCorrection;
                outcome
            }
            ToolError::StaleRead { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string())
                    .with_failure_code(ToolFailureCode::StaleRead);
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
                outcome.retry = ToolRetryDisposition::AfterCorrection;
                outcome
            }
            ToolError::AmbiguousEdit { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string())
                    .with_failure_code(ToolFailureCode::AmbiguousEdit);
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
                outcome.retry = ToolRetryDisposition::AfterCorrection;
                outcome
            }
            ToolError::NotAvailable { .. } | ToolError::PermissionDenied { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string());
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
                outcome.retry = ToolRetryDisposition::NotRetryable;
                outcome
            }
            ToolError::Timeout { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string());
                outcome.operation = ToolOperationStatus::Indeterminate;
                outcome.retry = ToolRetryDisposition::Unsafe;
                outcome
            }
            ToolError::ExecutionFailed { .. } => ToolOutcome::error(error.to_string()),
        };
        outcome.with_workspace_access_guarantee(workspace_access)
    }

    async fn dispatch(
        &self,
        run_id: &str,
        name: &str,
        input: Value,
        context: &ProductionToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        match name {
            "apply_patch" => execute_apply_patch(input, context),
            "browser_interact" => Ok(execute_browser_interact(
                run_id,
                input,
                Arc::clone(&self.semantic_browser_harness),
                self.controlled_network_allowed,
                context.cancellation_token().cloned().unwrap_or_default(),
            )
            .await),
            "browser_navigate" => Ok(execute_browser_navigate(
                run_id,
                input,
                Arc::clone(&self.semantic_browser_harness),
                self.controlled_network_allowed,
                self.browser_local_origin.as_deref(),
                context.cancellation_token().cloned().unwrap_or_default(),
            )
            .await),
            "edit_file" => execute_edit_file(input, context),
            "exec_shell" => execute_exec_shell(input, context, &self.shell, &self.shell_host).await,
            "file_search" => execute_file_search(input, context).await,
            "git_diff" => execute_git_diff(input, context),
            "git_status" => execute_git_status(input, context),
            "grep_files" => execute_grep_files(input, context).await,
            "list_dir" => execute_list_dir(input, context).await,
            "load_skill" => execute_load_skill(input, &self.skill_registry),
            "read_file" => execute_read_file(input, context, self.prefer_external_pdftotext),
            "run_tests" => execute_run_tests(input, context, &self.shell).await,
            "run_verifiers" => execute_run_verifiers(input, context, &self.shell).await,
            APPLICATION_PROBE_VERIFIER_ID => {
                execute_application_probe(input, context, &self.shell).await
            }
            "web_fetch" => Ok(execute_web_fetch(
                input,
                Arc::clone(&self.web_fetch_network),
                self.controlled_network_allowed,
            )
            .await),
            "web_search" => Ok(execute_web_search_cancellable(
                input,
                Arc::clone(&self.web_search_network),
                self.controlled_network_allowed,
                context.cancellation_token().cloned().unwrap_or_default(),
            )
            .await),
            _ => unreachable!("validated production tool missing direct dispatch: {name}"),
        }
    }
}

#[async_trait]
impl ToolExecutor for ProductionToolExecutor {
    fn definitions(&self) -> Vec<ToolDefinition> {
        production_tool_definitions()
    }

    fn definition_workspace_access(&self, name: &str) -> WorkspaceAccess {
        match name {
            "browser_navigate" | "file_search" | "git_diff" | "git_status" | "grep_files"
            | "list_dir" | "load_skill" | "read_file" | "web_fetch" | "web_search" => {
                WorkspaceAccess::ReadOnly
            }
            _ => WorkspaceAccess::MayWrite,
        }
    }

    fn workspace_access(&self, invocation: &ToolInvocation) -> WorkspaceAccess {
        match invocation.name.as_str() {
            "browser_navigate" | "file_search" | "git_diff" | "git_status" | "grep_files"
            | "list_dir" | "load_skill" | "read_file" | "web_fetch" | "web_search" => {
                WorkspaceAccess::ReadOnly
            }
            _ => WorkspaceAccess::MayWrite,
        }
    }

    fn preflight(&self, invocation: &ToolInvocation) -> Option<ToolOutcome> {
        if !PRODUCTION_TOOL_NAMES.contains(&invocation.name.as_str()) {
            return Some(Self::unavailable(&invocation.name));
        }
        let Some(input) = invocation.arguments.parsed.as_ref() else {
            return Some(
                ToolOutcome::rejected(
                    format!(
                        "工具 '{}' 的 JSON 参数格式错误，请重新生成有效对象。",
                        invocation.name
                    ),
                    ToolRetryDisposition::AfterCorrection,
                )
                .with_failure_code(ToolFailureCode::MalformedArguments),
            );
        };
        if let Err(error) = validate_production_input(&invocation.name, input) {
            return Some(Self::preflight_error_outcome(error));
        }
        if invocation.name == "apply_patch"
            && let Err(error) = preflight_apply_patch(input)
        {
            let error = if input.get("changes").is_some() {
                error
            } else {
                ToolError::patch_parse(error.to_string())
            };
            return Some(Self::preflight_error_outcome(error));
        }
        if invocation.name == "exec_shell" {
            return match preflight_exec_shell(input, &self.shell) {
                Ok(outcome) => outcome,
                Err(error) => Some(Self::preflight_error_outcome(error)),
            };
        }
        if invocation.name == "load_skill"
            && let Err(error) = preflight_load_skill(input, &self.skill_registry)
        {
            return Some(Self::preflight_error_outcome(error));
        }
        if invocation.name == "web_fetch" {
            return preflight_web_fetch(input);
        }
        if invocation.name == "web_search" {
            return preflight_web_search(input);
        }
        if invocation.name == "browser_navigate" {
            return preflight_browser_navigate(input, self.browser_local_origin.as_deref());
        }
        if invocation.name == "browser_interact" {
            return preflight_browser_interact(input);
        }
        None
    }

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        capture_workspace_revision(self.context.workspace())
            .await
            .map_err(|message| ToolExecutionError::new("workspace_revision_unavailable", message))
    }

    fn authorize(
        &self,
        mode: RunPermissionMode,
        execution_grant: &ToolExecutionGrant,
        invocation: &ToolInvocation,
        workspace_state: &WorkspaceState,
    ) -> Result<ToolAuthorizationDecision, ToolExecutionError> {
        if mode != self.context.permission_mode() {
            return Err(ToolExecutionError::new(
                "permission_mode_mismatch",
                "tool executor permission mode differs from the frozen Run",
            ));
        }
        let input = invocation.arguments.parsed.as_ref().ok_or_else(|| {
            ToolExecutionError::new(
                "authorization_arguments_missing",
                "tool authorization requires parsed arguments",
            )
        })?;
        execution_grant
            .validate()
            .map_err(|message| ToolExecutionError::new("execution_grant_invalid", message))?;
        let decision = AuthorizationDecisionBuilder {
            mode,
            execution_grant,
            invocation,
            workspace_state,
        };

        let allow_external_verifier_program = match execution_grant {
            ToolExecutionGrant::Ordinary => false,
            ToolExecutionGrant::TaskContractVerifier {
                verifier_sha256, ..
            } => self
                .resolve_verifier_spec(&invocation.name, input.clone())
                .is_ok_and(|verifier| verifier.sha256() == *verifier_sha256),
        };
        if matches!(
            execution_grant,
            ToolExecutionGrant::TaskContractVerifier { .. }
        ) && !allow_external_verifier_program
        {
            return Ok(decision.build(
                ToolAuthorizationDisposition::Deny,
                ApprovalRisk::Elevated,
                Some("task_contract_verifier_grant_invalid".to_owned()),
                "冻结 verifier 执行授权与当前 canonical verifier 不一致",
                None,
            ));
        }

        if invocation_has_external_path(
            &invocation.name,
            input,
            &self.context,
            allow_external_verifier_program,
        ) && !self.context.allows_external_paths()
        {
            return Ok(decision.build(
                ToolAuthorizationDisposition::Deny,
                ApprovalRisk::Elevated,
                Some(if mode == RunPermissionMode::Ask {
                    "ask_external_path_fail_closed".to_owned()
                } else {
                    "actor_external_path_denied".to_owned()
                }),
                if mode == RunPermissionMode::Ask {
                    "当前执行后端不能证明一次性外部路径授权范围，已安全拒绝"
                } else {
                    "当前 actor 的冻结执行边界禁止外部路径访问"
                },
                None,
            ));
        }

        if invocation.name == "exec_shell" {
            let command = input
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ToolExecutionError::new(
                        "authorization_command_missing",
                        "exec_shell authorization requires command",
                    )
                })?;
            let explicit_rule = self
                .shell_host
                .evaluate_exec_policy(command)
                .map_err(|error| {
                    ToolExecutionError::new("execpolicy_evaluation_failed", error.to_string())
                })?;
            if let Some(ExecShellPolicyDecision::Deny(reason)) = explicit_rule.as_ref() {
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Deny,
                    ApprovalRisk::Critical,
                    Some(reason.clone()),
                    "explicit execpolicy deny",
                    None,
                ));
            }
            let safety = analyze_command(command);
            if safety.level == SafetyLevel::Dangerous {
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Deny,
                    ApprovalRisk::Critical,
                    Some("host_hard_invariant".to_owned()),
                    format!("内建安全不变量拒绝该命令：{}", safety.reasons.join("; ")),
                    None,
                ));
            }
            let high_risk = command_is_high_impact(command);
            let needs_network = command_likely_needs_network(command);
            let sandbox_denies_network = self
                .shell
                .elevated_sandbox_policy
                .as_ref()
                .is_some_and(|policy| !policy.has_network_access());
            if needs_network && (mode == RunPermissionMode::Ask || sandbox_denies_network) {
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Deny,
                    ApprovalRisk::Elevated,
                    Some(if mode == RunPermissionMode::Ask {
                        "ask_network_fail_closed".to_owned()
                    } else {
                        "sandbox_network_denied".to_owned()
                    }),
                    if mode == RunPermissionMode::Ask {
                        "当前执行后端不能证明一次性网络授权范围，已安全拒绝"
                    } else {
                        "当前 actor 的冻结执行边界禁止网络访问"
                    },
                    None,
                ));
            }
            let needs_approval = match mode {
                RunPermissionMode::Ask => {
                    high_risk || safety.level == SafetyLevel::RequiresApproval
                }
                RunPermissionMode::Agent => high_risk,
                RunPermissionMode::FullAccess => false,
            };
            if let Some(ExecShellPolicyDecision::Allow(rule)) = explicit_rule.as_ref()
                && (!high_risk || mode == RunPermissionMode::FullAccess)
            {
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Allow,
                    if high_risk {
                        ApprovalRisk::Critical
                    } else {
                        ApprovalRisk::Routine
                    },
                    Some(rule.clone()),
                    "explicit execpolicy allow",
                    None,
                ));
            }
            if needs_approval {
                let risk = if high_risk {
                    ApprovalRisk::Critical
                } else {
                    ApprovalRisk::Elevated
                };
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Ask,
                    risk,
                    Some(if high_risk {
                        "host_high_risk".to_owned()
                    } else {
                        "host_elevated_shell".to_owned()
                    }),
                    "Host 工具分类要求本次调用获得明确批准",
                    Some(ToolApprovalPrompt {
                        title: "确认执行高风险命令".to_owned(),
                        description:
                            "批准仅绑定本次命令参数与当前工作区 revision；后续调用不会继承。"
                                .to_owned(),
                        risk,
                    }),
                ));
            }
        }

        if matches!(
            invocation.name.as_str(),
            "browser_interact" | "browser_navigate" | "web_fetch" | "web_search"
        ) {
            if !self.controlled_network_allowed {
                return Ok(decision.build(
                    ToolAuthorizationDisposition::Deny,
                    ApprovalRisk::Elevated,
                    Some("actor_controlled_network_denied".to_owned()),
                    "当前 actor 的冻结边界禁止 Host-controlled 网络访问",
                    None,
                ));
            }
            if invocation.name == "browser_interact" {
                let request =
                    parse_browser_interact_for_authorization(input).map_err(|message| {
                        ToolExecutionError::new("browser_interact_authorization_invalid", message)
                    })?;
                let preview = self
                    .semantic_browser_harness
                    .authorization_preview(&invocation.run_id.to_string(), &request);
                let local_fixture = preview.is_none() && self.browser_local_origin.is_some();
                if preview.is_none() && !local_fixture {
                    return Ok(decision.build(
                        ToolAuthorizationDisposition::Deny,
                        ApprovalRisk::Elevated,
                        Some("browser_session_scope_missing".to_owned()),
                        "browser_interact 没有同一 Run 的 live scoped browser session",
                        None,
                    ));
                }
                if local_fixture {
                    if matches!(
                        request,
                        BrowserInteractRequest::Submit(_)
                            | BrowserInteractRequest::Login { .. }
                            | BrowserInteractRequest::Upload { .. }
                            | BrowserInteractRequest::Download { .. }
                            | BrowserInteractRequest::PromoteDownload { .. }
                            | BrowserInteractRequest::SessionStatus
                            | BrowserInteractRequest::SessionClear
                    ) {
                        return Ok(decision.build(
                            ToolAuthorizationDisposition::Deny,
                            ApprovalRisk::Critical,
                            Some("browser_local_submit_not_admitted".to_owned()),
                            "exact-loopback semantic interaction 不冒充 managed public session 能力",
                            None,
                        ));
                    }
                    return Ok(decision.build(
                        ToolAuthorizationDisposition::Allow,
                        ApprovalRisk::Routine,
                        Some("exact_local_semantic_interaction".to_owned()),
                        "Host exact-loopback session 只消费同一 Run 当前 page epoch 的 typed opaque ref",
                        None,
                    ));
                }
                let preview = preview.expect("checked public browser authorization preview");
                let risk = if preview.external_side_effect {
                    ApprovalRisk::Elevated
                } else {
                    ApprovalRisk::Routine
                };
                let needs_approval = preview.external_side_effect
                    && mode != RunPermissionMode::FullAccess
                    || preview.public && mode == RunPermissionMode::Ask;
                let description = format!(
                    "exact origin: {}\nexact target: {}\nparameters: {}\nimpact: {}\n批准只绑定本次 invocation 与当前 workspace revision。",
                    preview.origin, preview.target, preview.parameters, preview.impact
                );
                return Ok(decision.build(
                    if needs_approval {
                        ToolAuthorizationDisposition::Ask
                    } else {
                        ToolAuthorizationDisposition::Allow
                    },
                    risk,
                    Some(
                        if preview.external_side_effect {
                            "scoped_public_reversible_side_effect"
                        } else {
                            "scoped_public_semantic_interaction"
                        }
                        .to_owned(),
                    ),
                    description.clone(),
                    needs_approval.then_some(ToolApprovalPrompt {
                        title: if preview.external_side_effect {
                            "确认可撤销的公开站点写操作"
                        } else {
                            "确认公开站点交互"
                        }
                        .to_owned(),
                        description,
                        risk,
                    }),
                ));
            }
            if invocation.name == "browser_navigate" {
                let exact_local = self.browser_local_origin.as_deref().is_some_and(|origin| {
                    invocation
                        .arguments
                        .parsed
                        .as_ref()
                        .and_then(|input| input.get("url"))
                        .and_then(Value::as_str)
                        .and_then(|url| reqwest::Url::parse(url).ok())
                        .is_some_and(|url| {
                            let mut value = format!(
                                "{}://{}",
                                url.scheme(),
                                url.host_str().unwrap_or_default()
                            );
                            if let Some(port) = url.port() {
                                value.push_str(&format!(":{port}"));
                            }
                            value == origin.trim_end_matches('/')
                                || url
                                    .as_str()
                                    .starts_with(&format!("{}/", origin.trim_end_matches('/')))
                        })
                });
                let requested_url = input.get("url").and_then(Value::as_str).unwrap_or_default();
                let needs_approval = !exact_local && mode == RunPermissionMode::Ask;
                let description = if exact_local {
                    "只读语义浏览由 Host exact-local-origin egress guard、GET/HEAD 与资源上限约束"
                        .to_owned()
                } else {
                    format!(
                        "exact target: {requested_url}\nimpact: 读取 public external_untrusted 页面；Host 强制 URL/DNS/connect/redirect、origin 与资源边界。"
                    )
                };
                return Ok(decision.build(
                    if needs_approval {
                        ToolAuthorizationDisposition::Ask
                    } else {
                        ToolAuthorizationDisposition::Allow
                    },
                    ApprovalRisk::Routine,
                    Some(
                        if exact_local {
                            "exact_local_semantic_browser"
                        } else {
                            "public_semantic_browser"
                        }
                        .to_owned(),
                    ),
                    description.clone(),
                    needs_approval.then_some(ToolApprovalPrompt {
                        title: "确认读取公开网页".to_owned(),
                        description,
                        risk: ApprovalRisk::Routine,
                    }),
                ));
            }
            if invocation.name == "web_search" {
                let query = input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let description = format!(
                    "query: {query}\nimpact: 向固定 Host-owned search endpoint 发送一次查询；结果只作 external_untrusted discovery，引用前必须读取并交叉核验原来源。"
                );
                let needs_approval = mode == RunPermissionMode::Ask;
                return Ok(decision.build(
                    if needs_approval {
                        ToolAuthorizationDisposition::Ask
                    } else {
                        ToolAuthorizationDisposition::Allow
                    },
                    ApprovalRisk::Routine,
                    Some("canonical_public_web_search".to_owned()),
                    description.clone(),
                    needs_approval.then_some(ToolApprovalPrompt {
                        title: "确认搜索公开 Web".to_owned(),
                        description,
                        risk: ApprovalRisk::Routine,
                    }),
                ));
            }
            let plaintext_http = invocation
                .arguments
                .parsed
                .as_ref()
                .and_then(|input| input.get("url"))
                .and_then(Value::as_str)
                .and_then(|url| reqwest::Url::parse(url).ok())
                .is_some_and(|url| url.scheme() == "http");
            let requested_url = input.get("url").and_then(Value::as_str).unwrap_or_default();
            let needs_approval = mode == RunPermissionMode::Ask;
            let reason = if plaintext_http {
                format!(
                    "exact target: {requested_url}\nimpact: 读取不受传输保护的 public HTTP external_untrusted 内容；Host 强制 URL/DNS/connect/redirect 边界。"
                )
            } else {
                format!(
                    "exact target: {requested_url}\nimpact: 读取 public HTTPS external_untrusted 内容；Host 强制 URL/DNS/connect/redirect 边界。"
                )
            };
            return Ok(decision.build(
                if needs_approval {
                    ToolAuthorizationDisposition::Ask
                } else {
                    ToolAuthorizationDisposition::Allow
                },
                ApprovalRisk::Routine,
                Some(
                    if plaintext_http {
                        "public_plaintext_http_web_fetch"
                    } else {
                        "public_https_web_fetch"
                    }
                    .to_owned(),
                ),
                reason.clone(),
                needs_approval.then_some(ToolApprovalPrompt {
                    title: "确认读取公开 URL".to_owned(),
                    description: reason,
                    risk: ApprovalRisk::Routine,
                }),
            ));
        }

        if allow_external_verifier_program {
            return Ok(decision.build(
                ToolAuthorizationDisposition::Allow,
                ApprovalRisk::Routine,
                Some("task_contract_verifier_exact".to_owned()),
                "Host 冻结的 TaskContract verifier 与 canonical 执行计划完全一致",
                None,
            ));
        }

        Ok(decision.build(
            ToolAuthorizationDisposition::Allow,
            ApprovalRisk::Routine,
            Some("permission_mode_allow".to_owned()),
            "frozen Run permission allows this invocation",
            None,
        ))
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        // AgentRuntime owns the durable preflight boundary. Re-running it
        // here could produce Rejected/NotStarted after Runtime has already
        // committed ToolExecutionStarted. Keep this boundary defensive, but
        // represent callers that skipped preflight as an observed operation
        // failure rather than corrupting the lifecycle.
        let workspace_access = self.workspace_access(&invocation);
        let host_application_probe = invocation.name == APPLICATION_PROBE_VERIFIER_ID
            && invocation.call_id.starts_with("host:");
        if !PRODUCTION_TOOL_NAMES.contains(&invocation.name.as_str()) && !host_application_probe {
            return Ok(Self::execution_error_outcome(
                ToolError::not_available(format!("工具 '{}' 不在固定生产目录中", invocation.name)),
                workspace_access,
            ));
        }
        let Some(input) = invocation.arguments.parsed else {
            return Ok(Self::execution_error_outcome(
                ToolError::invalid_input("JSON 参数格式错误"),
                workspace_access,
            ));
        };
        if cancellation.is_cancelled() {
            let mut outcome =
                ToolOutcome::error(format!("工具 '{}' 在执行前已取消", invocation.name));
            outcome.operation = ToolOperationStatus::Cancelled;
            outcome.side_effect = ToolSideEffectStatus::NotApplied;
            outcome.retry = ToolRetryDisposition::Safe;
            return Ok(outcome);
        }

        let tool_cancellation = TokioCancellationToken::new();
        let context = self.context.for_invocation(tool_cancellation.clone());
        let run_id = invocation.run_id.to_string();
        let execution = self.dispatch(&run_id, &invocation.name, input, &context);
        tokio::pin!(execution);
        tokio::select! {
            result = &mut execution => Ok(match result {
                Ok(outcome) => outcome.with_workspace_access_guarantee(workspace_access),
                Err(error) => Self::execution_error_outcome(error, workspace_access),
            }),
            () = cancellation.cancelled() => {
                tool_cancellation.cancel();
                match tokio::time::timeout(std::time::Duration::from_secs(5), &mut execution).await {
                    Ok(Ok(mut outcome)) => {
                        if !outcome.is_success() {
                            outcome.operation = ToolOperationStatus::Cancelled;
                        }
                        Ok(outcome.with_workspace_access_guarantee(workspace_access))
                    },
                    Ok(Err(error)) => Ok(Self::execution_error_outcome(error, workspace_access)),
                    Err(_) => {
                        let mut outcome = ToolOutcome::recovery_ambiguous(format!(
                            "工具 '{}' 收到取消后未在 5 秒内停止",
                            invocation.name
                        ));
                        outcome.operation = ToolOperationStatus::Cancelled;
                        Ok(outcome.with_workspace_access_guarantee(workspace_access))
                    }
                }
            }
        }
    }
}

/// Return the canonical catalog in prefix-stable alphabetical order.
#[must_use]
pub fn production_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        definition(
            "apply_patch",
            "用 unified diff 或 changes 完整内容修改工作区文件；每个文件原子发布，跨文件普通失败会回滚但崩溃窗口不是事务。patch 与 changes 二选一，path/fuzz/create_if_missing 仅适用于 patch。",
            apply_patch_schema(),
        ),
        definition(
            "browser_interact",
            "在 Host-owned semantic browser 中执行一个枚举化 action：click/fill/press/wait/scroll/select/back/tab、可撤销 submit，以及 managed login/upload/download/promotion/session_status/session_clear。element/page ref 只能来自最新 observation；credential_ref 只引用 Host secret grant，秘密/Cookie/storage 不进入模型。public managed action 使用项目隔离 Profile、exact origin/target、scoped approval、quarantine/static scan、显式 no-overwrite promotion 与 durable receipt；不接受 selector、坐标、脚本、任意 header 或个人 Chrome Profile。",
            browser_interact_schema(),
        ),
        definition(
            "browser_navigate",
            "读取一个已知 public HTTP(S) URL 或 Host-owned exact-loopback application 的 JavaScript 渲染结果，按可选 focus 做确定性 task-cue/role/interaction 优先级裁剪，并返回有界 DOM/accessibility snapshot、观察质量、action 后 semantic diff、page state 与 external_untrusted 信任标记；observation 可包含只供同一 Run 最新 page epoch 使用的 capability-bound opaque refs。public navigation 使用项目隔离 DSE managed Profile 以受控复用 Cookie/storage；exact-loopback 仍使用 teardown 即删除的临时 Profile。",
            browser_navigate_schema(),
        ),
        definition(
            "edit_file",
            "在已用 read_file 读取的单个文件中执行一次精确搜索替换；适合范围明确的局部修改，匹配失败时会自动进行有限回退。",
            edit_file_schema(),
        ),
        definition(
            "exec_shell",
            "在工作区中同步执行一条有界 shell 命令并返回退出状态与输出；优先使用更专用的文件、搜索、Git 或验证工具。",
            exec_shell_schema(),
        ),
        definition(
            "file_search",
            "按文件名或路径片段模糊查找工作区文件，可按扩展名和排除模式过滤并限制结果数量。",
            file_search_schema(),
        ),
        definition(
            "git_diff",
            "读取工作区未提交或已暂存的 Git 差异，可限定路径和上下文行数。",
            git_diff_schema(),
        ),
        definition(
            "git_status",
            "读取工作区的 Git 分支与文件状态，可限定到工作区内的路径。",
            git_status_schema(),
        ),
        definition(
            "grep_files",
            "用正则表达式搜索工作区文本，可限定路径、包含或排除模式，并返回匹配行及上下文。",
            grep_files_schema(),
        ),
        definition(
            "list_dir",
            "列出工作区内指定目录的直接子项；path 省略时读取工作区根目录。",
            list_dir_schema(),
        ),
        definition(
            "load_skill",
            "按系统提示中列出的精确名称读取本次运行已冻结的完整 Skill 定义；拒绝别名、路径、未发现、不可读、过大或歧义的 Skill，返回来源哈希与 external_untrusted 信任标记。",
            load_skill_schema(),
        ),
        definition(
            "read_file",
            "读取工作区内的 UTF-8 文本、PDF 或可 OCR 图像；大文件用 start_line/max_lines 分段，PDF 用 pages 指定页码。",
            read_file_schema(),
        ),
        definition(
            "run_tests",
            "在工作区根目录运行 cargo test，可传入额外参数或启用全部 feature，并返回确定性测试结果。",
            run_tests_schema(),
        ),
        definition(
            "run_verifiers",
            "同步运行与项目类型匹配的确定性验证，可选择快速或完整等级，也可提供明确的程序与参数。",
            run_verifiers_schema(),
        ),
        definition(
            "web_fetch",
            "读取一个已知公开 HTTP(S) URL，返回有界正文、标题、canonical links、received-content 来源哈希、传输轨迹与 external_untrusted 信任标记；HTTP 是不受保护的明文传输。不提供搜索、认证、Cookie、任意 header、脚本或浏览器执行。",
            web_fetch_schema(),
        ),
        definition(
            "web_search",
            "从一个固定 Host-owned Web Search endpoint 发现公开来源，返回有界 rank/title/URL/snippet、provider request/usage identity 与 external_untrusted 标记。snippet 只作 discovery；形成结论或引用前必须用 web_fetch 或 browser_navigate 读取原来源并交叉核验。不接受 provider、header、credential、proxy 或 browser 参数。",
            web_search_schema(),
        ),
    ]
}

fn definition(name: &str, description: &str, input_schema: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    }
}

fn validate_production_input(name: &str, input: &Value) -> Result<(), ToolError> {
    let definition = production_tool_definitions()
        .into_iter()
        .find(|definition| definition.name == name)
        .ok_or_else(|| ToolError::not_available(format!("未知生产工具 '{name}'")))?;
    validate_schema_value(input, &definition.input_schema, "$")
}

fn validate_schema_value(value: &Value, schema: &Value, path: &str) -> Result<(), ToolError> {
    const SUPPORTED_SCHEMA_KEYS: [&str; 13] = [
        "type",
        "properties",
        "required",
        "additionalProperties",
        "oneOf",
        "enum",
        "minimum",
        "maximum",
        "minLength",
        "minItems",
        "items",
        "default",
        "description",
    ];
    if let Some(key) = schema.as_object().and_then(|schema| {
        schema
            .keys()
            .find(|key| !SUPPORTED_SCHEMA_KEYS.contains(&key.as_str()))
    }) {
        return Err(ToolError::schema_validation(format!(
            "schema 在 {path} 使用未实现的约束 '{key}'"
        )));
    }
    let schema_type = schema.get("type").and_then(Value::as_str);
    let type_matches = match schema_type {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("boolean") => value.is_boolean(),
        Some(other) => {
            return Err(ToolError::schema_validation(format!(
                "schema 在 {path} 使用不支持的类型 '{other}'"
            )));
        }
        None => true,
    };
    if !type_matches {
        return Err(ToolError::schema_validation(format!(
            "字段 {path} 类型错误，期望 {}",
            schema_type.unwrap_or("有效 JSON")
        )));
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array)
        && !values.contains(value)
    {
        return Err(ToolError::schema_validation(format!(
            "字段 {path} 不在允许的枚举值中"
        )));
    }

    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
            && number < minimum
        {
            return Err(ToolError::schema_validation(format!(
                "字段 {path} 小于允许的最小值 {minimum}"
            )));
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
            && number > maximum
        {
            return Err(ToolError::schema_validation(format!(
                "字段 {path} 大于允许的最大值 {maximum}"
            )));
        }
    }

    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
            && length < minimum
        {
            return Err(ToolError::schema_validation(format!(
                "字段 {path} 长度小于允许的最小值 {minimum}"
            )));
        }
    }

    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(ToolError::missing_field(format!("{path}/{field}")));
                }
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            let mut unknown = object
                .keys()
                .filter(|field| {
                    properties.is_none_or(|properties| !properties.contains_key(*field))
                })
                .cloned()
                .collect::<Vec<_>>();
            unknown.sort();
            if !unknown.is_empty() {
                return Err(ToolError::invalid_input(format!(
                    "对象 {path} 不接受字段：{}",
                    unknown.join(", ")
                )));
            }
        }
        for (field, field_value) in object {
            if let Some(field_schema) = properties.and_then(|properties| properties.get(field)) {
                validate_schema_value(field_value, field_schema, &format!("{path}/{field}"))?;
            }
        }
        if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
            if branches.iter().any(|branch| {
                branch
                    .as_object()
                    .is_none_or(|branch| branch.keys().any(|key| key != "required"))
            }) {
                return Err(ToolError::schema_validation(format!(
                    "schema 在 {path} 使用了未实现的 oneOf 分支"
                )));
            }
            let matches = branches
                .iter()
                .filter(|branch| {
                    branch
                        .get("required")
                        .and_then(Value::as_array)
                        .is_some_and(|required| {
                            required
                                .iter()
                                .filter_map(Value::as_str)
                                .all(|field| object.contains_key(field))
                        })
                })
                .count();
            if matches != 1 {
                return Err(ToolError::schema_validation(format!(
                    "对象 {path} 必须且只能满足一个 oneOf 参数分支"
                )));
            }
        }
    }
    if let Some(items) = value.as_array() {
        let length = items.len() as u64;
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64)
            && length < minimum
        {
            return Err(ToolError::schema_validation(format!(
                "数组 {path} 少于允许的最小元素数 {minimum}"
            )));
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in items.iter().enumerate() {
                validate_schema_value(item, item_schema, &format!("{path}/{index}"))?;
            }
        }
    }
    Ok(())
}

fn apply_patch_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string","minLength":1},"patch":{"type":"string","minLength":1},"changes":{"type":"array","minItems":1,"items":{"type":"object","properties":{"path":{"type":"string","minLength":1},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}},"fuzz":{"type":"integer","minimum":0,"maximum":50,"default":3},"create_if_missing":{"type":"boolean"}},"oneOf":[{"required":["patch"]},{"required":["changes"]}],"additionalProperties":false})
}

fn browser_navigate_schema() -> Value {
    json!({"type":"object","properties":{"url":{"type":"string","minLength":1},"max_nodes":{"type":"integer","minimum":1,"maximum":256,"default":128},"max_chars":{"type":"integer","minimum":1,"maximum":50000,"default":20000},"focus":{"type":"string","minLength":1}},"required":["url"],"additionalProperties":false})
}

fn browser_interact_schema() -> Value {
    json!({"type":"object","properties":{"action":{"type":"string","enum":["click","fill","press","wait","scroll","select","back","tab_open","tab_switch","tab_close","submit","login","upload","download","promote_download","session_status","session_clear"]},"element_ref":{"type":"string","minLength":1},"value":{"type":"string"},"key":{"type":"string","enum":["enter","escape","tab","arrow_up","arrow_down","space"]},"condition":{"type":"string","enum":["document_ready","text_present","text_absent","url_equals"]},"timeout_ms":{"type":"integer","minimum":1,"maximum":10000},"direction":{"type":"string","enum":["up","down"]},"amount":{"type":"integer","minimum":1,"maximum":2000},"url":{"type":"string","minLength":1},"page_ref":{"type":"string","minLength":1},"credential_ref":{"type":"string","minLength":1},"workspace_path":{"type":"string","minLength":1},"download_ref":{"type":"string","minLength":37}},"required":["action"],"additionalProperties":false})
}

fn edit_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string","minLength":1},"search":{"type":"string","minLength":1},"replace":{"type":"string"}},"required":["path","search","replace"],"additionalProperties":false})
}

fn exec_shell_schema() -> Value {
    json!({"type":"object","properties":{"command":{"type":"string"},"timeout_ms":{"type":"integer"},"cwd":{"type":"string"}},"required":["command"],"additionalProperties":false})
}

fn file_search_schema() -> Value {
    json!({"type":"object","properties":{"query":{"type":"string","minLength":1},"path":{"type":"string"},"limit":{"type":"integer"},"extensions":{"type":"array","items":{"type":"string"}},"exclude":{"type":"array","items":{"type":"string"}}},"required":["query"],"additionalProperties":false})
}

fn git_diff_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"cached":{"type":"boolean"},"unified":{"type":"integer","minimum":0,"maximum":50,"default":3}},"additionalProperties":false})
}

fn git_status_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false})
}

fn grep_files_schema() -> Value {
    json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"include":{"type":"array","items":{"type":"string"}},"exclude":{"type":"array","items":{"type":"string"}},"context_lines":{"type":"integer"},"case_insensitive":{"type":"boolean"},"max_results":{"type":"integer","minimum":1,"maximum":100}},"required":["pattern"],"additionalProperties":false})
}

fn list_dir_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false})
}

fn load_skill_schema() -> Value {
    json!({"type":"object","properties":{"name":{"type":"string","minLength":1}},"required":["name"],"additionalProperties":false})
}

fn read_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1},"pages":{"type":"string"}},"required":["path"],"additionalProperties":false})
}

fn run_tests_schema() -> Value {
    json!({"type":"object","properties":{"args":{"type":"array","items":{"type":"string"},"default":[]},"all_features":{"type":"boolean"}},"additionalProperties":false})
}

fn run_verifiers_schema() -> Value {
    json!({"type":"object","properties":{"profile":{"type":"string","enum":["auto","rust","node","python","go","exact"],"default":"auto"},"level":{"type":"string","enum":["quick","full"],"default":"quick"},"max_python_files":{"type":"integer","minimum":1,"maximum":1000,"default":200},"commands":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"},"default":[]},"cwd":{"type":"string"}},"required":["name","program"],"additionalProperties":false},"default":[]}},"additionalProperties":false})
}

fn web_fetch_schema() -> Value {
    json!({"type":"object","properties":{"url":{"type":"string","minLength":1},"max_chars":{"type":"integer","minimum":1,"maximum":50000,"default":20000}},"required":["url"],"additionalProperties":false})
}

fn web_search_schema() -> Value {
    json!({"type":"object","properties":{"query":{"type":"string","minLength":1},"max_results":{"type":"integer","minimum":1,"maximum":10,"default":5}},"required":["query"],"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use dse_protocol::agent_runtime::{
        RunId, ToolArguments, ToolInvocationStatus, ToolTransportStatus,
    };
    use dse_protocol::task::WorkspaceRevision;

    fn invocation(name: &str, value: Value) -> ToolInvocation {
        ToolInvocation {
            run_id: RunId::from("run-1"),
            call_id: "call-1".to_string(),
            name: name.to_string(),
            arguments: ToolArguments::from_value(value),
        }
    }

    fn workspace_state() -> WorkspaceState {
        WorkspaceState {
            generation: 1,
            revision: WorkspaceRevision::Known {
                sha256: "fixture-revision".to_owned(),
            },
        }
    }

    #[test]
    fn catalog_is_exact_chinese_and_description_free() {
        fn has_description_key(value: &Value) -> bool {
            match value {
                Value::Array(items) => items.iter().any(has_description_key),
                Value::Object(object) => {
                    object.contains_key("description") || object.values().any(has_description_key)
                }
                _ => false,
            }
        }
        let definitions = production_tool_definitions();
        let names: Vec<_> = definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(names, PRODUCTION_TOOL_NAMES);
        assert!(definitions.iter().all(|definition| {
            definition
                .description
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
        }));
        let serialized = serde_json::to_string(&definitions).unwrap();
        assert!(
            definitions
                .iter()
                .all(|definition| !has_description_key(&definition.input_schema))
        );
        for stale in [
            "backward compatibility",
            "write_file",
            "task_shell_start",
            "exec_shell_wait",
            "task_shell_wait",
            "task/status",
            "Find files by name",
            "Execute a shell command",
            "Optional extra arguments",
        ] {
            assert!(
                !serialized.contains(stale),
                "catalog leaked stale surface: {stale}"
            );
        }
    }

    #[test]
    fn execution_identity_is_sorted_stable_and_contains_no_live_state() {
        let workspace = tempfile::tempdir().unwrap();
        let first = ProductionToolConfig::new(workspace.path())
            .with_permission_mode(RunPermissionMode::Agent)
            .with_follow_symlinks(true)
            .with_shell_policy(ShellPolicy::ReadOnly)
            .with_prefer_external_pdftotext(true)
            .execution_identity();
        let second = ProductionToolConfig::new(workspace.path())
            .with_permission_mode(RunPermissionMode::Agent)
            .with_follow_symlinks(true)
            .with_shell_policy(ShellPolicy::ReadOnly)
            .with_prefer_external_pdftotext(true)
            .execution_identity();
        assert_eq!(first, second);
        let serialized = serde_json::to_string(&first).unwrap();
        for forbidden in ["cancel", "shell_manager", "read_tracker", "api_key"] {
            assert!(!serialized.contains(forbidden));
        }
        let grant = BrowserCredentialGrant::new(
            "engineering-app",
            "https://example.com/login",
            "https://example.com/session",
            [("password", "super-secret-store-key")],
        )
        .unwrap();
        let with_grant = ProductionToolConfig::new(workspace.path())
            .with_browser_state_root(workspace.path().join("browser-state"))
            .with_browser_credential_grant(grant)
            .execution_identity();
        let serialized = serde_json::to_string(&with_grant).unwrap();
        assert_eq!(with_grant.schema, 7);
        assert!(!serialized.contains("super-secret-store-key"));
        assert!(!serialized.contains("password"));
        assert_ne!(
            with_grant.browser_credential_grants_sha256,
            first.browser_credential_grants_sha256
        );
    }

    #[tokio::test]
    async fn load_skill_is_exact_snapshot_backed_bounded_and_typed() {
        let workspace = tempfile::tempdir().unwrap();
        let skill_dir = workspace.path().join("skills/exact-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let source = skill_dir.join("SKILL.md");
        std::fs::write(
            &source,
            "---\nname: exact-skill\ndescription: Load exactly\n---\n\n# Exact\n\nDo it safely.\n",
        )
        .unwrap();
        let registry = Arc::new(SkillRegistry::discover(&workspace.path().join("skills")));
        let snapshot_hash = registry.snapshot_sha256();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path()).with_skill_registry(Arc::clone(&registry)),
        );

        let exact = invocation("load_skill", json!({"name":"exact-skill"}));
        assert!(executor.preflight(&exact).is_none());
        assert_eq!(
            executor.definition_workspace_access("load_skill"),
            WorkspaceAccess::ReadOnly
        );
        let authorization = executor
            .authorize(
                RunPermissionMode::Ask,
                &ToolExecutionGrant::Ordinary,
                &exact,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            authorization.disposition,
            ToolAuthorizationDisposition::Allow
        );
        assert_eq!(authorization.risk, ApprovalRisk::Routine);
        for invalid in [
            invocation("load_skill", json!({"name":"Exact Skill"})),
            invocation("load_skill", json!({"name":"missing"})),
            invocation(
                "load_skill",
                json!({"name":"exact-skill","path":"/etc/passwd"}),
            ),
        ] {
            let outcome = executor
                .preflight(&invalid)
                .expect("invalid load must fail before ToolExecutionStarted");
            assert_eq!(outcome.invocation, ToolInvocationStatus::Rejected);
            assert_eq!(outcome.operation, ToolOperationStatus::NotStarted);
            assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
            assert!(matches!(
                outcome.failure_code,
                Some(ToolFailureCode::InvalidField)
            ));
        }

        std::fs::remove_file(&source).unwrap();
        let outcome = executor
            .execute(exact, CancellationToken::default())
            .await
            .unwrap();
        assert!(outcome.is_success(), "{}", outcome.content);
        let loaded: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(loaded["name"], "exact-skill");
        assert_eq!(loaded["body"], "# Exact\n\nDo it safely.");
        assert_eq!(loaded["trust"], "external_untrusted");
        assert_eq!(loaded["truncated"], false);
        assert!(
            loaded["source_sha256"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(
            loaded["bytes_returned"],
            loaded["body"].as_str().unwrap().len()
        );
        assert_eq!(registry.snapshot_sha256(), snapshot_hash);
    }

    #[test]
    fn execution_identity_changes_with_skill_snapshot() {
        let workspace = tempfile::tempdir().unwrap();
        let skill_dir = workspace.path().join("skills/identity");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let source = skill_dir.join("SKILL.md");
        std::fs::write(
            &source,
            "---\nname: identity\ndescription: first\n---\nfirst\n",
        )
        .unwrap();
        let first = Arc::new(SkillRegistry::discover(&workspace.path().join("skills")));
        let first_identity = ProductionToolConfig::new(workspace.path())
            .with_skill_registry(first)
            .execution_identity();

        std::fs::write(
            source,
            "---\nname: identity\ndescription: second\n---\nsecond\n",
        )
        .unwrap();
        let second = Arc::new(SkillRegistry::discover(&workspace.path().join("skills")));
        let second_identity = ProductionToolConfig::new(workspace.path())
            .with_skill_registry(second)
            .execution_identity();
        assert_eq!(first_identity.schema, 7);
        assert_ne!(
            first_identity.skills_snapshot_sha256,
            second_identity.skills_snapshot_sha256
        );
    }

    #[test]
    fn production_resolver_replaces_untrusted_verifier_plan_fields() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(workspace.path()));
        let spec = executor
            .resolve_verifier_spec(
                "run_verifiers",
                json!({
                    "profile": "exact",
                    "commands": [{
                        "name": "exact",
                        "program": "/usr/bin/python3",
                        "args": ["-I", "-B", "verify.py", "."],
                        "cwd": ""
                    }]
                }),
            )
            .unwrap();
        assert_eq!(
            spec.plan.steps[0].env.get("PYTHONDONTWRITEBYTECODE"),
            Some(&"1".to_owned())
        );
        assert!(
            executor
                .resolve_verifier_spec("read_file", json!({"path": "src/lib.rs"}))
                .is_err()
        );
        assert!(
            executor
                .resolve_verifier_spec("run_verifiers", json!({"profile": "auto"}))
                .is_err()
        );
    }

    #[test]
    fn production_authorization_matrix_is_host_owned_and_mode_bound() {
        let workspace = tempfile::tempdir().unwrap();
        let authorize = |mode, name, input| {
            let invocation = invocation(name, input);
            ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path())
                    .with_permission_mode(mode)
                    .with_shell_policy(ShellPolicy::Full),
            )
            .authorize(
                mode,
                &ToolExecutionGrant::Ordinary,
                &invocation,
                &workspace_state(),
            )
            .unwrap()
        };

        for mode in [
            RunPermissionMode::Ask,
            RunPermissionMode::Agent,
            RunPermissionMode::FullAccess,
        ] {
            assert_eq!(
                authorize(mode, "apply_patch", json!({"patch": "x"})).disposition,
                ToolAuthorizationDisposition::Allow
            );
            assert_eq!(
                authorize(mode, "run_tests", json!({})).disposition,
                ToolAuthorizationDisposition::Allow
            );
            assert_eq!(
                authorize(mode, "exec_shell", json!({"command": "rm -rf $HOME"})).disposition,
                ToolAuthorizationDisposition::Deny
            );
        }
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "read_file",
                json!({"path": "/tmp/external"})
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "apply_patch",
                json!({
                    "patch": "--- /tmp/external\n+++ /tmp/external\n@@ -1,1 +1,1 @@\n-old\n+new\n"
                })
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "exec_shell",
                json!({"command": "pwd", "cwd": "/tmp"})
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "run_verifiers",
                json!({
                    "commands": [{
                        "name": "external",
                        "program": "/tmp/external-verifier",
                        "args": []
                    }]
                })
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "exec_shell",
                json!({"command": "curl https://example.invalid"})
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Ask,
                "exec_shell",
                json!({"command": "git push origin main"})
            )
            .disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Agent,
                "exec_shell",
                json!({"command": "git push origin main"})
            )
            .disposition,
            ToolAuthorizationDisposition::Ask
        );
        assert_eq!(
            authorize(
                RunPermissionMode::Agent,
                "exec_shell",
                json!({"command": "curl https://example.invalid"})
            )
            .disposition,
            ToolAuthorizationDisposition::Allow
        );
        assert_eq!(
            authorize(
                RunPermissionMode::FullAccess,
                "exec_shell",
                json!({"command": "git push origin main"})
            )
            .disposition,
            ToolAuthorizationDisposition::Allow
        );

        let high_risk_allow = ExecPolicy::parse_toml(
            r#"
[rules.release]
allow = ["git push"]
"#,
        )
        .unwrap();
        for (mode, expected, expected_rule) in [
            (
                RunPermissionMode::Ask,
                ToolAuthorizationDisposition::Deny,
                "ask_network_fail_closed",
            ),
            (
                RunPermissionMode::Agent,
                ToolAuthorizationDisposition::Ask,
                "host_high_risk",
            ),
        ] {
            let invocation = invocation("exec_shell", json!({"command": "git push origin main"}));
            let decision = ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path())
                    .with_permission_mode(mode)
                    .with_shell_policy(ShellPolicy::Full)
                    .with_exec_policy(Some(high_risk_allow.clone())),
            )
            .authorize(
                mode,
                &ToolExecutionGrant::Ordinary,
                &invocation,
                &workspace_state(),
            )
            .unwrap();
            assert_eq!(
                decision.disposition, expected,
                "execpolicy allow must not override Host critical classification"
            );
            assert_eq!(decision.matched_rule.as_deref(), Some(expected_rule));
        }
    }

    #[test]
    fn web_fetch_schema_catalog_and_authorization_follow_existing_actor_policy() {
        let workspace = tempfile::tempdir().unwrap();
        let valid_https = invocation(
            "web_fetch",
            json!({"url":"https://example.com/docs","max_chars":4096}),
        );
        let valid_http = invocation(
            "web_fetch",
            json!({"url":"http://example.com/docs","max_chars":4096}),
        );

        let ask = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Ask),
        );
        assert!(ask.preflight(&valid_https).is_none());
        assert!(ask.preflight(&valid_http).is_none());
        let definition = ask
            .definitions()
            .into_iter()
            .find(|definition| definition.name == "web_fetch")
            .unwrap();
        assert!(definition.description.contains("HTTP(S)"));
        assert!(definition.description.contains("明文传输"));
        assert!(definition.description.contains("external_untrusted"));
        assert_eq!(
            ask.definition_workspace_access("web_fetch"),
            WorkspaceAccess::ReadOnly
        );
        let requested = ask
            .authorize(
                RunPermissionMode::Ask,
                &ToolExecutionGrant::Ordinary,
                &valid_http,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(requested.disposition, ToolAuthorizationDisposition::Ask);
        assert_eq!(
            requested.matched_rule.as_deref(),
            Some("public_plaintext_http_web_fetch")
        );
        assert!(
            requested
                .prompt
                .unwrap()
                .description
                .contains("exact target")
        );

        for mode in [RunPermissionMode::Agent, RunPermissionMode::FullAccess] {
            let executor = ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path()).with_permission_mode(mode),
            );
            for (invocation, expected_rule) in [
                (&valid_https, "public_https_web_fetch"),
                (&valid_http, "public_plaintext_http_web_fetch"),
            ] {
                let allowed = executor
                    .authorize(
                        mode,
                        &ToolExecutionGrant::Ordinary,
                        invocation,
                        &workspace_state(),
                    )
                    .unwrap();
                assert_eq!(allowed.disposition, ToolAuthorizationDisposition::Allow);
                assert_eq!(allowed.matched_rule.as_deref(), Some(expected_rule));
            }
        }

        let writer = tempfile::tempdir().unwrap();
        let isolated = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .rebind_isolated_writer_workspace(writer.path()),
        );
        let denied = isolated
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &valid_http,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(denied.disposition, ToolAuthorizationDisposition::Deny);
        assert_eq!(
            denied.matched_rule.as_deref(),
            Some("actor_controlled_network_denied")
        );

        for field in [
            "headers",
            "cookie",
            "authorization",
            "proxy",
            "method",
            "path",
        ] {
            let mut input = json!({"url":"http://example.com/"});
            input
                .as_object_mut()
                .unwrap()
                .insert(field.to_owned(), Value::String("forbidden".to_owned()));
            let rejected = ask
                .preflight(&invocation("web_fetch", input))
                .expect("unsupported web authority must fail schema preflight");
            assert_eq!(rejected.failure_code, Some(ToolFailureCode::InvalidField));
        }
        for max_chars in [0, 50_001] {
            assert!(
                ask.preflight(&invocation(
                    "web_fetch",
                    json!({"url":"https://example.com/", "max_chars": max_chars}),
                ))
                .is_some()
            );
        }
        let unsafe_url = ask
            .preflight(&invocation(
                "web_fetch",
                json!({"url":"http://169.254.169.254/latest"}),
            ))
            .expect("public HTTP metadata URL must fail before authorization");
        assert_eq!(
            unsafe_url.failure_code,
            Some(ToolFailureCode::InvocationRejected)
        );
        assert_eq!(
            unsafe_url.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_private_address_denied"
        );
        let unsafe_port = ask
            .preflight(&invocation(
                "web_fetch",
                json!({"url":"http://example.com:8080/"}),
            ))
            .expect("non-default public HTTP port must fail before authorization");
        assert_eq!(
            unsafe_port.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_http_port_denied"
        );
    }

    #[test]
    fn web_search_is_one_read_only_surface_with_exact_authorization_and_no_provider_controls() {
        let workspace = tempfile::tempdir().unwrap();
        let search_invocation = invocation(
            "web_search",
            json!({"query":"Rust deterministic replay", "max_results":5}),
        );
        let ask = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Ask),
        );
        assert!(ask.preflight(&search_invocation).is_none());
        let definition = ask
            .definitions()
            .into_iter()
            .find(|definition| definition.name == "web_search")
            .unwrap();
        assert!(definition.description.contains("discovery"));
        assert!(definition.description.contains("web_fetch"));
        assert_eq!(
            ask.definition_workspace_access("web_search"),
            WorkspaceAccess::ReadOnly
        );
        let requested = ask
            .authorize(
                RunPermissionMode::Ask,
                &ToolExecutionGrant::Ordinary,
                &search_invocation,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(requested.disposition, ToolAuthorizationDisposition::Ask);
        assert_eq!(
            requested.matched_rule.as_deref(),
            Some("canonical_public_web_search")
        );
        assert!(requested.prompt.unwrap().description.contains("交叉核验"));

        for mode in [RunPermissionMode::Agent, RunPermissionMode::FullAccess] {
            let executor = ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path()).with_permission_mode(mode),
            );
            let allowed = executor
                .authorize(
                    mode,
                    &ToolExecutionGrant::Ordinary,
                    &search_invocation,
                    &workspace_state(),
                )
                .unwrap();
            assert_eq!(allowed.disposition, ToolAuthorizationDisposition::Allow);
        }

        let writer = tempfile::tempdir().unwrap();
        let isolated = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .rebind_isolated_writer_workspace(writer.path()),
        );
        let denied = isolated
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &search_invocation,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(denied.disposition, ToolAuthorizationDisposition::Deny);
        assert_eq!(
            denied.matched_rule.as_deref(),
            Some("actor_controlled_network_denied")
        );

        for field in ["provider", "api_key", "header", "proxy", "url", "answer"] {
            let mut input = json!({"query":"Rust deterministic replay"});
            input
                .as_object_mut()
                .unwrap()
                .insert(field.to_owned(), Value::String("forbidden".to_owned()));
            let rejected = ask
                .preflight(&invocation("web_search", input))
                .expect("provider controls must fail schema preflight");
            assert_eq!(rejected.failure_code, Some(ToolFailureCode::InvalidField));
        }
        for max_results in [0, 11] {
            assert!(
                ask.preflight(&invocation(
                    "web_search",
                    json!({"query":"Rust deterministic replay", "max_results":max_results}),
                ))
                .is_some()
            );
        }
    }

    #[derive(Debug)]
    struct FixtureSemanticBrowser {
        navigate_calls: std::sync::atomic::AtomicUsize,
        click_calls: std::sync::atomic::AtomicUsize,
        fill_calls: std::sync::atomic::AtomicUsize,
        shutdown_calls: std::sync::atomic::AtomicUsize,
        public_preview: bool,
    }

    #[async_trait]
    impl SemanticBrowserHarness for FixtureSemanticBrowser {
        fn identity(&self) -> String {
            "fixture_semantic_browser_v1".to_owned()
        }

        async fn navigate(
            &self,
            _run_id: &str,
            _request: crate::BrowserNavigateRequest,
            _cancellation: TokioCancellationToken,
        ) -> ToolOutcome {
            self.navigate_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ToolOutcome::json(&json!({
                "requested_url":"https://example.com/app",
                "final_url":"https://example.com/app",
                "title":"Ready",
                "snapshot":[{
                    "role":"status",
                    "accessible_name":"Deployment ready",
                    "text":"",
                    "state":{"data-state":"ready"}
                }],
                "trust":"external_untrusted",
                "truncated":false
            }))
            .expect("fixture browser outcome")
        }

        async fn click(
            &self,
            _run_id: &str,
            _request: crate::BrowserClickRequest,
            _cancellation: TokioCancellationToken,
        ) -> ToolOutcome {
            self.click_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ToolOutcome::json(&json!({"action":{"kind":"click"},"trust":"external_untrusted"}))
                .expect("fixture browser click outcome")
                .with_side_effect(ToolSideEffectStatus::Applied)
        }

        async fn fill(
            &self,
            _run_id: &str,
            request: crate::BrowserFillRequest,
            _cancellation: TokioCancellationToken,
        ) -> ToolOutcome {
            assert_eq!(request.value(), "canary");
            self.fill_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ToolOutcome::json(&json!({"action":{"kind":"fill"},"trust":"external_untrusted"}))
                .expect("fixture browser fill outcome")
                .with_side_effect(ToolSideEffectStatus::Applied)
        }

        fn authorization_preview(
            &self,
            _run_id: &str,
            request: &BrowserInteractRequest,
        ) -> Option<crate::semantic_browser::BrowserAuthorizationPreview> {
            self.public_preview.then(|| {
                let external_side_effect = matches!(
                    request,
                    BrowserInteractRequest::Submit(_)
                        | BrowserInteractRequest::Login { .. }
                        | BrowserInteractRequest::Upload { .. }
                        | BrowserInteractRequest::SessionClear
                );
                crate::semantic_browser::BrowserAuthorizationPreview {
                    public: true,
                    origin: "https://example.com".to_owned(),
                    target: "https://example.com/drafts/save".to_owned(),
                    parameters: "sha256:fixture-parameters".to_owned(),
                    impact: if external_side_effect {
                        "reversible_draft_write"
                    } else {
                        "ephemeral_page_state_only"
                    }
                    .to_owned(),
                    external_side_effect,
                }
            })
        }

        fn shutdown(&self) {
            self.shutdown_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn browser_navigate_schema_authorization_and_dispatch_are_host_owned() {
        let workspace = tempfile::tempdir().unwrap();
        let public = invocation(
            "browser_navigate",
            json!({"url":"https://example.com/app","max_nodes":1,"max_chars":4096}),
        );
        let ask = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Ask),
        );
        assert!(ask.preflight(&public).is_none());
        assert_eq!(
            ask.definition_workspace_access("browser_navigate"),
            WorkspaceAccess::ReadOnly
        );
        let requested = ask
            .authorize(
                RunPermissionMode::Ask,
                &ToolExecutionGrant::Ordinary,
                &public,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(requested.disposition, ToolAuthorizationDisposition::Ask);
        assert_eq!(
            requested.matched_rule.as_deref(),
            Some("public_semantic_browser")
        );
        assert!(
            requested
                .prompt
                .unwrap()
                .description
                .contains("exact target")
        );

        let harness = Arc::new(FixtureSemanticBrowser {
            navigate_calls: std::sync::atomic::AtomicUsize::new(0),
            click_calls: std::sync::atomic::AtomicUsize::new(0),
            fill_calls: std::sync::atomic::AtomicUsize::new(0),
            shutdown_calls: std::sync::atomic::AtomicUsize::new(0),
            public_preview: false,
        });
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_semantic_browser_harness(harness.clone()),
        );
        let allowed = executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &public,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(allowed.disposition, ToolAuthorizationDisposition::Allow);
        assert_eq!(
            allowed.matched_rule.as_deref(),
            Some("public_semantic_browser")
        );
        let outcome = executor
            .execute(public, CancellationToken::default())
            .await
            .unwrap();
        assert!(outcome.is_success(), "{}", outcome.content);
        assert_eq!(
            harness
                .navigate_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(outcome.content.contains("Deployment ready"));
        assert!(outcome.content.contains("external_untrusted"));

        for field in [
            "headers",
            "cookie",
            "authorization",
            "proxy",
            "method",
            "script",
            "screenshot",
        ] {
            let mut input = json!({"url":"https://example.com/"});
            input
                .as_object_mut()
                .unwrap()
                .insert(field.to_owned(), Value::String("forbidden".to_owned()));
            let rejected = executor
                .preflight(&invocation("browser_navigate", input))
                .expect("unsupported browser authority must fail schema preflight");
            assert_eq!(rejected.failure_code, Some(ToolFailureCode::InvalidField));
        }

        let local_origin = "http://127.0.0.1:32123";
        let local = invocation(
            "browser_navigate",
            json!({"url":"http://127.0.0.1:32123/app"}),
        );
        let local_executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_browser_local_origin(Some(local_origin.to_owned()))
                .with_semantic_browser_harness(harness.clone()),
        );
        assert!(local_executor.preflight(&local).is_none());
        let local_allowed = local_executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &local,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            local_allowed.matched_rule.as_deref(),
            Some("exact_local_semantic_browser")
        );
        let escaped = local_executor
            .preflight(&invocation(
                "browser_navigate",
                json!({"url":"http://127.0.0.1:32124/app"}),
            ))
            .expect("different loopback origin must fail before authorization");
        assert_eq!(
            escaped.failure_code,
            Some(ToolFailureCode::InvocationRejected)
        );

        let click = invocation(
            "browser_interact",
            json!({"action":"click","element_ref":"eref_0123456789abcdef0123456789abcdef"}),
        );
        assert!(local_executor.preflight(&click).is_none());
        assert_eq!(
            local_executor.definition_workspace_access("browser_interact"),
            WorkspaceAccess::MayWrite
        );
        let click_allowed = local_executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &click,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            click_allowed.disposition,
            ToolAuthorizationDisposition::Allow
        );
        assert_eq!(
            click_allowed.matched_rule.as_deref(),
            Some("exact_local_semantic_interaction")
        );
        let clicked = local_executor
            .execute(click, CancellationToken::default())
            .await
            .unwrap();
        assert!(clicked.is_success(), "{}", clicked.content);
        assert_eq!(clicked.side_effect, ToolSideEffectStatus::Applied);
        assert_eq!(
            harness
                .click_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let fill = invocation(
            "browser_interact",
            json!({
                "action":"fill",
                "element_ref":"eref_0123456789abcdef0123456789abcdef",
                "value":"canary"
            }),
        );
        assert!(local_executor.preflight(&fill).is_none());
        assert_eq!(
            local_executor.definition_workspace_access("browser_interact"),
            WorkspaceAccess::MayWrite
        );
        let fill_allowed = local_executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &fill,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            fill_allowed.disposition,
            ToolAuthorizationDisposition::Allow
        );
        assert_eq!(
            fill_allowed.matched_rule.as_deref(),
            Some("exact_local_semantic_interaction")
        );
        let full_executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::FullAccess)
                .with_browser_local_origin(Some(local_origin.to_owned()))
                .with_semantic_browser_harness(harness.clone()),
        );
        let full_fill_allowed = full_executor
            .authorize(
                RunPermissionMode::FullAccess,
                &ToolExecutionGrant::Ordinary,
                &invocation(
                    "browser_interact",
                    json!({
                        "action":"fill",
                        "element_ref":"eref_0123456789abcdef0123456789abcdef",
                        "value":"canary"
                    }),
                ),
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            full_fill_allowed.disposition,
            ToolAuthorizationDisposition::Allow
        );
        let filled = local_executor
            .execute(fill, CancellationToken::default())
            .await
            .unwrap();
        assert!(filled.is_success(), "{}", filled.content);
        assert_eq!(filled.side_effect, ToolSideEffectStatus::Applied);
        assert_eq!(
            harness.fill_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        for invalid in [
            json!({"action":"fill","element_ref":"eref_0123456789abcdef0123456789abcdef","value":""}),
            json!({"action":"fill","element_ref":"eref_0123456789abcdef0123456789abcdef","value":"line\nfeed"}),
            json!({"action":"fill","element_ref":"eref_0123456789abcdef0123456789abcdef","value":"canary","selector":"input"}),
        ] {
            assert!(
                local_executor
                    .preflight(&invocation("browser_interact", invalid))
                    .is_some()
            );
        }
        assert_eq!(
            harness.fill_calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "schema/value rejection must not touch the browser harness"
        );

        let public_click = invocation(
            "browser_interact",
            json!({"action":"click","element_ref":"eref_0123456789abcdef0123456789abcdef"}),
        );
        let public_click_denied = executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &public_click,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            public_click_denied.matched_rule.as_deref(),
            Some("browser_session_scope_missing")
        );
        let public_fill_denied = executor
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &invocation(
                    "browser_interact",
                    json!({
                        "action":"fill",
                        "element_ref":"eref_0123456789abcdef0123456789abcdef",
                        "value":"canary"
                    }),
                ),
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            public_fill_denied.matched_rule.as_deref(),
            Some("browser_session_scope_missing")
        );
        for forbidden in ["selector", "css", "xpath", "x", "y", "script"] {
            let mut input =
                json!({"action":"click","element_ref":"eref_0123456789abcdef0123456789abcdef"});
            input
                .as_object_mut()
                .unwrap()
                .insert(forbidden.to_owned(), Value::String("forbidden".to_owned()));
            assert!(
                local_executor
                    .preflight(&invocation("browser_interact", input))
                    .is_some(),
                "{forbidden}"
            );
        }
        for forbidden in [
            "selector",
            "css",
            "xpath",
            "x",
            "y",
            "script",
            "press",
            "submit",
            "headers",
            "cookie",
            "authorization",
            "path",
        ] {
            let mut input = json!({
                "action":"fill",
                "element_ref":"eref_0123456789abcdef0123456789abcdef",
                "value":"canary"
            });
            input
                .as_object_mut()
                .unwrap()
                .insert(forbidden.to_owned(), Value::String("forbidden".to_owned()));
            assert!(
                local_executor
                    .preflight(&invocation("browser_interact", input))
                    .is_some(),
                "{forbidden}"
            );
        }

        let writer = tempfile::tempdir().unwrap();
        let isolated = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_browser_local_origin(Some(local_origin.to_owned()))
                .with_semantic_browser_harness(harness.clone())
                .rebind_isolated_writer_workspace(writer.path()),
        );
        let writer_denied = isolated
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &invocation(
                    "browser_interact",
                    json!({"action":"click","element_ref":"eref_0123456789abcdef0123456789abcdef"}),
                ),
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            writer_denied.disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            writer_denied.matched_rule.as_deref(),
            Some("actor_controlled_network_denied")
        );
        let writer_fill_denied = isolated
            .authorize(
                RunPermissionMode::Agent,
                &ToolExecutionGrant::Ordinary,
                &invocation(
                    "browser_interact",
                    json!({
                        "action":"fill",
                        "element_ref":"eref_0123456789abcdef0123456789abcdef",
                        "value":"canary"
                    }),
                ),
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            writer_fill_denied.disposition,
            ToolAuthorizationDisposition::Deny
        );
        assert_eq!(
            writer_fill_denied.matched_rule.as_deref(),
            Some("actor_controlled_network_denied")
        );
    }

    #[test]
    fn public_semantic_interaction_authorization_is_exact_mode_and_impact_scoped() {
        let workspace = tempfile::tempdir().unwrap();
        let harness = Arc::new(FixtureSemanticBrowser {
            navigate_calls: std::sync::atomic::AtomicUsize::new(0),
            click_calls: std::sync::atomic::AtomicUsize::new(0),
            fill_calls: std::sync::atomic::AtomicUsize::new(0),
            shutdown_calls: std::sync::atomic::AtomicUsize::new(0),
            public_preview: true,
        });
        let click = invocation(
            "browser_interact",
            json!({"action":"click","element_ref":"eref_0123456789abcdef0123456789abcdef"}),
        );
        let submit = invocation(
            "browser_interact",
            json!({"action":"submit","element_ref":"eref_0123456789abcdef0123456789abcdef"}),
        );
        for (mode, invocation, disposition, rule) in [
            (
                RunPermissionMode::Ask,
                &click,
                ToolAuthorizationDisposition::Ask,
                "scoped_public_semantic_interaction",
            ),
            (
                RunPermissionMode::Agent,
                &click,
                ToolAuthorizationDisposition::Allow,
                "scoped_public_semantic_interaction",
            ),
            (
                RunPermissionMode::Agent,
                &submit,
                ToolAuthorizationDisposition::Ask,
                "scoped_public_reversible_side_effect",
            ),
            (
                RunPermissionMode::FullAccess,
                &submit,
                ToolAuthorizationDisposition::Allow,
                "scoped_public_reversible_side_effect",
            ),
        ] {
            let executor = ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path())
                    .with_permission_mode(mode)
                    .with_semantic_browser_harness(harness.clone()),
            );
            let decision = executor
                .authorize(
                    mode,
                    &ToolExecutionGrant::Ordinary,
                    invocation,
                    &workspace_state(),
                )
                .unwrap();
            assert_eq!(decision.disposition, disposition, "{mode:?}:{rule}");
            assert_eq!(decision.matched_rule.as_deref(), Some(rule));
            if disposition == ToolAuthorizationDisposition::Ask {
                let prompt = decision.prompt.expect("exact public action prompt");
                assert!(prompt.description.contains("https://example.com"));
                assert!(prompt.description.contains("exact target"));
                assert!(prompt.description.contains("parameters"));
                assert!(prompt.description.contains("impact"));
            }
        }
    }

    #[test]
    fn managed_browser_actions_share_exact_public_authorization_and_actor_denials() {
        let workspace = tempfile::tempdir().unwrap();
        let harness = Arc::new(FixtureSemanticBrowser {
            navigate_calls: std::sync::atomic::AtomicUsize::new(0),
            click_calls: std::sync::atomic::AtomicUsize::new(0),
            fill_calls: std::sync::atomic::AtomicUsize::new(0),
            shutdown_calls: std::sync::atomic::AtomicUsize::new(0),
            public_preview: true,
        });
        let element_ref = "eref_0123456789abcdef0123456789abcdef";
        let cases = [
            (
                json!({"action":"login","element_ref":element_ref,"credential_ref":"engineering-app"}),
                true,
            ),
            (
                json!({"action":"upload","element_ref":element_ref,"workspace_path":"artifact.txt"}),
                true,
            ),
            (
                json!({"action":"download","element_ref":element_ref}),
                false,
            ),
            (
                json!({"action":"promote_download","download_ref":"dref_0123456789abcdef0123456789abcdef","workspace_path":"result.txt"}),
                false,
            ),
            (json!({"action":"session_status"}), false),
            (json!({"action":"session_clear"}), true),
        ];
        for (input, external) in &cases {
            let invocation = invocation("browser_interact", input.clone());
            assert!(
                ProductionToolExecutor::new(
                    ProductionToolConfig::new(workspace.path())
                        .with_semantic_browser_harness(harness.clone())
                )
                .preflight(&invocation)
                .is_none(),
                "{input}"
            );
            for (mode, expected) in [
                (RunPermissionMode::Ask, ToolAuthorizationDisposition::Ask),
                (
                    RunPermissionMode::Agent,
                    if *external {
                        ToolAuthorizationDisposition::Ask
                    } else {
                        ToolAuthorizationDisposition::Allow
                    },
                ),
                (
                    RunPermissionMode::FullAccess,
                    ToolAuthorizationDisposition::Allow,
                ),
            ] {
                let executor = ProductionToolExecutor::new(
                    ProductionToolConfig::new(workspace.path())
                        .with_permission_mode(mode)
                        .with_semantic_browser_harness(harness.clone()),
                );
                let decision = executor
                    .authorize(
                        mode,
                        &ToolExecutionGrant::Ordinary,
                        &invocation,
                        &workspace_state(),
                    )
                    .unwrap();
                assert_eq!(decision.disposition, expected, "{mode:?}:{input}");
                if expected == ToolAuthorizationDisposition::Ask {
                    let prompt = decision.prompt.expect("scoped approval prompt");
                    assert!(prompt.description.contains("exact target"));
                    assert!(!prompt.description.contains("password"));
                }
            }
        }

        let writer = tempfile::tempdir().unwrap();
        let writer_executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_semantic_browser_harness(harness)
                .rebind_isolated_writer_workspace(writer.path()),
        );
        for (input, _) in cases {
            let decision = writer_executor
                .authorize(
                    RunPermissionMode::Agent,
                    &ToolExecutionGrant::Ordinary,
                    &invocation("browser_interact", input),
                    &workspace_state(),
                )
                .unwrap();
            assert_eq!(decision.disposition, ToolAuthorizationDisposition::Deny);
            assert_eq!(
                decision.matched_rule.as_deref(),
                Some("actor_controlled_network_denied")
            );
        }
    }

    #[test]
    fn m31_exact_contract_verifier_grant_is_narrow_and_matrix_bound() {
        let frozen: Value = serde_json::from_str(include_str!(
            "../../../eval/fixtures/m31-contract-verifier-permission-v1.json"
        ))
        .unwrap();
        assert_eq!(frozen["cases"].as_array().unwrap().len(), 16);
        assert_eq!(frozen["replay_windows"].as_array().unwrap().len(), 4);

        let workspace = tempfile::tempdir().unwrap();
        let exact_input = json!({
            "profile": "exact",
            "commands": [{
                "name": "exact",
                "program": "/usr/bin/python3",
                "args": ["-I", "-B", "verify.py", "."],
                "cwd": ""
            }]
        });
        for mode in [
            RunPermissionMode::Ask,
            RunPermissionMode::Agent,
            RunPermissionMode::FullAccess,
        ] {
            let executor = ProductionToolExecutor::new(
                ProductionToolConfig::new(workspace.path())
                    .with_permission_mode(mode)
                    .with_shell_policy(ShellPolicy::Full),
            );
            let verifier = executor
                .resolve_verifier_spec("run_verifiers", exact_input.clone())
                .unwrap();
            let exact_invocation = invocation("run_verifiers", exact_input.clone());
            let grant = ToolExecutionGrant::TaskContractVerifier {
                acceptance_id: dse_protocol::task::AcceptanceId::from("frozen-check"),
                verifier_sha256: verifier.sha256(),
            };
            let allowed = executor
                .authorize(mode, &grant, &exact_invocation, &workspace_state())
                .unwrap();
            assert_eq!(allowed.disposition, ToolAuthorizationDisposition::Allow);
            assert_eq!(
                allowed.matched_rule.as_deref(),
                Some("task_contract_verifier_exact")
            );

            if mode == RunPermissionMode::Ask {
                let ordinary = executor
                    .authorize(
                        mode,
                        &ToolExecutionGrant::Ordinary,
                        &exact_invocation,
                        &workspace_state(),
                    )
                    .unwrap();
                assert_eq!(ordinary.disposition, ToolAuthorizationDisposition::Deny);
                assert_eq!(
                    ordinary.matched_rule.as_deref(),
                    Some("ask_external_path_fail_closed")
                );

                let wrong_digest = ToolExecutionGrant::TaskContractVerifier {
                    acceptance_id: dse_protocol::task::AcceptanceId::from("frozen-check"),
                    verifier_sha256: format!("sha256:{}", "0".repeat(64)),
                };
                let rejected = executor
                    .authorize(mode, &wrong_digest, &exact_invocation, &workspace_state())
                    .unwrap();
                assert_eq!(rejected.disposition, ToolAuthorizationDisposition::Deny);
                assert_eq!(
                    rejected.matched_rule.as_deref(),
                    Some("task_contract_verifier_grant_invalid")
                );

                let drifted = invocation(
                    "run_verifiers",
                    json!({
                        "profile": "exact",
                        "commands": [{
                            "name": "exact",
                            "program": "/usr/bin/python3",
                            "args": ["-I", "-B", "different.py", "."],
                            "cwd": ""
                        }]
                    }),
                );
                assert_eq!(
                    executor
                        .authorize(mode, &grant, &drifted, &workspace_state())
                        .unwrap()
                        .disposition,
                    ToolAuthorizationDisposition::Deny
                );
            }
        }

        let writer = tempfile::tempdir().unwrap();
        let writer_executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_permission_mode(RunPermissionMode::Agent)
                .rebind_isolated_writer_workspace(writer.path()),
        );
        let writer_verifier = writer_executor
            .resolve_verifier_spec("run_verifiers", exact_input.clone())
            .unwrap();
        let writer_grant = ToolExecutionGrant::TaskContractVerifier {
            acceptance_id: dse_protocol::task::AcceptanceId::from("writer-check"),
            verifier_sha256: writer_verifier.sha256(),
        };
        let writer_decision = writer_executor
            .authorize(
                RunPermissionMode::Agent,
                &writer_grant,
                &invocation("run_verifiers", exact_input),
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(
            writer_decision.disposition,
            ToolAuthorizationDisposition::Allow
        );
        assert_eq!(
            writer_decision.matched_rule.as_deref(),
            Some("task_contract_verifier_exact")
        );
    }

    #[test]
    fn shell_text_never_downgrades_runtime_workspace_authority() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path()).with_shell_policy(ShellPolicy::Full),
        );

        for command in [
            "git status --short",
            "rg needle src",
            "rg --pre mutate-helper needle src",
            "fd --exec rm {}",
        ] {
            assert_eq!(
                executor.workspace_access(&invocation("exec_shell", json!({"command": command}))),
                WorkspaceAccess::MayWrite,
                "shell command text must not become a workspace authority certificate: {command}"
            );
        }
        assert_eq!(
            executor.workspace_access(&invocation("read_file", json!({"path":"src/lib.rs"}))),
            WorkspaceAccess::ReadOnly
        );
    }

    #[tokio::test]
    async fn hidden_fields_are_typed_preflight_rejections() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(temp.path()).with_shell_policy(ShellPolicy::Full),
        );
        for (tool, field) in [
            ("exec_shell", "background"),
            ("exec_shell", "interactive"),
            ("exec_shell", "tty"),
            ("exec_shell", "combined_output"),
            ("exec_shell", "stdin"),
            ("exec_shell", "input"),
            ("exec_shell", "data"),
            ("exec_shell", "working_dir"),
            ("run_verifiers", "background"),
        ] {
            let outcome = executor
                .preflight(&invocation(tool, json!({field: true})))
                .expect("hidden field must fail before execution starts");
            assert_eq!(
                outcome.invocation,
                ToolInvocationStatus::Rejected,
                "{tool}.{field}"
            );
            assert_eq!(outcome.retry, ToolRetryDisposition::AfterCorrection);
        }
    }

    #[tokio::test]
    async fn malformed_unknown_and_pre_cancel_have_typed_outcomes() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let unknown = executor
            .preflight(&invocation("write_file", json!({})))
            .expect("unknown tool must fail preflight");
        assert_eq!(unknown.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(unknown.failure_code, Some(ToolFailureCode::UnknownTool));

        let malformed = executor
            .preflight(&ToolInvocation {
                run_id: RunId::from("run-1"),
                call_id: "bad".to_string(),
                name: "list_dir".to_string(),
                arguments: ToolArguments::parse("{"),
            })
            .expect("malformed arguments must fail preflight");
        assert_eq!(malformed.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(
            malformed.failure_code,
            Some(ToolFailureCode::MalformedArguments)
        );
        assert!(!malformed.content.contains('{'));

        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let canceled = executor
            .execute(invocation("list_dir", json!({})), cancellation)
            .await
            .unwrap();
        assert_eq!(canceled.operation, ToolOperationStatus::Cancelled);
        assert_eq!(canceled.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(canceled.retry, ToolRetryDisposition::Safe);
    }

    #[test]
    fn readonly_failures_never_require_write_side_effect_recovery() {
        let ordinary = ToolOutcome::error("读取失败")
            .with_workspace_access_guarantee(WorkspaceAccess::ReadOnly);
        assert_eq!(ordinary.operation, ToolOperationStatus::Failed);
        assert_eq!(ordinary.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_eq!(ordinary.retry, ToolRetryDisposition::NotRetryable);
        ordinary.validate().unwrap();

        let timeout = ProductionToolExecutor::execution_error_outcome(
            ToolError::Timeout { seconds: 30 },
            WorkspaceAccess::ReadOnly,
        );
        assert_eq!(timeout.operation, ToolOperationStatus::Indeterminate);
        assert_eq!(timeout.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_eq!(timeout.retry, ToolRetryDisposition::Safe);
        timeout.validate().unwrap();

        let cancelled = ToolOutcome::recovery_ambiguous("只读任务取消后未及时停止")
            .with_workspace_access_guarantee(WorkspaceAccess::ReadOnly);
        assert_eq!(
            cancelled.failure_code,
            Some(ToolFailureCode::OperationFailed)
        );
        assert_eq!(cancelled.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_eq!(cancelled.retry, ToolRetryDisposition::Safe);
        cancelled.validate().unwrap();

        let writer_timeout = ProductionToolExecutor::execution_error_outcome(
            ToolError::Timeout { seconds: 30 },
            WorkspaceAccess::MayWrite,
        );
        assert_eq!(
            writer_timeout.side_effect,
            ToolSideEffectStatus::Indeterminate
        );
        assert_eq!(writer_timeout.retry, ToolRetryDisposition::Unsafe);
        writer_timeout.validate().unwrap();
    }

    #[tokio::test]
    async fn readonly_tool_owned_failure_is_projected_without_write_ambiguity() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let outcome = executor
            .execute(
                invocation("git_status", json!({})),
                CancellationToken::default(),
            )
            .await
            .unwrap();

        assert!(!outcome.is_success());
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_ne!(outcome.retry, ToolRetryDisposition::Unsafe);
        outcome.validate().unwrap();
    }

    #[tokio::test]
    async fn production_failure_matrix_preserves_code_side_effect_and_retry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, "same\nsame\n").unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));

        let missing = executor
            .preflight(&invocation("edit_file", json!({})))
            .expect("missing field must fail preflight");
        assert_eq!(missing.failure_code, Some(ToolFailureCode::MissingField));
        assert_eq!(missing.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(missing.side_effect, ToolSideEffectStatus::NotApplied);

        let extra = executor
            .preflight(&invocation(
                "read_file",
                json!({"path":"owned.txt","bogus":true}),
            ))
            .expect("extra field must fail preflight");
        assert_eq!(extra.failure_code, Some(ToolFailureCode::InvalidField));
        assert_eq!(extra.invocation, ToolInvocationStatus::Rejected);

        let read = executor
            .execute(
                invocation("read_file", json!({"path":"owned.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());
        let ambiguous = executor
            .execute(
                invocation(
                    "edit_file",
                    json!({"path":"owned.txt","search":"same","replace":"changed"}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(ambiguous.failure_code, Some(ToolFailureCode::AmbiguousEdit));
        assert_eq!(ambiguous.operation, ToolOperationStatus::Failed);
        assert_eq!(ambiguous.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(ambiguous.retry, ToolRetryDisposition::AfterCorrection);

        std::fs::write(&path, "external\n").unwrap();
        let stale = executor
            .execute(
                invocation(
                    "edit_file",
                    json!({"path":"owned.txt","search":"same","replace":"changed"}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(stale.failure_code, Some(ToolFailureCode::StaleRead));
        assert_eq!(stale.operation, ToolOperationStatus::Failed);
        assert_eq!(stale.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(stale.retry, ToolRetryDisposition::AfterCorrection);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external\n");

        let patch_path = temp.path().join("patch.txt");
        std::fs::write(&patch_path, "current\n").unwrap();
        let patch = invocation(
            "apply_patch",
            json!({
                "path":"patch.txt",
                "fuzz":0,
                "patch":"@@ -1 +1 @@\n-stale\n+changed"
            }),
        );
        assert!(
            executor.preflight(&patch).is_none(),
            "parse-valid stale patch must cross the execution boundary"
        );
        let precondition = executor
            .execute(patch, CancellationToken::default())
            .await
            .unwrap();
        assert_eq!(
            precondition.failure_code,
            Some(ToolFailureCode::WorkspacePrecondition)
        );
        assert_eq!(precondition.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(precondition.transport, ToolTransportStatus::Succeeded);
        assert_eq!(precondition.operation, ToolOperationStatus::Failed);
        assert_eq!(precondition.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(precondition.retry, ToolRetryDisposition::AfterCorrection);
        assert_eq!(std::fs::read_to_string(patch_path).unwrap(), "current\n");
    }

    #[test]
    fn schema_and_patch_preflight_reject_before_any_operation_starts() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let cases = [
            (
                invocation("read_file", json!([])),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("edit_file", json!({})),
                ToolFailureCode::MissingField,
            ),
            (
                invocation("read_file", json!({"path": 7})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("read_file", json!({"path":"a","extra":true})),
                ToolFailureCode::InvalidField,
            ),
            (
                invocation("git_diff", json!({"unified":-1})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("git_diff", json!({"unified":51})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("run_verifiers", json!({"profile":"invalid"})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation(
                    "run_verifiers",
                    json!({"commands":[{"name":"missing-program"}]}),
                ),
                ToolFailureCode::MissingField,
            ),
            (
                invocation("apply_patch", json!({})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("apply_patch", json!({"patch":"x","changes":[]})),
                ToolFailureCode::SchemaValidation,
            ),
            (
                invocation("apply_patch", json!({"patch":"not a unified patch"})),
                ToolFailureCode::PatchParse,
            ),
            (
                invocation(
                    "apply_patch",
                    json!({
                        "changes":[{"path":"value.txt","content":"changed\n"}],
                        "fuzz":1
                    }),
                ),
                ToolFailureCode::InvalidField,
            ),
            (
                invocation(
                    "apply_patch",
                    json!({"changes":[
                        {"path":"same.txt","content":"one\n"},
                        {"path":"same.txt","content":"two\n"}
                    ]}),
                ),
                ToolFailureCode::InvalidField,
            ),
        ];
        for (invocation, expected_code) in cases {
            let name = invocation.name.clone();
            let outcome = executor
                .preflight(&invocation)
                .unwrap_or_else(|| panic!("{name} input unexpectedly passed preflight"));
            assert_eq!(outcome.failure_code, Some(expected_code), "{name}");
            assert_eq!(outcome.invocation, ToolInvocationStatus::Rejected, "{name}");
            assert_eq!(outcome.operation, ToolOperationStatus::NotStarted, "{name}");
            assert_eq!(
                outcome.side_effect,
                ToolSideEffectStatus::NotApplied,
                "{name}"
            );
            assert!(outcome.validate().is_ok(), "{name}: {outcome:?}");
        }

        for invocation in [
            invocation("git_diff", json!({"unified":0})),
            invocation("git_diff", json!({"unified":50})),
            invocation(
                "apply_patch",
                json!({"changes":[{"path":"new.txt","content":"ok\n"}]}),
            ),
        ] {
            assert!(
                executor.preflight(&invocation).is_none(),
                "{} valid boundary input was rejected",
                invocation.name
            );
        }
    }

    #[tokio::test]
    async fn semantic_failure_after_start_is_accepted_failed_and_not_applied() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("owned.txt"), "same\n").unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let read = executor
            .execute(
                invocation("read_file", json!({"path":"owned.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());

        for invocation in [
            invocation(
                "edit_file",
                json!({"path":"owned.txt","search":"same","replace":"same"}),
            ),
            invocation("grep_files", json!({"pattern":"["})),
        ] {
            assert!(executor.preflight(&invocation).is_none());
            let name = invocation.name.clone();
            let outcome = executor
                .execute(invocation, CancellationToken::default())
                .await
                .unwrap();
            assert_eq!(outcome.invocation, ToolInvocationStatus::Accepted, "{name}");
            assert_eq!(outcome.operation, ToolOperationStatus::Failed, "{name}");
            assert_eq!(
                outcome.side_effect,
                ToolSideEffectStatus::NotApplied,
                "{name}"
            );
            assert_eq!(
                outcome.retry,
                ToolRetryDisposition::AfterCorrection,
                "{name}"
            );
            assert_eq!(outcome.failure_code, Some(ToolFailureCode::InvalidField));
            assert!(outcome.validate().is_ok(), "{name}: {outcome:?}");
        }
        assert_eq!(
            std::fs::read_to_string(temp.path().join("owned.txt")).unwrap(),
            "same\n"
        );
    }

    #[tokio::test]
    async fn m7c_canonical_edit_failure_codes_are_typed_and_non_mutating() {
        use std::fs::FileTimes;

        let temp = tempfile::tempdir().unwrap();
        let stale = temp.path().join("stale.txt");
        let drift = temp.path().join("drift.txt");
        let stable = temp.path().join("stable.txt");
        std::fs::write(&stale, "alpha\n").unwrap();
        std::fs::write(&drift, "target\nspacer\ntarget\n").unwrap();
        std::fs::write(&stable, "stable\n").unwrap();
        let modified = std::fs::metadata(&stale)
            .unwrap()
            .modified()
            .expect("mtime");
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let read = executor
            .execute(
                invocation("read_file", json!({"path":"stale.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());
        std::fs::write(&stale, "omega\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();

        let cases = [
            (
                invocation(
                    "edit_file",
                    json!({"path":"stale.txt","search":"alpha","replace":"beta"}),
                ),
                ToolFailureCode::StaleRead,
            ),
            (
                invocation(
                    "apply_patch",
                    json!({
                        "path":"drift.txt",
                        "patch":"@@ -2,1 +2,1 @@\n-target\n+changed\n",
                        "fuzz":1
                    }),
                ),
                ToolFailureCode::AmbiguousEdit,
            ),
            (
                invocation(
                    "apply_patch",
                    json!({"changes":[{"path":"stable.txt","content":"stable\n"}]}),
                ),
                ToolFailureCode::InvalidField,
            ),
        ];
        for (invocation, expected_code) in cases {
            assert!(executor.preflight(&invocation).is_none());
            let outcome = executor
                .execute(invocation, CancellationToken::default())
                .await
                .unwrap();
            assert_eq!(outcome.failure_code, Some(expected_code));
            assert_eq!(outcome.invocation, ToolInvocationStatus::Accepted);
            assert_eq!(outcome.operation, ToolOperationStatus::Failed);
            assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
            assert_eq!(outcome.retry, ToolRetryDisposition::AfterCorrection);
            assert!(outcome.model_content().contains("恢复建议："));
        }
        assert_eq!(std::fs::read(&stale).unwrap(), b"omega\n");
        assert_eq!(std::fs::read(&drift).unwrap(), b"target\nspacer\ntarget\n");
        assert_eq!(std::fs::read(&stable).unwrap(), b"stable\n");
    }

    #[tokio::test]
    async fn every_fixed_name_has_direct_dispatch_and_aliases_fail_preflight() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        for name in PRODUCTION_TOOL_NAMES {
            let outcome = executor
                .execute(invocation(name, json!({})), CancellationToken::default())
                .await
                .unwrap();
            assert!(
                outcome.failure_code != Some(ToolFailureCode::UnknownTool),
                "fixed tool {name} did not reach its direct implementation"
            );
        }

        for alias in ["write_file", "task_shell_start", "exec_shell_wait"] {
            let outcome = executor
                .preflight(&invocation(alias, json!({})))
                .expect("stale alias must fail before dispatch");
            assert_eq!(outcome.failure_code, Some(ToolFailureCode::UnknownTool));
            assert_eq!(outcome.invocation, ToolInvocationStatus::Rejected);
        }
    }

    #[tokio::test]
    async fn executors_do_not_share_read_before_edit_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, "before\n").unwrap();
        let config = ProductionToolConfig::new(temp.path());
        let reader = ProductionToolExecutor::new(config.clone());
        let writer = ProductionToolExecutor::new(config);

        let read = reader
            .execute(
                invocation("read_file", json!({"path":"owned.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());

        let edit = writer
            .execute(
                invocation(
                    "edit_file",
                    json!({"path":"owned.txt","search":"before","replace":"after"}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(edit.operation, ToolOperationStatus::Failed);
        assert_eq!(
            edit.failure_code,
            Some(ToolFailureCode::WorkspacePrecondition)
        );
        assert_eq!(edit.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(edit.retry, ToolRetryDisposition::AfterCorrection);
        assert!(edit.content.contains("尚未读取"), "{}", edit.content);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "before\n");
    }

    #[tokio::test]
    async fn isolated_writer_rebinding_is_restrictive_and_resets_read_state() {
        let root = tempfile::tempdir().unwrap();
        let writer = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("owned.txt"), "root\n").unwrap();
        std::fs::write(writer.path().join("owned.txt"), "writer\n").unwrap();
        let base = ProductionToolConfig::new(root.path())
            .with_permission_mode(RunPermissionMode::Agent)
            .with_follow_symlinks(true)
            .with_shell_policy(ShellPolicy::Full)
            .with_elevated_sandbox_policy(ExecutionSandboxPolicy::DangerFullAccess);
        let root_executor = ProductionToolExecutor::new(base.clone());
        let writer_config = base.rebind_isolated_writer_workspace(writer.path());
        let identity = writer_config.execution_identity();
        assert_eq!(
            identity.workspace,
            writer.path().canonicalize().unwrap().display().to_string()
        );
        assert!(!identity.allow_external_paths);
        assert!(!identity.follow_symlinks);
        assert_eq!(identity.permission_mode, RunPermissionMode::Agent);
        assert_eq!(identity.sandbox_backend, None);
        assert!(matches!(
            identity.elevated_sandbox_policy,
            Some(ExecutionSandboxPolicy::IsolatedWriter { ref workspace, .. })
                if workspace == writer.path()
        ));

        let writer_executor = ProductionToolExecutor::new(writer_config);
        for invocation in [
            invocation("read_file", json!({"path": root.path().join("owned.txt")})),
            invocation("exec_shell", json!({"command": "pwd", "cwd": root.path()})),
            invocation(
                "exec_shell",
                json!({"command": "curl https://example.invalid"}),
            ),
        ] {
            let decision = writer_executor
                .authorize(
                    RunPermissionMode::Agent,
                    &ToolExecutionGrant::Ordinary,
                    &invocation,
                    &workspace_state(),
                )
                .unwrap();
            assert_eq!(decision.disposition, ToolAuthorizationDisposition::Deny);
        }
        let read = root_executor
            .execute(
                invocation("read_file", json!({"path":"owned.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());
        let edit = writer_executor
            .execute(
                invocation(
                    "edit_file",
                    json!({"path":"owned.txt","search":"writer","replace":"changed"}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(edit.operation, ToolOperationStatus::Failed);
        assert_eq!(
            edit.failure_code,
            Some(ToolFailureCode::WorkspacePrecondition)
        );
        assert!(edit.content.contains("尚未读取"), "{}", edit.content);
        assert_eq!(
            std::fs::read_to_string(root.path().join("owned.txt")).unwrap(),
            "root\n"
        );
        assert_eq!(
            std::fs::read_to_string(writer.path().join("owned.txt")).unwrap(),
            "writer\n"
        );
    }

    #[tokio::test]
    async fn isolated_writer_apply_patch_rejects_metadata_and_outside_before_mutation() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let common_git = root.join(".git");
        let per_worktree_git = common_git.join("worktrees/writer");
        let writer = fixture.path().join("writer");
        let other = fixture.path().join("other");
        for directory in [
            &per_worktree_git,
            &writer.join(".dse"),
            &writer.join(".deepseek"),
            &other,
        ] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::write(
            writer.join(".git"),
            format!("gitdir: {}\n", per_worktree_git.display()),
        )
        .unwrap();
        std::fs::write(per_worktree_git.join("commondir"), "../..").unwrap();
        std::fs::write(
            per_worktree_git.join("gitdir"),
            writer.join(".git").display().to_string(),
        )
        .unwrap();
        std::fs::write(per_worktree_git.join("index"), "per-worktree\n").unwrap();
        std::fs::write(common_git.join("config"), "common\n").unwrap();
        std::fs::write(root.join("root.txt"), "root\n").unwrap();
        std::fs::write(other.join("other.txt"), "other\n").unwrap();
        std::fs::write(writer.join(".dse/state"), "dse\n").unwrap();
        std::fs::write(writer.join(".deepseek/config"), "deepseek\n").unwrap();
        std::fs::write(writer.join("owned.txt"), "before\n").unwrap();

        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(&root).rebind_isolated_writer_workspace(&writer),
        );
        let forbidden = [
            writer.join(".git"),
            writer.join(".git/index"),
            writer.join("nested/../.dse/state"),
            writer.join(".deepseek/config"),
            per_worktree_git.join("index"),
            common_git.join("config"),
            root.join("root.txt"),
            other.join("other.txt"),
        ];

        for target in forbidden {
            let target_before = std::fs::read(&target).ok();
            let outcome = executor
                .execute(
                    invocation(
                        "apply_patch",
                        json!({
                            "changes": [
                                {"path": "owned.txt", "content": "must-not-land\n"},
                                {"path": target, "content": "must-not-land\n"}
                            ]
                        }),
                    ),
                    CancellationToken::default(),
                )
                .await
                .unwrap();
            assert_eq!(
                outcome.invocation,
                ToolInvocationStatus::Accepted,
                "{}",
                outcome.content
            );
            assert_eq!(outcome.operation, ToolOperationStatus::Failed);
            assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
            assert_eq!(outcome.failure_code, Some(ToolFailureCode::OperationFailed));
            assert!(outcome.validate().is_ok(), "{}", outcome.content);
            assert!(
                outcome.content.contains("隔离 Writer 只能修改"),
                "{}",
                outcome.content
            );
            assert_eq!(
                std::fs::read(writer.join("owned.txt")).unwrap(),
                b"before\n"
            );
            assert_eq!(std::fs::read(&target).ok(), target_before);
        }

        let allowed = executor
            .execute(
                invocation(
                    "apply_patch",
                    json!({"changes":[{"path":"owned.txt","content":"after\n"}]}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(allowed.is_success(), "{}", allowed.content);
        assert_eq!(std::fs::read(writer.join("owned.txt")).unwrap(), b"after\n");
    }

    #[tokio::test]
    async fn isolated_writer_edit_file_rejects_protected_path_after_fresh_read() {
        let writer = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(writer.path().join(".deepseek")).unwrap();
        let protected = writer.path().join(".deepseek/config");
        let ordinary = writer.path().join("owned.txt");
        std::fs::write(&protected, "before\n").unwrap();
        std::fs::write(&ordinary, "before\n").unwrap();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(writer.path())
                .rebind_isolated_writer_workspace(writer.path()),
        );

        let read_protected = executor
            .execute(
                invocation("read_file", json!({"path":".deepseek/config"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read_protected.is_success(), "{}", read_protected.content);
        let denied = executor
            .execute(
                invocation(
                    "edit_file",
                    json!({
                        "path": ".deepseek/config",
                        "search": "before",
                        "replace": "after"
                    }),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(denied.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(denied.operation, ToolOperationStatus::Failed);
        assert_eq!(denied.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(denied.failure_code, Some(ToolFailureCode::OperationFailed));
        assert!(denied.validate().is_ok(), "{}", denied.content);
        assert!(
            denied.content.contains("隔离 Writer 只能修改"),
            "{}",
            denied.content
        );
        assert_eq!(std::fs::read(&protected).unwrap(), b"before\n");

        let read_ordinary = executor
            .execute(
                invocation("read_file", json!({"path":"owned.txt"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read_ordinary.is_success(), "{}", read_ordinary.content);
        let edited = executor
            .execute(
                invocation(
                    "edit_file",
                    json!({
                        "path": "owned.txt",
                        "search": "before",
                        "replace": "after"
                    }),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(edited.is_success(), "{}", edited.content);
        assert_eq!(std::fs::read(&ordinary).unwrap(), b"after\n");
    }

    #[tokio::test]
    async fn ordinary_root_builtin_write_paths_do_not_gain_writer_restrictions() {
        let root = tempfile::tempdir().unwrap();
        for directory in [".git", ".dse", ".deepseek"] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        std::fs::write(root.path().join(".git/local"), "git-before\n").unwrap();
        std::fs::write(root.path().join(".dse/state"), "state-before\n").unwrap();
        std::fs::write(root.path().join(".deepseek/config"), "config-before\n").unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(root.path()));

        let patch = executor
            .execute(
                invocation(
                    "apply_patch",
                    json!({
                        "changes": [
                            {"path": ".git/local", "content": "git-after\n"},
                            {"path": ".dse/state", "content": "state-after\n"}
                        ]
                    }),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(patch.is_success(), "{}", patch.content);

        let read = executor
            .execute(
                invocation("read_file", json!({"path":".deepseek/config"})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success(), "{}", read.content);
        let edit = executor
            .execute(
                invocation(
                    "edit_file",
                    json!({
                        "path": ".deepseek/config",
                        "search": "config-before",
                        "replace": "config-after"
                    }),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(edit.is_success(), "{}", edit.content);
        assert_eq!(
            std::fs::read(root.path().join(".git/local")).unwrap(),
            b"git-after\n"
        );
        assert_eq!(
            std::fs::read(root.path().join(".dse/state")).unwrap(),
            b"state-after\n"
        );
        assert_eq!(
            std::fs::read(root.path().join(".deepseek/config")).unwrap(),
            b"config-after\n"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_cancellation_stops_managed_process_tree() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("must-not-exist");
        let executor = Arc::new(ProductionToolExecutor::new(
            ProductionToolConfig::new(temp.path())
                .with_permission_mode(RunPermissionMode::FullAccess)
                .with_shell_policy(ShellPolicy::Full),
        ));
        let cancellation = CancellationToken::default();
        let running = {
            let executor = Arc::clone(&executor);
            let cancellation = cancellation.clone();
            let command = format!("sleep 2; touch {}", marker.display());
            tokio::spawn(async move {
                executor
                    .execute(
                        invocation("exec_shell", json!({"command": command})),
                        cancellation,
                    )
                    .await
                    .unwrap()
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        cancellation.cancel();
        let outcome = running.await.unwrap();
        assert_eq!(outcome.operation, ToolOperationStatus::Cancelled);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!marker.exists(), "descendant survived runtime cancellation");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exec_policy_deny_is_a_durable_authorization_and_safe_after_start() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("blocked");
        let policy = ExecPolicy {
            rules: BTreeMap::from([(
                "writes".to_string(),
                dse_execpolicy::ExecPolicyRuleSet {
                    allow: Vec::new(),
                    deny: vec!["touch *".to_string()],
                },
            )]),
        };
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(temp.path())
                .with_permission_mode(RunPermissionMode::FullAccess)
                .with_shell_policy(ShellPolicy::Full)
                .with_exec_policy(Some(policy)),
        );
        let invocation = invocation(
            "exec_shell",
            json!({"command": format!("touch {}", marker.display())}),
        );
        assert!(executor.preflight(&invocation).is_none());
        let decision = executor
            .authorize(
                RunPermissionMode::FullAccess,
                &ToolExecutionGrant::Ordinary,
                &invocation,
                &workspace_state(),
            )
            .unwrap();
        assert_eq!(decision.disposition, ToolAuthorizationDisposition::Deny);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("writes:deny:touch *")
        );
        assert!(!marker.exists());

        // Direct execution models a caller that bypassed Runtime's durable
        // Deny. The operation layer defensively rechecks and remains
        // side-effect free without claiming a preflight lifecycle.
        let outcome = executor
            .execute(invocation, CancellationToken::default())
            .await
            .unwrap();
        assert_eq!(outcome.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(outcome.operation, ToolOperationStatus::Failed);
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(outcome.failure_code, Some(ToolFailureCode::OperationFailed));
        assert_eq!(outcome.retry, ToolRetryDisposition::NotRetryable);
        assert!(!marker.exists());
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["execpolicy"]["decision"],
            "deny"
        );
    }
}
