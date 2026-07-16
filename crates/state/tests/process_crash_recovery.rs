//! Process-boundary recovery tests for the durable Agent runtime.
//!
//! The ignored helper is launched as a real child process. Its event sink or
//! tool aborts only after the preceding SQLite commit has returned, so the
//! parent exercises dead-PID lease takeover rather than an in-process mock.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codewhale_runtime::{
    ActorRequestAccounting, AgentRuntime, ApiSurface, CancellationToken, ModelAccounting,
    ModelFinishReason, ModelOutput, ModelPort, ModelPortError, ModelRequest, ModelStream,
    ModelStreamEvent, ModelToolCall, NullEventSink, PendingRuntimeEvent, RecoveryAmbiguityPhase,
    RunId, RunRequest, RunStore, RuntimeEventId, RuntimeEventKind, RuntimeEventSink,
    StoredRuntimeEvent, SurfaceUsage, TerminalState, ToolArguments, ToolDefinition,
    ToolExecutionError, ToolExecutor, ToolInvocation, ToolOutcome, Usage,
};
use codewhale_state::StateStore;
use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::TempDir;

const CHILD_SCENARIO: &str = "CODEWHALE_CRASH_TEST_SCENARIO";
const CHILD_DB: &str = "CODEWHALE_CRASH_TEST_DB";
const CHILD_MODEL_MARKER: &str = "CODEWHALE_CRASH_TEST_MODEL_MARKER";
const CHILD_TOOL_MARKER: &str = "CODEWHALE_CRASH_TEST_TOOL_MARKER";
const CHILD_ABORT_MARKER: &str = "CODEWHALE_CRASH_TEST_ABORT_MARKER";
const RUN_ID: &str = "process-crash-run";
const TOOL_NAME: &str = "write_marker";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashScenario {
    ModelInFlight,
    ToolInFlight,
    ModelResponseCommitted,
    TerminalCommitted,
}

impl CrashScenario {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelInFlight => "model_in_flight",
            Self::ToolInFlight => "tool_in_flight",
            Self::ModelResponseCommitted => "model_response_committed",
            Self::TerminalCommitted => "terminal_committed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "model_in_flight" => Self::ModelInFlight,
            "tool_in_flight" => Self::ToolInFlight,
            "model_response_committed" => Self::ModelResponseCommitted,
            "terminal_committed" => Self::TerminalCommitted,
            other => panic!("unknown crash test scenario: {other}"),
        }
    }
}

struct CrashFixture {
    _temp: TempDir,
    db: PathBuf,
    model_marker: PathBuf,
    tool_marker: PathBuf,
    abort_marker: PathBuf,
}

impl CrashFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create crash-test directory");
        Self {
            db: temp.path().join("state.db"),
            model_marker: temp.path().join("model-requests.log"),
            tool_marker: temp.path().join("tool-side-effects.log"),
            abort_marker: temp.path().join("abort.log"),
            _temp: temp,
        }
    }

    fn crash_child(&self, scenario: CrashScenario) {
        let status = Command::new(std::env::current_exe().expect("locate integration test binary"))
            .arg("--ignored")
            .arg("--exact")
            .arg("process_crash_helper")
            .arg("--test-threads=1")
            .env(CHILD_SCENARIO, scenario.as_str())
            .env(CHILD_DB, &self.db)
            .env(CHILD_MODEL_MARKER, &self.model_marker)
            .env(CHILD_TOOL_MARKER, &self.tool_marker)
            .env(CHILD_ABORT_MARKER, &self.abort_marker)
            .status()
            .expect("launch crash helper");
        assert!(!status.success(), "helper must terminate by process abort");
        assert_eq!(
            marker_count(&self.abort_marker),
            1,
            "the intended post-commit crash point must be reached"
        );
        let owner_pid = Connection::open(&self.db)
            .expect("open crash database for lease assertion")
            .query_row(
                "SELECT lease_owner_pid FROM agent_runs WHERE run_id = ?1",
                params![RUN_ID],
                |row| row.get::<_, Option<u32>>(0),
            )
            .expect("read crashed lease owner");
        if scenario == CrashScenario::TerminalCommitted {
            assert_eq!(owner_pid, None, "terminal commit must release the lease");
        } else {
            let child_pid = owner_pid.expect("non-terminal crash must retain child lease owner");
            assert_ne!(
                child_pid,
                std::process::id(),
                "the persisted lease must belong to the real child process"
            );
        }
    }

    fn reopen(&self, scenario: CrashScenario) -> (Arc<AgentRuntime>, Arc<StateStore>) {
        let store = Arc::new(StateStore::open(Some(self.db.clone())).expect("reopen SQLite store"));
        let runtime = Arc::new(AgentRuntime::new(
            Arc::new(MarkerModel::new(self.model_marker.clone(), scenario)),
            Arc::new(MarkerTools::new(self.tool_marker.clone(), false, None)),
            Arc::new(NullEventSink),
            store.clone(),
        ));
        (runtime, store)
    }
}

