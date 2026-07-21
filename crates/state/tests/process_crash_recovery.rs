//! Process-boundary recovery tests for the durable Agent runtime.
//!
//! The ignored helper is launched as a real child process. Its event sink or
//! tool writes a ready marker only after the preceding SQLite commit has
//! returned, then blocks until the parent kills it. This exercises external
//! process termination and dead-PID lease takeover rather than an in-process
//! mock.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use codewhale_context::compaction::{ContextInput, effective_context};
use codewhale_protocol::agent_runtime::{ReasoningEffort, RunLimits, ToolPolicy};
use codewhale_protocol::run_api::{
    PendingCreationKind, RunCommand, RunProductControls, StartRunCommand,
};
use codewhale_protocol::task::{
    AcceptanceId, EvidenceLineage, TaskAcceptance, TaskContract, TaskDefinition, TaskGenerationId,
    VerifierEvidencePolicy, VerifierObservation, VerifierPlan, VerifierSpec, VerifierStep,
    VerifierVerdict, WorkspaceRevision,
};
use codewhale_runtime::{
    AGENT_TOOL_NAME, ActorRequestAccounting, AgentActorKind, AgentControl, AgentOrchestrationError,
    AgentOrchestrationErrorKind, AgentOrchestrator, AgentRuntime, AgentTask, AgentWorkspaceAccess,
    AgentWorkspaceAssignment, ApiSurface, ApprovalRisk, CancellationToken, CommandId,
    DurableActionState, ModelAccounting, ModelFinishReason, ModelMessage, ModelOutput, ModelPort,
    ModelPortError, ModelRequest, ModelStream, ModelStreamEvent, ModelToolCall, NullEventSink,
    PendingRuntimeEvent, RecoveryAmbiguityPhase, RunId, RunRequest, RunStore, RunStoreError,
    RuntimeEventId, RuntimeEventKind, RuntimeEventSink, StoredRuntimeEvent, SurfaceUsage,
    TerminalState, ToolApprovalPrompt, ToolArguments, ToolArtifact, ToolDefinition, ToolEvidence,
    ToolEvidenceStatus, ToolExecutionError, ToolExecutor, ToolInvocation, ToolOutcome, Usage,
    UserInteractionResponse, VerificationArtifactPayload, WorkspaceState, WriteExecutionMode,
    WriterArtifactState, WriterBinding, WriterCleanupMode, WriterCleanupOwnership,
    WriterCleanupPhase, WriterCleanupPlan, WriterCleanupResult, WriterCleanupScope,
    WriterIntegration, WriterPlan, WriterPreparation, WriterRemovalState, WriterSeal,
    reduce_events, writer_path_set_sha256,
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
const CHILD_WRITER_MARKER: &str = "CODEWHALE_CRASH_TEST_WRITER_MARKER";
const RUN_ID: &str = "process-crash-run";
const CREATE_COMMAND_ID: &str = "process-crash-create-command";
const CREATE_COMMAND_SHA256: &str = "sha256:process-crash-create-payload";
const TOOL_NAME: &str = "write_marker";
const TEMPORAL_WRITE_TOOL: &str = "temporal_write";
const HOST_VERIFIER_ACCEPTANCE_ID: &str = "process-crash-verifier";
const HOST_VERIFIER_REVISION: &str = "sha256:process-crash-workspace";
const TEMPORAL_BROKEN_REVISION: &str = "sha256:process-crash-broken";
const TEMPORAL_FIXED_REVISION: &str = "sha256:process-crash-fixed";
const WRITER_BASE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const WRITER_FINAL_COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WRITER_DIRTY_REVISION: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const WRITER_SEALED_SCOPE_REVISION: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const WRITER_DIFF_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const WRITER_ROOT_WORKSPACE: &str = "/tmp/codewhale-process-crash-writer-root";
const WRITER_WORKSPACE: &str = "/tmp/codewhale-process-crash-writer-owned";
const WRITER_BRANCH: &str = "codewhale/writer/process-crash";
const WRITER_OWNER: &str = "process-crash-writer-owner";
const WRITER_WRITE_TOOL: &str = "writer_write";
const WRITER_VERIFY_TOOL: &str = "run_tests";

fn creation_intent() -> codewhale_runtime::CreationIntent {
    let command = StartRunCommand {
        task: TaskDefinition::host("执行进程恢复测试"),
        workspace: "/tmp/codewhale-process-crash-test".to_owned(),
        model: Some("deepseek-chat".to_owned()),
        reasoning_effort: ReasoningEffort::default(),
        max_output_tokens: None,
        max_api_requests: None,
        streaming: false,
        tool_policy: ToolPolicy::default(),
        limits: RunLimits::default(),
        controls: RunProductControls::default(),
    };
    codewhale_runtime::CreationIntent {
        kind: PendingCreationKind::Start,
        workspace: command.workspace.clone(),
        source_run_id: None,
        command: RunCommand::Start(command),
    }
}

fn host_verifier_spec() -> VerifierSpec {
    VerifierSpec {
        verifier_id: TOOL_NAME.to_owned(),
        parameters: json!({"mode": "process_crash"}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "write-deterministic-marker".to_owned(),
                program: "process-crash-verifier".to_owned(),
                args: vec!["--mode".to_owned(), "process_crash".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 10_000,
            }],
        },
    }
}

fn writer_verifier_spec() -> VerifierSpec {
    VerifierSpec {
        verifier_id: WRITER_VERIFY_TOOL.to_owned(),
        parameters: json!({"suite": "writer-process-crash"}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "writer-process-crash".to_owned(),
                program: "fixture-verifier".to_owned(),
                args: vec!["--exact".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 10_000,
            }],
        },
    }
}

fn is_host_verification_scenario(scenario: CrashScenario) -> bool {
    matches!(
        scenario,
        CrashScenario::HostVerificationPrepared
            | CrashScenario::HostVerificationInFlight
            | CrashScenario::HostVerificationCommitted
            | CrashScenario::TemporalFailureCommitted
            | CrashScenario::TemporalMutationCommitted
            | CrashScenario::TemporalHostVerificationCommitted
    )
}

fn is_temporal_verification_scenario(scenario: CrashScenario) -> bool {
    matches!(
        scenario,
        CrashScenario::TemporalFailureCommitted
            | CrashScenario::TemporalMutationCommitted
            | CrashScenario::TemporalHostVerificationCommitted
    )
}

fn is_writer_scenario(scenario: CrashScenario) -> bool {
    matches!(
        scenario,
        CrashScenario::WriterTaskPrepared
            | CrashScenario::WriterCreateSideEffect
            | CrashScenario::WriterRunning
            | CrashScenario::WriterSealSideEffect
            | CrashScenario::WriterSealCommitted
            | CrashScenario::WriterIntegrationPrepared
            | CrashScenario::WriterIntegrationStarted
            | CrashScenario::WriterIntegrationSideEffect
            | CrashScenario::WriterIntegrationCommitted
            | CrashScenario::WriterCleanupPrepared
            | CrashScenario::WriterCleanupSideEffect
            | CrashScenario::WriterFailedCleanupPrepared
            | CrashScenario::WriterFailedCleanupSideEffect
            | CrashScenario::WriterCleanupResultCommitted
            | CrashScenario::WriterDelegatedReceiptCommitted
    )
}

