use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codewhale_context::compaction::{ContextInput, effective_context};
use codewhale_runtime::*;
use serde_json::json;

const BASE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FINAL_COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIRTY_REVISION: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ARTIFACT_SHA256: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const DIFF_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const ROOT_WORKSPACE: &str = "/workspace/root";
const WRITER_WORKSPACE: &str = "/workspace/writers/task";

struct OneEventStream {
    event: Option<Result<ModelStreamEvent, ModelPortError>>,
}

#[async_trait]
impl ModelStream for OneEventStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        self.event.take()
    }
}

struct PendingStream;

#[async_trait]
impl ModelStream for PendingStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        std::future::pending().await
    }
}

#[derive(Clone, Copy)]
enum ModelScript {
    Writer,
    SlowWriter,
    TurnLimitedWriter,
    ReadOnlyRole,
    RejectWriter,
    ResumeWriter,
    TwoWritersSameTurn,
    TwoWritersLaterTurn,
    WriterThenReadOnly,
}

struct DeterministicModel {
    script: ModelScript,
    requests_by_run: Mutex<HashMap<RunId, usize>>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl DeterministicModel {
    fn new(script: ModelScript) -> Self {
        Self {
            script,
            requests_by_run: Mutex::new(HashMap::new()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn output(&self, request: &ModelRequest, request_index: usize) -> ModelOutput {
        let turn_limited_writer_child = matches!(self.script, ModelScript::TurnLimitedWriter)
            && request.actor.kind == AgentActorKind::Child;
        let isolated_writer_child = request.actor.kind == AgentActorKind::Child
            && request_index == 0
            && request.tools.iter().any(|tool| tool.name == "write");
        let tool_calls = if turn_limited_writer_child {
            vec![tool_call(
                &format!("writer-edit-{request_index}"),
                "write",
                json!({"path": "src/lib.rs", "content": "improved"}),
            )]
        } else if isolated_writer_child {
            vec![tool_call(
                "writer-edit",
                "write",
                json!({"path": "src/lib.rs", "content": "improved"}),
            )]
        } else {
            match (self.script, request.actor.kind, request_index) {
                (ModelScript::Writer, AgentActorKind::Root, 0)
                | (ModelScript::RejectWriter, AgentActorKind::Root, 0) => vec![tool_call(
                    "writer-call",
                    AGENT_TOOL_NAME,
                    json!({
                        "prompt": "只修改 src/lib.rs 并通过冻结验证",
                        "type": "implementer",
                        "workspace_access": "isolated_write",
                        "allowed_paths": ["src/lib.rs"],
                        "wall_time_secs": 180,
                        "expected_artifact": "一个 Host seal 的提交"
                    }),
                )],
                (ModelScript::TurnLimitedWriter, AgentActorKind::Root, 0) => vec![tool_call(
                    "turn-limited-writer-call",
                    AGENT_TOOL_NAME,
                    json!({
                        "prompt": "持续写入直到冻结的七轮上限",
                        "type": "implementer",
                        "workspace_access": "isolated_write",
                        "allowed_paths": ["src/lib.rs"],
                        "max_steps": 7,
                        "wall_time_secs": 180,
                        "expected_artifact": "一个有界 Writer 结果"
                    }),
                )],
                (ModelScript::SlowWriter, AgentActorKind::Root, 0) => vec![tool_call(
                    "slow-writer-call",
                    AGENT_TOOL_NAME,
                    json!({
                        "prompt": "在自身期限内修改 src/lib.rs",
                        "type": "implementer",
                        "workspace_access": "isolated_write",
                        "allowed_paths": ["src/lib.rs"],
                        "wall_time_secs": 1,
                        "expected_artifact": "一个有界 Writer 结果"
                    }),
                )],
                (ModelScript::ReadOnlyRole, AgentActorKind::Root, 0) => vec![tool_call(
                    "reader-call",
                    AGENT_TOOL_NAME,
                    json!({
                        "prompt": "检查 src/lib.rs",
                        "type": "writer",
                        "expected_artifact": "只读结论"
                    }),
                )],
                (ModelScript::TwoWritersSameTurn, AgentActorKind::Root, 0) => vec![
                    writer_launch_call("writer-one"),
                    writer_launch_call("writer-two"),
                ],
                (ModelScript::TwoWritersLaterTurn, AgentActorKind::Root, 0) => {
                    vec![writer_launch_call("writer-one")]
                }
                (ModelScript::TwoWritersLaterTurn, AgentActorKind::Root, 1) => {
                    vec![writer_launch_call("writer-two")]
                }
                (ModelScript::WriterThenReadOnly, AgentActorKind::Root, 0) => {
                    vec![writer_launch_call("writer-one")]
                }
                (ModelScript::WriterThenReadOnly, AgentActorKind::Root, 1) => vec![tool_call(
                    "reader-after-writer",
                    AGENT_TOOL_NAME,
                    json!({
                        "prompt": "读取合并后的 src/lib.rs 并给出结论",
                        "type": "explorer",
                        "expected_artifact": "只读结论"
                    }),
                )],
                _ => Vec::new(),
            }
        };
        ModelOutput {
            content: if tool_calls.is_empty() {
                match request.actor.kind {
                    AgentActorKind::Root => "根任务完成".to_owned(),
                    AgentActorKind::Child => "子任务完成".to_owned(),
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
            usage: Usage {
                input_tokens: 10,
                output_tokens: 2,
                cache_hit_tokens: 0,
                cache_miss_tokens: 10,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                reasoning_replay_tokens: 0,
            },
        }
    }
}

#[async_trait]
impl ModelPort for DeterministicModel {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        let request_index = {
            let mut by_run = self.requests_by_run.lock().expect("request index lock");
            let index = *by_run.get(&request.run_id).unwrap_or(&0);
            by_run.insert(request.run_id.clone(), index + 1);
            index
        };
        let output = self.output(&request, request_index);
        self.requests
            .lock()
            .expect("request log lock")
            .push(request.clone());
        if matches!(self.script, ModelScript::SlowWriter)
            && request.actor.kind == AgentActorKind::Child
        {
            return Ok(Box::new(PendingStream));
        }
        Ok(Box::new(OneEventStream {
            event: Some(Ok(ModelStreamEvent::Completed { output })),
        }))
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        let requests = self.requests.lock().expect("request log lock");
        let root_requests = requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Root)
            .count() as u64;
        let child_requests = requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Child)
            .count() as u64;
        let response_count = root_requests.saturating_add(child_requests);
        Ok(ModelAccounting {
            hard_request_limit: Some(128),
            root: ActorRequestAccounting {
                started: root_requests,
                completed: root_requests,
                ..ActorRequestAccounting::default()
            },
            child: ActorRequestAccounting {
                started: child_requests,
                completed: child_requests,
                ..ActorRequestAccounting::default()
            },
            complete: true,
            usage_complete: true,
            sealed: seal,
            usage_responses: response_count,
            usage: Usage {
                input_tokens: response_count.saturating_mul(10),
                output_tokens: response_count.saturating_mul(2),
                cache_miss_tokens: response_count.saturating_mul(10),
                ..Usage::default()
            },
            ..ModelAccounting::default()
        })
    }
}

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: ToolArguments::from_value(arguments),
    }
}

fn writer_launch_call(id: &str) -> ModelToolCall {
    tool_call(
        id,
        AGENT_TOOL_NAME,
        json!({
            "prompt": "只修改 src/lib.rs 并通过冻结验证",
            "type": "implementer",
            "workspace_access": "isolated_write",
            "allowed_paths": ["src/lib.rs"],
            "expected_artifact": "一个 Host seal 的提交"
        }),
    )
}

fn definition(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: name.to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

fn known(generation: u64, sha256: &str) -> WorkspaceState {
    WorkspaceState {
        generation,
        revision: WorkspaceRevision::Known {
            sha256: sha256.to_owned(),
        },
    }
}

fn physical_accounting(root_requests: u64, child_requests: u64, limit: u32) -> ModelAccounting {
    let response_count = root_requests.saturating_add(child_requests);
    ModelAccounting {
        hard_request_limit: Some(limit),
        root: ActorRequestAccounting {
            started: root_requests,
            completed: root_requests,
            ..ActorRequestAccounting::default()
        },
        child: ActorRequestAccounting {
            started: child_requests,
            completed: child_requests,
            ..ActorRequestAccounting::default()
        },
        complete: true,
        usage_complete: true,
        usage_responses: response_count,
        usage: Usage {
            input_tokens: response_count.saturating_mul(10),
            output_tokens: response_count.saturating_mul(2),
            cache_miss_tokens: response_count.saturating_mul(10),
            ..Usage::default()
        },
        ..ModelAccounting::default()
    }
}

fn verifier() -> VerifierSpec {
    VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: json!({"suite": "writer"}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "fixture".to_owned(),
                program: "fixture-verifier".to_owned(),
                args: vec!["--exact".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 10_000,
            }],
        },
    }
}

