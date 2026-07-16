//! TUI schema and diagnostics adapter for the production `apply_patch` tool.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

pub use codewhale_tools::preflight_apply_patch;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    lsp_diagnostics_for_paths,
};

/// Tool for applying unified diff patches to files.
///
/// The mutation, parsing, validation, rollback, and outcome semantics live in
/// `codewhale-tools`. M4-C deletes this adapter when schemas and diagnostics
/// move behind the shared application service.
pub struct ApplyPatchTool;

#[async_trait]
impl ToolSpec for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn description(&self) -> &'static str {
        "Apply a unified-diff patch (multi-hunk, multi-file). Use this instead of `git apply`, `patch`, or repeated `edit_file` calls in `exec_shell` — single transactional change with fuzzy matching and a rendered diff."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch (relative to workspace)"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch content"
                },
                "changes": {
                    "type": "array",
                    "description": "Optional full file replacements (path + content).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                },
                "fuzz": {
                    "type": "integer",
                    "description": "Maximum fuzz factor for fuzzy matching (default: 3)"
                },
                "create_if_missing": {
                    "type": "boolean",
                    "description": "Create the file if it doesn't exist (for new file patches)"
                }
            },
            "oneOf": [
                { "required": ["patch"] },
                { "required": ["changes"] }
            ]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let mut outcome =
            codewhale_tools::execute_apply_patch(input, context.production_context())?;
        let written_paths = written_paths_from_outcome(&outcome, context);
        let diagnostics = lsp_diagnostics_for_paths(context, &written_paths).await;
        if !diagnostics.is_empty() {
            outcome.content.push('\n');
            outcome.content.push_str(&diagnostics);
        }
        Ok(outcome)
    }
}

fn written_paths_from_outcome(outcome: &ToolOutcome, context: &ToolContext) -> Vec<PathBuf> {
    let Ok(result) = serde_json::from_str::<Value>(&outcome.content) else {
        return Vec::new();
    };

    result
        .get("file_summaries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|summary| summary.get("deleted").and_then(Value::as_bool) != Some(true))
        .filter_map(|summary| summary.get("path").and_then(Value::as_str))
        .filter_map(|path| context.resolve_path(path).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn apply_patch_schema_and_policy_are_unchanged() {
        let tool = ApplyPatchTool;

        assert_eq!(tool.name(), "apply_patch");
        assert_eq!(
            tool.description(),
            "Apply a unified-diff patch (multi-hunk, multi-file). Use this instead of `git apply`, `patch`, or repeated `edit_file` calls in `exec_shell` — single transactional change with fuzzy matching and a rendered diff."
        );
        assert_eq!(
            tool.input_schema(),
            json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to patch (relative to workspace)"
                    },
                    "patch": {
                        "type": "string",
                        "description": "Unified diff patch content"
                    },
                    "changes": {
                        "type": "array",
                        "description": "Optional full file replacements (path + content).",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["path", "content"]
                        }
                    },
                    "fuzz": {
                        "type": "integer",
                        "description": "Maximum fuzz factor for fuzzy matching (default: 3)"
                    },
                    "create_if_missing": {
                        "type": "boolean",
                        "description": "Create the file if it doesn't exist (for new file patches)"
                    }
                },
                "oneOf": [
                    { "required": ["patch"] },
                    { "required": ["changes"] }
                ]
            })
        );
        assert_eq!(
            tool.capabilities(),
            vec![
                ToolCapability::WritesFiles,
                ToolCapability::Sandboxable,
                ToolCapability::RequiresApproval,
            ]
        );
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
    }

    #[tokio::test]
    async fn tui_adapter_matches_direct_production_execution_without_lsp() {
        let direct_workspace = tempdir().expect("direct workspace");
        let adapter_workspace = tempdir().expect("adapter workspace");
        fs::write(
            direct_workspace.path().join("sample.txt"),
            b"old\r\nline\r\n",
        )
        .expect("direct fixture");
        fs::write(
            adapter_workspace.path().join("sample.txt"),
            b"old\r\nline\r\n",
        )
        .expect("adapter fixture");
        let input = json!({
            "path": "sample.txt",
            "patch": "@@ -1,2 +1,2 @@\n-old\n+new\n line\n"
        });

        let direct = codewhale_tools::execute_apply_patch(
            input.clone(),
            &codewhale_tools::ProductionToolContext::new(direct_workspace.path().to_path_buf()),
        )
        .expect("direct execution");
        let adapted = ApplyPatchTool
            .execute(
                input,
                &ToolContext::new(adapter_workspace.path().to_path_buf()),
            )
            .await
            .expect("adapter execution");

        assert_eq!(adapted, direct);
        assert_eq!(
            fs::read(adapter_workspace.path().join("sample.txt")).expect("adapter result"),
            fs::read(direct_workspace.path().join("sample.txt")).expect("direct result")
        );
    }

    #[test]
    fn diagnostics_targets_exclude_deleted_files() {
        let workspace = tempdir().expect("workspace");
        let context = ToolContext::new(workspace.path().to_path_buf());
        let outcome = ToolOutcome::success(
            json!({
                "file_summaries": [
                    {"path": "written.rs", "deleted": false},
                    {"path": "deleted.rs", "deleted": true}
                ]
            })
            .to_string(),
        );

        assert_eq!(
            written_paths_from_outcome(&outcome, &context),
            vec![context.resolve_path("written.rs").expect("resolved path")]
        );
    }
}