fn is_writer_resume_scenario(scenario: CrashScenario) -> bool {
    matches!(
        scenario,
        CrashScenario::WriterFailedCleanupPrepared
            | CrashScenario::WriterFailedCleanupSideEffect
            | CrashScenario::WriterCleanupResultCommitted
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashScenario {
    ModelPrepared,
    ModelInFlight,
    ToolInFlight,
    ToolInFlightControlRequested,
    ModelResponseCommitted,
    TerminalModelResponseCommitted,
    InteractionRequested,
    InteractionResolved,
    SteerQueued,
    SteerApplied,
    CompactionCommitted,
    HostVerificationPrepared,
    HostVerificationInFlight,
    HostVerificationCommitted,
    TemporalFailureCommitted,
    TemporalMutationCommitted,
    TemporalHostVerificationCommitted,
    WriterTaskPrepared,
    WriterCreateSideEffect,
    WriterRunning,
    WriterSealSideEffect,
    WriterSealCommitted,
    WriterIntegrationPrepared,
    WriterIntegrationStarted,
    WriterIntegrationSideEffect,
    WriterIntegrationCommitted,
    WriterCleanupPrepared,
    WriterCleanupSideEffect,
    WriterFailedCleanupPrepared,
    WriterFailedCleanupSideEffect,
    WriterCleanupResultCommitted,
    WriterDelegatedReceiptCommitted,
    CreationReserved,
    TerminalCommitted,
}

impl CrashScenario {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelPrepared => "model_prepared",
            Self::ModelInFlight => "model_in_flight",
            Self::ToolInFlight => "tool_in_flight",
            Self::ToolInFlightControlRequested => "tool_in_flight_control_requested",
            Self::ModelResponseCommitted => "model_response_committed",
            Self::TerminalModelResponseCommitted => "terminal_model_response_committed",
            Self::InteractionRequested => "interaction_requested",
            Self::InteractionResolved => "interaction_resolved",
            Self::SteerQueued => "steer_queued",
            Self::SteerApplied => "steer_applied",
            Self::CompactionCommitted => "compaction_committed",
            Self::HostVerificationPrepared => "host_verification_prepared",
            Self::HostVerificationInFlight => "host_verification_in_flight",
            Self::HostVerificationCommitted => "host_verification_committed",
            Self::TemporalFailureCommitted => "temporal_failure_committed",
            Self::TemporalMutationCommitted => "temporal_mutation_committed",
            Self::TemporalHostVerificationCommitted => "temporal_host_verification_committed",
            Self::WriterTaskPrepared => "writer_task_prepared",
            Self::WriterCreateSideEffect => "writer_create_side_effect",
            Self::WriterRunning => "writer_running",
            Self::WriterSealSideEffect => "writer_seal_side_effect",
            Self::WriterSealCommitted => "writer_seal_committed",
            Self::WriterIntegrationPrepared => "writer_integration_prepared",
            Self::WriterIntegrationStarted => "writer_integration_started",
            Self::WriterIntegrationSideEffect => "writer_integration_side_effect",
            Self::WriterIntegrationCommitted => "writer_integration_committed",
            Self::WriterCleanupPrepared => "writer_cleanup_prepared",
            Self::WriterCleanupSideEffect => "writer_cleanup_side_effect",
            Self::WriterFailedCleanupPrepared => "writer_failed_cleanup_prepared",
            Self::WriterFailedCleanupSideEffect => "writer_failed_cleanup_side_effect",
            Self::WriterCleanupResultCommitted => "writer_cleanup_result_committed",
            Self::WriterDelegatedReceiptCommitted => "writer_delegated_receipt_committed",
            Self::CreationReserved => "creation_reserved",
            Self::TerminalCommitted => "terminal_committed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "model_prepared" => Self::ModelPrepared,
            "model_in_flight" => Self::ModelInFlight,
            "tool_in_flight" => Self::ToolInFlight,
            "tool_in_flight_control_requested" => Self::ToolInFlightControlRequested,
            "model_response_committed" => Self::ModelResponseCommitted,
            "terminal_model_response_committed" => Self::TerminalModelResponseCommitted,
            "interaction_requested" => Self::InteractionRequested,
            "interaction_resolved" => Self::InteractionResolved,
            "steer_queued" => Self::SteerQueued,
            "steer_applied" => Self::SteerApplied,
            "compaction_committed" => Self::CompactionCommitted,
            "host_verification_prepared" => Self::HostVerificationPrepared,
            "host_verification_in_flight" => Self::HostVerificationInFlight,
            "host_verification_committed" => Self::HostVerificationCommitted,
            "temporal_failure_committed" => Self::TemporalFailureCommitted,
            "temporal_mutation_committed" => Self::TemporalMutationCommitted,
            "temporal_host_verification_committed" => Self::TemporalHostVerificationCommitted,
            "writer_task_prepared" => Self::WriterTaskPrepared,
            "writer_create_side_effect" => Self::WriterCreateSideEffect,
            "writer_running" => Self::WriterRunning,
            "writer_seal_side_effect" => Self::WriterSealSideEffect,
            "writer_seal_committed" => Self::WriterSealCommitted,
            "writer_integration_prepared" => Self::WriterIntegrationPrepared,
            "writer_integration_started" => Self::WriterIntegrationStarted,
            "writer_integration_side_effect" => Self::WriterIntegrationSideEffect,
            "writer_integration_committed" => Self::WriterIntegrationCommitted,
            "writer_cleanup_prepared" => Self::WriterCleanupPrepared,
            "writer_cleanup_side_effect" => Self::WriterCleanupSideEffect,
            "writer_failed_cleanup_prepared" => Self::WriterFailedCleanupPrepared,
            "writer_failed_cleanup_side_effect" => Self::WriterFailedCleanupSideEffect,
            "writer_cleanup_result_committed" => Self::WriterCleanupResultCommitted,
            "writer_delegated_receipt_committed" => Self::WriterDelegatedReceiptCommitted,
            "creation_reserved" => Self::CreationReserved,
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
    writer_marker: PathBuf,
    abort_marker: PathBuf,
}

impl CrashFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create crash-test directory");
        Self {
            db: temp.path().join("state.db"),
            model_marker: temp.path().join("model-requests.log"),
            tool_marker: temp.path().join("tool-side-effects.log"),
            writer_marker: temp.path().join("writer-side-effects.log"),
            abort_marker: temp.path().join("abort.log"),
            _temp: temp,
        }
    }

    fn crash_child(&self, scenario: CrashScenario) {
        let expected_ready_count =
            marker_line_count(&self.abort_marker, scenario.as_str()).saturating_add(1);
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
                .env(CHILD_WRITER_MARKER, &self.writer_marker)
                .env(CHILD_ABORT_MARKER, &self.abort_marker)
                .spawn()
                .expect("launch crash helper");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if marker_line_count(&self.abort_marker, scenario.as_str()) == expected_ready_count {
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
            marker_line_count(&self.abort_marker, scenario.as_str()),
            expected_ready_count,
            "the intended post-commit crash point must be reached"
        );
        if scenario == CrashScenario::CreationReserved {
            let connection =
                Connection::open(&self.db).expect("open crash database for reservation assertion");
            let (command_sha256, run_id) = connection
                .query_row(
                    r#"
                    SELECT command_sha256, run_id
                    FROM agent_run_creations
                    WHERE command_id = ?1
                    "#,
                    params![CREATE_COMMAND_ID],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .expect("read committed creation reservation");
            assert_eq!(command_sha256, CREATE_COMMAND_SHA256);
            assert_eq!(run_id, RUN_ID);
            let run_count = connection
                .query_row(
                    "SELECT COUNT(*) FROM agent_runs WHERE run_id = ?1",
                    params![RUN_ID],
                    |row| row.get::<_, i64>(0),
                )
                .expect("count runs before RunCreated");
            assert_eq!(
                run_count, 0,
                "reservation commit must precede the RunCreated transaction"
            );
            return;
        }
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
        let (runtime, store, _) = self.reopen_with_model(scenario);
        (runtime, store)
    }

    fn reopen_with_model(
        &self,
        scenario: CrashScenario,
    ) -> (Arc<AgentRuntime>, Arc<StateStore>, Arc<MarkerModel>) {
        let store = Arc::new(StateStore::open(Some(self.db.clone())).expect("reopen SQLite store"));
        let model = Arc::new(MarkerModel::new(self.model_marker.clone(), scenario));
        let runtime = Arc::new(AgentRuntime::new(
            model.clone(),
            Arc::new(MarkerTools::new(
                self.tool_marker.clone(),
                false,
                None,
                None,
                scenario,
            )),
            Arc::new(NullEventSink),
            store.clone(),
        ));
        (runtime, store, model)
    }

    fn reopen_writer(
        &self,
        scenario: CrashScenario,
    ) -> (Arc<AgentRuntime>, Arc<StateStore>, Arc<WriterMarkerModel>) {
        let store = Arc::new(StateStore::open(Some(self.db.clone())).expect("reopen SQLite store"));
        let model = Arc::new(WriterMarkerModel::new(self.model_marker.clone(), scenario));
        let root_tools = Arc::new(ProcessWriterTools::root(
            self.tool_marker.clone(),
            self.writer_marker.clone(),
        ));
        let orchestrator = Arc::new(ProcessWriterOrchestrator::new(
            scenario,
            self.writer_marker.clone(),
            self.tool_marker.clone(),
            self.abort_marker.clone(),
        ));
        let runtime = Arc::new(
            AgentRuntime::new(
                model.clone(),
                root_tools,
                Arc::new(NullEventSink),
                store.clone(),
            )
            .with_orchestrator(orchestrator),
        );
        (runtime, store, model)
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
    observed_requests: Mutex<Vec<ModelRequest>>,
}

impl MarkerModel {
    fn new(marker: PathBuf, scenario: CrashScenario) -> Self {
        Self {
            marker,
            scenario,
            ledger: Arc::new(ModelLedger::default()),
            observed_requests: Mutex::new(Vec::new()),
        }
    }

    fn observed_requests(&self) -> Vec<ModelRequest> {
        self.observed_requests
            .lock()
            .expect("observed model requests lock")
            .clone()
    }
}

#[async_trait]
impl ModelPort for MarkerModel {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        self.observed_requests
            .lock()
            .expect("observed model requests lock")
            .push(request.clone());
        if matches!(
            self.scenario,
            CrashScenario::SteerQueued | CrashScenario::SteerApplied
        ) && marker_count(&self.marker) > 0
        {
            assert_eq!(
                request
                    .messages
                    .iter()
                    .filter(|message| matches!(
                        message,
                        ModelMessage::User { content } if content == "改做新任务"
                    ))
                    .count(),
                1,
                "the applied steer must remain in canonical history exactly once"
            );
            assert!(matches!(
                request.messages.last(),
                Some(ModelMessage::User { content }) if content.starts_with("## 当前 Host 事实")
            ));
        }
        let temporal = is_temporal_verification_scenario(self.scenario);
        let request_number = if temporal {
            request.request_number.saturating_sub(1) as usize
        } else {
            marker_count(&self.marker)
        };
        let request_marker = if temporal {
            format!("temporal-model:{}", request.request_number)
        } else {
            "request".to_owned()
        };
        append_marker(&self.marker, &request_marker);
        self.ledger.started.fetch_add(1, Ordering::AcqRel);
        if self.scenario == CrashScenario::SteerQueued {
            return Ok(Box::new(PendingStream));
        }
        let output = if is_temporal_verification_scenario(self.scenario) && request_number == 1 {
            ModelOutput {
                content: String::new(),
                reasoning_content: Some("根据已持久化的失败事实修复工作区".to_owned()),
                tool_calls: vec![ModelToolCall {
                    id: "temporal-write-call".to_owned(),
                    name: TEMPORAL_WRITE_TOOL.to_owned(),
                    arguments: ToolArguments::parse("{}"),
                }],
                finish_reason: ModelFinishReason::ToolCalls,
                usage: one_usage(),
            }
        } else if matches!(
            self.scenario,
            CrashScenario::ToolInFlight | CrashScenario::ToolInFlightControlRequested
        ) || matches!(
            self.scenario,
            CrashScenario::InteractionRequested | CrashScenario::InteractionResolved
        ) && request_number == 0
            || self.scenario == CrashScenario::TerminalModelResponseCommitted
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

struct WriterMarkerModel {
    marker: PathBuf,
    scenario: CrashScenario,
    ledger: Arc<ModelLedger>,
    observed_requests: Mutex<Vec<ModelRequest>>,
}

impl WriterMarkerModel {
    fn new(marker: PathBuf, scenario: CrashScenario) -> Self {
        Self {
            marker,
            scenario,
            ledger: Arc::new(ModelLedger::default()),
            observed_requests: Mutex::new(Vec::new()),
        }
    }

    fn observed_requests(&self) -> Vec<ModelRequest> {
        self.observed_requests
            .lock()
            .expect("writer model request lock")
            .clone()
    }
}

#[async_trait]
impl ModelPort for WriterMarkerModel {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        self.observed_requests
            .lock()
            .expect("writer model request lock")
            .push(request.clone());
        append_marker(
            &self.marker,
            match request.actor.kind {
                AgentActorKind::Root => "writer-root-model",
                AgentActorKind::Child => "writer-child-model",
            },
        );
        self.ledger.started.fetch_add(1, Ordering::AcqRel);
        let temporal_writer = self.scenario == CrashScenario::WriterDelegatedReceiptCommitted;
        let tool_calls = match (request.actor.kind, request.request_number) {
            (AgentActorKind::Root, 1) => vec![ModelToolCall {
                id: "writer-agent-call".to_owned(),
                name: AGENT_TOOL_NAME.to_owned(),
                arguments: ToolArguments::from_value(json!({
                    "prompt": "只修改 src/lib.rs 并通过冻结验证",
                    "type": "implementer",
                    "workspace_access": "isolated_write",
                    "allowed_paths": ["src/lib.rs"],
                    "expected_artifact": "一个 Host seal 的提交"
                })),
            }],
            (AgentActorKind::Child, 1) if temporal_writer => vec![ModelToolCall {
                id: "writer-temporal-failure".to_owned(),
                name: WRITER_VERIFY_TOOL.to_owned(),
                arguments: ToolArguments::from_value(json!({"verifier_id": "writer-tests"})),
            }],
            (AgentActorKind::Child, 2) if temporal_writer => vec![ModelToolCall {
                id: "writer-edit-call".to_owned(),
                name: WRITER_WRITE_TOOL.to_owned(),
                arguments: ToolArguments::from_value(json!({"path": "src/lib.rs"})),
            }],
            (AgentActorKind::Child, 1) => vec![ModelToolCall {
                id: "writer-edit-call".to_owned(),
                name: WRITER_WRITE_TOOL.to_owned(),
                arguments: ToolArguments::from_value(json!({"path": "src/lib.rs"})),
            }],
            _ => Vec::new(),
        };
        let output = ModelOutput {
            content: if tool_calls.is_empty() {
                match request.actor.kind {
                    AgentActorKind::Root => "根任务完成".to_owned(),
                    AgentActorKind::Child => "writer 子任务完成".to_owned(),
                }
            } else {
                String::new()
            },
            reasoning_content: None,
            finish_reason: if tool_calls.is_empty() {
                ModelFinishReason::Stop
            } else {
                ModelFinishReason::ToolCalls
            },
            tool_calls,
            usage: one_usage(),
        };
        Ok(Box::new(OneShotStream {
            output: Some(output),
            ledger: self.ledger.clone(),
        }))
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        let started = self.ledger.started.load(Ordering::Acquire);
        let completed = self.ledger.completed.load(Ordering::Acquire);
        Ok(ModelAccounting {
            hard_request_limit: Some(32),
            root: ActorRequestAccounting {
                started,
                completed,
                in_flight: started.saturating_sub(completed),
                retries: 0,
            },
            complete: started == completed,
            usage_complete: started == completed,
            sealed: seal,
            usage: *self.ledger.usage.lock().expect("writer usage lock"),
            ..ModelAccounting::default()
        })
    }
}

struct ProcessWriterTools {
    root: bool,
    scenario: CrashScenario,
    tool_marker: PathBuf,
    writer_marker: PathBuf,
    abort_marker: Option<PathBuf>,
}

impl ProcessWriterTools {
    fn root(tool_marker: PathBuf, writer_marker: PathBuf) -> Self {
        Self {
            root: true,
            scenario: CrashScenario::WriterTaskPrepared,
            tool_marker,
            writer_marker,
            abort_marker: None,
        }
    }

    fn writer(
        scenario: CrashScenario,
        tool_marker: PathBuf,
        writer_marker: PathBuf,
        abort_marker: PathBuf,
    ) -> Self {
        Self {
            root: false,
            scenario,
            tool_marker,
            writer_marker,
            abort_marker: Some(abort_marker),
        }
    }

    fn revision(&self) -> &'static str {
        if self.root {
            if marker_line_count(&self.writer_marker, "integration") > 0 {
                WRITER_FINAL_COMMIT
            } else {
                WRITER_BASE_COMMIT
            }
        } else if marker_line_count(&self.writer_marker, "seal") > 0 {
            WRITER_FINAL_COMMIT
        } else if marker_line_count(&self.writer_marker, "write") > 0 {
            WRITER_DIRTY_REVISION
        } else {
            WRITER_BASE_COMMIT
        }
    }
}

#[async_trait]
impl ToolExecutor for ProcessWriterTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![ToolDefinition {
            name: WRITER_VERIFY_TOOL.to_owned(),
            description: "deterministic writer verifier".to_owned(),
            input_schema: json!({"type": "object"}),
        }];
        if !self.root {
            definitions.push(ToolDefinition {
                name: WRITER_WRITE_TOOL.to_owned(),
                description: "write the isolated fixture".to_owned(),
                input_schema: json!({"type": "object"}),
            });
        }
        definitions
    }

    fn definition_workspace_access(&self, name: &str) -> codewhale_runtime::WorkspaceAccess {
        if matches!(name, WRITER_WRITE_TOOL | WRITER_VERIFY_TOOL) {
            codewhale_runtime::WorkspaceAccess::MayWrite
        } else {
            codewhale_runtime::WorkspaceAccess::ReadOnly
        }
    }

    fn workspace_access(&self, invocation: &ToolInvocation) -> codewhale_runtime::WorkspaceAccess {
        if matches!(
            invocation.name.as_str(),
            WRITER_WRITE_TOOL | WRITER_VERIFY_TOOL
        ) {
            codewhale_runtime::WorkspaceAccess::MayWrite
        } else {
            codewhale_runtime::WorkspaceAccess::ReadOnly
        }
    }

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        Ok(self.revision().to_owned())
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        match invocation.name.as_str() {
            WRITER_WRITE_TOOL if !self.root => {
                if self.scenario == CrashScenario::WriterDelegatedReceiptCommitted {
                    append_marker(&self.writer_marker, "write");
                } else {
                    append_marker_once(&self.writer_marker, "write");
                }
                append_marker(&self.tool_marker, "writer-write");
                if self.scenario == CrashScenario::WriterRunning {
                    append_marker(
                        self.abort_marker
                            .as_ref()
                            .expect("writer running crash marker"),
                        self.scenario.as_str(),
                    );
                    wait_for_parent_kill().await;
                }
                Ok(ToolOutcome::success("writer fixture modified")
                    .with_side_effect(codewhale_runtime::ToolSideEffectStatus::Applied))
            }
            WRITER_VERIFY_TOOL => {
                append_marker(
                    &self.tool_marker,
                    if self.root {
                        "root-verifier"
                    } else {
                        "writer-verifier"
                    },
                );
                let revision = self.revision().to_owned();
                if self.root && revision != WRITER_FINAL_COMMIT {
                    return Ok(ToolOutcome::error(
                        "root exact verifier rejected an unintegrated writer revision",
                    ));
                }
                let verifier = VerifierSpec {
                    parameters: invocation
                        .arguments
                        .parsed
                        .expect("writer verifier arguments"),
                    ..writer_verifier_spec()
                };
                let workspace_revision = WorkspaceRevision::Known {
                    sha256: revision.clone(),
                };
                let failed = self.scenario == CrashScenario::WriterDelegatedReceiptCommitted
                    && !self.root
                    && marker_line_count(&self.writer_marker, "write") == 0;
                let verdict = if failed {
                    VerifierVerdict::Failed
                } else {
                    VerifierVerdict::Passed
                };
                let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
                    summary: if failed {
                        "exact writer verifier failed"
                    } else {
                        "exact writer verifier passed"
                    }
                    .to_owned(),
                    verifier: verifier.clone(),
                    verdict,
                    workspace_revision: workspace_revision.clone(),
                });
                let artifact_id = artifact.id.clone();
                let mut outcome = if failed {
                    ToolOutcome::error("exact writer verifier failed")
                } else {
                    ToolOutcome::success("exact writer verifier passed")
                };
                outcome.side_effect = codewhale_runtime::ToolSideEffectStatus::NotApplied;
                outcome.workspace_revision = Some(revision.clone());
                outcome.evidence = ToolEvidence {
                    status: ToolEvidenceStatus::Produced,
                    references: vec![artifact_id.clone()],
                };
                outcome.artifacts = vec![artifact];
                outcome.verifier_observation = Some(VerifierObservation {
                    spec: verifier,
                    verdict,
                    workspace_revision,
                    artifact_ids: vec![artifact_id],
                });
                Ok(outcome)
            }
            other => Err(ToolExecutionError::new(
                "unexpected_writer_tool",
                other.to_owned(),
            )),
        }
    }
}

