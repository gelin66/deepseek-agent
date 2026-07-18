//! Canonical interactive-TUI lifecycle acceptance.
//!
//! This test compiles the concrete TUI Run client and projector from their
//! production source files. It crosses a real `AgentApplication`, the SQLite
//! `RunStore`, and a loopback DeepSeek transport; no legacy Engine or session
//! persistence participates.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use codewhale_app::{
    AgentApplication, DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
    ProductionPromptConfig, ProductionToolConfig, ShellPolicy, TransportRetryPolicy,
};
use codewhale_protocol::agent_runtime::{
    ReasoningEffort, RunId, RunLimits, RuntimeEventKind, StoredRuntimeEvent, ToolPolicy,
};
use codewhale_protocol::run_api::{RunProductControls, StartRunCommand};
use codewhale_runtime::{RunReplay, RunStore};
use codewhale_state::StateStore;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::mpsc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[path = "../src/tui/run_client.rs"]
#[allow(dead_code)]
mod run_client;
#[path = "../src/tui/run_projection.rs"]
#[allow(dead_code)]
mod run_projection;

use run_client::TuiRunClient;
use run_projection::{
    CanonicalRunProjection, ProjectionEffect, ProjectionEffectKind, UserTranscriptSource,
};

const TEST_MODEL: &str = "deepseek-v4-flash";
const TEST_KEY: &str = "offline-canonical-tui-key";
const FIRST_INPUT: &str = "请检查当前项目并给出第一轮结论";
const CONTINUE_INPUT: &str = "继续完成剩余工作，并给出最终结论";
const FIRST_OUTPUT: &str = "第一轮已完成";
const CONTINUE_OUTPUT: &str = "后续工作已完成";
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
struct ChineseCompletionFixture;

