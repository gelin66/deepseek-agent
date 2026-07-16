use std::collections::BTreeMap;
use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use codewhale_runtime::{
    ModelFinishReason, ModelOutput, ModelStreamEvent, ModelToolCall, ToolArguments, Usage,
};
use futures_util::{Stream, StreamExt};
use reqwest::header::RETRY_AFTER;
use serde_json::Value;

use crate::{
    ApiRequestBudgetError, ApiRequestLease, ApiResponseAccountingGuard, ApiSurface, RequestPlan,
    ResponseMode, SharedApiRequestBudget,
};

const OFFICIAL_ROOT: &str = "https://api.deepseek.com";
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_SSE_BUFFER_BYTES: usize = 10 * 1024 * 1024;

pub type DeepSeekStream =
    Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, DeepSeekTransportError>> + Send + 'static>>;

#[derive(Clone, PartialEq, Eq)]
pub struct DeepSeekCredential(String);

impl DeepSeekCredential {
    pub fn new(value: impl Into<String>) -> Result<Self, DeepSeekTransportError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DeepSeekTransportError::InvalidConfig(
                "DeepSeek API credential must not be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DeepSeekCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeepSeekCredential([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepSeekEndpoint {
    Official,
    LoopbackFixture { root: String },
}

impl DeepSeekEndpoint {
    pub fn loopback_fixture(root: impl Into<String>) -> Result<Self, DeepSeekTransportError> {
        let root = normalize_fixture_root(&root.into())?;
        Ok(Self::LoopbackFixture { root })
    }

    #[must_use]
    pub fn root(&self) -> &str {
        match self {
            Self::Official => OFFICIAL_ROOT,
            Self::LoopbackFixture { root } => root,
        }
    }

    fn owns_url(&self, url: &str) -> bool {
        let root = self.root().trim_end_matches('/');
        let unversioned = root.strip_suffix("/v1").unwrap_or(root);
        url == format!("{root}/chat/completions")
            || url == format!("{unversioned}/beta/chat/completions")
    }

    fn url_for(&self, surface: ApiSurface) -> String {
        let root = self.root().trim_end_matches('/');
        match surface {
            ApiSurface::StandardChat => format!("{root}/chat/completions"),
            ApiSurface::StrictChat => format!(
                "{}/beta/chat/completions",
                root.strip_suffix("/v1").unwrap_or(root)
            ),
            ApiSurface::Fim => format!(
                "{}/beta/completions",
                root.strip_suffix("/v1").unwrap_or(root)
            ),
        }
    }
}

fn normalize_fixture_root(raw: &str) -> Result<String, DeepSeekTransportError> {
    let parsed = reqwest::Url::parse(raw).map_err(|error| {
        DeepSeekTransportError::InvalidConfig(format!(
            "invalid DeepSeek loopback fixture root: {error}"
        ))
    })?;
    if parsed.scheme() != "http"
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(DeepSeekTransportError::InvalidConfig(
            "DeepSeek loopback fixtures require plain HTTP with no credentials, query, or fragment"
                .to_string(),
        ));
    }
    let host = parsed.host_str().unwrap_or_default();
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !is_loopback {
        return Err(DeepSeekTransportError::InvalidConfig(
            "DeepSeek fixture endpoint must use localhost or a loopback IP address".to_string(),
        ));
    }
    let path = parsed.path().trim_end_matches('/');
    if !matches!(path, "" | "/v1") {
        return Err(DeepSeekTransportError::InvalidConfig(
            "DeepSeek fixture root path must be empty or /v1".to_string(),
        ));
    }
    Ok(raw.trim_end_matches('/').to_string())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportRetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_base: f64,
}

impl TransportRetryPolicy {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            max_retries: 0,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            exponential_base: 1.0,
        }
    }

    fn delay(self, retry_index: u32) -> Duration {
        let seconds = self.initial_delay.as_secs_f64()
            * self
                .exponential_base
                .powi(retry_index.min(i32::MAX as u32) as i32);
        Duration::from_secs_f64(seconds.min(self.max_delay.as_secs_f64()))
    }
}

