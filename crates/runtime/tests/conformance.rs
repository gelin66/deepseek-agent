use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use codewhale_context::compaction::{ContextCompactionPreparation, prepare_compaction};
use codewhale_runtime::*;
use serde_json::json;
use tokio::sync::Notify;

type Script = dyn Fn(ModelRequest) -> ScriptResponse + Send + Sync;

struct MockModel {
    script: Arc<Script>,
    ledger: Arc<Ledger>,
    force_incomplete: AtomicBool,
    force_exhausted: AtomicBool,
    seals: Mutex<Vec<bool>>,
}

impl MockModel {
    fn new(script: impl Fn(ModelRequest) -> ScriptResponse + Send + Sync + 'static) -> Self {
        Self {
            script: Arc::new(script),
            ledger: Arc::new(Ledger::default()),
            force_incomplete: AtomicBool::new(false),
            force_exhausted: AtomicBool::new(false),
            seals: Mutex::new(Vec::new()),
        }
    }

    fn incomplete(script: impl Fn(ModelRequest) -> ScriptResponse + Send + Sync + 'static) -> Self {
        Self {
            script: Arc::new(script),
            ledger: Arc::new(Ledger::default()),
            force_incomplete: AtomicBool::new(true),
            force_exhausted: AtomicBool::new(false),
            seals: Mutex::new(Vec::new()),
        }
    }

    fn exhausted(script: impl Fn(ModelRequest) -> ScriptResponse + Send + Sync + 'static) -> Self {
        Self {
            script: Arc::new(script),
            ledger: Arc::new(Ledger::default()),
            force_incomplete: AtomicBool::new(false),
            force_exhausted: AtomicBool::new(true),
            seals: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelPort for MockModel {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        let actor = request.actor.kind;
        self.ledger.begin(actor);
        match (self.script)(request) {
            ScriptResponse::OpenError(error) => {
                self.ledger.finish(actor, None);
                Err(error)
            }
            ScriptResponse::Events(events) => Ok(Box::new(MockStream {
                events: events.into(),
                actor,
                ledger: self.ledger.clone(),
                settled: false,
            })),
        }
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        self.seals.lock().expect("seal lock").push(seal);
        if seal {
            self.ledger.sealed.store(true, Ordering::Release);
        }
        let usage = *self.ledger.usage.lock().expect("usage lock");
        let responses = self.ledger.responses.load(Ordering::Acquire);
        let usage_responses = self.ledger.usage_responses.load(Ordering::Acquire);
        let mut accounting = ModelAccounting {
            hard_request_limit: Some(128),
            root: self.ledger.root.snapshot(),
            child: self.ledger.child.snapshot(),
            transport_retries: 0,
            runtime_retries: 0,
            sealed_denied: 0,
            exhausted_denied: 0,
            budget_exhausted: false,
            sealed: seal,
            complete: true,
            usage_complete: true,
            usage_missing: false,
            usage_incomplete: false,
            billing_unknown: false,
            unpriced: false,
            usage_responses,
            usage_missing_responses: 0,
            incomplete_responses: 0,
            billing_unknown_attempts: 0,
            unpriced_usage_responses: 0,
            records_after_seal: 0,
            usage,
            surface_usage: vec![SurfaceUsage {
                surface: ApiSurface::StandardChat,
                model: "deepseek-v4-flash".into(),
                response_count: responses,
                usage_response_count: usage_responses,
                usage,
                cost_nanousd: 0,
                cost_nanocny: 0,
            }],
            cost_nanousd: 0,
            cost_nanocny: 0,
        };
        if self.force_incomplete.load(Ordering::Acquire) {
            accounting.complete = false;
            accounting.usage_complete = false;
            accounting.usage_incomplete = true;
        }
        if self.force_exhausted.load(Ordering::Acquire) {
            accounting.budget_exhausted = true;
            accounting.exhausted_denied = 1;
        }
        Ok(accounting)
    }
}

enum ScriptResponse {
    Events(Vec<StreamStep>),
    OpenError(ModelPortError),
}

struct StreamStep {
    delay: Duration,
    event: Result<ModelStreamEvent, ModelPortError>,
}

impl StreamStep {
    fn now(event: ModelStreamEvent) -> Self {
        Self {
            delay: Duration::ZERO,
            event: Ok(event),
        }
    }

    fn delayed(delay: Duration, event: ModelStreamEvent) -> Self {
        Self {
            delay,
            event: Ok(event),
        }
    }

    fn error(error: ModelPortError) -> Self {
        Self {
            delay: Duration::ZERO,
            event: Err(error),
        }
    }
}

struct MockStream {
    events: VecDeque<StreamStep>,
    actor: AgentActorKind,
    ledger: Arc<Ledger>,
    settled: bool,
}

#[async_trait]
impl ModelStream for MockStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        let step = self.events.pop_front()?;
        if !step.delay.is_zero() {
            tokio::time::sleep(step.delay).await;
        }
        match &step.event {
            Ok(ModelStreamEvent::Completed { output }) => {
                self.ledger.finish(self.actor, Some(output.usage));
                self.settled = true;
            }
            Err(_) => {
                self.ledger.finish(self.actor, None);
                self.settled = true;
            }
            _ => {}
        }
        Some(step.event)
    }
}

impl Drop for MockStream {
    fn drop(&mut self) {
        if !self.settled {
            self.ledger.finish(self.actor, None);
            self.settled = true;
        }
    }
}

#[derive(Default)]
struct Ledger {
    root: ActorLedger,
    child: ActorLedger,
    responses: AtomicU64,
    usage_responses: AtomicU64,
    usage: Mutex<Usage>,
    sealed: AtomicBool,
}

impl Ledger {
    fn actor(&self, actor: AgentActorKind) -> &ActorLedger {
        match actor {
            AgentActorKind::Root => &self.root,
            AgentActorKind::Child => &self.child,
        }
    }

    fn begin(&self, actor: AgentActorKind) {
        let actor = self.actor(actor);
        actor.started.fetch_add(1, Ordering::AcqRel);
        actor.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    fn finish(&self, actor: AgentActorKind, usage: Option<Usage>) {
        let actor = self.actor(actor);
        actor.completed.fetch_add(1, Ordering::AcqRel);
        actor.in_flight.fetch_sub(1, Ordering::AcqRel);
        if let Some(usage) = usage {
            self.responses.fetch_add(1, Ordering::AcqRel);
            self.usage_responses.fetch_add(1, Ordering::AcqRel);
            self.usage.lock().expect("usage lock").add_assign(usage);
        }
    }
}

#[derive(Default)]
struct ActorLedger {
    started: AtomicU64,
    completed: AtomicU64,
    in_flight: AtomicU64,
}

impl ActorLedger {
    fn snapshot(&self) -> ActorRequestAccounting {
        ActorRequestAccounting {
            started: self.started.load(Ordering::Acquire),
            completed: self.completed.load(Ordering::Acquire),
            in_flight: self.in_flight.load(Ordering::Acquire),
            retries: 0,
        }
    }
}

#[derive(Default)]
struct CollectSink {
    events: Mutex<Vec<StoredRuntimeEvent>>,
    changed: Notify,
}

impl CollectSink {
    fn events(&self) -> Vec<StoredRuntimeEvent> {
        self.events.lock().expect("event lock").clone()
    }

    async fn wait_for(&self, predicate: impl Fn(&StoredRuntimeEvent) -> bool) {
        loop {
            let notified = self.changed.notified();
            if self
                .events
                .lock()
                .expect("event lock")
                .iter()
                .any(&predicate)
            {
                return;
            }
            notified.await;
        }
    }
}

#[async_trait]
impl RuntimeEventSink for CollectSink {
    async fn emit(&self, event: StoredRuntimeEvent) {
        self.events.lock().expect("event lock").push(event);
        self.changed.notify_waiters();
    }
}

#[derive(Default)]
struct MockTools {
    calls: Mutex<Vec<ToolInvocation>>,
    slow_cancelled: AtomicBool,
}

#[async_trait]
impl ToolExecutor for MockTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            definition("delay"),
            definition("approval"),
            definition("read"),
            definition("run_tests"),
            definition("run_verifiers"),
            definition("slow"),
            definition("write"),
        ]
    }

    fn approval_prompt(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<Option<ToolApprovalPrompt>, ToolExecutionError> {
        Ok((invocation.name == "approval").then(|| ToolApprovalPrompt {
            title: "确认测试工具".to_owned(),
            description: "测试工具必须在显式批准后执行。".to_owned(),
            risk: ApprovalRisk::Elevated,
        }))
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        self.calls
            .lock()
            .expect("tool call lock")
            .push(invocation.clone());
        if invocation.name == "slow" {
            cancellation.cancelled().await;
            self.slow_cancelled.store(true, Ordering::Release);
            return Err(ToolExecutionError::new("cancelled", "cancelled"));
        }
        if invocation.name == "delay" {
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
        if !self
            .definitions()
            .iter()
            .any(|definition| definition.name == invocation.name)
        {
            return Err(ToolExecutionError::new("not_found", "tool not found"));
        }
        Ok(ToolOutcome::success("tool-result"))
    }
}

fn definition(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: name.into(),
        input_schema: json!({"type": "object"}),
    }
}

fn usage() -> Usage {
    Usage {
        input_tokens: 10,
        output_tokens: 2,
        cache_hit_tokens: 3,
        cache_miss_tokens: 7,
        cache_write_tokens: 0,
        reasoning_tokens: 1,
        reasoning_replay_tokens: 0,
    }
}

fn completed(
    content: impl Into<String>,
    reasoning: Option<&str>,
    tool_calls: Vec<ModelToolCall>,
    finish_reason: ModelFinishReason,
) -> StreamStep {
    StreamStep::now(ModelStreamEvent::Completed {
        output: model_output(content, reasoning, tool_calls, finish_reason),
    })
}

fn model_output(
    content: impl Into<String>,
    reasoning: Option<&str>,
    tool_calls: Vec<ModelToolCall>,
    finish_reason: ModelFinishReason,
) -> ModelOutput {
    ModelOutput {
        content: content.into(),
        reasoning_content: reasoning.map(str::to_owned),
        tool_calls,
        finish_reason,
        usage: usage(),
    }
}

fn call(id: &str, name: &str, raw: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.into(),
        name: name.into(),
        arguments: ToolArguments::parse(raw),
    }
}

fn fixture(
    model: Arc<MockModel>,
) -> (
    Arc<AgentRuntime>,
    Arc<MockTools>,
    Arc<CollectSink>,
    Arc<InMemoryRunStore>,
) {
    let tools = Arc::new(MockTools::default());
    let sink = Arc::new(CollectSink::default());
    let store = Arc::new(InMemoryRunStore::default());
    let runtime = Arc::new(AgentRuntime::new(
        model,
        tools.clone(),
        sink.clone(),
        store.clone(),
    ));
    (runtime, tools, sink, store)
}

fn request(input: &str) -> RunRequest {
    let mut request = RunRequest::new(input, "系统提示");
    request.limits.max_model_requests = 128;
    request.limits.max_turns = 16;
    request
}

fn persisted_model_request(created: &CreatedRun, request_number: u32) -> ModelRequest {
    let snapshot = &created.replay.snapshot;
    ModelRequest {
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
        request_number,
        attempt: 0,
    }
}

async fn append_event(
    store: &InMemoryRunStore,
    lease: &RunLease,
    event_id: &str,
    event: RuntimeEventKind,
) -> StoredRuntimeEvent {
    store
        .append(
            lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(event_id.to_owned()),
                event,
            },
        )
        .await
        .unwrap()
}

async fn seed_committed_model_output(
    store: &InMemoryRunStore,
    created: &CreatedRun,
    output: ModelOutput,
) -> AttemptId {
    let attempt_id = AttemptId("attempt-1".into());
    append_event(
        store,
        &created.lease,
        "model-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(persisted_model_request(created, 1)),
        },
    )
    .await;
    append_event(
        store,
        &created.lease,
        "model-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: attempt_id.clone(),
        },
    )
    .await;
    append_event(
        store,
        &created.lease,
        "model-committed",
        RuntimeEventKind::ModelResponseCommitted {
            attempt_id: attempt_id.clone(),
            output: Box::new(output),
            accounting: Box::new(ModelAccounting::default()),
        },
    )
    .await;
    attempt_id
}

