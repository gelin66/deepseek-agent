use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use dse_protocol::agent_runtime::{
    AgentActor, AgentActorKind, AgentTask, AgentTaskId, AgentWorkspaceAccess,
    AgentWorkspaceAssignment, ReasoningEffort, RunId, RunLimits, RunRequest, ToolPolicy,
};
use dse_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunCommand, RunCommandEnvelope, RunCommandResponse, RunCommandResult,
    RunProductControls, StartRunCommand,
};
use dse_protocol::task::{TaskContract, TaskDefinition, TaskGenerationId};
use dse_runtime::{AgentOutcome, ModelAccounting, PendingRuntimeEvent, RunStore, TerminalState};
use dse_state::StateStore;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn dse_binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_dse")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_dse").map(PathBuf::from))
        .expect("Cargo must provide the dse dispatcher binary")
}

async fn seed_root(store: &StateStore, run_id: &str, workspace: &Path) {
    let run_id = RunId::from(run_id);
    let mut request = RunRequest::new(
        TaskContract {
            generation_id: TaskGenerationId::from(run_id.0.clone()),
            definition: TaskDefinition::host("fixture"),
        },
        "fixture system prompt",
    );
    request.environment.workspace = workspace.display().to_string();
    let created = store.create(request).await.expect("create canonical run");
    store
        .release(&created.lease)
        .await
        .expect("release canonical run");
}

async fn seed_child(store: &StateStore, run_id: &str, parent_run_id: &str, workspace: &Path) {
    let child_run_id = RunId::from(run_id);
    let parent_run_id = RunId::from(parent_run_id);
    let task_contract = TaskContract {
        generation_id: TaskGenerationId::from(child_run_id.0.clone()),
        definition: TaskDefinition::host("fixture child"),
    };
    let mut request = RunRequest::new(task_contract.clone(), "fixture child system prompt");
    let task = AgentTask {
        task_id: AgentTaskId::from("reader-child-agent"),
        root_run_id: parent_run_id.clone(),
        parent_run_id: parent_run_id.clone(),
        child_run_id: child_run_id.clone(),
        call_id: "fixture-child-call".to_owned(),
        role: "explorer".to_owned(),
        task_contract,
        workspace: AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::ReadOnly,
            root_workspace: workspace.display().to_string(),
            base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            worktree_path: None,
            root_branch: None,
            branch: None,
            allowed_paths: Vec::new(),
            owner_token: None,
        },
        model: request.model.clone(),
        reasoning_effort: request.reasoning_effort,
        max_output_tokens: request.max_output_tokens,
        context_policy: request.context_policy,
        route: request.route.clone(),
        tool_policy: request.tool_policy.clone(),
        limits: request.limits,
        deadline_unix_ms: request.deadline_unix_ms,
        expected_artifact: "fixture child result".to_owned(),
    };
    request.parent_run_id = Some(parent_run_id);
    request.actor = AgentActor {
        kind: AgentActorKind::Child,
        depth: 1,
    };
    request.agent_task = Some(task);
    request.environment.workspace = workspace.display().to_string();
    let created = store.create(request).await.expect("create canonical child");
    store
        .release(&created.lease)
        .await
        .expect("release canonical child");
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
                terminal: TerminalState::Blocked {
                    reason: "fixture terminal".to_owned(),
                },
                accounting: ModelAccounting::default(),
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
                details: Default::default(),
            }),
        )
        .await
        .expect("complete canonical run");
}

fn run_dispatcher(home: &Path, workspace: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(dse_binary());
    command
        .current_dir(workspace)
        .env("DSE_HOME", home)
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("DSE_CLI_API_KEY")
        .args(args);
    command.output().expect("run dse dispatcher")
}

