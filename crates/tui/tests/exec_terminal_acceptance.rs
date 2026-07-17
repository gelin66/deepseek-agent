//! Production-path acceptance tests for `exec --output-format stream-json`.
//!
//! These tests run the real Headless executable against a loopback HTTP
//! fixture. They are lifecycle evidence, not official DeepSeek wire-protocol
//! evidence: they validate stdout as strict NDJSON, bounded settlement,
//! request accounting, and exactly one terminal receipt with `done` last.
//! Official RequestPlan ownership is covered separately by planner/accounting
//! tests and the cost-bounded live canary.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use codewhale_runtime::{
    RunEnvironment, RunId, RunRequest, RunStore, RuntimeEventKind, StoredRuntimeEvent,
    TerminalState,
};
use codewhale_state::StateStore;
use serde_json::{Value, json};
use tempfile::TempDir;
use wait_timeout::ChildExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// Use a priced production model id so a nominal success must also prove exact
// cost attribution; a made-up id would correctly fail `cost_complete`.
const TEST_MODEL: &str = "deepseek-v4-flash";
const TEST_KEY: &str = "offline-exec-terminal-test-key";
const MULTI_AGENT_ROOT_PROMPT: &str = "exec-multi-agent-root-production-marker";
const MULTI_AGENT_SPAWN_CALL_ID: &str = "call_exec_multi_agent_spawn";
const MULTI_AGENT_CHILD_SYSTEM_MARKER: &str = "你是在同一 AgentRuntime 中运行的后台子 Agent";
const MULTI_AGENT_HANDOFF_MARKER: &str = "<codewhale:runtime_event kind=\"subagent_completion\"";
const MULTI_AGENT_ROOT_WAIT_MARKER: &str = "root-turn-ended-before-child-completion";
const MULTI_AGENT_CHILD_MARKER: &str = "child-production-complete-marker";
const MULTI_AGENT_PARENT_MARKER: &str = "parent-integrated-child-production-marker";
const NESTED_ROOT_PROMPT: &str = "exec-nested-agent-root-production-marker";
const NESTED_ROOT_SPAWN_CALL_ID: &str = "call_exec_nested_parent_spawn";
const NESTED_PARENT_PROMPT: &str = "nested-parent-task-production-marker";
const NESTED_GRANDCHILD_CALL_ID: &str = "call_exec_delayed_grandchild_spawn";
const NESTED_GRANDCHILD_PROMPT: &str = "nested-grandchild-task-production-marker";
const NESTED_GRANDCHILD_MARKER: &str = "nested-grandchild-production-complete";
const NESTED_PARENT_EARLY_MARKER: &str = "nested-parent-tried-to-finish-early";
const NESTED_PARENT_INTEGRATED_MARKER: &str = "nested-parent-integrated-grandchild";
const NESTED_ROOT_WAIT_MARKER: &str = "nested-root-ended-before-tree-completion";
const NESTED_ROOT_INTEGRATED_MARKER: &str = "nested-root-integrated-full-tree";
const NESTED_CHILD_HANDOFF_MARKER: &str =
    "<codewhale:runtime_event kind=\"child_subagent_completion\"";
const UNAUTHORIZED_WRITE_CALL_ID: &str = "call_exec_unauthorized_write";
const UNAUTHORIZED_WRITE_PATH: &str = "must-not-be-created.txt";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
static EXEC_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug)]
struct ExecOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
    elapsed: Duration,
}

#[derive(Default)]
struct NonAgentExecOptions<'a> {
    json_output: bool,
    stream_json: bool,
    allowed_tools: Option<&'a str>,
    disallowed_tools: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MultiAgentRequestKind {
    RootSpawn,
    RootWait,
    Child,
    ParentIntegration,
    Unexpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NestedAgentRequestKind {
    RootSpawn,
    RootWait,
    ParentSpawnGrandchild,
    ParentEarlyFinish,
    Grandchild,
    ParentIntegration,
    RootIntegration,
    Unexpected,
}

#[derive(Clone, Copy)]
struct MultiAgentResponder;

impl Respond for MultiAgentResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request.body_json::<Value>().unwrap_or(Value::Null);
        match classify_multi_agent_request(&body) {
            MultiAgentRequestKind::RootSpawn => sse_response(agent_spawn_sse()),
            MultiAgentRequestKind::RootWait => {
                sse_response(complete_sse(MULTI_AGENT_ROOT_WAIT_MARKER))
            }
            MultiAgentRequestKind::Child => {
                assert!(
                    !body.get("stream").and_then(Value::as_bool).unwrap_or(false),
                    "sub-agent request must use the non-streaming response contract: {body:#}"
                );
                // Starting the child request may race the parent's post-tool
                // request. Delay only the child response so the root turn
                // deterministically reaches TurnComplete first while the
                // child remains live in the manager.
                non_streaming_response(MULTI_AGENT_CHILD_MARKER).set_delay(Duration::from_secs(1))
            }
            MultiAgentRequestKind::ParentIntegration => {
                sse_response(complete_sse(MULTI_AGENT_PARENT_MARKER))
            }
            MultiAgentRequestKind::Unexpected => {
                sse_response(complete_sse("unexpected-multi-agent-request"))
            }
        }
    }
}

#[derive(Clone, Copy)]
struct AutoRouteResponder;

impl Respond for AutoRouteResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request.body_json::<Value>().unwrap_or(Value::Null);
        if body.get("stream").and_then(Value::as_bool) != Some(true) {
            non_streaming_response_with_usage(
                r#"{"provider":"deepseek","model":"deepseek-v4-flash","thinking":"off"}"#,
                7,
                2,
            )
        } else {
            sse_response(complete_sse("auto-route-production-marker"))
        }
    }
}

#[derive(Clone, Copy)]
struct UnauthorizedWriteResponder;

impl Respond for UnauthorizedWriteResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request.body_json::<Value>().unwrap_or(Value::Null);
        if json_strings_contain(&body, UNAUTHORIZED_WRITE_CALL_ID) {
            sse_response(complete_sse("unauthorized-write-denied-marker"))
        } else {
            sse_response(hallucinated_unauthorized_write_sse())
        }
    }
}

#[derive(Clone, Copy)]
struct SlowRouteAndRootResponder;

impl Respond for SlowRouteAndRootResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request.body_json::<Value>().unwrap_or(Value::Null);
        if body.get("stream").and_then(Value::as_bool) != Some(true) {
            non_streaming_response_with_usage(
                r#"{"provider":"deepseek","model":"deepseek-v4-flash","thinking":"off"}"#,
                7,
                2,
            )
            .set_delay(Duration::from_secs(1))
        } else {
            sse_response(complete_sse("must-not-outlive-the-global-deadline"))
                .set_delay(Duration::from_secs(4))
        }
    }
}

#[derive(Clone, Copy)]
struct NestedAgentResponder;