fn passed_verifier(invocation: &ToolInvocation, revision: &str) -> ToolOutcome {
    let artifact_id = format!("artifact:{}:{}", invocation.run_id, invocation.call_id);
    let mut outcome = ToolOutcome::success("冻结验证通过");
    outcome.workspace_revision = Some(revision.to_owned());
    outcome.evidence = ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: vec![artifact_id.clone()],
    };
    outcome.artifacts = vec![ToolArtifact {
        id: artifact_id.clone(),
        status: ToolArtifactStatus::Available,
        sha256: Some(ARTIFACT_SHA256.to_owned()),
        media_type: Some("application/json".to_owned()),
        byte_len: Some(2),
    }];
    outcome.verifier_observation = Some(VerifierObservation {
        spec: VerifierSpec {
            parameters: invocation
                .arguments
                .parsed
                .clone()
                .expect("Host verifier arguments are parsed"),
            ..verifier()
        },
        verdict: VerifierVerdict::Passed,
        workspace_revision: WorkspaceRevision::Known {
            sha256: revision.to_owned(),
        },
        artifact_ids: vec![artifact_id],
    });
    outcome
}

struct RootTools {
    revision: Mutex<String>,
    bytes: Mutex<Vec<u8>>,
    calls: Mutex<Vec<String>>,
    timeline: Arc<Mutex<Vec<String>>>,
}

impl RootTools {
    fn new(timeline: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            revision: Mutex::new(BASE_COMMIT.to_owned()),
            bytes: Mutex::new(b"root-before".to_vec()),
            calls: Mutex::new(Vec::new()),
            timeline,
        }
    }
}

#[async_trait]
impl ToolExecutor for RootTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![definition("run_tests")]
    }

    fn workspace_access(&self, _invocation: &ToolInvocation) -> WorkspaceAccess {
        WorkspaceAccess::ReadOnly
    }

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        Ok(self.revision.lock().expect("root revision lock").clone())
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        self.calls
            .lock()
            .expect("root call lock")
            .push(invocation.name.clone());
        self.timeline
            .lock()
            .expect("timeline lock")
            .push(format!("root-tool:{}", invocation.name));
        if invocation.name != "run_tests" {
            return Err(ToolExecutionError::new(
                "unexpected_root_tool",
                invocation.name,
            ));
        }
        let revision = self.revision.lock().expect("root revision lock").clone();
        Ok(passed_verifier(&invocation, &revision))
    }
}

struct WriterTools {
    revision: Mutex<String>,
    bytes: Mutex<Vec<u8>>,
    calls: Mutex<Vec<String>>,
    timeline: Arc<Mutex<Vec<String>>>,
}

impl WriterTools {
    fn new(timeline: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            revision: Mutex::new(BASE_COMMIT.to_owned()),
            bytes: Mutex::new(b"writer-before".to_vec()),
            calls: Mutex::new(Vec::new()),
            timeline,
        }
    }
}

#[async_trait]
impl ToolExecutor for WriterTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![definition("write"), definition("run_tests")]
    }

    fn workspace_access(&self, invocation: &ToolInvocation) -> WorkspaceAccess {
        if invocation.name == "write" {
            WorkspaceAccess::MayWrite
        } else {
            WorkspaceAccess::ReadOnly
        }
    }

    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        Ok(self.revision.lock().expect("writer revision lock").clone())
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        self.calls
            .lock()
            .expect("writer call lock")
            .push(invocation.name.clone());
        self.timeline
            .lock()
            .expect("timeline lock")
            .push(format!("writer-tool:{}", invocation.name));
        match invocation.name.as_str() {
            "write" => {
                *self.bytes.lock().expect("writer bytes lock") = b"writer-after".to_vec();
                *self.revision.lock().expect("writer revision lock") = DIRTY_REVISION.to_owned();
                Ok(ToolOutcome::success("writer 修改完成")
                    .with_side_effect(ToolSideEffectStatus::Applied))
            }
            "run_tests" => {
                let revision = self.revision.lock().expect("writer revision lock").clone();
                Ok(passed_verifier(&invocation, &revision))
            }
            other => Err(ToolExecutionError::new("unexpected_writer_tool", other)),
        }
    }
}

struct FakeOrchestrator {
    assignment: AgentWorkspaceAssignment,
    root_tools: Arc<RootTools>,
    writer_tools: Arc<WriterTools>,
    timeline: Arc<Mutex<Vec<String>>>,
    prepare_calls: AtomicUsize,
    bind_calls: AtomicUsize,
    bind_side_effects: AtomicUsize,
    seal_calls: AtomicUsize,
    integrate_calls: AtomicUsize,
    integrate_side_effects: AtomicUsize,
    cleanup_calls: AtomicUsize,
    cleanup_side_effects: AtomicUsize,
    workspace_exists: AtomicBool,
    integrated: AtomicBool,
    bind_recovery: AtomicBool,
    cleanup_ambiguous: AtomicBool,
}

impl FakeOrchestrator {
    fn new(root_tools: Arc<RootTools>, writer_tools: Arc<WriterTools>) -> Self {
        let timeline = root_tools.timeline.clone();
        Self {
            assignment: writer_assignment(),
            root_tools,
            writer_tools,
            timeline,
            prepare_calls: AtomicUsize::new(0),
            bind_calls: AtomicUsize::new(0),
            bind_side_effects: AtomicUsize::new(0),
            seal_calls: AtomicUsize::new(0),
            integrate_calls: AtomicUsize::new(0),
            integrate_side_effects: AtomicUsize::new(0),
            cleanup_calls: AtomicUsize::new(0),
            cleanup_side_effects: AtomicUsize::new(0),
            workspace_exists: AtomicBool::new(false),
            integrated: AtomicBool::new(false),
            bind_recovery: AtomicBool::new(false),
            cleanup_ambiguous: AtomicBool::new(false),
        }
    }

    fn with_preexisting_workspace(self) -> Self {
        self.workspace_exists.store(true, Ordering::Release);
        self.bind_side_effects.store(1, Ordering::Release);
        self
    }

    fn with_integrated_root(self) -> Self {
        self.integrated.store(true, Ordering::Release);
        self.integrate_side_effects.store(1, Ordering::Release);
        *self.root_tools.bytes.lock().expect("root bytes lock") = b"root-after".to_vec();
        *self.root_tools.revision.lock().expect("root revision lock") = FINAL_COMMIT.to_owned();
        self
    }

    fn with_cleaned_workspace(self) -> Self {
        self.workspace_exists.store(false, Ordering::Release);
        self.cleanup_side_effects.store(1, Ordering::Release);
        self
    }
}

#[async_trait]
impl AgentOrchestrator for FakeOrchestrator {
    async fn prepare_writer(
        &self,
        request: WriterPreparation,
    ) -> Result<WriterPlan, AgentOrchestrationError> {
        self.prepare_calls.fetch_add(1, Ordering::AcqRel);
        assert_eq!(request.root_workspace, ROOT_WORKSPACE);
        assert_eq!(request.allowed_paths, ["src/lib.rs"]);
        Ok(WriterPlan {
            assignment: self.assignment.clone(),
        })
    }

    async fn bind_writer(
        &self,
        task: &AgentTask,
        _sealed: Option<&WriterSeal>,
    ) -> Result<WriterBinding, AgentOrchestrationError> {
        self.bind_calls.fetch_add(1, Ordering::AcqRel);
        assert_eq!(task.workspace, self.assignment);
        if self.bind_recovery.load(Ordering::Acquire) {
            return Err(AgentOrchestrationError::new(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_bind_ambiguous",
                "无法证明 Writer workspace 是否已创建",
            ));
        }
        if !self.workspace_exists.swap(true, Ordering::AcqRel) {
            self.bind_side_effects.fetch_add(1, Ordering::AcqRel);
            self.timeline
                .lock()
                .expect("timeline lock")
                .push("orchestrator:bind-side-effect".to_owned());
        }
        Ok(WriterBinding {
            assignment: self.assignment.clone(),
            writer_workspace_state: known(0, BASE_COMMIT),
            tools: self.writer_tools.clone(),
        })
    }

    async fn seal_writer(&self, task: &AgentTask) -> Result<WriterSeal, AgentOrchestrationError> {
        self.seal_calls.fetch_add(1, Ordering::AcqRel);
        assert_eq!(task.workspace, self.assignment);
        *self
            .writer_tools
            .revision
            .lock()
            .expect("writer revision lock") = FINAL_COMMIT.to_owned();
        self.timeline
            .lock()
            .expect("timeline lock")
            .push("orchestrator:seal".to_owned());
        Ok(WriterSeal {
            base_commit: BASE_COMMIT.to_owned(),
            final_commit: FINAL_COMMIT.to_owned(),
            diff_sha256: DIFF_SHA256.to_owned(),
            changed_files: vec!["src/lib.rs".to_owned()],
            writer_workspace_state: known(0, FINAL_COMMIT),
        })
    }

