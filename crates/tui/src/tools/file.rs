//! File system tools: `read_file`, `write_file`, `edit_file`, `list_dir`
//!
//! These tools provide safe file system operations within the workspace,
//! with path validation to prevent escaping the workspace boundary.

use super::diff_format::make_unified_diff;
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    lsp_diagnostics_for_paths, optional_bool, required_str,
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
            production_read_file_host(),
        )
    }
}

fn production_read_file_host() -> codewhale_tools::ReadFileHost {
    codewhale_tools::ReadFileHost::new(
        prefer_external_pdftotext,
        crate::tools::image_ocr::ocr_image_path,
    )
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
        let path_str = required_str(&input, "path")?;
        let search = required_str(&input, "search")?;
        let replace = required_str(&input, "replace")?;
        let _fuzz = optional_bool(&input, "fuzz", false);

        if search == replace {
            return Err(ToolError::invalid_input(
                "search and replace are identical, no change intended",
            ));
        }

        let file_path = context.resolve_path(path_str)?;
        context.require_fresh_file_read(&file_path, path_str)?;

        let contents = fs::read_to_string(&file_path).map_err(|e| {
            ToolError::execution_failed(format!("Failed to read {}: {}", file_path.display(), e))
        })?;

        let count = contents.matches(search).count();
        let (updated, count, fuzz_kind) = if count == 0 {
            // First fallback: tolerate indentation differences.
            let indent_matches = leading_whitespace_fuzzy_matches(&contents, search);
            match indent_matches.as_slice() {
                [(start, end)] => {
                    let mut updated = contents.clone();
                    updated.replace_range(*start..*end, replace);
                    (updated, 1, Some("indentation"))
                }
                [] => {
                    // Second fallback: tolerate typographic-punctuation
                    // drift (smart quotes, em-dashes, NBSP). Picks up the
                    // copy-paste failure mode where a browser/chat client
                    // silently substituted Unicode punctuation in for the
                    // ASCII the file actually contains.
                    let punct_matches = punctuation_normalized_matches(&contents, search);
                    match punct_matches.as_slice() {
                        [] => {
                            return Err(ToolError::invalid_input(format!(
                                "Search string not found in {}. Recovery: call read_file with path=\"{path_str}\" to inspect the current contents, then retry with a search string copied from the file.",
                                file_path.display(),
                            )));
                        }
                        [(start, end)] => {
                            let mut updated = contents.clone();
                            updated.replace_range(*start..*end, replace);
                            (updated, 1, Some("punctuation"))
                        }
                        _ => {
                            return Err(ToolError::invalid_input(format!(
                                "edit_file search is non-unique after punctuation normalization: matched {} locations in {}. Recovery: call read_file with path=\"{path_str}\" and retry with surrounding lines that make the search unique.",
                                punct_matches.len(),
                                file_path.display()
                            )));
                        }
                    }
                }
                _ => {
                    return Err(ToolError::invalid_input(format!(
                        "edit_file search is non-unique after indentation normalization: matched {} locations in {}. Recovery: call read_file with path=\"{path_str}\" and retry with surrounding lines that make the search unique.",
                        indent_matches.len(),
                        file_path.display()
                    )));
                }
            }
        } else if count > 1 {
            return Err(ToolError::invalid_input(format!(
                "edit_file search is non-unique: matched {count} locations in {}. \
                 Recovery: call read_file with path=\"{path_str}\" and retry with surrounding lines that make the search unique.",
                file_path.display()
            )));
        } else {
            (contents.replace(search, replace), count, None)
        };

        crate::utils::write_atomic(&file_path, updated.as_bytes()).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;
        context.note_file_read(&file_path);

        let display = file_path.display().to_string();
        let diff = make_unified_diff(&display, &contents, &updated);
        let fuzz_note = match fuzz_kind {
            Some("indentation") => " (fuzzy indentation match)",
            Some("punctuation") => {
                " (fuzzy punctuation match — typographic quotes/dashes normalized)"
            }
            Some(other) => other,
            None => "",
        };
        let summary = format!("Replaced {count} occurrence in {display}{fuzz_note}");
        let body = if diff.is_empty() {
            format!("{summary}\n(no textual changes)")
        } else {
            format!("{diff}\n{summary}")
        };

        // Append LSP diagnostics for the edited file when enabled (#428).
        let diag_block = lsp_diagnostics_for_paths(context, &[file_path]).await;
        let full_body = if diag_block.is_empty() {
            body
        } else {
            format!("{body}\n{diag_block}")
        };

        Ok(ToolOutcome::success(full_body).with_side_effect(ToolSideEffectStatus::Applied))
    }
}