#[derive(Debug, Clone)]
pub struct DeepSeekTransportConfig {
    pub endpoint: DeepSeekEndpoint,
    pub credential: DeepSeekCredential,
    pub strict_tools: bool,
    pub response_header_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub retry: TransportRetryPolicy,
    pub request_budget: SharedApiRequestBudget,
}

impl DeepSeekTransportConfig {
    pub fn validate(&self) -> Result<(), DeepSeekTransportError> {
        if self.response_header_timeout.is_zero() {
            return Err(DeepSeekTransportError::InvalidConfig(
                "DeepSeek response header timeout must be greater than zero".to_string(),
            ));
        }
        if self.stream_idle_timeout.is_zero() {
            return Err(DeepSeekTransportError::InvalidConfig(
                "DeepSeek stream idle timeout must be greater than zero".to_string(),
            ));
        }
        if self.retry.exponential_base < 1.0 || !self.retry.exponential_base.is_finite() {
            return Err(DeepSeekTransportError::InvalidConfig(
                "DeepSeek retry exponential base must be finite and at least 1".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DeepSeekTransport {
    client: reqwest::Client,
    config: DeepSeekTransportConfig,
}

impl fmt::Debug for DeepSeekTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeepSeekTransport")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl DeepSeekTransport {
    pub fn new(
        client: reqwest::Client,
        config: DeepSeekTransportConfig,
    ) -> Result<Self, DeepSeekTransportError> {
        config.validate()?;
        Ok(Self { client, config })
    }

    #[must_use]
    pub fn config(&self) -> &DeepSeekTransportConfig {
        &self.config
    }

    #[must_use]
    pub fn with_request_budget(mut self, request_budget: SharedApiRequestBudget) -> Self {
        self.config.request_budget = request_budget;
        self
    }

    #[must_use]
    pub fn with_retries_disabled(mut self) -> Self {
        self.config.retry = TransportRetryPolicy::disabled();
        self
    }

    pub async fn complete(
        &self,
        plan: RequestPlan,
    ) -> Result<DeepSeekResponse, DeepSeekTransportError> {
        self.validate_plan(&plan, ResponseMode::NonStreaming)?;
        let response = self.send(&plan).await?;
        let mut accounting =
            ApiResponseAccountingGuard::new(response.lease, plan.model.clone(), plan.surface);
        let bytes = response
            .response
            .bytes()
            .await
            .map_err(|error| DeepSeekTransportError::Network(format_error_chain(&error)))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
            DeepSeekTransportError::InvalidJson(format!("invalid DeepSeek response JSON: {error}"))
        })?;
        let mut parsed = parse_chat_response(&value)?;
        parsed.output.usage.reasoning_replay_tokens =
            u64::from(plan.reasoning_replay_tokens.unwrap_or(0));
        accounting.set_model(parsed.model.clone());
        let wire_usage = value.get("usage").filter(|usage| usage.is_object());
        accounting.complete(wire_usage.map(|_| &parsed.output.usage), wire_usage);
        Ok(parsed)
    }

    pub async fn stream(
        &self,
        plan: RequestPlan,
    ) -> Result<DeepSeekStream, DeepSeekTransportError> {
        self.validate_plan(&plan, ResponseMode::Streaming)?;
        let response = self.send(&plan).await?;
        let idle = self.config.stream_idle_timeout;
        let model = plan.model.clone();
        let surface = plan.surface;
        let replay_tokens = u64::from(plan.reasoning_replay_tokens.unwrap_or(0));
        let byte_stream = response.response.bytes_stream();
        let accounting = ApiResponseAccountingGuard::new(response.lease, model.clone(), surface);

        Ok(Box::pin(async_stream::stream! {
            let mut accounting = accounting;
            let mut parser = SseParser::new(model, replay_tokens);
            let mut bytes = std::pin::pin!(byte_stream);
            let mut buffer = Vec::new();
            let mut failed = false;

            loop {
                let next = match tokio::time::timeout(idle, bytes.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        failed = true;
                        yield Err(DeepSeekTransportError::StreamStall { timeout: idle });
                        break;
                    }
                };
                let Some(chunk) = next else { break };
                match chunk {
                    Ok(chunk) => buffer.extend_from_slice(&chunk),
                    Err(error) => {
                        failed = true;
                        yield Err(DeepSeekTransportError::Network(format_error_chain(&error)));
                        break;
                    }
                }
                if buffer.len() > MAX_SSE_BUFFER_BYTES {
                    failed = true;
                    yield Err(DeepSeekTransportError::StreamOverflow {
                        limit: MAX_SSE_BUFFER_BYTES,
                    });
                    break;
                }

                while let Some(frame) = take_sse_frame(&mut buffer) {
                    match parser.push_frame(&frame) {
                        Ok(events) => {
                            for event in events {
                                yield Ok(event);
                            }
                        }
                        Err(error) => {
                            failed = true;
                            yield Err(error);
                            break;
                        }
                    }
                }
                if failed || parser.saw_done {
                    break;
                }
            }

            if !failed && !parser.saw_done && !buffer.is_empty() {
                match parse_trailing_sse_frame(&buffer) {
                    Ok(Some(frame)) => match parser.push_frame(&frame) {
                        Ok(events) => {
                            for event in events {
                                yield Ok(event);
                            }
                        }
                        Err(error) => {
                            failed = true;
                            yield Err(error);
                        }
                    },
                    Ok(None) => {}
                    Err(error) => {
                        failed = true;
                        yield Err(error);
                    }
                }
            }

            if failed {
                accounting.incomplete();
                return;
            }
            match parser.finish() {
                Ok(response) => {
                    accounting.set_model(response.model.clone());
                    let usage_value = parser.wire_usage.as_ref();
                    accounting.complete(usage_value.map(|_| &response.output.usage), usage_value);
                    yield Ok(ModelStreamEvent::Completed { output: response.output });
                }
                Err(error) => {
                    accounting.incomplete();
                    yield Err(error);
                }
            }
        }))
    }

    fn validate_plan(
        &self,
        plan: &RequestPlan,
        expected_mode: ResponseMode,
    ) -> Result<(), DeepSeekTransportError> {
        if plan.response_mode != expected_mode {
            return Err(DeepSeekTransportError::InvalidPlan(format!(
                "DeepSeek transport expected {expected_mode:?}, received {:?}",
                plan.response_mode
            )));
        }
        if !self.config.endpoint.owns_url(&plan.url) {
            return Err(DeepSeekTransportError::InvalidPlan(format!(
                "DeepSeek request URL is outside the configured official/fixture endpoint: {}",
                plan.url
            )));
        }
        let expected_url = self.config.endpoint.url_for(plan.surface);
        if plan.url != expected_url {
            return Err(DeepSeekTransportError::InvalidPlan(format!(
                "DeepSeek {:?} plan URL must be {expected_url}, received {}",
                plan.surface, plan.url
            )));
        }
        if plan.surface == ApiSurface::StrictChat && !self.config.strict_tools {
            return Err(DeepSeekTransportError::InvalidPlan(
                "DeepSeek Strict Chat plan was supplied while strict tools are disabled"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn send(&self, plan: &RequestPlan) -> Result<SentResponse, DeepSeekTransportError> {
        let mut retry_index = 0;
        loop {
            let mut lease = self
                .config
                .request_budget
                .try_reserve()
                .map_err(DeepSeekTransportError::RequestBudget)?;
            if retry_index > 0 {
                lease.mark_retry_attempt();
            }
            let request = self
                .client
                .post(&plan.url)
                .bearer_auth(self.config.credential.expose())
                .json(&plan.body);
            let response =
                match tokio::time::timeout(self.config.response_header_timeout, request.send())
                    .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => {
                        let error = DeepSeekTransportError::Network(format_error_chain(&error));
                        if self.should_retry(&error, retry_index) {
                            tokio::time::sleep(self.config.retry.delay(retry_index)).await;
                            retry_index += 1;
                            continue;
                        }
                        return Err(error);
                    }
                    Err(_) => {
                        let error = DeepSeekTransportError::ResponseHeaderTimeout {
                            timeout: self.config.response_header_timeout,
                        };
                        if self.should_retry(&error, retry_index) {
                            tokio::time::sleep(self.config.retry.delay(retry_index)).await;
                            retry_index += 1;
                            continue;
                        }
                        return Err(error);
                    }
                };
            lease.mark_response_received();
            let status = response.status();
            if status.is_success() {
                lease.mark_usage_expected();
                return Ok(SentResponse {
                    response,
                    lease: Some(lease),
                });
            }
            if status.is_server_error() {
                lease.mark_billing_unknown();
            }
            let retry_after = retry_after(response.headers());
            let body = bounded_body(response).await;
            let error = DeepSeekTransportError::Http {
                status: status.as_u16(),
                message: sanitize_error_body(&body),
                retry_after,
            };
            if self.should_retry(&error, retry_index) {
                let delay = retry_after.unwrap_or_else(|| self.config.retry.delay(retry_index));
                tokio::time::sleep(delay.min(self.config.retry.max_delay)).await;
                retry_index += 1;
                continue;
            }
            return Err(error);
        }
    }

    fn should_retry(&self, error: &DeepSeekTransportError, retry_index: u32) -> bool {
        retry_index < self.config.retry.max_retries && error.retryable()
    }
}

struct SentResponse {
    response: reqwest::Response,
    lease: Option<ApiRequestLease>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeepSeekResponse {
    pub id: String,
    pub model: String,
    pub output: ModelOutput,
}

#[derive(Debug, thiserror::Error)]
pub enum DeepSeekTransportError {
    #[error("invalid DeepSeek transport configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid DeepSeek request plan: {0}")]
    InvalidPlan(String),
    #[error("DeepSeek request budget rejected the physical request: {0}")]
    RequestBudget(#[source] ApiRequestBudgetError),
    #[error("DeepSeek response headers timed out after {timeout:?}")]
    ResponseHeaderTimeout { timeout: Duration },
    #[error("DeepSeek network transport failed: {0}")]
    Network(String),
    #[error("DeepSeek HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        retry_after: Option<Duration>,
    },
    #[error("{0}")]
    InvalidJson(String),
    #[error("DeepSeek SSE provider error: {0}")]
    SseProvider(String),
    #[error("DeepSeek SSE stream stalled for {timeout:?}")]
    StreamStall { timeout: Duration },
    #[error("DeepSeek SSE buffer exceeded {limit} bytes")]
    StreamOverflow { limit: usize },
    #[error("DeepSeek SSE stream ended before a trusted finish_reason")]
    StreamIncomplete,
    #[error("unsupported DeepSeek finish_reason: {0}")]
    UnsupportedFinishReason(String),
    #[error("DeepSeek response is missing {0}")]
    MissingField(&'static str),
}

impl DeepSeekTransportError {
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::ResponseHeaderTimeout { .. }
            | Self::Network(_)
            | Self::StreamStall { .. }
            | Self::StreamIncomplete => true,
            Self::Http { status, .. } => *status == 408 || *status == 429 || *status >= 500,
            Self::InvalidConfig(_)
            | Self::InvalidPlan(_)
            | Self::RequestBudget(_)
            | Self::InvalidJson(_)
            | Self::SseProvider(_)
            | Self::StreamOverflow { .. }
            | Self::UnsupportedFinishReason(_)
            | Self::MissingField(_) => false,
        }
    }
}

pub fn parse_chat_response(value: &Value) -> Result<DeepSeekResponse, DeepSeekTransportError> {
    if let Some(error) = value.get("error") {
        return Err(DeepSeekTransportError::SseProvider(provider_error(error)));
    }
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or(DeepSeekTransportError::MissingField("choices[0]"))?;
    let message = choice
        .get("message")
        .ok_or(DeepSeekTransportError::MissingField("choices[0].message"))?;
    let finish_reason = choice.get("finish_reason").and_then(Value::as_str).ok_or(
        DeepSeekTransportError::MissingField("choices[0].finish_reason"),
    )?;
    let tool_calls = parse_tool_calls(message.get("tool_calls"))?;
    Ok(DeepSeekResponse {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        output: ModelOutput {
            content: message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            reasoning_content: message
                .get("reasoning_content")
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_calls,
            finish_reason: parse_finish_reason(finish_reason)?,
            usage: parse_usage(value.get("usage")),
        },
    })
}

fn parse_tool_calls(value: Option<&Value>) -> Result<Vec<ModelToolCall>, DeepSeekTransportError> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, call)| {
            let function = call
                .get("function")
                .ok_or(DeepSeekTransportError::MissingField(
                    "tool_calls[].function",
                ))?;
            let raw = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(ModelToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{index}")),
                name: decode_tool_name(
                    function
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown_tool"),
                ),
                arguments: ToolArguments::parse(raw),
            })
        })
        .collect()
}

