//! File system tools: `read_file`, `write_file`, `edit_file`, `list_dir`
//!
//! These tools provide safe file system operations within the workspace,
//! with path validation to prevent escaping the workspace boundary.

use super::diff_format::make_unified_diff;
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    lsp_diagnostics_for_paths, required_str,
};
use async_trait::async_trait;
use codewhale_protocol::agent_runtime::ToolSideEffectStatus;
use serde_json::{Value, json};
use std::fs;

// === ReadFileTool ===

/// Tool for reading UTF-8 files from the workspace.
pub struct ReadFileTool;

#[async_trait]
impl ToolSpec for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 file from the workspace. Use this instead of `cat`, `head`, `tail`, or `sed -n '..p'` in `exec_shell` — it's faster, sandbox-aware, and skips the approval prompt. Plain text is returned as-is and records the file snapshot required before `edit_file` will make a narrow in-place edit. PDFs are auto-extracted via the bundled pure-Rust extractor (no Poppler install required). Image screenshots are OCR-extracted when local OCR is available. Cannot read other non-PDF binaries.\n\nFor large files, use `start_line` and `max_lines` to read in chunks. By default, returns at most 200 lines (~16KB). If `truncated=\"true\"` in the response, use `next_start_line` to continue reading. For PDFs, use `pages` instead — `start_line`/`max_lines` only apply to text files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace or absolute)"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Starting line (1-based, default 1)"
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Maximum lines to return (default 200, max 500)"
                },
                "pages": {
                    "type": "string",
                    "description": "PDF only: page range to extract, e.g. \"1-5\" or \"10\". Ignored for non-PDF files."
                }
            },
            "required": ["path"]
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
        codewhale_tools::execute_read_file(
            input,
            context.production_context(),
            prefer_external_pdftotext(),
        )
    }
}

fn prefer_external_pdftotext() -> bool {
    crate::settings::Settings::load()
        .map(|settings| settings.prefer_external_pdftotext)
        .unwrap_or(false)
}

// === WriteFileTool ===

/// Tool for writing UTF-8 files to the workspace.
pub struct WriteFileTool;

#[async_trait]
impl ToolSpec for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write content to a UTF-8 file in the workspace. Use this instead of heredocs (`cat <<EOF > file`) or `echo > file` in `exec_shell` — diffs render inline and approval is handled cleanly. Creates or overwrites; parent directories are auto-created."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
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
        let path_str = required_str(&input, "path")?;
        let file_content = required_str(&input, "content")?;

        let file_path = context.resolve_path(path_str)?;

        // Snapshot the existing contents (if any) before we overwrite — used
        // to render an inline diff in the tool result.
        let existed_before = file_path.exists();
        let prior_contents = if existed_before {
            fs::read_to_string(&file_path).unwrap_or_default()
        } else {
            String::new()
        };

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        crate::utils::write_atomic(&file_path, file_content.as_bytes()).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;
        context.note_file_read(&file_path);

        let display = file_path.display().to_string();
        let diff = make_unified_diff(&display, &prior_contents, file_content);
        let summary = if existed_before {
            format!("Wrote {} bytes to {}", file_content.len(), display)
        } else {
            format!("Created {} ({} bytes)", display, file_content.len())
        };
        let body = if diff.is_empty() {
            format!("{summary}\n(no changes)")
        } else {
            format!("{diff}\n{summary}")
        };

        // Append LSP diagnostics for the written file when enabled (#428).
        let diag_block = lsp_diagnostics_for_paths(context, &[file_path]).await;
        let full_body = if diag_block.is_empty() {
            body
        } else {
            format!("{body}\n{diag_block}")
        };

        Ok(ToolOutcome::success(full_body).with_side_effect(ToolSideEffectStatus::Applied))
    }
}

// === EditFileTool ===

/// Tool for search/replace editing of files.
pub struct EditFileTool;

