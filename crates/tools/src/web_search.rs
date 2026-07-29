use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
    ToolTransportStatus,
};
use dse_secrets::Secrets;
use futures_util::StreamExt;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderName, USER_AGENT,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::web_fetch::{canonical_public_http_url, is_public_ip};
use crate::{ToolOutcome, optional_u64, required_str};

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS: usize = 10;
const MAX_QUERY_CHARS: usize = 512;
const MAX_QUERY_BYTES: usize = 2_048;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_TITLE_CHARS: usize = 512;
const MAX_SNIPPET_CHARS: usize = 2_000;
const MAX_PUBLISHED_DATE_CHARS: usize = 128;
const SEARCH_DEADLINE: Duration = Duration::from_secs(12);
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";
const TAVILY_HOST: &str = "api.tavily.com";
const TAVILY_API_KEY: &str = "TAVILY_API_KEY";
const ADAPTER_ID: &str = "tavily_basic_web_search_https_v1";
const TRUST: &str = "external_untrusted";
const RESPONSE_SHA256_SCOPE: &str = "received_search_response_replay_identity";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSearchHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub bytes_read: u64,
}

impl WebSearchHttpResponse {
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WebSearchNetworkError {
    pub code: &'static str,
    pub message: String,
    pub request_started: bool,
    pub status: Option<u16>,
    pub retry: ToolRetryDisposition,
}

impl WebSearchNetworkError {
    fn before_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            request_started: false,
            status: None,
            retry: ToolRetryDisposition::AfterCorrection,
        }
    }

    fn transport(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            request_started: true,
            status: None,
            retry: ToolRetryDisposition::Safe,
        }
    }

    fn transport_ambiguous(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            request_started: true,
            status: None,
            retry: ToolRetryDisposition::Unsafe,
        }
    }
}

#[async_trait]
pub trait WebSearchNetwork: Send + Sync {
    fn identity(&self) -> String;

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        timeout: Duration,
    ) -> Result<WebSearchHttpResponse, WebSearchNetworkError>;
}

#[derive(Clone)]
pub struct SystemWebSearchNetwork {
    secrets: Secrets,
}

impl std::fmt::Debug for SystemWebSearchNetwork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SystemWebSearchNetwork")
            .field("adapter", &ADAPTER_ID)
            .field("secret_backend", &self.secrets.backend_name())
            .finish()
    }
}

impl Default for SystemWebSearchNetwork {
    fn default() -> Self {
        Self {
            secrets: Secrets::auto_detect(),
        }
    }
}

impl SystemWebSearchNetwork {
    #[must_use]
    pub fn with_secrets(secrets: Secrets) -> Self {
        Self { secrets }
    }
}

