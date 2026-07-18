//! Parallel verifier ensemble tool: `run_verifiers`.
//!
//! This is the agent-facing path for "parallelize the verifier, not the
//! generator": one tool call fans out to independent project checks across
//! common ecosystems and returns a single structured verdict.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shlex::try_join;

use crate::dependencies::ExternalTool;
#[cfg(test)]
use codewhale_tools::shell::ShellStatus;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
};

const DEFAULT_MAX_PYTHON_FILES: usize = 200;
const MAX_CUSTOM_GATES: usize = 12;
const VERIFIER_GATE_TIMEOUT_MS: u64 = 600_000;

/// Tool for running independent verifier gates concurrently.
pub struct RunVerifiersTool;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RunVerifiersInput {
    profile: String,
    level: String,
    max_python_files: usize,
    commands: Vec<CustomVerifierInput>,
    background: bool,
}

impl Default for RunVerifiersInput {
    fn default() -> Self {
        Self {
            profile: "auto".to_string(),
            level: "quick".to_string(),
            max_python_files: DEFAULT_MAX_PYTHON_FILES,
            commands: Vec::new(),
            background: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
struct BackgroundGateJob {
    name: String,
    ecosystem: String,
    status: String,
    command: String,
    cwd: String,
    task_id: Option<String>,
    skipped_reason: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunVerifiersBackgroundOutput {
    success: bool,
    profile: String,
    level: String,
    workspace: String,
    background: bool,
    gate_count: usize,
    started: usize,
    skipped: usize,
    failed_to_start: usize,
    summary: String,
    jobs: Vec<BackgroundGateJob>,
}

#[async_trait]
impl ToolSpec for RunVerifiersTool {
    fn name(&self) -> &'static str {
        "run_verifiers"
    }

    fn description(&self) -> &'static str {
        "Run independent verifier gates in parallel across detected Rust, Node, Python, and Go projects. Supports explicit custom verifier commands as program+args without requiring Bash."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {
                    "type": "string",
                    "enum": ["auto", "rust", "node", "python", "go"],
                    "default": "auto",
                    "description": "Which ecosystem verifier set to run. 'auto' detects all supported project types in the workspace."
                },
                "level": {
                    "type": "string",
                    "enum": ["quick", "full"],
                    "default": "quick",
                    "description": "Quick runs fast syntax/drift/build checks. Full adds heavier test/lint gates where available."
                },
                "max_python_files": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 1000,
                    "default": DEFAULT_MAX_PYTHON_FILES,
                    "description": "Maximum Python files to syntax-parse in the built-in python-syntax gate."
                },
                "commands": {
                    "type": "array",
                    "description": "Optional explicit verifier gates. Commands run directly as program+args, not through a shell. Use program='bash', args=['-lc', '...'] only when Bash is intentionally part of the verifier.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Short unique gate name."
                            },
                            "program": {
                                "type": "string",
                                "description": "Executable to spawn, for example 'uv', 'pytest', 'npm', 'make', 'cmd', 'powershell', or 'bash'."
                            },
                            "args": {
                                "type": "array",
                                "items": { "type": "string" },
                                "default": [],
                                "description": "Arguments passed directly to the executable."
                            },
                            "cwd": {
                                "type": "string",
                                "description": "Optional working directory relative to the workspace."
                            }
                        },
                        "required": ["name", "program"],
                        "additionalProperties": false
                    },
                    "default": []
                },
                "background": {
                    "type": "boolean",
                    "default": false,
                    "description": "Start verifier gates as background shell jobs and return task_ids immediately. Use for long build/test/lint gates; completion is tracked in task/status state, and exec_shell_wait/task_shell_wait are only for early output, final output, or true dependency barriers."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ExecutesCode, ToolCapability::Sandboxable]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    fn starts_detached_for(&self, input: &Value) -> bool {
        input.get("background").and_then(Value::as_bool) == Some(true)
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let input: RunVerifiersInput = serde_json::from_value(input)
            .map_err(|err| ToolError::invalid_input(err.to_string()))?;
        let profile = VerifierProfile::parse(input.profile.as_str())?;
        let level = VerifierLevel::parse(input.level.as_str())?;
        let verifier_params = normalized_goal_verifier_params(&input, profile, level);
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
        if input.background {
            // M4-C deletion point: the interactive TUI is the only remaining
            // consumer of detached verifier jobs. The production runtime
            // rejects background and always returns an observable result.
            let gates = build_gate_plan(
                context,
                profile,
                level,
                input.max_python_files,
                &input.commands,
            )?;
            return start_background_gates(context, profile, level, gates);
        }

        let revision_before =
            crate::tools::goal::capture_workspace_revision(context.workspace()).await;
        let shell = crate::tools::shell::exec_shell_options(context);
        let production_input = json!({
            "commands": input.commands,
            "level": level.as_str(),
            "max_python_files": input.max_python_files,
            "profile": profile.as_str(),
        });
        let mut result = codewhale_tools::execute_run_verifiers(
            production_input,
            context.production_context(),
            &shell,
        )
        .await?;
        let output = serde_json::from_str::<codewhale_tools::RunVerifiersOutput>(&result.content)
            .map_err(|error| {
            ToolError::execution_failed(format!(
                "tools-owned verifier returned invalid structured output: {error}"
            ))
        })?;
        attach_interactive_goal_projection(&mut result, output.verifier_verdict)?;
        let check = format!(
            "run_verifiers profile={} level={} gates={}",
            output.profile, output.level, output.gate_count
        );
        let summary = output.summary.clone();
        if result.is_success() {
            let built_in_passed = output.gates.iter().any(|gate| {
                gate.ecosystem != "custom" && gate.status == codewhale_tools::GateStatus::Passed
            });
            let revision_after =
                crate::tools::goal::capture_workspace_revision(context.workspace()).await;
            crate::tools::goal::attach_goal_evidence_artifact(
                &mut result,
                self.name(),
                &verifier_params,
                check.clone(),
                summary.clone(),
                revision_before.clone(),
                revision_after.clone(),
            );
            if level == VerifierLevel::Full && built_in_passed {
                if let Some(contract) = context.goal_contract.as_ref() {
                    crate::tools::goal::attach_host_verification(
                        &mut result,
                        self.name(),
                        contract,
                        &verifier_params,
                        crate::tools::goal::HostVerifierObservation {
                            check,
                            summary,
                            revision_before,
                            revision_after,
                        },
                    );
                } else {
                    crate::tools::goal::reject_host_verification(
                        &mut result,
                        "run_verifiers produced an evidence artifact, but no active Goal task contract was bound when it started",
                    );
                }
            } else {
                let reason = if level != VerifierLevel::Full {
                    "run_verifiers completed successfully, but Goal completion evidence requires level=full"
                } else {
                    "run_verifiers completed successfully, but no framework-defined verifier gate passed; custom commands alone cannot complete a Goal"
                };
                crate::tools::goal::reject_host_verification(&mut result, reason);
            }
        } else {
            crate::tools::goal::reject_goal_evidence_artifact(&mut result);
        }
        Ok(result)
    }
}