struct ProcessWriterOrchestrator {
    scenario: CrashScenario,
    writer_marker: PathBuf,
    tool_marker: PathBuf,
    abort_marker: PathBuf,
}

impl ProcessWriterOrchestrator {
    fn new(
        scenario: CrashScenario,
        writer_marker: PathBuf,
        tool_marker: PathBuf,
        abort_marker: PathBuf,
    ) -> Self {
        Self {
            scenario,
            writer_marker,
            tool_marker,
            abort_marker,
        }
    }

    fn assignment(&self) -> AgentWorkspaceAssignment {
        AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::IsolatedWrite,
            root_workspace: WRITER_ROOT_WORKSPACE.to_owned(),
            base_commit: WRITER_BASE_COMMIT.to_owned(),
            worktree_path: Some(WRITER_WORKSPACE.to_owned()),
            root_branch: Some("deepseek-agent".to_owned()),
            branch: Some(WRITER_BRANCH.to_owned()),
            allowed_paths: vec!["src/lib.rs".to_owned()],
            owner_token: Some(WRITER_OWNER.to_owned()),
        }
    }

    async fn crash_after_side_effect(&self) -> ! {
        append_marker(&self.abort_marker, self.scenario.as_str());
        wait_for_parent_kill().await
    }
}

#[async_trait]
impl AgentOrchestrator for ProcessWriterOrchestrator {
    async fn prepare_writer(
        &self,
        request: WriterPreparation,
    ) -> Result<WriterPlan, AgentOrchestrationError> {
        if request.root_workspace != WRITER_ROOT_WORKSPACE
            || request.allowed_paths != ["src/lib.rs"]
        {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_plan_mismatch",
                "writer fixture received an unexpected plan",
            ));
        }
        Ok(WriterPlan {
            assignment: self.assignment(),
        })
    }

    async fn bind_writer(
        &self,
        task: &AgentTask,
        _sealed: Option<&WriterSeal>,
    ) -> Result<WriterBinding, AgentOrchestrationError> {
        if task.workspace != self.assignment() {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_binding_mismatch",
                "writer fixture task changed across recovery",
            ));
        }
        let created = append_marker_once(&self.writer_marker, "create");
        if created && self.scenario == CrashScenario::WriterCreateSideEffect {
            self.crash_after_side_effect().await;
        }
        Ok(WriterBinding {
            assignment: self.assignment(),
            writer_workspace_state: known_workspace(0, WRITER_BASE_COMMIT),
            tools: Arc::new(ProcessWriterTools::writer(
                self.scenario,
                self.tool_marker.clone(),
                self.writer_marker.clone(),
                self.abort_marker.clone(),
            )),
        })
    }

    async fn seal_writer(&self, task: &AgentTask) -> Result<WriterSeal, AgentOrchestrationError> {
        if task.workspace != self.assignment() {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_seal_mismatch",
                "writer fixture seal task changed",
            ));
        }
        let sealed = append_marker_once(&self.writer_marker, "seal");
        if sealed && self.scenario == CrashScenario::WriterSealSideEffect {
            self.crash_after_side_effect().await;
        }
        Ok(WriterSeal {
            base_commit: WRITER_BASE_COMMIT.to_owned(),
            final_commit: WRITER_FINAL_COMMIT.to_owned(),
            diff_sha256: WRITER_DIFF_SHA256.to_owned(),
            changed_files: vec!["src/lib.rs".to_owned()],
            writer_workspace_state: known_workspace(0, WRITER_FINAL_COMMIT),
        })
    }

    async fn integrate_writer(
        &self,
        task: &AgentTask,
        seal: &WriterSeal,
        expected_root: &WorkspaceState,
    ) -> Result<WriterIntegration, AgentOrchestrationError> {
        if task.workspace != self.assignment() || seal.final_commit != WRITER_FINAL_COMMIT {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_integration_mismatch",
                "writer fixture integration identity changed",
            ));
        }
        let integrated = append_marker_once(&self.writer_marker, "integration");
        if integrated && self.scenario == CrashScenario::WriterIntegrationSideEffect {
            self.crash_after_side_effect().await;
        }
        Ok(WriterIntegration {
            root_head_commit: WRITER_FINAL_COMMIT.to_owned(),
            root_workspace_state: known_workspace(
                expected_root.generation.saturating_add(1),
                WRITER_FINAL_COMMIT,
            ),
        })
    }

    async fn inspect_writer_cleanup(
        &self,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
        phase: WriterCleanupPhase,
        reason_code: &str,
    ) -> Result<WriterCleanupPlan, AgentOrchestrationError> {
        if task.workspace != self.assignment() {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_cleanup_mismatch",
                "writer fixture cleanup identity changed",
            ));
        }
        append_marker(&self.writer_marker, "cleanup-inspect");
        let expected_branch_commit = seal
            .map(|seal| seal.final_commit.clone())
            .unwrap_or_else(|| WRITER_BASE_COMMIT.to_owned());
        Ok(WriterCleanupPlan {
            phase,
            reason_code: reason_code.to_owned(),
            ownership: WriterCleanupOwnership::Known {
                identity_sha256: "f".repeat(64),
            },
            artifact_state: seal.map_or(WriterArtifactState::KnownUnsealed, |seal| {
                WriterArtifactState::KnownHostSealed {
                    final_commit: seal.final_commit.clone(),
                    diff_sha256: seal.diff_sha256.clone(),
                }
            }),
            scope: WriterCleanupScope::Known {
                workspace_revision: WorkspaceRevision::Known {
                    sha256: if seal.is_some() {
                        WRITER_SEALED_SCOPE_REVISION
                    } else {
                        WRITER_DIRTY_REVISION
                    }
                    .to_owned(),
                },
                changed_count: 1,
                in_scope_count: 1,
                out_of_scope_count: 0,
                path_set_sha256: writer_path_set_sha256(&["src/lib.rs".to_owned()]).unwrap(),
            },
            mode: WriterCleanupMode::RemoveExact {
                expected_branch_commit,
            },
        })
    }

    async fn execute_writer_cleanup(
        &self,
        task: &AgentTask,
        plan: &WriterCleanupPlan,
    ) -> Result<WriterCleanupResult, AgentOrchestrationError> {
        if task.workspace != self.assignment() {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_fixture_cleanup_mismatch",
                "writer fixture cleanup identity changed",
            ));
        }
        append_marker(
            &self.writer_marker,
            &format!(
                "cleanup-plan:{}",
                serde_json::to_string(plan).expect("serialize cleanup plan marker")
            ),
        );
        append_marker(&self.writer_marker, "cleanup-execute");
        let cleaned = append_marker_once(&self.writer_marker, "cleanup");
        if cleaned
            && matches!(
                self.scenario,
                CrashScenario::WriterCleanupSideEffect
                    | CrashScenario::WriterFailedCleanupSideEffect
            )
        {
            self.crash_after_side_effect().await;
        }
        if cleaned {
            Ok(WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            })
        } else {
            Ok(WriterCleanupResult::AlreadyAbsent)
        }
    }
}

struct MarkerTools {
    marker: PathBuf,
    abort_after_side_effect: bool,
    abort_marker: Option<PathBuf>,
    cancel_control: Option<Arc<Mutex<Option<AgentControl>>>>,
    scenario: CrashScenario,
}

impl MarkerTools {
    fn new(
        marker: PathBuf,
        abort_after_side_effect: bool,
        abort_marker: Option<PathBuf>,
        cancel_control: Option<Arc<Mutex<Option<AgentControl>>>>,
        scenario: CrashScenario,
    ) -> Self {
        Self {
            marker,
            abort_after_side_effect,
            abort_marker,
            cancel_control,
            scenario,
        }
    }

    fn revision(&self) -> &'static str {
        if is_temporal_verification_scenario(self.scenario) {
            if marker_line_count(&self.marker, "temporal-write") > 0 {
                TEMPORAL_FIXED_REVISION
            } else {
                TEMPORAL_BROKEN_REVISION
            }
        } else {
            HOST_VERIFIER_REVISION
        }
    }
}

