// === ToolSpec Implementations ===

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use codewhale_tools::sandbox::SandboxPolicy as ExecutionSandboxPolicy;
use codewhale_tools::shell::output::{summarize_output, truncate_with_meta};
use codewhale_tools::shell::{
    ExecShellHost, ExecShellOptions, ExecShellPolicyDecision, ShellDeltaResult, ShellJobOwner,
    ShellResult, ShellStatus, attach_cargo_failure_summary, attach_python_build_dependency_hint,
    exec_shell_input_is_parallel_readonly, exec_shell_input_starts_detached, exit_code_label,
    macos_provenance_hint, python_build_dependency_hint,
};
use serde_json::{Value, json};

use crate::execpolicy::{ExecPolicyDecision, load_default_policy};
use crate::features::Feature;
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    optional_bool, optional_u64, required_str,
};

fn shell_job_owner_from_context(context: &ToolContext) -> Option<ShellJobOwner> {
    let agent_id = context
        .owner_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let agent_name = context
        .owner_agent_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(agent_id);
    Some(ShellJobOwner {
        agent_id: agent_id.to_string(),
        agent_name: agent_name.to_string(),
    })
}

fn attach_shell_owner_metadata(metadata: &mut Value, context: &ToolContext) {
    codewhale_tools::shell::attach_shell_owner_metadata(
        metadata,
        shell_job_owner_from_context(context).as_ref(),
    );
}

fn shell_network_restricted_hint<'a>(
    context: &'a ToolContext,
    command: &str,
    result: &ShellResult,
) -> Option<&'a str> {
    codewhale_tools::shell::shell_network_restricted_hint(
        context.shell_network_denied_hint.as_deref(),
        context.elevated_sandbox_policy.as_ref(),
        command,
        result,
    )
}

fn exec_shell_options(context: &ToolContext) -> ExecShellOptions {
    let mut options = ExecShellOptions::new(context.shell_manager.clone(), context.shell_policy);
    options.elevated_sandbox_policy = context.elevated_sandbox_policy.clone();
    options.shell_network_denied_hint = context.shell_network_denied_hint.clone();
    options.sandbox_backend = context.sandbox_backend.clone();
    options.owner = shell_job_owner_from_context(context);
    options.active_task_id = context.runtime.active_task_id.clone();
    options
}

struct TuiExecShellHost<'a> {
    context: &'a ToolContext,
}

impl ExecShellHost for TuiExecShellHost<'_> {
    fn evaluate_exec_policy(
        &self,
        command: &str,
    ) -> Result<Option<ExecShellPolicyDecision>, ToolError> {
        if !self.context.features.enabled(Feature::ExecPolicy) {
            return Ok(None);
        }
        let Some(policy) = load_default_policy().map_err(|error| {
            ToolError::execution_failed(format!("execpolicy load failed: {error}"))
        })?
        else {
            return Ok(None);
        };
        Ok(Some(match policy.evaluate(command) {
            ExecPolicyDecision::Allow => ExecShellPolicyDecision::Allow,
            ExecPolicyDecision::Deny(reason) => ExecShellPolicyDecision::Deny(reason),
            ExecPolicyDecision::AskUser(reason) => ExecShellPolicyDecision::AskUser(reason),
        }))
    }

    fn collect_shell_env(&self, input: &Value) -> HashMap<String, String> {
        let Some(hook_executor) = &self.context.runtime.hook_executor else {
            return HashMap::new();
        };
        let hook_context = crate::hooks::HookContext::new()
            .with_tool_name("exec_shell")
            .with_tool_args(input);
        hook_executor.collect_shell_env(&hook_context)
    }
}

/// Retain the existing verifier/test ToolSpec boundary while delegating its
/// process lifecycle to the single tools-owned implementation.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_managed_program(
    context: &ToolContext,
    display_command: &str,
    program: &str,
    args: &[String],
    working_dir: &Path,
    timeout_ms: u64,
    policy_override: Option<ExecutionSandboxPolicy>,
    extra_env: HashMap<String, String>,
) -> Result<ShellResult> {
    codewhale_tools::shell::execute_managed_program(
        context.production_context(),
        &context.shell_manager,
        shell_job_owner_from_context(context),
        display_command,
        program,
        args,
        working_dir,
        timeout_ms,
        policy_override,
        extra_env,
    )
    .await
}
pub struct ExecShellTool;

