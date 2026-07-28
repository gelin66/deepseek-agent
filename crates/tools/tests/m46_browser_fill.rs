use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use dse_protocol::agent_runtime::{
    RunId, RunPermissionMode, ToolArguments, ToolInvocation, ToolSideEffectStatus,
};
use dse_runtime::{CancellationToken, ToolExecutor};
use dse_tools::shell::ShellPolicy;
use dse_tools::{PRODUCTION_TOOL_NAMES, ProductionToolConfig, ProductionToolExecutor};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken as TokioCancellationToken;

const MANIFEST: &str = "eval/manifests/m46-post-w3-interaction-admission-v1.json";
const LEASE_PLACEHOLDER: &str = "{{DSE_APPLICATION_PROBE_LEASE}}";

struct LocalFixture {
    origin: String,
    cancellation: TokioCancellationToken,
    task: JoinHandle<()>,
}

impl LocalFixture {
    async fn start(body: Vec<u8>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind post-W3 fixture");
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
        self.task.await.expect("post-W3 fixture teardown");
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
    serde_json::from_slice(&std::fs::read(path).expect("read post-W3 manifest"))
        .expect("parse post-W3 manifest")
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
#[ignore = "requires repository-pinned Chrome for Testing; credential-free W3.1 fixture gate"]
async fn ref_based_fill_completes_both_frozen_exact_loopback_tasks() {
    assert_eq!(PRODUCTION_TOOL_NAMES.len(), 16);
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_navigate"));
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_click"));
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_fill"));
    for forbidden in ["browser_press", "browser_wait", "browser_snapshot"] {
        assert!(!PRODUCTION_TOOL_NAMES.contains(&forbidden));
    }

    let root = repository_root();
    let manifest = load_manifest();
    let tasks = manifest["tasks"].as_array().expect("post-W3 tasks");
    assert_eq!(tasks.len(), 2);
    let mut matrix = Vec::new();

    for task in tasks {
        let task_id = task["task_id"].as_str().expect("task id");
        let source = std::fs::read_to_string(
            root.join(task["fixture_path"].as_str().expect("fixture path")),
        )
        .expect("read post-W3 fixture");
        let expected = &task["expected_post_action_observation"];
        let served = source.replace(
            LEASE_PLACEHOLDER,
            &format!("dse-w3-1-browser-fill:{task_id}"),
        );
        let local = LocalFixture::start(served.into_bytes()).await;
        let executor = ProductionToolExecutor::new(
            ProductionToolConfig::new(root.as_path())
                .with_permission_mode(RunPermissionMode::Agent)
                .with_shell_policy(ShellPolicy::Full)
                .with_browser_local_origin(Some(local.origin.clone())),
        );

        let navigated = executor
            .execute(
                invocation(
                    task_id,
                    &format!("navigate:{task_id}"),
                    "browser_navigate",
                    json!({"url":format!("{}/",local.origin),"max_nodes":64,"max_chars":4096}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("production browser_navigate control");
        assert!(navigated.is_success(), "{task_id}: {}", navigated.content);
        let browser_payload: Value =
            serde_json::from_str(&navigated.content).expect("browser JSON");
        assert_eq!(browser_payload["trust"], "external_untrusted");
        assert_eq!(browser_payload["session_live"], true);
        assert_eq!(browser_payload["page_epoch"], 1);
        let nodes = browser_payload["snapshot"]
            .as_array()
            .expect("semantic snapshot");
        let target = nodes
            .iter()
            .find(|node| {
                node["role"] == task["action"]["role"]
                    && node["accessible_name"] == task["action"]["accessible_name"]
            })
            .expect("fill target in fresh semantic observation");
        let element_ref = target["element_ref"]
            .as_str()
            .expect("Host-generated fill-only opaque ref")
            .to_owned();
        assert!(!snapshot_contains(nodes, expected));

        let filled = executor
            .execute(
                invocation(
                    task_id,
                    &format!("fill:{task_id}"),
                    "browser_fill",
                    json!({"element_ref":element_ref,"value":task["action"]["value"]}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("production fill");
        assert!(filled.is_success(), "{task_id}: {}", filled.content);
        assert_eq!(filled.side_effect, ToolSideEffectStatus::Applied);
        let post: Value = serde_json::from_str(&filled.content).expect("fill JSON");
        assert_eq!(post["action"]["kind"], "fill");
        assert_eq!(post["page_epoch"], 2);
        assert_ne!(post["snapshot_id"], browser_payload["snapshot_id"]);
        assert_eq!(post["trust"], "external_untrusted");
        assert!(snapshot_contains(
            post["snapshot"]
                .as_array()
                .expect("fresh post-fill snapshot"),
            expected
        ));

        let stale = executor
            .execute(
                invocation(
                    task_id,
                    &format!("stale:{task_id}"),
                    "browser_fill",
                    json!({"element_ref":element_ref,"value":task["action"]["value"]}),
                ),
                CancellationToken::default(),
            )
            .await
            .expect("stale fill outcome");
        assert!(!stale.is_success());
        assert_eq!(stale.side_effect, ToolSideEffectStatus::NotApplied);
        let stale_payload: Value = serde_json::from_str(&stale.content).expect("stale JSON");
        assert_eq!(
            stale_payload["failure"]["code"],
            "browser_element_ref_stale"
        );
        assert_eq!(stale_payload["fresh_observation"]["page_epoch"], 3);

        matrix.push(json!({
            "task_id":task_id,
            "independence_key":task["independence_key"],
            "action_family":task["action"]["family"],
            "root_permission_mode":"agent",
            "initial_epoch":browser_payload["page_epoch"],
            "post_fill_epoch":post["page_epoch"],
            "fresh_post_fill_observed":true,
            "stale_reuse_side_effect":"not_applied",
            "trust":post["trust"]
        }));

        drop(executor);
        local.shutdown().await;
    }

    println!(
        "M46_W3_1_BROWSER_FILL_MATRIX={}",
        serde_json::to_string(&matrix).expect("serialize W3.1 fill matrix")
    );
}
