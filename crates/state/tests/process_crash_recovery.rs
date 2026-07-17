//! Process-boundary recovery tests for the durable Agent runtime.
//!
//! The ignored helper is launched as a real child process. Its event sink or
//! tool writes a ready marker only after the preceding SQLite commit has
//! returned, then blocks until the parent kills it. This exercises external
//! process termination and dead-PID lease takeover rather than an in-process
//! mock.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use codewhale_runtime::{
    ActorRequestAccounting, AgentControl, AgentRuntime, ApiSurface, ApprovalRisk,
    CancellationToken, CommandId, ModelAccounting, ModelFinishReason, ModelMessage, ModelOutput,
    ModelPort, ModelPortError, ModelRequest, ModelStream, ModelStreamEvent, ModelToolCall,
    NullEventSink, PendingRuntimeEvent, RecoveryAmbiguityPhase, RunId, RunRequest, RunStore,
    RuntimeEventId, RuntimeEventKind, RuntimeEventSink, StoredRuntimeEvent, SurfaceUsage,
    TerminalState, ToolApprovalPrompt, ToolArguments, ToolDefinition, ToolExecutionError,
    ToolExecutor, ToolInvocation, ToolOutcome, Usage, UserInteractionResponse,
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
    ToolInFlightControlRequested,
    ModelResponseCommitted,
    InteractionRequested,
    InteractionResolved,
    SteerQueued,
    SteerApplied,
    TerminalCommitted,
}

impl CrashScenario {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelInFlight => "model_in_flight",
            Self::ToolInFlight => "tool_in_flight",
            Self::ToolInFlightControlRequested => "tool_in_flight_control_requested",
            Self::ModelResponseCommitted => "model_response_committed",
            Self::InteractionRequested => "interaction_requested",
            Self::InteractionResolved => "interaction_resolved",
            Self::SteerQueued => "steer_queued",
            Self::SteerApplied => "steer_applied",
            Self::TerminalCommitted => "terminal_committed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "model_in_flight" => Self::ModelInFlight,
            "tool_in_flight" => Self::ToolInFlight,
            "tool_in_flight_control_requested" => Self::ToolInFlightControlRequested,
            "model_response_committed" => Self::ModelResponseCommitted,
            "interaction_requested" => Self::InteractionRequested,
            "interaction_resolved" => Self::InteractionResolved,
            "steer_queued" => Self::SteerQueued,
            "steer_applied" => Self::SteerApplied,
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
        let mut child =
            Command::new(std::env::current_exe().expect("locate integration test binary"))
                .arg("--ignored")
                .arg("--exact")
                .arg("process_crash_helper")
                .arg("--test-threads=1")
                .env(CHILD_SCENARIO, scenario.as_str())
                .env(CHILD_DB, &self.db)
                .env(CHILD_MODEL_MARKER, &self.model_marker)
                .env(CHILD_TOOL_MARKER, &self.tool_marker)
                .env(CHILD_ABORT_MARKER, &self.abort_marker)
                .spawn()
                .expect("launch crash helper");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if marker_count(&self.abort_marker) == 1 {
                break;
            }
            if let Some(status) = child.try_wait().expect("poll crash helper") {
                panic!("crash helper exited before the committed ready marker: {status}");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out waiting for the committed crash ready marker");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().expect("kill crash helper after ready marker");
        let status = child.wait().expect("reap killed crash helper");
        assert!(
            !status.success(),
            "helper must terminate by the parent process kill"
        );
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                status.signal(),
                Some(9),
                "Unix parent kill must terminate the helper with SIGKILL"
            );
        }
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
            Arc::new(MarkerTools::new(
                self.tool_marker.clone(),
                false,
                None,
                None,
            )),
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
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        if matches!(
            self.scenario,
            CrashScenario::SteerQueued | CrashScenario::SteerApplied
        ) && marker_count(&self.marker) > 0
        {
            assert!(matches!(
                request.messages.last(),
                Some(ModelMessage::User { content }) if content == "改做新任务"
            ));
        }
        let request_number = marker_count(&self.marker);
        append_marker(&self.marker, "request");
        self.ledger.started.fetch_add(1, Ordering::AcqRel);
        if self.scenario == CrashScenario::SteerQueued {
            return Ok(Box::new(PendingStream));
        }
        let output = if matches!(
            self.scenario,
            CrashScenario::ToolInFlight | CrashScenario::ToolInFlightControlRequested
        ) || matches!(
            self.scenario,
            CrashScenario::InteractionRequested | CrashScenario::InteractionResolved
        ) && request_number == 0
        {
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

struct PendingStream;

#[async_trait]
impl ModelStream for PendingStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        std::future::pending().await
    }
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
    cancel_control: Option<Arc<Mutex<Option<AgentControl>>>>,
}

