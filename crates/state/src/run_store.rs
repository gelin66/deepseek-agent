use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use codewhale_protocol::run_api::PendingCreationKind;
use codewhale_runtime::{
    AGENT_RUNTIME_EVENT_SCHEMA_VERSION, PendingRuntimeEvent, RunId, RunPurpose, RunRequest,
    RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent,
};
use codewhale_runtime::{
    AcquiredRun, CreatedRun, CreationIntent, CreationReservation, DurableActionState,
    ReservedCreation, RootRunRecord, RunLease, RunReplay, RunSnapshot, RunStore, RunStoreError,
    apply_event, reduce_events, validate_continuation_request,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::StateStore;

#[derive(Debug)]
struct RunProjection {
    parent_run_id: Option<RunId>,
    continued_from_run_id: Option<RunId>,
    workspace: String,
    last_sequence: u64,
    terminal: bool,
    execution_epoch: u64,
    lease_owner_id: Option<String>,
    lease_owner_pid: Option<u32>,
    pending_model_attempt_id: Option<String>,
    pending_model_in_flight: bool,
}

impl StateStore {
    fn reserve_creation_sync(
        &self,
        command_id: codewhale_runtime::CommandId,
        command_sha256: String,
        proposed_run_id: RunId,
        intent: CreationIntent,
    ) -> Result<ReservedCreation, RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let existing = tx
            .query_row(
                r#"
                SELECT command_sha256, run_id, created_at_unix_ms,
                       creation_kind, workspace, source_run_id, command_json
                FROM agent_run_creations
                WHERE command_id = ?1
                "#,
                params![command_id.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if let Some((
            existing_sha256,
            run_id,
            created_at,
            creation_kind,
            workspace,
            source_run_id,
            command_json,
        )) = existing
        {
            if existing_sha256 != command_sha256 {
                return Err(RunStoreError::CreationConflict { command_id });
            }
            let run_id = RunId(run_id);
            let stored_intent = decode_creation_intent(
                &run_id,
                creation_kind,
                workspace,
                source_run_id,
                command_json,
            )?;
            if stored_intent
                .as_ref()
                .is_some_and(|stored| stored != &intent)
            {
                return Err(RunStoreError::CreationConflict { command_id });
            }
            let reservation = CreationReservation {
                command_id,
                command_sha256: existing_sha256,
                created_at_unix_ms: from_store_u64(created_at, &run_id, "creation timestamp")?,
                intent: stored_intent,
                run_id,
            };
            tx.commit().map_err(backend)?;
            return Ok(ReservedCreation {
                reservation,
                newly_reserved: false,
            });
        }
        let created_at_unix_ms = now_unix_ms();
        let creation_kind = encode_creation_kind(intent.kind);
        let command_json = serde_json::to_string(&intent.command).map_err(backend)?;
        tx.execute(
            r#"
            INSERT INTO agent_run_creations(
                command_id, command_sha256, run_id, created_at_unix_ms,
                creation_kind, workspace, source_run_id, command_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                command_id.0,
                command_sha256,
                proposed_run_id.0,
                to_store_i64(created_at_unix_ms, "creation timestamp")?,
                creation_kind,
                intent.workspace,
                intent
                    .source_run_id
                    .as_ref()
                    .map(|run_id| run_id.0.as_str()),
                command_json,
            ],
        )
        .map_err(backend)?;
        let reservation = CreationReservation {
            command_id,
            command_sha256,
            run_id: proposed_run_id,
            created_at_unix_ms,
            intent: Some(intent),
        };
        tx.commit().map_err(backend)?;
        Ok(ReservedCreation {
            reservation,
            newly_reserved: true,
        })
    }

    fn creation_sync(
        &self,
        command_id: codewhale_runtime::CommandId,
    ) -> Result<Option<CreationReservation>, RunStoreError> {
        let conn = self.conn().map_err(backend)?;
        read_creation(&conn, &command_id)
    }

    fn list_pending_creations_sync(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<CreationReservation>, RunStoreError> {
        let conn = self.conn().map_err(backend)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT command_id, command_sha256, run_id, created_at_unix_ms,
                       creation_kind, workspace, source_run_id, command_json
                FROM agent_run_creations
                WHERE workspace = ?1 AND command_json IS NOT NULL
                ORDER BY created_at_unix_ms DESC, command_id DESC
                LIMIT ?2
                "#,
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![workspace, i64::from(limit)])
            .map_err(backend)?;
        let mut reservations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            reservations.push(decode_creation_row(row)?);
        }
        Ok(reservations)
    }

    fn create_run_sync(&self, mut request: RunRequest) -> Result<CreatedRun, RunStoreError> {
        let run_id = request.run_id.clone().unwrap_or_default();
        request.run_id = Some(run_id.clone());
        let lease = new_lease(run_id.clone(), 1);
        let now = now_unix_ms();
        let created = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: run_id.clone(),
            parent_run_id: request.parent_run_id.clone(),
            event_id: RuntimeEventId::run_created(),
            sequence: 1,
            occurred_at_unix_ms: now,
            event: codewhale_runtime::RuntimeEventKind::RunCreated {
                request: Box::new(request.clone()),
            },
        };
        let events = vec![created.clone()];
        let snapshot = reduce_events(&events)?;
        let replay = RunReplay {
            snapshot: snapshot.clone(),
            events,
        };

        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE run_id = ?1)",
                params![run_id.0],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if exists {
            return Err(RunStoreError::AlreadyExists { run_id });
        }
        if let Some(source_run_id) = request.continued_from_run_id.clone() {
            let source_projection = read_run_projection(&tx, &source_run_id)?.ok_or_else(|| {
                RunStoreError::NotFound {
                    run_id: source_run_id.clone(),
                }
            })?;
            let source = replay_from_conn(&tx, &source_run_id, &source_projection)?;
            validate_continuation_request(&source.snapshot, &request)?;
            validate_continuation_lineage(&tx, &source.snapshot, &request.environment.workspace)?;
        }

        tx.execute(
            r#"
            INSERT INTO agent_runs(
                run_id, parent_run_id, continued_from_run_id, workspace, last_sequence, terminal,
                execution_epoch, lease_owner_id, lease_owner_pid,
                created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, 1, 0, 1, ?5, ?6, ?7, ?7)
            "#,
            params![
                run_id.0,
                request.parent_run_id.as_ref().map(|id| id.0.as_str()),
                request
                    .continued_from_run_id
                    .as_ref()
                    .map(|id| id.0.as_str()),
                request.environment.workspace,
                lease.owner_id,
                i64::from(lease.owner_pid),
                to_store_i64(now, "run timestamp")?,
            ],
        )
        .map_err(backend)?;
        insert_event(&tx, &created)?;
        upsert_snapshot(&tx, &run_id, &snapshot)?;
        tx.execute(
            "UPDATE agent_run_creations SET command_json = NULL WHERE run_id = ?1",
            params![run_id.0],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(CreatedRun {
            lease,
            created,
            replay,
        })
    }

    fn acquire_run_sync(&self, run_id: RunId) -> Result<AcquiredRun, RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let projection =
            read_run_projection(&tx, &run_id)?.ok_or_else(|| RunStoreError::NotFound {
                run_id: run_id.clone(),
            })?;
        let replay = replay_from_conn(&tx, &run_id, &projection)?;
        if replay.snapshot.terminal.is_some() {
            tx.commit().map_err(backend)?;
            return Ok(AcquiredRun {
                lease: None,
                replay,
            });
        }
        if let Some(owner_pid) = projection.lease_owner_pid
            && process_is_alive(owner_pid)
        {
            return Err(RunStoreError::AlreadyRunning { run_id });
        }

        let next_epoch = projection.execution_epoch.saturating_add(1);
        let lease = new_lease(run_id.clone(), next_epoch);
        let changed = tx
            .execute(
                r#"
                UPDATE agent_runs
                SET execution_epoch = ?2, lease_owner_id = ?3, lease_owner_pid = ?4
                WHERE run_id = ?1 AND execution_epoch = ?5
                "#,
                params![
                    run_id.0,
                    to_store_i64(next_epoch, "execution epoch")?,
                    lease.owner_id,
                    i64::from(lease.owner_pid),
                    to_store_i64(projection.execution_epoch, "execution epoch")?,
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RunStoreError::StaleLease {
                run_id,
                epoch: projection.execution_epoch,
            });
        }
        tx.commit().map_err(backend)?;
        Ok(AcquiredRun {
            lease: Some(lease),
            replay,
        })
    }

    fn append_run_event_sync(
        &self,
        lease: RunLease,
        pending: PendingRuntimeEvent,
    ) -> Result<StoredRuntimeEvent, RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;

        if let Some(existing) = read_event_by_id(&tx, &lease.run_id, &pending.event_id)? {
            if existing.event == pending.event {
                tx.commit().map_err(backend)?;
                return Ok(existing);
            }
            return Err(RunStoreError::EventConflict {
                run_id: lease.run_id,
                event_id: pending.event_id,
            });
        }

        let projection =
            read_run_projection(&tx, &lease.run_id)?.ok_or_else(|| RunStoreError::NotFound {
                run_id: lease.run_id.clone(),
            })?;
        if projection.terminal {
            return Err(RunStoreError::AlreadyTerminal {
                run_id: lease.run_id,
            });
        }
        if projection.execution_epoch != lease.epoch
            || projection.lease_owner_id.as_deref() != Some(lease.owner_id.as_str())
            || projection.lease_owner_pid != Some(lease.owner_pid)
        {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id,
                epoch: lease.epoch,
            });
        }

        let sequence = projection.last_sequence.saturating_add(1);
        let stored = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: lease.run_id.clone(),
            parent_run_id: projection.parent_run_id.clone(),
            event_id: pending.event_id,
            sequence,
            occurred_at_unix_ms: now_unix_ms(),
            event: pending.event,
        };
        let projection_neutral = snapshot_projection_neutral(&stored.event);
        if projection_neutral {
            validate_projection_neutral_event(&stored.event, &lease.run_id, &projection)?;
        }
        let mut snapshot = (!projection_neutral)
            .then(|| read_persisted_snapshot(&tx, &lease.run_id, &projection))
            .transpose()?;
        if let Some(snapshot) = snapshot.as_mut() {
            apply_event(snapshot, &stored)?;
        }
        let terminal = stored.event.is_terminal();

        insert_event(&tx, &stored)?;
        if let Some(snapshot) = snapshot.as_ref() {
            upsert_snapshot(&tx, &lease.run_id, snapshot)?;
        } else {
            advance_snapshot_sequence(
                &tx,
                &lease.run_id,
                projection.last_sequence,
                stored.sequence,
            )?;
        }
        let (pending_model_attempt_id, pending_model_in_flight) = snapshot
            .as_ref()
            .map(snapshot_model_projection)
            .unwrap_or_else(|| {
                (
                    projection.pending_model_attempt_id.clone(),
                    projection.pending_model_in_flight,
                )
            });
        let changed = tx
            .execute(
                r#"
                UPDATE agent_runs
                SET last_sequence = ?2, terminal = ?3,
                    lease_owner_id = CASE WHEN ?3 = 1 THEN NULL ELSE lease_owner_id END,
                    lease_owner_pid = CASE WHEN ?3 = 1 THEN NULL ELSE lease_owner_pid END,
                    updated_at_unix_ms = ?4,
                    pending_model_attempt_id = ?5,
                    pending_model_in_flight = ?6
                WHERE run_id = ?1
                  AND execution_epoch = ?7
                  AND lease_owner_id = ?8
                  AND lease_owner_pid = ?9
                "#,
                params![
                    lease.run_id.0,
                    to_store_i64(sequence, "event sequence")?,
                    i64::from(terminal),
                    to_store_i64(stored.occurred_at_unix_ms, "event timestamp")?,
                    pending_model_attempt_id,
                    i64::from(pending_model_in_flight),
                    to_store_i64(lease.epoch, "execution epoch")?,
                    lease.owner_id,
                    i64::from(lease.owner_pid),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id,
                epoch: lease.epoch,
            });
        }
        tx.commit().map_err(backend)?;
        Ok(stored)
    }

    fn load_run_sync(&self, run_id: RunId) -> Result<Option<RunReplay>, RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(backend)?;
        let Some(projection) = read_run_projection(&tx, &run_id)? else {
            tx.commit().map_err(backend)?;
            return Ok(None);
        };
        let replay = replay_from_conn(&tx, &run_id, &projection)?;
        tx.commit().map_err(backend)?;
        Ok(Some(replay))
    }

    fn events_after_sync(
        &self,
        run_id: RunId,
        sequence: u64,
    ) -> Result<Vec<StoredRuntimeEvent>, RunStoreError> {
        let conn = self.conn().map_err(backend)?;
        read_events_after(&conn, &run_id, sequence)
    }

    fn release_run_sync(&self, lease: RunLease) -> Result<(), RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        let projection =
            read_run_projection(&tx, &lease.run_id)?.ok_or_else(|| RunStoreError::NotFound {
                run_id: lease.run_id.clone(),
            })?;
        if projection.execution_epoch != lease.epoch
            || projection.lease_owner_id.as_deref() != Some(lease.owner_id.as_str())
            || projection.lease_owner_pid != Some(lease.owner_pid)
        {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id,
                epoch: lease.epoch,
            });
        }
        let changed = tx
            .execute(
                r#"
                UPDATE agent_runs
                SET lease_owner_id = NULL, lease_owner_pid = NULL
                WHERE run_id = ?1 AND execution_epoch = ?2
                  AND lease_owner_id = ?3 AND lease_owner_pid = ?4
                "#,
                params![
                    lease.run_id.0,
                    to_store_i64(lease.epoch, "execution epoch")?,
                    lease.owner_id,
                    i64::from(lease.owner_pid),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id,
                epoch: lease.epoch,
            });
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn list_root_runs_sync(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<RootRunRecord>, RunStoreError> {
        let mut conn = self.conn().map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(backend)?;
        let mut records = {
            let mut statement = tx
                .prepare(
                    r#"
                    SELECT run_id, continued_from_run_id, workspace, last_sequence,
                           terminal, created_at_unix_ms, updated_at_unix_ms
                    FROM agent_runs
                    WHERE workspace = ?1 AND parent_run_id IS NULL
                    ORDER BY updated_at_unix_ms DESC, run_id DESC
                    LIMIT ?2
                    "#,
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![workspace, i64::from(limit)])
                .map_err(backend)?;
            let mut records = Vec::new();
            while let Some(row) = rows.next().map_err(backend)? {
                let run_id = RunId(row.get::<_, String>(0).map_err(backend)?);
                records.push(RootRunRecord {
                    run_id: run_id.clone(),
                    purpose: RunPurpose::Agent,
                    continued_from_run_id: row
                        .get::<_, Option<String>>(1)
                        .map_err(backend)?
                        .map(RunId),
                    workspace: row.get(2).map_err(backend)?,
                    last_sequence: from_store_u64(
                        row.get(3).map_err(backend)?,
                        &run_id,
                        "last_sequence",
                    )?,
                    terminal: match row.get::<_, i64>(4).map_err(backend)? {
                        0 => false,
                        1 => true,
                        _ => return Err(corrupt(&run_id, "terminal projection is not boolean")),
                    },
                    created_at_unix_ms: from_store_u64(
                        row.get(5).map_err(backend)?,
                        &run_id,
                        "created_at_unix_ms",
                    )?,
                    updated_at_unix_ms: from_store_u64(
                        row.get(6).map_err(backend)?,
                        &run_id,
                        "updated_at_unix_ms",
                    )?,
                });
            }
            records
        };
        for record in &mut records {
            let projection = read_run_projection(&tx, &record.run_id)?.ok_or_else(|| {
                RunStoreError::NotFound {
                    run_id: record.run_id.clone(),
                }
            })?;
            let replay = replay_from_conn(&tx, &record.run_id, &projection)?;
            if replay.snapshot.request.environment.workspace != workspace {
                return Err(RunStoreError::Corrupt {
                    run_id: record.run_id.clone(),
                    message: "workspace projection disagrees with run snapshot".to_owned(),
                });
            }
            if replay.snapshot.request.parent_run_id.is_some()
                || replay.snapshot.request.continued_from_run_id != record.continued_from_run_id
                || replay.snapshot.last_sequence != record.last_sequence
                || replay.snapshot.terminal.is_some() != record.terminal
            {
                return Err(corrupt(
                    &record.run_id,
                    "root run query projection disagrees with canonical replay",
                ));
            }
            record.purpose = replay.snapshot.request.purpose;
        }
        tx.commit().map_err(backend)?;
        Ok(records)
    }
}

