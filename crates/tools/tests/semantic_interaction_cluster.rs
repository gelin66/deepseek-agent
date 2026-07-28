use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dse_protocol::agent_runtime::{
    RunId, RunPermissionMode, ToolArguments, ToolAuthorizationDisposition, ToolExecutionGrant,
    ToolInvocation, ToolSideEffectStatus,
};
use dse_protocol::task::{WorkspaceRevision, WorkspaceState};
use dse_runtime::{CancellationToken, ToolExecutor};
use dse_tools::shell::ShellPolicy;
use dse_tools::{
    PRODUCTION_TOOL_NAMES, ProductionToolConfig, ProductionToolExecutor, WebFetchHttpResponse,
    WebFetchNetwork, WebFetchNetworkError,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken as TokioCancellationToken;

const PUBLIC_HOST: &str = "dse-public.example";
const PUBLIC_ADDRESS: &str = "93.184.216.34:80";

struct HttpFixture {
    origin: String,
    address: SocketAddr,
    cancellation: TokioCancellationToken,
    task: JoinHandle<()>,
}

impl HttpFixture {
    async fn local() -> Self {
        Self::start(false).await
    }

    async fn public() -> Self {
        Self::start(true).await
    }

    async fn start(public: bool) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind semantic interaction fixture");
        let address = listener.local_addr().expect("fixture address");
        let origin = if public {
            format!("http://{PUBLIC_HOST}")
        } else {
            format!("http://{address}")
        };
        let cancellation = TokioCancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            connections.spawn(serve_connection(stream, public));
                        }
                        Err(_) => break,
                    }
                }
            }
            while connections.join_next().await.is_some() {}
        });
        Self {
            origin,
            address,
            cancellation,
            task,
        }
    }

    async fn shutdown(self) {
        self.cancellation.cancel();
        self.task
            .await
            .expect("semantic interaction fixture teardown");
    }
}

async fn serve_connection(mut stream: TcpStream, public: bool) {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let Ok(read) = stream.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&chunk[..read]);
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if request.len() > 64 * 1024 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let Ok(read) = stream.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let first = head.lines().next().unwrap_or_default();
    let mut words = first.split_whitespace();
    let method = words.next().unwrap_or_default();
    let path = words.next().unwrap_or_default();
    let posted = String::from_utf8_lossy(&request[header_end..header_end + content_length]);

    let (status, extra_headers, body) = if public && method == "POST" && path == "/drafts/save" {
        let receipt = if posted.contains("notes=reviewed") && posted.contains("action=save_draft") {
            "draft-receipt-001"
        } else {
            "unexpected-parameters"
        };
        (
            "200 OK",
            format!("X-DSE-Receipt: {receipt}\r\n"),
            format!(
                "<!doctype html><title>Draft saved</title><main role=status data-receipt=\"{receipt}\">Draft saved: {receipt}</main>"
            ),
        )
    } else if public && method == "GET" && path == "/" {
        ("200 OK", String::new(), public_page().to_owned())
    } else if !public && method == "GET" && path == "/detail" {
        (
            "200 OK",
            String::new(),
            "<!doctype html><title>Detail</title><main><h1>Detail page</h1></main>".to_owned(),
        )
    } else if !public && method == "GET" && (path == "/" || path == "/tab") {
        (
            "200 OK",
            String::new(),
            local_page(path == "/tab").to_owned(),
        )
    } else {
        ("404 Not Found", String::new(), "not found".to_owned())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn local_page(tab: bool) -> &'static str {
    if tab {
        return "<!doctype html><title>Second tab</title><main><h1>Second tab ready</h1></main>";
    }
    r#"<!doctype html>
<title>Interaction cluster</title>
<main>
  <textarea aria-label="Notes" autocomplete="off" spellcheck="false"></textarea>
  <div role="textbox" aria-label="Editor" contenteditable="true" spellcheck="false"></div>
  <input aria-label="Command" type="text" autocomplete="off" spellcheck="false">
  <select aria-label="Environment"><option>Development</option><option>Staging</option></select>
  <section role="region" aria-label="Scrollable panel" tabindex="0" style="height:40px;overflow:auto"><div style="height:400px">Scrollable content</div></section>
  <a href="/detail">Open detail</a>
  <p role="status" data-state="idle">Idle</p>
</main>
<script>
const status = document.querySelector('[role=status]');
document.querySelector('textarea').addEventListener('input', e => { status.textContent = `Notes ${e.target.value}`; status.dataset.state = 'notes'; });
document.querySelector('[contenteditable]').addEventListener('input', e => { status.textContent = `Editor ${e.target.textContent}`; status.dataset.state = 'editor'; });
document.querySelector('input').addEventListener('keydown', e => { if (e.key === 'Enter') { status.textContent = `Command ${e.target.value}`; status.dataset.state = 'command'; }});
document.querySelector('select').addEventListener('change', e => { status.textContent = `Environment ${e.target.value}`; status.dataset.state = 'selected'; });
</script>"#
}

