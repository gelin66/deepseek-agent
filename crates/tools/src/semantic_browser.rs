//! Ephemeral semantic browser backed by pinned Chrome for Testing.
//!
//! Public and exact Host-owned loopback navigation retain one bounded,
//! in-memory session long enough for the same run to consume opaque
//! latest-snapshot refs through the typed `browser_interact` action surface.
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
const ADAPTER_ID: &str = "direct_tokio_cdp_semantic_interaction_v4";
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
const MAX_INTERACTION_VALUE_CHARS: usize = 2_048;
const MAX_WAIT_MILLIS: u64 = 10_000;
const DEFAULT_WAIT_MILLIS: u64 = 3_000;
const MAX_SCROLL_AMOUNT: u64 = 2_000;
#[cfg(target_os = "macos")]
const SELECT_ALL_MODIFIERS: u8 = 4;
#[cfg(not(target_os = "macos"))]
const SELECT_ALL_MODIFIERS: u8 = 2;
const MAX_STALE_REFS: usize = MAX_MAX_NODES * 2;
const MAX_BROWSER_PAGES: usize = 3;

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
                format!("browser_navigate 只允许 HTTP(S) URL；收到 scheme={scheme}"),
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

/// Canonical typed interaction request. This is intentionally an enum rather
/// than a model-provided script or selector language.
#[derive(Debug, Clone)]
pub enum BrowserInteractRequest {
    Click(BrowserClickRequest),
    Fill(BrowserFillRequest),
    Press {
        element_ref: String,
        key: BrowserKey,
    },
    Wait {
        condition: BrowserWaitCondition,
        value: Option<String>,
        timeout: Duration,
    },
    Scroll {
        element_ref: String,
        direction: BrowserScrollDirection,
        amount: u64,
    },
    Select {
        element_ref: String,
        value: String,
    },
    Back,
    TabOpen {
        url: Url,
    },
    TabSwitch {
        page_ref: String,
    },
    TabClose {
        page_ref: String,
    },
    Submit(BrowserClickRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKey {
    Enter,
    Escape,
    Tab,
    ArrowUp,
    ArrowDown,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserWaitCondition {
    DocumentReady,
    TextPresent,
    TextAbsent,
    UrlEquals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserScrollDirection {
    Up,
    Down,
}

/// Host-derived information used only to create a durable authorization
/// prompt. Page text cannot choose the permission mode or broaden this scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserAuthorizationPreview {
    pub public: bool,
    pub origin: String,
    pub target: String,
    pub parameters: String,
    pub impact: String,
    pub external_side_effect: bool,
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

    async fn interact(
        &self,
        run_id: &str,
        request: BrowserInteractRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        match request {
            BrowserInteractRequest::Click(request) => {
                self.click(run_id, request, cancellation).await
            }
            BrowserInteractRequest::Fill(request) => self.fill(run_id, request, cancellation).await,
            _ => ToolOutcome::error(
                "semantic browser fixture does not implement this typed interaction",
            )
            .with_failure_code(ToolFailureCode::OperationFailed)
            .with_side_effect(ToolSideEffectStatus::NotApplied),
        }
    }

    /// Return the exact current public-session target used by Host
    /// authorization. The system harness derives this from memory-only
    /// session state; deterministic fixtures may leave it unavailable.
    fn authorization_preview(
        &self,
        _run_id: &str,
        _request: &BrowserInteractRequest,
    ) -> Option<BrowserAuthorizationPreview> {
        None
    }

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

/// Default bounded live-session system harness. It never downloads or upgrades Chrome.
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

    fn public_authorization_preview(
        &self,
        run_id: &str,
        request: &BrowserInteractRequest,
    ) -> Option<BrowserAuthorizationPreview> {
        let session = self.session.lock().ok()?;
        let session = session.as_ref()?;
        if session.run_id != run_id || !matches!(session.request.scope, BrowserTargetScope::Public)
        {
            return None;
        }
        let origin = session.egress.initial_origin.canonical();
        let current = session.observation.final_url.clone();
        let ref_target = |element_ref: &str, capability: ElementCapability| {
            let target = session.refs.get(element_ref)?;
            target.capabilities.contains(&capability).then_some(target)
        };
        let preview = match request {
            BrowserInteractRequest::Click(request) => {
                let target = ref_target(request.element_ref(), ElementCapability::Click)?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin,
                    target: format!("{}#{}:{}", current, target.role, target.accessible_name),
                    parameters: "action=click".to_owned(),
                    impact: "reversible_ephemeral_page_interaction; same-origin GET/HEAD only"
                        .to_owned(),
                    external_side_effect: false,
                }
            }
            BrowserInteractRequest::Fill(request) => {
                let target = ref_target(request.element_ref(), ElementCapability::Fill)?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin,
                    target: format!("{}#{}:{}", current, target.role, target.accessible_name),
                    parameters: format!("value={:?}", request.value()),
                    impact: "ephemeral_page_state_only; submit not implied".to_owned(),
                    external_side_effect: false,
                }
            }
            BrowserInteractRequest::Press { element_ref, key } => {
                let target = ref_target(element_ref, ElementCapability::Press)?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin,
                    target: format!("{}#{}:{}", current, target.role, target.accessible_name),
                    parameters: format!("key={}", browser_key_name(*key)),
                    impact: "bounded keyboard interaction; non-granted POST remains blocked"
                        .to_owned(),
                    external_side_effect: false,
                }
            }
            BrowserInteractRequest::Wait {
                condition, value, ..
            } => BrowserAuthorizationPreview {
                public: true,
                origin,
                target: current,
                parameters: format!(
                    "condition={}; value={:?}",
                    wait_condition_name(*condition),
                    value
                ),
                impact: "read-only bounded wait".to_owned(),
                external_side_effect: false,
            },
            BrowserInteractRequest::Scroll {
                element_ref,
                direction,
                amount,
            } => {
                let target = ref_target(element_ref, ElementCapability::Scroll)?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin,
                    target: format!("{}#{}:{}", current, target.role, target.accessible_name),
                    parameters: format!(
                        "direction={}; amount={amount}",
                        scroll_direction_name(*direction)
                    ),
                    impact: "ephemeral viewport movement".to_owned(),
                    external_side_effect: false,
                }
            }
            BrowserInteractRequest::Select { element_ref, value } => {
                let target = ref_target(element_ref, ElementCapability::Select)?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin,
                    target: format!("{}#{}:{}", current, target.role, target.accessible_name),
                    parameters: format!("value={value:?}"),
                    impact: "ephemeral selection; non-granted POST remains blocked".to_owned(),
                    external_side_effect: false,
                }
            }
            BrowserInteractRequest::Back => BrowserAuthorizationPreview {
                public: true,
                origin,
                target: current,
                parameters: "action=back".to_owned(),
                impact: "same-origin history navigation".to_owned(),
                external_side_effect: false,
            },
            BrowserInteractRequest::TabOpen { url } => BrowserAuthorizationPreview {
                public: true,
                origin,
                target: url.to_string(),
                parameters: "action=tab_open".to_owned(),
                impact: "open one bounded same-origin page in the ephemeral session".to_owned(),
                external_side_effect: false,
            },
            BrowserInteractRequest::TabSwitch { page_ref } => BrowserAuthorizationPreview {
                public: true,
                origin,
                target: page_ref.clone(),
                parameters: "action=tab_switch".to_owned(),
                impact: "switch active ephemeral page".to_owned(),
                external_side_effect: false,
            },
            BrowserInteractRequest::TabClose { page_ref } => BrowserAuthorizationPreview {
                public: true,
                origin,
                target: page_ref.clone(),
                parameters: "action=tab_close".to_owned(),
                impact: "close one ephemeral page".to_owned(),
                external_side_effect: false,
            },
            BrowserInteractRequest::Submit(request) => {
                let target = ref_target(request.element_ref(), ElementCapability::Submit)?;
                let external = target.external_preview.as_ref()?;
                BrowserAuthorizationPreview {
                    public: true,
                    origin: external.origin.clone(),
                    target: format!("POST {}", external.target_url),
                    parameters: format!(
                        "{}; {}",
                        serde_json::to_string(&external.parameters).ok()?,
                        external.parameters_sha256
                    ),
                    impact: external.impact.to_owned(),
                    external_side_effect: true,
                }
            }
        };
        Some(preview)
    }

    async fn run_extended_interaction(
        &self,
        run_id: &str,
        request: BrowserInteractRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        let _guard = match self.begin_operation() {
            Ok(guard) => guard,
            Err(failure) => return interact_operation_outcome(failure, false),
        };
        let Some(mut session) = self.session.lock().expect("browser session lock").take() else {
            return interact_operation_outcome(
                BrowserFailure::operation(
                    "browser_session_missing",
                    "session",
                    "browser_interact requires a live browser_navigate observation",
                ),
                false,
            );
        };
        enum Completion {
            Finished(Box<ToolOutcome>),
            Cancelled,
            TimedOut,
        }
        let completion = {
            let execution = execute_live_extended_interaction(
                &mut session,
                run_id,
                &request,
                cancellation.clone(),
            );
            tokio::pin!(execution);
            tokio::select! {
                outcome = &mut execution => Completion::Finished(Box::new(outcome)),
                () = cancellation.cancelled() => Completion::Cancelled,
                () = tokio::time::sleep(OVERALL_DEADLINE) => Completion::TimedOut,
            }
        };
        match completion {
            Completion::Finished(outcome) => {
                let mut outcome = *outcome;
                if outcome.side_effect == ToolSideEffectStatus::Indeterminate {
                    let teardown = session.teardown().await;
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
                } else {
                    *self.session.lock().expect("browser session lock") = Some(session);
                }
                outcome
            }
            Completion::Cancelled | Completion::TimedOut => {
                let dispatched = session.egress.action_started.load(Ordering::SeqCst);
                session.egress.end_action();
                let failure = match completion {
                    Completion::Cancelled => BrowserFailure {
                        code: "browser_cancelled",
                        stage: "cancellation",
                        message: "browser_interact was cancelled".to_owned(),
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
                        format!(
                            "browser_interact exceeded {} ms",
                            OVERALL_DEADLINE.as_millis()
                        ),
                    ),
                    Completion::Finished(_) => unreachable!(),
                };
                if dispatched {
                    let teardown = session.teardown().await;
                    let mut outcome = interact_operation_outcome(failure, true);
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
                    let outcome = interact_operation_outcome(failure, false);
                    *self.session.lock().expect("browser session lock") = Some(session);
                    outcome
                }
            }
        }
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

fn browser_key_name(key: BrowserKey) -> &'static str {
    match key {
        BrowserKey::Enter => "enter",
        BrowserKey::Escape => "escape",
        BrowserKey::Tab => "tab",
        BrowserKey::ArrowUp => "arrow_up",
        BrowserKey::ArrowDown => "arrow_down",
        BrowserKey::Space => "space",
    }
}

fn wait_condition_name(condition: BrowserWaitCondition) -> &'static str {
    match condition {
        BrowserWaitCondition::DocumentReady => "document_ready",
        BrowserWaitCondition::TextPresent => "text_present",
        BrowserWaitCondition::TextAbsent => "text_absent",
        BrowserWaitCondition::UrlEquals => "url_equals",
    }
}

