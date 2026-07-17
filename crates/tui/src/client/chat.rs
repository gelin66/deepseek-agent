//! Chat Completions API helpers for DeepSeek's OpenAI-compatible endpoint.
//!
//! This is the production code path. Streaming (`create_message_stream`),
//! request building (`build_chat_messages*`), and SSE parsing
//! (`parse_sse_chunk_with_reasoning_style`) all live here.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::time::timeout as tokio_timeout;

use crate::config::wire_model_for_provider;

/// Default timeout for the initial streaming response headers.
///
/// `doctor` uses a bounded non-streaming request, but normal TUI turns first
/// wait for the SSE response to open. On some Windows/proxy paths that wait can
/// hang before any stream chunk exists, leaving the UI stuck at "Working...".
const DEFAULT_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(45);

/// Reads `DEEPSEEK_STREAM_OPEN_TIMEOUT_SECS` as a bounded override for the
/// response-header wait. This is intentionally shorter than the per-chunk idle
/// timeout because it only covers connection setup and upstream header return,
/// not model thinking time after streaming has started.
pub(super) fn stream_open_timeout() -> Duration {
    stream_open_timeout_from_env(
        std::env::var("DEEPSEEK_STREAM_OPEN_TIMEOUT_SECS")
            .ok()
            .as_deref(),
    )
}

fn stream_open_timeout_from_env(value: Option<&str>) -> Duration {
    let secs = value
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_STREAM_OPEN_TIMEOUT.as_secs())
        .clamp(5, 300);
    Duration::from_secs(secs)
}

fn stream_idle_error(idle: Duration) -> anyhow::Error {
    anyhow::Error::new(StreamError::Stall {
        timeout_secs: idle.as_secs(),
    })
}

fn stream_read_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut chain = error.to_string();
    let mut current = error.source();
    while let Some(source) = current {
        chain.push_str(&format!(" -> {source}"));
        current = source.source();
    }
    chain
}

fn stream_read_error(error: &reqwest::Error) -> anyhow::Error {
    anyhow::Error::new(LlmError::NetworkError(format!(
        "Stream read error: {}",
        stream_read_error_chain(error)
    )))
}

fn response_header_timeout_error(timeout: Duration) -> anyhow::Error {
    anyhow::Error::new(LlmError::Timeout(timeout))
}

fn response_body_read_error(error: &reqwest::Error) -> anyhow::Error {
    anyhow::Error::new(LlmError::NetworkError(format!(
        "Failed to read Chat API response body: {}",
        stream_read_error_chain(error)
    )))
}

use crate::config::ApiProvider;
use crate::error_taxonomy::StreamError;
use crate::llm_client::sanitize_http_error_body;
use crate::llm_client::{LlmError, StreamEventBox};
use crate::logging;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageDelta, MessageRequest, MessageResponse,
    StreamEvent, SystemPrompt, Tool, ToolCaller, Usage, is_openai_gpt_56_api_model,
    model_is_openai_reasoning_family, model_supports_reasoning,
};

use super::deepseek::ApiSurface;
use super::deepseek::{self, RequestPlan, ResponseMode};
use super::{
    DeepSeekClient, ERROR_BODY_MAX_BYTES, SSE_BACKPRESSURE_HIGH_WATERMARK,
    SSE_BACKPRESSURE_SLEEP_MS, SSE_MAX_LINES_PER_CHUNK, acquire_stream_buffer, api_url_with_suffix,
    apply_reasoning_effort, bounded_error_text, deepseek_accounting_usage, from_api_tool_name,
    parse_usage, release_stream_buffer, system_to_instructions, to_api_tool_name,
};
use codewhale_deepseek::ApiResponseAccountingGuard;

fn apply_provider_token_limit(
    body: &mut Value,
    provider: ApiProvider,
    model: &str,
    max_tokens: u32,
) {
    let use_max_completion_tokens = provider == ApiProvider::XiaomiMimo
        || (provider == ApiProvider::Openai && model_is_openai_reasoning_family(model));
    if !use_max_completion_tokens {
        return;
    }

    if let Some(object) = body.as_object_mut() {
        object.remove("max_tokens");
    }
    body["max_completion_tokens"] = json!(max_tokens);
}

fn apply_openai_reasoning_effort(
    body: &mut Value,
    provider: ApiProvider,
    model: &str,
    effort: Option<&str>,
) {
    let model_lower = model.trim().to_ascii_lowercase();
    let is_gpt_56 =
        provider == ApiProvider::Openai && is_openai_gpt_56_api_model(model_lower.as_str());
    let is_openai_reasoning =
        provider == ApiProvider::Openai && model_is_openai_reasoning_family(model);
    let is_muse_spark = provider == ApiProvider::Meta && model_lower == "muse-spark-1.1";
    if !is_openai_reasoning && !is_muse_spark {
        return;
    }
    let Some(effort) =
        effort.and_then(|value| openai_compatible_reasoning_effort(value, is_gpt_56, !is_gpt_56))
    else {
        return;
    };
    body["reasoning_effort"] = json!(effort);
}

fn openai_compatible_reasoning_effort(
    effort: &str,
    supports_max: bool,
    supports_minimal: bool,
) -> Option<&'static str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" | "false" => Some("none"),
        "minimal" if supports_minimal => Some("minimal"),
        "minimal" => Some("low"),
        "low" => Some("low"),
        "medium" | "mid" | "" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" | "highest" | "ultracode" if supports_max => Some("max"),
        "max" | "highest" | "ultracode" => Some("xhigh"),
        _ => None,
    }
}

fn mirror_minimax_reasoning_details_for_messages(messages: &mut [Value]) {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if message.get("reasoning_details").is_some() {
            continue;
        }
        let Some(reasoning) = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .filter(|reasoning| !reasoning.trim().is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        message["reasoning_details"] = json!([
            {
                "type": "text",
                "text": reasoning,
            }
        ]);
    }
}

fn mirror_minimax_reasoning_details_for_body(body: &mut Value, provider: ApiProvider) {
    if provider != ApiProvider::Minimax {
        return;
    }
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    mirror_minimax_reasoning_details_for_messages(messages);
}

fn official_deepseek_request_plan(
    client: &DeepSeekClient,
    request: &MessageRequest,
    response_mode: ResponseMode,
) -> Result<Option<RequestPlan>> {
    let plan = deepseek::plan_chat(
        client.api_provider,
        &client.base_url,
        client.path_suffix.as_deref(),
        client.strict_tool_mode,
        request,
        response_mode,
    )?;
    Ok(plan)
}

fn legacy_chat_request(
    client: &DeepSeekClient,
    request: &MessageRequest,
    response_mode: ResponseMode,
) -> (Value, String, String, Option<u32>) {
    let messages = build_chat_messages_for_request_and_provider(request, client.api_provider);
    let model = wire_model_for_provider(client.api_provider, &request.model);
    let streaming = response_mode == ResponseMode::Streaming;
    let mut body = json!({
        "model": model.clone(),
        "messages": messages,
        "max_tokens": request.max_tokens,
    });
    if streaming {
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});
    }
    apply_provider_token_limit(&mut body, client.api_provider, &model, request.max_tokens);
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = request.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(tools) = request.tools.as_ref() {
        let mut chat_tools: Vec<_> = tools
            .iter()
            .map(|tool| {
                tool_to_chat_for_legacy_route(tool, &client.base_url, client.path_suffix.as_deref())
            })
            .collect();
        if client.api_provider == ApiProvider::Moonshot {
            for tool in &mut chat_tools {
                if let Some(parameters) = tool
                    .as_object_mut()
                    .and_then(|tool| tool.get_mut("function"))
                    .and_then(|function| function.get_mut("parameters"))
                {
                    crate::tools::schema_sanitize::sanitize_for_kimi_parameters(parameters);
                }
            }
        }
        body["tools"] = json!(chat_tools);
    }
    if should_send_tool_choice_for_chat(client.api_provider, request.reasoning_effort.as_deref())
        && let Some(choice) = request.tool_choice.as_ref()
        && let Some(mapped) = map_tool_choice_for_chat(choice)
    {
        body["tool_choice"] = mapped;
    }
    apply_reasoning_effort(
        &mut body,
        request.reasoning_effort.as_deref(),
        client.api_provider,
    );
    apply_openai_reasoning_effort(
        &mut body,
        client.api_provider,
        &model,
        request.reasoning_effort.as_deref(),
    );
    let reasoning_replay_tokens = streaming
        .then(|| {
            reasoning_replay_tokens_for_messages(
                &body,
                &model,
                request.reasoning_effort.as_deref(),
                client.api_provider,
            )
        })
        .flatten();
    mirror_minimax_reasoning_details_for_body(&mut body, client.api_provider);
    let url = api_url_with_suffix(
        &client.base_url,
        "chat/completions",
        client.path_suffix.as_deref(),
    );
    (body, url, model, reasoning_replay_tokens)
}

impl DeepSeekClient {
    pub(super) async fn create_message_chat(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse> {
        let cacheable = crate::llm_response_cache::request_is_cacheable(request);
        let (plan, official_plan) = if let Some(plan) =
            official_deepseek_request_plan(self, request, ResponseMode::NonStreaming)?
        {
            debug_assert_eq!(plan.response_mode, ResponseMode::NonStreaming);
            (plan, true)
        } else {
            let (body, url, model, replay) =
                legacy_chat_request(self, request, ResponseMode::NonStreaming);
            (
                RequestPlan {
                    surface: ApiSurface::StandardChat,
                    url,
                    model,
                    body,
                    response_mode: ResponseMode::NonStreaming,
                    reasoning_replay_tokens: replay,
                },
                false,
            )
        };

        let response_cache_key = if cacheable {
            let wire_body =
                serde_json::to_vec(&plan.body).context("Failed to serialize Chat API cache key")?;
            let (cache_base_url, cache_path_suffix) = if official_plan {
                (plan.url.as_str(), None)
            } else {
                (self.base_url.as_str(), self.path_suffix.as_deref())
            };
            let key = crate::llm_response_cache::ResponseCache::make_key(
                self.api_provider.as_str(),
                cache_base_url,
                cache_path_suffix,
                &self.api_key,
                &wire_body,
            );
            if let Some(cached) = crate::llm_response_cache::response_cache().get(&key) {
                return Ok(cached);
            }
            Some(key)
        } else {
            None
        };

        if official_plan {
            let response = self
                .official_deepseek_transport()?
                .complete(plan)
                .await
                .map_err(anyhow::Error::new)?;
            let parsed = deepseek::message_response_from_deepseek(response);
            if let Some(key) = response_cache_key {
                crate::llm_response_cache::response_cache().put(key, parsed.clone());
            }
            return Ok(parsed);
        }
        self.send_legacy_chat_message(plan, response_cache_key)
            .await
    }

    /// Compatibility sender for non-DeepSeek providers still using Chat
    /// Completions. Official DeepSeek is owned by `codewhale-deepseek`.
    async fn send_legacy_chat_message(
        &self,
        plan: RequestPlan,
        response_cache_key: Option<[u8; 32]>,
    ) -> Result<MessageResponse> {
        let RequestPlan {
            surface,
            url,
            model,
            body,
            reasoning_replay_tokens,
            ..
        } = plan;

        let open_timeout = stream_open_timeout();
        let (response, request_lease) = match tokio_timeout(
            open_timeout,
            self.send_json_with_retry(&url, &body),
        )
        .await
        {
            Ok(result) => result?,
            Err(_elapsed) => {
                logging::warn(format!(
                    "SSE stream request did not receive response headers after {}s. \
                     `codewhale doctor` can still pass when non-streaming requests work; \
                     on Windows or proxy networks, try `DEEPSEEK_FORCE_HTTP1=1` and rerun `codewhale`.",
                    open_timeout.as_secs()
                ));
                return Err(response_header_timeout_error(open_timeout));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let raw_error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let error_text = sanitize_http_error_body(
                Some(self.api_provider.display_name()),
                status.as_u16(),
                &raw_error_text,
            );
            anyhow::bail!("Failed to call DeepSeek Chat API: HTTP {status}: {error_text}");
        }

        let mut response_accounting =
            ApiResponseAccountingGuard::new(request_lease, model, surface);
        let response_text = response
            .text()
            .await
            .map_err(|error| response_body_read_error(&error))?;
        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Chat API JSON")?;
        let observed_usage = value.get("usage").filter(|usage| usage.is_object());
        if let Some(wire_usage) = observed_usage {
            response_accounting.observe(
                &deepseek_accounting_usage(&parse_usage(Some(wire_usage))),
                Some(wire_usage),
            );
        }
        let mut parsed = parse_chat_message(&value)?;
        if let Some(tokens) = reasoning_replay_tokens {
            parsed.usage.reasoning_replay_tokens = Some(tokens);
        }
        response_accounting.set_model(parsed.model.clone());
        let accounting_usage = observed_usage.map(|_| deepseek_accounting_usage(&parsed.usage));
        response_accounting.complete(accounting_usage.as_ref(), observed_usage);
        if let Some(key) = response_cache_key {
            crate::llm_response_cache::response_cache().put(key, parsed.clone());
        }
        Ok(parsed)
    }
}

impl DeepSeekClient {
    pub(super) async fn handle_chat_completion_stream(
        &self,
        request: MessageRequest,
    ) -> Result<StreamEventBox> {
        if let Some(plan) = official_deepseek_request_plan(self, &request, ResponseMode::Streaming)?
        {
            debug_assert_eq!(plan.response_mode, ResponseMode::Streaming);
            let model = plan.model.clone();
            let source = self
                .official_deepseek_transport()?
                .stream(plan)
                .await
                .map_err(anyhow::Error::new)?;
            return Ok(deepseek::tui_stream_from_deepseek(source, model));
        }
        let (body, url, model, replay) =
            legacy_chat_request(self, &request, ResponseMode::Streaming);
        self.handle_legacy_chat_completion_stream(RequestPlan {
            surface: ApiSurface::StandardChat,
            url,
            model,
            body,
            response_mode: ResponseMode::Streaming,
            reasoning_replay_tokens: replay,
        })
        .await
    }

    /// Compatibility SSE sender/parser for non-DeepSeek providers. Official
    /// DeepSeek streaming is owned by `codewhale-deepseek`.
    async fn handle_legacy_chat_completion_stream(
        &self,
        plan: RequestPlan,
    ) -> Result<StreamEventBox> {
        let RequestPlan {
            surface,
            url,
            model,
            body,
            reasoning_replay_tokens: replay_input_tokens,
            ..
        } = plan;
        let (response, request_lease) = self.send_json_with_retry(&url, &body).await?;

        let status = response.status();
        if !status.is_success() {
            let raw_error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let error_text = sanitize_http_error_body(
                Some(self.api_provider.display_name()),
                status.as_u16(),
                &raw_error_text,
            );
            // If DeepSeek rejects `reasoning_content`, dump the outgoing
            // indices so an unowned legacy route or provider protocol change
            // remains diagnosable.
            if error_text.contains("reasoning_content") {
                log_thinking_mode_violations(&body);
            }
            anyhow::bail!("SSE stream request failed: HTTP {status}: {error_text}");
        }

        let api_provider = self.api_provider;

        // Capture transport-shape headers before we consume `response` into
        // `bytes_stream()`. They are surfaced in the decode-error log path so
        // we can tell HTTP/2 RST_STREAM from chunked-encoding corruption from
        // gzip-compressor failure when investigating #103.
        let response_headers = format_stream_headers(response.headers());
        let byte_stream = response.bytes_stream();
        let stream_idle_timeout = self.stream_idle_timeout;
        let configured_reasoning_stream_style = self.reasoning_stream_style.clone();
        let response_accounting =
            ApiResponseAccountingGuard::new(request_lease, model.clone(), surface);

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            let mut response_accounting = response_accounting;

            // Emit a synthetic MessageStart
            yield Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: String::new(),
                    r#type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: model.clone(),
                    stop_reason: None,
                    stop_sequence: None,
                    container: None,
                    usage: Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                        ..Usage::default()
                    },
                },
            });

            let mut line_buf = String::new();
            let mut byte_buf = acquire_stream_buffer();
            let mut content_index: u32 = 0;
            let mut text_started = false;
            let mut thinking_started = false;
            let mut tool_indices: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
            let mut reasoning_detail_buffers: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
            let mut inline_reasoning_tags = InlineReasoningTagState::default();
            let reasoning_stream_style = reasoning_stream_style_for_stream(
                api_provider,
                &model,
                configured_reasoning_stream_style.as_deref(),
            );

            let mut byte_stream = std::pin::pin!(byte_stream);
            let idle = stream_idle_timeout;

            // Telemetry for #103 stream-decode diagnostics: bytes received
            // since the start of this stream and last successful event time.
            // Surfaces in the error log when reqwest yields a chunk error so
            // we can tell HTTP/2 RST_STREAM from chunk-decode-failure from
            // gzip-corruption when investigating a flaky session.
            let stream_start = std::time::Instant::now();
            let mut last_event_at = std::time::Instant::now();
            let mut bytes_received: usize = 0;
            // Set when a `[DONE]` sentinel was seen, so the post-loop flush does
            // not re-process trailing post-DONE bytes.
            let mut saw_done = false;
            // A clean HTTP EOF is not a model terminal. DeepSeek may terminate
            // with `[DONE]` or a choice `finish_reason`; without either, any
            // partial content is untrusted and the turn must fail closed.
            let mut saw_terminal_signal = false;
            let mut stream_failed = false;
            let mut observed_usage: Option<Usage> = None;
            let mut observed_wire_usage: Option<Value> = None;
            // Do not expose a model terminal to the consumer until the HTTP
            // response has itself reached a trusted terminal and accounting
            // has been settled. Engine callers legitimately stop polling as
            // soon as they see `stop_reason`; yielding it here used to drop
            // the guard before a trailing usage frame / `[DONE]` was consumed,
            // producing a false incomplete response on the final agent turn.
            let mut pending_terminal_events: Vec<StreamEvent> = Vec::new();

            'stream: loop {
                let chunk_result = match tokio_timeout(idle, byte_stream.next()).await {
                    Ok(Some(result)) => result,
                    Ok(None) => break, // Stream ended normally
                    Err(_elapsed) => {
                        // Keep the transport-layer idle guard typed. The
                        // Engine has a second, event-progress guard with the
                        // same budget; whichever timer wins must converge on
                        // the same `stream_stall` terminal cause.
                        yield Err(stream_idle_error(idle));
                        stream_failed = true;
                        break;
                    }
                };
                let chunk = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        // Walk the error source chain so reqwest's underlying
                        // hyper / h2 / io error is visible — without this the
                        // outer "error decoding response body" message tells
                        // us nothing about WHY the stream died.
                        let error_chain = stream_read_error_chain(&e);
                        crate::logging::warn(format!(
                            "Stream read error: {error_chain} \
                             (elapsed: {}ms, bytes_received: {}, ms_since_last_event: {}, headers: {})",
                            stream_start.elapsed().as_millis(),
                            bytes_received,
                            last_event_at.elapsed().as_millis(),
                            response_headers,
                        ));
                        yield Err(stream_read_error(&e));
                        stream_failed = true;
                        break;
                    }
                };

                bytes_received = bytes_received.saturating_add(chunk.len());
                last_event_at = std::time::Instant::now();
                byte_buf.extend_from_slice(&chunk);

                // Guard against unbounded buffer growth (e.g., malformed stream without newlines)
                const MAX_SSE_BUF: usize = 10 * 1024 * 1024; // 10 MB
                if byte_buf.len() > MAX_SSE_BUF {
                    stream_failed = true;
                    yield Err(anyhow::anyhow!("SSE buffer exceeded {MAX_SSE_BUF} bytes — aborting stream"));
                    break;
                }

                if byte_buf.len() > SSE_BACKPRESSURE_HIGH_WATERMARK {
                    tokio::time::sleep(Duration::from_millis(SSE_BACKPRESSURE_SLEEP_MS)).await;
                }

                // Process complete SSE lines from the buffer
                let mut lines_processed = 0usize;
                while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                    let mut end = newline_pos;
                    if end > 0 && byte_buf[end - 1] == b'\r' {
                        end -= 1;
                    }
                    let line = String::from_utf8_lossy(&byte_buf[..end]).into_owned();
                    byte_buf.drain(..newline_pos + 1);

                    if line.is_empty() {
                        // Empty line = event boundary, process accumulated data
                        if !line_buf.is_empty() {
                            let data = std::mem::take(&mut line_buf);
                            match parse_sse_data_frame(
                                &data,
                                &mut content_index,
                                &mut text_started,
                                &mut thinking_started,
                                &mut tool_indices,
                                &mut reasoning_detail_buffers,
                                &mut inline_reasoning_tags,
                                reasoning_stream_style,
                            ) {
                                SseDataFrame::Done => {
                                    saw_done = true;
                                    saw_terminal_signal = true;
                                    break 'stream;
                                }
                                SseDataFrame::Events {
                                    events,
                                    wire_usage,
                                    response_model,
                                } => {
                                    if let Some(response_model) = response_model {
                                        response_accounting.set_model(response_model);
                                    }
                                    if wire_usage.is_some() {
                                        observed_wire_usage = wire_usage;
                                    }
                                    for mut event in events {
                                        let event_is_terminal =
                                            stream_event_has_stop_reason(&event);
                                        saw_terminal_signal |= event_is_terminal;
                                        // Stamp the client-side replay-token estimate
                                        // onto the final usage so the UI can surface
                                        // it (#30). We compute it pre-request and
                                        // overlay it on the server-reported usage at
                                        // stream completion.
                                        observe_stream_usage(
                                            &mut event,
                                            replay_input_tokens,
                                            &mut observed_usage,
                                        );
                                        if let Some(usage) = observed_usage.as_ref() {
                                            response_accounting.observe(
                                                &deepseek_accounting_usage(usage),
                                                observed_wire_usage.as_ref(),
                                            );
                                        }
                                        if saw_terminal_signal {
                                            pending_terminal_events.push(event);
                                        } else {
                                            yield Ok(event);
                                        }
                                    }
                                }
                                SseDataFrame::Invalid(error) => {
                                    yield Err(anyhow::anyhow!(error));
                                    stream_failed = true;
                                    break 'stream;
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(data) = super::extract_sse_data_value(&line) {
                        // The SSE spec joins multiple `data:` fields within one
                        // event with '\n'; concatenating with no separator would
                        // yield `{…}{…}` and fail JSON parsing, silently dropping
                        // the frame.
                        if !line_buf.is_empty() {
                            line_buf.push('\n');
                        }
                        line_buf.push_str(data);
                    }
                    // Ignore other SSE fields (event:, id:, retry:)

                    lines_processed = lines_processed.saturating_add(1);
                    if lines_processed >= SSE_MAX_LINES_PER_CHUNK {
                        // Yield cooperatively, then keep draining complete
                        // lines already buffered from this network chunk. A
                        // hard break here used to strand the tail until the
                        // next chunk; at EOF it was parsed as one multi-line
                        // JSON frame and silently lost.
                        tokio::task::yield_now().await;
                        lines_processed = 0;
                    }
                }
            }

            // Flush a final SSE frame that arrived without a terminating blank
            // line (the stream closed straight after the last `data:` line, or
            // that line lacked a trailing newline). Without this the final delta
            // — last tokens, finish_reason, and usage — is silently dropped.
            // Skipped after `[DONE]`, whose frame was already processed.
            if !saw_done && !stream_failed {
                if !byte_buf.is_empty() {
                    let mut end = byte_buf.len();
                    if end > 0 && byte_buf[end - 1] == b'\r' {
                        end -= 1;
                    }
                    let line = String::from_utf8_lossy(&byte_buf[..end]).into_owned();
                    if let Some(data) = super::extract_sse_data_value(&line) {
                        if !line_buf.is_empty() {
                            line_buf.push('\n');
                        }
                        line_buf.push_str(data);
                    }
                }
                if !line_buf.is_empty() {
                    let data = std::mem::take(&mut line_buf);
                    match parse_sse_data_frame(
                        &data,
                        &mut content_index,
                        &mut text_started,
                        &mut thinking_started,
                        &mut tool_indices,
                        &mut reasoning_detail_buffers,
                        &mut inline_reasoning_tags,
                        reasoning_stream_style,
                    ) {
                        SseDataFrame::Events {
                            events,
                            wire_usage,
                            response_model,
                        } => {
                            if let Some(response_model) = response_model {
                                response_accounting.set_model(response_model);
                            }
                            if wire_usage.is_some() {
                                observed_wire_usage = wire_usage;
                            }
                            for mut event in events {
                                let event_is_terminal = stream_event_has_stop_reason(&event);
                                saw_terminal_signal |= event_is_terminal;
                                observe_stream_usage(
                                    &mut event,
                                    replay_input_tokens,
                                    &mut observed_usage,
                                );
                                if let Some(usage) = observed_usage.as_ref() {
                                    response_accounting.observe(
                                        &deepseek_accounting_usage(usage),
                                        observed_wire_usage.as_ref(),
                                    );
                                }
                                if saw_terminal_signal {
                                    pending_terminal_events.push(event);
                                } else {
                                    yield Ok(event);
                                }
                            }
                        }
                        SseDataFrame::Invalid(error) => {
                            yield Err(anyhow::anyhow!(error));
                            stream_failed = true;
                        }
                        SseDataFrame::Done => {
                            saw_terminal_signal = true;
                        }
                    }
                }
            }

            if stream_failed {
                response_accounting.incomplete();
                release_stream_buffer(byte_buf);
                return;
            }

            if !saw_terminal_signal {
                response_accounting.incomplete();
                release_stream_buffer(byte_buf);
                yield Err(anyhow::anyhow!(
                    "DeepSeek SSE stream ended before [DONE] or finish_reason; partial output was not accepted"
                ));
                return;
            }

            let accounting_usage = observed_usage.as_ref().map(deepseek_accounting_usage);
            response_accounting.complete(accounting_usage.as_ref(), observed_wire_usage.as_ref());

            // Accounting is now atomically settled, so consumers may safely
            // treat the buffered stop_reason as the response terminal and
            // stop polling without turning a complete response into an
            // incomplete one.
            for event in pending_terminal_events {
                yield Ok(event);
            }

            // Close any open blocks — content_index points to the
            // currently active open block (it is only incremented
            // *after* a block is closed, not when opened).
            if thinking_started || text_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_index });
            }

            release_stream_buffer(byte_buf);
            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Pin::from(Box::new(stream)
            as Box<
                dyn futures_util::Stream<Item = Result<StreamEvent>> + Send,
            >))
    }
}

