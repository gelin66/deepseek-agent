use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use codewhale_context::compaction::{ContextInput, effective_context};
use codewhale_protocol::agent_runtime::{
    AGENT_RUNTIME_EVENT_SCHEMA_VERSION, AgentActor, AgentActorKind, AgentTask, AgentTaskId,
    AgentWorkspaceAccess, AgentWorkspaceAssignment, InheritedRunFacts, ReasoningEffort, RunLimits,
    ToolPolicy,
};
use codewhale_protocol::run_api::{
    PendingCreationKind, RunCommand, RunProductControls, StartRunCommand,
};
use codewhale_protocol::task::{
    AcceptanceId, AcceptanceSatisfaction, CompletionCandidate, CompletionCandidateId,
    CompletionDecision, EvidenceLineage, EvidenceReceipt, EvidenceReceiptId, TaskAcceptance,
    TaskContract, TaskDefinition, TaskGenerationId, VerificationId, VerifierEvidencePolicy,
    VerifierObservation, VerifierPlan, VerifierSpec, VerifierStep, VerifierVerdict,
    WorkspaceRevision, WorkspaceState,
};
use codewhale_runtime::{
    ActorRequestAccounting, AgentOutcome, AgentResultDetails, AttemptId, CommandId, CreatedRun,
    DurableActionState, InMemoryRunStore, ModelAccounting, ModelFinishReason, ModelOutput,
    ModelRequest, ModelToolCall, OperationId, PendingRuntimeEvent, RootRunRecord, RunId, RunLease,
    RunReplay, RunRequest, RunSnapshot, RunStore, RunStoreError, RuntimeEventId, RuntimeEventKind,
    RuntimeFailure, StoredRuntimeEvent, TerminalState, ToolArguments, ToolArtifact,
    ToolArtifactStatus, ToolDefinition, ToolEvidence, ToolEvidenceStatus, ToolInvocation,
    ToolInvocationStatus, ToolOperationStatus, ToolOutcome, ToolRetryDisposition,
    ToolSideEffectStatus, ToolTransportStatus, Usage, VerificationArtifactPayload, WorkspaceAccess,
    WriteExecutionMode, WriterIntegrationStatus, reduce_events,
};
use codewhale_state::StateStore;
use rusqlite::{Connection, params};

fn temp_state_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "codewhale_run_store_{label}_{}_{}.db",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ))
}

fn request(run_id: &str, workspace: &str) -> RunRequest {
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId::from(run_id),
            definition: TaskDefinition::host("实现功能"),
        },
        "你是编码 Agent",
    );
    request.run_id = Some(RunId::from(run_id));
    request.environment.workspace = workspace.to_owned();
    request
}

fn read_only_child_request(
    child_run_id: &str,
    parent_run_id: RunId,
    workspace: &str,
) -> RunRequest {
    let mut request = request(child_run_id, workspace);
    let task = AgentTask {
        task_id: AgentTaskId::from(format!("task-{child_run_id}")),
        root_run_id: parent_run_id.clone(),
        parent_run_id: parent_run_id.clone(),
        child_run_id: RunId::from(child_run_id),
        call_id: format!("call-{child_run_id}"),
        role: "researcher".to_owned(),
        task_contract: request.task_contract.clone().expect("child task contract"),
        workspace: AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::ReadOnly,
            root_workspace: workspace.to_owned(),
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
        expected_artifact: "structured_handoff".to_owned(),
    };
    request.parent_run_id = Some(parent_run_id);
    request.actor = AgentActor {
        kind: AgentActorKind::Child,
        depth: 1,
    };
    request.agent_task = Some(task);
    request
}

fn known_workspace(generation: u64, revision: &str) -> WorkspaceState {
    WorkspaceState {
        generation,
        revision: WorkspaceRevision::Known {
            sha256: revision.to_owned(),
        },
    }
}

fn writer_parity_verifier() -> VerifierSpec {
    VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: serde_json::json!({"args": ["--locked"]}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "writer-parity-test".to_owned(),
                program: "cargo".to_owned(),
                args: vec!["test".to_owned(), "--locked".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 600_000,
            }],
        },
    }
}

fn writer_parity_task() -> AgentTask {
    let child_run_id = RunId::from("writer-parity-child");
    AgentTask {
        task_id: AgentTaskId::from("writer-parity-task"),
        root_run_id: RunId::from("writer-parity-root"),
        parent_run_id: RunId::from("writer-parity-root"),
        child_run_id: child_run_id.clone(),
        call_id: "writer-parity-call".to_owned(),
        role: "writer".to_owned(),
        task_contract: TaskContract {
            generation_id: TaskGenerationId::from(child_run_id.0),
            definition: TaskDefinition {
                objective: "修改一个冻结文件".to_owned(),
                constraints: Vec::new(),
                non_goals: Vec::new(),
                acceptance: vec![TaskAcceptance::Verifier {
                    id: AcceptanceId::from("writer-parity-verifier"),
                    description: "冻结的 writer parity verifier 必须通过".to_owned(),
                    evidence_policy: VerifierEvidencePolicy::LatestPass,
                    verifier: writer_parity_verifier(),
                }],
            },
        },
        workspace: AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::IsolatedWrite,
            root_workspace: "/tmp/writer-parity-root".to_owned(),
            base_commit: "a".repeat(40),
            worktree_path: Some("/tmp/writer-parity-worktree".to_owned()),
            root_branch: Some("deepseek-agent".to_owned()),
            branch: Some("codewhale/writer/writer-parity-task".to_owned()),
            allowed_paths: vec!["src/lib.rs".to_owned()],
            owner_token: Some("writer-parity-owner".to_owned()),
        },
        tool_policy: ToolPolicy::default(),
        limits: RunLimits::default(),
        deadline_unix_ms: None,
        expected_artifact: "sealed_commit".to_owned(),
    }
}

fn writer_parity_outcome(integrated: bool) -> AgentOutcome {
    let task = writer_parity_task();
    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from("writer-parity-receipt"),
        generation_id: task.task_contract.generation_id.clone(),
        acceptance_id: AcceptanceId::from("writer-parity-verifier"),
        verification_id: VerificationId::from("writer-parity-verification"),
        verifier: writer_parity_verifier(),
        workspace_state: known_workspace(1, "writer-dirty"),
        artifact_ids: vec!["writer-parity-artifact".to_owned()],
        lineage: EvidenceLineage::LatestPass,
    };
    let integration = if integrated {
        WriterIntegrationStatus::Integrated {
            integration_id: OperationId("writer-parity-integration".to_owned()),
            writer_commit: "b".repeat(40),
            root_workspace_state: known_workspace(2, "root-integrated"),
        }
    } else {
        WriterIntegrationStatus::AwaitingHost
    };
    AgentOutcome {
        run_id: task.child_run_id.clone(),
        parent_run_id: Some(task.parent_run_id.clone()),
        terminal: TerminalState::Completed {
            message: "writer 完成".to_owned(),
            decision: CompletionDecision {
                candidate_id: CompletionCandidateId::from("writer-parity-candidate"),
                generation_id: task.task_contract.generation_id.clone(),
                workspace_state: known_workspace(1, "writer-dirty"),
                satisfied: vec![AcceptanceSatisfaction::Evidence {
                    acceptance_id: receipt.acceptance_id.clone(),
                    receipt_id: receipt.id.clone(),
                }],
            },
        },
        accounting: ModelAccounting::default(),
        runtime_model_requests: 1,
        runtime_retries: 0,
        tool_calls: 1,
        details: AgentResultDetails {
            summary: "修改并封存一个文件".to_owned(),
            evidence: vec![receipt],
            changed_files: vec!["src/lib.rs".to_owned()],
            checks: Vec::new(),
            unresolved: Vec::new(),
            artifacts: Vec::new(),
            workspace: Some(task.workspace),
            workspace_state: Some(known_workspace(2, "writer-sealed")),
            base_commit: Some("a".repeat(40)),
            final_commit: Some("b".repeat(40)),
            diff_sha256: Some("d".repeat(64)),
            integration,
        },
    }
}

fn creation_intent(workspace: &str) -> codewhale_runtime::CreationIntent {
    let command = StartRunCommand {
        task: TaskDefinition::host("实现功能"),
        workspace: workspace.to_owned(),
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
        workspace: workspace.to_owned(),
        source_run_id: None,
        command: RunCommand::Start(command),
    }
}

fn user_event(id: &str, content: &str) -> PendingRuntimeEvent {
    PendingRuntimeEvent {
        event_id: RuntimeEventId(id.to_owned()),
        event: RuntimeEventKind::SteerQueued {
            command_id: CommandId::from(format!("command-{id}")),
            content: content.to_owned(),
        },
    }
}

fn content_delta(id: &str, attempt_id: &AttemptId, content: &str) -> PendingRuntimeEvent {
    PendingRuntimeEvent {
        event_id: RuntimeEventId(id.to_owned()),
        event: RuntimeEventKind::ContentDelta {
            attempt_id: attempt_id.clone(),
            index: 0,
            delta: content.to_owned(),
        },
    }
}

fn model_request_for_snapshot(
    run_id: RunId,
    snapshot: &RunSnapshot,
    tools: Vec<ToolDefinition>,
) -> ModelRequest {
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
    .expect("build canonical model request projection");
    ModelRequest {
        run_id,
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools,
        reasoning_effort: snapshot.request.reasoning_effort,
        max_output_tokens: snapshot.request.max_output_tokens,
        streaming: snapshot.request.streaming,
        request_number: snapshot.local_turns.saturating_add(1),
        attempt: 0,
    }
}

fn model_request(created: &CreatedRun) -> ModelRequest {
    model_request_for_snapshot(
        created.lease.run_id.clone(),
        &created.replay.snapshot,
        Vec::new(),
    )
}

fn continuation_request(
    mut request: RunRequest,
    source: &RunReplay,
    source_run_id: RunId,
) -> RunRequest {
    request.continued_from_run_id = Some(source_run_id);
    request.transcript = source.snapshot.transcript.clone();
    request.context_projection = source.snapshot.context_projection.clone();
    request.inherited_facts = Some(InheritedRunFacts {
        workspace_state: source.snapshot.workspace_state.clone(),
        last_completion_rejection: source.snapshot.last_completion_rejection.clone(),
        last_host_verification_failure: source.snapshot.last_host_verification_failure.clone(),
    });
    request
}

