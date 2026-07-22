//! Fixed production tool catalog and direct runtime executor.
//!
//! This is the sole model-visible tool owner. It intentionally uses direct
//! exact-name dispatch instead of a second registry or handler abstraction.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    ApprovalRisk, ToolApprovalPrompt, ToolDefinition, ToolFailureCode, ToolOperationStatus,
    ToolRetryDisposition, ToolSideEffectStatus, WorkspaceAccess,
};
use codewhale_protocol::task::VerifierSpec;
use codewhale_runtime::{CancellationToken, ToolExecutionError, ToolExecutor, ToolInvocation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken as TokioCancellationToken;

use crate::command_safety::command_is_high_impact;
use crate::sandbox::SandboxPolicy as ExecutionSandboxPolicy;
use crate::sandbox::backend::{SandboxBackend, SandboxBackendIdentity};
use crate::shell::{
    ExecShellHost, ExecShellOptions, ExecShellPolicyDecision, ShellPolicy,
    exec_shell_input_is_parallel_readonly, execute_exec_shell, new_shared_shell_manager,
    preflight_exec_shell,
};
use crate::{
    ProductionToolContext, ToolError, ToolOutcome, capture_workspace_revision, execute_apply_patch,
    execute_edit_file, execute_file_search, execute_git_diff, execute_git_status,
    execute_grep_files, execute_list_dir, execute_read_file, execute_run_tests,
    execute_run_verifiers, preflight_apply_patch, resolve_run_tests_spec,
    resolve_run_verifiers_spec,
};

pub const PRODUCTION_TOOL_NAMES: [&str; 11] = [
    "apply_patch",
    "edit_file",
    "exec_shell",
    "file_search",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "read_file",
    "run_tests",
    "run_verifiers",
];

/// Immutable per-run configuration. It never contains live shell jobs,
/// cancellation state or read-before-edit freshness.
#[derive(Clone)]
pub struct ProductionToolConfig {
    workspace: PathBuf,
    trust_mode: bool,
    trusted_external_paths: Vec<PathBuf>,
    follow_symlinks: bool,
    auto_approve: bool,
    shell_policy: ShellPolicy,
    elevated_sandbox_policy: Option<ExecutionSandboxPolicy>,
    shell_network_denied_hint: Option<String>,
    sandbox_backend: Option<Arc<dyn SandboxBackend>>,
    prefer_external_pdftotext: bool,
    exec_policy: Option<ProductionExecPolicySnapshot>,
}

/// Serializable snapshot of the legacy exec-policy file used by the fixed
/// runtime. Loading/parsing remains a composition concern; evaluation is
/// tools-owned so app and CLI enforce the same deny/allow decision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExecPolicySnapshot {
    #[serde(default)]
    pub rules: BTreeMap<String, ProductionExecPolicyRuleSet>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExecPolicyRuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ProductionExecPolicySnapshot {
    fn evaluate(&self, command: &str) -> ExecShellPolicyDecision {
        for (group, rules) in &self.rules {
            for pattern in &rules.deny {
                if exec_policy_pattern_matches(pattern, command) {
                    return ExecShellPolicyDecision::Deny(format!(
                        "execpolicy denied by {group}: {pattern}"
                    ));
                }
            }
        }
        for rules in self.rules.values() {
            for pattern in &rules.allow {
                if crate::command_safety::prefix_allow_matches(pattern, command)
                    || exec_policy_pattern_matches(pattern, command)
                {
                    return ExecShellPolicyDecision::Allow;
                }
            }
        }
        ExecShellPolicyDecision::AskUser("execpolicy: no matching allow rule".to_string())
    }
}

#[derive(Clone)]
struct ProductionExecShellHost {
    exec_policy: Option<ProductionExecPolicySnapshot>,
}

impl ExecShellHost for ProductionExecShellHost {
    fn evaluate_exec_policy(
        &self,
        command: &str,
    ) -> Result<Option<ExecShellPolicyDecision>, ToolError> {
        Ok(self
            .exec_policy
            .as_ref()
            .map(|policy| policy.evaluate(command)))
    }
}

/// Stable non-secret identity material for resume fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionToolExecutionIdentity {
    pub schema: u32,
    pub workspace: String,
    pub trust_mode: bool,
    pub trusted_external_paths: Vec<String>,
    pub follow_symlinks: bool,
    pub auto_approve: bool,
    pub shell_policy: ShellPolicy,
    pub elevated_sandbox_policy: Option<ExecutionSandboxPolicy>,
    pub sandbox_backend: Option<SandboxBackendIdentity>,
    pub prefer_external_pdftotext: bool,
    pub shell_network_denied_hint_sha256: Option<String>,
    pub exec_policy_sha256: Option<String>,
}

