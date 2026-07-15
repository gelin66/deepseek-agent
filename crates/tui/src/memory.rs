//! User-level memory file (deprecated — see Moraine).
//!
//! ## Deprecation
//!
//! DEPRECATED(v0.8.66–v0.8.71): Superseded by Moraine MCP recall.
//! The legacy push/inject path is gated behind `MemoryConfig.moraine_fallback`.
//! When Moraine lands (v0.8.66/67), this module can be deleted entirely.
//!
//! Migration guide: use Moraine MCP tools (`search_sessions`, `open`,
//! `list_sessions`, `file_attention`) instead of `<user_memory>` injection.
//!
//! Ref: https://github.com/Hmbown/CodeWhale/issues/3495 (Moraine adoption)
//! Ref: https://github.com/Hmbown/CodeWhale/issues/3490 (v0.8.71 dead-code inventory)
//!
//! ### Migration
//!
//! 1. Install Moraine: `uv tool install moraine-cli && moraine setup && moraine up`
//! 2. Enable `moraine-mcp` in `~/.codewhale/mcp.json` (set `disabled` to `false`)
//! 3. Set `[memory] moraine_fallback = true` in `config.toml` to skip the legacy
//!    `<user_memory>` block, `remember` tool, and `# foo` quick-add.
//!
//! ## Legacy docs (pre-Moraine)
//!
//! v0.8.8 shipped an MVP that let the user keep a persistent personal
//! note file the model sees on every turn:
//!
//! - **Load** `~/.codewhale/memory.md` (path is configurable via
//!   `memory_path` in `config.toml` and `DEEPSEEK_MEMORY_PATH` env),
//!   wrap it in a `<user_memory>` block, and prepend it to the system
//!   prompt alongside the existing `<project_instructions>` block.
//! - **`# foo`** typed in the composer appends `foo` to the memory
//!   file as a timestamped bullet — fast capture without leaving the TUI.
//! - **`/memory`** shows the resolved file path and current contents, and
//!   **`/memory edit`** prints a copy-pasteable `$VISUAL` / `$EDITOR`
//!   command for opening the file yourself.
//! - **`remember` tool** lets the model itself append a bullet when it
//!   notices a durable preference or convention worth keeping across
//!   sessions.
//!
//! Default behavior is **opt-in**: load + use the memory file only when
//! `[memory] enabled = true` in `config.toml` or `DEEPSEEK_MEMORY=on`.
//! That keeps existing users on zero-overhead behavior and makes the
//! feature explicit.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use chrono::Utc;

/// Maximum size of the user memory file. Larger files are loaded with both
/// their head and tail retained; the `<user_memory>` block carries a
/// `<truncated bytes=N source="...">` marker between them so the model sees
/// stable older facts plus the newest appended memories. Mirrors
/// `project_context::MAX_CONTEXT_SIZE`.
const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// Read the user memory file at `path`, returning `None` when the file
/// doesn't exist or is empty after trimming.
#[must_use]
pub fn load(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}

/// Wrap memory content in a `<user_memory>` block ready to prepend to the
/// system prompt. The `source` value is rendered verbatim into a
/// `source="…"` attribute — pass the path so the model can see where the
/// memory came from. Returns `None` for empty content.
#[must_use]
pub fn as_system_block(content: &str, source: &Path) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let display = source.display().to_string();
    let payload = if content.len() > MAX_MEMORY_SIZE {
        truncate_head_tail(content, &display)
    } else {
        trimmed.to_string()
    };

    Some(format!(
        "<user_memory source=\"{display}\">\n{payload}\n</user_memory>"
    ))
}

fn truncate_head_tail(content: &str, source: &str) -> String {
    let mut omitted_bytes = content.len().saturating_sub(MAX_MEMORY_SIZE);
    loop {
        let marker = truncation_marker(omitted_bytes, source);
        let retained_budget = MAX_MEMORY_SIZE.saturating_sub(marker.len());
        let head_budget = retained_budget / 2;
        let tail_budget = retained_budget.saturating_sub(head_budget);
        let head_end = previous_char_boundary(content, head_budget.min(content.len()));
        let tail_start = next_char_boundary(
            content,
            content.len().saturating_sub(tail_budget).max(head_end),
        );
        let actual_omitted_bytes = tail_start.saturating_sub(head_end);

        if actual_omitted_bytes == omitted_bytes {
            let mut payload = String::with_capacity(MAX_MEMORY_SIZE);
            payload.push_str(&content[..head_end]);
            payload.push_str(&marker);
            payload.push_str(&content[tail_start..]);
            return payload;
        }
        omitted_bytes = actual_omitted_bytes;
    }
}

fn truncation_marker(omitted_bytes: usize, source: &str) -> String {
    format!("\n<truncated bytes={omitted_bytes} source=\"{source}\">\n")
}

fn previous_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// Compose the `<user_memory>` block for the system prompt, honouring the
/// opt-in toggle. Returns `None` when the feature is disabled, when
/// `moraine_fallback` is active, or when the file is missing / empty so
/// the caller doesn't have to check both conditions.
///
/// Callers that hold a `&Config` should pass `config.memory_enabled() &&
/// !config.moraine_fallback()` and `config.memory_path()` directly.
/// The split keeps this module `Config`-free so it can be reused from
/// sub-agent / engine boundaries where the high-level `Config` isn't
/// available.
#[must_use]
pub fn compose_block(enabled: bool, path: &Path) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load(path)?;
    as_system_block(&content, path)
}

