//! Production `run_verifiers` operation.
//!
//! The foreground verifier ensemble is shared by every product surface. It
//! detects Rust, Node, Python and Go projects, runs independent gates in
//! parallel and returns one deterministic verdict. Background jobs and Goal
//! completion receipts remain outside this module.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use codewhale_protocol::agent_runtime::{
    ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::shell::{ExecShellOptions, ShellStatus, execute_managed_program};
use crate::verification_artifact::{
    attach_verification_artifact, capture_workspace_revision, reject_verification_artifact,
};
use crate::{ProductionToolContext, ToolError, ToolOutcome};

const MAX_GATE_OUTPUT_CHARS: usize = 16_000;
const DEFAULT_MAX_PYTHON_FILES: usize = 200;
const MAX_CUSTOM_GATES: usize = 12;
const VERIFIER_GATE_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum VerifierProfile {
    Auto,
    Rust,
    Node,
    Python,
    Go,
}

impl VerifierProfile {
    fn parse(raw: &str) -> Result<Self, ToolError> {
        match raw {
            "auto" => Ok(Self::Auto),
            "rust" => Ok(Self::Rust),
            "node" => Ok(Self::Node),
            "python" => Ok(Self::Python),
            "go" => Ok(Self::Go),
            other => Err(ToolError::invalid_input(format!(
                "Unsupported profile '{other}'. Expected one of: auto, rust, node, python, go"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Rust => "rust",
            Self::Node => "node",
            Self::Python => "python",
            Self::Go => "go",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum VerifierLevel {
    Quick,
    Full,
}

impl VerifierLevel {
    fn parse(raw: &str) -> Result<Self, ToolError> {
        match raw {
            "quick" => Ok(Self::Quick),
            "full" => Ok(Self::Full),
            other => Err(ToolError::invalid_input(format!(
                "Unsupported level '{other}'. Expected one of: quick, full"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct RunVerifiersInput {
    profile: String,
    level: String,
    max_python_files: usize,
    commands: Vec<CustomVerifierInput>,
}

impl Default for RunVerifiersInput {
    fn default() -> Self {
        Self {
            profile: "auto".to_string(),
            level: "quick".to_string(),
            max_python_files: DEFAULT_MAX_PYTHON_FILES,
            commands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct CustomVerifierInput {
    name: String,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
}

#[derive(Debug, Clone)]
struct VerifierGate {
    name: String,
    ecosystem: String,
    cwd: PathBuf,
    program: Option<String>,
    args: Vec<String>,
    env: Vec<(String, String)>,
    skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub name: String,
    pub ecosystem: String,
    pub status: GateStatus,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierVerdict {
    Pass,
    Partial,
    Fail,
}

impl VerifierVerdict {
    fn from_counts(gate_count: usize, failed: usize, skipped: usize) -> Self {
        if failed > 0 {
            Self::Fail
        } else if skipped > 0 || gate_count == 0 {
            Self::Partial
        } else {
            Self::Pass
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunVerifiersOutput {
    pub success: bool,
    pub profile: String,
    pub level: String,
    pub workspace: String,
    pub gate_count: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub verifier_verdict: VerifierVerdict,
    pub summary: String,
    pub gates: Vec<GateResult>,
}

/// Run the complete foreground verifier ensemble. Unknown fields (including
/// the legacy `background` flag) are rejected before any process starts.
pub async fn execute_run_verifiers(
    input: Value,
    context: &ProductionToolContext,
    shell: &ExecShellOptions,
) -> Result<ToolOutcome, ToolError> {
    let input: RunVerifiersInput = serde_json::from_value(input)
        .map_err(|error| ToolError::invalid_input(error.to_string()))?;
    let profile = VerifierProfile::parse(&input.profile)?;
    let level = VerifierLevel::parse(&input.level)?;
    if input.max_python_files == 0 || input.max_python_files > 1000 {
        return Err(ToolError::invalid_input(
            "max_python_files must be between 1 and 1000",
        ));
    }
    if input.commands.len() > MAX_CUSTOM_GATES {
        return Err(ToolError::invalid_input(format!(
            "commands may contain at most {MAX_CUSTOM_GATES} custom gates"
        )));
    }

    let gates = build_gate_plan(
        context,
        profile,
        level,
        input.max_python_files,
        &input.commands,
    )?;
    if gates.is_empty() {
        let mut outcome = verifier_tool_result(&RunVerifiersOutput {
            success: false,
            profile: profile.as_str().to_string(),
            level: level.as_str().to_string(),
            workspace: context.workspace().display().to_string(),
            gate_count: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            verifier_verdict: VerifierVerdict::Partial,
            summary: "No verifier gates were detected. Provide custom commands or choose a profile that matches this workspace.".to_string(),
            gates: Vec::new(),
        })?;
        reject_verification_artifact(&mut outcome);
        return Ok(outcome);
    }

    let revision_before = capture_workspace_revision(context.workspace()).await;
    let mut results = futures_util::future::join_all(
        gates
            .into_iter()
            .map(|gate| run_gate_with_timeout(context, shell, gate, VERIFIER_GATE_TIMEOUT_MS)),
    )
    .await;
    results.sort_by(|left, right| left.name.cmp(&right.name));
    let passed = results
        .iter()
        .filter(|result| result.status == GateStatus::Passed)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == GateStatus::Failed)
        .count();
    let skipped = results
        .iter()
        .filter(|result| result.status == GateStatus::Skipped)
        .count();
    let success = failed == 0 && skipped == 0;
    let summary = if success {
        format!("All {passed} verifier gates passed.")
    } else {
        format!("{passed} passed, {failed} failed, {skipped} skipped.")
    };
    let output = RunVerifiersOutput {
        success,
        profile: profile.as_str().to_string(),
        level: level.as_str().to_string(),
        workspace: context.workspace().display().to_string(),
        gate_count: results.len(),
        passed,
        failed,
        skipped,
        verifier_verdict: VerifierVerdict::from_counts(results.len(), failed, skipped),
        summary,
        gates: results,
    };
    let mut outcome = verifier_tool_result(&output)?;
    if outcome.is_success() {
        let revision_after = capture_workspace_revision(context.workspace()).await;
        attach_verification_artifact(
            &mut outcome,
            "run_verifiers",
            &json!({
                "commands": input.commands,
                "level": level.as_str(),
                "max_python_files": input.max_python_files,
                "profile": profile.as_str(),
            }),
            format!(
                "run_verifiers profile={} level={} gates={}",
                output.profile, output.level, output.gate_count
            ),
            output.summary.clone(),
            revision_before,
            revision_after,
        );
    } else {
        reject_verification_artifact(&mut outcome);
    }
    Ok(outcome)
}

fn verifier_tool_result(output: &RunVerifiersOutput) -> Result<ToolOutcome, ToolError> {
    ToolOutcome::json(output)
        .map_err(|error| ToolError::execution_failed(error.to_string()))
        .map(|mut outcome| {
            outcome.side_effect = ToolSideEffectStatus::Indeterminate;
            if !output.success {
                outcome.operation = ToolOperationStatus::Failed;
                outcome.retry = ToolRetryDisposition::Unsafe;
            }
            outcome.metadata = Some(json!({
                "verifier_verdict": output.verifier_verdict,
                "gate_count": output.gate_count,
                "passed": output.passed,
                "failed": output.failed,
                "skipped": output.skipped,
            }));
            outcome
        })
}

fn build_gate_plan(
    context: &ProductionToolContext,
    profile: VerifierProfile,
    level: VerifierLevel,
    max_python_files: usize,
    custom_commands: &[CustomVerifierInput],
) -> Result<Vec<VerifierGate>, ToolError> {
    let workspace = context.workspace();
    let mut gates = Vec::new();
    if profile == VerifierProfile::Auto && workspace.join(".git").exists() {
        gates.push(gate(
            "git-whitespace",
            "git",
            workspace,
            "git",
            ["diff", "--check"],
        ));
    }
    if profile_matches(profile, VerifierProfile::Rust) && workspace.join("Cargo.toml").exists() {
        add_rust_gates(&mut gates, workspace, level);
    }
    if profile_matches(profile, VerifierProfile::Node) && workspace.join("package.json").exists() {
        add_node_gates(&mut gates, workspace, level);
    }
    if profile_matches(profile, VerifierProfile::Python) && has_python_project(workspace) {
        add_python_gates(&mut gates, workspace, level, max_python_files);
    }
    if profile_matches(profile, VerifierProfile::Go) && workspace.join("go.mod").exists() {
        add_go_gates(&mut gates, workspace, level);
    }
    for custom in custom_commands {
        gates.push(custom_gate(context, custom)?);
    }
    Ok(gates)
}

fn profile_matches(selected: VerifierProfile, candidate: VerifierProfile) -> bool {
    selected == VerifierProfile::Auto || selected == candidate
}

fn add_rust_gates(gates: &mut Vec<VerifierGate>, workspace: &Path, level: VerifierLevel) {
    let locked = workspace.join("Cargo.lock").exists();
    gates.push(gate(
        "rust-fmt",
        "rust",
        workspace,
        "cargo",
        ["fmt", "--all", "--", "--check"],
    ));
    let metadata_args = if locked {
        vec!["metadata", "--locked", "--format-version", "1", "--no-deps"]
    } else {
        vec!["metadata", "--format-version", "1", "--no-deps"]
    };
    gates.push(gate_vec(
        "rust-metadata",
        "rust",
        workspace,
        "cargo",
        metadata_args,
    ));
    let mut check_args = vec!["check", "--workspace", "--all-targets"];
    if locked {
        check_args.push("--locked");
    }
    gates.push(gate_vec(
        "rust-check",
        "rust",
        workspace,
        "cargo",
        check_args,
    ));
    if level == VerifierLevel::Full {
        let mut clippy_args = vec!["clippy", "--workspace", "--all-targets", "--all-features"];
        if locked {
            clippy_args.push("--locked");
        }
        clippy_args.extend(["--", "-D", "warnings"]);
        gates.push(gate_vec(
            "rust-clippy",
            "rust",
            workspace,
            "cargo",
            clippy_args,
        ));
        let mut test_args = vec!["test", "--workspace", "--all-features"];
        if locked {
            test_args.push("--locked");
        }
        gates.push(gate_vec("rust-test", "rust", workspace, "cargo", test_args));
    }
}

fn add_node_gates(gates: &mut Vec<VerifierGate>, workspace: &Path, level: VerifierLevel) {
    let Some(scripts) = package_json_scripts(workspace) else {
        gates.push(skipped_gate(
            "node-package-json",
            "node",
            workspace,
            "package.json is missing or could not be parsed",
        ));
        return;
    };
    let package_manager = detect_node_package_manager(workspace);
    for script in ["format:check", "check", "typecheck", "lint"] {
        if has_meaningful_script(&scripts, script) {
            gates.push(node_script_gate(workspace, package_manager, script));
        }
    }
    if level == VerifierLevel::Full && has_meaningful_script(&scripts, "test") {
        gates.push(node_script_gate(workspace, package_manager, "test"));
    }
}

fn add_python_gates(
    gates: &mut Vec<VerifierGate>,
    workspace: &Path,
    level: VerifierLevel,
    max_python_files: usize,
) {
    match collect_python_files(workspace, max_python_files) {
        PythonFiles::Files(files) if !files.is_empty() => {
            gates.push(python_syntax_gate(workspace, &files));
        }
        PythonFiles::TooMany { limit, found } => gates.push(skipped_gate(
            "python-syntax",
            "python",
            workspace,
            format!(
                "found more than {limit} Python files ({found}); raise max_python_files to verify them"
            ),
        )),
        PythonFiles::Files(_) => {}
    }
    if level == VerifierLevel::Full && has_pytest_signal(workspace) {
        gates.push(python_module_gate(
            "python-pytest",
            workspace,
            ["-m", "pytest"],
        ));
    }
}

fn add_go_gates(gates: &mut Vec<VerifierGate>, workspace: &Path, level: VerifierLevel) {
    gates.push(gate("go-test", "go", workspace, "go", ["test", "./..."]));
    if level == VerifierLevel::Full {
        gates.push(gate("go-vet", "go", workspace, "go", ["vet", "./..."]));
    }
}

fn gate<const N: usize>(
    name: &str,
    ecosystem: &str,
    cwd: &Path,
    program: &str,
    args: [&str; N],
) -> VerifierGate {
    gate_vec(name, ecosystem, cwd, program, args)
}

fn gate_vec<I, S>(name: &str, ecosystem: &str, cwd: &Path, program: &str, args: I) -> VerifierGate
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    VerifierGate {
        name: name.to_string(),
        ecosystem: ecosystem.to_string(),
        cwd: cwd.to_path_buf(),
        program: Some(program.to_string()),
        args: args
            .into_iter()
            .map(|argument| argument.as_ref().to_string())
            .collect(),
        env: Vec::new(),
        skipped_reason: None,
    }
}

fn skipped_gate(
    name: &str,
    ecosystem: &str,
    cwd: &Path,
    reason: impl Into<String>,
) -> VerifierGate {
    VerifierGate {
        name: name.to_string(),
        ecosystem: ecosystem.to_string(),
        cwd: cwd.to_path_buf(),
        program: None,
        args: Vec::new(),
        env: Vec::new(),
        skipped_reason: Some(reason.into()),
    }
}

fn custom_gate(
    context: &ProductionToolContext,
    custom: &CustomVerifierInput,
) -> Result<VerifierGate, ToolError> {
    if custom.name.trim().is_empty() {
        return Err(ToolError::invalid_input(
            "Custom verifier command is missing 'name'",
        ));
    }
    if custom.program.trim().is_empty() {
        return Err(ToolError::invalid_input(format!(
            "Custom verifier '{}' is missing 'program'",
            custom.name
        )));
    }
    let cwd = match custom.cwd.as_deref() {
        Some(raw) if !raw.trim().is_empty() => context.resolve_path(raw)?,
        _ => context.workspace().to_path_buf(),
    };
    Ok(VerifierGate {
        name: custom.name.clone(),
        ecosystem: "custom".to_string(),
        cwd,
        program: Some(custom.program.clone()),
        args: custom.args.clone(),
        env: Vec::new(),
        skipped_reason: None,
    })
}

fn node_script_gate(
    workspace: &Path,
    package_manager: NodePackageManager,
    script: &str,
) -> VerifierGate {
    let (program, args) = package_manager.command_for_script(script);
    gate_vec(&format!("node-{script}"), "node", workspace, program, args)
}

fn python_syntax_gate(workspace: &Path, files: &[PathBuf]) -> VerifierGate {
    let Some((program, mut args)) = python_command_parts() else {
        return skipped_gate(
            "python-syntax",
            "python",
            workspace,
            "Python interpreter is not installed or not in PATH",
        );
    };
    args.push("-c".to_string());
    args.push(PYTHON_SYNTAX_SCRIPT.to_string());
    args.extend(files.iter().map(|path| path.display().to_string()));
    let mut gate = gate_vec("python-syntax", "python", workspace, &program, args);
    gate.env
        .push(("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string()));
    gate
}

fn python_module_gate<const N: usize>(
    name: &str,
    workspace: &Path,
    module_args: [&str; N],
) -> VerifierGate {
    let Some((program, mut args)) = python_command_parts() else {
        return skipped_gate(
            name,
            "python",
            workspace,
            "Python interpreter is not installed or not in PATH",
        );
    };
    args.extend(module_args.into_iter().map(str::to_string));
    gate_vec(name, "python", workspace, &program, args)
}

fn python_command_parts() -> Option<(String, Vec<String>)> {
    static PYTHON: OnceLock<Option<(String, Vec<String>)>> = OnceLock::new();
    PYTHON
        .get_or_init(|| {
            for candidate in ["python3", "python", "py -3"] {
                let mut parts = candidate.split_whitespace();
                let program = parts.next()?;
                let fixed_args: Vec<String> = parts.map(str::to_string).collect();
                let status = Command::new(program)
                    .args(&fixed_args)
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                if status.is_ok_and(|status| status.success()) {
                    return Some((program.to_string(), fixed_args));
                }
            }
            None
        })
        .clone()
}

const PYTHON_SYNTAX_SCRIPT: &str = r#"
import ast
import pathlib
import sys

failures = []
for raw in sys.argv[1:]:
    path = pathlib.Path(raw)
    try:
        source = path.read_text(encoding="utf-8")
        ast.parse(source, filename=raw)
    except Exception as exc:
        failures.append(f"{raw}: {exc.__class__.__name__}: {exc}")

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)

print(f"parsed {len(sys.argv) - 1} Python file(s)")
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodePackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl NodePackageManager {
    fn command_for_script(self, script: &str) -> (&'static str, Vec<String>) {
        match self {
            Self::Npm => ("npm", vec!["run".to_string(), script.to_string()]),
            Self::Pnpm => ("pnpm", vec!["run".to_string(), script.to_string()]),
            Self::Yarn => ("yarn", vec!["run".to_string(), script.to_string()]),
            Self::Bun => ("bun", vec!["run".to_string(), script.to_string()]),
        }
    }
}

fn detect_node_package_manager(workspace: &Path) -> NodePackageManager {
    if workspace.join("pnpm-lock.yaml").exists() {
        NodePackageManager::Pnpm
    } else if workspace.join("yarn.lock").exists() {
        NodePackageManager::Yarn
    } else if workspace.join("bun.lock").exists() || workspace.join("bun.lockb").exists() {
        NodePackageManager::Bun
    } else {
        NodePackageManager::Npm
    }
}

fn package_json_scripts(workspace: &Path) -> Option<HashMap<String, String>> {
    let raw = fs::read_to_string(workspace.join("package.json")).ok()?;
    let parsed = serde_json::from_str::<Value>(&raw).ok()?;
    let scripts = parsed.get("scripts")?.as_object()?;
    Some(
        scripts
            .iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .map(|script| (key.clone(), script.to_string()))
            })
            .collect(),
    )
}

fn has_meaningful_script(scripts: &HashMap<String, String>, name: &str) -> bool {
    let Some(script) = scripts.get(name).map(|value| value.trim()) else {
        return false;
    };
    !(script.is_empty()
        || name == "test"
            && script.contains("Error: no test specified")
            && script.contains("exit 1"))
}

fn has_python_project(workspace: &Path) -> bool {
    workspace.join("pyproject.toml").exists()
        || workspace.join("setup.py").exists()
        || workspace.join("setup.cfg").exists()
        || workspace.join("requirements.txt").exists()
        || match collect_python_files(workspace, 1) {
            PythonFiles::Files(files) => !files.is_empty(),
            PythonFiles::TooMany { .. } => true,
        }
}

fn has_pytest_signal(workspace: &Path) -> bool {
    if workspace.join("pytest.ini").exists()
        || workspace.join("tox.ini").exists()
        || workspace.join("tests").is_dir()
    {
        return true;
    }
    fs::read_to_string(workspace.join("pyproject.toml"))
        .map(|raw| raw.contains("pytest") || raw.contains("[tool.pytest"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PythonFiles {
    Files(Vec<PathBuf>),
    TooMany { limit: usize, found: usize },
}

fn collect_python_files(workspace: &Path, limit: usize) -> PythonFiles {
    let mut files = BTreeSet::new();
    collect_python_files_inner(workspace, workspace, limit, &mut files);
    let found = files.len();
    if found > limit {
        PythonFiles::TooMany { limit, found }
    } else {
        PythonFiles::Files(files.into_iter().collect())
    }
}

fn collect_python_files_inner(
    root: &Path,
    directory: &Path,
    limit: usize,
    files: &mut BTreeSet<PathBuf>,
) {
    if files.len() > limit {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() > limit {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            if should_skip_dir_name(&entry.file_name().to_string_lossy()) {
                continue;
            }
            collect_python_files_inner(root, &path, limit, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("py")
            && let Ok(relative) = path.strip_prefix(root)
        {
            files.insert(relative.to_path_buf());
        }
    }
}

fn should_skip_dir_name(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".venv"
            | "venv"
            | "env"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".tox"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
    )
}

async fn run_gate_with_timeout(
    context: &ProductionToolContext,
    shell: &ExecShellOptions,
    gate: VerifierGate,
    timeout_ms: u64,
) -> GateResult {
    let command = render_command(gate.program.as_deref(), &gate.args);
    if let Some(reason) = gate.skipped_reason {
        return skipped_result(gate.name, gate.ecosystem, command, gate.cwd, reason);
    }
    let Some(program) = gate.program else {
        return skipped_result(
            gate.name,
            gate.ecosystem,
            command,
            gate.cwd,
            "verifier has no executable program".to_string(),
        );
    };
    let env = gate.env.into_iter().collect::<HashMap<_, _>>();
    let output = match execute_managed_program(
        context,
        &shell.shell_manager,
        shell.owner.clone(),
        &command,
        &program,
        &gate.args,
        &gate.cwd,
        timeout_ms,
        shell.elevated_sandbox_policy.clone(),
        env,
    )
    .await
    {
        Ok(output) => output,
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
            }) =>
        {
            return skipped_result(
                gate.name,
                gate.ecosystem,
                command,
                gate.cwd,
                format!("{program} is not installed or not in PATH"),
            );
        }
        Err(error) => {
            return GateResult {
                name: gate.name,
                ecosystem: gate.ecosystem,
                status: GateStatus::Failed,
                command,
                cwd: gate.cwd.display().to_string(),
                exit_code: None,
                duration_ms: 0,
                stdout: String::new(),
                stderr: format!("Failed to spawn verifier: {error}"),
                stdout_truncated: false,
                stderr_truncated: false,
                skipped_reason: None,
            };
        }
    };
    let mut stderr_raw = output.stderr;
    if output.status == ShellStatus::TimedOut {
        stderr_raw.push_str("\nVerifier timed out; managed process tree was killed.");
    } else if output.status == ShellStatus::Killed
        && context
            .cancellation_token()
            .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    {
        stderr_raw.push_str("\nVerifier canceled; managed process tree was killed.");
    }
    let (stdout, stdout_truncated) = truncate_with_note(&output.stdout, MAX_GATE_OUTPUT_CHARS);
    let (stderr, stderr_truncated) = truncate_with_note(&stderr_raw, MAX_GATE_OUTPUT_CHARS);
    GateResult {
        name: gate.name,
        ecosystem: gate.ecosystem,
        status: if output.status == ShellStatus::Completed {
            GateStatus::Passed
        } else {
            GateStatus::Failed
        },
        command,
        cwd: gate.cwd.display().to_string(),
        exit_code: output.exit_code,
        duration_ms: output.duration_ms,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        skipped_reason: None,
    }
}

fn skipped_result(
    name: String,
    ecosystem: String,
    command: String,
    cwd: PathBuf,
    reason: String,
) -> GateResult {
    GateResult {
        name,
        ecosystem,
        status: GateStatus::Skipped,
        command,
        cwd: cwd.display().to_string(),
        exit_code: None,
        duration_ms: 0,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        skipped_reason: Some(reason),
    }
}

fn render_command(program: Option<&str>, args: &[String]) -> String {
    let mut parts = vec![program.unwrap_or("<unavailable>").to_string()];
    parts.extend(args.iter().cloned());
    parts.join(" ")
}

fn truncate_with_note(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let end = char_boundary_index(text, max_chars);
    let truncated = &text[..end];
    let omitted_chars = text
        .chars()
        .count()
        .saturating_sub(truncated.chars().count());
    (
        format!(
            "{truncated}\n\n[output truncated to {max_chars} characters; {omitted_chars} characters omitted]"
        ),
        true,
    )
}

fn char_boundary_index(text: &str, max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return index;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{ShellPolicy, new_shared_shell_manager};
    use codewhale_protocol::agent_runtime::ToolEvidenceStatus;

    fn initialized_workspace() -> tempfile::TempDir {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join(".gitignore"), "/target\n").unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(workspace.path())
                .status()
                .unwrap()
                .success()
        );
        workspace
    }

    fn shell(workspace: &Path) -> ExecShellOptions {
        ExecShellOptions::new(
            new_shared_shell_manager(workspace.to_path_buf()),
            ShellPolicy::Full,
        )
    }

    #[test]
    fn auto_plan_keeps_all_supported_ecosystems() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint ."}}"#,
        )
        .unwrap();
        fs::write(temp.path().join("main.py"), "print('ok')\n").unwrap();
        fs::write(temp.path().join("go.mod"), "module fixture\n").unwrap();
        let context = ProductionToolContext::new(temp.path());
        let gates = build_gate_plan(
            &context,
            VerifierProfile::Auto,
            VerifierLevel::Quick,
            200,
            &[],
        )
        .unwrap();
        let ecosystems: BTreeSet<_> = gates.iter().map(|gate| gate.ecosystem.as_str()).collect();
        assert!(ecosystems.is_superset(&BTreeSet::from(["git", "rust", "node", "python", "go"])));
    }

    #[test]
    fn production_input_rejects_background() {
        let error =
            serde_json::from_value::<RunVerifiersInput>(json!({"background": true})).unwrap_err();
        assert!(error.to_string().contains("unknown field `background`"));
    }

    #[test]
    fn custom_cwd_cannot_escape_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let context = ProductionToolContext::new(temp.path());
        let custom = CustomVerifierInput {
            name: "escape".to_string(),
            program: "true".to_string(),
            args: Vec::new(),
            cwd: Some("../outside".to_string()),
        };
        assert!(matches!(
            custom_gate(&context, &custom),
            Err(ToolError::PathEscape { .. })
        ));
    }

    #[test]
    fn rust_full_plan_is_a_strict_quick_superset() {
        let workspace = initialized_workspace();
        fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(workspace.path().join("Cargo.lock"), "version = 4\n").unwrap();
        let context = ProductionToolContext::new(workspace.path());
        let quick = build_gate_plan(
            &context,
            VerifierProfile::Rust,
            VerifierLevel::Quick,
            200,
            &[],
        )
        .unwrap();
        let full = build_gate_plan(
            &context,
            VerifierProfile::Rust,
            VerifierLevel::Full,
            200,
            &[],
        )
        .unwrap();
        assert_eq!(quick.len(), 3);
        assert_eq!(full.len(), 5);
        assert!(full.iter().any(|gate| gate.name == "rust-clippy"));
        assert!(full.iter().any(|gate| gate.name == "rust-test"));
    }

    #[tokio::test]
    async fn custom_gates_run_in_parallel_and_sort_stably_with_neutral_artifact() {
        let workspace = initialized_workspace();
        let context = ProductionToolContext::new(workspace.path());
        let executable = std::env::current_exe().unwrap().display().to_string();
        let input = json!({
            "commands": [
                {"name": "zeta", "program": executable, "args": ["--list"]},
                {"name": "alpha", "program": executable, "args": ["--list"]}
            ]
        });
        let outcome = execute_run_verifiers(input, &context, &shell(workspace.path()))
            .await
            .unwrap();
        assert!(outcome.is_success(), "{}", outcome.content);
        let output: RunVerifiersOutput = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(
            output
                .gates
                .iter()
                .map(|gate| gate.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "git-whitespace", "zeta"]
        );
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Produced);
        assert!(outcome.workspace_revision.is_some());
        let serialized = serde_json::to_string(&outcome).unwrap();
        for forbidden in ["hunt_verdict", "goal_status", "task_updates"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[tokio::test]
    async fn missing_custom_program_is_skipped_and_rejects_evidence() {
        let workspace = initialized_workspace();
        let context = ProductionToolContext::new(workspace.path());
        let outcome = execute_run_verifiers(
            json!({"commands": [{"name": "missing", "program": "codewhale-program-that-does-not-exist"}]}),
            &context,
            &shell(workspace.path()),
        )
        .await
        .unwrap();
        let output: RunVerifiersOutput = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(output.skipped, 1);
        assert_eq!(output.verifier_verdict, VerifierVerdict::Partial);
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Rejected);
    }
}
