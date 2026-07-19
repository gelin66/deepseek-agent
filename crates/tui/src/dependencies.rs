//! External-binary dependency resolution for tools that shell out to
//! locally-installed programs (Python for project verification, `pdftotext`
//! for PDF reading in `read_file`, future tools as added).
//!
//! Before v0.8.31, tools that called external binaries hardcoded the
//! command name and failed at execution time when the binary wasn't on
//! `PATH`. The most-cited example was `code_execution`, which spawned
//! `python3` directly — Windows users (where the launcher is `py` or
//! `python`, not `python3`) saw `Failed to execute tool: program not
//! found` with no upstream hint of what was wrong.
//!
//! This module centralises the probe-then-decide pattern. The supported
//! callers today are:
//!
//! - Doctor command (`run_doctor` in `main.rs`): for surfacing the
//!   resolved state to the user so missing dependencies aren't an
//!   invisible failure.
//! - Retained TUI tools that invoke Git, Pandoc, PDF extraction, or other
//!   local executables.
//!
//! Results are cached for the process lifetime via [`std::sync::OnceLock`]
//! — probing a binary involves a `Command::output` per candidate and
//! we'd rather not pay that on every model turn.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use wait_timeout::ChildExt;

/// Candidate executable names for the Python interpreter, in the
/// order we try them. On Windows the launcher convention is `py -3`,
/// so we add it as a third option; the resolver splits on whitespace
/// at execution time so `py -3 /tmp/code.py` runs correctly.
///
/// Order matters: `python3` first because it's the unambiguous v3
/// binary on Unix and rules out Python 2 leftovers. `python` second
/// covers Windows installations that drop the version suffix and
/// modern macOS where Homebrew installs both. `py -3` last as a
/// Windows-launcher fallback.
pub const PYTHON_CANDIDATES: &[&str] = &["python3", "python", "py -3"];

const PYTHON_CAPABILITY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PYTHON_CAPABILITY_PROBE: &str = "import json, math";

/// Probe a single executable. Returns `true` when the candidate
/// responds to `--version` with a successful exit. Splits on
/// whitespace so `"py -3"` works as a candidate.
///
/// We deliberately use `--version` rather than `which` so the probe
/// is portable across Unix, Windows (no `which` by default), and
/// containers. The downside is that we spawn a subprocess per
/// candidate; the resolver caches the result so this only fires
/// once per process.
#[must_use]
pub fn probe_executable(spec: &str) -> bool {
    probe_executable_with_flag(spec, "--version")
}

/// Probe a single executable using an explicit version/help flag.
///
/// Most tools report their presence via `--version`, but some do not:
/// Poppler's `pdftotext` treats `--version` as an input *filename* and
/// exits non-zero ("I/O Error: Couldn't open file '--version'"), so the
/// default probe reports it missing even when it is installed (#1667).
/// Such tools pass their own flag (e.g. `-v`) here.
#[must_use]
pub fn probe_executable_with_flag(spec: &str, version_flag: &str) -> bool {
    let mut parts = spec.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    let mut cmd = Command::new(program);
    crate::utils::suppress_console_window(&mut cmd);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd.arg(version_flag);

    // Silence the subprocess's stdout/stderr — the version banner would
    // otherwise print to our terminal during startup, which is
    // confusing on the TUI's first frame.
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    matches!(cmd.status(), Ok(status) if status.success())
}

fn executable_path_candidates(program: &str) -> Vec<PathBuf> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return vec![program_path.to_path_buf()];
    }

    let Some(path) = std::env::var_os("PATH") else {
        return vec![PathBuf::from(program)];
    };

    let mut candidates = Vec::new();
    for dir in std::env::split_paths(&path) {
        let bare = dir.join(program);
        candidates.push(bare.clone());

        #[cfg(windows)]
        if Path::new(program).extension().is_none() {
            let pathext =
                std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            for ext in pathext.to_string_lossy().split(';') {
                if ext.is_empty() {
                    continue;
                }
                candidates.push(bare.with_extension(ext.trim_start_matches('.')));
            }
        }
    }

    candidates
}