/// Append `entry` to the memory file at `path`, creating it (and its
/// parent directory) if needed. The entry is timestamped so the user can
/// later see when each note was added. The leading `#` from a `# foo`
/// quick-add is stripped so the file stays as readable Markdown.
pub fn append_entry(path: &Path, entry: &str) -> io::Result<()> {
    let trimmed = entry.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory entry is empty after stripping `#` prefix",
        ));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "- ({timestamp}) {trimmed}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("never-existed.md");
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_none_for_whitespace_only_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "   \n   \n").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "remember the milk").unwrap();
        assert_eq!(load(&path).as_deref(), Some("remember the milk"));
    }

    #[test]
    fn as_system_block_produces_xml_wrapper() {
        let block = as_system_block("note 1", Path::new("/tmp/m.md")).unwrap();
        assert!(block.contains("<user_memory source=\"/tmp/m.md\">"));
        assert!(block.contains("note 1"));
        assert!(block.ends_with("</user_memory>"));
    }

    #[test]
    fn as_system_block_returns_none_for_empty_content() {
        assert!(as_system_block("   ", Path::new("/tmp/m.md")).is_none());
    }

    #[test]
    fn as_system_block_truncates_oversize_input() {
        let big = format!(
            "oldest-memory:{}:newest-memory",
            "x".repeat(MAX_MEMORY_SIZE)
        );
        let block = as_system_block(&big, Path::new("/tmp/m.md")).unwrap();
        let payload = user_memory_payload(&block);
        assert_eq!(payload.len(), MAX_MEMORY_SIZE);
        let (head, omitted_bytes, tail) = truncated_payload_parts(payload);
        assert!(head.starts_with("oldest-memory:"));
        assert!(tail.ends_with(":newest-memory"));
        assert_eq!(head.len() + omitted_bytes + tail.len(), big.len());
    }

    #[test]
    fn as_system_block_truncates_non_ascii_at_char_boundary() {
        let content = format!("oldest-é-{}-newest-终", "数据".repeat(MAX_MEMORY_SIZE / 3));

        let block = as_system_block(&content, Path::new("/tmp/m.md")).unwrap();
        let payload = user_memory_payload(&block);
        let (head, omitted_bytes, tail) = truncated_payload_parts(payload);
        assert!(payload.len() <= MAX_MEMORY_SIZE);
        assert!(head.starts_with("oldest-é-"));
        assert!(tail.ends_with("-newest-终"));
        assert_eq!(head.len() + omitted_bytes + tail.len(), content.len());
    }

    #[test]
    fn as_system_block_truncates_emoji_at_char_boundary() {
        let content = format!("oldest-😀-{}-newest-🚀", "🐋".repeat(MAX_MEMORY_SIZE / 2));

        let block = as_system_block(&content, Path::new("/tmp/m.md")).unwrap();
        let payload = user_memory_payload(&block);
        let (head, omitted_bytes, tail) = truncated_payload_parts(payload);
        assert!(payload.len() <= MAX_MEMORY_SIZE);
        assert!(head.starts_with("oldest-😀-"));
        assert!(tail.ends_with("-newest-🚀"));
        assert_eq!(head.len() + omitted_bytes + tail.len(), content.len());
    }

    fn user_memory_payload(block: &str) -> &str {
        block
            .strip_prefix("<user_memory source=\"/tmp/m.md\">\n")
            .unwrap()
            .strip_suffix("\n</user_memory>")
            .unwrap()
    }

    fn truncated_payload_parts(payload: &str) -> (&str, usize, &str) {
        let marker_prefix = "\n<truncated bytes=";
        let marker_start = payload.find(marker_prefix).expect("truncation marker");
        let count_start = marker_start + marker_prefix.len();
        let count_end = payload[count_start..]
            .find(' ')
            .map(|offset| count_start + offset)
            .expect("truncated byte count");
        let omitted_bytes = payload[count_start..count_end]
            .parse()
            .expect("numeric truncated byte count");
        let tail_start = payload[count_end..]
            .find(">\n")
            .map(|offset| count_end + offset + 2)
            .expect("truncation marker terminator");
        (
            &payload[..marker_start],
            omitted_bytes,
            &payload[tail_start..],
        )
    }

    #[test]
    fn append_entry_creates_file_and_writes_one_bullet() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# remember the milk").unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("remember the milk"), "{body}");
        assert!(
            body.starts_with("- ("),
            "should start with bullet + date: {body}"
        );
        assert!(body.trim_end().ends_with("remember the milk"));
    }

    #[test]
    fn append_entry_appends_subsequent_lines() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# first").unwrap();
        append_entry(&path, "second").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("first"));
        assert!(body.contains("second"));
        // Two bullets means two lines of `- (date) entry`.
        assert_eq!(body.matches("- (").count(), 2);
    }

    #[test]
    fn latest_appended_entry_remains_visible_after_memory_is_truncated() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(
            &path,
            format!(
                "oldest durable preference\n{}",
                "x".repeat(MAX_MEMORY_SIZE + 128)
            ),
        )
        .unwrap();
        append_entry(&path, "newest durable preference").unwrap();

        let block = compose_block(true, &path).expect("memory block");

        assert!(block.contains("oldest durable preference"));
        assert!(block.contains("<truncated bytes="));
        assert!(block.contains("newest durable preference"));
    }

    #[test]
    fn append_entry_rejects_empty_after_strip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        let err = append_entry(&path, "###").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