#[async_trait]
impl ToolSpec for ExecShellTool {
    fn name(&self) -> &'static str {
        "exec_shell"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command in the workspace directory. Foreground mode is for bounded commands; use background=true or task_shell_start for work expected to take >5 seconds. Background jobs return immediately and report completion through task/status state instead of resuming the model."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000, max: 600000)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background and return task_id (default: false). Returns immediately; completion is tracked in task/status state. Prefer this for commands expected to take >5 seconds, including builds, test suites, servers, CI polling, sleep, or other long-running work. Use exec_shell_wait only when you need early output, final output, or a true dependency barrier."
                },
                "interactive": {
                    "type": "boolean",
                    "description": "Run interactively with terminal IO (default: false)"
                },
                "stdin": {
                    "type": "string",
                    "description": "Optional stdin data to send before waiting (non-interactive only)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the command"
                },
                "tty": {
                    "type": "boolean",
                    "description": "Allocate a pseudo-terminal for interactive programs (implies background)"
                },
                "combined_output": {
                    "type": "boolean",
                    "description": "Capture stdout and stderr as one chronological PTY stream (default false). In foreground mode, waits for completion; in background mode, implies tty."
                }
            },
            "required": ["command"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    fn approval_requirement_for(&self, input: &serde_json::Value) -> ApprovalRequirement {
        if exec_shell_input_is_parallel_readonly(input) {
            ApprovalRequirement::Auto
        } else {
            self.approval_requirement()
        }
    }

    fn is_read_only_for(&self, input: &serde_json::Value) -> bool {
        exec_shell_input_is_parallel_readonly(input)
    }

    fn supports_parallel_for(&self, input: &serde_json::Value) -> bool {
        exec_shell_input_is_parallel_readonly(input)
    }

    fn starts_detached_for(&self, input: &serde_json::Value) -> bool {
        exec_shell_input_starts_detached(input)
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        codewhale_tools::shell::execute_exec_shell(
            input,
            context.production_context(),
            &exec_shell_options(context),
            &TuiExecShellHost { context },
        )
        .await
    }
}

pub struct ShellWaitTool {
    name: &'static str,
}

impl ShellWaitTool {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
}

pub struct ShellInteractTool {
    name: &'static str,
}

impl ShellInteractTool {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
}

fn required_task_id(input: &serde_json::Value) -> Result<&str, ToolError> {
    input
        .get("task_id")
        .or_else(|| input.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolError::missing_field("task_id"))
}

