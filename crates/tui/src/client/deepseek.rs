//! Pure request planning for the official DeepSeek OpenAI-format API.

use std::time::Duration;

pub(crate) use codewhale_deepseek::{
    ApiSurface, ChatPlanError, DeepSeekEndpoint, DeepSeekResponse, DeepSeekTransport,
    DeepSeekTransportConfig, FimPlanError, RequestPlan, ResponseMode, TransportRetryPolicy,
};
use codewhale_deepseek::{
    ChatPlanInput, DeepSeekCredential, PlannedTool, ReasoningMode, RuntimeChatPlanInput,
};
use codewhale_runtime::{ModelFinishReason, ModelRequest, ModelStreamEvent};
use futures_util::StreamExt;

use crate::config::{ApiProvider, wire_model_for_provider};
use crate::llm_client::StreamEventBox;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, MessageDelta, MessageRequest, MessageResponse,
    StreamEvent, Tool, Usage,
};

use super::DeepSeekClient;

impl DeepSeekClient {
    pub(crate) fn official_deepseek_transport(&self) -> anyhow::Result<DeepSeekTransport> {
        let endpoint = if codewhale_deepseek::official_root(&self.base_url).is_some() {
            DeepSeekEndpoint::Official
        } else {
            DeepSeekEndpoint::loopback_fixture(super::versioned_base_url(&self.base_url))?
        };
        let retry = TransportRetryPolicy {
            max_retries: if self.retry.enabled {
                self.retry.max_retries
            } else {
                0
            },
            initial_delay: Duration::from_secs_f64(self.retry.initial_delay.clamp(0.0, 300.0)),
            max_delay: Duration::from_secs_f64(self.retry.max_delay.clamp(0.0, 300.0)),
            exponential_base: self.retry.exponential_base,
        };
        let config = DeepSeekTransportConfig {
            endpoint,
            credential: DeepSeekCredential::new(self.api_key.clone())?,
            strict_tools: self.strict_tool_mode,
            response_header_timeout: self.stream_open_timeout,
            stream_idle_timeout: self.stream_idle_timeout,
            retry,
            request_budget: self
                .api_request_budget
                .clone()
                .unwrap_or_else(codewhale_deepseek::SharedApiRequestBudget::tracking_only),
        };
        DeepSeekTransport::new(self.http_client.clone(), config).map_err(Into::into)
    }
}

pub(crate) fn message_response_from_deepseek(response: DeepSeekResponse) -> MessageResponse {
    let output = response.output;
    let mut content = Vec::new();
    if let Some(reasoning) = output.reasoning_content.filter(|value| !value.is_empty()) {
        content.push(ContentBlock::Thinking {
            signature: None,
            thinking: reasoning,
        });
    }
    if !output.content.is_empty() {
        content.push(ContentBlock::Text {
            text: output.content,
            cache_control: None,
        });
    }
    for tool_call in output.tool_calls {
        content.push(ContentBlock::ToolUse {
            id: tool_call.id,
            name: tool_call.name,
            input: tool_call
                .arguments
                .parsed
                .unwrap_or_else(|| serde_json::Value::String(tool_call.arguments.raw.clone())),
            raw_arguments: Some(tool_call.arguments.raw),
            caller: None,
        });
    }
    MessageResponse {
        id: response.id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: response.model,
        stop_reason: Some(finish_reason_name(output.finish_reason).to_string()),
        stop_sequence: None,
        container: None,
        usage: tui_usage(output.usage),
    }
}