fn install_tui_probe(home: &Path) -> (PathBuf, PathBuf) {
    let marker = home.join("tui-launched");
    let fake_tui = home.join("fake-dse-tui");
    std::fs::write(
        &fake_tui,
        "#!/bin/sh\nprintf launched > \"$DSE_TUI_MARKER\"\n",
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
    Command::new(dse_binary())
        .current_dir(workspace)
        .env("DSE_HOME", home)
        .env("DSE_TUI_BIN", fake_tui)
        .env("DSE_TUI_MARKER", marker)
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("DSE_CLI_API_KEY")
        .args(args)
        .output()
        .expect("run dse dispatcher with TUI probe")
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
async fn app_server_process_loads_the_same_config_home_prompt_override_as_exec() {
    const OVERRIDE_MARKER: &str = "app-server-process-override-marker";

    let home = tempfile::tempdir().expect("temporary app-server DSE_HOME");
    let workspace = tempfile::tempdir().expect("temporary app-server workspace");
    let prompt_path = home.path().join("prompts/constitution.md");
    std::fs::create_dir_all(prompt_path.parent().expect("prompt parent"))
        .expect("create prompt override directory");
    std::fs::write(&prompt_path, format!("# 系统契约\n\n{OVERRIDE_MARKER}\n"))
        .expect("write prompt override");

    let mut child = tokio::process::Command::new(dse_binary())
        .current_dir(workspace.path())
        .env("DSE_HOME", home.path())
        .env("DSE_ALLOW_BASE_PROMPT_OVERRIDE", "1")
        .env("DEEPSEEK_API_KEY", "offline-app-server-prompt-key")
        // Preserve the official endpoint contract while making any model
        // attempt fail locally; RunCreated is committed before the response.
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("https_proxy", "http://127.0.0.1:1")
        .env("ALL_PROXY", "http://127.0.0.1:1")
        .args(["app-server", "--stdio", "--transport-max-retries", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start production app-server process");

    let envelope = RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: "app-server-prompt-override-probe".to_owned(),
        command: RunCommand::Start(StartRunCommand {
            task: TaskDefinition::host("只建立 prompt 进程级证据"),
            workspace: workspace.path().display().to_string(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(128),
            max_api_requests: std::num::NonZeroU32::new(1),
            streaming: false,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits {
                max_depth: 0,
                max_turns: 1,
                max_model_requests: 1,
                max_model_retries: 0,
                max_tool_calls: 0,
                ..RunLimits::default()
            },
            controls: RunProductControls {
                auto_approve: true,
                interactive: false,
                sandbox: Some("workspace-write".to_owned()),
                ..RunProductControls::default()
            },
        }),
    };
    let mut stdin = child.stdin.take().expect("app-server stdin");
    stdin
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(&envelope).expect("encode start envelope")
            )
            .as_bytes(),
        )
        .await
        .expect("write app-server command");
    stdin.flush().await.expect("flush app-server command");

    let mut stdout = BufReader::new(child.stdout.take().expect("app-server stdout"));
    let mut response_line = String::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        stdout.read_line(&mut response_line),
    )
    .await
    .expect("app-server response timeout")
    .expect("read app-server response");
    let response: RunCommandResponse =
        serde_json::from_str(&response_line).expect("decode app-server response");
    let RunCommandResult::Run { run } = response.result else {
        panic!("app-server start failed: {response_line}");
    };
    let run_id = run.run_id.clone();

    child.kill().await.expect("stop app-server process");
    let _ = child.wait().await;

    let store = StateStore::open(Some(home.path().join("state.db")))
        .expect("open app-server canonical State DB");
    let replay = store
        .load(&run_id)
        .await
        .expect("load app-server run")
        .expect("app-server RunCreated");
    let prompt = replay
        .snapshot
        .request
        .system_prompt
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        prompt.contains(OVERRIDE_MARKER),
        "production app-server ignored the config-home prompt override"
    );
}

