//! Deterministic request planning for the official DeepSeek API.
//!
//! It owns the official wire decision, canonical `ModelRequest` projection,
//! physical request admission, and provider-reported usage ledger. The HTTP
//! sender and SSE decoder migrate in the next vertical slice.
//!
//! The next deletion point is the TUI legacy `MessageRequest` projection and
//! its local replay preflight, after the interactive loop emits canonical
//! `ModelRequest` directly.

use std::collections::HashMap;
use std::fmt;

use codewhale_runtime::{ModelMessage, ModelRequest, ReasoningEffort, SystemPrompt};
use serde_json::{Map, Value, json};

mod accounting;
mod pricing;

pub use accounting::{
    ApiRequestActor, ApiRequestActorSnapshot, ApiRequestBudgetError, ApiRequestBudgetSnapshot,
    ApiRequestKind, ApiRequestLease, ApiResponseAccountingGuard, ApiUsageBucket, ApiUsageSnapshot,
    SharedApiRequestBudget,
};
pub use pricing::{
    CostEstimate, CurrencyPricing, ModelPricing, calculate_turn_cost_estimate,
    pricing_for_official_model,
};

pub const FIM_MODEL: &str = "deepseek-v4-pro";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiSurface {
    StandardChat,
    StrictChat,
    Fim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseMode {
    NonStreaming,
    Streaming,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestPlan {
    pub surface: ApiSurface,
    pub url: String,
    pub model: String,
    pub body: Value,
    pub response_mode: ResponseMode,
    pub reasoning_replay_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningMode {
    Off,
    Auto,
    High,
    Max,
}

impl ReasoningMode {
    #[must_use]
    pub fn from_runtime(effort: ReasoningEffort) -> Self {
        match effort {
            ReasoningEffort::Off => Self::Off,
            ReasoningEffort::Auto => Self::Auto,
            ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High => Self::High,
            ReasoningEffort::Max => Self::Max,
        }
    }

    #[must_use]
    pub fn thinking_enabled(self) -> bool {
        self != Self::Off
    }
}

pub struct PlannedTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ChatPlanInput {
    pub model: String,
    pub messages: Vec<Value>,
    pub max_tokens: u32,
    pub response_mode: ResponseMode,
    pub tools: Option<Vec<PlannedTool>>,
    pub tool_choice: Option<Value>,
    pub reasoning: ReasoningMode,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

pub struct RuntimeChatPlanInput<'a> {
    /// Normalized endpoint root selected by the DeepSeek application adapter.
    /// Production uses the official host; offline conformance fixtures may
    /// substitute a loopback root without changing the protocol projection.
    pub root: &'a str,
    pub strict_enabled: bool,
    pub wire_model: String,
    pub max_tokens: u32,
}

/// A request cannot satisfy DeepSeek's exact history replay contract.
///
/// This is a local preflight error: callers must surface it without issuing an
/// HTTP request or inventing replacement reasoning text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatPlanError {
    MissingReasoningContent {
        message_index: usize,
    },
    AmbiguousReasoningContent {
        message_index: usize,
        block_count: usize,
    },
}

impl fmt::Display for ChatPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingReasoningContent { message_index } => write!(
                formatter,
                "Invalid DeepSeek request: exact reasoning replay requires assistant message {message_index} to contain its original reasoning_content; HTTP request was not sent"
            ),
            Self::AmbiguousReasoningContent {
                message_index,
                block_count,
            } => write!(
                formatter,
                "Invalid DeepSeek request: assistant message {message_index} contains {block_count} reasoning blocks, so the original single reasoning_content value cannot be reconstructed exactly; HTTP request was not sent"
            ),
        }
    }
}

impl std::error::Error for ChatPlanError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FimPlanError {
    InvalidMaxTokens,
    RequiresOfficialDeepSeek,
}

impl fmt::Display for FimPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMaxTokens => {
                formatter.write_str("DeepSeek FIM max_tokens must be between 1 and 4096")
            }
            Self::RequiresOfficialDeepSeek => {
                formatter.write_str("FIM requires the official DeepSeek OpenAI-format API")
            }
        }
    }
}

