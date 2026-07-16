//! Production `exec_shell` operation.
//!
//! The TUI remains responsible for product-surface schema, approval prompts,
//! legacy exec-policy loading, and hook composition. Command execution and its
//! canonical structured outcome live here so every caller shares one process,
//! sandbox, cancellation, timeout, and output contract.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result as AnyResult, anyhow};
use codewhale_protocol::agent_runtime::{ToolRetryDisposition, ToolSideEffectStatus};
use serde_json::{Value, json};

use super::cargo_failure_summary::summarize_cargo_failure;
use super::output::{summarize_output, truncate_with_meta};
use super::{SharedShellManager, ShellJobOwner, ShellPolicy, ShellResult, ShellStatus};
use crate::command_safety::{
    SafetyLevel, analyze_command, extract_primary_command, is_parallel_readonly_command,
};
use crate::sandbox::SandboxPolicy as ExecutionSandboxPolicy;
use crate::sandbox::backend::SandboxBackend;
use crate::{
    ProductionToolContext, ToolError, ToolOutcome, optional_bool, optional_u64, required_str,
};

pub const FOREGROUND_TIMEOUT_RECOVERY_HINT: &str = "Foreground exec_shell is for bounded commands. \
The timed-out process was killed; rerun long work with task_shell_start or exec_shell with \
background: true, then poll with task_shell_wait or exec_shell_wait.";

const MACOS_PROVENANCE_HINT: &str = "Docker buildx failed to update its activity file due to a macOS \
com.apple.provenance restriction. Files created by Docker Desktop's signed process carry a \
kernel-enforced provenance tag that blocks writes from child processes (including the TUI \
shell sandbox). Workarounds: (1) run the Docker build from a regular terminal outside the \
TUI, or (2) disable BuildKit with DOCKER_BUILDKIT=0 (only works if your Dockerfiles do not \
use RUN --mount directives).";

const PYTHON_BUILD_DEPENDENCY_HINT: &str = "Python build dependency missing: setuptools is not \
available in the active environment. Install the declared build requirements first, for example \
`python -m pip install -U pip setuptools wheel build`, then rerun the build command.";

/// Host-owned policy result. The operation records AskUser as metadata because
/// interactive approval has already happened before a ToolSpec is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecShellPolicyDecision {
    Allow,
    Deny(String),
    AskUser(String),
}

/// Narrow composition port for behavior that intentionally remains outside
/// the production tools crate.
pub trait ExecShellHost: Send + Sync {
    /// Evaluate the optional product exec policy after input-shape validation
    /// and before any filesystem or process side effect.
    fn evaluate_exec_policy(
        &self,
        _command: &str,
    ) -> Result<Option<ExecShellPolicyDecision>, ToolError> {
        Ok(None)
    }

    /// Collect configured environment-hook values immediately before spawn.
    fn collect_shell_env(&self, _input: &Value) -> HashMap<String, String> {
        HashMap::new()
    }
}

/// Default host for direct production-operation tests and callers without
/// optional exec-policy or environment hooks.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopExecShellHost;

impl ExecShellHost for NoopExecShellHost {}

/// Runtime state required by the shell operation but not shared by ordinary
/// file tools.
#[derive(Clone)]
pub struct ExecShellOptions {
    pub shell_manager: SharedShellManager,
    pub shell_policy: ShellPolicy,
    pub elevated_sandbox_policy: Option<ExecutionSandboxPolicy>,
    pub shell_network_denied_hint: Option<String>,
    pub sandbox_backend: Option<Arc<dyn SandboxBackend>>,
    pub owner: Option<ShellJobOwner>,
    pub active_task_id: Option<String>,
}

impl ExecShellOptions {
    #[must_use]
    pub fn new(shell_manager: SharedShellManager, shell_policy: ShellPolicy) -> Self {
        Self {
            shell_manager,
            shell_policy,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            sandbox_backend: None,
            owner: None,
            active_task_id: None,
        }
    }
}

/// Human-readable exit status for a shell result.
#[must_use]
pub fn exit_code_label(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".to_string(),
    }
}

pub fn attach_cargo_failure_summary(metadata: &mut Value, command: &str, result: &ShellResult) {
    if let Some(summary) =
        summarize_cargo_failure(command, &result.stdout, &result.stderr, result.exit_code)
    {
        metadata["cargo_failure_summary"] = summary.to_metadata_value();
    }
}