fn build_shell_delta_tool_result(delta: ShellDeltaResult, context: &ToolContext) -> ToolOutcome {
    let result = delta.result;
    let network_restricted_hint =
        shell_network_restricted_hint(context, &delta.command, &result).map(str::to_string);
    let provenance_hint = macos_provenance_hint(&result);
    let python_dependency_hint = python_build_dependency_hint(&delta.command, &result);
    let stdout_summary = summarize_output(&result.stdout);
    let stderr_summary = summarize_output(&result.stderr);
    let summary = if !stderr_summary.is_empty() {
        stderr_summary.clone()
    } else {
        stdout_summary.clone()
    };

    let mut output = if result.stdout.is_empty() && result.stderr.is_empty() {
        match result.status {
            ShellStatus::Running => "Background task running (no new output).".to_string(),
            ShellStatus::Completed => "(no new output)".to_string(),
            ShellStatus::Failed => {
                format!("Command failed ({})", exit_code_label(result.exit_code))
            }
            ShellStatus::TimedOut => "Command timed out (no new output).".to_string(),
            ShellStatus::Killed => "Command killed (no new output).".to_string(),
        }
    } else if result.stderr.is_empty() {
        result.stdout.clone()
    } else {
        format!("{}\n\nSTDERR:\n{}", result.stdout, result.stderr)
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
        "stdout_total_len": delta.stdout_total_len,
        "stderr_total_len": delta.stderr_total_len,
        "summary": summary,
        "stdout_summary": stdout_summary,
        "stderr_summary": stderr_summary,
        "command": delta.command,
        "stream_delta": true,
    });
    attach_shell_owner_metadata(&mut metadata, context);
    attach_cargo_failure_summary(&mut metadata, &delta.command, &result);
    attach_python_build_dependency_hint(&mut metadata, python_dependency_hint);

    let mut tool_result = if matches!(result.status, ShellStatus::Completed | ShellStatus::Running)
    {
        ToolOutcome::success(output)
    } else {
        ToolOutcome::error(output)
    }
    .with_metadata(metadata);
    if let Some(hint) = network_restricted_hint
        && let Some(metadata) = tool_result.metadata.as_mut()
        && let Some(object) = metadata.as_object_mut()
    {
        object.insert("sandbox_network_restricted".to_string(), json!(true));
        object.insert("sandbox_network_denied_hint".to_string(), json!(hint));
    }
    if provenance_hint.is_some()
        && let Some(metadata) = tool_result.metadata.as_mut()
        && let Some(object) = metadata.as_object_mut()
    {
        object.insert("macos_provenance_restricted".to_string(), json!(true));
    }
    tool_result
}

async fn wait_for_shell_delta_cancellable(
    context: &ToolContext,
    task_id: &str,
    timeout_ms: u64,
) -> Result<(ShellDeltaResult, bool), ToolError> {
    let timeout_ms = timeout_ms.clamp(1000, 600_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut stdout_accum = String::new();
    let mut stderr_accum = String::new();

    let (command, result, stdout_total_len, stderr_total_len) = loop {
        if context
            .cancellation_token()
            .is_some_and(|token| token.is_cancelled())
        {
            let mut manager = context
                .shell_manager
                .lock()
                .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
            let delta = manager
                .get_output_delta(task_id, false, 0)
                .map_err(|err| ToolError::execution_failed(err.to_string()))?;
            append_shell_delta_output(&mut stdout_accum, &mut stderr_accum, &delta.result);
            return Ok((
                shell_delta_with_accumulated_output(
                    delta.command,
                    delta.result,
                    &stdout_accum,
                    &stderr_accum,
                    delta.stdout_total_len,
                    delta.stderr_total_len,
                ),
                true,
            ));
        }

        let delta = {
            let mut manager = context
                .shell_manager
                .lock()
                .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
            manager
                .get_output_delta(task_id, false, 0)
                .map_err(|err| ToolError::execution_failed(err.to_string()))?
        };

        let stdout_total_len = delta.stdout_total_len;
        let stderr_total_len = delta.stderr_total_len;
        let command = delta.command.clone();
        append_shell_delta_output(&mut stdout_accum, &mut stderr_accum, &delta.result);

        let status = delta.result.status.clone();
        if status != ShellStatus::Running || Instant::now() >= deadline {
            break (command, delta.result, stdout_total_len, stderr_total_len);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    Ok((
        shell_delta_with_accumulated_output(
            command,
            result,
            &stdout_accum,
            &stderr_accum,
            stdout_total_len,
            stderr_total_len,
        ),
        false,
    ))
}

fn append_shell_delta_output(
    stdout_accum: &mut String,
    stderr_accum: &mut String,
    result: &ShellResult,
) {
    if !result.stdout.is_empty() {
        stdout_accum.push_str(&result.stdout);
    }
    if !result.stderr.is_empty() {
        stderr_accum.push_str(&result.stderr);
    }
}

fn shell_delta_with_accumulated_output(
    command: String,
    mut result: ShellResult,
    stdout_accum: &str,
    stderr_accum: &str,
    stdout_total_len: usize,
    stderr_total_len: usize,
) -> ShellDeltaResult {
    let (stdout, stdout_meta) = truncate_with_meta(stdout_accum);
    let (stderr, stderr_meta) = truncate_with_meta(stderr_accum);
    result.stdout = stdout;
    result.stderr = stderr;
    result.stdout_len = stdout_meta.original_len;
    result.stderr_len = stderr_meta.original_len;
    result.stdout_omitted = stdout_meta.omitted;
    result.stderr_omitted = stderr_meta.omitted;
    result.stdout_truncated = stdout_meta.truncated;
    result.stderr_truncated = stderr_meta.truncated;

    ShellDeltaResult {
        command,
        result,
        stdout_total_len,
        stderr_total_len,
    }
}

pub struct ShellCancelTool;

#[async_trait]
impl ToolSpec for ShellCancelTool {
    fn name(&self) -> &'static str {
        "exec_shell_cancel"
    }

    fn description(&self) -> &'static str {
        "Cancel a running background shell task by task_id, or cancel all running background shell tasks with all=true."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID returned by exec_shell or task_shell_start"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for task_id"
                },
                "all": {
                    "type": "boolean",
                    "description": "Cancel all currently running background shell tasks"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::RequiresApproval]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let cancel_all = optional_bool(&input, "all", false);
        let mut manager = context
            .shell_manager
            .lock()
            .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;

        if cancel_all {
            let results = manager
                .kill_running()
                .map_err(|err| ToolError::execution_failed(err.to_string()))?;
            if results.is_empty() {
                return Ok(
                    ToolOutcome::success("No running background commands.").with_metadata(json!({
                        "status": "Noop",
                        "canceled": 0,
                        "task_ids": [],
                    })),
                );
            }

            let task_ids = results
                .iter()
                .filter_map(|result| result.task_id.clone())
                .collect::<Vec<_>>();
            return Ok(ToolOutcome::success(format!(
                "Canceled {} background command{}: {}",
                task_ids.len(),
                if task_ids.len() == 1 { "" } else { "s" },
                task_ids.join(", ")
            ))
            .with_metadata(json!({
                "status": "Killed",
                "canceled": task_ids.len(),
                "task_ids": task_ids,
            })));
        }

        let task_id = required_task_id(&input)?;
        let result = manager
            .kill(task_id)
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
        let task_id = result
            .task_id
            .clone()
            .unwrap_or_else(|| task_id.to_string());
        Ok(
            ToolOutcome::success(format!("Canceled background command: {task_id}")).with_metadata(
                json!({
                    "status": format!("{:?}", result.status),
                    "task_id": task_id,
                    "exit_code": result.exit_code,
                    "duration_ms": result.duration_ms,
                }),
            ),
        )
    }
}