fn terminal_event(run_id: &RunId) -> PendingRuntimeEvent {
    PendingRuntimeEvent::terminal(AgentOutcome {
        run_id: run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::Blocked {
            reason: "测试终态".to_owned(),
        },
        accounting: ModelAccounting::default(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        details: Default::default(),
    })
}

fn assert_canonical_event_eq(left: &StoredRuntimeEvent, right: &StoredRuntimeEvent) {
    assert_eq!(left.schema_version, right.schema_version);
    assert_eq!(left.run_id, right.run_id);
    assert_eq!(left.parent_run_id, right.parent_run_id);
    assert_eq!(left.event_id, right.event_id);
    assert_eq!(left.sequence, right.sequence);
    assert_eq!(left.event, right.event);
    // Store implementations own wall-clock assignment, so parity deliberately
    // excludes only that non-semantic field. SQLite reopen below must preserve
    // it exactly.
    assert!(left.occurred_at_unix_ms > 0);
    assert!(right.occurred_at_unix_ms > 0);
}

fn assert_canonical_replay_eq(left: &RunReplay, right: &RunReplay) {
    assert_eq!(left.snapshot, right.snapshot);
    assert_eq!(left.events.len(), right.events.len());
    for (left_event, right_event) in left.events.iter().zip(&right.events) {
        assert_canonical_event_eq(left_event, right_event);
    }
}

async fn append_to_both(
    sqlite: &StateStore,
    sqlite_lease: &RunLease,
    memory: &InMemoryRunStore,
    memory_lease: &RunLease,
    pending: PendingRuntimeEvent,
) {
    let sqlite_event = sqlite
        .append(sqlite_lease, pending.clone())
        .await
        .expect("append lifecycle event to sqlite");
    let memory_event = memory
        .append(memory_lease, pending)
        .await
        .expect("append lifecycle event to memory");
    assert_canonical_event_eq(&sqlite_event, &memory_event);
}

fn root_list_semantics(
    records: Vec<RootRunRecord>,
) -> Vec<(RunId, Option<RunId>, String, u64, bool)> {
    records
        .into_iter()
        .map(|record| {
            assert!(record.created_at_unix_ms > 0);
            assert!(record.updated_at_unix_ms >= record.created_at_unix_ms);
            (
                record.run_id,
                record.continued_from_run_id,
                record.workspace,
                record.last_sequence,
                record.terminal,
            )
        })
        .collect()
}

fn v5_model_request(request: &RunRequest) -> ModelRequest {
    let run_id = request.run_id.clone().expect("v5 fixture run id");
    let snapshot = reduce_events(&[v5_event(
        &run_id,
        "model-request-source",
        1,
        RuntimeEventKind::RunCreated {
            request: Box::new(request.clone()),
        },
    )])
    .expect("reduce v5 model request source");
    model_request_for_snapshot(run_id, &snapshot, Vec::new())
}

fn v5_event(
    run_id: &RunId,
    event_id: &str,
    sequence: u64,
    event: RuntimeEventKind,
) -> StoredRuntimeEvent {
    StoredRuntimeEvent {
        // This fixture isolates the State schema v5 -> current migration.
        // RuntimeEvent readers intentionally accept v6 only after the
        // incompatible budget-failure taxonomy cutover.
        schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
        run_id: run_id.clone(),
        parent_run_id: None,
        event_id: RuntimeEventId(event_id.to_owned()),
        sequence,
        occurred_at_unix_ms: sequence,
        event,
    }
}

fn create_v5_run_store(path: &PathBuf) -> Connection {
    let conn = Connection::open(path).expect("open v5 fixture");
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        CREATE TABLE agent_runs (
            run_id TEXT PRIMARY KEY NOT NULL,
            parent_run_id TEXT,
            workspace TEXT NOT NULL,
            last_sequence INTEGER NOT NULL CHECK(last_sequence >= 1),
            terminal INTEGER NOT NULL DEFAULT 0 CHECK(terminal IN (0, 1)),
            execution_epoch INTEGER NOT NULL CHECK(execution_epoch >= 1),
            lease_owner_id TEXT,
            lease_owner_pid INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL,
            CHECK((lease_owner_id IS NULL) = (lease_owner_pid IS NULL)),
            CHECK(lease_owner_pid IS NULL OR lease_owner_pid > 0),
            CHECK(terminal = 0 OR lease_owner_id IS NULL)
        );
        CREATE INDEX idx_agent_runs_workspace_updated
            ON agent_runs(workspace, terminal, updated_at_unix_ms DESC);
        CREATE TABLE agent_run_events (
            run_id TEXT NOT NULL,
            sequence INTEGER NOT NULL CHECK(sequence >= 1),
            event_id TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            occurred_at_unix_ms INTEGER NOT NULL,
            terminal INTEGER NOT NULL DEFAULT 0 CHECK(terminal IN (0, 1)),
            event_json TEXT NOT NULL,
            PRIMARY KEY(run_id, sequence),
            UNIQUE(run_id, event_id),
            FOREIGN KEY(run_id) REFERENCES agent_runs(run_id) ON DELETE CASCADE
        );
        CREATE UNIQUE INDEX idx_agent_run_one_terminal
            ON agent_run_events(run_id) WHERE terminal = 1;
        CREATE TABLE agent_run_snapshots (
            run_id TEXT PRIMARY KEY NOT NULL,
            last_sequence INTEGER NOT NULL CHECK(last_sequence >= 1),
            snapshot_json TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES agent_runs(run_id) ON DELETE CASCADE
        );
        PRAGMA user_version = 5;
        "#,
    )
    .expect("create v5 run store schema");
    conn
}

fn insert_v5_run(
    conn: &Connection,
    run_id: &str,
    state: DurableActionState,
    include_projection_neutral_delta: bool,
) {
    let request = request(run_id, "/tmp/v5-migration");
    let run_id = request.run_id.clone().expect("fixture run id");
    let attempt_id = AttemptId(format!("{run_id}-attempt"));
    let mut events = vec![v5_event(
        &run_id,
        "run_created",
        1,
        RuntimeEventKind::RunCreated {
            request: Box::new(request.clone()),
        },
    )];
    events.push(v5_event(
        &run_id,
        "model-prepared",
        2,
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id: attempt_id.clone(),
            request: Box::new(v5_model_request(&request)),
        },
    ));
    if state == DurableActionState::InFlight {
        events.push(v5_event(
            &run_id,
            "model-in-flight",
            3,
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: attempt_id.clone(),
            },
        ));
    }

    let materialized_event_count = events.len();
    if include_projection_neutral_delta {
        assert_eq!(state, DurableActionState::InFlight);
        events.push(v5_event(
            &run_id,
            "content-delta",
            4,
            RuntimeEventKind::ContentDelta {
                attempt_id,
                index: 1,
                delta: "增量".to_owned(),
            },
        ));
    }
    let canonical = reduce_events(&events).expect("reduce canonical v5 fixture");
    let materialized =
        reduce_events(&events[..materialized_event_count]).expect("reduce materialized v5 fixture");

    conn.execute(
        r#"
        INSERT INTO agent_runs(
            run_id, parent_run_id, workspace, last_sequence, terminal,
            execution_epoch, lease_owner_id, lease_owner_pid,
            created_at_unix_ms, updated_at_unix_ms
        ) VALUES (?1, NULL, ?2, ?3, 0, 1, NULL, NULL, 1, 1)
        "#,
        params![
            run_id.0,
            canonical.request.environment.workspace,
            i64::try_from(canonical.last_sequence).expect("fixture sequence fits SQLite")
        ],
    )
    .expect("insert v5 run");
    for event in &events {
        conn.execute(
            r#"
            INSERT INTO agent_run_events(
                run_id, sequence, event_id, schema_version,
                occurred_at_unix_ms, terminal, event_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                event.run_id.0,
                i64::try_from(event.sequence).expect("fixture sequence fits SQLite"),
                event.event_id.0,
                i64::from(event.schema_version),
                i64::try_from(event.occurred_at_unix_ms).expect("fixture timestamp fits SQLite"),
                i64::from(event.event.is_terminal()),
                serde_json::to_string(event).expect("serialize v5 event"),
            ],
        )
        .expect("insert v5 event");
    }
    conn.execute(
        "INSERT INTO agent_run_snapshots(run_id, last_sequence, snapshot_json) VALUES (?1, ?2, ?3)",
        params![
            run_id.0,
            i64::try_from(canonical.last_sequence).expect("fixture sequence fits SQLite"),
            serde_json::to_string(&materialized).expect("serialize v5 snapshot"),
        ],
    )
    .expect("insert v5 snapshot");
}

async fn persist_committed_catalog_run(path: &std::path::Path, run_id: &str) -> RunId {
    let store = StateStore::open(Some(path.to_path_buf())).expect("open current state store");
    let created = store
        .create(request(run_id, "/tmp/v10-catalog-migration"))
        .await
        .expect("create catalog migration run");
    let attempt_id = AttemptId("catalog-migration-attempt".to_owned());
    let tools = vec![ToolDefinition {
        name: "read".to_owned(),
        description: "read".to_owned(),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    let prepared = model_request_for_snapshot(
        created.lease.run_id.clone(),
        &created.replay.snapshot,
        tools,
    );
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("catalog-migration-prepared".to_owned()),
                event: RuntimeEventKind::ModelRequestPrepared {
                    attempt_id: attempt_id.clone(),
                    request: Box::new(prepared),
                },
            },
        )
        .await
        .expect("append catalog request");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("catalog-migration-in-flight".to_owned()),
                event: RuntimeEventKind::ModelRequestInFlight {
                    attempt_id: attempt_id.clone(),
                },
            },
        )
        .await
        .expect("append catalog in-flight");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("catalog-migration-committed".to_owned()),
                event: RuntimeEventKind::ModelResponseCommitted {
                    attempt_id,
                    output: Box::new(ModelOutput {
                        content: "已提交响应".to_owned(),
                        reasoning_content: None,
                        tool_calls: Vec::new(),
                        finish_reason: ModelFinishReason::Stop,
                        usage: Usage::default(),
                    }),
                    accounting: Box::new(ModelAccounting::default()),
                },
            },
        )
        .await
        .expect("append committed catalog response");
    created.lease.run_id
}

fn downgrade_catalog_snapshot_to_v9(path: &PathBuf, corrupt: bool) {
    let conn = Connection::open(path).expect("open current database for v9 downgrade");
    let snapshot_json: String = conn
        .query_row(
            "SELECT snapshot_json FROM agent_run_snapshots LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("read current materialized snapshot");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("decode materialized snapshot");
    let object = snapshot.as_object_mut().expect("snapshot JSON object");
    object.remove("last_model_advertised_tool_names");
    if corrupt {
        object.insert(
            "runtime_model_requests".to_owned(),
            serde_json::Value::from(99),
        );
    }
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = ?1",
        [serde_json::to_string(&snapshot).expect("encode v9 snapshot")],
    )
    .expect("write v9 materialized snapshot");
    conn.pragma_update(None, "user_version", 9)
        .expect("downgrade schema marker to v9");
}