fn resolve_executable_path(spec: &str, version_flag: &str) -> Option<String> {
    let mut parts = spec.split_whitespace();
    let program = parts.next()?;
    let args: Vec<&str> = parts.collect();

    for candidate in executable_path_candidates(program) {
        if !candidate.is_file() {
            continue;
        }

        let mut cmd = Command::new(&candidate);
        cmd.args(&args)
            .arg(version_flag)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        if matches!(cmd.status(), Ok(status) if status.success()) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    None
}

/// Resolve the Python interpreter once per process. Returns an absolute,
/// shell-quoted interpreter spec (including fixed launcher arguments such as
/// `-3`) or `None` when every candidate failed.
///
/// A version banner is not enough: project verification needs Python's
/// standard and native modules. Each actual executable on `PATH` therefore
/// runs a bounded import probe. A wedged or broken interpreter is killed and
/// reaped before the resolver continues to the next path, including another
/// executable with the same basename later on `PATH`.
pub fn resolve_python_interpreter() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let resolved = resolve_python_from_candidates(
                PYTHON_CANDIDATES,
                executable_path_candidates,
                python_has_required_capabilities,
            );
            if let Some((program, fixed_args)) = resolved {
                let spec = format_interpreter_spec(&program, &fixed_args);
                tracing::info!(
                    target: "tool_dependencies",
                    interpreter = %program.display(),
                    "Resolved capable Python interpreter",
                );
                return Some(spec);
            }
            tracing::warn!(
                target: "tool_dependencies",
                tried = ?PYTHON_CANDIDATES,
                "No Python interpreter found",
            );
            None
        })
        .clone()
}

fn resolve_python_from_candidates<F, P>(
    specs: &[&str],
    mut path_candidates: F,
    mut probe: P,
) -> Option<(PathBuf, Vec<String>)>
where
    F: FnMut(&str) -> Vec<PathBuf>,
    P: FnMut(&Path, &[String]) -> bool,
{
    let mut seen = HashSet::new();
    for spec in specs {
        let (program, fixed_args) = split_interpreter_spec(spec);
        if program.is_empty() {
            continue;
        }
        for candidate in path_candidates(&program) {
            if !candidate.is_file() {
                continue;
            }
            let candidate = std::fs::canonicalize(&candidate).unwrap_or(candidate);
            if !seen.insert((candidate.clone(), fixed_args.clone())) {
                continue;
            }
            if probe(&candidate, &fixed_args) {
                return Some((candidate, fixed_args));
            }
        }
    }
    None
}

fn python_has_required_capabilities(program: &Path, fixed_args: &[String]) -> bool {
    let mut cmd = Command::new(program);
    crate::utils::suppress_console_window(&mut cmd);
    cmd.args(fixed_args)
        .args(["-I", "-c", PYTHON_CAPABILITY_PROBE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    codewhale_tools::child_env::apply_to_command(
        &mut cmd,
        std::iter::empty::<(std::ffi::OsString, std::ffi::OsString)>(),
    );

    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    match child.wait_timeout(PYTHON_CAPABILITY_PROBE_TIMEOUT) {
        Ok(Some(status)) => status.success(),
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            tracing::warn!(
                target: "tool_dependencies",
                interpreter = %program.display(),
                timeout_secs = PYTHON_CAPABILITY_PROBE_TIMEOUT.as_secs(),
                "Python capability probe timed out",
            );
            false
        }
    }
}

fn format_interpreter_spec(program: &Path, fixed_args: &[String]) -> String {
    std::iter::once(program.to_string_lossy().into_owned())
        .chain(fixed_args.iter().cloned())
        .map(|part| shell_words::quote(&part).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve `pdftotext` (from Poppler) once per process. Used by
/// `read_file`'s PDF path for graceful fallback messaging. Unlike
/// the Python case, `read_file` itself still works for text files
/// when `pdftotext` is missing — this resolver exists so the doctor
/// command can surface the miss explicitly rather than the user
/// hitting "PDF unsupported" on a read attempt.
pub fn resolve_pdftotext() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            // Poppler's `pdftotext` rejects `--version` (it is parsed as an
            // input filename and exits non-zero), so probe with `-v`, which
            // prints the version banner and exits 0 (#1667).
            if probe_executable_with_flag("pdftotext", "-v") {
                Some("pdftotext".to_string())
            } else {
                None
            }
        })
        .clone()
}

/// Resolve `pandoc` (universal document converter) once per
/// process. Used by the `pandoc_convert` tool to decide whether
/// to register itself with the model. Pandoc is a single-binary
/// install, so the candidate list is just `pandoc` — no platform
/// fallback path.
pub fn resolve_pandoc() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if let Some(path) = resolve_executable_path("pandoc", "--version") {
                tracing::info!(
                    target: "tool_dependencies",
                    "Resolved pandoc binary for pandoc_convert",
                );
                Some(path)
            } else {
                tracing::warn!(
                    target: "tool_dependencies",
                    "pandoc binary not found; pandoc_convert tool will not be registered",
                );
                None
            }
        })
        .clone()
}

// ---------------------------------------------------------------------------
// ExternalTool trait — unified subprocess interface
// ---------------------------------------------------------------------------