fn public_page() -> &'static str {
    r#"<!doctype html><title>Disposable draft</title><main>
<form id="draft-form" method="post" action="/drafts/save"></form>
<textarea form="draft-form" name="notes" aria-label="Draft notes"></textarea>
<input type="password" form="draft-form" name="password" aria-label="Password">
<input type="file" form="draft-form" name="upload" aria-label="Upload">
<button form="draft-form" formmethod="post" formaction="/drafts/save" name="action" value="save_draft">Save draft</button>
<button form="draft-form" formmethod="post" formaction="/publish" name="action" value="publish">Publish</button>
</main>"#
}

#[derive(Debug)]
struct PublicMappedNetwork {
    local_address: SocketAddr,
}

#[async_trait]
impl WebFetchNetwork for PublicMappedNetwork {
    fn identity(&self) -> &str {
        "semantic-interaction-public-map-v1"
    }

    async fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
        assert_eq!(host, PUBLIC_HOST);
        assert_eq!(port, 80);
        Ok(vec![
            PUBLIC_ADDRESS.parse().expect("public fixture address"),
        ])
    }

    async fn get(
        &self,
        _url: &reqwest::Url,
        _pinned_addresses: &[SocketAddr],
        _timeout: Duration,
    ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
        Err(WebFetchNetworkError::new(
            "unused_get",
            "semantic browser uses the connect seam",
        ))
    }

    async fn connect(&self, address: SocketAddr) -> Result<TcpStream, WebFetchNetworkError> {
        assert_eq!(address, PUBLIC_ADDRESS.parse().unwrap());
        TcpStream::connect(self.local_address)
            .await
            .map_err(|error| WebFetchNetworkError::new("fixture_connect", error.to_string()))
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tools crate is under repository root")
        .to_path_buf()
}

fn invocation(run_id: &str, call_id: &str, name: &str, arguments: Value) -> ToolInvocation {
    ToolInvocation {
        run_id: RunId::from(run_id),
        call_id: call_id.to_owned(),
        name: name.to_owned(),
        arguments: ToolArguments::from_value(arguments),
    }
}

async fn execute(
    executor: &ProductionToolExecutor,
    run_id: &str,
    call_id: &str,
    name: &str,
    arguments: Value,
) -> (dse_tools::ToolOutcome, Value) {
    let outcome = executor
        .execute(
            invocation(run_id, call_id, name, arguments),
            CancellationToken::default(),
        )
        .await
        .expect("production semantic interaction");
    let value = serde_json::from_str(&outcome.content).unwrap_or_else(|error| {
        panic!("{call_id} returned non-JSON ({error}): {}", outcome.content)
    });
    (outcome, value)
}

fn element_ref(snapshot: &Value, name: &str, capability: &str) -> String {
    snapshot["snapshot"]
        .as_array()
        .unwrap_or_else(|| panic!("missing snapshot nodes: {snapshot}"))
        .iter()
        .find(|node| {
            node["accessible_name"] == name
                && node["capabilities"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == capability))
        })
        .and_then(|node| node["element_ref"].as_str())
        .unwrap_or_else(|| panic!("missing {capability} ref for {name}: {snapshot}"))
        .to_owned()
}