fn read_creation(
    conn: &Connection,
    command_id: &codewhale_runtime::CommandId,
) -> Result<Option<CreationReservation>, RunStoreError> {
    conn.query_row(
        r#"
        SELECT command_id, command_sha256, run_id, created_at_unix_ms,
               creation_kind, workspace, source_run_id, command_json
        FROM agent_run_creations
        WHERE command_id = ?1
        "#,
        params![command_id.0],
        decode_creation_row_sql,
    )
    .optional()
    .map_err(backend)?
    .map(decode_creation_columns)
    .transpose()
}

type CreationColumns = (
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn decode_creation_row_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<CreationColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn decode_creation_row(row: &rusqlite::Row<'_>) -> Result<CreationReservation, RunStoreError> {
    decode_creation_columns(decode_creation_row_sql(row).map_err(backend)?)
}

fn decode_creation_columns(
    (
        command_id,
        command_sha256,
        run_id,
        created_at,
        creation_kind,
        workspace,
        source_run_id,
        command_json,
    ): CreationColumns,
) -> Result<CreationReservation, RunStoreError> {
    let run_id = RunId(run_id);
    Ok(CreationReservation {
        command_id: codewhale_runtime::CommandId::from(command_id),
        command_sha256,
        created_at_unix_ms: from_store_u64(created_at, &run_id, "creation timestamp")?,
        intent: decode_creation_intent(
            &run_id,
            creation_kind,
            workspace,
            source_run_id,
            command_json,
        )?,
        run_id,
    })
}

fn decode_creation_intent(
    run_id: &RunId,
    creation_kind: Option<String>,
    workspace: Option<String>,
    source_run_id: Option<String>,
    command_json: Option<String>,
) -> Result<Option<CreationIntent>, RunStoreError> {
    let Some(command_json) = command_json else {
        return Ok(None);
    };
    let kind = match creation_kind.as_deref() {
        Some("start") => PendingCreationKind::Start,
        Some("continue") => PendingCreationKind::Continue,
        Some("compact") => PendingCreationKind::Compact,
        _ => {
            return Err(corrupt(
                run_id,
                "pending creation kind is missing or invalid",
            ));
        }
    };
    let workspace =
        workspace.ok_or_else(|| corrupt(run_id, "pending creation workspace is missing"))?;
    let source_run_id = source_run_id.map(RunId);
    let command = serde_json::from_str(&command_json).map_err(|error| {
        corrupt(
            run_id,
            format!("pending creation command is invalid: {error}"),
        )
    })?;
    let metadata_matches = match (&kind, &command) {
        (PendingCreationKind::Start, codewhale_protocol::run_api::RunCommand::Start(command)) => {
            source_run_id.is_none() && command.workspace == workspace
        }
        (
            PendingCreationKind::Continue,
            codewhale_protocol::run_api::RunCommand::Continue(command),
        ) => {
            source_run_id.as_ref() == Some(&command.run_id)
                && command
                    .expected_workspace
                    .as_ref()
                    .is_none_or(|expected| expected == &workspace)
        }
        (
            PendingCreationKind::Compact,
            codewhale_protocol::run_api::RunCommand::Compact(command),
        ) => {
            source_run_id.as_ref() == Some(&command.run_id)
                && command
                    .expected_workspace
                    .as_ref()
                    .is_none_or(|expected| expected == &workspace)
        }
        _ => false,
    };
    if !metadata_matches {
        return Err(corrupt(
            run_id,
            "pending creation metadata disagrees with its canonical command",
        ));
    }
    Ok(Some(CreationIntent {
        kind,
        workspace,
        source_run_id,
        command,
    }))
}

const fn encode_creation_kind(kind: PendingCreationKind) -> &'static str {
    match kind {
        PendingCreationKind::Start => "start",
        PendingCreationKind::Continue => "continue",
        PendingCreationKind::Compact => "compact",
    }
}