async fn seed_pending_approval(
    store: &InMemoryRunStore,
    run_id: &str,
    resolved: bool,
) -> UserInteractionRequest {
    let mut run_request = request("resume approval");
    run_request.run_id = Some(RunId::from(run_id));
    run_request.environment.interactive = true;
    let created = store.create(run_request).await.unwrap();
    let tool_call = call("approval-call", "approval", r#"{"path":"src/lib.rs"}"#);
    seed_committed_model_output(
        store,
        &created,
        model_output(
            "",
            None,
            vec![tool_call.clone()],
            ModelFinishReason::ToolCalls,
        ),
    )
    .await;
    let operation_id = OperationId::from("approval-operation".to_owned());
    append_event(
        store,
        &created.lease,
        "approval-tool-prepared",
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: RunId::from(run_id),
                call_id: tool_call.id,
                name: tool_call.name,
                arguments: tool_call.arguments,
            },
        },
    )
    .await;
    let interaction = UserInteractionRequest {
        interaction_id: InteractionId::from("approval-interaction".to_owned()),
        operation_id,
        call_id: "approval-call".to_owned(),
        tool_name: "approval".to_owned(),
        prompt: UserInteractionPrompt::Approval {
            prompt: ToolApprovalPrompt {
                title: "确认测试工具".to_owned(),
                description: "测试工具必须在显式批准后执行。".to_owned(),
                risk: ApprovalRisk::Elevated,
            },
            arguments: json!({"path": "src/lib.rs"}),
        },
    };
    append_event(
        store,
        &created.lease,
        "approval-interaction-requested",
        RuntimeEventKind::InteractionRequested {
            request: interaction.clone(),
        },
    )
    .await;
    if resolved {
        append_event(
            store,
            &created.lease,
            "approval-interaction-resolved",
            RuntimeEventKind::InteractionResolved {
                command_id: CommandId::from("approval-command"),
                interaction_id: interaction.interaction_id.clone(),
                response: UserInteractionResponse::Approved,
            },
        )
        .await;
    }
    store.release(&created.lease).await.unwrap();
    interaction
}

#[tokio::test]
async fn final_is_store_first_and_terminal_is_exactly_once() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "完成",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, sink, store) = fixture(model);
    let outcome = runtime.start(request("任务")).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));

    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    let stored = replay.events;
    let observed = sink
        .events()
        .into_iter()
        .filter(|event| event.run_id == outcome.run_id)
        .collect::<Vec<_>>();
    assert_eq!(
        observed, stored,
        "sink can only see events returned by store"
    );
    assert_eq!(
        stored
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
    assert!(stored.iter().any(|event| matches!(
        &event.event,
        RuntimeEventKind::ContentDelta { delta, .. } if delta == "完成"
    )));
    assert!(
        stored
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1)
    );
    let terminal = stored
        .iter()
        .find(|event| event.event.is_terminal())
        .expect("terminal event")
        .clone();
    let stale_lease = RunLease {
        run_id: outcome.run_id.clone(),
        epoch: u64::MAX,
        owner_id: "stale-test-owner".into(),
        owner_pid: 0,
    };
    let duplicate = store
        .append(
            &stale_lease,
            PendingRuntimeEvent {
                event_id: terminal.event_id.clone(),
                event: terminal.event.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(duplicate, terminal, "terminal retry is idempotent");
    let error = store
        .append(
            &stale_lease,
            PendingRuntimeEvent::new(RuntimeEventKind::SteerQueued {
                command_id: CommandId::from("late-steer"),
                content: "late".into(),
            }),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RunStoreError::AlreadyTerminal { .. }));
}

#[tokio::test]
async fn ready_returns_only_after_run_created_is_durable() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "完成",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, _, store) = fixture(model);

    let run = runtime.start(request("握手")).ready().await.unwrap();
    let replay = store.load(&run.run_id).await.unwrap().unwrap();
    assert!(matches!(
        replay.events.first().map(|event| &event.event),
        Some(RuntimeEventKind::RunCreated { .. })
    ));

    run.wait().await.unwrap();
}

#[tokio::test]
async fn ready_preserves_typed_store_start_failure() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "不应调用",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, _, store) = fixture(model);
    let mut duplicate = request("重复");
    duplicate.run_id = Some(RunId::from("duplicate-run"));
    let created = store.create(duplicate.clone()).await.unwrap();

    let error = match runtime.start(duplicate).ready().await {
        Ok(_) => panic!("duplicate run unexpectedly became ready"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        RunReadyError::Store(RunStoreError::AlreadyExists { ref run_id })
            if run_id == &RunId::from("duplicate-run")
    ));

    store.release(&created.lease).await.unwrap();
}