#[derive(Default)]
struct ModelLedger {
    started: AtomicU64,
    completed: AtomicU64,
    usage: Mutex<Usage>,
}

struct MarkerModel {
    marker: PathBuf,
    scenario: CrashScenario,
    ledger: Arc<ModelLedger>,
}

impl MarkerModel {
    fn new(marker: PathBuf, scenario: CrashScenario) -> Self {
        Self {
            marker,
            scenario,
            ledger: Arc::new(ModelLedger::default()),
        }
    }
}

#[async_trait]
impl ModelPort for MarkerModel {
    async fn stream(&self, _request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        append_marker(&self.marker, "request");
        self.ledger.started.fetch_add(1, Ordering::AcqRel);
        let output = if self.scenario == CrashScenario::ToolInFlight {
            ModelOutput {
                content: String::new(),
                reasoning_content: None,
                tool_calls: vec![ModelToolCall {
                    id: "tool-call-1".to_owned(),
                    name: TOOL_NAME.to_owned(),
                    arguments: ToolArguments::parse("{}"),
                }],
                finish_reason: ModelFinishReason::ToolCalls,
                usage: one_usage(),
            }
        } else {
            ModelOutput {
                content: "已完成".to_owned(),
                reasoning_content: Some("已验证结果".to_owned()),
                tool_calls: Vec::new(),
                finish_reason: ModelFinishReason::Stop,
                usage: one_usage(),
            }
        };
        Ok(Box::new(OneShotStream {
            output: Some(output),
            ledger: self.ledger.clone(),
        }))
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        let started = self.ledger.started.load(Ordering::Acquire);
        let completed = self.ledger.completed.load(Ordering::Acquire);
        let usage = *self.ledger.usage.lock().expect("usage ledger lock");
        let in_flight = started.saturating_sub(completed);
        let surface_usage = (completed > 0)
            .then(|| SurfaceUsage {
                surface: ApiSurface::StandardChat,
                model: "deepseek-test".to_owned(),
                response_count: completed,
                usage_response_count: completed,
                usage,
                cost_nanousd: 0,
                cost_nanocny: 0,
            })
            .into_iter()
            .collect();
        Ok(ModelAccounting {
            hard_request_limit: Some(16),
            root: ActorRequestAccounting {
                started,
                completed,
                in_flight,
                retries: 0,
            },
            sealed: seal,
            complete: in_flight == 0,
            usage_complete: in_flight == 0,
            usage_responses: completed,
            usage,
            surface_usage,
            ..ModelAccounting::default()
        })
    }
}

struct OneShotStream {
    output: Option<ModelOutput>,
    ledger: Arc<ModelLedger>,
}

#[async_trait]
impl ModelStream for OneShotStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        let output = self.output.take()?;
        self.ledger.completed.fetch_add(1, Ordering::AcqRel);
        self.ledger
            .usage
            .lock()
            .expect("usage ledger lock")
            .add_assign(output.usage);
        Some(Ok(ModelStreamEvent::Completed { output }))
    }
}