pub fn attach_python_build_dependency_hint(metadata: &mut Value, hint: Option<&'static str>) {
    if let Some(hint) = hint {
        metadata["python_build_dependency_hint"] = json!({
            "kind": "missing_setuptools",
            "hint": hint,
            "recommended_first_step": "python -m pip install -U pip setuptools wheel build",
        });
    }
}

#[must_use]
pub fn looks_like_macos_provenance_failure(result: &ShellResult) -> bool {
    if matches!(result.status, ShellStatus::Completed) && result.exit_code == Some(0) {
        return false;
    }
    let combined = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    combined.contains("com.apple.provenance")
        || combined.contains("update builder last activity")
        || (combined.contains("buildx/activity") && combined.contains("operation not permitted"))
}

#[must_use]
pub fn macos_provenance_hint(result: &ShellResult) -> Option<&'static str> {
    looks_like_macos_provenance_failure(result).then_some(MACOS_PROVENANCE_HINT)
}

#[must_use]
pub fn python_build_dependency_hint(command: &str, result: &ShellResult) -> Option<&'static str> {
    if matches!(result.status, ShellStatus::Completed) && result.exit_code == Some(0) {
        return None;
    }

    let command = command.to_ascii_lowercase();
    let combined = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    let mentions_missing_setuptools = [
        "no module named 'setuptools'",
        "no module named \"setuptools\"",
        "setuptools is not available",
        "cannot import 'setuptools",
        "cannot import \"setuptools",
        "missing dependencies",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
        && combined.contains("setuptools");
    if !mentions_missing_setuptools {
        return None;
    }

    let pythonish_command = [
        "python",
        "pip",
        "pytest",
        "tox",
        "nox",
        "cython",
        "setup.py",
        "build_ext",
    ]
    .iter()
    .any(|needle| command.contains(needle));
    let pythonish_output = [
        "setup.py",
        "pyproject.toml",
        "build_meta",
        "build_ext",
        "pep 517",
        "cython",
    ]
    .iter()
    .any(|needle| combined.contains(needle));

    (pythonish_command || pythonish_output).then_some(PYTHON_BUILD_DEPENDENCY_HINT)
}

fn command_likely_needs_network(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    let Some(primary) = extract_primary_command(&normalized) else {
        return false;
    };
    let primary = primary.rsplit(['/', '\\']).next().unwrap_or(primary);

    match primary {
        "curl" | "wget" | "fetch" | "nc" | "netcat" | "ncat" | "ssh" | "scp" | "sftp" | "rsync"
        | "ftp" | "ping" | "traceroute" | "nslookup" | "dig" | "host" | "nmap" | "gh" | "hub" => {
            true
        }
        "git" => [
            " fetch",
            " pull",
            " clone",
            " ls-remote",
            " submodule",
            " push",
        ]
        .iter()
        .any(|needle| normalized.contains(needle)),
        "cargo" => [" install", " fetch", " update", " publish", " search"]
            .iter()
            .any(|needle| normalized.contains(needle)),
        "npm" | "pnpm" | "yarn" => [" install", " i", " add", " update", " publish"]
            .iter()
            .any(|needle| normalized.contains(needle)),
        "pip" | "pip3" | "uv" | "poetry" => [" install", " add", " sync", " update"]
            .iter()
            .any(|needle| normalized.contains(needle)),
        "brew" | "apt" | "apt-get" | "yum" | "dnf" | "pacman" => true,
        "go" => [" get", " install", " mod download"]
            .iter()
            .any(|needle| normalized.contains(needle)),
        _ => false,
    }
}

fn looks_like_network_blocked_failure(result: &ShellResult) -> bool {
    if matches!(result.status, ShellStatus::Completed | ShellStatus::Running)
        || result.exit_code == Some(0)
    {
        return false;
    }

    if result.stdout.trim() == "000" {
        return true;
    }
    if result.sandboxed && result.stdout.is_empty() && result.stderr.is_empty() {
        return true;
    }

    let output = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    [
        "operation not permitted",
        "network is unreachable",
        "could not resolve host",
        "couldn't resolve host",
        "failed to resolve",
        "temporary failure in name resolution",
        "name or service not known",
        "nodename nor servname provided",
        "no address associated",
        "failed to connect",
        "couldn't connect",
        "connection timed out",
        "connection reset",
    ]
    .iter()
    .any(|pattern| output.contains(pattern))
}