fn observe_stream_usage(
    event: &mut StreamEvent,
    replay_input_tokens: Option<u32>,
    observed_usage: &mut Option<Usage>,
) {
    if let StreamEvent::MessageDelta {
        usage: Some(usage), ..
    } = event
    {
        if let Some(tokens) = replay_input_tokens {
            usage.reasoning_replay_tokens = Some(tokens);
        }
        *observed_usage = Some(usage.clone());
    }
}

fn stream_event_has_stop_reason(event: &StreamEvent) -> bool {
    matches!(
        event,
        StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some(_),
                ..
            },
            ..
        }
    )
}

// === Chat Completions Helpers ===

#[cfg(test)]
pub(super) fn build_chat_messages(
    system: Option<&SystemPrompt>,
    messages: &[Message],
    model: &str,
) -> Vec<Value> {
    build_chat_messages_with_reasoning(
        system,
        messages,
        model,
        should_replay_reasoning_content(model, None),
        false,
    )
}

#[cfg(test)]
pub(super) fn build_chat_messages_for_request(request: &MessageRequest) -> Vec<Value> {
    PromptBuilder::for_request(request).build()
}

pub(super) fn build_chat_messages_for_request_and_provider(
    request: &MessageRequest,
    provider: ApiProvider,
) -> Vec<Value> {
    PromptBuilder::for_request(request).build_for_provider(provider)
}

struct PromptBuilder<'a> {
    system: Option<&'a SystemPrompt>,
    messages: &'a [Message],
    model: &'a str,
    reasoning_effort: Option<&'a str>,
}

impl<'a> PromptBuilder<'a> {
    fn for_request(request: &'a MessageRequest) -> Self {
        Self {
            system: request.system.as_ref(),
            messages: &request.messages,
            model: &request.model,
            reasoning_effort: request.reasoning_effort.as_deref(),
        }
    }

    #[cfg(test)]
    fn build(self) -> Vec<Value> {
        build_chat_messages_with_reasoning(
            self.system,
            self.messages,
            self.model,
            should_replay_reasoning_content(self.model, self.reasoning_effort),
            false,
        )
    }

    fn build_for_provider(self, provider: ApiProvider) -> Vec<Value> {
        let mut messages = build_chat_messages_with_reasoning(
            self.system,
            self.messages,
            self.model,
            should_replay_reasoning_content_for_provider(
                provider,
                self.model,
                self.reasoning_effort,
            ),
            false,
        );
        dump_system_prompt_if_requested(&messages);
        if provider == ApiProvider::Arcee {
            apply_arcee_waf_safe_message_encoding(&mut messages);
        }
        if provider == ApiProvider::Minimax {
            mirror_minimax_reasoning_details_for_messages(&mut messages);
        }
        messages
    }
}

const SYSTEM_PROMPT_DUMP_ENV: &str = "CODEWHALE_DUMP_SYSTEM_PROMPT";
const SYSTEM_PROMPT_DUMP_BEGIN: &str = "<<<CODEWHALE_SYSTEM_PROMPT_BEGIN>>>";
const SYSTEM_PROMPT_DUMP_END: &str = "<<<CODEWHALE_SYSTEM_PROMPT_END>>>";
const ARCEE_WAF_TEXT_SPLIT_TRIGGERS: &[(&str, &str, &str)] = &[("python -c", "python ", "-c")];

fn dump_system_prompt_if_requested(messages: &[Value]) {
    let Ok(flag) = std::env::var(SYSTEM_PROMPT_DUMP_ENV) else {
        return;
    };
    if !matches!(flag.trim(), "1" | "true" | "TRUE" | "yes" | "YES") {
        return;
    }
    let Some(prompt) = messages.iter().find_map(system_message_text) else {
        return;
    };
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{SYSTEM_PROMPT_DUMP_BEGIN}");
    let _ = writeln!(stderr, "{prompt}");
    let _ = writeln!(stderr, "{SYSTEM_PROMPT_DUMP_END}");
}

fn system_message_text(message: &Value) -> Option<String> {
    if message.get("role").and_then(Value::as_str) != Some("system") {
        return None;
    }
    match message.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn apply_arcee_waf_safe_message_encoding(messages: &mut [Value]) {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("system") {
            continue;
        }
        let Some(content) = message.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Some(parts) = arcee_waf_safe_text_parts(content) else {
            continue;
        };
        message["content"] = json!(parts);
    }
}

fn arcee_waf_safe_text_parts(content: &str) -> Option<Vec<Value>> {
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    let mut split_any = false;

    while cursor < content.len() {
        let Some((trigger_start, trigger, left, right)) = next_arcee_waf_trigger(content, cursor)
        else {
            push_text_part(&mut parts, &content[cursor..]);
            break;
        };

        push_text_part(&mut parts, &content[cursor..trigger_start]);
        push_text_part(&mut parts, left);
        push_text_part(&mut parts, right);
        cursor = trigger_start + trigger.len();
        split_any = true;
    }

    split_any.then_some(parts)
}

fn next_arcee_waf_trigger(content: &str, cursor: usize) -> Option<(usize, &str, &str, &str)> {
    ARCEE_WAF_TEXT_SPLIT_TRIGGERS
        .iter()
        .filter_map(|(trigger, left, right)| {
            content[cursor..]
                .find(trigger)
                .map(|offset| (cursor + offset, *trigger, *left, *right))
        })
        .min_by_key(|(start, _, _, _)| *start)
}

fn push_text_part(parts: &mut Vec<Value>, text: &str) {
    if !text.is_empty() {
        parts.push(json!({
            "type": "text",
            "text": text,
        }));
    }
}

const TOOL_RESULT_SENT_CHAR_BUDGET: usize = 12_000;
const TOOL_RESULT_HEAD_CHARS: usize = 4_000;
const TOOL_RESULT_TAIL_CHARS: usize = 4_000;
/// Tool results shorter than this stay inline even when repeated. The
/// extra prompt bytes are cheaper than forcing the model through an
/// unnecessary retrieval hop for tiny command outputs.
const TOOL_RESULT_DEDUP_MIN_CHARS: usize = 1_024;
/// Tool results shorter than this are also exempt from disk persistence —
/// no SHA file is written. The wire-dedup path won't fire for them
/// anyway (see `TOOL_RESULT_DEDUP_MIN_CHARS`), so there's no retrieval
/// burden to satisfy. Keeps `~/.deepseek/tool_outputs/` from filling
/// up with tiny `gh auth status` and `cat package.json` files.
const TOOL_RESULT_SHA_PERSIST_MIN_CHARS: usize = 1_024;

fn sha256_hex(bytes: &[u8]) -> String {
    crate::hashing::sha256_hex(bytes)
}

/// Persist a SHA-addressed copy of `content` to
/// `~/.deepseek/tool_outputs/sha_<sha>.txt` so the model can retrieve
/// the original bytes after the wire-dedup compactor has replaced
/// later occurrences with a `<TOOL_RESULT_REF sha="..." />` block.
///
/// Returns `true` when the persist succeeded (or the content is
/// below `TOOL_RESULT_SHA_PERSIST_MIN_CHARS` — there's no retrieval
/// need to satisfy). Returns `false` when the write failed and the
/// caller MUST skip dedup, because emitting a SHA ref the model
/// can't retrieve is worse than inlining the content twice. The
/// no-home-dir edge case (InvalidInput) is treated as a real
/// failure: we can't promise retrieval works without a writable
/// store.
fn persist_tool_result_for_sha(sha: &str, content: &str) -> bool {
    if content.chars().count() < TOOL_RESULT_SHA_PERSIST_MIN_CHARS {
        return true;
    }
    match crate::tools::truncate::write_sha_spillover(sha, content) {
        Ok(_) => true,
        Err(err) => {
            logging::warn(format!(
                "tool-result SHA spillover write failed for sha={sha}: {err} — dedup skipped"
            ));
            false
        }
    }
}

#[derive(Clone)]
struct PendingToolCallInfo {
    tool_name: String,
    input: Value,
}

pub(super) struct SeenToolResult {
    message_label: String,
    original_chars: usize,
}

pub(super) struct WireToolResult {
    pub(super) content: String,
    original_chars: usize,
    sent_chars: usize,
    truncated: bool,
    deduplicated: bool,
}

#[derive(Clone)]
struct TurnMetaBudget {
    original_chars: usize,
    sent_chars: usize,
    deduplicated: bool,
    sha256: String,
}

struct LastFullTurnMeta {
    sha256: String,
}

fn render_turn_meta_for_wire(
    text: &str,
    last_full_turn_meta: &mut Option<LastFullTurnMeta>,
) -> (String, TurnMetaBudget) {
    let original_chars = text.chars().count();
    let sha = sha256_hex(text.as_bytes());

    if last_full_turn_meta
        .as_ref()
        .is_some_and(|previous| previous.sha256 == sha)
    {
        // Keep the repeated metadata slot short without surfacing an
        // opaque hash the model cannot resolve.
        let rendered = "<turn_meta_unchanged />".to_string();
        let budget = TurnMetaBudget {
            original_chars,
            sent_chars: rendered.chars().count(),
            deduplicated: true,
            sha256: sha,
        };
        return (rendered, budget);
    }

    *last_full_turn_meta = Some(LastFullTurnMeta {
        sha256: sha.clone(),
    });
    (
        text.to_string(),
        TurnMetaBudget {
            original_chars,
            sent_chars: original_chars,
            deduplicated: false,
            sha256: sha,
        },
    )
}