#[async_trait]
impl ToolSpec for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Replace text in a single file via exact search/replace after the file has been read with `read_file` in this session. Use this instead of `sed -i` in `exec_shell` for one unambiguous in-place edit. `search` must match exactly one location by default; when no exact match is found the tool retries with leading-whitespace-tolerant fuzzy matching automatically. The optional `fuzz` parameter is accepted for backward compatibility and is no longer needed. Returns a compact unified diff, not the full file. For structural, multi-block, or cross-file changes, use `apply_patch` or `write_file` instead."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "search": {
                    "type": "string",
                    "description": "Exact text to search for, including whitespace, indentation, and newlines"
                },
                "replace": {
                    "type": "string",
                    "description": "Text to replace with"
                },
                "fuzz": {
                    "type": "boolean",
                    "description": "Deprecated: fuzzy fallback is now automatic. Accepted for backward compatibility but ignored."
                }
            },
            "required": ["path", "search", "replace"]
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
        // Temporary presentation adapter: the production mutation belongs to
        // `codewhale-tools`; TUI only appends its optional LSP diagnostics.
        let edited_path = input
            .get("path")
            .and_then(Value::as_str)
            .and_then(|path| context.resolve_path(path).ok());
        let mut outcome = codewhale_tools::execute_edit_file(input, context.production_context())?;

        if let Some(path) = edited_path {
            let diagnostics = lsp_diagnostics_for_paths(context, &[path]).await;
            if !diagnostics.is_empty() {
                outcome.content.push('\n');
                outcome.content.push_str(&diagnostics);
            }
        }
        Ok(outcome)
    }
}

// === ListDirTool ===

/// Tool for listing directory contents.
pub struct ListDirTool;