#[tokio::test]
async fn sqlite_replay_matches_memory_and_survives_reopen() {
    let path = temp_state_path("replay_parity");
    let sqlite = StateStore::open(Some(path.clone())).expect("open sqlite store");
    let memory = InMemoryRunStore::default();
    let request = request("parity-run", "/tmp/parity-workspace");

    let sqlite_created = sqlite.create(request.clone()).await.expect("sqlite create");
    let memory_created = memory.create(request).await.expect("memory create");
    assert_canonical_event_eq(&sqlite_created.created, &memory_created.created);

    let attempt_id = AttemptId("parity-model-attempt".to_owned());
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("model-prepared".to_owned()),
            event: RuntimeEventKind::ModelRequestPrepared {
                attempt_id: attempt_id.clone(),
                request: Box::new(model_request(&sqlite_created)),
            },
        },
    )
    .await;
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("model-in-flight".to_owned()),
            event: RuntimeEventKind::ModelRequestInFlight {
                attempt_id: attempt_id.clone(),
            },
        },
    )
    .await;

    let tool_call = ModelToolCall {
        id: "call-apply-patch".to_owned(),
        name: "apply_patch".to_owned(),
        arguments: ToolArguments::parse(r#"{"patch":"*** Begin Patch"}"#),
    };
    let usage = Usage {
        input_tokens: 120,
        output_tokens: 35,
        cache_hit_tokens: 80,
        cache_miss_tokens: 40,
        cache_write_tokens: 0,
        reasoning_tokens: 17,
        reasoning_replay_tokens: 5,
    };
    let output = ModelOutput {
        content: String::new(),
        reasoning_content: Some("先修改目标文件，再用测试验证".to_owned()),
        tool_calls: vec![tool_call.clone()],
        finish_reason: ModelFinishReason::ToolCalls,
        usage,
    };
    let accounting = ModelAccounting {
        hard_request_limit: Some(8),
        root: ActorRequestAccounting {
            started: 1,
            completed: 1,
            in_flight: 0,
            retries: 0,
        },
        complete: true,
        usage_complete: true,
        usage_responses: 1,
        usage,
        cost_nanousd: 42_000,
        cost_nanocny: 300_000,
        ..ModelAccounting::default()
    };
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("model-committed".to_owned()),
            event: RuntimeEventKind::ModelResponseCommitted {
                attempt_id,
                output: Box::new(output.clone()),
                accounting: Box::new(accounting.clone()),
            },
        },
    )
    .await;

    let operation_id = OperationId("parity-tool-operation".to_owned());
    let invocation = ToolInvocation {
        run_id: sqlite_created.lease.run_id.clone(),
        call_id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
    };
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("tool-prepared".to_owned()),
            event: RuntimeEventKind::ToolPrepared {
                operation_id: operation_id.clone(),
                invocation,
                workspace_access: codewhale_runtime::WorkspaceAccess::MayWrite,
            },
        },
    )
    .await;
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("tool-in-flight".to_owned()),
            event: RuntimeEventKind::ToolExecutionStarted {
                operation_id: operation_id.clone(),
            },
        },
    )
    .await;

    let tool_outcome = ToolOutcome {
        invocation: ToolInvocationStatus::Accepted,
        transport: ToolTransportStatus::Succeeded,
        operation: ToolOperationStatus::Succeeded,
        side_effect: ToolSideEffectStatus::Applied,
        retry: ToolRetryDisposition::NotNeeded,
        evidence: ToolEvidence {
            status: ToolEvidenceStatus::Produced,
            references: vec!["test://cargo/state-run-store".to_owned()],
        },
        artifacts: vec![ToolArtifact {
            id: "patch-receipt".to_owned(),
            status: ToolArtifactStatus::Available,
            sha256: Some("0123456789abcdef".to_owned()),
            media_type: Some("application/vnd.codewhale.patch+json".to_owned()),
            byte_len: Some(256),
            inline_content: None,
        }],
        workspace_revision: Some("workspace-revision-after-patch".to_owned()),
        verifier_observation: None,
        content: "补丁已应用并通过确定性验证".to_owned(),
        metadata: Some(serde_json::json!({"changed_files": 1})),
    };
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        PendingRuntimeEvent {
            event_id: RuntimeEventId("tool-committed".to_owned()),
            event: RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: tool_call.id,
                name: tool_call.name,
                outcome: Box::new(tool_outcome.clone()),
                workspace_state: Some(codewhale_runtime::WorkspaceState {
                    generation: 1,
                    revision: codewhale_runtime::WorkspaceRevision::Known {
                        sha256: "workspace-revision-after-patch".to_owned(),
                    },
                }),
            },
        },
    )
    .await;

    let terminal = PendingRuntimeEvent::terminal(AgentOutcome {
        run_id: sqlite_created.lease.run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::Blocked {
            reason: "测试终态".to_owned(),
        },
        accounting: accounting.clone(),
        runtime_model_requests: 1,
        runtime_retries: 0,
        tool_calls: 1,
        details: Default::default(),
    });
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        terminal,
    )
    .await;

    let sqlite_replay = sqlite
        .load(&sqlite_created.lease.run_id)
        .await
        .expect("sqlite load")
        .expect("sqlite run");
    let memory_replay = memory
        .load(&memory_created.lease.run_id)
        .await
        .expect("memory load")
        .expect("memory run");
    assert_canonical_replay_eq(&sqlite_replay, &memory_replay);
    assert_eq!(sqlite_replay.events.len(), 8);
    assert_eq!(sqlite_replay.snapshot.last_sequence, 8);
    assert_eq!(sqlite_replay.snapshot.usage, usage);
    assert_eq!(sqlite_replay.snapshot.accounting, accounting);
    assert_eq!(sqlite_replay.snapshot.last_model_output, Some(output));
    assert_eq!(sqlite_replay.snapshot.runtime_model_requests, 1);
    assert_eq!(sqlite_replay.snapshot.tool_calls, 1);
    assert!(sqlite_replay.snapshot.pending_model.is_none());
    assert!(sqlite_replay.snapshot.pending_tool.is_none());
    assert_eq!(
        sqlite_replay.snapshot.transcript.entries.last(),
        Some(&codewhale_runtime::TranscriptEntry::Tool {
            call_id: "call-apply-patch".to_owned(),
            name: "apply_patch".to_owned(),
            outcome: Box::new(tool_outcome),
        })
    );
    assert!(sqlite_replay.snapshot.terminal.is_some());
    assert_eq!(
        reduce_events(&sqlite_replay.events).expect("reduce sqlite event log"),
        sqlite_replay.snapshot
    );
    assert_eq!(
        reduce_events(&memory_replay.events).expect("reduce memory event log"),
        memory_replay.snapshot
    );

    drop(sqlite);
    let reopened = StateStore::open(Some(path)).expect("reopen sqlite store");
    let reopened_replay = reopened
        .load(&RunId::from("parity-run"))
        .await
        .expect("load reopened run")
        .expect("reopened run exists");
    assert_eq!(reopened_replay, sqlite_replay);
    assert_canonical_replay_eq(&reopened_replay, &memory_replay);
    assert_eq!(
        reduce_events(&reopened_replay.events).expect("reduce reopened sqlite event log"),
        reopened_replay.snapshot
    );
}

#[tokio::test]
async fn writer_lifecycle_replay_matches_memory_and_survives_sqlite_reopen() {
    let path = temp_state_path("writer_lifecycle_parity");
    let sqlite = StateStore::open(Some(path.clone())).expect("open writer SQLite store");
    let memory = InMemoryRunStore::default();
    let mut root_request = request("writer-parity-root", "/tmp/writer-parity-root");
    root_request.environment.write_execution_mode = WriteExecutionMode::IsolatedWriter;
    root_request.environment.auto_approve = true;

    let sqlite_created = sqlite
        .create(root_request.clone())
        .await
        .expect("create SQLite writer root");
    let memory_created = memory
        .create(root_request)
        .await
        .expect("create memory writer root");
    let task = writer_parity_task();
    let operation_id = OperationId("writer-parity-operation".to_owned());
    let integration_id = OperationId("writer-parity-integration".to_owned());
    let root_before = known_workspace(1, "root-base");
    let root_after = known_workspace(2, "root-integrated");

    let lifecycle = vec![
        RuntimeEventKind::WorkspaceObserved {
            workspace_state: root_before.clone(),
        },
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: RunId::from("writer-parity-root"),
                call_id: task.call_id.clone(),
                name: "agent".to_owned(),
                arguments: ToolArguments::from_value(serde_json::json!({
                    "workspace_access": "isolated_write",
                    "allowed_paths": ["src/lib.rs"]
                })),
            },
            workspace_access: WorkspaceAccess::MayWrite,
        },
        RuntimeEventKind::ToolExecutionStarted {
            operation_id: operation_id.clone(),
        },
        RuntimeEventKind::AgentTaskPrepared {
            task: Box::new(task.clone()),
        },
        RuntimeEventKind::AgentWorkspaceCreated {
            task_id: task.task_id.clone(),
            assignment: task.workspace.clone(),
            writer_workspace_state: known_workspace(0, "writer-created"),
        },
        RuntimeEventKind::ChildStarted {
            task_id: task.task_id.clone(),
            call_id: task.call_id.clone(),
            child_run_id: task.child_run_id.clone(),
            depth: 1,
        },
        RuntimeEventKind::AgentSealPrepared {
            task_id: task.task_id.clone(),
            base_commit: "a".repeat(40),
            writer_workspace_state_before: known_workspace(1, "writer-dirty"),
        },
        RuntimeEventKind::AgentSealCommitted {
            task_id: task.task_id.clone(),
            final_commit: "b".repeat(40),
            diff_sha256: "d".repeat(64),
            changed_files: vec!["src/lib.rs".to_owned()],
            writer_workspace_state_after: known_workspace(2, "writer-sealed"),
        },
        RuntimeEventKind::AgentResultCollected {
            task_id: task.task_id.clone(),
            outcome: Box::new(writer_parity_outcome(false)),
        },
        RuntimeEventKind::AgentIntegrationPrepared {
            task_id: task.task_id.clone(),
            integration_id: integration_id.clone(),
            base_commit: "a".repeat(40),
            writer_commit: "b".repeat(40),
            diff_sha256: "d".repeat(64),
            expected_root_workspace_state: root_before,
        },
        RuntimeEventKind::AgentIntegrationStarted {
            task_id: task.task_id.clone(),
            integration_id: integration_id.clone(),
        },
        RuntimeEventKind::AgentIntegrationCommitted {
            task_id: task.task_id.clone(),
            integration_id,
            root_head_commit: "b".repeat(40),
            root_workspace_state_after: root_after.clone(),
        },
        RuntimeEventKind::ChildFinished {
            call_id: task.call_id.clone(),
            outcome: Box::new(writer_parity_outcome(true)),
            accounting: Box::new(ModelAccounting::default()),
            handoff_content: "writer 已集成".to_owned(),
        },
        RuntimeEventKind::ToolOutcomeCommitted {
            operation_id,
            call_id: task.call_id.clone(),
            name: "agent".to_owned(),
            outcome: Box::new(
                ToolOutcome::success("writer 已封存并集成")
                    .with_side_effect(ToolSideEffectStatus::Applied),
            ),
            workspace_state: Some(root_after),
        },
        RuntimeEventKind::AgentCleanupPrepared {
            task_id: task.task_id.clone(),
            worktree_path: task.workspace.worktree_path.clone().unwrap(),
            branch: task.workspace.branch.clone().unwrap(),
            owner_token: task.workspace.owner_token.clone().unwrap(),
        },
        RuntimeEventKind::AgentCleanupCommitted {
            task_id: task.task_id,
            worktree_path: task.workspace.worktree_path.unwrap(),
            branch: task.workspace.branch.unwrap(),
            owner_token: task.workspace.owner_token.unwrap(),
            worktree_removed: true,
            branch_removed: true,
            retained_for_recovery: false,
            reason: None,
        },
    ];

    for (index, event) in lifecycle.into_iter().enumerate() {
        append_to_both(
            &sqlite,
            &sqlite_created.lease,
            &memory,
            &memory_created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("writer-lifecycle-{}", index + 1)),
                event,
            },
        )
        .await;
    }

    let sqlite_replay = sqlite
        .load(&sqlite_created.lease.run_id)
        .await
        .expect("load SQLite writer lifecycle")
        .expect("SQLite writer root exists");
    let memory_replay = memory
        .load(&memory_created.lease.run_id)
        .await
        .expect("load memory writer lifecycle")
        .expect("memory writer root exists");
    assert_canonical_replay_eq(&sqlite_replay, &memory_replay);
    assert_eq!(sqlite_replay.snapshot.agent_tasks.len(), 1);
    assert!(sqlite_replay.snapshot.pending_tool.is_none());
    assert_eq!(
        reduce_events(&sqlite_replay.events).expect("replay SQLite writer lifecycle"),
        sqlite_replay.snapshot
    );

    drop(sqlite);
    let reopened = StateStore::open(Some(path)).expect("reopen writer SQLite store");
    let reopened_replay = reopened
        .load(&RunId::from("writer-parity-root"))
        .await
        .expect("reload writer lifecycle")
        .expect("reopened writer root exists");
    assert_eq!(reopened_replay, sqlite_replay);
    assert_canonical_replay_eq(&reopened_replay, &memory_replay);
}

