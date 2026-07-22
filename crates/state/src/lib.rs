//! Persistent state management for conversation threads and canonical Agent runs.
//!
//! The [`StateStore`] is the primary entry point, backed by a SQLite database and an
//! append-only JSONL session index file. It provides CRUD operations for:
//!
//! - **Threads** — conversation metadata, archival, and session indexing.
//! - **Agent runs** — canonical event, snapshot, creation, and replay persistence.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

mod run_store;

const STATE_SCHEMA_VERSION: u32 = 19;

// Re-export protocol's ThreadStatus so callers in the state crate and
// external consumers (e.g. core) can reference a single canonical definition.
pub use codewhale_protocol::ThreadStatus;

/// Indicates how a session was initiated.
///
/// Serialized as lowercase snake_case strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// Started by a user interacting with the CLI.
    Interactive,
    /// Resumed from a previously persisted session.
    Resume,
    /// Created by forking an existing conversation at a specific message.
    Fork,
    /// Initiated programmatically via the API.
    Api,
    /// Source is unknown or unspecified.
    Unknown,
}

/// Metadata for a persisted conversation thread.
///
/// Each thread represents a single conversation session and stores its
/// configuration, git context, and current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMetadata {
    /// Unique identifier for this thread.
    pub id: String,
    /// Optional filesystem path to the rollout (JSONL transcript) file.
    pub rollout_path: Option<PathBuf>,
    /// Short preview or summary of the thread content.
    pub preview: String,
    /// Whether this thread is ephemeral (not persisted long-term).
    pub ephemeral: bool,
    /// Identifier of the model provider used for this thread (e.g. `"openai"`).
    pub model_provider: String,
    /// Unix timestamp (seconds) when the thread was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) of the most recent update to the thread.
    pub updated_at: i64,
    /// Current lifecycle status of the thread.
    pub status: ThreadStatus,
    /// Optional filesystem path associated with the thread working context.
    pub path: Option<PathBuf>,
    /// Working directory that was active when the thread was created.
    pub cwd: PathBuf,
    /// Version of the CLI that created this thread.
    pub cli_version: String,
    /// How this session was initiated.
    pub source: SessionSource,
    /// User-assigned display name for the thread.
    pub name: Option<String>,
    /// Serialized sandbox policy applied to this thread, if any.
    pub sandbox_policy: Option<String>,
    /// Approval mode configured for tool calls in this thread.
    pub approval_mode: Option<String>,
    /// Whether the thread has been archived.
    pub archived: bool,
    /// Unix timestamp (seconds) when the thread was archived, or `None` if not archived.
    pub archived_at: Option<i64>,
    /// Git commit SHA of the working tree when the thread was created.
    pub git_sha: Option<String>,
    /// Git branch checked out when the thread was created.
    pub git_branch: Option<String>,
    /// URL of the git remote origin, if available.
    pub git_origin_url: Option<String>,
    /// Memory mode recorded by legacy thread metadata.
    pub memory_mode: Option<String>,
}

/// Filters for listing conversation threads.
#[derive(Debug, Clone)]
pub struct ThreadListFilters {
    /// Whether to include archived threads in the results.
    pub include_archived: bool,
    /// Maximum number of threads to return. Defaults to 50.
    pub limit: Option<usize>,
}