#[async_trait]
impl ToolSpec for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List entries in a directory relative to the workspace. Use this instead of `ls`, `ls -la`, or `find . -maxdepth 1` in `exec_shell` for directory listings."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path (default: .)"
                }
            },
            "required": []
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
        codewhale_tools::execute_list_dir(input, context.production_context()).await
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn read_before_edit(ctx: &ToolContext, path: &str) {
        ReadFileTool
            .execute(json!({"path": path}), ctx)
            .await
            .expect("read before edit");
    }

    #[tokio::test]
    async fn edit_file_adapter_matches_tools_owner_without_lsp() {
        let workspace = tempdir().expect("workspace");
        let context = ToolContext::new(workspace.path().to_path_buf());
        let sample = workspace.path().join("sample.txt");
        let original = b"alpha\r\nbeta\r\ngamma\r\n";
        fs::write(&sample, original).expect("direct fixture");
        read_before_edit(&context, "sample.txt").await;
        let input = json!({"path": "sample.txt", "search": "beta", "replace": "BETA"});

        let direct =
            codewhale_tools::execute_edit_file(input.clone(), context.production_context())
                .expect("direct execution");
        let direct_bytes = fs::read(&sample).expect("direct result");

        fs::write(&sample, original).expect("restore fixture");
        read_before_edit(&context, "sample.txt").await;
        let adapted = EditFileTool
            .execute(input, &context)
            .await
            .expect("adapter execution");

        assert_eq!(adapted, direct);
        assert_eq!(fs::read(&sample).expect("adapter result"), direct_bytes);
    }

    #[tokio::test]
    async fn read_file_adapter_matches_tools_owner() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a test file
        let test_file = tmp.path().join("test.txt");
        fs::write(&test_file, "hello world").expect("write");

        let input = json!({"path": "test.txt"});
        let direct = codewhale_tools::execute_read_file(
            input.clone(),
            ctx.production_context(),
            prefer_external_pdftotext(),
        )
        .expect("direct owner");
        let adapter = ReadFileTool
            .execute(input, &ctx)
            .await
            .expect("TUI adapter");

        assert_eq!(adapter, direct);
        assert_eq!(adapter.content, "hello world");
    }

    #[tokio::test]
    async fn read_file_ocr_extracts_text_from_image_when_backend_exists() {
        if !codewhale_tools::ocr_available() {
            return;
        }
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ocr_hello.png");
        if !fixture.exists() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let copied = tmp.path().join("ocr_hello.png");
        fs::copy(&fixture, &copied).expect("copy fixture");
        if codewhale_tools::ocr_image_path(&copied).is_err() {
            // The backend can be installed yet unavailable to the current
            // process (for example, macOS Vision in a restricted runner).
            return;
        }
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let result = ReadFileTool
            .execute(json!({"path": "ocr_hello.png"}), &ctx)
            .await
            .expect("read image through OCR");

        assert!(result.is_success());
        assert!(result.content.contains("<image_ocr"));
        let normalized = result.content.to_uppercase();
        assert!(
            normalized.contains("HELLO") && normalized.contains("OCR"),
            "expected OCR text in read_file result, got {:?}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_write_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = WriteFileTool;
        let result = tool
            .execute(
                json!({"path": "output.txt", "content": "test content"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());
        // New file → "Created …" summary; the unified diff above the summary
        // primes the TUI's diff-aware renderer (#505).
        assert!(result.content.contains("Created"), "{}", result.content);
        assert!(result.content.contains("--- a/"), "{}", result.content);
        assert!(
            result.content.contains("+test content"),
            "{}",
            result.content
        );

        // Verify file was written
        let written = fs::read_to_string(tmp.path().join("output.txt")).expect("read");
        assert_eq!(written, "test content");
    }

    #[tokio::test]
    async fn test_write_file_creates_dirs() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = WriteFileTool;
        let result = tool
            .execute(
                json!({"path": "subdir/nested/file.txt", "content": "nested content"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());

        // Verify nested file was created
        let written = fs::read_to_string(tmp.path().join("subdir/nested/file.txt")).expect("read");
        assert_eq!(written, "nested content");
    }

    #[test]
    fn test_read_file_tool_properties() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_write_file_tool_properties() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
        assert!(!tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
    }

    #[test]
    fn test_edit_file_tool_properties() {
        let tool = EditFileTool;
        assert_eq!(tool.name(), "edit_file");
        assert!(!tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
        assert!(tool.description().contains("exact search/replace"));
        assert!(tool.description().contains("structural"));
    }

    #[test]
    fn test_list_dir_tool_properties() {
        let tool = ListDirTool;
        assert_eq!(tool.name(), "list_dir");
        assert!(tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_parallel_support_flags() {
        let read_tool = ReadFileTool;
        let list_tool = ListDirTool;
        let write_tool = WriteFileTool;

        assert!(read_tool.supports_parallel());
        assert!(list_tool.supports_parallel());
        assert!(!write_tool.supports_parallel());
    }

    #[test]
    fn test_input_schemas() {
        // Verify all tools have valid JSON schemas
        let read_schema = ReadFileTool.input_schema();
        assert!(read_schema.get("type").is_some());
        assert!(read_schema.get("properties").is_some());

        let write_schema = WriteFileTool.input_schema();
        let required = write_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("write schema should include required array");
        assert!(required.iter().any(|v| v.as_str() == Some("path")));
        assert!(required.iter().any(|v| v.as_str() == Some("content")));

        let edit_schema = EditFileTool.input_schema();
        let required = edit_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("edit schema should include required array");
        let required_fields: Vec<_> = required.iter().filter_map(|value| value.as_str()).collect();
        assert_eq!(required_fields, vec!["path", "search", "replace"]);
        assert!(!required_fields.contains(&"fuzz"));
        assert_eq!(
            edit_schema["properties"]["fuzz"]["type"].as_str(),
            Some("boolean")
        );
        let search_desc = edit_schema["properties"]["search"]["description"]
            .as_str()
            .expect("search description");
        assert!(search_desc.contains("Exact text"));
        assert!(search_desc.contains("whitespace"));

        let list_schema = ListDirTool.input_schema();
        let required = list_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("list schema should include required array");
        assert!(required.is_empty()); // path is optional
    }
}
