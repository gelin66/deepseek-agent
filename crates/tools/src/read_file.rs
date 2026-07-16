#[cfg(feature = "pdf")]
use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::{ProductionToolContext, ToolError, ToolOutcome, optional_str, required_str};

/// Execute the production `read_file` operation against a workspace context.
pub fn execute_read_file(
    input: Value,
    context: &ProductionToolContext,
    prefer_external_pdftotext: bool,
) -> Result<ToolOutcome, ToolError> {
    let requested_path = required_str(&input, "path")?;
    let file_path = context.resolve_path(requested_path)?;
    let pages = optional_str(&input, "pages");

    if is_pdf(&file_path)? {
        return read_pdf(&file_path, pages, prefer_external_pdftotext);
    }
    if is_image_for_ocr(&file_path) {
        return read_image_via_ocr(&file_path, requested_path);
    }

    // Open before parameter parsing so a missing file keeps the historical
    // "Failed to read …" error shape regardless of the other arguments.
    let file = fs::File::open(&file_path).map_err(|error| {
        ToolError::execution_failed(format!("Failed to read {}: {}", file_path.display(), error))
    })?;
    let file_bytes = file.metadata().map(|meta| meta.len()).unwrap_or(u64::MAX);

    let explicit_range = input
        .get("start_line")
        .or_else(|| input.get("max_lines"))
        .is_some();

    if !explicit_range && file_bytes <= SMALL_FILE_BYTES as u64 {
        drop(file);
        let contents = fs::read_to_string(&file_path).map_err(|error| {
            ToolError::execution_failed(format!(
                "Failed to read {}: {}",
                file_path.display(),
                error
            ))
        })?;
        context.note_file_read(&file_path);

        let total_lines = contents.lines().count();
        if total_lines <= SMALL_FILE_LINES {
            return Ok(ToolOutcome::success(contents));
        }

        let window: Vec<String> = contents
            .lines()
            .take(DEFAULT_READ_LINES)
            .map(str::to_string)
            .collect();
        return Ok(render_line_window(
            requested_path,
            &window,
            total_lines,
            1,
            DEFAULT_READ_LINES,
        ));
    }

    let start_line = match input.get("start_line").and_then(Value::as_u64) {
        Some(0) => {
            return Err(ToolError::invalid_input(
                "start_line must be 1-based and greater than 0".to_string(),
            ));
        }
        Some(value) => usize::try_from(value).map_err(|_| {
            ToolError::invalid_input("start_line exceeds platform addressable range".to_string())
        })?,
        None => 1,
    };

    let max_lines = match input.get("max_lines").and_then(Value::as_u64) {
        Some(0) => {
            return Err(ToolError::invalid_input(
                "max_lines must be greater than 0".to_string(),
            ));
        }
        Some(value) => {
            let converted = usize::try_from(value).map_err(|_| {
                ToolError::invalid_input("max_lines exceeds platform addressable range".to_string())
            })?;
            std::cmp::min(converted, HARD_MAX_READ_LINES)
        }
        None => DEFAULT_READ_LINES,
    };

    let (window, total_lines) =
        read_window_streaming(file, start_line, max_lines).map_err(|error| {
            ToolError::execution_failed(format!(
                "Failed to read {}: {}",
                file_path.display(),
                error
            ))
        })?;
    context.note_file_read(&file_path);

    if start_line > total_lines {
        let output = format!(
            "<file path=\"{requested_path}\" total_lines=\"{total_lines}\" shown_lines=\"none\" truncated=\"false\">\n\
             \n\
             [NO CONTENT] start_line {start_line} is beyond total_lines {total_lines}.\n\
             </file>"
        );
        return Ok(ToolOutcome::success(output));
    }

    Ok(render_line_window(
        requested_path,
        &window,
        total_lines,
        start_line,
        max_lines,
    ))
}

const DEFAULT_READ_LINES: usize = 200;
const HARD_MAX_READ_LINES: usize = 500;
const MAX_VISIBLE_BYTES: usize = 16 * 1024;
const SMALL_FILE_LINES: usize = 200;
const SMALL_FILE_BYTES: usize = 16 * 1024;