#[tokio::test]
async fn tool_reasoning_and_raw_arguments_replay_exactly() {
    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        if script_calls.fetch_add(1, Ordering::AcqRel) == 0 {
            return ScriptResponse::Events(vec![completed(
                "",
                Some("原始推理"),
                vec![call("call-1", "read", "{ \"path\" : \"src/lib.rs\" }")],
                ModelFinishReason::ToolCalls,
            )]);
        }
        assert!(request.messages.iter().any(|message| matches!(
            message,
            ModelMessage::Assistant {
                reasoning_content: Some(reasoning),
                tool_calls,
                ..
            } if reasoning == "原始推理"
                && tool_calls[0].arguments.raw == "{ \"path\" : \"src/lib.rs\" }"
        )));
        assert!(request.messages.iter().any(
            |message| matches!(message, ModelMessage::Tool { call_id, .. } if call_id == "call-1")
        ));
        ScriptResponse::Events(vec![completed(
            "已读取",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, tools, _, _) = fixture(model);
    let outcome = runtime.start(request("读取")).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    let calls = tools.calls.lock().unwrap();
    assert_eq!(calls[0].arguments.raw, "{ \"path\" : \"src/lib.rs\" }");
    assert!(calls[0].arguments.parsed.is_some());
}

#[tokio::test]
async fn approval_is_durable_and_precedes_every_tool_side_effect() {
    let requests = Arc::new(AtomicUsize::new(0));
    let script_requests = requests.clone();
    let model = Arc::new(MockModel::new(move |request| {
        if script_requests.fetch_add(1, Ordering::AcqRel) == 0 {
            return ScriptResponse::Events(vec![completed(
                "",
                None,
                vec![call(
                    "approval-call",
                    "approval",
                    r#"{"path":"src/lib.rs"}"#,
                )],
                ModelFinishReason::ToolCalls,
            )]);
        }
        assert!(request.messages.iter().any(|message| {
            matches!(message, ModelMessage::Tool { call_id, content, .. }
                if call_id == "approval-call" && content == "tool-result")
        }));
        ScriptResponse::Events(vec![completed(
            "approved",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, tools, sink, store) = fixture(model);
    let mut run_request = request("approval");
    run_request.environment.interactive = true;
    let run = runtime.start(run_request);
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::InteractionRequested { .. }))
        .await;
    assert!(tools.calls.lock().unwrap().is_empty());
    let interaction = sink
        .events()
        .into_iter()
        .find_map(|event| match event.event {
            RuntimeEventKind::InteractionRequested { request } => Some(request),
            _ => None,
        })
        .expect("approval request");
    assert!(matches!(
        interaction.prompt,
        UserInteractionPrompt::Approval { .. }
    ));
    let accepted_sequence = control
        .resolve_interaction(
            CommandId::from("approve-command"),
            interaction.interaction_id,
            UserInteractionResponse::Approved,
        )
        .await
        .expect("approval accepted durably");
    let outcome = run.wait().await.unwrap();
    assert_eq!(
        outcome.terminal,
        TerminalState::Completed {
            message: "approved".to_owned()
        }
    );
    assert_eq!(tools.calls.lock().unwrap().len(), 1);
    let replay = store.load(&run_id).await.unwrap().unwrap();
    let resolved = replay
        .events
        .iter()
        .position(|event| matches!(event.event, RuntimeEventKind::InteractionResolved { .. }))
        .expect("interaction resolution");
    let started = replay
        .events
        .iter()
        .position(|event| matches!(event.event, RuntimeEventKind::ToolExecutionStarted { .. }))
        .expect("tool execution start");
    assert_eq!(replay.events[resolved].sequence, accepted_sequence);
    assert!(resolved < started);
}

#[tokio::test]
async fn pending_approval_replays_without_reasking_and_resumes_each_safe_window_once() {
    for resolved_before_resume in [false, true] {
        let store = Arc::new(InMemoryRunStore::default());
        let run_id = if resolved_before_resume {
            "approval-resolved-before-start"
        } else {
            "approval-waiting"
        };
        let interaction = seed_pending_approval(&store, run_id, resolved_before_resume).await;
        let model = Arc::new(MockModel::new(|request| {
            assert!(request.messages.iter().any(|message| {
                matches!(message, ModelMessage::Tool { call_id, content, .. }
                    if call_id == "approval-call" && content == "tool-result")
            }));
            ScriptResponse::Events(vec![completed(
                "resumed",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )])
        }));
        let tools = Arc::new(MockTools::default());
        let sink = Arc::new(CollectSink::default());
        let runtime = Arc::new(AgentRuntime::new(
            model,
            tools.clone(),
            sink.clone(),
            store.clone(),
        ));
        let run = runtime.resume(RunId::from(run_id));
        let control = run.control();
        sink.wait_for(|event| matches!(event.event, RuntimeEventKind::InteractionRequested { .. }))
            .await;
        if !resolved_before_resume {
            assert!(tools.calls.lock().unwrap().is_empty());
            control
                .resolve_interaction(
                    CommandId::from("approval-command"),
                    interaction.interaction_id,
                    UserInteractionResponse::Approved,
                )
                .await
                .expect("resume approval accepted");
        }
        assert_eq!(
            run.wait().await.unwrap().terminal,
            TerminalState::Completed {
                message: "resumed".to_owned()
            }
        );
        assert_eq!(tools.calls.lock().unwrap().len(), 1);
        let replay = store.load(&RunId::from(run_id)).await.unwrap().unwrap();
        assert_eq!(
            replay
                .events
                .iter()
                .filter(|event| {
                    matches!(event.event, RuntimeEventKind::InteractionRequested { .. })
                })
                .count(),
            1,
            "resume must replay the same pending interaction instead of requesting again"
        );
        assert_eq!(
            replay
                .events
                .iter()
                .filter(|event| {
                    matches!(event.event, RuntimeEventKind::InteractionResolved { .. })
                })
                .count(),
            1
        );
        assert_eq!(
            replay
                .events
                .iter()
                .filter(|event| {
                    matches!(event.event, RuntimeEventKind::ToolExecutionStarted { .. })
                })
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn request_user_input_submit_and_cancel_are_canonical_tool_outcomes() {
    for cancel in [false, true] {
        let requests = Arc::new(AtomicUsize::new(0));
        let script_requests = requests.clone();
        let model = Arc::new(MockModel::new(move |request| {
            if script_requests.fetch_add(1, Ordering::AcqRel) == 0 {
                return ScriptResponse::Events(vec![completed(
                    "",
                    None,
                    vec![call(
                        "question-call",
                        REQUEST_USER_INPUT_TOOL_NAME,
                        r#"{"questions":[{"header":"范围","id":"scope","question":"选择范围","options":[{"label":"A","description":"选项 A"},{"label":"B","description":"选项 B"}],"allow_free_text":true,"multi_select":false}]}"#,
                    )],
                    ModelFinishReason::ToolCalls,
                )]);
            }
            let content = request
                .messages
                .iter()
                .find_map(|message| match message {
                    ModelMessage::Tool {
                        call_id, content, ..
                    } if call_id == "question-call" => Some(content.clone()),
                    _ => None,
                })
                .expect("user input tool outcome");
            if cancel {
                assert!(content.contains("user_input_cancelled"));
            } else {
                assert!(content.contains("自定义范围"));
            }
            ScriptResponse::Events(vec![completed(
                "continued",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )])
        }));
        let (runtime, tools, sink, _) = fixture(model);
        let mut run_request = request("ask");
        run_request.environment.interactive = true;
        let run = runtime.start(run_request);
        let control = run.control();
        sink.wait_for(|event| matches!(event.event, RuntimeEventKind::InteractionRequested { .. }))
            .await;
        let interaction = sink
            .events()
            .into_iter()
            .find_map(|event| match event.event {
                RuntimeEventKind::InteractionRequested { request } => Some(request),
                _ => None,
            })
            .expect("user input request");
        assert!(matches!(
            interaction.prompt,
            UserInteractionPrompt::UserInput { .. }
        ));
        let response = if cancel {
            UserInteractionResponse::Cancelled
        } else {
            UserInteractionResponse::Answered {
                answers: vec![UserInputAnswer {
                    id: "scope".to_owned(),
                    label: "自定义".to_owned(),
                    value: "自定义范围".to_owned(),
                }],
            }
        };
        control
            .resolve_interaction(
                CommandId::from(if cancel {
                    "cancel-input-command"
                } else {
                    "answer-input-command"
                }),
                interaction.interaction_id,
                response,
            )
            .await
            .expect("user input resolution accepted");
        assert!(matches!(
            run.wait().await.unwrap().terminal,
            TerminalState::Completed { .. }
        ));
        assert!(
            tools.calls.lock().unwrap().is_empty(),
            "request_user_input is runtime-owned and never reaches ToolExecutor"
        );
    }
}

#[tokio::test]
async fn malformed_and_unknown_tools_return_ordered_results_without_losing_raw() {
    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        if script_calls.fetch_add(1, Ordering::AcqRel) == 0 {
            return ScriptResponse::Events(vec![completed(
                "",
                None,
                vec![
                    call("bad-json", "read", "{not-json"),
                    call("unknown", "does_not_exist", "{}"),
                ],
                ModelFinishReason::ToolCalls,
            )]);
        }
        let results = request
            .messages
            .iter()
            .filter_map(|message| match message {
                ModelMessage::Tool {
                    call_id, content, ..
                } => Some((call_id.as_str(), content.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "bad-json");
        assert!(results[0].1.contains("{not-json"));
        assert_eq!(results[1].0, "unknown");
        assert!(results[1].1.contains("not_found"));
        ScriptResponse::Events(vec![completed(
            "recovered",
            None,
            vec![],
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, tools, _, store) = fixture(model);
    let outcome = runtime.start(request("bad tools")).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    {
        let executed = tools.calls.lock().unwrap();
        assert_eq!(executed.len(), 1, "malformed JSON never reaches executor");
        assert_eq!(executed[0].name, "does_not_exist");
    }
    let transcript = store
        .load(&outcome.run_id)
        .await
        .unwrap()
        .unwrap()
        .snapshot
        .transcript;
    assert!(transcript.entries.iter().any(|entry| matches!(
        entry,
        TranscriptEntry::Assistant { tool_calls, .. }
            if tool_calls[0].arguments.raw == "{not-json"
    )));
}

#[tokio::test]
async fn steer_during_tools_is_appended_only_after_every_tool_result() {
    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        if script_calls.fetch_add(1, Ordering::AcqRel) == 0 {
            return ScriptResponse::Events(vec![completed(
                "",
                Some("plan"),
                vec![call("delay", "delay", "{}"), call("read", "read", "{}")],
                ModelFinishReason::ToolCalls,
            )]);
        }
        let steer = request
            .messages
            .iter()
            .position(|message| matches!(message, ModelMessage::User { content } if content == "steer-after-tools"))
            .expect("steer message");
        let last_tool = request
            .messages
            .iter()
            .rposition(|message| matches!(message, ModelMessage::Tool { .. }))
            .expect("tool result");
        assert!(
            last_tool < steer,
            "steer must not split assistant/tool replay"
        );
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|message| matches!(message, ModelMessage::Tool { .. }))
                .count(),
            2
        );
        ScriptResponse::Events(vec![completed(
            "done",
            None,
            vec![],
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, sink, store) = fixture(model);
    let run = runtime.start(request("tool steer"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(&event.event, RuntimeEventKind::ToolExecutionStarted { .. }))
        .await;
    control.steer("steer-after-tools").unwrap();
    assert!(matches!(
        run.wait().await.unwrap().terminal,
        TerminalState::Completed { .. }
    ));
    let entries = store
        .load(&run_id)
        .await
        .unwrap()
        .unwrap()
        .snapshot
        .transcript
        .entries;
    let steer = entries
        .iter()
        .position(|entry| matches!(entry, TranscriptEntry::User { content } if content == "steer-after-tools"))
        .unwrap();
    let last_tool = entries
        .iter()
        .rposition(|entry| matches!(entry, TranscriptEntry::Tool { .. }))
        .unwrap();
    assert!(last_tool < steer);
}

#[tokio::test]
async fn invalid_tool_call_identity_fails_before_any_tool_event() {
    for calls in [
        vec![call("", "read", "{}")],
        vec![call("same", "read", "{}"), call("same", "read", "{}")],
        vec![call("named", "", "{}")],
    ] {
        let model = Arc::new(MockModel::new(move |_| {
            ScriptResponse::Events(vec![completed(
                "",
                None,
                calls.clone(),
                ModelFinishReason::ToolCalls,
            )])
        }));
        let (runtime, tools, sink, _) = fixture(model);
        let outcome = runtime.start(request("invalid call")).wait().await.unwrap();
        assert!(matches!(
            outcome.terminal,
            TerminalState::Failed {
                failure: RuntimeFailure::InvalidModelOutput { .. }
            }
        ));
        assert!(tools.calls.lock().unwrap().is_empty());
        assert!(
            !sink
                .events()
                .iter()
                .any(|event| matches!(event.event, RuntimeEventKind::ToolPrepared { .. }))
        );
    }
}

#[tokio::test]
async fn in_flight_steer_queues_safely_while_cancel_and_interrupt_remain_typed_terminals() {
    let model = Arc::new(MockModel::new(|request| {
        if request.messages.iter().any(
            |message| matches!(message, ModelMessage::User { content } if content == "改做新任务"),
        ) {
            ScriptResponse::Events(vec![completed(
                "新结果",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )])
        } else {
            ScriptResponse::Events(vec![StreamStep::delayed(
                Duration::from_millis(25),
                ModelStreamEvent::Completed {
                    output: ModelOutput {
                        content: "旧结果".into(),
                        reasoning_content: None,
                        tool_calls: Vec::new(),
                        finish_reason: ModelFinishReason::Stop,
                        usage: usage(),
                    },
                },
            )])
        }
    }));
    let (runtime, _, sink, _) = fixture(model);
    let run = runtime.start(request("旧任务"));
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
        .await;
    control.steer("改做新任务").unwrap();
    let outcome = run.wait().await.unwrap();
    assert_eq!(
        outcome.terminal,
        TerminalState::Completed {
            message: "新结果".to_owned()
        }
    );
    let events = sink.events();
    let queued = events
        .iter()
        .position(|event| matches!(event.event, RuntimeEventKind::SteerQueued { .. }))
        .expect("steer queued durably");
    let old_response = events
        .iter()
        .position(|event| {
            matches!(
                &event.event,
                RuntimeEventKind::ModelResponseCommitted { output, .. }
                    if output.content == "旧结果"
            )
        })
        .expect("old response committed atomically");
    let applied = events
        .iter()
        .position(|event| matches!(event.event, RuntimeEventKind::SteerApplied { .. }))
        .expect("steer applied at a safe point");
    assert!(queued < old_response && old_response < applied);

    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![StreamStep::delayed(
            Duration::from_secs(5),
            ModelStreamEvent::Completed {
                output: ModelOutput {
                    content: "too late".into(),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: usage(),
                },
            },
        )])
    }));
    let (runtime, _, sink, _) = fixture(model);
    let run = runtime.start(request("取消"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
        .await;
    control.cancel().unwrap();
    assert_eq!(run.wait().await.unwrap().terminal, TerminalState::Cancelled);
    let cancelled_events = sink
        .events()
        .into_iter()
        .filter(|event| event.run_id == run_id)
        .collect::<Vec<_>>();
    assert_eq!(
        cancelled_events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
    assert!(cancelled_events.last().unwrap().event.is_terminal());

    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![StreamStep::delayed(
            Duration::from_secs(5),
            ModelStreamEvent::Completed {
                output: ModelOutput {
                    content: "too late".into(),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: usage(),
                },
            },
        )])
    }));
    let (runtime, _, sink, _) = fixture(model);
    let run = runtime.start(request("中断"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
        .await;
    control.interrupt().unwrap();
    assert_eq!(
        run.wait().await.unwrap().terminal,
        TerminalState::Interrupted
    );
    let interrupted_events = sink
        .events()
        .into_iter()
        .filter(|event| event.run_id == run_id)
        .collect::<Vec<_>>();
    assert_eq!(
        interrupted_events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
    assert!(interrupted_events.last().unwrap().event.is_terminal());
}

#[tokio::test]
async fn concurrent_same_command_id_accepts_only_one_payload() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![StreamStep::delayed(
            Duration::from_secs(5),
            ModelStreamEvent::Completed {
                output: model_output("too late", None, Vec::new(), ModelFinishReason::Stop),
            },
        )])
    }));
    let (runtime, _, sink, store) = fixture(model);
    let run = runtime.start(request("并发 command id"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
        .await;

    let command_id = CommandId::from("same-command");
    let (first, second) = tokio::join!(
        control.steer_durable(command_id.clone(), "内容 A"),
        control.steer_durable(command_id, "内容 B")
    );
    assert!(matches!(
        (&first, &second),
        (Ok(_), Err(ControlError::CommandPayloadMismatch))
            | (Err(ControlError::CommandPayloadMismatch), Ok(_))
    ));
    let replay = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(
                &event.event,
                RuntimeEventKind::SteerQueued { command_id, .. }
                    if command_id == &CommandId::from("same-command")
            ))
            .count(),
        1
    );

    control.cancel().unwrap();
    assert_eq!(run.wait().await.unwrap().terminal, TerminalState::Cancelled);
}

#[tokio::test]
async fn resume_after_steer_applied_never_reuses_the_superseded_stop_response() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("旧任务")).await.unwrap();
    seed_committed_model_output(
        &store,
        &created,
        model_output("旧结果", None, Vec::new(), ModelFinishReason::Stop),
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "steer-queued-before-crash",
        RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("steer-before-crash"),
            content: "改做新任务".to_owned(),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "steer-applied-before-crash",
        RuntimeEventKind::SteerApplied {
            command_id: CommandId::from("steer-before-crash"),
            content: "改做新任务".to_owned(),
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|request| {
        assert!(matches!(
            request.messages.last(),
            Some(ModelMessage::User { content }) if content == "改做新任务"
        ));
        ScriptResponse::Events(vec![completed(
            "新结果",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store,
    ));
    let outcome = runtime.resume(created.lease.run_id).wait().await.unwrap();
    assert_eq!(
        outcome.terminal,
        TerminalState::Completed {
            message: "新结果".to_owned()
        }
    );
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn cancelling_a_tool_waits_for_its_cancellation_token() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "",
            None,
            vec![call("slow-1", "slow", "{}")],
            ModelFinishReason::ToolCalls,
        )])
    }));
    let (runtime, tools, sink, _) = fixture(model);
    let run = runtime.start(request("慢工具"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ToolExecutionStarted { .. }))
        .await;
    control.cancel().unwrap();
    let outcome = run.wait().await.unwrap();
    assert_eq!(outcome.terminal, TerminalState::Cancelled);
    assert!(tools.slow_cancelled.load(Ordering::Acquire));
    let events = sink
        .events()
        .into_iter()
        .filter(|event| event.run_id == run_id)
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEventKind::ToolOutcomeCommitted { call_id, outcome, .. }
            if call_id == "slow-1" && outcome.operation == ToolOperationStatus::Cancelled
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
    assert!(events.last().unwrap().event.is_terminal());
}

#[tokio::test]
async fn retryable_transport_reopens_only_without_output_and_preserves_first_failure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        let index = script_calls.fetch_add(1, Ordering::AcqRel);
        if index == 0 {
            ScriptResponse::Events(vec![StreamStep::error(ModelPortError::new(
                "deepseek_transport",
                ModelErrorCategory::Transport,
                "connection reset while reading response body",
                true,
            ))])
        } else {
            ScriptResponse::OpenError(ModelPortError::new(
                "service_unavailable",
                ModelErrorCategory::Service,
                "503",
                true,
            ))
        }
    }));
    let (runtime, _, sink, _) = fixture(model);
    let mut retry_request = request("retry");
    retry_request.limits.max_model_retries = 3;
    let outcome = runtime.start(retry_request).wait().await.unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::Model { ref code, .. }
        } if code == "deepseek_transport"
    ));
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(
                &event.event,
                RuntimeEventKind::ModelRequestFailed {
                    retry: ModelRetryDecision::Retry { prepared },
                    ..
                } if prepared.request.attempt > 0
            ))
            .count(),
        1
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        script_calls.fetch_add(1, Ordering::AcqRel);
        ScriptResponse::Events(vec![
            StreamStep::now(ModelStreamEvent::ContentDelta {
                delta: "片段".into(),
            }),
            StreamStep::error(ModelPortError::new(
                "deepseek_transport",
                ModelErrorCategory::Transport,
                "connection reset after actionable delta",
                true,
            )),
        ])
    }));
    let (runtime, _, _, _) = fixture(model);
    let mut partial_request = request("partial");
    partial_request.limits.max_model_retries = 3;
    let outcome = runtime.start(partial_request).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Failed { .. }));
    assert_eq!(
        calls.load(Ordering::Acquire),
        1,
        "partial output must not replay"
    );
}