fn strip_line_leading_whitespace_with_map(input: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(input.len());
    let mut byte_map = Vec::with_capacity(input.len());
    let mut at_line_start = true;
    for (idx, ch) in input.char_indices() {
        if at_line_start && matches!(ch, ' ' | '\t') {
            continue;
        }
        normalized.push(ch);
        for _ in 0..ch.len_utf8() {
            byte_map.push(idx);
        }
        at_line_start = ch == '\n';
    }
    (normalized, byte_map)
}

fn line_start_before(input: &str, idx: usize) -> usize {
    input[..idx]
        .rfind('\n')
        .map_or(0, |newline| newline.saturating_add(1))
}

fn next_char_boundary(input: &str, idx: usize) -> usize {
    if idx >= input.len() {
        return input.len();
    }

    let mut next = idx.saturating_add(1);
    while next < input.len() && !input.is_char_boundary(next) {
        next = next.saturating_add(1);
    }
    next
}

fn leading_whitespace_fuzzy_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
    let (normalized_contents, byte_map) = strip_line_leading_whitespace_with_map(contents);
    let (normalized_search, _) = strip_line_leading_whitespace_with_map(search);
    if normalized_search.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut cursor = 0;
    while let Some(rel_idx) = normalized_contents[cursor..].find(&normalized_search) {
        let norm_start = cursor + rel_idx;
        let norm_end = norm_start + normalized_search.len();
        let Some(&mapped_start) = byte_map.get(norm_start) else {
            break;
        };
        // Use the actual match start position, expanding to line start only
        // when the match begins at a line boundary in the normalized text.
        // This prevents destroying preceding text on the same line when
        // the match starts mid-line after whitespace stripping.
        let original_start =
            if norm_start == 0 || normalized_contents.as_bytes()[norm_start - 1] == b'\n' {
                // Match starts at a line boundary — use line start for full-line replacement.
                line_start_before(contents, mapped_start)
            } else {
                // Match starts mid-line — use the exact mapped position.
                mapped_start
            };
        let original_end = byte_map.get(norm_end).copied().unwrap_or(contents.len());
        matches.push((original_start, original_end));
        cursor = next_char_boundary(&normalized_contents, norm_start);
    }
    matches
}

/// Normalize typographic punctuation to its ASCII counterpart:
///
/// * `"` `"` / U+201C U+201D → `"`
/// * `'` `'` / U+2018 U+2019 → `'`
/// * `–` `—` / U+2013 U+2014 → `-`
/// * U+00A0 (non-breaking space) → ASCII space
///
/// Returns the normalized string plus a byte-map sized to
/// `normalized.len()` whose i-th entry is the original byte offset of
/// the character that produced normalized byte i. Used to recover the
/// original-byte range after finding a match in normalized space.
fn punctuation_normalized_with_map(input: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(input.len());
    let mut byte_map = Vec::with_capacity(input.len());
    for (idx, ch) in input.char_indices() {
        let replacement: Option<char> = match ch {
            '\u{201C}' | '\u{201D}' => Some('"'),
            '\u{2018}' | '\u{2019}' => Some('\''),
            '\u{2013}' | '\u{2014}' => Some('-'),
            '\u{00A0}' => Some(' '),
            _ => None,
        };
        let written = replacement.unwrap_or(ch);
        normalized.push(written);
        for _ in 0..written.len_utf8() {
            byte_map.push(idx);
        }
    }
    (normalized, byte_map)
}