impl std::error::Error for FimPlanError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSurfaceDecision {
    pub surface: ApiSurface,
    pub strict_compatible: bool,
    pub strict_fallback: bool,
}

/// Decide strict mode atomically for a complete DeepSeek tool catalog.
///
/// A single incompatible schema keeps the entire catalog on ordinary Chat;
/// callers must never drop individual tools to force Beta Strict mode.
pub fn plan_tool_surface<'a>(
    strict_enabled: bool,
    schemas: impl IntoIterator<Item = &'a Value>,
) -> ToolSurfaceDecision {
    let mut schemas = schemas.into_iter().peekable();
    let strict_requested = strict_enabled && schemas.peek().is_some();
    let strict_compatible = strict_requested && schemas.all(strict_schema_supported);
    ToolSurfaceDecision {
        surface: chat_surface(strict_compatible),
        strict_compatible,
        strict_fallback: strict_requested && !strict_compatible,
    }
}

/// Freeze an already-projected official Chat request.
///
/// Both the canonical runtime projection and the legacy interactive
/// projection call this one decision core. The sender is not allowed to
/// re-evaluate strict compatibility, endpoint, reasoning, or body fields.
pub fn plan_chat(
    root: &str,
    strict_enabled: bool,
    input: ChatPlanInput,
) -> Result<RequestPlan, ChatPlanError> {
    let tool_surface = plan_tool_surface(
        strict_enabled,
        input.tools.iter().flatten().map(|tool| &tool.input_schema),
    );
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
                .map(|tool| planned_tool_to_chat(tool, tool_surface.strict_compatible))
                .collect(),
        );
    }
    if input.reasoning == ReasoningMode::Off
        && let Some(tool_choice) = input.tool_choice
    {
        body["tool_choice"] = tool_choice;
    }
    match input.reasoning {
        ReasoningMode::Off | ReasoningMode::Auto => {}
        ReasoningMode::High => body["reasoning_effort"] = json!("high"),
        ReasoningMode::Max => body["reasoning_effort"] = json!("max"),
    }
    body["thinking"] = if input.reasoning.thinking_enabled() {
        json!({"type": "enabled"})
    } else {
        json!({"type": "disabled"})
    };

    let model = body["model"]
        .as_str()
        .expect("planner model is a JSON string")
        .to_owned();
    validate_exact_reasoning_replay(&body, &model)?;
    Ok(RequestPlan {
        surface: tool_surface.surface,
        url: chat_url(root, tool_surface.surface),
        model,
        reasoning_replay_tokens: reasoning_replay_tokens(&body),
        body,
        response_mode: input.response_mode,
    })
}

