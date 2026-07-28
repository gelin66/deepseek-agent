//! One-shot, read-only semantic browser backed by pinned Chrome for Testing.
//!
//! The model receives one `browser_navigate` tool. Each invocation starts an
//! ephemeral Chrome profile, navigates once, captures a bounded AX/DOM
//! observation, and tears the whole process tree down. There is deliberately
//! no durable browser session, action surface, screenshot, or arbitrary JS.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};
use tokio_util::sync::CancellationToken;

use crate::shell::{ProcessTreeOwner, configure_process_tree, shutdown_tokio_process_tree};
use crate::web_fetch::{WebFetchNetwork, is_public_ip};
use crate::{ToolOutcome, optional_u64, required_str};
use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
    ToolTransportStatus,
};

const TRUST: &str = "external_untrusted";
const ADAPTER_ID: &str = "direct_tokio_cdp_read_only_v1";
const PINNED_CFT_VERSION: &str = "151.0.7922.47";
const DEFAULT_MAX_NODES: usize = 128;
const MAX_MAX_NODES: usize = 256;
const DEFAULT_MAX_CHARS: usize = 20_000;
const MAX_MAX_CHARS: usize = 50_000;
const MAX_URL_CHARS: usize = 4_096;
const MAX_CDP_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROXY_REQUEST_BYTES: usize = 64 * 1024;
const MAX_NETWORK_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_NETWORK_REQUEST_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_NETWORK_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_NETWORK_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_DECODED_BODY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REDIRECTS: u64 = 5;
const OVERALL_DEADLINE: Duration = Duration::from_secs(20);
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PROFILE_DISCOVERY_DEADLINE: Duration = Duration::from_secs(15);
const PAGE_LOAD_DEADLINE: Duration = Duration::from_secs(10);
const RENDER_SETTLE_DELAY: Duration = Duration::from_millis(150);
const PROCESS_TEARDOWN_GRACE: Duration = Duration::from_millis(750);
const PROXY_TEARDOWN_GRACE: Duration = Duration::from_secs(2);