/// A tool that DeepSeek-TUI shells out to. Instead of scattering
/// `Command::new("git")` / `Command::new("gh")` across the codebase,
/// each external dependency implements this trait once in this module.
/// Callers ask the tool for a pre-populated [`Command`] and chain their
/// own args, working directory, and spawn method.
///
/// # Example
///
/// ```ignore
/// let output = Git::command()
///     .expect("git not found")
///     .args(["diff", "--stat"])
///     .current_dir(&workspace)
///     .output()?;
/// ```
pub trait ExternalTool {
    /// Candidate binary names, tried in order until one responds to
    /// `--version`.  For single-binary tools (git, gh, node) this is a
    /// one-element slice.
    fn candidates() -> &'static [&'static str];

    /// Resolve the best candidate once per process (cached). Returns
    /// the spec string (e.g. `"python3"` or `"py -3"`).
    fn resolve() -> Option<String>;

    /// Build a `std::process::Command` pre-populated with the resolved
    /// binary (and any fixed arguments from a multi-word candidate like
    /// `"py -3"`). Returns `None` when the tool isn't installed.
    ///
    /// Callers should chain `.args(...)`, `.current_dir(...)`, and then
    /// call `.output()`, `.status()`, or `.spawn()`.
    fn command() -> Option<Command> {
        let spec = Self::resolve()?;
        let (program, fixed_args) = split_interpreter_spec(&spec);
        let mut cmd = Command::new(&program);
        crate::utils::suppress_console_window(&mut cmd);
        for arg in &fixed_args {
            cmd.arg(arg);
        }
        Some(cmd)
    }
}

// ---------------------------------------------------------------------------
// Concrete tool implementations
// ---------------------------------------------------------------------------

/// Git version control.
pub struct Git;