impl Respond for ChineseCompletionFixture {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = request
            .body_json::<Value>()
            .expect("DeepSeek request must be JSON");
        let content = if json_strings_contain(&body, CONTINUE_INPUT) {
            CONTINUE_OUTPUT
        } else {
            assert!(
                json_strings_contain(&body, FIRST_INPUT),
                "first request lost the Chinese user input: {body:#}"
            );
            FIRST_OUTPUT
        };
        sse_response(completion_sse(content))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn canonical_tui_rebuild_replays_then_continues_without_legacy_state() {
    let model = MockServer::start().await;
    mount_deepseek_fixture(&model).await;

    let isolated = TempDir::new().expect("isolated canonical TUI root");
    let workspace = isolated.path().join("workspace");
    let codewhale_home = isolated.path().join("home/.codewhale");
    let skills_dir = codewhale_home.join("skills");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(&skills_dir).expect("create isolated CodeWhale home");
    let state_path = codewhale_home.join("state.db");
    let canonical_workspace = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .display()
        .to_string();

    let first_application =
        production_application(&state_path, &model.uri(), &workspace, &skills_dir);
    let (first_client, mut first_rx) = TuiRunClient::new(first_application.clone());
    let first_view = first_client
        .submit(start_command(FIRST_INPUT, &canonical_workspace))
        .await
        .expect("fresh canonical TUI run starts");
    let mut first_projection = CanonicalRunProjection::new();
    let (first_events, first_effects) =
        collect_terminal(&mut first_rx, &mut first_projection, &first_view.run_id).await;

    assert_canonical_projection(&first_events, &first_effects, FIRST_INPUT);
    assert_stored_event_identity(&first_events, &first_view.run_id);
    let first_store = StateStore::open(Some(state_path.clone())).expect("open canonical RunStore");
    let source_before = load_replay(&first_store, &first_view.run_id).await;
    assert_eq!(source_before.events, first_events);
    drop(first_store);
    drop(first_client);
    drop(first_application);

    // Rebuild both process-local TUI objects and the production application
    // over the same SQLite truth. Attaching the terminal source must replay
    // exact StoredRuntimeEvents before submit plans a Continue command.
    let rebuilt_application =
        production_application(&state_path, &model.uri(), &workspace, &skills_dir);
    let (rebuilt_client, mut rebuilt_rx) = TuiRunClient::new(rebuilt_application);
    let attached = rebuilt_client
        .attach_or_resume(first_view.run_id.clone(), Some(canonical_workspace.clone()))
        .await
        .expect("attach terminal source");
    assert_eq!(attached.run_id, first_view.run_id);
    let mut rebuilt_projection = CanonicalRunProjection::new();
    let (replayed_events, replayed_effects) =
        collect_terminal(&mut rebuilt_rx, &mut rebuilt_projection, &first_view.run_id).await;
    assert_eq!(replayed_events, first_events);
    assert_canonical_projection(&replayed_events, &replayed_effects, FIRST_INPUT);

    let continued_view = rebuilt_client
        .submit(start_command(CONTINUE_INPUT, &canonical_workspace))
        .await
        .expect("terminal follow-up becomes canonical Continue");
    assert_ne!(continued_view.run_id, first_view.run_id);
    assert_eq!(
        continued_view.continued_from_run_id,
        Some(first_view.run_id.clone())
    );
    let (continued_events, continued_effects) = collect_terminal(
        &mut rebuilt_rx,
        &mut rebuilt_projection,
        &continued_view.run_id,
    )
    .await;
    assert_canonical_projection(&continued_events, &continued_effects, CONTINUE_INPUT);
    assert_stored_event_identity(&continued_events, &continued_view.run_id);

    let store = StateStore::open(Some(state_path)).expect("reopen canonical RunStore");
    let source_after = load_replay(&store, &first_view.run_id).await;
    let continuation = load_replay(&store, &continued_view.run_id).await;
    assert_eq!(
        source_after, source_before,
        "Continue must not rewrite its source run"
    );
    assert_eq!(
        continuation.snapshot.request.continued_from_run_id,
        Some(first_view.run_id.clone())
    );
    assert_eq!(continuation.snapshot.request.parent_run_id, None);
    match &continuation.events[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.input, CONTINUE_INPUT);
            assert_eq!(request.transcript, source_before.snapshot.transcript);
        }
        event => panic!("continuation must start with RunCreated, got {event:?}"),
    }

    let requests = model
        .received_requests()
        .await
        .expect("DeepSeek request journal")
        .into_iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .map(|request| request.body_json::<Value>().expect("DeepSeek request body"))
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2, "each root should make one model request");
    assert!(json_strings_contain(&requests[0], FIRST_INPUT));
    assert!(!json_strings_contain(&requests[0], CONTINUE_INPUT));
    assert!(json_strings_contain(&requests[1], FIRST_INPUT));
    assert!(json_strings_contain(&requests[1], CONTINUE_INPUT));

    assert_no_legacy_json_state(isolated.path());
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
        response_header_timeout: Duration::from_secs(10),
        stream_idle_timeout: Duration::from_secs(10),
        retry: TransportRetryPolicy {
            max_retries: 0,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            exponential_base: 1.0,
        },
    };
    let prompt = ProductionPromptConfig {
        skills_dir: Some(skills_dir.to_path_buf()),
        project_context_pack_enabled: false,
        ..ProductionPromptConfig::default()
    };
    let tools = ProductionToolConfig::new(workspace).with_shell_policy(ShellPolicy::None);
    let config = ProductionApplicationConfig::official()
        .with_state_db_path(state_path)
        .with_deepseek_connection(connection)
        .with_tool_config(tools)
        .with_prompt(prompt)
        .with_default_max_api_requests(NonZeroU32::new(2).expect("non-zero request limit"))
        .with_api_key(TEST_KEY)
        .expect("fixture credential");
    Arc::new(AgentApplication::production(config).expect("production AgentApplication"))
}

