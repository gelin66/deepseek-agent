//! External-process recovery contract for the canonical app-server.
//!
//! The ignored child test is launched as a separate process and runs the real
//! `AgentApplication` plus canonical stdio transport. The parent owns the
//! loopback DeepSeek fixture, stops/kills the whole child process group, and
//! audits the reopened SQLite event log. No production endpoint, Runtime hook,
//! Store, Agent loop, or compatibility path exists only for this test.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::num::NonZeroU32;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use codewhale_app::{
    AgentApplication, DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
    TransportRetryPolicy,
};
use codewhale_app_server::run_stdio;
use codewhale_protocol::agent_runtime::{
    ReasoningEffort, RunId, RunLimits, StoredRuntimeEvent, TerminalState, ToolPolicy,
};
use codewhale_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunApiErrorCode, RunCommand, RunCommandEnvelope, RunCommandResponse,
    RunCommandResult, RunProductControls, RunView, StartRunCommand,
};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::sync::{Semaphore, oneshot};

const CHILD_MODE: &str = "CODEWHALE_M4B_APP_SERVER_CHILD";
const CHILD_DB: &str = "CODEWHALE_M4B_APP_SERVER_DB";
const CHILD_ENDPOINT: &str = "CODEWHALE_M4B_APP_SERVER_ENDPOINT";
const CHILD_WITH_KEY: &str = "CODEWHALE_M4B_APP_SERVER_WITH_KEY";
const CHILD_HOME: &str = "CODEWHALE_M4B_APP_SERVER_HOME";
const COMPOSITION_REVISION: &str = "m4b-app-server-process-contract-v1";
const CHILD_TEST_NAME: &str = "app_server_process_child";

#[derive(Clone)]
struct FixtureState {
    requests: Arc<AtomicUsize>,
    observed: Arc<(Mutex<usize>, Condvar)>,
    release: Arc<Semaphore>,
}

struct DeepSeekFixture {
    root: String,
    requests: Arc<AtomicUsize>,
    observed: Arc<(Mutex<usize>, Condvar)>,
    release: Arc<Semaphore>,
    shutdown: Option<oneshot::Sender<()>>,
    server: Option<thread::JoinHandle<()>>,
}

impl DeepSeekFixture {
    fn start() -> Self {
        async fn complete(State(state): State<FixtureState>) -> Json<Value> {
            let count = state.requests.fetch_add(1, Ordering::AcqRel) + 1;
            let (lock, changed) = &*state.observed;
            *lock.lock().expect("fixture observation lock") = count;
            changed.notify_all();
            let permit = state.release.acquire().await.expect("fixture remains open");
            permit.forget();
            Json(json!({
                "id": format!("fixture-response-{count}"),
                "model": "deepseek-v4-flash",
                "choices": [{
                    "finish_reason": "stop",
                    "message": {"role": "assistant", "content": "外部进程恢复完成"}
                }],
                "usage": {
                    "prompt_tokens": 11,
                    "completion_tokens": 3,
                    "total_tokens": 14
                }
            }))
        }

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback DeepSeek fixture");
        listener
            .set_nonblocking(true)
            .expect("make fixture listener nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new((Mutex::new(0), Condvar::new()));
        let release = Arc::new(Semaphore::new(0));
        let state = FixtureState {
            requests: requests.clone(),
            observed: observed.clone(),
            release: release.clone(),
        };
        let (shutdown, shutdown_rx) = oneshot::channel();
        let (ready, ready_rx) = mpsc::sync_channel(0);
        let server = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build fixture runtime");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("adopt fixture listener");
                let router = Router::new()
                    .route("/v1/chat/completions", post(complete))
                    .with_state(state);
                ready.send(()).expect("announce fixture readiness");
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    })
                    .await
                    .expect("serve loopback DeepSeek fixture");
            });
        });
        ready_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("fixture becomes ready");
        Self {
            root: format!("http://{address}/v1"),
            requests,
            observed,
            release,
            shutdown: Some(shutdown),
            server: Some(server),
        }
    }

    fn wait_requests(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (lock, changed) = &*self.observed;
        let mut count = lock.lock().expect("fixture observation lock");
        while *count < expected {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fixture did not receive request {expected}"
            );
            let (next, timed) = changed
                .wait_timeout(count, remaining)
                .expect("wait for fixture request");
            count = next;
            assert!(
                !timed.timed_out() || *count >= expected,
                "fixture did not receive request {expected}"
            );
        }
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    fn release_one(&self) {
        self.release.add_permits(1);
    }
}

