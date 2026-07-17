//! Fixed production tool catalog and direct runtime executor.
//!
//! This is the sole model-visible tool owner. It intentionally uses direct
//! exact-name dispatch instead of a second registry or handler abstraction.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    ApprovalRisk, ToolApprovalPrompt, ToolDefinition, ToolOperationStatus, ToolRetryDisposition,
    ToolSideEffectStatus,
};
use codewhale_runtime::{CancellationToken, ToolExecutionError, ToolExecutor, ToolInvocation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken as TokioCancellationToken;

use crate::sandbox::SandboxPolicy as ExecutionSandboxPolicy;
use crate::sandbox::backend::{SandboxBackend, SandboxBackendIdentity};
use crate::shell::{
    ExecShellHost, ExecShellOptions, ExecShellPolicyDecision, ShellPolicy,
    exec_shell_input_is_parallel_readonly, execute_exec_shell, new_shared_shell_manager,
};
use crate::{
    ProductionToolContext, ToolError, ToolOutcome, execute_apply_patch, execute_edit_file,
    execute_file_search, execute_git_diff, execute_git_status, execute_grep_files,
    execute_list_dir, execute_read_file, execute_run_tests, execute_run_verifiers,
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
        let context = ProductionToolContext::new(config.workspace.clone())
            .with_trust_mode(config.trust_mode)
            .with_trusted_external_paths(config.trusted_external_paths)
            .with_follow_symlinks(config.follow_symlinks)
            .with_auto_approve(config.auto_approve);
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

    fn unavailable(name: &str) -> ToolOutcome {
        ToolOutcome::rejected(
            format!("tool_not_available：工具 '{name}' 未在生产 AgentRuntime 工具目录中提供"),
            ToolRetryDisposition::NotRetryable,
        )
    }

    fn tool_error_outcome(error: ToolError) -> ToolOutcome {
        match &error {
            ToolError::InvalidInput { .. }
            | ToolError::MissingField { .. }
            | ToolError::PathEscape { .. } => {
                ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::AfterCorrection)
            }
            ToolError::NotAvailable { .. } | ToolError::PermissionDenied { .. } => {
                ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::NotRetryable)
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
        validate_input_shape(name, &input)?;
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
                    risk: ApprovalRisk::Elevated,
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
        if !PRODUCTION_TOOL_NAMES.contains(&invocation.name.as_str()) {
            return Ok(Self::unavailable(&invocation.name));
        }
        let Some(input) = invocation.arguments.parsed else {
            return Ok(ToolOutcome::rejected(
                format!(
                    "invalid_arguments：工具 '{}' 的 JSON 参数格式错误：{}",
                    invocation.name, invocation.arguments.raw
                ),
                ToolRetryDisposition::AfterCorrection,
            ));
        };
        if cancellation.is_cancelled() {
            let mut outcome = ToolOutcome::error(format!(
                "tool_cancelled：工具 '{}' 在执行前已取消",
                invocation.name
            ));
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
                Err(error) => Self::tool_error_outcome(error),
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
                    Ok(Err(error)) => Ok(Self::tool_error_outcome(error)),
                    Err(_) => {
                        let mut outcome = ToolOutcome::recovery_ambiguous(format!(
                            "tool_cancel_timeout：工具 '{}' 收到取消后未在 5 秒内停止",
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

/// SHA-256 of the exact serialized canonical catalog.
#[must_use]
pub fn production_tool_catalog_sha256() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let bytes = serde_json::to_vec(&production_tool_definitions())
            .expect("production tool definitions are serializable");
        let hex = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    })
}

fn definition(name: &str, description: &str, input_schema: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    }
}

fn validate_input_shape(name: &str, input: &Value) -> Result<(), ToolError> {
    let Some(object) = input.as_object() else {
        return Err(ToolError::invalid_input("tool input must be a JSON object"));
    };
    let allowed: &[&str] = match name {
        "apply_patch" => &["path", "patch", "changes", "fuzz", "create_if_missing"],
        "edit_file" => &["path", "search", "replace"],
        "exec_shell" => &["command", "timeout_ms", "cwd"],
        "file_search" => &["query", "path", "limit", "extensions", "exclude"],
        "git_diff" => &["path", "cached", "unified"],
        "git_status" | "list_dir" => &["path"],
        "grep_files" => &[
            "pattern",
            "path",
            "include",
            "exclude",
            "context_lines",
            "case_insensitive",
            "max_results",
        ],
        "read_file" => &["path", "start_line", "max_lines", "pages"],
        "run_tests" => &["args", "all_features"],
        "run_verifiers" => &["profile", "level", "max_python_files", "commands"],
        _ => unreachable!("unknown production tool validated: {name}"),
    };
    let unknown: Vec<_> = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(ToolError::invalid_input(format!(
            "tool '{name}' does not accept field(s): {}",
            unknown.join(", ")
        )));
    }
    Ok(())
}

fn apply_patch_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"patch":{"type":"string"},"changes":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}},"fuzz":{"type":"integer"},"create_if_missing":{"type":"boolean"}},"oneOf":[{"required":["patch"]},{"required":["changes"]}]})
}

