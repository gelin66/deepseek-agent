//! Real-PTY acceptance for the canonical interactive TUI foreground.
//!
//! The scenario launches the built `codewhale-tui` binary in a real
//! pseudo-terminal, sends a Chinese multi-line prompt to a loopback DeepSeek
//! endpoint, waits for the canonical terminal projection, exits normally, and
//! then verifies the durable SQLite RunStore rather than trusting screen text
//! alone.

#![cfg(unix)]

#[path = "support/qa_harness/mod.rs"]
mod qa_harness;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Context;
use codewhale_protocol::agent_runtime::{
    CommandId, ReasoningEffort, RunId, RunLimits, RuntimeEventKind, TerminalState, ToolPolicy,
};
use codewhale_protocol::run_api::{
    PendingCreationKind, RunCommand, RunProductControls, StartRunCommand,
};
use codewhale_runtime::{CreationIntent, RunStore};
use codewhale_state::StateStore;
use qa_harness::harness::{Harness, make_sealed_workspace};
use qa_harness::keys;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BOOT_TIMEOUT: Duration = Duration::from_secs(20);
const RUN_TIMEOUT: Duration = Duration::from_secs(20);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const COMPOSER_READY_TEXT: &str = "Write a task";
const PROMPT: &str = "请审计真实 PTY 路径。\n第二行：确认 canonical RunStore。";
const COMPLETION_MARKER: &str = "CANONICAL-PTY-DONE";
const ONBOARDING_KEY: &str = "sk-offline-canonical-onboarding-key";
const RECOVERY_CREATION_ID: &str = "pty-recover-explicit-creation";
const RECOVERY_RESERVED_RUN_ID: &str = "pty-reserved-explicit-run";
const RECOVERY_PROMPT: &str = "恢复中断的显式创建，不得生成第二个 run。";
const UNKNOWN_CREATION_ID: &str = "pty-recover-unknown-billing";
const UNKNOWN_RESERVED_RUN_ID: &str = "pty-reserved-unknown-billing";
const UNKNOWN_PENDING_PROMPT: &str = "这条自动选模请求已经处于未知计费状态。";
const GUARDED_INITIAL_PROMPT: &str = "这条 CLI 初始提示必须等待我明确按 Enter。";

#[test]
fn real_pty_chinese_multiline_reaches_canonical_terminal_and_sqlite_truth() -> anyhow::Result<()> {
    let (base_url, request_rx, server) = spawn_deepseek_fixture()?;
    let isolated = make_sealed_workspace()?;
    let codewhale_home = isolated.home().join(".codewhale");
    let state_path = codewhale_home.join("state.db");
    let canonical_workspace = std::fs::canonicalize(isolated.workspace())?
        .display()
        .to_string();

    let mut tui = Harness::builder(Harness::cargo_bin("codewhale-tui"))
        .cwd(isolated.workspace())
        .clear_env()
        .seal_home(isolated.home())
        .env("CODEWHALE_HOME", codewhale_home.to_string_lossy())
        .env("DEEPSEEK_API_KEY", "offline-canonical-pty-key")
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("NO_ANIMATIONS", "1")
        .env("RUST_LOG", "warn")
        .args([
            "--workspace",
            isolated
                .workspace()
                .to_str()
                .expect("UTF-8 fixture workspace"),
            "--no-project-config",
            "--skip-onboarding",
        ])
        .size(40, 140)
        .spawn()?;

    tui.wait_for_text(COMPOSER_READY_TEXT, BOOT_TIMEOUT)?;
    tui.paste(PROMPT)?;
    tui.wait_for_text("第二行", Duration::from_secs(5))?;
    tui.send(keys::key::enter())?;
    tui.wait_for_text(COMPLETION_MARKER, RUN_TIMEOUT)?;
    tui.wait_for(
        |frame| frame.contains("✓ done") || frame.contains("运行已结束：已完成"),
        RUN_TIMEOUT,
    )?;

    tui.send(b"\x04")?; // Ctrl+D exits only after the canonical Terminal event.
    assert_eq!(
        tui.wait_for_exit(EXIT_TIMEOUT),
        Some(0),
        "TUI did not exit cleanly after canonical Terminal:\n{}",
        tui.debug_dump()
    );

    let request_body = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("loopback fixture did not receive the DeepSeek request");
    assert!(
        json_strings_contain(&request_body, PROMPT),
        "DeepSeek request lost or rewrote the Chinese multi-line input: {request_body:#}"
    );
    server
        .join()
        .expect("loopback DeepSeek fixture thread panicked")?;

    assert!(
        state_path.is_file(),
        "canonical TUI did not create {}",
        state_path.display()
    );
    assert_canonical_sqlite_truth(&state_path, &canonical_workspace)?;
    assert_no_legacy_execution_json(isolated.home());
    assert_no_legacy_execution_json(isolated.workspace());
    Ok(())
}