impl Drop for DeepSeekFixture {
    fn drop(&mut self) {
        self.release.add_permits(32);
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(server) = self.server.take() {
            server.join().expect("join fixture server");
        }
    }
}

struct StdioProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: mpsc::Receiver<String>,
    stdout_thread: Option<thread::JoinHandle<()>>,
    stderr: Arc<Mutex<Vec<String>>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
    first_response: bool,
}

impl StdioProcess {
    fn spawn(db: &Path, endpoint: &str, with_key: bool, home: &Path) -> Self {
        let mut command = Command::new(std::env::current_exe().expect("locate integration test"));
        command
            .arg("--ignored")
            .arg("--exact")
            .arg(CHILD_TEST_NAME)
            .arg("--test-threads=1")
            .arg("--nocapture")
            .env_clear()
            .env(CHILD_MODE, "1")
            .env(CHILD_DB, db)
            .env(CHILD_ENDPOINT, endpoint)
            .env(CHILD_WITH_KEY, if with_key { "1" } else { "0" })
            .env(CHILD_HOME, home)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("NO_COLOR", "1")
            .env("RUST_BACKTRACE", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().expect("spawn external app-server process");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr_pipe = child.stderr.take().expect("child stderr");
        let (sender, receiver) = mpsc::channel();
        let stdout_thread = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let captured_stderr = stderr.clone();
        let stderr_thread = thread::spawn(move || {
            for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
                captured_stderr
                    .lock()
                    .expect("stderr capture lock")
                    .push(line);
            }
        });
        Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: receiver,
            stdout_thread: Some(stdout_thread),
            stderr,
            stderr_thread: Some(stderr_thread),
            first_response: true,
        }
    }

    fn pid(&self) -> u32 {
        self.child.as_ref().expect("live child").id()
    }

    fn send(&mut self, envelope: &RunCommandEnvelope) {
        let stdin = self.stdin.as_mut().expect("open child stdin");
        serde_json::to_writer(&mut *stdin, envelope).expect("serialize stdio command");
        stdin.write_all(b"\n").expect("terminate stdio command");
        stdin.flush().expect("flush stdio command");
    }

    fn request(&mut self, envelope: &RunCommandEnvelope) -> RunCommandResponse {
        self.send(envelope);
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "app-server response timeout; stderr={:?}",
                self.stderr.lock().expect("stderr capture lock")
            );
            let line = self.stdout.recv_timeout(remaining).unwrap_or_else(|error| {
                panic!(
                    "app-server response {} ended: {error}; stderr={:?}",
                    envelope.request_id,
                    self.stderr.lock().expect("stderr capture lock")
                )
            });
            let candidate = if self.first_response {
                line.find("{\"schema_version\":")
                    .map_or(line.as_str(), |start| &line[start..])
            } else {
                line.as_str()
            };
            if let Ok(response) = serde_json::from_str::<RunCommandResponse>(candidate) {
                self.first_response = false;
                assert_eq!(response.schema_version, RUN_API_SCHEMA_VERSION);
                return response;
            }
        }
    }

    fn signal_group(&self, signal: libc::c_int) {
        let process_group = -(self.pid() as libc::pid_t);
        // SAFETY: the child is created as the leader of a fresh process group.
        let result = unsafe { libc::kill(process_group, signal) };
        assert_eq!(
            result,
            0,
            "signal child process group: {}",
            std::io::Error::last_os_error()
        );
    }

    fn kill_group(&mut self) -> i32 {
        self.signal_group(libc::SIGKILL);
        let status = self
            .child
            .as_mut()
            .expect("live child")
            .wait()
            .expect("wait for killed app-server");
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        status.signal().expect("signal exit")
    }

    fn shutdown(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            let child = self.child.as_mut().expect("live child");
            if let Some(status) = child.try_wait().expect("poll app-server exit") {
                break status;
            }
            if Instant::now() >= deadline {
                // SAFETY: the child is the leader of this test's private process group.
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                break child.wait().expect("reap timed-out app-server");
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(status.success(), "app-server child failed: {status}");
        self.join_readers();
        self.child.take();
    }

    fn join_readers(&mut self) {
        if let Some(reader) = self.stdout_thread.take() {
            reader.join().expect("join stdout reader");
        }
        if let Some(reader) = self.stderr_thread.take() {
            reader.join().expect("join stderr reader");
        }
    }
}