#[tokio::test]
async fn sqlite_replay_preserves_disjoint_request_budget_failures_verbatim() {
    for (label, failure, expected_kind) in [
        (
            "logical",
            RuntimeFailure::ModelRequestBudgetExceeded { limit: 8 },
            "model_request_budget_exceeded",
        ),
        (
            "physical",
            RuntimeFailure::ApiRequestBudgetExceeded { limit: 10 },
            "api_request_budget_exceeded",
        ),
    ] {
        let path = temp_state_path(&format!("budget-taxonomy-{label}"));
        let store = StateStore::open(Some(path.clone())).expect("open budget taxonomy store");
        let created = store
            .create(request(
                &format!("budget-{label}-run"),
                "/tmp/budget-taxonomy",
            ))
            .await
            .expect("create budget taxonomy run");
        let expected = AgentOutcome {
            run_id: created.lease.run_id.clone(),
            parent_run_id: None,
            terminal: TerminalState::Failed {
                failure: failure.clone(),
            },
            accounting: ModelAccounting::default(),
            runtime_model_requests: 8,
            runtime_retries: 0,
            tool_calls: 0,
            details: Default::default(),
        };
        store
            .append(
                &created.lease,
                PendingRuntimeEvent::terminal(expected.clone()),
            )
            .await
            .expect("persist budget terminal");

        let replay = store
            .load(&created.lease.run_id)
            .await
            .expect("load budget terminal")
            .expect("budget run exists");
        assert_eq!(replay.snapshot.terminal.as_ref(), Some(&expected));
        assert_eq!(
            replay.events.last().map(|event| &event.event),
            Some(&RuntimeEventKind::Terminal {
                outcome: Box::new(expected.clone()),
            })
        );

        drop(store);
        let conn = Connection::open(&path).expect("inspect budget terminal JSON");
        let raw: String = conn
            .query_row(
                "SELECT event_json FROM agent_run_events WHERE terminal = 1",
                [],
                |row| row.get(0),
            )
            .expect("read raw budget terminal");
        let raw: serde_json::Value =
            serde_json::from_str(&raw).expect("budget terminal JSON is valid");
        assert_eq!(
            raw["event"]["outcome"]["terminal"]["failure"]["kind"],
            expected_kind
        );
        assert_ne!(
            raw["event"]["outcome"]["terminal"]["failure"]["kind"],
            "request_budget_exceeded"
        );
        drop(conn);

        let reopened = StateStore::open(Some(path)).expect("reopen budget taxonomy store");
        let reopened = reopened
            .load(&created.lease.run_id)
            .await
            .expect("load reopened budget terminal")
            .expect("reopened budget run exists");
        assert_eq!(reopened.snapshot.terminal, Some(expected));
    }
}

#[tokio::test]
async fn append_is_event_idempotent_and_sequences_are_monotonic() {
    let store = StateStore::open(Some(temp_state_path("idempotent"))).expect("open store");
    let created = store
        .create(request("idempotent-run", "/tmp/idempotent"))
        .await
        .expect("create");
    let pending = user_event("same-id", "一次");
    let first = store
        .append(&created.lease, pending.clone())
        .await
        .expect("first append");
    let repeated = store
        .append(&created.lease, pending)
        .await
        .expect("idempotent append");
    assert_eq!(first, repeated);

    let conflict = store
        .append(&created.lease, user_event("same-id", "不同内容"))
        .await
        .expect_err("same id with different content must fail");
    assert!(matches!(conflict, RunStoreError::EventConflict { .. }));

    let third = store
        .append(&created.lease, user_event("third", "三"))
        .await
        .expect("third event");
    assert_eq!(third.sequence, 3);
    let replay = store
        .load(&created.lease.run_id)
        .await
        .expect("load")
        .expect("run");
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        store
            .events_after(&created.lease.run_id, u64::MAX)
            .await
            .expect("cursor above SQLite range is an empty tail")
            .is_empty()
    );
}