/// Cancellation signal accepted by deterministic browser harness fixtures.
pub type BrowserCancellationToken = CancellationToken;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const PINNED_CFT_EXECUTABLE_SHA256: Option<&str> =
    Some("e9e1c766953cf2ff5ea38c6cb63fa32b443a958c3fda7dcc3b60dd9b20436855");
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const PINNED_CFT_EXECUTABLE_SHA256: Option<&str> = None;

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserTargetScope {
    Public,
    ExactLocal(Origin),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl Origin {
    fn from_url(url: &Url) -> Result<Self, BrowserFailure> {
        let scheme = url.scheme();
        if !matches!(scheme, "http" | "https") {
            return Err(BrowserFailure::rejected(
                "browser_scheme_denied",
                "url",
                "browser_navigate 只允许 HTTP(S) URL",
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(BrowserFailure::rejected(
                "browser_userinfo_denied",
                "url",
                "browser URL 不允许包含 userinfo、认证信息或密码",
            ));
        }
        let host = url.host_str().ok_or_else(|| {
            BrowserFailure::rejected("browser_url_host_missing", "url", "browser URL 缺少主机名")
        })?;
        let port = url.port_or_known_default().ok_or_else(|| {
            BrowserFailure::rejected("browser_port_missing", "url", "browser URL 缺少有效端口")
        })?;
        Ok(Self {
            scheme: scheme.to_owned(),
            host: host
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .unwrap_or(host)
                .trim_end_matches('.')
                .to_ascii_lowercase(),
            port,
        })
    }

    fn canonical(&self) -> String {
        let default = (self.scheme == "http" && self.port == 80)
            || (self.scheme == "https" && self.port == 443);
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if default {
            format!("{}://{host}", self.scheme)
        } else {
            format!("{}://{host}:{}", self.scheme, self.port)
        }
    }
}

/// Canonical input already checked by the fixed tool schema and Host target policy.
#[derive(Debug, Clone)]
pub struct BrowserNavigateRequest {
    requested_url: String,
    initial_url: Url,
    scope: BrowserTargetScope,
    max_nodes: usize,
    max_chars: usize,
}

/// Narrow deterministic seam used only by production loopback/reopen tests.
/// The system implementation below remains the only default production path.
#[async_trait]
pub trait SemanticBrowserHarness: Send + Sync {
    fn identity(&self) -> String;

    async fn navigate(
        &self,
        request: BrowserNavigateRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome;
}

#[derive(Debug, Clone)]
struct PinnedChromeForTesting {
    executable: PathBuf,
    version: &'static str,
    sha256: &'static str,
}

impl PinnedChromeForTesting {
    fn discover() -> Option<Self> {
        let sha256 = PINNED_CFT_EXECUTABLE_SHA256?;
        let executable = std::env::var_os("DSE_CHROME_FOR_TESTING_PATH")
            .map(PathBuf::from)
            .or_else(default_cft_path)?;
        Some(Self {
            executable,
            version: PINNED_CFT_VERSION,
            sha256,
        })
    }

    fn identity(&self) -> String {
        format!(
            "{ADAPTER_ID}:cft={}:sha256={}:path_sha256={}",
            self.version,
            self.sha256,
            sha256_text(&self.executable.display().to_string())
        )
    }
}

fn default_cft_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some(home.join(format!(
            "Library/Caches/dse/chrome-for-testing/{PINNED_CFT_VERSION}/mac-arm64/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        )))
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        let _ = home;
        None
    }
}

/// Default one-shot system harness. It never downloads or upgrades Chrome.
pub struct SystemSemanticBrowserHarness {
    binary: Option<PinnedChromeForTesting>,
    resolver: Arc<dyn WebFetchNetwork>,
}

impl SystemSemanticBrowserHarness {
    #[must_use]
    pub fn new(resolver: Arc<dyn WebFetchNetwork>) -> Self {
        Self {
            binary: PinnedChromeForTesting::discover(),
            resolver,
        }
    }

    pub(crate) fn configured_identity(resolver: &dyn WebFetchNetwork) -> String {
        let binary = PinnedChromeForTesting::discover().map_or_else(
            || format!("{ADAPTER_ID}:unavailable"),
            |binary| binary.identity(),
        );
        format!("{binary}:resolver={}", resolver.identity())
    }
}

impl std::fmt::Debug for SystemSemanticBrowserHarness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SystemSemanticBrowserHarness")
            .field("binary_configured", &self.binary.is_some())
            .field("resolver", &self.resolver.identity())
            .finish()
    }
}

#[derive(Debug, Clone)]
struct BrowserFailure {
    code: &'static str,
    stage: &'static str,
    message: String,
    transport: bool,
    retry: ToolRetryDisposition,
}

impl BrowserFailure {
    fn rejected(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            transport: false,
            retry: ToolRetryDisposition::AfterCorrection,
        }
    }

    fn operation(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            transport: false,
            retry: ToolRetryDisposition::NotRetryable,
        }
    }

    fn transport(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            transport: true,
            retry: ToolRetryDisposition::Safe,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SemanticNode {
    role: String,
    accessible_name: String,
    text: String,
    value: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    state: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserResult {
    requested_url: String,
    final_url: String,
    title: String,
    snapshot: Vec<SemanticNode>,
    snapshot_sha256: String,
    snapshot_sha256_scope: &'static str,
    nodes_read: usize,
    nodes_returned: usize,
    bytes_returned: usize,
    truncated: bool,
    redirect_count: u64,
    network_requests: u64,
    network_bytes_sent: u64,
    network_bytes_received: u64,
    decoded_body_bytes: u64,
    blocked_requests: u64,
    browser_product: String,
    cdp_protocol_version: String,
    chrome_for_testing_version: &'static str,
    chrome_executable_sha256: String,
    retrieved_at: String,
    trust: &'static str,
    profile_ephemeral: bool,
    process_tree_settled: bool,
    teardown: TeardownFacts,
}

pub(crate) fn browser_harness_identity(
    harness: Option<&dyn SemanticBrowserHarness>,
    resolver: &dyn WebFetchNetwork,
) -> String {
    harness.map_or_else(
        || SystemSemanticBrowserHarness::configured_identity(resolver),
        SemanticBrowserHarness::identity,
    )
}

pub(crate) fn preflight_browser_navigate(
    input: &Value,
    exact_local_origin: Option<&str>,
) -> Option<ToolOutcome> {
    match parse_request(input, exact_local_origin) {
        Ok(_) => None,
        Err(failure) => Some(rejected_outcome("", failure)),
    }
}

pub(crate) async fn execute_browser_navigate(
    input: Value,
    harness: Arc<dyn SemanticBrowserHarness>,
    network_allowed: bool,
    exact_local_origin: Option<&str>,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let requested = input
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let request = match parse_request(&input, exact_local_origin) {
        Ok(request) => request,
        Err(failure) => return operation_outcome(&requested, failure, None),
    };
    if !network_allowed {
        return operation_outcome(
            &requested,
            BrowserFailure::operation(
                "browser_network_not_authorized",
                "authorization",
                "当前 Run permission 或 actor sandbox 禁止浏览器网络访问",
            ),
            None,
        );
    }
    harness.navigate(request, cancellation).await
}

fn parse_request(
    input: &Value,
    exact_local_origin: Option<&str>,
) -> Result<BrowserNavigateRequest, BrowserFailure> {
    let requested = required_str(input, "url").map_err(|error| {
        BrowserFailure::rejected("browser_url_missing", "url", error.to_string())
    })?;
    if requested.trim() != requested || requested.chars().count() > MAX_URL_CHARS {
        return Err(BrowserFailure::rejected(
            "browser_url_invalid",
            "url",
            format!("URL 必须无首尾空白且不超过 {MAX_URL_CHARS} 字符"),
        ));
    }
    let mut url = Url::parse(requested).map_err(|error| {
        BrowserFailure::rejected(
            "browser_url_invalid",
            "url",
            format!("URL 解析失败：{error}"),
        )
    })?;
    url.set_fragment(None);
    let origin = Origin::from_url(&url)?;
    let scope = if is_loopback_origin(&origin) {
        let address = origin.host.parse::<IpAddr>().map_err(|_| {
            BrowserFailure::rejected(
                "browser_local_origin_invalid",
                "url",
                "exact local browser URL must use a literal loopback IP",
            )
        })?;
        if !address.is_loopback() {
            return Err(BrowserFailure::rejected(
                "browser_local_origin_invalid",
                "url",
                "exact local browser URL is not loopback",
            ));
        }
        let allowed = exact_local_origin
            .map(parse_exact_local_origin)
            .transpose()?;
        if allowed.as_ref() != Some(&origin) {
            return Err(BrowserFailure::rejected(
                "browser_local_origin_denied",
                "url",
                "loopback browser navigation requires an exact Host-owned local origin grant",
            ));
        }
        BrowserTargetScope::ExactLocal(origin)
    } else {
        validate_public_origin(&origin)?;
        BrowserTargetScope::Public
    };
    let max_nodes = usize::try_from(optional_u64(input, "max_nodes", DEFAULT_MAX_NODES as u64))
        .unwrap_or(MAX_MAX_NODES)
        .clamp(1, MAX_MAX_NODES);
    let max_chars = usize::try_from(optional_u64(input, "max_chars", DEFAULT_MAX_CHARS as u64))
        .unwrap_or(MAX_MAX_CHARS)
        .clamp(1, MAX_MAX_CHARS);
    Ok(BrowserNavigateRequest {
        requested_url: requested.to_owned(),
        initial_url: url,
        scope,
        max_nodes,
        max_chars,
    })
}

fn parse_exact_local_origin(value: &str) -> Result<Origin, BrowserFailure> {
    let parsed = Url::parse(value).map_err(|error| {
        BrowserFailure::rejected(
            "browser_local_origin_invalid",
            "url",
            format!("Host exact-local-origin grant is invalid: {error}"),
        )
    })?;
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(BrowserFailure::rejected(
            "browser_local_origin_invalid",
            "url",
            "Host exact-local-origin grant must contain only scheme, literal loopback IP, and port",
        ));
    }
    let origin = Origin::from_url(&parsed)?;
    if !origin
        .host
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
    {
        return Err(BrowserFailure::rejected(
            "browser_local_origin_invalid",
            "url",
            "Host exact-local-origin grant must use a literal loopback IP",
        ));
    }
    Ok(origin)
}

fn is_loopback_origin(origin: &Origin) -> bool {
    origin.host == "localhost"
        || origin.host.ends_with(".localhost")
        || origin
            .host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn validate_public_origin(origin: &Origin) -> Result<(), BrowserFailure> {
    if (origin.scheme == "http" && origin.port != 80)
        || (origin.scheme == "https" && origin.port != 443)
    {
        return Err(BrowserFailure::rejected(
            "browser_port_denied",
            "url",
            "public browser navigation only allows default HTTP(S) ports 80/443",
        ));
    }
    if origin.host == "localhost"
        || origin.host.ends_with(".localhost")
        || matches!(
            origin.host.as_str(),
            "metadata.google.internal"
                | "instance-data.ec2.internal"
                | "metadata.azure.internal"
                | "metadata"
        )
    {
        return Err(BrowserFailure::rejected(
            "browser_metadata_target_denied",
            "url",
            "browser URL 主机属于本地或 metadata 命名空间",
        ));
    }
    let literal = origin
        .host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&origin.host);
    if let Ok(address) = literal.parse::<IpAddr>()
        && !is_public_ip(address)
    {
        return Err(BrowserFailure::rejected(
            "browser_private_address_denied",
            "url",
            "browser URL IP 不是 public unicast 地址",
        ));
    }
    Ok(())
}

fn rejected_outcome(requested: &str, failure: BrowserFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!(
            "browser_navigate 拒绝：code={}；{}",
            failure.code, failure.message
        ),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(failure_metadata(requested, &failure, None))
}

fn operation_outcome(
    requested: &str,
    failure: BrowserFailure,
    teardown: Option<&TeardownFacts>,
) -> ToolOutcome {
    let mut outcome = if failure.transport {
        let mut outcome = ToolOutcome::transport_failure(format!(
            "browser_navigate 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.retry = failure.retry;
        outcome
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "browser_navigate 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.transport = ToolTransportStatus::Succeeded;
        outcome.operation = ToolOperationStatus::Failed;
        outcome.retry = failure.retry;
        outcome
    };
    outcome.side_effect = ToolSideEffectStatus::NotApplicable;
    outcome.metadata = Some(failure_metadata(requested, &failure, teardown));
    outcome
}

fn failure_metadata(
    requested: &str,
    failure: &BrowserFailure,
    teardown: Option<&TeardownFacts>,
) -> Value {
    json!({
        "semantic_browser": {
            "requested_url": requested,
            "trust": TRUST,
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            },
            "teardown": teardown,
        }
    })
}

fn sha256_text(value: &str) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(value.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn sha256_bytes(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[derive(Debug, Clone, Serialize)]
struct TeardownFacts {
    attempted: bool,
    process_tree_settled: bool,
    proxy_settled: bool,
    profile_removed: bool,
}

#[derive(Debug, Default)]
struct BrowserMetrics {
    requests: AtomicU64,
    blocked_requests: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    decoded_body_bytes: AtomicU64,
}

#[derive(Debug)]
struct EgressState {
    scope: BrowserTargetScope,
    initial_origin: Origin,
    allowed_origins: Mutex<BTreeSet<Origin>>,
    last_document_url: Mutex<Option<String>>,
    redirect_count: AtomicU64,
    fatal_failure: Mutex<Option<BrowserFailure>>,
    metrics: BrowserMetrics,
}

impl EgressState {
    fn new(request: &BrowserNavigateRequest) -> Self {
        let initial_origin = Origin::from_url(&request.initial_url)
            .expect("validated browser request has an origin");
        Self {
            scope: request.scope.clone(),
            initial_origin: initial_origin.clone(),
            allowed_origins: Mutex::new(BTreeSet::from([initial_origin])),
            last_document_url: Mutex::new(None),
            redirect_count: AtomicU64::new(0),
            fatal_failure: Mutex::new(None),
            metrics: BrowserMetrics::default(),
        }
    }

    fn authorize_cdp_request(
        &self,
        url: &str,
        method: &str,
        resource_type: Option<&str>,
    ) -> Result<(), BrowserFailure> {
        if !matches!(method, "GET" | "HEAD") {
            return Err(BrowserFailure::operation(
                "browser_method_denied",
                "egress",
                format!("read-only browser blocked HTTP method {method}"),
            ));
        }
        let parsed = Url::parse(url).map_err(|error| {
            BrowserFailure::operation(
                "browser_request_url_invalid",
                "egress",
                format!("browser request URL invalid: {error}"),
            )
        })?;
        let origin = Origin::from_url(&parsed)?;
        match &self.scope {
            BrowserTargetScope::ExactLocal(expected) => {
                if &origin != expected {
                    return Err(BrowserFailure::operation(
                        "browser_local_origin_escape",
                        "egress",
                        format!(
                            "local browser session blocked origin escape to {}",
                            origin.canonical()
                        ),
                    ));
                }
            }
            BrowserTargetScope::Public => {
                validate_public_origin(&origin)?;
                if resource_type == Some("Document") && origin != self.initial_origin {
                    return Err(BrowserFailure::operation(
                        "browser_cross_origin_navigation_denied",
                        "redirect",
                        format!(
                            "top-level/frame document navigation escaped authorized origin to {}",
                            origin.canonical()
                        ),
                    ));
                }
            }
        }
        if resource_type == Some("Document") {
            let mut last = self
                .last_document_url
                .lock()
                .expect("browser document URL lock");
            if last
                .as_deref()
                .is_some_and(|prior| prior != parsed.as_str())
            {
                let redirects = self.redirect_count.fetch_add(1, Ordering::SeqCst) + 1;
                if redirects > MAX_REDIRECTS {
                    return Err(BrowserFailure::operation(
                        "browser_redirect_limit",
                        "redirect",
                        format!("browser redirect 超过 {MAX_REDIRECTS} 次上限"),
                    ));
                }
            }
            *last = Some(parsed.as_str().to_owned());
        }
        self.allowed_origins
            .lock()
            .expect("browser allowed origin lock")
            .insert(origin);
        self.metrics.requests.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn proxy_origin_allowed(&self, origin: &Origin) -> bool {
        self.allowed_origins
            .lock()
            .expect("browser allowed origin lock")
            .contains(origin)
    }

    fn record_denial(&self, failure: BrowserFailure, fatal: bool) {
        self.metrics.blocked_requests.fetch_add(1, Ordering::SeqCst);
        if fatal {
            let mut current = self
                .fatal_failure
                .lock()
                .expect("browser fatal failure lock");
            if current.is_none() {
                *current = Some(failure);
            }
        }
    }

    fn take_fatal_failure(&self) -> Option<BrowserFailure> {
        self.fatal_failure
            .lock()
            .expect("browser fatal failure lock")
            .take()
    }
}

struct EgressProxy {
    address: SocketAddr,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl EgressProxy {
    async fn start(
        state: Arc<EgressState>,
        resolver: Arc<dyn WebFetchNetwork>,
    ) -> Result<Self, BrowserFailure> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| {
                BrowserFailure::transport(
                    "browser_proxy_bind_failed",
                    "proxy",
                    format!("cannot bind Host browser egress proxy: {error}"),
                )
            })?;
        let address = listener.local_addr().map_err(|error| {
            BrowserFailure::transport(
                "browser_proxy_identity_failed",
                "proxy",
                format!("cannot read Host browser proxy identity: {error}"),
            )
        })?;
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            let state = Arc::clone(&state);
                            let resolver = Arc::clone(&resolver);
                            let cancellation = task_cancellation.clone();
                            connections.spawn(async move {
                                if let Err(failure) = handle_proxy_connection(
                                    stream,
                                    Arc::clone(&state),
                                    resolver,
                                    cancellation,
                                )
                                .await
                                {
                                    let fatal = matches!(
                                        failure.code,
                                        "browser_dns_target_denied"
                                            | "browser_local_origin_escape"
                                            | "browser_local_origin_invalid"
                                            | "browser_private_address_denied"
                                            | "browser_request_bytes_exceeded"
                                            | "browser_response_bytes_exceeded"
                                            | "browser_total_request_bytes_exceeded"
                                            | "browser_total_response_bytes_exceeded"
                                    );
                                    state.record_denial(failure, fatal);
                                }
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
            while connections.join_next().await.is_some() {}
        });
        Ok(Self {
            address,
            cancellation,
            task,
        })
    }

    async fn shutdown(self) -> bool {
        self.cancellation.cancel();
        let mut task = self.task;
        match tokio::time::timeout(PROXY_TEARDOWN_GRACE, &mut task).await {
            Ok(Ok(())) => true,
            _ => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

async fn handle_proxy_connection(
    mut client: TcpStream,
    state: Arc<EgressState>,
    resolver: Arc<dyn WebFetchNetwork>,
    cancellation: CancellationToken,
) -> Result<(), BrowserFailure> {
    let head = read_proxy_head(&mut client, &cancellation).await?;
    let text = std::str::from_utf8(&head).map_err(|_| {
        BrowserFailure::operation(
            "browser_proxy_request_invalid",
            "proxy",
            "browser proxy request head is not valid ASCII/UTF-8",
        )
    })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(BrowserFailure::operation(
            "browser_proxy_request_invalid",
            "proxy",
            "browser proxy request line is invalid",
        ));
    }

    if method == "CONNECT" {
        let origin = origin_from_authority("https", target)?;
        if !state.proxy_origin_allowed(&origin) {
            return Err(BrowserFailure::operation(
                "browser_proxy_origin_denied",
                "proxy",
                format!("proxy denied unapproved origin {}", origin.canonical()),
            ));
        }
        let mut upstream = connect_pinned(&origin, &state.scope, resolver.as_ref()).await?;
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n")
            .await
            .map_err(|error| {
                BrowserFailure::transport(
                    "browser_proxy_response_failed",
                    "proxy",
                    format!("cannot establish browser proxy tunnel: {error}"),
                )
            })?;
        relay_bounded(&mut client, &mut upstream, &state.metrics, &cancellation).await
    } else {
        if !matches!(method, "GET" | "HEAD") {
            return Err(BrowserFailure::operation(
                "browser_method_denied",
                "proxy",
                format!("browser proxy denied method {method}"),
            ));
        }
        let url = Url::parse(target).map_err(|error| {
            BrowserFailure::operation(
                "browser_proxy_request_invalid",
                "proxy",
                format!("browser proxy requires absolute HTTP URL: {error}"),
            )
        })?;
        let origin = Origin::from_url(&url)?;
        if !state.proxy_origin_allowed(&origin) {
            return Err(BrowserFailure::operation(
                "browser_proxy_origin_denied",
                "proxy",
                format!("proxy denied unapproved origin {}", origin.canonical()),
            ));
        }
        let mut upstream = connect_pinned(&origin, &state.scope, resolver.as_ref()).await?;
        let sanitized = sanitize_http_proxy_request(method, &url, lines)?;
        reserve_bytes(
            &state.metrics.bytes_sent,
            sanitized.len() as u64,
            MAX_TOTAL_NETWORK_REQUEST_BYTES,
            "browser_total_request_bytes_exceeded",
            "browser total request bytes",
        )?;
        upstream.write_all(&sanitized).await.map_err(|error| {
            BrowserFailure::transport(
                "browser_proxy_upstream_write_failed",
                "proxy",
                format!("cannot write sanitized browser request: {error}"),
            )
        })?;
        relay_bounded(&mut client, &mut upstream, &state.metrics, &cancellation).await
    }
}

async fn read_proxy_head(
    stream: &mut TcpStream,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, BrowserFailure> {
    let mut head = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 2048];
    loop {
        let read = tokio::select! {
            () = cancellation.cancelled() => {
                return Err(BrowserFailure::operation(
                    "browser_cancelled",
                    "proxy",
                    "browser proxy cancelled",
                ));
            }
            result = stream.read(&mut chunk) => result.map_err(|error| {
                BrowserFailure::transport(
                    "browser_proxy_request_failed",
                    "proxy",
                    format!("cannot read browser proxy request: {error}"),
                )
            })?
        };
        if read == 0 {
            return Err(BrowserFailure::transport(
                "browser_proxy_request_incomplete",
                "proxy",
                "browser proxy connection closed before request head",
            ));
        }
        head.extend_from_slice(&chunk[..read]);
        if head.len() > MAX_PROXY_REQUEST_BYTES {
            return Err(BrowserFailure::operation(
                "browser_proxy_request_too_large",
                "proxy",
                format!("browser proxy request exceeds {MAX_PROXY_REQUEST_BYTES} bytes"),
            ));
        }
        if let Some(index) = head.windows(4).position(|window| window == b"\r\n\r\n") {
            let end = index + 4;
            if end != head.len() {
                return Err(BrowserFailure::operation(
                    "browser_proxy_request_pipelined",
                    "proxy",
                    "browser proxy request contained bytes after its bounded request head",
                ));
            }
            return Ok(head);
        }
    }
}

fn origin_from_authority(scheme: &str, authority: &str) -> Result<Origin, BrowserFailure> {
    let url = Url::parse(&format!("{scheme}://{authority}/")).map_err(|error| {
        BrowserFailure::operation(
            "browser_proxy_authority_invalid",
            "proxy",
            format!("browser proxy authority invalid: {error}"),
        )
    })?;
    Origin::from_url(&url)
}

fn sanitize_http_proxy_request<'a>(
    method: &str,
    url: &Url,
    lines: impl Iterator<Item = &'a str>,
) -> Result<Vec<u8>, BrowserFailure> {
    let mut preserved = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(BrowserFailure::operation(
                "browser_proxy_header_invalid",
                "proxy",
                "browser proxy request contains an invalid header",
            ));
        };
        let lower = name.trim().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "accept"
                | "accept-language"
                | "accept-encoding"
                | "user-agent"
                | "sec-fetch-dest"
                | "sec-fetch-mode"
                | "sec-fetch-site"
                | "upgrade-insecure-requests"
        ) {
            let value = value.trim();
            if value.contains(['\r', '\n']) {
                return Err(BrowserFailure::operation(
                    "browser_proxy_header_invalid",
                    "proxy",
                    "browser proxy header contains a line break",
                ));
            }
            preserved.push((name.trim().to_owned(), value.to_owned()));
        }
    }
    let mut path = url.path().to_owned();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let origin = Origin::from_url(url)?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        origin
            .canonical()
            .trim_start_matches(&format!("{}://", origin.scheme))
    );
    for (name, value) in preserved {
        request.push_str(&name);
        request.push_str(": ");
        request.push_str(&value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    Ok(request.into_bytes())
}

async fn connect_pinned(
    origin: &Origin,
    scope: &BrowserTargetScope,
    resolver: &dyn WebFetchNetwork,
) -> Result<TcpStream, BrowserFailure> {
    let mut addresses = match scope {
        BrowserTargetScope::ExactLocal(expected) => {
            if origin != expected {
                return Err(BrowserFailure::operation(
                    "browser_local_origin_escape",
                    "connect",
                    "browser proxy local target differs from Host grant",
                ));
            }
            let address = origin.host.parse::<IpAddr>().map_err(|_| {
                BrowserFailure::operation(
                    "browser_local_origin_invalid",
                    "connect",
                    "exact local browser origin must use a literal loopback IP",
                )
            })?;
            if !address.is_loopback() {
                return Err(BrowserFailure::operation(
                    "browser_local_origin_invalid",
                    "connect",
                    "exact local browser origin is not loopback",
                ));
            }
            vec![SocketAddr::new(address, origin.port)]
        }
        BrowserTargetScope::Public => {
            validate_public_origin(origin)?;
            let addresses = resolver
                .resolve(&origin.host, origin.port)
                .await
                .map_err(|error| {
                    BrowserFailure::transport(
                        error.code,
                        "dns",
                        format!("browser DNS resolution failed: {}", error.message),
                    )
                })?;
            if addresses.is_empty() {
                return Err(BrowserFailure::transport(
                    "browser_dns_empty",
                    "dns",
                    "browser DNS returned no addresses",
                ));
            }
            if let Some(address) = addresses
                .iter()
                .find(|address| address.port() != origin.port || !is_public_ip(address.ip()))
            {
                return Err(BrowserFailure::operation(
                    "browser_dns_target_denied",
                    "dns",
                    format!("browser DNS/connect target {address} is not public unicast"),
                ));
            }
            addresses
        }
    };
    addresses.sort_unstable();
    addresses.dedup();
    let mut last_error = None;
    for address in addresses {
        match tokio::time::timeout(CONNECT_DEADLINE, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => last_error = Some("connect deadline exceeded".to_owned()),
        }
    }
    Err(BrowserFailure::transport(
        "browser_connect_failed",
        "connect",
        format!(
            "browser could not connect to a validated target: {}",
            last_error.unwrap_or_else(|| "no target".to_owned())
        ),
    ))
}

async fn relay_bounded(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    metrics: &BrowserMetrics,
    cancellation: &CancellationToken,
) -> Result<(), BrowserFailure> {
    let (mut client_read, mut client_write) = client.split();
    let (mut upstream_read, mut upstream_write) = upstream.split();
    let mut request_closed = false;
    let mut response_closed = false;
    let mut request_bytes = 0_u64;
    let mut response_bytes = 0_u64;
    let mut request_buffer = [0_u8; 8192];
    let mut response_buffer = [0_u8; 8192];
    while !request_closed || !response_closed {
        tokio::select! {
            () = cancellation.cancelled() => {
                return Err(BrowserFailure::operation(
                    "browser_cancelled",
                    "proxy",
                    "browser network relay cancelled",
                ));
            }
            read = client_read.read(&mut request_buffer), if !request_closed => {
                let read = read.map_err(|error| BrowserFailure::transport(
                    "browser_proxy_request_failed", "proxy", format!("browser request relay failed: {error}")))?;
                if read == 0 {
                    request_closed = true;
                    let _ = upstream_write.shutdown().await;
                } else {
                    request_bytes = request_bytes.saturating_add(read as u64);
                    if request_bytes > MAX_NETWORK_REQUEST_BYTES {
                        return Err(BrowserFailure::operation(
                            "browser_request_bytes_exceeded", "bounds",
                            format!("browser request bytes exceed {MAX_NETWORK_REQUEST_BYTES}")));
                    }
                    reserve_bytes(
                        &metrics.bytes_sent,
                        read as u64,
                        MAX_TOTAL_NETWORK_REQUEST_BYTES,
                        "browser_total_request_bytes_exceeded",
                        "browser total request bytes",
                    )?;
                    upstream_write.write_all(&request_buffer[..read]).await.map_err(|error| BrowserFailure::transport(
                        "browser_proxy_upstream_write_failed", "proxy", format!("browser request relay failed: {error}")))?;
                }
            }
            read = upstream_read.read(&mut response_buffer), if !response_closed => {
                let read = read.map_err(|error| BrowserFailure::transport(
                    "browser_proxy_response_failed", "proxy", format!("browser response relay failed: {error}")))?;
                if read == 0 {
                    response_closed = true;
                    let _ = client_write.shutdown().await;
                } else {
                    response_bytes = response_bytes.saturating_add(read as u64);
                    if response_bytes > MAX_NETWORK_RESPONSE_BYTES {
                        return Err(BrowserFailure::operation(
                            "browser_response_bytes_exceeded", "bounds",
                            format!("browser response bytes exceed {MAX_NETWORK_RESPONSE_BYTES}")));
                    }
                    reserve_bytes(
                        &metrics.bytes_received,
                        read as u64,
                        MAX_TOTAL_NETWORK_RESPONSE_BYTES,
                        "browser_total_response_bytes_exceeded",
                        "browser total response bytes",
                    )?;
                    client_write.write_all(&response_buffer[..read]).await.map_err(|error| BrowserFailure::transport(
                        "browser_proxy_client_write_failed", "proxy", format!("browser response relay failed: {error}")))?;
                }
            }
        }
    }
    Ok(())
}

fn reserve_bytes(
    counter: &AtomicU64,
    amount: u64,
    limit: u64,
    code: &'static str,
    label: &'static str,
) -> Result<(), BrowserFailure> {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            current.checked_add(amount).filter(|next| *next <= limit)
        })
        .map(|_| ())
        .map_err(|_| {
            BrowserFailure::operation(code, "bounds", format!("{label} exceed {limit} bytes"))
        })
}

struct CdpClient {
    socket: tokio_tungstenite::WebSocketStream<TcpStream>,
    next_id: u64,
    observed_events: BTreeSet<(String, String)>,
    egress: Arc<EgressState>,
    primary_target_id: Option<String>,
}

impl CdpClient {
    async fn call(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value, BrowserFailure> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session_id) = session_id {
            request["sessionId"] = json!(session_id);
        }
        self.socket
            .send(Message::text(request.to_string()))
            .await
            .map_err(|error| {
                BrowserFailure::transport(
                    "browser_cdp_send_failed",
                    "cdp",
                    format!("CDP send failed for {method}: {error}"),
                )
            })?;
        loop {
            let message = self.next_message().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(BrowserFailure::operation(
                        "browser_cdp_command_failed",
                        "cdp",
                        format!("CDP {method} failed: {error}"),
                    ));
                }
                return Ok(message.get("result").cloned().unwrap_or_else(|| json!({})));
            }
            self.handle_event(&message).await?;
        }
    }

    async fn wait_event(&mut self, method: &str, session_id: &str) -> Result<(), BrowserFailure> {
        let identity = (session_id.to_owned(), method.to_owned());
        if self.observed_events.remove(&identity) {
            return Ok(());
        }
        loop {
            let message = self.next_message().await?;
            if message.get("method").and_then(Value::as_str) == Some(method)
                && message.get("sessionId").and_then(Value::as_str) == Some(session_id)
            {
                return Ok(());
            }
            self.handle_event(&message).await?;
        }
    }

    async fn next_message(&mut self) -> Result<Value, BrowserFailure> {
        loop {
            let message = self.socket.next().await.ok_or_else(|| {
                BrowserFailure::transport(
                    "browser_cdp_closed",
                    "cdp",
                    "Chrome closed the CDP connection",
                )
            })?;
            match message {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(text.as_str()).map_err(|error| {
                        BrowserFailure::operation(
                            "browser_cdp_message_invalid",
                            "cdp",
                            format!("CDP returned invalid JSON: {error}"),
                        )
                    });
                }
                Ok(Message::Ping(payload)) => {
                    self.socket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| {
                            BrowserFailure::transport(
                                "browser_cdp_send_failed",
                                "cdp",
                                format!("CDP pong failed: {error}"),
                            )
                        })?;
                }
                Ok(Message::Close(_)) => {
                    return Err(BrowserFailure::transport(
                        "browser_cdp_closed",
                        "cdp",
                        "Chrome closed the CDP connection",
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    return Err(BrowserFailure::transport(
                        "browser_cdp_receive_failed",
                        "cdp",
                        format!("CDP receive failed: {error}"),
                    ));
                }
            }
        }
    }

    async fn handle_event(&mut self, event: &Value) -> Result<(), BrowserFailure> {
        let method = event.get("method").and_then(Value::as_str);
        let session_id = event.get("sessionId").and_then(Value::as_str);
        if method == Some("Target.attachedToTarget") {
            let target_id = event
                .get("params")
                .and_then(|params| params.get("targetInfo"))
                .and_then(|target| target.get("targetId"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    BrowserFailure::operation(
                        "browser_cdp_message_invalid",
                        "target",
                        "Target.attachedToTarget is missing targetId",
                    )
                })?;
            if self.primary_target_id.as_deref() != Some(target_id) {
                self.egress.record_denial(
                    BrowserFailure::operation(
                        "browser_additional_target_denied",
                        "target",
                        "read-only browser closed an additional page, worker, or popup target",
                    ),
                    false,
                );
                self.send_unobserved("Target.closeTarget", json!({"targetId":target_id}), None)
                    .await?;
            }
            return Ok(());
        }
        if method == Some("Network.dataReceived") {
            let amount = event
                .get("params")
                .and_then(|params| params.get("dataLength"))
                .and_then(nonnegative_u64)
                .ok_or_else(|| {
                    BrowserFailure::operation(
                        "browser_cdp_message_invalid",
                        "bounds",
                        "Network.dataReceived is missing a valid dataLength",
                    )
                })?;
            if let Err(failure) = reserve_bytes(
                &self.egress.metrics.decoded_body_bytes,
                amount,
                MAX_DECODED_BODY_BYTES,
                "browser_decoded_body_bytes_exceeded",
                "browser decoded body bytes",
            ) {
                self.egress.record_denial(failure.clone(), true);
                return Err(failure);
            }
        }
        if method == Some("Network.responseReceived")
            && event
                .get("params")
                .and_then(|params| params.get("type"))
                .and_then(Value::as_str)
                == Some("Document")
        {
            let media_type = event
                .get("params")
                .and_then(|params| params.get("response"))
                .and_then(|response| response.get("mimeType"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(media_type.as_str(), "text/html" | "application/xhtml+xml") {
                let failure = BrowserFailure::operation(
                    "browser_content_type_denied",
                    "response",
                    format!("semantic browser denied Document media type {media_type:?}"),
                );
                self.egress.record_denial(failure.clone(), true);
                return Err(failure);
            }
        }
        if method == Some("Fetch.requestPaused") {
            return self.handle_request_paused(event).await;
        }
        if method == Some("Page.javascriptDialogOpening") {
            self.egress.record_denial(
                BrowserFailure::operation(
                    "browser_dialog_denied",
                    "page",
                    "read-only browser dismissed a JavaScript dialog",
                ),
                false,
            );
            if let Some(session_id) = session_id {
                self.send_unobserved(
                    "Page.handleJavaScriptDialog",
                    json!({"accept":false}),
                    Some(session_id),
                )
                .await?;
            }
            return Ok(());
        }
        if let (Some(method), Some(session_id)) = (method, session_id)
            && matches!(method, "Page.loadEventFired" | "Page.frameStoppedLoading")
        {
            self.observed_events
                .insert((session_id.to_owned(), method.to_owned()));
        }
        Ok(())
    }

    async fn handle_request_paused(&mut self, event: &Value) -> Result<(), BrowserFailure> {
        let params = event
            .get("params")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                BrowserFailure::operation(
                    "browser_cdp_message_invalid",
                    "cdp",
                    "Fetch.requestPaused is missing params",
                )
            })?;
        let request_id = params
            .get("requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BrowserFailure::operation(
                    "browser_cdp_message_invalid",
                    "cdp",
                    "Fetch.requestPaused is missing requestId",
                )
            })?;
        let request = params
            .get("request")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                BrowserFailure::operation(
                    "browser_cdp_message_invalid",
                    "cdp",
                    "Fetch.requestPaused is missing request",
                )
            })?;
        let url = request
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let resource_type = params.get("resourceType").and_then(Value::as_str);
        let fatal = resource_type == Some("Document");
        let session_id = event.get("sessionId").and_then(Value::as_str);
        match self
            .egress
            .authorize_cdp_request(url, method, resource_type)
        {
            Ok(()) => {
                let headers = sanitized_cdp_headers(request.get("headers"));
                self.send_unobserved(
                    "Fetch.continueRequest",
                    json!({"requestId":request_id,"headers":headers}),
                    session_id,
                )
                .await
            }
            Err(failure) => {
                self.egress.record_denial(failure, fatal);
                self.send_unobserved(
                    "Fetch.failRequest",
                    json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
                    session_id,
                )
                .await
            }
        }
    }

    async fn send_unobserved(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<(), BrowserFailure> {
        self.next_id += 1;
        let mut request = json!({"id":self.next_id,"method":method,"params":params});
        if let Some(session_id) = session_id {
            request["sessionId"] = json!(session_id);
        }
        self.socket
            .send(Message::text(request.to_string()))
            .await
            .map_err(|error| {
                BrowserFailure::transport(
                    "browser_cdp_send_failed",
                    "cdp",
                    format!("CDP send failed for {method}: {error}"),
                )
            })
    }
}

