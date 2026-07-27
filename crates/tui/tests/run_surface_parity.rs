//! Production acceptance proof that exec, HTTP, and stdio are thin clients of
//! one canonical run contract.
//!
//! The fixture admits one fixed read-only tool call and then completes. Exec
//! runs as the real binary and is read back from its SQLite RunStore. HTTP and
//! stdio share one production `AgentApplication` and cross the real app-server
//! transports. Transport event lists must be byte-for-byte equivalent to the
//! Store; cross-surface comparison removes only generated identities and
//! timestamps.

use std::collections::BTreeMap;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header::ACCEPT, header::CONTENT_TYPE};
use dse_app::{
    AgentApplication, DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
    ProductionPromptConfig, ProductionToolConfig, ShellPolicy,
};
use dse_app_server::{AppServerOptions, router, serve_stdio};
use dse_config::PromptPreferences;
use dse_protocol::agent_runtime::{
    AGENT_RUNTIME_EVENT_SCHEMA_VERSION, ActorRequestAccounting, AgentOutcome, AgentResultDetails,
    AgentTask, AgentTaskId, AgentWorkspaceAccess, AgentWorkspaceAssignment, ContextPolicy,
    ModelAccounting, ModelRouteAudit, ModelRouteProfile, OperationId, ReasoningEffort, RunId,
    RunPermissionMode, RunRequest, RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent,
    TerminalState, ToolArtifact, ToolArtifactStatus, ToolPolicy, Usage, WriterArtifactState,
    WriterCleanupMode, WriterCleanupOwnership, WriterCleanupPhase, WriterCleanupPlan,
    WriterCleanupResult, WriterCleanupScope, WriterIntegrationStatus, WriterRemovalState,
    writer_path_set_sha256,
};
use dse_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunCommand, RunCommandEnvelope, RunCommandResponse, RunCommandResult,
    RunProductControls, RunView, StartRunCommand,
};
use dse_protocol::task::{
    AcceptanceId, AcceptanceSatisfaction, CompletionCandidateId, CompletionDecision,
    EvidenceLineage, EvidenceReceipt, EvidenceReceiptId, TaskAcceptance, TaskContract,
    TaskDefinition, TaskGenerationId, VerificationId, VerifierEvidencePolicy, VerifierPlan,
    VerifierSpec, VerifierStep, WorkspaceRevision, WorkspaceState,
};
use dse_runtime::{RunReplay, RunStore};
use dse_state::StateStore;
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;
use wait_timeout::ChildExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request as ModelRequest, Respond, ResponseTemplate};

#[path = "../src/exec_lifecycle_stream.rs"]
mod exec_lifecycle_stream;

const TEST_MODEL: &str = "deepseek-v4-flash";
const TEST_KEY: &str = "offline-run-surface-parity-key";
const TEST_PROMPT: &str = "读取 fixture.txt，并根据文件内容给出固定结论。";
const TOOL_CALL_ID: &str = "call_surface_parity_read";
const FINAL_MESSAGE: &str = "三个入口读取到了同一份内容";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const WRITER_BASE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const WRITER_FINAL_COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WRITER_DIFF_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[derive(Clone, Copy)]
struct ReadThenComplete;

