//! Production `run_tests` operation.
//!
//! This module owns Cargo process execution, cancellation and result parsing.
//! Goal completion remains a host concern and is deliberately absent here.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
};
use dse_protocol::task::{VerifierPlan, VerifierSpec, VerifierStep};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::shell::cargo_failure_summary::summarize_cargo_failure;
use crate::shell::{ExecShellOptions, ShellResult, ShellStatus, execute_managed_program};
use crate::verification_artifact::{
    attach_verifier_observation, capture_workspace_revision, reject_verification_artifact,
};
use crate::{ProductionToolContext, ToolError, ToolOutcome};

const MAX_OUTPUT_CHARS: usize = 40_000;
const RUN_TESTS_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RunTestsInput {
    args: Vec<String>,
    all_features: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTestsOutput {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CargoTestEvidence {
    pub running: u64,
    pub result_lines: u64,
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
    pub measured: u64,
    pub filtered_out: u64,
}

impl CargoTestEvidence {
    #[must_use]
    pub fn executed(&self) -> u64 {
        self.passed.saturating_add(self.failed)
    }
}

/// Run `cargo test` in the context workspace through the shared managed
/// process owner. The outcome contains deterministic process/test evidence,
/// but never decides whether a product Goal is complete.
pub(crate) async fn execute_run_tests(
    input: Value,
    context: &ProductionToolContext,
    shell: &ExecShellOptions,
) -> Result<ToolOutcome, ToolError> {
    let input = parse_run_tests_input(input)?;
    let verifier = run_tests_verifier_spec(&input);
    let args = verifier.plan.steps[0].args.clone();

    let command = format_command(context.workspace(), &args);
    let revision_before = capture_workspace_revision(context.workspace()).await;
    let output = run_cargo(context, shell, &command, &args, RUN_TESTS_TIMEOUT_MS).await?;
    let exit_code = output.exit_code.unwrap_or(-1);
    let exited_with_failure = output.status == ShellStatus::Failed;
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
    let verification_usable = evidence.running > 0 && evidence.executed() > 0;

    let result = RunTestsOutput {
        success: output.status == ShellStatus::Completed,
        exit_code,
        stdout,
        stderr,
        command,
    };
    let mut outcome = ToolOutcome::json(&result)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;
    outcome.side_effect = ToolSideEffectStatus::Indeterminate;
    if !result.success {
        outcome.failure_code = Some(ToolFailureCode::OperationFailed);
        outcome.operation = ToolOperationStatus::Failed;
        outcome.retry = ToolRetryDisposition::Unsafe;
    }
    outcome.metadata = Some(json!({
        "cargo_test_evidence": evidence,
        "process_status": shell_status_name(&output.status),
        "canceled": canceled,
        "verification_usable": verification_usable,
        "focused_args": !input.args.is_empty(),
        "all_features": input.all_features,
    }));
    if let Some(summary) = summarize_cargo_failure(
        &result.command,
        &result.stdout,
        &result.stderr,
        Some(result.exit_code),
    ) && let Some(metadata) = outcome.metadata.as_mut().and_then(Value::as_object_mut)
    {
        let summary_metadata = summary.to_metadata_value();
        metadata.insert("summary".to_string(), Value::String(summary.summary));
        metadata.insert("cargo_failure_summary".to_string(), summary_metadata);
    }
    if verification_usable && (outcome.is_success() || exited_with_failure) {
        let revision_after = capture_workspace_revision(context.workspace()).await;
        let verdict = if outcome.is_success() {
            dse_protocol::task::VerifierVerdict::Passed
        } else {
            dse_protocol::task::VerifierVerdict::Failed
        };
        attach_verifier_observation(
            &mut outcome,
            verifier,
            verdict,
            format!(
                "cargo test observed {} passed and {} failed test(s) with exit code {}",
                evidence.passed, evidence.failed, result.exit_code
            ),
            revision_before,
            revision_after,
        );
    } else {
        reject_verification_artifact(&mut outcome);
    }
    Ok(outcome)
}

pub(crate) fn resolve_run_tests_spec(parameters: Value) -> Result<VerifierSpec, ToolError> {
    parse_run_tests_input(parameters).map(|input| run_tests_verifier_spec(&input))
}

fn parse_run_tests_input(input: Value) -> Result<RunTestsInput, ToolError> {
    serde_json::from_value(input).map_err(|error| ToolError::invalid_input(error.to_string()))
}

fn run_tests_verifier_spec(input: &RunTestsInput) -> VerifierSpec {
    let mut args = vec!["test".to_owned()];
    if input.all_features {
        args.push("--all-features".to_owned());
    }
    args.extend(input.args.iter().cloned());
    VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: json!({
            "all_features": input.all_features,
            "args": input.args,
        }),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "cargo-test".to_owned(),
                program: "cargo".to_owned(),
                args,
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: RUN_TESTS_TIMEOUT_MS,
            }],
        },
    }
    .canonicalized()
}