/// Validate the schema-v7 continuation projection against canonical RunCreated
/// events. Existing rows are expected to have a null continuation id; any
/// disagreement aborts migration rather than manufacturing lineage.
pub(super) fn validate_v7_continuation_projections(conn: &Connection) -> Result<(), RunStoreError> {
    let run_ids = {
        let mut statement = conn
            .prepare("SELECT run_id FROM agent_runs ORDER BY run_id")
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut run_ids = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            run_ids.push(RunId(row.get::<_, String>(0).map_err(backend)?));
        }
        run_ids
    };
    for run_id in run_ids {
        let projection =
            read_run_projection(conn, &run_id)?.ok_or_else(|| RunStoreError::NotFound {
                run_id: run_id.clone(),
            })?;
        let replay = replay_from_conn(conn, &run_id, &projection)?;
        if replay.snapshot.request.continued_from_run_id != projection.continued_from_run_id {
            return Err(corrupt(
                &run_id,
                "continuation projection disagrees with canonical replay during schema migration",
            ));
        }
        validate_continuation_lineage(
            conn,
            &replay.snapshot,
            &replay.snapshot.request.environment.workspace,
        )?;
    }
    Ok(())
}

fn validate_continuation_lineage(
    conn: &Connection,
    source: &RunSnapshot,
    workspace: &str,
) -> Result<(), RunStoreError> {
    let source_run_id = source.request.run_id.clone().unwrap_or_default();
    let invalid = || RunStoreError::InvalidContinuation {
        source_run_id: source_run_id.clone(),
        reason: codewhale_runtime::ContinuationError::LineageCorrupt,
    };
    let mut visited = HashSet::from([source_run_id.clone()]);
    let mut cursor = source.request.continued_from_run_id.clone();
    while let Some(run_id) = cursor {
        if !visited.insert(run_id.clone()) {
            return Err(invalid());
        }
        let projection = read_run_projection(conn, &run_id)?.ok_or_else(invalid)?;
        let replay = replay_from_conn(conn, &run_id, &projection)?;
        let snapshot = replay.snapshot;
        if snapshot.request.parent_run_id.is_some()
            || snapshot.request.actor.kind != codewhale_runtime::AgentActorKind::Root
            || snapshot.request.actor.depth != 0
            || snapshot.request.environment.workspace != workspace
            || snapshot.terminal.is_none()
        {
            return Err(invalid());
        }
        cursor = snapshot.request.continued_from_run_id;
    }
    Ok(())
}

