//! Ephemeral semantic browser backed by pinned Chrome for Testing.
//!
//! Public navigation remains one-shot and read-only. An exact Host-owned
//! loopback navigation may retain one in-memory page long enough for the same
//! run to consume an opaque latest-snapshot ref through `browser_click` or
//! `browser_fill`.
//! Neither the live page nor its refs become durable truth, and there is no
//! screenshot, selector, coordinate input, arbitrary JavaScript, or second
//! browser store.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use uuid::Uuid;

use crate::shell::{ProcessTreeOwner, configure_process_tree, shutdown_tokio_process_tree};
use crate::web_fetch::{WebFetchNetwork, is_public_ip};
use crate::{ToolOutcome, optional_u64, required_str};
use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
    ToolTransportStatus,
};

const TRUST: &str = "external_untrusted";
const ADAPTER_ID: &str = "direct_tokio_cdp_ref_click_fill_v3";
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
const MAX_ELEMENT_REF_CHARS: usize = 40;
const MAX_FILL_VALUE_CHARS: usize = 1_024;
const MAX_FILL_VALUE_BYTES: usize = 4_096;
#[cfg(target_os = "macos")]
const SELECT_ALL_MODIFIERS: u8 = 4;
#[cfg(not(target_os = "macos"))]
const SELECT_ALL_MODIFIERS: u8 = 2;
const MAX_STALE_REFS: usize = MAX_MAX_NODES * 2;

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

/// Canonical click input after fixed-schema and opaque-ref validation.
#[derive(Debug, Clone)]
pub struct BrowserClickRequest {
    element_ref: String,
}

/// Canonical fill input after fixed-schema, value, and opaque-ref validation.
#[derive(Debug, Clone)]
pub struct BrowserFillRequest {
    element_ref: String,
    value: String,
}

