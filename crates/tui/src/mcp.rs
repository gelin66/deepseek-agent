//! Async MCP (Model Context Protocol) Implementation
//!
//! This module provides full async support for MCP servers with:
//! - Connection pooling for server reuse
//! - Automatic tool discovery via `tools/list`
//! - Configurable timeouts per-server and globally

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

mod headers;
pub mod oauth;
mod sse;
mod stdio;
mod streamable_http;

use self::headers::{apply_safe_custom_headers, with_default_mcp_http_headers};
use self::sse::SseTransport;
use self::stdio::StdioTransport;
#[cfg(all(test, unix))]
use self::stdio::{STDIO_SHUTDOWN_GRACE, StderrTail};
use self::streamable_http::{StreamableHttpTransport, StreamableSendError};
use crate::network_policy::{Decision, NetworkPolicyDecider, host_from_url};
use crate::utils::write_atomic;

// === Error diagnostics helpers (#71) ===

/// Bytes of a non-2xx response body to surface in connection errors.
const ERROR_BODY_PREVIEW_BYTES: usize = 200;

fn validate_mcp_config_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("MCP config path cannot be empty");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("MCP config path cannot contain '..' components");
    }
    Ok(())
}

/// Expand `${NAME}` placeholders in an MCP config value from the process
/// environment. This lets secrets (API keys, bearer tokens, …) be supplied
/// through environment variables instead of being written in cleartext into
/// the MCP config file on disk.
///
/// On a missing or malformed placeholder the error names only the offending
/// variable, never the surrounding value, so a secret-bearing string is never
/// echoed into logs or error output.
fn expand_env_placeholders(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            anyhow::bail!("unterminated environment placeholder in MCP config value");
        };
        let name = &after[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            anyhow::bail!("invalid environment placeholder in MCP config value");
        }
        let env_value = std::env::var(name).with_context(|| {
            format!("environment variable {name} required by MCP config is not set")
        })?;
        out.push_str(&env_value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Expand `${NAME}` placeholders across every value of an MCP config map
/// (e.g. the stdio child `env`). `context` only labels expansion errors so a
/// failure can be attributed to the right map.
fn expand_env_placeholders_map(
    values: &HashMap<String, String>,
    context: &str,
) -> Result<HashMap<String, String>> {
    let mut expanded = HashMap::with_capacity(values.len());
    for (key, value) in values {
        expanded.insert(
            key.clone(),
            expand_env_placeholders(value)
                .with_context(|| format!("failed to expand MCP {context} value for {key}"))?,
        );
    }
    Ok(expanded)
}

/// Mask a URL so any embedded credentials in the userinfo portion (e.g.
/// `https://user:secret@host`) are replaced with `***`. Failures fall back to
/// the original string so we don't lose context — we never want masking to
/// produce an empty error.
fn mask_url_secrets(url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        let mut clone = parsed.clone();
        if !parsed.username().is_empty() || parsed.password().is_some() {
            let _ = clone.set_username("***");
            let _ = clone.set_password(Some("***"));
        }
        return clone.to_string();
    }
    url.to_string()
}

/// Redact the userinfo segment (`username[:password]@…` portion) from
/// a proxy URL so it can be safely included in `tracing::warn!` output
/// without leaking the
/// password into the on-disk log. URLs without userinfo are returned
/// unchanged. Garbage input (no `://` scheme separator) is also returned
/// unchanged — the malformed-URL warning path is the only caller, so an
/// unparseable input is already the failure case.
fn redact_proxy_userinfo(proxy_url: &str) -> String {
    let Some(scheme_end) = proxy_url.find("://") else {
        return proxy_url.to_string();
    };
    let after_scheme = scheme_end + 3;
    // The userinfo segment ends at the next `@`, but only if that `@`
    // comes before the next `/`, `?`, or `#` (otherwise the `@` is in a
    // path / query and the URL has no userinfo at all).
    let rest = &proxy_url[after_scheme..];
    let at_idx = rest.find('@');
    let path_idx = rest.find(['/', '?', '#']);
    let userinfo_end = match (at_idx, path_idx) {
        (Some(a), Some(p)) if a < p => Some(a),
        (Some(a), None) => Some(a),
        _ => None,
    };
    if let Some(end) = userinfo_end {
        let mut out = String::with_capacity(proxy_url.len());
        out.push_str(&proxy_url[..after_scheme]);
        out.push_str("***@");
        out.push_str(&rest[end + 1..]);
        out
    } else {
        proxy_url.to_string()
    }
}

/// Mask any obvious token-like substrings in a body excerpt before surfacing
/// it. Conservative: replaces `Bearer <token>` and `api_key=...` shapes.
fn redact_body_preview(body: &str) -> String {
    let mut out = body.to_string();
    if let Some(idx) = out.to_lowercase().find("bearer ") {
        let tail_start = idx + "bearer ".len();
        if tail_start < out.len() {
            let end = out[tail_start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == ',')
                .map_or(out.len(), |off| tail_start + off);
            out.replace_range(tail_start..end, "***");
        }
    }
    for needle in ["api_key=", "apikey=", "api-key=", "token="] {
        if let Some(idx) = out.to_lowercase().find(needle) {
            let tail_start = idx + needle.len();
            let end = out[tail_start..]
                .find(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == ',')
                .map_or(out.len(), |off| tail_start + off);
            out.replace_range(tail_start..end, "***");
        }
    }
    out
}

/// Read up to `max_bytes` of a reqwest Response body and produce a single-line
/// excerpt suitable for an error message. Best-effort — if the body can't be
/// read, returns the literal string `<no body>`.
async fn bounded_body_excerpt(response: reqwest::Response, max_bytes: usize) -> String {
    let body_text = response.text().await.unwrap_or_default();
    if body_text.is_empty() {
        return "<no body>".to_string();
    }
    let trimmed: String = body_text.chars().take(max_bytes).collect();
    let suffix = if body_text.len() > trimmed.len() {
        "…"
    } else {
        ""
    };
    let one_line = trimmed.replace(['\n', '\r'], " ");
    format!("{}{}", redact_body_preview(&one_line), suffix)
}

fn invalid_json_preview(bytes: &[u8]) -> String {
    let body_text = String::from_utf8_lossy(bytes);
    if body_text.is_empty() {
        return "<empty>".to_string();
    }

    let trimmed: String = body_text.chars().take(ERROR_BODY_PREVIEW_BYTES).collect();
    let suffix = if body_text.chars().count() > ERROR_BODY_PREVIEW_BYTES {
        "…"
    } else {
        ""
    };
    let one_line = trimmed.replace(['\n', '\r'], " ");
    format!("{}{}", redact_body_preview(&one_line), suffix)
}