/// Project one canonical runtime request and freeze its official Chat plan.
///
/// Tool-result compaction remains a context/tool concern. The caller supplies
/// only the final content projection; this crate still owns message ordering,
/// reasoning/raw-argument replay, strict selection, and every wire decision.
pub fn plan_runtime_chat<N, F>(
    input: RuntimeChatPlanInput<'_>,
    request: &ModelRequest,
    mut project_tool_name: N,
    mut project_tool_result: F,
) -> Result<RequestPlan, ChatPlanError>
where
    N: FnMut(&str) -> String,
    F: FnMut(&str, &Value, &str, &str) -> String,
{
    validate_runtime_reasoning_replay(request, &input.wire_model)?;
    let response_mode = if request.streaming {
        ResponseMode::Streaming
    } else {
        ResponseMode::NonStreaming
    };
    let messages = runtime_chat_messages(
        request,
        &input.wire_model,
        &mut project_tool_name,
        &mut project_tool_result,
    );
    let tools = (!request.tools.is_empty()).then(|| {
        request
            .tools
            .iter()
            .map(|tool| PlannedTool {
                name: project_tool_name(&tool.name),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
            .collect()
    });
    plan_chat(
        input.root,
        input.strict_enabled,
        ChatPlanInput {
            model: input.wire_model,
            messages,
            max_tokens: input.max_tokens,
            response_mode,
            tools,
            tool_choice: None,
            reasoning: ReasoningMode::from_runtime(request.reasoning_effort),
            temperature: None,
            top_p: None,
        },
    )
}

pub fn plan_fim(
    base_url: &str,
    path_suffix: Option<&str>,
    prompt: &str,
    suffix: &str,
    max_tokens: u32,
) -> Result<RequestPlan, FimPlanError> {
    if !(1..=4096).contains(&max_tokens) {
        return Err(FimPlanError::InvalidMaxTokens);
    }
    let root = official_root(base_url).ok_or(FimPlanError::RequiresOfficialDeepSeek)?;
    if path_suffix.is_some() {
        return Err(FimPlanError::RequiresOfficialDeepSeek);
    }
    Ok(RequestPlan {
        surface: ApiSurface::Fim,
        url: format!("{root}/beta/completions"),
        model: FIM_MODEL.to_owned(),
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

#[must_use]
pub fn official_root(base_url: &str) -> Option<&'static str> {
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

fn chat_surface(strict_compatible: bool) -> ApiSurface {
    if strict_compatible {
        ApiSurface::StrictChat
    } else {
        ApiSurface::StandardChat
    }
}

fn chat_url(root: &str, surface: ApiSurface) -> String {
    let root = root.trim_end_matches('/');
    if surface == ApiSurface::StrictChat {
        let unversioned = root.strip_suffix("/v1").unwrap_or(root);
        format!("{unversioned}/beta/chat/completions")
    } else {
        format!("{root}/chat/completions")
    }
}

fn planned_tool_to_chat(tool: &PlannedTool, strict: bool) -> Value {
    let mut value = json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    });
    if strict {
        value["function"]["strict"] = json!(true);
    }
    value
}

fn runtime_chat_messages<N, F>(
    request: &ModelRequest,
    wire_model: &str,
    project_tool_name: &mut N,
    project_tool_result: &mut F,
) -> Vec<Value>
where
    N: FnMut(&str) -> String,
    F: FnMut(&str, &Value, &str, &str) -> String,
{
    let mut messages = Vec::new();
    let mut pending_tool_calls: HashMap<String, (String, Value)> = HashMap::new();
    if let Some(system) = runtime_system_instructions(&request.system_prompt) {
        messages.push(json!({"role": "system", "content": system}));
    }

    let replay_current_reasoning =
        ReasoningMode::from_runtime(request.reasoning_effort).thinking_enabled();
    for (message_index, message) in request.messages.iter().enumerate() {
        match message {
            ModelMessage::User { content } => {
                pending_tool_calls.clear();
                messages.push(json!({"role": "user", "content": content}));
            }
            ModelMessage::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => {
                let replay_tool_reasoning =
                    !tool_calls.is_empty() && requires_tool_call_reasoning_replay(wire_model);
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
                                    "name": project_tool_name(&call.name),
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
                    continue;
                };
                let content = project_tool_result(
                    &tool_name,
                    &input,
                    content,
                    &format!("Message #{message_index}"),
                );
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content,
                }));
            }
        }
    }
    messages
}