fn is_turn_meta_text(text: &str) -> bool {
    text.trim_start().starts_with("<turn_meta>")
}

fn turn_meta_budget_json(turn_meta: &TurnMetaBudget) -> Value {
    json!({
        "original_chars": turn_meta.original_chars,
        "sent_chars": turn_meta.sent_chars,
        "deduplicated": turn_meta.deduplicated,
        "sha256": turn_meta.sha256,
    })
}

/// Mutating/write tools whose result body is a *confirmation* (it embeds
/// the unified diff + summary of what was just written), not retrievable
/// reference data. Two identical large `write_file` calls must each keep
/// their full confirmation inline: collapsing the later one to a
/// `<TOOL_RESULT_REF sha="..." />` makes the model lose the write-success
/// context and behave as if the file is missing (issue #1695). Read-style
/// tools (`read_file`, `grep_files`, `exec_shell`, …) are unaffected and
/// still dedup normally.
fn is_mutation_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file" | "apply_patch")
}

pub(super) fn compact_tool_result_for_wire(
    tool_name: &str,
    input: &Value,
    content: &str,
    message_label: &str,
    seen_tool_results: &mut HashMap<String, SeenToolResult>,
) -> WireToolResult {
    let original_chars = content.chars().count();
    let sha = sha256_hex(content.as_bytes());

    // Two independent size-and-kind predicates, deliberately decoupled:
    //
    // * `persist_eligible` — size only. Any large result (including a
    //   mutation tool's big diff) is written to the SHA-addressed store
    //   so that, if it gets truncated below, the elided middle stays
    //   retrievable via `retrieve_tool_result`. Mutation tools must NOT
    //   be excluded here: a >12k-char `write_file` diff that we truncate
    //   without persisting would leave the model unable to recover it.
    // * `dedup_eligible` — size AND non-mutation. Only this predicate
    //   gates collapsing a later identical result to a
    //   `<TOOL_RESULT_REF>`. Mutation-tool results are write
    //   *confirmations*, never dedup-eligible (#1695): two identical
    //   large `write_file` calls must each keep their full confirmation
    //   inline.
    //
    // Below the threshold, repeating the content is safer than asking
    // the model to chase a reference, and there's no retrieval burden to
    // satisfy, so both predicates are false.
    let persist_eligible = original_chars >= TOOL_RESULT_DEDUP_MIN_CHARS;
    let dedup_eligible = persist_eligible && !is_mutation_tool(tool_name);

    if dedup_eligible && let Some(previous) = seen_tool_results.get(&sha) {
        // Re-check persistence before emitting a ref. If the file is
        // already present this is a cheap no-op; if the write now fails,
        // inline the content rather than producing an orphan reference.
        if !persist_tool_result_for_sha(&sha, content) {
            return WireToolResult {
                content: content.to_string(),
                original_chars,
                sent_chars: original_chars,
                truncated: false,
                deduplicated: false,
            };
        }
        let content = format!(
            "<TOOL_RESULT_REF sha=\"{sha}\" original_message=\"{label}\" chars=\"{chars}\">\n\
             retrieve: retrieve_tool_result ref=sha:{sha}\n\
             </TOOL_RESULT_REF>",
            label = previous.message_label,
            chars = previous.original_chars,
        );
        return WireToolResult {
            sent_chars: content.chars().count(),
            content,
            original_chars,
            truncated: false,
            deduplicated: true,
        };
    }

    if persist_eligible {
        // Persist any large result so a later truncation below stays
        // retrievable by SHA — this includes mutation tools, whose big
        // diffs are NOT dedup-eligible but still must be recoverable
        // when elided. Only register the SHA as dedup-able (eligible to
        // be replaced by a back-reference later) when `dedup_eligible`:
        // if the write fails, skip registration so later occurrences
        // stay inline instead of pointing at a file that was never
        // created.
        let persisted = persist_tool_result_for_sha(&sha, content);
        if persisted && dedup_eligible {
            seen_tool_results.insert(
                sha.clone(),
                SeenToolResult {
                    message_label: message_label.to_string(),
                    original_chars,
                },
            );
        }
    }

    if original_chars <= TOOL_RESULT_SENT_CHAR_BUDGET {
        return WireToolResult {
            content: content.to_string(),
            original_chars,
            sent_chars: original_chars,
            truncated: false,
            deduplicated: false,
        };
    }

    let head = first_chars(content, TOOL_RESULT_HEAD_CHARS);
    let tail = last_chars(content, TOOL_RESULT_TAIL_CHARS);
    let kept = head.chars().count() + tail.chars().count();
    let omitted = original_chars.saturating_sub(kept);
    let compacted = format!(
        "[TOOL_RESULT_TRUNCATED]\n\
         tool_name: {tool_name}\n\
         command_or_query: {}\n\
         exit_status: {}\n\
         original_chars: {original_chars}\n\
         sha256: {sha}\n\
         retrieve: retrieve_tool_result ref=sha:{sha}\n\
         first_chars:\n\
         {head}\n\n\
         [... truncated {omitted} chars from middle ...]\n\n\
         last_chars:\n\
         {tail}",
        tool_command_or_query(input),
        tool_exit_status(content)
    );

    WireToolResult {
        sent_chars: compacted.chars().count(),
        content: compacted,
        original_chars,
        truncated: true,
        deduplicated: false,
    }
}

fn tool_command_or_query(input: &Value) -> String {
    for key in ["command", "cmd", "query", "q", "pattern", "path", "url"] {
        if let Some(value) = input.get(key) {
            return summarize_for_metadata(value, 500);
        }
    }
    summarize_for_metadata(input, 500)
}

fn tool_exit_status(content: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        for key in ["exit_code", "exit_status", "status", "code"] {
            if let Some(value) = value.get(key) {
                return summarize_for_metadata(value, 120);
            }
        }
    }

    for line in content.lines().take(20) {
        let trimmed = line.trim();
        for prefix in ["Exit code:", "exit code:", "Exit status:", "exit status:"] {
            if let Some(value) = trimmed.strip_prefix(prefix) {
                return value.trim().to_string();
            }
        }
    }
    "unknown".to_string()
}

fn summarize_for_metadata(value: &Value, max_chars: usize) -> String {
    let raw = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    let mut summarized = first_chars(&raw.replace('\n', "\\n"), max_chars);
    if raw.chars().count() > max_chars {
        summarized.push_str("...");
    }
    summarized
}

fn first_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn last_chars(value: &str, count: usize) -> String {
    let mut chars: Vec<char> = value.chars().rev().take(count).collect();
    chars.reverse();
    chars.into_iter().collect()
}

fn build_chat_messages_with_reasoning(
    system: Option<&SystemPrompt>,
    messages: &[Message],
    model: &str,
    include_reasoning: bool,
    include_tool_budget_metadata: bool,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut pending_tool_calls: HashMap<String, PendingToolCallInfo> = HashMap::new();
    let mut seen_tool_results: HashMap<String, SeenToolResult> = HashMap::new();
    let mut last_full_turn_meta: Option<LastFullTurnMeta> = None;

    if let Some(instructions) = system_to_instructions(system.cloned())
        && !instructions.trim().is_empty()
    {
        out.push(json!({
            "role": "system",
            "content": instructions,
        }));
    }

    for (message_index, message) in messages.iter().enumerate() {
        let role = message.role.as_str();
        let mut text_parts = Vec::new();
        let mut image_parts = Vec::new();
        let mut thinking_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_infos = Vec::new();
        let mut tool_results: Vec<(String, String, String)> = Vec::new();
        let mut turn_meta_budget: Option<TurnMetaBudget> = None;

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    if is_turn_meta_text(text) {
                        let (rendered, budget) =
                            render_turn_meta_for_wire(text, &mut last_full_turn_meta);
                        text_parts.push(rendered);
                        turn_meta_budget = Some(budget);
                    } else {
                        text_parts.push(text.clone());
                    }
                }
                ContentBlock::ImageUrl { image_url } => {
                    image_parts.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": image_url.url.clone(),
                        },
                    }));
                }
                ContentBlock::Thinking { thinking, .. } => thinking_parts.push(thinking.clone()),
                ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    raw_arguments,
                    caller,
                    ..
                } => {
                    let args = raw_arguments.clone().unwrap_or_else(|| {
                        serde_json::to_string(input).unwrap_or_else(|_| input.to_string())
                    });
                    let mut call = json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": to_api_tool_name(name),
                            "arguments": args,
                        }
                    });
                    if let Some(caller) = caller {
                        call["caller"] = json!({
                            "type": caller.caller_type,
                            "tool_id": caller.tool_id,
                        });
                    }
                    tool_calls.push(call);
                    tool_call_infos.push((
                        id.clone(),
                        PendingToolCallInfo {
                            tool_name: name.clone(),
                            input: input.clone(),
                        },
                    ));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    let message_label = format!("Message #{message_index}");
                    tool_results.push((tool_use_id.clone(), content.clone(), message_label));
                }
                ContentBlock::ServerToolUse { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. } => {}
            }
        }

        if role == "assistant" {
            let content = text_parts.join("\n");
            let reasoning_content = thinking_parts.join("\n");
            let has_text = !content.trim().is_empty();
            let has_tool_calls = !tool_calls.is_empty();
            // The current request's effort controls newly generated thinking,
            // not the validity of an earlier DeepSeek tool round. DeepSeek
            // requires an assistant tool-call message's reasoning_content to
            // be passed back on every subsequent request, including after the
            // caller switches thinking off.
            let replay_tool_call_reasoning =
                has_tool_calls && requires_tool_call_reasoning_replay(model);
            let replay_reasoning = include_reasoning || replay_tool_call_reasoning;
            // Reasoning replay must be a function of the stored message ONLY,
            // never of later history. DeepSeek's prefix cache hashes the raw
            // bytes of every message; flipping `reasoning_content` on/off
            // depending on whether a follow-up user turn exists rewrites a
            // historical message between turns and busts the cache from that
            // point onwards. Always emit `reasoning_content` when the model
            // requires replay AND the stored message carries thinking text.
            // Never invent reasoning for a historical tool round. The official
            // RequestPlan validates required history and fails before transport
            // when the canonical transcript does not contain the original
            // `reasoning_content`.
            let has_reasoning = replay_reasoning && !reasoning_content.is_empty();

            // DeepSeek rejects assistant messages where both `content` and
            // `tool_calls` are missing/null. Skip such entries even if they
            // carry reasoning-only metadata unless we can send a non-null
            // placeholder content field.
            if !has_text && !has_tool_calls && !has_reasoning {
                pending_tool_calls.clear();
                continue;
            }

            let mut msg = json!({
                "role": "assistant",
                "content": if has_text {
                    json!(content)
                } else if has_reasoning {
                    json!("")
                } else {
                    Value::Null
                },
            });
            if has_reasoning {
                msg["reasoning_content"] = json!(reasoning_content);
            }
            if has_tool_calls {
                msg["tool_calls"] = json!(tool_calls);
                pending_tool_calls = tool_call_infos.into_iter().collect();
            } else {
                pending_tool_calls.clear();
            }
            out.push(msg);
        } else if role == "system" {
            let content = text_parts.join("\n");
            if !content.trim().is_empty() {
                let mut msg = json!({
                    "role": "system",
                    "content": content,
                });
                if include_tool_budget_metadata && let Some(turn_meta) = &turn_meta_budget {
                    msg["_turn_meta_budget"] = turn_meta_budget_json(turn_meta);
                }
                out.push(msg);
            }
        } else if role == "user" {
            let content = text_parts.join("\n");
            let has_text = !content.trim().is_empty();
            let has_images = !image_parts.is_empty();
            if has_text || has_images {
                let wire_content = if has_images {
                    let mut parts = Vec::new();
                    if has_text {
                        parts.push(json!({
                            "type": "text",
                            "text": content,
                        }));
                    }
                    parts.extend(image_parts);
                    json!(parts)
                } else {
                    json!(content)
                };
                let mut msg = json!({
                    "role": "user",
                    "content": wire_content,
                });
                if include_tool_budget_metadata && let Some(turn_meta) = &turn_meta_budget {
                    msg["_turn_meta_budget"] = turn_meta_budget_json(turn_meta);
                }
                out.push(msg);
            }
        }

        if !tool_results.is_empty() {
            if pending_tool_calls.is_empty() {
                logging::warn("Dropping tool results without matching tool_calls");
            } else {
                for (tool_id, content, message_label) in tool_results {
                    if let Some(tool_info) = pending_tool_calls.remove(&tool_id) {
                        let wire_result = compact_tool_result_for_wire(
                            &tool_info.tool_name,
                            &tool_info.input,
                            &content,
                            &message_label,
                            &mut seen_tool_results,
                        );
                        let mut tool_msg = json!({
                            "role": "tool",
                            "tool_call_id": tool_id,
                            "content": wire_result.content,
                        });
                        if include_tool_budget_metadata {
                            tool_msg["_tool_result_budget"] = json!({
                                "original_chars": wire_result.original_chars,
                                "sent_chars": wire_result.sent_chars,
                                "truncated": wire_result.truncated,
                                "deduplicated": wire_result.deduplicated,
                            });
                        }
                        out.push(tool_msg);
                    } else {
                        logging::warn(format!(
                            "Dropping tool result for unknown tool_call_id: {tool_id}"
                        ));
                    }
                }
            }
        } else if role != "assistant" {
            pending_tool_calls.clear();
        }
    }

    // Safety net: after compaction, an assistant message may have tool_calls
    // whose results were summarized away. The API rejects these, so strip
    // the tool_calls (downgrading to a plain assistant message) and remove
    // the now-orphaned tool result messages.
    let mut i = 0;
    while i < out.len() {
        let is_assistant_with_tools = out[i].get("role").and_then(Value::as_str)
            == Some("assistant")
            && out[i].get("tool_calls").is_some();

        if is_assistant_with_tools {
            let expected_ids: HashSet<String> = out[i]
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|c| c.get("id").and_then(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            // Collect tool result IDs immediately following this assistant message.
            let mut found_ids: HashSet<String> = HashSet::new();
            let mut tool_result_end = i + 1;
            while tool_result_end < out.len() {
                if out[tool_result_end].get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(id) = out[tool_result_end]
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                    {
                        found_ids.insert(id.to_string());
                    }
                    tool_result_end += 1;
                } else {
                    break;
                }
            }

            // Also scan non-contiguous tool results up to the next assistant message
            // in case compaction left gaps.
            let mut scan = tool_result_end;
            while scan < out.len() {
                if out[scan].get("role").and_then(Value::as_str) == Some("assistant") {
                    break;
                }
                if out[scan].get("role").and_then(Value::as_str) == Some("tool")
                    && let Some(id) = out[scan].get("tool_call_id").and_then(Value::as_str)
                {
                    found_ids.insert(id.to_string());
                }
                scan += 1;
            }

            if !expected_ids.is_subset(&found_ids) {
                let missing: Vec<_> = expected_ids.difference(&found_ids).collect();
                logging::warn(format!(
                    "Stripping orphaned tool_calls from assistant message \
                     (expected {} tool results, found {}, missing: {:?})",
                    expected_ids.len(),
                    found_ids.len(),
                    missing
                ));
                if let Some(obj) = out[i].as_object_mut() {
                    obj.remove("tool_calls");
                }
                // If tool_calls were the only assistant content, remove the now-invalid
                // assistant message entirely (DeepSeek requires content or tool_calls).
                let assistant_content_empty = out[i]
                    .get("content")
                    .is_none_or(|v| v.is_null() || v.as_str().is_some_and(str::is_empty));
                if assistant_content_empty {
                    // Remove orphaned tool results tied to this stripped assistant call set.
                    let mut j = out.len();
                    while j > i + 1 {
                        j -= 1;
                        if out[j].get("role").and_then(Value::as_str) == Some("tool")
                            && let Some(id) = out[j].get("tool_call_id").and_then(Value::as_str)
                            && expected_ids.contains(id)
                        {
                            out.remove(j);
                        }
                    }
                    out.remove(i);
                    i = i.saturating_sub(1);
                    continue;
                }
                // Remove contiguous tool results first
                if tool_result_end > i + 1 {
                    out.drain((i + 1)..tool_result_end);
                }
                // Remove any remaining non-contiguous tool results referencing expected_ids
                // (scan backward to avoid index shifting issues)
                let mut j = out.len();
                while j > i + 1 {
                    j -= 1;
                    if out[j].get("role").and_then(Value::as_str) == Some("tool")
                        && let Some(id) = out[j].get("tool_call_id").and_then(Value::as_str)
                        && expected_ids.contains(id)
                    {
                        out.remove(j);
                    }
                }
            }
        }
        i += 1;
    }

    out
}