    async fn integrate_writer(
        &self,
        task: &AgentTask,
        seal: &WriterSeal,
        expected_root: &WorkspaceState,
    ) -> Result<WriterIntegration, AgentOrchestrationError> {
        self.integrate_calls.fetch_add(1, Ordering::AcqRel);
        assert_eq!(task.workspace, self.assignment);
        assert_eq!(seal.final_commit, FINAL_COMMIT);
        if !self.integrated.swap(true, Ordering::AcqRel) {
            assert_eq!(
                self.root_tools
                    .bytes
                    .lock()
                    .expect("root bytes lock")
                    .as_slice(),
                b"root-before",
                "root executor bytes changed before the guarded integration side effect"
            );
            assert_eq!(
                expected_root.revision,
                WorkspaceRevision::Known {
                    sha256: BASE_COMMIT.to_owned()
                }
            );
            *self.root_tools.bytes.lock().expect("root bytes lock") = b"root-after".to_vec();
            *self.root_tools.revision.lock().expect("root revision lock") = FINAL_COMMIT.to_owned();
            self.integrate_side_effects.fetch_add(1, Ordering::AcqRel);
            self.timeline
                .lock()
                .expect("timeline lock")
                .push("orchestrator:integrate-side-effect".to_owned());
        } else {
            self.timeline
                .lock()
                .expect("timeline lock")
                .push("orchestrator:integrate-already-applied".to_owned());
        }
        Ok(WriterIntegration {
            root_head_commit: FINAL_COMMIT.to_owned(),
            root_workspace_state: known(expected_root.generation + 1, FINAL_COMMIT),
        })
    }

    async fn cleanup_writer(
        &self,
        task: &AgentTask,
        _seal: Option<&WriterSeal>,
    ) -> Result<WriterCleanup, AgentOrchestrationError> {
        self.cleanup_calls.fetch_add(1, Ordering::AcqRel);
        assert_eq!(task.workspace, self.assignment);
        if self.cleanup_ambiguous.load(Ordering::Acquire) {
            return Ok(WriterCleanup {
                worktree_removed: false,
                branch_removed: false,
                retained_for_recovery: true,
                reason: Some("所有权状态无法精确证明".to_owned()),
            });
        }
        if self.workspace_exists.swap(false, Ordering::AcqRel) {
            self.cleanup_side_effects.fetch_add(1, Ordering::AcqRel);
            self.timeline
                .lock()
                .expect("timeline lock")
                .push("orchestrator:cleanup-side-effect".to_owned());
        } else {
            self.timeline
                .lock()
                .expect("timeline lock")
                .push("orchestrator:cleanup-already-absent".to_owned());
        }
        Ok(WriterCleanup {
            worktree_removed: true,
            branch_removed: true,
            retained_for_recovery: false,
            reason: None,
        })
    }
}

fn writer_assignment() -> AgentWorkspaceAssignment {
    AgentWorkspaceAssignment {
        access: AgentWorkspaceAccess::IsolatedWrite,
        root_workspace: ROOT_WORKSPACE.to_owned(),
        base_commit: BASE_COMMIT.to_owned(),
        worktree_path: Some(WRITER_WORKSPACE.to_owned()),
        root_branch: Some("deepseek-agent".to_owned()),
        branch: Some("codewhale/writer/task".to_owned()),
        allowed_paths: vec!["src/lib.rs".to_owned()],
        owner_token: Some("owner-task".to_owned()),
    }
}

#[derive(Default)]
struct CollectSink {
    events: Mutex<Vec<StoredRuntimeEvent>>,
    timeline: Arc<Mutex<Vec<String>>>,
}

impl CollectSink {
    fn with_timeline(timeline: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            timeline,
        }
    }
}

#[async_trait]
impl RuntimeEventSink for CollectSink {
    async fn emit(&self, event: StoredRuntimeEvent) {
        let label = match &event.event {
            RuntimeEventKind::AgentTaskPrepared { .. } => Some("event:task-prepared"),
            RuntimeEventKind::AgentWorkspaceCreated { .. } => Some("event:workspace-created"),
            RuntimeEventKind::ChildStarted { .. } => Some("event:child-started"),
            RuntimeEventKind::AgentSealPrepared { .. } => Some("event:seal-prepared"),
            RuntimeEventKind::AgentSealCommitted { .. } => Some("event:seal-committed"),
            RuntimeEventKind::AgentIntegrationPrepared { .. } => Some("event:integration-prepared"),
            RuntimeEventKind::AgentIntegrationStarted { .. } => Some("event:integration-started"),
            RuntimeEventKind::AgentIntegrationCommitted { .. } => {
                Some("event:integration-committed")
            }
            RuntimeEventKind::AgentCleanupPrepared { .. } => Some("event:cleanup-prepared"),
            RuntimeEventKind::AgentCleanupCommitted { .. } => Some("event:cleanup-committed"),
            RuntimeEventKind::HostVerificationCommitted { .. } if event.parent_run_id.is_none() => {
                Some("event:host-verification-committed")
            }
            _ => None,
        };
        if let Some(label) = label {
            self.timeline
                .lock()
                .expect("timeline lock")
                .push(label.to_owned());
        }
        self.events.lock().expect("event lock").push(event);
    }
}

fn root_request(exact_verifier: bool, auto_approve: bool) -> RunRequest {
    let run_id = RunId::new();
    let definition = if exact_verifier {
        TaskDefinition {
            objective: "让隔离 writer 修改一个文件".to_owned(),
            constraints: vec!["只修改 src/lib.rs".to_owned()],
            non_goals: Vec::new(),
            acceptance: vec![TaskAcceptance::Verifier {
                id: AcceptanceId::from("tests"),
                description: "冻结验证必须通过".to_owned(),
                verifier: verifier(),
            }],
        }
    } else {
        TaskDefinition::host("让隔离 writer 修改一个文件")
    };
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId::from(run_id.0.clone()),
            definition,
        },
        "系统提示",
    );
    request.run_id = Some(run_id);
    request.environment.workspace = ROOT_WORKSPACE.to_owned();
    request.environment.auto_approve = auto_approve;
    request.context_policy.hard_input_tokens = 900_000;
    request.limits.max_model_requests = 32;
    request.limits.max_turns = 16;
    request
}

struct RuntimeFixture {
    runtime: Arc<AgentRuntime>,
    model: Arc<DeterministicModel>,
    root_tools: Arc<RootTools>,
    writer_tools: Arc<WriterTools>,
    orchestrator: Arc<FakeOrchestrator>,
    sink: Arc<CollectSink>,
    store: Arc<InMemoryRunStore>,
}

fn runtime_fixture(script: ModelScript) -> RuntimeFixture {
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline.clone()));
    let orchestrator = Arc::new(FakeOrchestrator::new(
        root_tools.clone(),
        writer_tools.clone(),
    ));
    let sink = Arc::new(CollectSink::with_timeline(timeline));
    let store = Arc::new(InMemoryRunStore::default());
    let model = Arc::new(DeterministicModel::new(script));
    let runtime = Arc::new(
        AgentRuntime::new(
            model.clone(),
            root_tools.clone(),
            sink.clone(),
            store.clone(),
        )
        .with_orchestrator(orchestrator.clone()),
    );
    RuntimeFixture {
        runtime,
        model,
        root_tools,
        writer_tools,
        orchestrator,
        sink,
        store,
    }
}

fn recovery_root_request() -> RunRequest {
    let mut request = root_request(true, true);
    let run_id = RunId::from("recovery-root");
    request.run_id = Some(run_id.clone());
    request
        .task_contract
        .as_mut()
        .expect("root task contract")
        .generation_id = TaskGenerationId::from(run_id.0);
    request
}

fn recovery_task(request: &RunRequest) -> AgentTask {
    let child_run_id = RunId::from("recovery-child");
    let mut child_policy = request.tool_policy.clone();
    child_policy.denied.push(AGENT_TOOL_NAME.to_owned());
    AgentTask {
        task_id: AgentTaskId::from("writer-recovery-child"),
        root_run_id: request.run_id.clone().expect("root run id"),
        parent_run_id: request.run_id.clone().expect("parent run id"),
        child_run_id: child_run_id.clone(),
        call_id: "writer-call".to_owned(),
        role: "implementer".to_owned(),
        task_contract: TaskContract {
            generation_id: TaskGenerationId::from(child_run_id.0),
            definition: TaskDefinition {
                objective: "只修改 src/lib.rs 并通过冻结验证".to_owned(),
                constraints: vec!["只允许修改这些相对路径：src/lib.rs".to_owned()],
                non_goals: vec!["不得修改主工作区、其他 worktree 或 Git 元数据".to_owned()],
                acceptance: request
                    .task_contract
                    .as_ref()
                    .expect("root task contract")
                    .definition
                    .acceptance
                    .clone(),
            },
        },
        workspace: writer_assignment(),
        tool_policy: child_policy,
        limits: request.limits,
        deadline_unix_ms: request.deadline_unix_ms,
        expected_artifact: "一个 Host seal 的提交".to_owned(),
    }
}