#[async_trait]
impl ToolExecutor for MarkerTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![ToolDefinition {
            name: TOOL_NAME.to_owned(),
            description: "write a process crash test marker".to_owned(),
            input_schema: json!({"type": "object", "additionalProperties": false}),
        }];
        if is_temporal_verification_scenario(self.scenario) {
            definitions.push(ToolDefinition {
                name: TEMPORAL_WRITE_TOOL.to_owned(),
                description: "apply the deterministic temporal repair".to_owned(),
                input_schema: json!({"type": "object", "additionalProperties": false}),
            });
        }
        definitions
    }

    fn workspace_access(&self, invocation: &ToolInvocation) -> codewhale_runtime::WorkspaceAccess {
        if invocation.name == TEMPORAL_WRITE_TOOL {
            codewhale_runtime::WorkspaceAccess::MayWrite
        } else {
            codewhale_runtime::WorkspaceAccess::ReadOnly
        }
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

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        if is_host_verification_scenario(self.scenario) {
            Ok(self.revision().to_owned())
        } else {
            Err(ToolExecutionError::new(
                "workspace_revision_unavailable",
                "process crash fixture has no workspace revision",
            ))
        }
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        if is_host_verification_scenario(self.scenario) && invocation.call_id.starts_with("host:") {
            append_marker(&self.marker, &invocation.call_id);
            if self.scenario == CrashScenario::HostVerificationInFlight {
                append_marker(
                    self.abort_marker
                        .as_ref()
                        .expect("Host verifier crash requires abort marker"),
                    self.scenario.as_str(),
                );
                wait_for_parent_kill().await;
            }

            let verifier = VerifierSpec {
                parameters: invocation
                    .arguments
                    .parsed
                    .expect("Host verifier arguments must remain canonical"),
                ..host_verifier_spec()
            };
            let failed = is_temporal_verification_scenario(self.scenario)
                && marker_line_count(&self.marker, "temporal-write") == 0;
            let workspace_revision = WorkspaceRevision::Known {
                sha256: self.revision().to_owned(),
            };
            let verdict = if failed {
                VerifierVerdict::Failed
            } else {
                VerifierVerdict::Passed
            };
            let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
                summary: if failed {
                    "deterministic Host verifier failed"
                } else {
                    "deterministic Host verifier passed"
                }
                .to_owned(),
                verifier: verifier.clone(),
                verdict,
                workspace_revision: workspace_revision.clone(),
            });
            let artifact_id = artifact.id.clone();
            let mut outcome = if failed {
                ToolOutcome::error("deterministic Host verifier failed")
            } else {
                ToolOutcome::success("deterministic Host verifier passed")
            };
            outcome.side_effect = codewhale_runtime::ToolSideEffectStatus::NotApplied;
            outcome.workspace_revision = Some(self.revision().to_owned());
            outcome.evidence = ToolEvidence {
                status: ToolEvidenceStatus::Produced,
                references: vec![artifact_id.clone()],
            };
            outcome.artifacts = vec![artifact];
            outcome.verifier_observation = Some(VerifierObservation {
                spec: verifier,
                verdict,
                workspace_revision,
                artifact_ids: vec![artifact_id],
            });
            return Ok(outcome);
        }

        if is_temporal_verification_scenario(self.scenario)
            && invocation.name == TEMPORAL_WRITE_TOOL
        {
            append_marker(&self.marker, "temporal-write");
            return Ok(ToolOutcome::success("temporal repair applied")
                .with_side_effect(codewhale_runtime::ToolSideEffectStatus::Applied));
        }

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
            CrashScenario::ModelPrepared => {
                matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. })
            }
            CrashScenario::ModelInFlight => {
                matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. })
            }
            CrashScenario::ModelResponseCommitted => {
                matches!(event.event, RuntimeEventKind::ModelResponseCommitted { .. })
            }
            CrashScenario::TerminalModelResponseCommitted => {
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
            CrashScenario::CompactionCommitted => matches!(
                event.event,
                RuntimeEventKind::ContextCompactionCommitted { .. }
            ),
            CrashScenario::HostVerificationPrepared => matches!(
                event.event,
                RuntimeEventKind::HostVerificationPrepared { .. }
            ),
            CrashScenario::HostVerificationInFlight => false,
            CrashScenario::HostVerificationCommitted => matches!(
                event.event,
                RuntimeEventKind::HostVerificationCommitted { .. }
            ),
            CrashScenario::TemporalFailureCommitted => matches!(
                event.event,
                RuntimeEventKind::HostVerificationCommitted {
                    receipt: None,
                    ref outcome,
                    ..
                } if outcome.verifier_observation.as_ref().is_some_and(|observation| {
                    observation.verdict == VerifierVerdict::Failed
                })
            ),
            CrashScenario::TemporalMutationCommitted => matches!(
                event.event,
                RuntimeEventKind::ToolOutcomeCommitted { ref name, .. }
                    if name == TEMPORAL_WRITE_TOOL
            ),
            CrashScenario::TemporalHostVerificationCommitted => matches!(
                event.event,
                RuntimeEventKind::HostVerificationCommitted {
                    receipt: Some(ref receipt),
                    ..
                } if matches!(&receipt.lineage, EvidenceLineage::FailedWritePass { .. })
            ),
            CrashScenario::WriterTaskPrepared => {
                matches!(event.event, RuntimeEventKind::AgentTaskPrepared { .. })
            }
            CrashScenario::WriterCreateSideEffect
            | CrashScenario::WriterRunning
            | CrashScenario::WriterSealSideEffect => false,
            CrashScenario::WriterSealCommitted => {
                matches!(event.event, RuntimeEventKind::AgentSealCommitted { .. })
            }
            CrashScenario::WriterIntegrationPrepared => matches!(
                event.event,
                RuntimeEventKind::AgentIntegrationPrepared { .. }
            ),
            CrashScenario::WriterIntegrationStarted => matches!(
                event.event,
                RuntimeEventKind::AgentIntegrationStarted { .. }
            ),
            CrashScenario::WriterIntegrationSideEffect => false,
            CrashScenario::WriterIntegrationCommitted => matches!(
                event.event,
                RuntimeEventKind::AgentIntegrationCommitted { .. }
            ),
            CrashScenario::WriterCleanupPrepared => {
                matches!(event.event, RuntimeEventKind::AgentCleanupPrepared { .. })
            }
            CrashScenario::WriterFailedCleanupPrepared => {
                matches!(event.event, RuntimeEventKind::AgentCleanupPrepared { .. })
            }
            CrashScenario::WriterCleanupSideEffect
            | CrashScenario::WriterFailedCleanupSideEffect => false,
            CrashScenario::WriterCleanupResultCommitted => {
                matches!(event.event, RuntimeEventKind::AgentCleanupCommitted { .. })
            }
            CrashScenario::WriterDelegatedReceiptCommitted => matches!(
                event.event,
                RuntimeEventKind::HostVerificationCommitted {
                    receipt: Some(ref receipt),
                    ..
                } if matches!(
                    &receipt.lineage,
                    EvidenceLineage::DelegatedFailedWritePass { .. }
                )
            ),
            CrashScenario::CreationReserved => false,
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
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId::from(RUN_ID),
            definition: TaskDefinition::host("执行进程恢复测试"),
        },
        "只执行测试脚本",
    );
    request.run_id = Some(RunId::from(RUN_ID));
    request.model = "deepseek-test".to_owned();
    request.environment.workspace = "/tmp/codewhale-process-crash-test".to_owned();
    request.limits.max_turns = 4;
    request.limits.max_model_requests = 4;
    request.limits.max_tool_calls = 4;
    request.context_policy = codewhale_runtime::ContextPolicy {
        hard_input_tokens: 90_000,
    };
    request
}

fn scenario_request(scenario: CrashScenario) -> RunRequest {
    let mut request = runtime_request();
    if is_host_verification_scenario(scenario) {
        request
            .task_contract
            .as_mut()
            .expect("Agent task contract")
            .definition
            .acceptance = vec![TaskAcceptance::Verifier {
            id: AcceptanceId::from(HOST_VERIFIER_ACCEPTANCE_ID),
            description: "冻结的进程级 Host verifier 必须通过".to_owned(),
            evidence_policy: if is_temporal_verification_scenario(scenario) {
                VerifierEvidencePolicy::FailedWritePass
            } else {
                VerifierEvidencePolicy::LatestPass
            },
            verifier: host_verifier_spec(),
        }];
    }
    if matches!(
        scenario,
        CrashScenario::ModelPrepared | CrashScenario::TerminalModelResponseCommitted
    ) {
        request.limits.max_turns = 1;
        request.limits.max_model_requests = 1;
    }
    request.environment.interactive = matches!(
        scenario,
        CrashScenario::InteractionRequested | CrashScenario::InteractionResolved
    );
    if scenario == CrashScenario::CompactionCommitted {
        request
            .task_contract
            .as_mut()
            .expect("Agent task contract")
            .definition
            .objective = "继续完成当前编码任务".to_owned();
        request
            .transcript
            .entries
            .push(codewhale_runtime::TranscriptEntry::System {
                prompt: request.system_prompt.clone(),
            });
        for turn in 0..6 {
            request
                .transcript
                .entries
                .push(codewhale_runtime::TranscriptEntry::User {
                    content: format!("历史用户约束 {turn} {}", "甲".repeat(600)),
                });
            request
                .transcript
                .entries
                .push(codewhale_runtime::TranscriptEntry::Assistant {
                    content: Some(format!("历史处理结果 {turn}")),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                });
        }
        request.context_policy = codewhale_runtime::ContextPolicy {
            hard_input_tokens: 2_000,
        };
    }
    request
}

fn writer_request(scenario: CrashScenario) -> RunRequest {
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId::from(RUN_ID),
            definition: TaskDefinition {
                objective: "让隔离 writer 修改一个文件".to_owned(),
                constraints: vec!["只修改 src/lib.rs".to_owned()],
                non_goals: vec!["不得修改 root 工作区中的其他文件".to_owned()],
                acceptance: vec![TaskAcceptance::Verifier {
                    id: AcceptanceId::from("writer-tests"),
                    description: "冻结的 writer 验证必须通过".to_owned(),
                    evidence_policy: if scenario == CrashScenario::WriterDelegatedReceiptCommitted {
                        VerifierEvidencePolicy::FailedWritePass
                    } else {
                        VerifierEvidencePolicy::LatestPass
                    },
                    verifier: writer_verifier_spec(),
                }],
            },
        },
        "只执行 canonical writer crash fixture",
    );
    request.run_id = Some(RunId::from(RUN_ID));
    request.model = "deepseek-test".to_owned();
    request.environment.workspace = WRITER_ROOT_WORKSPACE.to_owned();
    request.environment.write_execution_mode = WriteExecutionMode::IsolatedWriter;
    request.environment.auto_approve = true;
    request.limits.max_turns = 12;
    request.limits.max_model_requests = 16;
    request.limits.max_tool_calls = 16;
    request.limits.max_concurrent_children = 1;
    request.context_policy = codewhale_runtime::ContextPolicy {
        hard_input_tokens: 90_000,
    };
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

fn append_marker_once(path: &Path, value: &str) -> bool {
    if marker_line_count(path, value) > 0 {
        return false;
    }
    append_marker(path, value);
    true
}

fn marker_count(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|content| content.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

fn marker_line_count(path: &Path, expected: &str) -> usize {
    marker_lines(path)
        .iter()
        .filter(|line| line.as_str() == expected)
        .count()
}

fn marker_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn known_workspace(generation: u64, sha256: &str) -> WorkspaceState {
    WorkspaceState {
        generation,
        revision: WorkspaceRevision::Known {
            sha256: sha256.to_owned(),
        },
    }
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

fn assert_projection_request(
    request: &ModelRequest,
    projection: &codewhale_runtime::ContextProjection,
    base_prompt: &codewhale_runtime::SystemPrompt,
) {
    assert!(request.streaming);
    assert!(!request.tools.is_empty());
    assert!(
        request.messages.starts_with(&projection.messages),
        "the request must start with the exact durable projection"
    );
    assert_eq!(
        request.messages.len(),
        projection.messages.len() + 1,
        "the only non-canonical suffix is the deterministic current Host-facts message"
    );
    assert!(
        request.system_prompt == *base_prompt,
        "deterministic compaction must preserve the stable system prompt byte-for-byte"
    );
}

async fn commit_steer_applied_prefix(store: &StateStore, model_marker: &Path, abort_marker: &Path) {
    let created = store
        .create(runtime_request())
        .await
        .expect("create steer-applied crash run");
    let snapshot = &created.replay.snapshot;
    let attempt_id = codewhale_runtime::AttemptId("steer-applied-attempt".to_owned());
    let tools = Vec::new();
    let context = effective_context(ContextInput {
        transcript: &snapshot.transcript,
        projection: snapshot.context_projection.as_ref(),
        task_contract: snapshot.request.task_contract.as_ref(),
        workspace_state: &snapshot.workspace_state,
        evidence_receipts: &snapshot.evidence_receipts,
        last_completion_rejection: snapshot.last_completion_rejection.as_ref(),
        last_verifier_failure: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.outcome),
        last_verifier_failure_workspace: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.workspace_state),
        tools: &tools,
    })
    .expect("build canonical steer-applied request projection");
    let model_request = ModelRequest {
        run_id: created.lease.run_id.clone(),
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools,
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

async fn commit_creation_reservation_prefix(store: &StateStore, abort_marker: &Path) {
    let reserved = store
        .reserve_creation(
            &CommandId::from(CREATE_COMMAND_ID),
            CREATE_COMMAND_SHA256,
            RunId::from(RUN_ID),
            creation_intent(),
        )
        .await
        .expect("commit creation reservation");
    assert!(reserved.newly_reserved);
    assert_eq!(reserved.reservation.run_id, RunId::from(RUN_ID));
    assert!(
        store
            .load(&reserved.reservation.run_id)
            .await
            .expect("verify RunCreated absence")
            .is_none(),
        "the crash marker must be written before RunCreated is committed"
    );
    append_marker(abort_marker, CrashScenario::CreationReserved.as_str());
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
    let writer_marker =
        PathBuf::from(std::env::var_os(CHILD_WRITER_MARKER).expect("child writer marker"));
    let abort_marker =
        PathBuf::from(std::env::var_os(CHILD_ABORT_MARKER).expect("child abort marker"));
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build child runtime");
    tokio.block_on(async move {
        let store = Arc::new(StateStore::open(Some(db)).expect("open child SQLite store"));
        if scenario == CrashScenario::CreationReserved {
            commit_creation_reservation_prefix(&store, &abort_marker).await;
            unreachable!("creation-reserved helper aborts");
        }
        if scenario == CrashScenario::SteerApplied {
            commit_steer_applied_prefix(&store, &model_marker, &abort_marker).await;
            unreachable!("steer-applied helper aborts");
        }
        if is_writer_scenario(scenario) {
            let model = Arc::new(WriterMarkerModel::new(model_marker, scenario));
            let root_tools = Arc::new(ProcessWriterTools::root(
                tool_marker.clone(),
                writer_marker.clone(),
            ));
            let orchestrator = Arc::new(ProcessWriterOrchestrator::new(
                scenario,
                writer_marker,
                tool_marker,
                abort_marker.clone(),
            ));
            let runtime = Arc::new(
                AgentRuntime::new(
                    model,
                    root_tools,
                    Arc::new(CrashSink {
                        scenario,
                        abort_marker,
                        control: None,
                    }),
                    store,
                )
                .with_orchestrator(orchestrator),
            );
            let run = if is_writer_resume_scenario(scenario) {
                runtime.resume(RunId::from(RUN_ID))
            } else {
                runtime.start(writer_request(scenario))
            };
            let outcome = run.wait().await.expect("writer crash helper join");
            panic!("writer crash helper unexpectedly completed: {outcome:?}");
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
            scenario,
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

fn assert_writer_lifecycle_is_single(
    replay: &codewhale_runtime::RunReplay,
    fixture: &CrashFixture,
) {
    assert_eq!(
        replay.snapshot.agent_tasks.len(),
        1,
        "one root agent tool call must own exactly one writer lifecycle"
    );
    let task = &replay.snapshot.agent_tasks[0].task;
    assert_eq!(
        task.workspace.worktree_path.as_deref(),
        Some(WRITER_WORKSPACE)
    );
    assert_eq!(task.workspace.branch.as_deref(), Some(WRITER_BRANCH));
    assert_eq!(task.workspace.owner_token.as_deref(), Some(WRITER_OWNER));
    for (label, count) in [
        (
            "AgentTaskPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentTaskPrepared { .. })
            }),
        ),
        (
            "AgentWorkspaceCreated",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentWorkspaceCreated { .. })
            }),
        ),
        (
            "AgentSealPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentSealPrepared { .. })
            }),
        ),
        (
            "AgentSealCommitted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentSealCommitted { .. })
            }),
        ),
        (
            "AgentIntegrationPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationPrepared { .. })
            }),
        ),
        (
            "AgentIntegrationStarted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationStarted { .. })
            }),
        ),
        (
            "AgentIntegrationCommitted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationCommitted { .. })
            }),
        ),
        (
            "AgentCleanupPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentCleanupPrepared { .. })
            }),
        ),
        (
            "AgentCleanupCommitted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentCleanupCommitted { .. })
            }),
        ),
    ] {
        assert!(
            count <= 1,
            "{label} must not be committed more than once, observed {count}"
        );
    }
    for side_effect in ["create", "write", "seal", "integration", "cleanup"] {
        assert!(
            marker_line_count(&fixture.writer_marker, side_effect) <= 1,
            "writer side effect '{side_effect}' must not run twice"
        );
    }
}

