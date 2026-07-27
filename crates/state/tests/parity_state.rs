use std::path::PathBuf;

use dse_state::StateStore;
use rusqlite::Connection;

const RETIRED_TABLES: [&str; 11] = [
    "threads",
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
];

fn temp_state_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "deepseek_state_test_{}_{}_{}.db",
        label,
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ))
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .unwrap_or_else(|error| panic!("inspect table {table}: {error}"))
}

fn assert_current_schema(conn: &Connection) {
    let user_version: u32 = conn
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, 28);

    for table in [
        "agent_runs",
        "agent_run_events",
        "agent_run_snapshots",
        "agent_run_creations",
    ] {
        assert!(table_exists(conn, table), "missing canonical table {table}");
    }
    for table in RETIRED_TABLES {
        assert!(!table_exists(conn, table), "retired table {table} survived");
    }
    let foreign_key_errors: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("run foreign key check");
    assert_eq!(foreign_key_errors, 0);
}

#[test]
fn fresh_schema_contains_only_canonical_run_state() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let path = directory.path().join("state.db");
    StateStore::open(Some(path.clone())).expect("open state store");

    let conn = Connection::open(path).expect("open state db");
    assert_current_schema(&conn);
    assert!(
        !directory.path().join("session_index.jsonl").exists(),
        "retired thread-name sidecar was recreated"
    );
}

#[test]
fn v1_schema_deletes_all_retired_thread_and_workflow_state() {
    let path = temp_state_path("v1_retired_state_cleanup");
    let conn = Connection::open(&path).expect("open state db");
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        CREATE TABLE threads (
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
            memory_mode TEXT,
            current_leaf_id INTEGER
        );
        CREATE TABLE thread_dynamic_tools (
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            input_schema TEXT NOT NULL,
            PRIMARY KEY(thread_id, position)
        );
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY,
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            item_json TEXT,
            created_at INTEGER NOT NULL,
            parent_entry_id INTEGER
        );
        CREATE TABLE checkpoints (
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            checkpoint_id TEXT NOT NULL,
            state_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(thread_id, checkpoint_id)
        );
        CREATE TABLE jobs (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            progress INTEGER,
            detail TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE workflow_runs (id TEXT PRIMARY KEY);
        CREATE TABLE branch_runs (
            id TEXT PRIMARY KEY,
            workflow_run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE
        );
        CREATE TABLE leaf_runs (
            id TEXT PRIMARY KEY,
            workflow_run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
            branch_run_id TEXT REFERENCES branch_runs(id) ON DELETE SET NULL
        );
        CREATE TABLE control_node_runs (
            id TEXT PRIMARY KEY,
            workflow_run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE
        );
        CREATE TABLE teacher_candidates (
            id TEXT PRIMARY KEY,
            workflow_run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
            control_node_run_id TEXT NOT NULL REFERENCES control_node_runs(id) ON DELETE CASCADE,
            branch_run_id TEXT REFERENCES branch_runs(id) ON DELETE SET NULL
        );

        INSERT INTO threads (
            id, preview, ephemeral, model_provider, created_at, updated_at,
            status, cwd, cli_version, source, archived, current_leaf_id
        ) VALUES (
            'thread-test-1', 'hello', 0, 'deepseek', 0, 0,
            'running', '/tmp/project', '0.0.0-test', 'interactive', 0, 1
        );
        INSERT INTO thread_dynamic_tools VALUES (
            'thread-test-1', 0, 'legacy', NULL, '{"type":"object"}'
        );
        INSERT INTO messages VALUES (
            1, 'thread-test-1', 'user', 'legacy', NULL, 0, NULL
        );
        INSERT INTO checkpoints VALUES (
            'thread-test-1', 'legacy', '{}', 0
        );
        INSERT INTO jobs VALUES (
            'legacy', 'legacy', 'queued', NULL, NULL, 0, 0
        );
        INSERT INTO workflow_runs VALUES ('workflow-1');
        INSERT INTO branch_runs VALUES ('branch-1', 'workflow-1');
        INSERT INTO leaf_runs VALUES ('leaf-1', 'workflow-1', 'branch-1');
        INSERT INTO control_node_runs VALUES ('control-1', 'workflow-1');
        INSERT INTO teacher_candidates VALUES (
            'candidate-1', 'workflow-1', 'control-1', 'branch-1'
        );
        PRAGMA user_version = 1;
        "#,
    )
    .expect("create legacy v1 state");
    drop(conn);

    StateStore::open(Some(path.clone())).expect("migrate legacy state");
    let conn = Connection::open(path).expect("inspect migrated state");
    assert_current_schema(&conn);
}