struct MarkerTools {
    marker: PathBuf,
    abort_after_side_effect: bool,
    abort_marker: Option<PathBuf>,
}

impl MarkerTools {
    fn new(marker: PathBuf, abort_after_side_effect: bool, abort_marker: Option<PathBuf>) -> Self {
        Self {
            marker,
            abort_after_side_effect,
            abort_marker,
        }
    }
}

#[async_trait]
impl ToolExecutor for MarkerTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: TOOL_NAME.to_owned(),
            description: "write a process crash test marker".to_owned(),
            input_schema: json!({"type": "object", "additionalProperties": false}),
        }]
    }

    async fn execute(
        &self,
        _invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        append_marker(&self.marker, "side-effect");
        if self.abort_after_side_effect {
            append_marker(
                self.abort_marker
                    .as_ref()
                    .expect("tool crash requires abort marker"),
                "tool_in_flight",
            );
            std::process::abort();
        }
        Ok(ToolOutcome::success("side effect applied"))
    }
}

struct CrashSink {
    scenario: CrashScenario,
    abort_marker: PathBuf,
}

#[async_trait]
impl RuntimeEventSink for CrashSink {
    async fn emit(&self, event: StoredRuntimeEvent) {
        let should_abort = match self.scenario {
            CrashScenario::ModelInFlight => {
                matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. })
            }
            CrashScenario::ModelResponseCommitted => {
                matches!(event.event, RuntimeEventKind::ModelResponseCommitted { .. })
            }
            CrashScenario::TerminalCommitted => event.event.is_terminal(),
            CrashScenario::ToolInFlight => false,
        };
        if should_abort {
            append_marker(&self.abort_marker, self.scenario.as_str());
            std::process::abort();
        }
    }
}

fn one_usage() -> Usage {
    Usage {
        input_tokens: 11,
        output_tokens: 3,
        cache_hit_tokens: 5,
        cache_miss_tokens: 6,
        cache_write_tokens: 0,
        reasoning_tokens: 1,
        reasoning_replay_tokens: 0,
    }
}

fn runtime_request() -> RunRequest {
    let mut request = RunRequest::new("执行进程恢复测试", "只执行测试脚本");
    request.run_id = Some(RunId::from(RUN_ID));
    request.model = "deepseek-test".to_owned();
    request.environment.workspace = "/tmp/codewhale-process-crash-test".to_owned();
    request.limits.max_turns = 4;
    request.limits.max_model_requests = 4;
    request.limits.max_tool_calls = 4;
    request
}

fn append_marker(path: &Path, value: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open process marker");
    writeln!(file, "{value}").expect("append process marker");
    file.sync_all().expect("sync process marker");
}

fn marker_count(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|content| content.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

fn event_count(
    replay: &codewhale_runtime::RunReplay,
    predicate: impl Fn(&RuntimeEventKind) -> bool,
) -> usize {
    replay
        .events
        .iter()
        .filter(|event| predicate(&event.event))
        .count()
}

/// This test is not run by the normal harness. Parent tests launch it with an
/// exact filter and scenario environment, then require an abnormal exit.
#[test]
#[ignore = "process crash helper"]
fn process_crash_helper() {
    let Ok(raw_scenario) = std::env::var(CHILD_SCENARIO) else {
        return;
    };
    let scenario = CrashScenario::parse(&raw_scenario);
    let db = PathBuf::from(std::env::var_os(CHILD_DB).expect("child DB path"));
    let model_marker =
        PathBuf::from(std::env::var_os(CHILD_MODEL_MARKER).expect("child model marker"));
    let tool_marker =
        PathBuf::from(std::env::var_os(CHILD_TOOL_MARKER).expect("child tool marker"));
    let abort_marker =
        PathBuf::from(std::env::var_os(CHILD_ABORT_MARKER).expect("child abort marker"));
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build child runtime");
    tokio.block_on(async move {
        let store = Arc::new(StateStore::open(Some(db)).expect("open child SQLite store"));
        let tools = Arc::new(MarkerTools::new(
            tool_marker,
            scenario == CrashScenario::ToolInFlight,
            Some(abort_marker.clone()),
        ));
        let runtime = Arc::new(AgentRuntime::new(
            Arc::new(MarkerModel::new(model_marker, scenario)),
            tools,
            Arc::new(CrashSink {
                scenario,
                abort_marker,
            }),
            store,
        ));
        let outcome = runtime
            .start(runtime_request())
            .wait()
            .await
            .expect("child runtime join");
        panic!("crash helper unexpectedly completed: {outcome:?}");
    });
}

#[tokio::test]
async fn model_request_in_flight_crash_is_not_reissued_after_reopen() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::ModelInFlight);
    assert_eq!(marker_count(&fixture.model_marker), 0);

    let (runtime, store) = fixture.reopen(CrashScenario::ModelInFlight);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume runtime");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: codewhale_runtime::RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ModelRequest,
                ..
            }
        }
    ));
    assert_eq!(marker_count(&fixture.model_marker), 0);

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered run")
        .expect("recovered run exists");
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ModelRequestInFlight { .. }
        )),
        1
    );
}

