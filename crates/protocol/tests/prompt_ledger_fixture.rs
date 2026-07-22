use std::collections::HashMap;

use codewhale_protocol::agent_runtime::{
    AGENT_RUNTIME_EVENT_SCHEMA_VERSION, ActorRequestAccounting, AgentActor, AgentActorKind,
    AgentOutcome, AgentResultDetails, AgentTask, AgentTaskId, AgentWorkspaceAccess,
    AgentWorkspaceAssignment, AttemptId, ContextProjection, ModelAccounting, ModelAttemptFailure,
    ModelErrorCategory, ModelMessage, ModelRequest, ModelResponseEvidence, ModelRetryDecision,
    ModelRetryStopReason, PreparedModelRetry, PromptCacheControl, ReasoningEffort, RunId,
    RunRequest, RuntimeEventId, RuntimeEventKind, RuntimeFailure, StoredRuntimeEvent, SystemPrompt,
    SystemPromptBlock, TerminalState, ToolDefinition,
};
use codewhale_protocol::task::{TaskContract, TaskDefinition, TaskGenerationId};

fn stable_prefix(prompt: &SystemPrompt) -> Vec<&str> {
    prompt
        .blocks
        .iter()
        .take_while(|block| block.cache_control == PromptCacheControl::Stable)
        .map(|block| block.text.as_str())
        .collect()
}

fn request(
    run_id: &RunId,
    parent_run_id: Option<RunId>,
    actor: AgentActor,
    objective: &str,
    volatile_prompt: &str,
) -> RunRequest {
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId(run_id.0.clone()),
            definition: TaskDefinition::host(objective),
        },
        SystemPrompt {
            blocks: vec![
                SystemPromptBlock {
                    text: "你是 CodeWhale 编码 Agent。".to_owned(),
                    cache_control: PromptCacheControl::Stable,
                },
                SystemPromptBlock {
                    text: volatile_prompt.to_owned(),
                    cache_control: PromptCacheControl::Volatile,
                },
            ],
        },
    );
    request.run_id = Some(run_id.clone());
    request.parent_run_id = parent_run_id;
    request.actor = actor;
    request.reasoning_effort = ReasoningEffort::High;
    request.max_output_tokens = Some(256);
    request.environment.workspace = "/fixture/workspace".to_owned();
    if actor.kind == AgentActorKind::Child {
        let parent_run_id = request
            .parent_run_id
            .clone()
            .expect("child fixture parent run id");
        request.agent_task = Some(AgentTask {
            task_id: AgentTaskId::from(format!("task:{}", run_id.0)),
            root_run_id: parent_run_id.clone(),
            parent_run_id,
            child_run_id: run_id.clone(),
            call_id: format!("call:{}", run_id.0),
            role: "explorer".to_owned(),
            task_contract: request
                .task_contract
                .clone()
                .expect("child fixture task contract"),
            workspace: AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::ReadOnly,
                root_workspace: request.environment.workspace.clone(),
                base_commit: "a".repeat(40),
                worktree_path: None,
                root_branch: None,
                branch: None,
                allowed_paths: Vec::new(),
                owner_token: None,
            },
            tool_policy: request.tool_policy.clone(),
            limits: request.limits,
            deadline_unix_ms: request.deadline_unix_ms,
            expected_artifact: "fixture result".to_owned(),
        });
    }
    request
}

fn model_request(request: &RunRequest, attempt: u32) -> ModelRequest {
    ModelRequest {
        run_id: request.run_id.clone().expect("fixture run id"),
        parent_run_id: request.parent_run_id.clone(),
        actor: request.actor,
        model: request.model.clone(),
        system_prompt: request.system_prompt.clone(),
        messages: vec![ModelMessage::User {
            content: request
                .task_contract
                .as_ref()
                .expect("fixture task contract")
                .definition
                .model_message(),
        }],
        tools: Vec::new(),
        reasoning_effort: request.reasoning_effort,
        max_output_tokens: request.max_output_tokens,
        streaming: request.streaming,
        request_number: 1,
        attempt,
    }
}

fn accounting(actor: AgentActorKind, started: u64, retries: u64, sealed: bool) -> ModelAccounting {
    let actor_accounting = ActorRequestAccounting {
        started,
        completed: started,
        in_flight: 0,
        retries,
    };
    let mut accounting = ModelAccounting {
        complete: true,
        usage_complete: true,
        sealed,
        ..ModelAccounting::default()
    };
    match actor {
        AgentActorKind::Root => accounting.root = actor_accounting,
        AgentActorKind::Child => accounting.child = actor_accounting,
    }
    accounting
}

