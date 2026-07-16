//! Pure request planning for the official DeepSeek OpenAI-format API.

use std::collections::HashMap;
use std::fmt;

use codewhale_runtime::{
    ModelMessage, ModelRequest, ReasoningEffort, SystemPrompt as RuntimeSystemPrompt,
};
use serde_json::{Value, json};

use crate::config::{ApiProvider, wire_model_for_provider};
use crate::models::{ContentBlock, MessageRequest, Tool};

pub(crate) const FIM_MODEL: &str = "deepseek-v4-pro";

/// Resolve the official DeepSeek thinking switch for one request.
///
/// DeepSeek documents thinking as enabled by default. Keeping that default
/// explicit in the request plan gives the response assembler an unambiguous
/// provenance rule for tool calls: enabled responses must carry their original
/// reasoning, while disabled responses legitimately omit it.
pub(crate) fn thinking_enabled_for_request(effort: Option<&str>) -> bool {
    DeepSeekReasoning::from_legacy(effort).thinking_enabled()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiSurface {
    StandardChat,
    StrictChat,
    Fim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseMode {
    NonStreaming,
    Streaming,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestPlan {
    pub surface: ApiSurface,
    pub url: String,
    pub model: String,
    pub body: Value,
    pub response_mode: ResponseMode,
    pub reasoning_replay_tokens: Option<u32>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeepSeekReasoning {
    Off,
    Auto,
    High,
    Max,
}

impl DeepSeekReasoning {
    fn from_legacy(effort: Option<&str>) -> Self {
        match effort
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("off" | "disabled" | "none" | "false") => Self::Off,
            Some("low" | "minimal" | "medium" | "mid" | "high" | "") => Self::High,
            Some("xhigh" | "max" | "highest" | "ultracode") => Self::Max,
            _ => Self::Auto,
        }
    }

    fn from_runtime(effort: ReasoningEffort) -> Self {
        match effort {
            ReasoningEffort::Off => Self::Off,
            ReasoningEffort::Auto => Self::Auto,
            ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High => Self::High,
            ReasoningEffort::Max => Self::Max,
        }
    }

    fn thinking_enabled(self) -> bool {
        self != Self::Off
    }
}

struct PlannedTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a Value,
}

struct ChatPlanInput<'a> {
    model: String,
    messages: Vec<Value>,
    max_tokens: u32,
    response_mode: ResponseMode,
    tools: Option<Vec<PlannedTool<'a>>>,
    tool_choice: Option<Value>,
    reasoning: DeepSeekReasoning,
    temperature: Option<f32>,
    top_p: Option<f32>,
}

/// A request cannot satisfy DeepSeek's exact history replay contract.
///
/// This is a local preflight error: callers must surface it without issuing an
/// HTTP request or inventing replacement reasoning text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChatPlanError {
    MissingReasoningContent {
        message_index: usize,
    },
    AmbiguousReasoningContent {
        message_index: usize,
        block_count: usize,
    },
}

impl fmt::Display for ChatPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingReasoningContent { message_index } => write!(
                f,
                "Invalid DeepSeek request: exact reasoning replay requires assistant message {message_index} to contain its original reasoning_content; HTTP request was not sent"
            ),
            Self::AmbiguousReasoningContent {
                message_index,
                block_count,
            } => write!(
                f,
                "Invalid DeepSeek request: assistant message {message_index} contains {block_count} reasoning blocks, so the original single reasoning_content value cannot be reconstructed exactly; HTTP request was not sent"
            ),
        }
    }
}

impl std::error::Error for ChatPlanError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FimPlanError {
    RequiresOfficialDeepSeek,
    InvalidMaxTokens,
}

impl fmt::Display for FimPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiresOfficialDeepSeek => {
                f.write_str("FIM requires the official DeepSeek OpenAI-format API")
            }
            Self::InvalidMaxTokens => {
                f.write_str("DeepSeek FIM max_tokens must be between 1 and 4096")
            }
        }
    }
}