// === Configuration Types ===

/// Full MCP configuration from mcp.json
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpConfig {
    #[serde(default)]
    pub timeouts: McpTimeouts,
    #[serde(default, alias = "mcpServers")]
    pub servers: HashMap<String, McpServerConfig>,
}

/// Global timeout configuration
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct McpTimeouts {
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_execute_timeout")]
    pub execute_timeout: u64,
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
}

fn default_connect_timeout() -> u64 {
    10
}
fn default_execute_timeout() -> u64 {
    60
}
fn default_read_timeout() -> u64 {
    120
}

impl Default for McpTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout: default_connect_timeout(),
            execute_timeout: default_execute_timeout(),
            read_timeout: default_read_timeout(),
        }
    }
}

/// Configuration for a single MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    pub url: Option<String>,
    /// Optional explicit HTTP transport override.
    ///
    /// By default URL-based MCP servers use Streamable HTTP first and fall
    /// back to legacy SSE only when the server rejects Streamable HTTP with
    /// a known incompatible status. Set this to `"sse"` for legacy SSE
    /// endpoints that must start with a long-lived GET endpoint discovery
    /// stream and cannot accept an initial POST to the configured URL.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default)]
    pub connect_timeout: Option<u64>,
    #[serde(default)]
    pub execute_timeout: Option<u64>,
    #[serde(default)]
    pub read_timeout: Option<u64>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Extra HTTP headers sent with every request to this MCP server.
    /// Only the HTTP transports (streamable HTTP today; SSE in a
    /// follow-up) honor this — `command`-based stdio servers ignore it.
    ///
    /// Mirrors the `headers` field that Claude Code, Codex, and
    /// OpenCode already accept in their MCP config formats. Use it to
    /// authenticate against gateways that require a Bearer token or
    /// API key, e.g.:
    ///
    /// ```jsonc
    /// "huggingface": {
    ///     "url": "https://huggingface.co/api/mcp",
    ///     "headers": { "Authorization": "Bearer ${HF_TOKEN}" }
    /// }
    /// ```
    ///
    /// Header keys and values are passed through as-is — we do not
    /// substitute environment variables in v0.8.31. If you store a
    /// real token here, the value lives in plain text in
    /// `~/.deepseek/mcp.json`; treat that file with the same care
    /// as any other secret-bearing config.
    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// HTTP headers whose values are read from environment variables at request
    /// time. This keeps common bearer/API-token integrations out of mcp.json.
    #[serde(default, alias = "env_http_headers")]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub env_headers: HashMap<String, String>,
    /// Environment variable containing a bearer token. When present and set,
    /// CodeWhale sends `Authorization: Bearer <value>` for URL-based servers.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    /// OAuth scopes requested during `codewhale mcp login`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// OAuth client override for MCP servers that require a pre-registered
    /// public client instead of dynamic registration.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpServerOAuthConfig>,
    /// Optional RFC 8707 resource parameter appended to the authorization URL.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_resource: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpServerOAuthConfig {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl McpServerConfig {
    pub fn effective_connect_timeout(&self, global: &McpTimeouts) -> u64 {
        self.connect_timeout.unwrap_or(global.connect_timeout)
    }

    pub fn effective_read_timeout(&self, global: &McpTimeouts) -> u64 {
        self.read_timeout.unwrap_or(global.read_timeout)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.disabled
    }

    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        let allowed = if self.enabled_tools.is_empty() {
            true
        } else {
            self.enabled_tools.iter().any(|t| t == tool_name)
        };
        if !allowed {
            return false;
        }
        !self.disabled_tools.iter().any(|t| t == tool_name)
    }
}

// === MCP Tool Definition ===

/// Tool discovered from an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: serde_json::Value,
}

// === Connection State ===

/// State of an MCP connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Ready,
    Disconnected,
}

// === McpConnection - Async Connection Management ===

// === Transport Trait ===

#[async_trait::async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&mut self, msg: Vec<u8>) -> Result<()>;
    async fn recv(&mut self) -> Result<Vec<u8>>;

    /// Graceful shutdown for any transport-owned processes or async tasks.
    /// Returns `true` only when every owned process/task was settled within
    /// its bounded shutdown budget. The default is a successful no-op for
    /// transports with no owned lifecycle.
    async fn shutdown(&mut self) -> bool {
        true
    }
}

struct HttpTransport {
    mode: HttpTransportMode,
    client: reqwest::Client,
    base_url: String,
    auth: McpHttpAuth,
    cancel_token: tokio_util::sync::CancellationToken,
    endpoint_timeout: Duration,
}

enum HttpTransportMode {
    Streamable(StreamableHttpTransport),
    Sse(SseTransport),
}

#[derive(Clone, Default)]
struct McpHttpAuth {
    headers: HashMap<String, String>,
    env_headers: HashMap<String, String>,
    bearer_token_env_var: Option<String>,
    oauth: Option<oauth::McpOAuthRuntime>,
}

impl McpHttpAuth {
    fn from_config(config: &McpServerConfig, oauth: Option<oauth::McpOAuthRuntime>) -> Self {
        Self {
            headers: config.headers.clone(),
            env_headers: config.env_headers.clone(),
            bearer_token_env_var: config.bearer_token_env_var.clone(),
            oauth,
        }
    }

    async fn resolved_headers(&self) -> Result<HashMap<String, String>> {
        let mut headers = self.headers.clone();
        for (name, env_var) in &self.env_headers {
            if let Ok(value) = std::env::var(env_var)
                && !value.trim().is_empty()
            {
                headers.insert(name.clone(), value);
            }
        }
        if !mcp_headers_have_authorization(&headers)
            && let Some(env_var) = self.bearer_token_env_var.as_deref()
            && let Ok(token) = std::env::var(env_var)
        {
            let token = token.trim();
            if !token.is_empty() {
                headers.insert("Authorization".to_string(), format!("Bearer {token}"));
            }
        }
        if !mcp_headers_have_authorization(&headers)
            && let Some(oauth) = &self.oauth
            && let Some(value) = oauth.authorization_header().await?
        {
            headers.insert("Authorization".to_string(), value);
        }
        Ok(headers)
    }
}