impl Drop for StdioProcess {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(child) = self.child.as_mut()
            && child.try_wait().ok().flatten().is_none()
        {
            // SAFETY: best-effort cleanup of this test's private process group.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.wait();
        }
        // Do not join reader threads from an unwinding Drop. A descendant
        // accidentally retaining a pipe must never delay process-group reap.
        self.stdout_thread.take();
        self.stderr_thread.take();
    }
}

#[derive(Debug, Clone)]
struct DbState {
    run_id: String,
    last_sequence: u64,
    terminal: bool,
    execution_epoch: u64,
    lease_owner_id: Option<String>,
    lease_owner_pid: Option<u32>,
    pending_model_in_flight: bool,
    event_count: usize,
    event_digest: String,
    event_kinds: Vec<String>,
    sequences_contiguous: bool,
    unique_event_id_count: usize,
    event_ids_unique: bool,
    event_run_ids_match: bool,
    terminal_outcome_run_ids_match: bool,
    terminal_count: usize,
    terminal_last: bool,
    model_request_count: usize,
    tool_side_effect_count: usize,
    tool_side_effect_digest: String,
}

type EventRow = (i64, String, u32, i64, bool, String);

fn length_prefixed(digest: &mut Sha256, value: impl ToString) {
    let bytes = value.to_string().into_bytes();
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn finish_sha256(digest: Sha256) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.finalize();
    let mut encoded = String::with_capacity(7 + bytes.len() * 2);
    encoded.push_str("sha256:");
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn event_digest(rows: &[EventRow]) -> String {
    let mut digest = Sha256::new();
    for (sequence, event_id, schema, occurred_at, terminal, event_json) in rows {
        length_prefixed(&mut digest, sequence);
        length_prefixed(&mut digest, event_id);
        length_prefixed(&mut digest, schema);
        length_prefixed(&mut digest, occurred_at);
        length_prefixed(&mut digest, i32::from(*terminal));
        length_prefixed(&mut digest, event_json);
    }
    finish_sha256(digest)
}

fn side_effect_digest(rows: &[(i64, String, String, String, String)]) -> String {
    let mut digest = Sha256::new();
    for row in rows {
        length_prefixed(&mut digest, row.0);
        length_prefixed(&mut digest, &row.1);
        length_prefixed(&mut digest, &row.2);
        length_prefixed(&mut digest, &row.3);
        length_prefixed(&mut digest, &row.4);
    }
    finish_sha256(digest)
}

fn read_db_state(db: &Path) -> rusqlite::Result<Option<DbState>> {
    if !db.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut runs = connection.prepare(
        "SELECT run_id, last_sequence, terminal, execution_epoch, lease_owner_id, \
         lease_owner_pid, pending_model_in_flight \
         FROM agent_runs ORDER BY created_at_unix_ms, run_id LIMIT 2",
    )?;
    let run_rows = runs
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<u32>>(5)?,
                row.get::<_, bool>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if run_rows.len() != 1 {
        return Ok(None);
    }
    let run = &run_rows[0];
    let mut events = connection.prepare(
        "SELECT sequence, event_id, schema_version, occurred_at_unix_ms, terminal, event_json \
         FROM agent_run_events WHERE run_id = ?1 ORDER BY sequence",
    )?;
    let rows = events
        .query_map(params![&run.0], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<EventRow>>>()?;
    let mut kinds = Vec::with_capacity(rows.len());
    let mut event_run_ids_match = true;
    let mut terminal_outcome_run_ids_match = true;
    let mut side_effects = Vec::new();
    for row in &rows {
        let event: Value = serde_json::from_str(&row.5).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        event_run_ids_match &= event.get("run_id").and_then(Value::as_str) == Some(&run.0);
        let body = event.get("event").unwrap_or(&Value::Null);
        let kind = body
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("invalid")
            .to_owned();
        if kind == "terminal" {
            terminal_outcome_run_ids_match &=
                body.pointer("/outcome/run_id").and_then(Value::as_str) == Some(&run.0);
        }
        if kind == "tool_outcome_committed"
            && body.pointer("/outcome/side_effect").and_then(Value::as_str) == Some("applied")
        {
            side_effects.push((
                row.0,
                body.get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                body.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                "applied".to_owned(),
                body.pointer("/outcome/workspace_revision")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        kinds.push(kind);
    }
    let sequences = rows.iter().map(|row| row.0).collect::<Vec<_>>();
    let event_ids = rows.iter().map(|row| &row.1).collect::<Vec<_>>();
    let terminal_count = rows.iter().filter(|row| row.4).count();
    let unique_event_id_count = event_ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .len();
    Ok(Some(DbState {
        run_id: run.0.clone(),
        last_sequence: run.1.try_into().expect("non-negative sequence"),
        terminal: run.2,
        execution_epoch: run.3.try_into().expect("non-negative epoch"),
        lease_owner_id: run.4.clone(),
        lease_owner_pid: run.5,
        pending_model_in_flight: run.6,
        event_count: rows.len(),
        event_digest: event_digest(&rows),
        event_kinds: kinds.clone(),
        sequences_contiguous: sequences == (1..=rows.len() as i64).collect::<Vec<_>>(),
        unique_event_id_count,
        event_ids_unique: unique_event_id_count == rows.len(),
        event_run_ids_match,
        terminal_outcome_run_ids_match,
        terminal_count,
        terminal_last: terminal_count == 1 && rows.last().is_some_and(|row| row.4),
        model_request_count: kinds
            .iter()
            .filter(|kind| kind.as_str() == "model_request_in_flight")
            .count(),
        tool_side_effect_count: side_effects.len(),
        tool_side_effect_digest: side_effect_digest(&side_effects),
    }))
}

fn event_prefix_digest(db: &Path, run_id: &str, count: usize) -> rusqlite::Result<Option<String>> {
    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        "SELECT sequence, event_id, schema_version, occurred_at_unix_ms, terminal, event_json \
         FROM agent_run_events WHERE run_id = ?1 ORDER BY sequence LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![run_id, count as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<EventRow>>>()?;
    Ok((rows.len() == count).then(|| event_digest(&rows)))
}

fn wait_db_state(db: &Path, timeout: Duration, predicate: impl Fn(&DbState) -> bool) -> DbState {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(state)) = read_db_state(db)
            && predicate(&state)
        {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for durable run state"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn safe_pre_model_io(state: &DbState, expected_pid: u32) -> bool {
    !state.terminal
        && state.terminal_count == 0
        && !state.pending_model_in_flight
        && state.lease_owner_id.is_some()
        && state.lease_owner_pid == Some(expected_pid)
        && state.event_count >= 1
        && state.sequences_contiguous
        && state.event_ids_unique
        && state.event_run_ids_match
        && state
            .event_kinds
            .iter()
            .all(|kind| matches!(kind.as_str(), "run_created" | "model_request_prepared"))
        && state.event_kinds.iter().any(|kind| kind == "run_created")
}

fn wait_for_stop(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut status = 0;
        // SAFETY: waitpid observes only the spawned child process.
        let result = unsafe {
            libc::waitpid(
                pid as libc::pid_t,
                &mut status,
                libc::WNOHANG | libc::WUNTRACED,
            )
        };
        if result == pid as libc::pid_t && libc::WIFSTOPPED(status) {
            return;
        }
        assert!(
            result >= 0,
            "waitpid failed: {}",
            std::io::Error::last_os_error()
        );
        assert!(
            Instant::now() < deadline,
            "child did not enter SIGSTOP state"
        );
        thread::sleep(Duration::from_micros(50));
    }
}

fn capture_safe_state_and_sigkill(process: &mut StdioProcess, db: &Path) -> (DbState, i32) {
    let pid = process.pid();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        process.signal_group(libc::SIGSTOP);
        wait_for_stop(pid);
        let observed = read_db_state(db).ok().flatten();
        if let Some(state) = observed.as_ref()
            && safe_pre_model_io(state, pid)
        {
            let captured = state.clone();
            let signal = process.kill_group();
            assert_eq!(signal, libc::SIGKILL);
            assert!(!process_alive(pid), "killed lease owner PID remains live");
            return (captured, signal);
        }
        if let Some(state) = observed
            && (state.pending_model_in_flight
                || state.event_kinds.iter().any(|kind| {
                    matches!(
                        kind.as_str(),
                        "model_request_in_flight" | "tool_execution_started"
                    )
                })
                || state.terminal)
        {
            process.kill_group();
            panic!(
                "missed durable pre-model-I/O state: {:?}",
                state.event_kinds
            );
        }
        assert!(Instant::now() < deadline, "pre-model-I/O capture timed out");
        process.signal_group(libc::SIGCONT);
        thread::sleep(Duration::from_micros(50));
    }
}

fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 only probes process existence.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn envelope(request_id: &str, command: RunCommand) -> RunCommandEnvelope {
    RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        command,
    }
}

fn start_command(workspace: &Path) -> StartRunCommand {
    StartRunCommand {
        input: "完成外部进程恢复契约".to_owned(),
        workspace: workspace
            .canonicalize()
            .expect("canonical fixture workspace")
            .display()
            .to_string(),
        model: Some("deepseek-v4-flash".to_owned()),
        reasoning_effort: ReasoningEffort::High,
        max_output_tokens: Some(4_096),
        max_api_requests: NonZeroU32::new(4),
        streaming: false,
        tool_policy: ToolPolicy {
            enabled: false,
            ..ToolPolicy::default()
        },
        limits: RunLimits {
            max_turns: 4,
            max_model_requests: 4,
            max_model_retries: 0,
            max_tool_calls: 0,
            max_depth: 0,
            max_concurrent_children: 0,
            model_event_idle_ms: Some(5_000),
            wall_time_ms: Some(15_000),
        },
        controls: RunProductControls::default(),
    }
}

fn run_from_response(response: RunCommandResponse) -> RunView {
    match response.result {
        RunCommandResult::Run { run } => *run,
        other => panic!("expected run response, got {other:?}"),
    }
}

fn events_from_response(response: RunCommandResponse) -> Vec<StoredRuntimeEvent> {
    match response.result {
        RunCommandResult::Events { events, .. } => events,
        other => panic!("expected event response, got {other:?}"),
    }
}

fn tree_digest(root: &Path) -> String {
    fn collect(root: &Path, path: &Path, entries: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(path).expect("read fixture workspace") {
            let entry = entry.expect("workspace entry");
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, entries);
            } else {
                entries.push(
                    path.strip_prefix(root)
                        .expect("relative workspace path")
                        .to_owned(),
                );
            }
        }
    }
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort();
    let mut digest = Sha256::new();
    for relative in entries {
        length_prefixed(&mut digest, relative.display());
        let bytes = fs::read(root.join(relative)).expect("read workspace file");
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    finish_sha256(digest)
}

fn fixture_paths(prefix: &str) -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("temporary app-server process fixture");
    let workspace = temp.path().join("workspace");
    let home = temp.path().join("home");
    fs::create_dir_all(&workspace).expect("create fixture workspace");
    fs::create_dir_all(&home).expect("create child home");
    fs::write(
        workspace.join("README.md"),
        "canonical app-server crash fixture\n",
    )
    .expect("write fixture file");
    let db = temp.path().join("state.db");
    (temp, workspace, home, db)
}

