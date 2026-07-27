use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use flate2::read::MultiGzDecoder;
use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, USER_AGENT,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{ToolOutcome, optional_u64, required_str};
use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
    ToolTransportStatus,
};

const DEFAULT_MAX_CHARS: usize = 20_000;
const MAX_MAX_CHARS: usize = 50_000;
const MAX_URL_CHARS: usize = 4_096;
const MAX_REDIRECTS: usize = 5;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINKS: usize = 32;
const MAX_LINK_CHARS: usize = 2_048;
const MAX_TITLE_CHARS: usize = 512;
const FETCH_DEADLINE: Duration = Duration::from_secs(15);
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const NETWORK_IDENTITY: &str = "rustls_pinned_public_https_v1";
const TRUST: &str = "external_untrusted";

/// One response returned by the narrow HTTP seam used by `web_fetch`.
///
/// Production uses [`SystemWebFetchNetwork`]. The seam exists so the real
/// AgentApplication/AgentRuntime path can prove replay without making tests
/// depend on public network state. All URL, DNS/IP, redirect, type and size
/// gates remain above this seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFetchHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub bytes_read: u64,
}

impl WebFetchHttpResponse {
    #[must_use]
    pub fn new(status: u16, headers: BTreeMap<String, String>, body: Vec<u8>) -> Self {
        Self {
            status,
            bytes_read: u64::try_from(body.len()).unwrap_or(u64::MAX),
            headers,
            body,
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// Typed failure from the narrow DNS/HTTP transport seam.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WebFetchNetworkError {
    pub code: &'static str,
    pub message: String,
}

impl WebFetchNetworkError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Narrow, stateless DNS/HTTP port for one bounded fetch.
#[async_trait]
pub trait WebFetchNetwork: Send + Sync {
    fn identity(&self) -> &str;

    async fn resolve(&self, host: &str, port: u16)
    -> Result<Vec<SocketAddr>, WebFetchNetworkError>;

    async fn get(
        &self,
        url: &Url,
        pinned_addresses: &[SocketAddr],
        timeout: Duration,
    ) -> Result<WebFetchHttpResponse, WebFetchNetworkError>;
}

/// Production DNS + Rustls implementation. Redirects, proxies, cookies and
/// automatic decompression are intentionally disabled.
#[derive(Debug, Default)]
pub struct SystemWebFetchNetwork;

#[async_trait]
impl WebFetchNetwork for SystemWebFetchNetwork {
    fn identity(&self) -> &str {
        NETWORK_IDENTITY
    }

    async fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
        tokio::net::lookup_host((host, port))
            .await
            .map(|addresses| addresses.collect())
            .map_err(|error| {
                WebFetchNetworkError::new("web_dns_failed", format!("DNS 解析失败：{error}"))
            })
    }

    async fn get(
        &self,
        url: &Url,
        pinned_addresses: &[SocketAddr],
        timeout: Duration,
    ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
        let host = url.host_str().ok_or_else(|| {
            WebFetchNetworkError::new("web_url_host_missing", "HTTPS URL 缺少主机名")
        })?;
        let expected_port = url.port_or_known_default().unwrap_or(443);
        if pinned_addresses.is_empty()
            || pinned_addresses
                .iter()
                .any(|address| address.port() != expected_port || !is_public_ip(address.ip()))
        {
            return Err(WebFetchNetworkError::new(
                "web_connect_target_denied",
                "连接目标未通过 public HTTPS IP 安全门",
            ));
        }

        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .no_gzip()
            .https_only(true)
            .connect_timeout(CONNECT_DEADLINE.min(timeout))
            .timeout(timeout)
            .resolve_to_addrs(host, pinned_addresses)
            .build()
            .map_err(|error| {
                WebFetchNetworkError::new(
                    "web_http_client_failed",
                    format!("无法构造受限 HTTPS client：{error}"),
                )
            })?;
        let response = client
            .get(url.clone())
            .header(
                ACCEPT,
                "text/html, application/xhtml+xml;q=0.9, text/plain;q=0.8",
            )
            .header(ACCEPT_ENCODING, "gzip, identity")
            .header(
                USER_AGENT,
                concat!("DSE/", env!("CARGO_PKG_VERSION"), " web_fetch"),
            )
            .send()
            .await
            .map_err(|error| {
                let code = if error.is_timeout() {
                    "web_request_timeout"
                } else if error.is_connect() {
                    "web_connect_failed"
                } else {
                    "web_request_failed"
                };
                WebFetchNetworkError::new(code, format!("HTTPS 请求失败：{error}"))
            })?;

        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for name in [CONTENT_TYPE, CONTENT_ENCODING, CONTENT_LENGTH, LOCATION] {
            if let Some(value) = response.headers().get(&name) {
                let value = value.to_str().map_err(|_| {
                    WebFetchNetworkError::new(
                        "web_header_invalid",
                        format!("响应头 {} 不是有效 ASCII", name.as_str()),
                    )
                })?;
                headers.insert(name.as_str().to_owned(), value.to_owned());
            }
        }

        if is_redirect_status(status) {
            return Ok(WebFetchHttpResponse {
                status,
                headers,
                body: Vec::new(),
                bytes_read: 0,
            });
        }
        if let Some(length) = headers
            .get(CONTENT_LENGTH.as_str())
            .and_then(|value| value.parse::<u64>().ok())
            && length > MAX_RESPONSE_BYTES as u64
        {
            return Err(WebFetchNetworkError::new(
                "web_response_too_large",
                format!("响应声明 {length} bytes，超过 {MAX_RESPONSE_BYTES} bytes 上限"),
            ));
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                WebFetchNetworkError::new(
                    "web_body_read_failed",
                    format!("读取 HTTPS response body 失败：{error}"),
                )
            })?;
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(WebFetchNetworkError::new(
                    "web_response_too_large",
                    format!("响应超过 {MAX_RESPONSE_BYTES} bytes 上限"),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(WebFetchHttpResponse::new(status, headers, body))
    }
}

#[derive(Debug)]
struct FetchFailure {
    code: &'static str,
    stage: &'static str,
    message: String,
    final_url: Option<String>,
    status: Option<u16>,
    transport: bool,
    retry: ToolRetryDisposition,
}

impl FetchFailure {
    fn rejected(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            final_url: None,
            status: None,
            transport: false,
            retry: ToolRetryDisposition::NotRetryable,
        }
    }

    fn operation(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            final_url: None,
            status: None,
            transport: false,
            retry: ToolRetryDisposition::NotRetryable,
        }
    }