fn mcp_headers_have_authorization(headers: &HashMap<String, String>) -> bool {
    headers
        .keys()
        .any(|key| key.trim().eq_ignore_ascii_case("authorization"))
}

impl HttpTransport {
    fn new(
        client: reqwest::Client,
        url: String,
        auth: McpHttpAuth,
        cancel_token: tokio_util::sync::CancellationToken,
        endpoint_timeout: Duration,
    ) -> Self {
        Self {
            mode: HttpTransportMode::Streamable(StreamableHttpTransport::new(
                client.clone(),
                url.clone(),
                auth.clone(),
            )),
            client,
            base_url: url,
            auth,
            cancel_token,
            endpoint_timeout,
        }
    }

    async fn switch_to_sse_and_send(&mut self, msg: Vec<u8>) -> Result<()> {
        let sse = SseTransport::connect(
            self.client.clone(),
            self.base_url.clone(),
            self.auth.clone(),
            self.cancel_token.clone(),
            self.endpoint_timeout,
        )
        .await?;
        self.mode = HttpTransportMode::Sse(sse);
        match &mut self.mode {
            HttpTransportMode::Sse(transport) => transport.send(msg).await,
            HttpTransportMode::Streamable(_) => unreachable!("SSE mode was just installed"),
        }
    }

    /// Best-effort session-establishment GET preflight.
    ///
    /// Per the Streamable HTTP spec, the server may return an
    /// `Mcp-Session-Id` header on the `initialize` response (the normal
    /// path handled inside [`StreamableHttpTransport::send`] above).
    /// However some servers (e.g. Hindsight, #1629) **require** a session
    /// ID on every POST including `initialize`, creating a chicken-and-egg
    /// problem. For those servers we send a short-lived GET before the
    /// first POST: if the server returns a session ID in the GET response
    /// it will be captured by the header-reading code in
    /// [`StreamableHttpTransport::send`] just as if it came from a POST
    /// response.
    ///
    /// This is intentionally best-effort:
    /// * The GET uses a tight per-request inner timeout so it never
    ///   blocks connection startup for long.
    /// * If the server doesn't support GET (405, 404, …) we log a debug
    ///   line and move on — the `initialize` POST will proceed without a
    ///   session ID.
    /// * If the server opens an SSE stream in response (the GET from old
    ///   SSE transport), we read only the headers, then discard the body
    ///   so the SSE stream is torn down. The actual SSE path uses a
    ///   dedicated `SseTransport` and is triggered by the incompatible-
    ///   status fallback in [`HttpTransport::send`].
    async fn try_establish_session(&mut self) -> Result<()> {
        let transport = match &mut self.mode {
            HttpTransportMode::Streamable(t) => t,
            // Already on SSE — session is implicit via the long-lived GET.
            HttpTransportMode::Sse(_) => return Ok(()),
        };

        let headers = transport.auth.resolved_headers().await?;
        let request = apply_safe_custom_headers(
            with_default_mcp_http_headers(transport.client.get(&transport.url), false),
            &headers,
        );
        let response = tokio::time::timeout(Duration::from_secs(5), request.send())
            .await
            .map_err(|_| anyhow::anyhow!("GET timeout"))?
            .map_err(|e| anyhow::anyhow!("GET error: {e}"))?;

        // Capture session ID from the GET response so subsequent POSTs
        // (including `initialize`) can include it. This is the same
        // header-reading logic that would be hit inside
        // `StreamableHttpTransport::send` for POST responses, but since
        // the GET is sent before any POST we do it here directly.
        if let Some(sid) = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            && transport.session_id.as_deref() != Some(sid)
        {
            let session_ref = crate::utils::redacted_identifier_for_log(sid);
            tracing::debug!(target: "mcp", session = %session_ref, "captured MCP session ID via GET preflight");
            transport.session_id = Some(sid.to_string());
        }

        // We only care about the response headers — discard the body.
        // If the server opened an SSE stream in response (some servers
        // do this on GET), it will be torn down when response is dropped.
        drop(response);

        Ok(())
    }
}

#[async_trait::async_trait]
impl McpTransport for HttpTransport {
    async fn send(&mut self, msg: Vec<u8>) -> Result<()> {
        match &mut self.mode {
            HttpTransportMode::Streamable(transport) => match transport.send(msg.clone()).await {
                Ok(()) => Ok(()),
                Err(StreamableSendError::Incompatible(detail)) => {
                    tracing::debug!(
                        "MCP Streamable HTTP unavailable; falling back to SSE endpoint discovery: {}",
                        detail
                    );
                    self.switch_to_sse_and_send(msg).await
                }
                Err(StreamableSendError::StaleSession(detail)) => {
                    if let HttpTransportMode::Streamable(transport) = &mut self.mode {
                        tracing::debug!(
                            target: "mcp",
                            error = %detail,
                            "MCP Streamable HTTP session expired; clearing cached session ID"
                        );
                        transport.session_id = None;
                    }
                    Err(anyhow::anyhow!(
                        "MCP Streamable HTTP session expired; retry with a new session required ({detail})"
                    ))
                }
                Err(StreamableSendError::Other(err)) => Err(err),
            },
            HttpTransportMode::Sse(transport) => transport.send(msg).await,
        }
    }

    async fn recv(&mut self) -> Result<Vec<u8>> {
        match &mut self.mode {
            HttpTransportMode::Streamable(transport) => transport.recv().await,
            HttpTransportMode::Sse(transport) => transport.recv().await,
        }
    }

    async fn shutdown(&mut self) -> bool {
        self.cancel_token.cancel();
        if let HttpTransportMode::Sse(transport) = &mut self.mode {
            return transport.shutdown().await;
        }
        true
    }
}

fn is_mcp_stale_session_body(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("session") && (body.contains("expired") || body.contains("invalid"))
}

fn parse_sse_message_data(body: &str) -> Vec<Vec<u8>> {
    let normalized = body.replace("\r\n", "\n");
    let mut messages = Vec::new();

    for block in normalized.split("\n\n") {
        let mut event_type = "message";
        let mut data = String::new();

        for line in block.lines() {
            if let Some(value) = sse_field_value(line, "event:") {
                event_type = value;
            } else if let Some(value) = sse_field_value(line, "data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
        }

        if event_type != "message" || data.trim().is_empty() {
            continue;
        }

        messages.push(data.trim().as_bytes().to_vec());
    }

    messages
}

// Retained for tests; the SSE transport now uses the byte-oriented twin.
#[cfg(test)]
fn find_sse_event_separator(buffer: &str) -> Option<(usize, usize)> {
    match (buffer.find("\n\n"), buffer.find("\r\n\r\n")) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (_, Some(crlf)) => Some((crlf, 4)),
        _ => None,
    }
}