#[tokio::test]
async fn tool_side_effect_crash_is_not_executed_twice_after_reopen() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::ToolInFlight);
    assert_eq!(marker_count(&fixture.tool_marker), 1);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::ToolInFlight);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume runtime");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: codewhale_runtime::RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ToolExecution,
                ..
            }
        }
    ));
    assert_eq!(marker_count(&fixture.tool_marker), 1);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered run")
        .expect("recovered run exists");
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ToolExecutionStarted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ToolOutcomeCommitted { .. }
        )),
        0
    );
}

#[tokio::test]
async fn committed_model_response_resumes_without_duplicate_request_usage_or_assistant() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::ModelResponseCommitted);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::ModelResponseCommitted);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume runtime");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(marker_count(&fixture.model_marker), 1);
    assert_eq!(outcome.accounting.usage, one_usage());

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered run")
        .expect("recovered run exists");
    assert_eq!(replay.snapshot.usage, one_usage());
    assert_eq!(replay.snapshot.accounting.usage, one_usage());
    assert_eq!(
        replay
            .snapshot
            .transcript
            .entries
            .iter()
            .filter(|entry| matches!(entry, codewhale_runtime::TranscriptEntry::Assistant { .. }))
            .count(),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ModelResponseCommitted { .. }
        )),
        1
    );
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn committed_terminal_is_returned_after_reopen_without_second_terminal() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::TerminalCommitted);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::TerminalCommitted);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume terminal run");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load terminal run")
        .expect("terminal run exists");
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
    assert_eq!(replay.snapshot.usage, one_usage());
    assert_eq!(replay.snapshot.accounting.usage, one_usage());
}

#[tokio::test]
async fn caller_retry_after_reopen_returns_the_committed_event_by_id() {
    let fixture = CrashFixture::new();
    let store = StateStore::open(Some(fixture.db.clone())).expect("open SQLite store");
    let created = store
        .create(runtime_request())
        .await
        .expect("create durable run");
    let pending = PendingRuntimeEvent {
        event_id: RuntimeEventId("retry-after-commit".to_owned()),
        event: RuntimeEventKind::Steered {
            content: "调用方未收到 commit 返回值".to_owned(),
        },
    };
    let committed = store
        .append(&created.lease, pending.clone())
        .await
        .expect("commit event");
    drop(store);

    let reopened = StateStore::open(Some(fixture.db.clone())).expect("reopen SQLite store");
    let retried = reopened
        .append(&created.lease, pending)
        .await
        .expect("retry same event id after reopen");
    assert_eq!(retried, committed);
    let replay = reopened
        .load(&created.lease.run_id)
        .await
        .expect("load retry run")
        .expect("retry run exists");
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| event.event_id.0 == "retry-after-commit")
            .count(),
        1
    );
}