fn nonnegative_u64(value: &Value) -> Option<u64> {
    if let Some(value) = value.as_u64() {
        return Some(value);
    }
    let value = value.as_f64()?;
    (value.is_finite() && value >= 0.0 && value <= u64::MAX as f64).then(|| value.ceil() as u64)
}

fn sanitized_cdp_headers(headers: Option<&Value>) -> Vec<Value> {
    let mut values = headers
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| {
            let lower = name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "cookie" | "authorization" | "proxy-authorization" | "referer"
            ) {
                return None;
            }
            value
                .as_str()
                .map(|value| json!({"name":name,"value":value}))
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    values
}

#[derive(Debug)]
struct SessionObservation {
    final_url: String,
    title: String,
    nodes: Vec<SemanticNode>,
    nodes_read: usize,
    truncated: bool,
    browser_product: String,
    protocol_version: String,
}

#[async_trait]
impl SemanticBrowserHarness for SystemSemanticBrowserHarness {
    fn identity(&self) -> String {
        let binary = self.binary.as_ref().map_or_else(
            || format!("{ADAPTER_ID}:unavailable"),
            PinnedChromeForTesting::identity,
        );
        format!("{binary}:resolver={}", self.resolver.identity())
    }

    async fn navigate(
        &self,
        request: BrowserNavigateRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        let Some(binary) = self.binary.clone() else {
            return operation_outcome(
                &request.requested_url,
                BrowserFailure::operation(
                    "browser_unavailable",
                    "preflight",
                    format!(
                        "pinned Chrome for Testing {PINNED_CFT_VERSION} is not installed for this platform"
                    ),
                ),
                None,
            );
        };
        if let Err(failure) = verify_pinned_binary(&binary).await {
            return operation_outcome(&request.requested_url, failure, None);
        }
        execute_system_browser(request, binary, Arc::clone(&self.resolver), cancellation).await
    }
}