#[test]
fn first_run_configures_only_deepseek_then_reaches_canonical_terminal() -> anyhow::Result<()> {
    let isolated = make_sealed_workspace()?;
    let codewhale_home = isolated.home().join(".codewhale");
    let state_path = codewhale_home.join("state.db");
    let config_path = codewhale_home.join("config.toml");
    let canonical_workspace = std::fs::canonicalize(isolated.workspace())?
        .display()
        .to_string();

    let mut onboarding = Harness::builder(Harness::cargo_bin("codewhale-tui"))
        .cwd(isolated.workspace())
        .clear_env()
        .seal_home(isolated.home())
        .env("CODEWHALE_HOME", codewhale_home.to_string_lossy())
        .env("NO_ANIMATIONS", "1")
        .env("RUST_LOG", "warn")
        .args([
            "--workspace",
            isolated
                .workspace()
                .to_str()
                .expect("UTF-8 fixture workspace"),
            "--no-project-config",
        ])
        .size(40, 140)
        .spawn()?;

    onboarding
        .wait_for_text("Code means two things", BOOT_TIMEOUT)
        .context("wait for first-run welcome")?;
    onboarding.send(keys::key::enter())?;
    onboarding
        .wait_for_text("Choose your language", Duration::from_secs(5))
        .context("wait for language picker")?;
    onboarding.send(b"4")?;
    onboarding
        .wait_for_text("连接你的 API 密钥", Duration::from_secs(5))
        .context("wait for DeepSeek key entry")?;
    onboarding.paste(ONBOARDING_KEY)?;
    onboarding.send(keys::key::enter())?;
    onboarding
        .wait_for_text("信任工作目录", Duration::from_secs(5))
        .context("wait for workspace trust")?;
    onboarding.send(b"y")?;
    onboarding
        .wait_for_text("从设置开始", Duration::from_secs(5))
        .context("wait for first-run tips")?;
    onboarding.send(keys::key::enter())?;
    onboarding
        .wait_for_text("编写任务", Duration::from_secs(5))
        .context("wait for canonical composer after onboarding")?;
    onboarding.send(b"\x04")?;
    assert_eq!(
        onboarding.wait_for_exit(EXIT_TIMEOUT),
        Some(0),
        "first-run setup did not exit cleanly:\n{}",
        onboarding.debug_dump()
    );

    let config = std::fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&config)?;
    assert_eq!(
        config.get("provider").and_then(toml::Value::as_str),
        Some("deepseek")
    );
    assert_eq!(
        config.get("api_key").and_then(toml::Value::as_str),
        Some(ONBOARDING_KEY)
    );
    assert!(
        config
            .get("projects")
            .and_then(toml::Value::as_table)
            .is_some_and(|projects| projects.values().any(|project| {
                project.get("trust_level").and_then(toml::Value::as_str) == Some("trusted")
            }))
    );
    assert!(codewhale_home.join(".onboarded").is_file());
    assert!(
        !isolated.home().join(".deepseek").exists(),
        "first-run flow must not write legacy .deepseek state"
    );

    let (base_url, request_rx, server) = spawn_deepseek_fixture()?;
    let mut tui = Harness::builder(Harness::cargo_bin("codewhale-tui"))
        .cwd(isolated.workspace())
        .clear_env()
        .seal_home(isolated.home())
        .env("CODEWHALE_HOME", codewhale_home.to_string_lossy())
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("NO_ANIMATIONS", "1")
        .env("RUST_LOG", "warn")
        .args([
            "--workspace",
            isolated
                .workspace()
                .to_str()
                .expect("UTF-8 fixture workspace"),
            "--no-project-config",
        ])
        .size(40, 140)
        .spawn()?;

    tui.wait_for_text("编写任务", BOOT_TIMEOUT)
        .context("wait for canonical composer after first-run restart")?;
    tui.paste(PROMPT)?;
    tui.send(keys::key::enter())?;
    tui.wait_for_text(COMPLETION_MARKER, RUN_TIMEOUT)?;
    tui.wait_for(
        |frame| frame.contains("✓ done") || frame.contains("✓ 完成"),
        RUN_TIMEOUT,
    )?;
    tui.send(b"\x04")?;
    assert_eq!(
        tui.wait_for_exit(EXIT_TIMEOUT),
        Some(0),
        "first-run TUI did not exit cleanly:\n{}",
        tui.debug_dump()
    );

    let request_body = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("loopback fixture did not receive the first-run request");
    assert!(json_strings_contain(&request_body, PROMPT));
    server
        .join()
        .expect("loopback DeepSeek fixture thread panicked")?;

    assert_canonical_sqlite_truth(&state_path, &canonical_workspace)?;
    assert_no_legacy_execution_json(isolated.home());
    Ok(())
}