fn assert_failed_writer_never_sealed_or_integrated(replay: &codewhale_runtime::RunReplay) {
    for (label, count) in [
        (
            "AgentSealPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentSealPrepared { .. })
            }),
        ),
        (
            "AgentSealCommitted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentSealCommitted { .. })
            }),
        ),
        (
            "AgentIntegrationPrepared",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationPrepared { .. })
            }),
        ),
        (
            "AgentIntegrationStarted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationStarted { .. })
            }),
        ),
        (
            "AgentIntegrationFailed",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationFailed { .. })
            }),
        ),
        (
            "AgentIntegrationCommitted",
            event_count(replay, |event| {
                matches!(event, RuntimeEventKind::AgentIntegrationCommitted { .. })
            }),
        ),
    ] {
        assert_eq!(count, 0, "failed Writer must not emit {label}");
    }
    let lifecycle = replay
        .snapshot
        .agent_tasks
        .first()
        .expect("Writer lifecycle");
    assert!(lifecycle.seal.is_none());
    assert!(lifecycle.integration.is_none());
}

async fn assert_writer_sigkill_recovery(scenario: CrashScenario, expect_completed: bool) {
    let fixture = CrashFixture::new();
    fixture.crash_child(scenario);
    let store_before =
        StateStore::open(Some(fixture.db.clone())).expect("open writer crash SQLite prefix");
    let before = store_before
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load writer crash prefix")
        .expect("writer crash root exists");
    drop(store_before);
    assert_writer_lifecycle_is_single(&before, &fixture);
    assert_eq!(event_count(&before, RuntimeEventKind::is_terminal), 0);

    match scenario {
        CrashScenario::WriterTaskPrepared => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "create"), 0);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentWorkspaceCreated { .. }
                )),
                0
            );
        }
        CrashScenario::WriterCreateSideEffect => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "create"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentWorkspaceCreated { .. }
                )),
                0,
                "create side effect must precede its durable publication"
            );
        }
        CrashScenario::WriterRunning => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentSealPrepared { .. }
                )),
                0
            );
        }
        CrashScenario::WriterSealSideEffect => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "seal"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentSealPrepared { .. }
                )),
                1,
                "seal preparation must be durable before the side effect"
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentSealCommitted { .. }
                )),
                0,
                "seal side effect must precede its durable commit"
            );
        }
        CrashScenario::WriterSealCommitted => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "seal"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentSealCommitted { .. }
                )),
                1
            );
        }
        CrashScenario::WriterIntegrationPrepared => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "integration"), 0);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentIntegrationPrepared { .. }
                )),
                1
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentIntegrationStarted { .. }
                )),
                0
            );
        }
        CrashScenario::WriterIntegrationStarted => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "integration"), 0);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentIntegrationStarted { .. }
                )),
                1
            );
        }
        CrashScenario::WriterIntegrationSideEffect => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "integration"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentIntegrationCommitted { .. }
                )),
                0,
                "integration side effect must precede its durable commit"
            );
        }
        CrashScenario::WriterIntegrationCommitted => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "integration"), 1);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentIntegrationCommitted { .. }
                )),
                1
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::ToolOutcomeCommitted { name, .. }
                        if name == AGENT_TOOL_NAME
                )),
                0,
                "the agent tool outcome must still be unpublished at this crash point"
            );
        }
        CrashScenario::WriterCleanupPrepared => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 0);
            assert_eq!(
                marker_line_count(&fixture.writer_marker, "cleanup-execute"),
                0
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentCleanupPrepared { .. }
                )),
                1
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentCleanupCommitted { .. }
                )),
                0
            );
        }
        CrashScenario::WriterCleanupSideEffect => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 1);
            assert_eq!(
                marker_line_count(&fixture.writer_marker, "cleanup-execute"),
                1
            );
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::AgentCleanupCommitted { .. }
                )),
                0,
                "cleanup side effect must precede its durable commit"
            );
        }
        CrashScenario::WriterDelegatedReceiptCommitted => {
            assert_eq!(marker_line_count(&fixture.writer_marker, "create"), 1);
            assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);
            assert_eq!(marker_line_count(&fixture.writer_marker, "seal"), 1);
            assert_eq!(marker_line_count(&fixture.writer_marker, "integration"), 1);
            assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 0);
            let [receipt] = before.snapshot.evidence_receipts.as_slice() else {
                panic!("delegated receipt must be durable before SIGKILL");
            };
            assert!(matches!(
                &receipt.lineage,
                EvidenceLineage::DelegatedFailedWritePass { .. }
            ));
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::HostVerificationCommitted {
                        receipt: Some(receipt),
                        ..
                    } if matches!(
                        &receipt.lineage,
                        EvidenceLineage::DelegatedFailedWritePass { .. }
                    )
                )),
                1
            );
        }
        _ => panic!("not a writer crash scenario: {scenario:?}"),
    }

    let (runtime, store, model) = fixture.reopen_writer(scenario);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume writer crash lifecycle");
    if expect_completed {
        assert!(
            matches!(outcome.terminal, TerminalState::Completed { .. }),
            "writer lifecycle should complete after exact recovery: {:?}",
            outcome.terminal
        );
    } else {
        assert!(
            !matches!(outcome.terminal, TerminalState::Completed { .. }),
            "ambiguous writer execution must fail closed"
        );
    }
    assert!(
        model.observed_requests().iter().all(
            |request| request.actor.kind != AgentActorKind::Root || request.request_number != 1
        ),
        "recovery must not reissue the root request that prepared the writer task"
    );
    if scenario == CrashScenario::WriterDelegatedReceiptCommitted {
        assert!(
            model.observed_requests().is_empty(),
            "a committed delegated receipt must complete without another model request"
        );
    }

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered writer lifecycle")
        .expect("recovered writer root exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce recovered writer lifecycle"),
        after.snapshot
    );
    assert_writer_lifecycle_is_single(&after, &fixture);
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "create"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    if matches!(
        scenario,
        CrashScenario::WriterCleanupPrepared | CrashScenario::WriterCleanupSideEffect
    ) {
        let prepared = before.snapshot.agent_tasks[0]
            .cleanup
            .as_ref()
            .expect("cleanup crash prefix must persist the frozen plan");
        assert_eq!(prepared.plan.phase, WriterCleanupPhase::PostIntegration);
        assert_eq!(prepared.plan.reason_code, "writer_integrated");
        assert_eq!(
            prepared.plan.artifact_state,
            WriterArtifactState::KnownHostSealed {
                final_commit: WRITER_FINAL_COMMIT.to_owned(),
                diff_sha256: WRITER_DIFF_SHA256.to_owned(),
            }
        );
        assert_eq!(
            prepared.plan.scope,
            WriterCleanupScope::Known {
                workspace_revision: WorkspaceRevision::Known {
                    sha256: WRITER_SEALED_SCOPE_REVISION.to_owned(),
                },
                changed_count: 1,
                in_scope_count: 1,
                out_of_scope_count: 0,
                path_set_sha256: writer_path_set_sha256(&["src/lib.rs".to_owned()]).unwrap(),
            }
        );
        assert_eq!(
            prepared.plan.mode,
            WriterCleanupMode::RemoveExact {
                expected_branch_commit: WRITER_FINAL_COMMIT.to_owned(),
            }
        );
        let frozen_plan_marker = format!(
            "cleanup-plan:{}",
            serde_json::to_string(&prepared.plan).expect("serialize frozen cleanup plan")
        );
        let expected_execute_count = if scenario == CrashScenario::WriterCleanupPrepared {
            1
        } else {
            2
        };
        assert_eq!(
            marker_line_count(&fixture.writer_marker, "cleanup-execute"),
            expected_execute_count,
            "recovery must execute only the persisted cleanup action"
        );
        assert_eq!(
            marker_line_count(&fixture.writer_marker, "cleanup-inspect"),
            1,
            "recovery must not rebuild a committed cleanup plan"
        );
        assert_eq!(
            marker_line_count(&fixture.writer_marker, &frozen_plan_marker),
            expected_execute_count,
            "every cleanup execution must receive the exact persisted plan"
        );
        assert_eq!(
            marker_line_count(&fixture.writer_marker, "cleanup"),
            1,
            "idempotent re-entry must not repeat the physical deletion"
        );
        let committed = after.snapshot.agent_tasks[0]
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.committed.as_ref())
            .expect("cleanup recovery must commit its exact result");
        let expected = if scenario == CrashScenario::WriterCleanupPrepared {
            WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            }
        } else {
            WriterCleanupResult::AlreadyAbsent
        };
        assert_eq!(committed, &expected);
    }
    if expect_completed {
        for side_effect in ["seal", "integration", "cleanup"] {
            assert_eq!(
                marker_line_count(&fixture.writer_marker, side_effect),
                1,
                "completed recovery must settle '{side_effect}' exactly once"
            );
        }
        assert_eq!(
            event_count(&after, |event| matches!(
                event,
                RuntimeEventKind::AgentWorkspaceCreated { .. }
            )),
            1
        );
        assert_eq!(
            event_count(&after, |event| matches!(
                event,
                RuntimeEventKind::AgentSealCommitted { .. }
            )),
            1
        );
        assert_eq!(
            event_count(&after, |event| matches!(
                event,
                RuntimeEventKind::AgentIntegrationCommitted { .. }
            )),
            1
        );
        assert_eq!(
            event_count(&after, |event| matches!(
                event,
                RuntimeEventKind::AgentCleanupCommitted { .. }
            )),
            1
        );
        assert_eq!(
            marker_line_count(&fixture.tool_marker, "root-verifier"),
            1,
            "root exact verifier must execute once for the integrated revision"
        );
        assert_eq!(
            event_count(&after, |event| matches!(
                event,
                RuntimeEventKind::HostVerificationCommitted { .. }
            )),
            1,
            "root receipt must commit exactly once"
        );
        let [receipt] = after.snapshot.evidence_receipts.as_slice() else {
            panic!("completed writer recovery must retain exactly one root receipt");
        };
        assert_eq!(
            receipt.generation_id,
            TaskGenerationId::from(RUN_ID),
            "child evidence must not replace the root task generation"
        );
        assert_eq!(receipt.acceptance_id, AcceptanceId::from("writer-tests"));
        assert_eq!(receipt.verifier, writer_verifier_spec());
        assert_eq!(
            receipt.workspace_state, after.snapshot.workspace_state,
            "root receipt must bind the latest canonical workspace"
        );
        assert_eq!(
            receipt.workspace_state.revision,
            WorkspaceRevision::Known {
                sha256: WRITER_FINAL_COMMIT.to_owned(),
            },
            "root receipt must verify the integrated commit"
        );
        let lifecycle = &after.snapshot.agent_tasks[0];
        let seal = lifecycle
            .seal
            .as_ref()
            .and_then(|seal| seal.committed.as_ref())
            .expect("completed recovery must retain the exact writer seal");
        assert_eq!(seal.final_commit, WRITER_FINAL_COMMIT);
        assert_eq!(seal.diff_sha256, WRITER_DIFF_SHA256);
        assert_eq!(seal.changed_files, vec!["src/lib.rs".to_owned()]);
        let integration = lifecycle
            .integration
            .as_ref()
            .and_then(|integration| integration.committed.as_ref())
            .expect("completed recovery must retain the exact integration");
        assert_eq!(integration.root_head_commit, WRITER_FINAL_COMMIT);
        assert_eq!(
            integration.root_workspace_state_after.revision,
            WorkspaceRevision::Known {
                sha256: WRITER_FINAL_COMMIT.to_owned(),
            }
        );
        let cleanup = lifecycle
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.committed.as_ref())
            .expect("completed recovery must retain cleanup");
        assert!(matches!(
            cleanup,
            WriterCleanupResult::Removed { .. } | WriterCleanupResult::AlreadyAbsent
        ));
        if scenario == CrashScenario::WriterDelegatedReceiptCommitted {
            assert_eq!(
                marker_line_count(&fixture.model_marker, "writer-root-model"),
                2
            );
            assert_eq!(
                marker_line_count(&fixture.model_marker, "writer-child-model"),
                3
            );
            assert_eq!(
                marker_line_count(&fixture.tool_marker, "writer-verifier"),
                2,
                "child failed verifier and final Host verifier must each execute once"
            );
            assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
            let child = store
                .load(&lifecycle.task.child_run_id)
                .await
                .expect("load temporal Writer child")
                .expect("temporal Writer child exists");
            let [child_receipt] = child.snapshot.evidence_receipts.as_slice() else {
                panic!("temporal Writer child must retain one local receipt");
            };
            assert!(matches!(
                &child_receipt.lineage,
                EvidenceLineage::FailedWritePass { .. }
            ));
            let EvidenceLineage::DelegatedFailedWritePass {
                child_run_id,
                child_receipt_id,
                integration: delegated_integration,
            } = &receipt.lineage
            else {
                panic!("root receipt must preserve delegated temporal lineage");
            };
            assert_eq!(child_run_id, &lifecycle.task.child_run_id.0);
            assert_eq!(child_receipt_id, &child_receipt.id);
            assert_eq!(
                delegated_integration.operation_id,
                lifecycle
                    .integration
                    .as_ref()
                    .expect("integration lifecycle")
                    .integration_id
                    .0
            );
            assert_eq!(
                delegated_integration.workspace_state_after,
                integration.root_workspace_state_after
            );
        }
    } else {
        assert_eq!(
            marker_line_count(&fixture.writer_marker, "integration"),
            0,
            "ambiguous writer bytes must never be integrated"
        );
        assert!(
            after.events.iter().all(|event| !matches!(
                &event.event,
                RuntimeEventKind::Terminal { outcome }
                    if matches!(outcome.terminal, TerminalState::Completed { .. })
            )),
            "ambiguous writer execution must never commit Completed"
        );
    }
}

