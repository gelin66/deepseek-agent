//! Cargo test runner tool: `run_tests`.
//!
//! `cargo test` runs workspace code, so this tool follows the same explicit
//! approval policy as the other code-executing tools.

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    optional_bool, optional_str,
};
use codewhale_tools::shell::cargo_failure_summary::summarize_cargo_failure;

use crate::dependencies::ExternalTool;
use crate::tools::shell::execute_managed_program;
use codewhale_tools::shell::{ShellResult, ShellStatus};

const MAX_OUTPUT_CHARS: usize = 40_000;
const RUN_TESTS_TIMEOUT_MS: u64 = 600_000;

/// Tool for running `cargo test` in the workspace root.
pub struct RunTestsTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunTestsOutput {
    success: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
    command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CargoTestEvidence {
    running: u64,
    result_lines: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    measured: u64,
    filtered_out: u64,
}

impl CargoTestEvidence {
    fn executed(&self) -> u64 {
        self.passed.saturating_add(self.failed)
    }
}

#[async_trait]
impl ToolSpec for RunTestsTool {
    fn name(&self) -> &'static str {
        "run_tests"
    }

    fn description(&self) -> &'static str {
        "Run `cargo test` in the workspace root with optional extra arguments."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "string",
                    "description": "Optional extra arguments to pass to `cargo test` (shell-style)."
                },
                "all_features": {
                    "type": "boolean",
                    "description": "When true, include `--all-features`."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ExecutesCode, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        // `run_tests` declares `ToolCapability::ExecutesCode` — match the
        // default approval policy for code-executing tools.
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let all_features = optional_bool(&input, "all_features", false);
        let extra_args = optional_str(&input, "args")
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut args = vec!["test".to_string()];
        if all_features {
            args.push("--all-features".to_string());
        }
        if let Some(extra) = extra_args {
            let split = shlex::split(extra).ok_or_else(|| {
                ToolError::invalid_input("Failed to parse 'args' as shell-style tokens")
            })?;
            args.extend(split);
        }

        let revision_before =
            crate::tools::goal::capture_workspace_revision(context.workspace()).await;
        let command_str = format_command(context.workspace(), &args);
        let output = run_cargo(context, &args, RUN_TESTS_TIMEOUT_MS).await?;

        let exit_code = output.exit_code.unwrap_or(-1);
        let mut stderr_raw = output.stderr;
        let canceled = output.status == ShellStatus::Killed
            && context
                .cancellation_token()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled);
        if output.status == ShellStatus::TimedOut {
            stderr_raw.push_str("\ncargo test timed out; managed process tree was killed.");
        } else if canceled {
            stderr_raw.push_str("\ncargo test canceled; managed process tree was killed.");
        }
        let stdout = truncate_with_note(&output.stdout, MAX_OUTPUT_CHARS);
        let stderr = truncate_with_note(&stderr_raw, MAX_OUTPUT_CHARS);
        let evidence = parse_cargo_test_evidence(&output.stdout, &stderr_raw);

        let result = RunTestsOutput {
            success: output.status == ShellStatus::Completed,
            exit_code,
            stdout,
            stderr,
            command: command_str,
        };

        let mut tool_result =
            ToolOutcome::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        tool_result.side_effect = ToolSideEffectStatus::Indeterminate;
        if !result.success {
            tool_result.operation = ToolOperationStatus::Failed;
            tool_result.retry = ToolRetryDisposition::Unsafe;
        }
        tool_result.metadata = Some(json!({
            "cargo_test_evidence": evidence,
            "process_status": shell_status_name(&output.status),
            "canceled": canceled,
        }));
        if let Some(summary) = summarize_cargo_failure(
            &result.command,
            &result.stdout,
            &result.stderr,
            Some(result.exit_code),
        ) && let Some(metadata) = tool_result.metadata.as_mut().and_then(Value::as_object_mut)
        {
            let summary_metadata = summary.to_metadata_value();
            metadata.insert("summary".to_string(), Value::String(summary.summary));
            metadata.insert("cargo_failure_summary".to_string(), summary_metadata);
        }
        if tool_result.is_success() {
            if evidence.running == 0 || evidence.executed() == 0 {
                crate::tools::goal::reject_host_verification(
                    &mut tool_result,
                    "cargo test exited successfully but executed zero tests; no Goal evidence artifact was produced",
                );
                crate::tools::goal::reject_goal_evidence_artifact(&mut tool_result);
            } else {
                let rejection = if extra_args.is_some() {
                    "cargo test succeeded, but focused/free-form args produce only an evidence artifact and cannot complete a Goal"
                } else {
                    match context
                        .goal_contract
                        .as_ref()
                        .map(|contract| &contract.acceptance)
                    {
                        Some(crate::tools::goal::TaskAcceptance::HostAcceptanceRequired) => {
                            "run_tests output is a revision-bound evidence artifact; this objective-only Goal requires host acceptance and cannot be completed by the model"
                        }
                        Some(crate::tools::goal::TaskAcceptance::Verifier { .. }) => {
                            "run_tests output is a revision-bound evidence artifact; Goal completion requires the exact run_verifiers invocation in the active task contract"
                        }
                        None => {
                            "run_tests output is a revision-bound evidence artifact; no active Goal acceptance contract was bound when it started"
                        }
                    }
                };
                crate::tools::goal::reject_host_verification(&mut tool_result, rejection);
                let revision_after =
                    crate::tools::goal::capture_workspace_revision(context.workspace()).await;
                crate::tools::goal::attach_goal_evidence_artifact(
                    &mut tool_result,
                    self.name(),
                    &json!({
                        "all_features": all_features,
                        "args": extra_args.unwrap_or_default(),
                    }),
                    result.command.clone(),
                    format!(
                        "cargo test passed {} test(s) with exit code {}",
                        evidence.passed, result.exit_code
                    ),
                    revision_before,
                    revision_after,
                );
            }
        } else {
            crate::tools::goal::reject_goal_evidence_artifact(&mut tool_result);
        }
        Ok(tool_result)
    }
}

