//! Fleet executor — runs a fleet worker as a real `codewhale exec` subprocess.
//!
//! A fleet worker IS a headless `codewhale exec` run. There is no separate
//! "fleet worker" execution engine: the sub-agent runtime, full tool surface,
//! and recursion depth all come from the one `codewhale exec` runtime, so
//! fleet and sub-agents are one substrate (not two moving targets).
//!
//! This module is the bridge:
//! - [`build_worker_exec_command_with_profiles`] turns a `FleetTaskSpec` + `FleetExecConfig`
//!   into the `codewhale exec --output-format stream-json …` argv that a host
//!   adapter ([`super::host`]) launches locally or over SSH.
//! - [`map_exec_stream_line`] maps one stream-json line emitted by that worker
//!   into a [`FleetWorkerEventPayload`] for the durable ledger, so the ledger
//!   persists the worker's own event vocabulary instead of a simulated one.
//! - [`classify_worker_exit`] turns the process exit into a terminal event.
//!
//! The TUI/CLI/Runtime API observe the ledger's compact event stream — they
//! never render a child session, which is what keeps the orchestrator light at
//! high fanout.

use anyhow::Result;
use codewhale_config::FleetExecConfig;
use codewhale_protocol::fleet::{FleetHostSpec, FleetTaskSpec, FleetWorkerEventPayload};

use crate::exec_lifecycle_stream::{EXEC_STREAM_SCHEMA, EXEC_STREAM_SCHEMA_VERSION};

use super::host::{FleetHostAdapter, FleetWorkerCommand};
use super::profile::AgentProfile;
use super::worker_runtime::{
    fleet_task_prompt_with_profiles, fleet_worker_launch_reasoning_effort,
    fleet_worker_launch_route,
};

/// Build a worker command after resolving workspace Fleet profile input.
///
/// `--auto` is always passed because a headless worker cannot answer approval
/// prompts; it does not grant unrestricted external-path trust. Credentials
/// and provider selectors never enter argv. An optional official DeepSeek
/// model/reasoning pin is resolved from the validated worker profile.
pub fn build_worker_exec_command_with_profiles(
    codewhale_binary: &str,
    task_spec: &FleetTaskSpec,
    exec_config: &FleetExecConfig,
    model: Option<&str>,
    agent_profiles: &[AgentProfile],
) -> Result<FleetWorkerCommand> {
    super::worker_runtime::validate_task_agent_profiles(
        std::slice::from_ref(task_spec),
        agent_profiles,
    )?;
    let worker_model =
        fleet_worker_launch_route(task_spec, agent_profiles, model.unwrap_or_default());
    let worker_reasoning_effort = fleet_worker_launch_reasoning_effort(task_spec, agent_profiles);
    Ok(build_worker_exec_command_from_prompt(
        codewhale_binary,
        fleet_task_prompt_with_profiles(task_spec, agent_profiles)?,
        exec_config,
        Some(worker_model.as_str()),
        worker_reasoning_effort.as_deref(),
    ))
}

