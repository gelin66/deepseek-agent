use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use codewhale_runtime::{
    ActorRequestAccounting, AgentOutcome, AttemptId, CommandId, CreatedRun, DurableActionState,
    InMemoryRunStore, ModelAccounting, ModelFinishReason, ModelOutput, ModelRequest, ModelToolCall,
    OperationId, PendingRuntimeEvent, RootRunRecord, RunId, RunLease, RunPurpose, RunReplay,
    RunRequest, RunStore, RunStoreError, RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent,
    TerminalState, ToolArguments, ToolArtifact, ToolArtifactStatus, ToolEvidence,
    ToolEvidenceStatus, ToolInvocation, ToolInvocationStatus, ToolOperationStatus, ToolOutcome,
    ToolRetryDisposition, ToolSideEffectStatus, ToolTransportStatus, Usage, reduce_events,
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
    let mut request = RunRequest::new("实现功能", "你是编码 Agent");
    request.run_id = Some(RunId::from(run_id));
    request.environment.workspace = workspace.to_owned();
    request
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

fn model_request(created: &CreatedRun) -> ModelRequest {
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
        request_number: 1,
        attempt: 0,
    }
}

fn terminal_event(run_id: &RunId) -> PendingRuntimeEvent {
    PendingRuntimeEvent::terminal(AgentOutcome {
        run_id: run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::Completed {
            message: "完成".to_owned(),
        },
        accounting: ModelAccounting::default(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
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
) -> Vec<(RunId, RunPurpose, Option<RunId>, String, u64, bool)> {
    records
        .into_iter()
        .map(|record| {
            assert!(record.created_at_unix_ms > 0);
            assert!(record.updated_at_unix_ms >= record.created_at_unix_ms);
            (
                record.run_id,
                record.purpose,
                record.continued_from_run_id,
                record.workspace,
                record.last_sequence,
                record.terminal,
            )
        })
        .collect()
}

fn v5_model_request(request: &RunRequest) -> ModelRequest {
    let mut transcript = request.transcript.clone();
    if !matches!(
        transcript.entries.first(),
        Some(codewhale_runtime::TranscriptEntry::System { .. })
    ) {
        transcript.entries.insert(
            0,
            codewhale_runtime::TranscriptEntry::System {
                prompt: request.system_prompt.clone(),
            },
        );
    }
    if !request.input.is_empty() {
        transcript
            .entries
            .push(codewhale_runtime::TranscriptEntry::User {
                content: request.input.clone(),
            });
    }
    ModelRequest {
        run_id: request.run_id.clone().expect("v5 fixture run id"),
        parent_run_id: request.parent_run_id.clone(),
        actor: request.actor,
        model: request.model.clone(),
        system_prompt: request.system_prompt.clone(),
        messages: transcript.project_messages(),
        tools: Vec::new(),
        reasoning_effort: request.reasoning_effort,
        max_output_tokens: request.max_output_tokens,
        streaming: request.streaming,
        request_number: 1,
        attempt: 0,
    }
}

fn v5_event(
    run_id: &RunId,
    event_id: &str,
    sequence: u64,
    event: RuntimeEventKind,
) -> StoredRuntimeEvent {
    StoredRuntimeEvent {
        // State schema v5 persisted RuntimeEvent v4. Keep this historical
        // fixture independent from the current event writer version.
        schema_version: 4,
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
            status: ToolEvidenceStatus::Verified,
            references: vec!["test://cargo/state-run-store".to_owned()],
        },
        artifacts: vec![ToolArtifact {
            id: "patch-receipt".to_owned(),
            status: ToolArtifactStatus::Available,
            sha256: Some("0123456789abcdef".to_owned()),
            media_type: Some("application/vnd.codewhale.patch+json".to_owned()),
            byte_len: Some(256),
        }],
        workspace_revision: Some("workspace-revision-after-patch".to_owned()),
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
                outcome: tool_outcome.clone(),
            },
        },
    )
    .await;

    let terminal = PendingRuntimeEvent::terminal(AgentOutcome {
        run_id: sqlite_created.lease.run_id.clone(),
        parent_run_id: None,
        terminal: TerminalState::Completed {
            message: "编码任务完成".to_owned(),
        },
        accounting: accounting.clone(),
        runtime_model_requests: 1,
        runtime_retries: 0,
        tool_calls: 1,
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
            outcome: tool_outcome,
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
        .reserve_creation(&command_id, "sha256:first", proposed.clone())
        .await
        .expect("reserve sqlite creation");
    let memory_first = memory
        .reserve_creation(&command_id, "sha256:first", proposed.clone())
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
        .reserve_creation(&command_id, "sha256:first", RunId::from("ignored-proposal"))
        .await
        .expect("retry sqlite reservation");
    let memory_retry = memory
        .reserve_creation(&command_id, "sha256:first", RunId::from("ignored-proposal"))
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
            .reserve_creation(&command_id, "sha256:different", RunId::from("other-run"))
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
        )
        .await
        .expect("retry after reopen");
    assert_eq!(reopened_retry, sqlite_retry);
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
    let mut child_request = request("workspace-newer-child", "/tmp/workspace");
    child_request.parent_run_id = Some(RunId::from("workspace-latest"));
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
    continuation.continued_from_run_id = Some(sqlite_source.lease.run_id.clone());
    continuation.input = "继续完成列表验收".to_owned();
    continuation.transcript = source.snapshot.transcript.clone();
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

    let mut compaction = request("root-list-03-context-compaction", workspace);
    compaction.continued_from_run_id = Some(sqlite_source.lease.run_id.clone());
    compaction.purpose = RunPurpose::ContextCompaction;
    compaction.input.clear();
    compaction.transcript = source.snapshot.transcript.clone();
    sqlite
        .create(compaction.clone())
        .await
        .expect("create sqlite context-compaction root");
    memory
        .create(compaction)
        .await
        .expect("create memory context-compaction root");

    let active_request = request("root-list-04-active", workspace);
    sqlite
        .create(active_request.clone())
        .await
        .expect("create sqlite active root");
    memory
        .create(active_request)
        .await
        .expect("create memory active root");

    let mut child_request = request("root-list-99-child", workspace);
    child_request.parent_run_id = Some(sqlite_source.lease.run_id.clone());
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
                RunPurpose::Agent,
                None,
                workspace.to_owned(),
                1,
                false,
            ),
            (
                RunId::from("root-list-03-context-compaction"),
                RunPurpose::ContextCompaction,
                Some(RunId::from("root-list-01-source")),
                workspace.to_owned(),
                1,
                false,
            ),
            (
                RunId::from("root-list-02-agent-continuation"),
                RunPurpose::Agent,
                Some(RunId::from("root-list-01-source")),
                workspace.to_owned(),
                2,
                false,
            ),
            (
                RunId::from("root-list-01-source"),
                RunPurpose::Agent,
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
    continuation.continued_from_run_id = Some(sqlite_source.lease.run_id.clone());
    continuation.input = "继续完成验收".to_owned();
    continuation.transcript = source_before.snapshot.transcript.clone();
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
            if content == "继续完成验收"
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

    let mut child_request = request("child-source", "/tmp/workspace");
    child_request.parent_run_id = Some(RunId::from("parent"));
    let child = store.create(child_request).await.expect("child source");
    store
        .append(
            &child.lease,
            PendingRuntimeEvent::terminal(AgentOutcome {
                run_id: child.lease.run_id.clone(),
                parent_run_id: Some(RunId::from("parent")),
                terminal: TerminalState::Completed {
                    message: "child completed".to_owned(),
                },
                accounting: ModelAccounting::default(),
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
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
async fn v5_migration_replays_and_backfills_prepared_and_in_flight_runs() {
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
    assert_eq!(user_version, 8);
    for (run_id, expected_state) in [
        ("v5-prepared", DurableActionState::Prepared),
        ("v5-in-flight", DurableActionState::InFlight),
    ] {
        let (attempt_id, in_flight): (Option<String>, i64) = conn
            .query_row(
                r#"
                SELECT pending_model_attempt_id, pending_model_in_flight
                FROM agent_runs WHERE run_id = ?1
                "#,
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read migrated pending projection");
        let expected_attempt_id = format!("{run_id}-attempt");
        assert_eq!(attempt_id.as_deref(), Some(expected_attempt_id.as_str()));
        assert_eq!(
            in_flight,
            i64::from(expected_state == DurableActionState::InFlight)
        );

        let replay = store
            .load(&RunId::from(run_id))
            .await
            .expect("load migrated run")
            .expect("migrated run exists");
        assert_eq!(
            replay
                .snapshot
                .pending_model
                .as_ref()
                .expect("pending model survives migration")
                .state,
            expected_state
        );
        assert_eq!(replay.snapshot.last_sequence, replay.events.len() as u64);
    }
}

#[test]
fn corrupt_v5_run_aborts_and_rolls_back_the_v6_migration() {
    let path = temp_state_path("v5_corrupt_migration");
    let conn = create_v5_run_store(&path);
    insert_v5_run(&conn, "v5-corrupt", DurableActionState::Prepared, false);
    conn.execute(
        "UPDATE agent_run_snapshots SET snapshot_json = '{}' WHERE run_id = 'v5-corrupt'",
        [],
    )
    .expect("corrupt v5 snapshot fixture");
    drop(conn);

    let error = StateStore::open(Some(path.clone())).expect_err("corrupt v5 run must fail closed");
    assert!(error.to_string().contains("failed to backfill"));

    let conn = Connection::open(path).expect("inspect rolled-back migration");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read rolled-back schema version");
    assert_eq!(user_version, 5);
    let v6_column_count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*) FROM pragma_table_info('agent_runs')
            WHERE name IN ('pending_model_attempt_id', 'pending_model_in_flight')
            "#,
            [],
            |row| row.get(0),
        )
        .expect("inspect rolled-back v6 columns");
    assert_eq!(v6_column_count, 0);
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
        assert_eq!(user_version, 8);
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read concurrent journal mode");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    }
}

#[test]
fn newer_database_schema_fails_closed() {
    let path = temp_state_path("future_schema");
    let conn = Connection::open(&path).expect("open sqlite");
    conn.pragma_update(None, "user_version", 9)
        .expect("set future version");
    drop(conn);
    let error = StateStore::open(Some(path)).expect_err("future schema must fail");
    assert!(error.to_string().contains("newer than supported version 8"));
}