fn read_window_streaming(
    file: fs::File,
    start_line: usize,
    max_lines: usize,
) -> std::io::Result<(Vec<String>, usize)> {
    use std::io::BufRead;

    let mut reader = std::io::BufReader::new(file);
    let mut raw: Vec<u8> = Vec::new();
    let mut window: Vec<String> = Vec::new();
    let mut total_lines = 0usize;
    let start_idx = start_line - 1;

    loop {
        raw.clear();
        let read = reader.read_until(b'\n', &mut raw)?;
        if read == 0 {
            break;
        }
        let mut end = raw.len();
        if raw[..end].ends_with(b"\n") {
            end -= 1;
            if raw[..end].ends_with(b"\r") {
                end -= 1;
            }
        }
        let line = std::str::from_utf8(&raw[..end]).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )
        })?;
        if total_lines >= start_idx && window.len() < max_lines {
            window.push(line.to_string());
        }
        total_lines += 1;
    }

    Ok((window, total_lines))
}

fn render_line_window(
    requested_path: &str,
    window: &[String],
    total_lines: usize,
    start_line: usize,
    max_lines: usize,
) -> ToolOutcome {
    let zero_based_start = start_line - 1;
    let zero_based_end = std::cmp::min(zero_based_start + max_lines, total_lines);
    let shown_first = start_line;
    let shown_last = zero_based_end;

    let mut numbered = String::new();
    for (offset, line) in window.iter().enumerate() {
        let line_no = start_line + offset;
        numbered.push_str(&format!("{line_no:>6}│ {line}\n"));
    }

    let truncated_by_bytes = numbered.len() > MAX_VISIBLE_BYTES;
    let shown_content = if truncated_by_bytes {
        let mut end = MAX_VISIBLE_BYTES;
        while end > 0 && !numbered.is_char_boundary(end) {
            end -= 1;
        }
        &numbered[..end]
    } else {
        &numbered
    };

    let truncated_by_lines = zero_based_end < total_lines;
    let truncated = truncated_by_lines || truncated_by_bytes;
    let next_start = zero_based_end + 1;

    let mut attrs = format!(
        "path=\"{requested_path}\" total_lines=\"{total_lines}\" shown_lines=\"{shown_first}-{shown_last}\" truncated=\"{truncated}\""
    );
    if truncated_by_lines {
        attrs.push_str(&format!(" next_start_line=\"{next_start}\""));
    }

    let mut output = format!("<file {attrs}>\n{shown_content}");
    if truncated_by_lines {
        output.push_str(&format!(
            "\n[TRUNCATED] Showing lines {shown_first}-{shown_last} of {total_lines}. To continue, call read_file with path=\"{requested_path}\" start_line={next_start} max_lines={max_lines}\n"
        ));
    }
    if truncated_by_bytes {
        output.push_str(
            "\n[TRUNCATED] The selected range exceeded 16KB. Continue with a smaller max_lines value.\n",
        );
    }
    output.push_str("</file>");

    ToolOutcome::success(output)
}

fn read_image_via_ocr(path: &Path, requested_path: &str) -> Result<ToolOutcome, ToolError> {
    let text = crate::ocr_image_path(path)?;
    Ok(render_image_ocr(requested_path, &text))
}

fn render_image_ocr(requested_path: &str, text: &str) -> ToolOutcome {
    ToolOutcome::success(format!(
        "<image_ocr path=\"{requested_path}\">\n{text}\n</image_ocr>"
    ))
}

fn is_pdf(path: &Path) -> Result<bool, ToolError> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return Ok(true);
    }
    let mut buffer = [0u8; 4];
    let result = match fs::File::open(path) {
        Ok(mut file) => {
            use std::io::Read;
            file.read_exact(&mut buffer).map(|_| buffer)
        }
        Err(_) => return Ok(false),
    };
    Ok(matches!(result, Ok(bytes) if &bytes == b"%PDF"))
}

fn is_image_for_ocr(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp"
            )
        })
}