impl std::error::Error for FimPlanError {}

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

    let strict_requested = strict_enabled && tools.is_some_and(|tools| !tools.is_empty());
    let strict_compatible = strict_requested
        && tools.is_some_and(|tools| {
            tools.iter().all(|tool| {
                crate::tools::schema_sanitize::strict_schema_supported(&tool.input_schema)
            })
        });
    let mut planned_tools = tools.map(<[Tool]>::to_vec);
    if let Some(tools) = planned_tools.as_mut() {
        for tool in tools {
            tool.strict = strict_compatible.then_some(true);
        }
    }

    Some(ToolPlan {
        surface: chat_surface(strict_compatible),
        tools: planned_tools,
        strict_fallback: strict_requested && !strict_compatible,
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
                name: &tool.name,
                description: &tool.description,
                input_schema: &tool.input_schema,
            })
            .collect()
    });
    let tool_choice = request
        .tool_choice
        .as_ref()
        .and_then(super::chat::map_tool_choice_for_chat);
    plan_chat_core(
        root,
        strict_enabled,
        ChatPlanInput {
            model: wire_model,
            messages,
            max_tokens: request.max_tokens,
            response_mode,
            tools,
            tool_choice,
            reasoning: DeepSeekReasoning::from_legacy(request.reasoning_effort.as_deref()),
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
    let Some(root) = official_root(provider, base_url) else {
        return Ok(None);
    };
    if path_suffix.is_some() {
        return Ok(None);
    }

    let wire_model = wire_model_for_provider(provider, &request.model);
    validate_runtime_reasoning_replay(request, &wire_model)?;
    let response_mode = if request.streaming {
        ResponseMode::Streaming
    } else {
        ResponseMode::NonStreaming
    };
    let messages = runtime_chat_messages(request, &wire_model);
    let tools = (!request.tools.is_empty()).then(|| {
        request
            .tools
            .iter()
            .map(|tool| PlannedTool {
                name: &tool.name,
                description: &tool.description,
                input_schema: &tool.input_schema,
            })
            .collect()
    });
    plan_chat_core(
        root,
        strict_enabled,
        ChatPlanInput {
            model: wire_model,
            messages,
            max_tokens: request.max_output_tokens.unwrap_or_else(|| {
                crate::models::max_output_tokens_for_model(&request.model).unwrap_or(4096)
            }),
            response_mode,
            tools,
            tool_choice: None,
            reasoning: DeepSeekReasoning::from_runtime(request.reasoning_effort),
            temperature: None,
            top_p: None,
        },
    )
    .map(Some)
}

fn plan_chat_core(
    root: &str,
    strict_enabled: bool,
    input: ChatPlanInput<'_>,
) -> Result<RequestPlan, ChatPlanError> {
    let strict_requested =
        strict_enabled && input.tools.as_ref().is_some_and(|tools| !tools.is_empty());
    let strict_compatible = strict_requested
        && input.tools.as_ref().is_some_and(|tools| {
            tools.iter().all(|tool| {
                crate::tools::schema_sanitize::strict_schema_supported(tool.input_schema)
            })
        });
    let surface = chat_surface(strict_compatible);
    let streaming = input.response_mode == ResponseMode::Streaming;
    let mut body = json!({
        "model": input.model,
        "messages": input.messages,
        "max_tokens": input.max_tokens,
        "stream": streaming,
    });
    if streaming {
        body["stream_options"] = json!({"include_usage": true});
    }
    if let Some(temperature) = input.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = input.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(tools) = input.tools {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| planned_tool_to_chat(tool, strict_compatible))
                .collect(),
        );
    }
    if input.reasoning == DeepSeekReasoning::Off
        && let Some(tool_choice) = input.tool_choice
    {
        body["tool_choice"] = tool_choice;
    }
    match input.reasoning {
        DeepSeekReasoning::Off | DeepSeekReasoning::Auto => {}
        DeepSeekReasoning::High => body["reasoning_effort"] = json!("high"),
        DeepSeekReasoning::Max => body["reasoning_effort"] = json!("max"),
    }
    body["thinking"] = if input.reasoning.thinking_enabled() {
        json!({ "type": "enabled" })
    } else {
        json!({ "type": "disabled" })
    };
    let model = body["model"]
        .as_str()
        .expect("planner model is a JSON string")
        .to_owned();
    validate_exact_reasoning_replay(&body, &model)?;
    Ok(RequestPlan {
        surface,
        url: chat_url(root, surface),
        model,
        reasoning_replay_tokens: reasoning_replay_tokens(&body),
        body,
        response_mode: input.response_mode,
    })
}