pub(super) fn tool_to_chat(tool: &Tool) -> Value {
    let mut value = json!({
        "type": "function",
        "function": {
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    });
    if let Some(strict) = tool.strict
        && let Some(function) = value.get_mut("function")
    {
        function["strict"] = json!(strict);
    }
    value
}

#[cfg(test)]
pub(super) fn tool_to_chat_for_legacy_base_url(tool: &Tool, base_url: &str) -> Value {
    tool_to_chat_for_legacy_route(tool, base_url, None)
}

pub(super) fn tool_to_chat_for_legacy_route(
    tool: &Tool,
    base_url: &str,
    path_suffix: Option<&str>,
) -> Value {
    let mut value = tool_to_chat(tool);
    if !legacy_route_supports_strict_tools(base_url, path_suffix)
        && let Some(function) = value.get_mut("function")
        && let Some(obj) = function.as_object_mut()
    {
        obj.remove("strict");
    }
    value
}

fn is_official_deepseek_base_url(base_url: &str) -> bool {
    let trimmed = base_url.trim_end_matches('/').to_ascii_lowercase();
    trimmed == "https://api.deepseek.com"
        || trimmed == "https://api.deepseek.com/v1"
        || trimmed == "https://api.deepseek.com/beta"
}

fn targets_deepseek_owned_host(base_url: &str) -> bool {
    reqwest::Url::parse(base_url).ok().is_some_and(|url| {
        url.host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
    })
}

pub(super) fn legacy_route_supports_strict_tools(
    base_url: &str,
    path_suffix: Option<&str>,
) -> bool {
    if !is_official_deepseek_base_url(base_url) {
        if targets_deepseek_owned_host(base_url) {
            // A malformed/customized URL on DeepSeek's own host is not an
            // unknown gateway contract. Refuse to claim Beta strict support
            // for ports, query strings, userinfo, or undocumented paths.
            return false;
        }
        // Custom OpenAI-compatible routes own their strict-schema contract.
        return true;
    }
    super::api_url_with_suffix(base_url, "chat/completions", path_suffix)
        .to_ascii_lowercase()
        .ends_with("/beta/chat/completions")
}

pub(super) fn map_tool_choice_for_chat(choice: &Value) -> Option<Value> {
    if let Some(choice_str) = choice.as_str() {
        return Some(json!(choice_str));
    }
    let Some(choice_type) = choice.get("type").and_then(Value::as_str) else {
        return Some(choice.clone());
    };

    match choice_type {
        "auto" | "none" => Some(json!(choice_type)),
        "any" => Some(json!("auto")),
        "tool" => choice.get("name").and_then(Value::as_str).map(|name| {
            json!({
                "type": "function",
                "function": { "name": to_api_tool_name(name) }
            })
        }),
        _ => Some(choice.clone()),
    }
}

pub(super) fn should_send_tool_choice_for_chat(
    provider: ApiProvider,
    effort: Option<&str>,
) -> bool {
    if !matches!(provider, ApiProvider::Deepseek | ApiProvider::DeepseekCN) {
        return true;
    }
    !deepseek::thinking_enabled_for_request(effort)
}

/// Tallies the exact stored `reasoning_content` replayed in an outgoing
/// chat-completions payload. This function never inserts or rewrites history.
/// The official DeepSeek RequestPlan separately fails closed when a historical
/// tool-call message is missing protocol-required reasoning.
///
/// Logs the replay size so
/// users on `RUST_LOG=codewhale_tui=debug` can see how much of their input
/// budget is being spent re-sending prior thinking traces.
pub(super) fn reasoning_replay_tokens_for_messages(
    body: &Value,
    model: &str,
    effort: Option<&str>,
    provider: ApiProvider,
) -> Option<u32> {
    let replay_reasoning = should_replay_reasoning_content_for_provider(provider, model, effort);
    let replay_tool_call_reasoning = requires_tool_call_reasoning_replay(model);
    if !replay_reasoning && !replay_tool_call_reasoning {
        return None;
    }
    let messages = body.get("messages").and_then(Value::as_array)?;
    let mut replay_chars: u64 = 0;
    let mut replay_messages: u32 = 0;
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(reasoning) = msg.get("reasoning_content").and_then(Value::as_str) {
            let len = reasoning.len() as u64;
            if len > 0 {
                replay_chars = replay_chars.saturating_add(len);
                replay_messages = replay_messages.saturating_add(1);
            }
        }
    }
    if replay_messages == 0 {
        return None;
    }
    // ~4 chars/token is the standard rough estimate; DeepSeek tokens skew
    // a touch shorter on Chinese/code but this is order-of-magnitude info.
    let approx_tokens = (replay_chars / 4).min(u64::from(u32::MAX)) as u32;
    logging::info(format!(
        "Reasoning-content replay: {replay_messages} assistant message(s), ~{approx_tokens} input tokens ({replay_chars} chars) being re-sent in this request",
    ));
    Some(approx_tokens)
}

/// Sums the byte length of `reasoning_content` across all assistant messages in
/// an outgoing chat-completions body. Used by tests; production replay
/// accounting computes the same number inline and logs it.
#[cfg(test)]
pub(super) fn count_reasoning_replay_chars(body: &Value) -> u64 {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return 0;
    };
    messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        .filter_map(|m| m.get("reasoning_content").and_then(Value::as_str))
        .map(|s| s.len() as u64)
        .sum()
}

/// Render the transport-shape headers we care about for #103 diagnostics.
/// Always returns SOMETHING printable so the decode-error log line is parseable
/// even when the server stripped a header we expected.
fn format_stream_headers(headers: &reqwest::header::HeaderMap) -> String {
    const FIELDS: &[&str] = &[
        "content-encoding",
        "transfer-encoding",
        "connection",
        "server",
    ];
    let mut parts: Vec<String> = Vec::with_capacity(FIELDS.len());
    for field in FIELDS {
        let rendered = headers
            .get(*field)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("(absent)");
        parts.push(format!("{field}={rendered}"));
    }
    parts.join(", ")
}

/// Diagnostic logger for provider-side `reasoning_content` rejection. Official
/// RequestPlan traffic has already passed local exact-replay preflight; this
/// also makes legacy-route and upstream protocol drift visible.
fn log_thinking_mode_violations(body: &Value) {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        logging::warn("reasoning-rejection: body has no `messages` array");
        return;
    };
    let mut violations: Vec<String> = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let reasoning = msg
            .get("reasoning_content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let has_tc = msg.get("tool_calls").is_some();
        if reasoning.trim().is_empty() {
            violations.push(format!(
                "assistant[{idx}] (reasoning_content missing, tool_calls={has_tc})"
            ));
        }
    }
    if violations.is_empty() {
        logging::warn(
            "reasoning-rejection: all assistant messages have reasoning_content — DeepSeek rejected for a different reason",
        );
    } else {
        logging::warn(format!(
            "reasoning-rejection: {} assistant message(s) lack reasoning_content: {}",
            violations.len(),
            violations.join(", ")
        ));
    }
}

fn requires_reasoning_content(model: &str) -> bool {
    let lower = model.to_lowercase();
    // V4-family direct model IDs.
    lower.contains("deepseek-v4")
        // Public DeepSeek API aliases routed server-side to the V4 family.
        // `deepseek-chat` resolves to `deepseek-v4-flash` and `deepseek-reasoner`
        // resolves to `deepseek-v4-pro`; both have thinking mode enabled by
        // default, so any assistant message carrying tool_calls must replay
        // `reasoning_content` on subsequent turns or the API returns 400.
        || lower.starts_with("deepseek-chat")
        || lower.starts_with("deepseek-reasoner")
        // Generic reasoning markers used by custom/proxied deployments.
        || lower.contains("reasoner")
        || lower.contains("-reasoning")
        || lower.contains("-thinking")
        || has_deepseek_r_series_marker(&lower)
}

/// Whether historical assistant tool calls must keep their reasoning payload.
///
/// This is intentionally independent of the current request's reasoning
/// effort: disabling thinking affects the next completion, while DeepSeek's
/// wire protocol still requires prior tool-call reasoning to be replayed.
pub(super) fn requires_tool_call_reasoning_replay(model: &str) -> bool {
    requires_reasoning_content(model)
}

#[cfg(test)]
fn should_replay_reasoning_content(model: &str, effort: Option<&str>) -> bool {
    if effort
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "off" | "disabled" | "none" | "false"
            )
        })
        .unwrap_or(false)
    {
        return false;
    }

    requires_reasoning_content(model)
}

fn should_replay_reasoning_content_for_provider(
    provider: ApiProvider,
    model: &str,
    effort: Option<&str>,
) -> bool {
    if effort
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "off" | "disabled" | "none" | "false"
            )
        })
        .unwrap_or(false)
    {
        return false;
    }

    if requires_reasoning_content(model) {
        return true;
    }

    if !provider_accepts_reasoning_content(provider) {
        // Generic non-DeepSeek model on a provider that rejects the field:
        // keep stripping it (preserves the #1542 fix). But a known DeepSeek
        // reasoning model pointed at a DeepSeek-compatible endpoint via the
        // generic `openai` provider still requires reasoning_content replay,
        // or the thinking-mode API returns 400 (#1739 / #1694).
        return false;
    }

    model_supports_reasoning(model)
}

/// Should the SSE parser treat incoming `reasoning_content` deltas as thinking
/// (vs. inlining them as answer text)?
///
/// DeepSeek-family models are classified on any provider because their API
/// requires `reasoning_content` replay on later turns (#1739 / #1694). Other
/// known reasoning-capable large models are classified only on providers whose
/// streaming shape exposes reasoning fields, so `reasoning`/`reasoning_content`
/// deltas become Thinking cells instead of leaking as normal answer text.
fn is_reasoning_model_for_stream(provider: ApiProvider, model: &str) -> bool {
    if requires_reasoning_content(model) {
        return true;
    }
    provider_accepts_reasoning_content(provider) && model_supports_reasoning(model)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReasoningStreamStyle {
    SeparateField,
    InlineTags,
    None,
}

fn reasoning_stream_style_for_stream(
    provider: ApiProvider,
    model: &str,
    configured: Option<&str>,
) -> ReasoningStreamStyle {
    if let Some(configured) = configured {
        if let Some(style) = parse_reasoning_stream_style(configured) {
            return style;
        }
        logging::warn(format!(
            "Ignoring unrecognized reasoning_stream_style `{configured}`; expected separate_field, inline_tags, or none"
        ));
    }
    if is_reasoning_model_for_stream(provider, model) {
        ReasoningStreamStyle::SeparateField
    } else {
        ReasoningStreamStyle::None
    }
}

fn parse_reasoning_stream_style(value: &str) -> Option<ReasoningStreamStyle> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "separate_field" | "separate" | "field" => Some(ReasoningStreamStyle::SeparateField),
        "inline_tags" | "inline" | "think_tags" | "thinking_tags" => {
            Some(ReasoningStreamStyle::InlineTags)
        }
        "none" | "text" | "disabled" | "off" => Some(ReasoningStreamStyle::None),
        _ => None,
    }
}

/// Providers whose chat-completions API both returns and accepts a dedicated
/// `reasoning_content` field on assistant messages.
///
/// Arcee is intentionally included. Trinity-Large-Thinking natively emits
/// `<think>...</think>` traces, but Arcee's hosted API serves it through vLLM
/// with `--reasoning-parser deepseek_r1`, which parses those blocks into a
/// `reasoning_content` field (verified live against `api.arcee.ai`: thinking
/// streams as `delta.reasoning_content`, the answer as `delta.content`, with no
/// `<think>` tags on the wire). Arcee's docs require replaying `reasoning_content`
/// on assistant tool-call turns; dropping it makes the model emit tool calls as
/// raw XML inside its thinking ("xml_in_reasoning" pitfall). Do not remove Arcee
/// here without new live evidence — see docs.arcee.ai/capabilities/reasoning-traces.
fn provider_accepts_reasoning_content(provider: ApiProvider) -> bool {
    matches!(
        provider,
        ApiProvider::Deepseek
            | ApiProvider::DeepseekCN
            | ApiProvider::NvidiaNim
            | ApiProvider::Openrouter
            | ApiProvider::XiaomiMimo
            | ApiProvider::Novita
            | ApiProvider::Fireworks
            | ApiProvider::Siliconflow
            | ApiProvider::SiliconflowCn
            | ApiProvider::Volcengine
            | ApiProvider::Arcee
            | ApiProvider::Minimax
            | ApiProvider::Sglang
            | ApiProvider::Zai
            | ApiProvider::Moonshot // #3016: Kimi thinking traces use reasoning_content
    )
}

fn has_deepseek_r_series_marker(model_lower: &str) -> bool {
    const PREFIX: &str = "deepseek-r";
    model_lower.match_indices(PREFIX).any(|(idx, _)| {
        model_lower[idx + PREFIX.len()..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
    })
}

fn reasoning_delta(
    value: &Value,
    choice_index: u32,
    reasoning_detail_buffers: &mut std::collections::HashMap<u32, String>,
) -> Option<String> {
    if let Some(reasoning) = value
        .get("reasoning_content")
        .or_else(|| value.get("reasoning"))
        .and_then(Value::as_str)
    {
        return Some(reasoning.to_string());
    }

    let details = value.get("reasoning_details").and_then(Value::as_array)?;
    let full_text = details
        .iter()
        .filter_map(|detail| detail.get("text").and_then(Value::as_str))
        .collect::<String>();
    if full_text.is_empty() {
        return None;
    }

    let previous = reasoning_detail_buffers.entry(choice_index).or_default();
    let delta = full_text
        .strip_prefix(previous.as_str())
        .unwrap_or(&full_text)
        .to_string();
    *previous = full_text;
    Some(delta)
}

fn reasoning_message_text(value: &Value) -> Option<String> {
    if let Some(reasoning) = value
        .get("reasoning_content")
        .or_else(|| value.get("reasoning"))
        .and_then(Value::as_str)
    {
        return Some(reasoning.to_string());
    }
    value
        .get("reasoning_details")
        .and_then(Value::as_array)
        .map(|details| {
            details
                .iter()
                .filter_map(|detail| detail.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
}

pub(super) fn parse_chat_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .context("Chat API response missing choices")?;
    let choice = choices
        .first()
        .context("Chat API response missing first choice")?;
    let message = choice
        .get("message")
        .context("Chat API response missing message")?;

    let mut content_blocks = Vec::new();
    if let Some(reasoning) =
        reasoning_message_text(message).filter(|reasoning| !reasoning.trim().is_empty())
    {
        content_blocks.push(ContentBlock::Thinking {
            signature: None,
            thinking: reasoning.to_string(),
        });
    }
    if let Some(text) = message.get("content").and_then(Value::as_str)
        && !text.trim().is_empty()
    {
        content_blocks.push(ContentBlock::Text {
            text: text.to_string(),
            cache_control: None,
        });
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string();
            let function = call.get("function");
            let name = tool_name_or_fallback(
                function.and_then(|f| f.get("name")).and_then(Value::as_str),
                &id,
                "Non-streaming response",
            );
            let raw_arguments = function
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let arguments = raw_arguments
                .as_deref()
                .map(|raw| serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string())))
                .unwrap_or(Value::Null);
            let caller = call.get("caller").and_then(|v| {
                v.get("type")
                    .and_then(Value::as_str)
                    .map(|caller_type| ToolCaller {
                        caller_type: caller_type.to_string(),
                        tool_id: v
                            .get("tool_id")
                            .and_then(Value::as_str)
                            .map(std::string::ToString::to_string),
                    })
            });

            content_blocks.push(ContentBlock::ToolUse {
                id,
                name: from_api_tool_name(&name),
                input: arguments,
                raw_arguments,
                caller,
            });
        }
    }

    let usage = parse_usage(payload.get("usage"));

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model,
        stop_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        stop_sequence: None,
        container: None,
        usage,
    })
}