#[tokio::test]
async fn streaming_deltas_advance_durability_without_rewriting_snapshot_json() {
    let path = temp_state_path("delta_snapshot");
    let store = StateStore::open(Some(path.clone())).expect("open store");
    let created = store
        .create(request("delta-run", "/tmp/delta"))
        .await
        .expect("create");
    let attempt_id = AttemptId("delta-attempt".to_owned());
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("delta-prepared".to_owned()),
                event: RuntimeEventKind::ModelRequestPrepared {
                    attempt_id: attempt_id.clone(),
                    request: Box::new(model_request(&created)),
                },
            },
        )
        .await
        .expect("prepare model request");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("delta-in-flight".to_owned()),
                event: RuntimeEventKind::ModelRequestInFlight {
                    attempt_id: attempt_id.clone(),
                },
            },
        )
        .await
        .expect("start model request");
    let wrong_delta = store
        .append(
            &created.lease,
            content_delta(
                "wrong-delta-attempt",
                &AttemptId("other-attempt".to_owned()),
                "不应落库",
            ),
        )
        .await
        .expect_err("delta for another request must fail before append");
    assert!(matches!(wrong_delta, RunStoreError::Corrupt { .. }));
    let snapshot_before = Connection::open(&path)
        .expect("open snapshot reader")
        .query_row(
            "SELECT last_sequence, snapshot_json FROM agent_run_snapshots WHERE run_id = ?1",
            params![created.lease.run_id.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("read initial snapshot");
    assert_eq!(snapshot_before.0, 3);

    store
        .append(&created.lease, content_delta("delta-1", &attempt_id, "一"))
        .await
        .expect("first delta");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId("delta-2".to_owned()),
                event: RuntimeEventKind::ReasoningDelta {
                    attempt_id,
                    index: 1,
                    delta: "二".to_owned(),
                },
            },
        )
        .await
        .expect("reasoning delta");

    let snapshot_after_deltas = Connection::open(&path)
        .expect("open snapshot reader")
        .query_row(
            "SELECT last_sequence, snapshot_json FROM agent_run_snapshots WHERE run_id = ?1",
            params![created.lease.run_id.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("read delta snapshot");
    assert_eq!(snapshot_after_deltas.0, 5);
    assert_eq!(
        snapshot_after_deltas.1, snapshot_before.1,
        "projection-neutral streaming deltas rewrote the full snapshot"
    );

    store
        .append(&created.lease, user_event("steer-after-deltas", "继续"))
        .await
        .expect("materialize state-changing event");
    let replay = store
        .load(&created.lease.run_id)
        .await
        .expect("load delta run")
        .expect("delta run exists");
    assert_eq!(replay.snapshot.last_sequence, 6);
    assert_eq!(replay.events.len(), 6);
}

#[tokio::test]
async fn terminal_is_exactly_once_and_immutable_after_reopen() {
    let path = temp_state_path("terminal");
    let store = StateStore::open(Some(path.clone())).expect("open store");
    let created = store
        .create(request("terminal-run", "/tmp/terminal"))
        .await
        .expect("create");
    let pending = terminal_event(&created.lease.run_id);
    let terminal = store
        .append(&created.lease, pending.clone())
        .await
        .expect("terminal append");
    assert_eq!(terminal.sequence, 2);
    let repeated = store
        .append(&created.lease, pending)
        .await
        .expect("same terminal event is idempotent");
    assert_eq!(terminal, repeated);
    let error = store
        .append(&created.lease, user_event("after-terminal", "不允许"))
        .await
        .expect_err("event after terminal must fail");
    assert!(matches!(error, RunStoreError::AlreadyTerminal { .. }));
    drop(store);

    let reopened = StateStore::open(Some(path)).expect("reopen");
    let acquired = reopened
        .acquire(&RunId::from("terminal-run"))
        .await
        .expect("acquire terminal");
    assert!(acquired.lease.is_none());
    assert!(acquired.replay.snapshot.terminal.is_some());
    assert_eq!(
        acquired
            .replay
            .events
            .iter()
            .filter(|event| event.event.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn execution_epoch_fences_stale_leases_and_reclaims_dead_pid() {
    let path = temp_state_path("lease_fence");
    let store = StateStore::open(Some(path.clone())).expect("open store");
    let created = store
        .create(request("lease-run", "/tmp/lease"))
        .await
        .expect("create");

    let live_error = store
        .acquire(&created.lease.run_id)
        .await
        .expect_err("live owner must be rejected");
    assert!(matches!(live_error, RunStoreError::AlreadyRunning { .. }));

    let conn = Connection::open(&path).expect("open direct sqlite connection");
    conn.execute(
        "UPDATE agent_runs SET lease_owner_pid = ?2 WHERE run_id = ?1",
        params![created.lease.run_id.0, i64::from(i32::MAX)],
    )
    .expect("simulate dead process owner");
    drop(conn);

    let reclaimed = store
        .acquire(&created.lease.run_id)
        .await
        .expect("reclaim dead owner")
        .lease
        .expect("writer lease");
    assert_eq!(reclaimed.epoch, created.lease.epoch + 1);
    let stale = store
        .append(&created.lease, user_event("stale", "旧执行者"))
        .await
        .expect_err("old epoch must be fenced");
    assert!(matches!(stale, RunStoreError::StaleLease { .. }));
    store
        .append(&reclaimed, user_event("current", "新执行者"))
        .await
        .expect("new lease append");
}

#[tokio::test]
async fn concurrent_store_instances_allow_only_one_writer() {
    let path = temp_state_path("concurrent");
    let first = StateStore::open(Some(path.clone())).expect("open first");
    let created = first
        .create(request("concurrent-run", "/tmp/concurrent"))
        .await
        .expect("create");
    first
        .release(&created.lease)
        .await
        .expect("release creator");
    let second = StateStore::open(Some(path)).expect("open second");
    let run_id = created.lease.run_id;

    let (left, right) = tokio::join!(first.acquire(&run_id), second.acquire(&run_id));
    let success_count = usize::from(left.is_ok()) + usize::from(right.is_ok());
    assert_eq!(success_count, 1);
    let failure = if let Err(error) = left {
        error
    } else {
        right.expect_err("second acquire must fail")
    };
    assert!(matches!(failure, RunStoreError::AlreadyRunning { .. }));
}

#[tokio::test]
async fn creation_reservation_is_durable_idempotent_and_rejects_payload_reuse() {
    let path = temp_state_path("creation-reservation");
    let sqlite = StateStore::open(Some(path.clone())).expect("open sqlite");
    let memory = InMemoryRunStore::default();
    let command_id = CommandId::from("create-command");
    let proposed = RunId::from("reserved-run");

    let sqlite_first = sqlite
        .reserve_creation(
            &command_id,
            "sha256:first",
            proposed.clone(),
            creation_intent("/tmp/creation"),
        )
        .await
        .expect("reserve sqlite creation");
    let memory_first = memory
        .reserve_creation(
            &command_id,
            "sha256:first",
            proposed.clone(),
            creation_intent("/tmp/creation"),
        )
        .await
        .expect("reserve memory creation");
    assert_eq!(
        sqlite_first.reservation.command_id,
        memory_first.reservation.command_id
    );
    assert_eq!(
        sqlite_first.reservation.command_sha256,
        memory_first.reservation.command_sha256
    );
    assert_eq!(
        sqlite_first.reservation.run_id,
        memory_first.reservation.run_id
    );
    assert!(sqlite_first.newly_reserved);

    let sqlite_retry = sqlite
        .reserve_creation(
            &command_id,
            "sha256:first",
            RunId::from("ignored-proposal"),
            creation_intent("/tmp/creation"),
        )
        .await
        .expect("retry sqlite reservation");
    let memory_retry = memory
        .reserve_creation(
            &command_id,
            "sha256:first",
            RunId::from("ignored-proposal"),
            creation_intent("/tmp/creation"),
        )
        .await
        .expect("retry memory reservation");
    assert_eq!(
        sqlite_retry.reservation.command_id,
        memory_retry.reservation.command_id
    );
    assert_eq!(
        sqlite_retry.reservation.command_sha256,
        memory_retry.reservation.command_sha256
    );
    assert_eq!(
        sqlite_retry.reservation.run_id,
        memory_retry.reservation.run_id
    );
    assert!(!sqlite_retry.newly_reserved);
    assert_eq!(sqlite_retry.reservation.run_id, proposed);

    assert!(matches!(
        sqlite
            .reserve_creation(
                &command_id,
                "sha256:different",
                RunId::from("other-run"),
                creation_intent("/tmp/creation"),
            )
            .await,
        Err(RunStoreError::CreationConflict { .. })
    ));
    drop(sqlite);
    let reopened = StateStore::open(Some(path)).expect("reopen sqlite");
    let reopened_retry = reopened
        .reserve_creation(
            &command_id,
            "sha256:first",
            RunId::from("second-ignored-proposal"),
            creation_intent("/tmp/creation"),
        )
        .await
        .expect("retry after reopen");
    assert_eq!(reopened_retry, sqlite_retry);
    let pending = reopened
        .list_pending_creations("/tmp/creation", 10)
        .await
        .expect("list pending creation after reopen");
    assert_eq!(pending, vec![reopened_retry.reservation.clone()]);

    let mut reserved_request = request("ignored", "/tmp/creation");
    reserved_request.run_id = Some(reopened_retry.reservation.run_id.clone());
    reserved_request
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .generation_id = TaskGenerationId::from(reopened_retry.reservation.run_id.0.clone());
    reopened
        .create(reserved_request)
        .await
        .expect("create reserved run");
    assert!(
        reopened
            .list_pending_creations("/tmp/creation", 10)
            .await
            .expect("list after RunCreated")
            .is_empty()
    );
    let completed_receipt = reopened
        .creation(&command_id)
        .await
        .expect("load completed creation receipt")
        .expect("creation receipt remains durable");
    assert_eq!(completed_receipt.run_id, proposed);
    assert!(completed_receipt.intent.is_none());
}

#[tokio::test]
async fn root_runs_are_listed_from_durable_request_workspace() {
    let store = StateStore::open(Some(temp_state_path("latest"))).expect("open store");
    let first = store
        .create(request("workspace-first", "/tmp/workspace"))
        .await
        .expect("first");
    store.release(&first.lease).await.expect("release first");
    let other = store
        .create(request("workspace-other", "/tmp/other"))
        .await
        .expect("other");
    store.release(&other.lease).await.expect("release other");
    let latest = store
        .create(request("workspace-latest", "/tmp/workspace"))
        .await
        .expect("latest");
    store.release(&latest.lease).await.expect("release latest");
    let child_request = read_only_child_request(
        "workspace-newer-child",
        RunId::from("workspace-latest"),
        "/tmp/workspace",
    );
    let child = store.create(child_request).await.expect("newer child");
    store.release(&child.lease).await.expect("release child");

    let roots = store
        .list_root_runs("/tmp/workspace", 10)
        .await
        .expect("root query");
    assert_eq!(
        roots
            .iter()
            .map(|record| record.run_id.clone())
            .collect::<Vec<_>>(),
        vec![
            RunId::from("workspace-latest"),
            RunId::from("workspace-first")
        ]
    );
    assert!(
        store
            .list_root_runs("/tmp/missing", 10)
            .await
            .expect("missing query")
            .is_empty()
    );
}

#[tokio::test]
async fn root_list_semantics_match_memory_for_purpose_lineage_order_limit_and_state() {
    let workspace = "/tmp/root-list-parity";
    let sqlite =
        StateStore::open(Some(temp_state_path("root_list_parity"))).expect("open sqlite store");
    let memory = InMemoryRunStore::default();

    let source_request = request("root-list-01-source", workspace);
    let sqlite_source = sqlite
        .create(source_request.clone())
        .await
        .expect("create sqlite source");
    let memory_source = memory
        .create(source_request)
        .await
        .expect("create memory source");
    append_to_both(
        &sqlite,
        &sqlite_source.lease,
        &memory,
        &memory_source.lease,
        terminal_event(&sqlite_source.lease.run_id),
    )
    .await;
    let source = sqlite
        .load(&sqlite_source.lease.run_id)
        .await
        .expect("load source")
        .expect("source exists");

    let mut continuation = request("root-list-02-agent-continuation", workspace);
    continuation
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .definition
        .objective = "继续完成列表验收".to_owned();
    let continuation =
        continuation_request(continuation, &source, sqlite_source.lease.run_id.clone());
    let sqlite_continuation = sqlite
        .create(continuation.clone())
        .await
        .expect("create sqlite Agent continuation");
    let memory_continuation = memory
        .create(continuation)
        .await
        .expect("create memory Agent continuation");
    append_to_both(
        &sqlite,
        &sqlite_continuation.lease,
        &memory,
        &memory_continuation.lease,
        user_event("root-list-steer", "补充列表验收"),
    )
    .await;

    let active_request = request("root-list-04-active", workspace);
    sqlite
        .create(active_request.clone())
        .await
        .expect("create sqlite active root");
    memory
        .create(active_request)
        .await
        .expect("create memory active root");

    let child_request = read_only_child_request(
        "root-list-99-child",
        sqlite_source.lease.run_id.clone(),
        workspace,
    );
    sqlite
        .create(child_request.clone())
        .await
        .expect("create sqlite child");
    memory
        .create(child_request)
        .await
        .expect("create memory child");

    let other_workspace_request = request("root-list-98-other-workspace", "/tmp/root-list-other");
    sqlite
        .create(other_workspace_request.clone())
        .await
        .expect("create sqlite other-workspace root");
    memory
        .create(other_workspace_request)
        .await
        .expect("create memory other-workspace root");

    let sqlite_all = root_list_semantics(
        sqlite
            .list_root_runs(workspace, 10)
            .await
            .expect("list sqlite roots"),
    );
    let memory_all = root_list_semantics(
        memory
            .list_root_runs(workspace, 10)
            .await
            .expect("list memory roots"),
    );
    assert_eq!(sqlite_all, memory_all);
    assert_eq!(
        sqlite_all,
        vec![
            (
                RunId::from("root-list-04-active"),
                None,
                workspace.to_owned(),
                1,
                false,
            ),
            (
                RunId::from("root-list-02-agent-continuation"),
                Some(RunId::from("root-list-01-source")),
                workspace.to_owned(),
                2,
                false,
            ),
            (
                RunId::from("root-list-01-source"),
                None,
                workspace.to_owned(),
                2,
                true,
            ),
        ]
    );

    let sqlite_limited = root_list_semantics(
        sqlite
            .list_root_runs(workspace, 2)
            .await
            .expect("list limited sqlite roots"),
    );
    let memory_limited = root_list_semantics(
        memory
            .list_root_runs(workspace, 2)
            .await
            .expect("list limited memory roots"),
    );
    assert_eq!(sqlite_limited, memory_limited);
    assert_eq!(sqlite_limited, sqlite_all[..2]);
}

#[tokio::test]
async fn continuation_create_is_atomic_and_matches_memory_store() {
    let sqlite = StateStore::open(Some(temp_state_path("continuation"))).expect("open store");
    let memory = InMemoryRunStore::default();
    let source_request = request("source-root", "/tmp/workspace");
    let sqlite_source = sqlite
        .create(source_request.clone())
        .await
        .expect("sqlite source");
    let memory_source = memory.create(source_request).await.expect("memory source");
    append_to_both(
        &sqlite,
        &sqlite_source.lease,
        &memory,
        &memory_source.lease,
        terminal_event(&sqlite_source.lease.run_id),
    )
    .await;
    let source_before = sqlite
        .load(&sqlite_source.lease.run_id)
        .await
        .expect("load source")
        .expect("source exists");

    let mut continuation = request("continued-root", "/tmp/workspace");
    continuation
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .definition
        .objective = "继续完成验收".to_owned();
    continuation
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .definition
        .constraints = vec!["只修改 canonical 路径".to_owned()];
    continuation
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .definition
        .non_goals = vec!["不恢复旧兼容层".to_owned()];
    let continuation = continuation_request(
        continuation,
        &source_before,
        sqlite_source.lease.run_id.clone(),
    );
    let sqlite_continued = sqlite
        .create(continuation.clone())
        .await
        .expect("sqlite continuation");
    let memory_continued = memory
        .create(continuation)
        .await
        .expect("memory continuation");
    assert_canonical_replay_eq(&sqlite_continued.replay, &memory_continued.replay);
    assert_eq!(sqlite_continued.replay.snapshot.request.parent_run_id, None);
    assert_eq!(
        sqlite_continued
            .replay
            .snapshot
            .request
            .continued_from_run_id,
        Some(sqlite_source.lease.run_id.clone())
    );
    assert_eq!(
        &sqlite_continued.replay.snapshot.transcript.entries
            [..source_before.snapshot.transcript.entries.len()],
        source_before.snapshot.transcript.entries.as_slice()
    );
    assert!(matches!(
        sqlite_continued.replay.snapshot.transcript.entries.last(),
        Some(codewhale_runtime::TranscriptEntry::User { content })
            if content == concat!(
                "任务目标：\n继续完成验收",
                "\n\n约束：\n- 只修改 canonical 路径",
                "\n\n非目标：\n- 不恢复旧兼容层",
                "\n\n验收条件：\n- 由 Host 明确接受完成候选"
            )
    ));
    assert_eq!(
        sqlite
            .load(&sqlite_source.lease.run_id)
            .await
            .expect("reload source")
            .expect("source remains")
            .events,
        source_before.events
    );

    let roots = sqlite
        .list_root_runs("/tmp/workspace", 10)
        .await
        .expect("list roots");
    let continued_record = roots
        .iter()
        .find(|record| record.run_id == RunId::from("continued-root"))
        .expect("continued root is listed");
    assert_eq!(
        continued_record.continued_from_run_id,
        Some(RunId::from("source-root"))
    );
}

#[tokio::test]
async fn continuation_rejects_nonterminal_child_recovery_and_workspace_mismatch() {
    let store = StateStore::open(Some(temp_state_path("invalid-continuation")))
        .expect("open invalid continuation store");
    let active = store
        .create(request("active-source", "/tmp/workspace"))
        .await
        .expect("active source");
    let mut from_active = request("from-active", "/tmp/workspace");
    from_active.continued_from_run_id = Some(active.lease.run_id.clone());
    from_active.transcript = active.replay.snapshot.transcript.clone();
    assert!(matches!(
        store.create(from_active).await,
        Err(RunStoreError::InvalidContinuation {
            reason: codewhale_runtime::ContinuationError::SourceNotTerminal,
            ..
        })
    ));

    let child_request =
        read_only_child_request("child-source", RunId::from("parent"), "/tmp/workspace");
    let child = store.create(child_request).await.expect("child source");
    store
        .append(
            &child.lease,
            PendingRuntimeEvent::terminal(AgentOutcome {
                run_id: child.lease.run_id.clone(),
                parent_run_id: Some(RunId::from("parent")),
                terminal: TerminalState::Blocked {
                    reason: "child completed".to_owned(),
                },
                accounting: ModelAccounting::default(),
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
                details: Default::default(),
            }),
        )
        .await
        .expect("terminal child");
    let child_replay = store
        .load(&child.lease.run_id)
        .await
        .expect("load child")
        .expect("child exists");
    let mut from_child = request("from-child", "/tmp/workspace");
    from_child.continued_from_run_id = Some(child.lease.run_id.clone());
    from_child.transcript = child_replay.snapshot.transcript;
    assert!(matches!(
        store.create(from_child).await,
        Err(RunStoreError::InvalidContinuation {
            reason: codewhale_runtime::ContinuationError::SourceIsChild,
            ..
        })
    ));

    let recovery = store
        .create(request("recovery-source", "/tmp/workspace"))
        .await
        .expect("recovery source");
    let recovery_outcome = AgentOutcome {
        run_id: recovery.lease.run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::RecoveryRequired {
            ambiguity: codewhale_runtime::RecoveryAmbiguity {
                phase: codewhale_runtime::RecoveryAmbiguityPhase::ModelRequest,
                action_id: "attempt-1".to_owned(),
                message: "billing unknown".to_owned(),
            },
        },
        accounting: ModelAccounting::default(),
        runtime_model_requests: 1,
        runtime_retries: 0,
        tool_calls: 0,
        details: Default::default(),
    };
    store
        .append(
            &recovery.lease,
            PendingRuntimeEvent::terminal(recovery_outcome),
        )
        .await
        .expect("terminal recovery source");
    let recovery_replay = store
        .load(&recovery.lease.run_id)
        .await
        .expect("load recovery")
        .expect("recovery exists");
    let mut from_recovery = request("from-recovery", "/tmp/workspace");
    from_recovery.continued_from_run_id = Some(recovery.lease.run_id.clone());
    from_recovery.transcript = recovery_replay.snapshot.transcript;
    assert!(matches!(
        store.create(from_recovery).await,
        Err(RunStoreError::InvalidContinuation {
            reason: codewhale_runtime::ContinuationError::RecoveryRequired,
            ..
        })
    ));

    let terminal = store
        .create(request("workspace-source", "/tmp/workspace"))
        .await
        .expect("workspace source");
    store
        .append(&terminal.lease, terminal_event(&terminal.lease.run_id))
        .await
        .expect("terminal workspace source");
    let terminal_replay = store
        .load(&terminal.lease.run_id)
        .await
        .expect("load terminal source")
        .expect("terminal source exists");
    let mut wrong_workspace = request("wrong-workspace", "/tmp/other");
    wrong_workspace.continued_from_run_id = Some(terminal.lease.run_id);
    wrong_workspace.transcript = terminal_replay.snapshot.transcript;
    assert!(matches!(
        store.create(wrong_workspace).await,
        Err(RunStoreError::InvalidContinuation {
            reason: codewhale_runtime::ContinuationError::WorkspaceMismatch,
            ..
        })
    ));
}

#[tokio::test]
async fn v5_migration_retires_incompatible_pre_orchestrator_runs() {
    let path = temp_state_path("v5_pending_model_migration");
    let conn = create_v5_run_store(&path);
    insert_v5_run(&conn, "v5-prepared", DurableActionState::Prepared, false);
    insert_v5_run(&conn, "v5-in-flight", DurableActionState::InFlight, true);
    drop(conn);

    let store = StateStore::open(Some(path.clone())).expect("migrate v5 store");
    let conn = Connection::open(&path).expect("inspect migrated store");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(user_version, 17);
    let remaining_runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row.get(0))
        .expect("count retired v5 runs");
    assert_eq!(remaining_runs, 0);
    drop(conn);
    for run_id in ["v5-prepared", "v5-in-flight"] {
        assert!(
            store
                .load(&RunId::from(run_id))
                .await
                .expect("query retired v5 run")
                .is_none(),
            "{run_id} survived the RuntimeEvent v10 cutover"
        );
    }
}

#[tokio::test]
async fn v16_cutover_retires_v15_runtime_rows_instead_of_upgrading_authority() {
    let path = temp_state_path("v15_terminal_accounting");
    let store = StateStore::open(Some(path.clone())).expect("open current state");
    let created = store
        .create(request(
            "v15-terminal-accounting",
            "/tmp/v15-terminal-accounting",
        ))
        .await
        .expect("create terminal accounting run");
    let terminal_accounting = ModelAccounting {
        root: ActorRequestAccounting {
            started: 1,
            completed: 1,
            ..ActorRequestAccounting::default()
        },
        sealed: true,
        complete: true,
        usage_complete: true,
        ..ModelAccounting::default()
    };
    store
        .append(
            &created.lease,
            PendingRuntimeEvent::terminal(AgentOutcome {
                run_id: created.lease.run_id.clone(),
                parent_run_id: None,
                terminal: TerminalState::Blocked {
                    reason: "测试 v15 accounting cutover".to_owned(),
                },
                accounting: terminal_accounting.clone(),
                runtime_model_requests: 1,
                runtime_retries: 0,
                tool_calls: 0,
                details: AgentResultDetails::default(),
            }),
        )
        .await
        .expect("append terminal outcome");
    let replay = store
        .load(&created.lease.run_id)
        .await
        .expect("load current replay")
        .expect("current replay exists");
    let mut legacy_snapshot = replay.snapshot;
    legacy_snapshot.accounting = ModelAccounting::default();
    drop(store);

    let conn = Connection::open(&path).expect("open raw v14 fixture");
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = ?1 WHERE run_id = ?2",
        params![
            serde_json::to_string(&legacy_snapshot).expect("encode v14 snapshot"),
            created.lease.run_id.0
        ],
    )
    .expect("restore v14 terminal snapshot shape");
    conn.pragma_update(None, "user_version", 14)
        .expect("mark v14 fixture");
    drop(conn);

    let migrated = StateStore::open(Some(path.clone())).expect("apply state v16 cutover");
    assert!(
        migrated
            .load(&created.lease.run_id)
            .await
            .expect("query retired runtime row")
            .is_none()
    );
    let conn = Connection::open(path).expect("inspect v16 database");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(user_version, 17);
}

#[tokio::test]
async fn v16_cutover_retires_corrupt_v15_snapshot_before_deserialization() {
    let path = temp_state_path("v15_terminal_accounting_corrupt");
    let store = StateStore::open(Some(path.clone())).expect("open current state");
    let created = store
        .create(request(
            "v15-terminal-accounting-corrupt",
            "/tmp/v15-terminal-accounting-corrupt",
        ))
        .await
        .expect("create corrupt migration run");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent::terminal(AgentOutcome {
                run_id: created.lease.run_id.clone(),
                parent_run_id: None,
                terminal: TerminalState::Blocked {
                    reason: "测试 v15 corruption gate".to_owned(),
                },
                accounting: ModelAccounting {
                    sealed: true,
                    complete: true,
                    usage_complete: true,
                    ..ModelAccounting::default()
                },
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
                details: AgentResultDetails::default(),
            }),
        )
        .await
        .expect("append terminal outcome");
    let mut legacy_snapshot = store
        .load(&created.lease.run_id)
        .await
        .expect("load current replay")
        .expect("current replay exists")
        .snapshot;
    legacy_snapshot.accounting = ModelAccounting::default();
    legacy_snapshot.workspace_state.generation = 99;
    drop(store);

    let conn = Connection::open(&path).expect("open raw corrupt fixture");
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = ?1 WHERE run_id = ?2",
        params![
            serde_json::to_string(&legacy_snapshot).expect("encode corrupt snapshot"),
            created.lease.run_id.0
        ],
    )
    .expect("write corrupt v14 snapshot");
    conn.pragma_update(None, "user_version", 14)
        .expect("mark corrupt v14 fixture");
    drop(conn);

    let migrated = StateStore::open(Some(path.clone())).expect("apply state v16 cutover");
    assert!(
        migrated
            .load(&created.lease.run_id)
            .await
            .expect("query retired corrupt runtime row")
            .is_none()
    );
    let conn = Connection::open(path).expect("inspect v16 database");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(user_version, 17);
}