async fn run_cargo(
    context: &ProductionToolContext,
    shell: &ExecShellOptions,
    display: &str,
    args: &[String],
    timeout_ms: u64,
) -> Result<ShellResult, ToolError> {
    execute_managed_program(
        context,
        &shell.shell_manager,
        display,
        "cargo",
        args,
        context.workspace(),
        timeout_ms,
        shell.elevated_sandbox_policy.clone(),
        HashMap::new(),
    )
    .await
    .map_err(|error| {
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
        }) {
            ToolError::not_available("cargo is not installed or not in PATH")
        } else {
            ToolError::execution_failed(format!("Failed to run cargo: {error}"))
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
    format!(
        "{truncated}\n\n[output truncated to {max_chars} characters; {omitted_chars} characters omitted]"
    )
}

fn char_boundary_index(text: &str, max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return index;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{ShellPolicy, new_shared_shell_manager};
    use dse_protocol::agent_runtime::ToolEvidenceStatus;

    fn rust_workspace(with_test: bool) -> tempfile::TempDir {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join("src")).unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        // This fixture lives outside the repository's rust-toolchain override.
        // Pin it independently so the production-path test does not rely on a
        // developer having configured a global rustup default.
        std::fs::write(
            workspace.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"stable\"\nprofile = \"minimal\"\n",
        )
        .unwrap();
        std::fs::write(workspace.path().join(".gitignore"), "/target\n").unwrap();
        let body = if with_test {
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)] mod tests { #[test] fn adds() { assert_eq!(super::add(1, 2), 3); } }\n"
        } else {
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n"
        };
        std::fs::write(workspace.path().join("src/lib.rs"), body).unwrap();
        assert!(
            std::process::Command::new("cargo")
                .arg("generate-lockfile")
                .current_dir(workspace.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(workspace.path())
                .status()
                .unwrap()
                .success()
        );
        workspace
    }

    fn shell(workspace: &Path) -> ExecShellOptions {
        ExecShellOptions::new(
            new_shared_shell_manager(workspace.to_path_buf()),
            ShellPolicy::Full,
        )
    }

    #[test]
    fn parses_cargo_test_evidence_deterministically() {
        let evidence = parse_cargo_test_evidence(
            "running 2 tests\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out",
            "",
        );
        assert_eq!(evidence.running, 2);
        assert_eq!(evidence.passed, 2);
        assert_eq!(evidence.failed, 0);
        assert_eq!(evidence.ignored, 1);
        assert_eq!(evidence.filtered_out, 3);
    }

    #[test]
    fn resolver_owns_normalized_parameters_and_execution_plan() {
        let spec = resolve_run_tests_spec(json!({"args": ["--locked"]})).unwrap();
        assert_eq!(spec.verifier_id, "run_tests");
        assert_eq!(
            spec.parameters,
            json!({"all_features": false, "args": ["--locked"]})
        );
        assert_eq!(spec.plan.steps.len(), 1);
        assert_eq!(spec.plan.steps[0].args, ["test", "--locked"]);
        assert_eq!(spec.plan.steps[0].timeout_ms, RUN_TESTS_TIMEOUT_MS);
    }

    #[test]
    fn truncation_is_utf8_safe() {
        assert_eq!(
            truncate_with_note("深海鲸", 2).split('\n').next(),
            Some("深海")
        );
    }

    #[tokio::test]
    async fn real_cargo_success_produces_revision_bound_artifact() {
        let workspace = rust_workspace(true);
        let context = ProductionToolContext::new(workspace.path());
        let outcome = execute_run_tests(
            json!({"args": ["--locked"]}),
            &context,
            &shell(workspace.path()),
        )
        .await
        .unwrap();
        assert!(outcome.is_success(), "{}", outcome.content);
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(outcome.artifacts.len(), 1);
        assert!(outcome.workspace_revision.is_some());
        let observation = outcome
            .verifier_observation
            .as_ref()
            .expect("typed verifier observation");
        assert_eq!(observation.spec.verifier_id, "run_tests");
        assert_eq!(
            observation.spec.parameters,
            json!({"all_features": false, "args": ["--locked"]})
        );
        assert_eq!(
            observation.spec.plan.steps[0].args,
            vec!["test", "--locked"]
        );
    }

    #[tokio::test]
    async fn real_cargo_failure_produces_revision_bound_failed_observation() {
        let workspace = rust_workspace(true);
        std::fs::write(
            workspace.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)] mod tests { #[test] fn adds() { assert_eq!(super::add(1, 2), 4); } }\n",
        )
        .unwrap();
        let context = ProductionToolContext::new(workspace.path());
        let outcome = execute_run_tests(
            json!({"args": ["--locked"]}),
            &context,
            &shell(workspace.path()),
        )
        .await
        .unwrap();

        assert!(!outcome.is_success());
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(
            outcome
                .verifier_observation
                .as_ref()
                .map(|value| value.verdict),
            Some(dse_protocol::task::VerifierVerdict::Failed)
        );
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::Indeterminate);
    }

    #[tokio::test]
    async fn zero_tests_are_not_usable_evidence() {
        let workspace = rust_workspace(false);
        let context = ProductionToolContext::new(workspace.path());
        let outcome = execute_run_tests(json!({}), &context, &shell(workspace.path()))
            .await
            .unwrap();
        assert!(outcome.is_success());
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Rejected);
        assert!(outcome.artifacts.is_empty());
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["verification_usable"],
            false
        );
    }
}