fn runtime_system_instructions(system: &SystemPrompt) -> Option<String> {
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
    if !requires_tool_call_reasoning_replay(model) {
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

fn validate_exact_reasoning_replay(body: &Value, model: &str) -> Result<(), ChatPlanError> {
    if !requires_tool_call_reasoning_replay(model) {
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
        if has_tool_calls && has_empty_reasoning {
            return Err(ChatPlanError::MissingReasoningContent { message_index });
        }
    }
    Ok(())
}

fn requires_tool_call_reasoning_replay(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("deepseek-v4")
        || lower.starts_with("deepseek-chat")
        || lower.starts_with("deepseek-reasoner")
        || lower.contains("reasoner")
        || lower.contains("-reasoning")
        || lower.contains("-thinking")
        || has_deepseek_r_series_marker(&lower)
}

fn has_deepseek_r_series_marker(model_lower: &str) -> bool {
    const PREFIX: &str = "deepseek-r";
    model_lower.match_indices(PREFIX).any(|(index, _)| {
        model_lower[index + PREFIX.len()..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })
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

/// Validate a schema against DeepSeek Beta Strict Function Calling's complete
/// documented subset. Unsupported catalogs must fall back as a whole.
#[must_use]
pub fn strict_schema_supported(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        && strict_schema_node_supported(schema)
}

fn strict_schema_node_supported(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    if object
        .get("description")
        .is_some_and(|description| !description.is_string())
        || !strict_definitions_supported(object)
    {
        return false;
    }

    if let Some(reference) = object.get("$ref") {
        return reference.is_string() && strict_keys_supported(object, &["$ref", "description"]);
    }
    if let Some(branches) = object.get("anyOf") {
        let Some(branches) = branches.as_array().filter(|branches| !branches.is_empty()) else {
            return false;
        };
        return strict_keys_supported(object, &["anyOf", "description", "$def"])
            && branches.iter().all(strict_schema_node_supported);
    }

    let Some(schema_type) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    if let Some(values) = object.get("enum")
        && !values.as_array().is_some_and(|values| !values.is_empty())
    {
        return false;
    }

    match schema_type {
        "object" => {
            if !strict_keys_supported(
                object,
                &[
                    "type",
                    "description",
                    "properties",
                    "required",
                    "additionalProperties",
                    "$def",
                ],
            ) || object.get("additionalProperties").and_then(Value::as_bool) != Some(false)
            {
                return false;
            }
            let Some(properties) = object.get("properties").and_then(Value::as_object) else {
                return false;
            };
            let Some(required_values) = object.get("required").and_then(Value::as_array) else {
                return false;
            };
            let Some(mut required) = required_values
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
            property_names.sort_unstable();
            required.sort_unstable();
            required == property_names && properties.values().all(strict_schema_node_supported)
        }
        "string" => {
            strict_keys_supported(
                object,
                &["type", "description", "pattern", "format", "enum", "$def"],
            ) && object.get("pattern").is_none_or(Value::is_string)
                && strict_enum_values_match(object, Value::is_string)
                && object.get("format").is_none_or(|format| {
                    matches!(
                        format.as_str(),
                        Some("email" | "hostname" | "ipv4" | "ipv6" | "uuid")
                    )
                })
        }
        "number" | "integer" => {
            strict_keys_supported(
                object,
                &[
                    "type",
                    "description",
                    "const",
                    "default",
                    "minimum",
                    "maximum",
                    "exclusiveMinimum",
                    "exclusiveMaximum",
                    "multipleOf",
                    "enum",
                    "$def",
                ],
            ) && [
                "const",
                "default",
                "minimum",
                "maximum",
                "exclusiveMinimum",
                "exclusiveMaximum",
                "multipleOf",
            ]
            .iter()
            .all(|key| object.get(*key).is_none_or(Value::is_number))
                && strict_enum_values_match(object, Value::is_number)
        }
        "boolean" => {
            strict_keys_supported(object, &["type", "description", "enum", "$def"])
                && strict_enum_values_match(object, Value::is_boolean)
        }
        "array" => {
            strict_keys_supported(object, &["type", "description", "items", "enum", "$def"])
                && object
                    .get("items")
                    .is_some_and(strict_schema_node_supported)
                && strict_enum_values_match(object, Value::is_array)
        }
        _ => false,
    }
}

fn strict_definitions_supported(object: &Map<String, Value>) -> bool {
    let Some(definitions) = object.get("$def") else {
        return true;
    };
    definitions
        .as_object()
        .is_some_and(|definitions| definitions.values().all(strict_schema_node_supported))
}

fn strict_keys_supported(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    object.keys().all(|key| allowed.contains(&key.as_str()))
}

fn strict_enum_values_match(
    object: &Map<String, Value>,
    predicate: impl Fn(&Value) -> bool,
) -> bool {
    object.get("enum").is_none_or(|values| {
        values
            .as_array()
            .is_some_and(|values| values.iter().all(predicate))
    })
}

#[cfg(test)]
mod tests {
    use codewhale_runtime::{AgentActor, ModelToolCall, RunId, ToolArguments, ToolDefinition};

    use super::*;

    fn compatible_tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: format!("{name} description"),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        }
    }

    fn runtime_request(streaming: bool) -> ModelRequest {
        ModelRequest {
            run_id: RunId::from("run-planner"),
            parent_run_id: None,
            actor: AgentActor::default(),
            model: "deepseek-v4-pro".to_owned(),
            system_prompt: SystemPrompt::from_text("稳定系统提示"),
            messages: vec![
                ModelMessage::Assistant {
                    content: None,
                    reasoning_content: Some("原始推理".to_owned()),
                    tool_calls: vec![ModelToolCall {
                        id: "call-1".to_owned(),
                        name: "read_file".to_owned(),
                        arguments: ToolArguments::parse(
                            "{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }",
                        ),
                    }],
                },
                ModelMessage::Tool {
                    call_id: "call-1".to_owned(),
                    name: "read_file".to_owned(),
                    content: "ok".to_owned(),
                },
            ],
            tools: vec![compatible_tool("read_file")],
            reasoning_effort: ReasoningEffort::Max,
            max_output_tokens: Some(64),
            streaming,
            request_number: 1,
            attempt: 0,
        }
    }

    #[test]
    fn canonical_projection_is_stable_and_replays_raw_arguments_exactly() {
        let request = runtime_request(true);
        let project = |_: &str, _: &Value, content: &str, _: &str| content.to_owned();
        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: false,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
            str::to_owned,
            project,
        )
        .unwrap();
        let repeated = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: false,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
            str::to_owned,
            |_, _, content, _| content.to_owned(),
        )
        .unwrap();

        assert_eq!(plan, repeated);
        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(plan.body["reasoning_effort"], "max");
        assert_eq!(
            plan.body["messages"][1]["tool_calls"][0]["function"]["arguments"],
            "{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }"
        );
    }

    #[test]
    fn strict_catalog_falls_back_atomically_without_losing_tools() {
        let mut request = runtime_request(false);
        request.tools.push(ToolDefinition {
            name: "optional".to_owned(),
            description: "not strict compatible".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": [],
                "additionalProperties": false
            }),
        });
        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: true,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
            str::to_owned,
            |_, _, content, _| content.to_owned(),
        )
        .unwrap();

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.body["tools"].as_array().unwrap().len(), 2);
        assert!(
            plan.body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool["function"].get("strict").is_none())
        );
    }

    #[test]
    fn chat_plan_errors_preserve_preflight_semantics() {
        assert_eq!(
            ChatPlanError::MissingReasoningContent { message_index: 2 }.to_string(),
            "Invalid DeepSeek request: exact reasoning replay requires assistant message 2 to contain its original reasoning_content; HTTP request was not sent"
        );
        assert_eq!(
            ChatPlanError::AmbiguousReasoningContent {
                message_index: 3,
                block_count: 2,
            }
            .to_string(),
            "Invalid DeepSeek request: assistant message 3 contains 2 reasoning blocks, so the original single reasoning_content value cannot be reconstructed exactly; HTTP request was not sent"
        );
    }

    #[test]
    fn loopback_root_keeps_standard_v1_and_beta_as_distinct_surfaces() {
        let standard = plan_chat(
            "http://127.0.0.1:43123/v1",
            false,
            ChatPlanInput {
                model: "deepseek-v4-pro".to_owned(),
                messages: Vec::new(),
                max_tokens: 64,
                response_mode: ResponseMode::NonStreaming,
                tools: None,
                tool_choice: None,
                reasoning: ReasoningMode::Auto,
                temperature: None,
                top_p: None,
            },
        )
        .unwrap();
        let strict = plan_chat(
            "http://127.0.0.1:43123/v1",
            true,
            ChatPlanInput {
                model: "deepseek-v4-pro".to_owned(),
                messages: Vec::new(),
                max_tokens: 64,
                response_mode: ResponseMode::NonStreaming,
                tools: Some(vec![PlannedTool {
                    name: "read_file".to_owned(),
                    description: "read".to_owned(),
                    input_schema: compatible_tool("read_file").input_schema,
                }]),
                tool_choice: None,
                reasoning: ReasoningMode::Auto,
                temperature: None,
                top_p: None,
            },
        )
        .unwrap();

        assert_eq!(standard.url, "http://127.0.0.1:43123/v1/chat/completions");
        assert_eq!(strict.url, "http://127.0.0.1:43123/beta/chat/completions");
    }
}