impl ProductionToolConfig {
    /// Create restrictive defaults for one workspace.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            trust_mode: false,
            trusted_external_paths: Vec::new(),
            follow_symlinks: false,
            auto_approve: false,
            shell_policy: ShellPolicy::None,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            sandbox_backend: None,
            prefer_external_pdftotext: false,
            exec_policy: None,
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
        rebound.trust_mode = false;
        rebound.trusted_external_paths.clear();
        rebound.follow_symlinks = false;
        rebound.auto_approve = true;
        rebound.elevated_sandbox_policy = Some(ExecutionSandboxPolicy::isolated_writer(workspace));
        rebound.shell_network_denied_hint = Some("隔离 Writer Agent 禁止访问网络".to_owned());
        rebound.sandbox_backend = None;
        rebound
    }

    #[must_use]
    pub fn with_trust_mode(mut self, trust_mode: bool) -> Self {
        self.trust_mode = trust_mode;
        self
    }

    #[must_use]
    pub fn with_trusted_external_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.trusted_external_paths = paths;
        self
    }

    #[must_use]
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    #[must_use]
    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.auto_approve = auto_approve;
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
    pub fn with_exec_policy(mut self, policy: Option<ProductionExecPolicySnapshot>) -> Self {
        self.exec_policy = policy;
        self
    }

    /// Project this config into deterministic, serializable and non-secret
    /// fingerprint material. Live executor state is intentionally absent.
    #[must_use]
    pub fn execution_identity(&self) -> ProductionToolExecutionIdentity {
        let mut trusted_external_paths = self
            .trusted_external_paths
            .iter()
            .map(|path| stable_path_identity(path))
            .collect::<Vec<_>>();
        trusted_external_paths.sort();
        trusted_external_paths.dedup();
        ProductionToolExecutionIdentity {
            schema: 1,
            workspace: stable_path_identity(&self.workspace),
            trust_mode: self.trust_mode,
            trusted_external_paths,
            follow_symlinks: self.follow_symlinks,
            auto_approve: self.auto_approve,
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

fn exec_policy_pattern_matches(pattern: &str, command: &str) -> bool {
    let pattern = normalize_exec_policy_command(pattern);
    let command = normalize_exec_policy_command(command);
    if pattern == "*" {
        return true;
    }
    let escaped = regex::escape(&pattern).replace("\\*", ".*");
    regex::Regex::new(&format!("^{escaped}$")).is_ok_and(|expression| expression.is_match(&command))
}

fn normalize_exec_policy_command(command: &str) -> String {
    let stripped = strip_exec_policy_heredoc_bodies(command);
    shlex::split(&stripped).map_or_else(
        || {
            stripped
                .split_whitespace()
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        },
        |tokens| tokens.join(" "),
    )
}

fn strip_exec_policy_heredoc_bodies(command: &str) -> String {
    if !command.contains("<<") {
        return command.to_string();
    }
    const HERESTRING_PLACEHOLDER: &str = "\u{0001}HERESTRING\u{0001}";
    let command = command.replace("<<<", HERESTRING_PLACEHOLDER);
    static HEREDOC_RE: OnceLock<regex::Regex> = OnceLock::new();
    let expression = HEREDOC_RE.get_or_init(|| {
        regex::Regex::new(r#"<<-?\s*(?:['"]?)([A-Za-z_][A-Za-z0-9_]*)(?:['"]?)"#)
            .expect("heredoc regex compiles")
    });
    let mut output = String::with_capacity(command.len());
    let mut lines = command.lines();
    while let Some(line) = lines.next() {
        let mut delimiter = None;
        let mut redacted = line.to_string();
        for captures in expression.captures_iter(line) {
            redacted = redacted.replace(captures.get(0).map_or("", |value| value.as_str()), "");
            delimiter = captures.get(1).map(|value| value.as_str().to_string());
        }
        output.push_str(
            &redacted
                .split_whitespace()
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        );
        output.push('\n');
        if let Some(delimiter) = delimiter {
            for body_line in lines.by_ref() {
                if body_line.trim() == delimiter {
                    break;
                }
            }
        }
    }
    output.replace(HERESTRING_PLACEHOLDER, "<<<")
}

/// Direct executor for the fixed eleven-tool production surface.
pub struct ProductionToolExecutor {
    context: ProductionToolContext,
    shell: ExecShellOptions,
    prefer_external_pdftotext: bool,
    shell_host: ProductionExecShellHost,
}

impl ProductionToolExecutor {
    /// Construct one isolated run executor. The concrete shell host preserves
    /// the resolved exec-policy snapshot; built-in command safety, shell
    /// policy, auto-approval posture and sandbox enforcement remain inside
    /// `execute_exec_shell`. A no-op host is therefore never used at the
    /// production cutover boundary.
    #[must_use]
    pub fn new(config: ProductionToolConfig) -> Self {
        let isolated_writer_workspace = match config.elevated_sandbox_policy.as_ref() {
            Some(ExecutionSandboxPolicy::IsolatedWriter { workspace }) => Some(workspace.clone()),
            _ => None,
        };
        let mut context = ProductionToolContext::new(config.workspace.clone())
            .with_trust_mode(config.trust_mode)
            .with_trusted_external_paths(config.trusted_external_paths)
            .with_follow_symlinks(config.follow_symlinks)
            .with_auto_approve(config.auto_approve);
        if let Some(workspace) = isolated_writer_workspace {
            context = context.with_isolated_writer_write_guard(workspace);
        }
        let mut shell = ExecShellOptions::new(
            new_shared_shell_manager(config.workspace),
            config.shell_policy,
        );
        shell.elevated_sandbox_policy = config.elevated_sandbox_policy;
        shell.shell_network_denied_hint = config.shell_network_denied_hint;
        shell.sandbox_backend = config.sandbox_backend;
        Self {
            context,
            shell,
            prefer_external_pdftotext: config.prefer_external_pdftotext,
            shell_host: ProductionExecShellHost {
                exec_policy: config.exec_policy,
            },
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
    fn execution_error_outcome(error: ToolError) -> ToolOutcome {
        match &error {
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
        }
    }

    async fn dispatch(
        &self,
        name: &str,
        input: Value,
        context: &ProductionToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        match name {
            "apply_patch" => execute_apply_patch(input, context),
            "edit_file" => execute_edit_file(input, context),
            "exec_shell" => execute_exec_shell(input, context, &self.shell, &self.shell_host).await,
            "file_search" => execute_file_search(input, context).await,
            "git_diff" => execute_git_diff(input, context),
            "git_status" => execute_git_status(input, context),
            "grep_files" => execute_grep_files(input, context).await,
            "list_dir" => execute_list_dir(input, context).await,
            "read_file" => execute_read_file(input, context, self.prefer_external_pdftotext),
            "run_tests" => execute_run_tests(input, context, &self.shell).await,
            "run_verifiers" => execute_run_verifiers(input, context, &self.shell).await,
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
            "file_search" | "git_diff" | "git_status" | "grep_files" | "list_dir" | "read_file" => {
                WorkspaceAccess::ReadOnly
            }
            _ => WorkspaceAccess::MayWrite,
        }
    }

    fn workspace_access(&self, invocation: &ToolInvocation) -> WorkspaceAccess {
        match invocation.name.as_str() {
            "file_search" | "git_diff" | "git_status" | "grep_files" | "list_dir" | "read_file" => {
                WorkspaceAccess::ReadOnly
            }
            "exec_shell"
                if invocation
                    .arguments
                    .parsed
                    .as_ref()
                    .is_some_and(exec_shell_input_is_parallel_readonly) =>
            {
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
            return Some(Self::preflight_error_outcome(ToolError::patch_parse(
                error.to_string(),
            )));
        }
        if invocation.name == "exec_shell" {
            return match preflight_exec_shell(input, &self.context, &self.shell, &self.shell_host) {
                Ok(outcome) => outcome,
                Err(error) => Some(Self::preflight_error_outcome(error)),
            };
        }
        None
    }

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        capture_workspace_revision(self.context.workspace())
            .await
            .map_err(|message| ToolExecutionError::new("workspace_revision_unavailable", message))
    }

    fn approval_prompt(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<Option<ToolApprovalPrompt>, ToolExecutionError> {
        if self.context.auto_approve() {
            return Ok(None);
        }
        let input = invocation.arguments.parsed.as_ref();
        let prompt = match invocation.name.as_str() {
            "apply_patch" | "edit_file" => Some(ToolApprovalPrompt {
                title: "确认修改工作区文件".to_owned(),
                description: format!(
                    "工具 {} 将修改当前工作区；确认后才会产生文件副作用。",
                    invocation.name
                ),
                risk: ApprovalRisk::Elevated,
            }),
            "exec_shell"
                if self.shell.shell_policy == ShellPolicy::Full
                    && input.is_some_and(|input| !exec_shell_input_is_parallel_readonly(input)) =>
            {
                Some(ToolApprovalPrompt {
                    title: "确认执行 Shell 命令".to_owned(),
                    description: "该命令可能修改文件、启动进程或访问外部资源。".to_owned(),
                    risk: input
                        .and_then(|input| input.get("command"))
                        .and_then(Value::as_str)
                        .filter(|command| command_is_high_impact(command))
                        .map_or(ApprovalRisk::Elevated, |_| ApprovalRisk::Critical),
                })
            }
            "run_tests" | "run_verifiers" if self.shell.shell_policy == ShellPolicy::Full => {
                Some(ToolApprovalPrompt {
                    title: "确认执行项目代码".to_owned(),
                    description: format!("工具 {} 会运行仓库中的命令或测试代码。", invocation.name),
                    risk: ApprovalRisk::Elevated,
                })
            }
            _ => None,
        };
        Ok(prompt)
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
        if !PRODUCTION_TOOL_NAMES.contains(&invocation.name.as_str()) {
            return Ok(Self::execution_error_outcome(ToolError::not_available(
                format!("工具 '{}' 不在固定生产目录中", invocation.name),
            )));
        }
        let Some(input) = invocation.arguments.parsed else {
            return Ok(Self::execution_error_outcome(ToolError::invalid_input(
                "JSON 参数格式错误",
            )));
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
        let execution = self.dispatch(&invocation.name, input, &context);
        tokio::pin!(execution);
        tokio::select! {
            result = &mut execution => Ok(match result {
                Ok(outcome) => outcome,
                Err(error) => Self::execution_error_outcome(error),
            }),
            () = cancellation.cancelled() => {
                tool_cancellation.cancel();
                match tokio::time::timeout(std::time::Duration::from_secs(5), &mut execution).await {
                    Ok(Ok(mut outcome)) => {
                        if !outcome.is_success() {
                            outcome.operation = ToolOperationStatus::Cancelled;
                        }
                        Ok(outcome)
                    },
                    Ok(Err(error)) => Ok(Self::execution_error_outcome(error)),
                    Err(_) => {
                        let mut outcome = ToolOutcome::recovery_ambiguous(format!(
                            "工具 '{}' 收到取消后未在 5 秒内停止",
                            invocation.name
                        ));
                        outcome.operation = ToolOperationStatus::Cancelled;
                        Ok(outcome)
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
            "用 unified diff 或完整文件内容原子修改一个或多个工作区文件；patch 与 changes 二选一，适合结构性、多处或跨文件变更。",
            apply_patch_schema(),
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
    json!({"type":"object","properties":{"path":{"type":"string"},"patch":{"type":"string"},"changes":{"type":"array","minItems":1,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}},"fuzz":{"type":"integer"},"create_if_missing":{"type":"boolean"}},"oneOf":[{"required":["patch"]},{"required":["changes"]}],"additionalProperties":false})
}

fn edit_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"search":{"type":"string"},"replace":{"type":"string"}},"required":["path","search","replace"],"additionalProperties":false})
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

fn read_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1},"pages":{"type":"string"}},"required":["path"],"additionalProperties":false})
}

fn run_tests_schema() -> Value {
    json!({"type":"object","properties":{"args":{"type":"array","items":{"type":"string"},"default":[]},"all_features":{"type":"boolean"}},"additionalProperties":false})
}

fn run_verifiers_schema() -> Value {
    json!({"type":"object","properties":{"profile":{"type":"string","enum":["auto","rust","node","python","go","exact"],"default":"auto"},"level":{"type":"string","enum":["quick","full"],"default":"quick"},"max_python_files":{"type":"integer","minimum":1,"maximum":1000,"default":200},"commands":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"},"default":[]},"cwd":{"type":"string"}},"required":["name","program"],"additionalProperties":false},"default":[]}},"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_protocol::agent_runtime::{RunId, ToolArguments, ToolInvocationStatus};

    fn invocation(name: &str, value: Value) -> ToolInvocation {
        ToolInvocation {
            run_id: RunId::from("run-1"),
            call_id: "call-1".to_string(),
            name: name.to_string(),
            arguments: ToolArguments::from_value(value),
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
            .with_trust_mode(true)
            .with_follow_symlinks(true)
            .with_auto_approve(true)
            .with_shell_policy(ShellPolicy::ReadOnly)
            .with_trusted_external_paths(vec![
                workspace.path().join("z"),
                workspace.path().join("a"),
                workspace.path().join("z"),
            ])
            .with_prefer_external_pdftotext(true)
            .execution_identity();
        let second = ProductionToolConfig::new(workspace.path())
            .with_trust_mode(true)
            .with_follow_symlinks(true)
            .with_auto_approve(true)
            .with_shell_policy(ShellPolicy::ReadOnly)
            .with_trusted_external_paths(vec![
                workspace.path().join("a"),
                workspace.path().join("z"),
            ])
            .with_prefer_external_pdftotext(true)
            .execution_identity();
        assert_eq!(first, second);
        assert_eq!(first.trusted_external_paths.len(), 2);
        let serialized = serde_json::to_string(&first).unwrap();
        for forbidden in ["cancel", "shell_manager", "read_tracker", "api_key"] {
            assert!(!serialized.contains(forbidden));
        }
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
    fn production_preflight_asks_only_for_real_write_or_code_execution() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path()).with_shell_policy(ShellPolicy::Full),
        );
        assert!(
            executor
                .approval_prompt(&invocation("apply_patch", json!({"patch": "x"})))
                .unwrap()
                .is_some()
        );
        assert!(
            executor
                .approval_prompt(&invocation("read_file", json!({"path": "src/lib.rs"})))
                .unwrap()
                .is_none()
        );
        assert!(
            executor
                .approval_prompt(&invocation(
                    "exec_shell",
                    json!({"command": "git status --short"})
                ))
                .unwrap()
                .is_none()
        );
        assert!(
            executor
                .approval_prompt(&invocation(
                    "exec_shell",
                    json!({"command": "touch changed"})
                ))
                .unwrap()
                .is_some()
        );
        assert!(
            executor
                .approval_prompt(&invocation("run_tests", json!({})))
                .unwrap()
                .is_some()
        );

        let automatic = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path())
                .with_shell_policy(ShellPolicy::Full)
                .with_auto_approve(true),
        );
        assert!(
            automatic
                .approval_prompt(&invocation(
                    "exec_shell",
                    json!({"command": "touch changed"})
                ))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn shell_preflight_reports_canonical_critical_and_elevated_risk() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(workspace.path()).with_shell_policy(ShellPolicy::Full),
        );

        for command in [
            "rm -rf /tmp/codewhale-critical-test",
            "git push origin main",
            "npm publish",
            "cargo publish",
            "gh release create v1.0.0",
            "git tag v1.0.0",
            "git tag --delete v1.0.0",
        ] {
            let prompt = executor
                .approval_prompt(&invocation("exec_shell", json!({"command": command})))
                .unwrap()
                .unwrap();
            assert_eq!(prompt.risk, ApprovalRisk::Critical, "{command}");
        }

        for command in ["cargo test --workspace", "touch changed", "git tag --list"] {
            let prompt = executor
                .approval_prompt(&invocation("exec_shell", json!({"command": command})))
                .unwrap()
                .unwrap();
            assert_eq!(prompt.risk, ApprovalRisk::Elevated, "{command}");
        }
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
            .with_trust_mode(true)
            .with_trusted_external_paths(vec![root.path().join("external")])
            .with_follow_symlinks(true)
            .with_auto_approve(false)
            .with_shell_policy(ShellPolicy::Full)
            .with_elevated_sandbox_policy(ExecutionSandboxPolicy::DangerFullAccess);
        let root_executor = ProductionToolExecutor::new(base.clone());
        let writer_config = base.rebind_isolated_writer_workspace(writer.path());
        let identity = writer_config.execution_identity();
        assert_eq!(
            identity.workspace,
            writer.path().canonicalize().unwrap().display().to_string()
        );
        assert!(!identity.trust_mode);
        assert!(identity.trusted_external_paths.is_empty());
        assert!(!identity.follow_symlinks);
        assert!(identity.auto_approve);
        assert_eq!(identity.sandbox_backend, None);
        assert!(matches!(
            identity.elevated_sandbox_policy,
            Some(ExecutionSandboxPolicy::IsolatedWriter { ref workspace })
                if workspace == writer.path()
        ));

        let writer_executor = ProductionToolExecutor::new(writer_config);
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
            &writer.join(".codewhale"),
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
        std::fs::write(writer.join(".codewhale/state"), "codewhale\n").unwrap();
        std::fs::write(writer.join(".deepseek/config"), "deepseek\n").unwrap();
        std::fs::write(writer.join("owned.txt"), "before\n").unwrap();

        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(&root).rebind_isolated_writer_workspace(&writer),
        );
        let forbidden = [
            writer.join(".git"),
            writer.join(".git/index"),
            writer.join("nested/../.codewhale/state"),
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
        for directory in [".git", ".codewhale", ".deepseek"] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        std::fs::write(root.path().join(".git/local"), "git-before\n").unwrap();
        std::fs::write(root.path().join(".codewhale/state"), "state-before\n").unwrap();
        std::fs::write(root.path().join(".deepseek/config"), "config-before\n").unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(root.path()));

        let patch = executor
            .execute(
                invocation(
                    "apply_patch",
                    json!({
                        "changes": [
                            {"path": ".git/local", "content": "git-after\n"},
                            {"path": ".codewhale/state", "content": "state-after\n"}
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
            std::fs::read(root.path().join(".codewhale/state")).unwrap(),
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
                .with_auto_approve(true)
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
    async fn exec_policy_deny_is_rejected_preflight_and_safe_after_start() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("blocked");
        let policy = ProductionExecPolicySnapshot {
            rules: BTreeMap::from([(
                "writes".to_string(),
                ProductionExecPolicyRuleSet {
                    allow: Vec::new(),
                    deny: vec!["touch *".to_string()],
                },
            )]),
        };
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(temp.path())
                .with_auto_approve(true)
                .with_shell_policy(ShellPolicy::Full)
                .with_exec_policy(Some(policy)),
        );
        let invocation = invocation(
            "exec_shell",
            json!({"command": format!("touch {}", marker.display())}),
        );
        let rejected = executor
            .preflight(&invocation)
            .expect("policy must reject before execution starts");
        assert_eq!(rejected.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(rejected.retry, ToolRetryDisposition::NotRetryable);
        assert!(!marker.exists());

        // Direct execution models a dynamic policy change after Runtime's
        // preflight. It must remain side-effect free without claiming the
        // impossible Rejected/NotStarted lifecycle after a start event.
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