#[test]
fn restart_recovers_unique_explicit_creation_with_same_reserved_run() -> anyhow::Result<()> {
    let (base_url, request_rx, server) = spawn_deepseek_fixture()?;
    let isolated = make_sealed_workspace()?;
    let codewhale_home = isolated.home().join(".codewhale");
    let state_path = codewhale_home.join("state.db");
    let canonical_workspace = std::fs::canonicalize(isolated.workspace())?
        .display()
        .to_string();
    seed_pending_start(
        &state_path,
        &canonical_workspace,
        RECOVERY_CREATION_ID,
        RECOVERY_RESERVED_RUN_ID,
        RECOVERY_PROMPT,
        Some("deepseek-v4-pro"),
    )?;

    let mut tui = Harness::builder(Harness::cargo_bin("codewhale-tui"))
        .cwd(isolated.workspace())
        .clear_env()
        .seal_home(isolated.home())
        .env("CODEWHALE_HOME", codewhale_home.to_string_lossy())
        .env("DEEPSEEK_API_KEY", "offline-canonical-recovery-key")
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("NO_ANIMATIONS", "1")
        .env("RUST_LOG", "warn")
        .args([
            "--workspace",
            isolated
                .workspace()
                .to_str()
                .expect("UTF-8 fixture workspace"),
            "--no-project-config",
            "--skip-onboarding",
        ])
        .size(40, 140)
        .spawn()?;

    tui.wait_for_text(COMPLETION_MARKER, RUN_TIMEOUT)?;
    tui.wait_for(
        |frame| frame.contains("✓ done") || frame.contains("运行已结束：已完成"),
        RUN_TIMEOUT,
    )?;
    tui.send(b"\x04")?;
    assert_eq!(
        tui.wait_for_exit(EXIT_TIMEOUT),
        Some(0),
        "TUI did not exit cleanly after recovering the reserved run:\n{}",
        tui.debug_dump()
    );

    let request_body = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("recovered creation did not reach the loopback DeepSeek fixture");
    assert!(
        json_strings_contain(&request_body, RECOVERY_PROMPT),
        "recovery did not replay the private canonical creation payload: {request_body:#}"
    );
    server
        .join()
        .expect("loopback DeepSeek fixture thread panicked")?;

    assert_recovered_creation_sqlite_truth(
        &state_path,
        &canonical_workspace,
        RECOVERY_CREATION_ID,
        RECOVERY_RESERVED_RUN_ID,
    )?;
    assert_no_legacy_execution_json(isolated.home());
    Ok(())
}