fn attach_interactive_goal_projection(
    result: &mut ToolOutcome,
    verdict: codewhale_tools::VerifierVerdict,
) -> Result<(), ToolError> {
    // M4-C/M5 deletion point: these fields belong to the unmigrated
    // interactive Goal/task consumer. The tools-owned production result stays
    // neutral and contains only verifier facts and revision-bound evidence.
    let (hunt_verdict, goal_status) = match verdict {
        codewhale_tools::VerifierVerdict::Pass => ("hunted", "complete"),
        codewhale_tools::VerifierVerdict::Partial => ("wounded", "paused"),
        codewhale_tools::VerifierVerdict::Fail => ("escaped", "blocked"),
    };
    let verifier_verdict = serde_json::to_value(verdict)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;
    let mut content: Value = serde_json::from_str(&result.content)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;
    let object = content.as_object_mut().ok_or_else(|| {
        ToolError::execution_failed("tools-owned verifier output is not a JSON object")
    })?;
    object.insert("hunt_verdict".to_string(), json!(hunt_verdict));
    object.insert("goal_status".to_string(), json!(goal_status));
    result.content = serde_json::to_string(&content)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;

    let metadata = result.metadata.get_or_insert_with(|| json!({}));
    let metadata = metadata.as_object_mut().ok_or_else(|| {
        ToolError::execution_failed("tools-owned verifier metadata is not a JSON object")
    })?;
    metadata.insert("verifier_verdict".to_string(), verifier_verdict);
    metadata.insert("hunt_verdict".to_string(), json!(hunt_verdict));
    metadata.insert("goal_status".to_string(), json!(goal_status));
    metadata.insert(
        "task_updates".to_string(),
        json!({"hunt_verdict": hunt_verdict}),
    );
    Ok(())
}