fn chat_surface(strict_compatible: bool) -> ApiSurface {
    if strict_compatible {
        ApiSurface::StrictChat
    } else {
        ApiSurface::StandardChat
    }
}

fn planned_tool_to_chat(tool: &PlannedTool<'_>, strict: bool) -> Value {
    let mut value = json!({
        "type": "function",
        "function": {
            "name": super::to_api_tool_name(tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    });
    if strict {
        value["function"]["strict"] = json!(true);
    }
    value
}

fn runtime_chat_messages(request: &ModelRequest, wire_model: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut pending_tool_calls: HashMap<String, (String, Value)> = HashMap::new();
    let mut seen_tool_results: HashMap<String, super::chat::SeenToolResult> = HashMap::new();
    if let Some(system) = runtime_system_instructions(&request.system_prompt) {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }

    let replay_current_reasoning =
        DeepSeekReasoning::from_runtime(request.reasoning_effort).thinking_enabled();
    for (message_index, message) in request.messages.iter().enumerate() {
        match message {
            ModelMessage::User { content } => {
                pending_tool_calls.clear();
                messages.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
            ModelMessage::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => {
                let replay_tool_reasoning = !tool_calls.is_empty()
                    && super::chat::requires_tool_call_reasoning_replay(wire_model);
                let reasoning = reasoning_content
                    .as_deref()
                    .filter(|reasoning| !reasoning.is_empty())
                    .filter(|_| replay_current_reasoning || replay_tool_reasoning);
                if content.as_deref().is_none_or(str::is_empty)
                    && reasoning.is_none()
                    && tool_calls.is_empty()
                {
                    continue;
                }

                let mut wire = json!({
                    "role": "assistant",
                    "content": match content.as_deref().filter(|content| !content.is_empty()) {
                        Some(content) => json!(content),
                        None if reasoning.is_some() => json!(""),
                        None => Value::Null,
                    },
                });
                if let Some(reasoning) = reasoning {
                    wire["reasoning_content"] = json!(reasoning);
                }
                if !tool_calls.is_empty() {
                    wire["tool_calls"] = Value::Array(
                        tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id,
                                    "type": "function",
                                    "function": {
                                        "name": super::to_api_tool_name(&call.name),
                                        "arguments": call.arguments.raw,
                                    }
                                })
                            })
                            .collect(),
                    );
                    pending_tool_calls = tool_calls
                        .iter()
                        .map(|call| {
                            (
                                call.id.clone(),
                                (
                                    call.name.clone(),
                                    call.arguments.parsed.clone().unwrap_or_else(|| {
                                        Value::String(call.arguments.raw.clone())
                                    }),
                                ),
                            )
                        })
                        .collect();
                } else {
                    pending_tool_calls.clear();
                }
                messages.push(wire);
            }
            ModelMessage::Tool {
                call_id, content, ..
            } => {
                let Some((tool_name, input)) = pending_tool_calls.remove(call_id) else {
                    crate::logging::warn(format!(
                        "Dropping tool result for unknown tool_call_id: {call_id}"
                    ));
                    continue;
                };
                let wire_result = super::chat::compact_tool_result_for_wire(
                    &tool_name,
                    &input,
                    content,
                    &format!("Message #{message_index}"),
                    &mut seen_tool_results,
                );
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": wire_result.content,
                }));
            }
        }
    }
    messages
}

