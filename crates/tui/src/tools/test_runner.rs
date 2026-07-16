//! Legacy interactive ToolSpec projection for the tools-owned `run_tests`.
//!
//! M4-C deletes this adapter when the interactive TUI consumes the shared
//! application service. Cargo execution and result parsing must not return
//! here.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
};

pub struct RunTestsTool;

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
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let all_features = input
            .get("all_features")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let extra_args = input
            .get("args")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string();
        let revision_before =
            crate::tools::goal::capture_workspace_revision(context.workspace()).await;
        let shell = crate::tools::shell::exec_shell_options(context);
        let mut outcome =
            codewhale_tools::execute_run_tests(input, context.production_context(), &shell).await?;

        let output = serde_json::from_str::<codewhale_tools::RunTestsOutput>(&outcome.content).ok();
        let evidence = outcome
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cargo_test_evidence"))
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<codewhale_tools::CargoTestEvidence>(value).ok()
            });
        if outcome.is_success()
            && let (Some(output), Some(evidence)) = (output, evidence)
            && evidence.running > 0
            && evidence.executed() > 0
        {
            let rejection = if !extra_args.is_empty() {
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
            crate::tools::goal::reject_host_verification(&mut outcome, rejection);
            let revision_after =
                crate::tools::goal::capture_workspace_revision(context.workspace()).await;
            crate::tools::goal::attach_goal_evidence_artifact(
                &mut outcome,
                self.name(),
                &json!({
                    "all_features": all_features,
                    "args": extra_args,
                }),
                output.command,
                format!(
                    "cargo test passed {} test(s) with exit code {}",
                    evidence.passed, output.exit_code
                ),
                revision_before,
                revision_after,
            );
        } else {
            crate::tools::goal::reject_goal_evidence_artifact(&mut outcome);
            if outcome.is_success() {
                crate::tools::goal::reject_host_verification(
                    &mut outcome,
                    "cargo test exited successfully but executed zero tests; no Goal evidence artifact was produced",
                );
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_schema_remains_a_narrow_m4_c_adapter() {
        let schema = RunTestsTool.input_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["properties"].get("args").is_some());
        assert!(schema["properties"].get("all_features").is_some());
    }
}