/// Populate the schema-v6 fast projection from the canonical schema-v5 event
/// log. This runs inside the same migration transaction that adds the two
/// columns: any malformed event, stale snapshot, or disagreeing projection
/// aborts the whole migration instead of manufacturing resumable state.
pub(super) fn backfill_v6_pending_model_projections(
    conn: &Connection,
) -> Result<(), RunStoreError> {
    let run_ids = {
        let mut statement = conn
            .prepare("SELECT run_id FROM agent_runs ORDER BY run_id")
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut run_ids = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            run_ids.push(RunId(row.get::<_, String>(0).map_err(backend)?));
        }
        run_ids
    };

    for run_id in run_ids {
        let mut projection =
            read_run_projection(conn, &run_id)?.ok_or_else(|| RunStoreError::NotFound {
                run_id: run_id.clone(),
            })?;
        let events = read_events_after(conn, &run_id, 0)?;
        let snapshot = reduce_events(&events)?;
        if snapshot.last_sequence != projection.last_sequence {
            return Err(corrupt(
                &run_id,
                "last_sequence projection disagrees with event log during schema migration",
            ));
        }
        if snapshot.terminal.is_some() != projection.terminal {
            return Err(corrupt(
                &run_id,
                "terminal projection disagrees with event log during schema migration",
            ));
        }
        if snapshot.request.parent_run_id != projection.parent_run_id {
            return Err(corrupt(
                &run_id,
                "parent run projection disagrees with event log during schema migration",
            ));
        }
        if snapshot.request.continued_from_run_id != projection.continued_from_run_id {
            return Err(corrupt(
                &run_id,
                "continuation projection disagrees with event log during schema migration",
            ));
        }
        if snapshot.request.environment.workspace != projection.workspace {
            return Err(corrupt(
                &run_id,
                "workspace projection disagrees with event log during schema migration",
            ));
        }

        let (attempt_id, in_flight) = snapshot_model_projection(&snapshot);
        let changed = conn
            .execute(
                r#"
                UPDATE agent_runs
                SET pending_model_attempt_id = ?2, pending_model_in_flight = ?3
                WHERE run_id = ?1
                "#,
                params![run_id.0, attempt_id, i64::from(in_flight)],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(corrupt(
                &run_id,
                "pending model projection was not backfilled exactly once",
            ));
        }
        projection.pending_model_attempt_id = attempt_id;
        projection.pending_model_in_flight = in_flight;

        let persisted = read_persisted_snapshot(conn, &run_id, &projection)?;
        if persisted != snapshot {
            return Err(corrupt(
                &run_id,
                "stored snapshot disagrees with canonical event replay during schema migration",
            ));
        }
    }
    Ok(())
}