fn parse_pages_arg(spec: &str) -> Option<(u32, u32)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((start, end)) = trimmed.split_once('-') {
        let start: u32 = start.trim().parse().ok()?;
        let end: u32 = end.trim().parse().ok()?;
        if start == 0 || end < start {
            return None;
        }
        Some((start, end))
    } else {
        let page: u32 = trimmed.parse().ok()?;
        if page == 0 {
            return None;
        }
        Some((page, page))
    }
}

fn clean_pdf_text(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut blank_run = 0usize;
    let mut any_content = false;
    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run = blank_run.saturating_add(1);
            if blank_run <= 1 {
                output.push('\n');
            }
        } else {
            blank_run = 0;
            any_content = true;
            for character in trimmed.chars() {
                match character {
                    '\0' => output.push('\u{FFFD}'),
                    '\u{A0}' => output.push(' '),
                    other => output.push(other),
                }
            }
            output.push('\n');
        }
    }
    if any_content {
        let start = output
            .find(|character: char| character != '\n')
            .unwrap_or(0);
        let end = output
            .rfind(|character: char| character != '\n')
            .map_or(output.len(), |index| {
                index + output[index..].chars().next().map_or(1, char::len_utf8)
            });
        output[start..end].to_string()
    } else {
        String::new()
    }
}

fn read_pdf(
    path: &Path,
    pages: Option<&str>,
    prefer_external_pdftotext: bool,
) -> Result<ToolOutcome, ToolError> {
    let page_range = match pages {
        Some(spec) => match parse_pages_arg(spec) {
            Some(range) => Some(range),
            None => {
                return Err(ToolError::invalid_input(format!(
                    "invalid `pages` value `{spec}` (expected `N` or `N-M`, e.g. `1-5`)"
                )));
            }
        },
        None => None,
    };

    if prefer_external_pdftotext {
        read_pdf_via_pdftotext(path, page_range)
    } else {
        #[cfg(feature = "pdf")]
        {
            read_pdf_via_pdf_extract(path, page_range)
        }
        #[cfg(not(feature = "pdf"))]
        {
            read_pdf_via_pdftotext(path, page_range)
        }
    }
}

#[cfg(feature = "pdf")]
fn read_pdf_via_pdf_extract(
    path: &Path,
    page_range: Option<(u32, u32)>,
) -> Result<ToolOutcome, ToolError> {
    let text = if let Some((start, end)) = page_range {
        let pages = guard_pdf_extract(|| pdf_extract::extract_text_by_pages(path)).map_err(|error| {
            ToolError::execution_failed(format!(
                "pdf-extract failed on {}: {error} (set `prefer_external_pdftotext = true` in settings.toml to retry via pdftotext)",
                path.display()
            ))
        })?;
        let total = pages.len();
        if total == 0 {
            String::new()
        } else {
            let start_index = (start as usize).saturating_sub(1).min(total);
            let end_index = (end as usize).min(total);
            if start_index >= end_index {
                String::new()
            } else {
                pages[start_index..end_index].join("\n")
            }
        }
    } else {
        guard_pdf_extract(|| pdf_extract::extract_text_by_pages(path))
            .map(|pages| pages.join("\n"))
            .map_err(|error| {
                ToolError::execution_failed(format!(
                    "pdf-extract failed on {}: {error} (set `prefer_external_pdftotext = true` in settings.toml to retry via pdftotext)",
                    path.display()
                ))
            })?
    };
    Ok(ToolOutcome::success(clean_pdf_text(&text)))
}

#[cfg(feature = "pdf")]
fn guard_pdf_extract<T, E, F>(extract: F) -> Result<T, String>
where
    E: Display,
    F: FnOnce() -> Result<T, E>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(extract)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(error.to_string()),
        Err(payload) => Err(format!(
            "extractor panicked: {}",
            panic_payload_message(payload.as_ref())
        )),
    }
}

