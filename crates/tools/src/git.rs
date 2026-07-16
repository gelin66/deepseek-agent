//! Production Git inspection operations: `git_status` and `git_diff`.
//!
//! These tools are read-only wrappers around common git inspection commands,
//! scoped to the workspace and optionally to a sub-path within it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::{
    ProductionToolContext, ToolError, ToolOutcome, optional_bool, optional_str, optional_u64,
};

const MAX_OUTPUT_CHARS: usize = 40_000;
const DEFAULT_UNIFIED: u64 = 3;
const MAX_UNIFIED: u64 = 50;

/// Execute the production `git_status` operation against a workspace context.
pub fn execute_git_status(
    input: Value,
    context: &ProductionToolContext,
) -> Result<ToolOutcome, ToolError> {
    let git_ctx = resolve_git_context(context, optional_str(&input, "path"))?;

    let mut args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "-b".to_string(),
    ];
    if let Some(pathspec) = &git_ctx.pathspec {
        args.push("--".to_string());
        args.push(pathspec.display().to_string());
    }

    let command_str = format_command(&git_ctx.working_dir, &args);
    let output = run_git_command(&git_ctx.working_dir, &args)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = format!("git status failed: {}", stderr.trim());
        return Ok(ToolOutcome::error(message).with_metadata(json!({
            "command": command_str,
            "exit_code": output.status.code(),
            "stderr": stderr.trim(),
        })));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (content, truncated, omitted_chars) = truncate_with_note(&stdout, MAX_OUTPUT_CHARS);

    Ok(ToolOutcome::success(content).with_metadata(json!({
        "command": command_str,
        "working_dir": git_ctx.working_dir,
        "pathspec": git_ctx.pathspec,
        "truncated": truncated,
        "omitted_chars": omitted_chars,
    })))
}

/// Execute the production `git_diff` operation against a workspace context.
pub fn execute_git_diff(
    input: Value,
    context: &ProductionToolContext,
) -> Result<ToolOutcome, ToolError> {
    let git_ctx = resolve_git_context(context, optional_str(&input, "path"))?;
    let cached = optional_bool(&input, "cached", false);
    let unified = optional_u64(&input, "unified", DEFAULT_UNIFIED).min(MAX_UNIFIED);

    let mut args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        format!("--unified={unified}"),
    ];
    if cached {
        args.push("--cached".to_string());
    }
    if let Some(pathspec) = &git_ctx.pathspec {
        args.push("--".to_string());
        args.push(pathspec.display().to_string());
    }

    let command_str = format_command(&git_ctx.working_dir, &args);
    let output = run_git_command(&git_ctx.working_dir, &args)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = format!("git diff failed: {}", stderr.trim());
        return Ok(ToolOutcome::error(message).with_metadata(json!({
            "command": command_str,
            "exit_code": output.status.code(),
            "stderr": stderr.trim(),
        })));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (content, truncated, omitted_chars) = truncate_with_note(&stdout, MAX_OUTPUT_CHARS);

    Ok(ToolOutcome::success(content).with_metadata(json!({
        "command": command_str,
        "working_dir": git_ctx.working_dir,
        "pathspec": git_ctx.pathspec,
        "cached": cached,
        "unified": unified,
        "truncated": truncated,
        "omitted_chars": omitted_chars,
    })))
}

// === Helpers ===

struct GitContext {
    working_dir: PathBuf,
    pathspec: Option<PathBuf>,
}

fn resolve_git_context(
    context: &ProductionToolContext,
    path: Option<&str>,
) -> Result<GitContext, ToolError> {
    let workspace = canonical_or_workspace(context.workspace());
    let mut working_dir = workspace.clone();
    let mut pathspec = None;

    if let Some(raw) = path {
        let resolved = context.resolve_path(raw)?;
        let metadata = fs::metadata(&resolved).map_err(|e| {
            ToolError::invalid_input(format!(
                "Path does not exist or is not accessible: {raw} ({e})"
            ))
        })?;

        if metadata.is_dir() {
            working_dir = resolved;
            pathspec = Some(PathBuf::from("."));
        } else {
            // For file paths, run from the parent and scope to the file name.
            let parent = resolved.parent().ok_or_else(|| {
                ToolError::invalid_input(format!("Path has no parent directory: {raw}"))
            })?;
            working_dir = parent.to_path_buf();
            pathspec = Some(pathspec_from(&working_dir, &resolved));
        }
    }

    if !working_dir.exists() {
        return Err(ToolError::invalid_input(format!(
            "Working directory does not exist: {}",
            working_dir.display()
        )));
    }

    Ok(GitContext {
        working_dir,
        pathspec,
    })
}