    fn transport(code: &'static str, stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            final_url: None,
            status: None,
            transport: true,
            retry: ToolRetryDisposition::Safe,
        }
    }

    fn at_url(mut self, url: &Url) -> Self {
        self.final_url = Some(url.as_str().to_owned());
        self
    }

    fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }
}

#[derive(Debug, Serialize)]
struct WebFetchResult {
    requested_url: String,
    final_url: String,
    status: u16,
    media_type: String,
    title: Option<String>,
    text: String,
    links: Vec<String>,
    retrieved_at: String,
    source_sha256: String,
    bytes_read: u64,
    bytes_returned: u64,
    truncated: bool,
    trust: &'static str,
}

#[derive(Debug)]
struct ExtractedPage {
    title: Option<String>,
    text: String,
    links: Vec<String>,
    links_truncated: bool,
}

pub(crate) fn preflight_web_fetch(input: &Value) -> Option<ToolOutcome> {
    let requested = match required_str(input, "url") {
        Ok(value) => value,
        Err(error) => {
            return Some(
                ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::AfterCorrection)
                    .with_failure_code(ToolFailureCode::MissingField),
            );
        }
    };
    match parse_and_validate_url(requested) {
        Ok(_) => None,
        Err(failure) => Some(rejected_outcome(requested, failure)),
    }
}

pub(crate) async fn execute_web_fetch(
    input: Value,
    network: Arc<dyn WebFetchNetwork>,
    network_allowed: bool,
) -> ToolOutcome {
    execute_web_fetch_with_deadline(input, network, network_allowed, FETCH_DEADLINE).await
}

async fn execute_web_fetch_with_deadline(
    input: Value,
    network: Arc<dyn WebFetchNetwork>,
    network_allowed: bool,
    deadline: Duration,
) -> ToolOutcome {
    let requested = match required_str(&input, "url") {
        Ok(value) => value.to_owned(),
        Err(error) => {
            return ToolOutcome::error(error.to_string())
                .with_failure_code(ToolFailureCode::InvalidField)
                .with_side_effect(ToolSideEffectStatus::NotApplicable);
        }
    };
    if !network_allowed {
        return operation_outcome(
            &requested,
            FetchFailure::operation(
                "web_network_not_authorized",
                "authorization",
                "当前 Run permission 或 actor sandbox 禁止网络访问",
            ),
        );
    }
    let max_chars = usize::try_from(optional_u64(&input, "max_chars", DEFAULT_MAX_CHARS as u64))
        .unwrap_or(MAX_MAX_CHARS)
        .clamp(1, MAX_MAX_CHARS);
    let initial = match parse_and_validate_url(&requested) {
        Ok(url) => url,
        Err(failure) => return operation_outcome(&requested, failure),
    };

    match tokio::time::timeout(
        deadline,
        fetch_inner(&requested, initial, max_chars, network.as_ref()),
    )
    .await
    {
        Ok(Ok(result)) => ToolOutcome::json(&result)
            .expect("bounded web_fetch result is serializable")
            .with_side_effect(ToolSideEffectStatus::NotApplicable),
        Ok(Err(failure)) => operation_outcome(&requested, failure),
        Err(_) => operation_outcome(
            &requested,
            FetchFailure::transport(
                "web_deadline_exceeded",
                "deadline",
                format!("web_fetch 超过 {} ms 总时限", deadline.as_millis()),
            ),
        ),
    }
}