/// Byte-oriented twin of [`find_sse_event_separator`]. Used by the SSE
/// transport so it can accumulate RAW bytes and decode only complete event
/// blocks — a multi-byte UTF-8 char split across two network reads is never
/// corrupted to U+FFFD (the `\n`/`\r` separators are ASCII and can never fall
/// inside a multi-byte sequence).
fn find_sse_event_separator_bytes(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|w| w == b"\n\n");
    let crlf = buffer.windows(4).position(|w| w == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (_, Some(crlf)) => Some((crlf, 4)),
        _ => None,
    }
}

/// Hard ceiling on the SSE frame-assembly buffer. A server that never emits a
/// frame separator would otherwise grow it without bound (OOM DoS).
pub(super) const MAX_SSE_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Hard ceiling on a single MCP HTTP response body / stdio line. A misbehaving
/// or malicious server could otherwise stream an unbounded body (or a
/// newline-free multi-GB "line") and OOM the process at transport-read time,
/// before the result reaches the canonical Runtime.
pub(super) const MAX_MCP_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

fn sse_field_value<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let value = line.strip_prefix(field)?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

fn is_legacy_sse_transport(config: &McpServerConfig) -> bool {
    config
        .transport
        .as_deref()
        .map(|transport| transport.trim().eq_ignore_ascii_case("sse"))
        .unwrap_or(false)
}

fn validate_mcp_transport(transport: Option<&str>) -> Result<()> {
    let Some(transport) = transport else {
        return Ok(());
    };
    if transport.trim().eq_ignore_ascii_case("sse") {
        return Ok(());
    }
    anyhow::bail!("Unsupported MCP transport '{transport}'. Supported values: sse");
}

fn response_id_matches(id: Option<&serde_json::Value>, expected_id: &str) -> bool {
    let Some(id) = id else {
        return false;
    };
    if id.as_str() == Some(expected_id) {
        return true;
    }
    id.as_u64()
        .map(|id| id.to_string() == expected_id)
        .unwrap_or(false)
}

// === McpConnection - Async Connection Management ===

/// Manages a single async connection to an MCP server
pub struct McpConnection {
    name: String,
    transport: Box<dyn McpTransport>,
    tools: Vec<McpTool>,
    request_id: AtomicU64,
    state: ConnectionState,
    config: McpServerConfig,
    read_timeout_secs: u64,
    cancel_token: tokio_util::sync::CancellationToken,
}