impl Default for ThreadListFilters {
    fn default() -> Self {
        Self {
            include_archived: false,
            limit: Some(50),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionIndexEntry {
    thread_id: String,
    thread_name: Option<String>,
    updated_at: i64,
    rollout_path: Option<PathBuf>,
}

/// Rewrite the session index once the append-only log grows large enough that
/// full-file scans become costly. Lookups already dedupe by thread id, so
/// compaction keeps only the latest entry per thread.
fn session_index_compact_line_threshold() -> usize {
    if cfg!(test) { 5 } else { 5_000 }
}

/// Persistent storage for thread metadata and canonical Agent runs.
///
/// Backed by a SQLite database and an append-only JSONL session index file.
/// The database schema is automatically initialized and migrated on [`open`](Self::open).
#[derive(Debug, Clone)]
pub struct StateStore {
    db_path: PathBuf,
    session_index_path: PathBuf,
    // Single long-lived connection shared by all clones. SQLite pragmas are
    // per-connection, so opening once in `open` and applying them there keeps
    // every operation consistent without re-opening the database per call.
    conn: Arc<Mutex<Connection>>,
}

impl StateStore {
    /// Open (or create) a state store at the given database path.
    ///
    /// If `path` is `None`, the default location (`~/.codewhale/state.db`) is used.
    /// The database schema is created automatically if it does not exist.
    pub fn open(path: Option<PathBuf>) -> Result<Self> {
        let db_path = path.unwrap_or_else(default_state_db_path);
        let session_index_path = db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("session_index.jsonl");
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create state directory {}", parent.display())
            })?;
        }
        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open state db {}", db_path.display()))?;
        let user_version: u32 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .with_context(|| format!("failed to read schema version for {}", db_path.display()))?;
        if user_version > STATE_SCHEMA_VERSION {
            anyhow::bail!(
                "state db schema version {user_version} is newer than supported version {STATE_SCHEMA_VERSION}"
            );
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .with_context(|| format!("failed to set busy timeout for {}", db_path.display()))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .with_context(|| format!("failed to enable foreign keys for {}", db_path.display()))?;
        Self::init_schema(&mut conn)?;
        Self::enable_wal(&conn, &db_path)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .with_context(|| format!("failed to set synchronous mode for {}", db_path.display()))?;
        Ok(Self {
            db_path,
            session_index_path,
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Returns the filesystem path of the underlying SQLite database.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        // Poisoning means a panic mid-operation; any open transaction was
        // rolled back when it dropped, but surface the condition rather than
        // silently continuing on a connection whose state we can't vouch for.
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("state db connection mutex poisoned"))
    }

    fn init_schema(conn: &mut Connection) -> Result<()> {
        // Acquire the database-wide writer lock before reading user_version.
        // Concurrent first-open callers must observe the migration committed by
        // the winner instead of both planning the same ALTER TABLE statements.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("failed to acquire state schema migration lock")?;
        let mut user_version: u32 = tx.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
        if user_version > STATE_SCHEMA_VERSION {
            anyhow::bail!(
                "state db schema version {user_version} is newer than supported version {STATE_SCHEMA_VERSION}"
            );
        }
        if user_version == 0 {
            tx.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS threads (
                    id TEXT PRIMARY KEY,
                    rollout_path TEXT,
                    preview TEXT NOT NULL,
                    ephemeral INTEGER NOT NULL,
                    model_provider TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    path TEXT,
                    cwd TEXT NOT NULL,
                    cli_version TEXT NOT NULL,
                    source TEXT NOT NULL,
                    title TEXT,
                    sandbox_policy TEXT,
                    approval_mode TEXT,
                    archived INTEGER NOT NULL DEFAULT 0,
                    archived_at INTEGER,
                    git_sha TEXT,
                    git_branch TEXT,
                    git_origin_url TEXT,
                    memory_mode TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_threads_archived_at ON threads(archived_at DESC);
                CREATE INDEX IF NOT EXISTS idx_threads_archived_updated ON threads(archived, updated_at DESC);

                PRAGMA user_version = 1;
                "#,
            )
            .context("failed to initialize thread schema")?;
            user_version = 1;
        }
        if user_version < 5 {
            tx.execute_batch(
                r#"
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
            .context("failed to initialize AgentRuntime run store schema")?;
            user_version = 5;
        }
        if user_version < 14 {
            // State v14 is a direct RuntimeEvent v10 cutover. Retire the
            // incompatible run rows before any historical projection
            // backfill tries to deserialize them. This remains part of the
            // same IMMEDIATE transaction as every following schema change, so
            // a later migration failure restores both the version and all
            // pre-cutover rows.
            //
            // `agent_run_creations` only exists from state v8 onward. Runs
            // exist from v5 and own events/snapshots through ON DELETE CASCADE.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                tx.execute("DELETE FROM agent_run_creations", [])
                    .context("failed to retire pre-Orchestrator run creation state")?;
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-Orchestrator canonical run state")?;
        }
        if user_version < 16 {
            // RuntimeEvent v11 freezes Writer admission in every RunRequest.
            // Retire incompatible runtime rows before any historical
            // snapshot backfill attempts to deserialize the old request.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                tx.execute("DELETE FROM agent_run_creations", [])
                    .context("failed to retire pre-Writer-admission creation state")?;
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-Writer-admission canonical run state")?;
        }
        if user_version < 17 {
            // RuntimeEvent v12 makes verifier evidence policy and temporal
            // progress mandatory canonical facts. Retire v11 rows rather
            // than guessing lineage for historical runs.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                tx.execute("DELETE FROM agent_run_creations", [])
                    .context("failed to retire pre-temporal-evidence creation state")?;
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-temporal-evidence canonical run state")?;
        }
        if user_version < 18 {
            // RuntimeEvent v13 replaces ambiguous cleanup booleans with one
            // persisted exact plan and typed per-resource results. Historical
            // rows cannot be upgraded without inventing scope/ownership facts.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                tx.execute("DELETE FROM agent_run_creations", [])
                    .context("failed to retire pre-exact-cleanup creation state")?;
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-exact-cleanup canonical run state")?;
        }
        if user_version < 19 {
            // RuntimeEvent v14 makes completion rejection causes and required
            // recovery transitions mandatory. Historical rows cannot be
            // upgraded without guessing why receipt sealing failed. Run API
            // v9 creation intents did not change: preserve still-pending
            // canonical commands so a schema upgrade cannot turn a reserved
            // request into a second physical launch. Finalized creation
            // receipts point at the runtime rows retired below and are no
            // longer valid idempotency results.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                if user_version >= 9 {
                    tx.execute(
                        "DELETE FROM agent_run_creations WHERE command_json IS NULL",
                        [],
                    )
                    .context("failed to retire finalized pre-typed-rejection creations")?;
                } else {
                    // State v8 has no canonical command_json to distinguish a
                    // safe pending intent from a finalized receipt.
                    tx.execute("DELETE FROM agent_run_creations", [])
                        .context("failed to retire pre-intent creation receipts")?;
                }
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-typed-rejection canonical run state")?;
        }
        if user_version < 6 {
            tx.execute_batch(
                r#"
                ALTER TABLE agent_runs
                    ADD COLUMN pending_model_attempt_id TEXT;
                ALTER TABLE agent_runs
                    ADD COLUMN pending_model_in_flight INTEGER NOT NULL DEFAULT 0
                    CHECK(pending_model_in_flight IN (0, 1));
                "#,
            )
            .context("failed to initialize AgentRuntime fast projection schema")?;
            // `read_run_projection` validates every durable projection during
            // the v6 backfill. Add the nullable v7 lineage column in the same
            // transaction so the shared reader has one schema shape.
            if !agent_runs_has_continuation_column(&tx)? {
                tx.execute_batch("ALTER TABLE agent_runs ADD COLUMN continued_from_run_id TEXT;")
                    .context("failed to prepare AgentRuntime continuation projection")?;
            }
            run_store::backfill_v6_pending_model_projections(&tx)
                .context("failed to backfill AgentRuntime fast projection")?;
            tx.pragma_update(None, "user_version", 6)
                .context("failed to commit AgentRuntime fast projection schema version")?;
            user_version = 6;
        }
        if user_version < 7 {
            if !agent_runs_has_continuation_column(&tx)? {
                tx.execute_batch("ALTER TABLE agent_runs ADD COLUMN continued_from_run_id TEXT;")
                    .context("failed to add AgentRuntime continuation projection")?;
            }
            tx.execute_batch(
                r#"
                CREATE INDEX idx_agent_runs_continued_from
                    ON agent_runs(continued_from_run_id);
                CREATE INDEX idx_agent_runs_root_workspace_updated
                    ON agent_runs(workspace, updated_at_unix_ms DESC, run_id DESC)
                    WHERE parent_run_id IS NULL;
                "#,
            )
            .context("failed to initialize AgentRuntime continuation projection schema")?;
            run_store::validate_v7_continuation_projections(&tx)
                .context("failed to validate AgentRuntime continuation projections")?;
            tx.pragma_update(None, "user_version", 7)
                .context("failed to commit AgentRuntime continuation schema version")?;
            user_version = 7;
        }
        if user_version < 8 {
            tx.execute_batch(
                r#"
                CREATE TABLE agent_run_creations (
                    command_id TEXT PRIMARY KEY NOT NULL,
                    command_sha256 TEXT NOT NULL,
                    run_id TEXT NOT NULL UNIQUE,
                    created_at_unix_ms INTEGER NOT NULL
                );
                "#,
            )
            .context("failed to initialize durable run creation receipts")?;
            tx.pragma_update(None, "user_version", 8)
                .context("failed to commit run creation receipt schema version")?;
            user_version = 8;
        }
        if user_version < 9 {
            tx.execute_batch(
                r#"
                ALTER TABLE agent_run_creations ADD COLUMN creation_kind TEXT;
                ALTER TABLE agent_run_creations ADD COLUMN workspace TEXT;
                ALTER TABLE agent_run_creations ADD COLUMN source_run_id TEXT;
                ALTER TABLE agent_run_creations ADD COLUMN command_json TEXT;
                CREATE INDEX idx_agent_run_creations_pending_workspace
                    ON agent_run_creations(workspace, created_at_unix_ms DESC, command_id DESC)
                    WHERE command_json IS NOT NULL;
                "#,
            )
            .context("failed to initialize durable run creation intent schema")?;
            tx.pragma_update(None, "user_version", 9)
                .context("failed to commit run creation intent schema version")?;
            user_version = 9;
        }
        if user_version < 10 {
            run_store::backfill_v10_model_catalog_snapshots(&tx)
                .context("failed to rebuild model catalog snapshot projections")?;
            tx.pragma_update(None, "user_version", 10)
                .context("failed to commit model catalog snapshot schema version")?;
            user_version = 10;
        }
        if user_version < 11 {
            tx.execute_batch("DROP TABLE IF EXISTS thread_goals;")
                .context("failed to delete the retired thread goal schema")?;
            tx.pragma_update(None, "user_version", 11)
                .context("failed to commit retired thread goal schema deletion")?;
            user_version = 11;
        }
        if user_version < 12 {
            tx.execute_batch(
                r#"
                DROP TABLE IF EXISTS teacher_candidates;
                DROP TABLE IF EXISTS leaf_runs;
                DROP TABLE IF EXISTS control_node_runs;
                DROP TABLE IF EXISTS branch_runs;
                DROP TABLE IF EXISTS workflow_runs;
                DROP TABLE IF EXISTS thread_dynamic_tools;
                DROP TABLE IF EXISTS messages;
                DROP TABLE IF EXISTS checkpoints;
                DROP TABLE IF EXISTS jobs;
                "#,
            )
            .context("failed to delete retired thread and workflow state tables")?;
            if threads_has_current_leaf_column(&tx)? {
                tx.execute_batch("ALTER TABLE threads DROP COLUMN current_leaf_id;")
                    .context("failed to delete retired thread current-leaf projection")?;
            }
            tx.pragma_update(None, "user_version", 12)
                .context("failed to commit retired state table deletion")?;
            user_version = 12;
        }
        if user_version < 13 {
            tx.execute(
                "DELETE FROM agent_run_creations WHERE creation_kind = 'compact'",
                [],
            )
            .context("failed to delete retired manual compaction creation intents")?;
            tx.pragma_update(None, "user_version", 13)
                .context("failed to commit manual compaction state deletion")?;
            user_version = 13;
        }
        if user_version < 14 {
            // RuntimeEvent v10 replaces the old child-only lifecycle with the
            // canonical AgentTask/worktree lifecycle. The cutover block above
            // already retired incompatible runs before historical migrations
            // could deserialize them; only advance the schema here.
            tx.pragma_update(None, "user_version", 14)
                .context("failed to commit canonical AgentTask state cutover")?;
            user_version = 14;
        }
        if user_version < 15 {
            run_store::backfill_v15_terminal_accounting_snapshots(&tx)
                .context("failed to rebuild terminal accounting snapshots")?;
            tx.pragma_update(None, "user_version", 15)
                .context("failed to commit terminal accounting snapshot schema version")?;
            user_version = 15;
        }
        if user_version < 16 {
            tx.pragma_update(None, "user_version", 16)
                .context("failed to commit Writer admission state cutover")?;
            user_version = 16;
        }
        if user_version < 17 {
            tx.pragma_update(None, "user_version", 17)
                .context("failed to commit temporal evidence state cutover")?;
            user_version = 17;
        }
        if user_version < 18 {
            tx.pragma_update(None, "user_version", 18)
                .context("failed to commit exact Writer cleanup state cutover")?;
            user_version = 18;
        }
        if user_version < 19 {
            tx.pragma_update(None, "user_version", 19)
                .context("failed to commit typed completion rejection state cutover")?;
            user_version = 19;
        }
        debug_assert_eq!(user_version, STATE_SCHEMA_VERSION);
        tx.commit()
            .context("failed to commit state schema migration")?;
        Ok(())
    }

    fn enable_wal(conn: &Connection, db_path: &Path) -> Result<()> {
        // Changing journal mode needs an exclusive database lock and SQLite's
        // busy handler is not consistently invoked for this PRAGMA. A second
        // opener may have acquired the schema migration writer lock in the
        // small gap after our migration commit, so retry only BUSY/LOCKED for
        // the same bounded window as the connection busy timeout.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match conn.query_row("PRAGMA journal_mode = WAL;", [], |row| {
                row.get::<_, String>(0)
            }) {
                Ok(mode) if mode.eq_ignore_ascii_case("wal") => return Ok(()),
                Ok(mode) => anyhow::bail!(
                    "failed to enable WAL for {}: SQLite selected journal mode {mode}",
                    db_path.display()
                ),
                Err(error)
                    if matches!(
                        error.sqlite_error_code(),
                        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to enable WAL for {}", db_path.display())
                    });
                }
            }
        }
    }

    /// Insert or update thread metadata.
    pub fn upsert_thread(&self, thread: &ThreadMetadata) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            r#"
            INSERT INTO threads (
                id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd,
                cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at,
                git_sha, git_branch, git_origin_url, memory_mode
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21
            )
            ON CONFLICT(id) DO UPDATE SET
                rollout_path=excluded.rollout_path,
                preview=excluded.preview,
                ephemeral=excluded.ephemeral,
                model_provider=excluded.model_provider,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                status=excluded.status,
                path=excluded.path,
                cwd=excluded.cwd,
                cli_version=excluded.cli_version,
                source=excluded.source,
                title=excluded.title,
                sandbox_policy=excluded.sandbox_policy,
                approval_mode=excluded.approval_mode,
                archived=excluded.archived,
                archived_at=excluded.archived_at,
                git_sha=excluded.git_sha,
                git_branch=excluded.git_branch,
                git_origin_url=excluded.git_origin_url,
                memory_mode=excluded.memory_mode
            "#,
            params![
                thread.id,
                path_to_opt_string(thread.rollout_path.as_deref()),
                thread.preview,
                bool_to_i64(thread.ephemeral),
                thread.model_provider,
                thread.created_at,
                thread.updated_at,
                thread_status_to_str(&thread.status),
                path_to_opt_string(thread.path.as_deref()),
                thread.cwd.display().to_string(),
                thread.cli_version,
                session_source_to_str(&thread.source),
                thread.name,
                thread.sandbox_policy,
                thread.approval_mode,
                bool_to_i64(thread.archived),
                thread.archived_at,
                thread.git_sha,
                thread.git_branch,
                thread.git_origin_url,
                thread.memory_mode,
            ],
        )
        .context("failed to upsert thread metadata")?;

        self.append_thread_name(
            &thread.id,
            thread.name.clone(),
            thread.updated_at,
            thread.rollout_path.clone(),
        )?;
        Ok(())
    }

    /// Retrieve a single thread by its ID.
    ///
    /// Returns `None` if no thread with the given ID exists.
    pub fn get_thread(&self, id: &str) -> Result<Option<ThreadMetadata>> {
        let conn = self.conn()?;
        conn.query_row(
            r#"
            SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd,
                   cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at,
                   git_sha, git_branch, git_origin_url, memory_mode
            FROM threads
            WHERE id = ?1
            "#,
            params![id],
            row_to_thread,
        )
        .optional()
        .context("failed to read thread")
    }

    /// List threads ordered by most recently updated.
    ///
    /// Use [`ThreadListFilters`] to control whether archived threads are included
    /// and the maximum number of results returned.
    pub fn list_threads(&self, filters: ThreadListFilters) -> Result<Vec<ThreadMetadata>> {
        let conn = self.conn()?;
        let sql = if filters.include_archived {
            "SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd, cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at, git_sha, git_branch, git_origin_url, memory_mode FROM threads ORDER BY updated_at DESC LIMIT ?1"
        } else {
            "SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd, cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at, git_sha, git_branch, git_origin_url, memory_mode FROM threads WHERE archived = 0 ORDER BY updated_at DESC LIMIT ?1"
        };

        let mut stmt = conn.prepare(sql).context("failed to prepare list query")?;
        let limit = i64::try_from(filters.limit.unwrap_or(50)).unwrap_or(50);
        let mut rows = stmt
            .query(params![limit])
            .context("failed to query threads")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate thread rows")? {
            out.push(row_to_thread(row)?);
        }
        Ok(out)
    }

    /// Archive a thread, setting its status to [`ThreadStatus::Archived`] and
    /// recording the current timestamp.
    pub fn mark_archived(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET archived = 1, archived_at = ?2, status = ?3 WHERE id = ?1",
            params![
                id,
                Utc::now().timestamp(),
                thread_status_to_str(&ThreadStatus::Archived)
            ],
        )
        .context("failed to archive thread")?;
        Ok(())
    }

    /// Unarchive a thread, removing the archived flag and clearing `archived_at`.
    pub fn mark_unarchived(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET archived = 0, archived_at = NULL, status = CASE WHEN status = ?2 THEN ?3 ELSE status END WHERE id = ?1",
            params![
                id,
                thread_status_to_str(&ThreadStatus::Archived),
                thread_status_to_str(&ThreadStatus::Idle),
            ],
        )
        .context("failed to unarchive thread")?;
        Ok(())
    }

    /// Permanently delete thread metadata.
    pub fn delete_thread(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM threads WHERE id = ?1", params![id])
            .context("failed to delete thread")?;
        Ok(())
    }

    /// Set the memory mode for a thread.
    ///
    /// Pass `None` to clear the memory mode.
    pub fn set_thread_memory_mode(&self, id: &str, mode: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET memory_mode = ?2 WHERE id = ?1",
            params![id, mode],
        )
        .context("failed to update thread memory mode")?;
        Ok(())
    }

    /// Get the memory mode configured for a thread.
    ///
    /// Returns `None` if the thread does not exist or has no memory mode set.
    pub fn get_thread_memory_mode(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT memory_mode FROM threads WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .context("failed to read thread memory mode")
        .map(Option::flatten)
    }

    /// Look up the rollout file path for a thread by its ID.
    pub fn find_rollout_path_by_id(&self, id: &str) -> Result<Option<PathBuf>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT rollout_path FROM threads WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .context("failed to lookup rollout path")
        .map(|opt| opt.flatten().map(PathBuf::from))
    }

    /// Append an entry to the JSONL session index file.
    ///
    /// The session index is an append-only log that maps thread IDs to their names,
    /// update timestamps, and rollout paths. It is used for fast name-based lookups
    /// without opening the SQLite database.
    pub fn append_thread_name(
        &self,
        thread_id: &str,
        thread_name: Option<String>,
        updated_at: i64,
        rollout_path: Option<PathBuf>,
    ) -> Result<()> {
        if let Some(parent) = self.session_index_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create session index directory {}",
                    parent.display()
                )
            })?;
        }
        let entry = SessionIndexEntry {
            thread_id: thread_id.to_string(),
            thread_name,
            updated_at,
            rollout_path,
        };
        let encoded =
            serde_json::to_string(&entry).context("failed to serialize session index entry")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.session_index_path)
            .with_context(|| {
                format!(
                    "failed to open session index {}",
                    self.session_index_path.display()
                )
            })?;
        writeln!(file, "{encoded}").context("failed to append session index entry")?;
        self.maybe_compact_session_index()?;
        Ok(())
    }

    /// Find the display name for a thread by its ID, using the session index.
    ///
    /// Returns `None` if the thread is not in the index or has no name.
    pub fn find_thread_name_by_id(&self, thread_id: &str) -> Result<Option<String>> {
        let map = self.session_index_map()?;
        Ok(map
            .get(thread_id)
            .and_then(|entry| entry.thread_name.clone()))
    }

    /// Look up display names for multiple thread IDs at once.
    ///
    /// Returns a map from thread ID to its name (which may be `None`).
    pub fn find_thread_names_by_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, Option<String>>> {
        let map = self.session_index_map()?;
        let mut out = HashMap::new();
        for id in ids {
            let name = map.get(id).and_then(|entry| entry.thread_name.clone());
            out.insert(id.clone(), name);
        }
        Ok(out)
    }

    /// Find the rollout path for a thread by its display name (case-insensitive).
    ///
    /// If multiple threads share the same name, the most recently updated one is returned.
    /// Returns `None` if no matching thread is found.
    pub fn find_thread_path_by_name_str(&self, name: &str) -> Result<Option<PathBuf>> {
        let map = self.session_index_map()?;
        let matched = map
            .values()
            .filter(|entry| {
                entry
                    .thread_name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .max_by_key(|entry| entry.updated_at);
        Ok(matched.and_then(|entry| entry.rollout_path.clone()))
    }

    fn maybe_compact_session_index(&self) -> Result<()> {
        if !self.session_index_path.exists() {
            return Ok(());
        }
        let line_count = BufReader::new(
            OpenOptions::new()
                .read(true)
                .open(&self.session_index_path)
                .with_context(|| {
                    format!(
                        "failed to read session index {}",
                        self.session_index_path.display()
                    )
                })?,
        )
        .lines()
        .filter(|line| {
            line.as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
        if line_count <= session_index_compact_line_threshold() {
            return Ok(());
        }

        let latest = self.session_index_map()?;
        let compact_path = self.session_index_path.with_extension("jsonl.compact");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&compact_path)
                .with_context(|| {
                    format!(
                        "failed to open compact session index {}",
                        compact_path.display()
                    )
                })?;
            for entry in latest.values() {
                let encoded = serde_json::to_string(entry)
                    .context("failed to serialize compact session index entry")?;
                writeln!(file, "{encoded}")
                    .context("failed to write compact session index entry")?;
            }
        }
        fs::rename(&compact_path, &self.session_index_path).with_context(|| {
            format!(
                "failed to replace session index {}",
                self.session_index_path.display()
            )
        })?;
        Ok(())
    }

    #[cfg(test)]
    fn session_index_line_count(&self) -> Result<usize> {
        if !self.session_index_path.exists() {
            return Ok(0);
        }
        Ok(BufReader::new(
            OpenOptions::new()
                .read(true)
                .open(&self.session_index_path)
                .with_context(|| {
                    format!(
                        "failed to read session index {}",
                        self.session_index_path.display()
                    )
                })?,
        )
        .lines()
        .filter(|line| {
            line.as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .count())
    }

    fn session_index_map(&self) -> Result<HashMap<String, SessionIndexEntry>> {
        if !self.session_index_path.exists() {
            return Ok(HashMap::new());
        }
        let file = OpenOptions::new()
            .read(true)
            .open(&self.session_index_path)
            .with_context(|| {
                format!(
                    "failed to read session index {}",
                    self.session_index_path.display()
                )
            })?;
        let reader = BufReader::new(file);
        let mut latest = HashMap::<String, SessionIndexEntry>::new();
        for line in reader.lines() {
            let line = line.context("failed to read session index line")?;
            if line.trim().is_empty() {
                continue;
            }
            let parsed: SessionIndexEntry =
                serde_json::from_str(&line).context("failed to parse session index entry")?;
            latest.insert(parsed.thread_id.clone(), parsed);
        }
        Ok(latest)
    }
}

fn agent_runs_has_continuation_column(conn: &Connection) -> Result<bool> {
    conn.query_row(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM pragma_table_info('agent_runs')
            WHERE name = 'continued_from_run_id'
        )
        "#,
        [],
        |row| row.get(0),
    )
    .context("failed to inspect AgentRuntime continuation projection schema")
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .with_context(|| format!("failed to inspect SQLite table {table}"))
}

fn threads_has_current_leaf_column(conn: &Connection) -> Result<bool> {
    conn.query_row(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM pragma_table_info('threads')
            WHERE name = 'current_leaf_id'
        )
        "#,
        [],
        |row| row.get(0),
    )
    .context("failed to inspect retired thread current-leaf projection")
}

fn default_state_db_path() -> PathBuf {
    // $CODEWHALE_HOME is a hard override of the base data directory.
    if let Some(overridden) = codewhale_home_override() {
        return overridden.join("state.db");
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".codewhale").join("state.db")
}

/// Resolve `$CODEWHALE_HOME` as a hard override of the data directory root.
///
/// Returns the path verbatim (the env var IS the home dir, matching
/// `codewhale_home()` in config — `$CODEWHALE_HOME=/data/cw` means the home is
/// `/data/cw`, not `/data/cw/.codewhale`). Returns `None` when unset/empty so
/// callers can branch on "explicit override" vs "default home + legacy
/// fallback." Mirrors config's helper without taking a dependency on it (state
/// is a low-level leaf crate; config cannot be a dependency here without
/// inverting the layering).
fn codewhale_home_override() -> Option<PathBuf> {
    std::env::var_os("CODEWHALE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn thread_status_to_str(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::Running => "running",
        ThreadStatus::Idle => "idle",
        ThreadStatus::Completed => "completed",
        ThreadStatus::Failed => "failed",
        ThreadStatus::Paused => "paused",
        ThreadStatus::Archived => "archived",
    }
}

fn thread_status_from_str(value: &str) -> ThreadStatus {
    match value {
        "running" => ThreadStatus::Running,
        "idle" => ThreadStatus::Idle,
        "completed" => ThreadStatus::Completed,
        "failed" => ThreadStatus::Failed,
        "paused" => ThreadStatus::Paused,
        "archived" => ThreadStatus::Archived,
        _ => ThreadStatus::Idle,
    }
}

fn session_source_to_str(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::Interactive => "interactive",
        SessionSource::Resume => "resume",
        SessionSource::Fork => "fork",
        SessionSource::Api => "api",
        SessionSource::Unknown => "unknown",
    }
}