fn parse_finish_reason(value: &str) -> Result<ModelFinishReason, DeepSeekTransportError> {
    match value {
        "stop" => Ok(ModelFinishReason::Stop),
        "tool_calls" => Ok(ModelFinishReason::ToolCalls),
        "length" => Ok(ModelFinishReason::Length),
        "content_filter" => Ok(ModelFinishReason::ContentFilter),
        "insufficient_system_resource" => Ok(ModelFinishReason::InsufficientSystemResource),
        other => Err(DeepSeekTransportError::UnsupportedFinishReason(
            other.to_string(),
        )),
    }
}

fn parse_usage(value: Option<&Value>) -> Usage {
    let input_tokens = value
        .and_then(|usage| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut output_tokens = value
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning_tokens = value
        .and_then(|usage| usage.get("completion_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if output_tokens == 0 {
        output_tokens = if reasoning_tokens > 0 {
            reasoning_tokens
        } else {
            value
                .and_then(|usage| usage.get("total_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(input_tokens)
                .saturating_sub(input_tokens)
        };
    }
    let cache_hit_tokens = value
        .and_then(|usage| usage.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .and_then(|usage| usage.get("prompt_tokens_details"))
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    let cache_miss_tokens = value
        .and_then(|usage| usage.get("prompt_cache_miss_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input_tokens.saturating_sub(cache_hit_tokens));
    Usage {
        input_tokens,
        output_tokens,
        cache_hit_tokens,
        cache_miss_tokens,
        cache_write_tokens: 0,
        reasoning_tokens,
        reasoning_replay_tokens: 0,
    }
}

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

struct SseParser {
    id: String,
    model: String,
    content: String,
    reasoning_content: String,
    tools: BTreeMap<u32, PartialToolCall>,
    finish_reason: Option<ModelFinishReason>,
    usage: Usage,
    wire_usage: Option<Value>,
    replay_tokens: u64,
    saw_done: bool,
}

impl SseParser {
    fn new(model: String, replay_tokens: u64) -> Self {
        Self {
            id: String::new(),
            model,
            content: String::new(),
            reasoning_content: String::new(),
            tools: BTreeMap::new(),
            finish_reason: None,
            usage: Usage::default(),
            wire_usage: None,
            replay_tokens,
            saw_done: false,
        }
    }

    fn push_frame(&mut self, data: &str) -> Result<Vec<ModelStreamEvent>, DeepSeekTransportError> {
        if data.trim() == "[DONE]" {
            self.saw_done = true;
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(data).map_err(|error| {
            DeepSeekTransportError::InvalidJson(format!(
                "invalid DeepSeek SSE JSON data frame: {error}"
            ))
        })?;
        if let Some(error) = value.get("error") {
            return Err(DeepSeekTransportError::SseProvider(provider_error(error)));
        }
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            self.id = id.to_string();
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        if let Some(usage) = value.get("usage").filter(|usage| usage.is_object()) {
            self.usage = parse_usage(Some(usage));
            self.wire_usage = Some(usage.clone());
        }
        let mut events = Vec::new();
        for choice in value
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(delta) = choice.get("delta") {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str)
                    && !reasoning.is_empty()
                {
                    self.reasoning_content.push_str(reasoning);
                    events.push(ModelStreamEvent::ReasoningDelta {
                        delta: reasoning.to_string(),
                    });
                }
                if let Some(content) = delta.get("content").and_then(Value::as_str)
                    && !content.is_empty()
                {
                    self.content.push_str(content);
                    events.push(ModelStreamEvent::ContentDelta {
                        delta: content.to_string(),
                    });
                }
                for call in delta
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let partial = self.tools.entry(index).or_default();
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        partial.id = Some(id.to_string());
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            partial.name = Some(name.to_string());
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            partial.arguments.push_str(arguments);
                        }
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(parse_finish_reason(reason)?);
            }
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<DeepSeekResponse, DeepSeekTransportError> {
        let finish_reason = self
            .finish_reason
            .ok_or(DeepSeekTransportError::StreamIncomplete)?;
        let tool_calls = std::mem::take(&mut self.tools)
            .into_iter()
            .enumerate()
            .map(|(fallback_index, (_, call))| ModelToolCall {
                id: call.id.unwrap_or_else(|| format!("call_{fallback_index}")),
                name: decode_tool_name(call.name.as_deref().unwrap_or("unknown_tool")),
                arguments: ToolArguments::parse(call.arguments),
            })
            .collect();
        self.usage.reasoning_replay_tokens = self.replay_tokens;
        Ok(DeepSeekResponse {
            id: std::mem::take(&mut self.id),
            model: std::mem::take(&mut self.model),
            output: ModelOutput {
                content: std::mem::take(&mut self.content),
                reasoning_content: (!self.reasoning_content.is_empty())
                    .then(|| std::mem::take(&mut self.reasoning_content)),
                tool_calls,
                finish_reason,
                usage: self.usage,
            },
        })
    }
}

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<String> {
    let boundary = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| (index, 4))
        })?;
    let raw = buffer.drain(..boundary.0 + boundary.1).collect::<Vec<_>>();
    let event = String::from_utf8_lossy(&raw[..boundary.0]);
    sse_data(&event)
}

fn parse_trailing_sse_frame(buffer: &[u8]) -> Result<Option<String>, DeepSeekTransportError> {
    let event = std::str::from_utf8(buffer).map_err(|error| {
        DeepSeekTransportError::InvalidJson(format!("invalid UTF-8 in DeepSeek SSE frame: {error}"))
    })?;
    Ok(sse_data(event))
}

fn sse_data(event: &str) -> Option<String> {
    let data = event
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data:")
                .map(|value| value.strip_prefix(' ').unwrap_or(value))
        })
        .collect::<Vec<_>>();
    (!data.is_empty()).then(|| data.join("\n"))
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn bounded_body(response: reqwest::Response) -> String {
    let bytes = response.bytes().await.unwrap_or_default();
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_ERROR_BODY_BYTES)]).into_owned()
}