impl Respond for ReadThenComplete {
    fn respond(&self, request: &ModelRequest) -> ResponseTemplate {
        let body = request.body_json::<Value>().expect("DeepSeek request JSON");
        if json_strings_contain(&body, TOOL_CALL_ID) {
            sse_response(final_sse())
        } else {
            let tools = body["tools"].as_array().expect("production tool catalog");
            assert_eq!(
                tools.len(),
                1,
                "only read_file may reach the model: {body:#}"
            );
            assert_eq!(tools[0]["function"]["name"], "read_file");
            sse_response(read_file_sse())
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exec_http_and_stdio_preserve_one_canonical_run() {
    let model = MockServer::start().await;
    mount_model_fixture(&model).await;

    let workspace = TempDir::new().expect("temporary parity workspace");
    std::fs::write(
        workspace.path().join("fixture.txt"),
        "surface-parity-fixture\n",
    )
    .expect("write read-only tool fixture");
    let exec_home = TempDir::new().expect("isolated exec home");
    let config_dir = prepare_exec_config(exec_home.path());

    let exec = run_exec(
        &model.uri(),
        workspace.path(),
        exec_home.path(),
        &config_dir.join("config.toml"),
    );
    assert!(
        exec.status.success(),
        "production exec failed\nstdout:\n{}\nstderr:\n{}",
        exec.stdout,
        exec.stderr
    );
    let exec_run_id = terminal_run_id(&exec.stdout);
    let exec_state_path = config_dir.join("state.db");
    let exec_replay = load_replay(&exec_state_path, &exec_run_id).await;
    assert_fixture_event_sequence(&exec_replay.events);
    let exec_request = run_created_request(&exec_replay.events).clone();
    assert_exec_command_contract(&exec_request, workspace.path());

    let shared_state_path = exec_home.path().join("app-surface-state.db");
    let application = production_application(
        &shared_state_path,
        &model.uri(),
        workspace.path(),
        &config_dir.join("skills"),
    );
    let http = router(application.clone(), &transport_options()).expect("HTTP Run API router");
    let start = equivalent_start_command(&exec_request);

    let http_start = http_post(
        &http,
        "/v1/runs",
        &envelope("http-start", RunCommand::Start(start.clone())),
    )
    .await;
    let http_run_id = run_from_response(http_start).run_id;
    let http_view = wait_http_terminal(&http, &http_run_id).await;
    let http_events = http_sse_events(&http, &http_run_id).await;
    let http_replay = load_replay(&shared_state_path, &http_run_id).await;
    assert_eq!(
        http_events, http_replay.events,
        "HTTP omitted or invented canonical events"
    );
    assert_eq!(http_view, view_from_replay(&http_replay));

    let stdio_start = stdio_command(
        application.clone(),
        envelope("stdio-start", RunCommand::Start(start)),
    )
    .await;
    let stdio_run_id = run_from_response(stdio_start).run_id;
    let stdio_view = wait_stdio_terminal(application.clone(), &stdio_run_id).await;
    let stdio_events = events_from_response(
        stdio_command(
            application,
            envelope(
                "stdio-events",
                RunCommand::Events {
                    run_id: stdio_run_id.clone(),
                    after_sequence: 0,
                },
            ),
        )
        .await,
    );
    let stdio_replay = load_replay(&shared_state_path, &stdio_run_id).await;
    assert_eq!(
        stdio_events, stdio_replay.events,
        "stdio omitted or invented canonical events"
    );
    assert_eq!(stdio_view, view_from_replay(&stdio_replay));

    assert_canonical_event_parity(&exec_replay.events, &http_replay.events, "exec versus HTTP");
    assert_canonical_event_parity(
        &exec_replay.events,
        &stdio_replay.events,
        "exec versus stdio",
    );

    let exec_view = view_from_replay(&exec_replay);
    assert_eq!(normalize_value(&exec_view), normalize_value(&http_view));
    assert_eq!(normalize_value(&exec_view), normalize_value(&stdio_view));
    assert_terminal_accounting(&view_from_replay(&exec_replay));

    let requests = model
        .received_requests()
        .await
        .expect("DeepSeek request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .map(|request| request.body_json::<Value>().expect("DeepSeek request body"))
        .collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        6,
        "each surface must make exactly two requests"
    );
    assert_eq!(
        normalize_value(&requests[0]),
        normalize_value(&requests[2]),
        "exec/HTTP first request drifted"
    );
    assert_eq!(
        normalize_value(&requests[0]),
        normalize_value(&requests[4]),
        "exec/stdio first request drifted"
    );
    assert_eq!(
        normalize_value(&requests[1]),
        normalize_value(&requests[3]),
        "exec/HTTP replay request drifted"
    );
    assert_eq!(
        normalize_value(&requests[1]),
        normalize_value(&requests[5]),
        "exec/stdio replay request drifted"
    );
}

#[test]
fn writer_lifecycle_stream_preserves_child_identity_outcome_accounting_and_handoff() {
    let events = writer_lifecycle_fixture();
    let stdout = events
        .iter()
        .map(|event| {
            String::from_utf8(
                exec_lifecycle_stream::agent_lifecycle_stream_line(event)
                    .expect("serialize production lifecycle stream line"),
            )
            .expect("lifecycle stream is UTF-8")
        })
        .collect::<String>();

    assert_exec_agent_lifecycle_parity(&stdout, &events);

    let values = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("strict lifecycle NDJSON"))
        .collect::<Vec<_>>();
    let child_started = values
        .iter()
        .find(|value| value["runtime_event"]["event"]["kind"] == "child_started")
        .expect("exact ChildStarted stream event");
    let child_finished = values
        .iter()
        .find(|value| value["runtime_event"]["event"]["kind"] == "child_finished")
        .expect("exact ChildFinished stream event");
    let expected_started = events
        .iter()
        .find(|event| matches!(event.event, RuntimeEventKind::ChildStarted { .. }))
        .expect("fixture ChildStarted");
    let expected_finished = events
        .iter()
        .find(|event| matches!(event.event, RuntimeEventKind::ChildFinished { .. }))
        .expect("fixture ChildFinished");

    assert_eq!(
        child_started["runtime_event"],
        serde_json::to_value(expected_started).expect("serialize expected ChildStarted")
    );
    assert_eq!(
        child_finished["runtime_event"],
        serde_json::to_value(expected_finished).expect("serialize expected ChildFinished")
    );
    assert_eq!(
        child_finished["runtime_event"]["event"]["handoff_content"],
        "writer 已集成；父 Agent 可继续使用变更"
    );
    assert_eq!(
        child_finished["runtime_event"]["event"]["outcome"]["details"]["changed_files"],
        json!(["src/lib.rs"])
    );
    assert_eq!(
        child_finished["runtime_event"]["event"]["outcome"]["details"]["integration"]["state"],
        "integrated"
    );
    assert_eq!(
        child_finished["runtime_event"]["event"]["outcome"]["accounting"]["usage"]["input_tokens"],
        101
    );
    assert_eq!(
        child_finished["runtime_event"]["event"]["accounting"]["child"]["started"],
        3
    );
}

struct ExecResult {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn prepare_exec_config(home: &Path) -> PathBuf {
    let config_dir = home.join(".dse");
    std::fs::create_dir_all(config_dir.join("skills")).expect("create isolated config");
    std::fs::write(
        config_dir.join("config.toml"),
        "[subagents]\nenabled = false\n",
    )
    .expect("write isolated config");
    config_dir
}

fn run_exec(base_url: &str, workspace: &Path, home: &Path, config: &Path) -> ExecResult {
    let mut command = Command::new(dse_tui_binary());
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
        .arg("--allowed-tools")
        .arg("read_file")
        .arg("--max-turns")
        .arg("4")
        .arg("--max-api-requests")
        .arg("4")
        .arg("--max-runtime-secs")
        .arg("60")
        .arg("--output-format")
        .arg("stream-json")
        .arg(TEST_PROMPT)
        .env("DSE_HOME", home.join(".dse"))
        .env("DSE_CONFIG_PATH", config)
        .env("DEEPSEEK_API_KEY", TEST_KEY)
        .env("DSE_BASE_URL", base_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("spawn production exec");
    let stdout = child.stdout.take().expect("exec stdout");
    let stderr = child.stderr.take().expect("exec stderr");
    let stdout = std::thread::spawn(move || read_all(stdout));
    let stderr = std::thread::spawn(move || read_all(stderr));
    let status = match child
        .wait_timeout(PROCESS_TIMEOUT)
        .expect("wait for production exec")
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("production exec exceeded {PROCESS_TIMEOUT:?}");
        }
    };
    ExecResult {
        status,
        stdout: String::from_utf8(stdout.join().expect("stdout reader")).expect("UTF-8 stdout"),
        stderr: String::from_utf8(stderr.join().expect("stderr reader")).expect("UTF-8 stderr"),
    }
}

fn read_all(mut reader: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("read child pipe");
    bytes
}

fn terminal_run_id(stdout: &str) -> RunId {
    stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("strict exec NDJSON"))
        .find_map(|line| {
            (line["type"] == "metadata" && line["meta"]["receipt_kind"] == "terminal")
                .then(|| line["meta"]["run_id"].as_str().map(str::to_owned))
                .flatten()
        })
        .map(RunId::from)
        .expect("exec terminal receipt run id")
}

fn production_application(
    state_path: &Path,
    base_url: &str,
    workspace: &Path,
    skills_dir: &Path,
) -> Arc<AgentApplication> {
    let connection = DeepSeekConnectionConfig {
        endpoint: DeepSeekEndpoint::loopback_fixture(format!("{base_url}/v1"))
            .expect("loopback DeepSeek endpoint"),
        strict_tools: false,
        response_header_timeout: Duration::from_secs(45),
        stream_idle_timeout: Duration::from_secs(900),
    };
    let tools = ProductionToolConfig::new(workspace)
        .with_permission_mode(RunPermissionMode::Agent)
        .with_shell_policy(ShellPolicy::Full);
    let prompt = ProductionPromptConfig {
        preferences: PromptPreferences::default(),
        instructions: Vec::new(),
        skills_dir: Some(skills_dir.to_path_buf()),
        verbosity: None,
        skills_scan_dse_only: false,
        shell_binary: dse_tools::shell_dispatcher::global_dispatcher()
            .kind()
            .binary()
            .to_owned(),
    };
    let config = ProductionApplicationConfig::official()
        .with_state_db_path(state_path)
        .with_deepseek_connection(connection)
        .with_tool_config(tools)
        .with_prompt(prompt)
        .with_default_max_api_requests(NonZeroU32::new(u32::MAX).expect("non-zero request limit"))
        .with_api_key(TEST_KEY)
        .expect("fixture credential");
    Arc::new(AgentApplication::production(config).expect("production AgentApplication"))
}

fn equivalent_start_command(exec: &RunRequest) -> StartRunCommand {
    StartRunCommand {
        task: TaskDefinition::host(TEST_PROMPT),
        workspace: exec.environment.workspace.clone(),
        model: Some(TEST_MODEL.to_owned()),
        reasoning_effort: exec.reasoning_effort,
        max_output_tokens: Some(384_000),
        max_api_requests: NonZeroU32::new(4),
        streaming: true,
        tool_policy: exec.tool_policy.clone(),
        limits: exec.limits,
        controls: RunProductControls {
            write_execution_mode: Default::default(),
            permission_mode: RunPermissionMode::Agent,
            interactive: false,
        },
    }
}

fn assert_exec_command_contract(request: &RunRequest, workspace: &Path) {
    assert_eq!(request.model, TEST_MODEL);
    assert_eq!(
        request
            .task_contract
            .as_ref()
            .expect("Agent task contract")
            .definition
            .objective,
        TEST_PROMPT
    );
    assert_eq!(
        request.environment.workspace,
        workspace
            .canonicalize()
            .expect("canonical parity workspace")
            .display()
            .to_string()
    );
    assert_eq!(request.reasoning_effort, ReasoningEffort::High);
    assert!(request.streaming);
    assert_eq!(
        request.tool_policy,
        ToolPolicy {
            enabled: true,
            allowed: Some(vec!["read_file".to_owned()]),
            denied: Vec::new(),
        }
    );
    assert_eq!(request.accounting_baseline.hard_request_limit, Some(4));
    assert_eq!(request.limits.max_turns, 4);
    assert_eq!(request.limits.max_model_requests, 4);
    assert_eq!(request.limits.max_depth, 0);
    assert_eq!(request.limits.max_concurrent_children, 0);
    assert_eq!(
        request.environment.permission_mode,
        RunPermissionMode::Agent
    );
}

fn transport_options() -> AppServerOptions {
    AppServerOptions {
        insecure_no_auth: true,
        ..AppServerOptions::default()
    }
}

fn envelope(request_id: &str, command: RunCommand) -> RunCommandEnvelope {
    RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        command,
    }
}