fn child_receipt(task: &AgentTask) -> EvidenceReceipt {
    EvidenceReceipt {
        id: EvidenceReceiptId::from("child-receipt"),
        generation_id: task.task_contract.generation_id.clone(),
        acceptance_id: AcceptanceId::from("tests"),
        verification_id: VerificationId::from("child-verification"),
        verifier: verifier(),
        workspace_state: known(3, DIRTY_REVISION),
        artifact_ids: vec!["child-artifact".to_owned()],
    }
}

fn collected_child_outcome(task: &AgentTask) -> AgentOutcome {
    let receipt = child_receipt(task);
    AgentOutcome {
        run_id: task.child_run_id.clone(),
        parent_run_id: Some(task.parent_run_id.clone()),
        terminal: TerminalState::Completed {
            message: "子任务完成".to_owned(),
            decision: CompletionDecision {
                candidate_id: CompletionCandidateId::from("child-candidate"),
                generation_id: task.task_contract.generation_id.clone(),
                workspace_state: known(3, DIRTY_REVISION),
                satisfied: vec![AcceptanceSatisfaction::Evidence {
                    acceptance_id: receipt.acceptance_id.clone(),
                    receipt_id: receipt.id.clone(),
                }],
            },
        },
        accounting: ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
        runtime_model_requests: 2,
        runtime_retries: 0,
        tool_calls: 2,
        details: AgentResultDetails {
            summary: "子任务完成".to_owned(),
            evidence: vec![receipt],
            changed_files: vec!["src/lib.rs".to_owned()],
            workspace: Some(task.workspace.clone()),
            workspace_state: Some(known(4, FINAL_COMMIT)),
            base_commit: Some(BASE_COMMIT.to_owned()),
            final_commit: Some(FINAL_COMMIT.to_owned()),
            diff_sha256: Some(DIFF_SHA256.to_owned()),
            integration: WriterIntegrationStatus::AwaitingHost,
            ..AgentResultDetails::default()
        },
    }
}

fn integrated_child_outcome(task: &AgentTask) -> AgentOutcome {
    let mut outcome = collected_child_outcome(task);
    outcome.details.integration = WriterIntegrationStatus::Integrated {
        integration_id: OperationId::from("recovery-integration"),
        writer_commit: FINAL_COMMIT.to_owned(),
        root_workspace_state: known(2, FINAL_COMMIT),
    };
    outcome
}

async fn append(store: &InMemoryRunStore, lease: &RunLease, name: &str, event: RuntimeEventKind) {
    store
        .append(
            lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(name.to_owned()),
                event,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("append {name}: {error}"));
}

fn context_for(snapshot: &RunSnapshot, tools: &[ToolDefinition]) -> ModelRequest {
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
        tools,
    })
    .expect("canonical recovery context");
    ModelRequest {
        run_id: snapshot.request.run_id.clone().expect("run id"),
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools: tools.to_vec(),
        reasoning_effort: snapshot.request.reasoning_effort,
        max_output_tokens: snapshot.request.max_output_tokens,
        streaming: snapshot.request.streaming,
        request_number: snapshot.local_turns + 1,
        attempt: 0,
    }
}

#[derive(Debug, Clone, Copy)]
enum RecoveryCheckpoint {
    TaskPrepared,
    WorkspaceCreated,
    ChildTerminal,
    SealCommitted,
    ResultCollected,
    IntegrationStarted,
    ChildFinished,
    CleanupPrepared,
}

