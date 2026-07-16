//! TUI contract adapter for the production `grep_files` operation.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec};

const MAX_RESULTS: usize = 100;

pub struct GrepFilesTool;

#[async_trait]
impl ToolSpec for GrepFilesTool {
    fn name(&self) -> &'static str {
        "grep_files"
    }

    fn description(&self) -> &'static str {
        "Search for a regex pattern in workspace files. Use this instead of `grep -r`, `rg`, or `find ... -exec grep` in `exec_shell` — pure-Rust, faster, and respects `.gitignore`. Returns matching lines with context (default: 2 lines before/after each match)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search (relative to workspace, default: .)"
                },
                "include": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Glob patterns for files to include (e.g., ['*.rs', '*.ts'])"
                },
                "exclude": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Glob patterns for files to exclude (e.g., ['*.min.js', 'node_modules/*'])"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match (default: 2)"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Whether to perform case-insensitive matching (default: false)"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_RESULTS,
                    "description": "Maximum number of results to return (default: 100, range: 1-100)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        // M4-C deletes this schema adapter when the fixed catalog moves out of TUI.
        codewhale_tools::execute_grep_files(input, context.production_context()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::ApprovalRequirement;

    #[test]
    fn grep_files_contract_is_unchanged() {
        let tool = GrepFilesTool;
        assert_eq!(tool.name(), "grep_files");
        assert!(tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
        assert!(tool.supports_parallel());

        let schema = tool.input_schema();
        let max_results = &schema["properties"]["max_results"];
        assert_eq!(max_results["minimum"], 1);
        assert_eq!(max_results["maximum"], MAX_RESULTS);
    }
}