#[tokio::test]
async fn model_event_idle_is_runtime_owned_and_obeys_actionable_output_gate() {
    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        script_calls.fetch_add(1, Ordering::AcqRel);
        ScriptResponse::Events(vec![StreamStep::delayed(
            Duration::from_secs(5),
            ModelStreamEvent::Completed {
                output: ModelOutput {
                    content: "too late".into(),
                    reasoning_content: None,
                    tool_calls: vec![],
                    finish_reason: ModelFinishReason::Stop,
                    usage: usage(),
                },
            },
        )])
    }));
    let (runtime, _, sink, _) = fixture(model);
    let mut stalled = request("idle");
    stalled.limits.model_event_idle_ms = Some(10);
    stalled.limits.max_model_retries = 1;
    let outcome = tokio::time::timeout(Duration::from_secs(1), runtime.start(stalled).wait())
        .await
        .expect("runtime event-idle deadline")
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::Model {
                ref code,
                category: ModelErrorCategory::StreamStall,
                ..
            }
        } if code == "stream_stall"
    ));
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(
                &event.event,
                RuntimeEventKind::ModelRequestFailed {
                    retry: ModelRetryDecision::Retry { prepared },
                    ..
                } if prepared.request.attempt > 0
            ))
            .count(),
        1
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let script_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        script_calls.fetch_add(1, Ordering::AcqRel);
        ScriptResponse::Events(vec![
            StreamStep::now(ModelStreamEvent::ContentDelta {
                delta: "actionable".into(),
            }),
            StreamStep::delayed(
                Duration::from_secs(5),
                ModelStreamEvent::Completed {
                    output: ModelOutput {
                        content: "actionable too late".into(),
                        reasoning_content: None,
                        tool_calls: vec![],
                        finish_reason: ModelFinishReason::Stop,
                        usage: usage(),
                    },
                },
            ),
        ])
    }));
    let (runtime, _, sink, _) = fixture(model);
    let mut partial = request("partial idle");
    partial.limits.model_event_idle_ms = Some(10);
    partial.limits.max_model_retries = 3;
    let outcome = tokio::time::timeout(Duration::from_secs(1), runtime.start(partial).wait())
        .await
        .expect("runtime event-idle deadline after delta")
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::Model { ref code, .. }
        } if code == "stream_stall"
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(!sink.events().iter().any(|event| matches!(
        &event.event,
        RuntimeEventKind::ModelRequestFailed {
            retry: ModelRetryDecision::Retry { prepared },
            ..
        } if prepared.request.attempt > 0
    )));
}

#[test]
fn agent_catalog_exposes_only_the_implemented_chinese_contract() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "完成",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, _, _) = fixture(model);
    let agent = runtime
        .tool_definitions(&ToolPolicy::default(), 0, 2, false)
        .into_iter()
        .find(|definition| definition.name == "agent")
        .expect("agent definition");
    assert!(agent.description.contains("只读后台子 Agent"));

    let properties = agent.input_schema["properties"]
        .as_object()
        .expect("agent properties");
    for expected in [
        "prompt",
        "type",
        "fork_context",
        "allowed_tools",
        "max_steps",
        "max_depth",
        "wall_time_secs",
        "expected_artifact",
    ] {
        assert!(
            properties.contains_key(expected),
            "missing field: {expected}"
        );
        assert!(
            properties[expected]["description"]
                .as_str()
                .is_some_and(|description| description
                    .chars()
                    .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))),
            "field description must be Chinese: {expected}"
        );
    }
    assert_eq!(properties.len(), 8);
    for removed in [
        "name",
        "task",
        "deliberate",
        "worktree",
        "workspace_policy",
        "write_authority",
    ] {
        assert!(
            !properties.contains_key(removed),
            "unsupported compatibility field leaked: {removed}"
        );
    }
    assert_eq!(agent.input_schema["required"], json!(["prompt"]));
}

#[tokio::test]
async fn async_child_launches_then_handoff_integrates_in_four_requests() {
    let root_calls = Arc::new(AtomicUsize::new(0));
    let child_calls = Arc::new(AtomicUsize::new(0));
    let root = root_calls.clone();
    let child = child_calls.clone();
    let model = Arc::new(MockModel::new(move |request| match request.actor.kind {
        AgentActorKind::Child => {
            child.fetch_add(1, Ordering::AcqRel);
            assert!(request.system_prompt.blocks.iter().any(|block| {
                block
                    .text
                    .contains("你是在同一 AgentRuntime 中运行的只读后台子 Agent")
            }));
            assert!(request.messages.iter().any(|message| matches!(
                message,
                ModelMessage::User { content }
                    if content.contains("调查\n期望产物：一份证据")
            )));
            assert!(request.tools.iter().any(|tool| tool.name == "read"));
            assert!(
                request.tools.iter().all(|tool| !matches!(
                    tool.name.as_str(),
                    "write" | "run_tests" | "run_verifiers"
                )),
                "children have no write or executable surface until WorkspaceLane owns a worktree"
            );
            ScriptResponse::Events(vec![StreamStep::delayed(
                Duration::from_millis(80),
                ModelStreamEvent::Completed {
                    output: ModelOutput {
                        content: "子结果".into(),
                        reasoning_content: None,
                        tool_calls: Vec::new(),
                        finish_reason: ModelFinishReason::Stop,
                        usage: usage(),
                    },
                },
            )])
        }
        AgentActorKind::Root => {
            let index = root.fetch_add(1, Ordering::AcqRel);
            let handoff = request.messages.iter().any(|message| {
                matches!(
                    message,
                    ModelMessage::User { content }
                        if content.contains("kind=\"subagent_completion\"")
                )
            });
            match index {
                0 => ScriptResponse::Events(vec![completed(
                    "",
                    None,
                    vec![call(
                        "agent-1",
                        "agent",
                        r#"{"prompt":"调查","expected_artifact":"一份证据"}"#,
                    )],
                    ModelFinishReason::ToolCalls,
                )]),
                1 => {
                    assert!(!handoff, "parent must get an early turn before handoff");
                    ScriptResponse::Events(vec![completed(
                        "先等待",
                        None,
                        Vec::new(),
                        ModelFinishReason::Stop,
                    )])
                }
                2 => {
                    assert!(handoff);
                    assert_eq!(
                        request
                            .messages
                            .iter()
                            .filter(|message| matches!(message, ModelMessage::Tool { name, .. } if name == "agent"))
                            .count(),
                        1,
                        "launch has one tool result; completion is an internal user handoff"
                    );
                    ScriptResponse::Events(vec![completed(
                        "整合完成",
                        None,
                        Vec::new(),
                        ModelFinishReason::Stop,
                    )])
                }
                _ => panic!("unexpected root request"),
            }
        }
    }));
    let model_for_assertion = model.clone();
    let (runtime, _, _, store) = fixture(model);
    let outcome = runtime.start(request("多Agent")).wait().await.unwrap();
    assert_eq!(root_calls.load(Ordering::Acquire), 3);
    assert_eq!(child_calls.load(Ordering::Acquire), 1);
    assert_eq!(outcome.runtime_model_requests, 4);
    assert_eq!(outcome.accounting.total_started(), 4);
    {
        let seals = model_for_assertion.seals.lock().expect("seal lock");
        assert_eq!(seals.last(), Some(&true));
        assert_eq!(seals.iter().filter(|seal| **seal).count(), 1);
    }
    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay.snapshot.runtime_model_requests,
        outcome.runtime_model_requests
    );
    assert_eq!(replay.snapshot.runtime_retries, outcome.runtime_retries);
    assert_eq!(replay.snapshot.tool_calls, outcome.tool_calls);
    assert_eq!(
        replay.snapshot.accounting.total_started(),
        outcome.accounting.total_started()
    );
    assert!(
        replay
            .snapshot
            .transcript
            .entries
            .iter()
            .any(|entry| matches!(
                entry,
                TranscriptEntry::ChildOutcome { handoff_content, .. }
                    if handoff_content.contains("kind=\"subagent_completion\"")
            ))
    );
}