async fn http_post(app: &Router, uri: &str, command: &RunCommandEnvelope) -> RunCommandResponse {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(command).expect("serialize HTTP command"),
                ))
                .expect("HTTP request"),
        )
        .await
        .expect("HTTP response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn http_get(app: &Router, uri: &str) -> RunCommandResponse {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("HTTP request"),
        )
        .await
        .expect("HTTP response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn http_sse_events(app: &Router, run_id: &RunId) -> Vec<StoredRuntimeEvent> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/runs/{}/events?after_sequence=0", run_id.0))
                .header(ACCEPT, "text/event-stream")
                .body(Body::empty())
                .expect("HTTP SSE request"),
        )
        .await
        .expect("HTTP SSE response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("HTTP SSE body");
    let body = String::from_utf8(bytes.to_vec()).expect("UTF-8 SSE body");
    body.split("\n\n")
        .filter(|frame| !frame.is_empty())
        .map(|frame| {
            let id = frame
                .lines()
                .find_map(|line| line.strip_prefix("id: "))
                .expect("canonical SSE sequence id")
                .parse::<u64>()
                .expect("numeric SSE sequence id");
            let data = frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .expect("canonical SSE event data");
            let event = serde_json::from_str::<StoredRuntimeEvent>(data)
                .expect("canonical StoredRuntimeEvent SSE data");
            assert_eq!(id, event.sequence, "SSE id must equal Store sequence");
            event
        })
        .collect()
}