#[async_trait]
impl WebSearchNetwork for SystemWebSearchNetwork {
    fn identity(&self) -> String {
        format!(
            "{ADAPTER_ID};endpoint={TAVILY_ENDPOINT};secret_backend={}",
            self.secrets.backend_name()
        )
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        timeout: Duration,
    ) -> Result<WebSearchHttpResponse, WebSearchNetworkError> {
        let api_key = self
            .secrets
            .resolve_direct(TAVILY_API_KEY, None)
            .ok_or_else(|| {
                WebSearchNetworkError::before_request(
                    "web_search_credential_unavailable",
                    "Host secret backend and environment do not contain TAVILY_API_KEY",
                )
            })?;
        let mut addresses = tokio::net::lookup_host((TAVILY_HOST, 443))
            .await
            .map_err(|error| {
                WebSearchNetworkError::transport(
                    "web_search_dns_failed",
                    format!("fixed search endpoint DNS resolution failed: {error}"),
                )
            })?
            .collect::<Vec<_>>();
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|address| address.port() != 443 || !is_public_ip(address.ip()))
        {
            return Err(WebSearchNetworkError::transport(
                "web_search_dns_target_denied",
                "fixed search endpoint did not resolve exclusively to public unicast addresses",
            ));
        }
        addresses.sort_unstable();
        addresses.dedup();

        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .no_gzip()
            .https_only(true)
            .connect_timeout(CONNECT_DEADLINE.min(timeout))
            .timeout(timeout)
            .resolve_to_addrs(TAVILY_HOST, &addresses)
            .build()
            .map_err(|error| {
                WebSearchNetworkError::before_request(
                    "web_search_client_failed",
                    format!("cannot construct bounded search client: {error}"),
                )
            })?;
        let response = client
            .post(TAVILY_ENDPOINT)
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .header(
                USER_AGENT,
                concat!("DSE/", env!("CARGO_PKG_VERSION"), " web_search"),
            )
            .bearer_auth(api_key)
            .json(&tavily_request_body(query, max_results))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    WebSearchNetworkError::transport_ambiguous(
                        "web_search_request_timeout",
                        format!("search request timed out after it may have started: {error}"),
                    )
                } else if error.is_connect() {
                    WebSearchNetworkError::transport(
                        "web_search_connect_failed",
                        format!("search request failed to connect: {error}"),
                    )
                } else {
                    WebSearchNetworkError::transport_ambiguous(
                        "web_search_request_failed",
                        format!("search request failed after it may have started: {error}"),
                    )
                }
            })?;

        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for name in [CONTENT_TYPE, CONTENT_LENGTH] {
            if let Some(value) = response.headers().get(&name) {
                let value = value.to_str().map_err(|_| {
                    WebSearchNetworkError::transport(
                        "web_search_header_invalid",
                        format!("response header {} is not valid ASCII", name.as_str()),
                    )
                })?;
                headers.insert(name.as_str().to_owned(), value.to_owned());
            }
        }
        let request_id_header = HeaderName::from_static("x-request-id");
        if let Some(value) = response.headers().get(&request_id_header)
            && let Ok(value) = value.to_str()
        {
            headers.insert(request_id_header.as_str().to_owned(), value.to_owned());
        }
        if let Some(length) = headers
            .get(CONTENT_LENGTH.as_str())
            .and_then(|value| value.parse::<u64>().ok())
            && length > MAX_RESPONSE_BYTES as u64
        {
            return Err(WebSearchNetworkError {
                code: "web_search_response_too_large",
                message: format!(
                    "search response declares {length} bytes, above {MAX_RESPONSE_BYTES} byte limit"
                ),
                request_started: true,
                status: Some(status),
                retry: ToolRetryDisposition::NotRetryable,
            });
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                WebSearchNetworkError::transport_ambiguous(
                    "web_search_response_failed",
                    format!("cannot read search response: {error}"),
                )
            })?;
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(WebSearchNetworkError {
                    code: "web_search_response_too_large",
                    message: format!("search response exceeded {MAX_RESPONSE_BYTES} byte limit"),
                    request_started: true,
                    status: Some(status),
                    retry: ToolRetryDisposition::NotRetryable,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(WebSearchHttpResponse::new(status, headers, body))
    }
}

fn tavily_request_body(query: &str, max_results: usize) -> Value {
    json!({
        "query": query,
        "search_depth": "basic",
        "max_results": max_results,
        "topic": "general",
        "auto_parameters": false,
        "include_answer": false,
        "include_raw_content": false,
        "include_images": false,
        "include_usage": true,
    })
}

#[derive(Debug, Serialize)]
struct WebSearchResult {
    query: String,
    provider: &'static str,
    provider_request_id: String,
    provider_response_time: Option<String>,
    provider_usage_credits: Option<u64>,
    billing_truth: &'static str,
    results: Vec<WebSearchItem>,
    results_read: usize,
    results_returned: usize,
    results_dropped: usize,
    truncated: bool,
    retrieved_at: String,
    response_sha256: String,
    response_sha256_scope: &'static str,
    bytes_read: u64,
    bytes_returned: u64,
    evidence_role: &'static str,
    citation_requirement: &'static str,
    trust: &'static str,
}

#[derive(Debug, Serialize)]
struct WebSearchItem {
    rank: usize,
    title: String,
    url: String,
    source_host: String,
    snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_score: Option<Value>,
}