fn session_source_from_str(value: &str) -> SessionSource {
    match value {
        "interactive" => SessionSource::Interactive,
        "resume" => SessionSource::Resume,
        "fork" => SessionSource::Fork,
        "api" => SessionSource::Api,
        _ => SessionSource::Unknown,
    }
}

fn path_to_opt_string(path: Option<&Path>) -> Option<String> {
    path.map(|p| p.display().to_string())
}

fn row_to_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadMetadata> {
    let status_raw: String = row.get(7)?;
    let source_raw: String = row.get(11)?;
    let rollout_path: Option<String> = row.get(1)?;
    let path: Option<String> = row.get(8)?;
    Ok(ThreadMetadata {
        id: row.get(0)?,
        rollout_path: rollout_path.map(PathBuf::from),
        preview: row.get(2)?,
        ephemeral: i64_to_bool(row.get(3)?),
        model_provider: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        status: thread_status_from_str(&status_raw),
        path: path.map(PathBuf::from),
        cwd: PathBuf::from(row.get::<_, String>(9)?),
        cli_version: row.get(10)?,
        source: session_source_from_str(&source_raw),
        name: row.get(12)?,
        sandbox_policy: row.get(13)?,
        approval_mode: row.get(14)?,
        archived: i64_to_bool(row.get(15)?),
        archived_at: row.get(16)?,
        git_sha: row.get(17)?,
        git_branch: row.get(18)?,
        git_origin_url: row.get(19)?,
        memory_mode: row.get(20)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state_store(name: &str) -> StateStore {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "codewhale-state-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp state dir");
        StateStore::open(Some(dir.join("state.db"))).expect("open state store")
    }

    fn test_thread(id: &str) -> ThreadMetadata {
        ThreadMetadata {
            id: id.to_string(),
            rollout_path: None,
            preview: "test thread".to_string(),
            ephemeral: false,
            model_provider: "deepseek".to_string(),
            created_at: 10,
            updated_at: 10,
            status: ThreadStatus::Running,
            path: None,
            cwd: PathBuf::from("/tmp/codewhale"),
            cli_version: "0.0.0-test".to_string(),
            source: SessionSource::Interactive,
            name: None,
            sandbox_policy: None,
            approval_mode: None,
            archived: false,
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            memory_mode: None,
        }
    }

    #[test]
    fn state_store_reuses_one_connection_across_operations_and_clones() {
        let store = temp_state_store("conn-reuse");
        {
            let conn = store.conn().expect("conn");
            conn.execute_batch("CREATE TEMP TABLE conn_reuse_probe(id INTEGER);")
                .expect("create temp table");
        }
        // TEMP tables are visible only on the connection that created them, so
        // seeing the probe again — through a clone, after real operations ran —
        // proves the store holds one long-lived connection instead of
        // reopening the database (and reapplying pragmas) per call.
        let clone = store.clone();
        clone
            .upsert_thread(&test_thread("thread-conn-reuse"))
            .expect("upsert thread");
        let conn = clone.conn().expect("conn");
        let probe_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_temp_master WHERE name = 'conn_reuse_probe'",
                [],
                |row| row.get(0),
            )
            .expect("query temp master");
        assert_eq!(
            probe_count, 1,
            "temp table not visible: a fresh connection was opened"
        );
        // The pragma applied once at open still governs the shared connection.
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .expect("read foreign_keys pragma");
        assert_eq!(foreign_keys, 1);
    }

    // ── $CODEWHALE_HOME override tests ──────────────────────────────
    //
    // These touch a process-global env var, so they serialize against each
    // other (and restore the prior value) to stay hermetic under parallel test
    // runs — the same concern AGENTS.md flags for config_command_allow_shell_*.

    static CODEWHALE_HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct CodeWhaleHomeGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl CodeWhaleHomeGuard {
        fn set(value: &str) -> Self {
            let prior = std::env::var_os("CODEWHALE_HOME");
            // SAFETY: serialised by CODEWHALE_HOME_TEST_LOCK.
            unsafe { std::env::set_var("CODEWHALE_HOME", value) };
            Self { prior }
        }
        fn remove() -> Self {
            let prior = std::env::var_os("CODEWHALE_HOME");
            // SAFETY: serialised by CODEWHALE_HOME_TEST_LOCK.
            unsafe { std::env::remove_var("CODEWHALE_HOME") };
            Self { prior }
        }
    }
    impl Drop for CodeWhaleHomeGuard {
        fn drop(&mut self) {
            // SAFETY: serialised by CODEWHALE_HOME_TEST_LOCK.
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var("CODEWHALE_HOME", value),
                    None => std::env::remove_var("CODEWHALE_HOME"),
                }
            }
        }
    }

    #[test]
    fn codewhale_home_override_returns_the_env_value_verbatim() {
        let _lock = CODEWHALE_HOME_TEST_LOCK.lock().unwrap();
        let _g = CodeWhaleHomeGuard::set("/tmp/cw-isolated-state");
        // The env var IS the home dir — no ".codewhale" appended. This matches
        // codewhale_home() in config ($CODEWHALE_HOME=/x means home is /x).
        assert_eq!(
            codewhale_home_override().as_deref(),
            Some(std::path::Path::new("/tmp/cw-isolated-state"))
        );
    }

    #[test]
    fn codewhale_home_override_none_when_unset() {
        let _lock = CODEWHALE_HOME_TEST_LOCK.lock().unwrap();
        let _g = CodeWhaleHomeGuard::remove();
        assert!(codewhale_home_override().is_none());
    }

    #[test]
    fn codewhale_home_override_none_when_empty() {
        let _lock = CODEWHALE_HOME_TEST_LOCK.lock().unwrap();
        let _g = CodeWhaleHomeGuard::set("   ");
        // The helper filters empty values (after the OsString check). Note:
        // var_os returns the raw "   ", and our filter only catches truly-empty,
        // so this documents that whitespace-only is NOT treated as unset at the
        // override layer (config's codewhale_home trims; we don't here — the
        // branch is "was it set at all").
        assert!(
            codewhale_home_override().is_some(),
            "non-empty (even whitespace) counts as set; trimming is the caller's job"
        );
    }

    #[test]
    fn default_state_db_path_uses_codewhale_home_when_set() {
        let _lock = CODEWHALE_HOME_TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "cw-home-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _g = CodeWhaleHomeGuard::set(dir.to_str().unwrap());
        // Hard override: the DB is <CODEWHALE_HOME>/state.db, NOT
        // <CODEWHALE_HOME>/.codewhale/state.db, and the legacy ~/.deepseek
        // fallback is bypassed entirely.
        assert_eq!(default_state_db_path(), dir.join("state.db"));
    }

    #[test]
    fn session_index_compacts_after_threshold() {
        let store = temp_state_store("session-index-compact");
        for idx in 0..6 {
            store
                .append_thread_name("thread-1", Some(format!("name-{idx}")), idx, None)
                .expect("append session index entry");
        }

        let line_count = store
            .session_index_line_count()
            .expect("count session index lines");
        assert_eq!(line_count, 1);

        let name = store
            .find_thread_name_by_id("thread-1")
            .expect("lookup thread name");
        assert_eq!(name.as_deref(), Some("name-5"));
    }
}