// === Helpers ===

async fn run_cargo(
    context: &ToolContext,
    args: &[String],
    timeout_ms: u64,
) -> Result<ShellResult, ToolError> {
    let Some(spec) = crate::dependencies::Cargo::resolve() else {
        return Err(ToolError::not_available(
            "cargo is not installed or not in PATH",
        ));
    };
    let (program, mut command_args) = crate::dependencies::split_interpreter_spec(&spec);
    command_args.extend(args.iter().cloned());
    let display = format_command(context.workspace(), args);
    execute_managed_program(
        context,
        &display,
        &program,
        &command_args,
        context.workspace(),
        timeout_ms,
        context.elevated_sandbox_policy.clone(),
        HashMap::new(),
    )
    .await
    .map_err(|err| {
        if err.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
        }) {
            ToolError::not_available("cargo is not installed or not in PATH")
        } else {
            ToolError::execution_failed(format!("Failed to run cargo: {err}"))
        }
    })
}

fn shell_status_name(status: &ShellStatus) -> &'static str {
    match status {
        ShellStatus::Running => "running",
        ShellStatus::Completed => "completed",
        ShellStatus::Failed => "failed",
        ShellStatus::Killed => "killed",
        ShellStatus::TimedOut => "timed_out",
    }
}

fn parse_cargo_test_evidence(stdout: &str, stderr: &str) -> CargoTestEvidence {
    let mut evidence = CargoTestEvidence::default();
    for line in stdout.lines().chain(stderr.lines()).map(str::trim) {
        if let Some(rest) = line.strip_prefix("running ") {
            let mut words = rest.split_whitespace();
            if let (Some(count), Some(kind)) = (words.next(), words.next())
                && matches!(kind, "test" | "tests")
                && let Ok(count) = count.parse::<u64>()
            {
                evidence.running = evidence.running.saturating_add(count);
            }
        }
        let Some(result) = line.strip_prefix("test result:") else {
            continue;
        };
        evidence.result_lines = evidence.result_lines.saturating_add(1);
        evidence.passed = evidence
            .passed
            .saturating_add(metric_before_label(result, "passed").unwrap_or(0));
        evidence.failed = evidence
            .failed
            .saturating_add(metric_before_label(result, "failed").unwrap_or(0));
        evidence.ignored = evidence
            .ignored
            .saturating_add(metric_before_label(result, "ignored").unwrap_or(0));
        evidence.measured = evidence
            .measured
            .saturating_add(metric_before_label(result, "measured").unwrap_or(0));
        evidence.filtered_out = evidence
            .filtered_out
            .saturating_add(metric_before_label(result, "filtered out").unwrap_or(0));
    }
    evidence
}