#[tokio::test]
async fn v17_cutover_retires_v16_runtime_rows_before_temporal_deserialization() {
    let path = temp_state_path("v16_temporal_evidence_cutover");
    let store = StateStore::open(Some(path.clone())).expect("open current state");
    let created = store
        .create(request(
            "v16-temporal-evidence",
            "/tmp/v16-temporal-evidence",
        ))
        .await
        .expect("create pre-cutover run");
    let run_id = created.lease.run_id.clone();
    drop(store);

    let conn = Connection::open(&path).expect("open raw v16 fixture");
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = '{\"legacy\":\"v16\"}' WHERE run_id = ?1",
        [&run_id.0],
    )
    .expect("write incompatible v16 snapshot");
    conn.execute(
        "UPDATE agent_run_events SET schema_version = 11 WHERE run_id = ?1",
        [&run_id.0],
    )
    .expect("mark v16 RuntimeEvent rows");
    conn.pragma_update(None, "user_version", 16)
        .expect("mark v16 fixture");
    drop(conn);

    let migrated = StateStore::open(Some(path.clone())).expect("apply state v17 cutover");
    assert!(
        migrated
            .load(&run_id)
            .await
            .expect("query retired v16 run")
            .is_none()
    );
    let conn = Connection::open(path).expect("inspect v17 database");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(user_version, 17);
}

#[tokio::test]
async fn v9_migration_retires_incompatible_catalog_run_without_replaying_it() {
    let path = temp_state_path("v9_model_catalog_migration");
    let run_id = persist_committed_catalog_run(&path, "v9-catalog-run").await;
    downgrade_catalog_snapshot_to_v9(&path, false);

    let store = StateStore::open(Some(path.clone())).expect("migrate v9 catalog snapshot");
    assert!(
        store
            .load(&run_id)
            .await
            .expect("query retired catalog run")
            .is_none()
    );

    let conn = Connection::open(path).expect("inspect migrated catalog database");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated state version");
    assert_eq!(user_version, 17);
}

#[tokio::test]
async fn corrupt_v9_snapshot_is_retired_instead_of_blocking_the_v14_cutover() {
    let path = temp_state_path("v9_corrupt_catalog_migration");
    let run_id = persist_committed_catalog_run(&path, "v9-corrupt-catalog-run").await;
    downgrade_catalog_snapshot_to_v9(&path, true);

    let store = StateStore::open(Some(path.clone()))
        .expect("v14 must retire corrupt incompatible runtime state");
    assert!(
        store
            .load(&run_id)
            .await
            .expect("query retired corrupt catalog run")
            .is_none()
    );
    drop(store);

    let conn = Connection::open(path).expect("inspect completed v14 migration");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(user_version, 17);
}

#[tokio::test]
async fn v10_migration_deletes_retired_state_and_incompatible_run_replay() {
    let path = temp_state_path("v10_retired_state_deletion");
    let store = StateStore::open(Some(path.clone())).expect("open current state store");
    let created = store
        .create(request(
            "v10-preserved-run",
            "/tmp/v10-retired-state-deletion",
        ))
        .await
        .expect("create canonical run before downgrade");
    store
        .append(&created.lease, terminal_event(&created.lease.run_id))
        .await
        .expect("complete canonical run before downgrade");
    drop(store);

    let conn = Connection::open(&path).expect("open current database for v10 downgrade");
    conn.execute_batch(
        r#"
        ALTER TABLE threads ADD COLUMN current_leaf_id INTEGER;
        CREATE TABLE thread_goals (
            thread_id TEXT PRIMARY KEY NOT NULL,
            goal_id TEXT NOT NULL,
            objective TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN (
                'active',
                'paused',
                'blocked',
                'usage_limited',
                'budget_limited',
                'complete'
            )),
            token_budget INTEGER,
            tokens_used INTEGER NOT NULL DEFAULT 0,
            time_used_seconds INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            continuation_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        );
        CREATE TABLE thread_dynamic_tools (legacy INTEGER);
        CREATE TABLE messages (legacy INTEGER);
        CREATE TABLE checkpoints (legacy INTEGER);
        CREATE TABLE jobs (legacy INTEGER);
        CREATE TABLE workflow_runs (legacy INTEGER);
        CREATE TABLE branch_runs (legacy INTEGER);
        CREATE TABLE leaf_runs (legacy INTEGER);
        CREATE TABLE control_node_runs (legacy INTEGER);
        CREATE TABLE teacher_candidates (legacy INTEGER);
        INSERT INTO threads (
            id, preview, ephemeral, model_provider, created_at, updated_at,
            status, cwd, cli_version, source, archived
        ) VALUES (
            'legacy-thread', 'retired goal fixture', 0, 'deepseek', 1, 1,
            'idle', '/tmp/v10-retired-state-deletion', '0.8.68', 'interactive', 0
        );
        INSERT INTO thread_goals (
            thread_id, goal_id, objective, status, token_budget, tokens_used,
            time_used_seconds, created_at, updated_at, continuation_count
        ) VALUES (
            'legacy-thread', 'legacy-goal', 'retired objective', 'active',
            1000, 200, 30, 1, 2, 3
        );
        PRAGMA user_version = 10;
        "#,
    )
    .expect("restore exact retired v10 thread goal schema and data");
    drop(conn);

    let reopened = StateStore::open(Some(path.clone())).expect("migrate v10 store to v14");
    assert!(
        reopened
            .load(&created.lease.run_id)
            .await
            .expect("query retired canonical run after v14 migration")
            .is_none()
    );
    let thread = reopened
        .get_thread("legacy-thread")
        .expect("read retained legacy thread")
        .expect("thread metadata survives v14 runtime cutover");
    assert_eq!(thread.preview, "retired goal fixture");
    drop(reopened);

    let conn = Connection::open(path).expect("inspect migrated v14 database");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated state version");
    assert_eq!(user_version, 17);
    for table in [
        "thread_goals",
        "thread_dynamic_tools",
        "messages",
        "checkpoints",
        "jobs",
        "workflow_runs",
        "branch_runs",
        "leaf_runs",
        "control_node_runs",
        "teacher_candidates",
    ] {
        let retired_table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("inspect retired table {table}: {error}"));
        assert!(!retired_table_exists, "{table} survived v12 migration");
    }
    let current_leaf_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name = 'current_leaf_id')",
            [],
            |row| row.get(0),
        )
        .expect("inspect retired thread current-leaf projection");
    assert!(
        !current_leaf_exists,
        "current_leaf_id survived v12 migration"
    );
}

