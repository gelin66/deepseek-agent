use std::path::PathBuf;

use codewhale_state::{SessionSource, StateStore, ThreadListFilters, ThreadMetadata, ThreadStatus};
use rusqlite::Connection;

const RETIRED_TABLES: [&str; 10] = [
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

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)");
    conn.query_row(&sql, [column], |row| row.get(0))
        .unwrap_or_else(|error| panic!("inspect {table}.{column}: {error}"))
}

fn assert_current_schema(conn: &Connection) {
    let user_version: u32 = conn
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, 19);

    for table in [
        "threads",
        "agent_runs",
        "agent_run_events",
        "agent_run_snapshots",
        "agent_run_creations",
    ] {
        assert!(table_exists(conn, table), "missing retained table {table}");
    }
    for table in RETIRED_TABLES {
        assert!(!table_exists(conn, table), "retired table {table} survived");
    }
    assert!(
        !column_exists(conn, "threads", "current_leaf_id"),
        "retired current_leaf_id projection survived"
    );
    let foreign_key_errors: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("run foreign key check");
    assert_eq!(foreign_key_errors, 0);
}

fn test_thread(now: i64) -> ThreadMetadata {
    ThreadMetadata {
        id: "thread-test-1".to_string(),
        rollout_path: Some(PathBuf::from("/tmp/rollout.jsonl")),
        preview: "hello".to_string(),
        ephemeral: false,
        model_provider: "deepseek".to_string(),
        created_at: now,
        updated_at: now,
        status: ThreadStatus::Running,
        path: Some(PathBuf::from("/tmp/project")),
        cwd: PathBuf::from("/tmp/project"),
        cli_version: "0.0.0-test".to_string(),
        source: SessionSource::Interactive,
        name: Some("Test Thread".to_string()),
        sandbox_policy: Some("workspace-write".to_string()),
        approval_mode: Some("on-request".to_string()),
        archived: false,
        archived_at: None,
        git_sha: None,
        git_branch: None,
        git_origin_url: None,
        memory_mode: Some("extended".to_string()),
    }
}

#[test]
fn upsert_and_resume_thread_metadata() {
    let path = temp_state_path("upsert_resume");
    let store = StateStore::open(Some(path.clone())).expect("open state store");
    let thread = test_thread(chrono::Utc::now().timestamp());
    store.upsert_thread(&thread).expect("upsert thread");

    let loaded = store
        .get_thread("thread-test-1")
        .expect("read thread")
        .expect("thread must exist");
    assert_eq!(loaded.id, "thread-test-1");
    assert_eq!(loaded.name.as_deref(), Some("Test Thread"));
    assert_eq!(loaded.memory_mode.as_deref(), Some("extended"));
    assert_eq!(
        loaded.rollout_path,
        Some(PathBuf::from("/tmp/rollout.jsonl"))
    );

    store
        .mark_archived("thread-test-1")
        .expect("archive thread");
    let archived = store
        .get_thread("thread-test-1")
        .expect("read archived thread")
        .expect("thread exists after archive");
    assert!(archived.archived);

    let listed = store
        .list_threads(ThreadListFilters {
            include_archived: true,
            limit: Some(10),
        })
        .expect("list threads");
    assert_eq!(listed.len(), 1);
}

#[test]
fn fresh_schema_contains_only_retained_state_surfaces() {
    let path = temp_state_path("fresh_schema");
    StateStore::open(Some(path.clone())).expect("open state store");

    let conn = Connection::open(path).expect("open state db");
    assert_current_schema(&conn);
}

#[test]
fn v1_schema_drops_retired_state_and_preserves_thread_metadata() {
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

    let store = StateStore::open(Some(path.clone())).expect("migrate legacy state");
    let thread = store
        .get_thread("thread-test-1")
        .expect("read thread")
        .expect("thread metadata survives migration");
    assert_eq!(thread.preview, "hello");
    assert_eq!(thread.model_provider, "deepseek");
    drop(store);

    let conn = Connection::open(path).expect("inspect migrated state");
    assert_current_schema(&conn);
}