async fn fetch_inner(
    requested: &str,
    initial: Url,
    max_chars: usize,
    network: &dyn WebFetchNetwork,
) -> Result<WebFetchResult, FetchFailure> {
    let mut current = initial;
    let mut visited = HashSet::from([current.as_str().to_owned()]);
    let mut redirects = 0usize;

    loop {
        validate_url_host(&current).map_err(|failure| failure.at_url(&current))?;
        let host = current.host_str().ok_or_else(|| {
            FetchFailure::operation("web_url_host_missing", "url", "HTTPS URL 缺少主机名")
                .at_url(&current)
        })?;
        let port = current.port_or_known_default().unwrap_or(443);
        let mut addresses = network.resolve(host, port).await.map_err(|error| {
            FetchFailure::transport(error.code, "dns", error.message).at_url(&current)
        })?;
        validate_resolved_addresses(&addresses, port)
            .map_err(|failure| failure.at_url(&current))?;
        addresses.sort_unstable();
        addresses.dedup();

        let response = network
            .get(&current, &addresses, FETCH_DEADLINE)
            .await
            .map_err(|error| {
                FetchFailure::transport(error.code, "request", error.message).at_url(&current)
            })?;
        if let Some(length) = response
            .header(CONTENT_LENGTH.as_str())
            .and_then(|value| value.parse::<u64>().ok())
            && length > MAX_RESPONSE_BYTES as u64
        {
            return Err(FetchFailure::operation(
                "web_response_too_large",
                "body",
                format!("响应声明 {length} bytes，超过 {MAX_RESPONSE_BYTES} bytes 上限"),
            )
            .at_url(&current)
            .with_status(response.status));
        }
        if response.body.len() > MAX_RESPONSE_BYTES
            || response.bytes_read > MAX_RESPONSE_BYTES as u64
        {
            return Err(FetchFailure::operation(
                "web_response_too_large",
                "body",
                format!("响应超过 {MAX_RESPONSE_BYTES} bytes 上限"),
            )
            .at_url(&current)
            .with_status(response.status));
        }

        if is_redirect_status(response.status) {
            if redirects >= MAX_REDIRECTS {
                return Err(FetchFailure::operation(
                    "web_redirect_limit",
                    "redirect",
                    format!("redirect 超过 {MAX_REDIRECTS} 次上限"),
                )
                .at_url(&current)
                .with_status(response.status));
            }
            let location = response.header(LOCATION.as_str()).ok_or_else(|| {
                FetchFailure::operation(
                    "web_redirect_location_missing",
                    "redirect",
                    "redirect response 缺少 Location",
                )
                .at_url(&current)
                .with_status(response.status)
            })?;
            let mut next = current.join(location).map_err(|error| {
                FetchFailure::operation(
                    "web_redirect_invalid",
                    "redirect",
                    format!("redirect Location 无效：{error}"),
                )
                .at_url(&current)
                .with_status(response.status)
            })?;
            next.set_fragment(None);
            validate_url_host(&next)
                .map_err(|failure| failure.at_url(&next).with_status(response.status))?;
            if !visited.insert(next.as_str().to_owned()) {
                return Err(FetchFailure::operation(
                    "web_redirect_loop",
                    "redirect",
                    "redirect 形成循环",
                )
                .at_url(&next)
                .with_status(response.status));
            }
            redirects += 1;
            current = next;
            continue;
        }

        if !(200..300).contains(&response.status) {
            return Err(FetchFailure::operation(
                "web_http_status",
                "response",
                format!("HTTPS response status {} 不可读取", response.status),
            )
            .at_url(&current)
            .with_status(response.status));
        }
        let media = parse_media_type(response.header(CONTENT_TYPE.as_str()))
            .map_err(|failure| failure.at_url(&current).with_status(response.status))?;
        let source =
            decode_response_body(&response.body, response.header(CONTENT_ENCODING.as_str()))
                .map_err(|failure| failure.at_url(&current).with_status(response.status))?;
        let source_text = decode_utf8_source(&source, &media.charset)
            .map_err(|failure| failure.at_url(&current).with_status(response.status))?;
        let extracted = if media.is_html {
            extract_html(source_text, &current)
        } else {
            ExtractedPage {
                title: None,
                text: normalize_plain_text(source_text),
                links: Vec::new(),
                links_truncated: false,
            }
        };
        let (text, text_truncated) = truncate_chars(&extracted.text, max_chars);
        let source_sha256 = format!(
            "sha256:{}",
            Sha256::digest(&source)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let bytes_returned = u64::try_from(text.len()).unwrap_or(u64::MAX);
        return Ok(WebFetchResult {
            requested_url: requested.to_owned(),
            final_url: current.as_str().to_owned(),
            status: response.status,
            media_type: media.media_type,
            title: extracted.title,
            text,
            links: extracted.links,
            retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source_sha256,
            bytes_read: response.bytes_read,
            bytes_returned,
            truncated: text_truncated || extracted.links_truncated,
            trust: TRUST,
        });
    }
}

fn rejected_outcome(requested: &str, failure: FetchFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!("web_fetch 拒绝：code={}；{}", failure.code, failure.message),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(failure_metadata(requested, &failure))
}

fn operation_outcome(requested: &str, failure: FetchFailure) -> ToolOutcome {
    let metadata = failure_metadata(requested, &failure);
    let mut outcome = if failure.transport {
        let mut outcome = ToolOutcome::transport_failure(format!(
            "web_fetch 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.retry = failure.retry;
        outcome
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "web_fetch 失败：code={}；{}",
            failure.code, failure.message
        ));
        outcome.operation = ToolOperationStatus::Failed;
        outcome.transport = ToolTransportStatus::Succeeded;
        outcome.retry = failure.retry;
        outcome
    };
    outcome.side_effect = ToolSideEffectStatus::NotApplicable;
    outcome.metadata = Some(metadata);
    outcome
}

fn failure_metadata(requested: &str, failure: &FetchFailure) -> Value {
    json!({
        "web_fetch": {
            "requested_url": requested,
            "final_url": failure.final_url,
            "status": failure.status,
            "trust": TRUST,
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            }
        }
    })
}

fn parse_and_validate_url(requested: &str) -> Result<Url, FetchFailure> {
    if requested.trim() != requested || requested.chars().count() > MAX_URL_CHARS {
        return Err(FetchFailure::rejected(
            "web_url_invalid",
            "url",
            format!("URL 必须无首尾空白且不超过 {MAX_URL_CHARS} 字符"),
        ));
    }
    let mut url = Url::parse(requested).map_err(|error| {
        FetchFailure::rejected("web_url_invalid", "url", format!("URL 解析失败：{error}"))
    })?;
    url.set_fragment(None);
    validate_url_host(&url)?;
    Ok(url)
}