#[tokio::test]
async fn child_limit_rejects_second_same_turn_spawn_without_fake_lifecycle_and_releases_on_join() {
    let root_calls = Arc::new(AtomicUsize::new(0));
    let child_calls = Arc::new(AtomicUsize::new(0));
    let roots = root_calls.clone();
    let children = child_calls.clone();
    let model =
        Arc::new(MockModel::new(move |request| match request.actor.kind {
            AgentActorKind::Child => {
                children.fetch_add(1, Ordering::AcqRel);
                ScriptResponse::Events(vec![completed(
                    "child result",
                    None,
                    vec![],
                    ModelFinishReason::Stop,
                )])
            }
            AgentActorKind::Root => match roots.fetch_add(1, Ordering::AcqRel) {
                0 => ScriptResponse::Events(vec![completed(
                    "",
                    None,
                    vec![
                        call("agent-1", "agent", r#"{"prompt":"one"}"#),
                        call("agent-2", "agent", r#"{"prompt":"two"}"#),
                    ],
                    ModelFinishReason::ToolCalls,
                )]),
                1 => {
                    let denied = request.messages.iter().any(|message| matches!(
                    message,
                    ModelMessage::Tool { call_id, content, .. }
                        if call_id == "agent-2" && content.contains("child_concurrency_limit")
                ));
                    assert!(denied);
                    ScriptResponse::Events(vec![completed(
                        "wait first",
                        None,
                        vec![],
                        ModelFinishReason::Stop,
                    )])
                }
                2 => ScriptResponse::Events(vec![completed(
                    "",
                    None,
                    vec![call("agent-3", "agent", r#"{"prompt":"after join"}"#)],
                    ModelFinishReason::ToolCalls,
                )]),
                3 => ScriptResponse::Events(vec![completed(
                    "wait third",
                    None,
                    vec![],
                    ModelFinishReason::Stop,
                )]),
                4 => ScriptResponse::Events(vec![completed(
                    "integrated",
                    None,
                    vec![],
                    ModelFinishReason::Stop,
                )]),
                _ => panic!("unexpected root request"),
            },
        }));
    let (runtime, _, sink, _) = fixture(model);
    let mut request = request("bounded children");
    request.limits.max_concurrent_children = 1;
    let outcome = runtime.start(request).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(root_calls.load(Ordering::Acquire), 5);
    assert_eq!(child_calls.load(Ordering::Acquire), 2);
    let child_started = sink
        .events()
        .into_iter()
        .filter_map(|event| match event.event {
            RuntimeEventKind::ChildStarted { call_id, .. } => Some(call_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(child_started, vec!["agent-1", "agent-3"]);
}

#[tokio::test]
async fn nested_child_uses_same_runtime_and_integrates_in_seven_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen_markers = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = calls.clone();
    let markers = seen_markers.clone();
    let model = Arc::new(MockModel::new(move |request| {
        calls_clone.fetch_add(1, Ordering::AcqRel);
        let marker = request.messages.iter().find_map(|message| match message {
            ModelMessage::User { content } if content.contains("subagent_completion") => {
                Some(content.clone())
            }
            _ => None,
        });
        if let Some(marker) = marker.as_ref() {
            markers.lock().unwrap().push(marker.clone());
        }
        let has_agent_result = request
            .messages
            .iter()
            .any(|message| matches!(message, ModelMessage::Tool { name, .. } if name == "agent"));
        let step = match (request.actor.depth, has_agent_result, marker.is_some()) {
            (0, false, false) => completed(
                "",
                None,
                vec![call("root-child", "agent", r#"{"prompt":"child"}"#)],
                ModelFinishReason::ToolCalls,
            ),
            (0, true, false) => completed("root early", None, vec![], ModelFinishReason::Stop),
            (0, true, true) => completed("root final", None, vec![], ModelFinishReason::Stop),
            (1, false, false) => completed(
                "",
                None,
                vec![call("grandchild", "agent", r#"{"prompt":"grandchild"}"#)],
                ModelFinishReason::ToolCalls,
            ),
            (1, true, false) => completed("child early", None, vec![], ModelFinishReason::Stop),
            (1, true, true) => completed("child final", None, vec![], ModelFinishReason::Stop),
            (2, false, false) => {
                completed("grandchild result", None, vec![], ModelFinishReason::Stop)
            }
            state => panic!("unexpected nested state: {state:?}"),
        };
        ScriptResponse::Events(vec![step])
    }));
    let (runtime, _, _, _) = fixture(model);
    let outcome = runtime.start(request("nested")).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(calls.load(Ordering::Acquire), 7);
    assert_eq!(outcome.runtime_model_requests, 7);
    let markers = seen_markers.lock().unwrap();
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("kind=\"child_subagent_completion\""))
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("kind=\"subagent_completion\""))
    );
}

#[tokio::test]
async fn logical_model_request_gate_does_not_report_physical_api_exhaustion() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        assert_eq!(
            observed_calls.fetch_add(1, Ordering::AcqRel),
            0,
            "the logical gate must reject request N+1 before ModelPort"
        );
        ScriptResponse::Events(vec![completed(
            "",
            None,
            vec![call("read-once", "read", "{}")],
            ModelFinishReason::ToolCalls,
        )])
    }));
    let (runtime, tools, _, _) = fixture(model);
    let mut limited = request("one logical model request");
    limited.limits.max_model_requests = 1;

    let outcome = runtime.start(limited).wait().await.unwrap();

    assert!(matches!(
        outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::ModelRequestBudgetExceeded { limit: 1 }
        }
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(tools.calls.lock().unwrap().len(), 1);
    assert_eq!(outcome.runtime_model_requests, 1);
    assert_eq!(outcome.accounting.total_started(), 1);
    assert_eq!(outcome.accounting.exhausted_denied, 0);
    assert!(!outcome.accounting.budget_exhausted);
}

#[tokio::test]
async fn typed_finish_failures_and_accounting_fail_closed() {
    for (reason, expected) in [
        (ModelFinishReason::Length, "output_limit"),
        (ModelFinishReason::ContentFilter, "content_filtered"),
        (
            ModelFinishReason::InsufficientSystemResource,
            "insufficient_system_resource",
        ),
    ] {
        let model = Arc::new(MockModel::new(move |_| {
            ScriptResponse::Events(vec![completed("partial", None, vec![], reason)])
        }));
        let (runtime, _, _, _) = fixture(model);
        let outcome = runtime.start(request("failure")).wait().await.unwrap();
        let encoded = serde_json::to_string(&outcome.terminal).unwrap();
        assert!(encoded.contains(expected), "{encoded}");
    }

    let model = Arc::new(MockModel::incomplete(|_| {
        ScriptResponse::Events(vec![completed(
            "looks done",
            None,
            vec![],
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, _, _, _) = fixture(model);
    let outcome = runtime.start(request("accounting")).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert!(!outcome.accounting.complete);
    assert!(outcome.accounting.usage_incomplete);

    let model = Arc::new(MockModel::exhausted(|_| {
        ScriptResponse::OpenError(ModelPortError::new(
            "admission_denied",
            ModelErrorCategory::Service,
            "physical request budget denied",
            false,
        ))
    }));
    let (runtime, _, _, _) = fixture(model);
    let outcome = runtime.start(request("budget")).wait().await.unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::Failed {
            failure: RuntimeFailure::ApiRequestBudgetExceeded { limit: 128 }
        }
    ));
    assert_eq!(outcome.accounting.total_started(), 1);
    assert_eq!(outcome.accounting.exhausted_denied, 1);
    assert!(outcome.accounting.budget_exhausted);
}

#[tokio::test]
async fn in_memory_store_event_ids_are_idempotent_and_sequences_are_monotonic() {
    let store = InMemoryRunStore::default();
    let created = store.create(request("event identity")).await.unwrap();
    let pending = PendingRuntimeEvent {
        event_id: RuntimeEventId("steer-1".into()),
        event: RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("command-1"),
            content: "first".into(),
        },
    };

    let first = store.append(&created.lease, pending.clone()).await.unwrap();
    let duplicate = store.append(&created.lease, pending).await.unwrap();
    assert_eq!(duplicate, first);
    assert_eq!(first.sequence, 2);

    let conflict = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("steer-1".into()),
                event: RuntimeEventKind::SteerQueued {
                    command_id: CommandId::from("command-1"),
                    content: "different".into(),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(conflict, RunStoreError::EventConflict { .. }));

    let next = append_event(
        &store,
        &created.lease,
        "steer-2",
        RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("command-2"),
            content: "second".into(),
        },
    )
    .await;
    assert_eq!(next.sequence, 3);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.event_id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["run_created", "steer-1", "steer-2"]
    );
}

#[tokio::test]
async fn reducer_enforces_steer_fifo_approval_identity_and_control_terminal_consistency() {
    let store = InMemoryRunStore::default();
    let created = store.create(request("reducer invariants")).await.unwrap();
    append_event(
        &store,
        &created.lease,
        "steer-first",
        RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("steer-first"),
            content: "第一条".to_owned(),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "steer-second",
        RuntimeEventKind::SteerQueued {
            command_id: CommandId::from("steer-second"),
            content: "第二条".to_owned(),
        },
    )
    .await;
    let out_of_order = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("steer-second-applied".to_owned()),
                event: RuntimeEventKind::SteerApplied {
                    command_id: CommandId::from("steer-second"),
                    content: "第二条".to_owned(),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        out_of_order,
        RunStoreError::Corrupt { ref message, .. } if message.contains("queued order")
    ));

    let approval_store = InMemoryRunStore::default();
    let approval_run = approval_store
        .create(request("approval invariants"))
        .await
        .unwrap();
    let operation_id = OperationId::from("approval-operation");
    append_event(
        &approval_store,
        &approval_run.lease,
        "approval-tool",
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: approval_run.lease.run_id.clone(),
                call_id: "approval-call".to_owned(),
                name: "approval".to_owned(),
                arguments: ToolArguments::from_value(json!({"path": "actual"})),
            },
        },
    )
    .await;
    let prompt = ToolApprovalPrompt {
        title: "确认".to_owned(),
        description: "确认参数".to_owned(),
        risk: ApprovalRisk::Elevated,
    };
    let mismatched_request = UserInteractionRequest {
        interaction_id: InteractionId::from("approval-interaction"),
        operation_id: operation_id.clone(),
        call_id: "approval-call".to_owned(),
        tool_name: "approval".to_owned(),
        prompt: UserInteractionPrompt::Approval {
            prompt: prompt.clone(),
            arguments: json!({"path": "different"}),
        },
    };
    let mismatch = approval_store
        .append(
            &approval_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("approval-mismatch".to_owned()),
                event: RuntimeEventKind::InteractionRequested {
                    request: mismatched_request,
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        mismatch,
        RunStoreError::Corrupt { ref message, .. } if message.contains("arguments")
    ));
    let interaction = UserInteractionRequest {
        interaction_id: InteractionId::from("approval-interaction"),
        operation_id: operation_id.clone(),
        call_id: "approval-call".to_owned(),
        tool_name: "approval".to_owned(),
        prompt: UserInteractionPrompt::Approval {
            prompt,
            arguments: json!({"path": "actual"}),
        },
    };
    append_event(
        &approval_store,
        &approval_run.lease,
        "approval-requested",
        RuntimeEventKind::InteractionRequested {
            request: interaction.clone(),
        },
    )
    .await;
    append_event(
        &approval_store,
        &approval_run.lease,
        "approval-denied",
        RuntimeEventKind::InteractionResolved {
            command_id: CommandId::from("deny-command"),
            interaction_id: interaction.interaction_id,
            response: UserInteractionResponse::Denied { reason: None },
        },
    )
    .await;
    let succeeded_after_denial = approval_store
        .append(
            &approval_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("denied-success".to_owned()),
                event: RuntimeEventKind::ToolOutcomeCommitted {
                    operation_id,
                    call_id: "approval-call".to_owned(),
                    name: "approval".to_owned(),
                    outcome: ToolOutcome::success("must be rejected"),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        succeeded_after_denial,
        RunStoreError::Corrupt { ref message, .. } if message.contains("rejected tool outcome")
    ));

    let user_input_store = InMemoryRunStore::default();
    let mut user_input_run_request = request("user-input reducer invariant");
    user_input_run_request.environment.interactive = true;
    let user_input_run = user_input_store
        .create(user_input_run_request)
        .await
        .unwrap();
    let user_input_operation = OperationId::from("user-input-operation");
    let invalid_user_input = UserInputRequest {
        questions: vec![
            UserInputQuestion {
                header: "范围".to_owned(),
                id: "scope".to_owned(),
                question: "选择范围".to_owned(),
                options: vec![
                    UserInputOption {
                        label: "A".to_owned(),
                        description: "选项 A".to_owned(),
                    },
                    UserInputOption {
                        label: "B".to_owned(),
                        description: "选项 B".to_owned(),
                    },
                ],
                allow_free_text: false,
                multi_select: false,
            },
            UserInputQuestion {
                header: "重复".to_owned(),
                id: "scope".to_owned(),
                question: "重复 ID".to_owned(),
                options: vec![
                    UserInputOption {
                        label: "C".to_owned(),
                        description: "选项 C".to_owned(),
                    },
                    UserInputOption {
                        label: "D".to_owned(),
                        description: "选项 D".to_owned(),
                    },
                ],
                allow_free_text: false,
                multi_select: false,
            },
        ],
    };
    append_event(
        &user_input_store,
        &user_input_run.lease,
        "user-input-tool",
        RuntimeEventKind::ToolPrepared {
            operation_id: user_input_operation.clone(),
            invocation: ToolInvocation {
                run_id: user_input_run.lease.run_id.clone(),
                call_id: "user-input-call".to_owned(),
                name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
                arguments: ToolArguments::from_value(
                    serde_json::to_value(&invalid_user_input).unwrap(),
                ),
            },
        },
    )
    .await;
    let invalid_interaction = user_input_store
        .append(
            &user_input_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("invalid-user-input".to_owned()),
                event: RuntimeEventKind::InteractionRequested {
                    request: UserInteractionRequest {
                        interaction_id: InteractionId::from("user-input-interaction"),
                        operation_id: user_input_operation,
                        call_id: "user-input-call".to_owned(),
                        tool_name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
                        prompt: UserInteractionPrompt::UserInput {
                            request: invalid_user_input,
                        },
                    },
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        invalid_interaction,
        RunStoreError::Corrupt { ref message, .. } if message.contains("duplicate question id")
    ));

    let defaulted_input_store = InMemoryRunStore::default();
    let mut defaulted_input_request = request("user-input omitted defaults");
    defaulted_input_request.environment.interactive = true;
    let defaulted_input_run = defaulted_input_store
        .create(defaulted_input_request)
        .await
        .unwrap();
    let defaulted_input_operation = OperationId::from("defaulted-input-operation");
    let raw_arguments = json!({
        "questions": [{
            "header": "范围",
            "id": "scope",
            "question": "选择范围",
            "options": [
                {"label": "A", "description": "选项 A"},
                {"label": "B", "description": "选项 B"}
            ]
        }]
    });
    append_event(
        &defaulted_input_store,
        &defaulted_input_run.lease,
        "defaulted-input-tool",
        RuntimeEventKind::ToolPrepared {
            operation_id: defaulted_input_operation.clone(),
            invocation: ToolInvocation {
                run_id: defaulted_input_run.lease.run_id.clone(),
                call_id: "defaulted-input-call".to_owned(),
                name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
                arguments: ToolArguments::from_value(raw_arguments.clone()),
            },
        },
    )
    .await;
    let approval_for_input = defaulted_input_store
        .append(
            &defaulted_input_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("approval-for-input".to_owned()),
                event: RuntimeEventKind::InteractionRequested {
                    request: UserInteractionRequest {
                        interaction_id: InteractionId::from("defaulted-input-interaction"),
                        operation_id: defaulted_input_operation.clone(),
                        call_id: "defaulted-input-call".to_owned(),
                        tool_name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
                        prompt: UserInteractionPrompt::Approval {
                            prompt: ToolApprovalPrompt {
                                title: "错误类型".to_owned(),
                                description: "request_user_input 不能走审批提示".to_owned(),
                                risk: ApprovalRisk::Elevated,
                            },
                            arguments: raw_arguments,
                        },
                    },
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        approval_for_input,
        RunStoreError::Corrupt { ref message, .. } if message.contains("requires a user-input")
    ));
    append_event(
        &defaulted_input_store,
        &defaulted_input_run.lease,
        "defaulted-input-requested",
        RuntimeEventKind::InteractionRequested {
            request: UserInteractionRequest {
                interaction_id: InteractionId::from("defaulted-input-interaction"),
                operation_id: defaulted_input_operation,
                call_id: "defaulted-input-call".to_owned(),
                tool_name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
                prompt: UserInteractionPrompt::UserInput {
                    request: UserInputRequest {
                        questions: vec![UserInputQuestion {
                            header: "范围".to_owned(),
                            id: "scope".to_owned(),
                            question: "选择范围".to_owned(),
                            options: vec![
                                UserInputOption {
                                    label: "A".to_owned(),
                                    description: "选项 A".to_owned(),
                                },
                                UserInputOption {
                                    label: "B".to_owned(),
                                    description: "选项 B".to_owned(),
                                },
                            ],
                            allow_free_text: false,
                            multi_select: false,
                        }],
                    },
                },
            },
        },
    )
    .await;

    let control_store = InMemoryRunStore::default();
    let control_run = control_store
        .create(request("control terminal invariant"))
        .await
        .unwrap();
    append_event(
        &control_store,
        &control_run.lease,
        "cancel-requested",
        RuntimeEventKind::ControlRequested {
            command_id: CommandId::from("cancel-command"),
            action: DurableControlAction::Cancel,
        },
    )
    .await;
    let work_after_control = control_store
        .append(
            &control_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("steer-after-control".to_owned()),
                event: RuntimeEventKind::SteerQueued {
                    command_id: CommandId::from("steer-after-control"),
                    content: "不应继续工作".to_owned(),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        work_after_control,
        RunStoreError::Corrupt { ref message, .. } if message.contains("settlement events")
    ));
    let mismatched_terminal = control_store
        .append(
            &control_run.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("wrong-terminal".to_owned()),
                event: RuntimeEventKind::Terminal {
                    outcome: Box::new(AgentOutcome {
                        run_id: control_run.lease.run_id.clone(),
                        parent_run_id: None,
                        terminal: TerminalState::Interrupted,
                        accounting: ModelAccounting::default(),
                        runtime_model_requests: 0,
                        runtime_retries: 0,
                        tool_calls: 0,
                    }),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        mismatched_terminal,
        RunStoreError::Corrupt { ref message, .. } if message.contains("durable control")
    ));
}

#[tokio::test]
async fn standalone_prepared_event_cannot_encode_a_retry() {
    let store = InMemoryRunStore::default();
    let created = store
        .create(request("reject standalone retry"))
        .await
        .unwrap();
    let mut retry_request = persisted_model_request(&created, 1);
    retry_request.attempt = 1;

    let error = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("standalone-retry".into()),
                event: RuntimeEventKind::ModelRequestPrepared {
                    attempt_id: AttemptId("standalone-retry".into()),
                    request: Box::new(retry_request),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RunStoreError::Corrupt { ref message, .. }
            if message.contains("must start at attempt zero")
    ));

    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.snapshot.runtime_model_requests, 0);
    assert!(replay.snapshot.pending_model.is_none());
}