async fn verify_pinned_binary(binary: &PinnedChromeForTesting) -> Result<(), BrowserFailure> {
    let metadata = tokio::fs::symlink_metadata(&binary.executable)
        .await
        .map_err(|error| {
            BrowserFailure::operation(
                "browser_unavailable",
                "preflight",
                format!("pinned Chrome for Testing is unavailable: {error}"),
            )
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BrowserFailure::operation(
            "browser_binary_identity_mismatch",
            "preflight",
            "Chrome for Testing executable must be an ordinary non-symlink file",
        ));
    }
    let bytes = tokio::fs::read(&binary.executable).await.map_err(|error| {
        BrowserFailure::operation(
            "browser_unavailable",
            "preflight",
            format!("cannot read pinned Chrome for Testing executable: {error}"),
        )
    })?;
    let actual = sha256_bytes(&bytes);
    if actual != binary.sha256 {
        return Err(BrowserFailure::operation(
            "browser_binary_identity_mismatch",
            "preflight",
            format!(
                "Chrome for Testing executable SHA-256 mismatch; expected {}, observed {actual}",
                binary.sha256
            ),
        ));
    }
    Ok(())
}

async fn execute_system_browser(
    request: BrowserNavigateRequest,
    binary: PinnedChromeForTesting,
    resolver: Arc<dyn WebFetchNetwork>,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let egress = Arc::new(EgressState::new(&request));
    let proxy = match EgressProxy::start(Arc::clone(&egress), resolver).await {
        Ok(proxy) => proxy,
        Err(failure) => return operation_outcome(&request.requested_url, failure, None),
    };
    let profile = match tempfile::Builder::new().prefix("dse-browser-").tempdir() {
        Ok(profile) => profile,
        Err(error) => {
            let proxy_settled = proxy.shutdown().await;
            let teardown = TeardownFacts {
                attempted: true,
                process_tree_settled: true,
                proxy_settled,
                profile_removed: true,
            };
            return operation_outcome(
                &request.requested_url,
                BrowserFailure::operation(
                    "browser_profile_failed",
                    "profile",
                    format!("cannot create ephemeral browser profile: {error}"),
                ),
                Some(&teardown),
            );
        }
    };
    let mut command = browser_command(&binary.executable, profile.path(), proxy.address);
    configure_process_tree(command.as_std_mut());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let proxy_settled = proxy.shutdown().await;
            let profile_removed = profile.close().is_ok();
            let teardown = TeardownFacts {
                attempted: true,
                process_tree_settled: true,
                proxy_settled,
                profile_removed,
            };
            return operation_outcome(
                &request.requested_url,
                BrowserFailure::transport(
                    "browser_spawn_failed",
                    "process",
                    format!("cannot start pinned Chrome for Testing: {error}"),
                ),
                Some(&teardown),
            );
        }
    };
    let mut owner = match ProcessTreeOwner::attach_tokio(&child, "semantic_browser") {
        Ok(owner) => owner,
        Err(error) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let proxy_settled = proxy.shutdown().await;
            let profile_removed = profile.close().is_ok();
            let teardown = TeardownFacts {
                attempted: true,
                process_tree_settled: false,
                proxy_settled,
                profile_removed,
            };
            return operation_outcome(
                &request.requested_url,
                BrowserFailure::operation(
                    "browser_process_ownership_failed",
                    "process",
                    format!("cannot own Chrome process tree: {error}"),
                ),
                Some(&teardown),
            );
        }
    };

    let execution = {
        let session = run_cdp_session(&request, profile.path(), Arc::clone(&egress));
        tokio::pin!(session);
        tokio::select! {
            () = cancellation.cancelled() => Err(BrowserFailure {
                code: "browser_cancelled",
                stage: "cancellation",
                message: "browser_navigate was cancelled".to_owned(),
                transport: false,
                retry: ToolRetryDisposition::Safe,
            }),
            result = tokio::time::timeout(OVERALL_DEADLINE, &mut session) => match result {
                Ok(result) => result,
                Err(_) => Err(BrowserFailure::transport(
                    "browser_deadline_exceeded",
                    "deadline",
                    format!("browser_navigate exceeded {} ms", OVERALL_DEADLINE.as_millis()),
                )),
            }
        }
    };
    let process_tree_settled =
        shutdown_tokio_process_tree(&mut child, &mut owner, PROCESS_TEARDOWN_GRACE).await;
    let proxy_settled = proxy.shutdown().await;
    let profile_removed = profile.close().is_ok();
    let teardown = TeardownFacts {
        attempted: true,
        process_tree_settled,
        proxy_settled,
        profile_removed,
    };
    if !process_tree_settled || !proxy_settled || !profile_removed {
        return operation_outcome(
            &request.requested_url,
            BrowserFailure::operation(
                "browser_teardown_incomplete",
                "teardown",
                "browser process, proxy, or ephemeral profile did not settle",
            ),
            Some(&teardown),
        );
    }
    let observation = match execution {
        Ok(observation) => observation,
        Err(failure) => {
            let mut outcome =
                operation_outcome(&request.requested_url, failure.clone(), Some(&teardown));
            if failure.code == "browser_cancelled" {
                outcome.operation = ToolOperationStatus::Cancelled;
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
            }
            return outcome;
        }
    };
    if let Some(failure) = egress.take_fatal_failure() {
        return operation_outcome(&request.requested_url, failure, Some(&teardown));
    }
    let snapshot_bytes = serde_json::to_vec(&observation.nodes)
        .expect("bounded semantic browser snapshot serializes");
    let result = BrowserResult {
        requested_url: request.requested_url,
        final_url: observation.final_url,
        title: observation.title,
        snapshot_sha256: format!("sha256:{}", sha256_bytes(&snapshot_bytes)),
        snapshot_sha256_scope: "bounded_semantic_observation_replay_identity",
        nodes_read: observation.nodes_read,
        nodes_returned: observation.nodes.len(),
        bytes_returned: snapshot_bytes.len(),
        truncated: observation.truncated,
        snapshot: observation.nodes,
        redirect_count: egress.redirect_count.load(Ordering::SeqCst),
        network_requests: egress.metrics.requests.load(Ordering::SeqCst),
        network_bytes_sent: egress.metrics.bytes_sent.load(Ordering::SeqCst),
        network_bytes_received: egress.metrics.bytes_received.load(Ordering::SeqCst),
        decoded_body_bytes: egress.metrics.decoded_body_bytes.load(Ordering::SeqCst),
        blocked_requests: egress.metrics.blocked_requests.load(Ordering::SeqCst),
        browser_product: observation.browser_product,
        cdp_protocol_version: observation.protocol_version,
        chrome_for_testing_version: binary.version,
        chrome_executable_sha256: format!("sha256:{}", binary.sha256),
        retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        trust: TRUST,
        profile_ephemeral: true,
        process_tree_settled: true,
        teardown,
    };
    ToolOutcome::json(&result)
        .expect("bounded browser result serializes")
        .with_side_effect(ToolSideEffectStatus::NotApplicable)
}