/// Launched only by the parent tests with an exact ignored-test filter.
#[test]
#[ignore = "external app-server child"]
fn app_server_process_child() {
    if std::env::var(CHILD_MODE).as_deref() != Ok("1") {
        return;
    }
    let db = PathBuf::from(std::env::var_os(CHILD_DB).expect("child DB path"));
    let endpoint = std::env::var(CHILD_ENDPOINT).expect("child loopback endpoint");
    let home = PathBuf::from(std::env::var_os(CHILD_HOME).expect("child home"));
    assert!(home.is_absolute());
    let connection = DeepSeekConnectionConfig {
        endpoint: DeepSeekEndpoint::loopback_fixture(&endpoint)
            .expect("child accepts only a loopback fixture"),
        strict_tools: false,
        response_header_timeout: Duration::from_secs(5),
        stream_idle_timeout: Duration::from_secs(5),
        retry: TransportRetryPolicy::disabled(),
    };
    let mut config = ProductionApplicationConfig::official()
        .with_state_db_path(db)
        .with_deepseek_connection(connection)
        .with_composition_build_revision(COMPOSITION_REVISION);
    if std::env::var(CHILD_WITH_KEY).as_deref() == Ok("1") {
        config = config
            .with_api_key("fixture-key")
            .expect("bind fixture key");
    }
    let application = Arc::new(AgentApplication::production(config).expect("production app"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build app-server child runtime");
    runtime
        .block_on(run_stdio(application))
        .expect("serve canonical stdio");
}

#[test]
fn panic_unwind_reaps_external_app_server_process_group() {
    let (_temp, _workspace, home, db) = fixture_paths("m4b-app-panic-reap-");
    let pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let captured_pid = pid.clone();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let process = StdioProcess::spawn(&db, "http://127.0.0.1:9/v1", false, &home);
        captured_pid.store(process.pid(), Ordering::Release);
        panic!("intentional cleanup regression");
    }));
    assert!(panic.is_err(), "cleanup regression must exercise unwind");
    let pid = pid.load(Ordering::Acquire);
    assert_ne!(pid, 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_alive(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!process_alive(pid), "unwinding Drop leaked child PID {pid}");
}