fn workspace_state() -> WorkspaceState {
    WorkspaceState {
        generation: 1,
        revision: WorkspaceRevision::Known {
            sha256: "semantic-interaction-fixture".to_owned(),
        },
    }
}

#[tokio::test]
#[ignore = "requires repository-pinned Chrome for Testing; credential-free interaction cluster gate"]
async fn semantic_interaction_cluster_completes_local_and_scoped_public_tasks() {
    assert_eq!(PRODUCTION_TOOL_NAMES.len(), 15);
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_navigate"));
    assert!(PRODUCTION_TOOL_NAMES.contains(&"browser_interact"));
    for deleted in ["browser_click", "browser_fill"] {
        assert!(!PRODUCTION_TOOL_NAMES.contains(&deleted));
    }
    for unintroduced in ["browser_press", "browser_wait", "browser_snapshot"] {
        assert!(!PRODUCTION_TOOL_NAMES.contains(&unintroduced));
    }

    let local = HttpFixture::local().await;
    let local_executor = ProductionToolExecutor::new(
        ProductionToolConfig::new(repository_root())
            .with_permission_mode(RunPermissionMode::Agent)
            .with_shell_policy(ShellPolicy::Full)
            .with_browser_local_origin(Some(local.origin.clone())),
    );
    let run_id = "semantic-local-cluster";
    let (_, mut state) = execute(
        &local_executor,
        run_id,
        "navigate",
        "browser_navigate",
        json!({"url":format!("{}/",local.origin),"max_nodes":128,"max_chars":20_000}),
    )
    .await;
    assert_eq!(state["session_live"], true);
    assert_eq!(state["page_epoch"], 1);

    for (call, name, value) in [
        ("fill-notes", "Notes", "reviewed"),
        ("fill-editor", "Editor", "ready"),
        ("fill-command", "Command", "deploy"),
    ] {
        let reference = element_ref(&state, name, "fill");
        let (outcome, next) = execute(
            &local_executor,
            run_id,
            call,
            "browser_interact",
            json!({"action":"fill","element_ref":reference,"value":value}),
        )
        .await;
        assert!(outcome.is_success(), "{}", outcome.content);
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::Applied);
        state = next;
    }

    let command = element_ref(&state, "Command", "press");
    let (pressed, next) = execute(
        &local_executor,
        run_id,
        "press-space",
        "browser_interact",
        json!({"action":"press","element_ref":command,"key":"space"}),
    )
    .await;
    assert!(pressed.is_success(), "{}", pressed.content);
    state = next;
    assert_eq!(state["action"]["kind"], "press");

    let select = element_ref(&state, "Environment", "select");
    let (selected, next) = execute(
        &local_executor,
        run_id,
        "select",
        "browser_interact",
        json!({"action":"select","element_ref":select,"value":"Staging"}),
    )
    .await;
    assert!(selected.is_success(), "{}", selected.content);
    state = next;
    assert!(
        state["snapshot"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| { node["accessible_name"] == "Environment" && node["value"] == "Staging" })
    );

    let region = element_ref(&state, "Scrollable panel", "scroll");
    let (scrolled, next) = execute(
        &local_executor,
        run_id,
        "scroll",
        "browser_interact",
        json!({"action":"scroll","element_ref":region,"direction":"down","amount":240}),
    )
    .await;
    assert!(scrolled.is_success(), "{}", scrolled.content);
    state = next;
    assert_eq!(state["action"]["kind"], "scroll");

    let (waited, next) = execute(
        &local_executor,
        run_id,
        "wait",
        "browser_interact",
        json!({"action":"wait","condition":"text_present","value":"Editor ready","timeout_ms":1_000}),
    )
    .await;
    assert!(waited.is_success(), "{}", waited.content);
    state = next;

    let link = element_ref(&state, "Open detail", "click");
    let (_, detail) = execute(
        &local_executor,
        run_id,
        "click-detail",
        "browser_interact",
        json!({"action":"click","element_ref":link}),
    )
    .await;
    assert!(detail.to_string().contains("Detail page"));
    let (_, state) = execute(
        &local_executor,
        run_id,
        "back",
        "browser_interact",
        json!({"action":"back"}),
    )
    .await;
    let first_page = state["active_page_ref"]
        .as_str()
        .expect("active page ref")
        .to_owned();
    let (_, tab) = execute(
        &local_executor,
        run_id,
        "tab-open",
        "browser_interact",
        json!({"action":"tab_open","url":format!("{}/tab",local.origin)}),
    )
    .await;
    assert!(tab.to_string().contains("Second tab ready"));
    let second_page = tab["active_page_ref"]
        .as_str()
        .expect("second page ref")
        .to_owned();
    let (_, switched) = execute(
        &local_executor,
        run_id,
        "tab-switch",
        "browser_interact",
        json!({"action":"tab_switch","page_ref":first_page}),
    )
    .await;
    assert_eq!(switched["active_page_ref"], first_page);
    let (closed, _) = execute(
        &local_executor,
        run_id,
        "tab-close",
        "browser_interact",
        json!({"action":"tab_close","page_ref":second_page}),
    )
    .await;
    assert!(closed.is_success(), "{}", closed.content);
    drop(local_executor);
    local.shutdown().await;

    let public = HttpFixture::public().await;
    let network = Arc::new(PublicMappedNetwork {
        local_address: public.address,
    });
    let public_executor = ProductionToolExecutor::new(
        ProductionToolConfig::new(repository_root())
            .with_permission_mode(RunPermissionMode::Ask)
            .with_shell_policy(ShellPolicy::Full)
            .with_web_fetch_network(network),
    );
    let public_run = "semantic-public-cluster";
    let (_, initial) = execute(
        &public_executor,
        public_run,
        "public-navigate",
        "browser_navigate",
        json!({"url":format!("{}/",public.origin),"max_nodes":128,"max_chars":20_000}),
    )
    .await;
    let notes = element_ref(&initial, "Draft notes", "fill");
    let (_, filled) = execute(
        &public_executor,
        public_run,
        "public-fill",
        "browser_interact",
        json!({"action":"fill","element_ref":notes,"value":"reviewed"}),
    )
    .await;
    let submit = element_ref(&filled, "Save draft", "submit");
    assert!(filled.to_string().contains("Password"));
    assert!(
        !filled
            .to_string()
            .contains("\"accessible_name\":\"Password\",\"capabilities\":[\"fill\"]")
    );
    assert!(
        !filled
            .to_string()
            .contains("\"accessible_name\":\"Publish\",\"capabilities\":[\"submit\"]")
    );
    let submit_invocation = invocation(
        public_run,
        "public-submit",
        "browser_interact",
        json!({"action":"submit","element_ref":submit}),
    );
    let decision = public_executor
        .authorize(
            RunPermissionMode::Ask,
            &ToolExecutionGrant::Ordinary,
            &submit_invocation,
            &workspace_state(),
        )
        .expect("Host authorization decision");
    assert_eq!(decision.disposition, ToolAuthorizationDisposition::Ask);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("scoped_public_reversible_side_effect")
    );
    let prompt = decision
        .prompt
        .expect("exact public action preview")
        .description;
    assert!(prompt.contains("http://dse-public.example/drafts/save"));
    assert!(prompt.contains("reversible_draft_write"));
    assert!(prompt.contains("parameters:"));

    let submitted = public_executor
        .execute(submit_invocation, CancellationToken::default())
        .await
        .expect("approved exact submit executes once");
    assert!(submitted.is_success(), "{}", submitted.content);
    assert_eq!(submitted.side_effect, ToolSideEffectStatus::Applied);
    let receipt: Value = serde_json::from_str(&submitted.content).expect("submit JSON");
    assert_eq!(receipt["action"]["kind"], "submit");
    assert_eq!(receipt["action"]["receipt"]["status"], 200);
    assert_eq!(
        receipt["action"]["receipt"]["remote_receipt"],
        "draft-receipt-001"
    );
    assert_eq!(
        receipt["action"]["receipt"]["semantic_receipt"],
        "draft-receipt-001"
    );
    drop(public_executor);
    public.shutdown().await;
}