fn browser_command(executable: &Path, profile: &Path, proxy: SocketAddr) -> Command {
    let mut command = Command::new(executable);
    command
        .args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--disable-background-networking",
            "--disable-breakpad",
            "--disable-client-side-phishing-detection",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-extensions",
            "--disable-features=OptimizationHints,MediaRouter,Translate,ServiceWorker",
            "--disable-plugins",
            "--disable-quic",
            "--disable-sync",
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
            "--incognito",
            "--metrics-recording-only",
            "--mute-audio",
            "--no-default-browser-check",
            "--no-first-run",
            "--password-store=basic",
            "--proxy-bypass-list=<-loopback>",
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1",
            "--use-mock-keychain",
            "about:blank",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--proxy-server=http://{proxy}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn run_cdp_session(
    request: &BrowserNavigateRequest,
    profile: &Path,
    egress: Arc<EgressState>,
) -> Result<SessionObservation, BrowserFailure> {
    let (port, websocket_path) = wait_for_devtools_active_port(profile).await?;
    let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|error| {
            BrowserFailure::transport(
                "browser_cdp_connect_failed",
                "cdp",
                format!("cannot connect to local Chrome CDP endpoint: {error}"),
            )
        })?;
    let websocket_url = format!("ws://127.0.0.1:{port}{websocket_path}");
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_CDP_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_CDP_MESSAGE_BYTES));
    let (socket, _) = tokio_tungstenite::client_async_with_config(websocket_url, tcp, Some(config))
        .await
        .map_err(|error| {
            BrowserFailure::transport(
                "browser_cdp_handshake_failed",
                "cdp",
                format!("local Chrome CDP WebSocket handshake failed: {error}"),
            )
        })?;
    let mut cdp = CdpClient {
        socket,
        next_id: 0,
        observed_events: BTreeSet::new(),
        egress,
        primary_target_id: None,
    };
    let version = cdp.call("Browser.getVersion", json!({}), None).await?;
    let product = version
        .get("product")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if product != format!("Chrome/{PINNED_CFT_VERSION}") {
        return Err(BrowserFailure::operation(
            "browser_binary_identity_mismatch",
            "preflight",
            format!(
                "Chrome product mismatch; expected Chrome/{PINNED_CFT_VERSION}, observed {product}"
            ),
        ));
    }
    let target = cdp
        .call("Target.createTarget", json!({"url":"about:blank"}), None)
        .await?;
    let target_id = target
        .get("targetId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "cdp",
                "Target.createTarget did not return targetId",
            )
        })?
        .to_owned();
    cdp.primary_target_id = Some(target_id.clone());
    let attached = cdp
        .call(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
            None,
        )
        .await?;
    let session_id = attached
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "cdp",
                "Target.attachToTarget did not return sessionId",
            )
        })?
        .to_owned();
    cdp.call("Page.enable", json!({}), Some(&session_id))
        .await?;
    cdp.call("Network.enable", json!({}), Some(&session_id))
        .await?;
    cdp.call(
        "Network.setCacheDisabled",
        json!({"cacheDisabled":true}),
        Some(&session_id),
    )
    .await?;
    cdp.call(
        "Network.setBypassServiceWorker",
        json!({"bypass":true}),
        Some(&session_id),
    )
    .await?;
    cdp.call("Network.clearBrowserCookies", json!({}), Some(&session_id))
        .await?;
    cdp.call("Accessibility.enable", json!({}), Some(&session_id))
        .await?;
    cdp.call(
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
        Some(&session_id),
    )
    .await?;
    cdp.call(
        "Browser.setDownloadBehavior",
        json!({"behavior":"deny"}),
        None,
    )
    .await?;
    cdp.call(
        "Target.setAutoAttach",
        json!({
            "autoAttach":true,
            "waitForDebuggerOnStart":true,
            "flatten":true,
        }),
        None,
    )
    .await?;
    let navigation = cdp
        .call(
            "Page.navigate",
            json!({"url":request.initial_url.as_str()}),
            Some(&session_id),
        )
        .await?;
    if let Some(error) = navigation.get("errorText").and_then(Value::as_str) {
        if let Some(failure) = cdp.egress.take_fatal_failure() {
            return Err(failure);
        }
        return Err(BrowserFailure::transport(
            "browser_navigation_failed",
            "navigation",
            format!("Chrome navigation failed: {error}"),
        ));
    }
    tokio::time::timeout(
        PAGE_LOAD_DEADLINE,
        cdp.wait_event("Page.loadEventFired", &session_id),
    )
    .await
    .map_err(|_| {
        BrowserFailure::transport(
            "browser_page_load_timeout",
            "navigation",
            format!("page load exceeded {} ms", PAGE_LOAD_DEADLINE.as_millis()),
        )
    })??;
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    if let Some(failure) = cdp.egress.take_fatal_failure() {
        return Err(failure);
    }
    let before = cdp
        .call("Page.getNavigationHistory", json!({}), Some(&session_id))
        .await?;
    let ax = cdp
        .call(
            "Accessibility.getFullAXTree",
            json!({"depth":16}),
            Some(&session_id),
        )
        .await?;
    let dom = cdp
        .call(
            "DOMSnapshot.captureSnapshot",
            json!({
                "computedStyles":[],
                "includeDOMRects":false,
                "includePaintOrder":false,
            }),
            Some(&session_id),
        )
        .await?;
    let after = cdp
        .call("Page.getNavigationHistory", json!({}), Some(&session_id))
        .await?;
    let (before_id, before_url) = current_history_identity(&before)?;
    let (after_id, after_url) = current_history_identity(&after)?;
    if before_id != after_id || before_url != after_url {
        return Err(BrowserFailure::operation(
            "browser_stale_document",
            "snapshot",
            "document navigation changed while the semantic snapshot was captured",
        ));
    }
    let initial_origin = Origin::from_url(&request.initial_url)?;
    let final_url = Url::parse(&after_url).map_err(|error| {
        BrowserFailure::operation(
            "browser_final_url_invalid",
            "snapshot",
            format!("Chrome returned invalid final URL: {error}"),
        )
    })?;
    if Origin::from_url(&final_url)? != initial_origin {
        return Err(BrowserFailure::operation(
            "browser_cross_origin_navigation_denied",
            "redirect",
            "final document origin differs from authorized navigate origin",
        ));
    }
    let (title, nodes, nodes_read, truncated) =
        extract_semantic_snapshot(&ax, &dom, request.max_nodes, request.max_chars)?;
    let _ = cdp.call("Browser.close", json!({}), None).await;
    Ok(SessionObservation {
        final_url: after_url,
        title,
        nodes,
        nodes_read,
        truncated,
        browser_product: product.to_owned(),
        protocol_version: version
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

async fn wait_for_devtools_active_port(profile: &Path) -> Result<(u16, String), BrowserFailure> {
    let path = profile.join("DevToolsActivePort");
    tokio::time::timeout(PROFILE_DISCOVERY_DEADLINE, async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                let mut lines = text.lines();
                let port = lines
                    .next()
                    .ok_or_else(|| {
                        BrowserFailure::operation(
                            "browser_cdp_identity_invalid",
                            "process",
                            "DevToolsActivePort is missing its port",
                        )
                    })?
                    .parse::<u16>()
                    .map_err(|error| {
                        BrowserFailure::operation(
                            "browser_cdp_identity_invalid",
                            "process",
                            format!("DevToolsActivePort port is invalid: {error}"),
                        )
                    })?;
                let websocket = lines.next().ok_or_else(|| {
                    BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        "DevToolsActivePort is missing its WebSocket path",
                    )
                })?;
                if !websocket.starts_with("/devtools/browser/") || websocket.contains(['\r', '\n'])
                {
                    return Err(BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        "DevToolsActivePort contains an invalid WebSocket path",
                    ));
                }
                return Ok((port, websocket.to_owned()));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| {
        BrowserFailure::transport(
            "browser_launch_timeout",
            "process",
            format!(
                "Chrome did not publish CDP identity within {} ms",
                PROFILE_DISCOVERY_DEADLINE.as_millis()
            ),
        )
    })?
}