pub(crate) fn tui_stream_from_deepseek(
    mut source: codewhale_deepseek::DeepSeekStream,
    model: String,
) -> StreamEventBox {
    Box::pin(async_stream::stream! {
        yield Ok(StreamEvent::MessageStart {
            message: MessageResponse {
                id: String::new(),
                r#type: "message".to_string(),
                role: "assistant".to_string(),
                content: Vec::new(),
                model,
                stop_reason: None,
                stop_sequence: None,
                container: None,
                usage: Usage::default(),
            },
        });
        let mut next_index = 0_u32;
        let mut reasoning_index = None;
        let mut text_index = None;
        let mut reasoning_started = false;
        let mut text_started = false;
        while let Some(event) = source.next().await {
            match event {
                Ok(ModelStreamEvent::ReasoningDelta { delta }) => {
                    let index = *reasoning_index.get_or_insert_with(|| {
                        let index = next_index;
                        next_index = next_index.saturating_add(1);
                        index
                    });
                    if delta.is_empty() {
                        continue;
                    }
                    if !reasoning_started {
                        reasoning_started = true;
                        yield Ok(StreamEvent::ContentBlockStart {
                            index,
                            content_block: ContentBlockStart::Thinking { thinking: String::new() },
                        });
                    }
                    yield Ok(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::ThinkingDelta { thinking: delta },
                    });
                }
                Ok(ModelStreamEvent::ContentDelta { delta }) => {
                    if delta.is_empty() {
                        continue;
                    }
                    let index = *text_index.get_or_insert_with(|| {
                        let index = next_index;
                        next_index = next_index.saturating_add(1);
                        index
                    });
                    if !text_started {
                        text_started = true;
                        yield Ok(StreamEvent::ContentBlockStart {
                            index,
                            content_block: ContentBlockStart::Text { text: String::new() },
                        });
                    }
                    yield Ok(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::TextDelta { text: delta },
                    });
                }
                Ok(ModelStreamEvent::Completed { output }) => {
                    if let Some(index) = reasoning_index.take() {
                        yield Ok(StreamEvent::ContentBlockStop { index });
                    }
                    if let Some(index) = text_index.take() {
                        yield Ok(StreamEvent::ContentBlockStop { index });
                    }
                    for tool_call in output.tool_calls {
                        let index = next_index;
                        next_index = next_index.saturating_add(1);
                        yield Ok(StreamEvent::ContentBlockStart {
                            index,
                            content_block: ContentBlockStart::ToolUse {
                                id: tool_call.id,
                                name: tool_call.name,
                                input: serde_json::json!({}),
                                caller: None,
                            },
                        });
                        if !tool_call.arguments.raw.is_empty() {
                            yield Ok(StreamEvent::ContentBlockDelta {
                                index,
                                delta: Delta::InputJsonDelta {
                                    partial_json: tool_call.arguments.raw,
                                },
                            });
                        }
                        yield Ok(StreamEvent::ContentBlockStop { index });
                    }
                    yield Ok(StreamEvent::MessageDelta {
                        delta: MessageDelta {
                            stop_reason: Some(finish_reason_name(output.finish_reason).to_string()),
                            stop_sequence: None,
                        },
                        usage: Some(tui_usage(output.usage)),
                    });
                    yield Ok(StreamEvent::MessageStop);
                }
                Err(error) => yield Err(anyhow::Error::new(error)),
            }
        }
    })
}

fn finish_reason_name(reason: ModelFinishReason) -> &'static str {
    match reason {
        ModelFinishReason::Stop => "stop",
        ModelFinishReason::ToolCalls => "tool_calls",
        ModelFinishReason::Length => "length",
        ModelFinishReason::ContentFilter => "content_filter",
        ModelFinishReason::InsufficientSystemResource => "insufficient_system_resource",
    }
}

fn tui_usage(usage: codewhale_runtime::Usage) -> Usage {
    Usage {
        input_tokens: usage.input_tokens.min(u64::from(u32::MAX)) as u32,
        output_tokens: usage.output_tokens.min(u64::from(u32::MAX)) as u32,
        prompt_cache_hit_tokens: Some(usage.cache_hit_tokens.min(u64::from(u32::MAX)) as u32),
        prompt_cache_miss_tokens: Some(usage.cache_miss_tokens.min(u64::from(u32::MAX)) as u32),
        prompt_cache_write_tokens: Some(usage.cache_write_tokens.min(u64::from(u32::MAX)) as u32),
        reasoning_tokens: Some(usage.reasoning_tokens.min(u64::from(u32::MAX)) as u32),
        reasoning_replay_tokens: Some(usage.reasoning_replay_tokens.min(u64::from(u32::MAX)) as u32),
        server_tool_use: None,
    }
}