fn scroll_direction_name(direction: BrowserScrollDirection) -> &'static str {
    match direction {
        BrowserScrollDirection::Up => "up",
        BrowserScrollDirection::Down => "down",
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<&'static str>,
    #[serde(skip)]
    backend_dom_node_id: Option<u64>,
    #[serde(skip)]
    click_safety: Option<ClickSafety>,
    #[serde(skip)]
    fill_safety: Option<FillSafety>,
    #[serde(skip)]
    dom: Option<DomFacts>,
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
    active_page_ref: String,
    pages: Vec<BrowserPageSummary>,
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
struct BrowserPageSummary {
    page_ref: String,
    url: String,
    title: String,
    page_epoch: u64,
    active: bool,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserActionResult {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    consumed_element_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<ExternalActionReceipt>,
}

#[derive(Debug, Clone, Serialize)]
struct ExternalActionReceipt {
    target_url: String,
    method: &'static str,
    status: u16,
    parameters_sha256: String,
    impact: &'static str,
    remote_receipt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_receipt: Option<String>,
    observed_at: String,
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

pub(crate) fn preflight_browser_interact(input: &Value) -> Option<ToolOutcome> {
    match parse_interact_request(input) {
        Ok(_) => None,
        Err(failure) => Some(rejected_interact_outcome(failure)),
    }
}

pub(crate) fn parse_browser_interact_for_authorization(
    input: &Value,
) -> Result<BrowserInteractRequest, String> {
    parse_interact_request(input).map_err(|failure| failure.message)
}

pub(crate) async fn execute_browser_interact(
    run_id: &str,
    input: Value,
    harness: Arc<dyn SemanticBrowserHarness>,
    controlled_network_allowed: bool,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let request = match parse_interact_request(&input) {
        Ok(request) => request,
        Err(failure) => return interact_operation_outcome(failure, false),
    };
    if !controlled_network_allowed {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_network_not_authorized",
                "authorization",
                "当前 actor 的冻结边界禁止浏览器网络访问",
            ),
            false,
        );
    }
    harness.interact(run_id, request, cancellation).await
}

fn parse_interact_request(input: &Value) -> Result<BrowserInteractRequest, BrowserFailure> {
    let action = required_str(input, "action").map_err(|error| {
        BrowserFailure::rejected("browser_action_missing", "action", error.to_string())
    })?;
    let allowed = |fields: &[&str]| {
        input
            .as_object()
            .is_some_and(|object| object.keys().all(|key| fields.contains(&key.as_str())))
    };
    match action {
        "click" if allowed(&["action", "element_ref"]) => {
            Ok(BrowserInteractRequest::Click(BrowserClickRequest {
                element_ref: parse_element_ref(input)?,
            }))
        }
        "fill" if allowed(&["action", "element_ref", "value"]) => {
            Ok(BrowserInteractRequest::Fill(parse_fill_request(input)?))
        }
        "press" if allowed(&["action", "element_ref", "key"]) => {
            let key = match required_str(input, "key").map_err(|error| {
                BrowserFailure::rejected("browser_key_missing", "key", error.to_string())
            })? {
                "enter" => BrowserKey::Enter,
                "escape" => BrowserKey::Escape,
                "tab" => BrowserKey::Tab,
                "arrow_up" => BrowserKey::ArrowUp,
                "arrow_down" => BrowserKey::ArrowDown,
                "space" => BrowserKey::Space,
                _ => {
                    return Err(BrowserFailure::rejected(
                        "browser_key_denied",
                        "key",
                        "key 必须是 enter/escape/tab/arrow_up/arrow_down/space",
                    ));
                }
            };
            Ok(BrowserInteractRequest::Press {
                element_ref: parse_element_ref(input)?,
                key,
            })
        }
        "wait" if allowed(&["action", "condition", "value", "timeout_ms"]) => {
            let condition = match required_str(input, "condition").map_err(|error| {
                BrowserFailure::rejected(
                    "browser_wait_condition_missing",
                    "condition",
                    error.to_string(),
                )
            })? {
                "document_ready" => BrowserWaitCondition::DocumentReady,
                "text_present" => BrowserWaitCondition::TextPresent,
                "text_absent" => BrowserWaitCondition::TextAbsent,
                "url_equals" => BrowserWaitCondition::UrlEquals,
                _ => {
                    return Err(BrowserFailure::rejected(
                        "browser_wait_condition_denied",
                        "condition",
                        "condition 必须是 document_ready/text_present/text_absent/url_equals",
                    ));
                }
            };
            let value = input
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if condition == BrowserWaitCondition::DocumentReady {
                if value.is_some() {
                    return Err(BrowserFailure::rejected(
                        "browser_wait_value_unexpected",
                        "value",
                        "document_ready 不接受 value",
                    ));
                }
            } else if value.as_deref().is_none_or(str::is_empty) {
                return Err(BrowserFailure::rejected(
                    "browser_wait_value_missing",
                    "value",
                    "该 wait condition 需要非空 value",
                ));
            }
            if value
                .as_ref()
                .is_some_and(|value| value.chars().count() > MAX_INTERACTION_VALUE_CHARS)
            {
                return Err(BrowserFailure::rejected(
                    "browser_wait_value_too_large",
                    "value",
                    format!("wait value 不得超过 {MAX_INTERACTION_VALUE_CHARS} 字符"),
                ));
            }
            let timeout_ms = input
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_WAIT_MILLIS);
            if timeout_ms == 0 || timeout_ms > MAX_WAIT_MILLIS {
                return Err(BrowserFailure::rejected(
                    "browser_wait_timeout_invalid",
                    "timeout_ms",
                    format!("timeout_ms 必须为 1..={MAX_WAIT_MILLIS}"),
                ));
            }
            Ok(BrowserInteractRequest::Wait {
                condition,
                value,
                timeout: Duration::from_millis(timeout_ms),
            })
        }
        "scroll" if allowed(&["action", "element_ref", "direction", "amount"]) => {
            let direction = match required_str(input, "direction").map_err(|error| {
                BrowserFailure::rejected(
                    "browser_scroll_direction_missing",
                    "direction",
                    error.to_string(),
                )
            })? {
                "up" => BrowserScrollDirection::Up,
                "down" => BrowserScrollDirection::Down,
                _ => {
                    return Err(BrowserFailure::rejected(
                        "browser_scroll_direction_denied",
                        "direction",
                        "direction 必须是 up/down",
                    ));
                }
            };
            let amount = input.get("amount").and_then(Value::as_u64).unwrap_or(600);
            if amount == 0 || amount > MAX_SCROLL_AMOUNT {
                return Err(BrowserFailure::rejected(
                    "browser_scroll_amount_invalid",
                    "amount",
                    format!("amount 必须为 1..={MAX_SCROLL_AMOUNT}"),
                ));
            }
            Ok(BrowserInteractRequest::Scroll {
                element_ref: parse_element_ref(input)?,
                direction,
                amount,
            })
        }
        "select" if allowed(&["action", "element_ref", "value"]) => {
            let value = parse_interaction_value(input, "browser_select_value_invalid")?;
            Ok(BrowserInteractRequest::Select {
                element_ref: parse_element_ref(input)?,
                value,
            })
        }
        "back" if allowed(&["action"]) => Ok(BrowserInteractRequest::Back),
        "tab_open" if allowed(&["action", "url"]) => {
            let raw = required_str(input, "url").map_err(|error| {
                BrowserFailure::rejected("browser_tab_url_missing", "url", error.to_string())
            })?;
            let url = Url::parse(raw).map_err(|error| {
                BrowserFailure::rejected(
                    "browser_tab_url_invalid",
                    "url",
                    format!("tab URL 解析失败：{error}"),
                )
            })?;
            Origin::from_url(&url)?;
            Ok(BrowserInteractRequest::TabOpen { url })
        }
        "tab_switch" if allowed(&["action", "page_ref"]) => Ok(BrowserInteractRequest::TabSwitch {
            page_ref: parse_page_ref(input)?,
        }),
        "tab_close" if allowed(&["action", "page_ref"]) => Ok(BrowserInteractRequest::TabClose {
            page_ref: parse_page_ref(input)?,
        }),
        "submit" if allowed(&["action", "element_ref"]) => {
            Ok(BrowserInteractRequest::Submit(BrowserClickRequest {
                element_ref: parse_element_ref(input)?,
            }))
        }
        "click" | "fill" | "press" | "wait" | "scroll" | "select" | "back" | "tab_open"
        | "tab_switch" | "tab_close" | "submit" => Err(BrowserFailure::rejected(
            "browser_action_arguments_invalid",
            "arguments",
            format!("action {action} 的参数集合不精确"),
        )),
        _ => Err(BrowserFailure::rejected(
            "browser_action_denied",
            "action",
            "action 不在 Host 固定 semantic interaction 集合中",
        )),
    }
}

fn parse_interaction_value(input: &Value, code: &'static str) -> Result<String, BrowserFailure> {
    let value = required_str(input, "value")
        .map_err(|error| BrowserFailure::rejected(code, "value", error.to_string()))?;
    if value.is_empty()
        || value.chars().count() > MAX_INTERACTION_VALUE_CHARS
        || value
            .chars()
            .any(|character| matches!(character as u32, 0x00..=0x1f | 0x7f..=0x9f))
    {
        return Err(BrowserFailure::rejected(
            code,
            "value",
            format!("value 必须非空、不含控制字符且不超过 {MAX_INTERACTION_VALUE_CHARS} 字符"),
        ));
    }
    Ok(value.to_owned())
}

fn parse_page_ref(input: &Value) -> Result<String, BrowserFailure> {
    let page_ref = required_str(input, "page_ref").map_err(|error| {
        BrowserFailure::rejected("browser_page_ref_missing", "page_ref", error.to_string())
    })?;
    if page_ref.len() != 37
        || !page_ref.starts_with("page_")
        || !page_ref[5..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(BrowserFailure::rejected(
            "browser_page_ref_invalid",
            "page_ref",
            "page_ref 必须是 Host 返回的 opaque page ref",
        ));
    }
    Ok(page_ref.to_owned())
}

#[cfg(test)]
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

fn rejected_interact_outcome(failure: BrowserFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!(
            "browser_interact 拒绝：code={}；{}",
            failure.code, failure.message
        ),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(json!({
        "semantic_browser": {
            "action": "interact",
            "trust": TRUST,
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            }
        }
    }))
}

fn interact_operation_outcome(failure: BrowserFailure, dispatched: bool) -> ToolOutcome {
    let mut outcome = if failure.transport {
        ToolOutcome::transport_failure(format!(
            "browser_interact 失败：code={}；{}",
            failure.code, failure.message
        ))
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "browser_interact 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.transport = ToolTransportStatus::Succeeded;
        outcome.operation = if dispatched {
            ToolOperationStatus::Indeterminate
        } else {
            ToolOperationStatus::Failed
        };
        outcome
    };
    outcome.side_effect = if dispatched {
        ToolSideEffectStatus::Indeterminate
    } else {
        ToolSideEffectStatus::NotApplied
    };
    outcome.retry = if dispatched {
        ToolRetryDisposition::Unsafe
    } else {
        failure.retry
    };
    outcome.metadata = Some(json!({
        "semantic_browser": {
            "action": "interact",
            "trust": TRUST,
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            }
        }
    }));
    outcome
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
    external_grant: Mutex<Option<ExternalRequestGrant>>,
    external_receipt: Mutex<Option<ObservedExternalReceipt>>,
    metrics: BrowserMetrics,
}