#[derive(Debug)]
struct SearchFailure {
    code: &'static str,
    stage: &'static str,
    message: String,
    status: Option<u16>,
    transport: bool,
    request_started: bool,
    retry: ToolRetryDisposition,
}

pub(crate) fn preflight_web_search(input: &Value) -> Option<ToolOutcome> {
    match parse_input(input) {
        Ok(_) => None,
        Err(failure) => Some(rejected_outcome(input, failure)),
    }
}

#[cfg(test)]
pub(crate) async fn execute_web_search(
    input: Value,
    network: Arc<dyn WebSearchNetwork>,
    network_allowed: bool,
) -> ToolOutcome {
    execute_web_search_cancellable(input, network, network_allowed, CancellationToken::new()).await
}

pub(crate) async fn execute_web_search_cancellable(
    input: Value,
    network: Arc<dyn WebSearchNetwork>,
    network_allowed: bool,
    cancellation: CancellationToken,
) -> ToolOutcome {
    execute_web_search_with_deadline(
        input,
        network,
        network_allowed,
        SEARCH_DEADLINE,
        cancellation,
    )
    .await
}

async fn execute_web_search_with_deadline(
    input: Value,
    network: Arc<dyn WebSearchNetwork>,
    network_allowed: bool,
    deadline: Duration,
    cancellation: CancellationToken,
) -> ToolOutcome {
    let (query, max_results) = match parse_input(&input) {
        Ok(parsed) => parsed,
        Err(failure) => return operation_outcome(&input, failure),
    };
    if !network_allowed {
        return operation_outcome(
            &input,
            SearchFailure {
                code: "web_search_network_not_authorized",
                stage: "authorization",
                message: "current actor boundary denies Host-controlled network access".to_owned(),
                status: None,
                transport: false,
                request_started: false,
                retry: ToolRetryDisposition::NotRetryable,
            },
        );
    }
    if cancellation.is_cancelled() {
        return cancelled_outcome(&input, false);
    }
    let operation = tokio::time::timeout(deadline, network.search(&query, max_results, deadline));
    tokio::pin!(operation);
    let result = tokio::select! {
        () = cancellation.cancelled() => return cancelled_outcome(&input, true),
        result = &mut operation => result,
    };
    match result {
        Ok(Ok(response)) => match parse_response(&query, max_results, response) {
            Ok(result) => ToolOutcome::json(&result)
                .expect("bounded web_search result serializes")
                .with_side_effect(ToolSideEffectStatus::NotApplicable),
            Err(failure) => operation_outcome(&input, failure),
        },
        Ok(Err(error)) => operation_outcome(
            &input,
            SearchFailure {
                code: error.code,
                stage: if error.request_started {
                    "request"
                } else {
                    "credential"
                },
                message: error.message,
                status: error.status,
                transport: error.request_started && error.status.is_none(),
                request_started: error.request_started,
                retry: error.retry,
            },
        ),
        Err(_) => operation_outcome(
            &input,
            SearchFailure {
                code: "web_search_deadline_exceeded",
                stage: "deadline",
                message: format!("web_search exceeded {} ms deadline", deadline.as_millis()),
                status: None,
                transport: true,
                request_started: true,
                retry: ToolRetryDisposition::Unsafe,
            },
        ),
    }
}

fn cancelled_outcome(input: &Value, request_started: bool) -> ToolOutcome {
    let failure = SearchFailure {
        code: "web_search_cancelled",
        stage: "cancellation",
        message: if request_started {
            "web_search was cancelled after a provider request may have started"
        } else {
            "web_search was cancelled before the provider request started"
        }
        .to_owned(),
        status: None,
        transport: request_started,
        request_started,
        retry: if request_started {
            ToolRetryDisposition::Unsafe
        } else {
            ToolRetryDisposition::Safe
        },
    };
    let mut outcome = operation_outcome(input, failure);
    outcome.operation = ToolOperationStatus::Cancelled;
    outcome.transport = if request_started {
        ToolTransportStatus::Indeterminate
    } else {
        ToolTransportStatus::NotStarted
    };
    outcome
}