/// Resolve the official DeepSeek thinking switch for one request.
///
/// DeepSeek documents thinking as enabled by default. Keeping that default
/// explicit in the request plan gives the response assembler an unambiguous
/// provenance rule for tool calls: enabled responses must carry their original
/// reasoning, while disabled responses legitimately omit it.
pub(crate) fn thinking_enabled_for_request(effort: Option<&str>) -> bool {
    legacy_reasoning_mode(effort).thinking_enabled()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolPlan {
    /// Prefix-time projection of the same pure decision used by
    /// [`plan_chat`]. The engine hashes `tools`; the sender serializes that
    /// exact projection and routes according to `surface`.
    pub surface: ApiSurface,
    pub tools: Option<Vec<Tool>>,
    pub strict_fallback: bool,
}

fn legacy_reasoning_mode(effort: Option<&str>) -> ReasoningMode {
    match effort
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("off" | "disabled" | "none" | "false") => ReasoningMode::Off,
        Some("low" | "minimal" | "medium" | "mid" | "high" | "") => ReasoningMode::High,
        Some("xhigh" | "max" | "highest" | "ultracode") => ReasoningMode::Max,
        _ => ReasoningMode::Auto,
    }
}

/// Plan one official DeepSeek chat request and its exact wire tool catalog.
///
/// `None` deliberately leaves custom bases, custom path suffixes, and other
/// providers on their existing compatibility path.
pub(crate) fn plan_tools(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
    strict_enabled: bool,
    tools: Option<&[Tool]>,
) -> Option<ToolPlan> {
    official_root(provider, base_url)?;
    if path_suffix.is_some() {
        return None;
    }

    let decision = codewhale_deepseek::plan_tool_surface(
        strict_enabled,
        tools.into_iter().flatten().map(|tool| &tool.input_schema),
    );
    let mut planned_tools = tools.map(<[Tool]>::to_vec);
    if let Some(tools) = planned_tools.as_mut() {
        for tool in tools {
            tool.strict = decision.strict_compatible.then_some(true);
        }
    }

    Some(ToolPlan {
        surface: decision.surface,
        tools: planned_tools,
        strict_fallback: decision.strict_fallback,
    })
}

/// Whether this planner owns the final route. Custom path suffixes and
/// compatible gateways stay on the legacy path until their explicit cleanup.
pub(crate) fn owns_route(provider: ApiProvider, base_url: &str, path_suffix: Option<&str>) -> bool {
    path_suffix.is_none() && official_root(provider, base_url).is_some()
}