async fn seed_recovery_checkpoint(
    store: &InMemoryRunStore,
    checkpoint: RecoveryCheckpoint,
    max_model_requests: u32,
    model_accounting: ModelAccounting,
) -> (RunId, AgentTask) {
    let mut request = recovery_root_request();
    request.limits.max_model_requests = max_model_requests;
    let task = recovery_task(&request);
    let run_id = request.run_id.clone().expect("root run id");
    let created = store.create(request).await.expect("create recovery root");
    append(
        store,
        &created.lease,
        "root-observed",
        RuntimeEventKind::WorkspaceObserved {
            workspace_state: known(1, BASE_COMMIT),
        },
    )
    .await;
    let replay = store.load(&run_id).await.unwrap().unwrap();
    let agent_definition = ToolDefinition {
        name: AGENT_TOOL_NAME.to_owned(),
        description: "agent".to_owned(),
        input_schema: json!({"type": "object"}),
    };
    let prepared_request = context_for(&replay.snapshot, std::slice::from_ref(&agent_definition));
    let call = tool_call(
        "writer-call",
        AGENT_TOOL_NAME,
        json!({
            "prompt": "只修改 src/lib.rs 并通过冻结验证",
            "type": "implementer",
            "workspace_access": "isolated_write",
            "allowed_paths": ["src/lib.rs"],
            "expected_artifact": "一个 Host seal 的提交"
        }),
    );
    let attempt_id = AttemptId("recovery-attempt".to_owned());
    append(
        store,
        &created.lease,
        "model-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(prepared_request),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "model-started",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: attempt_id.clone(),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "model-committed",
        RuntimeEventKind::ModelResponseCommitted {
            attempt_id,
            output: Box::new(ModelOutput {
                content: String::new(),
                reasoning_content: None,
                tool_calls: vec![call.clone()],
                finish_reason: ModelFinishReason::ToolCalls,
                usage: Usage::default(),
            }),
            accounting: Box::new(model_accounting),
        },
    )
    .await;
    let operation_id = OperationId::from("recovery-agent-operation");
    append(
        store,
        &created.lease,
        "tool-prepared",
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: run_id.clone(),
                call_id: call.id,
                name: call.name,
                arguments: call.arguments,
            },
            workspace_access: WorkspaceAccess::MayWrite,
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "tool-started",
        RuntimeEventKind::ToolExecutionStarted { operation_id },
    )
    .await;
    append(
        store,
        &created.lease,
        "task-prepared",
        RuntimeEventKind::AgentTaskPrepared {
            task: Box::new(task.clone()),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::TaskPrepared) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "workspace-created",
        RuntimeEventKind::AgentWorkspaceCreated {
            task_id: task.task_id.clone(),
            assignment: task.workspace.clone(),
            writer_workspace_state: known(0, BASE_COMMIT),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::WorkspaceCreated) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "child-started",
        RuntimeEventKind::ChildStarted {
            task_id: task.task_id.clone(),
            call_id: task.call_id.clone(),
            child_run_id: task.child_run_id.clone(),
            depth: 1,
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::ChildTerminal) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "seal-prepared",
        RuntimeEventKind::AgentSealPrepared {
            task_id: task.task_id.clone(),
            base_commit: BASE_COMMIT.to_owned(),
            writer_workspace_state_before: known(3, DIRTY_REVISION),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "seal-committed",
        RuntimeEventKind::AgentSealCommitted {
            task_id: task.task_id.clone(),
            final_commit: FINAL_COMMIT.to_owned(),
            diff_sha256: DIFF_SHA256.to_owned(),
            changed_files: vec!["src/lib.rs".to_owned()],
            writer_workspace_state_after: known(4, FINAL_COMMIT),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::SealCommitted) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "result-collected",
        RuntimeEventKind::AgentResultCollected {
            task_id: task.task_id.clone(),
            outcome: Box::new(collected_child_outcome(&task)),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::ResultCollected) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "integration-prepared",
        RuntimeEventKind::AgentIntegrationPrepared {
            task_id: task.task_id.clone(),
            integration_id: OperationId::from("recovery-integration"),
            base_commit: BASE_COMMIT.to_owned(),
            writer_commit: FINAL_COMMIT.to_owned(),
            diff_sha256: DIFF_SHA256.to_owned(),
            expected_root_workspace_state: known(1, BASE_COMMIT),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "integration-started",
        RuntimeEventKind::AgentIntegrationStarted {
            task_id: task.task_id.clone(),
            integration_id: OperationId::from("recovery-integration"),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::IntegrationStarted) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "integration-committed",
        RuntimeEventKind::AgentIntegrationCommitted {
            task_id: task.task_id.clone(),
            integration_id: OperationId::from("recovery-integration"),
            root_head_commit: FINAL_COMMIT.to_owned(),
            root_workspace_state_after: known(2, FINAL_COMMIT),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "child-finished",
        RuntimeEventKind::ChildFinished {
            call_id: task.call_id.clone(),
            outcome: Box::new(integrated_child_outcome(&task)),
            accounting: Box::new(ModelAccounting {
                complete: true,
                usage_complete: true,
                ..ModelAccounting::default()
            }),
            handoff_content: "writer integrated".to_owned(),
        },
    )
    .await;
    if matches!(checkpoint, RecoveryCheckpoint::ChildFinished) {
        store.release(&created.lease).await.unwrap();
        return (run_id, task);
    }
    append(
        store,
        &created.lease,
        "tool-committed",
        RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: OperationId::from("recovery-agent-operation"),
            call_id: task.call_id.clone(),
            name: AGENT_TOOL_NAME.to_owned(),
            outcome: Box::new(
                ToolOutcome::success("writer integrated")
                    .with_side_effect(ToolSideEffectStatus::Applied),
            ),
            workspace_state: Some(known(2, FINAL_COMMIT)),
        },
    )
    .await;
    append(
        store,
        &created.lease,
        "cleanup-prepared",
        RuntimeEventKind::AgentCleanupPrepared {
            task_id: task.task_id.clone(),
            worktree_path: WRITER_WORKSPACE.to_owned(),
            branch: "codewhale/writer/task".to_owned(),
            owner_token: "owner-task".to_owned(),
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();
    (run_id, task)
}

fn recovery_runtime(
    store: Arc<InMemoryRunStore>,
    root_tools: Arc<RootTools>,
    orchestrator: Arc<FakeOrchestrator>,
) -> Arc<AgentRuntime> {
    Arc::new(
        AgentRuntime::new(
            Arc::new(DeterministicModel::new(ModelScript::ResumeWriter)),
            root_tools,
            Arc::new(CollectSink::default()),
            store,
        )
        .with_orchestrator(orchestrator),
    )
}

fn recovery_child_request(task: &AgentTask, accounting_baseline: ModelAccounting) -> RunRequest {
    let mut request = RunRequest::new(task.task_contract.clone(), "writer 系统提示");
    request.run_id = Some(task.child_run_id.clone());
    request.parent_run_id = Some(task.parent_run_id.clone());
    request.streaming = false;
    request.actor = AgentActor {
        kind: AgentActorKind::Child,
        depth: 1,
    };
    request.agent_task = Some(task.clone());
    request.deadline_unix_ms = task.deadline_unix_ms;
    request.tool_policy = task.tool_policy.clone();
    request.limits = task.limits;
    request.environment.workspace = task.workspace.execution_workspace().to_owned();
    request.environment.auto_approve = true;
    request.environment.trust_mode = false;
    request.environment.allow_sandbox_elevation = false;
    request.environment.interactive = false;
    request.environment.sandbox = Some("isolated_writer".to_owned());
    request.context_policy.hard_input_tokens = 900_000;
    request.accounting_baseline = accounting_baseline;
    request
}

async fn complete_recovery_child(
    store: Arc<InMemoryRunStore>,
    task: &AgentTask,
    writer_tools: Arc<WriterTools>,
    accounting_baseline: ModelAccounting,
) -> AgentOutcome {
    let created = store
        .create(recovery_child_request(task, accounting_baseline))
        .await
        .expect("create seeded writer child");
    store
        .release(&created.lease)
        .await
        .expect("release seeded writer child");
    Arc::new(AgentRuntime::new(
        Arc::new(DeterministicModel::new(ModelScript::ResumeWriter)),
        writer_tools,
        Arc::new(CollectSink::default()),
        store.clone(),
    ))
    .resume(task.child_run_id.clone())
    .wait()
    .await
    .expect("complete seeded writer child")
}

fn position(timeline: &[String], expected: &str) -> usize {
    timeline
        .iter()
        .position(|event| event == expected)
        .unwrap_or_else(|| panic!("timeline is missing {expected}: {timeline:?}"))
}

#[tokio::test]
async fn writer_vertical_slice_uses_one_runtime_fresh_tools_and_root_post_merge_evidence() {
    let RuntimeFixture {
        runtime,
        model,
        root_tools,
        writer_tools,
        orchestrator,
        sink,
        store,
    } = runtime_fixture(ModelScript::Writer);
    let mut request = root_request(true, true);
    request.limits.wall_time_ms = Some(225_000);
    let outcome = runtime.start(request).wait().await.unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "unexpected root terminal: {:?}",
        outcome.terminal
    );

    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    let lifecycle = replay.snapshot.agent_tasks.first().expect("writer task");
    assert_eq!(
        lifecycle.task.workspace.access,
        AgentWorkspaceAccess::IsolatedWrite
    );
    assert_eq!(
        lifecycle.task.workspace.execution_workspace(),
        WRITER_WORKSPACE
    );
    let child_replay = store
        .load(&lifecycle.task.child_run_id)
        .await
        .unwrap()
        .expect("writer child replay");
    assert_eq!(
        lifecycle.task.deadline_unix_ms, child_replay.snapshot.request.deadline_unix_ms,
        "frozen AgentTask and child RunRequest must share one deadline"
    );
    assert_eq!(lifecycle.task.limits.wall_time_ms, Some(180_000));
    assert!(
        lifecycle.task.deadline_unix_ms < replay.snapshot.request.deadline_unix_ms,
        "writer child wall-time must shorten the parent deadline"
    );
    assert_eq!(
        root_tools.calls.lock().expect("root call lock").as_slice(),
        ["run_tests"]
    );
    assert_eq!(
        writer_tools
            .calls
            .lock()
            .expect("writer call lock")
            .as_slice(),
        ["write", "run_tests"]
    );
    assert_eq!(orchestrator.bind_side_effects.load(Ordering::Acquire), 1);
    assert_eq!(
        orchestrator.integrate_side_effects.load(Ordering::Acquire),
        1
    );
    assert_eq!(orchestrator.cleanup_side_effects.load(Ordering::Acquire), 1);
    let requests = model.requests.lock().expect("model request log");
    let writer_request = requests
        .iter()
        .find(|request| request.actor.kind == AgentActorKind::Child)
        .expect("writer child model request");
    let writer_instructions = writer_request
        .system_prompt
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(writer_instructions.contains("至少一次写工具成功前不得提出完成"));
    assert!(writer_instructions.contains("写后重新读取相关文件核对最终内容"));

    let seal = lifecycle.seal.as_ref().expect("seal lifecycle");
    let child_result = lifecycle.result.as_ref().expect("child result");
    let child_receipt = child_result
        .details
        .evidence
        .first()
        .expect("child receipt");
    assert_eq!(
        child_receipt.workspace_state, seal.writer_workspace_state_before,
        "child receipt proves the exact pre-seal bytes"
    );
    assert_ne!(
        child_receipt.workspace_state,
        seal.committed
            .as_ref()
            .expect("committed seal")
            .writer_workspace_state_after,
        "Host sealing changes Git workspace identity without promoting child evidence"
    );
    assert!(
        replay
            .snapshot
            .evidence_receipts
            .iter()
            .all(|receipt| receipt.id != child_receipt.id),
        "child receipts must not enter root evidence"
    );
    let root_receipt = replay
        .snapshot
        .evidence_receipts
        .first()
        .expect("post-merge root receipt");
    assert_eq!(
        root_receipt.generation_id,
        replay
            .snapshot
            .request
            .task_contract
            .as_ref()
            .expect("root task contract")
            .generation_id
    );
    assert_eq!(
        root_receipt.workspace_state, replay.snapshot.workspace_state,
        "root completion requires fresh evidence for the integrated revision"
    );
    assert_eq!(
        replay.snapshot.workspace_state.revision,
        WorkspaceRevision::Known {
            sha256: FINAL_COMMIT.to_owned()
        }
    );

    let integration_commits = replay
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.event,
                RuntimeEventKind::AgentIntegrationCommitted { .. }
            )
        })
        .count();
    assert_eq!(integration_commits, 1);
    let timeline = sink.timeline.lock().expect("timeline lock").clone();
    assert!(
        position(&timeline, "orchestrator:integrate-side-effect")
            < position(&timeline, "event:integration-committed")
    );
    assert!(
        position(&timeline, "event:integration-committed")
            < position(&timeline, "event:host-verification-committed"),
        "root exact verifier must run only after integration is durable"
    );
}

#[tokio::test]
async fn writer_child_times_out_at_its_shorter_frozen_deadline() {
    let RuntimeFixture { runtime, store, .. } = runtime_fixture(ModelScript::SlowWriter);
    let mut request = root_request(true, true);
    request.limits.wall_time_ms = Some(5_000);
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runtime.start(request).wait(),
    )
    .await
    .expect("root must converge after the child deadline")
    .expect("root runtime joins");
    let root_replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(
        root_replay.snapshot.terminal.as_ref(),
        Some(&outcome),
        "root deadline convergence must be durably terminal, not only return an in-memory outcome",
    );
    let lifecycle = root_replay
        .snapshot
        .agent_tasks
        .first()
        .expect("slow writer task");
    let child_replay = store
        .load(&lifecycle.task.child_run_id)
        .await
        .unwrap()
        .expect("slow writer child replay");
    assert_eq!(lifecycle.task.limits.wall_time_ms, Some(1_000));
    assert_eq!(
        lifecycle.task.deadline_unix_ms,
        child_replay.snapshot.request.deadline_unix_ms
    );
    assert!(lifecycle.task.deadline_unix_ms < root_replay.snapshot.request.deadline_unix_ms);
    assert!(matches!(
        child_replay.snapshot.terminal,
        Some(AgentOutcome {
            terminal: TerminalState::Failed {
                failure: RuntimeFailure::Timeout {
                    phase: RuntimeTimeoutPhase::Run,
                    timeout_ms: 1_000,
                },
            },
            ..
        })
    ));
}