async fn response_json(response: axum::response::Response) -> RunCommandResponse {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("HTTP response body");
    serde_json::from_slice(&bytes).expect("canonical HTTP response")
}

async fn wait_http_terminal(app: &Router, run_id: &RunId) -> RunView {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let run = run_from_response(http_get(app, &format!("/v1/runs/{}", run_id.0)).await);
            if run.terminal.is_some() {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("HTTP run reaches terminal")
}

async fn stdio_command(
    application: Arc<AgentApplication>,
    command: RunCommandEnvelope,
) -> RunCommandResponse {
    let (mut client, server) = tokio::io::duplex(2 * 1024 * 1024);
    let (reader, writer) = tokio::io::split(server);
    let task = tokio::spawn(async move {
        serve_stdio(application, BufReader::new(reader), writer)
            .await
            .expect("serve canonical stdio")
    });
    let mut encoded = serde_json::to_vec(&command).expect("serialize stdio command");
    encoded.push(b'\n');
    client
        .write_all(&encoded)
        .await
        .expect("write stdio command");
    client.shutdown().await.expect("close stdio input");
    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .await
        .expect("read stdio response");
    task.await.expect("stdio transport task");
    assert_eq!(
        output.lines().count(),
        1,
        "stdio must emit one response line"
    );
    serde_json::from_str(output.trim_end()).expect("canonical stdio response")
}

async fn wait_stdio_terminal(application: Arc<AgentApplication>, run_id: &RunId) -> RunView {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let run = run_from_response(
                stdio_command(
                    application.clone(),
                    envelope(
                        "stdio-get",
                        RunCommand::Get {
                            run_id: run_id.clone(),
                        },
                    ),
                )
                .await,
            );
            if run.terminal.is_some() {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stdio run reaches terminal")
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

async fn load_replay(state_path: &Path, run_id: &RunId) -> RunReplay {
    StateStore::open(Some(state_path.to_path_buf()))
        .expect("open canonical StateStore")
        .load(run_id)
        .await
        .expect("load canonical run")
        .expect("canonical run exists")
}

fn run_created_request(events: &[StoredRuntimeEvent]) -> &RunRequest {
    match &events.first().expect("run_created event").event {
        RuntimeEventKind::RunCreated { request } => request,
        other => panic!("first event is not run_created: {other:?}"),
    }
}

fn view_from_replay(replay: &RunReplay) -> RunView {
    let snapshot = &replay.snapshot;
    let request = &snapshot.request;
    RunView {
        run_id: request.run_id.clone().expect("persisted run id"),
        parent_run_id: request.parent_run_id.clone(),
        continued_from_run_id: request.continued_from_run_id.clone(),
        model: request.model.clone(),
        task_contract: request.task_contract.clone(),
        workspace: request.environment.workspace.clone(),
        last_sequence: snapshot.last_sequence,
        terminal: snapshot
            .terminal
            .as_ref()
            .map(|outcome| outcome.terminal.clone()),
        usage: snapshot.usage,
        accounting: snapshot.accounting.clone(),
        runtime_model_requests: snapshot.runtime_model_requests,
        runtime_retries: snapshot.runtime_retries,
        tool_calls: snapshot.tool_calls,
        local_turns: snapshot.local_turns,
    }
}

fn assert_terminal_accounting(view: &RunView) {
    assert!(matches!(
        view.terminal.as_ref(),
        Some(TerminalState::Completed { message, .. }) if message == FINAL_MESSAGE
    ));
    assert_eq!(view.runtime_model_requests, 2);
    assert_eq!(view.runtime_retries, 0);
    assert_eq!(view.tool_calls, 1);
    assert_eq!(view.local_turns, 2);
    assert_eq!(view.usage.input_tokens, 40);
    assert_eq!(view.usage.output_tokens, 9);
    assert_eq!(view.accounting.hard_request_limit, Some(4));
    assert_eq!(view.accounting.total_started(), 2);
    assert_eq!(view.accounting.total_completed(), 2);
    assert_eq!(view.accounting.total_in_flight(), 0);
    assert!(view.accounting.complete);
    assert!(view.accounting.usage_complete);
}

fn assert_fixture_event_sequence(events: &[StoredRuntimeEvent]) {
    let kinds = events
        .iter()
        .map(|event| match &event.event {
            RuntimeEventKind::RunCreated { .. } => "run_created",
            RuntimeEventKind::ContextCompactionCommitted { .. } => "context_compaction_committed",
            RuntimeEventKind::ModelRequestPrepared { .. } => "model_request_prepared",
            RuntimeEventKind::ModelRequestInFlight { .. } => "model_request_in_flight",
            RuntimeEventKind::ModelRequestFailed { .. } => "model_request_failed",
            RuntimeEventKind::ModelResponseCommitted { .. } => "model_response_committed",
            RuntimeEventKind::ContentDelta { .. } => "content_delta",
            RuntimeEventKind::ReasoningDelta { .. } => "reasoning_delta",
            RuntimeEventKind::ToolPrepared { .. } => "tool_prepared",
            RuntimeEventKind::ToolAuthorizationCommitted { .. } => "tool_authorization_committed",
            RuntimeEventKind::ToolExecutionStarted { .. } => "tool_execution_started",
            RuntimeEventKind::ToolOutcomeCommitted { name, outcome, .. } => {
                assert_eq!(name, "read_file");
                assert!(outcome.is_success(), "read_file must really execute");
                "tool_outcome_committed"
            }
            RuntimeEventKind::ChildStarted { .. } => "child_started",
            RuntimeEventKind::ChildFinished { .. } => "child_finished",
            RuntimeEventKind::InteractionRequested { .. } => "interaction_requested",
            RuntimeEventKind::InteractionResolved { .. } => "interaction_resolved",
            RuntimeEventKind::SteerQueued { .. } => "steer_queued",
            RuntimeEventKind::SteerApplied { .. } => "steer_applied",
            RuntimeEventKind::ControlRequested { .. } => "control_requested",
            RuntimeEventKind::WorkspaceObserved { .. } => "workspace_observed",
            RuntimeEventKind::CompletionProposed { .. } => "completion_proposed",
            RuntimeEventKind::HostVerificationPrepared { .. } => "host_verification_prepared",
            RuntimeEventKind::HostVerificationStarted { .. } => "host_verification_started",
            RuntimeEventKind::HostVerificationCommitted { .. } => "host_verification_committed",
            RuntimeEventKind::CompletionRejected { .. } => "completion_rejected",
            RuntimeEventKind::AgentTaskPrepared { .. } => "agent_task_prepared",
            RuntimeEventKind::AgentWorkspaceCreated { .. } => "agent_workspace_created",
            RuntimeEventKind::AgentSealPrepared { .. } => "agent_seal_prepared",
            RuntimeEventKind::AgentSealCommitted { .. } => "agent_seal_committed",
            RuntimeEventKind::AgentResultCollected { .. } => "agent_result_collected",
            RuntimeEventKind::AgentIntegrationPrepared { .. } => "agent_integration_prepared",
            RuntimeEventKind::AgentIntegrationStarted { .. } => "agent_integration_started",
            RuntimeEventKind::AgentIntegrationFailed { .. } => "agent_integration_failed",
            RuntimeEventKind::AgentIntegrationCommitted { .. } => "agent_integration_committed",
            RuntimeEventKind::AgentCleanupPrepared { .. } => "agent_cleanup_prepared",
            RuntimeEventKind::AgentCleanupCommitted { .. } => "agent_cleanup_committed",
            RuntimeEventKind::Terminal { .. } => "terminal",
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "run_created",
            "workspace_observed",
            "model_request_prepared",
            "model_request_in_flight",
            "reasoning_delta",
            "model_response_committed",
            "tool_prepared",
            "tool_authorization_committed",
            "tool_execution_started",
            "tool_outcome_committed",
            "workspace_observed",
            "model_request_prepared",
            "model_request_in_flight",
            "content_delta",
            "model_response_committed",
            "completion_proposed",
            "workspace_observed",
            "terminal",
        ]
    );
}

fn writer_lifecycle_fixture() -> Vec<StoredRuntimeEvent> {
    let root_run_id = RunId::from("writer-root-run");
    let child_run_id = RunId::from("writer-child-run");
    let task_id = AgentTaskId::from("writer-task");
    let integration_id = OperationId::from("writer-integration");
    let verifier = VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: json!({"suite": "writer-surface"}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "writer-tests".to_owned(),
                program: "cargo".to_owned(),
                args: vec!["test".to_owned(), "-p".to_owned(), "fixture".to_owned()],
                cwd: String::new(),
                env: BTreeMap::from([("CARGO_INCREMENTAL".to_owned(), "0".to_owned())]),
                timeout_ms: 60_000,
            }],
        },
    };
    let task_contract = TaskContract {
        generation_id: TaskGenerationId::from(child_run_id.0.clone()),
        definition: TaskDefinition {
            objective: "只修改 src/lib.rs 并通过冻结验证".to_owned(),
            constraints: vec!["只允许修改 src/lib.rs".to_owned()],
            non_goals: vec!["不得修改主工作区".to_owned()],
            acceptance: vec![TaskAcceptance::Verifier {
                id: AcceptanceId::from("writer-tests"),
                description: "writer 测试必须通过".to_owned(),
                evidence_policy: VerifierEvidencePolicy::LatestPass,
                verifier: verifier.clone(),
            }],
        },
    };
    let assignment = AgentWorkspaceAssignment {
        access: AgentWorkspaceAccess::IsolatedWrite,
        root_workspace: "/workspace/root".to_owned(),
        base_commit: WRITER_BASE_COMMIT.to_owned(),
        worktree_path: Some("/workspace/worktrees/writer-task".to_owned()),
        root_branch: Some("deepseek-agent".to_owned()),
        branch: Some("dse/writer-task".to_owned()),
        allowed_paths: vec!["src/lib.rs".to_owned()],
        owner_token: Some("writer-owner-token".to_owned()),
    };
    let task = AgentTask {
        task_id: task_id.clone(),
        root_run_id: root_run_id.clone(),
        parent_run_id: root_run_id.clone(),
        child_run_id: child_run_id.clone(),
        call_id: "writer-call".to_owned(),
        role: "implementer".to_owned(),
        task_contract: task_contract.clone(),
        workspace: assignment.clone(),
        model: "deepseek-v4-pro".to_owned(),
        reasoning_effort: ReasoningEffort::High,
        max_output_tokens: Some(262_144),
        context_policy: ContextPolicy {
            hard_input_tokens: 90_000,
        },
        route: ModelRouteAudit {
            profile: ModelRouteProfile::Explicit,
            policy_version: "fixture_explicit_v1".to_owned(),
            reason_code: "explicit_model".to_owned(),
        },
        tool_policy: ToolPolicy {
            enabled: true,
            allowed: Some(vec!["read_file".to_owned(), "apply_patch".to_owned()]),
            denied: vec!["agent".to_owned()],
        },
        limits: Default::default(),
        deadline_unix_ms: Some(1_790_000_120_000),
        expected_artifact: "一个 Host seal 的提交".to_owned(),
    };
    task.validate().expect("valid writer task fixture");

    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from("writer-evidence"),
        generation_id: task_contract.generation_id.clone(),
        acceptance_id: AcceptanceId::from("writer-tests"),
        verification_id: VerificationId::from("writer-verification"),
        verifier,
        workspace_state: writer_workspace_state(4, WRITER_FINAL_COMMIT),
        artifact_ids: vec!["writer-diff".to_owned()],
        lineage: EvidenceLineage::LatestPass,
    };
    let outcome_accounting = writer_outcome_accounting();
    let awaiting_outcome = writer_outcome(
        &task,
        &receipt,
        outcome_accounting.clone(),
        WriterIntegrationStatus::AwaitingHost,
    );
    let integrated_outcome = writer_outcome(
        &task,
        &receipt,
        outcome_accounting,
        WriterIntegrationStatus::Integrated {
            integration_id: integration_id.clone(),
            writer_commit: WRITER_FINAL_COMMIT.to_owned(),
            root_workspace_state: writer_workspace_state(2, WRITER_FINAL_COMMIT),
        },
    );

    let kinds = vec![
        RuntimeEventKind::AgentTaskPrepared {
            task: Box::new(task.clone()),
        },
        RuntimeEventKind::AgentWorkspaceCreated {
            task_id: task_id.clone(),
            assignment: assignment.clone(),
            writer_workspace_state: writer_workspace_state(0, WRITER_BASE_COMMIT),
        },
        RuntimeEventKind::ChildStarted {
            task_id: task_id.clone(),
            call_id: task.call_id.clone(),
            child_run_id: child_run_id.clone(),
            depth: 1,
        },
        RuntimeEventKind::AgentSealPrepared {
            task_id: task_id.clone(),
            base_commit: WRITER_BASE_COMMIT.to_owned(),
            writer_workspace_state_before: writer_workspace_state(
                3,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            ),
        },
        RuntimeEventKind::AgentSealCommitted {
            task_id: task_id.clone(),
            final_commit: WRITER_FINAL_COMMIT.to_owned(),
            diff_sha256: WRITER_DIFF_SHA256.to_owned(),
            changed_files: vec!["src/lib.rs".to_owned()],
            writer_workspace_state_after: writer_workspace_state(4, WRITER_FINAL_COMMIT),
        },
        RuntimeEventKind::AgentResultCollected {
            task_id: task_id.clone(),
            outcome: Box::new(awaiting_outcome),
        },
        RuntimeEventKind::AgentIntegrationPrepared {
            task_id: task_id.clone(),
            integration_id: integration_id.clone(),
            base_commit: WRITER_BASE_COMMIT.to_owned(),
            writer_commit: WRITER_FINAL_COMMIT.to_owned(),
            diff_sha256: WRITER_DIFF_SHA256.to_owned(),
            expected_root_workspace_state: writer_workspace_state(1, WRITER_BASE_COMMIT),
        },
        RuntimeEventKind::AgentIntegrationStarted {
            task_id: task_id.clone(),
            integration_id: integration_id.clone(),
        },
        RuntimeEventKind::AgentIntegrationCommitted {
            task_id: task_id.clone(),
            integration_id,
            root_head_commit: WRITER_FINAL_COMMIT.to_owned(),
            root_workspace_state_after: writer_workspace_state(2, WRITER_FINAL_COMMIT),
        },
        RuntimeEventKind::ChildFinished {
            call_id: task.call_id.clone(),
            outcome: Box::new(integrated_outcome),
            accounting: Box::new(writer_parent_accounting()),
            handoff_content: "writer 已集成；父 Agent 可继续使用变更".to_owned(),
        },
        RuntimeEventKind::AgentCleanupPrepared {
            task_id: task_id.clone(),
            plan: Box::new(WriterCleanupPlan {
                phase: WriterCleanupPhase::PostIntegration,
                reason_code: "writer_integrated".to_owned(),
                ownership: WriterCleanupOwnership::Known {
                    identity_sha256: "c".repeat(64),
                },
                artifact_state: WriterArtifactState::KnownHostSealed {
                    final_commit: WRITER_FINAL_COMMIT.to_owned(),
                    diff_sha256: WRITER_DIFF_SHA256.to_owned(),
                },
                scope: WriterCleanupScope::Known {
                    workspace_revision: WorkspaceRevision::Known {
                        sha256: WRITER_FINAL_COMMIT.to_owned(),
                    },
                    changed_count: 1,
                    in_scope_count: 1,
                    out_of_scope_count: 0,
                    path_set_sha256: writer_path_set_sha256(&["src/lib.rs".to_owned()])
                        .expect("cleanup scope hash"),
                },
                mode: WriterCleanupMode::RemoveExact {
                    expected_branch_commit: WRITER_FINAL_COMMIT.to_owned(),
                },
            }),
        },
        RuntimeEventKind::AgentCleanupCommitted {
            task_id,
            result: WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            },
        },
    ];

    kinds
        .into_iter()
        .enumerate()
        .map(|(index, event)| StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: root_run_id.clone(),
            parent_run_id: None,
            event_id: RuntimeEventId(format!("writer-event-{}", index + 1)),
            sequence: (index + 1) as u64,
            occurred_at_unix_ms: 1_790_000_000_000 + index as u64,
            event,
        })
        .collect()
}