fn runtime_system_instructions(system: &RuntimeSystemPrompt) -> Option<String> {
    let joined = system
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    (!joined.trim().is_empty()).then_some(joined)
}

fn validate_runtime_reasoning_replay(
    request: &ModelRequest,
    model: &str,
) -> Result<(), ChatPlanError> {
    if !super::chat::requires_tool_call_reasoning_replay(model) {
        return Ok(());
    }
    for (message_index, message) in request.messages.iter().enumerate() {
        let ModelMessage::Assistant {
            reasoning_content,
            tool_calls,
            ..
        } = message
        else {
            continue;
        };
        if !tool_calls.is_empty() && reasoning_content.as_deref() == Some("") {
            return Err(ChatPlanError::MissingReasoningContent { message_index });
        }
    }
    Ok(())
}

fn reasoning_replay_tokens(body: &Value) -> Option<u32> {
    let replay_bytes = body
        .get("messages")
        .and_then(Value::as_array)?
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .filter_map(|message| message.get("reasoning_content").and_then(Value::as_str))
        .filter(|reasoning| !reasoning.is_empty())
        .fold(0_u64, |total, reasoning| {
            total.saturating_add(reasoning.len() as u64)
        });
    (replay_bytes > 0).then(|| (replay_bytes / 4).min(u64::from(u32::MAX)) as u32)
}

pub(crate) fn plan_fim(
    provider: ApiProvider,
    base_url: &str,
    path_suffix: Option<&str>,
    prompt: &str,
    suffix: &str,
    max_tokens: u32,
) -> Result<RequestPlan, FimPlanError> {
    if !(1..=4096).contains(&max_tokens) {
        return Err(FimPlanError::InvalidMaxTokens);
    }
    let root = official_root(provider, base_url).ok_or(FimPlanError::RequiresOfficialDeepSeek)?;
    if path_suffix.is_some() {
        return Err(FimPlanError::RequiresOfficialDeepSeek);
    }
    Ok(RequestPlan {
        surface: ApiSurface::Fim,
        url: format!("{root}/beta/completions"),
        model: FIM_MODEL.to_string(),
        body: json!({
            "model": FIM_MODEL,
            "prompt": prompt,
            "suffix": suffix,
            "max_tokens": max_tokens,
        }),
        response_mode: ResponseMode::NonStreaming,
        reasoning_replay_tokens: None,
    })
}

fn chat_url(root: &str, surface: ApiSurface) -> String {
    if surface == ApiSurface::StrictChat {
        format!("{root}/beta/chat/completions")
    } else {
        format!("{root}/chat/completions")
    }
}

fn validate_exact_reasoning_replay(body: &Value, model: &str) -> Result<(), ChatPlanError> {
    if !super::chat::requires_tool_call_reasoning_replay(model) {
        return Ok(());
    }
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    for (message_index, message) in messages.iter().enumerate() {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        let has_empty_reasoning = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(str::is_empty);
        // Absence is valid provenance for a non-thinking tool round. An
        // explicitly present but empty value is not an exact replay.
        if has_tool_calls && has_empty_reasoning {
            return Err(ChatPlanError::MissingReasoningContent { message_index });
        }
    }
    Ok(())
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
    let base = base_url.trim_end_matches('/');
    [
        "https://api.deepseek.com",
        "https://api.deepseek.com/v1",
        "https://api.deepseek.com/beta",
    ]
    .iter()
    .any(|candidate| base.eq_ignore_ascii_case(candidate))
    .then_some("https://api.deepseek.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(plan.model, FIM_MODEL);
        assert_eq!(plan.body["model"], FIM_MODEL);
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