fn stored(
    run_id: &RunId,
    parent_run_id: Option<RunId>,
    event_id: &str,
    sequence: u64,
    event: RuntimeEventKind,
) -> StoredRuntimeEvent {
    StoredRuntimeEvent {
        schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
        run_id: run_id.clone(),
        parent_run_id,
        event_id: RuntimeEventId(event_id.to_owned()),
        sequence,
        occurred_at_unix_ms: sequence,
        event,
    }
}

fn prompt_ledger_fixture() -> Vec<StoredRuntimeEvent> {
    let root_id = RunId::from("fixture-root-run");
    let child_id = RunId::from("fixture-child-run");
    let root_actor = AgentActor {
        kind: AgentActorKind::Root,
        depth: 0,
    };
    let child_actor = AgentActor {
        kind: AgentActorKind::Child,
        depth: 1,
    };
    let root_request = request(
        &root_id,
        None,
        root_actor,
        "修复目标代码并验证结果。",
        "当前角色：根 Agent。",
    );
    let mut child_request = request(
        &child_id,
        Some(root_id.clone()),
        child_actor,
        "只读分析目标代码。",
        "当前角色：只读子 Agent。",
    );
    child_request.limits.max_model_requests = 0;
    child_request.context_policy.hard_input_tokens = 12_000;
    let initial_model_request = model_request(&root_request, 0);
    let retry_model_request = model_request(&root_request, 1);
    let root_attempt_0 = AttemptId("root-model-attempt-0".to_owned());
    let root_attempt_1 = AttemptId("root-model-attempt-1".to_owned());

    vec![
        stored(
            &root_id,
            None,
            "run_created",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request),
            },
        ),
        stored(
            &root_id,
            None,
            "root-model-prepared-0",
            2,
            RuntimeEventKind::ModelRequestPrepared {
                attempt_id: root_attempt_0.clone(),
                request: Box::new(initial_model_request),
            },
        ),
        stored(
            &root_id,
            None,
            "root-model-in-flight-0",
            3,
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: root_attempt_0.clone(),
            },
        ),
        stored(
            &root_id,
            None,
            "root-model-failed-0",
            4,
            RuntimeEventKind::ModelRequestFailed {
                attempt_id: root_attempt_0,
                failure: ModelAttemptFailure {
                    code: "timeout".to_owned(),
                    category: ModelErrorCategory::Timeout,
                    message: "首次模型请求超时。".to_owned(),
                    retryable: true,
                    retry_safe: true,
                    actionable_output: false,
                    response: ModelResponseEvidence::default(),
                },
                accounting: Box::new(accounting(AgentActorKind::Root, 1, 0, false)),
                retry: ModelRetryDecision::Retry {
                    prepared: PreparedModelRetry {
                        attempt_id: root_attempt_1.clone(),
                        request: Box::new(retry_model_request),
                    },
                },
            },
        ),
        stored(
            &root_id,
            None,
            "root-model-in-flight-1",
            5,
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: root_attempt_1.clone(),
            },
        ),
        stored(
            &root_id,
            None,
            "root-model-failed-1",
            6,
            RuntimeEventKind::ModelRequestFailed {
                attempt_id: root_attempt_1,
                failure: ModelAttemptFailure {
                    code: "timeout".to_owned(),
                    category: ModelErrorCategory::Timeout,
                    message: "模型重试仍然超时。".to_owned(),
                    retryable: true,
                    retry_safe: true,
                    actionable_output: false,
                    response: ModelResponseEvidence::default(),
                },
                accounting: Box::new(accounting(AgentActorKind::Root, 2, 1, false)),
                retry: ModelRetryDecision::Stop {
                    reason: ModelRetryStopReason::RetryLimitReached,
                },
            },
        ),
        stored(
            &root_id,
            None,
            "terminal",
            7,
            RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: root_id.clone(),
                    parent_run_id: None,
                    terminal: TerminalState::Failed {
                        failure: codewhale_protocol::agent_runtime::RuntimeFailure::Model {
                            code: "timeout".to_owned(),
                            category: ModelErrorCategory::Timeout,
                            message: "模型重试耗尽。".to_owned(),
                            retryable: true,
                        },
                    },
                    accounting: accounting(AgentActorKind::Root, 2, 1, true),
                    runtime_model_requests: 2,
                    runtime_retries: 1,
                    tool_calls: 0,
                    details: AgentResultDetails::default(),
                }),
            },
        ),
        stored(
            &child_id,
            Some(root_id.clone()),
            "run_created",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(child_request),
            },
        ),
        stored(
            &child_id,
            Some(root_id.clone()),
            "child-compaction-committed",
            2,
            RuntimeEventKind::ContextCompactionCommitted {
                projection: Box::new(ContextProjection {
                    source_entry_count: 8,
                    source_projection_sha256: "fixture-source-projection".to_owned(),
                    selected_entry_indices: vec![0, 7],
                    messages: vec![
                        ModelMessage::User {
                            content: "只读分析目标代码。".to_owned(),
                        },
                        ModelMessage::User {
                            content: "保留的最近约束。".to_owned(),
                        },
                    ],
                }),
                tools: vec![ToolDefinition {
                    name: "read_file".to_owned(),
                    description: "读取文件。".to_owned(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }),
                }],
                accounting: Box::new(ModelAccounting::default()),
                before_tokens: 12_000,
                after_tokens: 9_000,
            },
        ),
        stored(
            &child_id,
            Some(root_id.clone()),
            "terminal",
            3,
            RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: child_id.clone(),
                    parent_run_id: Some(root_id.clone()),
                    terminal: TerminalState::Failed {
                        failure: RuntimeFailure::ModelRequestBudgetExceeded { limit: 0 },
                    },
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 0,
                    runtime_retries: 0,
                    tool_calls: 0,
                    details: AgentResultDetails::default(),
                }),
            },
        ),
    ]
}