impl MarkerTools {
    fn new(
        marker: PathBuf,
        abort_after_side_effect: bool,
        abort_marker: Option<PathBuf>,
        cancel_control: Option<Arc<Mutex<Option<AgentControl>>>>,
    ) -> Self {
        Self {
            marker,
            abort_after_side_effect,
            abort_marker,
            cancel_control,
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

    fn approval_prompt(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<Option<ToolApprovalPrompt>, ToolExecutionError> {
        Ok((invocation.name == TOOL_NAME
            && !self.abort_after_side_effect
            && self.cancel_control.is_none())
        .then(|| ToolApprovalPrompt {
            title: "确认写入测试标记".to_owned(),
            description: "验证审批恢复窗口".to_owned(),
            risk: ApprovalRisk::Elevated,
        }))
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
            wait_for_parent_kill().await;
        }
        if let Some(control_slot) = &self.cancel_control {
            let control = loop {
                if let Some(control) = control_slot.lock().expect("control slot lock").clone() {
                    break control;
                }
                tokio::task::yield_now().await;
            };
            control.cancel().expect("queue crash-test cancel");
            std::future::pending::<()>().await;
            unreachable!("control-requested crash aborts the process");
        }
        Ok(ToolOutcome::success("side effect applied"))
    }
}

struct CrashSink {
    scenario: CrashScenario,
    abort_marker: PathBuf,
    control: Option<Arc<Mutex<Option<AgentControl>>>>,
}

#[async_trait]
impl RuntimeEventSink for CrashSink {
    async fn emit(&self, event: StoredRuntimeEvent) {
        if self.scenario == CrashScenario::InteractionResolved
            && let RuntimeEventKind::InteractionRequested { request } = &event.event
        {
            let control = wait_for_control(
                self.control
                    .as_ref()
                    .expect("interaction resolution requires a control slot"),
            )
            .await;
            let interaction_id = request.interaction_id.clone();
            tokio::spawn(async move {
                control
                    .resolve_interaction(
                        CommandId::from("process-crash-approval"),
                        interaction_id,
                        UserInteractionResponse::Approved,
                    )
                    .await
                    .expect("resolve crash-test interaction");
            });
        }
        if self.scenario == CrashScenario::SteerQueued
            && matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. })
        {
            let control = wait_for_control(
                self.control
                    .as_ref()
                    .expect("steer queueing requires a control slot"),
            )
            .await;
            tokio::spawn(async move {
                control
                    .steer_durable(CommandId::from("process-crash-steer"), "改做新任务")
                    .await
                    .expect("queue crash-test steer");
            });
        }
        let should_abort = match self.scenario {
            CrashScenario::ModelInFlight => {
                matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. })
            }
            CrashScenario::ModelResponseCommitted => {
                matches!(event.event, RuntimeEventKind::ModelResponseCommitted { .. })
            }
            CrashScenario::InteractionRequested => {
                matches!(event.event, RuntimeEventKind::InteractionRequested { .. })
            }
            CrashScenario::InteractionResolved => {
                matches!(event.event, RuntimeEventKind::InteractionResolved { .. })
            }
            CrashScenario::SteerQueued => {
                matches!(event.event, RuntimeEventKind::SteerQueued { .. })
            }
            CrashScenario::TerminalCommitted => event.event.is_terminal(),
            CrashScenario::ToolInFlight => false,
            CrashScenario::ToolInFlightControlRequested => {
                matches!(event.event, RuntimeEventKind::ControlRequested { .. })
            }
            CrashScenario::SteerApplied => false,
        };
        if should_abort {
            append_marker(&self.abort_marker, self.scenario.as_str());
            wait_for_parent_kill().await;
        }
    }
}