impl McpConnection {
    /// Connect to an MCP server and initialize it.
    ///
    /// `network_policy` (added in v0.7.0 for #135) is consulted for HTTP/SSE
    /// transports only — STDIO transports are unaffected. Pass `None` to
    /// match pre-v0.7.0 permissive behavior.
    pub async fn connect_with_policy(
        name: String,
        config: McpServerConfig,
        global_timeouts: &McpTimeouts,
        network_policy: Option<&NetworkPolicyDecider>,
    ) -> Result<Self> {
        let connect_timeout_secs = config.effective_connect_timeout(global_timeouts);
        let read_timeout_secs = config.effective_read_timeout(global_timeouts);
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let transport: Box<dyn McpTransport> = if let Some(url) = &config.url {
            // Per-domain network policy gate (#135). Only the HTTP/SSE transport
            // is gated; STDIO MCP servers run as local subprocesses and never
            // touch the network from this code path.
            if let Some(decider) = network_policy
                && let Some(host) = host_from_url(url)
            {
                match decider.evaluate(&host, "mcp") {
                    Decision::Allow => {}
                    Decision::Deny => {
                        anyhow::bail!(
                            "MCP server '{name}' connection to '{host}' blocked by network policy"
                        );
                    }
                    Decision::Prompt => {
                        anyhow::bail!(
                            "MCP server '{name}' connection to '{host}' requires approval; \
                             re-run after `/network allow {host}` or set network.default = \"allow\" in config"
                        );
                    }
                }
            }
            // Honor the standard `HTTP_PROXY` / `HTTPS_PROXY` (and their
            // lowercase equivalents) plus `NO_PROXY` env vars when
            // reaching MCP HTTP servers (#1408). Reqwest 0.13 does not
            // auto-detect these by default, so users behind corporate
            // proxies, on China-mainland connections routing through a
            // local Clash / Shadowsocks tunnel, etc. previously had MCP
            // HTTP traffic bypass the proxy entirely while every other
            // tool on the box (curl, npm, …) used it.
            // `connect_timeout` bounds only the connect phase. The total request
            // timeout uses the read timeout so slow MCP initialization and tool
            // discovery are not silently capped by the shorter connect budget.
            let mut client_builder = crate::tls::reqwest_client_builder()
                .connect_timeout(Duration::from_secs(connect_timeout_secs))
                .timeout(Duration::from_secs(read_timeout_secs));
            let env_proxy_url = std::env::var("HTTPS_PROXY")
                .or_else(|_| std::env::var("https_proxy"))
                .or_else(|_| std::env::var("HTTP_PROXY"))
                .or_else(|_| std::env::var("http_proxy"))
                .ok()
                .filter(|s| !s.trim().is_empty());
            if let Some(proxy_url) = env_proxy_url {
                match reqwest::Proxy::all(&proxy_url) {
                    Ok(proxy) => {
                        let proxy = proxy.no_proxy(reqwest::NoProxy::from_env());
                        client_builder = client_builder.proxy(proxy);
                    }
                    Err(err) => {
                        // Redact userinfo (the `username[:password]@…`
                        // portion of the URL) before logging so an
                        // HTTPS_PROXY that embeds credentials
                        // (common in corporate setups) doesn't leak the
                        // password to the on-disk `~/.deepseek/logs/`.
                        let proxy_redacted = redact_proxy_userinfo(&proxy_url);
                        tracing::warn!(
                            target: "mcp",
                            ?err,
                            proxy = %proxy_redacted,
                            "ignoring malformed HTTP(S)_PROXY env var; MCP connection will bypass proxy"
                        );
                    }
                }
            }
            let client = client_builder.build()?;
            let oauth_runtime = match oauth::build_default_headers(
                &config.headers,
                &config.env_headers,
            ) {
                Ok(default_headers) => match oauth::McpOAuthRuntime::from_server_config(
                    &name,
                    &config,
                    default_headers,
                )
                .await
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::warn!(
                            target: "mcp",
                            server = %name,
                            error = %err,
                            "failed to prepare MCP OAuth runtime; continuing without stored OAuth token"
                        );
                        None
                    }
                },
                Err(err) => {
                    tracing::warn!(
                        target: "mcp",
                        server = %name,
                        error = %err,
                        "failed to prepare MCP OAuth default headers; continuing without stored OAuth token"
                    );
                    None
                }
            };
            let http_auth = McpHttpAuth::from_config(&config, oauth_runtime);
            if is_legacy_sse_transport(&config) {
                Box::new(
                    SseTransport::connect(
                        client,
                        url.clone(),
                        http_auth,
                        cancel_token.clone(),
                        Duration::from_secs(connect_timeout_secs),
                    )
                    .await?,
                )
            } else {
                let mut http = HttpTransport::new(
                    client,
                    url.clone(),
                    http_auth,
                    cancel_token.clone(),
                    Duration::from_secs(connect_timeout_secs),
                );
                // Best-effort session preflight for servers that require
                // a session ID on every POST including `initialize`
                // (e.g. Hindsight, #1629). Failures are non-fatal — the
                // `initialize` POST will proceed and may capture a session
                // ID from the response instead.
                if let Err(e) = http.try_establish_session().await {
                    tracing::debug!(
                        target: "mcp",
                        server = %name,
                        error = %e,
                        "session-establishment GET skipped; proceeding with POST initialize"
                    );
                }
                Box::new(http)
            }
        } else if let Some(command) = &config.command {
            Box::new(StdioTransport::spawn(&name, command, &config)?)
        } else {
            anyhow::bail!("MCP server '{name}' config must have either 'command' or 'url'");
        };

        let mut conn = Self {
            name: name.clone(),
            transport,
            tools: Vec::new(),
            request_id: AtomicU64::new(1),
            state: ConnectionState::Connecting,
            config,
            read_timeout_secs,
            cancel_token,
        };

        let startup_result: Result<()> = async {
            // Initialize with timeout
            tokio::time::timeout(Duration::from_secs(connect_timeout_secs), conn.initialize())
                .await
                .with_context(|| format!("MCP server '{name}' initialization timed out"))??;

            // Discover the only MCP capability retained by the product: tools.
            tokio::time::timeout(
                Duration::from_secs(connect_timeout_secs),
                conn.discover_tools(),
            )
            .await
            .with_context(|| format!("MCP server '{name}' tool discovery timed out"))??;
            Ok(())
        }
        .await;
        if let Err(err) = startup_result {
            conn.shutdown().await;
            return Err(err);
        }

        conn.state = ConnectionState::Ready;
        Ok(conn)
    }

    /// Send initialize request and wait for response
    async fn initialize(&mut self) -> Result<()> {
        let init_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": &init_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "codewhale-tui",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            }
        }))
        .await?;

        self.recv(init_id).await?;

        // Send initialized notification (no id, no response expected)
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .await?;

        Ok(())
    }

    /// Discover available tools from the MCP server
    async fn discover_tools(&mut self) -> Result<()> {
        let mut cursor: Option<String> = None;
        loop {
            let list_id = self.next_id();
            let params = match &cursor {
                Some(c) => serde_json::json!({ "cursor": c }),
                None => serde_json::json!({}),
            };
            self.send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": &list_id,
                "method": "tools/list",
                "params": params
            }))
            .await?;

            let response = self.recv(list_id).await?;
            let Some(result) = response.get("result") else {
                break;
            };

            if let Some(arr) = result.get("tools").and_then(|t| t.as_array()) {
                for item in arr {
                    match serde_json::from_value::<McpTool>(item.clone()) {
                        Ok(tool) => self.tools.push(tool),
                        Err(err) => {
                            // Skip individual malformed entries instead of
                            // dropping the whole page (#1410). The old
                            // `unwrap_or_default()` would silently throw
                            // away every tool when one was misshapen.
                            tracing::debug!(target: "mcp", ?err, "skipping malformed tool item");
                        }
                    }
                }
            }

            cursor = result
                .get("nextCursor")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        // Sort by tool name so CLI discovery output is deterministic even
        // when server-side pagination returns an unstable order.
        self.tools.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }

    /// Get discovered tools
    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    /// Check if connection is ready
    pub fn is_ready(&self) -> bool {
        self.state == ConnectionState::Ready
    }

    /// Get server config
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    fn next_id(&self) -> String {
        self.request_id.fetch_add(1, Ordering::SeqCst).to_string()
    }

    async fn send(&mut self, msg: serde_json::Value) -> Result<()> {
        let bytes = serde_json::to_vec(&msg).context("Failed to serialize MCP JSON-RPC message")?;
        self.transport.send(bytes).await
    }

    async fn recv(&mut self, expected_id: String) -> Result<serde_json::Value> {
        loop {
            let bytes = match tokio::time::timeout(
                Duration::from_secs(self.read_timeout_secs),
                self.transport.recv(),
            )
            .await
            {
                Ok(result) => result.inspect_err(|_e| {
                    self.state = ConnectionState::Disconnected;
                })?,
                Err(_) => {
                    self.state = ConnectionState::Disconnected;
                    anyhow::bail!(
                        "Timed out waiting for MCP JSON-RPC response from server '{}' after {}s",
                        self.name,
                        self.read_timeout_secs
                    );
                }
            };
            let value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(err) => {
                    self.state = ConnectionState::Disconnected;
                    return Err(err).with_context(|| {
                        format!(
                            "Invalid MCP JSON-RPC message from server '{}': {}",
                            self.name,
                            invalid_json_preview(&bytes)
                        )
                    });
                }
            };

            // Check if this is a response with the expected id. We emit
            // string IDs because some MCP gateways reject numeric JSON-RPC
            // IDs, but accept numeric echoes for compatibility with older
            // servers and tests.
            if response_id_matches(value.get("id"), &expected_id) {
                if let Some(error) = value.get("error")
                    && is_mcp_stale_session_body(&error.to_string())
                {
                    anyhow::bail!("MCP session expired: {error}");
                }
                return Ok(value);
            }
            // Skip notifications (no id) and responses with different ids
        }
    }

    async fn shutdown(&mut self) -> bool {
        self.cancel_token.cancel();
        let settled = self.transport.shutdown().await;
        self.state = ConnectionState::Disconnected;
        settled
    }
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        self.cancel_token.cancel();
    }
}