fn parse_input(input: &Value) -> Result<(String, usize), SearchFailure> {
    let query = required_str(input, "query").map_err(|error| SearchFailure {
        code: "web_search_query_missing",
        stage: "query",
        message: error.to_string(),
        status: None,
        transport: false,
        request_started: false,
        retry: ToolRetryDisposition::AfterCorrection,
    })?;
    if query.trim() != query
        || query.is_empty()
        || query.chars().count() > MAX_QUERY_CHARS
        || query.len() > MAX_QUERY_BYTES
        || query.chars().any(char::is_control)
    {
        return Err(SearchFailure {
            code: "web_search_query_invalid",
            stage: "query",
            message: format!(
                "query must be non-empty without surrounding whitespace/control characters and within {MAX_QUERY_CHARS} chars/{MAX_QUERY_BYTES} bytes"
            ),
            status: None,
            transport: false,
            request_started: false,
            retry: ToolRetryDisposition::AfterCorrection,
        });
    }
    let max_results = usize::try_from(optional_u64(
        input,
        "max_results",
        DEFAULT_MAX_RESULTS as u64,
    ))
    .unwrap_or(MAX_RESULTS)
    .clamp(1, MAX_RESULTS);
    Ok((query.to_owned(), max_results))
}

fn parse_response(
    query: &str,
    max_results: usize,
    response: WebSearchHttpResponse,
) -> Result<WebSearchResult, SearchFailure> {
    if response.body.len() > MAX_RESPONSE_BYTES || response.bytes_read > MAX_RESPONSE_BYTES as u64 {
        return Err(response_failure(
            "web_search_response_too_large",
            "body",
            "search response exceeded the byte limit",
            Some(response.status),
        ));
    }
    if response.status == 401 || response.status == 403 {
        return Err(response_failure(
            "web_search_credential_rejected",
            "response",
            "search endpoint rejected the Host-owned credential",
            Some(response.status),
        ));
    }
    if response.status == 429 {
        let mut failure = response_failure(
            "web_search_rate_limited",
            "response",
            "search endpoint rate limit was reached",
            Some(response.status),
        );
        failure.retry = ToolRetryDisposition::Safe;
        return Err(failure);
    }
    if !(200..300).contains(&response.status) {
        return Err(response_failure(
            "web_search_http_status",
            "response",
            format!("search endpoint returned HTTP {}", response.status),
            Some(response.status),
        ));
    }
    let media_type = response
        .header(CONTENT_TYPE.as_str())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or_default();
    if media_type != "application/json" {
        return Err(response_failure(
            "web_search_content_type_denied",
            "content_type",
            "search endpoint did not return application/json",
            Some(response.status),
        ));
    }
    let payload: Value = serde_json::from_slice(&response.body).map_err(|error| {
        response_failure(
            "web_search_response_invalid",
            "decode",
            format!("search endpoint returned invalid JSON: {error}"),
            Some(response.status),
        )
    })?;
    let request_id = payload
        .get("request_id")
        .and_then(Value::as_str)
        .or_else(|| response.header("x-request-id"))
        .filter(|value| !value.is_empty() && value.chars().count() <= 256)
        .ok_or_else(|| {
            response_failure(
                "web_search_request_id_missing",
                "decode",
                "search response is missing a bounded request identity",
                Some(response.status),
            )
        })?;
    let raw_results = payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            response_failure(
                "web_search_results_missing",
                "decode",
                "search response is missing results[]",
                Some(response.status),
            )
        })?;
    let results_read = raw_results.len();
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    let mut results_dropped = 0usize;
    for raw in raw_results {
        if results.len() >= max_results {
            break;
        }
        let Some(raw_url) = raw.get("url").and_then(Value::as_str) else {
            results_dropped += 1;
            continue;
        };
        let Ok(url) = canonical_public_http_url(raw_url) else {
            results_dropped += 1;
            continue;
        };
        let canonical = url.as_str().to_owned();
        if !seen.insert(canonical.clone()) {
            results_dropped += 1;
            continue;
        }
        let title = raw
            .get("title")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, MAX_TITLE_CHARS))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned());
        let snippet = raw
            .get("content")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, MAX_SNIPPET_CHARS))
            .unwrap_or_default();
        let published_date = raw
            .get("published_date")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, MAX_PUBLISHED_DATE_CHARS))
            .filter(|value| !value.is_empty());
        let provider_score = raw.get("score").filter(|value| value.is_number()).cloned();
        results.push(WebSearchItem {
            rank: results.len() + 1,
            source_host: url.host_str().unwrap_or_default().to_owned(),
            title,
            url: canonical,
            snippet,
            published_date,
            provider_score,
        });
    }
    results_dropped += raw_results
        .len()
        .saturating_sub(results.len() + results_dropped);
    let response_sha256 = format!(
        "sha256:{}",
        Sha256::digest(&response.body)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let bytes_returned = serde_json::to_vec(&results)
        .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX);
    Ok(WebSearchResult {
        query: query.to_owned(),
        provider: ADAPTER_ID,
        provider_request_id: request_id.to_owned(),
        provider_response_time: payload
            .get("response_time")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, 64)),
        provider_usage_credits: payload
            .get("usage")
            .and_then(|usage| usage.get("credits"))
            .and_then(Value::as_u64),
        billing_truth: "provider_usage_units_only_actual_charge_unavailable",
        results,
        results_read,
        results_returned: results_read
            .saturating_sub(results_dropped)
            .min(max_results),
        results_dropped,
        truncated: results_read > max_results || results_dropped > 0,
        retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        response_sha256,
        response_sha256_scope: RESPONSE_SHA256_SCOPE,
        bytes_read: response.bytes_read,
        bytes_returned,
        evidence_role: "discovery_only",
        citation_requirement: "read_original_with_web_fetch_or_browser_and_cross_check_before_claim",
        trust: TRUST,
    })
}