fn validate_url_host(url: &Url) -> Result<(), FetchFailure> {
    if url.scheme() != "https" {
        return Err(FetchFailure::rejected(
            "web_scheme_denied",
            "url",
            "只允许 https URL",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchFailure::rejected(
            "web_userinfo_denied",
            "url",
            "URL 不允许包含 userinfo、认证信息或密码",
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        FetchFailure::rejected("web_url_host_missing", "url", "HTTPS URL 缺少主机名")
    })?;
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost"
        || normalized.ends_with(".localhost")
        || matches!(
            normalized.as_str(),
            "metadata.google.internal"
                | "instance-data.ec2.internal"
                | "metadata.azure.internal"
                | "metadata"
        )
    {
        return Err(FetchFailure::rejected(
            "web_metadata_target_denied",
            "url",
            "URL 主机属于本地或 metadata 命名空间",
        ));
    }
    let ip_literal = normalized
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&normalized);
    if let Ok(address) = ip_literal.parse::<IpAddr>()
        && !is_public_ip(address)
    {
        return Err(FetchFailure::rejected(
            "web_private_address_denied",
            "url",
            "URL IP 不是 public unicast 地址",
        ));
    }
    Ok(())
}

fn validate_resolved_addresses(
    addresses: &[SocketAddr],
    expected_port: u16,
) -> Result<(), FetchFailure> {
    if addresses.is_empty() {
        return Err(FetchFailure::transport(
            "web_dns_empty",
            "dns",
            "DNS 没有返回地址",
        ));
    }
    if let Some(address) = addresses
        .iter()
        .find(|address| address.port() != expected_port || !is_public_ip(address.ip()))
    {
        return Err(FetchFailure::operation(
            "web_dns_target_denied",
            "dns",
            format!("DNS/连接目标 {address} 不是允许的 public HTTPS 地址"),
        ));
    }
    Ok(())
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    ![
        ("0.0.0.0", 8),
        ("10.0.0.0", 8),
        ("100.64.0.0", 10),
        ("127.0.0.0", 8),
        ("169.254.0.0", 16),
        ("172.16.0.0", 12),
        ("192.0.0.0", 24),
        ("192.0.2.0", 24),
        ("192.88.99.0", 24),
        ("192.168.0.0", 16),
        ("198.18.0.0", 15),
        ("198.51.100.0", 24),
        ("203.0.113.0", 24),
        ("224.0.0.0", 4),
        ("240.0.0.0", 4),
    ]
    .into_iter()
    .any(|(network, prefix)| ipv4_in_prefix(value, network.parse().expect("literal IPv4"), prefix))
}

fn ipv4_in_prefix(value: u32, network: Ipv4Addr, prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    value & mask == u32::from(network) & mask
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    let value = u128::from(address);
    ipv6_in_prefix(value, "2000::".parse().expect("literal IPv6"), 3)
        && ![
            ("2001::", 32),
            ("2001:2::", 48),
            ("2001:10::", 28),
            ("2001:20::", 28),
            ("2001:db8::", 32),
            ("2002::", 16),
        ]
        .into_iter()
        .any(|(network, prefix)| {
            ipv6_in_prefix(value, network.parse().expect("literal IPv6"), prefix)
        })
}

fn ipv6_in_prefix(value: u128, network: Ipv6Addr, prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    value & mask == u128::from(network) & mask
}

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

struct MediaType {
    media_type: String,
    charset: String,
    is_html: bool,
}

fn parse_media_type(header: Option<&str>) -> Result<MediaType, FetchFailure> {
    let header = header.ok_or_else(|| {
        FetchFailure::operation(
            "web_content_type_missing",
            "content_type",
            "响应缺少 Content-Type",
        )
    })?;
    let mut parts = header.split(';');
    let media_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let is_html = matches!(media_type.as_str(), "text/html" | "application/xhtml+xml");
    if !is_html && media_type != "text/plain" {
        return Err(FetchFailure::operation(
            "web_content_type_denied",
            "content_type",
            format!("不支持 Content-Type '{media_type}'"),
        ));
    }
    let mut charset = "utf-8".to_owned();
    for parameter in parts {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("charset") {
            charset = value.trim().trim_matches('"').to_ascii_lowercase();
        }
    }
    if !matches!(charset.as_str(), "utf-8" | "utf8" | "us-ascii") {
        return Err(FetchFailure::operation(
            "web_charset_denied",
            "content_type",
            format!("只支持 UTF-8/US-ASCII，响应声明 '{charset}'"),
        ));
    }
    Ok(MediaType {
        media_type,
        charset,
        is_html,
    })
}

fn decode_response_body(raw: &[u8], encoding: Option<&str>) -> Result<Vec<u8>, FetchFailure> {
    let encoding = encoding.unwrap_or("identity").trim().to_ascii_lowercase();
    match encoding.as_str() {
        "" | "identity" => {
            if raw.len() > MAX_DECOMPRESSED_BYTES {
                return Err(FetchFailure::operation(
                    "web_decompressed_too_large",
                    "decompression",
                    format!("正文超过 {MAX_DECOMPRESSED_BYTES} bytes 解压后上限"),
                ));
            }
            Ok(raw.to_vec())
        }
        "gzip" => {
            let mut decoder = MultiGzDecoder::new(raw);
            let mut decoded = Vec::new();
            let mut chunk = [0u8; 8 * 1024];
            loop {
                let read = decoder.read(&mut chunk).map_err(|error| {
                    FetchFailure::operation(
                        "web_decompression_failed",
                        "decompression",
                        format!("gzip 解压失败：{error}"),
                    )
                })?;
                if read == 0 {
                    break;
                }
                if decoded.len().saturating_add(read) > MAX_DECOMPRESSED_BYTES {
                    return Err(FetchFailure::operation(
                        "web_decompressed_too_large",
                        "decompression",
                        format!("gzip 正文超过 {MAX_DECOMPRESSED_BYTES} bytes 解压后上限"),
                    ));
                }
                decoded.extend_from_slice(&chunk[..read]);
            }
            Ok(decoded)
        }
        _ => Err(FetchFailure::operation(
            "web_content_encoding_denied",
            "decompression",
            format!("不支持 Content-Encoding '{encoding}'"),
        )),
    }
}

fn decode_utf8_source<'a>(source: &'a [u8], charset: &str) -> Result<&'a str, FetchFailure> {
    if charset == "us-ascii" && source.iter().any(|byte| *byte > 0x7f) {
        return Err(FetchFailure::operation(
            "web_charset_invalid",
            "decode",
            "US-ASCII 响应包含非 ASCII byte",
        ));
    }
    std::str::from_utf8(source)
        .map(|text| text.strip_prefix('\u{feff}').unwrap_or(text))
        .map_err(|error| {
            FetchFailure::operation(
                "web_utf8_invalid",
                "decode",
                format!("正文不是有效 UTF-8：{error}"),
            )
        })
}