#[tokio::test]
async fn turn_limited_writer_still_persists_the_root_terminal() {
    let RuntimeFixture {
        runtime,
        model,
        store,
        ..
    } = runtime_fixture(ModelScript::TurnLimitedWriter);
    let mut request = root_request(true, true);
    request.limits.max_model_requests = 10;
    request.limits.max_turns = 10;
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runtime.start(request).wait(),
    )
    .await
    .expect("turn-limited writer root must converge")
    .expect("root runtime joins");
    let root_replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(
        root_replay.snapshot.terminal.as_ref(),
        Some(&outcome),
        "turn exhaustion must settle the Writer lifecycle before the root terminal",
    );
    let requests = model.requests.lock().expect("request log");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Root)
            .count(),
        2,
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Child)
            .count(),
        7,
    );
}

#[tokio::test]
async fn failed_turn_limited_writer_with_retained_cleanup_is_recovery_required() {
    let RuntimeFixture {
        runtime,
        model,
        root_tools,
        orchestrator,
        store,
        ..
    } = runtime_fixture(ModelScript::TurnLimitedWriter);
    orchestrator
        .cleanup_ambiguous
        .store(true, Ordering::Release);
    let mut request = root_request(true, true);
    request.limits.max_model_requests = 10;
    request.limits.max_turns = 10;
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runtime.start(request).wait(),
    )
    .await
    .expect("retained Writer cleanup must converge")
    .expect("root runtime joins");
    let root_replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    let lifecycle = root_replay
        .snapshot
        .agent_tasks
        .first()
        .expect("retained Writer lifecycle");
    let writer_terminal = lifecycle
        .finished
        .as_ref()
        .map(|finished| &finished.outcome.terminal);
    assert!(
        matches!(outcome.terminal, TerminalState::RecoveryRequired { .. }),
        "retained Writer cleanup must replace the ordinary root terminal; root={:?}, writer={writer_terminal:?}",
        outcome.terminal,
    );
    assert_eq!(
        root_replay.snapshot.terminal.as_ref(),
        Some(&outcome),
        "the recovery-required root terminal must be canonical",
    );
    assert!(matches!(
        writer_terminal,
        Some(TerminalState::RecoveryRequired { .. })
    ));
    assert_eq!(writer_terminal, Some(&outcome.terminal));
    assert!(lifecycle.integration.is_none());
    assert!(lifecycle.cleanup.as_ref().is_some_and(|cleanup| {
        cleanup.committed.as_ref().is_some_and(|committed| {
            committed.retained_for_recovery
                && !committed.worktree_removed
                && !committed.branch_removed
        })
    }));
    assert_eq!(
        root_replay.snapshot.workspace_state,
        known(1, BASE_COMMIT),
        "an unintegrated Writer recovery must not advance the root workspace generation",
    );
    assert_eq!(
        root_tools
            .revision
            .lock()
            .expect("root revision lock")
            .as_str(),
        BASE_COMMIT,
    );
    let agent_outcomes = root_replay
        .events
        .iter()
        .filter_map(|event| match &event.event {
            RuntimeEventKind::ToolOutcomeCommitted {
                call_id,
                name,
                outcome,
                workspace_state,
                ..
            } if call_id == "turn-limited-writer-call" && name == AGENT_TOOL_NAME => {
                Some((outcome, workspace_state))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_outcomes.len(), 1);
    let (agent_outcome, workspace_state) = agent_outcomes[0];
    assert_eq!(agent_outcome.invocation, ToolInvocationStatus::Accepted);
    assert_eq!(agent_outcome.transport, ToolTransportStatus::Indeterminate);
    assert_eq!(agent_outcome.operation, ToolOperationStatus::Cancelled);
    assert_eq!(
        agent_outcome.side_effect,
        ToolSideEffectStatus::Indeterminate
    );
    assert_eq!(agent_outcome.retry, ToolRetryDisposition::Unsafe);
    assert!(agent_outcome.workspace_revision.is_none());
    assert_eq!(workspace_state, &None);
    assert!(root_replay.snapshot.pending_tool.is_none());
    assert_eq!(
        root_replay
            .snapshot
            .transcript
            .entries
            .iter()
            .filter(|entry| matches!(
                entry,
                TranscriptEntry::Tool { call_id, .. }
                    if call_id == "turn-limited-writer-call"
            ))
            .count(),
        1,
    );
    assert_eq!(
        root_replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::Terminal { .. }))
            .count(),
        1,
        "the root recovery terminal must be appended exactly once",
    );
    let child_replay = store
        .load(&lifecycle.task.child_run_id)
        .await
        .unwrap()
        .expect("turn-limited Writer child replay");
    assert!(matches!(
        child_replay.snapshot.terminal,
        Some(AgentOutcome {
            terminal: TerminalState::Failed { .. },
            ..
        })
    ));
    assert_eq!(
        child_replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::Terminal { .. }))
            .count(),
        1,
        "the child must retain its single original failure terminal",
    );
    assert_eq!(orchestrator.cleanup_calls.load(Ordering::Acquire), 1);
    assert_eq!(orchestrator.seal_calls.load(Ordering::Acquire), 0);
    assert_eq!(orchestrator.integrate_calls.load(Ordering::Acquire), 0);
    let requests = model.requests.lock().expect("request log");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Root)
            .count(),
        1,
        "the root must stop immediately after canonical Writer recovery",
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Child)
            .count(),
        7,
    );
}

#[tokio::test]
async fn isolated_writer_requires_auto_approve_and_one_exact_verifier() {
    for (exact, auto_approve, expected_error) in [
        (true, false, "writer_requires_auto_approve"),
        (false, true, "writer_requires_exact_verifier"),
    ] {
        let RuntimeFixture {
            runtime,
            orchestrator,
            store,
            ..
        } = runtime_fixture(ModelScript::RejectWriter);
        let outcome = runtime
            .start(root_request(exact, auto_approve))
            .wait()
            .await
            .unwrap();
        assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
        let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
        assert!(replay.snapshot.agent_tasks.is_empty());
        assert_eq!(orchestrator.prepare_calls.load(Ordering::Acquire), 0);
        assert!(replay.events.iter().any(|event| matches!(
            &event.event,
            RuntimeEventKind::ToolOutcomeCommitted { outcome, .. }
                if outcome.content.contains(expected_error)
                    && outcome.invocation == ToolInvocationStatus::Rejected
                    && outcome.side_effect == ToolSideEffectStatus::NotApplied
        )));
    }
}

#[tokio::test]
async fn one_root_rejects_a_second_writer_in_the_same_batch_or_a_later_turn() {
    for script in [
        ModelScript::TwoWritersSameTurn,
        ModelScript::TwoWritersLaterTurn,
    ] {
        let RuntimeFixture {
            runtime,
            orchestrator,
            store,
            ..
        } = runtime_fixture(script);
        let outcome = runtime
            .start(root_request(true, true))
            .wait()
            .await
            .unwrap();
        assert!(
            matches!(outcome.terminal, TerminalState::Completed { .. }),
            "the rejected second writer must not poison the accepted first writer: {:?}",
            outcome.terminal
        );
        let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
        assert_eq!(
            replay
                .snapshot
                .agent_tasks
                .iter()
                .filter(|lifecycle| {
                    lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                })
                .count(),
            1
        );
        assert_eq!(orchestrator.prepare_calls.load(Ordering::Acquire), 1);
        assert_eq!(orchestrator.bind_side_effects.load(Ordering::Acquire), 1);
        assert_eq!(
            orchestrator.integrate_side_effects.load(Ordering::Acquire),
            1
        );
        assert!(replay.events.iter().any(|event| matches!(
            &event.event,
            RuntimeEventKind::ToolOutcomeCommitted {
                call_id,
                outcome,
                workspace_state: None,
                ..
            } if call_id == "writer-two"
                && outcome.content.contains("writer_single_root_limit")
                && outcome.invocation == ToolInvocationStatus::Rejected
                && outcome.side_effect == ToolSideEffectStatus::NotApplied
        )));
    }
}

#[tokio::test]
async fn a_finished_writer_does_not_block_a_later_read_only_agent() {
    let RuntimeFixture {
        runtime,
        orchestrator,
        store,
        ..
    } = runtime_fixture(ModelScript::WriterThenReadOnly);
    let outcome = runtime
        .start(root_request(true, true))
        .wait()
        .await
        .unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.agent_tasks.len(), 2);
    assert!(replay.snapshot.agent_tasks.iter().any(|lifecycle| {
        lifecycle.task.call_id == "writer-one"
            && lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
            && lifecycle.finished.is_some()
    }));
    assert!(replay.snapshot.agent_tasks.iter().any(|lifecycle| {
        lifecycle.task.call_id == "reader-after-writer"
            && lifecycle.task.workspace.access == AgentWorkspaceAccess::ReadOnly
            && lifecycle.finished.is_some()
    }));
    assert_eq!(orchestrator.prepare_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        orchestrator.integrate_side_effects.load(Ordering::Acquire),
        1
    );
}