#[async_trait]
impl ToolSpec for ShellWaitTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn model_visible(&self) -> bool {
        // `exec_wait` is a legacy alias; only `exec_shell_wait` is model-visible.
        self.name == "exec_shell_wait"
    }

    fn description(&self) -> &'static str {
        "Inspect a background shell task and return incremental output without blocking by default. Set wait=true only for a deliberate dependency barrier. Turn cancellation stops waiting but leaves the background task running."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID returned by exec_shell"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 30000, max: 600000). Use a higher value for long-running builds, CI watchers, and interactive commands that are expected to keep producing output."
                },
                "wait": {
                    "type": "boolean",
                    "default": false,
                    "description": "Snapshot the latest background output and return immediately (default). Background job completions are tracked in task/status state, so normally do not wait. Set wait=true only for a deliberate barrier at a true dependency or final gate."
                }
            },
            "required": ["task_id"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let task_id = required_task_id(&input)?;
        let wait = optional_bool(&input, "wait", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", 30_000);

        let (delta, wait_canceled) = if wait {
            wait_for_shell_delta_cancellable(context, task_id, timeout_ms).await?
        } else {
            let mut manager = context
                .shell_manager
                .lock()
                .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
            let delta = manager
                .get_output_delta(task_id, false, timeout_ms)
                .map_err(|err| ToolError::execution_failed(err.to_string()))?;
            (delta, false)
        };

        let status = delta.result.status.clone();
        let mut result = build_shell_delta_tool_result(delta, context);
        if wait_canceled {
            if matches!(status, ShellStatus::Running) {
                result.content = format!(
                    "Wait canceled; background shell task {task_id} is still running.\n\n{}",
                    result.content
                );
            }
            if let Some(metadata) = result.metadata.as_mut()
                && let Some(object) = metadata.as_object_mut()
            {
                object.insert("wait_canceled".to_string(), json!(true));
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl ToolSpec for ShellInteractTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn model_visible(&self) -> bool {
        // `exec_interact` is a legacy alias; only `exec_shell_interact` is model-visible.
        self.name == "exec_shell_interact"
    }

    fn description(&self) -> &'static str {
        "Send input to a background shell task and return incremental output."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID returned by exec_shell"
                },
                "input": {
                    "type": "string",
                    "description": "Input to send to the task's stdin"
                },
                "stdin": {
                    "type": "string",
                    "description": "Alias for input"
                },
                "data": {
                    "type": "string",
                    "description": "Alias for input"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Wait for output after sending input (default: 1000)"
                },
                "close_stdin": {
                    "type": "boolean",
                    "description": "Close stdin after sending input"
                }
            },
            "required": ["task_id"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let task_id = required_task_id(&input)?;
        let close_stdin = optional_bool(&input, "close_stdin", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", 1_000);
        let interaction_input = input
            .get("input")
            .or_else(|| input.get("stdin"))
            .or_else(|| input.get("data"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        {
            let mut manager = context
                .shell_manager
                .lock()
                .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
            if !interaction_input.is_empty() || close_stdin {
                manager
                    .write_stdin(task_id, interaction_input, close_stdin)
                    .map_err(|err| ToolError::execution_failed(err.to_string()))?;
            }
        }

        let mut elapsed = 0u64;
        loop {
            if context
                .cancellation_token()
                .is_some_and(|token| token.is_cancelled())
            {
                let mut manager = context
                    .shell_manager
                    .lock()
                    .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
                let delta = manager
                    .get_output_delta(task_id, false, 0)
                    .map_err(|err| ToolError::execution_failed(err.to_string()))?;
                let mut result = build_shell_delta_tool_result(delta, context);
                if let Some(metadata) = result.metadata.as_mut()
                    && let Some(object) = metadata.as_object_mut()
                {
                    object.insert("wait_canceled".to_string(), json!(true));
                }
                return Ok(result);
            }

            let delta = {
                let mut manager = context
                    .shell_manager
                    .lock()
                    .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
                manager
                    .get_output_delta(task_id, false, 0)
                    .map_err(|err| ToolError::execution_failed(err.to_string()))?
            };

            if !delta.result.stdout.is_empty()
                || !delta.result.stderr.is_empty()
                || delta.result.status != ShellStatus::Running
                || elapsed >= timeout_ms
            {
                return Ok(build_shell_delta_tool_result(delta, context));
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
            elapsed = elapsed.saturating_add(50);
        }
    }
}

/// Tool for appending notes to a notes file.
pub struct NoteTool;

#[async_trait]
impl ToolSpec for NoteTool {
    fn name(&self) -> &'static str {
        "note"
    }

    fn description(&self) -> &'static str {
        "Append a note to the agent notes file for persistent context across sessions."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The note content to append"
                }
            },
            "required": ["content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto // Notes are low-risk
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let note_content = required_str(&input, "content")?;

        // Ensure parent directory exists
        if let Some(parent) = context.notes_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolError::execution_failed(format!("Failed to create notes directory: {e}"))
            })?;
        }

        // Append to notes file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&context.notes_path)
            .map_err(|e| ToolError::execution_failed(format!("Failed to open notes file: {e}")))?;

        writeln!(file, "\n---\n{note_content}")
            .map_err(|e| ToolError::execution_failed(format!("Failed to write note: {e}")))?;

        Ok(ToolOutcome::success(format!(
            "Note appended to {}",
            context.notes_path.display()
        )))
    }
}

#[cfg(test)]
mod tests;