#[async_trait]
impl RunStore for StateStore {
    async fn reserve_creation(
        &self,
        command_id: &codewhale_runtime::CommandId,
        command_sha256: &str,
        proposed_run_id: RunId,
        intent: CreationIntent,
    ) -> Result<ReservedCreation, RunStoreError> {
        self.reserve_creation_sync(
            command_id.clone(),
            command_sha256.to_owned(),
            proposed_run_id,
            intent,
        )
    }

    async fn creation(
        &self,
        command_id: &codewhale_runtime::CommandId,
    ) -> Result<Option<CreationReservation>, RunStoreError> {
        let store = self.clone();
        let command_id = command_id.clone();
        tokio::task::spawn_blocking(move || store.creation_sync(command_id))
            .await
            .map_err(join_error)?
    }

    async fn list_pending_creations(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<CreationReservation>, RunStoreError> {
        let store = self.clone();
        let workspace = workspace.to_owned();
        tokio::task::spawn_blocking(move || store.list_pending_creations_sync(workspace, limit))
            .await
            .map_err(join_error)?
    }

    async fn create(&self, request: RunRequest) -> Result<CreatedRun, RunStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.create_run_sync(request))
            .await
            .map_err(join_error)?
    }

    async fn acquire(&self, run_id: &RunId) -> Result<AcquiredRun, RunStoreError> {
        let store = self.clone();
        let run_id = run_id.clone();
        tokio::task::spawn_blocking(move || store.acquire_run_sync(run_id))
            .await
            .map_err(join_error)?
    }

    async fn append(
        &self,
        lease: &RunLease,
        event: PendingRuntimeEvent,
    ) -> Result<StoredRuntimeEvent, RunStoreError> {
        let store = self.clone();
        let lease = lease.clone();
        tokio::task::spawn_blocking(move || store.append_run_event_sync(lease, event))
            .await
            .map_err(join_error)?
    }

    async fn load(&self, run_id: &RunId) -> Result<Option<RunReplay>, RunStoreError> {
        let store = self.clone();
        let run_id = run_id.clone();
        tokio::task::spawn_blocking(move || store.load_run_sync(run_id))
            .await
            .map_err(join_error)?
    }

    async fn events_after(
        &self,
        run_id: &RunId,
        sequence: u64,
    ) -> Result<Vec<StoredRuntimeEvent>, RunStoreError> {
        let store = self.clone();
        let run_id = run_id.clone();
        tokio::task::spawn_blocking(move || store.events_after_sync(run_id, sequence))
            .await
            .map_err(join_error)?
    }

    async fn release(&self, lease: &RunLease) -> Result<(), RunStoreError> {
        let store = self.clone();
        let lease = lease.clone();
        tokio::task::spawn_blocking(move || store.release_run_sync(lease))
            .await
            .map_err(join_error)?
    }

    async fn list_root_runs(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<RootRunRecord>, RunStoreError> {
        let store = self.clone();
        let workspace = workspace.to_owned();
        tokio::task::spawn_blocking(move || store.list_root_runs_sync(workspace, limit))
            .await
            .map_err(join_error)?
    }
}