#[test]
fn unknown_billing_blocks_automatic_cli_prompt_and_all_deepseek_posts() -> anyhow::Result<()> {
    let fixture = CountingDeepSeekFixture::spawn()?;
    let isolated = make_sealed_workspace()?;
    let codewhale_home = isolated.home().join(".codewhale");
    let state_path = codewhale_home.join("state.db");
    let canonical_workspace = std::fs::canonicalize(isolated.workspace())?
        .display()
        .to_string();
    seed_pending_start(
        &state_path,
        &canonical_workspace,
        UNKNOWN_CREATION_ID,
        UNKNOWN_RESERVED_RUN_ID,
        UNKNOWN_PENDING_PROMPT,
        None,
    )?;

    let mut tui = Harness::builder(Harness::cargo_bin("codewhale-tui"))
        .cwd(isolated.workspace())
        .clear_env()
        .seal_home(isolated.home())
        .env("CODEWHALE_HOME", codewhale_home.to_string_lossy())
        .env("DEEPSEEK_API_KEY", "offline-canonical-unknown-billing-key")
        .env("DEEPSEEK_BASE_URL", fixture.base_url())
        .env("NO_ANIMATIONS", "1")
        .env("RUST_LOG", "warn")
        .args([
            "--workspace",
            isolated
                .workspace()
                .to_str()
                .expect("UTF-8 fixture workspace"),
            "--no-project-config",
            "--skip-onboarding",
            "--prompt",
            GUARDED_INITIAL_PROMPT,
        ])
        .size(40, 140)
        .spawn()?;

    tui.wait_for_text("计费状态未知", BOOT_TIMEOUT)?;
    tui.wait_for_text(UNKNOWN_CREATION_ID, Duration::from_secs(5))?;
    tui.wait_for_text(GUARDED_INITIAL_PROMPT, Duration::from_secs(5))?;
    std::thread::sleep(Duration::from_millis(750));
    assert_eq!(
        fixture.post_count(),
        0,
        "unknown-billing startup must not issue a DeepSeek chat request"
    );

    assert_unknown_creation_remains_pending(
        &state_path,
        &canonical_workspace,
        UNKNOWN_CREATION_ID,
        UNKNOWN_RESERVED_RUN_ID,
    )?;
    tui.send(b"\x04")?;
    assert_eq!(
        tui.wait_for_exit(EXIT_TIMEOUT),
        Some(0),
        "unknown-billing warning did not leave the TUI usable and exit-safe:\n{}",
        tui.debug_dump()
    );
    fixture.shutdown()?;
    Ok(())
}

fn assert_canonical_sqlite_truth(state_path: &Path, workspace: &str) -> anyhow::Result<()> {
    let store = StateStore::open(Some(state_path.to_path_buf()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let roots = runtime.block_on(store.list_root_runs(workspace, 10))?;
    assert_eq!(roots.len(), 1, "expected one canonical root: {roots:#?}");
    let root = &roots[0];
    assert!(
        root.terminal,
        "canonical root never reached Terminal: {root:#?}"
    );

    let replay = runtime
        .block_on(store.load(&root.run_id))?
        .expect("canonical root must be replayable");
    assert_eq!(
        replay.events.len() as u64,
        root.last_sequence,
        "root sequence must describe the exact durable event stream"
    );
    assert!(
        replay
            .events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1),
        "canonical event sequences are not contiguous: {:#?}",
        replay.events
    );

    match &replay.events.first().expect("RunCreated event").event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.input, PROMPT);
            assert_eq!(request.environment.workspace, workspace);
            assert!(request.parent_run_id.is_none());
        }
        event => panic!("first canonical event was not RunCreated: {event:?}"),
    }
    assert!(
        replay.events.iter().any(|event| matches!(
            &event.event,
            RuntimeEventKind::ModelResponseCommitted { output, .. }
                if output.content.contains(COMPLETION_MARKER)
        )),
        "durable event stream lost the committed model output"
    );
    assert!(matches!(
        replay.events.last().map(|event| &event.event),
        Some(RuntimeEventKind::Terminal { outcome })
            if matches!(outcome.terminal, TerminalState::Completed { .. })
    ));
    Ok(())
}