#[tokio::test]
async fn writer_task_prepared_sigkill_creates_one_owned_workspace_on_resume() {
    assert_writer_sigkill_recovery(CrashScenario::WriterTaskPrepared, true).await;
}

#[tokio::test]
async fn writer_create_side_effect_sigkill_recovers_without_second_worktree_or_branch() {
    assert_writer_sigkill_recovery(CrashScenario::WriterCreateSideEffect, true).await;
}

#[tokio::test]
async fn writer_running_sigkill_fails_closed_without_integration() {
    assert_writer_sigkill_recovery(CrashScenario::WriterRunning, false).await;
}

#[tokio::test]
async fn writer_seal_side_effect_sigkill_proves_commit_without_resealing() {
    assert_writer_sigkill_recovery(CrashScenario::WriterSealSideEffect, true).await;
}

#[tokio::test]
async fn writer_seal_committed_sigkill_reuses_one_diff_and_seal() {
    assert_writer_sigkill_recovery(CrashScenario::WriterSealCommitted, true).await;
}

#[tokio::test]
async fn writer_integration_prepared_sigkill_starts_one_guarded_integration() {
    assert_writer_sigkill_recovery(CrashScenario::WriterIntegrationPrepared, true).await;
}

#[tokio::test]
async fn writer_integration_started_sigkill_recovers_one_guarded_integration() {
    assert_writer_sigkill_recovery(CrashScenario::WriterIntegrationStarted, true).await;
}

#[tokio::test]
async fn writer_integration_side_effect_sigkill_proves_commit_without_reapplying() {
    assert_writer_sigkill_recovery(CrashScenario::WriterIntegrationSideEffect, true).await;
}

#[tokio::test]
async fn writer_integration_committed_sigkill_replays_one_agent_tool_outcome() {
    assert_writer_sigkill_recovery(CrashScenario::WriterIntegrationCommitted, true).await;
}

#[tokio::test]
async fn writer_cleanup_prepared_sigkill_removes_owned_resources_once() {
    assert_writer_sigkill_recovery(CrashScenario::WriterCleanupPrepared, true).await;
}

#[tokio::test]
async fn writer_cleanup_side_effect_sigkill_proves_absence_without_second_removal() {
    assert_writer_sigkill_recovery(CrashScenario::WriterCleanupSideEffect, true).await;
}

async fn assert_failed_writer_cleanup_sigkill_window(
    scenario: CrashScenario,
    expected_mid_execute: usize,
    expected_final_execute: usize,
) {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::WriterRunning);
    let model_after_writer_crash = marker_lines(&fixture.model_marker);
    assert_eq!(
        marker_line_count(&fixture.model_marker, "writer-root-model"),
        1
    );
    assert_eq!(
        marker_line_count(&fixture.model_marker, "writer-child-model"),
        1
    );
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);

    fixture.crash_child(scenario);
    assert_eq!(
        marker_lines(&fixture.model_marker),
        model_after_writer_crash
    );
    let store_mid = StateStore::open(Some(fixture.db.clone())).expect("open failed-cleanup prefix");
    let mid = store_mid
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load failed-cleanup root")
        .expect("failed-cleanup root exists");
    let lifecycle = mid.snapshot.agent_tasks.first().expect("Writer lifecycle");
    let child_mid = store_mid
        .load(&lifecycle.task.child_run_id)
        .await
        .expect("load failed Writer child")
        .expect("failed Writer child exists");
    drop(store_mid);

    assert_writer_lifecycle_is_single(&mid, &fixture);
    assert_failed_writer_never_sealed_or_integrated(&mid);
    assert_eq!(
        reduce_events(&mid.events).expect("reduce failed-cleanup prefix"),
        mid.snapshot
    );
    assert_eq!(
        reduce_events(&child_mid.events).expect("reduce failed Writer child"),
        child_mid.snapshot
    );
    assert_eq!(mid.snapshot.request.run_id, Some(RunId::from(RUN_ID)));
    assert_eq!(mid.snapshot.request.parent_run_id, None);
    assert_eq!(mid.snapshot.request.actor.kind, AgentActorKind::Root);
    assert_eq!(mid.snapshot.request.actor.depth, 0);
    assert_eq!(lifecycle.task.root_run_id, RunId::from(RUN_ID));
    assert_eq!(lifecycle.task.parent_run_id, RunId::from(RUN_ID));
    assert_eq!(
        child_mid.snapshot.request.run_id.as_ref(),
        Some(&lifecycle.task.child_run_id)
    );
    assert_eq!(
        child_mid.snapshot.request.parent_run_id.as_ref(),
        Some(&RunId::from(RUN_ID))
    );
    assert_eq!(child_mid.snapshot.request.actor.kind, AgentActorKind::Child);
    assert_eq!(child_mid.snapshot.request.actor.depth, 1);
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);

    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::AgentResultCollected { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupCommitted { .. }
        )),
        0
    );
    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::ChildFinished { .. }
        )),
        0
    );
    assert_eq!(event_count(&mid, RuntimeEventKind::is_terminal), 0);
    for event in ["seal", "integration"] {
        assert_eq!(marker_line_count(&fixture.writer_marker, event), 0);
    }
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-execute"),
        expected_mid_execute
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-inspect"),
        1,
        "the cleanup scope must be inspected exactly once before the durable plan"
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup"),
        usize::from(expected_mid_execute > 0)
    );
    let cleanup = lifecycle.cleanup.as_ref().expect("cleanup plan");
    assert_eq!(cleanup.plan.phase, WriterCleanupPhase::Child);
    assert_eq!(cleanup.plan.reason_code, "writer_child_recovery_required");
    assert_eq!(
        cleanup.plan.artifact_state,
        WriterArtifactState::KnownUnsealed
    );
    assert_eq!(
        cleanup.plan.scope,
        WriterCleanupScope::Known {
            workspace_revision: WorkspaceRevision::Known {
                sha256: WRITER_DIRTY_REVISION.to_owned(),
            },
            changed_count: 1,
            in_scope_count: 1,
            out_of_scope_count: 0,
            path_set_sha256: writer_path_set_sha256(&["src/lib.rs".to_owned()]).unwrap(),
        }
    );
    assert_eq!(
        cleanup.plan.mode,
        WriterCleanupMode::RemoveExact {
            expected_branch_commit: WRITER_BASE_COMMIT.to_owned(),
        }
    );
    let frozen_plan_marker = format!(
        "cleanup-plan:{}",
        serde_json::to_string(&cleanup.plan).expect("serialize frozen cleanup plan")
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, &frozen_plan_marker),
        expected_mid_execute,
        "every cleanup execution must receive the exact persisted plan"
    );

    let (runtime, store, model) = fixture.reopen_writer(scenario);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("finish failed-cleanup recovery");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired { .. }
    ));
    assert!(model.observed_requests().is_empty());
    assert_eq!(
        marker_lines(&fixture.model_marker),
        model_after_writer_crash
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-execute"),
        expected_final_execute
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-inspect"),
        1,
        "recovery must not rebuild a committed cleanup plan"
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, &frozen_plan_marker),
        expected_final_execute,
        "recovery must reuse the exact persisted cleanup plan"
    );
    assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 1);
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load final failed-cleanup root")
        .expect("final failed-cleanup root exists");
    let child_after = store
        .load(&lifecycle.task.child_run_id)
        .await
        .expect("load final failed Writer child")
        .expect("final failed Writer child exists");
    assert_eq!(child_after.events, child_mid.events);
    assert_eq!(child_after.snapshot, child_mid.snapshot);
    assert_replay_prefix_preserved(&mid, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce failed-cleanup recovery"),
        after.snapshot
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupCommitted { .. }
        )),
        1
    );
    let committed_cleanup = after
        .snapshot
        .agent_tasks
        .first()
        .and_then(|lifecycle| lifecycle.cleanup.as_ref())
        .and_then(|cleanup| cleanup.committed.as_ref())
        .expect("committed failed Writer cleanup result");
    if expected_mid_execute == 0 {
        assert_eq!(
            committed_cleanup,
            &WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            }
        );
    } else {
        assert_eq!(committed_cleanup, &WriterCleanupResult::AlreadyAbsent);
    }
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::ChildFinished { .. }
        )),
        1
    );
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
    assert!(after.events.iter().all(|event| !matches!(
        &event.event,
        RuntimeEventKind::Terminal { outcome }
            if matches!(outcome.terminal, TerminalState::Completed { .. })
    )));
    let final_lifecycle = after
        .snapshot
        .agent_tasks
        .first()
        .expect("final failed Writer lifecycle");
    assert_writer_lifecycle_is_single(&after, &fixture);
    assert_failed_writer_never_sealed_or_integrated(&after);
    let collected = final_lifecycle
        .result
        .as_ref()
        .expect("collected failed Writer result");
    let finished = final_lifecycle
        .finished
        .as_ref()
        .expect("finished failed Writer result");
    assert_eq!(collected.run_id, final_lifecycle.task.child_run_id);
    assert_eq!(collected.parent_run_id.as_ref(), Some(&RunId::from(RUN_ID)));
    assert_eq!(finished.outcome.run_id, final_lifecycle.task.child_run_id);
    assert_eq!(finished.outcome.parent_run_id, collected.parent_run_id);
    assert_eq!(finished.outcome.terminal, collected.terminal);
    assert_eq!(child_mid.snapshot.terminal.as_ref(), Some(collected));
    assert!(collected.details.evidence.is_empty());
    assert!(finished.outcome.details.evidence.is_empty());
    let root_terminal = after.snapshot.terminal.as_ref().expect("root terminal");
    assert_eq!(root_terminal.run_id, RunId::from(RUN_ID));
    assert_eq!(root_terminal.parent_run_id, None);
    assert!(root_terminal.details.evidence.is_empty());
    assert!(after.snapshot.evidence_receipts.is_empty());
    assert_eq!(
        root_terminal.terminal, finished.outcome.terminal,
        "root recovery must preserve the exact child phase and action identity"
    );
}

#[tokio::test]
async fn failed_writer_cleanup_plan_sigkill_reuses_the_frozen_plan() {
    assert_failed_writer_cleanup_sigkill_window(CrashScenario::WriterFailedCleanupPrepared, 0, 1)
        .await;
}

#[tokio::test]
async fn failed_writer_cleanup_side_effect_sigkill_does_not_repeat_deletion() {
    assert_failed_writer_cleanup_sigkill_window(CrashScenario::WriterFailedCleanupSideEffect, 1, 2)
        .await;
}