fn normalize_plain_text(source: &str) -> String {
    let mut output = String::new();
    let mut blank = false;
    for line in source.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            if !output.is_empty() {
                blank = true;
            }
            continue;
        }
        if blank {
            output.push_str("\n\n");
        } else if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&line);
        blank = false;
    }
    output
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let Some((index, _)) = value.char_indices().nth(max_chars) else {
        return (value.to_owned(), false);
    };
    (value[..index].trim_end().to_owned(), true)
}

fn extract_html(source: &str, base: &Url) -> ExtractedPage {
    let mut all_text = TextAccumulator::default();
    let mut body_text = TextAccumulator::default();
    let mut title_text = TextAccumulator::default();
    let mut links = Vec::new();
    let mut seen_links = HashSet::new();
    let mut links_truncated = false;
    let mut position = 0usize;
    let mut suppressed_tags = Vec::<String>::new();
    let mut head_depth = 0usize;
    let mut body_depth = 0usize;
    let mut title_depth = 0usize;
    let mut saw_body = false;

    while position < source.len() {
        let rest = &source[position..];
        if rest.starts_with("<!--") {
            position += rest.find("-->").map_or(rest.len(), |offset| offset + 3);
            continue;
        }
        if rest.starts_with('<')
            && let Some(end) = find_tag_end(rest)
        {
            let raw = &rest[1..end];
            if let Some(tag) = parse_tag(raw) {
                let suppressed_tag = matches!(
                    tag.name.as_str(),
                    "script" | "style" | "noscript" | "template" | "svg" | "canvas"
                );
                if tag.closing {
                    if suppressed_tag && suppressed_tags.last() == Some(&tag.name) {
                        suppressed_tags.pop();
                    }
                    if suppressed_tags.is_empty() {
                        match tag.name.as_str() {
                            "head" => head_depth = head_depth.saturating_sub(1),
                            "body" => body_depth = body_depth.saturating_sub(1),
                            "title" => title_depth = title_depth.saturating_sub(1),
                            _ => {}
                        }
                    }
                } else {
                    if suppressed_tags.is_empty() {
                        match tag.name.as_str() {
                            "head" => head_depth += 1,
                            "body" => {
                                saw_body = true;
                                body_depth += 1;
                            }
                            "title" => title_depth += 1,
                            _ => {}
                        }
                    }
                    if suppressed_tags.is_empty()
                        && tag.name == "a"
                        && let Some(href) = tag.attribute("href")
                        && let Some(link) = canonical_link(base, href)
                        && seen_links.insert(link.clone())
                    {
                        if links.len() < MAX_LINKS {
                            links.push(link);
                        } else {
                            links_truncated = true;
                        }
                    }
                    if suppressed_tag && !tag.self_closing {
                        suppressed_tags.push(tag.name.clone());
                    }
                }
                if is_block_tag(&tag.name) && suppressed_tags.is_empty() {
                    all_text.boundary();
                    if body_depth > 0 {
                        body_text.boundary();
                    }
                }
            }
            position += end + 1;
            continue;
        }

        let next = rest.find('<').unwrap_or(rest.len());
        let raw_text = if next == 0 {
            let length = rest.chars().next().map_or(1, char::len_utf8);
            position += length;
            continue;
        } else {
            &rest[..next]
        };
        let decoded = decode_html_entities(raw_text);
        if title_depth > 0 && suppressed_tags.is_empty() {
            title_text.push(&decoded);
        }
        if suppressed_tags.is_empty() && head_depth == 0 {
            all_text.push(&decoded);
            if body_depth > 0 {
                body_text.push(&decoded);
            }
        }
        position += next;
    }

    let title = normalize_title(&title_text.finish());
    let text = if saw_body {
        body_text.finish()
    } else {
        all_text.finish()
    };
    ExtractedPage {
        title,
        text,
        links,
        links_truncated,
    }
}

#[derive(Default)]
struct TextAccumulator {
    value: String,
    pending_space: bool,
    pending_boundary: bool,
}

impl TextAccumulator {
    fn push(&mut self, text: &str) {
        for character in text.chars() {
            if character.is_whitespace() {
                self.pending_space = !self.value.is_empty();
                continue;
            }
            if self.pending_boundary && !self.value.is_empty() {
                while self.value.ends_with(' ') {
                    self.value.pop();
                }
                if !self.value.ends_with('\n') {
                    self.value.push('\n');
                }
            } else if self.pending_space
                && !self.value.is_empty()
                && !self.value.ends_with([' ', '\n'])
            {
                self.value.push(' ');
            }
            self.value.push(character);
            self.pending_space = false;
            self.pending_boundary = false;
        }
    }