fn edit_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"search":{"type":"string"},"replace":{"type":"string"}},"required":["path","search","replace"]})
}

fn exec_shell_schema() -> Value {
    json!({"type":"object","properties":{"command":{"type":"string"},"timeout_ms":{"type":"integer"},"cwd":{"type":"string"}},"required":["command"]})
}

fn file_search_schema() -> Value {
    json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer"},"extensions":{"type":"array","items":{"type":"string"}},"exclude":{"type":"array","items":{"type":"string"}}},"required":["query"]})
}

fn git_diff_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"cached":{"type":"boolean"},"unified":{"type":"integer","minimum":0,"maximum":50,"default":3}},"additionalProperties":false})
}

fn git_status_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false})
}

fn grep_files_schema() -> Value {
    json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"include":{"type":"array","items":{"type":"string"}},"exclude":{"type":"array","items":{"type":"string"}},"context_lines":{"type":"integer"},"case_insensitive":{"type":"boolean"},"max_results":{"type":"integer","minimum":1,"maximum":100}},"required":["pattern"]})
}

fn list_dir_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"}}})
}

fn read_file_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer"},"max_lines":{"type":"integer"},"pages":{"type":"string"}},"required":["path"]})
}

fn run_tests_schema() -> Value {
    json!({"type":"object","properties":{"args":{"type":"string"},"all_features":{"type":"boolean"}},"additionalProperties":false})
}

fn run_verifiers_schema() -> Value {
    json!({"type":"object","properties":{"profile":{"type":"string","enum":["auto","rust","node","python","go"],"default":"auto"},"level":{"type":"string","enum":["quick","full"],"default":"quick"},"max_python_files":{"type":"integer","minimum":1,"maximum":1000,"default":200},"commands":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"},"default":[]},"cwd":{"type":"string"}},"required":["name","program"],"additionalProperties":false},"default":[]}},"additionalProperties":false})
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
    fn catalog_hash_is_stable() {
        // Update only when the reviewed fixed catalog intentionally changes.
        assert_eq!(
            production_tool_catalog_sha256(),
            "sha256:eefa831960fb97eaab72d6fcf7c1d7f264ad8c942984066f402587e519a87b19"
        );
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

    #[tokio::test]
    async fn hidden_fields_are_typed_rejections() {
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
            ("run_verifiers", "background"),
        ] {
            let outcome = executor
                .execute(
                    invocation(tool, json!({field: true})),
                    CancellationToken::default(),
                )
                .await
                .unwrap();
            assert_eq!(
                outcome.invocation,
                ToolInvocationStatus::Rejected,
                "{tool}.{field}"
            );
            assert_eq!(outcome.retry, ToolRetryDisposition::AfterCorrection);
        }
    }

    #[tokio::test]
    async fn malformed_unknown_and_pre_cancel_are_normal_outcomes() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        let unknown = executor
            .execute(
                invocation("write_file", json!({})),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.invocation, ToolInvocationStatus::Rejected);

        let malformed = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "bad".to_string(),
                    name: "list_dir".to_string(),
                    arguments: ToolArguments::parse("{"),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.invocation, ToolInvocationStatus::Rejected);

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
    async fn every_fixed_name_has_direct_dispatch_and_aliases_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ProductionToolConfig::new(temp.path()));
        for name in PRODUCTION_TOOL_NAMES {
            let outcome = executor
                .execute(invocation(name, json!({})), CancellationToken::default())
                .await
                .unwrap();
            assert!(
                !serde_json::to_string(&outcome)
                    .unwrap()
                    .contains("tool_not_available"),
                "fixed tool {name} did not reach its direct implementation"
            );
        }

        for alias in ["write_file", "task_shell_start", "exec_shell_wait"] {
            let outcome = executor
                .execute(invocation(alias, json!({})), CancellationToken::default())
                .await
                .unwrap();
            assert!(
                serde_json::to_string(&outcome)
                    .unwrap()
                    .contains("tool_not_available"),
                "stale alias {alias} unexpectedly dispatched"
            );
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
        assert!(
            edit.content.contains("has not been read"),
            "{}",
            edit.content
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "before\n");
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
    async fn production_exec_policy_deny_survives_direct_executor_cutover() {
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
        let outcome = executor
            .execute(
                invocation(
                    "exec_shell",
                    json!({"command": format!("touch {}", marker.display())}),
                ),
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(outcome.retry, ToolRetryDisposition::NotRetryable);
        assert!(!marker.exists());
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["execpolicy"]["decision"],
            "deny"
        );
    }
}