fn canonical_or_workspace(workspace: &Path) -> PathBuf {
    workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf())
}

fn pathspec_from(working_dir: &Path, resolved: &Path) -> PathBuf {
    match resolved.strip_prefix(working_dir) {
        Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
        Ok(rel) => rel.to_path_buf(),
        Err(_) => PathBuf::from("."),
    }
}

fn run_git_command(working_dir: &Path, args: &[String]) -> Result<std::process::Output, ToolError> {
    let Some(mut cmd) = git_command() else {
        return Err(ToolError::not_available(
            "git is not installed or not in PATH",
        ));
    };
    cmd.args(args).current_dir(working_dir);
    cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::not_available("git is not installed or not in PATH")
        } else {
            ToolError::execution_failed(format!("Failed to run git: {e}"))
        }
    })
}

fn git_command() -> Option<Command> {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    if !AVAILABLE.get_or_init(probe_git) {
        return None;
    }
    let mut command = Command::new("git");
    suppress_console_window(&mut command);
    Some(command)
}

fn probe_git() -> bool {
    let mut command = Command::new("git");
    suppress_console_window(&mut command);
    command
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    matches!(command.status(), Ok(status) if status.success())
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

fn format_command(working_dir: &Path, args: &[String]) -> String {
    // `[String]::join` produces the same string as collecting `&str` first, so
    // join the slice directly and skip the intermediate `Vec<&str>` allocation.
    format!("git -C {} {}", working_dir.display(), args.join(" "))
}

fn truncate_with_note(text: &str, max_chars: usize) -> (String, bool, usize) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false, 0);
    }
    let end = char_boundary_index(text, max_chars);
    let truncated = &text[..end];
    let omitted_chars = text
        .chars()
        .count()
        .saturating_sub(truncated.chars().count());
    let note = format!(
        "\n\n[output truncated to {max_chars} characters; {omitted_chars} characters omitted]"
    );
    (format!("{truncated}{note}"), true, omitted_chars)
}