#[test]
fn live_owner_resume_is_rejected_across_app_server_processes() {
    let (_temp, workspace, home, db) = fixture_paths("m4b-app-live-owner-");
    let fixture = DeepSeekFixture::start();
    let mut owner = StdioProcess::spawn(&db, &fixture.root, true, &home);
    let started = run_from_response(owner.request(&envelope(
        "start-live-owner",
        RunCommand::Start(start_command(&workspace)),
    )));
    fixture.wait_requests(1);
    let before = wait_db_state(&db, Duration::from_secs(5), |state| {
        state.run_id == started.run_id.0
            && state.lease_owner_pid == Some(owner.pid())
            && state.pending_model_in_flight
    });
    let mut contender = StdioProcess::spawn(&db, &fixture.root, true, &home);
    let response = contender.request(&envelope(
        "resume-live-owner",
        RunCommand::Resume {
            run_id: started.run_id.clone(),
            expected_workspace: Some(started.workspace.clone()),
        },
    ));
    assert!(matches!(
        response.result,
        RunCommandResult::Error { error }
            if error.code == RunApiErrorCode::RunAlreadyRunning
                && error.run_id == Some(started.run_id.clone())
    ));
    let after = read_db_state(&db)
        .expect("read live-owner state")
        .expect("live-owner run");
    assert_eq!(after.event_count, before.event_count);
    assert_eq!(after.event_digest, before.event_digest);
    assert_eq!(after.execution_epoch, before.execution_epoch);
    assert_eq!(after.lease_owner_pid, Some(owner.pid()));
    assert_eq!(fixture.request_count(), 1);
    contender.shutdown();
    owner.kill_group();
    fixture.release_one();
}

