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

fn install_tui_probe(home: &Path) -> (PathBuf, PathBuf) {
    let marker = home.join("tui-launched");
    let fake_tui = home.join("fake-codewhale-tui");
    std::fs::write(
        &fake_tui,
        "#!/bin/sh\nprintf launched > \"$CODEWHALE_TUI_MARKER\"\n",
    )
    .expect("write fake TUI");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(&fake_tui)
            .expect("fake TUI metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tui, permissions).expect("make fake TUI executable");
    }
    (fake_tui, marker)
}

fn run_dispatcher_with_tui_probe(
    home: &Path,
    workspace: &Path,
    fake_tui: &Path,
    marker: &Path,
    args: &[&str],
) -> Output {
    Command::new(codewhale_binary())
        .current_dir(workspace)
        .env("CODEWHALE_HOME", home)
        .env("DEEPSEEK_TUI_BIN", fake_tui)
        .env("CODEWHALE_TUI_MARKER", marker)
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("CODEWHALE_CLI_API_KEY")
        .args(args)
        .output()
        .expect("run codewhale dispatcher with TUI probe")
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
fn dispatcher_help_exposes_runs_and_removes_retired_top_level_commands() {
    let output = Command::new(codewhale_binary())
        .arg("--help")
        .output()
        .expect("render dispatcher help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help_has_command(&help, "runs"));
    assert!(!help_has_command(&help, "run"));
    assert!(!help_has_command(&help, "sessions"));
    assert!(!help_has_command(&help, "fork"));
    assert!(!help.contains("Session id/prefix"));
    assert!(!help.contains("Windows note"));
}

#[test]
fn completion_bypasses_malformed_config_without_opening_store_or_tui() {
    let home = tempfile::tempdir().expect("temporary CODEWHALE_HOME");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let (fake_tui, marker) = install_tui_probe(home.path());
    std::fs::write(home.path().join("config.toml"), "provider = [")
        .expect("write malformed config");

    let output = run_dispatcher_with_tui_probe(
        home.path(),
        workspace.path(),
        &fake_tui,
        &marker,
        &["completion", "bash"],
    );

    assert!(
        output.status.success(),
        "completion failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("_codewhale"),
        "completion output did not contain the generated bash function"
    );
    assert!(!marker.exists(), "completion started the TUI");
    assert!(
        !home.path().join("state.db").exists(),
        "completion opened the canonical RunStore"
    );
}

#[tokio::test]
async fn canonical_runs_bypasses_malformed_config_without_tui_or_credentials() {
    let home = tempfile::tempdir().expect("temporary CODEWHALE_HOME");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace
        .path()
        .canonicalize()
        .expect("canonical temporary workspace");
    let store =
        StateStore::open(Some(home.path().join("state.db"))).expect("open canonical State DB");
    seed_root(
        &store,
        "agent-with-bad-config",
        &workspace,
        RunPurpose::Agent,
        None,
    )
    .await;
    let before_events = store
        .load(&RunId::from("agent-with-bad-config"))
        .await
        .expect("load seeded canonical run")
        .expect("seeded canonical run exists")
        .events
        .len();
    drop(store);
    std::fs::write(home.path().join("config.toml"), "provider = [")
        .expect("write malformed config");
    let (fake_tui, marker) = install_tui_probe(home.path());

    let output = run_dispatcher_with_tui_probe(
        home.path(),
        &workspace,
        &fake_tui,
        &marker,
        &["runs", "--json"],
    );
    let response = parse_response(&output);
    let RunCommandResult::Runs { runs, .. } = response.result else {
        panic!("expected canonical runs result");
    };
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, RunId::from("agent-with-bad-config"));
    assert!(!marker.exists(), "canonical runs started the TUI");
    let reopened =
        StateStore::open(Some(home.path().join("state.db"))).expect("reopen canonical State DB");
    let after_events = reopened
        .load(&RunId::from("agent-with-bad-config"))
        .await
        .expect("reload canonical run")
        .expect("canonical run remains")
        .events
        .len();
    assert_eq!(
        after_events, before_events,
        "canonical runs appended events instead of remaining read-only"
    );
}

#[test]
fn retired_commands_fail_before_config_tui_store_or_model_startup() {
    let home = tempfile::tempdir().expect("temporary CODEWHALE_HOME");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let (fake_tui, marker) = install_tui_probe(home.path());
    // A malformed real config proves the rejection happens before ConfigStore
    // parsing, not merely before application/model construction.
    std::fs::write(home.path().join("config.toml"), "provider = [")
        .expect("write malformed config");

    for (args, retired) in [
        (vec!["sessions"], "sessions"),
        (vec!["sessions", "--json"], "sessions"),
        (vec!["fork", "legacy-session-id"], "fork"),
        (vec!["fork", "--last"], "fork"),
        (vec!["run"], "run"),
        (vec!["run", "--help"], "run"),
        (vec!["run", "speech", "paid input"], "run"),
        (vec!["run", "exec", "paid input"], "run"),
        (vec!["mcp-server"], "mcp-server"),
        (vec!["mcp-server", "--legacy"], "mcp-server"),
        (vec!["mcp", "add-self"], "mcp add-self"),
        (
            vec!["mcp", "add-self", "--name", "legacy-self"],
            "mcp add-self",
        ),
    ] {
        let output =
            run_dispatcher_with_tui_probe(home.path(), workspace.path(), &fake_tui, &marker, &args);
        assert!(
            !output.status.success(),
            "retired command unexpectedly succeeded: {args:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "retired command wrote stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8(output.stderr).expect("UTF-8 rejection");
        assert!(
            stderr.contains(&format!("命令 `codewhale {retired}` 已删除")),
            "missing Chinese retired-command rejection for {args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("failed to parse config"),
            "retired command reached ConfigStore: {stderr}"
        );
        assert!(
            !marker.exists(),
            "retired command started the TUI: {args:?}"
        );
        assert!(
            !home.path().join("state.db").exists(),
            "retired command opened the canonical RunStore: {args:?}"
        );
    }

    // `serve` remains as an ACP-only command, so Clap rejects the removed MCP
    // flag before `run()` can open ConfigStore or delegate to the TUI.
    let output = Command::new(codewhale_binary())
        .current_dir(workspace.path())
        .env("CODEWHALE_HOME", home.path())
        .env("DEEPSEEK_TUI_BIN", &fake_tui)
        .env("CODEWHALE_TUI_MARKER", &marker)
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("CODEWHALE_CLI_API_KEY")
        .args(["serve", "--mcp"])
        .output()
        .expect("run removed serve --mcp command");
    assert!(
        !output.status.success(),
        "serve --mcp unexpectedly succeeded"
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 rejection");
    assert!(
        stderr.contains("unexpected argument '--mcp'"),
        "serve --mcp was not rejected by argument parsing: {stderr}"
    );
    assert!(
        !stderr.contains("failed to parse config"),
        "serve --mcp reached ConfigStore: {stderr}"
    );
    assert!(!marker.exists(), "serve --mcp started the TUI");
    assert!(
        !home.path().join("state.db").exists(),
        "serve --mcp opened the canonical RunStore"
    );
}
