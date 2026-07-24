//! Persistent state management for canonical Agent runs.
//!
//! The [`StateStore`] is the primary entry point, backed by SQLite. It owns
//! canonical event, snapshot, creation, and replay persistence.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{Connection, ErrorCode, TransactionBehavior};

mod run_store;

const STATE_SCHEMA_VERSION: u32 = 23;

/// Persistent storage for canonical Agent runs.
///
/// The SQLite schema is automatically initialized and migrated on [`open`](Self::open).
#[derive(Debug, Clone)]
pub struct StateStore {
    db_path: PathBuf,
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
            tx.pragma_update(None, "user_version", 1)
                .context("failed to initialize state schema version")?;
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
        if user_version < 20 {
            // RuntimeEvent v15 makes redacted response lifecycle evidence and
            // the safe-replay decision mandatory on every model failure.
            // Historical failures cannot be upgraded without guessing which
            // output or tool-call fragments crossed the wire. Preserve only
            // still-pending canonical creation commands; finalized receipts
            // point at the incompatible runtime rows retired below.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                if user_version >= 9 {
                    tx.execute(
                        "DELETE FROM agent_run_creations WHERE command_json IS NULL",
                        [],
                    )
                    .context("failed to retire finalized pre-response-evidence creations")?;
                } else {
                    tx.execute("DELETE FROM agent_run_creations", [])
                        .context("failed to retire pre-intent creation receipts")?;
                }
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-response-evidence canonical run state")?;
        }
        if user_version < 21 {
            // RuntimeEvent v16 makes a stable failure code mandatory for
            // every unsuccessful ToolOutcome. Historical text cannot be
            // classified without guessing, so retire incompatible runs while
            // preserving only pending Start intents. A pending Continue is
            // not independently recoverable after its source run is retired.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                if user_version >= 9 {
                    run_store::retain_recoverable_start_creation_intents(&tx)
                        .context("failed to retire unrecoverable pre-tool-failure creations")?;
                } else {
                    tx.execute("DELETE FROM agent_run_creations", [])
                        .context("failed to retire pre-intent creation receipts")?;
                }
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-tool-failure canonical run state")?;
        }
        if user_version < 22 {
            // RuntimeEvent v17 makes the requested model mode, requested
            // reasoning, Host policy version, reason code, and every child
            // selection mandatory replay facts. Existing run rows cannot
            // recover caller intent from only the selected model. Preserve
            // pending Start commands because the app-owned policy is now
            // deterministic and has no pre-RunCreated network side effect.
            if sqlite_table_exists(&tx, "agent_run_creations")? {
                if user_version >= 9 {
                    run_store::retain_recoverable_start_creation_intents(&tx)
                        .context("failed to retire unrecoverable pre-route-audit creations")?;
                } else {
                    tx.execute("DELETE FROM agent_run_creations", [])
                        .context("failed to retire pre-intent creation receipts")?;
                }
            }
            tx.execute("DELETE FROM agent_runs", [])
                .context("failed to retire pre-route-audit canonical run state")?;
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
        if user_version < 20 {
            tx.pragma_update(None, "user_version", 20)
                .context("failed to commit response evidence state cutover")?;
            user_version = 20;
        }
        if user_version < 21 {
            tx.pragma_update(None, "user_version", 21)
                .context("failed to commit typed tool failure state cutover")?;
            user_version = 21;
        }
        if user_version < 22 {
            tx.pragma_update(None, "user_version", 22)
                .context("failed to commit Host route audit state cutover")?;
            user_version = 22;
        }
        if user_version < 23 {
            tx.execute_batch("DROP TABLE IF EXISTS threads;")
                .context("failed to delete retired conversation thread state")?;
            tx.pragma_update(None, "user_version", 23)
                .context("failed to commit canonical RunStore-only state cutover")?;
            user_version = 23;
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

    #[test]
    fn state_store_reuses_one_connection_across_operations_and_clones() {
        let store = temp_state_store("conn-reuse");
        {
            let conn = store.conn().expect("conn");
            conn.execute_batch("CREATE TEMP TABLE conn_reuse_probe(id INTEGER);")
                .expect("create temp table");
        }
        // TEMP tables are visible only on the connection that created them, so
        // seeing the probe again through a clone
        // proves the store holds one long-lived connection instead of
        // reopening the database (and reapplying pragmas) per call.
        let clone = store.clone();
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
        // <CODEWHALE_HOME>/.codewhale/state.db.
        assert_eq!(default_state_db_path(), dir.join("state.db"));
    }
}
