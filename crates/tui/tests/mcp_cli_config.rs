use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn tui_binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codewhale-tui")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_codewhale-tui").map(PathBuf::from))
        .expect("Cargo must expose the codewhale-tui test binary")
}

fn run_cli(home: &Path, workspace: &Path, mcp_config: &Path, args: &[&str]) -> Output {
    let codewhale_home = home.join(".codewhale");
    let mut command = Command::new(tui_binary());
    command
        .current_dir(workspace)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CODEWHALE_HOME", &codewhale_home)
        .env("CODEWHALE_CONFIG_PATH", codewhale_home.join("config.toml"))
        .env("DEEPSEEK_MCP_CONFIG", mcp_config)
        .env("RUST_LOG", "error")
        .arg("--workspace")
        .arg(workspace)
        .arg("--no-project-config")
        .args(args);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command.output().expect("run codewhale-tui CLI")
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read MCP config"))
        .expect("parse MCP config JSON")
}

#[test]
fn real_cli_mcp_config_lifecycle_writes_only_the_global_owner() {
    let isolated = tempfile::tempdir().expect("isolated home");
    let home = isolated.path().join("home");
    let workspace = isolated.path().join("workspace");
    let codewhale_home = home.join(".codewhale");
    let global_config = codewhale_home.join("mcp.json");
    let project_config = workspace.join(".codewhale").join("mcp.json");
    std::fs::create_dir_all(project_config.parent().unwrap()).expect("project MCP directory");
    std::fs::create_dir_all(&codewhale_home).expect("CodeWhale home");
    std::fs::write(
        &project_config,
        serde_json::to_vec_pretty(&json!({
            "servers": {
                "project-only": {
                    "command": "project-mcp",
                    "args": []
                }
            }
        }))
        .unwrap(),
    )
    .expect("project MCP config");

    let canonical_workspace = workspace.canonicalize().expect("canonical workspace");
    std::fs::write(
        codewhale_home.join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            canonical_workspace.display()
        ),
    )
    .expect("trusted workspace config");

    let init = run_cli(&home, &workspace, &global_config, &["mcp", "init"]);
    assert_success(&init, "mcp init");

    let add = run_cli(
        &home,
        &workspace,
        &global_config,
        &[
            "mcp",
            "add",
            "remote",
            "--url",
            "https://example.com/mcp",
            "--transport",
            "sse",
            "--bearer-token-env-var",
            "MCP_TEST_TOKEN",
            "--oauth-client-id",
            "test-client",
            "--oauth-resource",
            "https://example.com/resource",
            "--scope",
            "tools:read",
            "--scope",
            "tools:write",
        ],
    );
    assert_success(&add, "mcp add");

    let disable = run_cli(
        &home,
        &workspace,
        &global_config,
        &["mcp", "disable", "remote"],
    );
    assert_success(&disable, "mcp disable");

    let list = run_cli(&home, &workspace, &global_config, &["mcp", "list"]);
    assert_success(&list, "mcp list");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("remote [disabled"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("project-only [enabled"),
        "stdout:\n{stdout}"
    );

    let global = read_json(&global_config);
    let remote = &global["servers"]["remote"];
    assert_eq!(remote["url"], "https://example.com/mcp");
    assert_eq!(remote["transport"], "sse");
    assert_eq!(remote["bearer_token_env_var"], "MCP_TEST_TOKEN");
    assert_eq!(remote["oauth"]["client_id"], "test-client");
    assert_eq!(remote["oauth_resource"], "https://example.com/resource");
    assert_eq!(remote["scopes"], json!(["tools:read", "tools:write"]));
    assert_eq!(remote["enabled"], false);
    assert_eq!(remote["disabled"], true);
    assert!(
        global["servers"].get("project-only").is_none(),
        "workspace/project servers must never be copied into the global writer"
    );

    let enable = run_cli(
        &home,
        &workspace,
        &global_config,
        &["mcp", "enable", "remote"],
    );
    assert_success(&enable, "mcp enable");
    let enabled = read_json(&global_config);
    assert_eq!(enabled["servers"]["remote"]["enabled"], true);
    assert_eq!(enabled["servers"]["remote"]["disabled"], false);

    let remove = run_cli(
        &home,
        &workspace,
        &global_config,
        &["mcp", "remove", "remote"],
    );
    assert_success(&remove, "mcp remove");
    let removed = read_json(&global_config);
    assert!(removed["servers"].get("remote").is_none());
    assert!(removed["servers"].get("project-only").is_none());
    assert!(read_json(&project_config)["servers"]["project-only"].is_object());
}

#[test]
fn setup_mcp_uses_the_isolated_global_config_path() {
    let isolated = tempfile::tempdir().expect("isolated home");
    let home = isolated.path().join("home");
    let workspace = isolated.path().join("workspace");
    let global_config = home.join("state").join("mcp.json");
    std::fs::create_dir_all(&workspace).expect("workspace");

    let setup = run_cli(&home, &workspace, &global_config, &["setup", "--mcp"]);
    assert_success(&setup, "setup --mcp");

    let config = read_json(&global_config);
    assert!(config["servers"]["example"].is_object());
    assert!(config["servers"]["moraine-mcp"].is_object());
    assert!(
        !workspace.join("mcp.json").exists(),
        "setup --mcp must honor the configured global MCP path"
    );
}