// === McpPool - Connection Pool Management ===

/// Pool of MCP connections for reuse
pub struct McpPool {
    connections: HashMap<String, McpConnection>,
    config: McpConfig,
    network_policy: Option<NetworkPolicyDecider>,
    /// Source paths the config was loaded from. Empty for pools constructed
    /// directly via `new` (tests, ad-hoc snapshots). Workspace-aware pools
    /// track both global and project-level MCP config paths so lazy reload sees
    /// either file appear or change.
    config_sources: Vec<PathBuf>,
    workspace: Option<PathBuf>,
    /// 64-bit content hash of the active config (`hash_mcp_config`). Compared
    /// against the freshly-loaded config after an mtime change to skip
    /// reloading when the file was merely touched.
    config_hash: u64,
    /// Most recently observed mtime for `config_sources`.
    last_mtimes: Vec<Option<std::time::SystemTime>>,
}

impl McpPool {
    /// Create a new pool with the given configuration
    pub fn new(config: McpConfig) -> Self {
        let config_hash = hash_mcp_config(&config);
        Self {
            connections: HashMap::new(),
            config,
            network_policy: None,
            config_sources: Vec::new(),
            workspace: None,
            config_hash,
            last_mtimes: Vec::new(),
        }
    }

    /// Create a pool from a configuration file path.
    #[cfg(test)]
    pub fn from_config_path(path: &std::path::Path) -> Result<Self> {
        let config = load_config(path)?;
        let mut pool = Self::new(config);
        pool.config_sources = vec![path.to_path_buf()];
        pool.last_mtimes = vec![mcp_config_mtime(path)];
        Ok(pool)
    }

    /// Create a pool from global MCP config plus workspace-local
    /// `.codewhale/mcp.json`. Project servers override same-name global
    /// servers and default stdio `cwd` to the workspace root.
    pub fn from_config_path_with_workspace(
        path: &std::path::Path,
        workspace: &Path,
    ) -> Result<Self> {
        let config = load_config_with_workspace(path, workspace)?;
        let workspace = checked_workspace_path(workspace)?;
        let mut pool = Self::new(config);
        pool.config_sources = vec![
            path.to_path_buf(),
            checked_workspace_mcp_config_path(&workspace)?,
        ];
        pool.config_sources
            .extend(crate::config::workspace_trust_config_candidate_paths());
        pool.last_mtimes = pool
            .config_sources
            .iter()
            .map(|source| mcp_config_mtime(source))
            .collect();
        pool.workspace = Some(workspace);
        Ok(pool)
    }

    /// Attach a per-domain network policy (#135). When set, HTTP/SSE
    /// transports are gated through it; STDIO transports are unaffected.
    pub fn with_network_policy(mut self, policy: NetworkPolicyDecider) -> Self {
        self.network_policy = Some(policy);
        self
    }

    async fn shutdown_connection(&mut self, server_name: &str, reason: &str) {
        if let Some(mut connection) = self.connections.remove(server_name) {
            tracing::debug!(
                target: "mcp",
                server = %server_name,
                reason = %reason,
                "shutting down MCP connection"
            );
            let _ = connection.shutdown().await;
        }
    }

    async fn shutdown_all_connections(&mut self, reason: &str) {
        if self.connections.is_empty() {
            return;
        }
        let connections = std::mem::take(&mut self.connections);
        let count = connections.len();
        tracing::debug!(
            target: "mcp",
            count,
            reason = %reason,
            "shutting down MCP connections"
        );
        let _ = futures_util::future::join_all(
            connections
                .into_values()
                .map(|mut connection| async move { connection.shutdown().await }),
        )
        .await;
    }

    /// If the source config file's mtime has changed since the last check,
    /// re-read it and (only when the content hash also changed) shut down all
    /// existing connections so the next `get_or_connect` reattaches under
    /// the new config. No-op when the pool was constructed via [`McpPool::new`]
    /// (no source path), when stat fails, or when the file content is
    /// byte-identical to what we last loaded. Returns `Ok(true)` if any
    /// connections were replaced, `Ok(false)` otherwise.
    ///
    /// This is the lazy half of the auto-reload story for #1267: instead of a
    /// long-lived file watcher, the next connection lookup pays a single `stat`
    /// call (and only re-reads the file when the mtime moved). On networked
    /// or remote filesystems where mtime granularity is poor, the hash
    /// compare keeps us from churning connections on every check.
    pub async fn reload_if_config_changed(&mut self) -> Result<bool> {
        if self.config_sources.is_empty() {
            return Ok(false);
        }
        let current_mtimes: Vec<_> = self
            .config_sources
            .iter()
            .map(|path| mcp_config_mtime(path))
            .collect();
        if current_mtimes == self.last_mtimes {
            return Ok(false);
        }
        // mtime moved — we owe a re-read.
        let primary = self
            .config_sources
            .first()
            .context("MCP config source list unexpectedly empty")?;
        let new_config = if let Some(workspace) = self.workspace.as_deref() {
            load_config_with_workspace(primary, workspace)?
        } else {
            load_config(primary)?
        };
        let new_hash = hash_mcp_config(&new_config);
        // Always advance mtimes so a touched-but-unchanged file doesn't
        // make us re-read on every subsequent call.
        self.last_mtimes = current_mtimes;
        if new_hash == self.config_hash {
            return Ok(false);
        }
        // Real content change — settle all live connections so the next
        // get_or_connect picks up the new config (sandbox flags, env, args).
        self.shutdown_all_connections("config reload").await;
        self.config = new_config;
        self.config_hash = new_hash;
        Ok(true)
    }