async fn wait_for_parent_kill() -> ! {
    std::future::pending().await
}

async fn wait_for_control(slot: &Arc<Mutex<Option<AgentControl>>>) -> AgentControl {
    loop {
        if let Some(control) = slot.lock().expect("control slot lock").clone() {
            return control;
        }
        tokio::task::yield_now().await;
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

fn scenario_request(scenario: CrashScenario) -> RunRequest {
    let mut request = runtime_request();
    request.environment.interactive = matches!(
        scenario,
        CrashScenario::InteractionRequested | CrashScenario::InteractionResolved
    );
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

fn assert_replay_prefix_preserved(
    before: &codewhale_runtime::RunReplay,
    after: &codewhale_runtime::RunReplay,
) {
    assert!(
        after.events.starts_with(&before.events),
        "resume must preserve every committed event byte-for-byte"
    );
    assert!(
        after
            .events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1),
        "event sequence must remain contiguous after resume"
    );
    let mut event_ids = after
        .events
        .iter()
        .map(|event| event.event_id.0.clone())
        .collect::<Vec<_>>();
    let event_count = event_ids.len();
    event_ids.sort();
    event_ids.dedup();
    assert_eq!(
        event_ids.len(),
        event_count,
        "event ids must remain unique after resume"
    );
}

async fn commit_steer_applied_prefix(store: &StateStore, model_marker: &Path, abort_marker: &Path) {
    let created = store
        .create(runtime_request())
        .await
        .expect("create steer-applied crash run");
    let snapshot = &created.replay.snapshot;
    let attempt_id = codewhale_runtime::AttemptId("steer-applied-attempt".to_owned());
    let model_request = ModelRequest {
        run_id: created.lease.run_id.clone(),
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: snapshot.request.system_prompt.clone(),
        messages: snapshot.transcript.project_messages(),
        tools: Vec::new(),
        reasoning_effort: snapshot.request.reasoning_effort,
        max_output_tokens: snapshot.request.max_output_tokens,
        streaming: snapshot.request.streaming,
        request_number: 1,
        attempt: 0,
    };
    for (event_id, event) in [
        (
            "steer-applied-model-prepared",
            RuntimeEventKind::ModelRequestPrepared {
                attempt_id: attempt_id.clone(),
                request: Box::new(model_request),
            },
        ),
        (
            "steer-applied-model-in-flight",
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: attempt_id.clone(),
            },
        ),
        (
            "steer-applied-model-response",
            RuntimeEventKind::ModelResponseCommitted {
                attempt_id,
                output: Box::new(ModelOutput {
                    content: "旧结果".to_owned(),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: one_usage(),
                }),
                accounting: Box::new(ModelAccounting::default()),
            },
        ),
        (
            "steer-queued-before-crash",
            RuntimeEventKind::SteerQueued {
                command_id: CommandId::from("steer-before-crash"),
                content: "改做新任务".to_owned(),
            },
        ),
        (
            "steer-applied-before-crash",
            RuntimeEventKind::SteerApplied {
                command_id: CommandId::from("steer-before-crash"),
                content: "改做新任务".to_owned(),
            },
        ),
    ] {
        store
            .append(
                &created.lease,
                PendingRuntimeEvent {
                    event_id: RuntimeEventId(event_id.to_owned()),
                    event,
                },
            )
            .await
            .expect("commit steer-applied crash prefix");
    }
    append_marker(model_marker, "request");
    append_marker(abort_marker, CrashScenario::SteerApplied.as_str());
    wait_for_parent_kill().await;
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
        if scenario == CrashScenario::SteerApplied {
            commit_steer_applied_prefix(&store, &model_marker, &abort_marker).await;
            unreachable!("steer-applied helper aborts");
        }
        let control_slot = matches!(
            scenario,
            CrashScenario::ToolInFlightControlRequested
                | CrashScenario::InteractionResolved
                | CrashScenario::SteerQueued
        )
        .then(|| Arc::new(Mutex::new(None)));
        let tools = Arc::new(MarkerTools::new(
            tool_marker,
            scenario == CrashScenario::ToolInFlight,
            Some(abort_marker.clone()),
            (scenario == CrashScenario::ToolInFlightControlRequested)
                .then(|| control_slot.as_ref().expect("control slot").clone()),
        ));
        let runtime = Arc::new(AgentRuntime::new(
            Arc::new(MarkerModel::new(model_marker, scenario)),
            tools,
            Arc::new(CrashSink {
                scenario,
                abort_marker,
                control: control_slot.clone(),
            }),
            store,
        ));
        let run = runtime.start(scenario_request(scenario));
        if let Some(control_slot) = control_slot {
            *control_slot.lock().expect("control slot lock") = Some(run.control());
        }
        let outcome = run.wait().await.expect("child runtime join");
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
async fn control_requested_crash_does_not_hide_in_flight_tool_side_effect_ambiguity() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::ToolInFlightControlRequested);
    assert_eq!(marker_count(&fixture.tool_marker), 1);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::ToolInFlightControlRequested);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume control-requested run");
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
        .expect("load control-requested run")
        .expect("control-requested run exists");
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ControlRequested { .. }
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
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
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
async fn interaction_requested_crash_replays_one_pending_approval_before_any_side_effect() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::InteractionRequested);
    assert_eq!(marker_count(&fixture.model_marker), 1);
    assert_eq!(marker_count(&fixture.tool_marker), 0);

    let (runtime, store) = fixture.reopen(CrashScenario::InteractionRequested);
    let before_resume = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load pending interaction run")
        .expect("pending interaction run exists");
    let interaction_id = before_resume
        .snapshot
        .pending_tool
        .as_ref()
        .and_then(|tool| tool.interaction.as_ref())
        .map(|interaction| interaction.request.interaction_id.clone())
        .expect("pending interaction id");
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::InteractionRequested { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::InteractionResolved { .. }
        )),
        0
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::ToolExecutionStarted { .. }
        )),
        0
    );
    assert!(
        before_resume.snapshot.command_receipts.is_empty(),
        "request commit alone must not invent a resolution receipt"
    );
    let run = runtime.resume(RunId::from(RUN_ID));
    run.control()
        .resolve_interaction(
            CommandId::from("resume-process-crash-approval"),
            interaction_id,
            UserInteractionResponse::Approved,
        )
        .await
        .expect("resolve replayed interaction");
    let outcome = run.wait().await.expect("resume pending interaction");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(marker_count(&fixture.model_marker), 2);
    assert_eq!(marker_count(&fixture.tool_marker), 1);

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered interaction run")
        .expect("recovered interaction run exists");
    assert_replay_prefix_preserved(&before_resume, &replay);
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::InteractionRequested { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::InteractionResolved { .. }
        )),
        1
    );
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
        1
    );
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn interaction_resolved_crash_starts_the_approved_tool_exactly_once_after_reopen() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::InteractionResolved);
    assert_eq!(marker_count(&fixture.model_marker), 1);
    assert_eq!(marker_count(&fixture.tool_marker), 0);

    let (runtime, store) = fixture.reopen(CrashScenario::InteractionResolved);
    let before_resume = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load resolved interaction prefix")
        .expect("resolved interaction prefix exists");
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::InteractionRequested { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::InteractionResolved { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::ToolExecutionStarted { .. }
        )),
        0
    );
    let expected_interaction_id = before_resume
        .snapshot
        .pending_tool
        .as_ref()
        .and_then(|tool| tool.interaction.as_ref())
        .map(|interaction| interaction.request.interaction_id.clone())
        .expect("resolved interaction id");
    assert_eq!(before_resume.snapshot.command_receipts.len(), 1);
    assert!(matches!(
        before_resume
            .snapshot
            .pending_tool
            .as_ref()
            .and_then(|tool| tool.interaction.as_ref())
            .and_then(|interaction| interaction.response.as_ref()),
        Some(UserInteractionResponse::Approved)
    ));
    let receipt = before_resume
        .snapshot
        .command_receipts
        .first()
        .expect("resolved interaction command receipt");
    assert_eq!(
        receipt.command_id,
        CommandId::from("process-crash-approval")
    );
    assert!(matches!(
        &receipt.command,
        codewhale_runtime::DurableCommand::ResolveInteraction {
            interaction_id,
            response: UserInteractionResponse::Approved,
        } if interaction_id == &expected_interaction_id
    ));
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume resolved interaction");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(marker_count(&fixture.model_marker), 2);
    assert_eq!(marker_count(&fixture.tool_marker), 1);

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered resolved interaction")
        .expect("resolved interaction run exists");
    assert_replay_prefix_preserved(&before_resume, &replay);
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::InteractionRequested { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::InteractionResolved { .. }
        )),
        1
    );
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
        1
    );
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn steer_queued_crash_never_applies_or_reissues_an_ambiguous_model_request() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::SteerQueued);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::SteerQueued);
    let before_resume = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load queued-steer prefix")
        .expect("queued-steer prefix exists");
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::ModelRequestInFlight { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::SteerQueued { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before_resume, |event| matches!(
            event,
            RuntimeEventKind::SteerApplied { .. }
        )),
        0
    );
    assert_eq!(before_resume.snapshot.pending_steers.len(), 1);
    assert_eq!(before_resume.snapshot.command_receipts.len(), 1);
    let receipt = before_resume
        .snapshot
        .command_receipts
        .first()
        .expect("queued steer command receipt");
    assert_eq!(receipt.command_id, CommandId::from("process-crash-steer"));
    assert!(matches!(
        &receipt.command,
        codewhale_runtime::DurableCommand::Steer { content } if content == "改做新任务"
    ));
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume queued-steer run");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: codewhale_runtime::RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ModelRequest,
                ..
            }
        }
    ));
    assert!(
        outcome.accounting.billing_unknown,
        "an externally killed in-flight request must retain unknown billing"
    );
    assert_eq!(
        marker_count(&fixture.model_marker),
        1,
        "an ambiguous in-flight model request must not be reissued"
    );

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load queued-steer run")
        .expect("queued-steer run exists");
    assert_replay_prefix_preserved(&before_resume, &replay);
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::SteerQueued { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::SteerApplied { .. }
        )),
        0
    );
    assert_eq!(event_count(&replay, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn steer_applied_crash_resumes_with_a_new_model_request_instead_of_old_stop() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::SteerApplied);
    assert_eq!(marker_count(&fixture.model_marker), 1);

    let (runtime, store) = fixture.reopen(CrashScenario::SteerApplied);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume steer-applied run");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(
        marker_count(&fixture.model_marker),
        2,
        "the applied steer must force a new physical model request"
    );

    let replay = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load steer-applied run")
        .expect("steer-applied run exists");
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::SteerQueued { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::SteerApplied { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&replay, |event| matches!(
            event,
            RuntimeEventKind::ModelResponseCommitted { .. }
        )),
        2
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
        event: RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("retry-after-commit-command"),
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