fn writer_outcome(
    task: &AgentTask,
    receipt: &EvidenceReceipt,
    accounting: ModelAccounting,
    integration: WriterIntegrationStatus,
) -> AgentOutcome {
    let outcome = AgentOutcome {
        run_id: task.child_run_id.clone(),
        parent_run_id: Some(task.parent_run_id.clone()),
        terminal: TerminalState::Completed {
            message: "writer 子任务完成".to_owned(),
            decision: CompletionDecision {
                candidate_id: CompletionCandidateId::from("writer-candidate"),
                generation_id: task.task_contract.generation_id.clone(),
                workspace_state: writer_workspace_state(4, WRITER_FINAL_COMMIT),
                satisfied: vec![AcceptanceSatisfaction::Evidence {
                    acceptance_id: receipt.acceptance_id.clone(),
                    receipt_id: receipt.id.clone(),
                }],
            },
        },
        accounting,
        runtime_model_requests: 2,
        runtime_retries: 1,
        tool_calls: 3,
        details: AgentResultDetails {
            summary: "writer 修改并验证了目标文件".to_owned(),
            evidence: vec![receipt.clone()],
            changed_files: vec!["src/lib.rs".to_owned()],
            checks: Vec::new(),
            unresolved: vec!["父 Agent 仍需消费 handoff".to_owned()],
            artifacts: vec![ToolArtifact {
                id: "writer-diff".to_owned(),
                status: ToolArtifactStatus::Available,
                sha256: Some(WRITER_DIFF_SHA256.to_owned()),
                media_type: Some("text/x-diff".to_owned()),
                byte_len: Some(128),
                inline_content: None,
            }],
            workspace: Some(task.workspace.clone()),
            workspace_state: Some(writer_workspace_state(4, WRITER_FINAL_COMMIT)),
            base_commit: Some(WRITER_BASE_COMMIT.to_owned()),
            final_commit: Some(WRITER_FINAL_COMMIT.to_owned()),
            diff_sha256: Some(WRITER_DIFF_SHA256.to_owned()),
            integration,
        },
    };
    outcome.validate().expect("valid writer outcome fixture");
    outcome
}