    fn boundary(&mut self) {
        if !self.value.is_empty() {
            self.pending_boundary = true;
            self.pending_space = false;
        }
    }

    fn finish(mut self) -> String {
        while self.value.ends_with([' ', '\n']) {
            self.value.pop();
        }
        self.value
    }
}

struct ParsedTag {
    name: String,
    closing: bool,
    self_closing: bool,
    attributes: BTreeMap<String, String>,
}

impl ParsedTag {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

fn find_tag_end(source: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in source.char_indices().skip(1) {
        match (quote, character) {
            (Some(active), current) if active == current => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_tag(raw: &str) -> Option<ParsedTag> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('!') || raw.starts_with('?') {
        return None;
    }
    let closing = raw.starts_with('/');
    let raw = raw.strip_prefix('/').unwrap_or(raw).trim_start();
    let self_closing = raw.trim_end().ends_with('/');
    let name_end = raw
        .find(|character: char| character.is_ascii_whitespace() || character == '/')
        .unwrap_or(raw.len());
    let name = raw[..name_end].to_ascii_lowercase();
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(ParsedTag {
        name,
        closing,
        self_closing,
        attributes: if closing {
            BTreeMap::new()
        } else {
            parse_attributes(&raw[name_end..])
        },
    })
}

fn parse_attributes(mut raw: &str) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    while !raw.is_empty() {
        raw = raw.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == '/'
        });
        if raw.is_empty() {
            break;
        }
        let name_end = raw
            .find(|character: char| {
                character.is_ascii_whitespace() || character == '=' || character == '/'
            })
            .unwrap_or(raw.len());
        if name_end == 0 {
            raw = &raw[raw.chars().next().map_or(1, char::len_utf8)..];
            continue;
        }
        let name = raw[..name_end].to_ascii_lowercase();
        raw = &raw[name_end..];
        raw = raw.trim_start();
        if !raw.starts_with('=') {
            attributes.entry(name).or_default();
            continue;
        }
        raw = raw[1..].trim_start();
        let (value, remaining) = if let Some(quote @ ('\'' | '"')) = raw.chars().next() {
            let after_quote = &raw[quote.len_utf8()..];
            match after_quote.find(quote) {
                Some(end) => (&after_quote[..end], &after_quote[end + quote.len_utf8()..]),
                None => (after_quote, ""),
            }
        } else {
            let end = raw
                .find(|character: char| character.is_ascii_whitespace() || character == '/')
                .unwrap_or(raw.len());
            (&raw[..end], &raw[end..])
        };
        attributes
            .entry(name)
            .or_insert_with(|| decode_html_entities(value));
        raw = remaining;
    }
    attributes
}

fn canonical_link(base: &Url, href: &str) -> Option<String> {
    let mut link = base.join(href.trim()).ok()?;
    link.set_fragment(None);
    validate_url_host(&link).ok()?;
    let value = link.as_str();
    if value.chars().count() > MAX_LINK_CHARS {
        return None;
    }
    Some(value.to_owned())
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "br"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figcaption"
            | "figure"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "ul"
    )
}