impl BrowserFillRequest {
    #[must_use]
    pub fn element_ref(&self) -> &str {
        &self.element_ref
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl BrowserClickRequest {
    #[must_use]
    pub fn element_ref(&self) -> &str {
        &self.element_ref
    }
}

/// Narrow deterministic seam used only by production loopback/reopen tests.
/// The system implementation below remains the only default production path.
#[async_trait]
pub trait SemanticBrowserHarness: Send + Sync {
    fn identity(&self) -> String;

    async fn navigate(
        &self,
        run_id: &str,
        request: BrowserNavigateRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome;

    async fn click(
        &self,
        run_id: &str,
        request: BrowserClickRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome;

    async fn fill(
        &self,
        run_id: &str,
        request: BrowserFillRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome;

    /// Synchronous fail-safe used when the owning production executor drops.
    /// Implementations must not leave a live process, proxy, or profile behind.
    fn shutdown(&self);
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
    session: Mutex<Option<LiveBrowserSession>>,
    last_drop_teardown: Arc<Mutex<Option<TeardownFacts>>>,
    operation_active: AtomicBool,
}

impl SystemSemanticBrowserHarness {
    #[must_use]
    pub fn new(resolver: Arc<dyn WebFetchNetwork>) -> Self {
        Self {
            binary: PinnedChromeForTesting::discover(),
            resolver,
            session: Mutex::new(None),
            last_drop_teardown: Arc::new(Mutex::new(None)),
            operation_active: AtomicBool::new(false),
        }
    }

    pub(crate) fn configured_identity(resolver: &dyn WebFetchNetwork) -> String {
        let binary = PinnedChromeForTesting::discover().map_or_else(
            || format!("{ADAPTER_ID}:unavailable"),
            |binary| binary.identity(),
        );
        format!("{binary}:resolver={}", resolver.identity())
    }

    fn begin_operation(&self) -> Result<BrowserOperationGuard<'_>, BrowserFailure> {
        self.operation_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| BrowserOperationGuard(&self.operation_active))
            .map_err(|_| {
                BrowserFailure::operation(
                    "browser_session_busy",
                    "session",
                    "another semantic browser operation is already active",
                )
            })
    }

    #[cfg(test)]
    async fn await_last_teardown(&self) -> Option<TeardownFacts> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(facts) = self
                    .last_drop_teardown
                    .lock()
                    .expect("browser teardown lock")
                    .clone()
                {
                    break facts;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .ok()
    }
}

struct BrowserOperationGuard<'a>(&'a AtomicBool);

impl Drop for BrowserOperationGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for SystemSemanticBrowserHarness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SystemSemanticBrowserHarness")
            .field("binary_configured", &self.binary.is_some())
            .field("resolver", &self.resolver.identity())
            .field(
                "active_session",
                &self.session.lock().expect("browser session lock").is_some(),
            )
            .field(
                "operation_active",
                &self.operation_active.load(Ordering::SeqCst),
            )
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
    #[serde(skip_serializing_if = "Option::is_none")]
    element_ref: Option<String>,
    role: String,
    accessible_name: String,
    text: String,
    value: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    state: BTreeMap<String, String>,
    #[serde(skip)]
    backend_dom_node_id: Option<u64>,
    #[serde(skip)]
    click_safety: Option<ClickSafety>,
    #[serde(skip)]
    fill_safety: Option<FillSafety>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClickSafety {
    Allowed,
    Disabled,
    SideEffectDenied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FillSafety {
    Allowed,
    Disabled,
    ReadOnly,
    SensitiveDenied,
    Ineligible,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<BrowserActionResult>,
    requested_url: String,
    final_url: String,
    title: String,
    snapshot: Vec<SemanticNode>,
    snapshot_id: String,
    page_epoch: u64,
    element_refs_returned: usize,
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
    browser_identity: String,
    cdp_protocol_version: String,
    chrome_for_testing_version: &'static str,
    chrome_executable_sha256: String,
    retrieved_at: String,
    trust: &'static str,
    profile_ephemeral: bool,
    session_scope: &'static str,
    session_live: bool,
    process_tree_settled: bool,
    teardown: TeardownFacts,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserActionResult {
    kind: &'static str,
    consumed_element_ref: String,
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
    run_id: &str,
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
    harness.navigate(run_id, request, cancellation).await
}

pub(crate) fn preflight_browser_click(input: &Value) -> Option<ToolOutcome> {
    match parse_click_request(input) {
        Ok(_) => None,
        Err(failure) => Some(rejected_click_outcome("", failure)),
    }
}

pub(crate) async fn execute_browser_click(
    run_id: &str,
    input: Value,
    harness: Arc<dyn SemanticBrowserHarness>,
    network_allowed: bool,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let element_ref = input
        .get("element_ref")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let request = match parse_click_request(&input) {
        Ok(request) => request,
        Err(failure) => return click_operation_outcome(&element_ref, failure, None, false),
    };
    if !network_allowed {
        return click_operation_outcome(
            &element_ref,
            BrowserFailure::operation(
                "browser_network_not_authorized",
                "authorization",
                "当前 Run permission 或 actor sandbox 禁止浏览器网络访问",
            ),
            None,
            false,
        );
    }
    harness.click(run_id, request, cancellation).await
}

pub(crate) fn preflight_browser_fill(input: &Value) -> Option<ToolOutcome> {
    match parse_fill_request(input) {
        Ok(_) => None,
        Err(failure) => Some(rejected_fill_outcome("", failure)),
    }
}

pub(crate) async fn execute_browser_fill(
    run_id: &str,
    input: Value,
    harness: Arc<dyn SemanticBrowserHarness>,
    network_allowed: bool,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let element_ref = input
        .get("element_ref")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let request = match parse_fill_request(&input) {
        Ok(request) => request,
        Err(failure) => return fill_operation_outcome(&element_ref, failure, None, false),
    };
    if !network_allowed {
        return fill_operation_outcome(
            &element_ref,
            BrowserFailure::operation(
                "browser_network_not_authorized",
                "authorization",
                "当前 Run permission 或 actor sandbox 禁止浏览器网络访问",
            ),
            None,
            false,
        );
    }
    harness.fill(run_id, request, cancellation).await
}

fn parse_click_request(input: &Value) -> Result<BrowserClickRequest, BrowserFailure> {
    let element_ref = parse_element_ref(input)?;
    Ok(BrowserClickRequest { element_ref })
}

fn parse_fill_request(input: &Value) -> Result<BrowserFillRequest, BrowserFailure> {
    let element_ref = parse_element_ref(input)?;
    let value = required_str(input, "value").map_err(|error| {
        BrowserFailure::rejected("browser_fill_value_missing", "value", error.to_string())
    })?;
    let chars = value.chars().count();
    if chars == 0 || chars > MAX_FILL_VALUE_CHARS || value.len() > MAX_FILL_VALUE_BYTES {
        return Err(BrowserFailure::rejected(
            "browser_fill_value_invalid",
            "value",
            format!(
                "value 必须非空且不超过 {MAX_FILL_VALUE_CHARS} 字符/{MAX_FILL_VALUE_BYTES} UTF-8 字节"
            ),
        ));
    }
    if value
        .chars()
        .any(|character| matches!(character as u32, 0x00..=0x1f | 0x7f..=0x9f))
    {
        return Err(BrowserFailure::rejected(
            "browser_fill_value_control_denied",
            "value",
            "value 不允许 NUL、C0/C1、DEL 或换行控制字符",
        ));
    }
    Ok(BrowserFillRequest {
        element_ref,
        value: value.to_owned(),
    })
}

fn parse_element_ref(input: &Value) -> Result<String, BrowserFailure> {
    let element_ref = required_str(input, "element_ref").map_err(|error| {
        BrowserFailure::rejected(
            "browser_element_ref_missing",
            "element_ref",
            error.to_string(),
        )
    })?;
    if element_ref.trim() != element_ref
        || element_ref.is_empty()
        || element_ref.chars().count() > MAX_ELEMENT_REF_CHARS
        || !element_ref.starts_with("eref_")
        || !element_ref[5..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(BrowserFailure::rejected(
            "browser_element_ref_invalid",
            "element_ref",
            "element_ref 必须是 Host 返回的有界 opaque ref",
        ));
    }
    Ok(element_ref.to_owned())
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

fn rejected_click_outcome(element_ref: &str, failure: BrowserFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!(
            "browser_click 拒绝：code={}；{}",
            failure.code, failure.message
        ),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(click_failure_metadata(element_ref, &failure, None))
}

fn rejected_fill_outcome(element_ref: &str, failure: BrowserFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!(
            "browser_fill 拒绝：code={}；{}",
            failure.code, failure.message
        ),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(action_failure_metadata(element_ref, "fill", &failure, None))
}

fn click_operation_outcome(
    element_ref: &str,
    failure: BrowserFailure,
    teardown: Option<&TeardownFacts>,
    dispatched: bool,
) -> ToolOutcome {
    let mut outcome = if failure.transport {
        let mut outcome = ToolOutcome::transport_failure(format!(
            "browser_click 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.retry = if dispatched {
            ToolRetryDisposition::Unsafe
        } else {
            failure.retry
        };
        outcome
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "browser_click 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.transport = ToolTransportStatus::Succeeded;
        outcome.operation = ToolOperationStatus::Failed;
        outcome.retry = if dispatched {
            ToolRetryDisposition::NotRetryable
        } else {
            failure.retry
        };
        outcome
    };
    outcome.side_effect = if dispatched {
        ToolSideEffectStatus::Indeterminate
    } else {
        ToolSideEffectStatus::NotApplied
    };
    outcome.metadata = Some(click_failure_metadata(element_ref, &failure, teardown));
    outcome
}

fn fill_operation_outcome(
    element_ref: &str,
    failure: BrowserFailure,
    teardown: Option<&TeardownFacts>,
    dispatched: bool,
) -> ToolOutcome {
    let mut outcome = if failure.transport {
        let mut outcome = ToolOutcome::transport_failure(format!(
            "browser_fill 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.retry = if dispatched {
            ToolRetryDisposition::Unsafe
        } else {
            failure.retry
        };
        outcome
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "browser_fill 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.transport = ToolTransportStatus::Succeeded;
        outcome.operation = if dispatched {
            ToolOperationStatus::Indeterminate
        } else {
            ToolOperationStatus::Failed
        };
        outcome.retry = if dispatched {
            ToolRetryDisposition::Unsafe
        } else {
            failure.retry
        };
        outcome
    };
    outcome.side_effect = if dispatched {
        ToolSideEffectStatus::Indeterminate
    } else {
        ToolSideEffectStatus::NotApplied
    };
    outcome.metadata = Some(action_failure_metadata(
        element_ref,
        "fill",
        &failure,
        teardown,
    ));
    outcome
}

fn click_failure_metadata(
    element_ref: &str,
    failure: &BrowserFailure,
    teardown: Option<&TeardownFacts>,
) -> Value {
    action_failure_metadata(element_ref, "click", failure, teardown)
}

fn action_failure_metadata(
    element_ref: &str,
    action: &'static str,
    failure: &BrowserFailure,
    teardown: Option<&TeardownFacts>,
) -> Value {
    json!({
        "semantic_browser": {
            "element_ref": element_ref,
            "action": action,
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
    action_started: AtomicBool,
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
            action_started: AtomicBool::new(false),
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
        if fatal || self.action_started.load(Ordering::SeqCst) {
            let mut current = self
                .fatal_failure
                .lock()
                .expect("browser fatal failure lock");
            if current.is_none() {
                *current = Some(failure);
            }
        }
    }

    fn record_proxy_denial(&self, failure: BrowserFailure, fatal: bool) {
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

    fn begin_action(&self) {
        self.action_started.store(true, Ordering::SeqCst);
    }

    fn end_action(&self) {
        self.action_started.store(false, Ordering::SeqCst);
    }
}

struct EgressProxy {
    address: SocketAddr,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.task.abort();
    }
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
                                    state.record_proxy_denial(failure, fatal);
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

    async fn shutdown(mut self) -> bool {
        self.cancellation.cancel();
        match tokio::time::timeout(PROXY_TEARDOWN_GRACE, &mut self.task).await {
            Ok(Ok(())) => true,
            _ => {
                self.task.abort();
                let _ = (&mut self.task).await;
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
        if let Some(failure) = prohibited_browser_event(method) {
            self.egress.record_denial(failure.clone(), true);
            return Err(failure);
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

fn prohibited_browser_event(method: Option<&str>) -> Option<BrowserFailure> {
    (method == Some("Browser.downloadWillBegin")).then(|| {
        BrowserFailure::operation(
            "browser_download_denied",
            "page",
            "semantic browser denied a download",
        )
    })
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

#[derive(Debug, Clone)]
struct ElementTarget {
    backend_dom_node_id: u64,
    role: String,
    accessible_name: String,
    capability: ElementCapability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ElementCapability {
    Click(ClickSafety),
    Fill,
}

struct LiveBrowserSession {
    run_id: String,
    browser_identity: String,
    page_epoch: u64,
    snapshot_id: String,
    refs: HashMap<String, ElementTarget>,
    stale_refs: BTreeSet<String>,
    observation_fingerprint: String,
    observation: SessionObservation,
    request: BrowserNavigateRequest,
    binary: PinnedChromeForTesting,
    egress: Arc<EgressState>,
    cdp: CdpClient,
    cdp_session_id: String,
    proxy: Option<EgressProxy>,
    profile: Option<tempfile::TempDir>,
    child: tokio::process::Child,
    owner: ProcessTreeOwner,
}

impl Drop for LiveBrowserSession {
    fn drop(&mut self) {
        let _ = self.owner.kill();
        let _ = self.child.start_kill();
        if let Some(proxy) = self.proxy.as_mut() {
            proxy.cancellation.cancel();
            proxy.task.abort();
        }
    }
}

struct OpenBrowser {
    session: LiveBrowserSession,
}

impl LiveBrowserSession {
    async fn teardown(mut self) -> TeardownFacts {
        let _ = self.cdp.call("Browser.close", json!({}), None).await;
        let process_tree_settled =
            shutdown_tokio_process_tree(&mut self.child, &mut self.owner, PROCESS_TEARDOWN_GRACE)
                .await;
        let proxy_settled = match self.proxy.take() {
            Some(proxy) => proxy.shutdown().await,
            None => true,
        };
        let profile_removed = self
            .profile
            .take()
            .is_none_or(|profile| profile.close().is_ok());
        TeardownFacts {
            attempted: true,
            process_tree_settled,
            proxy_settled,
            profile_removed,
        }
    }
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
        run_id: &str,
        request: BrowserNavigateRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        let _guard = match self.begin_operation() {
            Ok(guard) => guard,
            Err(failure) => return operation_outcome(&request.requested_url, failure, None),
        };
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
        let previous = self.session.lock().expect("browser session lock").take();
        if let Some(previous) = previous {
            let facts = previous.teardown().await;
            *self
                .last_drop_teardown
                .lock()
                .expect("browser teardown lock") = Some(facts);
        }
        let mut open = match start_system_browser(
            run_id,
            request,
            binary,
            Arc::clone(&self.resolver),
            cancellation,
        )
        .await
        {
            Ok(open) => open,
            Err(outcome) => return outcome,
        };
        let retain = matches!(
            open.session.request.scope,
            BrowserTargetScope::ExactLocal(_)
        );
        if retain {
            rotate_observation(&mut open.session, true);
            let outcome = browser_result_outcome(&open.session, None, false);
            *self.session.lock().expect("browser session lock") = Some(open.session);
            outcome
        } else {
            rotate_observation(&mut open.session, false);
            let result = browser_result_value(&open.session, None, false);
            let teardown = open.session.teardown().await;
            if !teardown.process_tree_settled
                || !teardown.proxy_settled
                || !teardown.profile_removed
            {
                return operation_outcome(
                    result.requested_url.as_str(),
                    BrowserFailure::operation(
                        "browser_teardown_incomplete",
                        "teardown",
                        "browser process, proxy, or ephemeral profile did not settle",
                    ),
                    Some(&teardown),
                );
            }
            let mut settled = result;
            settled.process_tree_settled = true;
            settled.teardown = teardown;
            ToolOutcome::json(&settled)
                .expect("bounded browser result serializes")
                .with_side_effect(ToolSideEffectStatus::NotApplicable)
        }
    }

    async fn click(
        &self,
        run_id: &str,
        request: BrowserClickRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        let _guard = match self.begin_operation() {
            Ok(guard) => guard,
            Err(failure) => {
                return click_operation_outcome(&request.element_ref, failure, None, false);
            }
        };
        let Some(mut session) = self.session.lock().expect("browser session lock").take() else {
            return click_operation_outcome(
                &request.element_ref,
                BrowserFailure::operation(
                    "browser_session_missing",
                    "session",
                    "browser_click requires a live exact-loopback browser_navigate observation",
                ),
                None,
                false,
            );
        };
        enum Completion {
            Finished(Box<ToolOutcome>),
            Cancelled,
            TimedOut,
        }
        let completion = {
            let execution =
                execute_live_click(&mut session, run_id, &request, cancellation.clone());
            tokio::pin!(execution);
            tokio::select! {
                outcome = &mut execution => Completion::Finished(Box::new(outcome)),
                () = cancellation.cancelled() => Completion::Cancelled,
                () = tokio::time::sleep(OVERALL_DEADLINE) => Completion::TimedOut,
            }
        };
        match completion {
            Completion::Finished(outcome) => {
                let outcome = *outcome;
                if outcome.side_effect == ToolSideEffectStatus::Indeterminate {
                    let teardown = session.teardown().await;
                    let mut outcome = outcome;
                    if let Some(metadata) = outcome.metadata.as_mut()
                        && let Some(browser) = metadata
                            .get_mut("semantic_browser")
                            .and_then(Value::as_object_mut)
                    {
                        browser.insert(
                            "teardown".to_owned(),
                            serde_json::to_value(teardown)
                                .expect("browser teardown facts serialize"),
                        );
                    }
                    outcome
                } else {
                    *self.session.lock().expect("browser session lock") = Some(session);
                    outcome
                }
            }
            Completion::Cancelled | Completion::TimedOut => {
                let dispatched = session.egress.action_started.load(Ordering::SeqCst);
                session.egress.end_action();
                let failure = match completion {
                    Completion::Cancelled => BrowserFailure {
                        code: "browser_cancelled",
                        stage: "cancellation",
                        message: "browser_click was cancelled".to_owned(),
                        transport: false,
                        retry: if dispatched {
                            ToolRetryDisposition::Unsafe
                        } else {
                            ToolRetryDisposition::Safe
                        },
                    },
                    Completion::TimedOut => BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        format!("browser_click exceeded {} ms", OVERALL_DEADLINE.as_millis()),
                    ),
                    Completion::Finished(_) => unreachable!(),
                };
                if dispatched {
                    let teardown = session.teardown().await;
                    click_operation_outcome(&request.element_ref, failure, Some(&teardown), true)
                } else {
                    let outcome =
                        click_operation_outcome(&request.element_ref, failure, None, false);
                    *self.session.lock().expect("browser session lock") = Some(session);
                    outcome
                }
            }
        }
    }

    async fn fill(
        &self,
        run_id: &str,
        request: BrowserFillRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        let _guard = match self.begin_operation() {
            Ok(guard) => guard,
            Err(failure) => {
                return fill_operation_outcome(&request.element_ref, failure, None, false);
            }
        };
        let Some(mut session) = self.session.lock().expect("browser session lock").take() else {
            return fill_operation_outcome(
                &request.element_ref,
                BrowserFailure::operation(
                    "browser_session_missing",
                    "session",
                    "browser_fill requires a live exact-loopback browser_navigate observation",
                ),
                None,
                false,
            );
        };
        enum Completion {
            Finished(Box<ToolOutcome>),
            Cancelled,
            TimedOut,
        }
        let completion = {
            let execution = execute_live_fill(&mut session, run_id, &request, cancellation.clone());
            tokio::pin!(execution);
            tokio::select! {
                outcome = &mut execution => Completion::Finished(Box::new(outcome)),
                () = cancellation.cancelled() => Completion::Cancelled,
                () = tokio::time::sleep(OVERALL_DEADLINE) => Completion::TimedOut,
            }
        };
        match completion {
            Completion::Finished(outcome) => {
                let outcome = *outcome;
                if outcome.side_effect == ToolSideEffectStatus::Indeterminate {
                    let teardown = session.teardown().await;
                    let mut outcome = outcome;
                    if let Some(metadata) = outcome.metadata.as_mut()
                        && let Some(browser) = metadata
                            .get_mut("semantic_browser")
                            .and_then(Value::as_object_mut)
                    {
                        browser.insert(
                            "teardown".to_owned(),
                            serde_json::to_value(teardown)
                                .expect("browser teardown facts serialize"),
                        );
                    }
                    outcome
                } else {
                    *self.session.lock().expect("browser session lock") = Some(session);
                    outcome
                }
            }
            Completion::Cancelled | Completion::TimedOut => {
                let dispatched = session.egress.action_started.load(Ordering::SeqCst);
                session.egress.end_action();
                let failure = match completion {
                    Completion::Cancelled => BrowserFailure {
                        code: "browser_cancelled",
                        stage: "cancellation",
                        message: "browser_fill was cancelled".to_owned(),
                        transport: false,
                        retry: if dispatched {
                            ToolRetryDisposition::Unsafe
                        } else {
                            ToolRetryDisposition::Safe
                        },
                    },
                    Completion::TimedOut => BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        format!("browser_fill exceeded {} ms", OVERALL_DEADLINE.as_millis()),
                    ),
                    Completion::Finished(_) => unreachable!(),
                };
                if dispatched {
                    let teardown = session.teardown().await;
                    fill_operation_outcome(&request.element_ref, failure, Some(&teardown), true)
                } else {
                    let outcome =
                        fill_operation_outcome(&request.element_ref, failure, None, false);
                    *self.session.lock().expect("browser session lock") = Some(session);
                    outcome
                }
            }
        }
    }

    fn shutdown(&self) {
        let session = self.session.lock().expect("browser session lock").take();
        let Some(session) = session else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let facts = Arc::clone(&self.last_drop_teardown);
            runtime.spawn(async move {
                *facts.lock().expect("browser teardown lock") = Some(session.teardown().await);
            });
        } else {
            *self
                .last_drop_teardown
                .lock()
                .expect("browser teardown lock") = Some(TeardownFacts {
                attempted: true,
                process_tree_settled: false,
                proxy_settled: false,
                profile_removed: false,
            });
            drop(session);
        }
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

async fn start_system_browser(
    run_id: &str,
    request: BrowserNavigateRequest,
    binary: PinnedChromeForTesting,
    resolver: Arc<dyn WebFetchNetwork>,
    cancellation: CancellationToken,
) -> Result<OpenBrowser, ToolOutcome> {
    let egress = Arc::new(EgressState::new(&request));
    let proxy = match EgressProxy::start(Arc::clone(&egress), resolver).await {
        Ok(proxy) => proxy,
        Err(failure) => return Err(operation_outcome(&request.requested_url, failure, None)),
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
            return Err(operation_outcome(
                &request.requested_url,
                BrowserFailure::operation(
                    "browser_profile_failed",
                    "profile",
                    format!("cannot create ephemeral browser profile: {error}"),
                ),
                Some(&teardown),
            ));
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
            return Err(operation_outcome(
                &request.requested_url,
                BrowserFailure::transport(
                    "browser_spawn_failed",
                    "process",
                    format!("cannot start pinned Chrome for Testing: {error}"),
                ),
                Some(&teardown),
            ));
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
            return Err(operation_outcome(
                &request.requested_url,
                BrowserFailure::operation(
                    "browser_process_ownership_failed",
                    "process",
                    format!("cannot own Chrome process tree: {error}"),
                ),
                Some(&teardown),
            ));
        }
    };

    let execution = {
        let cdp_session = run_cdp_session(&request, profile.path(), Arc::clone(&egress));
        tokio::pin!(cdp_session);
        tokio::select! {
            () = cancellation.cancelled() => Err(BrowserFailure {
                code: "browser_cancelled",
                stage: "cancellation",
                message: "browser_navigate was cancelled".to_owned(),
                transport: false,
                retry: ToolRetryDisposition::Safe,
            }),
            result = tokio::time::timeout(OVERALL_DEADLINE, &mut cdp_session) => match result {
                Ok(result) => result,
                Err(_) => Err(BrowserFailure::transport(
                    "browser_deadline_exceeded",
                    "deadline",
                    format!("browser_navigate exceeded {} ms", OVERALL_DEADLINE.as_millis()),
                )),
            }
        }
    };
    let (cdp, cdp_session_id, observation) = match execution {
        Ok(open) => open,
        Err(failure) => {
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
            let mut outcome =
                operation_outcome(&request.requested_url, failure.clone(), Some(&teardown));
            if failure.code == "browser_cancelled" {
                outcome.operation = ToolOperationStatus::Cancelled;
                outcome.side_effect = ToolSideEffectStatus::NotApplied;
            }
            return Err(outcome);
        }
    };
    if let Some(failure) = egress.take_fatal_failure() {
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
        return Err(operation_outcome(
            &request.requested_url,
            failure,
            Some(&teardown),
        ));
    }
    let observation_fingerprint = observation_fingerprint(&observation.nodes);
    Ok(OpenBrowser {
        session: LiveBrowserSession {
            run_id: run_id.to_owned(),
            browser_identity: format!("browser_{}", Uuid::new_v4().simple()),
            page_epoch: 0,
            snapshot_id: String::new(),
            refs: HashMap::new(),
            stale_refs: BTreeSet::new(),
            observation_fingerprint,
            observation,
            request,
            binary,
            egress,
            cdp,
            cdp_session_id,
            proxy: Some(proxy),
            profile: Some(profile),
            child,
            owner,
        },
    })
}

fn observation_fingerprint(nodes: &[SemanticNode]) -> String {
    let identity = nodes
        .iter()
        .map(|node| {
            json!({
                "backend_dom_node_id": node.backend_dom_node_id,
                "role": node.role,
                "accessible_name": node.accessible_name,
                "text": node.text,
                "value": node.value,
                "state": node.state,
                "click_safety": format!("{:?}", node.click_safety),
                "fill_safety": format!("{:?}", node.fill_safety),
            })
        })
        .collect::<Vec<_>>();
    sha256_bytes(&serde_json::to_vec(&identity).expect("browser fingerprint serializes"))
}

fn rotate_observation(session: &mut LiveBrowserSession, allow_refs: bool) {
    for element_ref in session.refs.keys() {
        session.stale_refs.insert(element_ref.clone());
    }
    while session.stale_refs.len() > MAX_STALE_REFS {
        let Some(first) = session.stale_refs.iter().next().cloned() else {
            break;
        };
        session.stale_refs.remove(&first);
    }
    session.refs.clear();
    for node in &mut session.observation.nodes {
        node.element_ref = None;
        if !allow_refs {
            continue;
        }
        let Some(backend_dom_node_id) = node.backend_dom_node_id else {
            continue;
        };
        let capability = match (&node.click_safety, &node.fill_safety) {
            (Some(safety @ (ClickSafety::Allowed | ClickSafety::Disabled)), _) => {
                ElementCapability::Click(safety.clone())
            }
            (None, Some(FillSafety::Allowed)) => ElementCapability::Fill,
            _ => continue,
        };
        let element_ref = new_element_ref(&session.refs, &session.stale_refs);
        session.refs.insert(
            element_ref.clone(),
            ElementTarget {
                backend_dom_node_id,
                role: node.role.clone(),
                accessible_name: node.accessible_name.clone(),
                capability,
            },
        );
        node.element_ref = Some(element_ref);
    }
    session.page_epoch = session.page_epoch.saturating_add(1).max(1);
    session.snapshot_id = format!("snapshot_{}", Uuid::new_v4().simple());
    session.observation_fingerprint = observation_fingerprint(&session.observation.nodes);
}

fn new_element_ref(refs: &HashMap<String, ElementTarget>, stale_refs: &BTreeSet<String>) -> String {
    loop {
        let candidate = format!("eref_{}", Uuid::new_v4().simple());
        if !refs.contains_key(&candidate) && !stale_refs.contains(&candidate) {
            return candidate;
        }
    }
}

fn browser_result_value(
    session: &LiveBrowserSession,
    action: Option<BrowserActionResult>,
    process_tree_settled: bool,
) -> BrowserResult {
    let snapshot_bytes = serde_json::to_vec(&session.observation.nodes)
        .expect("bounded semantic browser snapshot serializes");
    BrowserResult {
        action,
        requested_url: session.request.requested_url.clone(),
        final_url: session.observation.final_url.clone(),
        title: session.observation.title.clone(),
        snapshot: session.observation.nodes.clone(),
        snapshot_id: session.snapshot_id.clone(),
        page_epoch: session.page_epoch,
        element_refs_returned: session.refs.len(),
        snapshot_sha256: format!("sha256:{}", sha256_bytes(&snapshot_bytes)),
        snapshot_sha256_scope: "bounded_semantic_observation_replay_identity",
        nodes_read: session.observation.nodes_read,
        nodes_returned: session.observation.nodes.len(),
        bytes_returned: snapshot_bytes.len(),
        truncated: session.observation.truncated,
        redirect_count: session.egress.redirect_count.load(Ordering::SeqCst),
        network_requests: session.egress.metrics.requests.load(Ordering::SeqCst),
        network_bytes_sent: session.egress.metrics.bytes_sent.load(Ordering::SeqCst),
        network_bytes_received: session.egress.metrics.bytes_received.load(Ordering::SeqCst),
        decoded_body_bytes: session
            .egress
            .metrics
            .decoded_body_bytes
            .load(Ordering::SeqCst),
        blocked_requests: session
            .egress
            .metrics
            .blocked_requests
            .load(Ordering::SeqCst),
        browser_product: session.observation.browser_product.clone(),
        browser_identity: session.browser_identity.clone(),
        cdp_protocol_version: session.observation.protocol_version.clone(),
        chrome_for_testing_version: session.binary.version,
        chrome_executable_sha256: format!("sha256:{}", session.binary.sha256),
        retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        trust: TRUST,
        profile_ephemeral: true,
        session_scope: match session.request.scope {
            BrowserTargetScope::ExactLocal(_) => "same_run_in_memory_exact_loopback",
            BrowserTargetScope::Public => "one_shot_public",
        },
        session_live: matches!(session.request.scope, BrowserTargetScope::ExactLocal(_)),
        process_tree_settled,
        teardown: TeardownFacts {
            attempted: false,
            process_tree_settled,
            proxy_settled: process_tree_settled,
            profile_removed: process_tree_settled,
        },
    }
}

fn browser_result_outcome(
    session: &LiveBrowserSession,
    action: Option<BrowserActionResult>,
    process_tree_settled: bool,
) -> ToolOutcome {
    ToolOutcome::json(&browser_result_value(session, action, process_tree_settled))
        .expect("bounded browser result serializes")
        .with_side_effect(ToolSideEffectStatus::NotApplicable)
}

async fn capture_current_observation(
    session: &mut LiveBrowserSession,
) -> Result<SessionObservation, BrowserFailure> {
    let before = session
        .cdp
        .call(
            "Page.getNavigationHistory",
            json!({}),
            Some(&session.cdp_session_id),
        )
        .await?;
    let ax = session
        .cdp
        .call(
            "Accessibility.getFullAXTree",
            json!({"depth":16}),
            Some(&session.cdp_session_id),
        )
        .await?;
    let dom = session
        .cdp
        .call(
            "DOMSnapshot.captureSnapshot",
            json!({
                "computedStyles":[],
                "includeDOMRects":false,
                "includePaintOrder":false,
            }),
            Some(&session.cdp_session_id),
        )
        .await?;
    let after = session
        .cdp
        .call(
            "Page.getNavigationHistory",
            json!({}),
            Some(&session.cdp_session_id),
        )
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
    let final_url = Url::parse(&after_url).map_err(|error| {
        BrowserFailure::operation(
            "browser_final_url_invalid",
            "snapshot",
            format!("Chrome returned invalid final URL: {error}"),
        )
    })?;
    let expected = match &session.request.scope {
        BrowserTargetScope::ExactLocal(origin) => origin,
        BrowserTargetScope::Public => &session.egress.initial_origin,
    };
    if &Origin::from_url(&final_url)? != expected {
        return Err(BrowserFailure::operation(
            "browser_cross_origin_navigation_denied",
            "redirect",
            "current document escaped the Host-authorized origin",
        ));
    }
    let (title, nodes, nodes_read, truncated) = extract_semantic_snapshot(
        &ax,
        &dom,
        session.request.max_nodes,
        session.request.max_chars,
    )?;
    Ok(SessionObservation {
        final_url: after_url,
        title,
        nodes,
        nodes_read,
        truncated,
        browser_product: session.observation.browser_product.clone(),
        protocol_version: session.observation.protocol_version.clone(),
    })
}

fn click_failure_with_fresh_observation(
    session: &LiveBrowserSession,
    request: &BrowserClickRequest,
    failure: BrowserFailure,
    dispatched: bool,
) -> ToolOutcome {
    let observation = browser_result_value(
        session,
        Some(BrowserActionResult {
            kind: "click",
            consumed_element_ref: request.element_ref.clone(),
        }),
        false,
    );
    let content = json!({
        "failure": {
            "code": failure.code,
            "stage": failure.stage,
            "message": failure.message,
        },
        "fresh_observation": observation,
        "trust": TRUST,
    })
    .to_string();
    let mut outcome =
        ToolOutcome::error(content).with_failure_code(ToolFailureCode::OperationFailed);
    outcome.transport = if failure.transport {
        ToolTransportStatus::Failed
    } else {
        ToolTransportStatus::Succeeded
    };
    outcome.operation = ToolOperationStatus::Failed;
    outcome.side_effect = if dispatched {
        ToolSideEffectStatus::Applied
    } else {
        ToolSideEffectStatus::NotApplied
    };
    outcome.retry = if dispatched {
        ToolRetryDisposition::NotRetryable
    } else {
        failure.retry
    };
    outcome.metadata = Some(click_failure_metadata(&request.element_ref, &failure, None));
    outcome
}

fn box_center(model: &Value) -> Option<(f64, f64)> {
    let quad = model
        .get("model")
        .and_then(|model| model.get("content").or_else(|| model.get("border")))
        .and_then(Value::as_array)?;
    if quad.len() != 8 {
        return None;
    }
    let values = quad.iter().map(Value::as_f64).collect::<Option<Vec<_>>>()?;
    let xs = [values[0], values[2], values[4], values[6]];
    let ys = [values[1], values[3], values[5], values[7]];
    let min_x = xs.into_iter().fold(f64::INFINITY, f64::min);
    let max_x = xs.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.into_iter().fold(f64::INFINITY, f64::min);
    let max_y = ys.into_iter().fold(f64::NEG_INFINITY, f64::max);
    (min_x.is_finite()
        && max_x.is_finite()
        && min_y.is_finite()
        && max_y.is_finite()
        && max_x - min_x >= 1.0
        && max_y - min_y >= 1.0)
        .then_some(((min_x + max_x) / 2.0, (min_y + max_y) / 2.0))
}

async fn invalidate_with_failure(
    session: &mut LiveBrowserSession,
    request: &BrowserClickRequest,
    observation: SessionObservation,
    failure: BrowserFailure,
) -> ToolOutcome {
    session.observation = observation;
    rotate_observation(session, true);
    click_failure_with_fresh_observation(session, request, failure, false)
}

async fn invalidate_fill_with_failure(
    session: &mut LiveBrowserSession,
    request: &BrowserFillRequest,
    observation: SessionObservation,
    failure: BrowserFailure,
) -> ToolOutcome {
    session.observation = observation;
    rotate_observation(session, true);
    fill_failure_with_fresh_observation(session, request, failure, false)
}

fn resolve_element_ref(
    session_run_id: &str,
    caller_run_id: &str,
    refs: &HashMap<String, ElementTarget>,
    stale_refs: &BTreeSet<String>,
    element_ref: &str,
) -> Result<ElementTarget, BrowserFailure> {
    if session_run_id != caller_run_id {
        return Err(BrowserFailure::operation(
            "browser_element_ref_cross_run",
            "element_ref",
            "element_ref belongs to a different Run",
        ));
    }
    if stale_refs.contains(element_ref) {
        return Err(BrowserFailure::operation(
            "browser_element_ref_stale",
            "element_ref",
            "element_ref is from an older page epoch",
        ));
    }
    refs.get(element_ref).cloned().ok_or_else(|| {
        BrowserFailure::operation(
            "browser_element_ref_missing",
            "element_ref",
            "element_ref is not present in the current Host snapshot",
        )
    })
}

fn validate_target_identity<'a>(
    target: &ElementTarget,
    nodes: &'a [SemanticNode],
) -> Result<Option<&'a SemanticNode>, BrowserFailure> {
    let mut matching = nodes
        .iter()
        .filter(|node| node.backend_dom_node_id == Some(target.backend_dom_node_id));
    let first = matching.next();
    if matching.next().is_some() {
        return Err(BrowserFailure::operation(
            "browser_element_ref_ambiguous",
            "element_ref",
            "element_ref resolved to more than one semantic node",
        ));
    }
    let Some(current) = first else {
        return Ok(None);
    };
    if current.role != target.role || current.accessible_name != target.accessible_name {
        return Err(BrowserFailure::operation(
            "browser_element_ref_stale_target",
            "element_ref",
            "element_ref target identity changed after the Host observation",
        ));
    }
    Ok(Some(current))
}

fn validate_click_target<'a>(
    target: &ElementTarget,
    nodes: &'a [SemanticNode],
) -> Result<Option<&'a SemanticNode>, BrowserFailure> {
    let Some(current) = validate_target_identity(target, nodes)? else {
        return Ok(None);
    };
    let ElementCapability::Click(safety) = &target.capability else {
        return Err(BrowserFailure::operation(
            "browser_element_ref_capability_mismatch",
            "element_ref",
            "browser_click cannot consume a fill-only element_ref",
        ));
    };
    match (safety, &current.click_safety) {
        (ClickSafety::Disabled, _) | (_, Some(ClickSafety::Disabled)) => {
            Err(BrowserFailure::operation(
                "browser_element_ref_disabled",
                "element_ref",
                "element_ref resolves to a disabled target",
            ))
        }
        (ClickSafety::SideEffectDenied, _) | (_, Some(ClickSafety::SideEffectDenied)) => {
            Err(BrowserFailure::operation(
                "browser_element_ref_side_effect_denied",
                "element_ref",
                "element_ref target is outside the admitted click-only control family",
            ))
        }
        _ => Ok(Some(current)),
    }
}

fn validate_fill_target<'a>(
    target: &ElementTarget,
    nodes: &'a [SemanticNode],
) -> Result<Option<&'a SemanticNode>, BrowserFailure> {
    let Some(current) = validate_target_identity(target, nodes)? else {
        return Ok(None);
    };
    if target.capability != ElementCapability::Fill {
        return Err(BrowserFailure::operation(
            "browser_element_ref_capability_mismatch",
            "element_ref",
            "browser_fill cannot consume a click-only element_ref",
        ));
    }
    match &current.fill_safety {
        Some(FillSafety::Allowed) => Ok(Some(current)),
        Some(FillSafety::Disabled) => Err(BrowserFailure::operation(
            "browser_element_ref_disabled",
            "element_ref",
            "element_ref resolves to a disabled text-entry target",
        )),
        Some(FillSafety::ReadOnly) => Err(BrowserFailure::operation(
            "browser_element_ref_readonly",
            "element_ref",
            "element_ref resolves to a readonly text-entry target",
        )),
        Some(FillSafety::SensitiveDenied) => Err(BrowserFailure::operation(
            "browser_fill_sensitive_target_denied",
            "element_ref",
            "browser_fill denies login, secret, token, credential, and OTP targets",
        )),
        Some(FillSafety::Ineligible) | None => Err(BrowserFailure::operation(
            "browser_fill_target_ineligible",
            "element_ref",
            "element_ref no longer resolves to an admitted text/search input",
        )),
    }
}

fn absent_target_failure(attached_but_hidden: bool) -> BrowserFailure {
    BrowserFailure::operation(
        if attached_but_hidden {
            "browser_element_ref_hidden"
        } else {
            "browser_element_ref_detached"
        },
        "element_ref",
        "element_ref no longer resolves to one visible semantic target",
    )
}

async fn execute_live_click(
    session: &mut LiveBrowserSession,
    run_id: &str,
    request: &BrowserClickRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    if session.run_id != run_id {
        return click_operation_outcome(
            &request.element_ref,
            BrowserFailure::operation(
                "browser_element_ref_cross_run",
                "element_ref",
                "element_ref belongs to a different Run",
            ),
            None,
            false,
        );
    }
    let fresh =
        match tokio::time::timeout(OVERALL_DEADLINE, capture_current_observation(session)).await {
            Ok(Ok(observation)) => observation,
            Ok(Err(failure)) => {
                return click_operation_outcome(&request.element_ref, failure, None, false);
            }
            Err(_) => {
                return click_operation_outcome(
                    &request.element_ref,
                    BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        "browser_click pre-action observation exceeded its deadline",
                    ),
                    None,
                    false,
                );
            }
        };
    if cancellation.is_cancelled() {
        return invalidate_with_failure(
            session,
            request,
            fresh,
            BrowserFailure {
                code: "browser_cancelled",
                stage: "cancellation",
                message: "browser_click was cancelled before dispatch".to_owned(),
                transport: false,
                retry: ToolRetryDisposition::Safe,
            },
        )
        .await;
    }
    let target = match resolve_element_ref(
        &session.run_id,
        run_id,
        &session.refs,
        &session.stale_refs,
        &request.element_ref,
    ) {
        Ok(target) => target,
        Err(failure) => {
            return invalidate_with_failure(session, request, fresh, failure).await;
        }
    };
    let current = match validate_click_target(&target, &fresh.nodes) {
        Ok(current) => current,
        Err(failure) => {
            return invalidate_with_failure(session, request, fresh, failure).await;
        }
    };
    let Some(_current) = current else {
        let described = session
            .cdp
            .call(
                "DOM.describeNode",
                json!({"backendNodeId":target.backend_dom_node_id}),
                Some(&session.cdp_session_id),
            )
            .await;
        return invalidate_with_failure(
            session,
            request,
            fresh,
            absent_target_failure(described.is_ok()),
        )
        .await;
    };
    let fresh_fingerprint = observation_fingerprint(&fresh.nodes);
    if fresh_fingerprint != session.observation_fingerprint {
        return invalidate_with_failure(
            session,
            request,
            fresh,
            BrowserFailure::operation(
                "browser_element_ref_stale_snapshot",
                "element_ref",
                "page semantics changed after the latest Host observation",
            ),
        )
        .await;
    }
    let model = match session
        .cdp
        .call(
            "DOM.getBoxModel",
            json!({"backendNodeId":target.backend_dom_node_id}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        Ok(model) => model,
        Err(_) => {
            return invalidate_with_failure(
                session,
                request,
                fresh,
                BrowserFailure::operation(
                    "browser_element_ref_detached",
                    "element_ref",
                    "element_ref detached before action dispatch",
                ),
            )
            .await;
        }
    };
    let Some((x, y)) = box_center(&model) else {
        return invalidate_with_failure(
            session,
            request,
            fresh,
            BrowserFailure::operation(
                "browser_element_ref_hidden",
                "element_ref",
                "element_ref has no visible non-zero layout box",
            ),
        )
        .await;
    };
    session.egress.begin_action();
    let press = session
        .cdp
        .call(
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
            Some(&session.cdp_session_id),
        )
        .await;
    if let Err(failure) = press {
        session.egress.end_action();
        return click_operation_outcome(&request.element_ref, failure, None, false);
    }
    let release = session
        .cdp
        .call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
            Some(&session.cdp_session_id),
        )
        .await;
    if let Err(failure) = release {
        session.egress.end_action();
        return click_operation_outcome(&request.element_ref, failure, None, true);
    }
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    let post =
        match tokio::time::timeout(OVERALL_DEADLINE, capture_current_observation(session)).await {
            Ok(Ok(observation)) => observation,
            Ok(Err(failure)) => {
                session.egress.end_action();
                return click_operation_outcome(&request.element_ref, failure, None, true);
            }
            Err(_) => {
                session.egress.end_action();
                return click_operation_outcome(
                    &request.element_ref,
                    BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        "browser_click post-action observation exceeded its deadline",
                    ),
                    None,
                    true,
                );
            }
        };
    session.egress.end_action();
    session.observation = post;
    rotate_observation(session, true);
    let action = Some(BrowserActionResult {
        kind: "click",
        consumed_element_ref: request.element_ref.clone(),
    });
    if let Some(failure) = session.egress.take_fatal_failure() {
        return click_failure_with_fresh_observation(session, request, failure, true);
    }
    let _ = session
        .cdp
        .call(
            "Network.clearBrowserCookies",
            json!({}),
            Some(&session.cdp_session_id),
        )
        .await;
    let mut outcome = browser_result_outcome(session, action, false);
    outcome.side_effect = ToolSideEffectStatus::Applied;
    outcome
}

fn fill_failure_with_fresh_observation(
    session: &LiveBrowserSession,
    request: &BrowserFillRequest,
    failure: BrowserFailure,
    dispatched: bool,
) -> ToolOutcome {
    let observation = browser_result_value(
        session,
        Some(BrowserActionResult {
            kind: "fill",
            consumed_element_ref: request.element_ref.clone(),
        }),
        false,
    );
    let content = json!({
        "failure": {
            "code": failure.code,
            "stage": failure.stage,
            "message": failure.message,
        },
        "fresh_observation": observation,
        "trust": TRUST,
    })
    .to_string();
    let mut outcome =
        ToolOutcome::error(content).with_failure_code(ToolFailureCode::OperationFailed);
    outcome.transport = if failure.transport {
        ToolTransportStatus::Failed
    } else {
        ToolTransportStatus::Succeeded
    };
    outcome.operation = ToolOperationStatus::Failed;
    outcome.side_effect = if dispatched {
        ToolSideEffectStatus::Applied
    } else {
        ToolSideEffectStatus::NotApplied
    };
    outcome.retry = if dispatched {
        ToolRetryDisposition::NotRetryable
    } else {
        failure.retry
    };
    outcome.metadata = Some(action_failure_metadata(
        &request.element_ref,
        "fill",
        &failure,
        None,
    ));
    outcome
}

async fn execute_live_fill(
    session: &mut LiveBrowserSession,
    run_id: &str,
    request: &BrowserFillRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    let fresh =
        match tokio::time::timeout(OVERALL_DEADLINE, capture_current_observation(session)).await {
            Ok(Ok(observation)) => observation,
            Ok(Err(failure)) => {
                return fill_operation_outcome(&request.element_ref, failure, None, false);
            }
            Err(_) => {
                return fill_operation_outcome(
                    &request.element_ref,
                    BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        "browser_fill pre-action observation exceeded its deadline",
                    ),
                    None,
                    false,
                );
            }
        };
    if cancellation.is_cancelled() {
        return invalidate_fill_with_failure(
            session,
            request,
            fresh,
            BrowserFailure {
                code: "browser_cancelled",
                stage: "cancellation",
                message: "browser_fill was cancelled before dispatch".to_owned(),
                transport: false,
                retry: ToolRetryDisposition::Safe,
            },
        )
        .await;
    }
    let target = match resolve_element_ref(
        &session.run_id,
        run_id,
        &session.refs,
        &session.stale_refs,
        &request.element_ref,
    ) {
        Ok(target) => target,
        Err(failure) => {
            return invalidate_fill_with_failure(session, request, fresh, failure).await;
        }
    };
    let current = match validate_fill_target(&target, &fresh.nodes) {
        Ok(current) => current,
        Err(failure) => {
            return invalidate_fill_with_failure(session, request, fresh, failure).await;
        }
    };
    let Some(_current) = current else {
        let described = session
            .cdp
            .call(
                "DOM.describeNode",
                json!({"backendNodeId":target.backend_dom_node_id}),
                Some(&session.cdp_session_id),
            )
            .await;
        return invalidate_fill_with_failure(
            session,
            request,
            fresh,
            absent_target_failure(described.is_ok()),
        )
        .await;
    };
    let fresh_fingerprint = observation_fingerprint(&fresh.nodes);
    if fresh_fingerprint != session.observation_fingerprint {
        return invalidate_fill_with_failure(
            session,
            request,
            fresh,
            BrowserFailure::operation(
                "browser_element_ref_stale_snapshot",
                "element_ref",
                "page semantics changed after the latest Host observation",
            ),
        )
        .await;
    }
    let model = match session
        .cdp
        .call(
            "DOM.getBoxModel",
            json!({"backendNodeId":target.backend_dom_node_id}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        Ok(model) => model,
        Err(_) => {
            return invalidate_fill_with_failure(
                session,
                request,
                fresh,
                BrowserFailure::operation(
                    "browser_element_ref_detached",
                    "element_ref",
                    "element_ref detached before fill dispatch",
                ),
            )
            .await;
        }
    };
    if box_center(&model).is_none() {
        return invalidate_fill_with_failure(
            session,
            request,
            fresh,
            BrowserFailure::operation(
                "browser_element_ref_hidden",
                "element_ref",
                "element_ref has no visible non-zero layout box",
            ),
        )
        .await;
    }

    session.egress.begin_action();
    let focus = session
        .cdp
        .call(
            "DOM.focus",
            json!({"backendNodeId":target.backend_dom_node_id}),
            Some(&session.cdp_session_id),
        )
        .await;
    if let Err(failure) = focus {
        session.egress.end_action();
        return fill_operation_outcome(&request.element_ref, failure, None, true);
    }
    for (event_type, key, code, modifiers, key_code) in [
        ("rawKeyDown", "a", "KeyA", SELECT_ALL_MODIFIERS, 65),
        ("keyUp", "a", "KeyA", SELECT_ALL_MODIFIERS, 65),
        ("rawKeyDown", "Backspace", "Backspace", 0, 8),
        ("keyUp", "Backspace", "Backspace", 0, 8),
    ] {
        let dispatched = session
            .cdp
            .call(
                "Input.dispatchKeyEvent",
                json!({
                    "type":event_type,
                    "key":key,
                    "code":code,
                    "modifiers":modifiers,
                    "windowsVirtualKeyCode":key_code,
                    "nativeVirtualKeyCode":key_code,
                }),
                Some(&session.cdp_session_id),
            )
            .await;
        if let Err(failure) = dispatched {
            session.egress.end_action();
            return fill_operation_outcome(&request.element_ref, failure, None, true);
        }
    }
    if let Err(failure) = session
        .cdp
        .call(
            "Input.insertText",
            json!({"text":request.value}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        session.egress.end_action();
        return fill_operation_outcome(&request.element_ref, failure, None, true);
    }
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    let post =
        match tokio::time::timeout(OVERALL_DEADLINE, capture_current_observation(session)).await {
            Ok(Ok(observation)) => observation,
            Ok(Err(failure)) => {
                session.egress.end_action();
                return fill_operation_outcome(&request.element_ref, failure, None, true);
            }
            Err(_) => {
                session.egress.end_action();
                return fill_operation_outcome(
                    &request.element_ref,
                    BrowserFailure::transport(
                        "browser_deadline_exceeded",
                        "deadline",
                        "browser_fill post-action observation exceeded its deadline",
                    ),
                    None,
                    true,
                );
            }
        };
    session.egress.end_action();
    session.observation = post;
    rotate_observation(session, true);
    let action = Some(BrowserActionResult {
        kind: "fill",
        consumed_element_ref: request.element_ref.clone(),
    });
    if let Some(failure) = session.egress.take_fatal_failure() {
        return fill_failure_with_fresh_observation(session, request, failure, true);
    }
    let _ = session
        .cdp
        .call(
            "Network.clearBrowserCookies",
            json!({}),
            Some(&session.cdp_session_id),
        )
        .await;
    let mut outcome = browser_result_outcome(session, action, false);
    outcome.side_effect = ToolSideEffectStatus::Applied;
    outcome
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
) -> Result<(CdpClient, String, SessionObservation), BrowserFailure> {
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
        json!({"behavior":"deny","eventsEnabled":true}),
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
    let observation = SessionObservation {
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
    };
    Ok((cdp, session_id, observation))
}

async fn wait_for_devtools_active_port(profile: &Path) -> Result<(u16, String), BrowserFailure> {
    let path = profile.join("DevToolsActivePort");
    tokio::time::timeout(PROFILE_DISCOVERY_DEADLINE, async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                let mut lines = text.lines();
                let Some(port_text) = lines.next() else {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                };
                let port = port_text.parse::<u16>().map_err(|error| {
                    BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        format!("DevToolsActivePort port is invalid: {error}"),
                    )
                })?;
                let Some(websocket) = lines.next() else {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                };
                if !websocket.starts_with("/devtools/browser/") || websocket.contains(['\r', '\n'])
                {
                    return Err(BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        "DevToolsActivePort contains an invalid WebSocket path",
                    ));
                }
                if lines.next().is_some() {
                    return Err(BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        "DevToolsActivePort contains unexpected extra identity fields",
                    ));
                }
                if port == 0 {
                    return Err(BrowserFailure::operation(
                        "browser_cdp_identity_invalid",
                        "process",
                        "DevToolsActivePort contains port zero",
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
                    | "data-channel"
                    | "data-filter"
                    | "aria-checked"
                    | "aria-selected"
                    | "aria-expanded"
                    | "aria-disabled"
                    | "aria-busy"
                    | "aria-pressed"
                    | "type"
                    | "disabled"
                    | "readonly"
                    | "hidden"
                    | "name"
                    | "id"
                    | "placeholder"
                    | "autocomplete"
                    | "aria-label"
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
        let backend_dom_node_id = node.get("backendDOMNodeId").and_then(Value::as_u64);
        let dom = backend_dom_node_id.and_then(|id| by_backend_id.get(&id));
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
                if name.starts_with("data-") || name.starts_with("aria-") {
                    state.insert(name.clone(), value.clone());
                }
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
        let click_safety = if matches!(role.to_ascii_lowercase().as_str(), "button" | "switch") {
            Some(
                if state.iter().any(|(name, value)| {
                    matches!(name.as_str(), "disabled" | "aria-disabled")
                        && value.eq_ignore_ascii_case("true")
                }) {
                    ClickSafety::Disabled
                } else {
                    ClickSafety::Allowed
                },
            )
        } else if matches!(role.to_ascii_lowercase().as_str(), "link") {
            Some(ClickSafety::SideEffectDenied)
        } else {
            None
        };
        let fill_safety = classify_fill_safety(&role, &accessible_name, dom, &state);
        eligible.push(SemanticNode {
            element_ref: None,
            role: bounded_text(&role, 512),
            accessible_name: bounded_text(&accessible_name, 1_024),
            text,
            value: bounded_text(&value, 1_024),
            state,
            backend_dom_node_id,
            click_safety,
            fill_safety,
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

fn classify_fill_safety(
    role: &str,
    accessible_name: &str,
    dom: Option<&DomFacts>,
    state: &BTreeMap<String, String>,
) -> Option<FillSafety> {
    if !matches!(role.to_ascii_lowercase().as_str(), "textbox" | "searchbox") {
        return None;
    }
    let Some(dom) = dom else {
        return Some(FillSafety::Ineligible);
    };
    if dom.node_name != "INPUT" {
        return Some(FillSafety::Ineligible);
    }
    let input_type = dom
        .attributes
        .get("type")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "text".to_owned());
    if !matches!(input_type.as_str(), "text" | "search") {
        return Some(if input_type == "password" {
            FillSafety::SensitiveDenied
        } else {
            FillSafety::Ineligible
        });
    }
    if dom.attributes.contains_key("disabled")
        || state.iter().any(|(name, value)| {
            matches!(name.as_str(), "disabled" | "aria-disabled")
                && value.eq_ignore_ascii_case("true")
        })
    {
        return Some(FillSafety::Disabled);
    }
    if dom.attributes.contains_key("readonly")
        || state
            .get("readonly")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return Some(FillSafety::ReadOnly);
    }
    if dom.attributes.contains_key("hidden") {
        return Some(FillSafety::Ineligible);
    }
    let mut identity = accessible_name.to_ascii_lowercase();
    for name in ["name", "id", "placeholder", "autocomplete", "aria-label"] {
        if let Some(value) = dom.attributes.get(name) {
            identity.push(' ');
            identity.push_str(&value.to_ascii_lowercase());
        }
    }
    let normalized = identity.replace(['-', '_'], " ");
    if [
        "password",
        "passcode",
        "secret",
        "token",
        "api key",
        "credential",
        "otp",
        "one time code",
        "verification code",
        "login",
        "sign in",
        "username",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        return Some(FillSafety::SensitiveDenied);
    }
    Some(FillSafety::Allowed)
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

    fn semantic_target(
        backend_dom_node_id: u64,
        role: &str,
        name: &str,
        safety: ClickSafety,
    ) -> (ElementTarget, SemanticNode) {
        (
            ElementTarget {
                backend_dom_node_id,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                capability: ElementCapability::Click(safety.clone()),
            },
            SemanticNode {
                element_ref: None,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                text: name.to_owned(),
                value: String::new(),
                state: BTreeMap::new(),
                backend_dom_node_id: Some(backend_dom_node_id),
                click_safety: Some(safety),
                fill_safety: None,
            },
        )
    }

    fn fill_target(
        backend_dom_node_id: u64,
        role: &str,
        name: &str,
        safety: FillSafety,
    ) -> (ElementTarget, SemanticNode) {
        (
            ElementTarget {
                backend_dom_node_id,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                capability: ElementCapability::Fill,
            },
            SemanticNode {
                element_ref: None,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                text: name.to_owned(),
                value: String::new(),
                state: BTreeMap::new(),
                backend_dom_node_id: Some(backend_dom_node_id),
                click_safety: None,
                fill_safety: Some(safety),
            },
        )
    }

    #[test]
    fn click_input_and_ref_registry_fail_closed_before_dispatch() {
        for input in [
            json!({}),
            json!({"element_ref":""}),
            json!({"element_ref":"button-1"}),
            json!({"element_ref":"eref_not_hex"}),
            json!({"element_ref":format!("eref_{}", "a".repeat(40))}),
        ] {
            assert!(parse_click_request(&input).is_err(), "{input}");
        }
        let valid = parse_click_request(&json!({
            "element_ref":"eref_0123456789abcdef0123456789abcdef"
        }))
        .expect("opaque Host ref");
        assert_eq!(valid.element_ref.len(), 37);

        let (target, _) = semantic_target(7, "button", "Deploy", ClickSafety::Allowed);
        let refs = HashMap::from([("eref_0123456789abcdef0123456789abcdef".to_owned(), target)]);
        let stale = BTreeSet::from(["eref_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()]);
        for (caller_run, element_ref, expected) in [
            (
                "other-run",
                "eref_0123456789abcdef0123456789abcdef",
                "browser_element_ref_cross_run",
            ),
            (
                "run-1",
                "eref_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "browser_element_ref_stale",
            ),
            (
                "run-1",
                "eref_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "browser_element_ref_missing",
            ),
        ] {
            assert_eq!(
                resolve_element_ref("run-1", caller_run, &refs, &stale, element_ref)
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }

    #[test]
    fn fill_value_and_ref_preflight_is_bounded_and_control_free() {
        let element_ref = "eref_0123456789abcdef0123456789abcdef";
        let valid = parse_fill_request(&json!({
            "element_ref":element_ref,
            "value":"canary"
        }))
        .expect("bounded fill request");
        assert_eq!(valid.element_ref(), element_ref);
        assert_eq!(valid.value(), "canary");
        assert!(
            parse_fill_request(&json!({
                "element_ref":element_ref,
                "value":"🦀".repeat(MAX_FILL_VALUE_CHARS)
            }))
            .is_ok(),
            "the exact 1024-char/4096-byte UTF-8 boundary is admitted"
        );

        for (value, expected) in [
            (String::new(), "browser_fill_value_invalid"),
            (
                "a".repeat(MAX_FILL_VALUE_CHARS + 1),
                "browser_fill_value_invalid",
            ),
            ("line\nfeed".to_owned(), "browser_fill_value_control_denied"),
            ("nul\0byte".to_owned(), "browser_fill_value_control_denied"),
            (
                format!("c1{}", '\u{0085}'),
                "browser_fill_value_control_denied",
            ),
        ] {
            assert_eq!(
                parse_fill_request(&json!({"element_ref":element_ref,"value":value}))
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }

    #[test]
    fn current_target_matrix_distinguishes_ambiguous_disabled_and_stale_identity() {
        let (allowed, node) = semantic_target(7, "button", "Deploy", ClickSafety::Allowed);
        assert!(
            validate_click_target(&allowed, std::slice::from_ref(&node))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            validate_click_target(&allowed, &[node.clone(), node.clone()])
                .unwrap_err()
                .code,
            "browser_element_ref_ambiguous"
        );
        assert!(validate_click_target(&allowed, &[]).unwrap().is_none());

        let (_, mut renamed) = semantic_target(7, "button", "Other", ClickSafety::Allowed);
        renamed.backend_dom_node_id = Some(7);
        assert_eq!(
            validate_click_target(&allowed, &[renamed])
                .unwrap_err()
                .code,
            "browser_element_ref_stale_target"
        );
        let (disabled, disabled_node) =
            semantic_target(8, "button", "Deploy", ClickSafety::Disabled);
        assert_eq!(
            validate_click_target(&disabled, &[disabled_node])
                .unwrap_err()
                .code,
            "browser_element_ref_disabled"
        );
        let (link, link_node) =
            semantic_target(9, "link", "External", ClickSafety::SideEffectDenied);
        assert_eq!(
            validate_click_target(&link, &[link_node]).unwrap_err().code,
            "browser_element_ref_side_effect_denied"
        );
        assert_eq!(
            absent_target_failure(true).code,
            "browser_element_ref_hidden"
        );
        assert_eq!(
            absent_target_failure(false).code,
            "browser_element_ref_detached"
        );
    }

    #[test]
    fn fill_target_and_cross_capability_matrix_fail_closed() {
        let (fill, allowed) = fill_target(11, "textbox", "Release channel", FillSafety::Allowed);
        assert!(
            validate_fill_target(&fill, std::slice::from_ref(&allowed))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            validate_click_target(&fill, std::slice::from_ref(&allowed))
                .unwrap_err()
                .code,
            "browser_element_ref_capability_mismatch"
        );
        let (click, click_node) = semantic_target(12, "button", "Deploy", ClickSafety::Allowed);
        assert_eq!(
            validate_fill_target(&click, &[click_node])
                .unwrap_err()
                .code,
            "browser_element_ref_capability_mismatch"
        );
        for (safety, expected) in [
            (FillSafety::Disabled, "browser_element_ref_disabled"),
            (FillSafety::ReadOnly, "browser_element_ref_readonly"),
            (
                FillSafety::SensitiveDenied,
                "browser_fill_sensitive_target_denied",
            ),
            (FillSafety::Ineligible, "browser_fill_target_ineligible"),
        ] {
            let (target, node) = fill_target(13, "textbox", "Release channel", safety);
            assert_eq!(
                validate_fill_target(&target, &[node]).unwrap_err().code,
                expected
            );
        }
    }

    #[test]
    fn only_non_sensitive_text_and_search_inputs_are_fill_eligible() {
        let state = BTreeMap::new();
        let facts = |node_name: &str, attributes: &[(&str, &str)]| DomFacts {
            node_name: node_name.to_owned(),
            node_value: String::new(),
            attributes: attributes
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        };
        assert_eq!(
            classify_fill_safety(
                "textbox",
                "Release channel",
                Some(&facts("INPUT", &[("type", "text")])),
                &state,
            ),
            Some(FillSafety::Allowed)
        );
        assert_eq!(
            classify_fill_safety(
                "searchbox",
                "Test filter",
                Some(&facts("INPUT", &[("type", "search")])),
                &state,
            ),
            Some(FillSafety::Allowed)
        );
        for (role, name, dom, expected) in [
            (
                "textbox",
                "Notes",
                facts("TEXTAREA", &[]),
                FillSafety::Ineligible,
            ),
            (
                "textbox",
                "Password",
                facts("INPUT", &[("type", "password")]),
                FillSafety::SensitiveDenied,
            ),
            (
                "textbox",
                "API key",
                facts("INPUT", &[("type", "text")]),
                FillSafety::SensitiveDenied,
            ),
            (
                "textbox",
                "Value",
                facts("INPUT", &[("type", "file")]),
                FillSafety::Ineligible,
            ),
            (
                "textbox",
                "Value",
                facts("INPUT", &[("type", "number")]),
                FillSafety::Ineligible,
            ),
            (
                "textbox",
                "Value",
                facts("INPUT", &[("type", "text"), ("readonly", "")]),
                FillSafety::ReadOnly,
            ),
            (
                "textbox",
                "Value",
                facts("INPUT", &[("type", "text"), ("disabled", "")]),
                FillSafety::Disabled,
            ),
        ] {
            assert_eq!(
                classify_fill_safety(role, name, Some(&dom), &state),
                Some(expected),
                "{role}:{name}:{:?}",
                dom.attributes
            );
        }
    }

    #[test]
    fn layout_box_and_action_denials_are_deterministic() {
        assert_eq!(
            box_center(&json!({"model":{"content":[0.0,0.0,20.0,0.0,20.0,10.0,0.0,10.0]}})),
            Some((10.0, 5.0))
        );
        for model in [
            json!({}),
            json!({"model":{"content":[0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]}}),
            json!({"model":{"content":[0.0,0.0,1.0]}}),
        ] {
            assert_eq!(box_center(&model), None);
        }
        assert_eq!(
            prohibited_browser_event(Some("Browser.downloadWillBegin"))
                .unwrap()
                .code,
            "browser_download_denied"
        );
        assert!(prohibited_browser_event(Some("Page.loadEventFired")).is_none());

        let request = parse_request(
            &input("http://127.0.0.1:32123/"),
            Some("http://127.0.0.1:32123"),
        )
        .unwrap();
        let egress = EgressState::new(&request);
        egress.begin_action();
        egress.record_denial(
            BrowserFailure::operation("browser_additional_target_denied", "target", "popup denied"),
            false,
        );
        assert_eq!(
            egress.take_fatal_failure().unwrap().code,
            "browser_additional_target_denied"
        );
        egress.end_action();
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

    #[tokio::test]
    async fn devtools_identity_waits_for_complete_file_and_rejects_malformed_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("DevToolsActivePort");
        std::fs::write(&path, b"").unwrap();
        let writer = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tokio::fs::write(&writer, b"43111\n/devtools/browser/opaque\n")
                .await
                .unwrap();
        });
        assert_eq!(
            wait_for_devtools_active_port(directory.path())
                .await
                .unwrap(),
            (43111, "/devtools/browser/opaque".to_owned())
        );

        let malformed = tempfile::tempdir().unwrap();
        std::fs::write(
            malformed.path().join("DevToolsActivePort"),
            b"not-a-port\n/devtools/browser/opaque\n",
        )
        .unwrap();
        assert_eq!(
            wait_for_devtools_active_port(malformed.path())
                .await
                .unwrap_err()
                .code,
            "browser_cdp_identity_invalid"
        );
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
    async fn pinned_cft_reads_js_only_ax_dom_and_retains_only_exact_local_session() {
        let (origin, server_cancellation, server, requests) = start_rendered_fixture().await;
        let system = Arc::new(SystemSemanticBrowserHarness::new(Arc::new(NoNetwork)));
        let harness: Arc<dyn SemanticBrowserHarness> = system.clone();
        let outcome = execute_browser_navigate(
            "pinned-cft-rendered",
            input(&format!("{origin}/app")),
            harness.clone(),
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
        assert_eq!(result["session_live"], true);
        assert_eq!(result["session_scope"], "same_run_in_memory_exact_loopback");
        assert_eq!(result["teardown"]["attempted"], false);
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
        harness.shutdown();
        let teardown = system
            .await_last_teardown()
            .await
            .expect("Host teardown facts");
        assert!(teardown.process_tree_settled);
        assert!(teardown.proxy_settled);
        assert!(teardown.profile_removed);
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
            "pinned-cft-cancel",
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
            "public-canary",
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