fn normalized_goal_verifier_params(
    input: &RunVerifiersInput,
    profile: VerifierProfile,
    level: VerifierLevel,
) -> Value {
    json!({
        "background": input.background,
        "commands": input.commands,
        "level": level.as_str(),
        "max_python_files": input.max_python_files,
        "profile": profile.as_str(),
    })
}

fn start_background_gates(
    context: &ToolContext,
    profile: VerifierProfile,
    level: VerifierLevel,
    gates: Vec<VerifierGate>,
) -> Result<ToolOutcome, ToolError> {
    let mut jobs = Vec::with_capacity(gates.len());
    let mut started = 0usize;
    let mut skipped = 0usize;
    let mut failed_to_start = 0usize;

    for gate in gates {
        let cwd = gate.cwd.display().to_string();
        let Some(program) = gate.program.as_deref() else {
            skipped += 1;
            jobs.push(BackgroundGateJob {
                name: gate.name,
                ecosystem: gate.ecosystem,
                status: "skipped".to_string(),
                command: String::new(),
                cwd,
                task_id: None,
                skipped_reason: gate.skipped_reason,
                error: None,
            });
            continue;
        };

        let command = render_gate_command(program, &gate.args)?;
        let env: HashMap<String, String> = gate.env.into_iter().collect();
        let spawn_result = {
            let mut manager = context
                .shell_manager
                .lock()
                .map_err(|_| ToolError::execution_failed("shell manager lock poisoned"))?;
            manager.execute_with_options_env(
                &command,
                Some(&cwd),
                VERIFIER_GATE_TIMEOUT_MS,
                true,
                None,
                false,
                context.elevated_sandbox_policy.clone(),
                env,
            )
        };

        match spawn_result {
            Ok(result) => {
                started += 1;
                jobs.push(BackgroundGateJob {
                    name: gate.name,
                    ecosystem: gate.ecosystem,
                    status: "running".to_string(),
                    command,
                    cwd,
                    task_id: result.task_id,
                    skipped_reason: None,
                    error: None,
                });
            }
            Err(err) => {
                failed_to_start += 1;
                jobs.push(BackgroundGateJob {
                    name: gate.name,
                    ecosystem: gate.ecosystem,
                    status: "failed_to_start".to_string(),
                    command,
                    cwd,
                    task_id: None,
                    skipped_reason: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    jobs.sort_by(|a, b| a.name.cmp(&b.name));
    let success = failed_to_start == 0 && started > 0;
    let summary = if failed_to_start == 0 {
        format!(
            "Started {started} verifier gate(s) in the background; {skipped} skipped. Completion is tracked in task/status state. Continue inspecting or implementing while they run."
        )
    } else {
        format!(
            "Started {started} verifier gate(s), failed to start {failed_to_start}, and skipped {skipped}. Completion is tracked in task/status state. Continue inspecting or implementing while they run."
        )
    };
    let task_ids = jobs
        .iter()
        .filter_map(|job| job.task_id.clone())
        .collect::<Vec<_>>();
    let output = RunVerifiersBackgroundOutput {
        success,
        profile: profile.as_str().to_string(),
        level: level.as_str().to_string(),
        workspace: context.workspace().display().to_string(),
        background: true,
        gate_count: jobs.len(),
        started,
        skipped,
        failed_to_start,
        summary,
        jobs,
    };

    let mut result =
        ToolOutcome::json(&output).map_err(|err| ToolError::execution_failed(err.to_string()))?;
    result.side_effect = ToolSideEffectStatus::Indeterminate;
    if !success {
        result.operation = ToolOperationStatus::Failed;
        result.retry = ToolRetryDisposition::Unsafe;
    }
    Ok(result.with_metadata(json!({
        "backgrounded": true,
        "detached_start": true,
        "verifier_background": true,
        "auto_resume_on_completion": false,
        "completion_surface": "task_status",
        "background_policy": "nonblocking",
        "task_ids": task_ids,
        "poll_with": ["exec_shell_wait", "task_shell_wait"]
    })))
}

fn render_gate_command(program: &str, args: &[String]) -> Result<String, ToolError> {
    try_join(std::iter::once(program).chain(args.iter().map(String::as_str)))
        .map_err(|err| ToolError::execution_failed(format!("failed to render gate command: {err}")))
}

fn build_gate_plan(
    context: &ToolContext,
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
    let scripts = package_json_scripts(workspace);
    let Some(scripts) = scripts else {
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
            gates.push(node_script_gate(workspace, &package_manager, script));
        }
    }
    if level == VerifierLevel::Full && has_meaningful_script(&scripts, "test") {
        gates.push(node_script_gate(workspace, &package_manager, "test"));
    }
}

fn add_python_gates(
    gates: &mut Vec<VerifierGate>,
    workspace: &Path,
    level: VerifierLevel,
    max_python_files: usize,
) {
    let python_files = collect_python_files(workspace, max_python_files);
    match python_files {
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
            .map(|arg| arg.as_ref().to_string())
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
    context: &ToolContext,
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
    package_manager: &NodePackageManager,
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
    let spec = crate::dependencies::Python::resolve()?;
    Some(crate::dependencies::split_interpreter_spec(&spec))
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
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
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
    let pyproject = workspace.join("pyproject.toml");
    fs::read_to_string(pyproject)
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
    dir: &Path,
    limit: usize,
    files: &mut BTreeSet<PathBuf>,
) {
    if files.len() > limit {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() > limit {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if should_skip_dir_name(&name.to_string_lossy()) {
                continue;
            }
            collect_python_files_inner(root, &path, limit, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("py")
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

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_protocol::agent_runtime::{ToolArtifactStatus, ToolEvidenceStatus};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    const BACKGROUND_COMPLETION_WAIT_MS: u64 = 30_000;

    fn explicit_goal_contract(objective: &str) -> crate::tools::goal::TaskContract {
        let mut goal = crate::tools::goal::GoalState::default();
        goal.create_with_contract(
            objective.to_string(),
            None,
            Vec::new(),
            Vec::new(),
            crate::tools::goal::default_run_verifiers_contract_params(),
        );
        goal.active_task_contract()
            .expect("active task contract")
            .clone()
    }

    fn wait_for_completed_shell(
        manager: &mut codewhale_tools::shell::ShellManager,
        task_id: &str,
    ) -> codewhale_tools::shell::ShellResult {
        let deadline = Instant::now() + Duration::from_millis(BACKGROUND_COMPLETION_WAIT_MS);

        loop {
            let result = manager
                .get_output(task_id, true, 1_000)
                .expect("background output");
            if result.status != ShellStatus::Running || Instant::now() >= deadline {
                return result;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn run_verifiers_requires_user_approval() {
        let tool = RunVerifiersTool;
        assert_eq!(
            tool.approval_requirement(),
            ApprovalRequirement::Required,
            "run_verifiers executes project code and must require approval"
        );
    }

    #[test]
    fn run_verifiers_background_advertises_detached_start() {
        let tool = RunVerifiersTool;
        let schema = tool.input_schema();
        let background_description = schema["properties"]["background"]["description"]
            .as_str()
            .expect("background description");

        assert!(background_description.contains("exec_shell_wait"));
        assert!(background_description.contains("task_shell_wait"));
        assert!(tool.starts_detached_for(&json!({"background": true})));
        assert!(!tool.starts_detached_for(&json!({"profile": "auto"})));
    }

    #[test]
    fn auto_profile_detects_multiple_ecosystems_without_bash() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").expect("cargo manifest");
        fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","test":"echo ok"}}"#,
        )
        .expect("package json");
        fs::write(tmp.path().join("main.py"), "print('ok')\n").expect("python file");
        fs::write(tmp.path().join("go.mod"), "module example.com/app\n").expect("go mod");

        let ctx = ToolContext::new(tmp.path());
        let gates = build_gate_plan(
            &ctx,
            VerifierProfile::Auto,
            VerifierLevel::Quick,
            DEFAULT_MAX_PYTHON_FILES,
            &[],
        )
        .expect("plan");
        let names: BTreeSet<&str> = gates.iter().map(|gate| gate.name.as_str()).collect();

        assert!(names.contains("rust-fmt"));
        assert!(names.contains("node-lint"));
        assert!(names.contains("python-syntax"));
        assert!(names.contains("go-test"));
        assert!(
            gates
                .iter()
                .filter_map(|gate| gate.program.as_deref())
                .all(|program| program != "bash"),
            "built-in verifier gates must not require bash"
        );
    }

    #[test]
    fn custom_commands_can_choose_bash_explicitly() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let custom = CustomVerifierInput {
            name: "shell-check".to_string(),
            program: "bash".to_string(),
            args: vec!["-lc".to_string(), "echo ok".to_string()],
            cwd: None,
        };

        let gate = custom_gate(&ctx, &custom).expect("custom gate");

        assert_eq!(gate.program.as_deref(), Some("bash"));
        assert_eq!(gate.args, vec!["-lc", "echo ok"]);
    }

    #[test]
    fn node_default_npm_init_test_script_is_not_a_verifier() {
        let mut scripts = HashMap::new();
        scripts.insert(
            "test".to_string(),
            "echo \"Error: no test specified\" && exit 1".to_string(),
        );

        assert!(!has_meaningful_script(&scripts, "test"));
    }

    #[tokio::test]
    async fn run_verifiers_executes_custom_direct_command() {
        if !crate::dependencies::RustC::available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let tool = RunVerifiersTool;
        let result = tool
            .execute(
                json!({
                    "profile": "auto",
                    "level": "full",
                    "commands": [
                        {
                            "name": "rustc-version",
                            "program": crate::dependencies::RustC::resolve().expect("rustc"),
                            "args": ["--version"]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("execute");

        let parsed: codewhale_tools::RunVerifiersOutput =
            serde_json::from_str(&result.content).expect("verifier output json");
        assert!(parsed.success, "result: {}", result.content);
        assert_eq!(parsed.passed, 1);
        assert_eq!(parsed.failed, 0);
        assert_eq!(parsed.skipped, 0);
        assert!(
            parsed.gates[0].stdout.contains("rustc"),
            "stdout should include rustc version: {:?}",
            parsed.gates[0].stdout
        );
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Missing);
        assert!(result.evidence.references.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.workspace_revision.is_none());
        let metadata = result.metadata.expect("verifier metadata");
        assert!(
            metadata.get("goal_host_verification").is_none(),
            "a pure custom verifier must never mint Goal completion evidence"
        );
        assert!(
            metadata["goal_host_verification_rejected"]
                .as_str()
                .is_some_and(|reason| reason.contains("custom commands alone")),
            "metadata: {metadata}"
        );
    }

    #[tokio::test]
    async fn only_full_framework_gate_mints_goal_evidence() {
        if !crate::dependencies::Git::available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let status = crate::dependencies::Git::command()
            .expect("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");

        let contract = explicit_goal_contract("verify this workspace");
        let ctx = ToolContext::new(tmp.path()).with_goal_contract(Some(contract));
        let tool = RunVerifiersTool;
        let full = tool
            .execute(json!({"profile": "auto", "level": "full"}), &ctx)
            .await
            .expect("full verifier");
        assert!(full.is_success(), "result: {}", full.content);
        assert_produced_evidence(&full);
        let full_metadata = full.metadata.expect("full verifier metadata");
        assert!(
            full_metadata.get("goal_host_verification").is_some(),
            "an exact contract-matching full framework gate should mint a receipt: {full_metadata}"
        );
        assert!(full_metadata.get("goal_evidence_artifact").is_some());
        assert!(
            full_metadata
                .get("goal_host_verification_rejected")
                .is_none(),
            "metadata: {full_metadata}"
        );

        let quick = tool
            .execute(json!({"profile": "auto", "level": "quick"}), &ctx)
            .await
            .expect("quick verifier");
        assert!(quick.is_success(), "result: {}", quick.content);
        let quick_metadata = quick.metadata.expect("quick verifier metadata");
        assert!(
            quick_metadata.get("goal_host_verification").is_none(),
            "quick gates are useful feedback, not Goal completion evidence"
        );
        assert!(
            quick_metadata["goal_host_verification_rejected"]
                .as_str()
                .is_some_and(|reason| reason.contains("level=full")),
            "metadata: {quick_metadata}"
        );
        assert!(quick_metadata.get("goal_evidence_artifact").is_some());
    }

    #[tokio::test]
    async fn nondefault_verifier_params_are_artifact_only() {
        if !crate::dependencies::Git::available() {
            return;
        }
        let tmp = tempdir().expect("tempdir");
        let status = crate::dependencies::Git::command()
            .expect("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");
        let contract = explicit_goal_contract("verify exact parameters");
        let ctx = ToolContext::new(tmp.path()).with_goal_contract(Some(contract));

        let result = RunVerifiersTool
            .execute(
                json!({
                    "profile": "auto",
                    "level": "full",
                    "max_python_files": DEFAULT_MAX_PYTHON_FILES + 1
                }),
                &ctx,
            )
            .await
            .expect("nondefault verifier run");
        assert!(result.is_success(), "result: {}", result.content);
        let metadata = result.metadata.expect("metadata");
        assert!(metadata.get("goal_evidence_artifact").is_some());
        assert!(metadata.get("goal_host_verification").is_none());
        assert!(
            metadata["goal_host_verification_rejected"]
                .as_str()
                .is_some_and(|reason| reason.contains("does not exactly match")),
            "metadata: {metadata}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn foreground_verifier_cancel_kills_descendant_process_tree() {
        let tmp = tempdir().expect("tempdir");
        let pid_path = tmp.path().join("verifier-cancel-child.pid");
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut ctx = ToolContext::new(tmp.path());
        ctx.set_invocation_cancellation(cancel.clone());
        let task_ctx = ctx.clone();
        let task = tokio::spawn(async move {
            RunVerifiersTool
                .execute(
                    json!({
                        "profile": "auto",
                        "level": "full",
                        "commands": [{
                            "name": "long-custom-gate",
                            "program": "/bin/sh",
                            "args": [
                                "-c",
                                "sleep 60 & child=$!; echo $child > verifier-cancel-child.pid; wait"
                            ]
                        }]
                    }),
                    &task_ctx,
                )
                .await
        });

        let child_pid = wait_for_pid_file(&pid_path, Duration::from_secs(5)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("cancel should settle the verifier promptly")
            .expect("verifier task join")
            .expect("verifier result");
        assert!(
            !result.is_success(),
            "a canceled gate must fail: {}",
            result.content
        );
        let parsed: codewhale_tools::RunVerifiersOutput =
            serde_json::from_str(&result.content).expect("verifier output");
        assert!(
            parsed.gates[0].stderr.contains("Verifier canceled"),
            "stderr: {:?}",
            parsed.gates[0].stderr
        );
        assert_process_exited(child_pid, Duration::from_secs(2)).await;
        assert!(
            ctx.shell_manager
                .lock()
                .expect("shell manager")
                .list_jobs()
                .iter()
                .all(|job| job.status != ShellStatus::Running),
            "canceled foreground verifier left a managed job running"
        );
    }

    #[cfg(unix)]
    async fn wait_for_pid_file(path: &Path, timeout: Duration) -> libc::pid_t {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(raw) = fs::read_to_string(path)
                && let Ok(pid) = raw.trim().parse::<libc::pid_t>()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "process did not publish pid at {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    async fn assert_process_exited(pid: libc::pid_t, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while process_exists(pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if process_exists(pid) {
            // Keep a failing lifecycle test from leaking its own fixture.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            panic!("descendant process {pid} survived managed kill+reap");
        }
    }

    #[cfg(unix)]
    fn process_exists(pid: libc::pid_t) -> bool {
        let status = unsafe { libc::kill(pid, 0) };
        status == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[tokio::test]
    async fn run_verifiers_emits_hunt_verdict_mapping() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let tool = RunVerifiersTool;

        let partial = tool
            .execute(json!({"profile": "auto"}), &ctx)
            .await
            .expect("execute partial verifier");
        assert!(
            !partial.is_success(),
            "a verifier with no runnable gates must not report tool success"
        );
        assert_rejected_evidence(&partial);
        assert_hunt_mapping(&partial.content, "partial", "wounded", "paused");
        assert_hunt_metadata(&partial, "partial", "wounded", "paused");

        if !crate::dependencies::RustC::available() {
            return;
        }

        let pass = tool
            .execute(
                json!({
                    "profile": "auto",
                    "commands": [
                        {
                            "name": "rustc-version",
                            "program": crate::dependencies::RustC::resolve().expect("rustc"),
                            "args": ["--version"]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("execute passing verifier");
        assert!(
            pass.is_success(),
            "all passing gates should report tool success"
        );
        assert_hunt_mapping(&pass.content, "pass", "hunted", "complete");
        assert_hunt_metadata(&pass, "pass", "hunted", "complete");

        let fail = tool
            .execute(
                json!({
                    "profile": "auto",
                    "commands": [
                        {
                            "name": "rustc-bad-flag",
                            "program": crate::dependencies::RustC::resolve().expect("rustc"),
                            "args": ["--definitely-not-a-rustc-flag"]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("execute failing verifier");
        assert!(
            !fail.is_success(),
            "a failed verifier gate must be a failed tool result"
        );
        assert_rejected_evidence(&fail);
        assert_hunt_mapping(&fail.content, "fail", "escaped", "blocked");
        assert_hunt_metadata(&fail, "fail", "escaped", "blocked");
    }

    fn assert_hunt_mapping(content: &str, verifier: &str, hunt: &str, goal: &str) {
        let parsed: Value = serde_json::from_str(content).expect("verifier output json");
        assert_eq!(parsed["verifier_verdict"], verifier, "{content}");
        assert_eq!(parsed["hunt_verdict"], hunt, "{content}");
        assert_eq!(parsed["goal_status"], goal, "{content}");
    }

    fn assert_hunt_metadata(result: &ToolOutcome, verifier: &str, hunt: &str, goal: &str) {
        let metadata = result.metadata.as_ref().expect("hunt metadata");
        assert_eq!(metadata["verifier_verdict"], verifier, "{metadata}");
        assert_eq!(metadata["hunt_verdict"], hunt, "{metadata}");
        assert_eq!(metadata["goal_status"], goal, "{metadata}");
        assert_eq!(metadata["task_updates"]["hunt_verdict"], hunt, "{metadata}");
    }

    fn assert_produced_evidence(result: &ToolOutcome) {
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(result.artifacts.len(), 1);
        let artifact = &result.artifacts[0];
        assert_eq!(result.evidence.references, vec![artifact.id.clone()]);
        assert_eq!(artifact.status, ToolArtifactStatus::Available);
        assert!(
            artifact
                .sha256
                .as_deref()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        assert_eq!(artifact.media_type.as_deref(), Some("application/json"));
        assert!(artifact.byte_len.is_some_and(|len| len > 0));
        assert!(result.workspace_revision.is_some());
        result.validate().expect("typed evidence must be valid");
    }

    fn assert_rejected_evidence(result: &ToolOutcome) {
        assert_eq!(result.evidence.status, ToolEvidenceStatus::Rejected);
        assert!(result.evidence.references.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.workspace_revision.is_none());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn run_verifiers_background_starts_shell_jobs_and_returns_task_ids() {
        if !crate::dependencies::RustC::available() {
            return;
        }
        // The spawned `rustc` is usually the rustup shim, which resolves its
        // toolchain through $HOME. Hold the process-wide env mutex so tests
        // that temporarily swap HOME cannot break the child process.
        let _env_lock = crate::test_support::lock_test_env();
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let tool = RunVerifiersTool;
        let result = tool
            .execute(
                json!({
                    "profile": "auto",
                    "background": true,
                    "commands": [
                        {
                            "name": "rustc-version",
                            "program": crate::dependencies::RustC::resolve().expect("rustc"),
                            "args": ["--version"]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(
            result.is_success(),
            "starting every background gate should succeed"
        );
        let parsed: RunVerifiersBackgroundOutput =
            serde_json::from_str(&result.content).expect("background verifier output json");
        assert!(parsed.success, "result: {}", result.content);
        assert!(parsed.background);
        assert_eq!(parsed.started, 1);
        assert_eq!(parsed.failed_to_start, 0);
        assert!(parsed.summary.contains("Completion is tracked"));
        let task_id = parsed.jobs[0]
            .task_id
            .as_deref()
            .expect("background task id");
        let metadata = result.metadata.as_ref().expect("metadata");
        assert!(
            metadata
                .get("verifier_background")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "metadata should mark verifier background start"
        );
        assert_eq!(
            metadata
                .get("auto_notify_on_completion")
                .and_then(Value::as_bool),
            None
        );
        assert_eq!(
            metadata
                .get("auto_resume_on_completion")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            metadata.get("completion_surface").and_then(Value::as_str),
            Some("task_status")
        );
        assert_eq!(
            metadata.get("background_policy").and_then(Value::as_str),
            Some("nonblocking")
        );

        let output = wait_for_completed_shell(
            &mut ctx.shell_manager.lock().expect("shell manager"),
            task_id,
        );
        assert_eq!(
            output.status,
            ShellStatus::Completed,
            "stdout: {:?} stderr: {:?}",
            output.stdout,
            output.stderr
        );
        assert!(
            output.stdout.contains("rustc"),
            "stdout should include rustc version: {:?}",
            output.stdout
        );
    }

    #[test]
    fn run_verifiers_background_start_failure_is_a_failed_tool_result() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path());
        let missing_cwd = tmp.path().join("missing-cwd");
        let result = start_background_gates(
            &ctx,
            VerifierProfile::Auto,
            VerifierLevel::Quick,
            vec![gate(
                "cannot-start",
                "custom",
                &missing_cwd,
                "unused-program",
                ["--version"],
            )],
        )
        .expect("structured background verifier result");

        assert!(
            !result.is_success(),
            "a background gate start failure must not report tool success"
        );
        let parsed: RunVerifiersBackgroundOutput =
            serde_json::from_str(&result.content).expect("background verifier output json");
        assert!(!parsed.success);
        assert_eq!(parsed.started, 0);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.failed_to_start, 1);
        assert_eq!(parsed.jobs[0].status, "failed_to_start");
        assert!(parsed.jobs[0].error.is_some());
        assert_eq!(
            result
                .metadata
                .as_ref()
                .and_then(|metadata| metadata["verifier_background"].as_bool()),
            Some(true),
            "failure must retain background verifier metadata"
        );
    }
}