fn current_history_identity(history: &Value) -> Result<(i64, String), BrowserFailure> {
    let current_index = history
        .get("currentIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page.getNavigationHistory is missing currentIndex",
            )
        })?;
    let entry = history
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| {
            usize::try_from(current_index)
                .ok()
                .and_then(|index| entries.get(index))
        })
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page.getNavigationHistory current entry is missing",
            )
        })?;
    let id = entry.get("id").and_then(Value::as_i64).ok_or_else(|| {
        BrowserFailure::operation(
            "browser_cdp_message_invalid",
            "snapshot",
            "Page navigation entry is missing id",
        )
    })?;
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page navigation entry is missing URL",
            )
        })?
        .to_owned();
    Ok((id, url))
}

#[derive(Debug, Default)]
struct DomFacts {
    node_name: String,
    node_value: String,
    attributes: BTreeMap<String, String>,
}

fn extract_semantic_snapshot(
    ax: &Value,
    dom: &Value,
    max_nodes: usize,
    max_chars: usize,
) -> Result<(String, Vec<SemanticNode>, usize, bool), BrowserFailure> {
    let strings = dom
        .get("strings")
        .and_then(Value::as_array)
        .ok_or_else(|| snapshot_invalid("DOMSnapshot is missing its string table"))?;
    let document = dom
        .get("documents")
        .and_then(Value::as_array)
        .and_then(|documents| documents.first())
        .ok_or_else(|| snapshot_invalid("DOMSnapshot is missing its root document"))?;
    let title =
        string_at(strings, document.get("title").and_then(Value::as_i64)).unwrap_or_default();
    let dom_nodes = document
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| snapshot_invalid("DOMSnapshot document is missing nodes"))?;
    let backend_ids = required_array(dom_nodes, "backendNodeId")?;
    let node_names = required_array(dom_nodes, "nodeName")?;
    let node_values = required_array(dom_nodes, "nodeValue")?;
    let attributes = required_array(dom_nodes, "attributes")?;
    if backend_ids.len() != node_names.len()
        || backend_ids.len() != node_values.len()
        || backend_ids.len() != attributes.len()
    {
        return Err(snapshot_invalid(
            "DOMSnapshot parallel node arrays have different lengths",
        ));
    }
    let mut by_backend_id = HashMap::new();
    for index in 0..backend_ids.len() {
        let Some(backend_id) = backend_ids[index].as_u64() else {
            continue;
        };
        let node_name = string_at(strings, node_names[index].as_i64()).unwrap_or_default();
        let node_value = string_at(strings, node_values[index].as_i64()).unwrap_or_default();
        let attribute_indexes = attributes[index]
            .as_array()
            .ok_or_else(|| snapshot_invalid("DOMSnapshot node attributes are not an array"))?;
        if attribute_indexes.len() % 2 != 0 {
            return Err(snapshot_invalid(
                "DOMSnapshot node attributes are not name/value pairs",
            ));
        }
        let mut parsed_attributes = BTreeMap::new();
        for pair in attribute_indexes.chunks_exact(2) {
            let name = string_at(strings, pair[0].as_i64()).unwrap_or_default();
            let value = string_at(strings, pair[1].as_i64()).unwrap_or_default();
            if matches!(
                name.as_str(),
                "data-state"
                    | "aria-checked"
                    | "aria-selected"
                    | "aria-expanded"
                    | "aria-disabled"
                    | "aria-busy"
                    | "aria-pressed"
            ) {
                parsed_attributes.insert(name, bounded_text(&value, 512));
            }
        }
        by_backend_id.insert(
            backend_id,
            DomFacts {
                node_name,
                node_value,
                attributes: parsed_attributes,
            },
        );
    }

    let ax_nodes = ax
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| snapshot_invalid("Accessibility tree is missing nodes"))?;
    let ax_by_id = ax_nodes
        .iter()
        .filter_map(|node| {
            node.get("nodeId")
                .and_then(Value::as_str)
                .map(|id| (id, node))
        })
        .collect::<HashMap<_, _>>();
    let mut eligible = Vec::new();
    for node in ax_nodes {
        if node.get("ignored").and_then(Value::as_bool) != Some(false) {
            continue;
        }
        let role = ax_value(node.get("role"));
        let mut accessible_name = ax_value(node.get("name"));
        if accessible_name.is_empty() {
            accessible_name = descendant_ax_text(node, &ax_by_id, 32, 1_024);
        }
        let value = ax_value(node.get("value"));
        let dom = node
            .get("backendDOMNodeId")
            .and_then(Value::as_u64)
            .and_then(|id| by_backend_id.get(&id));
        let mut state = BTreeMap::new();
        if let Some(properties) = node.get("properties").and_then(Value::as_array) {
            for property in properties {
                let name = property
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(
                    name,
                    "busy"
                        | "checked"
                        | "disabled"
                        | "expanded"
                        | "pressed"
                        | "readonly"
                        | "required"
                        | "selected"
                ) {
                    let value = ax_value(property.get("value"));
                    if !value.is_empty() {
                        state.insert(name.to_owned(), bounded_text(&value, 512));
                    }
                }
            }
        }
        if let Some(dom) = dom.as_ref() {
            for (name, value) in &dom.attributes {
                state.insert(name.clone(), value.clone());
            }
        }
        let mut text = if matches!(role.as_str(), "StaticText" | "InlineTextBox") {
            accessible_name.clone()
        } else if let Some(dom) = dom.as_ref()
            && !matches!(dom.node_name.as_str(), "SCRIPT" | "STYLE")
        {
            bounded_text(&dom.node_value, 1_024)
        } else {
            String::new()
        };
        if text.is_empty() && !accessible_name.is_empty() && role != "RootWebArea" {
            text = accessible_name.clone();
        }
        if role.is_empty()
            && accessible_name.is_empty()
            && text.is_empty()
            && value.is_empty()
            && state.is_empty()
        {
            continue;
        }
        eligible.push(SemanticNode {
            role: bounded_text(&role, 512),
            accessible_name: bounded_text(&accessible_name, 1_024),
            text,
            value: bounded_text(&value, 1_024),
            state,
        });
    }
    let nodes_read = eligible.len();
    let mut returned = Vec::new();
    let mut truncated = nodes_read > max_nodes;
    for node in eligible.into_iter().take(max_nodes) {
        let mut candidate = returned.clone();
        candidate.push(node.clone());
        let chars = serde_json::to_string(&candidate)
            .expect("semantic nodes serialize")
            .chars()
            .count();
        if chars > max_chars {
            truncated = true;
            break;
        }
        returned.push(node);
    }
    Ok((bounded_text(&title, 512), returned, nodes_read, truncated))
}