fn build_worker_exec_command_from_prompt(
    codewhale_binary: &str,
    task_prompt: String,
    exec_config: &FleetExecConfig,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> FleetWorkerCommand {
    let mut args: Vec<String> = vec![
        "exec".to_string(),
        "--auto".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ];

    if let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    // A thinking tier is profile metadata; omit it when the profile inherits.
    if let Some(reasoning_effort) = reasoning_effort.map(str::trim).filter(|e| !e.is_empty()) {
        args.push("--reasoning-effort".to_string());
        args.push(reasoning_effort.to_string());
    }

    if !exec_config.allowed_tools.is_empty() {
        args.push("--allowed-tools".to_string());
        args.push(exec_config.allowed_tools.join(","));
    }
    if !exec_config.disallowed_tools.is_empty() {
        args.push("--disallowed-tools".to_string());
        args.push(exec_config.disallowed_tools.join(","));
    }
    if exec_config.max_turns > 0 && exec_config.max_turns != u32::MAX {
        args.push("--max-turns".to_string());
        args.push(exec_config.max_turns.to_string());
    }
    if !exec_config.append_system_prompt.trim().is_empty() {
        args.push("--append-system-prompt".to_string());
        args.push(exec_config.append_system_prompt.clone());
    }

    // The composed task prompt is the final positional argument.
    args.push(task_prompt);

    FleetWorkerCommand::new(codewhale_binary.to_string(), args)
}

/// Map one `codewhale exec` stream-json line into a fleet ledger event.
///
/// Returns `None` for lines that don't correspond to a worker lifecycle
/// transition (e.g. `session_capture`, `metadata`). Only the exact current
/// exec-stream envelope is accepted; a worker speaking an older or unversioned
/// protocol cannot claim progress or completion.
pub fn map_exec_stream_line(line: &str) -> Option<FleetWorkerEventPayload> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("schema").and_then(serde_json::Value::as_str) != Some(EXEC_STREAM_SCHEMA)
        || value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(u64::from(EXEC_STREAM_SCHEMA_VERSION))
    {
        return None;
    }
    match value.get("type").and_then(serde_json::Value::as_str)? {
        "tool_use" => {
            let tool = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let call_id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Some(FleetWorkerEventPayload::RunningTool { tool, call_id })
        }
        // Streaming model output / tool results mean the worker is alive and
        // making progress; surface a coarse Running heartbeat.
        "content" | "tool_result" => Some(FleetWorkerEventPayload::Running),
        "done" => Some(FleetWorkerEventPayload::Completed {
            exit_code: Some(0),
            summary: None,
        }),
        "error" => {
            let reason = value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("worker reported an error")
                .to_string();
            Some(FleetWorkerEventPayload::Failed {
                reason,
                recoverable: false,
            })
        }
        _ => None,
    }
}

/// Classify a worker process exit into a terminal fleet event.
///
/// `stopped` means the operator stopped the worker (cancellation), which takes
/// precedence over the exit code.
pub fn classify_worker_exit(exit_code: Option<i32>, stopped: bool) -> FleetWorkerEventPayload {
    if stopped {
        return FleetWorkerEventPayload::Cancelled { cancelled_by: None };
    }
    match exit_code {
        Some(0) => FleetWorkerEventPayload::Completed {
            exit_code: Some(0),
            summary: None,
        },
        Some(code) => FleetWorkerEventPayload::Failed {
            reason: format!("worker exited with code {code}"),
            recoverable: true,
        },
        None => FleetWorkerEventPayload::Failed {
            reason: "worker exited without a status code".to_string(),
            recoverable: true,
        },
    }
}

/// Drives fleet workers as real `codewhale exec` subprocesses on the local
/// host, incrementally draining each worker's stream-json output into fleet
/// ledger events.
///
/// The caller (the `codewhale fleet run` loop / `FleetManager`) owns the
/// ledger; the executor owns the OS process boundary and the incremental log
/// parse. Because the worker is a separate process, its heavy runtime/tool
/// construction never touches the orchestrator — the parent only ingests a
/// compact event stream, which is what keeps it light at high fanout.
pub struct FleetExecutor {
    workspace: std::path::PathBuf,
    adapter: super::host::LocalProcessFleetHostAdapter,
    ssh_adapters: std::collections::BTreeMap<String, super::host::SshFleetHostAdapter>,
    streams: std::collections::BTreeMap<String, WorkerStream>,
}

struct WorkerStream {
    log_path: std::path::PathBuf,
    host: WorkerStreamHost,
    offset: u64,
    pending: String,
    terminal: bool,
}

enum WorkerStreamHost {
    Local,
    Ssh(String),
}

#[derive(Debug, Clone)]
pub struct FleetWorkerTerminalEvent {
    pub payload: FleetWorkerEventPayload,
    pub exit_code: Option<i32>,
}