/// Try to find `search` inside `contents` after normalizing typographic
/// punctuation in both. Catches the copy-paste failure mode where a
/// browser, word processor, or chat client silently converted ASCII
/// quotes/dashes to their Unicode "pretty" forms.
fn punctuation_normalized_matches(contents: &str, search: &str) -> Vec<(usize, usize)> {
    let (norm_contents, byte_map) = punctuation_normalized_with_map(contents);
    let (norm_search, _) = punctuation_normalized_with_map(search);
    if norm_search.is_empty() {
        return Vec::new();
    }
    // If normalization didn't change anything, the exact-match pass
    // already considered this case — skip to avoid double-reporting.
    if norm_contents == contents && norm_search == search {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut cursor = 0;
    while let Some(rel_idx) = norm_contents[cursor..].find(&norm_search) {
        let norm_start = cursor + rel_idx;
        let norm_end = norm_start + norm_search.len();
        let Some(&original_start) = byte_map.get(norm_start) else {
            break;
        };
        let original_end = byte_map.get(norm_end).copied().unwrap_or(contents.len());
        matches.push((original_start, original_end));
        cursor = next_char_boundary(&norm_contents, norm_start);
    }
    matches
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
            production_read_file_host(),
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
        if !crate::tools::image_ocr::ocr_available() {
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
        if crate::tools::image_ocr::ocr_image_path(&copied).is_err() {
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

    #[tokio::test]
    async fn test_edit_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a file to edit
        let test_file = tmp.path().join("edit_me.txt");
        fs::write(&test_file, "hello world").expect("write");
        read_before_edit(&ctx, "edit_me.txt").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({"path": "edit_me.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());
        assert!(result.content.contains("Replaced 1 occurrence"));
        // Inline diff (#505) — the unified diff lands above the summary
        // line so the TUI's diff-aware renderer kicks in.
        assert!(result.content.contains("--- a/"), "{}", result.content);
        assert!(
            result.content.contains("-hello world"),
            "{}",
            result.content
        );
        assert!(result.content.contains("+hi world"), "{}", result.content);

        // Verify edit was applied
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "hi world");
    }

    #[tokio::test]
    async fn edit_file_requires_prior_read() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("blind.txt");
        fs::write(&test_file, "hello world").expect("write");

        let err = EditFileTool
            .execute(
                json!({"path": "blind.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await
            .expect_err("edit without read should fail");
        let message = err.to_string();
        assert!(message.contains("not been read"), "{message}");
        assert!(message.contains("read_file"), "{message}");

        let unchanged = fs::read_to_string(&test_file).expect("read");
        assert_eq!(unchanged, "hello world");
    }

    #[tokio::test]
    async fn edit_file_rejects_stale_prior_read() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("stale.txt");
        fs::write(&test_file, "alpha beta").expect("write");
        read_before_edit(&ctx, "stale.txt").await;
        fs::write(&test_file, "alpha beta gamma").expect("external write");

        let err = EditFileTool
            .execute(
                json!({"path": "stale.txt", "search": "alpha", "replace": "omega"}),
                &ctx,
            )
            .await
            .expect_err("stale read should fail");
        let message = err.to_string();
        assert!(message.contains("changed since"), "{message}");
        assert!(message.contains("read_file"), "{message}");

        let unchanged = fs::read_to_string(&test_file).expect("read");
        assert_eq!(unchanged, "alpha beta gamma");
    }

    #[tokio::test]
    async fn edit_file_rejects_non_unique_exact_match() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("multi.txt");
        fs::write(&test_file, "hello world hello").expect("write");
        read_before_edit(&ctx, "multi.txt").await;

        let err = EditFileTool
            .execute(
                json!({"path": "multi.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await
            .expect_err("non-unique exact match should fail");
        let message = err.to_string();
        assert!(message.contains("non-unique"), "{message}");
        assert!(message.contains("matched 2"), "{message}");
        assert!(message.contains("read_file"), "{message}");

        let unchanged = fs::read_to_string(&test_file).expect("read");
        assert_eq!(unchanged, "hello world hello");
    }

    #[tokio::test]
    async fn test_edit_file_accepts_omitted_and_explicit_fuzz() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let tool = EditFileTool;

        for (file_name, fuzz) in [
            ("fuzz_omitted.txt", None),
            ("fuzz_false.txt", Some(false)),
            ("fuzz_true.txt", Some(true)),
        ] {
            let test_file = tmp.path().join(file_name);
            fs::write(&test_file, "hello world").expect("write");
            read_before_edit(&ctx, file_name).await;

            let mut input = serde_json::Map::from_iter([
                ("path".to_string(), json!(file_name)),
                ("search".to_string(), json!("hello")),
                ("replace".to_string(), json!("hi")),
            ]);
            if let Some(fuzz) = fuzz {
                input.insert("fuzz".to_string(), json!(fuzz));
            }

            let result = tool
                .execute(Value::Object(input), &ctx)
                .await
                .expect("execute");

            assert!(result.is_success(), "{file_name}: {}", result.content);
            assert!(result.content.contains("Replaced 1 occurrence"));
            let edited = fs::read_to_string(&test_file).expect("read");
            assert_eq!(edited, "hi world");
        }
    }

    #[tokio::test]
    async fn test_edit_file_single_match_has_no_multi_match_warning() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("single.txt");
        fs::write(&test_file, "hello world").expect("write");
        read_before_edit(&ctx, "single.txt").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({"path": "single.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());
        assert!(result.content.contains("Replaced 1 occurrence"));
        assert!(!result.content.contains("multiple matches were replaced"));
    }

    #[tokio::test]
    async fn test_edit_file_fuzz_tolerates_leading_whitespace() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("fuzzy.txt");
        fs::write(
            &test_file,
            "fn main() {\n    if true {\n        let value = 1;\n    }\n}\n",
        )
        .expect("write");
        read_before_edit(&ctx, "fuzzy.txt").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "fuzzy.txt",
                    "search": "if true {\n    let value = 1;\n}",
                    "replace": "    if true {\n        let value = 2;\n    }",
                    "fuzz": true
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());
        assert!(result.content.contains("fuzzy indentation match"));
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(
            edited,
            "fn main() {\n    if true {\n        let value = 2;\n    }\n}\n"
        );
    }

    #[tokio::test]
    async fn test_edit_file_fuzz_tolerates_leading_whitespace_after_multibyte_start() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("fuzzy_cjk.txt");
        fs::write(&test_file, "数据\n").expect("write");
        read_before_edit(&ctx, "fuzzy_cjk.txt").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "fuzzy_cjk.txt",
                    "search": "    数据",
                    "replace": "记录",
                    "fuzz": true
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success(), "{}", result.content);
        assert!(result.content.contains("fuzzy indentation match"));
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "记录\n");
    }

    #[tokio::test]
    async fn test_edit_file_fuzz_tolerates_smart_quote_substitution() {
        // The file on disk has ASCII quotes. The search comes from a
        // browser paste with curly quotes. Exact match fails; the
        // punctuation-normalized fallback should still land the edit.
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("smart.rs");
        fs::write(&test_file, "let s = \"hello world\";\n").expect("write");
        read_before_edit(&ctx, "smart.rs").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "smart.rs",
                    // \u{201C} \u{201D} are the curly double-quote pair.
                    "search": "let s = \u{201C}hello world\u{201D};",
                    "replace": "let s = \"hello universe\";",
                    "fuzz": true
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success(), "fuzzy punctuation edit should succeed");
        assert!(
            result.content.contains("fuzzy punctuation match"),
            "expected punctuation-fuzz note, got: {}",
            result.content
        );
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "let s = \"hello universe\";\n");
    }

    #[tokio::test]
    async fn test_edit_file_fuzz_tolerates_smart_quote_after_multibyte_start() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("smart_cjk.md");
        fs::write(&test_file, "数据 \"x\"\n").expect("write");
        read_before_edit(&ctx, "smart_cjk.md").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "smart_cjk.md",
                    "search": "数据 \u{201C}x\u{201D}",
                    "replace": "数据 y",
                    "fuzz": true
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success(), "{}", result.content);
        assert!(result.content.contains("fuzzy punctuation match"));
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "数据 y\n");
    }

    #[tokio::test]
    async fn test_edit_file_fuzz_tolerates_em_dash_and_nbsp() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("dash.md");
        // File has an ASCII hyphen and ASCII space.
        fs::write(&test_file, "alpha - beta\n").expect("write");
        read_before_edit(&ctx, "dash.md").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "dash.md",
                    // Search uses em-dash + NBSP, common after a copy-paste
                    // from a styled document.
                    "search": "alpha\u{00A0}\u{2014}\u{00A0}beta",
                    "replace": "alpha - gamma",
                    "fuzz": true
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.is_success());
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "alpha - gamma\n");
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a file without the search string
        let test_file = tmp.path().join("no_match.txt");
        fs::write(&test_file, "foo bar baz").expect("write");
        read_before_edit(&ctx, "no_match.txt").await;

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({"path": "no_match.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("read_file"));
    }

    #[tokio::test]
    async fn test_edit_file_rejects_identical_search_and_replace() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("same.txt");
        fs::write(&test_file, "a := \"foo\"").expect("write");

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({
                    "path": "same.txt",
                    "search": "a := \"foo\"",
                    "replace": "a := \"foo\""
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("search and replace are identical"),
            "error must explain the no-op input: {err}"
        );
        let unchanged = fs::read_to_string(&test_file).expect("read");
        assert_eq!(unchanged, "a := \"foo\"");
    }

    /// #157 — When the model uses `replacement` instead of `replace`,
    /// the error should name the provided fields so the model can
    /// self-correct without a second round-trip.
    #[tokio::test]
    async fn test_edit_file_wrong_param_name_shows_provided_fields() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("test.txt");
        fs::write(&test_file, "hello world").expect("write");

        let tool = EditFileTool;
        // Model uses `replacement` instead of `replace`.
        let result = tool
            .execute(
                json!({"path": "test.txt", "search": "hello", "replacement": "hi"}),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // The error must name both the missing field AND the provided ones.
        assert!(
            err.contains("missing required field 'replace'"),
            "error must name the missing field: {err}"
        );
        assert!(
            err.contains("Input provided:") || err.contains("provided:"),
            "error must list the fields the model did supply: {err}"
        );
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