#[derive(Debug, Default)]
struct InlineReasoningTagState {
    inside_think: bool,
    pending: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ReasoningSegment {
    Text(String),
    Thinking(String),
}

fn inline_reasoning_segments(
    content: &str,
    state: &mut InlineReasoningTagState,
    flush: bool,
) -> Vec<ReasoningSegment> {
    state.pending.push_str(content);
    let mut segments = Vec::new();

    loop {
        if state.pending.is_empty() {
            break;
        }

        if state.inside_think {
            if let Some(close_at) = state.pending.find("</think>") {
                push_reasoning_segment(
                    &mut segments,
                    ReasoningSegment::Thinking(state.pending[..close_at].to_string()),
                );
                state.pending.drain(..close_at + "</think>".len());
                state.inside_think = false;
                continue;
            }

            let hold_len = if flush {
                0
            } else {
                trailing_tag_prefix_len(&state.pending, "</think>")
            };
            let emit_len = state.pending.len().saturating_sub(hold_len);
            if emit_len > 0 {
                push_reasoning_segment(
                    &mut segments,
                    ReasoningSegment::Thinking(state.pending[..emit_len].to_string()),
                );
                state.pending.drain(..emit_len);
            }
            break;
        }

        if let Some(open_at) = state.pending.find("<think>") {
            push_reasoning_segment(
                &mut segments,
                ReasoningSegment::Text(state.pending[..open_at].to_string()),
            );
            state.pending.drain(..open_at + "<think>".len());
            state.inside_think = true;
            continue;
        }

        let hold_len = if flush {
            0
        } else {
            trailing_tag_prefix_len(&state.pending, "<think>")
        };
        let emit_len = state.pending.len().saturating_sub(hold_len);
        if emit_len > 0 {
            push_reasoning_segment(
                &mut segments,
                ReasoningSegment::Text(state.pending[..emit_len].to_string()),
            );
            state.pending.drain(..emit_len);
        }
        break;
    }

    segments
}

fn trailing_tag_prefix_len(content: &str, tag: &str) -> usize {
    let max_len = tag.len().min(content.len());
    for len in (1..=max_len).rev() {
        let start = content.len() - len;
        if content.is_char_boundary(start) && tag.starts_with(&content[start..]) {
            return len;
        }
    }
    0
}

fn push_reasoning_segment(segments: &mut Vec<ReasoningSegment>, segment: ReasoningSegment) {
    match &segment {
        ReasoningSegment::Text(text) | ReasoningSegment::Thinking(text) if text.is_empty() => {}
        _ => segments.push(segment),
    }
}

fn push_text_delta(
    events: &mut Vec<StreamEvent>,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    text: String,
) {
    if *thinking_started {
        events.push(StreamEvent::ContentBlockStop {
            index: *content_index,
        });
        *content_index += 1;
        *thinking_started = false;
    }
    if !*text_started {
        events.push(StreamEvent::ContentBlockStart {
            index: *content_index,
            content_block: ContentBlockStart::Text {
                text: String::new(),
            },
        });
        *text_started = true;
    }
    events.push(StreamEvent::ContentBlockDelta {
        index: *content_index,
        delta: Delta::TextDelta { text },
    });
}

fn push_thinking_delta(
    events: &mut Vec<StreamEvent>,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    thinking: String,
) {
    if *text_started {
        events.push(StreamEvent::ContentBlockStop {
            index: *content_index,
        });
        *content_index += 1;
        *text_started = false;
    }
    if !*thinking_started {
        events.push(StreamEvent::ContentBlockStart {
            index: *content_index,
            content_block: ContentBlockStart::Thinking {
                thinking: String::new(),
            },
        });
        *thinking_started = true;
    }
    events.push(StreamEvent::ContentBlockDelta {
        index: *content_index,
        delta: Delta::ThinkingDelta { thinking },
    });
}

// === SSE Chunk Parser ===

enum SseDataFrame {
    Done,
    Events {
        events: Vec<StreamEvent>,
        wire_usage: Option<Value>,
        response_model: Option<String>,
    },
    Invalid(String),
}

// The six `&mut` streaming-state fields plus the style flag are a deliberate,
// shared parser-state set (mirrored by `parse_sse_chunk*`); bundling them into a
// struct would only add reborrow noise on this hot SSE path.
#[allow(clippy::too_many_arguments)]
fn parse_sse_data_frame(
    data: &str,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    tool_indices: &mut std::collections::HashMap<u32, u32>,
    reasoning_detail_buffers: &mut std::collections::HashMap<u32, String>,
    inline_reasoning_tags: &mut InlineReasoningTagState,
    reasoning_stream_style: ReasoningStreamStyle,
) -> SseDataFrame {
    if data.trim() == "[DONE]" {
        return SseDataFrame::Done;
    }
    match serde_json::from_str::<Value>(data) {
        Ok(chunk_json) => {
            let wire_usage = chunk_json
                .get("usage")
                .filter(|usage| usage.is_object())
                .cloned();
            let response_model = chunk_json
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
            SseDataFrame::Events {
                events: parse_sse_chunk_with_reasoning_style(
                    &chunk_json,
                    content_index,
                    text_started,
                    thinking_started,
                    tool_indices,
                    reasoning_detail_buffers,
                    inline_reasoning_tags,
                    reasoning_stream_style,
                ),
                wire_usage,
                response_model,
            }
        }
        Err(error) => SseDataFrame::Invalid(format!(
            "DeepSeek SSE contained an invalid JSON data frame: {error}"
        )),
    }
}

/// Parse a single SSE chunk from the Chat Completions streaming API into
/// our internal `StreamEvent` representation.
#[cfg(test)]
pub(super) fn parse_sse_chunk(
    chunk: &Value,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    tool_indices: &mut std::collections::HashMap<u32, u32>,
    reasoning_detail_buffers: &mut std::collections::HashMap<u32, String>,
    is_reasoning_model: bool,
) -> Vec<StreamEvent> {
    let mut inline_reasoning_tags = InlineReasoningTagState::default();
    let reasoning_stream_style = if is_reasoning_model {
        ReasoningStreamStyle::SeparateField
    } else {
        ReasoningStreamStyle::None
    };
    parse_sse_chunk_with_reasoning_style(
        chunk,
        content_index,
        text_started,
        thinking_started,
        tool_indices,
        reasoning_detail_buffers,
        &mut inline_reasoning_tags,
        reasoning_stream_style,
    )
}

// Same deliberate shared parser-state set as `parse_sse_data_frame`.
#[allow(clippy::too_many_arguments)]
fn parse_sse_chunk_with_reasoning_style(
    chunk: &Value,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    tool_indices: &mut std::collections::HashMap<u32, u32>,
    reasoning_detail_buffers: &mut std::collections::HashMap<u32, String>,
    inline_reasoning_tags: &mut InlineReasoningTagState,
    reasoning_stream_style: ReasoningStreamStyle,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
        // Usage-only chunk (sent at end with stream_options)
        if let Some(usage_val) = chunk.get("usage") {
            let usage = parse_usage(Some(usage_val));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage),
            });
        }
        return events;
    };

    if choices.is_empty() {
        if let Some(usage_val) = chunk.get("usage") {
            let usage = parse_usage(Some(usage_val));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage),
            });
        }
        return events;
    }

    for choice in choices {
        let choice_index = choice.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
        let delta = choice.get("delta");
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string);

        if let Some(delta) = delta {
            let reasoning_text = reasoning_delta(delta, choice_index, reasoning_detail_buffers)
                .filter(|s| !s.is_empty());
            let content_text = delta
                .get("content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);

            // Handle reasoning_content / reasoning thinking deltas.
            if reasoning_stream_style == ReasoningStreamStyle::SeparateField
                && let Some(reasoning) = reasoning_text.as_deref()
            {
                push_thinking_delta(
                    &mut events,
                    content_index,
                    text_started,
                    thinking_started,
                    reasoning.to_string(),
                );
            }

            // Generic OpenAI-compatible proxies sometimes stream answer text
            // in `reasoning_content`. If this route is configured with no
            // reasoning semantics, render that field as normal text when no
            // `content` delta is present.
            match (content_text, reasoning_stream_style) {
                (Some(content), ReasoningStreamStyle::InlineTags) => {
                    for segment in inline_reasoning_segments(&content, inline_reasoning_tags, false)
                    {
                        match segment {
                            ReasoningSegment::Text(text) => push_text_delta(
                                &mut events,
                                content_index,
                                text_started,
                                thinking_started,
                                text,
                            ),
                            ReasoningSegment::Thinking(thinking) => push_thinking_delta(
                                &mut events,
                                content_index,
                                text_started,
                                thinking_started,
                                thinking,
                            ),
                        }
                    }
                }
                (Some(content), _) => push_text_delta(
                    &mut events,
                    content_index,
                    text_started,
                    thinking_started,
                    content,
                ),
                (None, ReasoningStreamStyle::None) => {
                    if let Some(content) = reasoning_text {
                        push_text_delta(
                            &mut events,
                            content_index,
                            text_started,
                            thinking_started,
                            content,
                        );
                    }
                }
                (None, _) => {}
            }

            // Handle tool calls
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let tc_index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let tool_block_index = match tool_indices.entry(tc_index) {
                        std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            // Close text block if transitioning to tool use
                            if *text_started {
                                events.push(StreamEvent::ContentBlockStop {
                                    index: *content_index,
                                });
                                *content_index += 1;
                                *text_started = false;
                            }
                            if *thinking_started {
                                events.push(StreamEvent::ContentBlockStop {
                                    index: *content_index,
                                });
                                *content_index += 1;
                                *thinking_started = false;
                            }

                            let block_index = *content_index;
                            let id = tc
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                // Some upstream gateways (and the responses-API
                                // bridge) elide the `id` on the first chunk of a
                                // tool call. Falling back to a constant string
                                // collides when the model emits parallel tool
                                // calls in the same delta — every call ended up
                                // with the same id and downstream tool-result
                                // routing matched the first one twice. Index by
                                // the content-block position to keep the
                                // fallback unique within the response.
                                .unwrap_or_else(|| format!("call_{block_index}"));
                            let name = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(Value::as_str);
                            let name = tool_name_or_fallback(name, &id, "Streaming response chunk");
                            let caller = tc.get("caller").and_then(|v| {
                                v.get("type").and_then(Value::as_str).map(|caller_type| {
                                    ToolCaller {
                                        caller_type: caller_type.to_string(),
                                        tool_id: v
                                            .get("tool_id")
                                            .and_then(Value::as_str)
                                            .map(std::string::ToString::to_string),
                                    }
                                })
                            });

                            events.push(StreamEvent::ContentBlockStart {
                                index: block_index,
                                content_block: ContentBlockStart::ToolUse {
                                    id,
                                    name: from_api_tool_name(&name),
                                    input: json!({}),
                                    caller,
                                },
                            });
                            *content_index = (*content_index).saturating_add(1);
                            entry.insert(block_index);
                            block_index
                        }
                    };

                    // Stream tool call arguments
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                    {
                        events.push(StreamEvent::ContentBlockDelta {
                            index: tool_block_index,
                            delta: Delta::InputJsonDelta {
                                partial_json: args.to_string(),
                            },
                        });
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(reason) = finish_reason {
            if reasoning_stream_style == ReasoningStreamStyle::InlineTags {
                for segment in inline_reasoning_segments("", inline_reasoning_tags, true) {
                    match segment {
                        ReasoningSegment::Text(text) => push_text_delta(
                            &mut events,
                            content_index,
                            text_started,
                            thinking_started,
                            text,
                        ),
                        ReasoningSegment::Thinking(thinking) => push_thinking_delta(
                            &mut events,
                            content_index,
                            text_started,
                            thinking_started,
                            thinking,
                        ),
                    }
                }
            }
            // Close any open blocks
            if *text_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *text_started = false;
            }
            if *thinking_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *thinking_started = false;
            }
            // Close tool blocks
            let mut open_tool_indices: Vec<u32> =
                tool_indices.drain().map(|(_, idx)| idx).collect();
            open_tool_indices.sort_unstable();
            for tool_block_index in open_tool_indices {
                events.push(StreamEvent::ContentBlockStop {
                    index: tool_block_index,
                });
            }

            // Emit usage from the chunk if available
            let chunk_usage = chunk.get("usage").map(|u| parse_usage(Some(u)));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some(reason),
                    stop_sequence: None,
                },
                usage: chunk_usage,
            });
        }
    }

    events
}

fn tool_name_or_fallback(name: Option<&str>, id: &str, source: &str) -> String {
    let trimmed = name.unwrap_or("").trim();
    if trimmed.is_empty() {
        logging::warn(format!(
            "{source} returned an empty tool name for call {id}; using unknown_tool"
        ));
        "unknown_tool".to_string()
    } else {
        trimmed.to_string()
    }
}

// === #103 Phase 1: stream-decode diagnostics ===================================

#[cfg(test)]
mod stream_diagnostics_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn stream_open_timeout_defaults_and_clamps_env_values() {
        assert_eq!(stream_open_timeout_from_env(None), Duration::from_secs(45));
        assert_eq!(
            stream_open_timeout_from_env(Some("not-a-number")),
            Duration::from_secs(45)
        );
        assert_eq!(
            stream_open_timeout_from_env(Some("1")),
            Duration::from_secs(5)
        );
        assert_eq!(
            stream_open_timeout_from_env(Some("120")),
            Duration::from_secs(120)
        );
        assert_eq!(
            stream_open_timeout_from_env(Some("999")),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn non_streaming_header_timeout_remains_typed() {
        let error = response_header_timeout_error(Duration::from_secs(5));
        assert!(matches!(
            error.downcast_ref::<LlmError>(),
            Some(LlmError::Timeout(timeout)) if *timeout == Duration::from_secs(5)
        ));
    }

    #[test]
    fn deepseek_thinking_omits_tool_choice() {
        for effort in [None, Some("high"), Some("max"), Some("medium"), Some("")] {
            assert!(
                !should_send_tool_choice_for_chat(ApiProvider::Deepseek, effort),
                "DeepSeek thinking rejects explicit tool_choice for {effort:?}"
            );
            assert!(
                !should_send_tool_choice_for_chat(ApiProvider::DeepseekCN, effort),
                "DeepSeek CN thinking rejects explicit tool_choice for {effort:?}"
            );
        }

        for effort in [Some("off"), Some("disabled"), Some("none"), Some("false")] {
            assert!(should_send_tool_choice_for_chat(
                ApiProvider::Deepseek,
                effort
            ));
        }
        assert!(should_send_tool_choice_for_chat(
            ApiProvider::Openrouter,
            Some("high")
        ));
    }

    #[test]
    fn format_stream_headers_renders_all_fields_when_present() {
        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", HeaderValue::from_static("gzip"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("connection", HeaderValue::from_static("keep-alive"));
        headers.insert("server", HeaderValue::from_static("openresty/1.25.3.1"));

        let rendered = format_stream_headers(&headers);
        // Order is fixed by FIELDS in the helper; assert each field appears.
        assert!(
            rendered.contains("content-encoding=gzip"),
            "got: {rendered}"
        );
        assert!(
            rendered.contains("transfer-encoding=chunked"),
            "got: {rendered}"
        );
        assert!(
            rendered.contains("connection=keep-alive"),
            "got: {rendered}"
        );
        assert!(
            rendered.contains("server=openresty/1.25.3.1"),
            "got: {rendered}"
        );
    }

    #[test]
    fn format_stream_headers_marks_missing_fields_as_absent() {
        // DeepSeek frequently omits content-encoding when not compressing.
        // The diagnostic must still produce a parseable line so log scrapers
        // don't lose the slot.
        let headers = HeaderMap::new();
        let rendered = format_stream_headers(&headers);
        assert!(
            rendered.contains("content-encoding=(absent)"),
            "missing field must be explicitly marked; got: {rendered}"
        );
        assert!(
            rendered.contains("transfer-encoding=(absent)"),
            "missing field must be explicitly marked; got: {rendered}"
        );
    }

    #[test]
    fn format_stream_headers_handles_non_ascii_value_gracefully() {
        // If a header value isn't UTF-8, `.to_str()` fails — we must not panic
        // and should still produce a parseable line.
        let mut headers = HeaderMap::new();
        // 0xFF is a valid byte but invalid UTF-8 start byte.
        headers.insert(
            "server",
            HeaderValue::from_bytes(b"\xff\xfemystery").expect("header value"),
        );
        let rendered = format_stream_headers(&headers);
        assert!(
            rendered.contains("server=(absent)"),
            "non-UTF8 header values fall back to (absent); got: {rendered}"
        );
    }
}

#[cfg(test)]
mod arcee_waf_message_encoding_tests {
    use super::build_chat_messages_for_request_and_provider;
    use crate::config::ApiProvider;
    use crate::models::{MessageRequest, SystemPrompt};
    use serde_json::Value;

    fn request_with_system(system: &str) -> MessageRequest {
        MessageRequest {
            model: "trinity-large-thinking".to_string(),
            messages: Vec::new(),
            max_tokens: 16,
            system: Some(SystemPrompt::Text(system.to_string())),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
        }
    }

    fn decoded_content(content: &Value) -> String {
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        content
            .as_array()
            .expect("content parts")
            .iter()
            .map(|part| part.get("text").and_then(Value::as_str).expect("text part"))
            .collect()
    }

    #[test]
    fn arcee_splits_waf_trigger_without_changing_decoded_system_prompt() {
        let system = "Run calculations with `python -c 'print(1)'` when a tool is available.";
        let request = request_with_system(system);

        let messages = build_chat_messages_for_request_and_provider(&request, ApiProvider::Arcee);
        let content = &messages[0]["content"];

        assert!(
            content.is_array(),
            "Arcee system content with a WAF trigger should be encoded as text parts"
        );
        assert_eq!(decoded_content(content), system);
        let serialized = serde_json::to_string(&messages).expect("serialize messages");
        assert!(
            !serialized.contains("python -c"),
            "wire JSON should not contain the Cloudflare trigger contiguously: {serialized}"
        );
    }

    #[test]
    fn non_arcee_providers_keep_system_prompt_as_string() {
        let system = "Run calculations with `python -c 'print(1)'` when a tool is available.";
        let request = request_with_system(system);

        let messages = build_chat_messages_for_request_and_provider(&request, ApiProvider::Openai);

        assert_eq!(messages[0]["content"].as_str(), Some(system));
    }

    #[test]
    fn arcee_keeps_non_triggering_system_prompt_as_string() {
        let system = "Use read-only tools to inspect files before reporting results.";
        let request = request_with_system(system);

        let messages = build_chat_messages_for_request_and_provider(&request, ApiProvider::Arcee);

        assert_eq!(messages[0]["content"].as_str(), Some(system));
    }
}

#[cfg(test)]
mod minimax_reasoning_replay_tests {
    use super::build_chat_messages_for_request_and_provider;
    use crate::config::{ApiProvider, DEFAULT_MINIMAX_MODEL};
    use crate::models::{ContentBlock, Message, MessageRequest};

    fn request_with_assistant_thinking() -> MessageRequest {
        MessageRequest {
            model: DEFAULT_MINIMAX_MODEL.to_string(),
            messages: vec![Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Inspect tool state".to_string(),
                        signature: None,
                    },
                    ContentBlock::Text {
                        text: "Done.".to_string(),
                        cache_control: None,
                    },
                ],
            }],
            max_tokens: 16,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
        }
    }

    #[test]
    fn minimax_history_replays_thinking_as_reasoning_details() {
        let request = request_with_assistant_thinking();

        let messages = build_chat_messages_for_request_and_provider(&request, ApiProvider::Minimax);
        let assistant = &messages[0];

        assert_eq!(
            assistant
                .get("reasoning_content")
                .and_then(|value| value.as_str()),
            Some("Inspect tool state")
        );
        assert_eq!(
            assistant
                .pointer("/reasoning_details/0/type")
                .and_then(|value| value.as_str()),
            Some("text")
        );
        assert_eq!(
            assistant
                .pointer("/reasoning_details/0/text")
                .and_then(|value| value.as_str()),
            Some("Inspect tool state")
        );
    }
}

// === #103 Phase 4: SSE decoder behavior on canned chunk sequences ============

#[cfg(test)]
mod stream_decoder_tests {
    //! Drive `parse_sse_chunk` (the in-place SSE event extractor) over canned
    //! chunk sequences. The full `handle_chat_completion_stream` path needs a
    //! live `reqwest::Response` so it isn't unit-testable without a mock HTTP
    //! harness (issue #69 tracks that). For #103 we exercise the chunk decoder
    //! directly to verify each "class of stream failure" the engine relies on.
    use super::*;
    use crate::models::{ContentBlockStart, Delta, StreamEvent};
    use futures_util::StreamExt as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn transport_idle_failure_remains_a_typed_stream_stall() {
        let error = stream_idle_error(Duration::from_secs(17));
        let stall = error
            .downcast_ref::<StreamError>()
            .expect("transport idle must remain downcastable");

        assert!(matches!(stall, StreamError::Stall { timeout_secs: 17 }));
        let envelope = stall.clone().into_envelope();
        assert_eq!(envelope.code, "stream_stall");
    }