#[cfg(feature = "pdf")]
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn read_pdf_via_pdftotext(
    path: &Path,
    page_range: Option<(u32, u32)>,
) -> Result<ToolOutcome, ToolError> {
    let mut command = Command::new("pdftotext");
    command.arg("-layout");

    if let Some((start, end)) = page_range {
        command.arg("-f").arg(start.to_string());
        command.arg("-l").arg(end.to_string());
    }

    command.arg(path).arg("-");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let payload = json!({
                "type": "binary_unavailable",
                "path": path.display().to_string(),
                "kind": "pdf",
                "reason": "pdftotext not installed (prefer_external_pdftotext = true in settings)",
                "hint": "install poppler (macOS: `brew install poppler`; Debian/Ubuntu: `apt install poppler-utils`) — or unset `prefer_external_pdftotext` to use the bundled pure-Rust extractor"
            });
            return Ok(ToolOutcome::error(payload.to_string()).with_metadata(payload));
        }
        Err(error) => {
            return Err(ToolError::execution_failed(format!(
                "failed to launch pdftotext: {error}"
            )));
        }
    };

    let output = child.wait_with_output().map_err(|error| {
        ToolError::execution_failed(format!("pdftotext failed to complete: {error}"))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ToolError::execution_failed(format!(
            "pdftotext failed (exit {:?}): {stderr}",
            output.status.code()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(ToolOutcome::success(clean_pdf_text(&text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    fn outcome(context: &ProductionToolContext, input: Value) -> Result<ToolOutcome, ToolError> {
        execute_read_file(input, context, false)
    }

    #[test]
    fn characterization_outputs_are_byte_exact() {
        let expected: Value =
            serde_json::from_str(include_str!("../tests/read_file_characterization.json"))
                .expect("characterization fixture");
        let workspace = tempdir().expect("workspace");
        let context = ProductionToolContext::new(workspace.path());

        fs::write(
            workspace.path().join("small.txt"),
            "line 1\nline 2\nline 3\n",
        )
        .expect("small fixture");
        let small = outcome(&context, json!({"path": "small.txt"})).expect("small read");
        assert_eq!(small.content, expected["small"]);

        let ranged_body: String = (1..=10).map(|line| format!("line {line}\n")).collect();
        fs::write(workspace.path().join("ranged.txt"), ranged_body).expect("ranged fixture");
        let ranged = outcome(
            &context,
            json!({"path": "ranged.txt", "start_line": 3, "max_lines": 4}),
        )
        .expect("ranged read");
        assert_eq!(ranged.content, expected["ranged"]);

        fs::write(workspace.path().join("short.txt"), "only\nthree\nlines\n")
            .expect("short fixture");
        let past_end = outcome(&context, json!({"path": "short.txt", "start_line": 99}))
            .expect("past-end read");
        assert_eq!(past_end.content, expected["past_end"]);

        let ocr = render_image_ocr("ocr.png", "HELLO OCR");
        assert_eq!(ocr.content, expected["ocr"]);

        fs::write(workspace.path().join("document.pdf"), b"not a PDF")
            .expect("PDF extension fixture");
        let invalid_pages = outcome(&context, json!({"path": "document.pdf", "pages": "1-"}))
            .expect_err("invalid pages");
        assert_eq!(invalid_pages.to_string(), expected["invalid_pages"]);

        let missing_path = outcome(&context, json!({})).expect_err("missing path");
        assert_eq!(missing_path.to_string(), expected["missing_path"]);
    }

    #[test]
    fn small_file_returns_raw_contents_and_records_freshness() {
        let workspace = tempdir().expect("workspace");
        let file = workspace.path().join("test.txt");
        fs::write(&file, "hello world").expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let result = outcome(&context, json!({"path": "test.txt"})).expect("read");

        assert!(result.is_success());
        assert_eq!(result.content, "hello world");
        let resolved = context
            .resolve_path("test.txt")
            .expect("resolved test path");
        context
            .require_fresh_file_read(&resolved, "test.txt")
            .expect("read must record the edit precondition");
    }

    #[test]
    fn missing_file_error_precedes_range_validation() {
        let workspace = tempdir().expect("workspace");
        let context = ProductionToolContext::new(workspace.path());

        let error = outcome(
            &context,
            json!({"path": "missing.txt", "start_line": 0, "max_lines": 0}),
        )
        .expect_err("missing file");
        let message = error.to_string();
        assert!(message.contains("Failed to read"), "{message}");
        assert!(message.contains("missing.txt"), "{message}");
        assert!(!message.contains("start_line must"), "{message}");
    }

    #[test]
    fn explicit_range_uses_one_based_lines_and_pagination_metadata() {
        let workspace = tempdir().expect("workspace");
        let body: String = (1..=10).map(|line| format!("line {line}\n")).collect();
        fs::write(workspace.path().join("ranged.txt"), body).expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let result = outcome(
            &context,
            json!({"path": "ranged.txt", "start_line": 3, "max_lines": 4}),
        )
        .expect("read");

        assert!(result.content.contains("shown_lines=\"3-6\""));
        assert!(result.content.contains("next_start_line=\"7\""));
        assert!(result.content.contains("     3│ line 3"));
        assert!(result.content.contains("     6│ line 6"));
        assert!(!result.content.contains("     7│ line 7"));
    }

    #[test]
    fn zero_ranges_are_rejected_and_max_lines_is_hard_capped() {
        let workspace = tempdir().expect("workspace");
        let body: String = (1..=600).map(|line| format!("L{line}\n")).collect();
        fs::write(workspace.path().join("lines.txt"), body).expect("write");
        let context = ProductionToolContext::new(workspace.path());

        assert!(outcome(&context, json!({"path": "lines.txt", "start_line": 0})).is_err());
        assert!(outcome(&context, json!({"path": "lines.txt", "max_lines": 0})).is_err());

        let capped = outcome(&context, json!({"path": "lines.txt", "max_lines": 5000}))
            .expect("capped read");
        assert!(capped.content.contains("   500│ L500"));
        assert!(!capped.content.contains("   501│ L501"));
        assert!(capped.content.contains("next_start_line=\"501\""));
    }

    #[test]
    fn large_file_streaming_preserves_default_and_explicit_windows() {
        let workspace = tempdir().expect("workspace");
        let body: String = (1..=2000)
            .map(|line| format!("line {line} {}\n", "x".repeat(20)))
            .collect();
        assert!(body.len() > SMALL_FILE_BYTES);
        fs::write(workspace.path().join("large.txt"), body).expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let explicit = outcome(
            &context,
            json!({"path": "large.txt", "start_line": 1500, "max_lines": 10}),
        )
        .expect("explicit window");
        assert!(explicit.content.contains("total_lines=\"2000\""));
        assert!(explicit.content.contains("shown_lines=\"1500-1509\""));
        assert!(explicit.content.contains("next_start_line=\"1510\""));
        assert!(explicit.content.contains("  1500│ line 1500"));
        assert!(explicit.content.contains("  1509│ line 1509"));
        assert!(!explicit.content.contains("  1510│"));

        let default = outcome(&context, json!({"path": "large.txt"})).expect("default window");
        assert!(default.content.contains("shown_lines=\"1-200\""));
        assert!(default.content.contains("next_start_line=\"201\""));
    }

    #[test]
    fn streaming_rejects_invalid_utf8_even_outside_selected_window() {
        let workspace = tempdir().expect("workspace");
        let mut bytes = b"good line\n".repeat(5);
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
        fs::write(workspace.path().join("mixed.bin"), bytes).expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let error = outcome(
            &context,
            json!({"path": "mixed.bin", "start_line": 1, "max_lines": 2}),
        )
        .expect_err("invalid UTF-8");
        let message = error.to_string();
        assert!(message.contains("Failed to read"), "{message}");
        assert!(message.contains("valid UTF-8"), "{message}");
    }

    #[test]
    fn pdf_detection_accepts_extension_and_magic_without_false_positive() {
        let workspace = tempdir().expect("workspace");
        let extension = workspace.path().join("paper.PDF");
        fs::write(&extension, b"not really a PDF").expect("write");
        assert!(is_pdf(&extension).expect("extension detection"));

        let magic = workspace.path().join("blob");
        fs::write(&magic, b"%PDF-1.7\nrest").expect("write");
        assert!(is_pdf(&magic).expect("magic detection"));

        let text = workspace.path().join("notes.txt");
        fs::write(&text, b"hello").expect("write");
        assert!(!is_pdf(&text).expect("plain text detection"));
    }

    #[test]
    fn pages_parser_accepts_closed_positive_ranges_only() {
        assert_eq!(parse_pages_arg("3"), Some((3, 3)));
        assert_eq!(parse_pages_arg(" 1 - 5 "), Some((1, 5)));
        for invalid in ["", " ", "0", "0-3", "5-1", "1-", "-5", "-3-5", "abc", "3.5"] {
            assert_eq!(parse_pages_arg(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn image_route_matches_the_tools_owned_ocr_backend() {
        let workspace = tempdir().expect("workspace");
        fs::write(workspace.path().join("image.png"), b"fake image").expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let direct = crate::ocr_image_path(&workspace.path().join("image.png"));
        let dispatched = execute_read_file(json!({"path": "image.png"}), &context, false);
        assert_eq!(
            dispatched
                .map(|outcome| outcome.content)
                .map_err(|error| error.to_string()),
            direct
                .map(|text| render_image_ocr("image.png", &text).content)
                .map_err(|error| error.to_string())
        );
    }

    #[test]
    fn pdf_text_cleanup_preserves_content_contract() {
        assert_eq!(
            clean_pdf_text("line1\n\n\n\nline2\n\n\nline3"),
            "line1\n\nline2\n\nline3"
        );
        assert_eq!(clean_pdf_text("hello\0world"), "hello�world");
        assert_eq!(clean_pdf_text("hello\u{a0}world"), "hello world");
        assert_eq!(clean_pdf_text("hello   "), "hello");
        assert_eq!(
            clean_pdf_text("   indented line\nregular line"),
            "   indented line\nregular line"
        );
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn pdf_extract_panic_is_returned_as_tool_error_text() {
        let error = guard_pdf_extract(|| -> Result<String, &'static str> {
            panic!("assertion failed: name == \"Identity-H\"");
        })
        .expect_err("panic should become an error");
        assert!(error.contains("extractor panicked"));
        assert!(error.contains("Identity-H"));
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn bundled_pdf_extractor_preserves_page_selection() {
        let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../");
        let fixture = workspace.join("docs/2512.24601v2.pdf");
        if !fixture.exists() {
            return;
        }

        let whole = read_pdf_via_pdf_extract(&fixture, None).expect("whole PDF");
        let first = read_pdf_via_pdf_extract(&fixture, Some((1, 1))).expect("first page");
        let first_two = read_pdf_via_pdf_extract(&fixture, Some((1, 2))).expect("first two pages");
        assert!(whole.content.contains("Recursive Language Models"));
        assert!(first.content.contains("Recursive Language Models"));
        assert!(first_two.content.len() >= first.content.len());

        let context = ProductionToolContext::new(workspace);
        let dispatched = outcome(
            &context,
            json!({"path": "docs/2512.24601v2.pdf", "pages": "1"}),
        )
        .expect("full dispatch");
        assert!(dispatched.content.contains("Recursive Language Models"));
    }

    #[test]
    fn external_pdf_preference_routes_to_pdftotext_without_fallback() {
        let workspace = tempdir().expect("workspace");
        fs::write(workspace.path().join("document.pdf"), b"%PDF-1.7\n%%EOF").expect("write");
        let context = ProductionToolContext::new(workspace.path());
        let result = execute_read_file(json!({"path": "document.pdf"}), &context, true);

        let pdftotext_present = Command::new("pdftotext")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok();
        if pdftotext_present {
            let message = result.expect_err("stub PDF must fail").to_string();
            assert!(message.contains("pdftotext"), "{message}");
        } else {
            let unavailable = result.expect("missing binary is a typed outcome");
            assert!(!unavailable.is_success());
            assert!(unavailable.content.contains("binary_unavailable"));
            assert!(unavailable.content.contains("prefer_external_pdftotext"));
        }
    }
}