fn char_boundary_index(text: &str, max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return idx;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn git_available() -> bool {
        git_command().is_some()
    }

    fn run_git(args: &[&str], root: &Path) -> std::io::Result<std::process::ExitStatus> {
        let mut command = git_command().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "git not found on PATH")
        })?;
        command.args(args).current_dir(root).status()
    }

    fn init_git_repo(root: &Path) {
        let run = |args: &[&str]| {
            let status = run_git(args, root).expect("git should spawn");
            assert!(status.success(), "git {args:?} failed");
        };

        run(&["init", "-q"]);
        run(&["config", "core.autocrlf", "false"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test User"]);
    }

    fn commit_all(root: &Path, message: &str) {
        let run = |args: &[&str]| {
            let status = run_git(args, root).expect("git should spawn");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["add", "."]);
        run(&["commit", "-q", "-m", message]);
    }

    #[tokio::test]
    async fn git_status_reports_branch_and_changes() {
        if !git_available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        init_git_repo(tmp.path());

        let file = tmp.path().join("file.txt");
        fs::write(&file, "hello\n").expect("write");
        commit_all(tmp.path(), "init");

        fs::write(&file, "hello\nworld\n").expect("modify");

        let context = ProductionToolContext::new(tmp.path());
        let result = execute_git_status(json!({}), &context).expect("execute");
        assert!(result.is_success());
        assert!(result.content.contains("##"));
        assert!(result.content.contains("file.txt"));
    }

    #[tokio::test]
    async fn git_status_reports_unquoted_unicode_paths() {
        if !git_available() {
            return;
        }

        let tmp = tempdir().expect("tempdir");
        init_git_repo(tmp.path());

        let file = tmp.path().join("中文-данные.txt");
        fs::write(&file, "hello\n").expect("write");
        commit_all(tmp.path(), "init");

        fs::write(&file, "hello\nworld\n").expect("modify");

        let context = ProductionToolContext::new(tmp.path());
        let result = execute_git_status(json!({}), &context).expect("execute");
        assert!(result.is_success());
        assert!(
            result
                .metadata
                .as_ref()
                .and_then(|m| m.get("command"))
                .and_then(Value::as_str)
                .is_some_and(|command| command.contains("-c core.quotepath=false"))
        );
        assert!(result.content.contains("中文-данные.txt"));
        assert!(!result.content.contains("\\344"));
        assert!(!result.content.contains("\\320"));
    }

    #[tokio::test]
    async fn git_diff_supports_cached_and_path_scoping() {
        if !git_available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        init_git_repo(tmp.path());

        let subdir = tmp.path().join("src");
        fs::create_dir_all(&subdir).expect("mkdir");
        let file = subdir.join("lib.rs");
        fs::write(&file, "pub fn one() -> i32 { 1 }\n").expect("write");
        commit_all(tmp.path(), "init");

        fs::write(&file, "pub fn one() -> i32 { 2 }\n").expect("modify");

        let context = ProductionToolContext::new(tmp.path());

        let uncached = execute_git_diff(json!({ "path": "src" }), &context).expect("diff");
        assert!(uncached.is_success());
        assert!(uncached.content.contains("diff --git"));
        assert!(uncached.content.contains("lib.rs"));

        let _ = run_git(&["add", "src/lib.rs"], tmp.path()).expect("git add");

        let cached = execute_git_diff(json!({ "path": "src", "cached": true }), &context)
            .expect("diff cached");
        assert!(cached.is_success());
        assert!(cached.content.contains("diff --git"));
        assert!(
            cached
                .metadata
                .as_ref()
                .and_then(|m| m.get("cached"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn git_diff_reports_unquoted_unicode_paths() {
        if !git_available() {
            return;
        }

        let tmp = tempdir().expect("tempdir");
        init_git_repo(tmp.path());

        let unicode_name = "\u{4e2d}\u{6587}-\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435}.txt";
        let file = tmp.path().join(unicode_name);
        fs::write(&file, "hello\n").expect("write");
        commit_all(tmp.path(), "init");

        fs::write(&file, "hello\nworld\n").expect("modify");

        let context = ProductionToolContext::new(tmp.path());
        let result = execute_git_diff(json!({}), &context).expect("execute");

        assert!(result.is_success());
        assert!(
            result
                .metadata
                .as_ref()
                .and_then(|m| m.get("command"))
                .and_then(Value::as_str)
                .is_some_and(|command| command.contains("-c core.quotepath=false"))
        );
        assert!(result.content.contains(unicode_name));
        assert!(!result.content.contains("\\344"));
        assert!(!result.content.contains("\\320"));
    }

    #[test]
    fn format_command_joins_args_without_intermediate_vec() {
        // Locks the output shape after dropping the collect-before-join
        // allocation: joining the `&[String]` slice directly must be byte-for-byte
        // identical to the previous `.map(String::as_str).collect().join(" ")`.
        let args = vec![
            "-c".to_string(),
            "core.quotepath=false".to_string(),
            "status".to_string(),
            "--porcelain=v1".to_string(),
            "-b".to_string(),
        ];
        let rendered = format_command(Path::new("/tmp/repo"), &args);
        assert_eq!(
            rendered,
            "git -C /tmp/repo -c core.quotepath=false status --porcelain=v1 -b"
        );

        // Empty args still render cleanly (trailing space, matching prior behavior).
        assert_eq!(
            format_command(Path::new("/tmp/repo"), &[]),
            "git -C /tmp/repo "
        );
    }

    #[test]
    fn truncation_adds_note() {
        let long = "a".repeat(MAX_OUTPUT_CHARS + 100);
        let (truncated, did_truncate, omitted) = truncate_with_note(&long, MAX_OUTPUT_CHARS);
        assert!(did_truncate);
        assert!(omitted > 0);
        assert!(truncated.contains("output truncated"));
    }
}