#[tokio::test]
async fn atomic_retry_decision_rejects_a_changed_request_projection() {
    let store = InMemoryRunStore::default();
    let created = store.create(request("reject changed retry")).await.unwrap();
    let attempt_id = AttemptId("changed-retry-primary".into());
    append_event(
        &store,
        &created.lease,
        "changed-retry-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "changed-retry-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: attempt_id.clone(),
        },
    )
    .await;

    let failure = ModelAttemptFailure {
        code: "deepseek_transport".into(),
        category: ModelErrorCategory::Transport,
        message: "connection reset".into(),
        retryable: true,
        actionable_output: false,
    };
    let mut changed_request = persisted_model_request(&created, 1);
    changed_request.attempt = 1;
    changed_request.model.push_str("-changed");
    let error = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("changed-retry-decision".into()),
                event: RuntimeEventKind::ModelRequestFailed {
                    attempt_id,
                    failure,
                    accounting: Box::new(ModelAccounting::default()),
                    retry: ModelRetryDecision::Retry {
                        prepared: PreparedModelRetry {
                            attempt_id: AttemptId("changed-retry-next".into()),
                            request: Box::new(changed_request),
                        },
                    },
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RunStoreError::Corrupt { ref message, .. }
            if message.contains("changed fields other than the attempt number")
    ));

    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(replay.events.len(), 3);
    assert_eq!(replay.snapshot.runtime_model_requests, 1);
    assert_eq!(replay.snapshot.runtime_retries, 0);
    assert_eq!(
        replay.snapshot.pending_model.as_ref().unwrap().state,
        DurableActionState::InFlight
    );

    let mut live_projection = replay.snapshot.clone();
    let before_invalid_transition = live_projection.clone();
    let pending = live_projection.pending_model.as_ref().unwrap();
    let mut changed_request = pending.request.clone();
    changed_request.attempt = changed_request.attempt.saturating_add(1);
    changed_request.model.push_str("-changed-again");
    let invalid_transition = StoredRuntimeEvent {
        schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
        run_id: created.lease.run_id.clone(),
        parent_run_id: None,
        event_id: RuntimeEventId("changed-retry-live-projection".into()),
        sequence: live_projection.last_sequence.saturating_add(1),
        occurred_at_unix_ms: 1,
        event: RuntimeEventKind::ModelRequestFailed {
            attempt_id: pending.attempt_id.clone(),
            failure: ModelAttemptFailure {
                code: "deepseek_transport".into(),
                category: ModelErrorCategory::Transport,
                message: "connection reset".into(),
                retryable: true,
                actionable_output: false,
            },
            accounting: Box::new(ModelAccounting::default()),
            retry: ModelRetryDecision::Retry {
                prepared: PreparedModelRetry {
                    attempt_id: AttemptId("changed-retry-live-next".into()),
                    request: Box::new(changed_request),
                },
            },
        },
    };
    assert!(apply_event(&mut live_projection, &invalid_transition).is_err());
    assert_eq!(live_projection, before_invalid_transition);
}

#[tokio::test]
async fn model_response_atomically_replays_assistant_usage_and_accounting() {
    let store = InMemoryRunStore::default();
    let created = store
        .create(request("atomic model response"))
        .await
        .unwrap();
    let attempt_id = AttemptId("atomic-model".into());
    append_event(
        &store,
        &created.lease,
        "atomic-model-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "atomic-model-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: attempt_id.clone(),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "atomic-model-delta",
        RuntimeEventKind::ContentDelta {
            attempt_id: attempt_id.clone(),
            index: 1,
            delta: "atomic answer".into(),
        },
    )
    .await;

    let before_commit = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(before_commit.snapshot.usage, Usage::default());
    assert!(
        !before_commit
            .snapshot
            .transcript
            .entries
            .iter()
            .any(|entry| matches!(entry, TranscriptEntry::Assistant { .. }))
    );
    assert_eq!(
        before_commit.snapshot.pending_model.as_ref().unwrap().state,
        DurableActionState::InFlight
    );

    let output = model_output(
        "atomic answer",
        Some("atomic reasoning"),
        Vec::new(),
        ModelFinishReason::Stop,
    );
    let accounting = ModelAccounting {
        complete: true,
        usage_complete: true,
        usage: output.usage,
        ..ModelAccounting::default()
    };
    let committed_event = RuntimeEventKind::ModelResponseCommitted {
        attempt_id: attempt_id.clone(),
        output: Box::new(output.clone()),
        accounting: Box::new(accounting.clone()),
    };
    let committed = append_event(
        &store,
        &created.lease,
        "atomic-model-committed",
        committed_event.clone(),
    )
    .await;
    let duplicate = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("atomic-model-committed".into()),
                event: committed_event,
            },
        )
        .await
        .unwrap();
    assert_eq!(duplicate, committed);

    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.usage, output.usage);
    assert_eq!(replay.snapshot.accounting, accounting);
    assert!(replay.snapshot.pending_model.is_none());
    let assistants = replay
        .snapshot
        .transcript
        .entries
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => Some((content, reasoning_content, tool_calls)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(assistants.len(), 1);
    assert_eq!(assistants[0].0.as_deref(), Some("atomic answer"));
    assert_eq!(assistants[0].1.as_deref(), Some("atomic reasoning"));
}

#[tokio::test]
async fn tool_commit_atomically_replays_typed_outcome_and_transcript() {
    let store = InMemoryRunStore::default();
    let created = store.create(request("atomic tool outcome")).await.unwrap();
    let tool_call = call("read-1", "read", r#"{"path":"src/lib.rs"}"#);
    seed_committed_model_output(
        &store,
        &created,
        model_output(
            "",
            Some("read first"),
            vec![tool_call.clone()],
            ModelFinishReason::ToolCalls,
        ),
    )
    .await;
    let operation_id = OperationId("operation-read-1".into());
    let invocation = ToolInvocation {
        run_id: created.lease.run_id.clone(),
        call_id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
    };
    append_event(
        &store,
        &created.lease,
        "tool-prepared",
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation,
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "tool-in-flight",
        RuntimeEventKind::ToolExecutionStarted {
            operation_id: operation_id.clone(),
        },
    )
    .await;

    let before_commit = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert!(
        !before_commit
            .snapshot
            .transcript
            .entries
            .iter()
            .any(|entry| matches!(entry, TranscriptEntry::Tool { .. }))
    );
    assert_eq!(
        before_commit.snapshot.pending_tool.as_ref().unwrap().state,
        DurableActionState::InFlight
    );

    let mut outcome = ToolOutcome::success("file contents").with_evidence(ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: vec!["file://src/lib.rs".into()],
    });
    outcome.workspace_revision = Some("revision-after-read".into());
    outcome.artifacts.push(ToolArtifact {
        id: "read-receipt".into(),
        status: ToolArtifactStatus::Available,
        sha256: Some("deadbeef".into()),
        media_type: Some("text/plain".into()),
        byte_len: Some(13),
    });
    let committed_event = RuntimeEventKind::ToolOutcomeCommitted {
        operation_id,
        call_id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        outcome: outcome.clone(),
    };
    let committed = append_event(
        &store,
        &created.lease,
        "tool-committed",
        committed_event.clone(),
    )
    .await;
    let duplicate = store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("tool-committed".into()),
                event: committed_event,
            },
        )
        .await
        .unwrap();
    assert_eq!(duplicate, committed);

    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert!(replay.snapshot.pending_tool.is_none());
    let tools = replay
        .snapshot
        .transcript
        .entries
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Tool {
                call_id,
                name,
                outcome,
            } => Some((call_id, name, outcome)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].0, "read-1");
    assert_eq!(tools[0].1, "read");
    assert_eq!(tools[0].2, &outcome);
}