#[tokio::test]
async fn role_label_never_grants_write_and_read_only_child_keeps_minimal_lifecycle() {
    let RuntimeFixture {
        runtime,
        orchestrator,
        store,
        ..
    } = runtime_fixture(ModelScript::ReadOnlyRole);
    let definitions = runtime.tool_definitions(&ToolPolicy::default(), 0, 1, false);
    let agent = definitions
        .iter()
        .find(|definition| definition.name == AGENT_TOOL_NAME)
        .expect("agent definition");
    assert_eq!(
        agent.input_schema["properties"]["workspace_access"]["enum"],
        json!(["read_only", "isolated_write"])
    );
    assert!(
        agent.input_schema["properties"]["type"]["description"]
            .as_str()
            .expect("role description")
            .contains("不授予写权限")
    );

    let outcome = runtime
        .start(root_request(false, false))
        .wait()
        .await
        .unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    let lifecycle = replay.snapshot.agent_tasks.first().expect("read-only task");
    assert_eq!(lifecycle.task.role, "writer");
    assert_eq!(
        lifecycle.task.workspace.access,
        AgentWorkspaceAccess::ReadOnly
    );
    assert!(lifecycle.workspace_created.is_none());
    assert!(lifecycle.seal.is_none());
    assert!(lifecycle.integration.is_none());
    assert!(lifecycle.cleanup.is_none());
    assert!(lifecycle.finished.is_some());
    assert_eq!(orchestrator.prepare_calls.load(Ordering::Acquire), 0);
    assert_eq!(orchestrator.bind_calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn ambiguous_cleanup_fails_closed_as_recovery_required() {
    let RuntimeFixture {
        runtime,
        orchestrator,
        store,
        ..
    } = runtime_fixture(ModelScript::Writer);
    orchestrator
        .cleanup_ambiguous
        .store(true, Ordering::Release);
    let outcome = runtime
        .start(root_request(true, true))
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::RecoveryRequired { .. }),
        "unexpected root terminal: {:?}",
        outcome.terminal
    );
    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert!(replay.events.iter().any(|event| matches!(
        &event.event,
        RuntimeEventKind::AgentCleanupCommitted {
            retained_for_recovery: true,
            worktree_removed: false,
            branch_removed: false,
            reason: Some(reason),
            ..
        } if reason.contains("无法精确证明")
    )));
    assert!(
        !replay.snapshot.evidence_receipts.is_empty(),
        "cleanup runs after post-merge root verification"
    );
    assert!(
        replay.events.iter().all(|event| !matches!(
            &event.event,
            RuntimeEventKind::Terminal { outcome }
                if matches!(outcome.terminal, TerminalState::Completed { .. })
        )),
        "ambiguous cleanup must replace the proposed completed terminal"
    );
}

#[tokio::test]
async fn uncreated_writer_recovery_does_not_advance_the_root_workspace() {
    let RuntimeFixture {
        runtime,
        model,
        orchestrator,
        store,
        ..
    } = runtime_fixture(ModelScript::Writer);
    orchestrator.bind_recovery.store(true, Ordering::Release);
    let outcome = runtime
        .start(root_request(true, true))
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired { .. }
    ));
    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.terminal.as_ref(), Some(&outcome));
    assert_eq!(replay.snapshot.workspace_state, known(1, BASE_COMMIT));
    let lifecycle = replay.snapshot.agent_tasks.first().expect("Writer task");
    assert!(lifecycle.workspace_created.is_none());
    assert!(lifecycle.integration.is_none());
    assert!(lifecycle.cleanup.is_none());
    assert_eq!(
        lifecycle
            .finished
            .as_ref()
            .map(|finished| &finished.outcome.terminal),
        Some(&outcome.terminal),
    );
    assert!(replay.events.iter().any(|event| matches!(
        &event.event,
        RuntimeEventKind::ToolOutcomeCommitted {
            call_id,
            name,
            outcome,
            workspace_state: None,
            ..
        } if call_id == "writer-call"
            && name == AGENT_TOOL_NAME
            && outcome.transport == ToolTransportStatus::Indeterminate
            && outcome.operation == ToolOperationStatus::Cancelled
            && outcome.side_effect == ToolSideEffectStatus::Indeterminate
            && outcome.retry == ToolRetryDisposition::Unsafe
    )));
    assert_eq!(orchestrator.bind_calls.load(Ordering::Acquire), 1);
    assert_eq!(orchestrator.bind_side_effects.load(Ordering::Acquire), 0);
    assert_eq!(orchestrator.cleanup_calls.load(Ordering::Acquire), 0);
    let requests = model.requests.lock().expect("request log");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Root)
            .count(),
        1,
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.actor.kind == AgentActorKind::Child)
            .count(),
        0,
    );
}

#[tokio::test]
async fn writer_recovery_absorbs_terminal_child_budget_at_every_unfinished_parent_window() {
    for checkpoint in [
        RecoveryCheckpoint::ChildTerminal,
        RecoveryCheckpoint::SealCommitted,
        RecoveryCheckpoint::ResultCollected,
        RecoveryCheckpoint::IntegrationStarted,
    ] {
        let store = Arc::new(InMemoryRunStore::default());
        let (run_id, task) = seed_recovery_checkpoint(
            &store,
            checkpoint,
            3,
            ModelAccounting {
                complete: true,
                usage_complete: true,
                ..ModelAccounting::default()
            },
        )
        .await;
        let timeline = Arc::new(Mutex::new(Vec::new()));
        let root_tools = Arc::new(RootTools::new(timeline.clone()));
        let writer_tools = Arc::new(WriterTools::new(timeline));
        let child_outcome = complete_recovery_child(
            store.clone(),
            &task,
            writer_tools.clone(),
            ModelAccounting::default(),
        )
        .await;
        assert!(
            matches!(child_outcome.terminal, TerminalState::Completed { .. }),
            "seeded child must reach a real terminal replay: {child_outcome:?}"
        );
        assert_eq!(child_outcome.runtime_model_requests, 2);

        let orchestrator = Arc::new(
            FakeOrchestrator::new(root_tools.clone(), writer_tools).with_preexisting_workspace(),
        );
        let outcome = recovery_runtime(store.clone(), root_tools, orchestrator)
            .resume(run_id.clone())
            .wait()
            .await
            .unwrap();
        assert!(
            matches!(
                &outcome.terminal,
                TerminalState::RecoveryRequired { ambiguity }
                    if ambiguity.action_id
                        == "writer-recovery-child:writer_recovery_budget"
            ),
            "{checkpoint:?} must fail closed after absorbing the terminal child counters: {:?}",
            outcome.terminal
        );
        let replay = store.load(&run_id).await.unwrap().unwrap();
        assert_eq!(
            replay.snapshot.runtime_model_requests, 3,
            "the terminal root must persist its one request plus both recovered child requests"
        );
        let child_replay = store
            .load(&task.child_run_id)
            .await
            .unwrap()
            .expect("terminal child replay");
        assert_eq!(child_replay.snapshot.runtime_model_requests, 2);
    }
}

#[tokio::test]
async fn terminal_child_budget_is_absorbed_once_and_exact_limit_can_finish_recovery() {
    let store = Arc::new(InMemoryRunStore::default());
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::ChildTerminal,
        4,
        ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
    )
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let child_outcome = complete_recovery_child(
        store.clone(),
        &task,
        writer_tools.clone(),
        ModelAccounting::default(),
    )
    .await;
    assert_eq!(child_outcome.runtime_model_requests, 2);
    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools).with_preexisting_workspace(),
    );

    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator)
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "one root request + one reserved root terminal request + two recovered child requests must fit exactly: {:?}",
        outcome.terminal
    );
    assert_eq!(
        store
            .load(&run_id)
            .await
            .unwrap()
            .unwrap()
            .snapshot
            .runtime_model_requests,
        4
    );
}

#[tokio::test]
async fn resumed_root_new_writer_child_inherits_the_physical_epoch_baseline() {
    let store = Arc::new(InMemoryRunStore::default());
    let baseline = physical_accounting(1, 0, 8);
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::TaskPrepared,
        8,
        baseline.clone(),
    )
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(FakeOrchestrator::new(root_tools.clone(), writer_tools));

    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator)
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    let child = store
        .load(&task.child_run_id)
        .await
        .unwrap()
        .expect("writer child replay");
    assert_eq!(child.snapshot.request.accounting_baseline, baseline);
    assert_eq!(child.snapshot.accounting.total_started(), 3);
    assert_eq!(child.snapshot.accounting.root.started, 1);
    assert_eq!(child.snapshot.accounting.child.started, 2);
    let root = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(root.snapshot.accounting.total_started(), 4);
    assert_eq!(root.snapshot.accounting.root.started, 2);
    assert_eq!(root.snapshot.accounting.child.started, 2);
}