fn response_failure(
    code: &'static str,
    stage: &'static str,
    message: impl Into<String>,
    status: Option<u16>,
) -> SearchFailure {
    SearchFailure {
        code,
        stage,
        message: message.into(),
        status,
        transport: false,
        request_started: true,
        retry: ToolRetryDisposition::NotRetryable,
    }
}

fn rejected_outcome(input: &Value, failure: SearchFailure) -> ToolOutcome {
    let retry = failure.retry;
    ToolOutcome::rejected(
        format!(
            "web_search rejected: code={}；{}",
            failure.code, failure.message
        ),
        retry,
    )
    .with_failure_code(ToolFailureCode::InvocationRejected)
    .with_metadata(failure_metadata(input, &failure))
}

fn operation_outcome(input: &Value, failure: SearchFailure) -> ToolOutcome {
    let mut outcome = if failure.transport {
        let mut outcome = ToolOutcome::transport_failure(format!(
            "web_search failed: code={}；{}",
            failure.code, failure.message
        ));
        outcome.retry = failure.retry;
        outcome
    } else {
        let mut outcome = ToolOutcome::error(format!(
            "web_search failed: code={}；{}",
            failure.code, failure.message
        ));
        outcome.operation = ToolOperationStatus::Failed;
        outcome.transport = if failure.request_started {
            ToolTransportStatus::Succeeded
        } else {
            ToolTransportStatus::NotStarted
        };
        outcome.retry = failure.retry;
        outcome
    };
    outcome.side_effect = ToolSideEffectStatus::NotApplicable;
    outcome.metadata = Some(failure_metadata(input, &failure));
    if failure.stage == "query" {
        outcome.failure_code = Some(ToolFailureCode::InvalidField);
    }
    outcome
}