#[derive(Debug, Clone)]
struct ExternalRequestGrant {
    target_url: String,
    parameters: Vec<(String, String)>,
    ignored_empty_parameters: BTreeSet<String>,
    parameters_sha256: String,
    impact: &'static str,
}

#[derive(Debug, Clone)]
struct ObservedExternalReceipt {
    target_url: String,
    status: u16,
    remote_receipt: String,
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
            external_grant: Mutex::new(None),
            external_receipt: Mutex::new(None),
            metrics: BrowserMetrics::default(),
        }
    }

    fn authorize_cdp_request(
        &self,
        url: &str,
        method: &str,
        resource_type: Option<&str>,
        post_data: Option<&str>,
    ) -> Result<(), BrowserFailure> {
        if !matches!(method, "GET" | "HEAD") {
            let grant = self
                .external_grant
                .lock()
                .expect("browser external grant lock")
                .clone();
            let allowed = grant.as_ref().is_some_and(|grant| {
                method == "POST"
                    && grant.target_url == url
                    && post_data
                        .and_then(|post_data| {
                            canonical_granted_form_parameters(
                                post_data,
                                &grant.ignored_empty_parameters,
                            )
                        })
                        .is_some_and(|parameters| parameters == grant.parameters)
            });
            if !allowed {
                return Err(BrowserFailure::operation(
                    "browser_method_denied",
                    "egress",
                    format!("browser blocked ungranted HTTP method {method}"),
                ));
            }
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

    fn proxy_request_allowed(&self, method: &str, url: &Url) -> bool {
        matches!(method, "GET" | "HEAD")
            || self
                .external_grant
                .lock()
                .expect("browser external grant lock")
                .as_ref()
                .is_some_and(|grant| method == "POST" && grant.target_url == url.as_str())
    }

    fn begin_external_action(&self, grant: ExternalRequestGrant) {
        *self
            .external_grant
            .lock()
            .expect("browser external grant lock") = Some(grant);
        *self
            .external_receipt
            .lock()
            .expect("browser external receipt lock") = None;
        self.begin_action();
    }

    fn record_external_response(&self, url: &str, status: u16, remote_receipt: Option<&str>) {
        let matches = self
            .external_grant
            .lock()
            .expect("browser external grant lock")
            .as_ref()
            .is_some_and(|grant| grant.target_url == url);
        if matches {
            *self
                .external_receipt
                .lock()
                .expect("browser external receipt lock") = Some(ObservedExternalReceipt {
                target_url: url.to_owned(),
                status,
                remote_receipt: remote_receipt.unwrap_or_default().to_owned(),
            });
        }
    }

    fn finish_external_action(&self) -> Option<ObservedExternalReceipt> {
        *self
            .external_grant
            .lock()
            .expect("browser external grant lock") = None;
        self.end_action();
        self.external_receipt
            .lock()
            .expect("browser external receipt lock")
            .take()
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
    let (head, buffered_body) = read_proxy_head(&mut client, &cancellation).await?;
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
        if !buffered_body.is_empty() {
            return Err(BrowserFailure::operation(
                "browser_proxy_request_pipelined",
                "proxy",
                "CONNECT request contained bytes after its header",
            ));
        }
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
        let result = async {
            if !matches!(method, "GET" | "HEAD" | "POST") {
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
            if !state.proxy_request_allowed(method, &url) {
                return Err(BrowserFailure::operation(
                    "browser_method_denied",
                    "proxy",
                    format!("browser proxy denied ungranted method {method} for {url}"),
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
            if !buffered_body.is_empty() {
                reserve_bytes(
                    &state.metrics.bytes_sent,
                    buffered_body.len() as u64,
                    MAX_TOTAL_NETWORK_REQUEST_BYTES,
                    "browser_total_request_bytes_exceeded",
                    "browser total request bytes",
                )?;
                upstream.write_all(&buffered_body).await.map_err(|error| {
                    BrowserFailure::transport(
                        "browser_proxy_upstream_write_failed",
                        "proxy",
                        format!("cannot write bounded browser request body: {error}"),
                    )
                })?;
            }
            relay_bounded(&mut client, &mut upstream, &state.metrics, &cancellation).await
        }
        .await;
        if method == "POST"
            && let Err(failure) = &result
        {
            state.record_proxy_denial(failure.clone(), true);
        }
        result
    }
}

async fn read_proxy_head(
    stream: &mut TcpStream,
    cancellation: &CancellationToken,
) -> Result<(Vec<u8>, Vec<u8>), BrowserFailure> {
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
            let buffered_body = head.split_off(end);
            return Ok((head, buffered_body));
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
                | "content-type"
                | "content-length"
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
        match tokio::time::timeout(CONNECT_DEADLINE, resolver.connect(address)).await {
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
    pending_commands: HashMap<u64, String>,
    observed_events: BTreeSet<(String, String)>,
    egress: Arc<EgressState>,
    primary_target_id: Option<String>,
    managed_target_ids: BTreeSet<String>,
    allow_next_page_target: bool,
    attached_page_sessions: HashMap<String, String>,
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
        if let Some(id) = event.get("id").and_then(Value::as_u64)
            && let Some(command) = self.pending_commands.remove(&id)
        {
            if let Some(error) = event.get("error") {
                let failure = BrowserFailure::operation(
                    "browser_cdp_command_failed",
                    "cdp",
                    format!("CDP {command} failed: {error}"),
                );
                self.egress.record_denial(failure.clone(), true);
                return Err(failure);
            }
            return Ok(());
        }
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
            let session_id = event
                .get("params")
                .and_then(|params| params.get("sessionId"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let target_type = event
                .get("params")
                .and_then(|params| params.get("targetInfo"))
                .and_then(|target| target.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if self.primary_target_id.as_deref() == Some(target_id)
                || self.managed_target_ids.contains(target_id)
            {
                return Ok(());
            }
            if self.allow_next_page_target && target_type == "page" && !session_id.is_empty() {
                self.managed_target_ids.insert(target_id.to_owned());
                self.attached_page_sessions
                    .insert(target_id.to_owned(), session_id.to_owned());
            } else {
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
        if method == Some("Network.responseReceived") {
            let response = event
                .get("params")
                .and_then(|params| params.get("response"));
            if let (Some(url), Some(status)) = (
                response
                    .and_then(|response| response.get("url"))
                    .and_then(Value::as_str),
                response
                    .and_then(|response| response.get("status"))
                    .and_then(Value::as_f64)
                    .map(|status| status as u16),
            ) {
                let remote_receipt = response
                    .and_then(|response| response.get("headers"))
                    .and_then(Value::as_object)
                    .and_then(|headers| {
                        headers.iter().find_map(|(name, value)| {
                            name.eq_ignore_ascii_case("x-dse-receipt")
                                .then(|| value.as_str())
                                .flatten()
                        })
                    });
                self.egress
                    .record_external_response(url, status, remote_receipt);
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
        let post_data = request.get("postData").and_then(Value::as_str);
        let resource_type = params.get("resourceType").and_then(Value::as_str);
        let fatal = resource_type == Some("Document");
        let session_id = event.get("sessionId").and_then(Value::as_str);
        match self
            .egress
            .authorize_cdp_request(url, method, resource_type, post_data)
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
            })?;
        self.pending_commands
            .insert(self.next_id, method.to_owned());
        Ok(())
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

fn canonical_form_parameters(post_data: &str) -> Option<Vec<(String, String)>> {
    if post_data.len() > 8 * 1024 {
        return None;
    }
    let parsed = Url::parse(&format!("http://form.invalid/?{post_data}")).ok()?;
    let mut parameters = parsed
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    if parameters.len() > 32 {
        return None;
    }
    parameters.sort();
    Some(parameters)
}

fn canonical_granted_form_parameters(
    post_data: &str,
    ignored_empty_parameters: &BTreeSet<String>,
) -> Option<Vec<(String, String)>> {
    let parameters = canonical_form_parameters(post_data)?;
    let mut granted = Vec::with_capacity(parameters.len());
    for (name, value) in parameters {
        if ignored_empty_parameters.contains(&name) {
            if !value.is_empty() {
                return None;
            }
        } else {
            granted.push((name, value));
        }
    }
    Some(granted)
}

#[derive(Debug, Clone)]
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
    capabilities: BTreeSet<ElementCapability>,
    external_preview: Option<ExternalActionPreview>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ElementCapability {
    Click,
    Fill,
    Press,
    Scroll,
    Select,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalActionPreview {
    origin: String,
    target_url: String,
    parameters: Vec<(String, String)>,
    ignored_empty_parameters: BTreeSet<String>,
    parameters_sha256: String,
    impact: &'static str,
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
    page_ref: String,
    target_id: String,
    cdp_session_id: String,
    background_pages: BTreeMap<String, BrowserPageState>,
    proxy: Option<EgressProxy>,
    profile: Option<tempfile::TempDir>,
    child: tokio::process::Child,
    owner: ProcessTreeOwner,
}

#[derive(Debug)]
struct BrowserPageState {
    page_ref: String,
    target_id: String,
    cdp_session_id: String,
    page_epoch: u64,
    snapshot_id: String,
    refs: HashMap<String, ElementTarget>,
    stale_refs: BTreeSet<String>,
    observation_fingerprint: String,
    observation: SessionObservation,
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
    fn swap_active_page(&mut self, page: BrowserPageState) -> BrowserPageState {
        let BrowserPageState {
            mut page_ref,
            mut target_id,
            mut cdp_session_id,
            mut page_epoch,
            mut snapshot_id,
            mut refs,
            mut stale_refs,
            mut observation_fingerprint,
            mut observation,
        } = page;
        std::mem::swap(&mut self.page_ref, &mut page_ref);
        std::mem::swap(&mut self.target_id, &mut target_id);
        std::mem::swap(&mut self.cdp_session_id, &mut cdp_session_id);
        std::mem::swap(&mut self.page_epoch, &mut page_epoch);
        std::mem::swap(&mut self.snapshot_id, &mut snapshot_id);
        std::mem::swap(&mut self.refs, &mut refs);
        std::mem::swap(&mut self.stale_refs, &mut stale_refs);
        std::mem::swap(
            &mut self.observation_fingerprint,
            &mut observation_fingerprint,
        );
        std::mem::swap(&mut self.observation, &mut observation);
        self.cdp.primary_target_id = Some(self.target_id.clone());
        BrowserPageState {
            page_ref,
            target_id,
            cdp_session_id,
            page_epoch,
            snapshot_id,
            refs,
            stale_refs,
            observation_fingerprint,
            observation,
        }
    }

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
        rotate_observation(&mut open.session, true);
        let outcome = browser_result_outcome(&open.session, None, false);
        *self.session.lock().expect("browser session lock") = Some(open.session);
        outcome
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
                    "browser_interact click requires a live browser_navigate observation",
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
                    "browser_interact fill requires a live browser_navigate observation",
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

    async fn interact(
        &self,
        run_id: &str,
        request: BrowserInteractRequest,
        cancellation: BrowserCancellationToken,
    ) -> ToolOutcome {
        match request {
            BrowserInteractRequest::Click(request) => {
                self.click(run_id, request, cancellation).await
            }
            BrowserInteractRequest::Fill(request) => self.fill(run_id, request, cancellation).await,
            request => {
                self.run_extended_interaction(run_id, request, cancellation)
                    .await
            }
        }
    }

    fn authorization_preview(
        &self,
        run_id: &str,
        request: &BrowserInteractRequest,
    ) -> Option<BrowserAuthorizationPreview> {
        self.public_authorization_preview(run_id, request)
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
    let (cdp, target_id, cdp_session_id, observation) = match execution {
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
            page_ref: format!("page_{}", Uuid::new_v4().simple()),
            target_id,
            cdp_session_id,
            background_pages: BTreeMap::new(),
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
                "dom": node.dom.as_ref().map(|dom| (&dom.node_name, &dom.attributes)),
            })
        })
        .collect::<Vec<_>>();
    sha256_bytes(&serde_json::to_vec(&identity).expect("browser fingerprint serializes"))
}

fn derive_external_action_previews(
    current_url: &str,
    nodes: &[SemanticNode],
) -> HashMap<u64, ExternalActionPreview> {
    let Ok(base) = Url::parse(current_url) else {
        return HashMap::new();
    };
    let Ok(base_origin) = Origin::from_url(&base) else {
        return HashMap::new();
    };
    let mut previews = HashMap::new();
    for node in nodes {
        let Some(backend_id) = node.backend_dom_node_id else {
            continue;
        };
        let Some(dom) = node.dom.as_ref() else {
            continue;
        };
        let Some(form_id) = dom.attributes.get("form").filter(|value| !value.is_empty()) else {
            continue;
        };
        if !dom
            .attributes
            .get("formmethod")
            .is_some_and(|method| method.eq_ignore_ascii_case("post"))
        {
            continue;
        }
        let Some(action) = dom.attributes.get("formaction") else {
            continue;
        };
        let Ok(target) = base.join(action) else {
            continue;
        };
        if Origin::from_url(&target).ok().as_ref() != Some(&base_origin) {
            continue;
        }
        let semantic_identity = format!(
            "{} {}",
            node.accessible_name.to_ascii_lowercase(),
            target.path().to_ascii_lowercase()
        );
        if !semantic_identity.contains("draft")
            || ["publish", "delete", "purchase", "buy", "send", "message"]
                .iter()
                .any(|denied| semantic_identity.contains(denied))
        {
            continue;
        }
        let mut parameters = Vec::new();
        let mut ignored_empty_parameters = BTreeSet::new();
        for field in nodes {
            let Some(facts) = field.dom.as_ref() else {
                continue;
            };
            if facts.attributes.get("form") != Some(form_id) {
                continue;
            }
            let Some(name) = facts.attributes.get("name").map(|name| name.trim()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let input_type = facts
                .attributes
                .get("type")
                .map(|value| value.to_ascii_lowercase());
            if facts.node_name == "BUTTON"
                || input_type
                    .as_deref()
                    .is_some_and(|kind| matches!(kind, "submit" | "button" | "reset"))
            {
                continue;
            }
            let sensitive = field.fill_safety == Some(FillSafety::SensitiveDenied)
                || field
                    .accessible_name
                    .to_ascii_lowercase()
                    .contains("password")
                || input_type
                    .as_deref()
                    .is_some_and(|kind| matches!(kind, "password" | "file"));
            if sensitive {
                ignored_empty_parameters.insert(name.to_owned());
                continue;
            }
            let value = if field.value.is_empty() {
                facts.attributes.get("value").cloned().unwrap_or_default()
            } else {
                field.value.clone()
            };
            parameters.push((bounded_text(name, 256), bounded_text(&value, 1_024)));
        }
        if let Some(name) = dom.attributes.get("name").filter(|name| !name.is_empty()) {
            parameters.push((
                bounded_text(name, 256),
                bounded_text(
                    dom.attributes
                        .get("value")
                        .map(String::as_str)
                        .unwrap_or("save_draft"),
                    1_024,
                ),
            ));
        }
        parameters.sort();
        parameters.dedup();
        if parameters.len() > 32 {
            continue;
        }
        let encoded = serde_json::to_vec(&parameters).expect("bounded form parameters serialize");
        if encoded.len() > 8 * 1024 {
            continue;
        }
        previews.insert(
            backend_id,
            ExternalActionPreview {
                origin: base_origin.canonical(),
                target_url: target.to_string(),
                parameters_sha256: format!("sha256:{}", sha256_bytes(&encoded)),
                parameters,
                ignored_empty_parameters,
                impact: "reversible_draft_write",
            },
        );
    }
    previews
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
    let external_previews =
        derive_external_action_previews(&session.observation.final_url, &session.observation.nodes);
    for node in &mut session.observation.nodes {
        node.element_ref = None;
        if !allow_refs {
            continue;
        }
        let Some(backend_dom_node_id) = node.backend_dom_node_id else {
            continue;
        };
        let mut capabilities = BTreeSet::new();
        if node.click_safety == Some(ClickSafety::Allowed) {
            capabilities.insert(ElementCapability::Click);
        }
        if node.fill_safety == Some(FillSafety::Allowed) {
            capabilities.insert(ElementCapability::Fill);
        }
        if node.capabilities.contains(&"press") {
            capabilities.insert(ElementCapability::Press);
        }
        if node.capabilities.contains(&"scroll") {
            capabilities.insert(ElementCapability::Scroll);
        }
        if node.capabilities.contains(&"select") {
            capabilities.insert(ElementCapability::Select);
        }
        let external_preview = external_previews.get(&backend_dom_node_id).cloned();
        if external_preview.is_some() {
            capabilities.insert(ElementCapability::Submit);
            if !node.capabilities.contains(&"submit") {
                node.capabilities.push("submit");
            }
        }
        if capabilities.is_empty() {
            continue;
        }
        let element_ref = new_element_ref(&session.refs, &session.stale_refs);
        session.refs.insert(
            element_ref.clone(),
            ElementTarget {
                backend_dom_node_id,
                role: node.role.clone(),
                accessible_name: node.accessible_name.clone(),
                capabilities,
                external_preview,
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
    let mut pages = session
        .background_pages
        .values()
        .map(|page| BrowserPageSummary {
            page_ref: page.page_ref.clone(),
            url: page.observation.final_url.clone(),
            title: page.observation.title.clone(),
            page_epoch: page.page_epoch,
            active: false,
        })
        .collect::<Vec<_>>();
    pages.push(BrowserPageSummary {
        page_ref: session.page_ref.clone(),
        url: session.observation.final_url.clone(),
        title: session.observation.title.clone(),
        page_epoch: session.page_epoch,
        active: true,
    });
    pages.sort_by(|left, right| left.page_ref.cmp(&right.page_ref));
    BrowserResult {
        action,
        requested_url: session.request.requested_url.clone(),
        final_url: session.observation.final_url.clone(),
        title: session.observation.title.clone(),
        snapshot: session.observation.nodes.clone(),
        snapshot_id: session.snapshot_id.clone(),
        page_epoch: session.page_epoch,
        active_page_ref: session.page_ref.clone(),
        pages,
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
            BrowserTargetScope::Public => "same_run_in_memory_public_origin",
        },
        session_live: true,
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
    async fn bounded_call(
        cdp: &mut CdpClient,
        session_id: &str,
        method: &'static str,
        params: Value,
    ) -> Result<Value, BrowserFailure> {
        tokio::time::timeout(CONNECT_DEADLINE, cdp.call(method, params, Some(session_id)))
            .await
            .map_err(|_| {
                BrowserFailure::transport(
                    "browser_semantic_snapshot_deadline",
                    "snapshot",
                    format!("CDP {method} exceeded its bounded snapshot deadline"),
                )
            })?
    }

    let before = bounded_call(
        &mut session.cdp,
        &session.cdp_session_id,
        "Page.getFrameTree",
        json!({}),
    )
    .await?;
    let ax = bounded_call(
        &mut session.cdp,
        &session.cdp_session_id,
        "Accessibility.getFullAXTree",
        json!({"depth":16}),
    )
    .await?;
    let dom = bounded_call(
        &mut session.cdp,
        &session.cdp_session_id,
        "DOMSnapshot.captureSnapshot",
        json!({
            "computedStyles":[],
            "includeDOMRects":false,
            "includePaintOrder":false,
        }),
    )
    .await?;
    let after = bounded_call(
        &mut session.cdp,
        &session.cdp_session_id,
        "Page.getFrameTree",
        json!({}),
    )
    .await?;
    let (before_loader, before_url) = current_frame_identity(&before)?;
    let (after_loader, after_url) = current_frame_identity(&after)?;
    if before_loader != after_loader || before_url != after_url {
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

fn current_frame_identity(frame_tree: &Value) -> Result<(String, String), BrowserFailure> {
    let frame = frame_tree
        .get("frameTree")
        .and_then(|tree| tree.get("frame"))
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page.getFrameTree is missing its main frame",
            )
        })?;
    let loader_id = frame
        .get("loaderId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page.getFrameTree main frame is missing loaderId",
            )
        })?;
    let url = frame
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_cdp_message_invalid",
                "snapshot",
                "Page.getFrameTree main frame is missing URL",
            )
        })?;
    Ok((loader_id.to_owned(), url.to_owned()))
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
            consumed_element_ref: Some(request.element_ref.clone()),
            page_ref: None,
            receipt: None,
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
    if !target.capabilities.contains(&ElementCapability::Click) {
        return Err(BrowserFailure::operation(
            "browser_element_ref_capability_mismatch",
            "element_ref",
            "element_ref does not carry click capability",
        ));
    }
    match &current.click_safety {
        Some(ClickSafety::Disabled) => Err(BrowserFailure::operation(
            "browser_element_ref_disabled",
            "element_ref",
            "element_ref resolves to a disabled target",
        )),
        Some(ClickSafety::SideEffectDenied) => Err(BrowserFailure::operation(
            "browser_element_ref_side_effect_denied",
            "element_ref",
            "element_ref target is outside the admitted click-only control family",
        )),
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
    if !target.capabilities.contains(&ElementCapability::Fill) {
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
        consumed_element_ref: Some(request.element_ref.clone()),
        page_ref: None,
        receipt: None,
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
            consumed_element_ref: Some(request.element_ref.clone()),
            page_ref: None,
            receipt: None,
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
    for (event_type, key, code, modifiers) in [
        ("rawKeyDown", "a", "KeyA", SELECT_ALL_MODIFIERS),
        ("keyUp", "a", "KeyA", SELECT_ALL_MODIFIERS),
        ("rawKeyDown", "Backspace", "Backspace", 0),
        ("keyUp", "Backspace", "Backspace", 0),
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
        consumed_element_ref: Some(request.element_ref.clone()),
        page_ref: None,
        receipt: None,
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

async fn execute_live_extended_interaction(
    session: &mut LiveBrowserSession,
    run_id: &str,
    request: &BrowserInteractRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    if session.run_id != run_id {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_element_ref_cross_run",
                "session",
                "browser interaction belongs to a different Run",
            ),
            false,
        );
    }
    match request {
        BrowserInteractRequest::Press { .. }
        | BrowserInteractRequest::Scroll { .. }
        | BrowserInteractRequest::Select { .. } => {
            execute_live_ref_interaction(session, request, cancellation).await
        }
        BrowserInteractRequest::Wait { .. } => {
            execute_live_wait(session, request, cancellation).await
        }
        BrowserInteractRequest::Back => execute_live_back(session, cancellation).await,
        BrowserInteractRequest::TabOpen { .. }
        | BrowserInteractRequest::TabSwitch { .. }
        | BrowserInteractRequest::TabClose { .. } => {
            execute_live_tab(session, request, cancellation).await
        }
        BrowserInteractRequest::Submit(request) => {
            execute_live_submit(session, run_id, request, cancellation).await
        }
        BrowserInteractRequest::Click(_) | BrowserInteractRequest::Fill(_) => unreachable!(),
    }
}

fn request_action_identity(
    request: &BrowserInteractRequest,
) -> (&'static str, Option<&str>, ElementCapability) {
    match request {
        BrowserInteractRequest::Press { element_ref, .. } => {
            ("press", Some(element_ref), ElementCapability::Press)
        }
        BrowserInteractRequest::Scroll { element_ref, .. } => {
            ("scroll", Some(element_ref), ElementCapability::Scroll)
        }
        BrowserInteractRequest::Select { element_ref, .. } => {
            ("select", Some(element_ref), ElementCapability::Select)
        }
        _ => ("interact", None, ElementCapability::Press),
    }
}

fn extended_failure_with_fresh_observation(
    session: &LiveBrowserSession,
    kind: &'static str,
    element_ref: Option<&str>,
    failure: BrowserFailure,
    dispatched: bool,
) -> ToolOutcome {
    let observation = browser_result_value(
        session,
        Some(BrowserActionResult {
            kind,
            consumed_element_ref: element_ref.map(str::to_owned),
            page_ref: None,
            receipt: None,
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
    outcome.operation = if dispatched && failure.transport {
        ToolOperationStatus::Indeterminate
    } else {
        ToolOperationStatus::Failed
    };
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
    outcome.metadata = Some(json!({
        "semantic_browser": {
            "action": kind,
            "element_ref": element_ref,
            "trust": TRUST,
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            }
        }
    }));
    outcome
}

async fn invalidate_extended_with_failure(
    session: &mut LiveBrowserSession,
    kind: &'static str,
    element_ref: Option<&str>,
    observation: SessionObservation,
    failure: BrowserFailure,
) -> ToolOutcome {
    session.observation = observation;
    rotate_observation(session, true);
    extended_failure_with_fresh_observation(session, kind, element_ref, failure, false)
}

async fn execute_live_ref_interaction(
    session: &mut LiveBrowserSession,
    request: &BrowserInteractRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    let (kind, element_ref, capability) = request_action_identity(request);
    let element_ref = element_ref.expect("ref interaction carries element_ref");
    let fresh = match tokio::time::timeout(PAGE_LOAD_DEADLINE, capture_current_observation(session))
        .await
    {
        Ok(Ok(observation)) => observation,
        Ok(Err(failure)) => return interact_operation_outcome(failure, false),
        Err(_) => {
            return interact_operation_outcome(
                BrowserFailure::transport(
                    "browser_interaction_observation_deadline",
                    "pre_action_observation",
                    "fresh pre-action semantic observation exceeded its deadline",
                ),
                false,
            );
        }
    };
    if cancellation.is_cancelled() {
        return invalidate_extended_with_failure(
            session,
            kind,
            Some(element_ref),
            fresh,
            BrowserFailure::operation(
                "browser_cancelled",
                "cancellation",
                "browser interaction was cancelled before dispatch",
            ),
        )
        .await;
    }
    let target = match resolve_element_ref(
        &session.run_id,
        &session.run_id,
        &session.refs,
        &session.stale_refs,
        element_ref,
    ) {
        Ok(target) => target,
        Err(failure) => {
            return invalidate_extended_with_failure(
                session,
                kind,
                Some(element_ref),
                fresh,
                failure,
            )
            .await;
        }
    };
    if !target.capabilities.contains(&capability) {
        return invalidate_extended_with_failure(
            session,
            kind,
            Some(element_ref),
            fresh,
            BrowserFailure::operation(
                "browser_element_ref_capability_mismatch",
                "element_ref",
                format!("element_ref does not carry {kind} capability"),
            ),
        )
        .await;
    }
    let current = match validate_target_identity(&target, &fresh.nodes) {
        Ok(Some(current)) => current,
        Ok(None) => {
            return invalidate_extended_with_failure(
                session,
                kind,
                Some(element_ref),
                fresh,
                absent_target_failure(false),
            )
            .await;
        }
        Err(failure) => {
            return invalidate_extended_with_failure(
                session,
                kind,
                Some(element_ref),
                fresh,
                failure,
            )
            .await;
        }
    };
    if observation_fingerprint(&fresh.nodes) != session.observation_fingerprint {
        return invalidate_extended_with_failure(
            session,
            kind,
            Some(element_ref),
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
            return invalidate_extended_with_failure(
                session,
                kind,
                Some(element_ref),
                fresh,
                absent_target_failure(false),
            )
            .await;
        }
    };
    let Some((x, y)) = box_center(&model) else {
        return invalidate_extended_with_failure(
            session,
            kind,
            Some(element_ref),
            fresh,
            absent_target_failure(true),
        )
        .await;
    };
    session.egress.begin_action();
    let dispatch_operation = async {
        match request {
            BrowserInteractRequest::Press { key, .. } => {
                match session
                    .cdp
                    .call(
                        "DOM.focus",
                        json!({"backendNodeId":target.backend_dom_node_id}),
                        Some(&session.cdp_session_id),
                    )
                    .await
                {
                    Ok(_) => dispatch_key(&mut session.cdp, &session.cdp_session_id, *key).await,
                    Err(failure) => Err(failure),
                }
            }
            BrowserInteractRequest::Scroll {
                direction, amount, ..
            } => {
                let delta = match direction {
                    BrowserScrollDirection::Up => -(*amount as i64),
                    BrowserScrollDirection::Down => *amount as i64,
                };
                session
                    .cdp
                    .call(
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":delta}),
                        Some(&session.cdp_session_id),
                    )
                    .await
            }
            BrowserInteractRequest::Select { value, .. } => {
                dispatch_select(
                    &mut session.cdp,
                    &session.cdp_session_id,
                    target.backend_dom_node_id,
                    value,
                    current,
                )
                .await
            }
            _ => unreachable!(),
        }
    };
    let dispatch = match tokio::time::timeout(PAGE_LOAD_DEADLINE, dispatch_operation).await {
        Ok(result) => result,
        Err(_) => Err(BrowserFailure::transport(
            "browser_interaction_dispatch_deadline",
            "action_dispatch",
            format!("browser_{kind} dispatch exceeded its deadline"),
        )),
    };
    if let Err(failure) = dispatch {
        session.egress.end_action();
        return extended_failure_with_fresh_observation(
            session,
            kind,
            Some(element_ref),
            failure,
            true,
        );
    }
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    let post = match tokio::time::timeout(PAGE_LOAD_DEADLINE, capture_current_observation(session))
        .await
    {
        Ok(Ok(observation)) => observation,
        Ok(Err(failure)) => {
            session.egress.end_action();
            return extended_failure_with_fresh_observation(
                session,
                kind,
                Some(element_ref),
                failure,
                true,
            );
        }
        Err(_) => {
            session.egress.end_action();
            return extended_failure_with_fresh_observation(
                session,
                kind,
                Some(element_ref),
                BrowserFailure::transport(
                    "browser_interaction_observation_deadline",
                    "post_action_observation",
                    "fresh post-action semantic observation exceeded its deadline",
                ),
                true,
            );
        }
    };
    session.egress.end_action();
    session.observation = post;
    rotate_observation(session, true);
    if let Some(failure) = session.egress.take_fatal_failure() {
        return extended_failure_with_fresh_observation(
            session,
            kind,
            Some(element_ref),
            failure,
            true,
        );
    }
    let mut outcome = browser_result_outcome(
        session,
        Some(BrowserActionResult {
            kind,
            consumed_element_ref: Some(element_ref.to_owned()),
            page_ref: None,
            receipt: None,
        }),
        false,
    );
    outcome.side_effect = ToolSideEffectStatus::Applied;
    outcome
}

async fn dispatch_key(
    cdp: &mut CdpClient,
    session_id: &str,
    key: BrowserKey,
) -> Result<Value, BrowserFailure> {
    let (key_name, code) = match key {
        BrowserKey::Enter => ("Enter", "Enter"),
        BrowserKey::Escape => ("Escape", "Escape"),
        BrowserKey::Tab => ("Tab", "Tab"),
        BrowserKey::ArrowUp => ("ArrowUp", "ArrowUp"),
        BrowserKey::ArrowDown => ("ArrowDown", "ArrowDown"),
        BrowserKey::Space => (" ", "Space"),
    };
    cdp.call(
        "Input.dispatchKeyEvent",
        json!({
            "type":"keyDown",
            "key":key_name,
            "code":code,
        }),
        Some(session_id),
    )
    .await?;
    cdp.call(
        "Input.dispatchKeyEvent",
        json!({
            "type":"keyUp",
            "key":key_name,
            "code":code,
        }),
        Some(session_id),
    )
    .await
}

async fn dispatch_select(
    cdp: &mut CdpClient,
    session_id: &str,
    backend_dom_node_id: u64,
    value: &str,
    current: &SemanticNode,
) -> Result<Value, BrowserFailure> {
    if current
        .dom
        .as_ref()
        .is_none_or(|dom| dom.node_name != "SELECT")
    {
        return Err(BrowserFailure::operation(
            "browser_select_target_ineligible",
            "element_ref",
            "select target is not a native SELECT element",
        ));
    }
    let described = cdp
        .call(
            "DOM.describeNode",
            json!({"backendNodeId":backend_dom_node_id,"depth":2,"pierce":false}),
            Some(session_id),
        )
        .await?;
    let children = described
        .get("node")
        .and_then(|node| node.get("children"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_select_options_missing",
                "element_ref",
                "SELECT has no inspectable OPTION children",
            )
        })?;
    let mut option_index = None;
    let mut option_backend_ids = Vec::new();
    let mut selected_backend_id = None;
    let mut logical_index = 0_usize;
    for option in children {
        if option.get("nodeName").and_then(Value::as_str) != Some("OPTION") {
            continue;
        }
        let attributes = option
            .get("attributes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let attribute_value = attributes.chunks_exact(2).find_map(|pair| {
            (pair[0].as_str() == Some("value")).then(|| pair[1].as_str().unwrap_or_default())
        });
        let text = option
            .get("children")
            .and_then(Value::as_array)
            .and_then(|children| children.first())
            .and_then(|text| text.get("nodeValue"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let semantic_value = attribute_value.unwrap_or(text);
        let backend_id = option.get("backendNodeId").and_then(Value::as_u64);
        option_backend_ids.extend(backend_id);
        if semantic_value == value {
            option_index = Some(logical_index);
            selected_backend_id = backend_id;
        }
        logical_index += 1;
    }
    if option_index.is_none() {
        return Err(BrowserFailure::operation(
            "browser_select_option_missing",
            "value",
            "requested option value/text is not present",
        ));
    }
    cdp.call(
        "DOM.focus",
        json!({"backendNodeId":backend_dom_node_id}),
        Some(session_id),
    )
    .await?;
    let selected_backend_id = selected_backend_id.ok_or_else(|| {
        BrowserFailure::operation(
            "browser_select_option_identity_missing",
            "value",
            "requested OPTION has no inspectable backend identity",
        )
    })?;
    cdp.call(
        "DOM.getDocument",
        json!({"depth":0,"pierce":false}),
        Some(session_id),
    )
    .await?;
    let pushed = cdp
        .call(
            "DOM.pushNodesByBackendIdsToFrontend",
            json!({"backendNodeIds":option_backend_ids}),
            Some(session_id),
        )
        .await?;
    let option_node_ids = pushed
        .get("nodeIds")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_select_option_identity_missing",
                "value",
                "OPTION backend identities did not resolve to frontend nodes",
            )
        })?;
    let selected_index = option_backend_ids
        .iter()
        .position(|backend_id| *backend_id == selected_backend_id)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_select_option_identity_missing",
                "value",
                "requested OPTION identity was not present in the resolved option set",
            )
        })?;
    let selected_node_id = option_node_ids
        .get(selected_index)
        .and_then(Value::as_u64)
        .filter(|node_id| *node_id != 0)
        .ok_or_else(|| {
            BrowserFailure::operation(
                "browser_select_option_identity_missing",
                "value",
                "requested OPTION did not resolve to a frontend node",
            )
        })?;
    for option_node_id in option_node_ids
        .iter()
        .filter_map(Value::as_u64)
        .filter(|node_id| *node_id != 0)
    {
        cdp.call(
            "DOM.removeAttribute",
            json!({"nodeId":option_node_id,"name":"selected"}),
            Some(session_id),
        )
        .await?;
    }
    cdp.call(
        "DOM.setAttributeValue",
        json!({"nodeId":selected_node_id,"name":"selected","value":""}),
        Some(session_id),
    )
    .await
}

async fn execute_live_wait(
    session: &mut LiveBrowserSession,
    request: &BrowserInteractRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    let BrowserInteractRequest::Wait {
        condition,
        value,
        timeout,
    } = request
    else {
        unreachable!()
    };
    let started = tokio::time::Instant::now();
    loop {
        if cancellation.is_cancelled() {
            return interact_operation_outcome(
                BrowserFailure::operation(
                    "browser_cancelled",
                    "cancellation",
                    "browser_wait was cancelled",
                ),
                false,
            );
        }
        let observation = match capture_current_observation(session).await {
            Ok(observation) => observation,
            Err(failure) => return interact_operation_outcome(failure, false),
        };
        let matched = wait_condition_matches(*condition, value.as_deref(), &observation);
        session.observation = observation;
        if matched {
            rotate_observation(session, true);
            return browser_result_outcome(
                session,
                Some(BrowserActionResult {
                    kind: "wait",
                    consumed_element_ref: None,
                    page_ref: None,
                    receipt: None,
                }),
                false,
            );
        }
        if started.elapsed() >= *timeout {
            rotate_observation(session, true);
            return extended_failure_with_fresh_observation(
                session,
                "wait",
                None,
                BrowserFailure::operation(
                    "browser_wait_timeout",
                    "wait",
                    format!(
                        "typed condition {} did not match within {} ms",
                        wait_condition_name(*condition),
                        timeout.as_millis()
                    ),
                ),
                false,
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn wait_condition_matches(
    condition: BrowserWaitCondition,
    value: Option<&str>,
    observation: &SessionObservation,
) -> bool {
    match condition {
        BrowserWaitCondition::DocumentReady => true,
        BrowserWaitCondition::UrlEquals => value == Some(observation.final_url.as_str()),
        BrowserWaitCondition::TextPresent | BrowserWaitCondition::TextAbsent => {
            let needle = value.unwrap_or_default();
            let present = observation.nodes.iter().any(|node| {
                node.accessible_name.contains(needle)
                    || node.text.contains(needle)
                    || node.value.contains(needle)
                    || node.state.values().any(|state| state.contains(needle))
            });
            present == (condition == BrowserWaitCondition::TextPresent)
        }
    }
}

async fn execute_live_back(
    session: &mut LiveBrowserSession,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    if cancellation.is_cancelled() {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_cancelled",
                "cancellation",
                "browser_back was cancelled before dispatch",
            ),
            false,
        );
    }
    let history = match session
        .cdp
        .call(
            "Page.getNavigationHistory",
            json!({}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        Ok(history) => history,
        Err(failure) => return interact_operation_outcome(failure, false),
    };
    let current = history
        .get("currentIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if current == 0 {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_back_history_empty",
                "history",
                "current page has no previous navigation entry",
            ),
            false,
        );
    }
    let Some(entry_id) = history
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| entries.get(current as usize - 1))
        .and_then(|entry| entry.get("id"))
        .and_then(Value::as_i64)
    else {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_back_history_invalid",
                "history",
                "previous navigation entry is missing",
            ),
            false,
        );
    };
    session.egress.begin_action();
    if let Err(failure) = session
        .cdp
        .call(
            "Page.navigateToHistoryEntry",
            json!({"entryId":entry_id}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        session.egress.end_action();
        return interact_operation_outcome(failure, true);
    }
    let _ = tokio::time::timeout(
        PAGE_LOAD_DEADLINE,
        session
            .cdp
            .wait_event("Page.frameStoppedLoading", &session.cdp_session_id),
    )
    .await;
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    let post = match capture_current_observation(session).await {
        Ok(observation) => observation,
        Err(failure) => {
            session.egress.end_action();
            return interact_operation_outcome(failure, true);
        }
    };
    session.egress.end_action();
    session.observation = post;
    rotate_observation(session, true);
    if let Some(failure) = session.egress.take_fatal_failure() {
        return extended_failure_with_fresh_observation(session, "back", None, failure, true);
    }
    let mut outcome = browser_result_outcome(
        session,
        Some(BrowserActionResult {
            kind: "back",
            consumed_element_ref: None,
            page_ref: None,
            receipt: None,
        }),
        false,
    );
    outcome.side_effect = ToolSideEffectStatus::Applied;
    outcome
}

async fn execute_live_tab(
    session: &mut LiveBrowserSession,
    request: &BrowserInteractRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    if cancellation.is_cancelled() {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_cancelled",
                "cancellation",
                "browser tab action was cancelled before dispatch",
            ),
            false,
        );
    }
    match request {
        BrowserInteractRequest::TabOpen { url } => {
            if session.background_pages.len() + 1 >= MAX_BROWSER_PAGES {
                return interact_operation_outcome(
                    BrowserFailure::operation(
                        "browser_tab_limit",
                        "tab",
                        format!("browser session is limited to {MAX_BROWSER_PAGES} pages"),
                    ),
                    false,
                );
            }
            let expected = match &session.request.scope {
                BrowserTargetScope::ExactLocal(origin) => origin,
                BrowserTargetScope::Public => &session.egress.initial_origin,
            };
            if Origin::from_url(url).ok().as_ref() != Some(expected) {
                return interact_operation_outcome(
                    BrowserFailure::operation(
                        "browser_tab_origin_escape",
                        "tab",
                        "tab_open only accepts the session's exact authorized origin",
                    ),
                    false,
                );
            }
            session.egress.begin_action();
            session.cdp.allow_next_page_target = true;
            let created = session
                .cdp
                .call("Target.createTarget", json!({"url":"about:blank"}), None)
                .await;
            session.cdp.allow_next_page_target = false;
            let created = match created {
                Ok(created) => created,
                Err(failure) => {
                    session.egress.end_action();
                    return interact_operation_outcome(failure, false);
                }
            };
            let Some(target_id) = created
                .get("targetId")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                session.egress.end_action();
                return interact_operation_outcome(
                    BrowserFailure::operation(
                        "browser_cdp_message_invalid",
                        "tab",
                        "Target.createTarget did not return targetId",
                    ),
                    true,
                );
            };
            let (cdp_session_id, resume_paused) =
                if let Some(session_id) = session.cdp.attached_page_sessions.remove(&target_id) {
                    (session_id, true)
                } else {
                    let attached = match session
                        .cdp
                        .call(
                            "Target.attachToTarget",
                            json!({"targetId":target_id,"flatten":true}),
                            None,
                        )
                        .await
                    {
                        Ok(attached) => attached,
                        Err(failure) => {
                            session.egress.end_action();
                            return interact_operation_outcome(failure, true);
                        }
                    };
                    let Some(session_id) = attached
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                    else {
                        session.egress.end_action();
                        return interact_operation_outcome(
                            BrowserFailure::operation(
                                "browser_cdp_message_invalid",
                                "tab",
                                "Target.attachToTarget did not return sessionId",
                            ),
                            true,
                        );
                    };
                    session.cdp.managed_target_ids.insert(target_id.clone());
                    (session_id, false)
                };
            if let Err(failure) =
                configure_cdp_page(&mut session.cdp, &cdp_session_id, resume_paused).await
            {
                session.egress.end_action();
                return interact_operation_outcome(failure, true);
            }
            let placeholder = BrowserPageState {
                page_ref: format!("page_{}", Uuid::new_v4().simple()),
                target_id: target_id.clone(),
                cdp_session_id: cdp_session_id.clone(),
                page_epoch: 0,
                snapshot_id: String::new(),
                refs: HashMap::new(),
                stale_refs: BTreeSet::new(),
                observation_fingerprint: String::new(),
                observation: session.observation.clone(),
            };
            let previous = session.swap_active_page(placeholder);
            session
                .background_pages
                .insert(previous.page_ref.clone(), previous);
            let navigation = session
                .cdp
                .call(
                    "Page.navigate",
                    json!({"url":url.as_str()}),
                    Some(&session.cdp_session_id),
                )
                .await;
            if let Err(failure) = navigation {
                session.egress.end_action();
                return interact_operation_outcome(failure, true);
            }
            let _ = session
                .cdp
                .call(
                    "Target.activateTarget",
                    json!({"targetId":session.target_id}),
                    None,
                )
                .await;
            let _ = tokio::time::timeout(
                PAGE_LOAD_DEADLINE,
                session
                    .cdp
                    .wait_event("Page.loadEventFired", &session.cdp_session_id),
            )
            .await;
            tokio::time::sleep(RENDER_SETTLE_DELAY).await;
            let post = match capture_current_observation(session).await {
                Ok(observation) => observation,
                Err(failure) => {
                    session.egress.end_action();
                    return interact_operation_outcome(failure, true);
                }
            };
            session.egress.end_action();
            session.observation = post;
            rotate_observation(session, true);
            if let Some(failure) = session.egress.take_fatal_failure() {
                return extended_failure_with_fresh_observation(
                    session, "tab_open", None, failure, true,
                );
            }
            let mut outcome = browser_result_outcome(
                session,
                Some(BrowserActionResult {
                    kind: "tab_open",
                    consumed_element_ref: None,
                    page_ref: Some(session.page_ref.clone()),
                    receipt: None,
                }),
                false,
            );
            outcome.side_effect = ToolSideEffectStatus::Applied;
            outcome
        }
        BrowserInteractRequest::TabSwitch { page_ref } => {
            if page_ref == &session.page_ref {
                return interact_operation_outcome(
                    BrowserFailure::operation(
                        "browser_tab_already_active",
                        "tab",
                        "requested page_ref is already active",
                    ),
                    false,
                );
            }
            let Some(page) = session.background_pages.remove(page_ref) else {
                return interact_operation_outcome(
                    BrowserFailure::operation(
                        "browser_page_ref_stale",
                        "page_ref",
                        "page_ref is not a live page in this session",
                    ),
                    false,
                );
            };
            let previous = session.swap_active_page(page);
            session
                .background_pages
                .insert(previous.page_ref.clone(), previous);
            if let Err(failure) = session
                .cdp
                .call(
                    "Target.activateTarget",
                    json!({"targetId":session.target_id}),
                    None,
                )
                .await
            {
                return interact_operation_outcome(failure, true);
            }
            let observation = match capture_current_observation(session).await {
                Ok(observation) => observation,
                Err(failure) => return interact_operation_outcome(failure, true),
            };
            session.observation = observation;
            rotate_observation(session, true);
            let mut outcome = browser_result_outcome(
                session,
                Some(BrowserActionResult {
                    kind: "tab_switch",
                    consumed_element_ref: None,
                    page_ref: Some(session.page_ref.clone()),
                    receipt: None,
                }),
                false,
            );
            outcome.side_effect = ToolSideEffectStatus::Applied;
            outcome
        }
        BrowserInteractRequest::TabClose { page_ref } => {
            if page_ref == &session.page_ref {
                let Some(next_ref) = session.background_pages.keys().next().cloned() else {
                    return interact_operation_outcome(
                        BrowserFailure::operation(
                            "browser_tab_last_page_denied",
                            "tab",
                            "cannot close the only live page",
                        ),
                        false,
                    );
                };
                let next = session
                    .background_pages
                    .remove(&next_ref)
                    .expect("selected background page exists");
                let closing = session.swap_active_page(next);
                let _ = session
                    .cdp
                    .call(
                        "Target.closeTarget",
                        json!({"targetId":closing.target_id}),
                        None,
                    )
                    .await;
                session.cdp.managed_target_ids.remove(&closing.target_id);
            } else {
                let Some(closing) = session.background_pages.remove(page_ref) else {
                    return interact_operation_outcome(
                        BrowserFailure::operation(
                            "browser_page_ref_stale",
                            "page_ref",
                            "page_ref is not a live page in this session",
                        ),
                        false,
                    );
                };
                let _ = session
                    .cdp
                    .call(
                        "Target.closeTarget",
                        json!({"targetId":closing.target_id}),
                        None,
                    )
                    .await;
                session.cdp.managed_target_ids.remove(&closing.target_id);
            }
            let observation = match capture_current_observation(session).await {
                Ok(observation) => observation,
                Err(failure) => return interact_operation_outcome(failure, true),
            };
            session.observation = observation;
            rotate_observation(session, true);
            let mut outcome = browser_result_outcome(
                session,
                Some(BrowserActionResult {
                    kind: "tab_close",
                    consumed_element_ref: None,
                    page_ref: Some(session.page_ref.clone()),
                    receipt: None,
                }),
                false,
            );
            outcome.side_effect = ToolSideEffectStatus::Applied;
            outcome
        }
        _ => unreachable!(),
    }
}

async fn execute_live_submit(
    session: &mut LiveBrowserSession,
    run_id: &str,
    request: &BrowserClickRequest,
    cancellation: BrowserCancellationToken,
) -> ToolOutcome {
    if !matches!(session.request.scope, BrowserTargetScope::Public) {
        return interact_operation_outcome(
            BrowserFailure::operation(
                "browser_submit_public_scope_required",
                "submit",
                "scoped submit is limited to an authorized disposable public origin",
            ),
            false,
        );
    }
    let fresh = match capture_current_observation(session).await {
        Ok(observation) => observation,
        Err(failure) => return interact_operation_outcome(failure, false),
    };
    if cancellation.is_cancelled() {
        return invalidate_extended_with_failure(
            session,
            "submit",
            Some(request.element_ref()),
            fresh,
            BrowserFailure::operation(
                "browser_cancelled",
                "cancellation",
                "browser_submit was cancelled before dispatch",
            ),
        )
        .await;
    }
    let target = match resolve_element_ref(
        &session.run_id,
        run_id,
        &session.refs,
        &session.stale_refs,
        request.element_ref(),
    ) {
        Ok(target) => target,
        Err(failure) => {
            return invalidate_extended_with_failure(
                session,
                "submit",
                Some(request.element_ref()),
                fresh,
                failure,
            )
            .await;
        }
    };
    if !target.capabilities.contains(&ElementCapability::Submit) {
        return invalidate_extended_with_failure(
            session,
            "submit",
            Some(request.element_ref()),
            fresh,
            BrowserFailure::operation(
                "browser_submit_target_denied",
                "element_ref",
                "element_ref is not a Host-classified reversible draft submit target",
            ),
        )
        .await;
    }
    let previews = derive_external_action_previews(&fresh.final_url, &fresh.nodes);
    let Some(current_preview) = previews.get(&target.backend_dom_node_id) else {
        return invalidate_extended_with_failure(
            session,
            "submit",
            Some(request.element_ref()),
            fresh,
            BrowserFailure::operation(
                "browser_submit_preview_stale",
                "submit",
                "external action preview no longer matches the current page",
            ),
        )
        .await;
    };
    if target.external_preview.as_ref() != Some(current_preview)
        || observation_fingerprint(&fresh.nodes) != session.observation_fingerprint
    {
        return invalidate_extended_with_failure(
            session,
            "submit",
            Some(request.element_ref()),
            fresh,
            BrowserFailure::operation(
                "browser_submit_preview_stale",
                "submit",
                "target, parameters, or impact changed after durable authorization",
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
        Err(failure) => return interact_operation_outcome(failure, false),
    };
    let Some((x, y)) = box_center(&model) else {
        return invalidate_extended_with_failure(
            session,
            "submit",
            Some(request.element_ref()),
            fresh,
            absent_target_failure(true),
        )
        .await;
    };
    let grant = ExternalRequestGrant {
        target_url: current_preview.target_url.clone(),
        parameters: current_preview.parameters.clone(),
        ignored_empty_parameters: current_preview.ignored_empty_parameters.clone(),
        parameters_sha256: current_preview.parameters_sha256.clone(),
        impact: current_preview.impact,
    };
    session.egress.begin_external_action(grant.clone());
    if let Err(failure) = session
        .cdp
        .call(
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        session.egress.finish_external_action();
        return interact_operation_outcome(failure, false);
    }
    if let Err(failure) = session
        .cdp
        .call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
            Some(&session.cdp_session_id),
        )
        .await
    {
        session.egress.finish_external_action();
        return interact_operation_outcome(failure, true);
    }
    let _ = tokio::time::timeout(
        PAGE_LOAD_DEADLINE,
        session
            .cdp
            .wait_event("Page.frameStoppedLoading", &session.cdp_session_id),
    )
    .await;
    tokio::time::sleep(RENDER_SETTLE_DELAY).await;
    let post = match capture_current_observation(session).await {
        Ok(observation) => observation,
        Err(failure) => {
            session.egress.finish_external_action();
            return interact_operation_outcome(
                session.egress.take_fatal_failure().unwrap_or(failure),
                true,
            );
        }
    };
    let observed = session.egress.finish_external_action();
    session.observation = post;
    rotate_observation(session, true);
    if let Some(failure) = session.egress.take_fatal_failure() {
        return extended_failure_with_fresh_observation(
            session,
            "submit",
            Some(request.element_ref()),
            failure,
            true,
        );
    }
    let semantic_receipt = session.observation.nodes.iter().find_map(|node| {
        node.state
            .get("data-receipt")
            .filter(|receipt| !receipt.is_empty())
            .cloned()
    });
    let Some(observed) = observed else {
        return extended_failure_with_fresh_observation(
            session,
            "submit",
            Some(request.element_ref()),
            BrowserFailure::operation(
                "browser_external_receipt_missing",
                "receipt",
                "external request started but no matching response receipt was observed",
            ),
            true,
        );
    };
    let remote_receipt = (!observed.remote_receipt.is_empty())
        .then_some(observed.remote_receipt)
        .or_else(|| semantic_receipt.clone());
    let Some(remote_receipt) = remote_receipt else {
        return extended_failure_with_fresh_observation(
            session,
            "submit",
            Some(request.element_ref()),
            BrowserFailure::operation(
                "browser_external_receipt_missing",
                "receipt",
                "external response did not provide a bounded receipt",
            ),
            true,
        );
    };
    if !(200..300).contains(&observed.status) {
        return extended_failure_with_fresh_observation(
            session,
            "submit",
            Some(request.element_ref()),
            BrowserFailure::operation(
                "browser_external_action_failed",
                "receipt",
                format!("external action returned HTTP {}", observed.status),
            ),
            true,
        );
    }
    let receipt = ExternalActionReceipt {
        target_url: observed.target_url,
        method: "POST",
        status: observed.status,
        parameters_sha256: grant.parameters_sha256,
        impact: grant.impact,
        remote_receipt: bounded_text(&remote_receipt, 512),
        semantic_receipt: semantic_receipt.map(|receipt| bounded_text(&receipt, 512)),
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    };
    let mut outcome = browser_result_outcome(
        session,
        Some(BrowserActionResult {
            kind: "submit",
            consumed_element_ref: Some(request.element_ref().to_owned()),
            page_ref: Some(session.page_ref.clone()),
            receipt: Some(receipt),
        }),
        false,
    );
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

async fn configure_cdp_page(
    cdp: &mut CdpClient,
    session_id: &str,
    resume_paused_target: bool,
) -> Result<(), BrowserFailure> {
    cdp.call("Page.enable", json!({}), Some(session_id)).await?;
    cdp.call("Network.enable", json!({}), Some(session_id))
        .await?;
    cdp.call(
        "Network.setCacheDisabled",
        json!({"cacheDisabled":true}),
        Some(session_id),
    )
    .await?;
    cdp.call(
        "Network.setBypassServiceWorker",
        json!({"bypass":true}),
        Some(session_id),
    )
    .await?;
    cdp.call("Network.clearBrowserCookies", json!({}), Some(session_id))
        .await?;
    cdp.call("Accessibility.enable", json!({}), Some(session_id))
        .await?;
    cdp.call(
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
        Some(session_id),
    )
    .await?;
    if resume_paused_target {
        cdp.call(
            "Runtime.runIfWaitingForDebugger",
            json!({}),
            Some(session_id),
        )
        .await?;
    }
    Ok(())
}

async fn run_cdp_session(
    request: &BrowserNavigateRequest,
    profile: &Path,
    egress: Arc<EgressState>,
) -> Result<(CdpClient, String, String, SessionObservation), BrowserFailure> {
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
        pending_commands: HashMap::new(),
        observed_events: BTreeSet::new(),
        egress,
        primary_target_id: None,
        managed_target_ids: BTreeSet::new(),
        allow_next_page_target: false,
        attached_page_sessions: HashMap::new(),
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
    cdp.managed_target_ids.insert(target_id.clone());
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
    configure_cdp_page(&mut cdp, &session_id, false).await?;
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
    Ok((cdp, target_id, session_id, observation))
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
                    | "data-receipt"
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
                    | "contenteditable"
                    | "form"
                    | "formaction"
                    | "formmethod"
                    | "href"
                    | "value"
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
        let lower_role = role.to_ascii_lowercase();
        let click_safety = if matches!(lower_role.as_str(), "button" | "switch") {
            Some(
                if state.iter().any(|(name, value)| {
                    matches!(name.as_str(), "disabled" | "aria-disabled")
                        && value.eq_ignore_ascii_case("true")
                }) {
                    ClickSafety::Disabled
                } else if dom.as_ref().is_some_and(|dom| {
                    dom.attributes.contains_key("form")
                        || dom
                            .attributes
                            .get("type")
                            .is_some_and(|value| value.eq_ignore_ascii_case("submit"))
                }) {
                    ClickSafety::SideEffectDenied
                } else {
                    ClickSafety::Allowed
                },
            )
        } else if lower_role == "link" {
            Some(ClickSafety::Allowed)
        } else {
            None
        };
        let fill_safety = classify_fill_safety(&role, &accessible_name, dom, &state);
        let mut capabilities = Vec::new();
        if click_safety == Some(ClickSafety::Allowed) {
            capabilities.push("click");
        }
        if fill_safety == Some(FillSafety::Allowed) {
            capabilities.push("fill");
        }
        if click_safety == Some(ClickSafety::Allowed)
            || fill_safety == Some(FillSafety::Allowed)
            || dom.as_ref().is_some_and(|dom| dom.node_name == "SELECT")
        {
            capabilities.push("press");
        }
        if dom.as_ref().is_some_and(|dom| dom.node_name == "SELECT") {
            capabilities.push("select");
        }
        if matches!(
            lower_role.as_str(),
            "region" | "list" | "listbox" | "document"
        ) {
            capabilities.push("scroll");
        }
        eligible.push(SemanticNode {
            element_ref: None,
            role: bounded_text(&role, 512),
            accessible_name: bounded_text(&accessible_name, 1_024),
            text,
            value: bounded_text(&value, 1_024),
            state,
            capabilities,
            backend_dom_node_id,
            click_safety,
            fill_safety,
            dom: dom.cloned(),
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
    let editable = dom.node_name == "INPUT"
        || dom.node_name == "TEXTAREA"
        || dom.attributes.get("contenteditable").is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "true" | "plaintext-only"
            )
        });
    if !editable {
        return Some(FillSafety::Ineligible);
    }
    let input_type = (dom.node_name == "INPUT").then(|| {
        dom.attributes
            .get("type")
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text".to_owned())
    });
    if input_type
        .as_deref()
        .is_some_and(|input_type| !matches!(input_type, "text" | "search"))
    {
        return Some(if input_type.as_deref() == Some("password") {
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
                capabilities: BTreeSet::from([ElementCapability::Click]),
                external_preview: None,
            },
            SemanticNode {
                element_ref: None,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                text: name.to_owned(),
                value: String::new(),
                state: BTreeMap::new(),
                capabilities: vec!["click"],
                backend_dom_node_id: Some(backend_dom_node_id),
                click_safety: Some(safety),
                fill_safety: None,
                dom: None,
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
                capabilities: BTreeSet::from([ElementCapability::Fill]),
                external_preview: None,
            },
            SemanticNode {
                element_ref: None,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                text: name.to_owned(),
                value: String::new(),
                state: BTreeMap::new(),
                capabilities: vec!["fill"],
                backend_dom_node_id: Some(backend_dom_node_id),
                click_safety: None,
                fill_safety: Some(safety),
                dom: None,
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
    fn host_synthetic_capability_does_not_stale_semantic_fingerprint() {
        let (_, mut node) = semantic_target(7, "button", "Save draft", ClickSafety::Allowed);
        let before = observation_fingerprint(std::slice::from_ref(&node));
        node.capabilities.push("submit");
        assert_eq!(
            observation_fingerprint(std::slice::from_ref(&node)),
            before,
            "Host-derived authorization capabilities are not page semantic mutation"
        );
        node.accessible_name = "Publish".to_owned();
        assert_ne!(observation_fingerprint(&[node]), before);
    }

    #[test]
    fn typed_interaction_parser_admits_the_cluster_and_rejects_argument_escape() {
        let element_ref = "eref_0123456789abcdef0123456789abcdef";
        let page_ref = "page_0123456789abcdef0123456789abcdef";
        let admitted = [
            json!({"action":"click","element_ref":element_ref}),
            json!({"action":"fill","element_ref":element_ref,"value":"ready"}),
            json!({"action":"press","element_ref":element_ref,"key":"enter"}),
            json!({"action":"wait","condition":"document_ready","timeout_ms":100}),
            json!({"action":"wait","condition":"text_present","value":"ready","timeout_ms":100}),
            json!({"action":"scroll","element_ref":element_ref,"direction":"down","amount":200}),
            json!({"action":"select","element_ref":element_ref,"value":"staging"}),
            json!({"action":"back"}),
            json!({"action":"tab_open","url":"https://example.com/review"}),
            json!({"action":"tab_switch","page_ref":page_ref}),
            json!({"action":"tab_close","page_ref":page_ref}),
            json!({"action":"submit","element_ref":element_ref}),
        ];
        for input in admitted {
            parse_interact_request(&input).unwrap_or_else(|failure| {
                panic!("admitted input failed {}: {input}", failure.code)
            });
        }
        for denied in [
            json!({"action":"press","element_ref":element_ref,"key":"meta"}),
            json!({"action":"wait","condition":"text_present","timeout_ms":100}),
            json!({"action":"wait","condition":"document_ready","value":"forbidden"}),
            json!({"action":"scroll","element_ref":element_ref,"direction":"left"}),
            json!({"action":"tab_open","url":"file:///tmp/escape"}),
            json!({"action":"click","element_ref":element_ref,"selector":"button"}),
            json!({"action":"submit","element_ref":element_ref,"headers":{}}),
        ] {
            assert!(parse_interact_request(&denied).is_err(), "{denied}");
        }
    }

    #[test]
    fn reversible_draft_preview_and_post_grant_are_exact_and_sensitive_free() {
        let node =
            |backend_dom_node_id, role: &str, name: &str, value: &str, attributes| SemanticNode {
                element_ref: None,
                role: role.to_owned(),
                accessible_name: name.to_owned(),
                text: name.to_owned(),
                value: value.to_owned(),
                state: BTreeMap::new(),
                capabilities: Vec::new(),
                backend_dom_node_id: Some(backend_dom_node_id),
                click_safety: None,
                fill_safety: Some(FillSafety::Allowed),
                dom: Some(DomFacts {
                    node_name: if role == "button" {
                        "BUTTON"
                    } else {
                        "TEXTAREA"
                    }
                    .to_owned(),
                    node_value: String::new(),
                    attributes,
                }),
            };
        let fields = vec![
            node(
                1,
                "textbox",
                "Draft notes",
                "reviewed",
                BTreeMap::from([
                    ("form".to_owned(), "draft-form".to_owned()),
                    ("name".to_owned(), "notes".to_owned()),
                ]),
            ),
            SemanticNode {
                fill_safety: Some(FillSafety::SensitiveDenied),
                ..node(
                    2,
                    "textbox",
                    "Password",
                    "must-not-leak",
                    BTreeMap::from([
                        ("form".to_owned(), "draft-form".to_owned()),
                        ("name".to_owned(), "password".to_owned()),
                    ]),
                )
            },
            node(
                3,
                "button",
                "Save draft",
                "",
                BTreeMap::from([
                    ("form".to_owned(), "draft-form".to_owned()),
                    ("formmethod".to_owned(), "post".to_owned()),
                    ("formaction".to_owned(), "/drafts/save".to_owned()),
                    ("name".to_owned(), "action".to_owned()),
                    ("value".to_owned(), "save_draft".to_owned()),
                ]),
            ),
            node(
                4,
                "button",
                "Publish",
                "",
                BTreeMap::from([
                    ("form".to_owned(), "draft-form".to_owned()),
                    ("formmethod".to_owned(), "post".to_owned()),
                    ("formaction".to_owned(), "/publish".to_owned()),
                ]),
            ),
        ];
        let previews = derive_external_action_previews("https://example.com/review", &fields);
        assert_eq!(previews.len(), 1);
        assert!(
            !previews.contains_key(&4),
            "publish must not gain a submit ref"
        );
        let preview = previews.get(&3).expect("reversible draft preview");
        assert_eq!(preview.target_url, "https://example.com/drafts/save");
        assert_eq!(
            preview.parameters,
            vec![
                ("action".to_owned(), "save_draft".to_owned()),
                ("notes".to_owned(), "reviewed".to_owned()),
            ]
        );
        assert!(
            !preview
                .parameters
                .iter()
                .any(|(name, _)| name == "password"),
            "sensitive values never enter the durable preview"
        );
        assert_eq!(
            preview.ignored_empty_parameters,
            BTreeSet::from(["password".to_owned()])
        );

        let request = parse_request(&input("https://example.com/review"), None).unwrap();
        let egress = EgressState::new(&request);
        egress.begin_external_action(ExternalRequestGrant {
            target_url: preview.target_url.clone(),
            parameters: preview.parameters.clone(),
            ignored_empty_parameters: preview.ignored_empty_parameters.clone(),
            parameters_sha256: preview.parameters_sha256.clone(),
            impact: preview.impact,
        });
        egress
            .authorize_cdp_request(
                "https://example.com/drafts/save",
                "POST",
                Some("Document"),
                Some("notes=reviewed&action=save_draft"),
            )
            .expect("exact canonical POST grant");
        egress
            .authorize_cdp_request(
                "https://example.com/drafts/save",
                "POST",
                Some("Document"),
                Some("notes=reviewed&password=&action=save_draft"),
            )
            .expect("empty sensitive control is excluded without entering the preview");
        assert_eq!(
            egress
                .authorize_cdp_request(
                    "https://example.com/drafts/save",
                    "POST",
                    Some("Document"),
                    Some("notes=reviewed&password=secret&action=save_draft"),
                )
                .unwrap_err()
                .code,
            "browser_method_denied"
        );
        for (url, body) in [
            (
                "https://example.com/publish",
                "notes=reviewed&action=save_draft",
            ),
            (
                "https://example.com/drafts/save",
                "notes=changed&action=save_draft",
            ),
            (
                "https://example.com/drafts/save",
                "notes=reviewed&action=publish",
            ),
        ] {
            assert_eq!(
                egress
                    .authorize_cdp_request(url, "POST", Some("Document"), Some(body))
                    .unwrap_err()
                    .code,
                "browser_method_denied"
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
                FillSafety::Allowed,
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
            .authorize_cdp_request("https://example.com/app", "GET", Some("Document"), None)
            .unwrap();
        public
            .authorize_cdp_request(
                "https://cdn.example.net/app.js",
                "GET",
                Some("Script"),
                None,
            )
            .unwrap();
        assert_eq!(
            public
                .authorize_cdp_request("https://other.example/", "GET", Some("Document"), None,)
                .unwrap_err()
                .code,
            "browser_cross_origin_navigation_denied"
        );
        assert_eq!(
            public
                .authorize_cdp_request("https://example.com/form", "POST", Some("Fetch"), None,)
                .unwrap_err()
                .code,
            "browser_method_denied"
        );
        for index in 0..=MAX_REDIRECTS {
            let result = public.authorize_cdp_request(
                &format!("https://example.com/redirect-{index}"),
                "GET",
                Some("Document"),
                None,
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
                .authorize_cdp_request("http://127.0.0.1:32124/", "GET", Some("Script"), None,)
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