fn seed_pending_start(
    state_path: &Path,
    workspace: &str,
    creation_request_id: &str,
    reserved_run_id: &str,
    input: &str,
    model: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let command = RunCommand::Start(StartRunCommand {
        input: input.to_owned(),
        workspace: workspace.to_owned(),
        model: model.map(str::to_owned),
        reasoning_effort: ReasoningEffort::default(),
        max_output_tokens: None,
        max_api_requests: None,
        streaming: true,
        tool_policy: ToolPolicy::default(),
        limits: RunLimits::default(),
        controls: RunProductControls {
            interactive: true,
            ..RunProductControls::default()
        },
    });
    let intent = CreationIntent {
        kind: PendingCreationKind::Start,
        workspace: workspace.to_owned(),
        source_run_id: None,
        command: command.clone(),
    };
    let command_sha256 = creation_command_sha256(&command);
    let store = StateStore::open(Some(state_path.to_path_buf()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(store.reserve_creation(
        &CommandId::from(creation_request_id),
        &command_sha256,
        RunId::from(reserved_run_id),
        intent,
    ))?;
    Ok(())
}

fn creation_command_sha256(command: &RunCommand) -> String {
    let digest = Sha256::digest(
        serde_json::to_vec(command).expect("canonical RunCommand must remain serializable"),
    )
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
    format!("sha256:{digest}")
}

fn assert_recovered_creation_sqlite_truth(
    state_path: &Path,
    workspace: &str,
    creation_request_id: &str,
    reserved_run_id: &str,
) -> anyhow::Result<()> {
    let store = StateStore::open(Some(state_path.to_path_buf()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let roots = runtime.block_on(store.list_root_runs(workspace, 10))?;
    assert_eq!(roots.len(), 1, "recovery created an extra root: {roots:#?}");
    assert_eq!(roots[0].run_id, RunId::from(reserved_run_id));
    assert!(roots[0].terminal, "recovered root did not reach Terminal");

    let replay = runtime
        .block_on(store.load(&RunId::from(reserved_run_id)))?
        .expect("reserved run must be replayable after recovery");
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::RunCreated { .. }))
            .count(),
        1,
        "recovery must commit exactly one RunCreated"
    );
    match &replay.events.first().expect("RunCreated event").event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.run_id.as_ref(), Some(&RunId::from(reserved_run_id)));
            assert_eq!(request.input, RECOVERY_PROMPT);
            assert_eq!(request.environment.workspace, workspace);
        }
        event => panic!("first recovered event was not RunCreated: {event:?}"),
    }
    let pending = runtime.block_on(store.list_pending_creations(workspace, 10))?;
    assert!(
        pending.is_empty(),
        "recovered creation intent was not cleared: {pending:#?}"
    );
    assert!(
        runtime
            .block_on(store.creation(&CommandId::from(creation_request_id)))?
            .is_some_and(|reservation| {
                reservation.run_id == RunId::from(reserved_run_id) && reservation.intent.is_none()
            }),
        "durable creation receipt lost its run identity or retained private intent"
    );
    Ok(())
}

fn assert_unknown_creation_remains_pending(
    state_path: &Path,
    workspace: &str,
    creation_request_id: &str,
    reserved_run_id: &str,
) -> anyhow::Result<()> {
    let store = StateStore::open(Some(state_path.to_path_buf()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    assert!(
        runtime
            .block_on(store.list_root_runs(workspace, 10))?
            .is_empty(),
        "unknown-billing startup must not create a replacement root"
    );
    assert!(
        runtime
            .block_on(store.load(&RunId::from(reserved_run_id)))?
            .is_none(),
        "unknown-billing startup must not create the reserved run"
    );
    let pending = runtime.block_on(store.list_pending_creations(workspace, 10))?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].command_id, CommandId::from(creation_request_id));
    assert_eq!(pending[0].run_id, RunId::from(reserved_run_id));
    assert!(
        pending[0]
            .intent
            .as_ref()
            .is_some_and(CreationIntent::is_unknown_billing)
    );
    Ok(())
}

fn assert_no_legacy_execution_json(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    let mut legacy = Vec::<PathBuf>::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("inspect {}: {error}", directory.display()))
        {
            let path = entry.expect("isolated state entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("entry stays under isolated root")
                .to_string_lossy()
                .to_ascii_lowercase();
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                && ["session", "checkpoint", "task", "runtime"]
                    .iter()
                    .any(|marker| relative.contains(marker))
            {
                legacy.push(path);
            }
        }
    }
    assert!(
        legacy.is_empty(),
        "canonical PTY path wrote legacy execution JSON: {legacy:#?}"
    );
}