#[tokio::test]
async fn second_reopen_uses_child_cumulative_accounting_once_and_preserves_the_hard_limit() {
    let store = Arc::new(InMemoryRunStore::default());
    let parent_accounting = physical_accounting(1, 0, 4);
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::ChildTerminal,
        4,
        parent_accounting.clone(),
    )
    .await;
    let root_before = store.load(&run_id).await.unwrap().unwrap();
    let expected_sequence = root_before.snapshot.last_sequence;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let child_outcome = complete_recovery_child(
        store.clone(),
        &task,
        writer_tools.clone(),
        parent_accounting,
    )
    .await;
    assert!(matches!(
        child_outcome.terminal,
        TerminalState::Completed { .. }
    ));
    let child = store
        .load(&task.child_run_id)
        .await
        .unwrap()
        .expect("terminal writer child");
    assert_eq!(child.snapshot.accounting.total_started(), 3);
    assert_eq!(child.snapshot.accounting.hard_request_limit, Some(4));

    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools).with_preexisting_workspace(),
    );
    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator)
        .resume_with_accounting_baseline(
            run_id.clone(),
            expected_sequence,
            child.snapshot.accounting.clone(),
        )
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "the second reopen must retain exactly one physical request slot: {:?}",
        outcome.terminal
    );
    assert_eq!(outcome.accounting.hard_request_limit, Some(4));
    assert_eq!(outcome.accounting.total_started(), 4);
    assert_eq!(outcome.accounting.root.started, 2);
    assert_eq!(outcome.accounting.child.started, 2);
    let persisted = store
        .load(&run_id)
        .await
        .unwrap()
        .unwrap()
        .snapshot
        .accounting;
    assert_eq!(persisted.hard_request_limit, Some(4));
    assert_eq!(persisted.total_started(), 4);
    assert_eq!(persisted.root, outcome.accounting.root);
    assert_eq!(persisted.child, outcome.accounting.child);
    assert_eq!(persisted.usage, outcome.accounting.usage);
}

#[tokio::test]
async fn accounting_resume_override_rejects_a_root_that_advanced_before_acquire() {
    let store = Arc::new(InMemoryRunStore::default());
    let baseline = physical_accounting(1, 0, 8);
    let (run_id, _) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::TaskPrepared,
        8,
        baseline.clone(),
    )
    .await;
    let replay = store.load(&run_id).await.unwrap().unwrap();
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(FakeOrchestrator::new(root_tools.clone(), writer_tools));

    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator.clone())
        .resume_with_accounting_baseline(
            run_id.clone(),
            replay.snapshot.last_sequence.saturating_sub(1),
            baseline,
        )
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        &outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::Store { message }
        } if message.contains("run_resume_replay_advanced")
    ));
    assert_eq!(orchestrator.bind_calls.load(Ordering::Acquire), 0);
    assert!(
        store
            .load(&run_id)
            .await
            .unwrap()
            .unwrap()
            .snapshot
            .terminal
            .is_none(),
        "a rejected bootstrap must not invent a canonical terminal event"
    );
}

async fn assert_resume_reuses_frozen_identity(checkpoint: RecoveryCheckpoint) {
    let store = Arc::new(InMemoryRunStore::default());
    let (run_id, task) = Box::pin(seed_recovery_checkpoint(
        &store,
        checkpoint,
        32,
        ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
    ))
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools).with_preexisting_workspace(),
    );
    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator.clone())
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "unexpected recovery terminal: {:?}",
        outcome.terminal
    );
    let replay = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.agent_tasks.len(), 1);
    let recovered = &replay.snapshot.agent_tasks[0];
    assert_eq!(recovered.task.task_id, task.task_id);
    assert_eq!(recovered.task.child_run_id, task.child_run_id);
    assert_eq!(recovered.task.workspace, task.workspace);
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::AgentTaskPrepared { .. }))
            .count(),
        1
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ChildStarted { .. }))
            .count(),
        1
    );
    assert!(
        store.load(&task.child_run_id).await.unwrap().is_some(),
        "resume must create the frozen child id, not a replacement"
    );
    assert_eq!(
        orchestrator.bind_side_effects.load(Ordering::Acquire),
        1,
        "an already-created worktree must be recovered without a second create side effect"
    );
    assert_eq!(
        orchestrator.integrate_side_effects.load(Ordering::Acquire),
        1
    );
}

#[tokio::test]
async fn resume_after_task_prepared_reuses_frozen_workspace_and_child_identity() {
    Box::pin(assert_resume_reuses_frozen_identity(
        RecoveryCheckpoint::TaskPrepared,
    ))
    .await;
}

#[tokio::test]
async fn resume_after_workspace_created_reuses_frozen_child_identity() {
    Box::pin(assert_resume_reuses_frozen_identity(
        RecoveryCheckpoint::WorkspaceCreated,
    ))
    .await;
}

#[tokio::test]
async fn resume_after_integration_side_effect_commits_once_without_reapplying_it() {
    let store = Arc::new(InMemoryRunStore::default());
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::IntegrationStarted,
        32,
        ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
    )
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools)
            .with_preexisting_workspace()
            .with_integrated_root(),
    );
    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator.clone())
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "unexpected recovery terminal: {:?}",
        outcome.terminal
    );
    let replay = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.agent_tasks.len(), 1);
    assert_eq!(replay.snapshot.agent_tasks[0].task.task_id, task.task_id);
    assert_eq!(orchestrator.integrate_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        orchestrator.integrate_side_effects.load(Ordering::Acquire),
        1,
        "the pre-crash integration side effect must not be applied twice"
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(
                event.event,
                RuntimeEventKind::AgentIntegrationCommitted { .. }
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_after_child_finished_rebuilds_exact_tool_outcome_without_live_writer_work() {
    let store = Arc::new(InMemoryRunStore::default());
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::ChildFinished,
        32,
        ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
    )
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools)
            .with_preexisting_workspace()
            .with_integrated_root(),
    );

    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator.clone())
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "a durable completed ChildFinished must continue from the rebuilt tool result: {:?}",
        outcome.terminal
    );
    let replay = store.load(&run_id).await.unwrap().unwrap();
    let tool_commits = replay
        .events
        .iter()
        .filter_map(|event| match &event.event {
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id,
                name,
                outcome,
                workspace_state,
            } if call_id == &task.call_id && name == AGENT_TOOL_NAME => {
                Some((operation_id, outcome, workspace_state))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_commits.len(), 1);
    let (operation_id, tool_outcome, workspace_state) = tool_commits[0];
    assert_eq!(operation_id, &OperationId::from("recovery-agent-operation"));
    assert_eq!(tool_outcome.invocation, ToolInvocationStatus::Accepted);
    assert_eq!(tool_outcome.side_effect, ToolSideEffectStatus::Applied);
    assert_eq!(
        tool_outcome.evidence.references,
        vec!["child-receipt".to_owned()]
    );
    assert_eq!(
        tool_outcome.workspace_revision.as_deref(),
        Some(FINAL_COMMIT)
    );
    assert_eq!(workspace_state, &Some(known(2, FINAL_COMMIT)));
    assert_eq!(
        tool_outcome
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("writer_commit"))
            .and_then(serde_json::Value::as_str),
        Some(FINAL_COMMIT)
    );
    assert_eq!(orchestrator.bind_calls.load(Ordering::Acquire), 0);
    assert_eq!(orchestrator.seal_calls.load(Ordering::Acquire), 0);
    assert_eq!(orchestrator.integrate_calls.load(Ordering::Acquire), 0);
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ChildFinished { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_after_cleanup_side_effect_proves_exact_absence_without_second_removal() {
    let store = Arc::new(InMemoryRunStore::default());
    let (run_id, task) = seed_recovery_checkpoint(
        &store,
        RecoveryCheckpoint::CleanupPrepared,
        32,
        ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        },
    )
    .await;
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let root_tools = Arc::new(RootTools::new(timeline.clone()));
    let writer_tools = Arc::new(WriterTools::new(timeline));
    let orchestrator = Arc::new(
        FakeOrchestrator::new(root_tools.clone(), writer_tools)
            .with_preexisting_workspace()
            .with_integrated_root()
            .with_cleaned_workspace(),
    );
    let outcome = recovery_runtime(store.clone(), root_tools, orchestrator.clone())
        .resume(run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(
        matches!(outcome.terminal, TerminalState::Completed { .. }),
        "unexpected recovery terminal: {:?}",
        outcome.terminal
    );
    let replay = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.agent_tasks.len(), 1);
    assert_eq!(replay.snapshot.agent_tasks[0].task.task_id, task.task_id);
    assert_eq!(orchestrator.cleanup_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        orchestrator.cleanup_side_effects.load(Ordering::Acquire),
        1,
        "the pre-crash cleanup side effect must not remove an unrelated path or branch"
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::AgentCleanupCommitted { .. }))
            .count(),
        1
    );
}