#[tokio::test]
async fn dispatcher_lists_workspace_scoped_agent_roots_without_credentials() {
    let home = tempfile::tempdir().expect("temporary DSE_HOME");
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
    seed_root(&store, "aa-agent-old", &workspace).await;
    complete_root(&store, "aa-agent-old").await;
    seed_root(&store, "other-agent", &other_workspace).await;
    seed_child(&store, "child-agent", "aa-agent-old", &workspace).await;
    seed_root(&store, "yy-agent-new", &workspace).await;
    drop(store);

    let all_en_output = run_dispatcher(
        home.path(),
        &workspace,
        &["--language", "en", "runs", "--json"],
    );
    let all_zh_output = run_dispatcher(
        home.path(),
        &workspace,
        &["--language", "zh-Hans", "runs", "--json"],
    );
    assert_eq!(
        all_en_output.stdout, all_zh_output.stdout,
        "human language must not change canonical Run API JSON"
    );
    let all = parse_response(&all_en_output);
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

    let human_en = run_dispatcher(
        home.path(),
        &workspace,
        &["--language", "en", "runs", "--limit", "1"],
    );
    assert!(
        human_en.status.success(),
        "English human runs command failed: {}",
        String::from_utf8_lossy(&human_en.stderr)
    );
    let human_en = String::from_utf8(human_en.stdout).expect("UTF-8 English human output");
    assert!(human_en.contains("Agent runs in current workspace:"));
    assert!(human_en.contains("yy-agent-new"));
    assert!(human_en.contains("active"));
    assert!(!human_en.contains("zz-internal-compaction"));

    let human_zh = run_dispatcher(
        home.path(),
        &workspace,
        &["--language", "zh-Hans", "runs", "--limit", "1"],
    );
    assert!(
        human_zh.status.success(),
        "Chinese human runs command failed: {}",
        String::from_utf8_lossy(&human_zh.stderr)
    );
    let human_zh = String::from_utf8(human_zh.stdout).expect("UTF-8 Chinese human output");
    assert!(human_zh.contains("当前工作区 Agent 运行："));
    assert!(human_zh.contains("yy-agent-new"));
    assert!(human_zh.contains("进行中"));
    assert!(!human_zh.contains("zz-internal-compaction"));

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

    let empty = run_dispatcher(
        home.path(),
        &empty_workspace,
        &["--language", "zh-Hans", "runs"],
    );
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
    let output = Command::new(dse_binary())
        .arg("--help")
        .output()
        .expect("render dispatcher help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help_has_command(&help, "runs"));
    assert!(!help_has_command(&help, "run"));
    assert!(!help_has_command(&help, "sessions"));
    assert!(!help_has_command(&help, "fork"));
    assert!(!help_has_command(&help, "update"));
    assert!(!help_has_command(&help, "metrics"));
    assert!(!help.contains("Session id/prefix"));
    assert!(!help.contains("Windows note"));
}

#[test]
fn completion_bypasses_malformed_config_without_opening_store_or_tui() {
    let home = tempfile::tempdir().expect("temporary DSE_HOME");
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
        String::from_utf8_lossy(&output.stdout).contains("_dse"),
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
    let home = tempfile::tempdir().expect("temporary DSE_HOME");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let workspace = workspace
        .path()
        .canonicalize()
        .expect("canonical temporary workspace");
    let store =
        StateStore::open(Some(home.path().join("state.db"))).expect("open canonical State DB");
    seed_root(&store, "agent-with-bad-config", &workspace).await;
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
fn removed_commands_and_flags_fail_before_config_tui_store_or_model_startup() {
    let home = tempfile::tempdir().expect("temporary DSE_HOME");
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let (fake_tui, marker) = install_tui_probe(home.path());
    // A malformed real config proves the rejection happens before ConfigStore
    // parsing, not merely before application/model construction.
    std::fs::write(home.path().join("config.toml"), "provider = [")
        .expect("write malformed config");

    for args in [
        vec!["thread"],
        vec!["thread", "list"],
        vec!["thread", "read", "legacy-thread-id"],
        vec!["thread", "resume", "legacy-thread-id"],
        vec!["thread", "fork", "legacy-thread-id"],
        vec!["thread", "archive", "legacy-thread-id"],
        vec!["thread", "unarchive", "legacy-thread-id"],
        vec!["thread", "set-name", "legacy-thread-id", "name"],
        vec!["thread", "clear-name", "legacy-thread-id"],
        vec!["sessions"],
        vec!["sessions", "--json"],
        vec!["fork", "legacy-session-id"],
        vec!["fork", "--last"],
        vec!["run"],
        vec!["run", "speech", "paid input"],
        vec!["run", "exec", "paid input"],
        vec!["mcp-server"],
        vec!["mcp-server", "--legacy"],
        vec!["update"],
        vec!["update", "--check"],
        vec!["update", "--proxy", "socks5://127.0.0.1:1080"],
        vec!["metrics"],
        vec!["metrics", "--json"],
        vec!["workflow"],
        vec!["workflow", "run", "stopship", "--fleet", "v0868-stopship"],
        vec!["workflow-tool"],
        vec![
            "workflow-tool",
            "--approval-source",
            "explicit-workflow-command",
            "--input-json",
            r#"{"action":"run"}"#,
        ],
        vec!["fleet"],
        vec!["fleet", "status"],
        vec!["lane"],
        vec!["lane", "list"],
        vec!["mcp", "add-self"],
        vec!["mcp", "add-self", "--name", "legacy-self"],
        vec!["review"],
        vec!["review", "--staged"],
        vec!["speech"],
        vec!["speech", "paid input", "--model", "tts"],
        vec!["tts"],
        vec!["tts", "paid input"],
        vec!["serve", "--acp"],
        vec!["serve", "--mcp"],
    ] {
        let output =
            run_dispatcher_with_tui_probe(home.path(), workspace.path(), &fake_tui, &marker, &args);
        assert!(
            !output.status.success(),
            "removed command or flag unexpectedly succeeded: {args:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "removed command or flag wrote stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8(output.stderr).expect("UTF-8 rejection");
        if args.first() == Some(&"serve") {
            assert!(
                stderr.contains(&format!("unexpected argument '{}'", args[1])),
                "removed serve flag did not fail in Clap: {args:?}: {stderr}"
            );
        }
        assert!(
            !stderr.contains("failed to parse config"),
            "removed command or flag reached ConfigStore: {args:?}: {stderr}"
        );
        assert!(
            !marker.exists(),
            "removed command or flag started the TUI: {args:?}"
        );
        assert!(
            !home.path().join("state.db").exists(),
            "removed command or flag opened the canonical RunStore: {args:?}"
        );
    }

    let explicit_home = tempfile::tempdir().expect("temporary explicit-prompt DSE_HOME");
    let (explicit_tui, explicit_marker) = install_tui_probe(explicit_home.path());
    let explicit_prompt = run_dispatcher_with_tui_probe(
        explicit_home.path(),
        workspace.path(),
        &explicit_tui,
        &explicit_marker,
        &["--prompt", "update", "the", "dependencies"],
    );
    assert!(
        explicit_prompt.status.success(),
        "explicit --prompt update ... should remain a legal prompt: {}",
        String::from_utf8_lossy(&explicit_prompt.stderr)
    );
    assert!(
        explicit_marker.exists(),
        "explicit --prompt update ... was mistaken for the retired command"
    );

    let explicit_workflow_home =
        tempfile::tempdir().expect("temporary explicit-workflow-prompt DSE_HOME");
    let (explicit_workflow_tui, explicit_workflow_marker) =
        install_tui_probe(explicit_workflow_home.path());
    let explicit_workflow_prompt = run_dispatcher_with_tui_probe(
        explicit_workflow_home.path(),
        workspace.path(),
        &explicit_workflow_tui,
        &explicit_workflow_marker,
        &["--prompt", "workflow", "run", "an", "audit"],
    );
    assert!(
        explicit_workflow_prompt.status.success(),
        "explicit --prompt workflow ... should remain a legal prompt: {}",
        String::from_utf8_lossy(&explicit_workflow_prompt.stderr)
    );
    assert!(
        explicit_workflow_marker.exists(),
        "explicit --prompt workflow ... was mistaken for the retired command"
    );

    let explicit_acp_home = tempfile::tempdir().expect("temporary explicit-ACP-prompt DSE_HOME");
    let (explicit_acp_tui, explicit_acp_marker) = install_tui_probe(explicit_acp_home.path());
    let explicit_acp_prompt = run_dispatcher_with_tui_probe(
        explicit_acp_home.path(),
        workspace.path(),
        &explicit_acp_tui,
        &explicit_acp_marker,
        &["--prompt", "serve --acp"],
    );
    assert!(
        explicit_acp_prompt.status.success(),
        "explicit --prompt \"serve --acp\" should remain legal: {}",
        String::from_utf8_lossy(&explicit_acp_prompt.stderr)
    );
    assert!(
        explicit_acp_marker.exists(),
        "explicit --prompt \"serve --acp\" was mistaken for the removed command"
    );

    let explicit_review_home =
        tempfile::tempdir().expect("temporary explicit-review-prompt DSE_HOME");
    let (explicit_review_tui, explicit_review_marker) =
        install_tui_probe(explicit_review_home.path());
    let explicit_review_prompt = run_dispatcher_with_tui_probe(
        explicit_review_home.path(),
        workspace.path(),
        &explicit_review_tui,
        &explicit_review_marker,
        &["--prompt", "审查当前 git diff"],
    );
    assert!(
        explicit_review_prompt.status.success(),
        "explicit --prompt \"审查当前 git diff\" should remain legal: {}",
        String::from_utf8_lossy(&explicit_review_prompt.stderr)
    );
    assert!(
        explicit_review_marker.exists(),
        "explicit --prompt \"审查当前 git diff\" was mistaken for the removed command"
    );

    let explicit_speech_home =
        tempfile::tempdir().expect("temporary explicit-speech-prompt DSE_HOME");
    let (explicit_speech_tui, explicit_speech_marker) =
        install_tui_probe(explicit_speech_home.path());
    let explicit_speech_prompt = run_dispatcher_with_tui_probe(
        explicit_speech_home.path(),
        workspace.path(),
        &explicit_speech_tui,
        &explicit_speech_marker,
        &["--prompt", "生成语音"],
    );
    assert!(
        explicit_speech_prompt.status.success(),
        "explicit --prompt \"生成语音\" should remain legal: {}",
        String::from_utf8_lossy(&explicit_speech_prompt.stderr)
    );
    assert!(
        explicit_speech_marker.exists(),
        "explicit --prompt \"生成语音\" was mistaken for the removed command"
    );
}