#[test]
fn corrupt_v5_run_is_retired_before_any_legacy_projection_backfill() {
    let path = temp_state_path("v5_corrupt_migration");
    let conn = create_v5_run_store(&path);
    insert_v5_run(&conn, "v5-corrupt", DurableActionState::Prepared, false);
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = '{}' WHERE run_id = 'v5-corrupt'",
        [],
    )
    .expect("corrupt v5 snapshot fixture");
    drop(conn);

    StateStore::open(Some(path.clone()))
        .expect("v14 must retire incompatible state before legacy projection backfill");

    let conn = Connection::open(path).expect("inspect completed v14 migration");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated schema version");
    assert_eq!(user_version, 17);
    let remaining_runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row.get(0))
        .expect("count incompatible runs");
    assert_eq!(remaining_runs, 0);
}

#[tokio::test]
async fn v14_cutover_deletes_only_old_runtime_state_and_preserves_local_evidence() {
    let path = temp_state_path("v14_scoped_runtime_cutover");
    let store = StateStore::open(Some(path.clone())).expect("open current state store");
    let created = store
        .create(request(
            "v13-runtime-run",
            "/tmp/v14-scoped-runtime-cutover",
        ))
        .await
        .expect("create pre-cutover run");
    drop(store);

    let conn = Connection::open(&path).expect("prepare v13 cutover fixture");
    conn.execute_batch(
        r#"
        INSERT INTO agent_run_creations(
            command_id, command_sha256, run_id, created_at_unix_ms,
            creation_kind, workspace, source_run_id, command_json
        ) VALUES (
            'legacy-create-command', 'sha256:legacy-command', 'legacy-create-run', 1,
            'start', '/tmp/v14-scoped-runtime-cutover', NULL, '{"legacy":true}'
        );
        INSERT INTO threads (
            id, preview, ephemeral, model_provider, created_at, updated_at,
            status, cwd, cli_version, source, archived
        ) VALUES (
            'retained-thread', 'local development metadata', 0, 'deepseek', 1, 1,
            'idle', '/tmp/v14-scoped-runtime-cutover', 'test', 'interactive', 0
        );
        CREATE TABLE evaluation_evidence (
            id TEXT PRIMARY KEY NOT NULL,
            summary TEXT NOT NULL
        );
        INSERT INTO evaluation_evidence VALUES (
            'retained-eval', 'redacted offline evaluation result'
        );
        PRAGMA user_version = 13;
        "#,
    )
    .expect("prepare v13 rows outside the RuntimeEvent v10 contract");
    drop(conn);

    let reopened = StateStore::open(Some(path.clone())).expect("apply atomic v14 cutover");
    assert!(
        reopened
            .load(&created.lease.run_id)
            .await
            .expect("query retired v13 run")
            .is_none()
    );
    let retained_thread = reopened
        .get_thread("retained-thread")
        .expect("read retained thread")
        .expect("thread metadata must survive the runtime-only cutover");
    assert_eq!(retained_thread.preview, "local development metadata");
    drop(reopened);

    let conn = Connection::open(path).expect("inspect v14 scoped cutover");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read current state version");
    assert_eq!(user_version, 17);
    for table in [
        "agent_run_creations",
        "agent_runs",
        "agent_run_events",
        "agent_run_snapshots",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|error| panic!("count cutover rows in {table}: {error}"));
        assert_eq!(count, 0, "{table} retained incompatible runtime rows");
    }
    let retained_eval: String = conn
        .query_row(
            "SELECT summary FROM evaluation_evidence WHERE id = 'retained-eval'",
            [],
            |row| row.get(0),
        )
        .expect("evaluation evidence must survive runtime-only cutover");
    assert_eq!(retained_eval, "redacted offline evaluation result");
}

#[tokio::test]
async fn failed_v14_runtime_cleanup_rolls_back_rows_and_schema_version_together() {
    let path = temp_state_path("v14_atomic_runtime_cutover");
    let store = StateStore::open(Some(path.clone())).expect("open current state store");
    let created = store
        .create(request(
            "v13-rollback-run",
            "/tmp/v14-atomic-runtime-cutover",
        ))
        .await
        .expect("create pre-cutover run");
    drop(store);

    let conn = Connection::open(&path).expect("prepare failed v14 cutover");
    conn.execute_batch(
        r#"
        INSERT INTO agent_run_creations(
            command_id, command_sha256, run_id, created_at_unix_ms,
            creation_kind, workspace, source_run_id, command_json
        ) VALUES (
            'rollback-create-command', 'sha256:rollback-command', 'rollback-create-run', 1,
            'start', '/tmp/v14-atomic-runtime-cutover', NULL, '{"legacy":true}'
        );
        CREATE TRIGGER reject_v14_run_cleanup
        BEFORE DELETE ON agent_runs
        BEGIN
            SELECT RAISE(ABORT, 'injected v14 cleanup failure');
        END;
        PRAGMA user_version = 13;
        "#,
    )
    .expect("prepare v13 rollback counterexample");
    drop(conn);

    let error =
        StateStore::open(Some(path.clone())).expect_err("injected v14 cleanup must fail closed");
    assert!(
        error
            .to_string()
            .contains("failed to retire pre-Orchestrator canonical run state")
    );

    let conn = Connection::open(path).expect("inspect rolled-back v14 cutover");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read rolled-back schema version");
    assert_eq!(user_version, 13);
    let run_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_runs WHERE run_id = ?1",
            [created.lease.run_id.0],
            |row| row.get(0),
        )
        .expect("count restored pre-cutover run");
    assert_eq!(run_count, 1);
    let creation_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_run_creations WHERE command_id = 'rollback-create-command'",
            [],
            |row| row.get(0),
        )
        .expect("count restored creation intent");
    assert_eq!(creation_count, 1);
}

#[test]
fn two_state_stores_can_open_and_migrate_a_fresh_database_concurrently() {
    for _ in 0..16 {
        let path = temp_state_path("concurrent_first_open");
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let path = path.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                StateStore::open(Some(path))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }));
        }
        for worker in workers {
            worker
                .join()
                .expect("concurrent open thread panicked")
                .expect("concurrent first open failed");
        }

        let conn = Connection::open(path).expect("inspect concurrent migration");
        let user_version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read concurrent schema version");
        assert_eq!(user_version, 17);
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read concurrent journal mode");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    }
}

#[tokio::test]
async fn sqlite_and_memory_reject_stale_receipt_after_same_hash_write_epoch() {
    let path = temp_state_path("stale_receipt_epoch");
    let sqlite = StateStore::open(Some(path.clone())).expect("open SQLite store");
    let memory = InMemoryRunStore::default();
    let verifier = VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: serde_json::json!({"all_features": false, "args": ["--locked"]}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "cargo-test".to_owned(),
                program: "cargo".to_owned(),
                args: vec!["test".to_owned(), "--locked".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 600_000,
            }],
        },
    };
    let mut run_request = request("stale-receipt-run", "/tmp/stale-receipt");
    run_request
        .task_contract
        .as_mut()
        .expect("Agent task contract")
        .definition
        .acceptance = vec![TaskAcceptance::Verifier {
        id: AcceptanceId::from("tests"),
        description: "冻结测试必须通过".to_owned(),
        evidence_policy: VerifierEvidencePolicy::LatestPass,
        verifier: verifier.clone(),
    }];
    let sqlite_created = sqlite
        .create(run_request.clone())
        .await
        .expect("create SQLite run");
    let memory_created = memory.create(run_request).await.expect("create memory run");
    let run_id = sqlite_created.lease.run_id.clone();
    let generation_id = TaskGenerationId::from(run_id.0.clone());
    let revision = WorkspaceRevision::Known {
        sha256: "sha256:workspace-a".to_owned(),
    };
    let candidate = CompletionCandidate {
        id: CompletionCandidateId::from("candidate-stale-receipt"),
        generation_id: generation_id.clone(),
        message: "任务完成".to_owned(),
    };
    let observed = WorkspaceState {
        generation: 1,
        revision: revision.clone(),
    };
    let pending = |id: &str, event: RuntimeEventKind| PendingRuntimeEvent {
        event_id: RuntimeEventId(id.to_owned()),
        event,
    };

    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "workspace-observed",
            RuntimeEventKind::WorkspaceObserved {
                workspace_state: observed.clone(),
            },
        ),
    )
    .await;
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "completion-proposed",
            RuntimeEventKind::CompletionProposed {
                candidate: candidate.clone(),
            },
        ),
    )
    .await;
    let verification_id = VerificationId::from("host-verification-stale-receipt");
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "host-verification-prepared",
            RuntimeEventKind::HostVerificationPrepared {
                verification_id: verification_id.clone(),
                candidate: candidate.clone(),
                acceptance_id: AcceptanceId::from("tests"),
                verifier: verifier.clone(),
                workspace_state_before: observed,
            },
        ),
    )
    .await;
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "host-verification-started",
            RuntimeEventKind::HostVerificationStarted {
                verification_id: verification_id.clone(),
            },
        ),
    )
    .await;
    let verified_state = WorkspaceState {
        generation: 2,
        revision: revision.clone(),
    };
    let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
        summary: "deterministic verifier passed".to_owned(),
        verifier: verifier.clone(),
        verdict: VerifierVerdict::Passed,
        workspace_revision: revision.clone(),
    });
    let artifact_id = artifact.id.clone();
    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from(format!("receipt:{}", verification_id.0)),
        generation_id: generation_id.clone(),
        acceptance_id: AcceptanceId::from("tests"),
        verification_id: verification_id.clone(),
        verifier: verifier.clone(),
        workspace_state: verified_state.clone(),
        artifact_ids: vec![artifact_id.clone()],
        lineage: EvidenceLineage::LatestPass,
    };
    let mut verifier_outcome = ToolOutcome::success("deterministic verifier passed");
    verifier_outcome.workspace_revision = Some("sha256:workspace-a".to_owned());
    verifier_outcome.evidence = ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: vec![artifact_id.clone()],
    };
    verifier_outcome.artifacts = vec![artifact];
    verifier_outcome.verifier_observation = Some(VerifierObservation {
        spec: verifier.clone(),
        verdict: VerifierVerdict::Passed,
        workspace_revision: revision.clone(),
        artifact_ids: vec![artifact_id],
    });
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "host-verification-committed",
            RuntimeEventKind::HostVerificationCommitted {
                verification_id,
                outcome: Box::new(verifier_outcome),
                receipt: Some(Box::new(receipt.clone())),
                workspace_state_after: verified_state,
            },
        ),
    )
    .await;

    let operation_id = OperationId::from("write-after-receipt");
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "write-prepared",
            RuntimeEventKind::ToolPrepared {
                operation_id: operation_id.clone(),
                invocation: ToolInvocation {
                    run_id: run_id.clone(),
                    call_id: "write-after-receipt".to_owned(),
                    name: "write_file".to_owned(),
                    arguments: ToolArguments::parse(r#"{"path":"same.txt"}"#),
                },
                workspace_access: WorkspaceAccess::MayWrite,
            },
        ),
    )
    .await;
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "write-started",
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: operation_id.clone(),
            },
        ),
    )
    .await;
    let mut write_outcome = ToolOutcome::success("same bytes restored");
    write_outcome.side_effect = ToolSideEffectStatus::Applied;
    let current_state = WorkspaceState {
        generation: 3,
        revision,
    };
    append_to_both(
        &sqlite,
        &sqlite_created.lease,
        &memory,
        &memory_created.lease,
        pending(
            "write-committed",
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: "write-after-receipt".to_owned(),
                name: "write_file".to_owned(),
                outcome: Box::new(write_outcome),
                workspace_state: Some(current_state.clone()),
            },
        ),
    )
    .await;

    let terminal = PendingRuntimeEvent::terminal(AgentOutcome {
        run_id: run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::Completed {
            message: candidate.message,
            decision: CompletionDecision {
                candidate_id: candidate.id,
                generation_id,
                workspace_state: current_state,
                satisfied: vec![AcceptanceSatisfaction::Evidence {
                    acceptance_id: AcceptanceId::from("tests"),
                    receipt_id: receipt.id,
                }],
            },
        },
        accounting: ModelAccounting::default(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 1,
        details: Default::default(),
    });
    for error in [
        sqlite
            .append(&sqlite_created.lease, terminal.clone())
            .await
            .expect_err("SQLite must reject stale evidence"),
        memory
            .append(&memory_created.lease, terminal)
            .await
            .expect_err("memory must reject stale evidence"),
    ] {
        assert!(matches!(
            error,
            RunStoreError::Corrupt { ref message, .. }
                if message.contains("current contract or workspace")
        ));
    }

    let sqlite_before_reopen = sqlite
        .load(&run_id)
        .await
        .expect("load SQLite run")
        .expect("SQLite run exists");
    let memory_replay = memory
        .load(&run_id)
        .await
        .expect("load memory run")
        .expect("memory run exists");
    assert_canonical_replay_eq(&sqlite_before_reopen, &memory_replay);
    assert!(sqlite_before_reopen.snapshot.terminal.is_none());
    assert_eq!(sqlite_before_reopen.snapshot.workspace_state.generation, 3);
    assert_eq!(sqlite_before_reopen.snapshot.evidence_receipts.len(), 1);
    drop(sqlite);
    let reopened = StateStore::open(Some(path)).expect("reopen SQLite store");
    let reopened_replay = reopened
        .load(&run_id)
        .await
        .expect("reload SQLite run")
        .expect("reopened SQLite run exists");
    assert_eq!(reopened_replay, sqlite_before_reopen);
}