fn metric_before_label(line: &str, label: &str) -> Option<u64> {
    let marker = format!(" {label}");
    let before = line.find(&marker).map(|index| &line[..index])?;
    before
        .split(|character: char| !character.is_ascii_digit())
        .rfind(|part| !part.is_empty())?
        .parse()
        .ok()
}

fn format_command(workspace: &Path, args: &[String]) -> String {
    format!(
        "(cd {} && cargo {})",
        workspace.display(),
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn truncate_with_note(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let end = char_boundary_index(text, max_chars);
    let truncated = &text[..end];
    let omitted_chars = text
        .chars()
        .count()
        .saturating_sub(truncated.chars().count());
    let note = format!(
        "\n\n[output truncated to {max_chars} characters; {omitted_chars} characters omitted]"
    );
    format!("{truncated}{note}")
}

fn char_boundary_index(text: &str, max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return idx;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_protocol::agent_runtime::{ToolArtifactStatus, ToolEvidenceStatus};
    use std::fs;
    use std::process::Command;
    #[cfg(unix)]
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn cargo_available() -> bool {
        Command::new("cargo")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn init_cargo_project(root: &Path) -> std::path::PathBuf {
        let project_dir = root.join("project");
        fs::create_dir_all(&project_dir).expect("create project dir");
        let status = crate::dependencies::Cargo::command()
            .expect("cargo not found")
            .args([
                "init",
                "--lib",
                "--vcs",
                "none",
                "-q",
                "--name",
                "eval_project",
            ])
            .current_dir(&project_dir)
            .status()
            .expect("cargo should spawn");
        assert!(status.success(), "cargo init failed");
        project_dir
    }

    fn write_passing_test(project_dir: &Path) {
        fs::write(
            project_dir.join("src/lib.rs"),
            r#"
#[cfg(test)]
mod tests {
    #[test]
    fn host_evidence_fixture() {
        assert_eq!(2 + 2, 4);
    }
}
"#,
        )
        .expect("write passing test");
    }

    fn prime_and_init_git(project_dir: &Path) {
        fs::write(project_dir.join(".gitignore"), "/target\n").expect("write gitignore");
        let cargo_status = crate::dependencies::Cargo::command()
            .expect("cargo")
            .args(["test", "-q"])
            .current_dir(project_dir)
            .status()
            .expect("prime cargo test");
        assert!(cargo_status.success(), "priming cargo test failed");
        let git_status = crate::dependencies::Git::command()
            .expect("git")
            .args(["init", "-q"])
            .current_dir(project_dir)
            .status()
            .expect("git init");
        assert!(git_status.success(), "git init failed");
    }

    /// `run_tests` is `ToolCapability::ExecutesCode`, so it must follow the
    /// explicit-approval policy that applies to other code-executing tools.
    #[test]
    fn run_tests_requires_user_approval() {
        let tool = RunTestsTool;
        assert_eq!(
            tool.approval_requirement(),
            ApprovalRequirement::Required,
            "run_tests must gate cargo test behind user approval"
        );
    }

    #[tokio::test]
    async fn run_tests_succeeds_on_fresh_project() {
        if !cargo_available() || !crate::dependencies::Git::available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let project_dir = init_cargo_project(tmp.path());
        write_passing_test(&project_dir);
        prime_and_init_git(&project_dir);

        let ctx = ToolContext::new(&project_dir);
        let tool = RunTestsTool;
        let result = tool
            .execute(json!({"all_features": true}), &ctx)
            .await
            .expect("execute");
        assert!(result.is_success());

        let parsed: RunTestsOutput =
            serde_json::from_str(&result.content).expect("tool result should be json");
        assert!(parsed.success);
        assert_eq!(parsed.exit_code, 0);
        assert!(parsed.command.contains("cargo test"));
        assert!(parsed.command.contains("--all-features"));
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(result.artifacts.len(), 1);
        let artifact = &result.artifacts[0];
        assert_eq!(result.evidence.references, vec![artifact.id.clone()]);
        assert_eq!(artifact.status, ToolArtifactStatus::Available);
        assert!(
            artifact
                .sha256
                .as_deref()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        assert_eq!(artifact.media_type.as_deref(), Some("application/json"));
        assert!(artifact.byte_len.is_some_and(|len| len > 0));
        assert!(
            result
                .workspace_revision
                .as_deref()
                .is_some_and(|revision| revision.starts_with("sha256:"))
        );
        result.validate().expect("typed evidence must be valid");
        let metadata = result.metadata.expect("run_tests metadata");
        assert!(
            metadata["cargo_test_evidence"]["passed"]
                .as_u64()
                .is_some_and(|passed| passed > 0),
            "metadata: {metadata}"
        );
        assert!(metadata.get("goal_host_verification").is_none());
        assert!(
            metadata.get("goal_evidence_artifact").is_some(),
            "an unfiltered positive test run should mint only a revision-bound artifact: {metadata}"
        );
    }

    #[tokio::test]
    async fn run_tests_zero_tests_rejects_goal_evidence() {
        if !cargo_available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let project_dir = init_cargo_project(tmp.path());
        fs::write(
            project_dir.join("src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .expect("write zero-test crate");

        let ctx = ToolContext::new(&project_dir);
        let result = RunTestsTool
            .execute(json!({}), &ctx)
            .await
            .expect("run zero tests");
        assert!(result.is_success(), "cargo test itself should succeed");
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Rejected);
        assert!(result.evidence.references.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.workspace_revision.is_none());
        let metadata = result.metadata.expect("run_tests metadata");
        assert_eq!(metadata["cargo_test_evidence"]["passed"], json!(0));
        assert!(metadata.get("goal_host_verification").is_none());
        assert!(
            metadata["goal_host_verification_rejected"]
                .as_str()
                .is_some_and(|reason| reason.contains("zero tests")),
            "metadata: {metadata}"
        );
    }

    #[tokio::test]
    async fn run_tests_free_form_args_reject_goal_evidence() {
        if !cargo_available() || !crate::dependencies::Git::available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let project_dir = init_cargo_project(tmp.path());
        write_passing_test(&project_dir);
        prime_and_init_git(&project_dir);

        let ctx = ToolContext::new(&project_dir);
        let result = RunTestsTool
            .execute(json!({"args": "host_evidence_fixture"}), &ctx)
            .await
            .expect("run focused test");
        assert!(result.is_success(), "focused cargo test should succeed");
        let metadata = result.metadata.expect("run_tests metadata");
        assert!(
            metadata["cargo_test_evidence"]["passed"]
                .as_u64()
                .is_some_and(|passed| passed > 0),
            "fixture must prove that rejection is caused by args, not zero tests: {metadata}"
        );
        assert!(metadata.get("goal_host_verification").is_none());
        assert!(
            metadata["goal_host_verification_rejected"]
                .as_str()
                .is_some_and(|reason| reason.contains("free-form args")),
            "metadata: {metadata}"
        );
        assert!(metadata.get("goal_evidence_artifact").is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_tests_cancel_kills_test_process_tree() {
        if !cargo_available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let project_dir = init_cargo_project(tmp.path());
        fs::write(
            project_dir.join("src/lib.rs"),
            r#"
#[cfg(test)]
mod tests {
    #[test]
    fn long_running() {
        std::fs::write("run-tests-child.pid", std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
"#,
        )
        .expect("write long test");

        let pid_path = project_dir.join("run-tests-child.pid");
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut ctx = ToolContext::new(&project_dir);
        ctx.set_invocation_cancellation(cancel.clone());
        let task_ctx = ctx.clone();
        let task = tokio::spawn(async move { RunTestsTool.execute(json!({}), &task_ctx).await });

        let child_pid = wait_for_pid_file(&pid_path, Duration::from_secs(30)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("cancel should settle cargo test promptly")
            .expect("run_tests join")
            .expect("run_tests result");
        assert!(!result.is_success(), "a canceled test run must fail");
        let metadata = result.metadata.expect("run_tests metadata");
        assert_eq!(metadata["process_status"], json!("killed"));
        assert_eq!(metadata["canceled"], json!(true));
        assert_process_exited(child_pid, Duration::from_secs(2)).await;
        assert!(
            ctx.shell_manager
                .lock()
                .expect("shell manager")
                .list_jobs()
                .iter()
                .all(|job| job.status != ShellStatus::Running),
            "canceled run_tests left a managed job running"
        );
    }

    #[tokio::test]
    async fn run_tests_reports_failures_without_hard_error() {
        if !cargo_available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let project_dir = init_cargo_project(tmp.path());

        let lib_rs = project_dir.join("src/lib.rs");
        let failing = r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[cfg(test)]
mod tests {
    #[test]
    fn fails() {
        assert_eq!(2 + 2, 5);
    }
}
"#;
        fs::write(&lib_rs, failing).expect("write failing test");

        let ctx = ToolContext::new(&project_dir);
        let tool = RunTestsTool;
        let result = tool.execute(json!({}), &ctx).await.expect("execute");
        assert!(
            !result.is_success(),
            "a non-zero cargo test exit must be a failed tool result"
        );
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Rejected);
        assert!(result.evidence.references.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.workspace_revision.is_none());

        let parsed: RunTestsOutput =
            serde_json::from_str(&result.content).expect("tool result should be json");
        assert!(!parsed.success);
        assert_ne!(parsed.exit_code, 0);
        let metadata = result.metadata.expect("metadata");
        assert_eq!(
            metadata["cargo_failure_summary"]["kind"],
            json!("test_failure")
        );
        assert!(
            metadata["cargo_failure_summary"]["summary"]
                .as_str()
                .unwrap()
                .contains("Failing tests:")
        );
    }

    #[test]
    fn truncation_adds_note() {
        let long = "x".repeat(MAX_OUTPUT_CHARS + 128);
        let truncated = truncate_with_note(&long, MAX_OUTPUT_CHARS);
        assert!(truncated.contains("output truncated"));
    }

    #[test]
    fn cargo_test_evidence_sums_multiple_harnesses() {
        let output = r#"
running 2 tests
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out
running 1 test
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let evidence = parse_cargo_test_evidence(output, "");
        assert_eq!(evidence.running, 3);
        assert_eq!(evidence.result_lines, 2);
        assert_eq!(evidence.passed, 2);
        assert_eq!(evidence.failed, 1);
        assert_eq!(evidence.ignored, 1);
        assert_eq!(evidence.filtered_out, 3);
        assert_eq!(evidence.executed(), 3);
    }

    #[cfg(unix)]
    async fn wait_for_pid_file(path: &Path, timeout: Duration) -> libc::pid_t {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(raw) = fs::read_to_string(path)
                && let Ok(pid) = raw.trim().parse::<libc::pid_t>()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "process did not publish pid at {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    async fn assert_process_exited(pid: libc::pid_t, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while process_exists(pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if process_exists(pid) {
            // Keep a failing lifecycle test from leaking its own fixture.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            panic!("test process {pid} survived managed kill+reap");
        }
    }

    #[cfg(unix)]
    fn process_exists(pid: libc::pid_t) -> bool {
        let status = unsafe { libc::kill(pid, 0) };
        status == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}