    #[tokio::test]
    async fn truncated_chunked_body_remains_a_typed_network_error() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind reset fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept fixture request");
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n40\r\ndata: {\"choices\":[",
                )
                .await
                .expect("write truncated chunk");
            // Dropping the socket before the declared 0x40-byte chunk is
            // complete reproduces reqwest/hyper's real body-read failure.
        });

        let response = crate::tls::reqwest_client_builder()
            .build()
            .expect("fixture HTTP client")
            .get(format!("http://{address}/chat"))
            .send()
            .await
            .expect("headers arrive before reset");
        let mut body = response.bytes_stream();
        let reset = loop {
            match body.next().await {
                Some(Ok(_)) => continue,
                Some(Err(error)) => break error,
                None => panic!("truncated chunk must fail rather than look complete"),
            }
        };
        server.await.expect("reset fixture task");

        let typed = stream_read_error(&reset);
        let network = typed
            .downcast_ref::<LlmError>()
            .expect("stream reset must remain typed for AgentRuntime");
        assert!(matches!(network, LlmError::NetworkError(message) if
            message.contains("Stream read error") && message.contains("response body")));
    }

    /// Decode a raw SSE-data JSON chunk into our internal events, mirroring
    /// the per-event call shape used by `handle_chat_completion_stream`.
    fn decode_chunk(json_text: &str) -> Vec<StreamEvent> {
        decode_chunk_with_reasoning(json_text, true)
    }

    fn decode_chunk_with_reasoning(json_text: &str, is_reasoning_model: bool) -> Vec<StreamEvent> {
        let chunk: Value = serde_json::from_str(json_text).expect("valid SSE JSON");
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        parse_sse_chunk(
            &chunk,
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            &mut reasoning_detail_buffers,
            is_reasoning_model,
        )
    }

    fn decode_chunks_with_style(
        chunks: &[&str],
        reasoning_stream_style: ReasoningStreamStyle,
    ) -> Vec<StreamEvent> {
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let mut inline_reasoning_tags = InlineReasoningTagState::default();
        let mut events = Vec::new();

        for chunk in chunks {
            let value: Value = serde_json::from_str(chunk).expect("valid SSE JSON");
            events.extend(parse_sse_chunk_with_reasoning_style(
                &value,
                &mut content_index,
                &mut text_started,
                &mut thinking_started,
                &mut tool_indices,
                &mut reasoning_detail_buffers,
                &mut inline_reasoning_tags,
                reasoning_stream_style,
            ));
        }
        events
    }

    fn text_delta_text(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    delta: Delta::TextDelta { text },
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn thinking_delta_text(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { thinking },
                    ..
                } => Some(thinking.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn decoder_emits_text_delta_for_content_chunk() {
        // The "happy" first chunk: a normal content delta. The engine treats
        // this as `any_content_received = true` and would NOT transparently
        // retry on a subsequent error.
        let events = decode_chunk(r#"{"choices":[{"delta":{"content":"hello"}}]}"#);
        assert!(
            matches!(
                events.first(),
                Some(StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::Text { .. },
                    ..
                })
            ),
            "first event should open a text block; got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockDelta {
                    delta: Delta::TextDelta { text },
                    ..
                } if text == "hello")),
            "should yield a TextDelta carrying 'hello'; got {events:?}"
        );
    }

    #[test]
    fn decoder_emits_thinking_delta_for_reasoning_chunk() {
        // V4 thinking models surface reasoning_content first — the engine
        // also counts these as content received (so a subsequent stream error
        // surfaces rather than retrying transparently).
        let events = decode_chunk(r#"{"choices":[{"delta":{"reasoning_content":"plan..."}}]}"#);
        assert!(
            matches!(
                events.first(),
                Some(StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::Thinking { .. },
                    ..
                })
            ),
            "first event should open a thinking block; got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { thinking },
                    ..
                } if thinking == "plan...")),
            "should yield a ThinkingDelta carrying 'plan...'; got {events:?}"
        );
    }

    #[test]
    fn decoder_streams_moonshot_multi_chunk_reasoning_as_thinking() {
        // #3016: recorded shape from Moonshot's native endpoint — kimi-k2.6
        // streams `reasoning_content` deltas before the answer text. The
        // thinking deltas must accumulate into ONE thinking block and the
        // answer must arrive as text, not be glued into the trace.
        let chunks = [
            r#"{"id":"cmpl-kimi","model":"kimi-k2.6","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Let me check"}}]}"#,
            r#"{"id":"cmpl-kimi","model":"kimi-k2.6","choices":[{"index":0,"delta":{"reasoning_content":" the config."}}]}"#,
            r#"{"id":"cmpl-kimi","model":"kimi-k2.6","choices":[{"index":0,"delta":{"content":"The answer is 42."}}]}"#,
        ];

        let is_reasoning =
            is_reasoning_model_for_stream(crate::config::ApiProvider::Moonshot, "kimi-k2.6");
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let mut events = Vec::new();
        for chunk in chunks {
            let value: Value = serde_json::from_str(chunk).expect("valid SSE JSON");
            events.extend(parse_sse_chunk(
                &value,
                &mut content_index,
                &mut text_started,
                &mut thinking_started,
                &mut tool_indices,
                &mut reasoning_detail_buffers,
                is_reasoning,
            ));
        }

        let thinking: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { thinking },
                    ..
                } => Some(thinking.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thinking, "Let me check the config.");

        let thinking_starts = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    StreamEvent::ContentBlockStart {
                        content_block: ContentBlockStart::Thinking { .. },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(thinking_starts, 1, "one thinking block: {events:?}");

        let text: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    delta: Delta::TextDelta { text },
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "The answer is 42.");
    }

    #[test]
    fn decoder_accepts_openrouter_reasoning_delta_with_extra_fields() {
        let events = decode_chunk(
            r#"{"id":"or-1","choices":[{"delta":{"reasoning":"openrouter thought","reasoning_details":[{"type":"summary","text":"extra"}],"native_finish_reason":null}}],"usage":{"completion_tokens_details":{"reasoning_tokens":3}}}"#,
        );

        assert!(
            events.iter().any(|e| matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { thinking },
                    ..
                } if thinking == "openrouter thought"
            )),
            "OpenRouter-style reasoning deltas with extra fields should not crash decoding; got {events:?}"
        );
    }

    #[test]
    fn decoder_streams_minimax_reasoning_details_as_incremental_thinking() {
        // MiniMax's reasoning_split stream reports reasoning_details text as
        // a cumulative buffer. Emit only the suffix so the Thinking cell does
        // not duplicate earlier reasoning chunks.
        let chunks = [
            r#"{"id":"minimax-1","choices":[{"index":0,"delta":{"reasoning_details":[{"type":"text","text":"Inspect"}]}}]}"#,
            r#"{"id":"minimax-1","choices":[{"index":0,"delta":{"reasoning_details":[{"type":"text","text":"Inspect config"}]}}]}"#,
            r#"{"id":"minimax-1","choices":[{"index":0,"delta":{"content":"Done."}}]}"#,
        ];

        let is_reasoning = is_reasoning_model_for_stream(ApiProvider::Minimax, "MiniMax-M3");
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let mut events = Vec::new();
        for chunk in chunks {
            let value: Value = serde_json::from_str(chunk).expect("valid SSE JSON");
            events.extend(parse_sse_chunk(
                &value,
                &mut content_index,
                &mut text_started,
                &mut thinking_started,
                &mut tool_indices,
                &mut reasoning_detail_buffers,
                is_reasoning,
            ));
        }

        let thinking: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { thinking },
                    ..
                } => Some(thinking.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thinking, "Inspect config");

        assert!(!events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: Delta::TextDelta { text },
                ..
            } if text == "Inspect" || text == "Inspect config"
        )));
    }

    #[test]
    fn decoder_does_not_render_reasoning_as_text_for_known_provider_models() {
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let is_reasoning_model =
            is_reasoning_model_for_stream(ApiProvider::XiaomiMimo, "mimo-v2.5-pro");
        let events = parse_sse_chunk(
            &serde_json::json!({
                "choices": [{
                    "delta": {
                        "reasoning_content": "private plan"
                    }
                }]
            }),
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            &mut reasoning_detail_buffers,
            is_reasoning_model,
        );

        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: Delta::ThinkingDelta { thinking },
                ..
            } if thinking == "private plan"
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: Delta::TextDelta { text },
                ..
            } if text == "private plan"
        )));
    }

    #[test]
    fn decoder_treats_reasoning_content_as_text_when_provider_does_not_support_reasoning() {
        let events = decode_chunk_with_reasoning(
            r#"{"choices":[{"delta":{"reasoning_content":"hello"}}]}"#,
            false,
        );

        assert!(
            matches!(
                events.first(),
                Some(StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::Text { .. },
                    ..
                })
            ),
            "first event should open a text block; got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: Delta::TextDelta { text },
                    ..
                } if text == "hello"
            )),
            "should yield a TextDelta carrying 'hello'; got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: Delta::ThinkingDelta { .. },
                    ..
                }
            )),
            "should not emit thinking deltas for generic providers; got {events:?}"
        );
    }

    #[test]
    fn reasoning_style_separate_field_routes_reasoning_to_thinking() {
        let events = decode_chunks_with_style(
            &[
                r#"{"choices":[{"delta":{"reasoning_content":"private plan"}}]}"#,
                r#"{"choices":[{"delta":{"content":"Public answer."}}]}"#,
            ],
            ReasoningStreamStyle::SeparateField,
        );

        assert_eq!(thinking_delta_text(&events), "private plan");
        assert_eq!(text_delta_text(&events), "Public answer.");
    }

    #[test]
    fn reasoning_style_inline_tags_routes_think_blocks_to_thinking() {
        let events = decode_chunks_with_style(
            &[
                r#"{"choices":[{"delta":{"content":"Before <thi"}}]}"#,
                r#"{"choices":[{"delta":{"content":"nk>private plan</thi"}}]}"#,
                r#"{"choices":[{"delta":{"content":"nk> after."}}]}"#,
            ],
            ReasoningStreamStyle::InlineTags,
        );

        assert_eq!(thinking_delta_text(&events), "private plan");
        assert_eq!(text_delta_text(&events), "Before  after.");
        assert!(
            !text_delta_text(&events).contains("<think>"),
            "inline reasoning tags must not leak into visible text: {events:?}"
        );
    }

    #[test]
    fn reasoning_style_inline_tags_flushes_unclosed_think_at_stream_end() {
        let events = decode_chunks_with_style(
            &[
                r#"{"choices":[{"delta":{"content":"Before <think>partial reasoning"}}]}"#,
                r#"{"choices":[{"finish_reason":"stop"}]}"#,
            ],
            ReasoningStreamStyle::InlineTags,
        );

        assert_eq!(thinking_delta_text(&events), "partial reasoning");
        assert_eq!(text_delta_text(&events), "Before ");
    }

    #[test]
    fn reasoning_style_inline_tags_ignores_separate_reasoning_field() {
        let events = decode_chunks_with_style(
            &[
                r#"{"choices":[{"delta":{"reasoning_content":"metadata","content":"<think>tagged</think> answer"}}]}"#,
            ],
            ReasoningStreamStyle::InlineTags,
        );

        assert_eq!(thinking_delta_text(&events), "tagged");
        assert_eq!(text_delta_text(&events), " answer");
    }

    #[test]
    fn reasoning_style_none_keeps_inline_tags_visible_text() {
        let events = decode_chunks_with_style(
            &[r#"{"choices":[{"delta":{"content":"<think>visible</think> answer"}}]}"#],
            ReasoningStreamStyle::None,
        );

        assert_eq!(thinking_delta_text(&events), "");
        assert_eq!(text_delta_text(&events), "<think>visible</think> answer");
    }

    #[test]
    fn configured_reasoning_style_overrides_route_default() {
        assert_eq!(
            reasoning_stream_style_for_stream(ApiProvider::Openai, "custom-minimax", None),
            ReasoningStreamStyle::None
        );
        assert_eq!(
            reasoning_stream_style_for_stream(
                ApiProvider::Openai,
                "custom-minimax",
                Some("inline-tags")
            ),
            ReasoningStreamStyle::InlineTags
        );
        assert_eq!(
            reasoning_stream_style_for_stream(ApiProvider::XiaomiMimo, "mimo-v2.5-pro", None),
            ReasoningStreamStyle::SeparateField
        );
        assert_eq!(
            reasoning_stream_style_for_stream(
                ApiProvider::XiaomiMimo,
                "mimo-v2.5-pro",
                Some("none")
            ),
            ReasoningStreamStyle::None
        );
    }

    #[test]
    fn decoder_yields_no_events_for_keepalive_chunk() {
        // DeepSeek often sends `{"choices":[]}` keepalive chunks before
        // emitting real content. The engine MUST treat a stream error after
        // these as "no content received" and be eligible for transparent
        // retry — assert here that the decoder yields no payload events.
        let events = decode_chunk(r#"{"choices":[]}"#);
        assert!(
            events.is_empty(),
            "empty-choices chunk must produce no events; got {events:?}"
        );
    }

    #[test]
    fn decoder_treats_done_frame_as_terminal() {
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let mut inline_reasoning_tags = InlineReasoningTagState::default();

        let outcome = parse_sse_data_frame(
            "  [DONE]  ",
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            &mut reasoning_detail_buffers,
            &mut inline_reasoning_tags,
            ReasoningStreamStyle::SeparateField,
        );

        assert!(
            matches!(outcome, SseDataFrame::Done),
            "`data: [DONE]` must terminate the stream instead of waiting for the HTTP connection to close"
        );
        assert_eq!(content_index, 0);
        assert!(!text_started);
        assert!(!thinking_started);
        assert!(tool_indices.is_empty());
    }

    #[test]
    fn decoder_finish_reason_is_a_terminal_signal() {
        let events = decode_chunk(r#"{"choices":[{"finish_reason":"stop"}]}"#);

        assert!(
            events.iter().any(stream_event_has_stop_reason),
            "finish_reason must make a stream terminal: {events:?}"
        );
    }

    #[test]
    fn decoder_partial_content_is_not_a_terminal_signal() {
        let events = decode_chunk(r#"{"choices":[{"delta":{"content":"partial"}}]}"#);

        assert!(
            !events.iter().any(stream_event_has_stop_reason),
            "content without finish_reason must not be accepted as a terminal: {events:?}"
        );
    }

    #[test]
    fn decoder_rejects_malformed_deepseek_sse_json() {
        let mut content_index = 0u32;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let mut reasoning_detail_buffers = std::collections::HashMap::new();
        let mut inline_reasoning_tags = InlineReasoningTagState::default();

        let outcome = parse_sse_data_frame(
            r#"{"choices":[BROKEN]}"#,
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            &mut reasoning_detail_buffers,
            &mut inline_reasoning_tags,
            ReasoningStreamStyle::SeparateField,
        );

        let SseDataFrame::Invalid(error) = outcome else {
            panic!("malformed provider data must not be silently discarded");
        };
        assert!(error.contains("invalid JSON data frame"), "{error}");
    }

    #[test]
    fn decoder_emits_tool_use_block_for_tool_call_delta() {
        // Tool-call deltas are content too — once one arrives, transparent
        // retry must be off (the model has committed to a tool invocation
        // path that DeepSeek has billed for).
        let events = decode_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"grep_files","arguments":"{\"pattern\":\"foo\"}"}}]}}]}"#,
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { name, .. },
                    ..
                } if name == "grep_files"
            )),
            "should open a ToolUse block for grep_files; got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: Delta::InputJsonDelta { partial_json },
                    ..
                } if partial_json.contains("\"pattern\"")
            )),
            "should yield InputJsonDelta carrying the tool args; got {events:?}"
        );
    }

    #[test]
    fn decoder_emits_an_explicit_empty_tool_argument_delta() {
        let events = decode_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_empty_args","function":{"name":"read_file","arguments":""}}]}}]}"#,
        );

        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: Delta::InputJsonDelta { partial_json },
                ..
            } if partial_json.is_empty()
        )));
    }

    #[test]
    fn decoder_uses_fallback_name_for_empty_streaming_tool_name() {
        let events = decode_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_empty","function":{"name":"","arguments":"{}"}}]}}]}"#,
        );

        assert!(
            events.iter().any(|event| matches!(
                event,
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { name, .. },
                    ..
                } if name == "unknown_tool"
            )),
            "empty upstream tool names should render as unknown_tool; got {events:?}"
        );
    }

    #[test]
    fn non_streaming_response_uses_fallback_name_for_missing_tool_name() {
        let payload: Value = serde_json::from_str(
            r#"{
                "id": "chatcmpl_1",
                "model": "deepseek-v4-pro",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_missing",
                            "function": { "arguments": "{}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .expect("valid response");

        let parsed = parse_chat_message(&payload).expect("message parses");
        let tool_name = parsed.content.iter().find_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        });

        assert_eq!(tool_name, Some("unknown_tool"));
    }

    #[test]
    fn non_streaming_response_preserves_exact_tool_argument_string() {
        let raw = "{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }";
        let payload = json!({
            "id": "chatcmpl_raw",
            "model": "deepseek-v4-pro",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_raw",
                        "function": { "name": "read_file", "arguments": raw }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_message(&payload).expect("message parses");
        let (input, raw_arguments) = parsed
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::ToolUse {
                    input,
                    raw_arguments,
                    ..
                } => Some((input, raw_arguments)),
                _ => None,
            })
            .expect("tool call present");
        assert_eq!(raw_arguments.as_deref(), Some(raw));
        assert_eq!(input, &json!({"z": 1, "path": "src/lib.rs", "a": 2}));
    }

    #[test]
    fn non_streaming_empty_tool_arguments_remain_explicit_and_unparsed() {
        let payload = json!({
            "id": "chatcmpl_empty",
            "model": "deepseek-v4-pro",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_empty",
                        "function": { "name": "read_file", "arguments": "" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_message(&payload).expect("message parses");
        let (input, raw_arguments) = parsed
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::ToolUse {
                    input,
                    raw_arguments,
                    ..
                } => Some((input, raw_arguments)),
                _ => None,
            })
            .expect("tool call present");
        assert_eq!(raw_arguments.as_deref(), Some(""));
        assert_eq!(input, &Value::String(String::new()));
    }

    #[test]
    fn non_streaming_malformed_tool_arguments_keep_raw_failure_input() {
        let payload = json!({
            "id": "chatcmpl_malformed",
            "model": "deepseek-v4-pro",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_bad",
                        "function": { "name": "read_file", "arguments": "{not-json" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_message(&payload).expect("message parses");
        let (input, raw_arguments) = parsed
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::ToolUse {
                    input,
                    raw_arguments,
                    ..
                } => Some((input, raw_arguments)),
                _ => None,
            })
            .expect("tool call present");
        assert_eq!(raw_arguments.as_deref(), Some("{not-json"));
        assert_eq!(input, &Value::String("{not-json".to_owned()));
    }

    /// Regression for the parallel-tool-calls-without-id collision (audit
    /// Finding 8): when the upstream chunk omits the `id` field, the
    /// fallback used to be the literal string `"tool_call"` for every
    /// parallel call, so two tool calls in one delta ended up sharing an
    /// id. Downstream routing then matched the first call's tool_result
    /// twice and the second call hung. The fallback is now indexed by the
    /// content-block position, keeping each call unique within the
    /// response.
    #[test]
    fn decoder_assigns_unique_fallback_ids_to_parallel_tool_calls_missing_id() {
        let events = decode_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[
                {"index":0,"function":{"name":"grep_files","arguments":"{\"pattern\":\"a\"}"}},
                {"index":1,"function":{"name":"read_file","arguments":"{\"path\":\"x\"}"}}
            ]}}]}"#,
        );

        let ids: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { id, .. },
                    ..
                } => Some(id.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            ids.len(),
            2,
            "expected two tool-use blocks for parallel tool calls; got {events:?}"
        );
        assert_ne!(
            ids[0], ids[1],
            "parallel tool calls without upstream `id` must get distinct fallback ids; got {ids:?}"
        );
    }

    #[test]
    fn decoder_preserves_upstream_tool_call_id_when_present() {
        // Counter-test to the fallback regression: when the upstream chunk
        // does include `id`, we forward it verbatim — we shouldn't quietly
        // rewrite ids the API gave us just because we have a fallback path.
        let events = decode_chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_xyz","function":{"name":"grep_files","arguments":"{}"}}]}}]}"#,
        );
        let id = events
            .iter()
            .find_map(|e| match e {
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { id, .. },
                    ..
                } => Some(id.as_str()),
                _ => None,
            })
            .expect("tool-use block present");
        assert_eq!(id, "call_xyz");
    }

    #[test]
    fn request_builder_preserves_internal_system_messages() {
        let messages = vec![Message {
            role: "system".to_string(),
            content: vec![ContentBlock::Text {
                text: "internal runtime event".to_string(),
                cache_control: None,
            }],
        }];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");

        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "system");
        assert_eq!(built[0]["content"], "internal runtime event");
    }

    fn tool_use_message(id: &str, name: &str, input: Value) -> Message {
        Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input,
                raw_arguments: None,
                caller: None,
            }],
        }
    }

    fn tool_result_message(id: &str, content: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: content.to_string(),
                is_error: None,
                content_blocks: None,
            }],
        }
    }

    fn user_message_with_turn_meta(turn_meta: &str, task: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: turn_meta.to_string(),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: task.to_string(),
                    cache_control: None,
                },
            ],
        }
    }

    fn user_message_with_tail_turn_meta(task: &str, turn_meta: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: task.to_string(),
                    cache_control: None,
                },
                ContentBlock::Text {
                    text: turn_meta.to_string(),
                    cache_control: None,
                },
            ],
        }
    }

    fn tool_message_content(messages: &[Value], index: usize) -> &str {
        messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
            .nth(index)
            .and_then(|message| message.get("content").and_then(Value::as_str))
            .expect("tool message content")
    }

    fn user_message_content(messages: &[Value], index: usize) -> &str {
        messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .nth(index)
            .and_then(|message| message.get("content").and_then(Value::as_str))
            .expect("user message content")
    }

    fn with_tool_result_sha_spillover_root<T>(f: impl FnOnce() -> T) -> T {
        let _guard = crate::tools::truncate::TEST_SPILLOVER_GUARD
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prior = crate::tools::truncate::set_test_spillover_root(Some(
            tmp.path().join(".deepseek").join("tool_outputs"),
        ));
        struct Restore(Option<std::path::PathBuf>);
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::tools::truncate::set_test_spillover_root(self.0.take());
            }
        }
        let _restore = Restore(prior);
        f()
    }

    #[test]
    fn request_builder_deduplicates_consecutive_identical_turn_meta_for_wire() {
        let turn_meta = "<turn_meta>\nCurrent local date: 2026-05-09\n</turn_meta>";
        let messages = vec![
            user_message_with_turn_meta(turn_meta, "first task"),
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "first answer".to_string(),
                    cache_control: None,
                }],
            },
            user_message_with_turn_meta(turn_meta, "second task"),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let first = user_message_content(&built, 0);
        let second = user_message_content(&built, 1);
        let expected_ref = "<turn_meta_unchanged />";

        assert!(first.starts_with(turn_meta), "got: {first}");
        assert!(second.starts_with(expected_ref), "got: {second}");
        assert!(second.ends_with("second task"), "got: {second}");
        assert_eq!(
            second,
            format!("{expected_ref}\nsecond task"),
            "ref text must stay stable"
        );
    }

    #[test]
    fn request_builder_keeps_tail_turn_meta_after_user_text_for_wire() {
        let turn_meta = "<turn_meta>\nCurrent local date: 2026-05-09\n</turn_meta>";
        let messages = vec![
            user_message_with_tail_turn_meta("first task", turn_meta),
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "first answer".to_string(),
                    cache_control: None,
                }],
            },
            user_message_with_tail_turn_meta("second task", turn_meta),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let first = user_message_content(&built, 0);
        let second = user_message_content(&built, 1);
        let expected_ref = "<turn_meta_unchanged />";

        assert_eq!(first, format!("first task\n{turn_meta}"));
        assert_eq!(second, format!("second task\n{expected_ref}"));
    }

    #[test]
    fn request_builder_keeps_changed_turn_meta_full_and_updates_recent_hash() {
        let first_meta = "<turn_meta>\nCurrent local date: 2026-05-09\n</turn_meta>";
        let second_meta =
            "<turn_meta>\nCurrent local date: 2026-05-09\nWorking set: src/lib.rs\n</turn_meta>";
        let messages = vec![
            user_message_with_turn_meta(first_meta, "first task"),
            user_message_with_turn_meta(second_meta, "second task"),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let first = user_message_content(&built, 0);
        let second = user_message_content(&built, 1);

        assert!(first.starts_with(first_meta), "got: {first}");
        assert!(second.starts_with(second_meta), "got: {second}");
        assert!(!second.contains("<TURN_META_REF"), "got: {second}");
    }

    #[test]
    fn turn_meta_dedup_is_wire_only_and_does_not_mutate_session_message() {
        let turn_meta = "<turn_meta>\nCurrent local date: 2026-05-09\n</turn_meta>";
        let messages = vec![
            user_message_with_turn_meta(turn_meta, "first task"),
            user_message_with_turn_meta(turn_meta, "second task"),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        assert!(
            user_message_content(&built, 1).starts_with("<turn_meta_unchanged />"),
            "got: {}",
            user_message_content(&built, 1)
        );

        match &messages[1].content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, turn_meta),
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn request_builder_truncates_large_tool_result_for_wire() {
        let long_output = format!("{}{}", "A".repeat(7_000), "Z".repeat(7_000));
        let messages = vec![
            tool_use_message(
                "tool-long",
                "shell_command",
                json!({"command": "cargo test"}),
            ),
            tool_result_message("tool-long", &long_output),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let sent = tool_message_content(&built, 0);

        assert!(sent.contains("[TOOL_RESULT_TRUNCATED]"), "got: {sent}");
        assert!(sent.contains("tool_name: shell_command"), "got: {sent}");
        assert!(sent.contains("command_or_query: cargo test"), "got: {sent}");
        assert!(sent.contains("original_chars: 14000"), "got: {sent}");
        assert!(sent.contains("sha256:"), "got: {sent}");
        assert!(
            sent.contains("retrieve: retrieve_tool_result ref=sha:"),
            "got: {sent}"
        );
        assert!(sent.contains(&"A".repeat(4_000)), "got: {sent}");
        assert!(sent.contains(&"Z".repeat(4_000)), "got: {sent}");
        assert!(
            sent.contains("truncated 6000 chars from middle"),
            "got: {sent}"
        );
        assert_ne!(sent, long_output);
    }

    #[test]
    fn request_builder_keeps_extreme_tool_output_bounded_and_retrievable() {
        with_tool_result_sha_spillover_root(|| {
            let huge_output = format!(
                "{}{}{}",
                "DIFF_HEAD\n".repeat(10_000),
                "MIDDLE_POISON\n".repeat(10_000),
                "DIFF_TAIL\n".repeat(10_000)
            );
            let sha = sha256_hex(huge_output.as_bytes());
            let messages = vec![
                tool_use_message("tool-huge", "exec_shell", json!({"command": "git diff"})),
                tool_result_message("tool-huge", &huge_output),
            ];

            let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
            let sent = tool_message_content(&built, 0);

            assert!(sent.contains("[TOOL_RESULT_TRUNCATED]"), "got: {sent}");
            assert!(sent.contains("tool_name: exec_shell"), "got: {sent}");
            assert!(sent.contains("command_or_query: git diff"), "got: {sent}");
            assert!(sent.contains(&format!("sha256: {sha}")), "got: {sent}");
            assert!(
                sent.contains(&format!("retrieve: retrieve_tool_result ref=sha:{sha}")),
                "got: {sent}"
            );
            assert!(
                sent.chars().count() <= TOOL_RESULT_SENT_CHAR_BUDGET,
                "truncated result should stay bounded, sent {} chars",
                sent.chars().count()
            );
            assert!(
                !sent.contains("MIDDLE_POISON"),
                "omitted middle should not be sent to the next model turn"
            );
            assert_ne!(sent, huge_output);
        });
    }

    #[test]
    fn request_builder_does_not_dedup_short_tool_results_for_wire() {
        let output = "same tool output";
        let messages = vec![
            tool_use_message("tool-1", "read_file", json!({"path": "README.md"})),
            tool_result_message("tool-1", output),
            tool_use_message("tool-2", "read_file", json!({"path": "README.md"})),
            tool_result_message("tool-2", output),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let first = tool_message_content(&built, 0);
        let second = tool_message_content(&built, 1);

        assert_eq!(first, output);
        assert_eq!(second, output);
        assert!(!second.contains("<TOOL_RESULT_REF"), "got: {second}");
    }

    #[test]
    fn request_builder_deduplicates_medium_identical_tool_results_with_retrieval_hint() {
        with_tool_result_sha_spillover_root(|| {
            // 2,000 chars is intentionally above TOOL_RESULT_DEDUP_MIN_CHARS
            // (1,024) but below TOOL_RESULT_SENT_CHAR_BUDGET (12,000). This
            // verifies the cache-saving path for repeated medium outputs that
            // do not otherwise need truncation.
            let output = "A".repeat(2_000);
            let messages = vec![
                tool_use_message("tool-1", "read_file", json!({"path": "README.md"})),
                tool_result_message("tool-1", &output),
                tool_use_message("tool-2", "read_file", json!({"path": "README.md"})),
                tool_result_message("tool-2", &output),
            ];

            let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
            let first = tool_message_content(&built, 0);
            let second = tool_message_content(&built, 1);

            assert_eq!(first, output);
            assert!(!first.contains("[TOOL_RESULT_TRUNCATED]"), "got: {first}");
            assert!(
                second.starts_with("<TOOL_RESULT_REF sha=\""),
                "got: {second}"
            );
            assert!(
                second.contains("original_message=\"Message #1\""),
                "got: {second}"
            );
            assert!(second.contains("chars=\"2000\""), "got: {second}");
            assert!(
                second.contains("retrieve: retrieve_tool_result ref=sha:"),
                "got: {second}"
            );
        });
    }

    #[test]
    fn request_builder_never_dedups_large_identical_write_file_confirmations() {
        with_tool_result_sha_spillover_root(|| {
            // A `write_file` result embeds the unified diff + summary; it is a
            // confirmation, not retrievable data. Two identical >1024-char
            // write_file results must BOTH stay inline — collapsing the second
            // to a SHA ref makes the model lose write-success context and
            // report the file as missing (#1695).
            let output = "A".repeat(2_000);
            let messages = vec![
                tool_use_message("tool-1", "write_file", json!({"path": "big.txt"})),
                tool_result_message("tool-1", &output),
                tool_use_message("tool-2", "write_file", json!({"path": "big.txt"})),
                tool_result_message("tool-2", &output),
            ];

            let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
            let first = tool_message_content(&built, 0);
            let second = tool_message_content(&built, 1);

            assert_eq!(first, output);
            assert_eq!(second, output);
            assert!(!second.contains("<TOOL_RESULT_REF"), "got: {second}");

            // Non-mutation tools still dedup: an identical large read_file
            // result collapses to a retrievable SHA ref.
            let read_messages = vec![
                tool_use_message("read-1", "read_file", json!({"path": "README.md"})),
                tool_result_message("read-1", &output),
                tool_use_message("read-2", "read_file", json!({"path": "README.md"})),
                tool_result_message("read-2", &output),
            ];
            let read_built = build_chat_messages(None, &read_messages, "deepseek-v4-flash");
            let read_first = tool_message_content(&read_built, 0);
            let read_second = tool_message_content(&read_built, 1);
            assert_eq!(read_first, output);
            assert!(
                read_second.starts_with("<TOOL_RESULT_REF sha=\""),
                "got: {read_second}"
            );
        });
    }

    #[test]
    fn large_write_file_result_stays_inline_but_is_persisted_for_retrieval() {
        // Decoupling regression (#1695 follow-up): a SINGLE very large
        // `write_file` result must (a) never collapse to a
        // `<TOOL_RESULT_REF>` (mutation confirmations stay inline) yet
        // (b) still be persisted to the SHA store so the content elided
        // by truncation remains retrievable via `retrieve_tool_result`.
        // Before the fix, folding `!is_mutation_tool` into the single
        // `dedup_eligible` gate also disabled persistence, so a >12k
        // mutation diff was truncated AND unrecoverable.
        let _guard = crate::tools::truncate::TEST_SPILLOVER_GUARD
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prior = crate::tools::truncate::set_test_spillover_root(Some(
            tmp.path().join(".deepseek").join("tool_outputs"),
        ));
        struct Restore(Option<std::path::PathBuf>);
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::tools::truncate::set_test_spillover_root(self.0.take());
            }
        }
        let _restore = Restore(prior);

        // > TOOL_RESULT_SENT_CHAR_BUDGET (12_000) so the wire path
        // truncates and would need a SHA to recover the middle.
        let big_diff = "D".repeat(20_000);
        let sha = sha256_hex(big_diff.as_bytes());

        let messages = vec![
            tool_use_message("w-1", "write_file", json!({"path": "huge.rs"})),
            tool_result_message("w-1", &big_diff),
            tool_use_message("w-2", "write_file", json!({"path": "huge.rs"})),
            tool_result_message("w-2", &big_diff),
        ];
        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let first = tool_message_content(&built, 0);
        let second = tool_message_content(&built, 1);

        // (a) Both confirmations stay inline — truncated, never a ref.
        assert!(
            first.contains("[TOOL_RESULT_TRUNCATED]"),
            "first should be truncated, got: {first}"
        );
        assert!(
            !first.contains("<TOOL_RESULT_REF"),
            "first must not be a dedup ref, got: {first}"
        );
        assert!(
            !second.contains("<TOOL_RESULT_REF"),
            "second identical write_file must stay inline (#1695), got: {second}"
        );
        assert!(
            second.contains("[TOOL_RESULT_TRUNCATED]"),
            "second should also be inline-truncated, got: {second}"
        );
        assert!(
            first.contains(&format!("sha256: {sha}")),
            "truncation block should advertise the recovery SHA, got: {first}"
        );
        assert!(
            first.contains(&format!("retrieve: retrieve_tool_result ref=sha:{sha}")),
            "truncation block should advertise the recovery command, got: {first}"
        );

        // (b) The full content was persisted to the SHA store and is
        // retrievable — the seam `persist_tool_result_for_sha` writes
        // to and `retrieve_tool_result ref=sha:` reads back from.
        let path = crate::tools::truncate::sha_spillover_path(&sha)
            .expect("sha spillover path resolvable under test root");
        assert!(
            path.exists(),
            "large write_file output not persisted: {path:?}"
        );
        let persisted = std::fs::read_to_string(&path).expect("read persisted spillover");
        assert_eq!(
            persisted, big_diff,
            "persisted content must match the original write_file result verbatim"
        );

        // Sanity: a large NON-mutation result still dedups (back-ref on
        // the second sighting) — decoupling didn't regress #1695's
        // preserved read-path behavior.
        let read_messages = vec![
            tool_use_message("r-1", "read_file", json!({"path": "huge.rs"})),
            tool_result_message("r-1", &big_diff),
            tool_use_message("r-2", "read_file", json!({"path": "huge.rs"})),
            tool_result_message("r-2", &big_diff),
        ];
        let read_built = build_chat_messages(None, &read_messages, "deepseek-v4-flash");
        let read_second = tool_message_content(&read_built, 1);
        assert!(
            read_second.starts_with("<TOOL_RESULT_REF sha=\""),
            "large read_file must still dedup to a ref, got: {read_second}"
        );
    }

    #[test]
    fn tool_result_budget_is_wire_only_and_does_not_mutate_session_message() {
        let long_output = format!("{}{}", "A".repeat(7_000), "Z".repeat(7_000));
        let messages = vec![
            tool_use_message(
                "tool-long",
                "shell_command",
                json!({"command": "cargo test"}),
            ),
            tool_result_message("tool-long", &long_output),
        ];

        let built = build_chat_messages(None, &messages, "deepseek-v4-flash");
        let sent = tool_message_content(&built, 0);
        assert_ne!(sent, long_output);

        match &messages[1].content[0] {
            ContentBlock::ToolResult { content, .. } => assert_eq!(content, &long_output),
            other => panic!("expected tool result, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod deepseek_tool_reasoning_replay_tests {
    use super::{
        build_chat_messages_for_request_and_provider, reasoning_replay_tokens_for_messages,
    };
    use crate::config::ApiProvider;
    use crate::models::{ContentBlock, Message, MessageRequest};
    use serde_json::{Value, json};

    fn request_with_history(assistant_content: Vec<ContentBlock>) -> MessageRequest {
        MessageRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![
                Message {
                    role: "assistant".to_string(),
                    content: assistant_content,
                },
                Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call-1".to_string(),
                        content: "tests passed".to_string(),
                        is_error: None,
                        content_blocks: None,
                    }],
                },
            ],
            max_tokens: 128,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            // The earlier tool round was produced with thinking enabled; the
            // next completion intentionally disables new thinking.
            reasoning_effort: Some("off".to_string()),
            stream: None,
            temperature: None,
            top_p: None,
        }
    }

    fn tool_call() -> ContentBlock {
        ContentBlock::ToolUse {
            id: "call-1".to_string(),
            name: "shell_command".to_string(),
            input: json!({"command": "cargo test"}),
            raw_arguments: None,
            caller: None,
        }
    }

    fn assistant_message(messages: &[Value]) -> &Value {
        messages
            .iter()
            .find(|message| message["role"] == "assistant")
            .expect("assistant history message")
    }

    #[test]
    fn switching_thinking_off_preserves_tool_call_reasoning_history() {
        let request = request_with_history(vec![
            ContentBlock::Thinking {
                thinking: "Run the targeted tests before claiming success.".to_string(),
                signature: None,
            },
            tool_call(),
        ]);

        let messages =
            build_chat_messages_for_request_and_provider(&request, ApiProvider::Deepseek);
        let assistant = assistant_message(&messages);

        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("Run the targeted tests before claiming success.")
        );
        assert!(assistant.get("tool_calls").is_some());
    }

    #[test]
    fn missing_tool_round_reasoning_is_never_fabricated() {
        let request = request_with_history(vec![tool_call()]);

        let messages =
            build_chat_messages_for_request_and_provider(&request, ApiProvider::Deepseek);
        let assistant = assistant_message(&messages);

        assert!(
            assistant.get("reasoning_content").is_none(),
            "message projection must not invent canonical reasoning history"
        );

        // Replay accounting must likewise remain read-only for restored or
        // cached payloads that bypass the normal message builder.
        let body = json!({
            "messages": [{
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": {"name": "shell_command", "arguments": "{}"}
                }]
            }]
        });
        let replay_tokens = reasoning_replay_tokens_for_messages(
            &body,
            "deepseek-v4-pro",
            Some("off"),
            ApiProvider::Deepseek,
        );
        assert_eq!(replay_tokens, None);
        assert!(
            body.pointer("/messages/0/reasoning_content").is_none(),
            "replay accounting must not mutate history"
        );
    }

    #[test]
    fn thinking_off_does_not_replay_reasoning_for_plain_assistant_history() {
        let mut request = request_with_history(vec![
            ContentBlock::Thinking {
                thinking: "Private reasoning from the prior answer.".to_string(),
                signature: None,
            },
            ContentBlock::Text {
                text: "The prior answer.".to_string(),
                cache_control: None,
            },
        ]);
        request.messages.truncate(1);

        let messages =
            build_chat_messages_for_request_and_provider(&request, ApiProvider::Deepseek);
        let assistant = assistant_message(&messages);

        assert_eq!(
            assistant.get("content").and_then(Value::as_str),
            Some("The prior answer.")
        );
        assert!(
            assistant.get("reasoning_content").is_none(),
            "plain assistant history must not gain reasoning_content when thinking is off"
        );
        assert!(assistant.get("tool_calls").is_none());
    }
}