fn failure_metadata(input: &Value, failure: &SearchFailure) -> Value {
    json!({
        "web_search": {
            "query": input.get("query").and_then(Value::as_str),
            "provider": ADAPTER_ID,
            "status": failure.status,
            "request_started": failure.request_started,
            "trust": TRUST,
            "evidence_role": "discovery_only",
            "billing_truth": "actual_charge_unavailable",
            "failure": {
                "code": failure.code,
                "stage": failure.stage,
                "message": failure.message,
            }
        }
    })
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug)]
    struct FixtureNetwork {
        response: Mutex<Option<Result<WebSearchHttpResponse, WebSearchNetworkError>>>,
        calls: Mutex<Vec<(String, usize)>>,
    }

    impl FixtureNetwork {
        fn success(payload: Value) -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Some(Ok(WebSearchHttpResponse::new(
                    200,
                    BTreeMap::from([(
                        "content-type".to_owned(),
                        "application/json; charset=utf-8".to_owned(),
                    )]),
                    serde_json::to_vec(&payload).unwrap(),
                )))),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl WebSearchNetwork for FixtureNetwork {
        fn identity(&self) -> String {
            "fixture-web-search-v1".to_owned()
        }

        async fn search(
            &self,
            query: &str,
            max_results: usize,
            _timeout: Duration,
        ) -> Result<WebSearchHttpResponse, WebSearchNetworkError> {
            self.calls
                .lock()
                .unwrap()
                .push((query.to_owned(), max_results));
            self.response.lock().unwrap().take().unwrap()
        }
    }

    fn response(results: Value) -> Value {
        json!({
            "query":"rust replay",
            "results":results,
            "response_time":"0.42",
            "request_id":"search-request-001",
            "usage":{"credits":1}
        })
    }

    #[test]
    fn provider_request_is_fixed_basic_discovery_with_usage() {
        assert_eq!(
            tavily_request_body("Rust replay", 5),
            json!({
                "query":"Rust replay",
                "search_depth":"basic",
                "max_results":5,
                "topic":"general",
                "auto_parameters":false,
                "include_answer":false,
                "include_raw_content":false,
                "include_images":false,
                "include_usage":true,
            })
        );
    }

    #[tokio::test]
    async fn bounded_search_results_are_discovery_only_with_canonical_sources() {
        let network = FixtureNetwork::success(response(json!([
            {"title":"Primary source","url":"https://example.com/docs#section","content":"Authoritative-looking but still only a snippet.","score":0.9},
            {"title":"Duplicate","url":"https://example.com/docs","content":"duplicate"},
            {"title":"Metadata","url":"http://169.254.169.254/latest","content":"denied"},
            {"title":"Second source","url":"http://example.org/reference","content":"Independent source"}
        ])));
        let outcome = execute_web_search(
            json!({"query":"rust replay","max_results":5}),
            network.clone(),
            true,
        )
        .await;
        assert!(outcome.is_success(), "{}", outcome.content);
        let payload: Value = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(payload["provider"], ADAPTER_ID);
        assert_eq!(payload["provider_usage_credits"], 1);
        assert_eq!(payload["evidence_role"], "discovery_only");
        assert_eq!(payload["trust"], TRUST);
        assert_eq!(payload["results_read"], 4);
        assert_eq!(payload["results_returned"], 2);
        assert_eq!(payload["results_dropped"], 2);
        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["results"][0]["rank"], 1);
        assert_eq!(payload["results"][0]["url"], "https://example.com/docs");
        assert_eq!(payload["results"][1]["rank"], 2);
        assert_eq!(
            payload["citation_requirement"],
            "read_original_with_web_fetch_or_browser_and_cross_check_before_claim"
        );
        assert_eq!(
            network.calls.lock().unwrap().as_slice(),
            &[("rust replay".to_owned(), 5)]
        );
    }

    #[tokio::test]
    async fn query_bounds_network_denial_and_provider_failures_are_typed() {
        let unused = FixtureNetwork::success(response(json!([])));
        for query in ["", " leading", "line\nbreak"] {
            let outcome = execute_web_search(json!({"query":query}), unused.clone(), true).await;
            assert_eq!(
                outcome.failure_code,
                Some(ToolFailureCode::InvalidField),
                "{query:?}: {outcome:?}"
            );
            assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplicable);
        }
        assert!(unused.calls.lock().unwrap().is_empty());

        let denied_network = FixtureNetwork::success(response(json!([])));
        let denied = execute_web_search(
            json!({"query":"bounded query"}),
            denied_network.clone(),
            false,
        )
        .await;
        assert_eq!(
            denied.metadata.as_ref().unwrap()["web_search"]["failure"]["code"],
            "web_search_network_not_authorized"
        );
        assert!(denied_network.calls.lock().unwrap().is_empty());

        let auth = Arc::new(FixtureNetwork {
            response: Mutex::new(Some(Ok(WebSearchHttpResponse::new(
                401,
                BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]),
                br#"{"detail":"redacted"}"#.to_vec(),
            )))),
            calls: Mutex::new(Vec::new()),
        });
        let failed = execute_web_search(json!({"query":"bounded query"}), auth, true).await;
        assert_eq!(
            failed.metadata.as_ref().unwrap()["web_search"]["failure"]["code"],
            "web_search_credential_rejected"
        );
        assert!(!failed.content.contains("redacted"));
    }

    #[tokio::test]
    async fn deadline_and_response_bounds_fail_without_leaking_body() {
        #[derive(Debug)]
        struct Pending;
        #[async_trait]
        impl WebSearchNetwork for Pending {
            fn identity(&self) -> String {
                "pending".to_owned()
            }

            async fn search(
                &self,
                _query: &str,
                _max_results: usize,
                _timeout: Duration,
            ) -> Result<WebSearchHttpResponse, WebSearchNetworkError> {
                std::future::pending().await
            }
        }
        let timeout = execute_web_search_with_deadline(
            json!({"query":"bounded query"}),
            Arc::new(Pending),
            true,
            Duration::from_millis(1),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(
            timeout.metadata.as_ref().unwrap()["web_search"]["failure"]["code"],
            "web_search_deadline_exceeded"
        );
        assert_eq!(timeout.retry, ToolRetryDisposition::Unsafe);

        let oversized = Arc::new(FixtureNetwork {
            response: Mutex::new(Some(Ok(WebSearchHttpResponse {
                status: 200,
                headers: BTreeMap::from([(
                    "content-type".to_owned(),
                    "application/json".to_owned(),
                )]),
                body: vec![b'x'; MAX_RESPONSE_BYTES + 1],
                bytes_read: (MAX_RESPONSE_BYTES + 1) as u64,
            }))),
            calls: Mutex::new(Vec::new()),
        });
        let failed = execute_web_search(json!({"query":"bounded query"}), oversized, true).await;
        assert_eq!(
            failed.metadata.as_ref().unwrap()["web_search"]["failure"]["code"],
            "web_search_response_too_large"
        );
        assert!(!failed.content.contains(&"x".repeat(64)));
    }

    #[tokio::test]
    async fn cancellation_is_immediate_and_unknown_provider_charge_is_not_retryable() {
        #[derive(Debug)]
        struct Pending;
        #[async_trait]
        impl WebSearchNetwork for Pending {
            fn identity(&self) -> String {
                "pending-cancellation".to_owned()
            }

            async fn search(
                &self,
                _query: &str,
                _max_results: usize,
                _timeout: Duration,
            ) -> Result<WebSearchHttpResponse, WebSearchNetworkError> {
                std::future::pending().await
            }
        }
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            trigger.cancel();
        });
        let outcome = execute_web_search_with_deadline(
            json!({"query":"bounded query"}),
            Arc::new(Pending),
            true,
            Duration::from_secs(30),
            cancellation,
        )
        .await;
        assert_eq!(outcome.operation, ToolOperationStatus::Cancelled);
        assert_eq!(outcome.transport, ToolTransportStatus::Indeterminate);
        assert_eq!(outcome.retry, ToolRetryDisposition::Unsafe);
        assert_eq!(
            outcome.metadata.as_ref().unwrap()["web_search"]["failure"]["code"],
            "web_search_cancelled"
        );
    }
}