fn start_command(input: &str, workspace: &str) -> StartRunCommand {
    StartRunCommand {
        input: input.to_owned(),
        workspace: workspace.to_owned(),
        model: Some(TEST_MODEL.to_owned()),
        reasoning_effort: ReasoningEffort::Off,
        max_output_tokens: Some(4_096),
        max_api_requests: NonZeroU32::new(2),
        streaming: true,
        tool_policy: ToolPolicy {
            enabled: false,
            allowed: None,
            denied: Vec::new(),
        },
        limits: RunLimits {
            max_turns: 2,
            max_model_requests: 2,
            max_model_retries: 0,
            max_tool_calls: 0,
            ..RunLimits::default()
        },
        controls: RunProductControls {
            interactive: true,
            ..RunProductControls::default()
        },
    }
}

async fn collect_terminal(
    receiver: &mut mpsc::Receiver<StoredRuntimeEvent>,
    projection: &mut CanonicalRunProjection,
    run_id: &RunId,
) -> (Vec<StoredRuntimeEvent>, Vec<ProjectionEffect>) {
    let mut events = Vec::new();
    let mut effects = Vec::new();
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, receiver.recv())
            .await
            .expect("canonical event monitor timed out")
            .expect("canonical event monitor closed before Terminal");
        assert_eq!(&event.run_id, run_id, "monitor mixed independent roots");
        effects.extend(
            projection
                .apply(event.clone())
                .expect("canonical event projects without loss"),
        );
        let terminal = event.event.is_terminal();
        events.push(event);
        if terminal {
            return (events, effects);
        }
    }
}

fn assert_canonical_projection(
    events: &[StoredRuntimeEvent],
    effects: &[ProjectionEffect],
    expected_input: &str,
) {
    let projected_events = effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            ProjectionEffectKind::Canonical(event) => Some((**event).clone()),
            ProjectionEffectKind::UserTranscript { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected_events, events,
        "projector changed canonical events"
    );

    let user_inputs = effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            ProjectionEffectKind::UserTranscript {
                source: UserTranscriptSource::RunCreated,
                content,
            } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        user_inputs,
        vec![expected_input],
        "one canonical RunCreated input must appear exactly once"
    );
}

fn assert_stored_event_identity(events: &[StoredRuntimeEvent], run_id: &RunId) {
    assert!(!events.is_empty());
    let mut event_ids = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        assert_eq!(&event.run_id, run_id);
        assert_eq!(event.sequence, index as u64 + 1);
        assert!(
            !event.event_id.0.trim().is_empty(),
            "StoredRuntimeEvent id must not be empty"
        );
        assert!(
            event_ids.insert(event.event_id.0.clone()),
            "event id {} was reused within run {run_id}",
            event.event_id.0
        );
    }
    assert!(events.last().is_some_and(|event| event.event.is_terminal()));
}

async fn load_replay(store: &StateStore, run_id: &RunId) -> RunReplay {
    store
        .load(run_id)
        .await
        .expect("load canonical run")
        .expect("canonical run exists")
}

fn assert_no_legacy_json_state(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    let mut legacy = Vec::<PathBuf>::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path).expect("inspect isolated state") {
            let entry = entry.expect("state entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("file remains under isolated root")
                .to_string_lossy()
                .to_ascii_lowercase();
            let is_json = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
            if is_json
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
        "canonical TUI wrote legacy JSON state: {legacy:#?}"
    );
}

async fn mount_deepseek_fixture(server: &MockServer) {
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
        .respond_with(ChineseCompletionFixture)
        .mount(server)
        .await;
}

fn completion_sse(content: &str) -> String {
    [
        sse_chunk(json!({
            "id": "chatcmpl-canonical-tui",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": content},
                "finish_reason": null
            }]
        })),
        sse_chunk(json!({
            "id": "chatcmpl-canonical-tui",
            "object": "chat.completion.chunk",
            "model": TEST_MODEL,
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 5,
                "total_tokens": 17,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 12
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