#[tokio::test]
async fn writer_cleanup_result_sigkill_finishes_without_reexecuting_child_or_cleanup() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::WriterRunning);
    let model_before_cleanup = marker_lines(&fixture.model_marker);
    assert_eq!(
        marker_line_count(&fixture.model_marker, "writer-root-model"),
        1
    );
    assert_eq!(
        marker_line_count(&fixture.model_marker, "writer-child-model"),
        1
    );
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);

    fixture.crash_child(CrashScenario::WriterCleanupResultCommitted);
    let store_mid = StateStore::open(Some(fixture.db.clone())).expect("open cleanup-result prefix");
    let mid = store_mid
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load cleanup-result prefix")
        .expect("cleanup-result root exists");
    let lifecycle = mid.snapshot.agent_tasks.first().expect("Writer lifecycle");
    let child_mid = store_mid
        .load(&lifecycle.task.child_run_id)
        .await
        .expect("load Writer child prefix")
        .expect("Writer child exists");
    drop(store_mid);

    assert_writer_lifecycle_is_single(&mid, &fixture);
    assert_failed_writer_never_sealed_or_integrated(&mid);
    assert_eq!(
        reduce_events(&mid.events).expect("reduce cleanup-result prefix"),
        mid.snapshot
    );
    assert_eq!(
        reduce_events(&child_mid.events).expect("reduce cleanup-result child"),
        child_mid.snapshot
    );
    assert_eq!(mid.snapshot.request.run_id, Some(RunId::from(RUN_ID)));
    assert_eq!(mid.snapshot.request.parent_run_id, None);
    assert_eq!(mid.snapshot.request.actor.kind, AgentActorKind::Root);
    assert_eq!(mid.snapshot.request.actor.depth, 0);
    assert_eq!(lifecycle.task.root_run_id, RunId::from(RUN_ID));
    assert_eq!(lifecycle.task.parent_run_id, RunId::from(RUN_ID));
    assert_eq!(
        child_mid.snapshot.request.run_id.as_ref(),
        Some(&lifecycle.task.child_run_id)
    );
    assert_eq!(
        child_mid.snapshot.request.parent_run_id.as_ref(),
        Some(&RunId::from(RUN_ID))
    );
    assert_eq!(child_mid.snapshot.request.actor.kind, AgentActorKind::Child);
    assert_eq!(child_mid.snapshot.request.actor.depth, 1);

    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupCommitted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&mid, |event| matches!(
            event,
            RuntimeEventKind::ChildFinished { .. }
        )),
        0,
        "SIGKILL must occur after cleanup result and before ChildFinished"
    );
    assert_eq!(event_count(&mid, RuntimeEventKind::is_terminal), 0);
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-execute"),
        1
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-inspect"),
        1
    );
    assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 1);
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);
    for event_name in ["seal", "integration"] {
        assert_eq!(marker_line_count(&fixture.writer_marker, event_name), 0);
    }
    let cleanup = lifecycle.cleanup.as_ref().expect("prepared cleanup");
    assert_eq!(cleanup.plan.phase, WriterCleanupPhase::Child);
    assert_eq!(cleanup.plan.reason_code, "writer_child_recovery_required");
    assert_eq!(
        cleanup.plan.artifact_state,
        WriterArtifactState::KnownUnsealed
    );
    assert_eq!(
        cleanup.committed.as_ref(),
        Some(&WriterCleanupResult::Removed {
            worktree: WriterRemovalState::Removed,
            branch: WriterRemovalState::Removed,
        })
    );
    let frozen_plan_marker = format!(
        "cleanup-plan:{}",
        serde_json::to_string(&cleanup.plan).expect("serialize frozen cleanup plan")
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, &frozen_plan_marker),
        1
    );

    let (runtime, store, model) =
        fixture.reopen_writer(CrashScenario::WriterCleanupResultCommitted);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("finish cleanup-result recovery");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired { .. }
    ));
    assert!(
        model.observed_requests().is_empty(),
        "committed cleanup result must not request the model again"
    );
    assert_eq!(marker_lines(&fixture.model_marker), model_before_cleanup);
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-execute"),
        1
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, "cleanup-inspect"),
        1,
        "committed cleanup result must skip inspection"
    );
    assert_eq!(
        marker_line_count(&fixture.writer_marker, &frozen_plan_marker),
        1,
        "committed cleanup result must skip execution"
    );
    assert_eq!(marker_line_count(&fixture.writer_marker, "cleanup"), 1);
    assert_eq!(marker_line_count(&fixture.tool_marker, "writer-write"), 1);
    assert_eq!(marker_line_count(&fixture.writer_marker, "write"), 1);

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load final cleanup recovery")
        .expect("final root exists");
    let child_after = store
        .load(&lifecycle.task.child_run_id)
        .await
        .expect("load final Writer child")
        .expect("final Writer child exists");
    assert_eq!(child_after.events, child_mid.events);
    assert_eq!(child_after.snapshot, child_mid.snapshot);
    assert_replay_prefix_preserved(&mid, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce final cleanup recovery"),
        after.snapshot
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::AgentCleanupCommitted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::ChildFinished { .. }
        )),
        1
    );
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
    assert!(after.events.iter().all(|event| !matches!(
        &event.event,
        RuntimeEventKind::Terminal { outcome }
            if matches!(outcome.terminal, TerminalState::Completed { .. })
    )));
    let final_lifecycle = after
        .snapshot
        .agent_tasks
        .first()
        .expect("final cleanup-result Writer lifecycle");
    assert_writer_lifecycle_is_single(&after, &fixture);
    assert_failed_writer_never_sealed_or_integrated(&after);
    assert_eq!(
        final_lifecycle
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.committed.as_ref()),
        Some(&WriterCleanupResult::Removed {
            worktree: WriterRemovalState::Removed,
            branch: WriterRemovalState::Removed,
        }),
        "recovery must preserve the originally committed cleanup result"
    );
    let collected = final_lifecycle
        .result
        .as_ref()
        .expect("collected cleanup-result Writer outcome");
    let finished = final_lifecycle
        .finished
        .as_ref()
        .expect("finished cleanup-result Writer outcome");
    assert_eq!(collected.run_id, final_lifecycle.task.child_run_id);
    assert_eq!(collected.parent_run_id.as_ref(), Some(&RunId::from(RUN_ID)));
    assert_eq!(finished.outcome.run_id, final_lifecycle.task.child_run_id);
    assert_eq!(finished.outcome.parent_run_id, collected.parent_run_id);
    assert_eq!(finished.outcome.terminal, collected.terminal);
    assert_eq!(child_mid.snapshot.terminal.as_ref(), Some(collected));
    assert!(collected.details.evidence.is_empty());
    assert!(finished.outcome.details.evidence.is_empty());
    let root_terminal = after.snapshot.terminal.as_ref().expect("root terminal");
    assert_eq!(root_terminal.run_id, RunId::from(RUN_ID));
    assert_eq!(root_terminal.parent_run_id, None);
    assert_eq!(root_terminal.terminal, finished.outcome.terminal);
    assert!(root_terminal.details.evidence.is_empty());
    assert!(after.snapshot.evidence_receipts.is_empty());
}

#[tokio::test]
async fn writer_delegated_temporal_receipt_survives_sigkill_without_reexecution() {
    assert_writer_sigkill_recovery(CrashScenario::WriterDelegatedReceiptCommitted, true).await;
}

#[tokio::test]
async fn host_verification_prepared_sigkill_reuses_verification_id_and_executes_once() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::HostVerificationPrepared);
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);
    assert!(marker_lines(&fixture.tool_marker).is_empty());

    let (runtime, store, model) =
        fixture.reopen_with_model(CrashScenario::HostVerificationPrepared);
    let before = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load prepared Host verification")
        .expect("prepared Host verification exists");
    let pending = before
        .snapshot
        .pending_host_verification
        .clone()
        .expect("prepared Host verification remains durable");
    assert_eq!(pending.state, DurableActionState::Prepared);
    assert_eq!(
        pending.acceptance_id,
        AcceptanceId::from(HOST_VERIFIER_ACCEPTANCE_ID)
    );
    assert_eq!(pending.verifier, host_verifier_spec());
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationStarted { .. }
        )),
        0
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted { .. }
        )),
        0
    );
    assert!(before.snapshot.evidence_receipts.is_empty());
    assert_eq!(event_count(&before, RuntimeEventKind::is_terminal), 0);

    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume prepared Host verification");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert!(
        model.observed_requests().is_empty(),
        "prepared Host verification recovery must not issue another model request"
    );
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);
    assert_eq!(
        marker_lines(&fixture.tool_marker),
        vec![format!("host:{}", pending.verification_id.0)]
    );

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered Host verification")
        .expect("recovered Host verification exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce recovered Host verification"),
        after.snapshot
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationPrepared {
                verification_id,
                ..
            } if verification_id == &pending.verification_id
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationStarted { verification_id }
                if verification_id == &pending.verification_id
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted {
                verification_id,
                receipt: Some(receipt),
                ..
            } if verification_id == &pending.verification_id
                && receipt.verification_id == pending.verification_id
        )),
        1
    );
    assert!(after.snapshot.pending_host_verification.is_none());
    assert_eq!(after.snapshot.evidence_receipts.len(), 1);
    assert_eq!(
        after.snapshot.evidence_receipts[0].verification_id,
        pending.verification_id
    );
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn host_verification_in_flight_sigkill_requires_recovery_without_rerun() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::HostVerificationInFlight);
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);

    let store_before =
        StateStore::open(Some(fixture.db.clone())).expect("reopen in-flight Host verifier store");
    let before = store_before
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load in-flight Host verification")
        .expect("in-flight Host verification exists");
    let pending = before
        .snapshot
        .pending_host_verification
        .clone()
        .expect("in-flight Host verification remains durable");
    drop(store_before);
    assert_eq!(pending.state, DurableActionState::InFlight);
    assert_eq!(
        marker_lines(&fixture.tool_marker),
        vec![format!("host:{}", pending.verification_id.0)]
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationStarted {
                verification_id
            } if verification_id == &pending.verification_id
        )),
        1
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted { .. }
        )),
        0
    );
    assert!(before.snapshot.evidence_receipts.is_empty());

    let (runtime, store, model) =
        fixture.reopen_with_model(CrashScenario::HostVerificationInFlight);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume in-flight Host verification");
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: codewhale_runtime::RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::HostVerification,
                ref action_id,
                ..
            }
        } if action_id == &pending.verification_id.0
    ));
    assert!(
        model.observed_requests().is_empty(),
        "ambiguous Host verification recovery must not issue another model request"
    );
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);
    assert_eq!(
        marker_lines(&fixture.tool_marker),
        vec![format!("host:{}", pending.verification_id.0)],
        "an in-flight Host verifier must never be rerun"
    );

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load failed-closed Host verification")
        .expect("failed-closed Host verification exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce failed-closed Host verification"),
        after.snapshot
    );
    assert_eq!(
        after.snapshot.pending_host_verification.as_ref(),
        Some(&pending)
    );
    assert!(after.snapshot.evidence_receipts.is_empty());
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted { .. }
        )),
        0
    );
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn host_verification_committed_sigkill_replays_receipt_and_completes_exactly_once() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::HostVerificationCommitted);
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);

    let (runtime, store, model) =
        fixture.reopen_with_model(CrashScenario::HostVerificationCommitted);
    let before = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load committed Host verification")
        .expect("committed Host verification exists");
    assert!(before.snapshot.pending_host_verification.is_none());
    let receipt = before
        .snapshot
        .evidence_receipts
        .first()
        .cloned()
        .expect("committed Host verification receipt");
    assert_eq!(
        receipt.acceptance_id,
        AcceptanceId::from(HOST_VERIFIER_ACCEPTANCE_ID)
    );
    assert_eq!(receipt.verifier, host_verifier_spec());
    assert_eq!(
        marker_lines(&fixture.tool_marker),
        vec![format!("host:{}", receipt.verification_id.0)]
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationPrepared { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationStarted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted {
                verification_id,
                receipt: Some(committed),
                ..
            } if verification_id == &receipt.verification_id && **committed == receipt
        )),
        1
    );
    assert_eq!(event_count(&before, RuntimeEventKind::is_terminal), 0);

    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume committed Host verification");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert!(
        model.observed_requests().is_empty(),
        "committed Host verification recovery must not issue another model request"
    );
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);
    assert_eq!(
        marker_lines(&fixture.tool_marker),
        vec![format!("host:{}", receipt.verification_id.0)],
        "a committed verifier receipt must be replayed without rerunning the verifier"
    );

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load completed Host verification")
        .expect("completed Host verification exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce completed Host verification"),
        after.snapshot
    );
    assert_eq!(after.snapshot.evidence_receipts, vec![receipt]);
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&after, RuntimeEventKind::is_terminal),
        1,
        "receipt replay may produce exactly one canonical terminal"
    );
}