    /// Get or create a connection to a server
    pub async fn get_or_connect(&mut self, server_name: &str) -> Result<&mut McpConnection> {
        // Lazy auto-reload (#1267 part 2): cheap mtime-then-hash check before
        // each connection lookup. Transient FS errors are logged but not
        // propagated so a brief hiccup can't take down the diagnostic command.
        if let Err(e) = self.reload_if_config_changed().await {
            tracing::warn!("MCP config reload check failed: {e:#}");
        }

        let is_ready = self
            .connections
            .get(server_name)
            .map(|conn| conn.is_ready())
            .unwrap_or(false);
        if is_ready {
            return self
                .connections
                .get_mut(server_name)
                .ok_or_else(|| anyhow::anyhow!("MCP connection disappeared for {server_name}"));
        }

        self.shutdown_connection(server_name, "reconnect").await;

        let server_config = self
            .config
            .servers
            .get(server_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Failed to find MCP server: {server_name}"))?;

        if !server_config.is_enabled() {
            anyhow::bail!("Failed to connect MCP server '{server_name}': server is disabled");
        }

        let connection = McpConnection::connect_with_policy(
            server_name.to_string(),
            server_config,
            &self.config.timeouts,
            self.network_policy.as_ref(),
        )
        .await?;

        self.connections.insert(server_name.to_string(), connection);
        self.connections
            .get_mut(server_name)
            .ok_or_else(|| anyhow::anyhow!("Failed to store MCP connection for {server_name}"))
    }

    /// Connect to all enabled servers, returning errors for failed connections
    pub async fn connect_all(&mut self) -> Vec<(String, anyhow::Error)> {
        let mut errors = Vec::new();
        let names: Vec<String> = self
            .config
            .servers
            .keys()
            .filter(|n| self.config.servers[*n].is_enabled())
            .cloned()
            .collect();

        for name in names {
            if let Err(e) = self.get_or_connect(&name).await {
                errors.push((name, e));
            }
        }

        for (name, server_cfg) in &self.config.servers {
            if server_cfg.required
                && server_cfg.is_enabled()
                && !self
                    .connections
                    .get(name)
                    .is_some_and(McpConnection::is_ready)
            {
                errors.push((
                    name.clone(),
                    anyhow::anyhow!("required MCP server failed to initialize"),
                ));
            }
        }

        errors
    }

    /// Get all discovered tools with server-prefixed names
    pub fn all_tools(&self) -> Vec<(String, &McpTool)> {
        let mut tools = Vec::new();
        for (server, conn) in &self.connections {
            for tool in conn.tools() {
                if !conn.config().is_tool_enabled(&tool.name) {
                    continue;
                }
                // Format: mcp_{server}_{tool}
                tools.push((format!("mcp_{}_{}", server, tool.name), tool));
            }
        }
        // Sort by prefixed name so combined CLI output is deterministic across
        // HashMap iteration orders.
        tools.sort_by(|a, b| a.0.cmp(&b.0));
        tools
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpWriteStatus {
    Created,
    Overwritten,
    SkippedExists,
}

pub fn load_config(path: &Path) -> Result<McpConfig> {
    validate_mcp_config_path(path)?;
    let Some(contents) = read_mcp_config_file(path)? else {
        return Ok(McpConfig::default());
    };
    serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse MCP config {}", path.display()))
}

fn read_mcp_config_file(path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to inspect MCP config {}", path.display()));
        }
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_file() {
        anyhow::bail!("MCP config path must be a regular file: {}", path.display());
    }

    let mut file = open_mcp_config_file(path)
        .with_context(|| format!("Failed to read MCP config {}", path.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .with_context(|| format!("Failed to read MCP config {}", path.display()))?;
    Ok(Some(contents))
}

#[cfg(unix)]
fn open_mcp_config_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_mcp_config_file(path: &Path) -> std::io::Result<fs::File> {
    fs::File::open(path)
}

pub fn workspace_mcp_config_path(workspace: &Path) -> PathBuf {
    normalize_workspace_path(workspace)
        .join(".codewhale")
        .join("mcp.json")
}

pub fn load_config_with_workspace(global_path: &Path, workspace: &Path) -> Result<McpConfig> {
    let mut merged = load_config(global_path)?;
    let workspace = checked_workspace_path(workspace)?;
    let project_path = checked_workspace_mcp_config_path(&workspace)?;
    if !project_path.exists() || paths_refer_to_same_config(global_path, &project_path) {
        return Ok(merged);
    }
    // Workspace-local MCP can spawn stdio servers, so it is only honored after
    // the user has trusted this workspace in user-owned config. Do not accept
    // project-local legacy trust markers here: a repository could carry those
    // files itself and silently reintroduce the project-scope `mcp_config_path`
    // risk denied in #417.
    if !workspace_allows_project_mcp_config(&workspace) {
        return Ok(merged);
    }

    let mut project = load_config(&project_path)?;
    for server in project.servers.values_mut() {
        if server.command.is_some() && server.url.is_none() {
            server.cwd = Some(resolve_project_mcp_cwd(&workspace, server.cwd.as_deref())?);
        }
    }
    merged.servers.extend(project.servers);

    merged = merge_plugin_mcp_servers(merged)?;

    Ok(merged)
}

fn merge_plugin_mcp_servers(config: McpConfig) -> Result<McpConfig> {
    let plugins = crate::plugins::try_with_registry(|r| {
        r.list_enabled()
            .into_iter()
            .map(|(name, plugin)| (name.clone(), plugin.clone()))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();

    merge_plugin_mcp_servers_from_plugins(config, plugins)
}

fn merge_plugin_mcp_servers_from_plugins(
    mut config: McpConfig,
    plugins: impl IntoIterator<Item = (String, crate::plugins::manifest::LoadedPlugin)>,
) -> Result<McpConfig> {
    for (plugin_name, plugin) in plugins {
        if let Some(mcp_servers) = &plugin.manifest.mcp_servers {
            for (server_name, server_config) in mcp_servers {
                let qualified_name = format!("{}-{}", plugin_name, server_name);
                let mut server_config = server_config.clone();

                if server_config.command.is_some() && server_config.url.is_none() {
                    server_config.cwd = Some(resolve_plugin_mcp_cwd(
                        &plugin.base_path,
                        server_config.cwd.as_deref(),
                    )?);
                }

                config.servers.insert(qualified_name, server_config);
            }
        }
    }

    Ok(config)
}

fn resolve_plugin_mcp_cwd(plugin_path: &Path, cwd: Option<&Path>) -> Result<PathBuf> {
    let cwd = match cwd {
        Some(cwd) if cwd.is_relative() => normalize_path_components(&plugin_path.join(cwd)),
        Some(cwd) => normalize_path_components(cwd),
        None => plugin_path.to_path_buf(),
    };
    Ok(cwd
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_components(&cwd)))
}

fn workspace_allows_project_mcp_config(workspace: &Path) -> bool {
    crate::config::is_workspace_trusted(workspace)
}

fn checked_workspace_mcp_config_path(workspace: &Path) -> Result<PathBuf> {
    Ok(checked_workspace_path(workspace)?
        .join(".codewhale")
        .join("mcp.json"))
}

fn checked_workspace_path(workspace: &Path) -> Result<PathBuf> {
    if workspace.as_os_str().is_empty() {
        anyhow::bail!("workspace path cannot be empty");
    }
    if workspace
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("workspace path cannot contain '..' components");
    }
    let absolute = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for workspace")?
            .join(workspace)
    };
    match absolute.canonicalize() {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_path_components(&absolute))
        }
        Err(err) => {
            Err(err).with_context(|| format!("failed to resolve workspace {}", workspace.display()))
        }
    }
}

fn normalize_workspace_path(workspace: &Path) -> PathBuf {
    if let Ok(canonical) = workspace.canonicalize() {
        return canonical;
    }
    let absolute = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace)
    };
    normalize_path_components(&absolute)
}