fn decode_html_entities(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let candidate = &rest[start + 1..];
        let Some(end) = candidate.find(';').filter(|end| *end <= 12) else {
            output.push('&');
            rest = candidate;
            continue;
        };
        let entity = &candidate[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            value if value.starts_with("#x") || value.starts_with("#X") => {
                u32::from_str_radix(&value[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            value if value.starts_with('#') => {
                value[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(character) = decoded {
            output.push(character);
        } else {
            output.push('&');
            output.push_str(entity);
            output.push(';');
        }
        rest = &candidate[end + 1..];
    }
    output.push_str(rest);
    output
}

fn normalize_title(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        None
    } else {
        Some(truncate_chars(&title, MAX_TITLE_CHARS).0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write;
    use std::sync::Mutex;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    fn public_address() -> SocketAddr {
        "93.184.216.34:443".parse().unwrap()
    }

    #[derive(Default)]
    struct ScriptedNetwork {
        resolutions: Mutex<VecDeque<Result<Vec<SocketAddr>, WebFetchNetworkError>>>,
        responses: Mutex<VecDeque<Result<WebFetchHttpResponse, WebFetchNetworkError>>>,
        requests: Mutex<Vec<(String, Vec<SocketAddr>)>>,
    }

    impl ScriptedNetwork {
        fn with_steps(
            resolutions: Vec<Result<Vec<SocketAddr>, WebFetchNetworkError>>,
            responses: Vec<Result<WebFetchHttpResponse, WebFetchNetworkError>>,
        ) -> Self {
            Self {
                resolutions: Mutex::new(resolutions.into()),
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl WebFetchNetwork for ScriptedNetwork {
        fn identity(&self) -> &str {
            "scripted_web_fetch_v1"
        }

        async fn resolve(
            &self,
            _host: &str,
            _port: u16,
        ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
            self.resolutions
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted resolution")
        }

        async fn get(
            &self,
            url: &Url,
            pinned_addresses: &[SocketAddr],
            _timeout: Duration,
        ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
            self.requests
                .lock()
                .unwrap()
                .push((url.as_str().to_owned(), pinned_addresses.to_vec()));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted response")
        }
    }

    struct NeverNetwork;

    #[async_trait]
    impl WebFetchNetwork for NeverNetwork {
        fn identity(&self) -> &str {
            "never_web_fetch_v1"
        }

        async fn resolve(
            &self,
            _host: &str,
            _port: u16,
        ) -> Result<Vec<SocketAddr>, WebFetchNetworkError> {
            std::future::pending().await
        }

        async fn get(
            &self,
            _url: &Url,
            _pinned_addresses: &[SocketAddr],
            _timeout: Duration,
        ) -> Result<WebFetchHttpResponse, WebFetchNetworkError> {
            unreachable!("pending DNS must not reach request")
        }
    }

    fn html_response(status: u16, html: &str) -> WebFetchHttpResponse {
        WebFetchHttpResponse::new(
            status,
            BTreeMap::from([(
                CONTENT_TYPE.as_str().to_owned(),
                "text/html; charset=utf-8".to_owned(),
            )]),
            html.as_bytes().to_vec(),
        )
    }

    #[test]
    fn url_gate_rejects_scheme_userinfo_metadata_and_literal_ssrf_targets() {
        for (url, code) in [
            ("http://example.com/", "web_scheme_denied"),
            ("file:///etc/passwd", "web_scheme_denied"),
            ("https://user:pass@example.com/", "web_userinfo_denied"),
            ("https://localhost/", "web_metadata_target_denied"),
            (
                "https://metadata.google.internal/",
                "web_metadata_target_denied",
            ),
            ("https://127.0.0.1/", "web_private_address_denied"),
            (
                "https://169.254.169.254/latest",
                "web_private_address_denied",
            ),
            ("https://[::1]/", "web_private_address_denied"),
            ("https://[fe80::1]/", "web_private_address_denied"),
        ] {
            assert_eq!(parse_and_validate_url(url).unwrap_err().code, code, "{url}");
        }
        assert!(parse_and_validate_url("https://example.com/path?q=1#part").is_ok());
    }

    #[test]
    fn public_ip_gate_rejects_all_required_special_classes() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
        ] {
            assert!(!is_public_ip(address.parse().unwrap()), "{address}");
        }
        for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(address.parse().unwrap()), "{address}");
        }
    }

    #[tokio::test]
    async fn redirect_revalidates_dns_pins_and_extracts_bounded_untrusted_html() {
        let redirect = WebFetchHttpResponse::new(
            302,
            BTreeMap::from([(LOCATION.as_str().to_owned(), "/final#fragment".to_owned())]),
            Vec::new(),
        );
        let network = Arc::new(ScriptedNetwork::with_steps(
            vec![Ok(vec![public_address()]), Ok(vec![public_address()])],
            vec![
                Ok(redirect),
                Ok(html_response(
                    200,
                    "<html><head><title> Example &amp; page </title><script></style>steal()</script></head><body><h1>Hello</h1><p>public   text</p><a href='/docs#x'>Docs</a><a href='http://unsafe.test'>No</a></body></html>",
                )),
            ],
        ));
        let outcome = execute_web_fetch(
            json!({"url":"https://example.com/start","max_chars":100}),
            network.clone(),
            true,
        )
        .await;
        assert!(outcome.is_success(), "{}", outcome.content);
        let payload: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(payload["requested_url"], "https://example.com/start");
        assert_eq!(payload["final_url"], "https://example.com/final");
        assert_eq!(payload["status"], 200);
        assert_eq!(payload["media_type"], "text/html");
        assert_eq!(payload["title"], "Example & page");
        assert_eq!(payload["text"], "Hello\npublic text\nDocsNo");
        assert_eq!(payload["links"], json!(["https://example.com/docs"]));
        assert_eq!(payload["trust"], TRUST);
        assert!(!payload["text"].as_str().unwrap().contains("steal"));
        assert!(
            payload["source_sha256"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(network.requests.lock().unwrap().len(), 2);
        assert!(
            network
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|(_, addresses)| addresses == &[public_address()])
        );
    }

    #[tokio::test]
    async fn dns_rebinding_and_mixed_public_private_answers_fail_before_connect() {
        for addresses in [
            vec!["127.0.0.1:443".parse().unwrap()],
            vec![public_address(), "169.254.169.254:443".parse().unwrap()],
        ] {
            let network = Arc::new(ScriptedNetwork::with_steps(vec![Ok(addresses)], vec![]));
            let outcome =
                execute_web_fetch(json!({"url":"https://example.com/"}), network.clone(), true)
                    .await;
            assert_eq!(
                outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
                "web_dns_target_denied"
            );
            assert!(network.requests.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn redirect_escape_is_blocked_without_resolving_or_connecting_target() {
        for location in [
            "http://example.com/plain",
            "https://user:pass@example.com/secret",
            "https://169.254.169.254/latest/meta-data",
        ] {
            let network = Arc::new(ScriptedNetwork::with_steps(
                vec![Ok(vec![public_address()])],
                vec![Ok(WebFetchHttpResponse::new(
                    302,
                    BTreeMap::from([(LOCATION.as_str().to_owned(), location.to_owned())]),
                    Vec::new(),
                ))],
            ));
            let outcome =
                execute_web_fetch(json!({"url":"https://example.com/"}), network.clone(), true)
                    .await;
            assert!(!outcome.is_success());
            assert_eq!(network.requests.lock().unwrap().len(), 1);
            assert!(network.resolutions.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn redirect_limit_and_loop_are_typed_and_bounded() {
        let resolutions = (0..=MAX_REDIRECTS)
            .map(|_| Ok(vec![public_address()]))
            .collect::<Vec<_>>();
        let responses = (0..=MAX_REDIRECTS)
            .map(|index| {
                Ok(WebFetchHttpResponse::new(
                    302,
                    BTreeMap::from([(
                        LOCATION.as_str().to_owned(),
                        format!("/redirect/{}", index + 1),
                    )]),
                    Vec::new(),
                ))
            })
            .collect::<Vec<_>>();
        let network = Arc::new(ScriptedNetwork::with_steps(resolutions, responses));
        let outcome = execute_web_fetch(
            json!({"url":"https://example.com/redirect/0"}),
            network.clone(),
            true,
        )
        .await;
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_redirect_limit"
        );
        assert_eq!(network.requests.lock().unwrap().len(), MAX_REDIRECTS + 1);

        let looped = Arc::new(ScriptedNetwork::with_steps(
            vec![Ok(vec![public_address()])],
            vec![Ok(WebFetchHttpResponse::new(
                302,
                BTreeMap::from([(
                    LOCATION.as_str().to_owned(),
                    "https://example.com/".to_owned(),
                )]),
                Vec::new(),
            ))],
        ));
        let outcome = execute_web_fetch(json!({"url":"https://example.com/"}), looped, true).await;
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_redirect_loop"
        );
    }

    #[tokio::test]
    async fn total_deadline_cancels_pending_dns_with_typed_safe_failure() {
        let outcome = execute_web_fetch_with_deadline(
            json!({"url":"https://example.com/"}),
            Arc::new(NeverNetwork),
            true,
            Duration::from_millis(10),
        )
        .await;
        assert_eq!(outcome.failure_code, Some(ToolFailureCode::TransportFailed));
        assert_eq!(outcome.retry, ToolRetryDisposition::Safe);
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_deadline_exceeded"
        );
        outcome.validate().unwrap();
    }

    #[tokio::test]
    async fn content_type_encoding_utf8_and_body_limits_fail_closed_with_typed_codes() {
        let cases = [
            (
                WebFetchHttpResponse::new(
                    200,
                    BTreeMap::from([(
                        CONTENT_TYPE.as_str().to_owned(),
                        "application/pdf".to_owned(),
                    )]),
                    b"pdf".to_vec(),
                ),
                "web_content_type_denied",
            ),
            (
                WebFetchHttpResponse::new(
                    200,
                    BTreeMap::from([
                        (CONTENT_TYPE.as_str().to_owned(), "text/plain".to_owned()),
                        (CONTENT_ENCODING.as_str().to_owned(), "br".to_owned()),
                    ]),
                    b"body".to_vec(),
                ),
                "web_content_encoding_denied",
            ),
            (
                WebFetchHttpResponse::new(
                    200,
                    BTreeMap::from([(
                        CONTENT_TYPE.as_str().to_owned(),
                        "text/plain; charset=utf-8".to_owned(),
                    )]),
                    vec![0xff],
                ),
                "web_utf8_invalid",
            ),
            (
                WebFetchHttpResponse::new(
                    200,
                    BTreeMap::from([(CONTENT_TYPE.as_str().to_owned(), "text/plain".to_owned())]),
                    vec![b'x'; MAX_RESPONSE_BYTES + 1],
                ),
                "web_response_too_large",
            ),
            (
                WebFetchHttpResponse::new(
                    200,
                    BTreeMap::from([
                        (CONTENT_TYPE.as_str().to_owned(), "text/plain".to_owned()),
                        (
                            CONTENT_LENGTH.as_str().to_owned(),
                            (MAX_RESPONSE_BYTES + 1).to_string(),
                        ),
                    ]),
                    Vec::new(),
                ),
                "web_response_too_large",
            ),
        ];
        for (response, expected) in cases {
            let network = Arc::new(ScriptedNetwork::with_steps(
                vec![Ok(vec![public_address()])],
                vec![Ok(response)],
            ));
            let outcome =
                execute_web_fetch(json!({"url":"https://example.com/"}), network, true).await;
            assert_eq!(
                outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
                expected
            );
            outcome.validate().unwrap();
        }
    }

    #[test]
    fn gzip_decompression_is_bounded_and_validated() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hello gzip").unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(
            decode_response_body(&compressed, Some("gzip")).unwrap(),
            b"hello gzip"
        );
        assert_eq!(
            decode_response_body(b"not gzip", Some("gzip"))
                .unwrap_err()
                .code,
            "web_decompression_failed"
        );

        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(&vec![b'x'; MAX_DECOMPRESSED_BYTES + 1])
            .unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(compressed.len() < MAX_RESPONSE_BYTES);
        assert_eq!(
            decode_response_body(&compressed, Some("gzip"))
                .unwrap_err()
                .code,
            "web_decompressed_too_large"
        );
    }

    #[tokio::test]
    async fn unicode_text_and_link_count_are_bounded_without_splitting_characters() {
        let links = (0..MAX_LINKS + 2)
            .map(|index| format!("<a href='/p/{index}'>链接{index}</a>"))
            .collect::<String>();
        let html = format!("<body><p>甲乙丙丁</p>{links}</body>");
        let network = Arc::new(ScriptedNetwork::with_steps(
            vec![Ok(vec![public_address()])],
            vec![Ok(html_response(200, &html))],
        ));
        let outcome = execute_web_fetch(
            json!({"url":"https://example.com/","max_chars":3}),
            network,
            true,
        )
        .await;
        let payload: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(payload["text"], "甲乙丙");
        assert_eq!(payload["links"].as_array().unwrap().len(), MAX_LINKS);
        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["bytes_returned"], 9);
    }

    #[tokio::test]
    async fn permission_denial_does_not_touch_dns_or_http() {
        let network = Arc::new(ScriptedNetwork::default());
        let outcome = execute_web_fetch(
            json!({"url":"https://example.com/"}),
            network.clone(),
            false,
        )
        .await;
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["web_fetch"]["failure"]["code"],
            "web_network_not_authorized"
        );
        assert!(network.resolutions.lock().unwrap().is_empty());
        assert!(network.requests.lock().unwrap().is_empty());
    }
}