impl Respond for NestedAgentResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request.body_json::<Value>().unwrap_or(Value::Null);
        match classify_nested_agent_request(&body) {
            NestedAgentRequestKind::RootSpawn => sse_response(nested_root_spawn_sse()),
            NestedAgentRequestKind::RootWait => sse_response(complete_sse(NESTED_ROOT_WAIT_MARKER)),
            NestedAgentRequestKind::ParentSpawnGrandchild => {
                assert_non_streaming(&body, "nested parent spawn");
                nested_parent_spawn_response()
            }
            NestedAgentRequestKind::ParentEarlyFinish => {
                assert_non_streaming(&body, "nested parent early finish");
                non_streaming_response(NESTED_PARENT_EARLY_MARKER)
            }
            NestedAgentRequestKind::Grandchild => {
                assert_non_streaming(&body, "nested grandchild");
                non_streaming_response(NESTED_GRANDCHILD_MARKER).set_delay(Duration::from_secs(1))
            }
            NestedAgentRequestKind::ParentIntegration => {
                assert_non_streaming(&body, "nested parent integration");
                non_streaming_response(NESTED_PARENT_INTEGRATED_MARKER)
            }
            NestedAgentRequestKind::RootIntegration => {
                sse_response(complete_sse(NESTED_ROOT_INTEGRATED_MARKER))
            }
            NestedAgentRequestKind::Unexpected => {
                sse_response(complete_sse("unexpected-nested-agent-request"))
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_exec_resume_replays_the_same_terminal_without_another_model_request() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("durable-terminal-replay-marker")),
    )
    .await;

    let (command, workspace, home) = prepare_exec(
        &server.uri(),
        60,
        "return the durable terminal replay marker",
        "",
        None,
    );
    let first = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        first.status.success(),
        "initial durable exec failed\nstdout:\n{}\nstderr:\n{}",
        first.stdout,
        first.stderr
    );
    let first_events = parse_strict_ndjson(&first.stdout);
    let first_metadata = assert_terminal_tail(&first_events, None);
    let run_id = first_metadata["run_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .expect("terminal metadata must expose the canonical run id")
        .to_owned();
    assert!(
        first_metadata["resume_command"]
            .as_str()
            .is_some_and(|command| command.contains(&run_id)),
        "resume command must identify the canonical run"
    );
    assert_eq!(chat_request_count(&server).await, 1);

    let state_db = home.path().join(".codewhale/state.db");
    assert!(
        state_db.is_file(),
        "production exec did not create the canonical SQLite store: {}",
        state_db.display()
    );
    let store = StateStore::open(Some(state_db.clone())).expect("open production state db");
    let replay_before = store
        .load(&RunId::from(run_id.clone()))
        .await
        .expect("load completed run")
        .expect("completed run exists");
    assert_eq!(terminal_event_count(&replay_before.events), 1);
    assert!(matches!(
        replay_before
            .snapshot
            .terminal
            .as_ref()
            .map(|outcome| &outcome.terminal),
        Some(TerminalState::Completed { .. })
    ));
    drop(store);

    let mut resume_command = prepare_resume_exec(
        &server.uri(),
        workspace.path(),
        home.path(),
        &run_id,
        true,
        None,
    );
    resume_command.env_remove("DEEPSEEK_API_KEY");
    let resumed = run_with_timeout(resume_command, PROCESS_TIMEOUT);
    assert!(
        resumed.status.success(),
        "terminal replay failed\nstdout:\n{}\nstderr:\n{}",
        resumed.stdout,
        resumed.stderr
    );
    let resumed_events = parse_strict_ndjson(&resumed.stdout);
    let resumed_metadata = assert_terminal_tail(&resumed_events, None);
    assert_eq!(resumed_metadata["run_id"], run_id);
    assert_eq!(resumed_metadata["status"], first_metadata["status"]);
    assert_eq!(
        resumed_metadata["termination_reason"],
        first_metadata["termination_reason"]
    );
    assert_eq!(
        resumed_metadata["prompt_sha256"], first_metadata["prompt_sha256"],
        "resume must use the persisted prompt, not the placeholder CLI prompt"
    );
    assert_eq!(
        resumed_metadata["api_request_count"],
        first_metadata["api_request_count"]
    );
    assert_eq!(
        resumed_metadata["input_tokens"],
        first_metadata["input_tokens"]
    );
    assert_eq!(
        resumed_metadata["output_tokens"],
        first_metadata["output_tokens"]
    );
    assert_eq!(
        content_events(&resumed_events),
        content_events(&first_events),
        "terminal replay changed the visible model output"
    );
    assert_eq!(
        chat_request_count(&server).await,
        1,
        "resuming a terminal run sent another model request"
    );

    let store = StateStore::open(Some(state_db)).expect("reopen production state db");
    let replay_after = store
        .load(&RunId::from(run_id))
        .await
        .expect("reload completed run")
        .expect("completed run still exists");
    assert_eq!(
        replay_after.events, replay_before.events,
        "terminal replay appended or rewrote the canonical event log"
    );
    assert_eq!(terminal_event_count(&replay_after.events), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequential_fresh_runs_and_continue_share_one_store_without_creation_conflicts() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("sequential-creation-marker")),
    )
    .await;

    let first_prompt = "first fresh creation intent";
    let second_prompt = "second fresh creation intent";
    let continue_prompt = "continue only the second fresh run";
    let (mut first_command, workspace, home) =
        prepare_exec(&server.uri(), 30, first_prompt, "", None);
    let codewhale_home = home.path().join(".codewhale");
    first_command.env("CODEWHALE_HOME", &codewhale_home);
    let first = run_with_timeout(first_command, PROCESS_TIMEOUT);
    assert!(
        first.status.success(),
        "first fresh exec failed\nstdout:\n{}\nstderr:\n{}",
        first.stdout,
        first.stderr
    );
    let first_events = parse_strict_ndjson(&first.stdout);
    let first_run_id = RunId::from(
        assert_terminal_tail(&first_events, None)["run_id"]
            .as_str()
            .expect("first run id"),
    );

    std::thread::sleep(Duration::from_millis(5));
    let second = run_with_timeout(
        prepare_existing_exec(
            &server.uri(),
            workspace.path(),
            home.path(),
            second_prompt,
            false,
        ),
        PROCESS_TIMEOUT,
    );
    assert!(
        second.status.success(),
        "second fresh exec hit a creation conflict\nstdout:\n{}\nstderr:\n{}",
        second.stdout,
        second.stderr
    );
    let second_events = parse_strict_ndjson(&second.stdout);
    let second_run_id = RunId::from(
        assert_terminal_tail(&second_events, None)["run_id"]
            .as_str()
            .expect("second run id"),
    );
    assert_ne!(second_run_id, first_run_id);

    let state_db = codewhale_home.join("state.db");
    let store = StateStore::open(Some(state_db.clone())).expect("open shared exec state db");
    let second_before = store
        .load(&second_run_id)
        .await
        .expect("load second fresh run")
        .expect("second fresh run exists");
    drop(store);

    std::thread::sleep(Duration::from_millis(5));
    let continued = run_with_timeout(
        prepare_existing_exec(
            &server.uri(),
            workspace.path(),
            home.path(),
            continue_prompt,
            true,
        ),
        PROCESS_TIMEOUT,
    );
    assert!(
        continued.status.success(),
        "continue exec hit a creation conflict\nstdout:\n{}\nstderr:\n{}",
        continued.stdout,
        continued.stderr
    );
    let continued_events = parse_strict_ndjson(&continued.stdout);
    let continued_run_id = RunId::from(
        assert_terminal_tail(&continued_events, None)["run_id"]
            .as_str()
            .expect("continued run id"),
    );
    assert_ne!(continued_run_id, first_run_id);
    assert_ne!(continued_run_id, second_run_id);
    assert_eq!(chat_request_count(&server).await, 3);

    let store = StateStore::open(Some(state_db)).expect("reopen shared exec state db");
    let first_replay = store
        .load(&first_run_id)
        .await
        .expect("load first fresh run")
        .expect("first fresh run exists");
    let second_after = store
        .load(&second_run_id)
        .await
        .expect("reload second fresh run")
        .expect("second fresh run still exists");
    let continued_replay = store
        .load(&continued_run_id)
        .await
        .expect("load continued run")
        .expect("continued run exists");
    assert_eq!(
        second_after.events, second_before.events,
        "creating the continuation must not rewrite its source run"
    );
    assert_eq!(first_replay.snapshot.request.continued_from_run_id, None);
    assert_eq!(second_after.snapshot.request.continued_from_run_id, None);
    assert_eq!(continued_replay.snapshot.request.parent_run_id, None);
    assert_eq!(
        continued_replay.snapshot.request.continued_from_run_id,
        Some(second_run_id.clone())
    );
    match &continued_replay.events[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.input, continue_prompt);
            assert_eq!(request.transcript, second_after.snapshot.transcript);
        }
        event => panic!("continued run must start with RunCreated, got {event:?}"),
    }
    let canonical_workspace =
        std::fs::canonicalize(workspace.path()).expect("canonical shared workspace");
    let roots = store
        .list_root_runs(&canonical_workspace.display().to_string(), 10)
        .await
        .expect("list shared-store roots");
    assert_eq!(roots.len(), 3);
    assert!(roots.iter().any(|run| run.run_id == first_run_id));
    assert!(roots.iter().any(|run| run.run_id == second_run_id));
    assert!(roots.iter().any(|run| run.run_id == continued_run_id));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_in_flight_model_request_resumes_fail_closed_without_resending() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("must-not-be-committed-after-kill"))
            .set_delay(Duration::from_secs(30)),
    )
    .await;

    let (mut command, workspace, home) = prepare_exec(
        &server.uri(),
        120,
        "hold one model request in flight",
        "",
        None,
    );
    let mut child = command.spawn().expect("spawn durable exec to kill");
    let stdout_reader = read_pipe(child.stdout.take().expect("stdout pipe"));
    let stderr_reader = read_pipe(child.stderr.take().expect("stderr pipe"));
    let observed_deadline = Instant::now() + Duration::from_secs(15);
    while chat_request_count(&server).await == 0 {
        assert!(
            Instant::now() < observed_deadline,
            "model request never reached the loopback server before kill"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let state_db = home.path().join(".codewhale/state.db");
    let canonical_workspace = std::fs::canonicalize(workspace.path()).expect("canonical workspace");
    let store = StateStore::open(Some(state_db.clone())).expect("open live run state db");
    let live_run = store
        .list_root_runs(&canonical_workspace.display().to_string(), 1)
        .await
        .expect("find live run")
        .into_iter()
        .next()
        .expect("in-flight run must already be durable");
    assert!(!live_run.terminal);
    let run_id = live_run.run_id;
    drop(store);

    let mut concurrent_command = prepare_resume_exec(
        &server.uri(),
        workspace.path(),
        home.path(),
        &run_id.0,
        true,
        None,
    );
    concurrent_command.env_remove("DEEPSEEK_API_KEY");
    let concurrent = run_with_timeout(concurrent_command, PROCESS_TIMEOUT);
    assert!(!concurrent.status.success());
    assert!(
        !concurrent.stdout.trim().is_empty(),
        "concurrent resume emitted no NDJSON\nstderr:\n{}",
        concurrent.stderr
    );
    let concurrent_events = parse_strict_ndjson(&concurrent.stdout);
    assert_eq!(concurrent_events.len(), 3, "{concurrent_events:#?}");
    assert_eq!(concurrent_events[0]["type"], "metadata");
    assert_eq!(
        concurrent_events[0]["meta"]["receipt_kind"],
        "runtime_failure"
    );
    assert_eq!(concurrent_events[0]["meta"]["run_id"], run_id.0);
    assert_eq!(concurrent_events[1]["type"], "error");
    assert_eq!(concurrent_events[1]["code"], "runtime_store_failed");
    assert_eq!(concurrent_events[2]["type"], "done");
    assert_eq!(
        chat_request_count(&server).await,
        1,
        "concurrent resume reached the model"
    );

    child.kill().expect("SIGKILL in-flight exec");
    let killed_status = child.wait().expect("reap killed exec");
    assert!(
        !killed_status.success(),
        "SIGKILL unexpectedly exited cleanly"
    );
    let _killed_stdout = join_pipe(stdout_reader, "killed stdout");
    let _killed_stderr = join_pipe(stderr_reader, "killed stderr");
    assert_eq!(chat_request_count(&server).await, 1);

    let store = StateStore::open(Some(state_db.clone())).expect("open killed run state db");
    let killed_replay = store
        .load(&run_id)
        .await
        .expect("load killed run")
        .expect("killed run exists");
    assert_eq!(
        killed_replay
            .events
            .iter()
            .filter(|event| matches!(&event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
            .count(),
        1,
        "kill did not land in the durable model in-flight window"
    );
    assert_eq!(terminal_event_count(&killed_replay.events), 0);
    drop(store);

    let mut first_resume_command = prepare_resume_exec(
        &server.uri(),
        workspace.path(),
        home.path(),
        &run_id.0,
        true,
        None,
    );
    first_resume_command.env_remove("DEEPSEEK_API_KEY");
    let first_resume = run_with_timeout(first_resume_command, PROCESS_TIMEOUT);
    assert!(
        !first_resume.status.success(),
        "ambiguous in-flight request must not resume as success\nstdout:\n{}\nstderr:\n{}",
        first_resume.stdout,
        first_resume.stderr
    );
    let first_resume_events = parse_strict_ndjson(&first_resume.stdout);
    let first_resume_metadata =
        assert_terminal_tail(&first_resume_events, Some("runtime_recovery_ambiguous"));
    assert_eq!(first_resume_metadata["status"], "failed");
    assert_eq!(first_resume_metadata["termination_reason"], "unresolved");
    assert_eq!(first_resume_metadata["billing_unknown_attempts"], 1);
    assert_eq!(
        chat_request_count(&server).await,
        1,
        "recovery repeated an ambiguously sent model request"
    );

    let store = StateStore::open(Some(state_db.clone())).expect("reopen recovered state db");
    let recovered_replay = store
        .load(&run_id)
        .await
        .expect("load recovered run")
        .expect("recovered run exists");
    assert_eq!(terminal_event_count(&recovered_replay.events), 1);
    assert!(matches!(
        recovered_replay
            .snapshot
            .terminal
            .as_ref()
            .map(|outcome| &outcome.terminal),
        Some(TerminalState::RecoveryRequired { .. })
    ));
    drop(store);

    let mut second_resume_command = prepare_resume_exec(
        &server.uri(),
        workspace.path(),
        home.path(),
        &run_id.0,
        true,
        None,
    );
    second_resume_command.env_remove("DEEPSEEK_API_KEY");
    let second_resume = run_with_timeout(second_resume_command, PROCESS_TIMEOUT);
    assert!(!second_resume.status.success());
    let second_resume_events = parse_strict_ndjson(&second_resume.stdout);
    assert_terminal_tail(&second_resume_events, Some("runtime_recovery_ambiguous"));
    assert_eq!(chat_request_count(&server).await, 1);

    let store = StateStore::open(Some(state_db)).expect("reopen state db after terminal replay");
    let replay_after_second_resume = store
        .load(&run_id)
        .await
        .expect("reload recovered run")
        .expect("recovered run still exists");
    assert_eq!(
        replay_after_second_resume.events, recovered_replay.events,
        "replaying RecoveryRequired appended a second terminal"
    );
    assert_eq!(terminal_event_count(&replay_after_second_resume.events), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_environment_mismatches_fail_closed_before_model_io() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("environment-mismatch-must-not-call-model")),
    )
    .await;

    let workspace = TempDir::new().expect("workspace tempdir");
    let other_workspace = TempDir::new().expect("other workspace tempdir");
    let canonical_workspace = std::fs::canonicalize(workspace.path()).expect("canonical workspace");
    let canonical_other_workspace =
        std::fs::canonicalize(other_workspace.path()).expect("canonical other workspace");
    let home = TempDir::new().expect("home tempdir");
    let config_dir = home.path().join(".codewhale");
    std::fs::create_dir_all(&config_dir).expect("create isolated config dir");
    std::fs::write(config_dir.join("config.toml"), "[retry]\nenabled = false\n")
        .expect("write isolated exec config");
    let state_db = config_dir.join("state.db");
    let store = StateStore::open(Some(state_db)).expect("open mismatch fixture state db");

    let workspace_mismatch = create_released_run(
        &store,
        RunEnvironment {
            workspace: canonical_other_workspace.display().to_string(),
            provider: "deepseek".to_owned(),
            tool_catalog_sha256: None,
            ..RunEnvironment::default()
        },
    )
    .await;
    let provider_mismatch = create_released_run(
        &store,
        RunEnvironment {
            workspace: canonical_workspace.display().to_string(),
            provider: "not-deepseek".to_owned(),
            tool_catalog_sha256: None,
            ..RunEnvironment::default()
        },
    )
    .await;
    let catalog_mismatch = create_released_run(
        &store,
        RunEnvironment {
            workspace: canonical_workspace.display().to_string(),
            provider: "deepseek".to_owned(),
            tool_catalog_sha256: Some("sha256:deliberately-wrong-catalog".to_owned()),
            auto_approve: true,
            trust_mode: true,
            ..RunEnvironment::default()
        },
    )
    .await;
    drop(store);

    for (run_id, expected_marker) in [
        (workspace_mismatch, "exec_resume_workspace_mismatch"),
        (provider_mismatch, "exec_resume_provider_mismatch"),
        (catalog_mismatch, "exec_resume_tool_catalog_mismatch"),
    ] {
        let output = run_with_timeout(
            prepare_resume_exec(
                &server.uri(),
                workspace.path(),
                home.path(),
                &run_id.0,
                true,
                None,
            ),
            PROCESS_TIMEOUT,
        );
        assert!(
            !output.status.success(),
            "{expected_marker} unexpectedly resumed as success\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
        let events = parse_strict_ndjson(&output.stdout);
        assert_startup_failure_contains(&events, expected_marker);
        assert_eq!(
            chat_request_count(&server).await,
            0,
            "{expected_marker} reached the model transport"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_execution_fingerprint_missing_or_mismatch_fails_closed_before_model_io() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("fingerprint-seed-response")),
    )
    .await;

    let (seed_command, workspace, home) =
        prepare_exec(&server.uri(), 10, "seed execution fingerprint", "", None);
    let seed_output = run_with_timeout(seed_command, PROCESS_TIMEOUT);
    assert!(
        seed_output.status.success(),
        "fingerprint seed exec failed\nstdout:\n{}\nstderr:\n{}",
        seed_output.stdout,
        seed_output.stderr
    );
    let seed_events = parse_strict_ndjson(&seed_output.stdout);
    let seed_metadata = assert_terminal_tail(&seed_events, None);
    let seed_run_id = RunId::from(
        seed_metadata["run_id"]
            .as_str()
            .expect("seed terminal run id"),
    );
    assert_eq!(chat_request_count(&server).await, 1);

    let state_db = home.path().join(".codewhale/state.db");
    let store = StateStore::open(Some(state_db)).expect("open fingerprint fixture state db");
    let seed_replay = store
        .load(&seed_run_id)
        .await
        .expect("load fingerprint seed run")
        .expect("fingerprint seed run exists");
    let exact_environment = seed_replay.snapshot.request.environment;
    assert!(exact_environment.tool_catalog_sha256.is_some());
    assert!(exact_environment.execution_fingerprint_sha256.is_some());

    let mut missing_environment = exact_environment.clone();
    missing_environment.execution_fingerprint_sha256 = None;
    let fingerprint_missing = create_released_run(&store, missing_environment).await;
    let mut mismatch_environment = exact_environment;
    mismatch_environment.execution_fingerprint_sha256 =
        Some("sha256:deliberately-wrong-execution-fingerprint".to_owned());
    let fingerprint_mismatch = create_released_run(&store, mismatch_environment).await;
    drop(store);

    for (run_id, expected_marker) in [
        (fingerprint_missing, "exec_resume_fingerprint_missing"),
        (fingerprint_mismatch, "exec_resume_fingerprint_mismatch"),
    ] {
        let output = run_with_timeout(
            prepare_resume_exec(
                &server.uri(),
                workspace.path(),
                home.path(),
                &run_id.0,
                true,
                None,
            ),
            PROCESS_TIMEOUT,
        );
        assert!(
            !output.status.success(),
            "{expected_marker} unexpectedly resumed as success\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
        assert_startup_failure_contains(&parse_strict_ndjson(&output.stdout), expected_marker);
        assert_eq!(
            chat_request_count(&server).await,
            1,
            "{expected_marker} reached the model transport"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_exec_emits_strict_ndjson_and_one_terminal_receipt() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("offline-success-marker")),
    )
    .await;

    let output = run_exec(&server, 10, "return the offline success marker");
    assert!(
        output.status.success(),
        "successful exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["status"], "completed");
    assert_eq!(metadata["termination_reason"], "resolved");
    assert_exact_success_accounting(metadata, 1, 11, 3);
    assert!(
        events.iter().any(|event| {
            event["type"] == "content"
                && event["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("offline-success-marker"))
        }),
        "missing streamed content event: {events:#?}"
    );
    assert_eq!(chat_request_count(&server).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_exec_uses_the_runtime_without_tools() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        non_streaming_response("plain-runtime-production-marker"),
    )
    .await;

    let (command, _workspace, _home) = prepare_non_agent_exec(
        &server.uri(),
        "return the plain runtime production marker",
        NonAgentExecOptions::default(),
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "plain exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    assert_eq!(output.stdout.trim(), "plain-runtime-production-marker");
    assert_eq!(chat_request_count(&server).await, 1);

    let requests = server
        .received_requests()
        .await
        .expect("plain runtime request journal");
    let body = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("plain runtime chat request")
        .body_json::<Value>()
        .expect("plain runtime request JSON");
    assert_non_streaming(&body, "plain runtime");
    assert!(
        body.get("tools").is_none_or(Value::is_null),
        "plain runtime must not expose tools: {body:#}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summary_json_exec_uses_the_runtime_terminal_without_tools() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        non_streaming_response("json-runtime-production-marker"),
    )
    .await;

    let (command, _workspace, _home) = prepare_non_agent_exec(
        &server.uri(),
        "return the JSON runtime production marker",
        NonAgentExecOptions {
            json_output: true,
            ..NonAgentExecOptions::default()
        },
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "summary JSON exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let summary: Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("summary JSON is invalid ({error}): {}", output.stdout));
    assert_ne!(summary["mode"], "one-shot");
    assert_eq!(summary["status"], "completed");
    assert_eq!(summary["termination_reason"], "resolved");
    assert_eq!(summary["output"], "json-runtime-production-marker");
    assert_exact_success_accounting(&summary, 1, 11, 3);
    assert_eq!(chat_request_count(&server).await, 1);

    let requests = server
        .received_requests()
        .await
        .expect("summary runtime request journal");
    let body = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("summary runtime chat request")
        .body_json::<Value>()
        .expect("summary runtime request JSON");
    assert_non_streaming(&body, "summary JSON runtime");
    assert!(
        body.get("tools").is_none_or(Value::is_null),
        "summary JSON runtime must not expose tools: {body:#}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_allowed_tools_enable_the_runtime_tool_catalog() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("explicit-tool-surface-production-marker")),
    )
    .await;

    let (command, _workspace, _home) = prepare_non_agent_exec(
        &server.uri(),
        "return the explicit tool surface marker",
        NonAgentExecOptions {
            allowed_tools: Some("read_file,grep_files"),
            disallowed_tools: Some("grep_files"),
            ..NonAgentExecOptions::default()
        },
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "explicit tool surface exec failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let requests = server
        .received_requests()
        .await
        .expect("explicit tool surface request journal");
    let body = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("explicit tool surface chat request")
        .body_json::<Value>()
        .expect("explicit tool surface request JSON");
    assert_eq!(body["stream"], true);
    let tools = body["tools"].as_array().expect("runtime tool catalog");
    assert_eq!(tools.len(), 1, "unexpected tool catalog: {tools:#?}");
    assert_eq!(tools[0]["function"]["name"], "read_file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_json_without_auto_denies_a_hallucinated_write() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(UnauthorizedWriteResponder)
        .mount(&server)
        .await;

    let (command, workspace, _home) = prepare_non_agent_exec(
        &server.uri(),
        "return the unauthorized-write-denied marker",
        NonAgentExecOptions {
            stream_json: true,
            ..NonAgentExecOptions::default()
        },
    );
    let forbidden_path = workspace.path().join(UNAUTHORIZED_WRITE_PATH);
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "tool-less stream-json exec failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    assert!(
        !forbidden_path.exists(),
        "stream-json without --auto executed an unauthorized write: {}",
        forbidden_path.display()
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["status"], "completed");
    assert!(events.iter().any(|event| {
        event["type"] == "tool_result"
            && event["name"] == "apply_patch"
            && event["status"] == "error"
            && event["output"]
                .as_str()
                .is_some_and(|output| output.contains("tool_not_allowed"))
    }));

    let requests = server
        .received_requests()
        .await
        .expect("tool-less stream-json request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        2,
        "denial must be returned to the model once"
    );
    for request in requests {
        let body = request
            .body_json::<Value>()
            .expect("tool-less stream-json request JSON");
        assert_eq!(body["stream"], true);
        assert!(
            body.get("tools").is_none_or(Value::is_null),
            "stream-json alone exposed tools: {body:#}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disallowed_tools_alone_do_not_authorize_the_remaining_catalog() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        non_streaming_response("deny-only-production-marker"),
    )
    .await;

    let (command, _workspace, _home) = prepare_non_agent_exec(
        &server.uri(),
        "return the deny-only marker",
        NonAgentExecOptions {
            disallowed_tools: Some("exec_shell"),
            ..NonAgentExecOptions::default()
        },
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "deny-only exec failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let requests = server
        .received_requests()
        .await
        .expect("deny-only request journal");
    let body = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("deny-only chat request")
        .body_json::<Value>()
        .expect("deny-only request JSON");
    assert_non_streaming(&body, "deny-only exec");
    assert!(
        body.get("tools").is_none_or(Value::is_null),
        "a deny-list granted the remaining tools: {body:#}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_subagents_remove_agent_from_the_exec_runtime() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("subagents-disabled-production-marker")),
    )
    .await;

    let output = run_exec_at(
        &server.uri(),
        10,
        "return the subagents-disabled marker",
        "[subagents]\nenabled = false\n",
        None,
    );
    assert!(
        output.status.success(),
        "exec with disabled subagents failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["api_request_child_started"], 0);
    assert_eq!(metadata["api_request_child_completed"], 0);

    let requests = server
        .received_requests()
        .await
        .expect("subagents-disabled request journal");
    let body = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("subagents-disabled chat request")
        .body_json::<Value>()
        .expect("subagents-disabled request JSON");
    let tool_names = body["tools"]
        .as_array()
        .expect("enabled exec tool catalog")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !tool_names.contains(&"agent"),
        "disabled subagents leaked the agent tool: {tool_names:?}"
    );
    assert_eq!(chat_request_count(&server).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_deepseek_startup_failure_emits_one_machine_terminal_tail() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    let (command, _workspace, _home) = prepare_exec_with_options(
        &server.uri(),
        10,
        "reject the unsupported provider before runtime start",
        "",
        None,
        TEST_MODEL,
        None,
        Some("openai"),
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(!output.status.success(), "unsupported provider must fail");

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_startup_tail(&events, "exec_provider_unsupported");
    assert_eq!(metadata["status"], "failed");
    assert_eq!(metadata["termination_reason"], "infrastructure_error");
    assert_eq!(chat_request_count(&server).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_and_runtime_share_one_absolute_wall_clock_deadline() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(SlowRouteAndRootResponder)
        .mount(&server)
        .await;
    let (command, _workspace, _home) = prepare_exec_with_options(
        &server.uri(),
        3,
        "prove route time is charged to the same deadline",
        "",
        None,
        "auto",
        None,
        None,
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        !output.status.success(),
        "combined route/runtime delay must fail"
    );
    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, Some("exec_watchdog_timeout"));
    assert_eq!(metadata["termination_reason"], "timeout");
    assert!(
        metadata["duration_ms"]
            .as_u64()
            .is_some_and(|duration| duration < 4_000),
        "route and runtime exceeded the single 3-second lifecycle: {metadata:#?}"
    );
    assert_eq!(chat_request_count(&server).await, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_router_and_root_agent_share_one_exact_request_ledger() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(AutoRouteResponder)
        .mount(&server)
        .await;

    let (command, _workspace, home) = prepare_exec_with_options(
        &server.uri(),
        10,
        "return the auto route production marker",
        "",
        None,
        "auto",
        None,
        None,
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(
        output.status.success(),
        "auto-routed exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["status"], "completed");
    assert_eq!(metadata["termination_reason"], "resolved");
    assert_eq!(metadata["route_source"], "auto_resolver");
    // Router: 7/2 tokens. Root execution: 11/3 tokens.
    assert_exact_success_accounting(metadata, 2, 18, 5);

    let run_id = RunId::from(
        metadata["run_id"]
            .as_str()
            .expect("terminal metadata must expose the canonical run id"),
    );
    let store = StateStore::open(Some(home.path().join(".codewhale/state.db")))
        .expect("open auto-route production state db");
    let replay = store
        .load(&run_id)
        .await
        .expect("load auto-route run")
        .expect("auto-route run exists");
    let baseline = match &replay.events[0].event {
        RuntimeEventKind::RunCreated { request } => &request.accounting_baseline,
        event => panic!("first canonical event must be run_created, got {event:?}"),
    };
    assert_eq!(baseline.total_started(), 1);
    assert_eq!(baseline.total_completed(), 1);
    assert_eq!(baseline.total_in_flight(), 0);
    assert_eq!(baseline.usage_responses, 1);
    assert_eq!(baseline.usage.input_tokens, 7);
    assert_eq!(baseline.usage.output_tokens, 2);
    assert!(baseline.cost_nanousd > 0);
    assert!(baseline.cost_nanocny > 0);
    assert!(baseline.complete);
    assert!(baseline.usage_complete);
    assert_eq!(baseline.surface_usage.len(), 1);
    assert_eq!(baseline.surface_usage[0].response_count, 1);
    assert_eq!(baseline.surface_usage[0].usage_response_count, 1);

    let chat_requests = server
        .received_requests()
        .await
        .expect("loopback request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    assert_eq!(chat_requests.len(), 2, "router + root request expected");
    assert_ne!(
        chat_requests[0].body_json::<Value>().expect("router body")["stream"],
        true,
        "router request must be non-streaming"
    );
    assert_eq!(
        chat_requests[1].body_json::<Value>().expect("root body")["stream"],
        true
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_model_alias_fails_before_any_classifier_or_root_request() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(&server, sse_response(complete_sse("must-not-reach-model"))).await;

    let (command, _workspace, _home) = prepare_exec_with_options(
        &server.uri(),
        10,
        "reject the explicit compatibility alias locally",
        "",
        None,
        "deepseek-v4flash",
        None,
        None,
    );
    let output = run_with_timeout(command, PROCESS_TIMEOUT);
    assert!(!output.status.success(), "explicit alias must fail closed");
    assert!(
        output
            .stderr
            .contains("unsupported official DeepSeek model")
            || output
                .stdout
                .contains("unsupported official DeepSeek model"),
        "missing typed capability failure\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    assert_eq!(chat_request_count(&server).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_agent_exec_waits_for_child_handoff_before_one_success_terminal() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(MultiAgentResponder)
        .mount(&server)
        .await;

    let output = run_exec_at(
        &server.uri(),
        20,
        MULTI_AGENT_ROOT_PROMPT,
        "[subagents]\nmax_concurrent = 1\nlaunch_concurrency = 1\nmax_admitted = 1\napi_timeout_secs = 10\n",
        None,
    );
    assert!(
        output.status.success(),
        "multi-agent exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["status"], "completed");
    assert_eq!(metadata["termination_reason"], "resolved");
    // Root spawn reports 19/8 tokens; root wait, child, and parent
    // integration each report 11/3.
    assert_exact_success_accounting(metadata, 4, 52, 17);

    let root_wait_index = events
        .iter()
        .position(|event| {
            event["type"] == "content"
                && event["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(MULTI_AGENT_ROOT_WAIT_MARKER))
        })
        .expect("root turn must visibly finish while the child is still running");
    let parent_integration_index = events
        .iter()
        .position(|event| {
            event["type"] == "content"
                && event["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(MULTI_AGENT_PARENT_MARKER))
        })
        .expect("parent must integrate the child completion in a later turn");
    assert!(
        root_wait_index < parent_integration_index,
        "parent integration appeared before the root waiting turn completed: {events:#?}"
    );
    assert!(
        events[..=parent_integration_index]
            .iter()
            .all(|event| event["type"] != "metadata" && event["type"] != "done"),
        "Headless emitted a terminal receipt before parent integration: {events:#?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "tool_use" && event["name"] == "agent")
            .count(),
        1,
        "root must launch exactly one child: {events:#?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event["type"] == "tool_result"
                    && event["name"] == "agent"
                    && event["status"] == "success"
            })
            .count(),
        1,
        "the agent launch must have one successful tool receipt: {events:#?}"
    );

    let chat_requests = server
        .received_requests()
        .await
        .expect("loopback server request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    let request_kinds = chat_requests
        .iter()
        .map(|request| {
            classify_multi_agent_request(&request.body_json::<Value>().unwrap_or(Value::Null))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        request_kinds.len(),
        4,
        "expected root spawn, root wait, child, and parent integration requests: {request_kinds:?}"
    );
    assert_eq!(request_kinds[0], MultiAgentRequestKind::RootSpawn);
    assert_eq!(
        request_kinds[3],
        MultiAgentRequestKind::ParentIntegration,
        "parent integration must be the final request: {request_kinds:?}"
    );
    assert_eq!(
        request_kinds[1..3]
            .iter()
            .filter(|kind| **kind == MultiAgentRequestKind::RootWait)
            .count(),
        1,
        "root post-tool request missing or duplicated: {request_kinds:?}"
    );
    assert_eq!(
        request_kinds[1..3]
            .iter()
            .filter(|kind| **kind == MultiAgentRequestKind::Child)
            .count(),
        1,
        "child request missing or duplicated: {request_kinds:?}"
    );
    let integration_body = chat_requests[3]
        .body_json::<Value>()
        .expect("parent integration request JSON");
    assert!(
        json_strings_contain(&integration_body, MULTI_AGENT_CHILD_MARKER),
        "parent integration request did not contain the child result: {integration_body:#}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nested_agent_exec_integrates_delayed_grandchild_before_terminal() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(NestedAgentResponder)
        .mount(&server)
        .await;

    let output = run_exec_at(
        &server.uri(),
        20,
        NESTED_ROOT_PROMPT,
        "[subagents]\nmax_concurrent = 2\nlaunch_concurrency = 2\nmax_admitted = 2\nmax_depth = 3\napi_timeout_secs = 10\n",
        None,
    );
    assert!(
        output.status.success(),
        "nested multi-agent exec exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, None);
    assert_eq!(metadata["status"], "completed");
    assert_eq!(metadata["termination_reason"], "resolved");
    // Root spawn: 19/8. Parent spawn: 17/6. The remaining five
    // successful responses each report 11/3.
    assert_exact_success_accounting(metadata, 7, 91, 29);

    let root_wait_index = content_event_index(&events, NESTED_ROOT_WAIT_MARKER);
    let root_integration_index = content_event_index(&events, NESTED_ROOT_INTEGRATED_MARKER);
    assert!(
        root_wait_index < root_integration_index,
        "root integrated the tree before its deliberately early turn: {events:#?}"
    );
    assert!(
        events[..=root_integration_index]
            .iter()
            .all(|event| event["type"] != "metadata" && event["type"] != "done"),
        "terminal receipt appeared before nested tree integration: {events:#?}"
    );

    let chat_requests = server
        .received_requests()
        .await
        .expect("nested request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    let kinds = chat_requests
        .iter()
        .map(|request| {
            classify_nested_agent_request(&request.body_json::<Value>().unwrap_or(Value::Null))
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds.len(), 7, "unexpected nested request tree: {kinds:?}");
    for expected in [
        NestedAgentRequestKind::RootSpawn,
        NestedAgentRequestKind::RootWait,
        NestedAgentRequestKind::ParentSpawnGrandchild,
        NestedAgentRequestKind::ParentEarlyFinish,
        NestedAgentRequestKind::Grandchild,
        NestedAgentRequestKind::ParentIntegration,
        NestedAgentRequestKind::RootIntegration,
    ] {
        assert_eq!(
            kinds.iter().filter(|kind| **kind == expected).count(),
            1,
            "nested request kind missing or duplicated: {expected:?} in {kinds:?}"
        );
    }
    assert_eq!(
        kinds.last(),
        Some(&NestedAgentRequestKind::RootIntegration),
        "root integration must be the final model request: {kinds:?}"
    );

    let parent_integration = chat_requests
        .iter()
        .find(|request| {
            classify_nested_agent_request(&request.body_json::<Value>().unwrap_or(Value::Null))
                == NestedAgentRequestKind::ParentIntegration
        })
        .expect("parent integration request")
        .body_json::<Value>()
        .expect("parent integration JSON");
    assert!(
        json_strings_contain(&parent_integration, NESTED_GRANDCHILD_MARKER),
        "parent did not receive the delayed grandchild handoff: {parent_integration:#}"
    );
    let root_integration = chat_requests
        .last()
        .expect("root integration request")
        .body_json::<Value>()
        .expect("root integration JSON");
    assert!(
        json_strings_contain(&root_integration, NESTED_PARENT_INTEGRATED_MARKER),
        "root did not receive the fully integrated child result: {root_integration:#}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_sse_eof_is_a_typed_failure_not_success() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(&server, sse_response(partial_sse())).await;

    let output = run_exec(&server, 10, "exercise an incomplete stream");
    assert!(
        !output.status.success(),
        "incomplete SSE must not exit successfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, Some("llm_stream_incomplete"));
    assert_eq!(metadata["status"], "failed");
    assert_ne!(metadata["termination_reason"], "resolved");
    assert_eq!(metadata["api_request_count"], 1);
    assert_eq!(metadata["api_request_completed"], 1);
    assert_eq!(metadata["api_request_in_flight"], 0);
    assert_eq!(metadata["standard_chat_response_count"], 1);
    assert_eq!(metadata["usage_incomplete_responses"], 1);
    assert_eq!(metadata["cost_complete"], false);
    assert_eq!(chat_request_count(&server).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchdog_cancels_a_hung_request_and_emits_one_failure_terminal() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(
        &server,
        sse_response(complete_sse("must-never-complete")).set_delay(Duration::from_secs(10)),
    )
    .await;

    // Debug builds perform enough Agent startup that a short global deadline
    // can expire before the first request reaches the server, especially when
    // this acceptance binary runs all cases under the serial process lock.
    // Eight seconds leaves startup room while the delayed response still
    // guarantees that the production watchdog fires with one request live.
    let output = run_exec(&server, 8, "exercise the headless watchdog");
    assert!(
        !output.status.success(),
        "watchdog timeout must not exit successfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    assert!(
        output.elapsed < Duration::from_secs(22),
        "watchdog did not bound wall time: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.elapsed,
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, Some("exec_watchdog_timeout"));
    assert_eq!(metadata["status"], "failed");
    assert_eq!(metadata["termination_reason"], "timeout");
    assert_eq!(metadata["api_request_count"], 1);
    assert_eq!(metadata["api_request_completed"], 1);
    assert_eq!(metadata["api_request_in_flight"], 0);
    assert_eq!(metadata["billing_unknown_attempts"], 1);
    assert_eq!(chat_request_count(&server).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn established_sse_without_events_hits_typed_stream_stall() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = LoopbackSseServer::start(RawSseMode::EventStall);
    let output = run_exec_at(
        &server.uri(),
        20,
        "exercise the SSE idle timeout",
        "[tui]\nstream_chunk_timeout_secs = 1\n",
        Some(1),
    );

    assert!(
        !output.status.success(),
        "idle SSE must not exit successfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    assert!(
        output.elapsed < Duration::from_secs(18),
        "stream stall did not stop promptly: {:?}, requests={}\nstdout:\n{}\nstderr:\n{}",
        output.elapsed,
        server.chat_requests(),
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, Some("stream_stall"));
    assert_eq!(metadata["status"], "failed");
    assert_ne!(metadata["termination_reason"], "resolved");
    // Initial attempt plus at least one outer retry, capped at three retries.
    // A retry that fails during open/send terminates immediately while still
    // retaining the first stall, so the exact count may stop before four.
    let request_count = metadata["api_request_count"]
        .as_u64()
        .expect("typed request count");
    assert!(
        (2..=4).contains(&request_count),
        "unexpected request ledger count: {request_count}"
    );
    assert_eq!(
        metadata["api_request_completed"],
        metadata["api_request_count"]
    );
    assert_eq!(metadata["api_request_in_flight"], 0);
    assert_eq!(
        metadata["standard_chat_response_count"].as_u64(),
        Some(server.chat_requests() as u64)
    );
    assert!(
        (1..=4).contains(&server.chat_requests()),
        "unexpected stream retry count: {}",
        server.chat_requests()
    );
    assert!(
        server.chat_requests() as u64 <= request_count,
        "fixture cannot parse more requests than the ledger started"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_stall_remains_primary_when_retry_open_fails() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = LoopbackSseServer::start(RawSseMode::StallThenUnavailable);
    let output = run_exec_at(
        &server.uri(),
        20,
        "exercise primary SSE stall retention",
        "[tui]\nstream_chunk_timeout_secs = 1\n",
        Some(1),
    );

    assert!(
        !output.status.success(),
        "idle SSE followed by a failed retry must not exit successfully\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );

    let events = parse_strict_ndjson(&output.stdout);
    let metadata = assert_terminal_tail(&events, Some("stream_stall"));
    assert_eq!(metadata["status"], "failed");
    assert_eq!(metadata["api_request_count"], 2);
    assert_eq!(metadata["api_request_completed"], 2);
    assert_eq!(metadata["api_request_in_flight"], 0);
    // The shared request ledger reserves before reqwest opens the socket, so
    // it is the authority for attempts. Depending on the local scheduler, the
    // retry either fails during open/send (the raw fixture parses one POST) or
    // reaches the fixture and receives its 503 (two POSTs). Both must retain
    // the first typed stream-stall cause.
    assert!(
        (1..=2).contains(&server.chat_requests()),
        "unexpected parsed retry count: {}\nstdout:\n{}\nstderr:\n{}",
        server.chat_requests(),
        output.stdout,
        output.stderr
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_stdout_reader_forces_a_bounded_nonzero_exit() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = MockServer::start().await;
    mount_models(&server).await;
    mount_chat(&server, sse_response(complete_sse("closed-stdout-marker"))).await;
    let (command, _workspace, _home) = prepare_exec(
        &server.uri(),
        10,
        "write to a stdout pipe whose reader has closed",
        "",
        None,
    );

    let output = run_with_unconsumed_stdout(
        command,
        StdoutPipeMode::CloseImmediately,
        Duration::from_secs(20),
    );
    assert!(
        !output.status.success(),
        "closed stdout must not be reported as success\nstderr:\n{}",
        output.stderr
    );
    assert!(
        output.elapsed < Duration::from_secs(15),
        "closed stdout did not terminate promptly: {:?}\nstderr:\n{}",
        output.elapsed,
        output.stderr
    );
    assert_eq!(chat_request_count(&server).await, 1);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_unread_stdout_pipe_still_honors_the_runtime_bound() {
    let _serial = EXEC_TEST_LOCK.lock().await;
    let server = LoopbackSseServer::start(RawSseMode::ContentFlood);
    let (mut command, _workspace, _home) =
        prepare_exec(&server.uri(), 8, "fill an unread stdout pipe", "", None);
    command.env("CODEWHALE_TOOL_SURFACE", "shell-only");

    let output = run_with_unconsumed_stdout(
        command,
        StdoutPipeMode::HoldOpenWithoutReading,
        Duration::from_secs(25),
    );
    assert!(
        !output.status.success(),
        "a full unread stdout pipe must not be reported as success\nstderr:\n{}",
        output.stderr
    );
    assert!(
        output.elapsed < Duration::from_secs(22),
        "full unread stdout pipe exceeded its process bound: {:?}\nstderr:\n{}",
        output.elapsed,
        output.stderr
    );
    assert_eq!(
        server.chat_requests(),
        1,
        "flood request never reached the loopback server\nstderr:\n{}",
        output.stderr
    );
    assert!(
        output.stdout.len() >= 8 * 1024,
        "stdout never accumulated enough flood data to exercise pipe backpressure: {} bytes",
        output.stdout.len()
    );
}

async fn mount_models(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "object": "list",
                    "data": [{ "id": TEST_MODEL, "object": "model" }]
                })),
        )
        .mount(server)
        .await;
}

async fn mount_chat(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(response)
        .mount(server)
        .await;
}

fn run_exec(server: &MockServer, max_runtime_secs: u64, prompt: &str) -> ExecOutput {
    run_exec_at(&server.uri(), max_runtime_secs, prompt, "", None)
}

fn run_exec_at(
    base_url: &str,
    max_runtime_secs: u64,
    prompt: &str,
    extra_config: &str,
    stream_idle_timeout_secs: Option<u64>,
) -> ExecOutput {
    let (command, _workspace, _home) = prepare_exec(
        base_url,
        max_runtime_secs,
        prompt,
        extra_config,
        stream_idle_timeout_secs,
    );
    run_with_timeout(command, PROCESS_TIMEOUT)
}

fn prepare_exec(
    base_url: &str,
    max_runtime_secs: u64,
    prompt: &str,
    extra_config: &str,
    stream_idle_timeout_secs: Option<u64>,
) -> (Command, TempDir, TempDir) {
    prepare_exec_with_options(
        base_url,
        max_runtime_secs,
        prompt,
        extra_config,
        stream_idle_timeout_secs,
        TEST_MODEL,
        None,
        None,
    )
}

fn prepare_resume_exec(
    base_url: &str,
    workspace: &Path,
    home: &Path,
    run_id: &str,
    auto: bool,
    provider: Option<&str>,
) -> Command {
    let config_path = home.join(".codewhale/config.toml");
    assert!(
        config_path.is_file(),
        "resume fixture config does not exist: {}",
        config_path.display()
    );
    let mut command = Command::new(codewhale_tui_binary());
    preserve_host_env(&mut command);
    command
        .current_dir(workspace)
        .arg("--workspace")
        .arg(workspace)
        .arg("--no-project-config")
        .arg("exec");
    if auto {
        command.arg("--auto");
    }
    if let Some(provider) = provider {
        command.arg("--provider").arg(provider);
    }
    command
        .arg("--model")
        .arg(TEST_MODEL)
        .arg("--max-turns")
        .arg("4")
        .arg("--max-runtime-secs")
        .arg("120")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--resume")
        .arg(run_id)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("CODEWHALE_CONFIG_PATH", config_path)
        .env("DEEPSEEK_API_KEY", TEST_KEY)
        .env("CODEWHALE_BASE_URL", base_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn prepare_existing_exec(
    base_url: &str,
    workspace: &Path,
    home: &Path,
    prompt: &str,
    continue_latest: bool,
) -> Command {
    let codewhale_home = home.join(".codewhale");
    let config_path = codewhale_home.join("config.toml");
    assert!(
        config_path.is_file(),
        "shared exec fixture config does not exist: {}",
        config_path.display()
    );
    let mut command = Command::new(codewhale_tui_binary());
    preserve_host_env(&mut command);
    command
        .current_dir(workspace)
        .arg("--workspace")
        .arg(workspace)
        .arg("--no-project-config")
        .arg("exec")
        .arg("--auto")
        .arg("--model")
        .arg(TEST_MODEL)
        .arg("--max-turns")
        .arg("4")
        .arg("--max-runtime-secs")
        .arg("30")
        .arg("--output-format")
        .arg("stream-json");
    if continue_latest {
        command.arg("--continue");
    }
    command
        .arg(prompt)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("CODEWHALE_HOME", &codewhale_home)
        .env("CODEWHALE_CONFIG_PATH", config_path)
        .env("DEEPSEEK_API_KEY", TEST_KEY)
        .env("CODEWHALE_BASE_URL", base_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

async fn create_released_run(store: &StateStore, environment: RunEnvironment) -> RunId {
    let mut request = RunRequest::new("resume mismatch fixture", "fixture system prompt");
    request.model = TEST_MODEL.to_owned();
    request.environment = environment;
    let created = store.create(request).await.expect("create mismatch run");
    let run_id = created.lease.run_id.clone();
    store
        .release(&created.lease)
        .await
        .expect("release mismatch run lease");
    run_id
}

fn prepare_non_agent_exec(
    base_url: &str,
    prompt: &str,
    options: NonAgentExecOptions<'_>,
) -> (Command, TempDir, TempDir) {
    let workspace = TempDir::new().expect("workspace tempdir");
    let home = TempDir::new().expect("home tempdir");
    let config_dir = home.path().join(".codewhale");
    std::fs::create_dir_all(&config_dir).expect("create isolated config dir");
    std::fs::write(config_dir.join("config.toml"), "[retry]\nenabled = false\n")
        .expect("write isolated exec config");

    let mut command = Command::new(codewhale_tui_binary());
    preserve_host_env(&mut command);
    command
        .current_dir(workspace.path())
        .arg("--workspace")
        .arg(workspace.path())
        .arg("--no-project-config")
        .arg("exec")
        .arg("--model")
        .arg(TEST_MODEL);
    if options.json_output {
        command.arg("--json");
    }
    if options.stream_json {
        command.arg("--output-format").arg("stream-json");
    }
    if let Some(tools) = options.allowed_tools {
        command.arg("--allowed-tools").arg(tools);
    }
    if let Some(tools) = options.disallowed_tools {
        command.arg("--disallowed-tools").arg(tools);
    }
    command
        .arg(prompt)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("CODEWHALE_CONFIG_PATH", config_dir.join("config.toml"))
        .env("DEEPSEEK_API_KEY", TEST_KEY)
        .env("CODEWHALE_BASE_URL", base_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    (command, workspace, home)
}

#[allow(clippy::too_many_arguments)]
fn prepare_exec_with_options(
    base_url: &str,
    max_runtime_secs: u64,
    prompt: &str,
    extra_config: &str,
    stream_idle_timeout_secs: Option<u64>,
    model: &str,
    max_api_requests: Option<u32>,
    provider: Option<&str>,
) -> (Command, TempDir, TempDir) {
    let workspace = TempDir::new().expect("workspace tempdir");
    let home = TempDir::new().expect("home tempdir");
    let config_dir = home.path().join(".codewhale");
    std::fs::create_dir_all(&config_dir).expect("create isolated config dir");
    std::fs::write(
        config_dir.join("config.toml"),
        format!("[retry]\nenabled = false\n{extra_config}"),
    )
    .expect("write isolated exec config");

    let mut command = Command::new(codewhale_tui_binary());
    preserve_host_env(&mut command);
    command
        .current_dir(workspace.path())
        .arg("--workspace")
        .arg(workspace.path())
        .arg("--no-project-config")
        .arg("exec")
        .arg("--auto")
        .arg("--model")
        .arg(model)
        .arg("--max-turns")
        .arg("4")
        .arg("--max-runtime-secs")
        .arg(max_runtime_secs.to_string())
        .arg("--output-format")
        .arg("stream-json")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("CODEWHALE_CONFIG_PATH", config_dir.join("config.toml"))
        .env("DEEPSEEK_API_KEY", TEST_KEY)
        .env("CODEWHALE_BASE_URL", base_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(limit) = max_api_requests {
        command.arg("--max-api-requests").arg(limit.to_string());
    }
    if let Some(provider) = provider {
        command.arg("--provider").arg(provider);
    }
    command.arg(prompt);
    if let Some(timeout) = stream_idle_timeout_secs {
        command.env("DEEPSEEK_STREAM_IDLE_TIMEOUT_SECS", timeout.to_string());
    }

    (command, workspace, home)
}

fn run_with_timeout(mut command: Command, timeout: Duration) -> ExecOutput {
    let started = Instant::now();
    let mut child = command.spawn().expect("spawn codewhale-tui exec");
    let stdout_reader = read_pipe(child.stdout.take().expect("stdout pipe"));
    let stderr_reader = read_pipe(child.stderr.take().expect("stderr pipe"));

    let status = match child.wait_timeout(timeout).expect("wait for exec process") {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = join_pipe(stdout_reader, "stdout");
            let stderr = join_pipe(stderr_reader, "stderr");
            panic!(
                "exec process exceeded {timeout:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
    };

    ExecOutput {
        status,
        stdout: String::from_utf8(join_pipe(stdout_reader, "stdout"))
            .expect("stdout must be UTF-8"),
        stderr: String::from_utf8(join_pipe(stderr_reader, "stderr"))
            .expect("stderr must be UTF-8"),
        elapsed: started.elapsed(),
    }
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum StdoutPipeMode {
    CloseImmediately,
    HoldOpenWithoutReading,
}

#[cfg(unix)]
fn run_with_unconsumed_stdout(
    mut command: Command,
    mode: StdoutPipeMode,
    timeout: Duration,
) -> ExecOutput {
    let started = Instant::now();
    let mut child = command.spawn().expect("spawn exec with unconsumed stdout");
    let stdout = child.stdout.take().expect("stdout pipe");
    let held_stdout = match mode {
        StdoutPipeMode::CloseImmediately => {
            drop(stdout);
            None
        }
        StdoutPipeMode::HoldOpenWithoutReading => Some(stdout),
    };
    let stderr_reader = read_pipe(child.stderr.take().expect("stderr pipe"));

    let status = match child.wait_timeout(timeout).expect("wait for exec process") {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            drop(held_stdout);
            let stderr = join_pipe(stderr_reader, "stderr");
            panic!(
                "exec with unconsumed stdout exceeded {timeout:?}\nstderr:\n{}",
                String::from_utf8_lossy(&stderr)
            );
        }
    };
    let stdout = held_stdout.map_or_else(String::new, |mut pipe| {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)
            .expect("read unread stdout only after child exit");
        String::from_utf8(bytes).expect("stdout must be UTF-8")
    });

    ExecOutput {
        status,
        stdout,
        stderr: String::from_utf8(join_pipe(stderr_reader, "stderr"))
            .expect("stderr must be UTF-8"),
        elapsed: started.elapsed(),
    }
}

fn read_pipe<R>(mut reader: R) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map(|_| bytes)
    })
}

fn join_pipe(handle: std::thread::JoinHandle<std::io::Result<Vec<u8>>>, name: &str) -> Vec<u8> {
    handle
        .join()
        .unwrap_or_else(|_| panic!("{name} reader thread panicked"))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn parse_strict_ndjson(stdout: &str) -> Vec<Value> {
    let lines = stdout
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert!(!lines.is_empty(), "exec emitted no NDJSON events");

    lines
        .into_iter()
        .map(|(index, line)| {
            assert_eq!(
                line,
                line.trim(),
                "stdout line {} contains non-JSON padding: {line:?}",
                index + 1
            );
            let event: Value = serde_json::from_str(line).unwrap_or_else(|error| {
                panic!(
                    "stdout line {} is not strict JSON: {error}\nline: {line:?}\nstdout:\n{stdout}",
                    index + 1
                )
            });
            assert_eq!(event["schema"], "codewhale.exec-stream");
            assert_eq!(event["schema_version"], 1);
            assert!(
                event["type"].is_string(),
                "stdout line {} has no event type: {event:#}",
                index + 1
            );
            event
        })
        .collect()
}

fn content_events(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["type"] == "content")
        .filter_map(|event| event["content"].as_str().map(str::to_owned))
        .collect()
}

fn terminal_event_count(events: &[StoredRuntimeEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(&event.event, RuntimeEventKind::Terminal { .. }))
        .count()
}

fn assert_startup_failure_contains(events: &[Value], marker: &str) {
    assert_eq!(
        events.len(),
        3,
        "startup mismatch must emit one terminal tail: {events:#?}"
    );
    assert_eq!(events[0]["type"], "metadata");
    assert_eq!(events[0]["meta"]["receipt_kind"], "startup_failure");
    assert_eq!(events[0]["meta"]["status"], "failed");
    assert_eq!(events[1]["type"], "error");
    assert_eq!(
        events[1]["code"], marker,
        "startup failure machine code is not precise: {events:#?}"
    );
    assert!(
        events[1]["error"]
            .as_str()
            .is_some_and(|error| error.contains(marker)),
        "startup failure did not preserve {marker}: {events:#?}"
    );
    assert_eq!(events[2]["type"], "done");
}

fn assert_terminal_tail<'a>(events: &'a [Value], error_code: Option<&str>) -> &'a Value {
    let metadata = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["type"] == "metadata")
        .collect::<Vec<_>>();
    let errors = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["type"] == "error")
        .collect::<Vec<_>>();
    let done = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["type"] == "done")
        .collect::<Vec<_>>();

    assert_eq!(metadata.len(), 1, "metadata must be unique: {events:#?}");
    assert_eq!(done.len(), 1, "done must be unique: {events:#?}");
    assert_eq!(done[0].0, events.len() - 1, "done must be the last event");
    assert_eq!(metadata[0].1["meta"]["receipt_kind"], "terminal");

    match error_code {
        None => {
            assert!(errors.is_empty(), "success emitted an error: {events:#?}");
            assert_eq!(metadata[0].0 + 1, done[0].0);
        }
        Some(expected_code) => {
            assert_eq!(errors.len(), 1, "failure error must be unique: {events:#?}");
            assert_eq!(
                errors[0].1["code"], expected_code,
                "unexpected terminal error code: {events:#?}"
            );
            assert_eq!(metadata[0].0 + 1, errors[0].0);
            assert_eq!(errors[0].0 + 1, done[0].0);
        }
    }

    &metadata[0].1["meta"]
}

fn assert_startup_tail<'a>(events: &'a [Value], error_code: &str) -> &'a Value {
    assert_eq!(
        events.len(),
        3,
        "startup tail must contain exactly 3 events"
    );
    assert_eq!(events[0]["type"], "metadata");
    assert_eq!(events[0]["meta"]["receipt_kind"], "startup_failure");
    assert_eq!(events[1]["type"], "error");
    assert_eq!(events[1]["code"], error_code);
    assert_eq!(events[2]["type"], "done");
    &events[0]["meta"]
}

fn assert_exact_success_accounting(
    metadata: &Value,
    expected_requests: u64,
    expected_input_tokens: u64,
    expected_output_tokens: u64,
) {
    assert_eq!(metadata["api_request_count"], expected_requests);
    assert_eq!(metadata["api_request_completed"], expected_requests);
    assert_eq!(metadata["api_request_in_flight"], 0);
    assert_eq!(metadata["transport_retry_count"], 0);
    assert_eq!(metadata["api_request_budget_exhausted"], false);
    assert_eq!(metadata["api_request_rejected_exhausted"], 0);
    assert_eq!(metadata["input_tokens"], expected_input_tokens);
    assert_eq!(metadata["output_tokens"], expected_output_tokens);
    assert_eq!(
        metadata["total_tokens"],
        expected_input_tokens + expected_output_tokens
    );
    assert_eq!(metadata["usage_response_count"], expected_requests);
    assert_eq!(metadata["standard_chat_response_count"], expected_requests);
    assert_eq!(metadata["strict_chat_response_count"], 0);
    assert_eq!(metadata["fim_response_count"], 0);
    assert_eq!(metadata["usage_missing_responses"], 0);
    assert_eq!(metadata["usage_incomplete_responses"], 0);
    assert_eq!(metadata["billing_unknown_attempts"], 0);
    assert_eq!(metadata["unpriced_usage_responses"], 0);
    assert_eq!(metadata["usage_complete"], true);
    assert_eq!(metadata["cost_complete"], true);
    assert!(metadata["cost_usd"].as_f64().is_some_and(|cost| cost > 0.0));
    assert!(metadata["cost_cny"].as_f64().is_some_and(|cost| cost > 0.0));

    let buckets = metadata["surface_model_usage_buckets"]
        .as_array()
        .expect("surface/model accounting buckets");
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0]["model"], TEST_MODEL);
    assert_eq!(buckets[0]["api_surface"], "standard_chat");
    assert_eq!(buckets[0]["response_count"], expected_requests);
    assert_eq!(buckets[0]["usage_response_count"], expected_requests);
    assert_eq!(buckets[0]["input_tokens"], expected_input_tokens);
    assert_eq!(buckets[0]["output_tokens"], expected_output_tokens);
}

async fn chat_request_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .expect("loopback server request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .count()
}

/// Minimal raw HTTP server for states that WireMock cannot model precisely:
/// an established stream with no events, and an unbounded content stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RawSseMode {
    EventStall,
    StallThenUnavailable,
    ContentFlood,
}

struct LoopbackSseServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    chat_requests: Arc<AtomicUsize>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LoopbackSseServer {
    fn start(mode: RawSseMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled SSE server");
        let address = listener.local_addr().expect("stalled SSE server address");
        listener
            .set_nonblocking(true)
            .expect("make stalled SSE listener nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let chat_requests = Arc::new(AtomicUsize::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_requests = Arc::clone(&chat_requests);
        let worker = std::thread::spawn(move || {
            let mut handlers = Vec::new();
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler_stop = Arc::clone(&worker_stop);
                        let handler_requests = Arc::clone(&worker_requests);
                        handlers.push(std::thread::spawn(move || {
                            serve_loopback_sse_connection(
                                stream,
                                &handler_stop,
                                &handler_requests,
                                mode,
                            );
                        }));
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) if worker_stop.load(Ordering::Acquire) => break,
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            for handler in handlers {
                handler.join().expect("stalled SSE connection thread");
            }
        });

        Self {
            address,
            stop,
            chat_requests,
            worker: Some(worker),
        }
    }

    fn uri(&self) -> String {
        format!("http://{}", self.address)
    }

    fn chat_requests(&self) -> usize {
        self.chat_requests.load(Ordering::Acquire)
    }
}

impl Drop for LoopbackSseServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("stalled SSE server thread");
        }
    }
}

fn serve_loopback_sse_connection(
    mut stream: TcpStream,
    stop: &AtomicBool,
    chat_requests: &AtomicUsize,
    mode: RawSseMode,
) {
    stream
        // A full workspace test run can heavily deschedule this raw fixture
        // while the child process is still writing its request headers. Two
        // seconds occasionally closed an otherwise valid first connection and
        // changed the asserted stream-stall into an unrelated network error.
        // Keep the bound finite for Drop/join safety, but outside the local
        // scheduler jitter observed under all-target test load.
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set stalled SSE read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("set loopback SSE write timeout");
    let reader_stream = stream.try_clone().expect("clone stalled SSE stream");
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let mut content_length = 0_usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || matches!(header.as_str(), "\r\n" | "\n" | "") {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let Ok(parsed) = value.trim().parse::<usize>() else {
                return;
            };
            content_length = parsed;
        }
    }
    // Consume the complete request before writing or closing the response.
    // Closing a socket with an unread POST body can produce a TCP reset on
    // macOS, which turns the intended SSE stall/503 fixture into a scheduler-
    // dependent reqwest send error.
    if content_length > 0 {
        let mut body = vec![0_u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
    }

    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method == "GET" && path == "/v1/models" {
        let body = serde_json::to_vec(&json!({
            "object": "list",
            "data": [{ "id": TEST_MODEL, "object": "model" }]
        }))
        .expect("serialize models response");
        write_http_response(&mut stream, "application/json", &body);
        return;
    }
    if method != "POST" || !path.ends_with("/chat/completions") {
        write_http_response(&mut stream, "text/plain", b"not found");
        return;
    }

    let request_number = chat_requests.fetch_add(1, Ordering::AcqRel) + 1;
    if mode == RawSseMode::StallThenUnavailable && request_number > 1 {
        let body = b"temporarily unavailable";
        let response = format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream
            .write_all(response.as_bytes())
            .and_then(|()| stream.write_all(body))
            .and_then(|()| stream.flush());
        return;
    }
    if stream
        .write_all(
            b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Transfer-Encoding: chunked\r\n\
Connection: close\r\n\
\r\n",
        )
        .and_then(|()| stream.flush())
        .is_err()
    {
        return;
    }

    match mode {
        RawSseMode::EventStall | RawSseMode::StallThenUnavailable => {
            serve_event_stall(&mut stream, stop);
        }
        RawSseMode::ContentFlood => serve_content_flood(&mut stream, stop),
    }
}

fn serve_event_stall(stream: &mut TcpStream, stop: &AtomicBool) {
    // Keep transport bytes flowing so the HTTP decoder's byte-idle timeout
    // does not fire. SSE comments are complete protocol frames but produce no
    // model event, allowing Runtime's per-event stall guard to own the typed
    // outcome without accumulating a permanently partial line.
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline && !stop.load(Ordering::Acquire) {
        if write_chunked_payload(stream, b": keepalive\n\n").is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn serve_content_flood(stream: &mut TcpStream, stop: &AtomicBool) {
    let content = "x".repeat(32 * 1024);
    let event = sse_chunk(json!({
        "id": "chatcmpl-output-flood",
        "object": "chat.completion.chunk",
        "model": TEST_MODEL,
        "choices": [{
            "index": 0,
            "delta": { "content": content },
            "finish_reason": null
        }]
    }));
    while !stop.load(Ordering::Acquire) {
        if write_chunked_payload(stream, event.as_bytes()).is_err() {
            break;
        }
    }
}

fn write_chunked_payload(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    write!(stream, "{:X}\r\n", payload.len())?;
    stream.write_all(payload)?;
    stream.write_all(b"\r\n")?;
    stream.flush()
}

fn write_http_response(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write HTTP response head");
    stream.write_all(body).expect("write HTTP response body");
    stream.flush().expect("flush HTTP response");
}

fn classify_multi_agent_request(body: &Value) -> MultiAgentRequestKind {
    if json_strings_contain(body, MULTI_AGENT_CHILD_SYSTEM_MARKER) {
        MultiAgentRequestKind::Child
    } else if json_strings_contain(body, MULTI_AGENT_HANDOFF_MARKER) {
        MultiAgentRequestKind::ParentIntegration
    } else if json_strings_contain(body, MULTI_AGENT_SPAWN_CALL_ID) {
        MultiAgentRequestKind::RootWait
    } else if json_strings_contain(body, MULTI_AGENT_ROOT_PROMPT) {
        MultiAgentRequestKind::RootSpawn
    } else {
        MultiAgentRequestKind::Unexpected
    }
}

fn classify_nested_agent_request(body: &Value) -> NestedAgentRequestKind {
    if json_strings_contain(body, NESTED_GRANDCHILD_PROMPT)
        && !json_strings_contain(body, NESTED_PARENT_PROMPT)
    {
        NestedAgentRequestKind::Grandchild
    } else if json_strings_contain(body, MULTI_AGENT_CHILD_SYSTEM_MARKER) {
        if json_strings_contain(body, NESTED_CHILD_HANDOFF_MARKER) {
            NestedAgentRequestKind::ParentIntegration
        } else if json_strings_contain(body, NESTED_GRANDCHILD_CALL_ID) {
            NestedAgentRequestKind::ParentEarlyFinish
        } else if json_strings_contain(body, NESTED_PARENT_PROMPT) {
            NestedAgentRequestKind::ParentSpawnGrandchild
        } else {
            NestedAgentRequestKind::Unexpected
        }
    } else if json_strings_contain(body, MULTI_AGENT_HANDOFF_MARKER)
        && json_strings_contain(body, NESTED_PARENT_INTEGRATED_MARKER)
    {
        NestedAgentRequestKind::RootIntegration
    } else if json_strings_contain(body, NESTED_ROOT_SPAWN_CALL_ID) {
        NestedAgentRequestKind::RootWait
    } else if json_strings_contain(body, NESTED_ROOT_PROMPT) {
        NestedAgentRequestKind::RootSpawn
    } else {
        NestedAgentRequestKind::Unexpected
    }
}

fn assert_non_streaming(body: &Value, label: &str) {
    assert!(
        !body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        "{label} request must use the non-streaming contract: {body:#}"
    );
}

fn content_event_index(events: &[Value], marker: &str) -> usize {
    events
        .iter()
        .position(|event| {
            event["type"] == "content"
                && event["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(marker))
        })
        .unwrap_or_else(|| panic!("missing content marker {marker}: {events:#?}"))
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

fn agent_spawn_sse() -> String {
    let arguments = json!({
        "name": "exec_acceptance_child",
        "prompt": format!("Return {MULTI_AGENT_CHILD_MARKER} exactly, then stop."),
        "type": "explore",
        "fork_context": false,
        "max_steps": 2,
        "wall_time_secs": 10
    })
    .to_string();

    [
        sse_chunk(json!({
            "id": "chatcmpl-multi-agent-spawn",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": MULTI_AGENT_SPAWN_CALL_ID,
                        "type": "function",
                        "function": {
                            "name": "agent",
                            "arguments": arguments
                        }
                    }]
                },
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-multi-agent-spawn",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 19,
                "completion_tokens": 8,
                "total_tokens": 27,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 19
            }
        })),
        "data: [DONE]\n\n".to_string(),
    ]
    .join("")
}

fn nested_root_spawn_sse() -> String {
    let arguments = json!({
        "name": "exec_nested_parent",
        "prompt": NESTED_PARENT_PROMPT,
        "type": "general",
        "fork_context": false,
        "max_steps": 3,
        "wall_time_secs": 12
    })
    .to_string();

    [
        sse_chunk(json!({
            "id": "chatcmpl-nested-root-spawn",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": NESTED_ROOT_SPAWN_CALL_ID,
                        "type": "function",
                        "function": { "name": "agent", "arguments": arguments }
                    }]
                },
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-nested-root-spawn",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }],
            "usage": {
                "prompt_tokens": 19,
                "completion_tokens": 8,
                "total_tokens": 27,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 19
            }
        })),
        "data: [DONE]\n\n".to_string(),
    ]
    .join("")
}

fn nested_parent_spawn_response() -> ResponseTemplate {
    let arguments = json!({
        "name": "exec_delayed_grandchild",
        "prompt": NESTED_GRANDCHILD_PROMPT,
        "type": "explore",
        "workspace_policy": "shared",
        "write_authority": "read_only",
        "expected_artifact": "nested production evidence",
        "deliberate": true,
        "fork_context": false,
        "max_steps": 2,
        "wall_time_secs": 10
    })
    .to_string();
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-nested-parent-spawn",
        "object": "chat.completion",
        "model": TEST_MODEL,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": NESTED_GRANDCHILD_CALL_ID,
                    "type": "function",
                    "function": { "name": "agent", "arguments": arguments }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 17,
            "completion_tokens": 6,
            "total_tokens": 23,
            "prompt_cache_hit_tokens": 0,
            "prompt_cache_miss_tokens": 17
        }
    }))
}

fn hallucinated_unauthorized_write_sse() -> String {
    let arguments = json!({
        "patch": format!(
            "*** Begin Patch\n*** Add File: {UNAUTHORIZED_WRITE_PATH}\n+unauthorized\n*** End Patch"
        )
    })
    .to_string();

    [
        sse_chunk(json!({
            "id": "chatcmpl-unauthorized-write",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": UNAUTHORIZED_WRITE_CALL_ID,
                        "type": "function",
                        "function": {
                            "name": "apply_patch",
                            "arguments": arguments
                        }
                    }]
                },
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-unauthorized-write",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 13,
                "completion_tokens": 5,
                "total_tokens": 18,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 13
            }
        })),
        "data: [DONE]\n\n".to_string(),
    ]
    .join("")
}

fn complete_sse(content: &str) -> String {
    [
        sse_chunk(json!({
            "id": "chatcmpl-terminal-acceptance",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": { "content": content },
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-terminal-acceptance",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 3,
                "total_tokens": 14,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 11
            }
        })),
        "data: [DONE]\n\n".to_string(),
    ]
    .join("")
}

fn non_streaming_response(content: &str) -> ResponseTemplate {
    non_streaming_response_with_usage(content, 11, 3)
}

fn non_streaming_response_with_usage(
    content: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-terminal-acceptance-non-streaming",
        "object": "chat.completion",
        "model": TEST_MODEL,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
            "prompt_cache_hit_tokens": 0,
            "prompt_cache_miss_tokens": prompt_tokens
        }
    }))
}

fn partial_sse() -> String {
    sse_chunk(json!({
        "id": "chatcmpl-incomplete",
        "object": "chat.completion.chunk",
        "model": TEST_MODEL,
        "choices": [{
            "index": 0,
            "delta": { "content": "partial-content-without-terminal" },
            "finish_reason": null
        }]
    }))
}

fn sse_chunk(value: Value) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(&value).expect("serialize SSE event")
    )
}

fn sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .insert_header("cache-control", "no-cache")
        .set_body_string(body)
}

fn preserve_host_env(command: &mut Command) {
    command.env_clear();
    for key in [
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "SystemDrive",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "TERM",
        "COLORTERM",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn codewhale_tui_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_codewhale-tui") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_codewhale-tui") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().expect("current test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(format!("codewhale-tui{}", std::env::consts::EXE_SUFFIX));
    path
}