#[cfg(test)]
mod alias_thinking_detection_tests {
    //! Regression coverage for the DeepSeek public model aliases.
    //!
    //! `deepseek-chat` and `deepseek-reasoner` are the canonical alias names
    //! published in DeepSeek's API docs. Server-side they resolve to V4-flash
    //! and V4-pro respectively, both of which have thinking mode enabled by
    //! default. If the TUI does not classify those aliases as reasoning
    //! models, projection skips replaying `reasoning_content` on tool-call
    //! assistant messages and DeepSeek returns a 400 ("the `reasoning_content`
    //! in the thinking mode must be passed back to the API") on the second
    //! turn. See upstream API docs:
    //! https://api-docs.deepseek.com/guides/thinking_mode
    use super::{
        apply_openai_reasoning_effort, apply_provider_token_limit, is_reasoning_model_for_stream,
        provider_accepts_reasoning_content, requires_reasoning_content,
        requires_tool_call_reasoning_replay, should_replay_reasoning_content,
        should_replay_reasoning_content_for_provider,
    };
    use crate::config::ApiProvider;
    use serde_json::json;

    #[test]
    fn aliases_routed_to_v4_require_reasoning_content() {
        // Documented public aliases.
        assert!(requires_reasoning_content("deepseek-chat"));
        assert!(requires_reasoning_content("deepseek-reasoner"));
        // Case-insensitive: users sometimes copy/paste with capitalisation.
        assert!(requires_reasoning_content("DeepSeek-Chat"));
        assert!(requires_reasoning_content("DEEPSEEK-REASONER"));
    }