#[must_use]
pub fn shell_network_restricted_hint<'a>(
    hint: Option<&'a str>,
    policy: Option<&ExecutionSandboxPolicy>,
    command: &str,
    result: &ShellResult,
) -> Option<&'a str> {
    let hint = hint?;
    let policy_blocks_network = policy.is_some_and(|policy| !policy.has_network_access());
    if !policy_blocks_network || !command_likely_needs_network(command) {
        return None;
    }
    (result.sandbox_denied || looks_like_network_blocked_failure(result)).then_some(hint)
}

pub fn attach_shell_owner_metadata(metadata: &mut Value, owner: Option<&ShellJobOwner>) {
    let Some(owner) = owner else {
        return;
    };
    metadata["owner_agent_id"] = json!(owner.agent_id);
    metadata["owner_agent_name"] = json!(owner.agent_name);
}

#[must_use]
pub fn exec_shell_input_is_parallel_readonly(input: &Value) -> bool {
    let Some(command) = input.get("command").and_then(Value::as_str) else {
        return false;
    };
    if ["background", "interactive", "tty", "combined_output"]
        .iter()
        .any(|key| input.get(*key).and_then(Value::as_bool) == Some(true))
    {
        return false;
    }
    if ["stdin", "input", "data"]
        .iter()
        .any(|key| input.get(*key).is_some())
    {
        return false;
    }

    is_parallel_readonly_command(command)
}

#[must_use]
pub fn exec_shell_input_starts_detached(input: &Value) -> bool {
    input.get("command").and_then(Value::as_str).is_some()
        && input.get("interactive").and_then(Value::as_bool) != Some(true)
        && (input.get("background").and_then(Value::as_bool) == Some(true)
            || input.get("tty").and_then(Value::as_bool) == Some(true))
}

struct ForegroundShellRequest<'a> {
    command: &'a str,
    working_dir: Option<&'a str>,
    timeout_ms: u64,
    stdin_data: Option<&'a str>,
    tty: bool,
    policy_override: Option<ExecutionSandboxPolicy>,
    extra_env: HashMap<String, String>,
}

async fn execute_foreground_via_background(
    context: &ProductionToolContext,
    options: &ExecShellOptions,
    request: ForegroundShellRequest<'_>,
) -> AnyResult<ShellResult> {
    let timeout_ms = request.timeout_ms.clamp(1000, 600_000);
    if context
        .cancellation_token()
        .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    {
        return Err(anyhow!("foreground command canceled before start"));
    }
    let spawned = {
        let mut manager = options
            .shell_manager
            .lock()
            .map_err(|_| anyhow!("shell manager lock poisoned"))?;
        manager.clear_foreground_background_request();
        manager.execute_with_options_env(
            request.command,
            request.working_dir,
            timeout_ms,
            true,
            request.stdin_data,
            request.tty,
            request.policy_override,
            request.extra_env,
        )?
    };
    wait_for_managed_foreground(
        context,
        &options.shell_manager,
        spawned,
        timeout_ms,
        true,
        true,
    )
    .await
}

/// Execute one directly addressed program through the same process owner as
/// shell commands. Test/verifier tools may use this lifecycle primitive
/// without acquiring a second process implementation.
#[allow(clippy::too_many_arguments)]
pub async fn execute_managed_program(
    context: &ProductionToolContext,
    shell_manager: &SharedShellManager,
    owner: Option<ShellJobOwner>,
    display_command: &str,
    program: &str,
    args: &[String],
    working_dir: &Path,
    timeout_ms: u64,
    policy_override: Option<ExecutionSandboxPolicy>,
    extra_env: HashMap<String, String>,
) -> AnyResult<ShellResult> {
    let timeout_ms = timeout_ms.clamp(1_000, 600_000);
    if context
        .cancellation_token()
        .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    {
        return Err(anyhow!("managed verifier command canceled before start"));
    }

    let spawned = {
        let mut manager = shell_manager
            .lock()
            .map_err(|_| anyhow!("shell manager lock poisoned"))?;
        manager.clear_foreground_background_request();
        manager.spawn_managed_program(
            display_command,
            program,
            args,
            working_dir,
            timeout_ms,
            policy_override,
            extra_env,
            owner,
        )?
    };
    wait_for_managed_foreground(context, shell_manager, spawned, timeout_ms, true, false).await
}