fn resolve_project_mcp_cwd(workspace: &Path, cwd: Option<&Path>) -> Result<PathBuf> {
    let cwd = match cwd {
        Some(cwd) if cwd.is_relative() => normalize_path_components(&workspace.join(cwd)),
        Some(cwd) => normalize_path_components(cwd),
        None => workspace.to_path_buf(),
    };
    let resolved = cwd
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_components(&cwd));
    if !resolved.starts_with(workspace) {
        anyhow::bail!(
            "Project MCP server cwd must stay within workspace: {}",
            resolved.display()
        );
    }
    Ok(resolved)
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn paths_refer_to_same_config(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => normalize_workspace_path(left) == normalize_workspace_path(right),
    }
}

/// 64-bit content hash of an [`McpConfig`]. Used by [`McpPool`] to decide
/// whether a freshly-read config differs from the one currently driving the
/// live connections. Hashing the JSON serialization avoids forcing every
/// nested config type to derive `Hash` (the timeouts struct, network policy
/// stubs, etc.). The hash is stable across runs of the same Rust toolchain
/// for byte-identical input.
fn hash_mcp_config(config: &McpConfig) -> u64 {
    use std::hash::{Hash, Hasher};
    let bytes = serde_json::to_vec(config).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Best-effort fetch of the MCP config file's last-modified time. Returns
/// `None` when the file is missing, when stat fails, when the platform
/// doesn't expose mtime, or when the path fails the same allow-list check
/// that `load_config` / `save_config` apply. The lazy-reload check in
/// `McpPool::get_or_connect` treats `None` as "skip the check this turn",
/// so a rejected path simply degrades to "no auto-reload" rather than an
/// error path. Callers already validate via `validate_mcp_config_path` at
/// construction time; the redundant validation here keeps this helper
/// safe-by-construction for any future caller and ties the validation to
/// the call site rather than relying on cross-function reasoning.
fn mcp_config_mtime(path: &Path) -> Option<std::time::SystemTime> {
    validate_mcp_config_path(path).ok()?;
    fs::metadata(path).ok()?.modified().ok()
}

pub fn save_config(path: &Path, cfg: &McpConfig) -> Result<()> {
    validate_mcp_config_path(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create MCP config directory {}", parent.display())
        })?;
    }
    let rendered = serde_json::to_string_pretty(cfg).context("Failed to serialize MCP config")?;
    write_atomic(path, rendered.as_bytes())
        .with_context(|| format!("Failed to write MCP config {}", path.display()))?;
    Ok(())
}

fn mcp_template_json() -> Result<String> {
    let mut cfg = McpConfig::default();
    cfg.servers.insert(
        "example".to_string(),
        McpServerConfig {
            command: Some("node".to_string()),
            args: vec!["./path/to/your-mcp-server.js".to_string()],
            env: HashMap::new(),
            cwd: None,
            url: None,
            transport: None,
            connect_timeout: None,
            execute_timeout: None,
            read_timeout: None,
            disabled: true,
            enabled: true,
            required: false,
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            headers: HashMap::new(),
            env_headers: HashMap::new(),
            bearer_token_env_var: None,
            scopes: Vec::new(),
            oauth: None,
            oauth_resource: None,
        },
    );
    cfg.servers.insert(
        "moraine-mcp".to_string(),
        McpServerConfig {
            command: Some("moraine".to_string()),
            args: vec!["mcp".to_string()],
            env: HashMap::new(),
            cwd: None,
            url: None,
            transport: None,
            connect_timeout: None,
            execute_timeout: None,
            read_timeout: None,
            disabled: true,
            enabled: true,
            required: false,
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            headers: HashMap::new(),
            env_headers: HashMap::new(),
            bearer_token_env_var: None,
            scopes: Vec::new(),
            oauth: None,
            oauth_resource: None,
        },
    );
    serde_json::to_string_pretty(&cfg).context("Failed to render MCP template JSON")
}

pub fn init_config(path: &Path, force: bool) -> Result<McpWriteStatus> {
    validate_mcp_config_path(path)?;
    if path.exists() && !force {
        return Ok(McpWriteStatus::SkippedExists);
    }
    let status = if path.exists() {
        McpWriteStatus::Overwritten
    } else {
        McpWriteStatus::Created
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create MCP config directory {}", parent.display())
        })?;
    }
    let template = mcp_template_json()?;
    write_atomic(path, template.as_bytes())
        .with_context(|| format!("Failed to write MCP config {}", path.display()))?;
    Ok(status)
}

pub fn add_server_config(path: &Path, name: String, server: McpServerConfig) -> Result<()> {
    if server.command.is_none() && server.url.is_none() {
        anyhow::bail!("Provide either a command or URL for MCP server '{name}'.");
    }
    validate_mcp_transport(server.transport.as_deref())?;
    let mut cfg = load_config(path)?;
    cfg.servers.insert(name, server);
    save_config(path, &cfg)
}

pub fn remove_server_config(path: &Path, name: &str) -> Result<()> {
    let mut cfg = load_config(path)?;
    if cfg.servers.remove(name).is_none() {
        anyhow::bail!("MCP server '{name}' not found");
    }
    save_config(path, &cfg)
}

pub fn set_server_enabled(path: &Path, name: &str, enabled: bool) -> Result<()> {
    let mut cfg = load_config(path)?;
    let server = cfg
        .servers
        .get_mut(name)
        .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not found"))?;
    server.enabled = enabled;
    server.disabled = !enabled;
    save_config(path, &cfg)
}

// === Unit Tests ===

#[cfg(test)]
mod tests;