fn writer_outcome_accounting() -> ModelAccounting {
    ModelAccounting {
        hard_request_limit: Some(12),
        root: ActorRequestAccounting {
            started: 0,
            completed: 0,
            in_flight: 0,
        },
        child: ActorRequestAccounting {
            started: 2,
            completed: 2,
            in_flight: 0,
        },
        runtime_retries: 1,
        sealed: true,
        complete: true,
        usage_complete: true,
        usage_responses: 2,
        usage: Usage {
            input_tokens: 101,
            output_tokens: 17,
            cache_hit_tokens: 41,
            cache_miss_tokens: 60,
            reasoning_tokens: 9,
            reasoning_replay_tokens: 3,
            ..Usage::default()
        },
        cost_nanousd: 12_345,
        cost_nanocny: 88_888,
        ..ModelAccounting::default()
    }
}

fn writer_parent_accounting() -> ModelAccounting {
    ModelAccounting {
        hard_request_limit: Some(12),
        root: ActorRequestAccounting {
            started: 1,
            completed: 1,
            in_flight: 0,
        },
        child: ActorRequestAccounting {
            started: 3,
            completed: 3,
            in_flight: 0,
        },
        runtime_retries: 1,
        sealed: true,
        complete: true,
        usage_complete: true,
        usage_responses: 4,
        usage: Usage {
            input_tokens: 211,
            output_tokens: 29,
            cache_hit_tokens: 81,
            cache_miss_tokens: 130,
            reasoning_tokens: 15,
            reasoning_replay_tokens: 5,
            ..Usage::default()
        },
        cost_nanousd: 23_456,
        cost_nanocny: 166_666,
        ..ModelAccounting::default()
    }
}

