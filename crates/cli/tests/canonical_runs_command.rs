use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use codewhale_protocol::agent_runtime::{RunId, RunPurpose, RunRequest};
use codewhale_protocol::run_api::{RUN_API_SCHEMA_VERSION, RunCommandResponse, RunCommandResult};
use codewhale_runtime::{
    AgentOutcome, ModelAccounting, PendingRuntimeEvent, RunStore, TerminalState,
};
use codewhale_state::StateStore;

fn codewhale_binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codewhale")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_codewhale").map(PathBuf::from))
        .expect("Cargo must provide the codewhale dispatcher binary")
}

async fn seed_root(
    store: &StateStore,
    run_id: &str,
    workspace: &Path,
    purpose: RunPurpose,
    parent_run_id: Option<RunId>,
) {
    let mut request = RunRequest::new("fixture", "fixture system prompt");
    request.run_id = Some(RunId::from(run_id));
    request.purpose = purpose;
    request.parent_run_id = parent_run_id;
    request.environment.workspace = workspace.display().to_string();
    let created = store.create(request).await.expect("create canonical run");
    store
        .release(&created.lease)
        .await
        .expect("release canonical run");
}

async fn complete_root(store: &StateStore, run_id: &str) {
    let run_id = RunId::from(run_id);
    let acquired = store.acquire(&run_id).await.expect("acquire canonical run");
    let lease = acquired.lease.expect("active root lease");
    store
        .append(
            &lease,
            PendingRuntimeEvent::terminal(AgentOutcome {
                run_id: run_id.clone(),
                parent_run_id: None,
                terminal: TerminalState::Completed {
                    message: "fixture complete".to_owned(),
                },
                accounting: ModelAccounting::default(),
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
            }),
        )
        .await
        .expect("complete canonical run");
}

async fn seed_compaction(store: &StateStore, run_id: &str, source_run_id: &str) {
    let source_run_id = RunId::from(source_run_id);
    let source = store
        .load(&source_run_id)
        .await
        .expect("load source root")
        .expect("source root exists");
    let mut request = source.snapshot.request.clone();
    request.run_id = Some(RunId::from(run_id));
    request.parent_run_id = None;
    request.continued_from_run_id = Some(source_run_id);
    request.purpose = RunPurpose::ContextCompaction;
    request.input.clear();
    request.transcript = source.snapshot.transcript;
    let created = store
        .create(request)
        .await
        .expect("create canonical compaction root");
    store
        .release(&created.lease)
        .await
        .expect("release canonical compaction root");
}

fn run_dispatcher(home: &Path, workspace: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(codewhale_binary());
    command
        .current_dir(workspace)
        .env("CODEWHALE_HOME", home)
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("CODEWHALE_CLI_API_KEY")
        .args(args);
    command.output().expect("run codewhale dispatcher")
}

fn parse_response(output: &Output) -> RunCommandResponse {
    assert!(
        output.status.success(),
        "runs command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode versioned Run API response")
}

fn help_has_command(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        line.strip_prefix("  ")
            .and_then(|line| line.split_whitespace().next())
            == Some(command)
    })
}

#[tokio::test]
async fn dispatcher_lists_workspace_scoped_agent_roots_without_credentials() {
    let home = tempfile::tempdir().expect("temporary CODEWHALE_HOME");
    let workspace_root = tempfile::tempdir().expect("temporary workspace root");
    let workspace = workspace_root.path().join("current");
    let other_workspace = workspace_root.path().join("other");
    let empty_workspace = workspace_root.path().join("empty");
    std::fs::create_dir_all(&workspace).expect("create current workspace");
    std::fs::create_dir_all(&other_workspace).expect("create other workspace");
    std::fs::create_dir_all(&empty_workspace).expect("create empty workspace");
    let workspace = workspace
        .canonicalize()
        .expect("canonical current workspace");
    let other_workspace = other_workspace
        .canonicalize()
        .expect("canonical other workspace");
    let empty_workspace = empty_workspace
        .canonicalize()
        .expect("canonical empty workspace");

    let store =
        StateStore::open(Some(home.path().join("state.db"))).expect("open canonical State DB");
    seed_root(&store, "aa-agent-old", &workspace, RunPurpose::Agent, None).await;
    complete_root(&store, "aa-agent-old").await;
    seed_root(
        &store,
        "other-agent",
        &other_workspace,
        RunPurpose::Agent,
        None,
    )
    .await;
    seed_root(
        &store,
        "child-agent",
        &workspace,
        RunPurpose::Agent,
        Some(RunId::from("aa-agent-old")),
    )
    .await;
    seed_root(&store, "yy-agent-new", &workspace, RunPurpose::Agent, None).await;
    // This is newest so a raw ListRoots(limit=1) would hide the user run.
    // The CLI must filter purpose before applying its user-facing limit.
    seed_compaction(&store, "zz-internal-compaction", "aa-agent-old").await;
    drop(store);

    let all = parse_response(&run_dispatcher(
        home.path(),
        &workspace,
        &["runs", "--json"],
    ));
    assert_eq!(all.schema_version, RUN_API_SCHEMA_VERSION);
    let RunCommandResult::Runs {
        workspace: listed_workspace,
        runs,
    } = all.result
    else {
        panic!("expected canonical runs result");
    };
    assert_eq!(listed_workspace, workspace.display().to_string());
    assert_eq!(
        runs.iter()
            .map(|run| run.run_id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["yy-agent-new", "aa-agent-old"]
    );
    assert!(runs.iter().all(|run| run.purpose == RunPurpose::Agent));

    let limited = parse_response(&run_dispatcher(
        home.path(),
        &workspace,
        &["runs", "--limit", "1", "--json"],
    ));
    let RunCommandResult::Runs { runs, .. } = limited.result else {
        panic!("expected limited canonical runs result");
    };
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, RunId::from("yy-agent-new"));

    let human = run_dispatcher(home.path(), &workspace, &["runs", "--limit", "1"]);
    assert!(
        human.status.success(),
        "human runs command failed: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8(human.stdout).expect("UTF-8 human output");
    assert!(human.contains("当前工作区 Agent 运行："));
    assert!(human.contains("yy-agent-new"));
    assert!(human.contains("进行中"));
    assert!(!human.contains("zz-internal-compaction"));

    let scoped = parse_response(&run_dispatcher(
        home.path(),
        &workspace,
        &[
            "--workspace",
            other_workspace.to_str().expect("UTF-8 workspace"),
            "runs",
            "--json",
        ],
    ));
    let RunCommandResult::Runs { runs, .. } = scoped.result else {
        panic!("expected scoped canonical runs result");
    };
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, RunId::from("other-agent"));

    let empty = run_dispatcher(home.path(), &empty_workspace, &["runs"]);
    assert!(
        empty.status.success(),
        "empty runs command failed: {}",
        String::from_utf8_lossy(&empty.stderr)
    );
    assert_eq!(
        String::from_utf8(empty.stdout).expect("UTF-8 empty message"),
        "当前工作区还没有 Agent 运行。\n"
    );
}

#[test]
fn dispatcher_help_exposes_runs_and_removes_legacy_session_commands() {
    let output = Command::new(codewhale_binary())
        .arg("--help")
        .output()
        .expect("render dispatcher help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help_has_command(&help, "runs"));
    assert!(!help_has_command(&help, "sessions"));
    assert!(!help_has_command(&help, "fork"));
    assert!(!help.contains("Session id/prefix"));
    assert!(!help.contains("Windows note"));
}