struct CountingDeepSeekFixture {
    base_url: String,
    post_count: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

impl CountingDeepSeekFixture {
    fn spawn() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let post_count = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_posts = Arc::clone(&post_count);
        let thread_stop = Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let request = read_http_request(&mut stream)?;
                if request.starts_with("GET /v1/models ") {
                    write_json_response(
                        &mut stream,
                        &json!({
                            "object": "list",
                            "data": [{ "id": "deepseek-chat", "object": "model" }]
                        }),
                    )?;
                } else if request.starts_with("POST /v1/chat/completions ") {
                    thread_posts.fetch_add(1, Ordering::AcqRel);
                    write_sse_response(&mut stream)?;
                } else {
                    write_not_found(&mut stream)?;
                }
            }
            Ok(())
        });
        Ok(Self {
            base_url: format!("http://{address}"),
            post_count,
            stop,
            server: Some(server),
        })
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn post_count(&self) -> usize {
        self.post_count.load(Ordering::Acquire)
    }

    fn shutdown(mut self) -> anyhow::Result<()> {
        self.stop.store(true, Ordering::Release);
        self.server
            .take()
            .expect("counting fixture server handle")
            .join()
            .expect("counting DeepSeek fixture thread panicked")
    }
}

impl Drop for CountingDeepSeekFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

fn spawn_deepseek_fixture() -> anyhow::Result<(
    String,
    mpsc::Receiver<Value>,
    std::thread::JoinHandle<anyhow::Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = mpsc::sync_channel(1);
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let request = read_http_request(&mut stream)?;
            if request.starts_with("GET /v1/models ") {
                write_json_response(
                    &mut stream,
                    &json!({
                        "object": "list",
                        "data": [{ "id": "deepseek-chat", "object": "model" }]
                    }),
                )?;
                continue;
            }
            if request.starts_with("POST /v1/chat/completions ") {
                let body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, body)| body)
                    .ok_or_else(|| anyhow::anyhow!("fixture request has no body"))?;
                request_tx
                    .send(serde_json::from_str(body)?)
                    .map_err(|_| anyhow::anyhow!("PTY test dropped request receiver"))?;
                write_sse_response(&mut stream)?;
                return Ok(());
            }
            write_not_found(&mut stream)?;
        }
        anyhow::bail!("loopback DeepSeek fixture timed out waiting for chat completion")
    });
    Ok((format!("http://{address}"), request_rx, handle))
}

fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    let expected_len = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            anyhow::bail!("fixture client closed before request headers");
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = find_subsequence(&bytes, b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..header_end])?;
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>())
                })
                .transpose()?
                .unwrap_or(0);
            break header_end + 4 + content_length;
        }
        if bytes.len() > 1024 * 1024 {
            anyhow::bail!("fixture request headers exceeded 1 MiB");
        }
    };
    while bytes.len() < expected_len {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            anyhow::bail!("fixture client closed before request body");
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(String::from_utf8(bytes[..expected_len].to_vec())?)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_json_response(stream: &mut TcpStream, body: &Value) -> anyhow::Result<()> {
    let body = serde_json::to_string(body)?;
    write_http_response(stream, "application/json", &body)
}

fn write_sse_response(stream: &mut TcpStream) -> anyhow::Result<()> {
    let body = [
        format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl-canonical-pty",
                "object": "chat.completion.chunk",
                "model": "deepseek-chat",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": format!("真实终端验收完成：{COMPLETION_MARKER}")
                    },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl-canonical-pty",
                "object": "chat.completion.chunk",
                "model": "deepseek-chat",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 6,
                    "total_tokens": 18,
                    "prompt_cache_hit_tokens": 0,
                    "prompt_cache_miss_tokens": 12
                }
            })
        ),
        "data: [DONE]\n\n".to_owned(),
    ]
    .join("");
    write_http_response(stream, "text/event-stream", &body)
}

fn write_not_found(stream: &mut TcpStream) -> anyhow::Result<()> {
    let body = r#"{"error":"not found"}"#;
    let response = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn write_http_response(
    stream: &mut TcpStream,
    content_type: &str,
    body: &str,
) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn json_strings_contain(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(values) => values
            .iter()
            .any(|value| json_strings_contain(value, needle)),
        Value::Object(values) => values
            .values()
            .any(|value| json_strings_contain(value, needle)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