fn insert_event(conn: &Connection, event: &StoredRuntimeEvent) -> Result<(), RunStoreError> {
    let event_json = serde_json::to_string(event).map_err(backend)?;
    conn.execute(
        r#"
        INSERT INTO agent_run_events(
            run_id, sequence, event_id, schema_version,
            occurred_at_unix_ms, terminal, event_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event.run_id.0,
            to_store_i64(event.sequence, "event sequence")?,
            event.event_id.0,
            i64::from(event.schema_version),
            to_store_i64(event.occurred_at_unix_ms, "event timestamp")?,
            i64::from(event.event.is_terminal()),
            event_json,
        ],
    )
    .map_err(backend)?;
    Ok(())
}

fn upsert_snapshot(
    conn: &Connection,
    run_id: &RunId,
    snapshot: &RunSnapshot,
) -> Result<(), RunStoreError> {
    let snapshot_json = serde_json::to_string(snapshot).map_err(backend)?;
    conn.execute(
        r#"
        INSERT INTO agent_run_snapshots(run_id, last_sequence, snapshot_json)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(run_id) DO UPDATE SET
            last_sequence = excluded.last_sequence,
            snapshot_json = excluded.snapshot_json
        "#,
        params![
            run_id.0,
            to_store_i64(snapshot.last_sequence, "snapshot sequence")?,
            snapshot_json,
        ],
    )
    .map_err(backend)?;
    Ok(())
}

/// Streaming deltas are canonical events, but they do not change resumable
/// state. Keep them append-only without repeatedly serializing the growing
/// transcript snapshot. The next state-changing event materializes the same
/// reducer view from the persisted snapshot at the current logical sequence.
fn snapshot_projection_neutral(event: &RuntimeEventKind) -> bool {
    matches!(
        event,
        RuntimeEventKind::ContentDelta { .. } | RuntimeEventKind::ReasoningDelta { .. }
    )
}

fn validate_projection_neutral_event(
    event: &RuntimeEventKind,
    run_id: &RunId,
    projection: &RunProjection,
) -> Result<(), RunStoreError> {
    let attempt_id = match event {
        RuntimeEventKind::ContentDelta { attempt_id, .. }
        | RuntimeEventKind::ReasoningDelta { attempt_id, .. } => attempt_id,
        _ => return Ok(()),
    };
    if projection.pending_model_attempt_id.as_deref() != Some(attempt_id.0.as_str())
        || !projection.pending_model_in_flight
    {
        return Err(corrupt(
            run_id,
            "model delta does not match an in-flight request",
        ));
    }
    Ok(())
}

fn snapshot_model_projection(snapshot: &RunSnapshot) -> (Option<String>, bool) {
    snapshot
        .pending_model
        .as_ref()
        .map_or((None, false), |pending| {
            (
                Some(pending.attempt_id.0.clone()),
                pending.state == DurableActionState::InFlight,
            )
        })
}