#[tokio::test]
async fn temporal_failure_progress_matches_memory_and_survives_sqlite_reopen() {
    let path = temp_state_path("temporal_failure_progress");
    let sqlite = StateStore::open(Some(path.clone())).expect("open temporal SQLite store");
    let memory = InMemoryRunStore::default();
    let verifier = VerifierSpec {
        verifier_id: "run_tests".to_owned(),
        parameters: serde_json::json!({"all_features": false, "args": ["--locked"]}),
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: "cargo-test".to_owned(),
                program: "cargo".to_owned(),
                args: vec!["test".to_owned(), "--locked".to_owned()],
                cwd: String::new(),
                env: BTreeMap::new(),
                timeout_ms: 600_000,
            }],
        },
    };
    let mut run_request = request("temporal-progress-run", "/tmp/temporal-progress");
    run_request
        .task_contract
        .as_mut()
        .expect("task contract")
        .definition
        .acceptance = vec![TaskAcceptance::Verifier {
        id: AcceptanceId::from("tests"),
        description: "必须先失败再修复".to_owned(),
        evidence_policy: VerifierEvidencePolicy::FailedWritePass,
        verifier: verifier.clone(),
    }];
    let sqlite_created = sqlite.create(run_request.clone()).await.unwrap();
    let memory_created = memory.create(run_request).await.unwrap();
    let workspace = known_workspace(1, "sha256:broken");
    let failed_workspace = known_workspace(2, "sha256:broken");
    let operation_id = OperationId::from("temporal-failure-operation");
    let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
        summary: "deterministic verifier failed".to_owned(),
        verifier: verifier.clone(),
        verdict: VerifierVerdict::Failed,
        workspace_revision: workspace.revision.clone(),
    });
    let artifact_id = artifact.id.clone();
    let mut outcome = ToolOutcome::error("deterministic verifier failed");
    outcome.side_effect = ToolSideEffectStatus::NotApplied;
    outcome.workspace_revision = Some("sha256:broken".to_owned());
    outcome.evidence = ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: vec![artifact_id.clone()],
    };
    outcome.artifacts = vec![artifact];
    outcome.verifier_observation = Some(VerifierObservation {
        spec: verifier.clone(),
        verdict: VerifierVerdict::Failed,
        workspace_revision: workspace.revision.clone(),
        artifact_ids: vec![artifact_id],
    });
    let events = vec![
        RuntimeEventKind::WorkspaceObserved {
            workspace_state: workspace.clone(),
        },
        RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: sqlite_created.lease.run_id.clone(),
                call_id: "temporal-failure".to_owned(),
                name: "run_tests".to_owned(),
                arguments: ToolArguments::from_value(serde_json::json!({
                    "verifier_id": "tests"
                })),
            },
            workspace_access: WorkspaceAccess::MayWrite,
        },
        RuntimeEventKind::ToolExecutionStarted {
            operation_id: operation_id.clone(),
        },
        RuntimeEventKind::ToolOutcomeCommitted {
            operation_id,
            call_id: "temporal-failure".to_owned(),
            name: "run_tests".to_owned(),
            outcome: Box::new(outcome),
            workspace_state: Some(failed_workspace.clone()),
        },
    ];
    for (index, event) in events.into_iter().enumerate() {
        append_to_both(
            &sqlite,
            &sqlite_created.lease,
            &memory,
            &memory_created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("temporal-progress-{index}")),
                event,
            },
        )
        .await;
    }
    let sqlite_replay = sqlite
        .load(&sqlite_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    let memory_replay = memory
        .load(&memory_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_canonical_replay_eq(&sqlite_replay, &memory_replay);
    let progress = sqlite_replay
        .snapshot
        .temporal_evidence_progress
        .as_ref()
        .expect("failed verifier progress");
    assert_eq!(progress.acceptance_id, AcceptanceId::from("tests"));
    assert_eq!(progress.failure.workspace_state, failed_workspace);
    assert!(progress.mutation.is_none());
    drop(sqlite);
    let reopened = StateStore::open(Some(path)).expect("reopen temporal SQLite store");
    let reopened_replay = reopened
        .load(&sqlite_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened_replay, sqlite_replay);

    let write_operation = OperationId::from("temporal-write-operation");
    let write_events = vec![
        RuntimeEventKind::ToolPrepared {
            operation_id: write_operation.clone(),
            invocation: ToolInvocation {
                run_id: sqlite_created.lease.run_id.clone(),
                call_id: "temporal-write".to_owned(),
                name: "write".to_owned(),
                arguments: ToolArguments::from_value(serde_json::json!({})),
            },
            workspace_access: WorkspaceAccess::MayWrite,
        },
        RuntimeEventKind::ToolExecutionStarted {
            operation_id: write_operation.clone(),
        },
        RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: write_operation,
            call_id: "temporal-write".to_owned(),
            name: "write".to_owned(),
            outcome: Box::new(
                ToolOutcome::success("fixed").with_side_effect(ToolSideEffectStatus::Applied),
            ),
            workspace_state: Some(known_workspace(3, "sha256:fixed")),
        },
    ];
    for (index, event) in write_events.into_iter().enumerate() {
        append_to_both(
            &reopened,
            &sqlite_created.lease,
            &memory,
            &memory_created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("temporal-write-{index}")),
                event,
            },
        )
        .await;
    }
    let after_write = reopened
        .load(&sqlite_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    let progress = after_write
        .snapshot
        .temporal_evidence_progress
        .clone()
        .expect("reopened failure progress gains one effective mutation");
    let mutation = progress.mutation.clone().expect("effective mutation");

    let candidate = CompletionCandidate {
        id: CompletionCandidateId::from("temporal-candidate"),
        generation_id: TaskGenerationId::from("temporal-progress-run"),
        message: "fixed".to_owned(),
    };
    let verification_id = VerificationId::from("temporal-host-verification");
    let verified_workspace = known_workspace(4, "sha256:fixed");
    let pass_artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
        summary: "deterministic verifier passed".to_owned(),
        verifier: verifier.clone(),
        verdict: VerifierVerdict::Passed,
        workspace_revision: verified_workspace.revision.clone(),
    });
    let pass_artifact_id = pass_artifact.id.clone();
    let mut pass_outcome = ToolOutcome::success("deterministic verifier passed");
    pass_outcome.workspace_revision = Some("sha256:fixed".to_owned());
    pass_outcome.evidence = ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: vec![pass_artifact_id.clone()],
    };
    pass_outcome.artifacts = vec![pass_artifact];
    pass_outcome.verifier_observation = Some(VerifierObservation {
        spec: verifier.clone(),
        verdict: VerifierVerdict::Passed,
        workspace_revision: verified_workspace.revision.clone(),
        artifact_ids: vec![pass_artifact_id.clone()],
    });
    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from(format!("receipt:{}", verification_id.0)),
        generation_id: candidate.generation_id.clone(),
        acceptance_id: AcceptanceId::from("tests"),
        verification_id: verification_id.clone(),
        verifier: verifier.clone(),
        workspace_state: verified_workspace.clone(),
        artifact_ids: vec![pass_artifact_id],
        lineage: EvidenceLineage::FailedWritePass {
            failure: progress.failure,
            mutation,
        },
    };
    let completion_events = vec![
        RuntimeEventKind::CompletionProposed {
            candidate: candidate.clone(),
        },
        RuntimeEventKind::HostVerificationPrepared {
            verification_id: verification_id.clone(),
            candidate,
            acceptance_id: AcceptanceId::from("tests"),
            verifier: verifier.clone(),
            workspace_state_before: known_workspace(3, "sha256:fixed"),
        },
        RuntimeEventKind::HostVerificationStarted {
            verification_id: verification_id.clone(),
        },
        RuntimeEventKind::HostVerificationCommitted {
            verification_id,
            outcome: Box::new(pass_outcome),
            receipt: Some(Box::new(receipt.clone())),
            workspace_state_after: verified_workspace,
        },
    ];
    for (index, event) in completion_events.into_iter().enumerate() {
        append_to_both(
            &reopened,
            &sqlite_created.lease,
            &memory,
            &memory_created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("temporal-completion-{index}")),
                event,
            },
        )
        .await;
    }
    let sqlite_final = reopened
        .load(&sqlite_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    let memory_final = memory
        .load(&memory_created.lease.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_canonical_replay_eq(&sqlite_final, &memory_final);
    assert_eq!(sqlite_final.snapshot.evidence_receipts, vec![receipt]);
    assert!(sqlite_final.snapshot.temporal_evidence_progress.is_none());
}

#[test]
fn newer_database_schema_fails_closed() {
    let path = temp_state_path("future_schema");
    let conn = Connection::open(&path).expect("open sqlite");
    conn.pragma_update(None, "user_version", 18)
        .expect("set future version");
    drop(conn);
    let error = StateStore::open(Some(path)).expect_err("future schema must fail");
    assert!(
        error
            .to_string()
            .contains("newer than supported version 17")
    );
}
