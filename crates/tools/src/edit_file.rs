//! Production `edit_file` search/replace operation.

use std::fs;

use codewhale_protocol::agent_runtime::ToolSideEffectStatus;
use serde_json::Value;

use crate::{
    ProductionToolContext, ToolError, ToolOutcome, make_unified_diff, optional_bool, required_str,
};

/// Execute the production search-and-replace operation.
pub fn execute_edit_file(
    input: Value,
    context: &ProductionToolContext,
) -> Result<ToolOutcome, ToolError> {
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

    crate::write_atomic(&file_path, updated.as_bytes()).map_err(|e| {
        ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
    })?;
    context.note_file_read(&file_path);

    let display = file_path.display().to_string();
    let diff = make_unified_diff(&display, &contents, &updated);
    let fuzz_note = match fuzz_kind {
        Some("indentation") => " (fuzzy indentation match)",
        Some("punctuation") => " (fuzzy punctuation match — typographic quotes/dashes normalized)",
        Some(other) => other,
        None => "",
    };
    let summary = format!("Replaced {count} occurrence in {display}{fuzz_note}");
    let body = if diff.is_empty() {
        format!("{summary}\n(no textual changes)")
    } else {
        format!("{diff}\n{summary}")
    };

    Ok(ToolOutcome::success(body).with_side_effect(ToolSideEffectStatus::Applied))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::{ReadFileHost, execute_read_file};
    use serde_json::json;
    use tempfile::tempdir;

    fn prefer_bundled_pdf() -> bool {
        false
    }

    fn unavailable_ocr(_path: &Path) -> Result<String, ToolError> {
        Err(ToolError::not_available(
            "OCR is not used by edit_file tests",
        ))
    }

    fn read_file_host() -> ReadFileHost {
        ReadFileHost::new(prefer_bundled_pdf, unavailable_ocr)
    }

    async fn read_before_edit(context: &ProductionToolContext, path: &str) {
        execute_read_file(json!({"path": path}), context, read_file_host())
            .expect("read before edit");
    }

    struct TestEditFileTool;

    impl TestEditFileTool {
        async fn execute(
            &self,
            input: Value,
            context: &ProductionToolContext,
        ) -> Result<ToolOutcome, ToolError> {
            execute_edit_file(input, context)
        }
    }

    #[tokio::test]
    async fn edit_file_characterization_is_byte_exact_fresh_and_workspace_bound() {
        let expected: Value =
            serde_json::from_str(include_str!("../tests/edit_file_characterization.json"))
                .expect("characterization fixture");
        let workspace = tempdir().expect("workspace");
        let context = ProductionToolContext::new(workspace.path().to_path_buf());
        let tool = TestEditFileTool;

        let sample = workspace.path().join("sample.txt");
        fs::write(&sample, b"alpha\nbeta\ngamma\n").expect("sample fixture");
        read_before_edit(&context, "sample.txt").await;
        let success = tool
            .execute(
                json!({"path": "sample.txt", "search": "beta", "replace": "BETA"}),
                &context,
            )
            .await
            .expect("exact edit");
        let display = context
            .resolve_path("sample.txt")
            .expect("resolved sample")
            .display()
            .to_string();
        assert_eq!(
            success.content,
            expected["success_content_template"]
                .as_str()
                .expect("success template")
                .replace("{path}", &display)
        );
        assert_eq!(success.side_effect, ToolSideEffectStatus::Applied);
        assert_eq!(success.metadata, None);
        assert_eq!(
            fs::read(&sample).expect("sample result"),
            expected["success_file_bytes"]
                .as_str()
                .expect("file bytes")
                .as_bytes()
        );

        // A successful edit refreshes the read snapshot, so the next exact
        // edit is allowed without another read_file call.
        tool.execute(
            json!({"path": "sample.txt", "search": "BETA", "replace": "beta-2"}),
            &context,
        )
        .await
        .expect("freshness refresh after write");
        assert_eq!(
            fs::read(&sample).expect("second result"),
            b"alpha\nbeta-2\ngamma\n"
        );

        let identical = tool
            .execute(
                json!({"path": "sample.txt", "search": "same", "replace": "same"}),
                &context,
            )
            .await
            .expect_err("identical input");
        assert_eq!(identical.to_string(), expected["identical_error"]);

        let missing_replace = tool
            .execute(
                json!({"path": "sample.txt", "search": "beta", "replacement": "BETA"}),
                &context,
            )
            .await
            .expect_err("missing replace");
        assert_eq!(
            missing_replace.to_string(),
            expected["missing_replace_error"]
        );

        let unread = workspace.path().join("unread.txt");
        fs::write(&unread, b"before").expect("unread fixture");
        let unread_error = tool
            .execute(
                json!({"path": "unread.txt", "search": "before", "replace": "after"}),
                &context,
            )
            .await
            .expect_err("prior read required");
        assert_eq!(
            unread_error.to_string(),
            expected["unread_error_template"]
                .as_str()
                .expect("unread template")
                .replace(
                    "{path}",
                    &context
                        .resolve_path("unread.txt")
                        .expect("resolved unread")
                        .display()
                        .to_string(),
                )
        );
        assert_eq!(fs::read(&unread).expect("unread unchanged"), b"before");

        let stale = workspace.path().join("stale.txt");
        fs::write(&stale, b"before").expect("stale fixture");
        read_before_edit(&context, "stale.txt").await;
        fs::write(&stale, b"externally changed").expect("external write");
        let stale_error = tool
            .execute(
                json!({"path": "stale.txt", "search": "before", "replace": "after"}),
                &context,
            )
            .await
            .expect_err("stale read rejected");
        assert_eq!(
            stale_error.to_string(),
            expected["stale_error_template"]
                .as_str()
                .expect("stale template")
                .replace(
                    "{path}",
                    &context
                        .resolve_path("stale.txt")
                        .expect("resolved stale")
                        .display()
                        .to_string(),
                )
        );
        assert_eq!(
            fs::read(&stale).expect("stale unchanged"),
            b"externally changed"
        );

        let multi = workspace.path().join("multi.txt");
        fs::write(&multi, b"same and same").expect("multi fixture");
        read_before_edit(&context, "multi.txt").await;
        let non_unique = tool
            .execute(
                json!({"path": "multi.txt", "search": "same", "replace": "new"}),
                &context,
            )
            .await
            .expect_err("non-unique search rejected");
        assert_eq!(
            non_unique.to_string(),
            expected["non_unique_error_template"]
                .as_str()
                .expect("non-unique template")
                .replace(
                    "{path}",
                    &context
                        .resolve_path("multi.txt")
                        .expect("resolved multi")
                        .display()
                        .to_string(),
                )
        );
        assert_eq!(fs::read(&multi).expect("multi unchanged"), b"same and same");

        let missing = workspace.path().join("missing.txt");
        fs::write(&missing, b"actual contents").expect("missing fixture");
        read_before_edit(&context, "missing.txt").await;
        let not_found = tool
            .execute(
                json!({"path": "missing.txt", "search": "absent", "replace": "new"}),
                &context,
            )
            .await
            .expect_err("search not found");
        assert_eq!(
            not_found.to_string(),
            expected["not_found_error_template"]
                .as_str()
                .expect("not-found template")
                .replace(
                    "{path}",
                    &context
                        .resolve_path("missing.txt")
                        .expect("resolved missing")
                        .display()
                        .to_string(),
                )
        );
        assert_eq!(
            fs::read(&missing).expect("not-found unchanged"),
            b"actual contents"
        );

        let outside = workspace.path().parent().unwrap().join(format!(
            "{}-edit-outside.txt",
            workspace.path().file_name().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_file(&outside);
        let escaped = tool
            .execute(
                json!({"path": outside, "search": "before", "replace": "after"}),
                &context,
            )
            .await
            .expect_err("outside path rejected");
        assert!(matches!(escaped, ToolError::PathEscape { .. }));
        assert!(
            !outside.exists(),
            "path rejection must not mutate outside workspace"
        );
    }

    #[tokio::test]
    async fn test_edit_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        // Create a file to edit
        let test_file = tmp.path().join("edit_me.txt");
        fs::write(&test_file, "hello world").expect("write");
        read_before_edit(&ctx, "edit_me.txt").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("blind.txt");
        fs::write(&test_file, "hello world").expect("write");

        let err = TestEditFileTool
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("stale.txt");
        fs::write(&test_file, "alpha beta").expect("write");
        read_before_edit(&ctx, "stale.txt").await;
        fs::write(&test_file, "alpha beta gamma").expect("external write");

        let err = TestEditFileTool
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("multi.txt");
        fs::write(&test_file, "hello world hello").expect("write");
        read_before_edit(&ctx, "multi.txt").await;

        let err = TestEditFileTool
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());
        let tool = TestEditFileTool;

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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("single.txt");
        fs::write(&test_file, "hello world").expect("write");
        read_before_edit(&ctx, "single.txt").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("fuzzy.txt");
        fs::write(
            &test_file,
            "fn main() {\n    if true {\n        let value = 1;\n    }\n}\n",
        )
        .expect("write");
        read_before_edit(&ctx, "fuzzy.txt").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("fuzzy_cjk.txt");
        fs::write(&test_file, "数据\n").expect("write");
        read_before_edit(&ctx, "fuzzy_cjk.txt").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("smart.rs");
        fs::write(&test_file, "let s = \"hello world\";\n").expect("write");
        read_before_edit(&ctx, "smart.rs").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("smart_cjk.md");
        fs::write(&test_file, "数据 \"x\"\n").expect("write");
        read_before_edit(&ctx, "smart_cjk.md").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("dash.md");
        // File has an ASCII hyphen and ASCII space.
        fs::write(&test_file, "alpha - beta\n").expect("write");
        read_before_edit(&ctx, "dash.md").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        // Create a file without the search string
        let test_file = tmp.path().join("no_match.txt");
        fs::write(&test_file, "foo bar baz").expect("write");
        read_before_edit(&ctx, "no_match.txt").await;

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("same.txt");
        fs::write(&test_file, "a := \"foo\"").expect("write");

        let tool = TestEditFileTool;
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
        let ctx = ProductionToolContext::new(tmp.path().to_path_buf());

        let test_file = tmp.path().join("test.txt");
        fs::write(&test_file, "hello world").expect("write");

        let tool = TestEditFileTool;
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
}