fn advance_snapshot_sequence(
    conn: &Connection,
    run_id: &RunId,
    previous_sequence: u64,
    next_sequence: u64,
) -> Result<(), RunStoreError> {
    let changed = conn
        .execute(
            r#"
            UPDATE agent_run_snapshots
            SET last_sequence = ?2
            WHERE run_id = ?1 AND last_sequence = ?3
            "#,
            params![
                run_id.0,
                to_store_i64(next_sequence, "snapshot sequence")?,
                to_store_i64(previous_sequence, "snapshot sequence")?,
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(corrupt(
            run_id,
            "snapshot sequence did not advance atomically",
        ));
    }
    Ok(())
}

fn read_run_projection(
    conn: &Connection,
    run_id: &RunId,
) -> Result<Option<RunProjection>, RunStoreError> {
    let raw = conn
        .query_row(
            r#"
            SELECT parent_run_id, continued_from_run_id, workspace, last_sequence,
                   terminal, execution_epoch,
                   lease_owner_id, lease_owner_pid, pending_model_attempt_id,
                   pending_model_in_flight
            FROM agent_runs WHERE run_id = ?1
            "#,
            params![run_id.0],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((
        parent,
        continued_from,
        workspace,
        last,
        terminal,
        epoch,
        owner_id,
        owner_pid,
        pending_model_attempt_id,
        pending_model_in_flight,
    )) = raw
    else {
        return Ok(None);
    };
    let projection = RunProjection {
        parent_run_id: parent.map(RunId),
        continued_from_run_id: continued_from.map(RunId),
        workspace,
        last_sequence: from_store_u64(last, run_id, "last_sequence")?,
        terminal: match terminal {
            0 => false,
            1 => true,
            _ => return Err(corrupt(run_id, "terminal projection is not boolean")),
        },
        execution_epoch: from_store_u64(epoch, run_id, "execution_epoch")?,
        lease_owner_id: owner_id,
        lease_owner_pid: owner_pid
            .map(|pid| {
                u32::try_from(pid)
                    .map_err(|_| corrupt(run_id, "lease_owner_pid is outside u32 range"))
            })
            .transpose()?,
        pending_model_attempt_id,
        pending_model_in_flight: match pending_model_in_flight {
            0 => false,
            1 => true,
            _ => return Err(corrupt(run_id, "pending_model_in_flight is not boolean")),
        },
    };
    if projection.lease_owner_id.is_some() != projection.lease_owner_pid.is_some() {
        return Err(corrupt(
            run_id,
            "lease owner id and pid are not both present",
        ));
    }
    if projection.execution_epoch == 0 {
        return Err(corrupt(run_id, "execution_epoch is zero"));
    }
    if projection.pending_model_in_flight && projection.pending_model_attempt_id.is_none() {
        return Err(corrupt(
            run_id,
            "in-flight model projection has no attempt id",
        ));
    }
    Ok(Some(projection))
}

fn replay_from_conn(
    conn: &Connection,
    run_id: &RunId,
    projection: &RunProjection,
) -> Result<RunReplay, RunStoreError> {
    let events = read_events_after(conn, run_id, 0)?;
    let snapshot = reduce_events(&events)?;
    if snapshot.last_sequence != projection.last_sequence {
        return Err(corrupt(
            run_id,
            "last_sequence projection disagrees with event log",
        ));
    }
    if snapshot.terminal.is_some() != projection.terminal {
        return Err(corrupt(
            run_id,
            "terminal projection disagrees with event log",
        ));
    }
    if snapshot.request.parent_run_id != projection.parent_run_id {
        return Err(corrupt(
            run_id,
            "parent run projection disagrees with event log",
        ));
    }
    if snapshot.request.continued_from_run_id != projection.continued_from_run_id {
        return Err(corrupt(
            run_id,
            "continuation projection disagrees with event log",
        ));
    }
    if snapshot.request.environment.workspace != projection.workspace {
        return Err(corrupt(
            run_id,
            "workspace projection disagrees with event log",
        ));
    }
    let persisted = read_persisted_snapshot(conn, run_id, projection)?;
    if persisted != snapshot {
        return Err(corrupt(
            run_id,
            "stored snapshot disagrees with canonical event replay",
        ));
    }
    Ok(RunReplay { snapshot, events })
}

fn read_persisted_snapshot(
    conn: &Connection,
    run_id: &RunId,
    projection: &RunProjection,
) -> Result<RunSnapshot, RunStoreError> {
    let stored_snapshot = conn
        .query_row(
            "SELECT last_sequence, snapshot_json FROM agent_run_snapshots WHERE run_id = ?1",
            params![run_id.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(backend)?
        .ok_or_else(|| corrupt(run_id, "run snapshot is missing"))?;
    let snapshot_sequence = from_store_u64(stored_snapshot.0, run_id, "snapshot sequence")?;
    let mut persisted: RunSnapshot = serde_json::from_str(&stored_snapshot.1)
        .map_err(|error| corrupt(run_id, format!("run snapshot JSON is invalid: {error}")))?;
    if snapshot_sequence != projection.last_sequence
        || persisted.last_sequence > snapshot_sequence
        || persisted.request.run_id.as_ref() != Some(run_id)
        || persisted.request.parent_run_id != projection.parent_run_id
        || persisted.request.continued_from_run_id != projection.continued_from_run_id
        || persisted.request.environment.workspace != projection.workspace
        || persisted.terminal.is_some() != projection.terminal
        || snapshot_model_projection(&persisted)
            != (
                projection.pending_model_attempt_id.clone(),
                projection.pending_model_in_flight,
            )
    {
        return Err(corrupt(
            run_id,
            "stored snapshot disagrees with run projection",
        ));
    }
    // The table column is the logical projection sequence. JSON may lag only
    // across ContentDelta/ReasoningDelta events, which are reducer no-ops.
    // Full load/acquire still replays every event and compares the canonical
    // reducer result with this materialized view, catching index corruption.
    persisted.last_sequence = snapshot_sequence;
    Ok(persisted)
}

fn read_event_by_id(
    conn: &Connection,
    run_id: &RunId,
    event_id: &RuntimeEventId,
) -> Result<Option<StoredRuntimeEvent>, RunStoreError> {
    let raw = conn
        .query_row(
            r#"
            SELECT sequence, event_id, schema_version, occurred_at_unix_ms, terminal, event_json
            FROM agent_run_events WHERE run_id = ?1 AND event_id = ?2
            "#,
            params![run_id.0, event_id.0],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    raw.map(|row| decode_event_row(run_id, row)).transpose()
}

fn read_events_after(
    conn: &Connection,
    run_id: &RunId,
    sequence: u64,
) -> Result<Vec<StoredRuntimeEvent>, RunStoreError> {
    if sequence > i64::MAX as u64 {
        return Ok(Vec::new());
    }
    let mut statement = conn
        .prepare(
            r#"
            SELECT sequence, event_id, schema_version, occurred_at_unix_ms, terminal, event_json
            FROM agent_run_events
            WHERE run_id = ?1 AND sequence > ?2
            ORDER BY sequence ASC
            "#,
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![run_id.0, to_store_i64(sequence, "event cursor")?])
        .map_err(backend)?;
    let mut events = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let raw = (
            row.get::<_, i64>(0).map_err(backend)?,
            row.get::<_, String>(1).map_err(backend)?,
            row.get::<_, i64>(2).map_err(backend)?,
            row.get::<_, i64>(3).map_err(backend)?,
            row.get::<_, i64>(4).map_err(backend)?,
            row.get::<_, String>(5).map_err(backend)?,
        );
        events.push(decode_event_row(run_id, raw)?);
    }
    Ok(events)
}

fn decode_event_row(
    run_id: &RunId,
    (sequence, event_id, schema_version, occurred_at, terminal, event_json): (
        i64,
        String,
        i64,
        i64,
        i64,
        String,
    ),
) -> Result<StoredRuntimeEvent, RunStoreError> {
    let event: StoredRuntimeEvent = serde_json::from_str(&event_json)
        .map_err(|error| corrupt(run_id, format!("event JSON is invalid: {error}")))?;
    let db_sequence = from_store_u64(sequence, run_id, "event sequence")?;
    let db_schema = u32::try_from(schema_version)
        .map_err(|_| corrupt(run_id, "event schema_version is outside u32 range"))?;
    let db_occurred = from_store_u64(occurred_at, run_id, "event timestamp")?;
    let db_terminal = match terminal {
        0 => false,
        1 => true,
        _ => return Err(corrupt(run_id, "event terminal marker is not boolean")),
    };
    if event.run_id != *run_id
        || event.sequence != db_sequence
        || event.event_id.0 != event_id
        || event.schema_version != db_schema
        || event.occurred_at_unix_ms != db_occurred
        || event.event.is_terminal() != db_terminal
    {
        return Err(corrupt(
            run_id,
            "event columns disagree with canonical event JSON",
        ));
    }
    Ok(event)
}

fn new_lease(run_id: RunId, epoch: u64) -> RunLease {
    RunLease {
        run_id,
        epoch,
        owner_id: RuntimeEventId::new().0,
        owner_pid: std::process::id(),
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 performs permission/liveness probing and sends no signal.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: the handle is query-only and is closed before returning.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if !handle.is_null() {
        // SAFETY: `handle` was returned by OpenProcess and is owned here.
        unsafe { CloseHandle(handle) };
        return true;
    }
    // Access denied still proves that a process owns the PID. Other errors,
    // including an invalid PID, mean the recorded owner is gone.
    unsafe { GetLastError() == ERROR_ACCESS_DENIED }
}

#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn to_store_i64(value: u64, field: &str) -> Result<i64, RunStoreError> {
    i64::try_from(value).map_err(|_| RunStoreError::Backend {
        message: format!("{field} exceeds SQLite INTEGER range"),
    })
}

fn from_store_u64(value: i64, run_id: &RunId, field: &str) -> Result<u64, RunStoreError> {
    u64::try_from(value).map_err(|_| corrupt(run_id, format!("{field} is negative")))
}

fn corrupt(run_id: &RunId, message: impl Into<String>) -> RunStoreError {
    RunStoreError::Corrupt {
        run_id: run_id.clone(),
        message: message.into(),
    }
}

fn backend(error: impl std::fmt::Display) -> RunStoreError {
    RunStoreError::Backend {
        message: error.to_string(),
    }
}

fn join_error(error: tokio::task::JoinError) -> RunStoreError {
    RunStoreError::Backend {
        message: format!("run store blocking task failed: {error}"),
    }
}