fn descendant_ax_text(
    node: &Value,
    by_id: &HashMap<&str, &Value>,
    max_nodes: usize,
    max_chars: usize,
) -> String {
    let mut pending = node
        .get("childIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .rev()
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut parts = Vec::new();
    while let Some(id) = pending.pop() {
        if visited.len() >= max_nodes || !visited.insert(id) {
            continue;
        }
        let Some(child) = by_id.get(id) else {
            continue;
        };
        let name = ax_value(child.get("name"));
        if !name.is_empty() && parts.last() != Some(&name) {
            parts.push(name);
        }
        if let Some(children) = child.get("childIds").and_then(Value::as_array) {
            pending.extend(children.iter().filter_map(Value::as_str).rev());
        }
    }
    bounded_text(&parts.join(" "), max_chars)
}

fn required_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Vec<Value>, BrowserFailure> {
    object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| snapshot_invalid(format!("DOMSnapshot nodes missing {field}")))
}

fn string_at(strings: &[Value], index: Option<i64>) -> Option<String> {
    let index = usize::try_from(index?).ok()?;
    strings.get(index)?.as_str().map(ToOwned::to_owned)
}

fn ax_value(value: Option<&Value>) -> String {
    value
        .and_then(|value| value.get("value"))
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Bool(value) => Some(value.to_string()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn snapshot_invalid(message: impl Into<String>) -> BrowserFailure {
    BrowserFailure::operation("browser_snapshot_invalid", "snapshot", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WebFetchHttpResponse, WebFetchNetworkError};

    #[derive(Debug)]
    struct NoNetwork;

    #[async_trait]
    impl WebFetchNetwork for NoNetwork {
        fn identity(&self) -> &str {
            "no_network_browser_fixture_v1"
        }

        async fn resolve(
            &self,
            _host: &str,
            _port: u16,
        ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
            panic!("fixture must not resolve")
        }

        async fn get(
            &self,
            _url: &Url,
            _pinned_addresses: &[SocketAddr],
            _timeout: Duration,
        ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
            panic!("browser never calls the web_fetch GET seam")
        }
    }

    fn input(url: &str) -> Value {
        json!({"url":url,"max_nodes":64,"max_chars":4096})
    }

    #[test]
    fn url_and_local_origin_preflight_fail_closed() {
        for (url, expected) in [
            ("file:///etc/passwd", "browser_scheme_denied"),
            ("http://127.0.0.1/", "browser_local_origin_denied"),
            ("http://169.254.169.254/", "browser_private_address_denied"),
            (
                "https://metadata.google.internal/",
                "browser_metadata_target_denied",
            ),
            ("https://user@example.com/", "browser_userinfo_denied"),
            ("http://example.com:8080/", "browser_port_denied"),
            ("https://example.com:8443/", "browser_port_denied"),
        ] {
            let failure = parse_request(&input(url), None).expect_err(url);
            assert_eq!(failure.code, expected, "{url}");
        }
        let local = parse_request(
            &input("http://127.0.0.1:32123/app"),
            Some("http://127.0.0.1:32123"),
        )
        .expect("exact Host local origin");
        assert!(matches!(local.scope, BrowserTargetScope::ExactLocal(_)));
        assert_eq!(local.initial_url.path(), "/app");
        for (url, grant) in [
            ("http://localhost:32123/app", "http://localhost:32123"),
            (
                "http://127.0.0.1:32123/app",
                "http://127.0.0.1:32123/not-an-origin",
            ),
        ] {
            assert_eq!(
                parse_request(&input(url), Some(grant)).unwrap_err().code,
                "browser_local_origin_invalid"
            );
        }
    }

    #[test]
    fn cdp_egress_blocks_methods_redirect_escape_and_local_escape() {
        let public = parse_request(&input("https://example.com/app"), None).unwrap();
        let public = EgressState::new(&public);
        public
            .authorize_cdp_request("https://example.com/app", "GET", Some("Document"))
            .unwrap();
        public
            .authorize_cdp_request("https://cdn.example.net/app.js", "GET", Some("Script"))
            .unwrap();
        assert_eq!(
            public
                .authorize_cdp_request("https://other.example/", "GET", Some("Document"))
                .unwrap_err()
                .code,
            "browser_cross_origin_navigation_denied"
        );
        assert_eq!(
            public
                .authorize_cdp_request("https://example.com/form", "POST", Some("Fetch"))
                .unwrap_err()
                .code,
            "browser_method_denied"
        );
        for index in 0..=MAX_REDIRECTS {
            let result = public.authorize_cdp_request(
                &format!("https://example.com/redirect-{index}"),
                "GET",
                Some("Document"),
            );
            if index == MAX_REDIRECTS {
                assert_eq!(result.unwrap_err().code, "browser_redirect_limit");
            } else {
                result.unwrap();
            }
        }

        let local = parse_request(
            &input("http://127.0.0.1:32123/"),
            Some("http://127.0.0.1:32123"),
        )
        .unwrap();
        let local = EgressState::new(&local);
        assert_eq!(
            local
                .authorize_cdp_request("http://127.0.0.1:32124/", "GET", Some("Script"))
                .unwrap_err()
                .code,
            "browser_local_origin_escape"
        );
    }

    #[tokio::test]
    async fn proxy_rejects_private_dns_before_connect() {
        #[derive(Debug)]
        struct PrivateDns;
        #[async_trait]
        impl WebFetchNetwork for PrivateDns {
            fn identity(&self) -> &str {
                "private_dns_browser_fixture_v1"
            }

            async fn resolve(
                &self,
                _host: &str,
                port: u16,
            ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
                Ok(vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)])
            }

            async fn get(
                &self,
                _url: &Url,
                _pinned_addresses: &[SocketAddr],
                _timeout: Duration,
            ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
                unreachable!()
            }
        }
        let origin = Origin {
            scheme: "https".to_owned(),
            host: "example.com".to_owned(),
            port: 443,
        };
        let error = connect_pinned(&origin, &BrowserTargetScope::Public, &PrivateDns)
            .await
            .unwrap_err();
        assert_eq!(error.code, "browser_dns_target_denied");
    }

    #[tokio::test]
    async fn pinned_binary_mismatch_is_typed_before_spawn() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("chrome");
        std::fs::write(&executable, b"not the pinned browser").unwrap();
        let error = verify_pinned_binary(&PinnedChromeForTesting {
            executable,
            version: PINNED_CFT_VERSION,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, "browser_binary_identity_mismatch");
        assert_eq!(error.stage, "preflight");
    }

    #[test]
    fn total_network_bounds_are_atomic_and_fail_closed() {
        let counter = AtomicU64::new(MAX_TOTAL_NETWORK_RESPONSE_BYTES - 3);
        reserve_bytes(
            &counter,
            3,
            MAX_TOTAL_NETWORK_RESPONSE_BYTES,
            "browser_total_response_bytes_exceeded",
            "response",
        )
        .unwrap();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            MAX_TOTAL_NETWORK_RESPONSE_BYTES
        );
        let error = reserve_bytes(
            &counter,
            1,
            MAX_TOTAL_NETWORK_RESPONSE_BYTES,
            "browser_total_response_bytes_exceeded",
            "response",
        )
        .unwrap_err();
        assert_eq!(error.code, "browser_total_response_bytes_exceeded");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            MAX_TOTAL_NETWORK_RESPONSE_BYTES
        );
    }

    #[test]
    fn snapshot_extracts_ax_name_and_bounded_dom_state_without_script_text() {
        let dom = json!({
            "strings":["Ready","UTF-8","#document","DIV","role","status","data-state","ready","SCRIPT","ignore me"],
            "documents":[{
                "title":0,
                "nodes":{
                    "backendNodeId":[1,2,3],
                    "nodeName":[2,3,8],
                    "nodeValue":[-1,-1,9],
                    "attributes":[[],[4,5,6,7],[]]
                }
            }]
        });
        let ax = json!({"nodes":[{
            "ignored":false,
            "role":{"value":"status"},
            "name":{"value":"Deployment ready"},
            "backendDOMNodeId":2,
            "properties":[]
        }]});
        let (title, nodes, read, truncated) =
            extract_semantic_snapshot(&ax, &dom, 8, 4096).unwrap();
        assert_eq!(title, "Ready");
        assert_eq!(read, 1);
        assert!(!truncated);
        assert_eq!(nodes[0].role, "status");
        assert_eq!(nodes[0].accessible_name, "Deployment ready");
        assert_eq!(nodes[0].state["data-state"], "ready");
        assert!(!serde_json::to_string(&nodes).unwrap().contains("ignore me"));

        let (_, nodes, _, truncated) = extract_semantic_snapshot(&ax, &dom, 8, 20).unwrap();
        assert!(nodes.is_empty());
        assert!(truncated);
        let (_, nodes, read, truncated) = extract_semantic_snapshot(&ax, &dom, 0, 4096).unwrap();
        assert_eq!(read, 1);
        assert!(nodes.is_empty());
        assert!(truncated);
    }

    #[test]
    fn default_system_identity_is_non_secret_and_pinned() {
        let identity = SystemSemanticBrowserHarness::configured_identity(&NoNetwork);
        assert!(identity.contains(ADAPTER_ID));
        assert!(identity.contains("resolver=no_network_browser_fixture_v1"));
        assert!(!identity.contains("Google Chrome for Testing.app"));
    }

    #[test]
    fn sanitized_headers_remove_credentials_and_referer() {
        let headers = json!({
            "Accept":"text/html",
            "Cookie":"secret=1",
            "Authorization":"Bearer secret",
            "Referer":"https://secret.invalid/",
            "User-Agent":"Chrome"
        });
        let sanitized = sanitized_cdp_headers(Some(&headers));
        let encoded = serde_json::to_string(&sanitized).unwrap();
        assert!(encoded.contains("Accept"));
        assert!(encoded.contains("User-Agent"));
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("Referer"));
    }

    async fn start_rendered_fixture() -> (
        String,
        CancellationToken,
        JoinHandle<()>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((mut stream, _)) => {
                            let requests = Arc::clone(&task_requests);
                            connections.spawn(async move {
                                let mut request = Vec::new();
                                let mut chunk = [0_u8; 1024];
                                while request.len() < 16 * 1024 {
                                    let Ok(read) = stream.read(&mut chunk).await else { return };
                                    if read == 0 { return; }
                                    request.extend_from_slice(&chunk[..read]);
                                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                                let request_line = std::str::from_utf8(&request)
                                    .ok()
                                    .and_then(|request| request.lines().next())
                                    .unwrap_or_default()
                                    .to_owned();
                                requests.lock().unwrap().push(request_line);
                                let body = r#"<!doctype html><html><head><title>Rendered fixture</title></head><body><script>
setTimeout(() => {
  const status = document.createElement('div');
  status.setAttribute('role', 'status');
  status.setAttribute('data-state', ['rea','dy'].join(''));
  status.textContent = String.fromCharCode(...[68,101,112,108,111,121,109,101,110,116,32,114,101,97,100,121]);
  document.body.appendChild(status);
  const toggle = document.createElement('button');
  toggle.setAttribute('role', 'switch');
  toggle.setAttribute('aria-checked', String(true));
  toggle.textContent = String.fromCharCode(...[65,117,116,111,109,97,116,105,99,32,114,101,116,114,105,101,115,32,101,110,97,98,108,101,100]);
  document.body.appendChild(toggle);
  window.open('/popup-that-must-not-navigate');
}, 25);
</script></body></html>"#;
                                assert!(!body.contains("Deployment ready"));
                                assert!(!body.contains("Automatic retries enabled"));
                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                );
                                let _ = stream.write_all(response.as_bytes()).await;
                                let _ = stream.shutdown().await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
            while connections.join_next().await.is_some() {}
        });
        (origin, cancellation, task, requests)
    }

    #[tokio::test]
    #[ignore = "requires pinned Chrome for Testing fixture"]
    async fn pinned_cft_reads_js_only_ax_dom_and_tears_down() {
        let (origin, server_cancellation, server, requests) = start_rendered_fixture().await;
        let harness: Arc<dyn SemanticBrowserHarness> =
            Arc::new(SystemSemanticBrowserHarness::new(Arc::new(NoNetwork)));
        let outcome = execute_browser_navigate(
            input(&format!("{origin}/app")),
            harness,
            true,
            Some(&origin),
            CancellationToken::new(),
        )
        .await;
        server_cancellation.cancel();
        server.await.unwrap();
        assert!(outcome.is_success(), "{}", outcome.content);
        let result: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(result["title"], "Rendered fixture");
        assert_eq!(result["trust"], TRUST);
        assert_eq!(result["profile_ephemeral"], true);
        assert_eq!(result["teardown"]["process_tree_settled"], true);
        assert_eq!(result["teardown"]["proxy_settled"], true);
        assert_eq!(result["teardown"]["profile_removed"], true);
        let nodes = result["snapshot"].as_array().unwrap();
        assert!(
            nodes.iter().any(|node| {
                node["role"] == "status"
                    && node["accessible_name"] == "Deployment ready"
                    && node["state"]["data-state"] == "ready"
            }),
            "{}",
            result
        );
        let observed_requests = requests.lock().unwrap().clone();
        assert!(
            observed_requests
                .iter()
                .all(|request| !request.contains(" /popup-that-must-not-navigate ")),
            "additional target escaped Host guard: {observed_requests:?}"
        );
        assert!(
            nodes.iter().any(|node| {
                node["role"] == "switch"
                    && node["accessible_name"] == "Automatic retries enabled"
                    && (node["state"]["aria-checked"] == "true"
                        || node["state"]["checked"] == "true")
            }),
            "{}",
            result
        );
    }

    #[tokio::test]
    #[ignore = "requires pinned Chrome for Testing fixture"]
    async fn pinned_cft_cancellation_reaps_process_proxy_and_profile() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            trigger.cancel();
        });
        let outcome = execute_browser_navigate(
            input(&format!("{origin}/hang")),
            Arc::new(SystemSemanticBrowserHarness::new(Arc::new(NoNetwork))),
            true,
            Some(&origin),
            cancellation,
        )
        .await;
        server.abort();
        let _ = server.await;
        assert_eq!(outcome.operation, ToolOperationStatus::Cancelled);
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
        let teardown = &outcome.metadata.as_ref().unwrap()["semantic_browser"]["teardown"];
        assert_eq!(teardown["attempted"], true);
        assert_eq!(teardown["process_tree_settled"], true);
        assert_eq!(teardown["proxy_settled"], true);
        assert_eq!(teardown["profile_removed"], true);
    }

    #[tokio::test]
    #[ignore = "credential-free public HTTPS browser canary; run at most once per W2 candidate"]
    async fn credential_free_public_https_browser_canary() {
        let outcome = execute_browser_navigate(
            json!({"url":"https://example.com/","max_nodes":16,"max_chars":4096}),
            Arc::new(SystemSemanticBrowserHarness::new(Arc::new(
                crate::SystemWebFetchNetwork,
            ))),
            true,
            None,
            CancellationToken::new(),
        )
        .await;
        assert!(outcome.is_success(), "{}", outcome.content);
        let result: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(result["requested_url"], "https://example.com/");
        assert_eq!(result["final_url"], "https://example.com/");
        assert_eq!(result["trust"], TRUST);
        assert_eq!(result["teardown"]["process_tree_settled"], true);
        assert_eq!(result["teardown"]["proxy_settled"], true);
        assert_eq!(result["teardown"]["profile_removed"], true);
        assert!(
            result["nodes_returned"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
    }
}
