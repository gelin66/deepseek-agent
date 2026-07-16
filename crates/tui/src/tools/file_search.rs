//! TUI contract adapter for the production `file_search` operation.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
};

pub struct FileSearchTool;

#[async_trait]
impl ToolSpec for FileSearchTool {
    fn name(&self) -> &'static str {
        "file_search"
    }

    fn description(&self) -> &'static str {
        "Find files by name using fuzzy matching with score-based ranking. Use this instead of `find -name` or `fd` in `exec_shell` for filename search. Pass `extensions` to filter by suffix."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (file name or path fragment)."
                },
                "path": {
                    "type": "string",
                    "description": "Optional base path to search (relative to workspace)."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 20)."
                },
                "extensions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of file extensions to include (e.g. [\"rs\", \"md\"])."
                },
                "exclude": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional glob patterns to exclude, matching grep_files' convention (e.g. [\"target/**\", \"*.lock\"])."
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        // M4-C deletes this schema adapter when the fixed catalog moves out of TUI.
        codewhale_tools::execute_file_search(input, context.production_context()).await
    }
}