fn sanitize_error_body(body: &str) -> String {
    let mut value = body.replace(['\r', '\n'], " ");
    for marker in ["sk-", "Bearer "] {
        while let Some(start) = value.find(marker) {
            let end = value[start..]
                .find(char::is_whitespace)
                .map_or(value.len(), |offset| start + offset);
            value.replace_range(start..end, "[REDACTED]");
        }
    }
    value.chars().take(2_000).collect()
}

fn provider_error(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(sanitize_error_body)
        .unwrap_or_else(|| sanitize_error_body(&error.to_string()))
}

fn format_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        rendered.push_str(" -> ");
        rendered.push_str(&next.to_string());
        source = next.source();
    }
    rendered
}

#[must_use]
pub fn encode_tool_name(name: &str) -> String {
    let mut encoded = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            encoded.push(character);
        } else if character == '-' {
            encoded.push_str("--");
        } else {
            encoded.push_str(&format!("-x{:06X}-", character as u32));
        }
    }
    encoded
}

#[must_use]
pub fn decode_tool_name(name: &str) -> String {
    let chars = name.chars().collect::<Vec<_>>();
    let mut decoded = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '-' && chars.get(index + 1) == Some(&'-') {
            decoded.push('-');
            index += 2;
            continue;
        }
        let delimited = chars[index] == '-'
            && chars.get(index + 1) == Some(&'x')
            && chars.get(index + 8) == Some(&'-');
        let bare = chars[index] == 'x';
        let hex_start = if delimited {
            index + 2
        } else if bare {
            index + 1
        } else {
            decoded.push(chars[index]);
            index += 1;
            continue;
        };
        if hex_start + 6 <= chars.len() {
            let hex = chars[hex_start..hex_start + 6].iter().collect::<String>();
            if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
                && let Some(character) = char::from_u32(codepoint)
                && !character.is_ascii_alphanumeric()
                && !matches!(character, '_' | '-')
            {
                decoded.push(character);
                index = hex_start
                    + 6
                    + usize::from(delimited || chars.get(hex_start + 6) == Some(&'-'));
                continue;
            }
        }
        decoded.push(chars[index]);
        index += 1;
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_rejects_non_loopback_fixture() {
        assert!(DeepSeekEndpoint::loopback_fixture("http://example.com/v1").is_err());
        assert!(DeepSeekEndpoint::loopback_fixture("https://127.0.0.1/v1").is_err());
        assert!(DeepSeekEndpoint::loopback_fixture("http://127.0.0.1/custom").is_err());
        assert_eq!(
            DeepSeekEndpoint::loopback_fixture("http://127.0.0.1:9000/v1")
                .expect("valid fixture")
                .root(),
            "http://127.0.0.1:9000/v1"
        );
    }

    #[test]
    fn parser_preserves_reasoning_raw_arguments_usage_and_finish_reason() {
        let response = parse_chat_response(&json!({
            "id": "chat-1",
            "model": "deepseek-v4-pro",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "reasoning_content": "exact reasoning",
                    "tool_calls": [{
                        "id": "call-1",
                        "function": {
                            "name": "web-x00002E-run",
                            "arguments": "{\"q\":\"中文\"}"
                        }
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_cache_hit_tokens": 70,
                "prompt_cache_miss_tokens": 30,
                "completion_tokens_details": {"reasoning_tokens": 8}
            }
        }))
        .expect("response parses");
        assert_eq!(
            response.output.reasoning_content.as_deref(),
            Some("exact reasoning")
        );
        assert_eq!(response.output.finish_reason, ModelFinishReason::ToolCalls);
        assert_eq!(response.output.tool_calls[0].name, "web.run");
        assert_eq!(
            response.output.tool_calls[0].arguments.raw,
            "{\"q\":\"中文\"}"
        );
        assert_eq!(response.output.usage.cache_hit_tokens, 70);
        assert_eq!(response.output.usage.reasoning_tokens, 8);
    }

    #[test]
    fn sse_parser_reassembles_raw_tool_arguments_and_trailing_usage() {
        let mut parser = SseParser::new("deepseek-v4-pro".to_string(), 9);
        let first = parser
            .push_frame(r#"{"id":"chat-1","choices":[{"delta":{"reasoning_content":"想","tool_calls":[{"index":0,"id":"call-1","function":{"name":"read-x00002E-file","arguments":"{\"path\":"}}]}}]}"#)
            .expect("first frame");
        assert_eq!(
            first,
            vec![ModelStreamEvent::ReasoningDelta {
                delta: "想".into()
            }]
        );
        parser
            .push_frame(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.rs\"}"}}]},"finish_reason":"tool_calls"}]}"#)
            .expect("terminal frame");
        parser
            .push_frame(r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":4,"prompt_cache_hit_tokens":2,"prompt_cache_miss_tokens":10}}"#)
            .expect("usage frame");
        parser.push_frame("[DONE]").expect("done frame");
        let response = parser.finish().expect("complete response");
        assert_eq!(response.output.tool_calls[0].name, "read.file");
        assert_eq!(
            response.output.tool_calls[0].arguments.raw,
            "{\"path\":\"a.rs\"}"
        );
        assert_eq!(response.output.usage.reasoning_replay_tokens, 9);
    }

    #[test]
    fn sse_error_and_unknown_finish_reason_are_typed() {
        let mut parser = SseParser::new("deepseek-v4-pro".to_string(), 0);
        assert!(matches!(
            parser.push_frame(r#"{"error":{"message":"upstream unavailable"}}"#),
            Err(DeepSeekTransportError::SseProvider(_))
        ));
        assert!(matches!(
            parse_finish_reason("future_reason"),
            Err(DeepSeekTransportError::UnsupportedFinishReason(_))
        ));
    }

    #[test]
    fn tool_name_projection_round_trips_unicode_and_punctuation() {
        for name in ["web.run", "apply-patch", "工具/读取", "snake_case"] {
            assert_eq!(decode_tool_name(&encode_tool_name(name)), name);
        }
        assert_eq!(decode_tool_name("webx00002Erun"), "web.run");
    }
}