fn writer_workspace_state(generation: u64, sha256: &str) -> WorkspaceState {
    WorkspaceState {
        generation,
        revision: WorkspaceRevision::Known {
            sha256: sha256.to_owned(),
        },
    }
}

fn assert_exec_agent_lifecycle_parity(stdout: &str, events: &[StoredRuntimeEvent]) {
    let expected = events
        .iter()
        .filter(|stored| exec_lifecycle_stream::is_agent_lifecycle_event(&stored.event))
        .map(|stored| serde_json::to_value(stored).expect("serialize canonical lifecycle event"))
        .collect::<Vec<_>>();
    assert!(
        !expected.is_empty(),
        "Agent lifecycle parity requires a non-empty canonical fixture"
    );
    let actual = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value["type"] == "agent_lifecycle")
        .map(|value| {
            value
                .get("runtime_event")
                .cloned()
                .expect("agent_lifecycle must carry the exact stored event")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actual, expected,
        "exec machine stream omitted, reordered, or rewrote canonical Agent lifecycle identity/facts"
    );
}

fn assert_canonical_event_parity(
    expected: &[StoredRuntimeEvent],
    actual: &[StoredRuntimeEvent],
    label: &str,
) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label} event count differs (missing or invented event)"
    );
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        assert_eq!(
            normalize_value(expected),
            normalize_value(actual),
            "{label} canonical event {} differs",
            index + 1
        );
    }
}