impl FleetExecutor {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        let workspace = workspace.as_ref().to_path_buf();
        Self {
            adapter: super::host::LocalProcessFleetHostAdapter::new(&workspace),
            workspace,
            ssh_adapters: std::collections::BTreeMap::new(),
            streams: std::collections::BTreeMap::new(),
        }
    }

    /// Start a worker on the requested fleet host.
    pub fn start_worker_on_host(
        &mut self,
        worker_id: &str,
        host: &FleetHostSpec,
        command: FleetWorkerCommand,
        cwd: Option<std::path::PathBuf>,
    ) -> super::host::FleetHostResult<super::host::FleetWorkerHandle> {
        let mut request = super::host::FleetWorkerStartRequest::new(worker_id, command);
        request.cwd = cwd;
        let (handle, host) = match host {
            FleetHostSpec::Local => {
                let handle = self.adapter.start_worker(request)?;
                (handle, WorkerStreamHost::Local)
            }
            FleetHostSpec::Ssh { .. } => {
                let config = super::host::SshFleetHostConfig::from_host_spec(host)?;
                let key = worker_id.to_string();
                let adapter = self.ssh_adapters.entry(key.clone()).or_insert(
                    super::host::SshFleetHostAdapter::new(&self.workspace, config)?,
                );
                let handle = adapter.start_worker(request)?;
                (handle, WorkerStreamHost::Ssh(key))
            }
            FleetHostSpec::Docker { image, .. } => {
                return Err(super::host::FleetHostError {
                    kind: super::host::FleetHostErrorKind::Configuration,
                    message: format!("docker fleet workers are not wired yet (image {image})"),
                });
            }
        };
        self.streams.insert(
            worker_id.to_string(),
            WorkerStream {
                log_path: handle.log_path.clone(),
                host,
                offset: 0,
                pending: String::new(),
                terminal: false,
            },
        );
        Ok(handle)
    }

    pub fn is_tracking(&self, worker_id: &str) -> bool {
        self.streams.contains_key(worker_id)
    }

    pub fn worker_ids(&self) -> Vec<String> {
        self.streams.keys().cloned().collect()
    }

    /// Stop tracking a terminal worker so the scheduler can reuse the same
    /// logical worker id for the next queued task.
    pub fn forget_worker(&mut self, worker_id: &str) {
        let Some(stream) = self.streams.remove(worker_id) else {
            return;
        };
        match stream.host {
            WorkerStreamHost::Local => {
                let _ = self.adapter.cleanup_worker(worker_id);
            }
            WorkerStreamHost::Ssh(key) => {
                if let Some(adapter) = self.ssh_adapters.get_mut(&key) {
                    let _ = adapter.cleanup_worker(worker_id);
                }
                self.ssh_adapters.remove(&key);
            }
        }
    }

    /// Read any newly-written stream-json lines for a worker and map them to
    /// fleet ledger events. Safe to call repeatedly; only new bytes are parsed,
    /// and a trailing partial line is buffered until its newline arrives.
    pub fn drain_events(&mut self, worker_id: &str) -> Vec<FleetWorkerEventPayload> {
        let Some(stream) = self.streams.get_mut(worker_id) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let Ok(mut file) = std::fs::File::open(&stream.log_path) else {
            return events;
        };
        use std::io::{Read, Seek, SeekFrom};
        if file.seek(SeekFrom::Start(stream.offset)).is_err() {
            return events;
        }
        let mut buf = Vec::new();
        if let Ok(read) = file.read_to_end(&mut buf) {
            stream.offset += read as u64;
            stream.pending.push_str(&String::from_utf8_lossy(&buf));
            while let Some(idx) = stream.pending.find('\n') {
                let line: String = stream.pending.drain(..=idx).collect();
                if let Some(event) = map_exec_stream_line(line.trim_end()) {
                    events.push(event);
                }
            }
        }
        events
    }

    /// Poll the worker process and include the raw exit code for receipt
    /// verification.
    pub fn poll_terminal_with_status(
        &mut self,
        worker_id: &str,
    ) -> Option<FleetWorkerTerminalEvent> {
        if self.streams.get(worker_id).is_none_or(|s| s.terminal) {
            return None;
        }
        let status = match self.streams.get(worker_id).map(|s| &s.host)? {
            WorkerStreamHost::Local => self.adapter.read_status(worker_id).ok()?,
            WorkerStreamHost::Ssh(key) => self
                .ssh_adapters
                .get_mut(key)
                .and_then(|adapter| adapter.read_status(worker_id).ok())?,
        };
        let terminal = match status.state {
            super::host::FleetHostWorkerState::Running
            | super::host::FleetHostWorkerState::Unknown => return None,
            super::host::FleetHostWorkerState::Stopped => {
                classify_worker_exit(status.exit_code, true)
            }
            super::host::FleetHostWorkerState::Exited
            | super::host::FleetHostWorkerState::Failed => {
                classify_worker_exit(status.exit_code, false)
            }
        };
        if let Some(stream) = self.streams.get_mut(worker_id) {
            stream.terminal = true;
        }
        Some(FleetWorkerTerminalEvent {
            payload: terminal,
            exit_code: status.exit_code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_config::{
        FleetDelegationHints, FleetLoadout, FleetProfile, FleetProfilePermissions, FleetRole,
        FleetSlot,
    };
    use codewhale_protocol::fleet::{FleetTaskSpec, FleetTaskWorkerProfile};
    use std::collections::BTreeMap;

    fn stream_line(mut value: serde_json::Value) -> String {
        let object = value.as_object_mut().expect("stream fixture is an object");
        object.insert(
            "schema".to_owned(),
            serde_json::Value::String(EXEC_STREAM_SCHEMA.to_owned()),
        );
        object.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(EXEC_STREAM_SCHEMA_VERSION),
        );
        serde_json::to_string(&value).expect("serialize stream fixture")
    }

    fn task(instructions: &str) -> FleetTaskSpec {
        FleetTaskSpec {
            id: "t1".to_string(),
            name: "Smoke".to_string(),
            description: None,
            objective: Some("prove it runs".to_string()),
            instructions: instructions.to_string(),
            worker: Some(FleetTaskWorkerProfile {
                agent_profile: None,
                role: Some("reviewer".to_string()),
                loadout: None,
                model_class: None,
                model: None,
                tool_profile: Some("read-only".to_string()),
                tools: vec![],
                capabilities: vec![],
            }),
            workspace: None,
            input_files: vec![],
            context: vec![],
            budget: None,
            tags: vec![],
            expected_artifacts: vec![],
            scorer: None,
            retry_policy: None,
            alert_policy: None,
            timeout_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn agent_profile(id: &str, role: &str, instructions: &str) -> AgentProfile {
        AgentProfile {
            id: id.to_string(),
            display_name: Some(format!("{role} profile")),
            description: Some(format!("{role} description")),
            profile: FleetProfile {
                slot: FleetSlot::from_name(role),
                role: FleetRole {
                    name: role.to_string(),
                    description: None,
                    instructions: Some(instructions.to_string()),
                },
                loadout: FleetLoadout::Inherit,
                model: None,
                reasoning_effort: None,
                permissions: FleetProfilePermissions::default(),
                delegation: FleetDelegationHints::default(),
            },
            source: std::path::PathBuf::from(format!("{id}.toml")),
            origin: crate::fleet::roster::ProfileOrigin::Workspace,
        }
    }

    #[test]
    fn worker_command_is_a_headless_codewhale_exec_run() {
        let exec = FleetExecConfig::default();
        let cmd = build_worker_exec_command_with_profiles(
            "codewhale",
            &task("read the file"),
            &exec,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(cmd.program, "codewhale");
        assert_eq!(cmd.args[0], "exec");
        assert!(cmd.args.contains(&"--auto".to_string()));
        assert!(!cmd.args.contains(&"--yolo".to_string()));
        // stream-json so the executor can ingest the worker's event stream.
        let joined = cmd.args.join(" ");
        assert!(joined.contains("--output-format stream-json"));
        // The task instructions ride in the positional prompt (last arg).
        assert!(cmd.args.last().unwrap().contains("read the file"));
    }

    #[test]
    fn worker_command_threads_exec_hardening_flags() {
        let exec = FleetExecConfig {
            allowed_tools: vec!["read_file".to_string(), "grep_files".to_string()],
            disallowed_tools: vec!["exec_shell".to_string()],
            max_turns: 40,
            append_system_prompt: "never push to main".to_string(),
            ..FleetExecConfig::default()
        };
        let cmd = build_worker_exec_command_with_profiles(
            "codewhale",
            &task("audit"),
            &exec,
            Some("deepseek-v4-flash"),
            &[],
        )
        .unwrap();
        let joined = cmd.args.join(" ");
        assert!(joined.contains("--model deepseek-v4-flash"));
        assert!(joined.contains("--allowed-tools read_file,grep_files"));
        assert!(joined.contains("--disallowed-tools exec_shell"));
        assert!(joined.contains("--max-turns 40"));
        assert!(cmd.args.iter().any(|a| a == "never push to main"));
    }

    #[test]
    fn worker_command_threads_agent_profile_prompt() {
        let mut task = task("audit");
        task.worker.as_mut().unwrap().agent_profile = Some("reviewer".to_string());
        let cmd = build_worker_exec_command_with_profiles(
            "codewhale",
            &task,
            &FleetExecConfig::default(),
            None,
            &[agent_profile(
                "reviewer",
                "reviewer",
                "Focus on defects, regressions, and missing tests.",
            )],
        )
        .unwrap();
        let prompt = cmd.args.last().unwrap();

        assert!(prompt.contains("Fleet profile: reviewer"));
        assert!(prompt.contains("Focus on defects, regressions, and missing tests."));
    }

    /// A worker launches on the one DeepSeek route without provider state.
    #[test]
    fn worker_command_without_profile_provider_omits_provider_and_keeps_run_model() {
        let cmd = build_worker_exec_command_with_profiles(
            "codewhale",
            &task("read"),
            &FleetExecConfig::default(),
            Some("deepseek-v4-pro"),
            &[],
        )
        .unwrap();

        assert!(
            !cmd.args.iter().any(|a| a == "--provider"),
            "profile-less worker must not carry --provider: {:?}",
            cmd.args
        );
        assert!(
            !cmd.args.iter().any(|a| a == "--reasoning-effort"),
            "profile-less worker must not carry --reasoning-effort: {:?}",
            cmd.args
        );
        let model_idx = cmd
            .args
            .iter()
            .position(|a| a == "--model")
            .expect("--model must be present");
        assert_eq!(
            cmd.args.get(model_idx + 1).map(String::as_str),
            Some("deepseek-v4-pro"),
            "{:?}",
            cmd.args
        );
    }

    #[test]
    fn unbounded_max_turns_is_not_passed() {
        let exec = FleetExecConfig::default(); // max_turns == u32::MAX
        let cmd =
            build_worker_exec_command_with_profiles("codewhale", &task("x"), &exec, None, &[])
                .unwrap();
        assert!(!cmd.args.join(" ").contains("--max-turns"));
    }

    #[test]
    fn stream_line_maps_tool_use_to_running_tool() {
        let line = stream_line(serde_json::json!({
            "type": "tool_use",
            "name": "read_file",
            "id": "call-7",
            "input": {}
        }));
        match map_exec_stream_line(&line) {
            Some(FleetWorkerEventPayload::RunningTool { tool, call_id }) => {
                assert_eq!(tool, "read_file");
                assert_eq!(call_id.as_deref(), Some("call-7"));
            }
            other => panic!("expected RunningTool, got {other:?}"),
        }
    }

    #[test]
    fn stream_line_maps_done_and_error() {
        let done = stream_line(serde_json::json!({"type": "done"}));
        assert!(matches!(
            map_exec_stream_line(&done),
            Some(FleetWorkerEventPayload::Completed { .. })
        ));
        let error = stream_line(serde_json::json!({"type": "error", "error": "boom"}));
        match map_exec_stream_line(&error) {
            Some(FleetWorkerEventPayload::Failed { reason, .. }) => assert_eq!(reason, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn stream_line_rejects_old_or_missing_envelopes_and_ignores_noise() {
        let noise = stream_line(serde_json::json!({
            "type": "session_capture",
            "content": "x"
        }));
        assert!(map_exec_stream_line(&noise).is_none());
        assert!(map_exec_stream_line(r#"{"type":"done"}"#).is_none());
        assert!(
            map_exec_stream_line(
                r#"{"schema":"codewhale.exec-stream","schema_version":1,"type":"done"}"#
            )
            .is_none()
        );
        assert!(map_exec_stream_line("not json").is_none());
        assert!(map_exec_stream_line("").is_none());
    }

    #[test]
    fn exit_classification() {
        assert!(matches!(
            classify_worker_exit(Some(0), false),
            FleetWorkerEventPayload::Completed { .. }
        ));
        assert!(matches!(
            classify_worker_exit(Some(1), false),
            FleetWorkerEventPayload::Failed {
                recoverable: true,
                ..
            }
        ));
        assert!(matches!(
            classify_worker_exit(Some(0), true),
            FleetWorkerEventPayload::Cancelled { .. }
        ));
    }

    /// End-to-end: run a REAL subprocess that emits stream-json (standing in for
    /// `codewhale exec`), and prove the executor drains its events and terminal
    /// exit through the real host adapter — no codewhale binary needed. This is
    /// the verifiable proof that a fleet worker is an out-of-process exec run.
    #[cfg(unix)]
    #[test]
    fn executor_runs_real_process_and_drains_stream_json_into_ledger_events() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut exec = FleetExecutor::new(tmp.path());
        let tool = stream_line(serde_json::json!({
            "type": "tool_use",
            "name": "read_file",
            "id": "c1",
            "input": {}
        }));
        let done = stream_line(serde_json::json!({"type": "done"}));
        let script = format!("printf '%s\\n' '{tool}' '{done}'");
        let command = FleetWorkerCommand::new("sh", vec!["-c".to_string(), script.to_string()]);
        exec.start_worker_on_host("w1", &FleetHostSpec::Local, command, None)
            .unwrap();

        let mut events = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            events.extend(exec.drain_events("w1"));
            if let Some(term) = exec.poll_terminal_with_status("w1") {
                events.extend(exec.drain_events("w1")); // final flush after exit
                events.push(term.payload);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not terminate; events so far: {events:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(
            events.iter().any(|e| matches!(
                e,
                FleetWorkerEventPayload::RunningTool { tool, .. } if tool == "read_file"
            )),
            "expected a RunningTool(read_file) event, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, FleetWorkerEventPayload::Completed { .. })),
            "expected a terminal Completed event, got {events:?}"
        );
        assert!(exec.poll_terminal_with_status("w1").is_none());
        exec.forget_worker("w1");
        assert!(!exec.is_tracking("w1"));
    }

    /// Dogfood smoke (#3166): several concurrent exec-style workers with one
    /// injected failure. Proves the executor drives a small fleet to terminal
    /// outcomes and that a failing worker is classified distinctly from the
    /// passing ones — all without the codewhale binary.
    #[cfg(unix)]
    #[test]
    fn executor_drives_concurrent_workers_with_injected_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut exec = FleetExecutor::new(tmp.path());

        // Three healthy workers emit a tool_use + done; one injected-failure
        // worker emits an error event and exits non-zero.
        let ok_tool = stream_line(serde_json::json!({
            "type": "tool_use",
            "name": "grep_files",
            "id": "c",
            "input": {}
        }));
        let done = stream_line(serde_json::json!({"type": "done"}));
        let ok = format!("printf '%s\\n' '{ok_tool}' '{done}'");
        let error = stream_line(serde_json::json!({
            "type": "error",
            "error": "injected failure"
        }));
        let bad = format!("printf '%s\\n' '{error}'; exit 7");
        for id in ["w1", "w2", "w3"] {
            exec.start_worker_on_host(
                id,
                &FleetHostSpec::Local,
                FleetWorkerCommand::new("sh", vec!["-c".to_string(), ok.to_string()]),
                None,
            )
            .unwrap();
        }
        exec.start_worker_on_host(
            "w-fail",
            &FleetHostSpec::Local,
            FleetWorkerCommand::new("sh", vec!["-c".to_string(), bad.to_string()]),
            None,
        )
        .unwrap();

        let ids = ["w1", "w2", "w3", "w-fail"];
        let mut terminals: std::collections::BTreeMap<&str, FleetWorkerEventPayload> =
            std::collections::BTreeMap::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while terminals.len() < ids.len() {
            for id in ids {
                let _ = exec.drain_events(id);
                if let Some(term) = exec.poll_terminal_with_status(id) {
                    terminals.insert(id, term.payload);
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "not all workers terminated: {terminals:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        for id in ids {
            assert!(exec.poll_terminal_with_status(id).is_none());
            exec.forget_worker(id);
            assert!(!exec.is_tracking(id));
        }
        for id in ["w1", "w2", "w3"] {
            assert!(
                matches!(terminals[id], FleetWorkerEventPayload::Completed { .. }),
                "{id} should pass, got {:?}",
                terminals[id]
            );
        }
        assert!(
            matches!(terminals["w-fail"], FleetWorkerEventPayload::Failed { .. }),
            "injected-failure worker should fail, got {:?}",
            terminals["w-fail"]
        );
    }
}