#[tokio::test]
async fn resuming_terminal_run_returns_durable_outcome_without_model_or_tool_calls() {
    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "durable completion",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let (runtime, tools, _, store) = fixture(model.clone());
    let completed = runtime
        .start(request("complete once"))
        .wait()
        .await
        .unwrap();
    let model_calls = model.ledger.root.started.load(Ordering::Acquire);
    let tool_calls = tools.calls.lock().unwrap().len();

    let resumed = runtime
        .resume(completed.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(resumed, completed);
    assert_eq!(
        model.ledger.root.started.load(Ordering::Acquire),
        model_calls
    );
    assert_eq!(tools.calls.lock().unwrap().len(), tool_calls);
    let replay = store.load(&completed.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_prepared_model_executes_the_persisted_attempt_once() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("prepared model")).await.unwrap();
    let attempt_id = AttemptId("prepared-model-attempt".into());
    append_event(
        &store,
        &created.lease,
        "prepared-model",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id,
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|request| {
        assert_eq!(request.request_number, 1);
        ScriptResponse::Events(vec![completed(
            "resumed prepared request",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let tools = Arc::new(MockTools::default());
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        tools.clone(),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(
        outcome.terminal,
        TerminalState::Completed {
            message: "resumed prepared request".into()
        }
    );
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 1);
    assert!(tools.calls.lock().unwrap().is_empty());
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. }))
            .count(),
        1
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ModelRequestInFlight { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn crash_after_atomic_retry_decision_resumes_the_prepared_retry_once() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("prepared retry")).await.unwrap();
    let primary = ModelAttemptFailure {
        code: "deepseek_transport".into(),
        category: ModelErrorCategory::Transport,
        message: "first connection reset".into(),
        retryable: true,
        actionable_output: false,
    };
    let first_attempt = AttemptId("failed-first-attempt".into());
    append_event(
        &store,
        &created.lease,
        "failed-first-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: first_attempt.clone(),
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "failed-first-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: first_attempt.clone(),
        },
    )
    .await;
    let retry_attempt = AttemptId("prepared-retry-attempt".into());
    let mut retry_request = persisted_model_request(&created, 1);
    retry_request.attempt = 1;
    append_event(
        &store,
        &created.lease,
        "failed-first-and-retry-prepared",
        RuntimeEventKind::ModelRequestFailed {
            attempt_id: first_attempt,
            failure: primary,
            accounting: Box::new(ModelAccounting::default()),
            retry: ModelRetryDecision::Retry {
                prepared: PreparedModelRetry {
                    attempt_id: retry_attempt.clone(),
                    request: Box::new(retry_request),
                },
            },
        },
    )
    .await;

    let crash_replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    let pending = crash_replay
        .snapshot
        .pending_model
        .as_ref()
        .expect("atomic retry decision prepares the next request");
    assert_eq!(pending.state, DurableActionState::Prepared);
    assert_eq!(pending.attempt_id, retry_attempt);
    assert_eq!(pending.request.attempt, 1);
    assert_eq!(crash_replay.snapshot.runtime_model_requests, 2);
    assert_eq!(crash_replay.snapshot.runtime_retries, 1);
    assert_eq!(
        crash_replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. }))
            .count(),
        1,
        "a retry is not a second standalone prepared event"
    );
    assert_eq!(
        crash_replay
            .events
            .iter()
            .filter(|event| matches!(
                event.event,
                RuntimeEventKind::ModelRequestFailed {
                    retry: ModelRetryDecision::Retry { .. },
                    ..
                }
            ))
            .count(),
        1
    );
    store.release(&created.lease).await.unwrap();

    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = attempts.clone();
    let model = Arc::new(MockModel::new(move |request| {
        observed.lock().unwrap().push(request.attempt);
        if request.attempt == 1 {
            ScriptResponse::OpenError(ModelPortError::new(
                "deepseek_transport",
                ModelErrorCategory::Transport,
                "second connection reset",
                true,
            ))
        } else {
            assert_eq!(request.attempt, 2);
            ScriptResponse::Events(vec![completed(
                "retry resumed and completed",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )])
        }
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model,
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(*attempts.lock().unwrap(), vec![1, 2]);
    assert_eq!(outcome.runtime_model_requests, 3);
    assert_eq!(outcome.runtime_retries, 2);
    assert!(matches!(
        outcome.terminal,
        TerminalState::Completed { ref message }
            if message == "retry resumed and completed"
    ));
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.runtime_model_requests, 3);
    assert_eq!(replay.snapshot.runtime_retries, 2);
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_in_flight_retry_requires_recovery_without_reissuing_the_retry() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("in-flight retry")).await.unwrap();
    let primary = ModelAttemptFailure {
        code: "deepseek_transport".into(),
        category: ModelErrorCategory::Transport,
        message: "first connection reset".into(),
        retryable: true,
        actionable_output: false,
    };
    let first_attempt = AttemptId("retry-primary-attempt".into());
    append_event(
        &store,
        &created.lease,
        "retry-primary-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: first_attempt.clone(),
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "retry-primary-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: first_attempt.clone(),
        },
    )
    .await;
    let retry_attempt = AttemptId("ambiguous-retry-attempt".into());
    let mut retry_request = persisted_model_request(&created, 1);
    retry_request.attempt = 1;
    append_event(
        &store,
        &created.lease,
        "retry-primary-failed-and-retry-prepared",
        RuntimeEventKind::ModelRequestFailed {
            attempt_id: first_attempt,
            failure: primary,
            accounting: Box::new(ModelAccounting::default()),
            retry: ModelRetryDecision::Retry {
                prepared: PreparedModelRetry {
                    attempt_id: retry_attempt.clone(),
                    request: Box::new(retry_request),
                },
            },
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "ambiguous-retry-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: retry_attempt.clone(),
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|_| {
        panic!("an in-flight retry must never be issued a second time")
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ModelRequest,
                ref action_id,
                ..
            }
        } if action_id == &retry_attempt.0
    ));
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 0);
    assert_eq!(outcome.runtime_model_requests, 2);
    assert_eq!(outcome.runtime_retries, 1);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ModelRequestPrepared { .. }))
            .count(),
        1
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_prepared_tool_executes_once_then_continues_from_committed_response() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("prepared tool")).await.unwrap();
    let tool_call = call("prepared-read", "read", r#"{"path":"src/lib.rs"}"#);
    seed_committed_model_output(
        &store,
        &created,
        model_output(
            "",
            Some("use prepared tool"),
            vec![tool_call.clone()],
            ModelFinishReason::ToolCalls,
        ),
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "prepared-tool",
        RuntimeEventKind::ToolPrepared {
            operation_id: OperationId("prepared-tool-operation".into()),
            invocation: ToolInvocation {
                run_id: created.lease.run_id.clone(),
                call_id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
            },
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|request| {
        assert!(request.messages.iter().any(|message| matches!(
            message,
            ModelMessage::Tool { call_id, content, .. }
                if call_id == "prepared-read" && content == "tool-result"
        )));
        ScriptResponse::Events(vec![completed(
            "continued after tool",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let tools = Arc::new(MockTools::default());
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        tools.clone(),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(
        outcome.terminal,
        TerminalState::Completed {
            message: "continued after tool".into()
        }
    );
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 1);
    assert_eq!(tools.calls.lock().unwrap().len(), 1);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ToolPrepared { .. }))
            .count(),
        1
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ToolExecutionStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(event.event, RuntimeEventKind::ToolOutcomeCommitted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_in_flight_model_requires_recovery_without_reissuing_request() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("in-flight model")).await.unwrap();
    let attempt_id = AttemptId("ambiguous-model-attempt".into());
    append_event(
        &store,
        &created.lease,
        "ambiguous-model-prepared",
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(persisted_model_request(&created, 1)),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "ambiguous-model-in-flight",
        RuntimeEventKind::ModelRequestInFlight {
            attempt_id: attempt_id.clone(),
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|_| {
        panic!("in-flight request must not be reissued")
    }));
    let tools = Arc::new(MockTools::default());
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        tools.clone(),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ModelRequest,
                ref action_id,
                ..
            }
        } if action_id == &attempt_id.0
    ));
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 0);
    assert!(tools.calls.lock().unwrap().is_empty());
    let repeated = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(repeated, outcome);
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 0);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn pending_cancel_does_not_hide_in_flight_tool_recovery_ambiguity() {
    let store = Arc::new(InMemoryRunStore::default());
    let created = store.create(request("in-flight tool")).await.unwrap();
    let tool_call = call("ambiguous-write", "write", r#"{"path":"out.txt"}"#);
    seed_committed_model_output(
        &store,
        &created,
        model_output(
            "",
            None,
            vec![tool_call.clone()],
            ModelFinishReason::ToolCalls,
        ),
    )
    .await;
    let operation_id = OperationId("ambiguous-tool-operation".into());
    append_event(
        &store,
        &created.lease,
        "ambiguous-tool-prepared",
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: created.lease.run_id.clone(),
                call_id: tool_call.id,
                name: tool_call.name,
                arguments: tool_call.arguments,
            },
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "ambiguous-tool-in-flight",
        RuntimeEventKind::ToolExecutionStarted {
            operation_id: operation_id.clone(),
        },
    )
    .await;
    append_event(
        &store,
        &created.lease,
        "cancel-requested-before-crash",
        RuntimeEventKind::ControlRequested {
            command_id: CommandId::from("cancel-before-crash"),
            action: DurableControlAction::Cancel,
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|_| {
        panic!("tool recovery must not call model")
    }));
    let tools = Arc::new(MockTools::default());
    let runtime = Arc::new(AgentRuntime::new(
        model.clone(),
        tools.clone(),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ToolExecution,
                ref action_id,
                ..
            }
        } if action_id == &operation_id.0
    ));
    assert_eq!(model.ledger.root.started.load(Ordering::Acquire), 0);
    assert!(tools.calls.lock().unwrap().is_empty());
    let repeated = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert_eq!(repeated, outcome);
    assert!(tools.calls.lock().unwrap().is_empty());
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn run_created_accounting_baseline_survives_crash_and_resume() {
    let store = Arc::new(InMemoryRunStore::default());
    let route_usage = Usage {
        input_tokens: 7,
        output_tokens: 2,
        cache_hit_tokens: 0,
        cache_miss_tokens: 7,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        reasoning_replay_tokens: 0,
    };
    let baseline = ModelAccounting {
        hard_request_limit: Some(2),
        root: ActorRequestAccounting {
            started: 1,
            completed: 1,
            in_flight: 0,
            retries: 0,
        },
        complete: true,
        usage_complete: true,
        usage_responses: 1,
        usage: route_usage,
        surface_usage: vec![SurfaceUsage {
            surface: ApiSurface::StandardChat,
            model: "deepseek-v4-flash".into(),
            response_count: 1,
            usage_response_count: 1,
            usage: route_usage,
            cost_nanousd: 700,
            cost_nanocny: 5_000,
        }],
        cost_nanousd: 700,
        cost_nanocny: 5_000,
        ..ModelAccounting::default()
    };
    let mut initial = request("resume after run_created accounting checkpoint");
    initial.limits.max_model_requests = 2;
    initial.accounting_baseline = baseline.clone();
    let created = store.create(initial).await.unwrap();

    assert_eq!(created.replay.snapshot.accounting, baseline);
    assert!(matches!(
        &created.created.event,
        RuntimeEventKind::RunCreated { request }
            if request.accounting_baseline == baseline
    ));
    store.release(&created.lease).await.unwrap();

    let model = Arc::new(MockModel::new(|_| {
        ScriptResponse::Events(vec![completed(
            "resumed after durable accounting checkpoint",
            None,
            Vec::new(),
            ModelFinishReason::Stop,
        )])
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model,
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();

    assert_eq!(outcome.accounting.total_started(), 2);
    assert_eq!(outcome.accounting.total_completed(), 2);
    assert_eq!(outcome.accounting.usage.input_tokens, 17);
    assert_eq!(outcome.accounting.usage.output_tokens, 4);
    assert_eq!(outcome.accounting.cost_nanousd, baseline.cost_nanousd);
    assert_eq!(outcome.accounting.cost_nanocny, baseline.cost_nanocny);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert_eq!(replay.snapshot.accounting.total_started(), 2);
    assert_eq!(replay.snapshot.accounting.total_completed(), 2);
    assert_eq!(replay.snapshot.accounting.usage, outcome.accounting.usage);
    assert_eq!(
        replay.snapshot.accounting.cost_nanousd,
        outcome.accounting.cost_nanousd
    );
    assert_eq!(
        replay.snapshot.accounting.cost_nanocny,
        outcome.accounting.cost_nanocny
    );
}

#[tokio::test]
async fn live_projection_matches_store_replay_through_tool_steer_and_terminal() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        match observed_calls.fetch_add(1, Ordering::AcqRel) {
            0 => ScriptResponse::Events(vec![completed(
                "",
                Some("先读取再继续"),
                vec![call("read-live-projection", "delay", "{}")],
                ModelFinishReason::ToolCalls,
            )]),
            1 => {
                assert!(request.messages.iter().any(|message| matches!(
                    message,
                    ModelMessage::Tool { call_id, .. } if call_id == "read-live-projection"
                )));
                assert!(request.messages.iter().any(|message| matches!(
                    message,
                    ModelMessage::User { content } if content == "追加约束"
                )));
                ScriptResponse::Events(vec![completed(
                    "投影一致",
                    None,
                    Vec::new(),
                    ModelFinishReason::Stop,
                )])
            }
            _ => panic!("unexpected model request"),
        }
    }));
    let (runtime, _, sink, store) = fixture(model);
    let run = runtime.start(request("验证 live projection"));
    let run_id = run.run_id.clone();
    let control = run.control();
    sink.wait_for(|event| matches!(event.event, RuntimeEventKind::ToolExecutionStarted { .. }))
        .await;
    control.steer("追加约束").unwrap();
    let outcome = run.wait().await.unwrap();

    let replay = store.load(&run_id).await.unwrap().unwrap();
    assert_eq!(reduce_events(&replay.events).unwrap(), replay.snapshot);
    assert_eq!(replay.snapshot.terminal.as_ref(), Some(&outcome));
    assert_eq!(replay.snapshot.runtime_model_requests, 2);
    assert_eq!(replay.snapshot.runtime_retries, 0);
    assert_eq!(replay.snapshot.tool_calls, 1);

    let mut prepared_requests = 0;
    for (index, event) in replay.events.iter().enumerate() {
        let RuntimeEventKind::ModelRequestPrepared { request, .. } = &event.event else {
            continue;
        };
        prepared_requests += 1;
        let projection = reduce_events(&replay.events[..=index]).unwrap();
        assert_eq!(request.messages, projection.transcript.project_messages());
        assert_eq!(request.request_number, projection.local_turns);
        assert_eq!(request.run_id, run_id);
    }
    assert_eq!(prepared_requests, 2);
}