    #[test]
    fn explicit_v4_ids_still_require_reasoning_content() {
        // Direct V4 IDs continue to match (regression guard for the existing
        // `lower.contains("deepseek-v4")` branch).
        assert!(requires_reasoning_content("deepseek-v4-flash"));
        assert!(requires_reasoning_content("deepseek-v4-pro"));
    }

    #[test]
    fn non_thinking_aliases_remain_excluded() {
        // Legacy non-thinking IDs and unrelated provider models must not be
        // misclassified, otherwise we would emit a provider-specific
        // `reasoning_content` field on providers that reject it.
        assert!(!requires_reasoning_content("deepseek-v3"));
        assert!(!requires_reasoning_content("deepseek-coder"));
        assert!(!requires_reasoning_content("qwen3-coder"));
        assert!(!requires_reasoning_content("claude-sonnet-4-6"));
    }

    #[test]
    fn alias_prefix_handles_suffixed_variants() {
        // OpenRouter / proxy deployments occasionally suffix the canonical
        // alias (e.g. `deepseek-chat:free`). Those routes still hit V4
        // server-side, so they must continue to require reasoning_content.
        assert!(requires_reasoning_content("deepseek-chat:free"));
        assert!(requires_reasoning_content("deepseek-reasoner-2025-05"));
    }

    #[test]
    fn explicit_reasoning_off_disables_plain_history_replay_only() {
        // `reasoning_effort = "off"` disables newly generated thinking and
        // avoids replaying reasoning on plain assistant history. It does not
        // relax DeepSeek's protocol requirement for historical tool rounds.
        assert!(!should_replay_reasoning_content(
            "deepseek-chat",
            Some("off")
        ));
        assert!(!should_replay_reasoning_content(
            "deepseek-reasoner",
            Some("disabled")
        ));
        assert!(requires_tool_call_reasoning_replay("deepseek-chat"));
        assert!(requires_tool_call_reasoning_replay("deepseek-reasoner"));
        // Without an explicit override, alias models still trigger replay.
        assert!(should_replay_reasoning_content("deepseek-chat", None));
        assert!(should_replay_reasoning_content(
            "deepseek-reasoner",
            Some("medium")
        ));
    }

    #[test]
    fn generic_openai_provider_does_not_accept_reasoning_content_semantics() {
        assert!(!provider_accepts_reasoning_content(ApiProvider::Openai));
        assert!(provider_accepts_reasoning_content(ApiProvider::Deepseek));
        assert!(provider_accepts_reasoning_content(ApiProvider::NvidiaNim));
        assert!(provider_accepts_reasoning_content(ApiProvider::XiaomiMimo));
        assert!(provider_accepts_reasoning_content(ApiProvider::Arcee));
        assert!(provider_accepts_reasoning_content(ApiProvider::Minimax));
        assert!(provider_accepts_reasoning_content(ApiProvider::Zai));
        // #3016: Moonshot's native endpoint streams Kimi thinking as
        // reasoning_content.
        assert!(provider_accepts_reasoning_content(ApiProvider::Moonshot));
    }

    #[test]
    fn stream_classifies_moonshot_kimi_as_reasoning() {
        // #3016: without this, Kimi thinking leaked into answer text.
        assert!(is_reasoning_model_for_stream(
            ApiProvider::Moonshot,
            "kimi-k2.6"
        ));
        assert!(
            is_reasoning_model_for_stream(ApiProvider::Moonshot, "kimi-for-coding"),
            "Kimi Code's stable model id now maps to K2.7 Code and streams reasoning_content"
        );
    }

    #[test]
    fn moonshot_and_minimax_replay_reasoning_content_for_supported_models() {
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Moonshot,
            "kimi-k2.7-code",
            None,
        ));
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Moonshot,
            "kimi-for-coding",
            None,
        ));
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Minimax,
            "MiniMax-M3",
            None,
        ));
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Zai,
            "GLM-5.2",
            None,
        ));
        assert!(!should_replay_reasoning_content_for_provider(
            ApiProvider::Moonshot,
            "kimi-for-coding",
            Some("off"),
        ));
    }

    #[test]
    fn xiaomi_mimo_uses_max_completion_tokens_payload_key() {
        let mut body = json!({
            "model": "mimo-v2.5-pro",
            "messages": [],
            "max_tokens": 8192,
        });

        apply_provider_token_limit(&mut body, ApiProvider::XiaomiMimo, "mimo-v2.5-pro", 8192);

        assert!(body.get("max_tokens").is_none());
        assert_eq!(
            body.get("max_completion_tokens")
                .and_then(serde_json::Value::as_u64),
            Some(8192)
        );
    }

    #[test]
    fn openai_reasoning_model_uses_completion_token_limit_and_effort_field() {
        let mut body = json!({
            "model": "gpt-5.5",
            "messages": [],
            "max_tokens": 4096,
        });

        apply_provider_token_limit(&mut body, ApiProvider::Openai, "gpt-5.5", 4096);
        apply_openai_reasoning_effort(&mut body, ApiProvider::Openai, "gpt-5.5", Some("high"));

        assert!(body.get("max_tokens").is_none());
        assert_eq!(
            body.get("max_completion_tokens")
                .and_then(serde_json::Value::as_u64),
            Some(4096)
        );
        assert_eq!(
            body.get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
    }

    #[test]
    fn gpt_56_uses_documented_max_reasoning_effort() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "messages": [],
            "max_tokens": 8192,
        });

        apply_provider_token_limit(&mut body, ApiProvider::Openai, "gpt-5.6-sol", 8192);
        apply_openai_reasoning_effort(&mut body, ApiProvider::Openai, "gpt-5.6-sol", Some("max"));

        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["max_completion_tokens"], json!(8192));
        assert_eq!(body["reasoning_effort"], json!("max"));
    }

    #[test]
    fn muse_spark_uses_meta_reasoning_effort_without_openai_token_rewrite() {
        let mut body = json!({
            "model": "muse-spark-1.1",
            "messages": [],
            "max_tokens": 8192,
        });

        apply_provider_token_limit(&mut body, ApiProvider::Meta, "muse-spark-1.1", 8192);
        apply_openai_reasoning_effort(&mut body, ApiProvider::Meta, "muse-spark-1.1", Some("max"));

        assert_eq!(body["max_tokens"], json!(8192));
        assert!(body.get("max_completion_tokens").is_none());
        assert_eq!(body["reasoning_effort"], json!("xhigh"));
    }

    #[test]
    fn openai_non_reasoning_model_omits_reasoning_only_fields() {
        let mut body = json!({
            "model": "gpt-4o",
            "messages": [],
            "max_tokens": 4096,
        });

        apply_provider_token_limit(&mut body, ApiProvider::Openai, "gpt-4o", 4096);
        apply_openai_reasoning_effort(&mut body, ApiProvider::Openai, "gpt-4o", Some("high"));

        assert_eq!(
            body.get("max_tokens").and_then(serde_json::Value::as_u64),
            Some(4096)
        );
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openai_provider_deepseek_compatible_model_keeps_chat_token_field() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [],
            "max_tokens": 4096,
        });

        apply_provider_token_limit(&mut body, ApiProvider::Openai, "deepseek-v4-pro", 4096);
        apply_openai_reasoning_effort(
            &mut body,
            ApiProvider::Openai,
            "deepseek-v4-pro",
            Some("high"),
        );

        assert_eq!(
            body.get("max_tokens").and_then(serde_json::Value::as_u64),
            Some(4096)
        );
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn deepseek_model_on_openai_provider_still_replays_reasoning_content() {
        // #1739 / #1694: a DeepSeek thinking model pointed at a
        // DeepSeek-compatible endpoint via the generic `openai` provider must
        // still replay reasoning_content, even though the provider itself does
        // not accept the field. Otherwise the thinking-mode API returns 400.
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "deepseek-v4-flash",
            None,
        ));
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "deepseek-v4-pro",
            None,
        ));
        assert!(should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "deepseek-reasoner",
            Some("medium"),
        ));
        // The documented escape hatch still wins over model detection.
        assert!(!should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "deepseek-v4-flash",
            Some("off"),
        ));
    }

    #[test]
    fn generic_model_on_openai_provider_still_strips_reasoning_content() {
        // #1542 no-regression guard: a genuine non-DeepSeek model on the
        // openai provider must continue to have reasoning_content stripped.
        assert!(!should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "qwen3-coder",
            None,
        ));
        assert!(!should_replay_reasoning_content_for_provider(
            ApiProvider::Openai,
            "claude-sonnet-4-6",
            None,
        ));
    }

    #[test]
    fn stream_classifies_deepseek_model_on_openai_provider_as_reasoning() {
        // #1739: the SSE parser must treat a DeepSeek thinking model on the
        // generic `openai` provider (DeepSeek-compatible endpoint) as a
        // reasoning model, or incoming `reasoning_content` tokens are stored
        // as answer text and the subsequent replay still 400s.
        assert!(is_reasoning_model_for_stream(
            ApiProvider::Openai,
            "deepseek-v4-flash"
        ));
        assert!(is_reasoning_model_for_stream(
            ApiProvider::Openai,
            "deepseek-v4-pro"
        ));
        assert!(is_reasoning_model_for_stream(
            ApiProvider::Openai,
            "deepseek-reasoner"
        ));
        // Native DeepSeek provider was already correct; stays correct.
        assert!(is_reasoning_model_for_stream(
            ApiProvider::Deepseek,
            "deepseek-v4-pro"
        ));
    }

    #[test]
    fn stream_classifies_known_large_reasoning_models_as_reasoning() {
        // Xiaomi MiMo and OpenRouter/Qwen/Trinity can stream private reasoning through a
        // `reasoning` delta without using a DeepSeek-looking model name. The
        // renderer must still route that field into Thinking cells instead
        // of plain assistant prose.
        assert!(
            is_reasoning_model_for_stream(ApiProvider::XiaomiMimo, "mimo-v2.5-pro"),
            "mimo-v2.5-pro should stream reasoning as thinking on Xiaomi MiMo"
        );
        assert!(
            is_reasoning_model_for_stream(ApiProvider::Arcee, "trinity-large-thinking"),
            "trinity-large-thinking should stream reasoning as thinking on direct Arcee"
        );
        assert!(
            is_reasoning_model_for_stream(ApiProvider::Zai, "GLM-5.2"),
            "GLM-5.2 should stream reasoning_content as thinking on direct Z.ai"
        );
        for model in [
            "arcee-ai/trinity-large-thinking",
            "minimax/minimax-m3",
            "xiaomi/mimo-v2.5-pro",
        ] {
            assert!(
                is_reasoning_model_for_stream(ApiProvider::Openrouter, model),
                "{model} should stream reasoning as thinking on OpenRouter"
            );
        }
    }

    #[test]
    fn stream_does_not_classify_generic_model_as_reasoning() {
        // #1542 no-regression guard: a genuine non-DeepSeek model on the
        // openai provider must NOT be treated as a reasoning model, so the
        // parser keeps inlining any `reasoning_content` it emits as text.
        assert!(!is_reasoning_model_for_stream(
            ApiProvider::Openai,
            "qwen3-coder"
        ));
        assert!(!is_reasoning_model_for_stream(
            ApiProvider::Openai,
            "claude-sonnet-4-6"
        ));
        // Non-DeepSeek model on a reasoning-aware provider is also unchanged.
        assert!(!is_reasoning_model_for_stream(
            ApiProvider::Deepseek,
            "qwen3-coder"
        ));
    }

    #[test]
    fn stream_classification_matches_replay_predicate() {
        // The streaming classifier and the replay predicate must agree on
        // model identity, or stream parsing and message sanitisation disagree
        // about where reasoning tokens live. Effort=None isolates the
        // model/provider dimension shared by both.
        for model in ["deepseek-v4-pro", "deepseek-reasoner", "qwen3-coder"] {
            for provider in [ApiProvider::Openai, ApiProvider::Deepseek] {
                assert_eq!(
                    is_reasoning_model_for_stream(provider, model),
                    should_replay_reasoning_content_for_provider(provider, model, None),
                    "stream vs replay disagree for {model} on {provider:?}"
                );
            }
        }
    }
}
