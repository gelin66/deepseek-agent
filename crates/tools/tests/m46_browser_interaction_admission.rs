use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dse_protocol::agent_runtime::{RunId, RunPermissionMode, ToolArguments, ToolInvocation};
use dse_runtime::{CancellationToken, ToolExecutor};
use dse_tools::shell::ShellPolicy;
use dse_tools::{
    PRODUCTION_TOOL_NAMES, ProductionToolConfig, ProductionToolExecutor, WebFetchHttpResponse,
    WebFetchNetwork, WebFetchNetworkError,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken as TokioCancellationToken;

const MANIFEST: &str = "eval/manifests/m46-browser-interaction-admission-v1.json";
const LEASE_PLACEHOLDER: &str = "{{DSE_APPLICATION_PROBE_LEASE}}";

#[derive(Debug)]
struct FixtureNetwork {
    expected_url: String,
    body: Vec<u8>,
}

#[async_trait]
impl WebFetchNetwork for FixtureNetwork {
    fn identity(&self) -> &str {
        "m46-browser-interaction-http-control-v1"
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

struct LocalFixture {
    origin: String,
    cancellation: TokioCancellationToken,
    task: JoinHandle<()>,
}

impl LocalFixture {
    async fn start(body: Vec<u8>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind interaction fixture");
        let origin = format!(
            "http://{}",
            listener.local_addr().expect("fixture identity")
        );
        let cancellation = TokioCancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((mut stream, _)) => {
                            let body = body.clone();
                            connections.spawn(async move {
                                let mut request = Vec::new();
                                let mut chunk = [0_u8; 2048];
                                while request.len() <= 64 * 1024 {
                                    let Ok(read) = stream.read(&mut chunk).await else { return };
                                    if read == 0 { return; }
                                    request.extend_from_slice(&chunk[..read]);
                                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                                let path = std::str::from_utf8(&request)
                                    .ok()
                                    .and_then(|request| request.lines().next())
                                    .and_then(|line| line.split_whitespace().nth(1))
                                    .unwrap_or_default();
                                let (status, media_type, response_body) = if path == "/" {
                                    ("200 OK", "text/html; charset=utf-8", body.as_slice())
                                } else {
                                    ("404 Not Found", "text/plain; charset=utf-8", b"not found".as_slice())
                                };
                                let head = format!(
                                    "HTTP/1.1 {status}\r\nContent-Type: {media_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    response_body.len()
                                );
                                let _ = stream.write_all(head.as_bytes()).await;
                                let _ = stream.write_all(response_body).await;
                                let _ = stream.shutdown().await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
            while connections.join_next().await.is_some() {}
        });
        Self {
            origin,
            cancellation,
            task,
        }
    }

    async fn shutdown(self) {
        self.cancellation.cancel();
        self.task.await.expect("interaction fixture teardown");
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
    serde_json::from_slice(&std::fs::read(path).expect("read interaction manifest"))
        .expect("parse interaction manifest")
}

fn invocation(run_id: &str, call_id: &str, name: &str, arguments: Value) -> ToolInvocation {
    ToolInvocation {
        run_id: RunId::from(run_id),
        call_id: call_id.to_owned(),
        name: name.to_owned(),
        arguments: ToolArguments::from_value(arguments),
    }
}

fn snapshot_contains(nodes: &[Value], expected: &Value) -> bool {
    nodes.iter().any(|node| {
        node["role"] == expected["role"]
            && node["accessible_name"] == expected["accessible_name"]
            && expected["state_attribute"]
                .as_str()
                .is_none_or(|attribute| node["state"][attribute] == expected["state_value"])
    })
}

#[tokio::test]
#[ignore = "requires pinned Chrome for Testing; run only through the post-W2 admission evaluator"]
async fn current_read_only_browser_cannot_complete_pre_registered_click_tasks() {
    assert_eq!(PRODUCTION_TOOL_NAMES.len(), 14);
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_navigate"));
    for forbidden in [
        "browser_click",
        "browser_fill",
        "browser_press",
        "browser_wait",
        "browser_snapshot",
    ] {
        assert!(!PRODUCTION_TOOL_NAMES.contains(&forbidden));
    }

    let root = repository_root();
    let manifest = load_manifest();
    let tasks = manifest["tasks"]
        .as_array()
        .expect("interaction task matrix");
    assert_eq!(tasks.len(), 2);
    let mut matrix = Vec::new();

    for task in tasks {
        let task_id = task["task_id"].as_str().expect("task id");
        let fixture = root.join(task["fixture_path"].as_str().expect("fixture path"));
        let source = std::fs::read_to_string(&fixture).expect("read interaction fixture");
        let expected = &task["expected_post_action_observation"];
        let expected_name = expected["accessible_name"]
            .as_str()
            .expect("post-action accessible name");
        assert!(
            !source.contains(expected_name),
            "{task_id}: post-action result must not occur literally in raw HTML"
        );

        let served = source.replace(
            LEASE_PLACEHOLDER,
            &format!("dse-browser-interaction:{task_id}"),
        );
        let local = LocalFixture::start(served.into_bytes()).await;
        let public_url = format!("https://example.com/m46-interaction/{task_id}");
        let network = Arc::new(FixtureNetwork {
            expected_url: public_url.clone(),
            body: source.into_bytes(),
        });
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(root.as_path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_shell_policy(ShellPolicy::Full)
                .with_web_fetch_network(network)
                .with_browser_local_origin(Some(local.origin.clone())),
        );

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
        let http_observed_result = fetched_payload["text"]
            .as_str()
            .expect("bounded fetch text")
            .contains(expected_name);
        assert!(
            !http_observed_result,
            "{task_id}: HTTP control false success"
        );

        let navigated = executor
            .execute(
                invocation(
                    task_id,
                    &format!("control:browser_navigate:{task_id}"),
                    "browser_navigate",
                    json!({"url": format!("{}/", local.origin), "max_nodes":64, "max_chars":4096}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("execute production browser_navigate control");
        local.shutdown().await;
        assert!(navigated.is_success(), "{task_id}: {}", navigated.content);
        let browser_payload: Value =
            serde_json::from_str(&navigated.content).expect("browser JSON outcome");
        assert_eq!(browser_payload["trust"], "external_untrusted");
        assert_eq!(browser_payload["teardown"]["process_tree_settled"], true);
        assert_eq!(browser_payload["teardown"]["proxy_settled"], true);
        assert_eq!(browser_payload["teardown"]["profile_removed"], true);
        let nodes = browser_payload["snapshot"]
            .as_array()
            .expect("bounded semantic snapshot");
        let initial_target_observed = snapshot_contains(nodes, &task["action"]);
        let post_action_observed = snapshot_contains(nodes, expected);
        let element_refs_returned = nodes
            .iter()
            .filter(|node| node.get("element_ref").is_some())
            .count();
        assert!(initial_target_observed, "{task_id}: action target absent");
        assert!(
            !post_action_observed,
            "{task_id}: semantic control false success"
        );
        assert_eq!(element_refs_returned, 0, "W2 must not invent W3 refs");

        matrix.push(json!({
            "task_id": task_id,
            "independence_key": task["independence_key"],
            "loss_code": task["loss_code"],
            "action_family": task["action"]["family"],
            "http_observed_post_action_state": http_observed_result,
            "browser_initial_action_target_observed": initial_target_observed,
            "browser_observed_post_action_state": post_action_observed,
            "browser_element_refs_returned": element_refs_returned,
            "browser_action_tools_visible": 0,
            "browser_teardown": {
                "process_tree_settled": browser_payload["teardown"]["process_tree_settled"],
                "proxy_settled": browser_payload["teardown"]["proxy_settled"],
                "profile_removed": browser_payload["teardown"]["profile_removed"]
            },
            "trust": browser_payload["trust"],
            "control_verified": false,
            "control_false_success": false
        }));
    }

    println!(
        "M46_INTERACTION_CONTROL_MATRIX={}",
        serde_json::to_string(&matrix).expect("serialize interaction control matrix")
    );
}