async fn assert_temporal_sigkill_recovery(scenario: CrashScenario) {
    let fixture = CrashFixture::new();
    fixture.crash_child(scenario);

    let store_before =
        StateStore::open(Some(fixture.db.clone())).expect("open temporal crash SQLite prefix");
    let before = store_before
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load temporal crash prefix")
        .expect("temporal crash run exists");
    drop(store_before);
    assert_eq!(event_count(&before, RuntimeEventKind::is_terminal), 0);

    let progress = before.snapshot.temporal_evidence_progress.as_ref();
    match scenario {
        CrashScenario::TemporalFailureCommitted => {
            let progress = progress.expect("failed verifier fact must survive SIGKILL");
            assert_eq!(
                progress.failure.workspace_state.revision,
                WorkspaceRevision::Known {
                    sha256: TEMPORAL_BROKEN_REVISION.to_owned(),
                }
            );
            assert!(progress.mutation.is_none());
            assert!(before.snapshot.evidence_receipts.is_empty());
            let pending = before
                .snapshot
                .pending_completion
                .as_ref()
                .expect("failed Host verifier must retain its pending candidate");
            let failure = before
                .snapshot
                .last_host_verification_failure
                .as_ref()
                .expect("failed Host verifier projection");
            assert_eq!(failure.rejection.candidate_id, pending.id);
            assert_eq!(
                event_count(&before, |event| matches!(
                    event,
                    RuntimeEventKind::CompletionRejected { .. }
                )),
                0,
                "SIGKILL must land before the deterministic rejection commit"
            );
            assert_eq!(marker_count(&fixture.model_marker), 1);
        }
        CrashScenario::TemporalMutationCommitted => {
            let progress = progress.expect("failure and mutation must survive SIGKILL");
            let mutation = progress
                .mutation
                .as_ref()
                .expect("effective mutation must be durable");
            assert_eq!(
                mutation.workspace_state_before.revision,
                WorkspaceRevision::Known {
                    sha256: TEMPORAL_BROKEN_REVISION.to_owned(),
                }
            );
            assert_eq!(
                mutation.workspace_state_after.revision,
                WorkspaceRevision::Known {
                    sha256: TEMPORAL_FIXED_REVISION.to_owned(),
                }
            );
            assert!(before.snapshot.evidence_receipts.is_empty());
            assert_eq!(marker_count(&fixture.model_marker), 2);
        }
        CrashScenario::TemporalHostVerificationCommitted => {
            assert!(progress.is_none());
            let [receipt] = before.snapshot.evidence_receipts.as_slice() else {
                panic!("temporal Host commit must persist exactly one receipt");
            };
            assert!(matches!(
                &receipt.lineage,
                EvidenceLineage::FailedWritePass { failure, mutation }
                    if failure.workspace_state.revision
                        == WorkspaceRevision::Known {
                            sha256: TEMPORAL_BROKEN_REVISION.to_owned(),
                        }
                        && mutation.workspace_state_after.revision
                            == WorkspaceRevision::Known {
                                sha256: TEMPORAL_FIXED_REVISION.to_owned(),
                            }
            ));
            assert_eq!(marker_count(&fixture.model_marker), 3);
        }
        _ => panic!("not a temporal crash scenario: {scenario:?}"),
    }

    let (runtime, store, model) = fixture.reopen_with_model(scenario);
    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume temporal crash lifecycle");
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "temporal recovery must complete from durable facts: {:?}",
        outcome.terminal
    );

    let expected_recovery_requests = match scenario {
        CrashScenario::TemporalFailureCommitted => 2,
        CrashScenario::TemporalMutationCommitted => 1,
        CrashScenario::TemporalHostVerificationCommitted => 0,
        _ => unreachable!(),
    };
    assert_eq!(
        model.observed_requests().len(),
        expected_recovery_requests,
        "resume must continue after the committed boundary without reissuing settled work"
    );
    assert_eq!(marker_count(&fixture.model_marker), 3);
    assert_eq!(
        marker_lines(&fixture.model_marker),
        ["temporal-model:1", "temporal-model:2", "temporal-model:3"],
        "each logical model request must execute exactly once"
    );
    assert_eq!(
        marker_line_count(&fixture.tool_marker, "temporal-write"),
        1,
        "the effective repair must execute exactly once"
    );
    let host_verifications = marker_lines(&fixture.tool_marker)
        .into_iter()
        .filter(|line| line.starts_with("host:"))
        .collect::<Vec<_>>();
    assert_eq!(
        host_verifications.len(),
        2,
        "the failed and final Host verifier executions must each occur exactly once"
    );
    assert_ne!(
        host_verifications[0], host_verifications[1],
        "the failed and final verifier must retain distinct durable identities"
    );

    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load recovered temporal lifecycle")
        .expect("recovered temporal lifecycle exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce recovered temporal lifecycle"),
        after.snapshot
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::ToolOutcomeCommitted { name, .. }
                if name == TEMPORAL_WRITE_TOOL
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::HostVerificationCommitted { .. }
        )),
        2
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::CompletionRejected { .. }
        )),
        1,
        "the original failed candidate must be rejected exactly once across recovery"
    );
    let [receipt] = after.snapshot.evidence_receipts.as_slice() else {
        panic!("recovered temporal lifecycle must retain exactly one receipt");
    };
    assert!(matches!(
        &receipt.lineage,
        EvidenceLineage::FailedWritePass { failure, mutation }
            if failure.workspace_state.revision
                == WorkspaceRevision::Known {
                    sha256: TEMPORAL_BROKEN_REVISION.to_owned(),
                }
                && mutation.workspace_state_after.revision
                    == WorkspaceRevision::Known {
                        sha256: TEMPORAL_FIXED_REVISION.to_owned(),
                    }
    ));
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
}

#[tokio::test]
async fn temporal_failure_commit_survives_sigkill_without_rerunning_the_failed_verifier() {
    assert_temporal_sigkill_recovery(CrashScenario::TemporalFailureCommitted).await;
}

#[tokio::test]
async fn temporal_mutation_commit_survives_sigkill_without_reapplying_the_write() {
    assert_temporal_sigkill_recovery(CrashScenario::TemporalMutationCommitted).await;
}

#[tokio::test]
async fn temporal_receipt_commit_survives_sigkill_and_completes_exactly_once() {
    assert_temporal_sigkill_recovery(CrashScenario::TemporalHostVerificationCommitted).await;
}

#[tokio::test]
async fn local_context_compaction_commit_survives_sigkill_without_model_or_transcript_side_effects()
{
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::CompactionCommitted);
    assert!(
        marker_lines(&fixture.model_marker).is_empty(),
        "local deterministic compaction must not call ModelPort"
    );

    let (runtime, store, model) = fixture.reopen_with_model(CrashScenario::CompactionCommitted);
    let before = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load committed compaction")
        .expect("committed compaction exists");
    let source_request = scenario_request(CrashScenario::CompactionCommitted);
    let mut source_transcript = source_request.transcript;
    source_transcript
        .entries
        .push(codewhale_runtime::TranscriptEntry::User {
            content: source_request
                .task_contract
                .expect("Agent task contract")
                .definition
                .model_message(),
        });
    let projection = before
        .snapshot
        .context_projection
        .clone()
        .expect("committed projection");
    let (persisted_projection, persisted_accounting, before_tokens, after_tokens) = before
        .events
        .iter()
        .find_map(|event| match &event.event {
            RuntimeEventKind::ContextCompactionCommitted {
                projection,
                accounting,
                before_tokens,
                after_tokens,
                ..
            } => Some((
                (**projection).clone(),
                (**accounting).clone(),
                *before_tokens,
                *after_tokens,
            )),
            _ => None,
        })
        .expect("durable local compaction event");
    let commit = before
        .snapshot
        .last_context_compaction
        .clone()
        .expect("durable compaction marker");
    assert_eq!(
        projection, persisted_projection,
        "SQLite reopen must rebuild the exact committed projection"
    );
    assert_eq!(
        projection.source_projection_sha256, commit.source_projection_sha256,
        "SQLite reopen must preserve the exact effective-context digest"
    );
    assert_eq!(before_tokens, commit.before_tokens);
    assert_eq!(after_tokens, commit.after_tokens);
    assert!(after_tokens < before_tokens);
    assert_eq!(
        projection.source_entry_count,
        u64::try_from(source_transcript.entries.len()).expect("source transcript length")
    );
    assert!(
        projection.messages.len() < source_transcript.project_messages().len(),
        "projection must reduce model-visible history"
    );
    assert_eq!(
        before.snapshot.transcript, source_transcript,
        "compaction must never rewrite the canonical transcript"
    );
    assert_eq!(persisted_accounting, ModelAccounting::default());
    assert_eq!(before.snapshot.usage, Usage::default());
    assert_eq!(before.snapshot.accounting, ModelAccounting::default());
    assert_eq!(before.snapshot.runtime_model_requests, 0);
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::ModelRequestPrepared { .. }
        )),
        0
    );
    assert_eq!(
        event_count(&before, |event| matches!(
            event,
            RuntimeEventKind::ContextCompactionCommitted { .. }
        )),
        1
    );
    assert_eq!(event_count(&before, RuntimeEventKind::is_terminal), 0);

    let outcome = runtime
        .resume(RunId::from(RUN_ID))
        .wait()
        .await
        .expect("resume committed compaction");
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(marker_lines(&fixture.model_marker), vec!["request"]);
    let observed = model.observed_requests();
    assert_eq!(
        observed.len(),
        1,
        "resume may issue only the ordinary Agent request"
    );
    assert_projection_request(
        &observed[0],
        &projection,
        &before.snapshot.request.system_prompt,
    );
    let after = store
        .load(&RunId::from(RUN_ID))
        .await
        .expect("load resumed committed compaction")
        .expect("resumed committed compaction exists");
    assert_replay_prefix_preserved(&before, &after);
    assert_eq!(
        reduce_events(&after.events).expect("reduce resumed committed compaction"),
        after.snapshot
    );
    assert_eq!(after.snapshot.context_projection, Some(projection));
    assert_eq!(after.snapshot.last_context_compaction, Some(commit));
    assert_eq!(
        after
            .snapshot
            .context_projection
            .as_ref()
            .expect("replayed projection")
            .source_projection_sha256,
        persisted_projection.source_projection_sha256
    );
    let ordinary_request = after
        .events
        .iter()
        .find_map(|event| match &event.event {
            RuntimeEventKind::ModelRequestPrepared { request, .. } => Some(request.as_ref()),
            _ => None,
        })
        .expect("ordinary Agent request");
    assert_eq!(&observed[0], ordinary_request);
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::ContextCompactionCommitted { .. }
        )),
        1
    );
    assert_eq!(
        event_count(&after, |event| matches!(
            event,
            RuntimeEventKind::ModelRequestPrepared { .. }
        )),
        1
    );
    assert_eq!(event_count(&after, RuntimeEventKind::is_terminal), 1);
    assert_eq!(after.snapshot.usage, one_usage());
    assert_eq!(after.snapshot.accounting.usage, one_usage());
    assert_eq!(after.snapshot.runtime_model_requests, 1);
    assert_eq!(outcome.runtime_model_requests, 1);
    assert_eq!(outcome.accounting.usage, one_usage());
    let committed_position = after
        .events
        .iter()
        .position(|event| {
            matches!(
                event.event,
                RuntimeEventKind::ContextCompactionCommitted { .. }
            )
        })
        .expect("compaction commit position");
    let ordinary_position = after
        .events
        .iter()
        .position(|event| matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. }))
        .expect("ordinary request position");
    assert!(
        committed_position < ordinary_position,
        "durable projection commit must precede the ordinary Agent request"
    );
    assert!(
        after
            .snapshot
            .transcript
            .entries
            .starts_with(&source_transcript.entries)
    );
    assert_eq!(
        after.snapshot.transcript.entries.len(),
        source_transcript.entries.len() + 1,
        "only the resumed Agent response may extend canonical history"
    );
    assert_eq!(
        after
            .snapshot
            .transcript
            .entries
            .iter()
            .filter(|entry| matches!(entry, codewhale_runtime::TranscriptEntry::Assistant { .. }))
            .count(),
        7,
        "six source assistants plus one resumed Agent response"
    );
}

#[tokio::test]
async fn creation_reservation_sigkill_before_run_created_reuses_identity_and_creates_once() {
    let fixture = CrashFixture::new();
    fixture.crash_child(CrashScenario::CreationReserved);

    let store =
        StateStore::open(Some(fixture.db.clone())).expect("reopen reservation crash database");
    let command_id = CommandId::from(CREATE_COMMAND_ID);
    let retried = store
        .reserve_creation(
            &command_id,
            CREATE_COMMAND_SHA256,
            RunId::from("ignored-retry-proposal"),
            creation_intent(),
        )
        .await
        .expect("retry identical creation reservation after reopen");
    assert!(!retried.newly_reserved);
    assert_eq!(retried.reservation.command_id, command_id);
    assert_eq!(retried.reservation.command_sha256, CREATE_COMMAND_SHA256);
    assert_eq!(retried.reservation.run_id, RunId::from(RUN_ID));
    assert!(retried.reservation.intent.is_some());
    assert_eq!(
        store
            .list_pending_creations("/tmp/codewhale-process-crash-test", 10)
            .await
            .expect("list pending creation after SIGKILL"),
        vec![retried.reservation.clone()]
    );

    assert!(matches!(
        store
            .reserve_creation(
                &command_id,
                "sha256:different-create-payload",
                RunId::from("different-proposal"),
                creation_intent(),
            )
            .await,
        Err(RunStoreError::CreationConflict {
            command_id: conflict
        }) if conflict == command_id
    ));

    let mut request = runtime_request();
    request.run_id = Some(retried.reservation.run_id.clone());
    let created = store
        .create(request.clone())
        .await
        .expect("create the reserved run after reopen");
    assert_eq!(created.lease.run_id, retried.reservation.run_id);
    assert!(matches!(
        store.create(request).await,
        Err(RunStoreError::AlreadyExists { run_id }) if run_id == retried.reservation.run_id
    ));

    let replay = store
        .load(&retried.reservation.run_id)
        .await
        .expect("load created reserved run")
        .expect("reserved run exists");
    assert_eq!(replay.events.len(), 1);
    assert!(matches!(
        &replay.events[0].event,
        RuntimeEventKind::RunCreated { request }
            if request.run_id.as_ref() == Some(&retried.reservation.run_id)
    ));
    assert_eq!(
        reduce_events(&replay.events).expect("reduce reserved run"),
        replay.snapshot
    );

    let connection = Connection::open(&fixture.db).expect("open database for single-run assertion");
    let reservation_count = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_run_creations WHERE command_id = ?1",
            params![CREATE_COMMAND_ID],
            |row| row.get::<_, i64>(0),
        )
        .expect("count creation reservations");
    let run_count = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_runs WHERE run_id = ?1",
            params![RUN_ID],
            |row| row.get::<_, i64>(0),
        )
        .expect("count created runs");
    assert_eq!(reservation_count, 1);
    assert_eq!(run_count, 1);
    let pending_payload_count = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_run_creations WHERE command_id = ?1 AND command_json IS NOT NULL",
            params![CREATE_COMMAND_ID],
            |row| row.get::<_, i64>(0),
        )
        .expect("count pending creation payloads");
    assert_eq!(pending_payload_count, 0);
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