fn compaction_policy(keep_recent_user_turns: u32) -> ContextPolicy {
    ContextPolicy {
        auto_compact: true,
        context_window_tokens: 100_000,
        trigger_tokens: 1,
        hard_input_tokens: 80_000,
        summary_max_output_tokens: 512,
        min_messages: 2,
        keep_recent_user_turns,
        max_retries: 2,
    }
}

fn long_transcript(turns: usize) -> CanonicalTranscript {
    let mut entries = vec![TranscriptEntry::System {
        prompt: SystemPrompt::from_text("系统提示"),
    }];
    for index in 0..turns {
        entries.push(TranscriptEntry::User {
            content: format!("历史用户约束 {index} {}", "甲".repeat(6_000)),
        });
        entries.push(TranscriptEntry::Assistant {
            content: Some(format!("历史答复 {index}")),
            reasoning_content: None,
            tool_calls: Vec::new(),
        });
    }
    CanonicalTranscript { entries }
}

#[tokio::test]
async fn automatic_compaction_is_durable_and_the_agent_consumes_only_the_projection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        match observed_calls.fetch_add(1, Ordering::AcqRel) {
            0 => {
                assert!(!request.streaming);
                assert!(request.tools.is_empty());
                assert!(request.messages.len() > 2);
                ScriptResponse::Events(vec![completed(
                    "已压缩：保留用户目标、代码状态和测试证据。",
                    None,
                    Vec::new(),
                    ModelFinishReason::Stop,
                )])
            }
            1 => {
                assert!(request.streaming);
                assert!(request.system_prompt.blocks.iter().any(|block| {
                    block
                        .text
                        .contains("已压缩：保留用户目标、代码状态和测试证据")
                }));
                assert!(
                    request.messages.len() < 8,
                    "ordinary Agent request must use the compacted projection"
                );
                ScriptResponse::Events(vec![completed(
                    "压缩后继续执行完成",
                    None,
                    Vec::new(),
                    ModelFinishReason::Stop,
                )])
            }
            _ => panic!("unexpected extra model request"),
        }
    }));
    let (runtime, _, _, store) = fixture(model);
    let original = long_transcript(8);
    let original_len = original.entries.len();
    let mut run_request = request("最新任务");
    run_request.transcript = original;
    run_request.context_policy = compaction_policy(2);
    let outcome = runtime.start(run_request).wait().await.unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));

    let replay = store.load(&outcome.run_id).await.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert_eq!(replay.snapshot.runtime_model_requests, 2);
    assert_eq!(
        replay.snapshot.transcript.entries.len(),
        original_len + 2,
        "only latest user input and final Agent response may extend canonical history"
    );
    assert!(replay.snapshot.context_projection.is_some());
    assert!(replay.snapshot.last_context_compaction.is_some());
    let kinds = replay
        .events
        .iter()
        .map(|event| &event.event)
        .collect::<Vec<_>>();
    let committed = kinds
        .iter()
        .position(|event| matches!(event, RuntimeEventKind::ContextCompactionCommitted { .. }))
        .expect("compaction committed");
    let ordinary = kinds
        .iter()
        .position(|event| matches!(event, RuntimeEventKind::ModelRequestPrepared { .. }))
        .expect("ordinary model prepared");
    assert!(committed < ordinary);
}

#[tokio::test]
async fn manual_compaction_is_an_input_free_continuation_with_a_completion_marker() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        match observed_calls.fetch_add(1, Ordering::AcqRel) {
            0 => ScriptResponse::Events(vec![completed(
                "源运行完成",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )]),
            1 => {
                assert!(!request.streaming);
                ScriptResponse::Events(vec![completed(
                    "手动压缩摘要",
                    None,
                    Vec::new(),
                    ModelFinishReason::Stop,
                )])
            }
            _ => panic!("manual compaction was issued more than once"),
        }
    }));
    let (runtime, _, _, store) = fixture(model);
    let mut source_request = request(&format!("源任务 {}", "乙".repeat(20_000)));
    source_request.transcript = long_transcript(3);
    let source = runtime.start(source_request).wait().await.unwrap();
    let source_replay = store.load(&source.run_id).await.unwrap().unwrap();

    let mut compact_request = source_replay.snapshot.request.clone();
    compact_request.run_id = None;
    compact_request.parent_run_id = None;
    compact_request.continued_from_run_id = Some(source.run_id.clone());
    compact_request.purpose = RunPurpose::ContextCompaction;
    compact_request.input.clear();
    compact_request.transcript = source_replay.snapshot.transcript.clone();
    compact_request.context_projection = source_replay.snapshot.context_projection.clone();
    compact_request.context_policy = compaction_policy(0);
    compact_request.deadline_unix_ms = None;
    compact_request.accounting_baseline = ModelAccounting::default();
    let compact = runtime.start(compact_request).wait().await.unwrap();
    assert!(matches!(compact.terminal, TerminalState::Completed { .. }));

    let replay = store.load(&compact.run_id).await.unwrap().unwrap();
    assert_eq!(
        replay.snapshot.request.purpose,
        RunPurpose::ContextCompaction
    );
    assert_eq!(
        replay.snapshot.request.continued_from_run_id,
        Some(source.run_id.clone())
    );
    assert!(replay.snapshot.last_context_compaction.is_some());
    assert_eq!(calls.load(Ordering::Acquire), 2);
    let source_after = store.load(&source.run_id).await.unwrap().unwrap();
    assert_eq!(source_after.events, source_replay.events);
}

async fn seed_prepared_compaction(
    store: &InMemoryRunStore,
) -> (CreatedRun, ContextCompactionId, AttemptId) {
    let mut run_request = request("最新任务");
    run_request.transcript = long_transcript(8);
    run_request.context_policy = compaction_policy(2);
    let created = store.create(run_request).await.unwrap();
    let snapshot = &created.replay.snapshot;
    let template = ModelRequest {
        run_id: created.lease.run_id.clone(),
        parent_run_id: None,
        actor: AgentActor::default(),
        model: snapshot.request.model.clone(),
        system_prompt: snapshot.request.system_prompt.clone(),
        messages: Vec::new(),
        tools: Vec::new(),
        reasoning_effort: ReasoningEffort::Low,
        max_output_tokens: Some(snapshot.request.context_policy.summary_max_output_tokens),
        streaming: false,
        request_number: 1,
        attempt: 0,
    };
    let ContextCompactionPreparation::Model { plan, request } = prepare_compaction(
        &snapshot.transcript,
        None,
        snapshot.request.context_policy,
        &template,
        false,
    )
    .expect("compaction plan") else {
        panic!("expected model compaction")
    };
    let compaction_id = ContextCompactionId::new();
    let attempt_id = AttemptId::new();
    append_event(
        store,
        &created.lease,
        "prepared-context-compaction",
        RuntimeEventKind::ContextCompactionPrepared {
            compaction_id: compaction_id.clone(),
            attempt_id: attempt_id.clone(),
            trigger: ContextCompactionTrigger::Threshold,
            plan: Box::new(plan),
            request: Box::new(request),
        },
    )
    .await;
    (created, compaction_id, attempt_id)
}

#[tokio::test]
async fn resume_prepared_compaction_sends_the_persisted_summary_once() {
    let store = Arc::new(InMemoryRunStore::default());
    let (created, _, _) = seed_prepared_compaction(&store).await;
    store.release(&created.lease).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let model = Arc::new(MockModel::new(move |request| {
        match observed.fetch_add(1, Ordering::AcqRel) {
            0 => {
                assert!(!request.streaming);
                ScriptResponse::Events(vec![completed(
                    "恢复后的压缩摘要",
                    None,
                    Vec::new(),
                    ModelFinishReason::Stop,
                )])
            }
            1 => ScriptResponse::Events(vec![completed(
                "继续执行完成",
                None,
                Vec::new(),
                ModelFinishReason::Stop,
            )]),
            _ => panic!("prepared compaction was sent more than once"),
        }
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model,
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store.clone(),
    ));
    let outcome = runtime
        .resume(created.lease.run_id.clone())
        .wait()
        .await
        .unwrap();
    assert!(matches!(outcome.terminal, TerminalState::Completed { .. }));
    assert_eq!(calls.load(Ordering::Acquire), 2);
    let replay = store.load(&created.lease.run_id).await.unwrap().unwrap();
    assert!(replay.snapshot.last_context_compaction.is_some());
    assert_eq!(
        replay
            .events
            .iter()
            .filter(|event| matches!(
                event.event,
                RuntimeEventKind::ContextCompactionPrepared { .. }
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_in_flight_compaction_requires_recovery_without_reissuing() {
    let store = Arc::new(InMemoryRunStore::default());
    let (created, compaction_id, attempt_id) = seed_prepared_compaction(&store).await;
    append_event(
        &store,
        &created.lease,
        "in-flight-context-compaction",
        RuntimeEventKind::ContextCompactionInFlight {
            compaction_id,
            attempt_id,
        },
    )
    .await;
    store.release(&created.lease).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let model = Arc::new(MockModel::new(move |_| {
        observed.fetch_add(1, Ordering::AcqRel);
        panic!("in-flight compaction must not be reissued")
    }));
    let runtime = Arc::new(AgentRuntime::new(
        model,
        Arc::new(MockTools::default()),
        Arc::new(CollectSink::default()),
        store,
    ));
    let outcome = runtime.resume(created.lease.run_id).wait().await.unwrap();
    assert!(matches!(
        outcome.terminal,
        TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ContextCompactionModelRequest,
                ..
            }
        }
    ));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(outcome.accounting.billing_unknown);
    assert_eq!(outcome.accounting.billing_unknown_attempts, 1);
}