impl ExternalTool for Git {
    fn candidates() -> &'static [&'static str] {
        &["git"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved git binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// GitHub CLI.
pub struct Gh;

impl ExternalTool for Gh {
    fn candidates() -> &'static [&'static str] {
        &["gh"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved gh binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// Rust compiler — used for version reporting in diagnostics.
pub struct RustC;

impl ExternalTool for RustC {
    fn candidates() -> &'static [&'static str] {
        &["rustc"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved rustc binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Interpreter command encoding
// ---------------------------------------------------------------------------

/// Split a shell-quoted interpreter spec like `"py -3"` into the program name
/// and any initial arguments. Returns `("py", vec!["-3"])` for the
/// example; returns `("python3", vec![])` for a bare name.
///
/// Callers spawn `Command::new(program).args(args).arg(script_path)`.
#[must_use]
pub fn split_interpreter_spec(spec: &str) -> (String, Vec<String>) {
    let mut parts = shell_words::split(spec).unwrap_or_default().into_iter();
    let program = parts.next().unwrap_or_default();
    let args = parts.collect();
    (program, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_executable_returns_false_for_unknown_binary() {
        // Pick a name we're confident isn't on any developer's PATH.
        // If this ever starts failing locally, rename it.
        assert!(!probe_executable("codewhale-tui-imaginary-binary-xyz123"));
    }

    #[test]
    fn probe_executable_handles_multi_word_specs() {
        // `py -3` should split correctly. The probe will fail on
        // most non-Windows machines (no `py` launcher), which is
        // fine — we're checking that the *split* doesn't crash.
        let _ = probe_executable("py -3");
    }

    #[test]
    fn probe_executable_with_flag_returns_false_for_unknown_binary() {
        assert!(!probe_executable_with_flag(
            "codewhale-tui-imaginary-binary-xyz123",
            "-v"
        ));
    }

    #[test]
    fn probe_executable_delegates_to_double_dash_version() {
        // `probe_executable` must remain exactly
        // `probe_executable_with_flag(.., "--version")`.
        let spec = "codewhale-tui-imaginary-binary-xyz123";
        assert_eq!(
            probe_executable(spec),
            probe_executable_with_flag(spec, "--version")
        );
    }

    #[test]
    fn pdftotext_resolver_detects_installed_poppler_via_dash_v() {
        // Regression for #1667: Poppler's `pdftotext` rejects `--version`
        // (it is parsed as an input filename and exits non-zero), so the
        // generic `--version` probe reports it missing even when installed.
        // The resolver must probe with `-v`. Gated on pdftotext actually
        // being installed so CI without Poppler stays green.
        if probe_executable_with_flag("pdftotext", "-v") {
            assert!(
                resolve_pdftotext().is_some(),
                "an installed pdftotext must be detected via -v (#1667)"
            );
        }
    }

    #[test]
    fn split_interpreter_spec_strips_args() {
        assert_eq!(
            split_interpreter_spec("python3"),
            ("python3".to_string(), Vec::<String>::new())
        );
        assert_eq!(
            split_interpreter_spec("py -3"),
            ("py".to_string(), vec!["-3".to_string()])
        );
        assert_eq!(
            split_interpreter_spec("  python3  "),
            ("python3".to_string(), Vec::<String>::new()),
            "leading/trailing whitespace must be tolerated"
        );
    }

    #[test]
    fn split_interpreter_spec_handles_empty_string() {
        assert_eq!(
            split_interpreter_spec(""),
            (String::new(), Vec::<String>::new())
        );
    }

    #[test]
    fn python_capability_resolution_skips_broken_path_and_keeps_searching() {
        let temp = tempfile::tempdir().expect("tempdir");
        let broken = temp.path().join("broken-python");
        let healthy = temp.path().join("healthy-python");
        std::fs::write(&broken, []).expect("write broken candidate");
        std::fs::write(&healthy, []).expect("write healthy candidate");
        let healthy = std::fs::canonicalize(healthy).expect("canonical healthy candidate");

        let mut probed = Vec::new();
        let resolved = resolve_python_from_candidates(
            &["python3"],
            |_| vec![broken.clone(), healthy.clone()],
            |program, args| {
                probed.push((program.to_path_buf(), args.to_vec()));
                program == healthy
            },
        );

        assert_eq!(resolved, Some((healthy.clone(), Vec::new())));
        assert_eq!(
            probed.len(),
            2,
            "broken interpreter must not stop PATH search"
        );
        assert_eq!(probed[1].0, healthy);
    }

    #[test]
    fn python_capability_resolution_preserves_launcher_args_and_spaced_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp.path().join("Program Files");
        std::fs::create_dir(&bin_dir).expect("create spaced directory");
        let launcher = bin_dir.join("py.exe");
        std::fs::write(&launcher, []).expect("write launcher candidate");
        let launcher = std::fs::canonicalize(launcher).expect("canonical launcher");

        let resolved = resolve_python_from_candidates(
            &["py -3"],
            |_| vec![launcher.clone()],
            |program, args| program == launcher && args == ["-3"],
        )
        .expect("launcher should resolve");
        let spec = format_interpreter_spec(&resolved.0, &resolved.1);

        assert_eq!(
            split_interpreter_spec(&spec),
            (
                launcher.to_string_lossy().into_owned(),
                vec!["-3".to_string()]
            )
        );
    }

    #[test]
    fn python_resolver_is_cached_across_calls() {
        // Whatever the first call returns, subsequent calls return
        // the same value (cached). If this test ever flakes, the
        // OnceLock semantics changed and we need to rethink the
        // resolver.
        let first = resolve_python_interpreter();
        let second = resolve_python_interpreter();
        assert_eq!(first, second);
    }

    #[test]
    fn python_resolver_returns_some_on_developer_machines() {
        // CI hosts have Python; developer machines have Python.
        // The one environment where this returns None is bare-bones
        // Windows / minimal CI containers — fine, those just don't
        // get code_execution registered, which is the whole point.
        // We don't assert Some() because we don't want this test
        // to fail in those environments. Instead we just confirm
        // the resolver doesn't panic and returns a stable value.
        let resolved = resolve_python_interpreter();
        if let Some(name) = resolved {
            assert!(
                !name.is_empty(),
                "resolved interpreter name must be non-empty"
            );
            let (program, _) = split_interpreter_spec(&name);
            assert!(
                Path::new(&program).is_absolute(),
                "resolved interpreter must be an absolute path: {name:?}"
            );
        }
    }

    // ===================================================================
    // ExternalTool trait tests
    // ===================================================================

    #[test]
    fn git_candidates_is_git_only() {
        assert_eq!(Git::candidates(), &["git"]);
    }

    #[test]
    fn gh_candidates_is_gh_only() {
        assert_eq!(Gh::candidates(), &["gh"]);
    }

    #[test]
    fn rustc_candidates_is_rustc_only() {
        assert_eq!(RustC::candidates(), &["rustc"]);
    }

    #[test]
    fn git_resolve_is_cached() {
        let first = Git::resolve();
        let second = Git::resolve();
        assert_eq!(first, second);
    }

    #[test]
    fn gh_resolve_is_cached() {
        let first = Gh::resolve();
        let second = Gh::resolve();
        assert_eq!(first, second);
    }

    #[test]
    fn rustc_resolve_is_cached() {
        let first = RustC::resolve();
        let second = RustC::resolve();
        assert_eq!(first, second);
    }
}