pub(crate) fn plan_chat(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
    strict_enabled: bool,
    request: &MessageRequest,
    response_mode: ResponseMode,
) -> Result<Option<RequestPlan>, ChatPlanError> {
    let Some(root) = official_root(provider, base_url) else {
        return Ok(None);
    };
    if path_suffix.is_some() {
        return Ok(None);
    }
    let wire_model = wire_model_for_provider(provider, &request.model);
    // Prompt projection must use the same canonical model identity that will
    // be sent on the wire. Compact aliases such as `pro` and `flash` do not
    // themselves carry V4 reasoning semantics; using the raw alias here would
    // discard exact reasoning history before the request is serialized.
    let mut wire_request = request.clone();
    wire_request.model.clone_from(&wire_model);
    validate_canonical_reasoning_replay(&wire_request, &wire_model)?;
    let messages =
        super::chat::build_chat_messages_for_request_and_provider(&wire_request, provider);
    let tools = request.tools.as_deref().map(|tools| {
        tools
            .iter()
            .map(|tool| PlannedTool {
                name: codewhale_deepseek::encode_tool_name(&tool.name),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
            .collect()
    });
    let tool_choice = request
        .tool_choice
        .as_ref()
        .and_then(super::chat::map_tool_choice_for_chat);
    codewhale_deepseek::plan_chat(
        root,
        strict_enabled,
        ChatPlanInput {
            model: wire_model,
            messages,
            max_tokens: request.max_tokens,
            response_mode,
            tools,
            tool_choice,
            reasoning: legacy_reasoning_mode(request.reasoning_effort.as_deref()),
            temperature: request.temperature,
            top_p: request.top_p,
        },
    )
    .map(Some)
}

/// Plan the official DeepSeek request used by the canonical AgentRuntime.
///
/// This path deliberately consumes [`ModelRequest`] directly. Runtime turns
/// must not be projected through the legacy TUI `MessageRequest`/content-block
/// model before the DeepSeek surface, exact history and body are frozen.
pub(crate) fn plan_runtime_chat(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
    strict_enabled: bool,
    request: &ModelRequest,
) -> Result<Option<RequestPlan>, ChatPlanError> {
    let Some(root) = runtime_planner_root(provider, base_url, path_suffix) else {
        return Ok(None);
    };
    let wire_model = wire_model_for_provider(provider, &request.model);
    let max_tokens = request.max_output_tokens.unwrap_or_else(|| {
        crate::models::max_output_tokens_for_model(&request.model).unwrap_or(4096)
    });
    codewhale_deepseek::plan_runtime_chat(
        RuntimeChatPlanInput {
            root: &root,
            strict_enabled,
            wire_model,
            max_tokens,
        },
        request,
    )
    .map(Some)
}

pub(crate) fn plan_fim(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
    prompt: &str,
    suffix: &str,
    max_tokens: u32,
) -> Result<RequestPlan, FimPlanError> {
    if official_root(provider, base_url).is_none() {
        return Err(FimPlanError::RequiresOfficialDeepSeek);
    }
    codewhale_deepseek::plan_fim(base_url, path_suffix, prompt, suffix, max_tokens)
}

fn validate_canonical_reasoning_replay(
    request: &MessageRequest,
    model: &str,
) -> Result<(), ChatPlanError> {
    if !super::chat::requires_tool_call_reasoning_replay(model) {
        return Ok(());
    }
    for (message_index, message) in request.messages.iter().enumerate() {
        if message.role != "assistant"
            || !message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
        {
            continue;
        }
        let reasoning = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Thinking { thinking, .. } => Some(thinking.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        // No Thinking block is the canonical representation of a tool call
        // generated with `thinking=disabled`. The production turn assembler
        // rejects a thinking-enabled tool response before it can enter history
        // without a non-empty block, making absence unambiguous for new runs.
        if reasoning.len() == 1 && reasoning[0].is_empty() {
            return Err(ChatPlanError::MissingReasoningContent { message_index });
        }
        if reasoning.len() > 1 {
            return Err(ChatPlanError::AmbiguousReasoningContent {
                message_index,
                block_count: reasoning.len(),
            });
        }
    }
    Ok(())
}

fn official_root(provider: ApiProvider, base_url: &str) -> Option<&'static str> {
    if !matches!(provider, ApiProvider::Deepseek | ApiProvider::DeepseekCN) {
        return None;
    }
    codewhale_deepseek::official_root(base_url)
}

/// Select the transport root for the canonical DeepSeek ModelPort.
///
/// Loopback is an offline protocol fixture, not a provider compatibility
/// fallback: it still receives the exact official DeepSeek request plan. The
/// interactive legacy route remains unclaimed for every custom base.
fn runtime_planner_root(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
) -> Option<String> {
    if path_suffix.is_some() || !matches!(provider, ApiProvider::Deepseek | ApiProvider::DeepseekCN)
    {
        return None;
    }
    if let Some(root) = codewhale_deepseek::official_root(base_url) {
        return Some(root.to_owned());
    }

    let url = reqwest::Url::parse(base_url).ok()?;
    let host = url.host_str()?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let fixture_path = matches!(url.path(), "" | "/" | "/v1" | "/v1/");
    if !loopback
        || !fixture_path
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(super::versioned_base_url(base_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_runtime::{
        AgentActor, ModelMessage, ModelToolCall, ReasoningEffort, RunId,
        SystemPrompt as RuntimeSystemPrompt, ToolArguments, ToolDefinition,
    };
    use serde_json::{Value, json};

    use crate::models::{ContentBlock, Message};

    fn tool(name: &str, schema: serde_json::Value) -> Tool {
        Tool {
            tool_type: Some("function".to_string()),
            name: name.to_string(),
            description: format!("{name} tool"),
            input_schema: schema,
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }
    }

    fn compatible_tool(name: &str) -> Tool {
        tool(
            name,
            json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            }),
        )
    }

    fn request(tools: Option<Vec<Tool>>) -> MessageRequest {
        MessageRequest {
            model: "deepseek-v4-flash".to_string(),
            messages: Vec::new(),
            max_tokens: 128,
            system: None,
            tools,
            tool_choice: Some(json!("auto")),
            metadata: None,
            thinking: None,
            reasoning_effort: Some("medium".to_string()),
            stream: None,
            temperature: Some(0.2),
            top_p: Some(0.9),
        }
    }

    #[test]
    fn compatible_catalog_selects_beta_strict_without_mutating_input() {
        let tools = vec![compatible_tool("first"), compatible_tool("second")];
        let original = tools.clone();
        let plan = plan_tools(
            ApiProvider::Deepseek,
            "https://api.deepseek.com/v1",
            None,
            true,
            Some(&tools),
        )
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert!(!plan.strict_fallback);
        assert!(
            plan.tools
                .as_ref()
                .is_some_and(|tools| tools.iter().all(|tool| tool.strict == Some(true)))
        );
        assert_eq!(tools, original, "planner must not mutate its input");
    }

    #[test]
    fn one_incompatible_schema_falls_back_atomically_to_standard_chat() {
        let mut tools = vec![compatible_tool("compatible")];
        tools[0].strict = Some(true);
        tools.push(tool(
            "incompatible",
            json!({
                "type": "object",
                "properties": {"optional": {"type": "string"}},
                "required": [],
                "additionalProperties": false
            }),
        ));
        tools[1].strict = Some(false);
        let plan = plan_tools(
            ApiProvider::DeepseekCN,
            "https://api.deepseek.com/beta",
            None,
            true,
            Some(&tools),
        )
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert!(plan.strict_fallback);
        let planned = plan.tools.expect("tools preserved");
        assert_eq!(planned.len(), tools.len());
        assert!(planned.iter().all(|tool| tool.strict.is_none()));
        assert_eq!(
            planned
                .iter()
                .map(|tool| (&tool.name, &tool.input_schema))
                .collect::<Vec<_>>(),
            tools
                .iter()
                .map(|tool| (&tool.name, &tool.input_schema))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn standard_chat_clears_stale_strict_flags() {
        let mut tools = vec![compatible_tool("lookup")];
        tools[0].strict = Some(true);
        let plan = plan_tools(
            ApiProvider::Deepseek,
            "https://api.deepseek.com/beta/",
            None,
            false,
            Some(&tools),
        )
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.tools.unwrap()[0].strict, None);
    }

    #[test]
    fn legacy_and_runtime_projections_freeze_the_same_wire_body() {
        let raw_arguments = "{ \"value\" : \"history\", \"order\" : 1 }";
        let mut legacy = request(Some(vec![compatible_tool("read-file")]));
        legacy.temperature = None;
        legacy.top_p = None;
        legacy.messages = vec![
            Message {
                role: "assistant".to_owned(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "原始推理".to_owned(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call-1".to_owned(),
                        name: "read-file".to_owned(),
                        input: json!({"value": "history", "order": 1}),
                        raw_arguments: Some(raw_arguments.to_owned()),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_owned(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-1".to_owned(),
                    content: "ok".to_owned(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];
        let runtime = ModelRequest {
            run_id: RunId::from("projection-equivalence"),
            parent_run_id: None,
            actor: AgentActor::default(),
            model: legacy.model.clone(),
            system_prompt: RuntimeSystemPrompt { blocks: Vec::new() },
            messages: vec![
                ModelMessage::Assistant {
                    content: None,
                    reasoning_content: Some("原始推理".to_owned()),
                    tool_calls: vec![ModelToolCall {
                        id: "call-1".to_owned(),
                        name: "read-file".to_owned(),
                        arguments: ToolArguments::parse(raw_arguments),
                    }],
                },
                ModelMessage::Tool {
                    call_id: "call-1".to_owned(),
                    name: "read-file".to_owned(),
                    content: "ok".to_owned(),
                },
            ],
            tools: vec![ToolDefinition {
                name: "read-file".to_owned(),
                description: "read-file tool".to_owned(),
                input_schema: legacy.tools.as_ref().unwrap()[0].input_schema.clone(),
            }],
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(legacy.max_tokens),
            streaming: true,
            request_number: 1,
            attempt: 0,
        };

        let legacy_plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &legacy,
            ResponseMode::Streaming,
        )
        .unwrap()
        .unwrap();
        let runtime_plan = plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &runtime,
        )
        .unwrap()
        .unwrap();

        assert_eq!(legacy_plan.body, runtime_plan.body);
        assert_eq!(
            runtime_plan.body["messages"][0]["tool_calls"][0]["function"]["arguments"],
            raw_arguments
        );
        assert_eq!(
            runtime_plan.body["tools"][0]["function"]["name"],
            "read--file"
        );
    }

    #[test]
    fn chat_plan_owns_the_final_strict_streaming_wire_request() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "inspect repository".to_string(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call-1".to_string(),
                        name: "lookup".to_string(),
                        input: json!({}),
                        raw_arguments: None,
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-1".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];
        let prefix_plan = plan_tools(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            request.tools.as_deref(),
        )
        .expect("prefix projection");
        let plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            &request,
            ResponseMode::Streaming,
        )
        .expect("valid exact replay history")
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert_eq!(plan.surface, prefix_plan.surface);
        assert_eq!(plan.url, "https://api.deepseek.com/beta/chat/completions");
        assert_eq!(plan.response_mode, ResponseMode::Streaming);
        assert_eq!(plan.model, "deepseek-v4-flash");
        assert_eq!(plan.body["model"], "deepseek-v4-flash");
        assert_eq!(plan.body["model"].as_str(), Some(plan.model.as_str()));
        assert_eq!(plan.body["stream"], true);
        assert_eq!(plan.body["stream_options"]["include_usage"], true);
        assert!(plan.body.get("tool_choice").is_none());
        assert!((plan.body["temperature"].as_f64().expect("temperature") - 0.2).abs() < 1e-6);
        assert!((plan.body["top_p"].as_f64().expect("top_p") - 0.9).abs() < 1e-6);
        assert_eq!(plan.body["tools"][0]["function"]["strict"], true);
        assert_eq!(
            plan.body["tools"],
            Value::Array(
                prefix_plan
                    .tools
                    .as_deref()
                    .expect("projected tools")
                    .iter()
                    .map(super::super::chat::tool_to_chat)
                    .collect()
            )
        );
        assert_eq!(plan.reasoning_replay_tokens, Some(4));
    }

    #[test]
    fn chat_plan_sends_auto_tool_choice_when_reasoning_is_off() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.reasoning_effort = Some("off".to_string());
        let plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com/v1",
            None,
            false,
            &request,
            ResponseMode::NonStreaming,
        )
        .expect("valid exact replay history")
        .expect("official plan");

        assert_eq!(plan.body["tool_choice"], "auto");
        assert_eq!(plan.body["thinking"]["type"], "disabled");
    }

    #[test]
    fn chat_plan_makes_the_documented_default_thinking_mode_explicit() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.reasoning_effort = None;

        let plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            &request,
            ResponseMode::NonStreaming,
        )
        .expect("valid request")
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert_eq!(plan.body["thinking"]["type"], "enabled");
        assert!(
            plan.body.get("tool_choice").is_none(),
            "thinking mode must not send the unsupported explicit tool_choice"
        );
    }

    #[test]
    fn strict_non_thinking_round_trips_tool_history_without_reasoning() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.reasoning_effort = Some("off".to_string());
        request.messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "call-non-thinking".to_string(),
                    name: "lookup".to_string(),
                    input: json!({"value": "history"}),
                    raw_arguments: None,
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-non-thinking".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            &request,
            ResponseMode::NonStreaming,
        )
        .expect("non-thinking history is protocol-valid")
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert_eq!(plan.body["thinking"]["type"], "disabled");
        assert!(plan.body["messages"][0].get("reasoning_content").is_none());
        assert!(plan.body["messages"][0].get("tool_calls").is_some());
        assert_eq!(plan.reasoning_replay_tokens, None);
    }

    #[test]
    fn official_standard_plan_replays_alias_reasoning_exactly() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.model = "pro".to_string();
        let original_reasoning = "先检查原始调用\nkeep trailing bytes  \t";
        request.messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: original_reasoning.to_string(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call-alias".to_string(),
                        name: "lookup".to_string(),
                        input: json!({"value": "history"}),
                        raw_arguments: None,
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-alias".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let plan = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &request,
            ResponseMode::Streaming,
        )
        .expect("valid exact replay history")
        .expect("official plan");

        assert_eq!(plan.model, "deepseek-v4-pro");
        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(plan.body["model"], "deepseek-v4-pro");
        assert_eq!(
            plan.body["messages"][0]["reasoning_content"],
            original_reasoning
        );
    }

    #[test]
    fn official_plan_rejects_empty_thinking_provenance_before_transport() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call-missing".to_string(),
                        name: "lookup".to_string(),
                        input: json!({"value": "history"}),
                        raw_arguments: None,
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-missing".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let error = plan_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &request,
            ResponseMode::Streaming,
        )
        .expect_err("empty thinking provenance must fail before HTTP");

        assert_eq!(
            error,
            ChatPlanError::MissingReasoningContent { message_index: 0 }
        );
        assert!(error.to_string().contains("HTTP request was not sent"));
    }

    #[test]
    fn incompatible_chat_plan_preserves_tools_on_the_standard_surface() {
        let tools = vec![tool(
            "optional_lookup",
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": [],
                "additionalProperties": false
            }),
        )];
        let request = request(Some(tools));
        let prefix_plan = plan_tools(
            ApiProvider::DeepseekCN,
            "https://api.deepseek.com/beta",
            None,
            true,
            request.tools.as_deref(),
        )
        .expect("prefix projection");
        let plan = plan_chat(
            ApiProvider::DeepseekCN,
            "https://api.deepseek.com/beta",
            None,
            true,
            &request,
            ResponseMode::NonStreaming,
        )
        .expect("valid exact replay history")
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.surface, prefix_plan.surface);
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(plan.response_mode, ResponseMode::NonStreaming);
        assert_eq!(plan.body["stream"], false);
        assert!(plan.body.get("stream_options").is_none());
        assert_eq!(plan.body["tools"].as_array().map(Vec::len), Some(1));
        assert!(plan.body["tools"][0]["function"].get("strict").is_none());
        assert_eq!(
            plan.body["tools"],
            Value::Array(
                prefix_plan
                    .tools
                    .as_deref()
                    .expect("projected tools")
                    .iter()
                    .map(super::super::chat::tool_to_chat)
                    .collect()
            )
        );
    }

    #[test]
    fn custom_routes_and_other_providers_are_not_claimed() {
        let tools = vec![compatible_tool("lookup")];
        for (provider, base, suffix) in [
            (ApiProvider::Deepseek, "https://gateway.example/v1", None),
            (
                ApiProvider::Deepseek,
                "https://api.deepseeki.com/beta",
                None,
            ),
            (
                ApiProvider::Deepseek,
                "https://api.deepseek.com/beta",
                Some("/tenant/chat/completions"),
            ),
            (
                ApiProvider::Openrouter,
                "https://api.deepseek.com/beta",
                None,
            ),
        ] {
            assert!(plan_tools(provider, base, suffix, true, Some(&tools)).is_none());
            assert!(!owns_route(provider, base, suffix));
        }

        assert!(owns_route(
            ApiProvider::DeepseekCN,
            "https://api.deepseek.com/v1",
            None
        ));
    }

    #[test]
    fn canonical_runtime_accepts_only_clean_loopback_protocol_fixtures() {
        assert_eq!(
            runtime_planner_root(ApiProvider::Deepseek, "http://127.0.0.1:43123", None),
            Some("http://127.0.0.1:43123/v1".to_owned())
        );
        assert_eq!(
            runtime_planner_root(ApiProvider::DeepseekCN, "http://localhost:43123/v1", None),
            Some("http://localhost:43123/v1".to_owned())
        );
        for (provider, base, suffix) in [
            (ApiProvider::Openrouter, "http://127.0.0.1:43123", None),
            (ApiProvider::Deepseek, "https://gateway.example/v1", None),
            (ApiProvider::Deepseek, "http://user@127.0.0.1:43123", None),
            (
                ApiProvider::Deepseek,
                "http://127.0.0.1:43123?token=secret",
                None,
            ),
            (
                ApiProvider::Deepseek,
                "http://127.0.0.1:43123",
                Some("/tenant/chat/completions"),
            ),
        ] {
            assert_eq!(runtime_planner_root(provider, base, suffix), None);
        }

        assert!(
            plan_chat(
                ApiProvider::Deepseek,
                "http://127.0.0.1:43123",
                None,
                false,
                &request(None),
                ResponseMode::Streaming,
            )
            .unwrap()
            .is_none(),
            "loopback must not become a generic interactive compatibility route"
        );
    }

    #[test]
    fn fim_is_fixed_to_official_beta_pro_and_validates_limits() {
        let plan = plan_fim(
            ApiProvider::Deepseek,
            "https://api.deepseek.com/v1",
            None,
            "prefix",
            "suffix",
            4096,
        )
        .expect("FIM plan");
        assert_eq!(plan.surface, ApiSurface::Fim);
        assert_eq!(plan.url, "https://api.deepseek.com/beta/completions");
        assert_eq!(plan.model, codewhale_deepseek::FIM_MODEL);
        assert_eq!(plan.body["model"], codewhale_deepseek::FIM_MODEL);
        assert_eq!(plan.body["prompt"], "prefix");
        assert_eq!(plan.body["suffix"], "suffix");
        assert_eq!(plan.body["max_tokens"], 4096);

        for max_tokens in [0, 4097] {
            assert_eq!(
                plan_fim(
                    ApiProvider::Deepseek,
                    "https://api.deepseek.com/beta",
                    None,
                    "prefix",
                    "suffix",
                    max_tokens
                ),
                Err(FimPlanError::InvalidMaxTokens)
            );
        }
        assert_eq!(
            plan_fim(
                ApiProvider::Deepseek,
                "https://gateway.example/v1",
                None,
                "prefix",
                "suffix",
                16
            ),
            Err(FimPlanError::RequiresOfficialDeepSeek)
        );
        assert_eq!(
            plan_fim(
                ApiProvider::Deepseek,
                "https://api.deepseek.com",
                Some("/tenant/completions"),
                "prefix",
                "suffix",
                16
            ),
            Err(FimPlanError::RequiresOfficialDeepSeek)
        );
    }
}
