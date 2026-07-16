//! TUI contract adapters for production Git inspection operations.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
};

const DEFAULT_UNIFIED: u64 = 3;
const MAX_UNIFIED: u64 = 50;

pub struct GitStatusTool;

#[async_trait]
impl ToolSpec for GitStatusTool {
    fn name(&self) -> &'static str {
        "git_status"
    }

    fn description(&self) -> &'static str {
        "Run `git status --porcelain=v1 -b` in the workspace (optionally scoped to a path)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory or file to scope the status to (must be within the workspace)."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        // M4-C deletes this schema adapter when the fixed catalog moves out of TUI.
        codewhale_tools::execute_git_status(input, context.production_context())
    }
}

pub struct GitDiffTool;

#[async_trait]
impl ToolSpec for GitDiffTool {
    fn name(&self) -> &'static str {
        "git_diff"
    }

    fn description(&self) -> &'static str {
        "Run `git diff` in the workspace with sensible defaults and safe truncation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory or file to scope the diff to (must be within the workspace)."
                },
                "cached": {
                    "type": "boolean",
                    "description": "When true, diff staged changes (`--cached`)."
                },
                "unified": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_UNIFIED,
                    "default": DEFAULT_UNIFIED,
                    "description": "Number of context lines to include around changes."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        // M4-C deletes this schema adapter when the fixed catalog moves out of TUI.
        codewhale_tools::execute_git_diff(input, context.production_context())
    }
}