async fn wait_for_managed_foreground(
    context: &ProductionToolContext,
    shell_manager: &SharedShellManager,
    spawned: ShellResult,
    timeout_ms: u64,
    close_stdin: bool,
    allow_background_request: bool,
) -> AnyResult<ShellResult> {
    let task_id = spawned
        .task_id
        .ok_or_else(|| anyhow!("foreground shell did not return a process id"))?;

    if close_stdin {
        let mut manager = shell_manager
            .lock()
            .map_err(|_| anyhow!("shell manager lock poisoned"))?;
        manager.write_stdin(&task_id, "", true)?;
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if context
            .cancellation_token()
            .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
        {
            let mut manager = shell_manager
                .lock()
                .map_err(|_| anyhow!("shell manager lock poisoned"))?;
            return manager.kill(&task_id);
        }

        let snapshot = {
            let mut manager = shell_manager
                .lock()
                .map_err(|_| anyhow!("shell manager lock poisoned"))?;
            let background_requested = manager.take_foreground_background_request();
            if allow_background_request && background_requested {
                return manager.get_output(&task_id, false, 0);
            }
            manager.get_output(&task_id, false, 0)?
        };

        if snapshot.status != ShellStatus::Running {
            return Ok(snapshot);
        }

        if Instant::now() >= deadline {
            let mut manager = shell_manager
                .lock()
                .map_err(|_| anyhow!("shell manager lock poisoned"))?;
            let mut result = manager.kill(&task_id)?;
            result.status = ShellStatus::TimedOut;
            return Ok(result);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Execute `exec_shell` and return the canonical tool outcome.
pub async fn execute_exec_shell(
    input: Value,
    context: &ProductionToolContext,
    options: &ExecShellOptions,
    host: &dyn ExecShellHost,
) -> Result<ToolOutcome, ToolError> {
    let command = required_str(&input, "command")?;
    match options.shell_policy {
        ShellPolicy::None => {
            return Ok(ToolOutcome::rejected(
                "Shell tools are disabled by the active permission profile.",
                ToolRetryDisposition::NotRetryable,
            ));
        }
        ShellPolicy::ReadOnly if !exec_shell_input_is_parallel_readonly(&input) => {
            return Ok(ToolOutcome::rejected(
                "Shell command blocked by read-only shell policy. Use a non-mutating, non-background inspection command, or switch to Act mode (`/mode act`) for write-capable shell work.",
                ToolRetryDisposition::NotRetryable,
            ));
        }
        ShellPolicy::ReadOnly | ShellPolicy::Full => {}
    }
    let timeout_ms = optional_u64(&input, "timeout_ms", 120_000).min(600_000);
    let background = optional_bool(&input, "background", false);
    let interactive = optional_bool(&input, "interactive", false);
    let combined_output = optional_bool(&input, "combined_output", false);
    let tty = optional_bool(&input, "tty", false) || (combined_output && background);
    let stdin_data = input
        .get("stdin")
        .or_else(|| input.get("input"))
        .or_else(|| input.get("data"))
        .and_then(Value::as_str)
        .map(str::to_string);

    if interactive && background {
        return Ok(ToolOutcome::rejected(
            "Interactive commands cannot run in background mode.",
            ToolRetryDisposition::AfterCorrection,
        ));
    }
    if interactive && (tty || combined_output) {
        return Ok(ToolOutcome::rejected(
            "Interactive mode cannot be combined with TTY or combined_output sessions.",
            ToolRetryDisposition::AfterCorrection,
        ));
    }
    if interactive && stdin_data.is_some() {
        return Ok(ToolOutcome::rejected(
            "Interactive mode cannot be combined with stdin data.",
            ToolRetryDisposition::AfterCorrection,
        ));
    }

    let background = background || tty;
    let execpolicy_decision = host.evaluate_exec_policy(command)?;
    if let Some(ExecShellPolicyDecision::Deny(reason)) = execpolicy_decision.as_ref() {
        return Ok(ToolOutcome::rejected(
            format!("BLOCKED: {reason}"),
            ToolRetryDisposition::NotRetryable,
        )
        .with_metadata(json!({
            "execpolicy": {
                "decision": "deny",
                "reason": reason,
            }
        })));
    }

    let safety = analyze_command(command);
    if !context.auto_approve() && safety.level == SafetyLevel::Dangerous {
        let reasons = safety.reasons.join("; ");
        let suggestions = if safety.suggestions.is_empty() {
            String::new()
        } else {
            format!("\nSuggestions: {}", safety.suggestions.join("; "))
        };
        return Ok(ToolOutcome::rejected(
            format!(
                "BLOCKED: This command was blocked for safety reasons.\n\nReasons: {reasons}{suggestions}\n\nNote: allow_shell=true exposes shell tools, but it does not disable built-in shell safety validation."
            ),
            ToolRetryDisposition::AfterCorrection,
        )
        .with_metadata(json!({
            "safety_level": "dangerous",
            "blocked": true,
            "reasons": safety.reasons,
            "suggestions": safety.suggestions,
        })));
    }

    let policy_override = options.elevated_sandbox_policy.clone();
    let working_dir = match input
        .get("cwd")
        .or_else(|| input.get("working_dir"))
        .and_then(Value::as_str)
    {
        Some(dir) => {
            let resolved = context.resolve_path(dir)?;
            Some(resolved.to_string_lossy().to_string())
        }
        None => None,
    };
    let extra_env = host.collect_shell_env(&input);

    if let Some(backend) = &options.sandbox_backend {
        if interactive {
            return Ok(ToolOutcome::rejected(
                "Interactive mode is not supported with external sandbox backends.",
                ToolRetryDisposition::AfterCorrection,
            ));
        }
        if background {
            return Ok(ToolOutcome::rejected(
                "Background mode is not supported with external sandbox backends.",
                ToolRetryDisposition::AfterCorrection,
            ));
        }
        if tty {
            return Ok(ToolOutcome::rejected(
                "TTY mode is not supported with external sandbox backends.",
                ToolRetryDisposition::AfterCorrection,
            ));
        }

        let started = Instant::now();
        let result = match backend.exec(command, &extra_env).await {
            Ok(output) => {
                let (stdout, stdout_meta) = truncate_with_meta(&output.stdout);
                let (stderr, stderr_meta) = truncate_with_meta(&output.stderr);
                ShellResult {
                    task_id: None,
                    status: if output.exit_code == 0 {
                        ShellStatus::Completed
                    } else {
                        ShellStatus::Failed
                    },
                    exit_code: Some(output.exit_code),
                    stdout,
                    stderr,
                    duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    stdout_len: stdout_meta.original_len,
                    stderr_len: stderr_meta.original_len,
                    stdout_omitted: stdout_meta.omitted,
                    stderr_omitted: stderr_meta.omitted,
                    stdout_truncated: stdout_meta.truncated,
                    stderr_truncated: stderr_meta.truncated,
                    sandboxed: true,
                    sandbox_type: Some("opensandbox".to_string()),
                    sandbox_denied: false,
                }
            }
            Err(error) => {
                return Ok(ToolOutcome::error(format!(
                    "Sandbox backend error: {error}"
                )));
            }
        };

        let stdout_summary = summarize_output(&result.stdout);
        let stderr_summary = summarize_output(&result.stderr);
        let summary = if stderr_summary.is_empty() {
            stdout_summary.clone()
        } else {
            stderr_summary.clone()
        };
        let python_dependency_hint = python_build_dependency_hint(command, &result);
        let mut output = if result.stdout.is_empty() && result.stderr.is_empty() {
            "(no output)".to_string()
        } else if result.stderr.is_empty() {
            result.stdout.clone()
        } else {
            format!("{}\n\nSTDERR:\n{}", result.stdout, result.stderr)
        };
        if let Some(hint) = python_dependency_hint {
            output = format!("{hint}\n\n{output}");
        }

        let mut metadata = json!({
            "exit_code": result.exit_code,
            "status": format!("{:?}", result.status),
            "duration_ms": result.duration_ms,
            "sandboxed": true,
            "sandbox_type": "opensandbox",
            "sandbox_denied": false,
            "task_id": result.task_id,
            "stdout_len": result.stdout_len,
            "stderr_len": result.stderr_len,
            "stdout_truncated": result.stdout_truncated,
            "stderr_truncated": result.stderr_truncated,
            "stdout_omitted": result.stdout_omitted,
            "stderr_omitted": result.stderr_omitted,
            "summary": summary,
            "stdout_summary": stdout_summary,
            "stderr_summary": stderr_summary,
            "safety_level": format!("{:?}", safety.level),
            "interactive": false,
            "canceled": false,
            "sandbox_backend": "opensandbox",
        });
        attach_shell_owner_metadata(&mut metadata, options.owner.as_ref());
        attach_cargo_failure_summary(&mut metadata, command, &result);
        attach_python_build_dependency_hint(&mut metadata, python_dependency_hint);

        let outcome = if result.status == ShellStatus::Completed {
            ToolOutcome::success(output)
        } else {
            ToolOutcome::error(output)
        }
        .with_side_effect(ToolSideEffectStatus::Indeterminate);
        return Ok(outcome.with_metadata(metadata));
    }

    let result = if interactive {
        let mut manager = options
            .shell_manager
            .lock()
            .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
        manager.execute_interactive_with_policy_env(
            command,
            working_dir.as_deref(),
            timeout_ms,
            policy_override,
            extra_env,
        )
    } else if background {
        let mut manager = options
            .shell_manager
            .lock()
            .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
        manager.execute_with_options_env_for_owner(
            command,
            working_dir.as_deref(),
            timeout_ms,
            true,
            stdin_data.as_deref(),
            tty,
            policy_override,
            extra_env,
            options.owner.clone(),
        )
    } else {
        execute_foreground_via_background(
            context,
            options,
            ForegroundShellRequest {
                command,
                working_dir: working_dir.as_deref(),
                timeout_ms,
                stdin_data: stdin_data.as_deref(),
                tty: combined_output,
                policy_override,
                extra_env,
            },
        )
        .await
    };

    match result {
        Ok(result) => {
            let backgrounded_foreground =
                !background && !interactive && result.status == ShellStatus::Running;
            if (background || backgrounded_foreground)
                && let (Some(shell_id), Some(task_id)) =
                    (result.task_id.as_deref(), options.active_task_id.clone())
                && let Ok(mut manager) = options.shell_manager.lock()
            {
                let _ = manager.tag_linked_task(shell_id, Some(task_id));
            }

            let was_cancelled = context
                .cancellation_token()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled);
            let task_id_str = result.task_id.clone().unwrap_or_default();
            let stdout_summary = summarize_output(&result.stdout);
            let stderr_summary = summarize_output(&result.stderr);
            let summary = if stderr_summary.is_empty() {
                stdout_summary.clone()
            } else {
                stderr_summary.clone()
            };
            let network_restricted_hint = shell_network_restricted_hint(
                options.shell_network_denied_hint.as_deref(),
                options.elevated_sandbox_policy.as_ref(),
                command,
                &result,
            )
            .map(str::to_string);
            let provenance_hint = macos_provenance_hint(&result);
            let python_dependency_hint = python_build_dependency_hint(command, &result);
            let mut output = if interactive {
                format!(
                    "Interactive command completed (exit code: {:?})",
                    result.exit_code
                )
            } else if result.status == ShellStatus::Completed {
                if result.stdout.is_empty() && result.stderr.is_empty() {
                    "(no output)".to_string()
                } else if result.stderr.is_empty() {
                    result.stdout.clone()
                } else {
                    format!("{}\n\nSTDERR:\n{}", result.stdout, result.stderr)
                }
            } else if result.status == ShellStatus::Running {
                if backgrounded_foreground {
                    format!(
                        "Foreground shell wait moved to /jobs: {task_id_str}\n\nReturns immediately; completion is tracked in task/status state. Keep working; call exec_shell_wait only if you need early output, final output, or wait=true at a true dependency."
                    )
                } else {
                    format!(
                        "Background task started: {task_id_str}\n\nReturns immediately; completion is tracked in task/status state. Keep working; call exec_shell_wait only if you need early output, final output, or wait=true at a true dependency."
                    )
                }
            } else if result.status == ShellStatus::Killed && was_cancelled {
                format!(
                    "Command canceled; process killed.\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                    result.stdout, result.stderr
                )
            } else if result.status == ShellStatus::TimedOut {
                format!(
                    "Command timed out after {timeout_ms}ms; process killed.\n\n{FOREGROUND_TIMEOUT_RECOVERY_HINT}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                    result.stdout, result.stderr
                )
            } else {
                format!(
                    "Command failed ({})\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                    exit_code_label(result.exit_code),
                    result.stdout,
                    result.stderr
                )
            };
            if let Some(hint) = network_restricted_hint.as_deref() {
                output = format!("{hint}\n\n{output}");
            }
            if let Some(hint) = provenance_hint {
                output = format!("{hint}\n\n{output}");
            }
            if let Some(hint) = python_dependency_hint {
                output = format!("{hint}\n\n{output}");
            }

            let mut metadata = json!({
                "exit_code": result.exit_code,
                "status": format!("{:?}", result.status),
                "duration_ms": result.duration_ms,
                "sandboxed": result.sandboxed,
                "sandbox_type": result.sandbox_type,
                "sandbox_denied": result.sandbox_denied,
                "task_id": result.task_id,
                "stdout_len": result.stdout_len,
                "stderr_len": result.stderr_len,
                "stdout_truncated": result.stdout_truncated,
                "stderr_truncated": result.stderr_truncated,
                "stdout_omitted": result.stdout_omitted,
                "stderr_omitted": result.stderr_omitted,
                "summary": summary,
                "stdout_summary": stdout_summary,
                "stderr_summary": stderr_summary,
                "safety_level": format!("{:?}", safety.level),
                "interactive": interactive,
                "combined_output": combined_output,
                "canceled": was_cancelled,
                "execpolicy": execpolicy_decision.as_ref().map(|decision| match decision {
                    ExecShellPolicyDecision::Allow => json!({"decision": "allow"}),
                    ExecShellPolicyDecision::Deny(reason) => json!({
                        "decision": "deny",
                        "reason": reason,
                    }),
                    ExecShellPolicyDecision::AskUser(reason) => json!({
                        "decision": "ask_user",
                        "reason": reason,
                    }),
                }),
            });
            metadata["backgrounded"] = json!(background || backgrounded_foreground);
            if background || backgrounded_foreground {
                metadata["auto_resume_on_completion"] = json!(false);
                metadata["completion_surface"] = json!("task_status");
                metadata["background_policy"] = json!("nonblocking");
            }
            if result.status == ShellStatus::TimedOut && !background && !interactive {
                metadata["foreground_timeout_recovery"] = json!({
                    "process_killed": true,
                    "hint": FOREGROUND_TIMEOUT_RECOVERY_HINT,
                    "recommended_tools": [
                        "task_shell_start",
                        "task_shell_wait",
                        "exec_shell",
                        "exec_shell_wait"
                    ],
                    "exec_shell_background": true,
                    "poll_with": ["task_shell_wait", "exec_shell_wait"]
                });
            }
            if let Some(hint) = network_restricted_hint {
                metadata["sandbox_network_restricted"] = json!(true);
                metadata["sandbox_network_denied_hint"] = json!(hint);
            }
            if provenance_hint.is_some() {
                metadata["macos_provenance_restricted"] = json!(true);
            }
            attach_shell_owner_metadata(&mut metadata, options.owner.as_ref());
            attach_cargo_failure_summary(&mut metadata, command, &result);
            attach_python_build_dependency_hint(&mut metadata, python_dependency_hint);

            let outcome = if matches!(result.status, ShellStatus::Completed | ShellStatus::Running)
            {
                ToolOutcome::success(output)
            } else {
                ToolOutcome::error(output)
            }
            .with_side_effect(ToolSideEffectStatus::Indeterminate);
            Ok(outcome.with_metadata(metadata))
        }
        Err(error) => Ok(ToolOutcome::error(format!(
            "Shell execution failed: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use codewhale_protocol::agent_runtime::ToolSideEffectStatus;
    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::shell::new_shared_shell_manager;

    fn options(context: &ProductionToolContext, policy: ShellPolicy) -> ExecShellOptions {
        ExecShellOptions::new(
            new_shared_shell_manager(context.workspace().to_path_buf()),
            policy,
        )
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn characterization_preserves_bytes_boundaries_and_no_start_failures() {
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/exec_shell_characterization.json"))
                .expect("exec_shell characterization fixture");
        let workspace = tempdir().expect("workspace");
        fs::create_dir(workspace.path().join("nested")).expect("nested cwd");
        let context = ProductionToolContext::new(workspace.path().to_path_buf());
        let resolved_cwd = context.resolve_path("nested").expect("resolved nested cwd");

        let success = execute_exec_shell(
            json!({
                "command": "printf 'cwd='; pwd; printf 'warn\\n' >&2",
                "cwd": "nested"
            }),
            &context,
            &options(&context, ShellPolicy::Full),
            &NoopExecShellHost,
        )
        .await
        .expect("successful shell");
        assert_eq!(
            success.content,
            expected["success_content_template"]
                .as_str()
                .expect("success template")
                .replace("{cwd}", &resolved_cwd.display().to_string())
        );
        assert!(success.is_success());
        assert_eq!(success.side_effect, ToolSideEffectStatus::Indeterminate);
        let metadata = success.metadata.expect("success metadata");
        assert_eq!(metadata["exit_code"], json!(0));
        assert_eq!(metadata["status"], json!("Completed"));
        assert!(
            metadata["task_id"]
                .as_str()
                .is_some_and(|task_id| task_id.starts_with("shell_")),
            "foreground execution must remain owned by ShellManager"
        );
        assert_eq!(metadata["stdout_truncated"], json!(false));
        assert_eq!(metadata["stderr_truncated"], json!(false));
        assert_eq!(metadata["stdout_omitted"], json!(0));
        assert_eq!(metadata["stderr_omitted"], json!(0));
        assert_eq!(metadata["summary"], json!("warn"));
        assert_eq!(metadata["interactive"], json!(false));
        assert_eq!(metadata["combined_output"], json!(false));
        assert_eq!(metadata["canceled"], json!(false));
        assert_eq!(metadata["backgrounded"], json!(false));

        let readonly_marker = workspace.path().join("readonly-marker");
        let rejected = execute_exec_shell(
            json!({"command": "printf changed > readonly-marker"}),
            &context,
            &options(&context, ShellPolicy::ReadOnly),
            &NoopExecShellHost,
        )
        .await
        .expect("read-only rejection");
        assert_eq!(rejected.content, expected["readonly_error"]);
        assert_eq!(rejected.side_effect, ToolSideEffectStatus::NotApplied);
        assert!(!readonly_marker.exists());

        let escaped_marker = workspace.path().parent().unwrap().join(format!(
            "{}-escaped-shell-marker",
            workspace.path().file_name().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_file(&escaped_marker);
        let escaped = execute_exec_shell(
            json!({
                "command": format!("printf changed > '{}'", escaped_marker.display()),
                "cwd": ".."
            }),
            &context,
            &options(&context, ShellPolicy::Full),
            &NoopExecShellHost,
        )
        .await
        .expect_err("cwd escape must fail before spawn");
        assert!(matches!(escaped, ToolError::PathEscape { .. }));
        assert!(!escaped_marker.exists());

        let canceled_marker = workspace.path().join("canceled-before-start");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let canceled_context = context.for_invocation(cancellation);
        let canceled = execute_exec_shell(
            json!({"command": "printf changed > canceled-before-start"}),
            &canceled_context,
            &options(&canceled_context, ShellPolicy::Full),
            &NoopExecShellHost,
        )
        .await
        .expect("pre-canceled execution returns an outcome");
        assert_eq!(canceled.content, expected["cancel_before_start_error"]);
        assert!(!canceled.is_success());
        assert!(!canceled_marker.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_and_cancellation_kill_the_owned_process_before_later_side_effects() {
        let workspace = tempdir().expect("workspace");
        let context = ProductionToolContext::new(workspace.path().to_path_buf());
        let timeout_marker = workspace.path().join("timeout-marker");
        let timed_out = execute_exec_shell(
            json!({
                "command": "sleep 10; printf changed > timeout-marker",
                "timeout_ms": 1000
            }),
            &context,
            &options(&context, ShellPolicy::Full),
            &NoopExecShellHost,
        )
        .await
        .expect("timeout outcome");
        assert!(!timed_out.is_success());
        assert_eq!(timed_out.metadata.as_ref().unwrap()["status"], "TimedOut");
        assert!(!timeout_marker.exists());

        let cancellation = CancellationToken::new();
        let canceled_context = context.for_invocation(cancellation.clone());
        let cancel_options = options(&canceled_context, ShellPolicy::Full);
        let cancel = execute_exec_shell(
            json!({"command": "sleep 10; printf changed > canceled-marker"}),
            &canceled_context,
            &cancel_options,
            &NoopExecShellHost,
        );
        tokio::pin!(cancel);
        tokio::select! {
            result = &mut cancel => panic!("command completed before cancellation: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(250)) => cancellation.cancel(),
        }
        let canceled = cancel.await.expect("cancellation outcome");
        assert!(!canceled.is_success());
        assert_eq!(canceled.metadata.as_ref().unwrap()["status"], "Killed");
        assert_eq!(canceled.metadata.as_ref().unwrap()["canceled"], true);
        assert!(!workspace.path().join("canceled-marker").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn large_output_keeps_exact_byte_accounting_and_truncation_metadata() {
        let workspace = tempdir().expect("workspace");
        let context = ProductionToolContext::new(workspace.path().to_path_buf());
        let outcome = execute_exec_shell(
            json!({"command": "printf '%035000d' 0"}),
            &context,
            &options(&context, ShellPolicy::Full),
            &NoopExecShellHost,
        )
        .await
        .expect("large output");
        assert!(outcome.is_success());
        let metadata = outcome.metadata.expect("metadata");
        assert_eq!(metadata["stdout_len"], 35_000);
        assert_eq!(metadata["stdout_truncated"], true);
        assert!(metadata["stdout_omitted"].as_u64().unwrap() > 0);
        assert!(outcome.content.contains("[Output truncated:"));
        assert!(outcome.content.contains("[Output tail]"));
    }
}