fn normalize_value(value: impl serde::Serialize) -> Value {
    let mut value = serde_json::to_value(value).expect("serialize canonical value");
    normalize_volatile_fields(&mut value);
    value
}

fn normalize_volatile_fields(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_volatile_fields),
        Value::Object(values) => normalize_object(values),
        Value::String(text) => {
            if text
                .lines()
                .any(|line| line.starts_with("- task_generation: `"))
            {
                *text = text
                    .lines()
                    .map(|line| {
                        if line.starts_with("- task_generation: `") {
                            "- task_generation: `<generation-id>`"
                        } else {
                            line
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn normalize_object(values: &mut Map<String, Value>) {
    for (key, value) in values {
        let placeholder = match key.as_str() {
            "run_id" | "parent_run_id" | "child_run_id" => Some("<run-id>"),
            "generation_id" => Some("<generation-id>"),
            "event_id" => Some("<event-id>"),
            "attempt_id" => Some("<attempt-id>"),
            "operation_id" => Some("<operation-id>"),
            "deadline_unix_ms" | "occurred_at_unix_ms" => Some("<time>"),
            _ => None,
        };
        if let Some(placeholder) = placeholder {
            if !value.is_null() {
                *value = Value::String(placeholder.to_owned());
            }
        } else {
            normalize_volatile_fields(value);
        }
    }
}

async fn mount_model_fixture(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{ "id": TEST_MODEL, "object": "model" }]
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ReadThenComplete)
        .mount(server)
        .await;
}

fn read_file_sse() -> String {
    let arguments = json!({"path": "fixture.txt"}).to_string();
    [
        sse_chunk(json!({
            "id": "chatcmpl-surface-parity-tool",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "reasoning_content": "读取指定文件以确认内容",
                    "tool_calls": [{
                        "index": 0,
                        "id": TOOL_CALL_ID,
                        "type": "function",
                        "function": {"name": "read_file", "arguments": arguments}
                    }]
                },
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-surface-parity-tool",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
            "usage": {
                "prompt_tokens": 17,
                "completion_tokens": 5,
                "total_tokens": 22,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 17
            }
        })),
        "data: [DONE]\n\n".to_owned(),
    ]
    .join("")
}

fn final_sse() -> String {
    [
        sse_chunk(json!({
            "id": "chatcmpl-surface-parity-final",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {"content": FINAL_MESSAGE},
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-surface-parity-final",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 23,
                "completion_tokens": 4,
                "total_tokens": 27,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 23
            }
        })),
        "data: [DONE]\n\n".to_owned(),
    ]
    .join("")
}

fn sse_chunk(value: Value) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(&value).expect("serialize SSE chunk")
    )
}

fn sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .insert_header("cache-control", "no-cache")
        .set_body_string(body)
}

fn json_strings_contain(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value.contains(needle),
        Value::Array(values) => values
            .iter()
            .any(|value| json_strings_contain(value, needle)),
        Value::Object(values) => values
            .values()
            .any(|value| json_strings_contain(value, needle)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
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
        "HOME",
        "USERPROFILE",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "SHELL",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn dse_tui_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_dse-tui") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_dse-tui") {
        return PathBuf::from(path);
    }
    let mut path = std::env::current_exe().expect("current test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(format!("dse-tui{}", std::env::consts::EXE_SUFFIX));
    path
}