fn assert_retry_only_advances_attempt(initial: &ModelRequest, retry: &ModelRequest) {
    assert_eq!(retry.request_number, initial.request_number);
    assert_eq!(retry.attempt, initial.attempt + 1);

    let mut normalized_retry = retry.clone();
    normalized_retry.attempt = initial.attempt;
    assert_eq!(&normalized_retry, initial);
}

#[test]
fn runtime_event_v10_prompt_ledger_fixture_matches_rust_contract() {
    let fixture = prompt_ledger_fixture();
    let fixture_json =
        serde_json::to_vec(&fixture).expect("RuntimeEvent v10 fixture must serialize");
    let fixture_wire = std::str::from_utf8(&fixture_json).expect("fixture JSON must be UTF-8");
    for deleted_kind in [
        "context_compaction_prepared",
        "context_compaction_in_flight",
        "context_compaction_attempt_failed",
    ] {
        assert!(
            !fixture_wire.contains(deleted_kind),
            "v10 fixture must not retain deleted event kind {deleted_kind}"
        );
    }
    let events: Vec<StoredRuntimeEvent> = serde_json::from_slice(&fixture_json)
        .expect("fixture must use the Rust RuntimeEvent v10 schema");
    assert!(!events.is_empty());

    for event in &events {
        assert_eq!(
            event.schema_version, AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            "fixture must remain pinned to the current writer schema"
        );
        let normalized =
            serde_json::to_vec(event).expect("StoredRuntimeEvent must serialize canonically");
        let decoded: StoredRuntimeEvent =
            serde_json::from_slice(&normalized).expect("canonical event must deserialize");
        assert_eq!(
            &decoded, event,
            "canonical serde round-trip changed the event"
        );
    }

    let mut runs: HashMap<&RunId, Vec<&StoredRuntimeEvent>> = HashMap::new();
    for event in &events {
        runs.entry(&event.run_id).or_default().push(event);
    }
    assert_eq!(runs.len(), 2, "fixture must contain one root and one child");

    let root_id = RunId::from("fixture-root-run");
    let child_id = RunId::from("fixture-child-run");
    let root = runs.get(&root_id).expect("root run must exist");
    let child = runs.get(&child_id).expect("child run must exist");

    for run in [root, child] {
        let sequences = run.iter().map(|event| event.sequence).collect::<Vec<_>>();
        assert_eq!(
            sequences,
            (1..=run.len() as u64).collect::<Vec<_>>(),
            "each run must have a gap-free sequence"
        );
        assert!(
            matches!(
                run.last().map(|event| &event.event),
                Some(RuntimeEventKind::Terminal { .. })
            ),
            "terminal must be the last event in each run"
        );
        assert_eq!(
            run.iter()
                .filter(|event| matches!(event.event, RuntimeEventKind::Terminal { .. }))
                .count(),
            1,
            "each run must contain exactly one terminal"
        );
    }

    let root_base_prompt = match &root[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.run_id.as_ref(), Some(&root_id));
            assert_eq!(request.parent_run_id, None);
            assert_eq!(request.actor.kind, AgentActorKind::Root);
            assert_eq!(request.actor.depth, 0);
            &request.system_prompt
        }
        event => panic!("root sequence 1 must be RunCreated, got {event:?}"),
    };
    let child_base_prompt = match &child[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.run_id.as_ref(), Some(&child_id));
            assert_eq!(request.parent_run_id.as_ref(), Some(&root_id));
            assert_eq!(request.actor.kind, AgentActorKind::Child);
            assert_eq!(request.actor.depth, 1);
            &request.system_prompt
        }
        event => panic!("child sequence 1 must be RunCreated, got {event:?}"),
    };
    assert_eq!(
        stable_prefix(root_base_prompt),
        stable_prefix(child_base_prompt),
        "root and child base prompts must share the Agent stable prefix"
    );
    assert!(root.iter().all(|event| event.parent_run_id.is_none()));
    assert!(
        child
            .iter()
            .all(|event| event.parent_run_id.as_ref() == Some(&root_id))
    );

    let (agent_prepared_sequence, agent_attempt, agent_request) = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestPrepared {
                attempt_id,
                request,
            } => Some((stored.sequence, attempt_id, request.as_ref())),
            _ => None,
        })
        .expect("fixture must contain an ordinary prepared model request");
    let agent_in_flight_sequence = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestInFlight { attempt_id }
                if attempt_id == agent_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("ordinary prepared request must become in-flight");
    let (agent_failed_sequence, agent_retry_attempt, agent_retry_request) = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestFailed {
                attempt_id,
                retry: ModelRetryDecision::Retry { prepared },
                ..
            } if attempt_id == agent_attempt => Some((
                stored.sequence,
                &prepared.attempt_id,
                prepared.request.as_ref(),
            )),
            _ => None,
        })
        .expect("ordinary failure must atomically prepare a retry");
    assert!(agent_prepared_sequence < agent_in_flight_sequence);
    assert!(agent_in_flight_sequence < agent_failed_sequence);
    assert_retry_only_advances_attempt(agent_request, agent_retry_request);
    let agent_retry_in_flight_sequence = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestInFlight { attempt_id }
                if attempt_id == agent_retry_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("ordinary prepared retry must become in-flight");
    assert!(agent_failed_sequence < agent_retry_in_flight_sequence);

    assert_eq!(
        child
            .iter()
            .filter(|stored| matches!(
                stored.event,
                RuntimeEventKind::ContextCompactionCommitted { .. }
            ))
            .count(),
        1,
        "deterministic compaction must commit one canonical ledger event"
    );
    assert!(
        child.iter().all(|stored| !matches!(
            stored.event,
            RuntimeEventKind::ModelRequestPrepared { .. }
                | RuntimeEventKind::ModelRequestInFlight { .. }
                | RuntimeEventKind::ModelRequestFailed { .. }
                | RuntimeEventKind::ModelResponseCommitted { .. }
        )),
        "deterministic compaction must not manufacture a model request lifecycle"
    );

    let compaction = child
        .iter()
        .find(|stored| {
            matches!(
                stored.event,
                RuntimeEventKind::ContextCompactionCommitted { .. }
            )
        })
        .expect("fixture must contain a committed deterministic compaction");
    let encoded = serde_json::to_value(compaction).expect("compaction event must serialize");
    assert_eq!(
        encoded["schema_version"],
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION
    );
    assert_eq!(encoded["event"]["kind"], "context_compaction_committed");
    assert!(
        encoded["event"].get("attempt_id").is_none()
            && encoded["event"].get("request").is_none()
            && encoded["event"].get("output").is_none(),
        "v10 compaction must not retain the deleted model-summary protocol"
    );
    match &compaction.event {
        RuntimeEventKind::ContextCompactionCommitted {
            projection,
            tools,
            accounting,
            before_tokens,
            after_tokens,
            ..
        } => {
            assert_eq!(projection.source_entry_count, 8);
            assert_eq!(
                projection.source_projection_sha256,
                "fixture-source-projection"
            );
            assert_eq!(projection.selected_entry_indices, [0, 7]);
            assert_eq!(projection.messages.len(), 2);
            assert_eq!(
                tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<Vec<_>>(),
                ["read_file"]
            );
            assert_eq!(accounting.total_started(), 0);
            assert_eq!(accounting.total_completed(), 0);
            assert_eq!(accounting.runtime_retries, 0);
            assert_eq!((*before_tokens, *after_tokens), (12_000, 9_000));
        }
        event => panic!("expected ContextCompactionCommitted, got {event:?}"),
    }

    match &child.last().expect("child terminal").event {
        RuntimeEventKind::Terminal { outcome } => {
            assert_eq!(
                outcome.terminal,
                TerminalState::Failed {
                    failure: RuntimeFailure::ModelRequestBudgetExceeded { limit: 0 }
                }
            );
            assert_eq!(outcome.runtime_model_requests, 0);
            assert_eq!(outcome.runtime_retries, 0);
            assert_eq!(outcome.accounting.total_started(), 0);
        }
        event => panic!("child ledger must end in Terminal, got {event:?}"),
    }
}
