use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dse_protocol::agent_runtime::{RunId, RunPermissionMode, ToolArguments, ToolInvocation};
use dse_protocol::task::{VerifierVerdict, WorkspaceRevision};
use dse_runtime::{CancellationToken, ToolExecutor};
use dse_tools::shell::ShellPolicy;
use dse_tools::{
    ProductionToolConfig, ProductionToolExecutor, WebFetchHttpResponse, WebFetchNetwork,
    WebFetchNetworkError,
};
use serde_json::{Value, json};

const MANIFEST: &str = "eval/manifests/m46-semantic-browser-admission-v1.json";

#[derive(Debug)]
struct FixtureNetwork {
    expected_url: String,
    body: Vec<u8>,
}

#[async_trait]
impl WebFetchNetwork for FixtureNetwork {
    fn identity(&self) -> &str {
        "m46-js-only-control-fixture-v1"
    }

    async fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
        Ok(vec![
            "93.184.216.34:443".parse().expect("public fixture address"),
        ])
    }

    async fn get(
        &self,
        url: &reqwest::Url,
        pinned_addresses: &[SocketAddr],
        _timeout: Duration,
    ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
        assert_eq!(url.as_str(), self.expected_url);
        assert_eq!(
            pinned_addresses,
            &["93.184.216.34:443".parse().expect("public fixture address")]
        );
        Ok(WebFetchHttpResponse::new(
            200,
            BTreeMap::from([(
                "content-type".to_owned(),
                "text/html; charset=utf-8".to_owned(),
            )]),
            self.body.clone(),
        ))
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tools crate is under the repository root")
        .to_path_buf()
}

fn load_manifest() -> Value {
    let path = repository_root().join(MANIFEST);
    serde_json::from_slice(&std::fs::read(path).expect("read M46 admission manifest"))
        .expect("parse M46 admission manifest")
}

fn invocation(run_id: &str, call_id: &str, name: &str, arguments: Value) -> ToolInvocation {
    ToolInvocation {
        run_id: RunId::from(run_id),
        call_id: call_id.to_owned(),
        name: name.to_owned(),
        arguments: ToolArguments::from_value(arguments),
    }
}

fn initialize_workspace(server: &Path, fixture: &Path) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("create M46 fixture workspace");
    std::fs::copy(server, workspace.path().join("server.py")).expect("copy fixture server");
    std::fs::copy(fixture, workspace.path().join("index.html")).expect("copy JS-only page");
    for arguments in [
        vec!["init", "--quiet"],
        vec!["add", "server.py", "index.html"],
        vec![
            "-c",
            "user.name=DSE M46 Audit",
            "-c",
            "user.email=m46.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(workspace.path())
                .status()
                .expect("run fixture git command")
                .success()
        );
    }
    workspace
}

#[tokio::test]
async fn m46_current_controls_cannot_claim_js_only_rendered_application_state() {
    let root = repository_root();
    let manifest = load_manifest();
    let shared = &manifest["shared_fixture"];
    let server = root.join(shared["server_path"].as_str().expect("server path"));
    let tasks = manifest["tasks"].as_array().expect("task matrix");
    assert_eq!(tasks.len(), 2);

    let mut matrix = Vec::new();
    for task in tasks {
        let task_id = task["task_id"].as_str().expect("task id");
        let fixture = root.join(task["fixture_path"].as_str().expect("fixture path"));
        let source = std::fs::read(&fixture).expect("read JS-only fixture");
        let expected = &task["expected_rendered_observation"];
        let accessible_name = expected["accessible_name"]
            .as_str()
            .expect("accessible name");
        assert!(
            !String::from_utf8_lossy(&source).contains(accessible_name),
            "{task_id}: rendered accessible name must not occur in the raw response"
        );

        let public_url = format!("https://example.com/m46/{task_id}");
        let workspace = initialize_workspace(&server, &fixture);
        let config = ProductionToolConfig::new(workspace.path())
            .with_permission_mode(RunPermissionMode::Agent)
            .with_shell_policy(ShellPolicy::Full)
            .with_web_fetch_network(Arc::new(FixtureNetwork {
                expected_url: public_url.clone(),
                body: source,
            }));
        let executor = ProductionToolExecutor::new(config);

        let fetched = executor
            .execute(
                invocation(
                    task_id,
                    &format!("control:web_fetch:{task_id}"),
                    "web_fetch",
                    json!({"url": public_url, "max_chars": 4096}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("execute production web_fetch control");
        assert!(fetched.is_success(), "{task_id}: {}", fetched.content);
        let fetched_payload: Value =
            serde_json::from_str(&fetched.content).expect("web_fetch JSON outcome");
        assert_eq!(fetched_payload["title"], expected["title"]);
        assert_eq!(fetched_payload["trust"], "external_untrusted");
        assert!(
            !fetched_payload["text"]
                .as_str()
                .expect("bounded web_fetch text")
                .contains(accessible_name),
            "{task_id}: web_fetch must not execute script or invent rendered state"
        );

        let resolved = executor
            .resolve_verifier_spec(
                "application_probe",
                json!({
                    "program": "/usr/bin/python3",
                    "args": ["-I", "-B", "server.py", "{dse_probe_lease}"],
                    "env": {"M46_FIXTURE_HTML": "index.html"},
                    "body_contains": accessible_name,
                    "startup_timeout_ms": 1000,
                    "health_timeout_ms": 2500,
                    "overall_timeout_ms": 5000,
                    "max_log_bytes": 4096,
                    "max_response_bytes": 16384
                }),
            )
            .expect("resolve Host-owned application probe");
        let probed = executor
            .execute(
                invocation(
                    task_id,
                    &format!("host:m46:{task_id}"),
                    "application_probe",
                    resolved.parameters,
                ),
                CancellationToken::default(),
            )
            .await
            .expect("execute production application probe control");
        assert!(!probed.is_success(), "{task_id}: {}", probed.content);
        let probe_payload: Value =
            serde_json::from_str(&probed.content).expect("application_probe JSON outcome");
        assert_eq!(
            probe_payload["failure_code"],
            task["expected_control"]["application_probe_failure_code"]
        );
        assert_eq!(probe_payload["health_ready"], true);
        assert_eq!(probe_payload["teardown"]["process_tree_settled"], true);
        assert!(
            !probe_payload["assertion"]["body_excerpt"]
                .as_str()
                .expect("bounded assertion excerpt")
                .contains(accessible_name)
        );
        let failed_observation = probed
            .verifier_observation
            .as_ref()
            .expect("typed failed verifier observation");
        assert_eq!(failed_observation.verdict, VerifierVerdict::Failed);
        let observed_revision = match &failed_observation.workspace_revision {
            WorkspaceRevision::Known { sha256 } => sha256,
            WorkspaceRevision::Unknown { reason } => {
                panic!("{task_id}: workspace revision must be known: {reason}")
            }
        };
        assert_eq!(
            probed.workspace_revision.as_deref(),
            Some(observed_revision.as_str())
        );

        matrix.push(json!({
            "task_id": task_id,
            "loss_code": task["loss_code"],
            "web_fetch_observed_rendered_state": false,
            "application_probe_failure_code": probe_payload["failure_code"],
            "application_probe_health_ready": probe_payload["health_ready"],
            "application_probe_teardown_settled": probe_payload["teardown"]["process_tree_settled"],
            "application_probe_verdict": "failed",
            "latest_revision_bound": true,
            "control_false_success": false
        }));
    }

    println!(
        "M46_CONTROL_MATRIX={}",
        serde_json::to_string(&matrix).expect("serialize deterministic control matrix")
    );
}