#[test]
fn sigkill_app_server_recovers_same_run_and_terminal_replays_without_key() {
    let (_temp, workspace, home, db) = fixture_paths("m4b-app-sigkill-");
    let fixture = DeepSeekFixture::start();
    let workspace_before = tree_digest(&workspace);

    let mut crashed_process = StdioProcess::spawn(&db, &fixture.root, true, &home);
    let readiness = crashed_process.request(&envelope(
        "child-readiness",
        RunCommand::Get {
            run_id: RunId::from("readiness-probe-does-not-exist"),
        },
    ));
    assert!(matches!(
        readiness.result,
        RunCommandResult::Error { error } if error.code == RunApiErrorCode::RunNotFound
    ));
    crashed_process.send(&envelope(
        "start-before-sigkill",
        RunCommand::Start(start_command(&workspace)),
    ));
    let crashed_pid = crashed_process.pid();
    let (pre_crash, signal) = capture_safe_state_and_sigkill(&mut crashed_process, &db);
    assert_eq!(signal, libc::SIGKILL);
    assert!(safe_pre_model_io(&pre_crash, crashed_pid));
    let after_kill = read_db_state(&db)
        .expect("read state after SIGKILL")
        .expect("run remains durable after SIGKILL");
    assert_eq!(after_kill.event_digest, pre_crash.event_digest);
    assert_eq!(after_kill.event_count, pre_crash.event_count);
    assert_eq!(after_kill.execution_epoch, pre_crash.execution_epoch);
    assert_eq!(after_kill.lease_owner_id, pre_crash.lease_owner_id);
    assert_eq!(after_kill.lease_owner_pid, Some(crashed_pid));
    assert_eq!(
        fixture.request_count(),
        0,
        "safe point must precede model I/O"
    );

    let run_id = RunId::from(pre_crash.run_id.clone());
    let mut resumed_process = StdioProcess::spawn(&db, &fixture.root, true, &home);
    let resumed_pid = resumed_process.pid();
    let resumed = run_from_response(
        resumed_process.request(&envelope(
            "resume-after-sigkill",
            RunCommand::Resume {
                run_id: run_id.clone(),
                expected_workspace: Some(
                    workspace
                        .canonicalize()
                        .expect("canonical workspace")
                        .display()
                        .to_string(),
                ),
            },
        )),
    );
    assert_eq!(resumed.run_id, run_id);
    fixture.wait_requests(1);
    let new_owner = wait_db_state(&db, Duration::from_secs(5), |state| {
        state.run_id == run_id.0
            && state.execution_epoch > pre_crash.execution_epoch
            && state.lease_owner_pid == Some(resumed_process.pid())
            && state.pending_model_in_flight
    });
    assert_ne!(new_owner.lease_owner_id, pre_crash.lease_owner_id);
    assert!(new_owner.execution_epoch > pre_crash.execution_epoch);
    fixture.release_one();
    let completed = wait_db_state(&db, Duration::from_secs(8), |state| state.terminal);
    assert_eq!(completed.run_id, run_id.0);
    assert!(completed.last_sequence > pre_crash.last_sequence);
    assert_eq!(completed.terminal_count, 1);
    assert!(completed.terminal_last);
    assert!(completed.sequences_contiguous);
    assert!(completed.event_ids_unique);
    assert_eq!(completed.unique_event_id_count, completed.event_count);
    assert!(completed.event_run_ids_match);
    assert!(completed.terminal_outcome_run_ids_match);
    assert_eq!(completed.lease_owner_id, None);
    assert_eq!(completed.lease_owner_pid, None);
    assert!(!completed.pending_model_in_flight);
    assert_eq!(
        completed.model_request_count - pre_crash.model_request_count,
        1
    );
    assert_eq!(
        completed.tool_side_effect_count,
        pre_crash.tool_side_effect_count
    );
    assert_eq!(
        completed.tool_side_effect_digest,
        pre_crash.tool_side_effect_digest
    );
    let prefix_after_reopen = event_prefix_digest(&db, &run_id.0, pre_crash.event_count)
        .expect("read reopened prefix")
        .expect("complete reopened prefix");
    assert_eq!(prefix_after_reopen, pre_crash.event_digest);
    assert_eq!(
        completed.last_sequence, completed.event_count as u64,
        "event sequence must be contiguous through terminal"
    );

    let completed_run = run_from_response(resumed_process.request(&envelope(
        "get-completed",
        RunCommand::Get {
            run_id: run_id.clone(),
        },
    )));
    assert!(matches!(
        completed_run.terminal,
        Some(TerminalState::Completed { .. })
    ));
    assert_eq!(completed_run.accounting.total_started(), 1);
    assert_eq!(completed_run.accounting.total_completed(), 1);
    assert_eq!(completed_run.accounting.total_in_flight(), 0);
    assert_eq!(completed_run.accounting.usage.input_tokens, 11);
    assert_eq!(completed_run.accounting.usage.output_tokens, 3);
    let frozen_events = events_from_response(resumed_process.request(&envelope(
        "events-completed",
        RunCommand::Events {
            run_id: run_id.clone(),
            after_sequence: 0,
        },
    )));
    assert_eq!(frozen_events.len(), completed.event_count);
    assert_eq!(
        frozen_events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
    assert_eq!(
        frozen_events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        (1..=completed.last_sequence).collect::<Vec<_>>()
    );
    resumed_process.shutdown();

    let requests_before_replay = fixture.request_count();
    let workspace_after_completion = tree_digest(&workspace);
    assert_eq!(workspace_after_completion, workspace_before);
    let mut replay_process = StdioProcess::spawn(&db, &fixture.root, false, &home);
    let replay_get = run_from_response(replay_process.request(&envelope(
        "no-key-get",
        RunCommand::Get {
            run_id: run_id.clone(),
        },
    )));
    let replay_events = events_from_response(replay_process.request(&envelope(
        "no-key-events",
        RunCommand::Events {
            run_id: run_id.clone(),
            after_sequence: 0,
        },
    )));
    let replay_resume = run_from_response(replay_process.request(&envelope(
        "no-key-resume",
        RunCommand::Resume {
            run_id: run_id.clone(),
            expected_workspace: None,
        },
    )));
    assert_eq!(replay_get, completed_run);
    assert_eq!(replay_resume, completed_run);
    assert_eq!(replay_events, frozen_events);
    let replay_state = read_db_state(&db)
        .expect("read no-key replay state")
        .expect("replayed run");
    assert_eq!(replay_state.event_count, completed.event_count);
    assert_eq!(replay_state.last_sequence, completed.last_sequence);
    assert_eq!(replay_state.event_digest, completed.event_digest);
    assert_eq!(replay_state.terminal_count, 1);
    assert_eq!(
        replay_state.tool_side_effect_count,
        completed.tool_side_effect_count
    );
    assert_eq!(
        replay_state.tool_side_effect_digest,
        completed.tool_side_effect_digest
    );
    assert_eq!(fixture.request_count(), requests_before_replay);
    assert_eq!(tree_digest(&workspace), workspace_after_completion);
    replay_process.shutdown();

    let record = json!({
        "schema": "codewhale.eval.m4b-app-server-recovery.v1",
        "run_id": run_id.0,
        "crash_phase": "durable_pre_model_io",
        "kill_mechanism": "SIGKILL",
        "same_run": completed.run_id == pre_crash.run_id,
        "last_committed_seq_before_crash": pre_crash.last_sequence,
        "first_committed_seq_after_resume": pre_crash.last_sequence + 1,
        "event_count_before_crash": pre_crash.event_count,
        "event_count_after_resume": completed.event_count,
        "event_prefix_digest_before_crash": pre_crash.event_digest,
        "event_prefix_digest_after_reopen": prefix_after_reopen,
        "final_event_digest": completed.event_digest,
        "unique_event_id_count": completed.unique_event_id_count,
        "terminal_count": completed.terminal_count,
        "model_request_delta": completed.model_request_count - pre_crash.model_request_count,
        "usage_delta": completed_run.usage,
        "unknown_billing": completed_run.accounting.billing_unknown,
        "tool_side_effect_count": {
            "before": pre_crash.tool_side_effect_count,
            "after": completed.tool_side_effect_count,
            "delta": completed.tool_side_effect_count - pre_crash.tool_side_effect_count
        },
        "tool_side_effect_digest": {
            "before": pre_crash.tool_side_effect_digest,
            "after": completed.tool_side_effect_digest
        },
        "lease_outcome": {
            "status": "reclaimed_dead_owner",
            "old_owner": {
                "owner_id": pre_crash.lease_owner_id,
                "owner_pid": crashed_pid,
                "execution_epoch": pre_crash.execution_epoch,
                "pid_dead_after_kill": !process_alive(crashed_pid)
            },
            "new_owner": {
                "owner_id": new_owner.lease_owner_id,
                "owner_pid": resumed_pid,
                "execution_epoch": new_owner.execution_epoch
            },
            "final_owner_released": completed.lease_owner_pid.is_none()
        },
        "no_key_replay": true,
        "workspace_revision_sha256": workspace_after_completion,
        "product_metric_eligible": false
    });
    assert_eq!(record["same_run"], true);
    assert_eq!(record["terminal_count"], 1);
    assert_eq!(record["no_key_replay"], true);
    println!("M4B_APP_SERVER_RECOVERY={record}");
}
