//! Deterministic request planning for the official DeepSeek API.
//!
//! It owns the official wire decision, canonical `ModelRequest` projection,
//! physical request admission, and provider-reported usage ledger. The HTTP
//! sender and SSE decoder for production Chat traffic.
//!
//! The next deletion point is the TUI legacy `MessageRequest` projection and
//! its local replay preflight, after the interactive loop emits canonical
//! `ModelRequest` directly.

use std::collections::HashSet;
use std::fmt;

use codewhale_runtime::{ModelMessage, ModelRequest, ReasoningEffort, SystemPrompt};
use serde_json::{Map, Value, json};

mod accounting;
mod auto_route;
mod model_port;
mod pricing;
mod transport;

pub use accounting::{
    ApiRequestActor, ApiRequestActorSnapshot, ApiRequestBudgetError, ApiRequestBudgetSnapshot,
    ApiRequestKind, ApiRequestLease, ApiResponseAccountingGuard, ApiUsageBucket, ApiUsageSnapshot,
    SharedApiRequestBudget,
};
pub use auto_route::{
    DEEPSEEK_AUTO_ROUTE_FLASH_MODEL, DEEPSEEK_AUTO_ROUTE_PRO_MODEL, DeepSeekAutoRouteError,
    DeepSeekAutoRouteFallback, DeepSeekAutoRouteInput, DeepSeekAutoRouteSelection,
    DeepSeekAutoRouteSource, resolve_deepseek_auto_route,
};
pub use model_port::{
    DeepSeekModelPort, OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS, OFFICIAL_V4_CONTEXT_WINDOW_TOKENS,
    OFFICIAL_V4_MAX_OUTPUT_TOKENS, OfficialModelCapabilities, OfficialModelCapabilityError,
    model_accounting_snapshot, official_model_capabilities, resume_api_request_budget,
};
pub use pricing::{
    CostEstimate, CurrencyPricing, ModelPricing, calculate_turn_cost_estimate,
    pricing_for_official_model,
};
pub use transport::{
    DeepSeekConnectionConfig, DeepSeekCredential, DeepSeekEndpoint, DeepSeekResponse,
    DeepSeekStream, DeepSeekTransport, DeepSeekTransportConfig, DeepSeekTransportError,
    TransportRetryPolicy, decode_tool_name, encode_tool_name, parse_chat_response,
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
    /// Frozen decision for Chat tool routing. FIM has no tool surface.
    pub tool_surface: Option<ToolSurfaceDecision>,
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
    MissingToolResults {
        message_index: usize,
        unresolved_count: usize,
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
            Self::MissingToolResults {
                message_index,
                unresolved_count,
            } => write!(
                formatter,
                "Invalid DeepSeek request: assistant message {message_index} has {unresolved_count} tool call(s) without canonical results; HTTP request was not sent"
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictSchemaIssue {
    pub path: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSurfaceReason {
    Disabled,
    NoTools,
    Compatible,
    IncompatibleCatalog {
        tool_name: String,
        issue: StrictSchemaIssue,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceDecision {
    pub surface: ApiSurface,
    pub strict_compatible: bool,
    pub strict_fallback: bool,
    pub reason: ToolSurfaceReason,
}

/// Decide strict mode atomically for a complete DeepSeek tool catalog.
///
/// A single incompatible schema keeps the entire catalog on ordinary Chat;
/// callers must never drop individual tools to force Beta Strict mode.
pub fn plan_tool_surface(strict_enabled: bool, tools: &[PlannedTool]) -> ToolSurfaceDecision {
    let reason = if !strict_enabled {
        ToolSurfaceReason::Disabled
    } else if tools.is_empty() {
        ToolSurfaceReason::NoTools
    } else if let Some((tool, issue)) = tools
        .iter()
        .find_map(|tool| strict_schema_issue(&tool.input_schema).map(|issue| (tool, issue)))
    {
        ToolSurfaceReason::IncompatibleCatalog {
            tool_name: tool.name.clone(),
            issue,
        }
    } else {
        ToolSurfaceReason::Compatible
    };
    let strict_compatible = reason == ToolSurfaceReason::Compatible;
    ToolSurfaceDecision {
        surface: chat_surface(strict_compatible),
        strict_compatible,
        strict_fallback: matches!(reason, ToolSurfaceReason::IncompatibleCatalog { .. }),
        reason,
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
    let tool_surface =
        plan_tool_surface(strict_enabled, input.tools.as_deref().unwrap_or_default());
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
    validate_exact_reasoning_replay(&body, &model, input.reasoning.thinking_enabled())?;
    let surface = tool_surface.surface;
    Ok(RequestPlan {
        surface,
        tool_surface: Some(tool_surface),
        url: chat_url(root, surface),
        model,
        reasoning_replay_tokens: reasoning_replay_tokens(&body),
        body,
        response_mode: input.response_mode,
    })
}

/// Project one canonical runtime request and freeze its official Chat plan.
///
/// Canonical tool results have already passed the runtime/tool output policy.
/// The provider layer replays them exactly and owns tool-name projection,
/// message ordering, reasoning/raw-argument replay, strict selection, and
/// every remaining wire decision.
pub fn plan_runtime_chat(
    input: RuntimeChatPlanInput<'_>,
    request: &ModelRequest,
) -> Result<RequestPlan, ChatPlanError> {
    validate_runtime_reasoning_replay(request, &input.wire_model)?;
    let response_mode = if request.streaming {
        ResponseMode::Streaming
    } else {
        ResponseMode::NonStreaming
    };
    let messages = runtime_chat_messages(request, &input.wire_model)?;
    let tools = (!request.tools.is_empty()).then(|| {
        request
            .tools
            .iter()
            .map(|tool| PlannedTool {
                name: encode_tool_name(&tool.name),
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
        tool_surface: None,
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

fn runtime_chat_messages(
    request: &ModelRequest,
    wire_model: &str,
) -> Result<Vec<Value>, ChatPlanError> {
    let mut messages = Vec::new();
    let mut pending_tool_calls: HashSet<String> = HashSet::new();
    let mut pending_assistant_index = None;
    let mut deferred_users = Vec::new();
    if let Some(system) = runtime_system_instructions(&request.system_prompt) {
        messages.push(json!({"role": "system", "content": system}));
    }

    let replay_current_reasoning =
        ReasoningMode::from_runtime(request.reasoning_effort).thinking_enabled();
    for (message_index, message) in request.messages.iter().enumerate() {
        match message {
            ModelMessage::User { content } => {
                let wire = json!({"role": "user", "content": content});
                if pending_tool_calls.is_empty() {
                    messages.push(wire);
                } else {
                    deferred_users.push(wire);
                }
            }
            ModelMessage::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => {
                if !pending_tool_calls.is_empty() {
                    return Err(ChatPlanError::MissingToolResults {
                        message_index: pending_assistant_index.unwrap_or(message_index),
                        unresolved_count: pending_tool_calls.len(),
                    });
                }
                let replay_tool_reasoning = !tool_calls.is_empty()
                    && replay_current_reasoning
                    && requires_tool_call_reasoning_replay(wire_model);
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
                                        "name": encode_tool_name(&call.name),
                                        "arguments": call.arguments.raw,
                                    }
                                })
                            })
                            .collect(),
                    );
                    pending_tool_calls = tool_calls.iter().map(|call| call.id.clone()).collect();
                    pending_assistant_index = Some(message_index);
                } else {
                    pending_tool_calls.clear();
                    pending_assistant_index = None;
                }
                messages.push(wire);
            }
            ModelMessage::Tool {
                call_id, content, ..
            } => {
                if !pending_tool_calls.remove(call_id) {
                    continue;
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content,
                }));
                if pending_tool_calls.is_empty() {
                    pending_assistant_index = None;
                    messages.append(&mut deferred_users);
                }
            }
        }
    }
    if !pending_tool_calls.is_empty() {
        return Err(ChatPlanError::MissingToolResults {
            message_index: pending_assistant_index.unwrap_or(request.messages.len()),
            unresolved_count: pending_tool_calls.len(),
        });
    }
    Ok(messages)
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
    if !ReasoningMode::from_runtime(request.reasoning_effort).thinking_enabled()
        || !requires_tool_call_reasoning_replay(model)
    {
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
        if !tool_calls.is_empty() && reasoning_content.as_deref().is_none_or(str::is_empty) {
            return Err(ChatPlanError::MissingReasoningContent { message_index });
        }
    }
    Ok(())
}

fn validate_exact_reasoning_replay(
    body: &Value,
    model: &str,
    thinking_enabled: bool,
) -> Result<(), ChatPlanError> {
    if !thinking_enabled || !requires_tool_call_reasoning_replay(model) {
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
        let missing_reasoning = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty);
        if has_tool_calls && missing_reasoning {
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
    strict_schema_issue(schema).is_none()
}

#[must_use]
pub fn strict_schema_issue(schema: &Value) -> Option<StrictSchemaIssue> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Some(strict_issue("$", "root_object_required"));
    }
    diagnose_strict_schema_node(schema, schema, "$").err()
}

fn diagnose_strict_schema_node(
    schema: &Value,
    root: &Value,
    path: &str,
) -> Result<(), StrictSchemaIssue> {
    let Some(object) = schema.as_object() else {
        return Err(strict_issue(path, "schema_object_required"));
    };
    if object
        .get("description")
        .is_some_and(|value| !value.is_string())
    {
        return Err(strict_issue(
            &format!("{path}/description"),
            "invalid_description",
        ));
    }
    if let Some(definitions) = object.get("$def") {
        let definitions = definitions
            .as_object()
            .ok_or_else(|| strict_issue(&format!("{path}/$def"), "invalid_definitions"))?;
        let mut names = definitions.keys().collect::<Vec<_>>();
        names.sort_unstable();
        for name in names {
            diagnose_strict_schema_node(
                &definitions[name],
                root,
                &format!("{path}/$def/{}", escape_json_pointer(name)),
            )?;
        }
    }

    if let Some(reference) = object.get("$ref") {
        first_unsupported_key(object, &["$ref", "description"])
            .map(|key| strict_issue(&format!("{path}/{key}"), "unsupported_keyword"))
            .map_or(Ok(()), Err)?;
        let reference = reference
            .as_str()
            .ok_or_else(|| strict_issue(&format!("{path}/$ref"), "invalid_ref"))?;
        if !reference.starts_with("#/$def/") || root.pointer(&reference[1..]).is_none() {
            return Err(strict_issue(&format!("{path}/$ref"), "invalid_ref"));
        }
        return Ok(());
    }
    if let Some(branches) = object.get("anyOf") {
        first_unsupported_key(object, &["anyOf", "description", "$def"])
            .map(|key| strict_issue(&format!("{path}/{key}"), "unsupported_keyword"))
            .map_or(Ok(()), Err)?;
        let branches = branches
            .as_array()
            .filter(|branches| !branches.is_empty())
            .ok_or_else(|| strict_issue(&format!("{path}/anyOf"), "invalid_any_of"))?;
        for (index, branch) in branches.iter().enumerate() {
            diagnose_strict_schema_node(branch, root, &format!("{path}/anyOf/{index}"))?;
        }
        return Ok(());
    }

    let schema_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| strict_issue(&format!("{path}/type"), "invalid_type"))?;
    if let Some(values) = object.get("enum")
        && !values.as_array().is_some_and(|values| !values.is_empty())
    {
        return Err(strict_issue(&format!("{path}/enum"), "invalid_enum"));
    }

    match schema_type {
        "object" => {
            let allowed = [
                "type",
                "description",
                "properties",
                "required",
                "additionalProperties",
                "$def",
            ];
            if let Some(key) = first_unsupported_key(object, &allowed) {
                return Err(strict_issue(
                    &format!("{path}/{key}"),
                    "unsupported_keyword",
                ));
            }
            if object.get("additionalProperties").and_then(Value::as_bool) != Some(false) {
                return Err(strict_issue(
                    &format!("{path}/additionalProperties"),
                    "closed_object_required",
                ));
            }
            let properties = object
                .get("properties")
                .and_then(Value::as_object)
                .ok_or_else(|| strict_issue(&format!("{path}/properties"), "invalid_properties"))?;
            let required_values = object
                .get("required")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    strict_issue(&format!("{path}/required"), "all_properties_required")
                })?;
            let Some(mut required) = required_values
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()
            else {
                return Err(strict_issue(
                    &format!("{path}/required"),
                    "all_properties_required",
                ));
            };
            let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
            property_names.sort_unstable();
            required.sort_unstable();
            if required != property_names {
                return Err(strict_issue(
                    &format!("{path}/required"),
                    "all_properties_required",
                ));
            }
            for name in property_names {
                diagnose_strict_schema_node(
                    &properties[name],
                    root,
                    &format!("{path}/properties/{}", escape_json_pointer(name)),
                )?;
            }
            Ok(())
        }
        "string" => {
            diagnose_scalar_keys(
                object,
                path,
                &["type", "description", "pattern", "format", "enum", "$def"],
            )?;
            if object
                .get("pattern")
                .is_some_and(|value| !value.is_string())
            {
                return Err(strict_issue(&format!("{path}/pattern"), "invalid_pattern"));
            }
            if !strict_enum_values_match(object, Value::is_string) {
                return Err(strict_issue(&format!("{path}/enum"), "invalid_enum"));
            }
            if !object.get("format").is_none_or(|format| {
                matches!(
                    format.as_str(),
                    Some("email" | "hostname" | "ipv4" | "ipv6" | "uuid")
                )
            }) {
                return Err(strict_issue(&format!("{path}/format"), "invalid_format"));
            }
            Ok(())
        }
        "number" | "integer" => {
            diagnose_scalar_keys(
                object,
                path,
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
            )?;
            if ![
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
            {
                return Err(strict_issue(path, "invalid_number_constraint"));
            }
            if schema_type == "integer"
                && [
                    "const",
                    "default",
                    "minimum",
                    "maximum",
                    "exclusiveMinimum",
                    "exclusiveMaximum",
                    "multipleOf",
                ]
                .iter()
                .any(|key| {
                    object
                        .get(*key)
                        .is_some_and(|value| value.as_i64().is_none() && value.as_u64().is_none())
                })
            {
                return Err(strict_issue(path, "invalid_integer_constraint"));
            }
            if object
                .get("multipleOf")
                .and_then(Value::as_f64)
                .is_some_and(|value| value <= 0.0)
            {
                return Err(strict_issue(
                    &format!("{path}/multipleOf"),
                    "invalid_multiple_of",
                ));
            }
            if !strict_enum_values_match(object, |value| {
                if schema_type == "integer" {
                    value.as_i64().is_some() || value.as_u64().is_some()
                } else {
                    value.is_number()
                }
            }) {
                return Err(strict_issue(&format!("{path}/enum"), "invalid_enum"));
            }
            Ok(())
        }
        "boolean" => {
            diagnose_scalar_keys(object, path, &["type", "description", "enum", "$def"])?;
            if !strict_enum_values_match(object, Value::is_boolean) {
                return Err(strict_issue(&format!("{path}/enum"), "invalid_enum"));
            }
            Ok(())
        }
        "array" => {
            diagnose_scalar_keys(
                object,
                path,
                &["type", "description", "items", "enum", "$def"],
            )?;
            let items = object
                .get("items")
                .ok_or_else(|| strict_issue(&format!("{path}/items"), "items_required"))?;
            diagnose_strict_schema_node(items, root, &format!("{path}/items"))?;
            if !strict_enum_values_match(object, Value::is_array) {
                return Err(strict_issue(&format!("{path}/enum"), "invalid_enum"));
            }
            Ok(())
        }
        _ => Err(strict_issue(&format!("{path}/type"), "unsupported_type")),
    }
}

fn diagnose_scalar_keys(
    object: &Map<String, Value>,
    path: &str,
    allowed: &[&str],
) -> Result<(), StrictSchemaIssue> {
    match first_unsupported_key(object, allowed) {
        Some(key) => Err(strict_issue(
            &format!("{path}/{key}"),
            "unsupported_keyword",
        )),
        None => Ok(()),
    }
}

fn first_unsupported_key<'a>(object: &'a Map<String, Value>, allowed: &[&str]) -> Option<&'a str> {
    let mut unsupported = object
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect::<Vec<_>>();
    unsupported.sort_unstable();
    unsupported.into_iter().next()
}

fn strict_issue(path: &str, code: &str) -> StrictSchemaIssue {
    StrictSchemaIssue {
        path: path.to_owned(),
        code: code.to_owned(),
    }
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
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
    use codewhale_context::compaction::{
        ContextCompactionPreparation, ContextInput, effective_context, prepare_compaction,
    };
    use codewhale_runtime::{
        AgentActor, CanonicalTranscript, ContextPolicy, ModelToolCall, RunId, TaskContract,
        TaskDefinition, TaskGenerationId, ToolArguments, ToolDefinition, ToolOutcome,
        TranscriptEntry, WorkspaceRevision, WorkspaceState,
    };

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
        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: false,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
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
    fn child_handoff_is_deferred_until_all_deepseek_tool_results() {
        let mut request = runtime_request(true);
        request.messages.insert(
            1,
            ModelMessage::User {
                content: "子 Agent 已完成并交回结构化结果".to_owned(),
            },
        );

        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: false,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
        )
        .expect("child handoff must not break DeepSeek tool-call adjacency");
        let messages = plan.body["messages"].as_array().expect("wire messages");
        let assistant_index = messages
            .iter()
            .position(|message| message["role"] == "assistant")
            .expect("assistant tool call");
        assert_eq!(messages[assistant_index + 1]["role"], "tool");
        assert_eq!(messages[assistant_index + 1]["tool_call_id"], "call-1");
        assert_eq!(messages[assistant_index + 2]["role"], "user");
        assert_eq!(
            messages[assistant_index + 2]["content"],
            "子 Agent 已完成并交回结构化结果"
        );
    }

    #[test]
    fn missing_canonical_tool_result_fails_before_http() {
        let mut request = runtime_request(true);
        request.messages.pop();
        request.messages.push(ModelMessage::User {
            content: "不能越过尚未结算的工具调用".to_owned(),
        });

        assert_eq!(
            plan_runtime_chat(
                RuntimeChatPlanInput {
                    root: "https://api.deepseek.com",
                    strict_enabled: false,
                    wire_model: request.model.clone(),
                    max_tokens: 64,
                },
                &request,
            ),
            Err(ChatPlanError::MissingToolResults {
                message_index: 0,
                unresolved_count: 1,
            })
        );
    }

    #[test]
    fn thinking_tool_call_without_original_reasoning_fails_before_http() {
        let mut request = runtime_request(true);
        let ModelMessage::Assistant {
            reasoning_content, ..
        } = &mut request.messages[0]
        else {
            panic!("fixture begins with an assistant tool call");
        };
        *reasoning_content = None;

        assert_eq!(
            plan_runtime_chat(
                RuntimeChatPlanInput {
                    root: "https://api.deepseek.com",
                    strict_enabled: false,
                    wire_model: request.model.clone(),
                    max_tokens: 64,
                },
                &request,
            ),
            Err(ChatPlanError::MissingReasoningContent { message_index: 0 })
        );
    }

    #[test]
    fn non_thinking_tool_call_without_reasoning_remains_valid() {
        let mut request = runtime_request(true);
        request.reasoning_effort = ReasoningEffort::Off;
        let ModelMessage::Assistant {
            reasoning_content, ..
        } = &mut request.messages[0]
        else {
            panic!("fixture begins with an assistant tool call");
        };
        *reasoning_content = None;

        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: false,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
        )
        .expect("non-thinking tool history does not require reasoning_content");
        assert!(plan.body["messages"][1].get("reasoning_content").is_none());
    }

    #[test]
    fn compacted_context_reaches_beta_chat_without_rewriting_reasoning_or_tools() {
        let task_contract = TaskContract {
            generation_id: TaskGenerationId::from("task-wire-compaction"),
            definition: TaskDefinition::host("压缩后继续检查 src/latest.rs"),
        };
        let workspace = WorkspaceState {
            generation: 7,
            revision: WorkspaceRevision::Known {
                sha256: "sha256:wire-current".to_owned(),
            },
        };
        let tools = vec![compatible_tool("read_file")];
        let mut transcript = CanonicalTranscript {
            entries: vec![
                TranscriptEntry::System {
                    prompt: SystemPrompt::from_text("稳定系统提示"),
                },
                TranscriptEntry::User {
                    content: task_contract.definition.model_message(),
                },
            ],
        };
        for index in 0..6 {
            let call_id = format!("old-call-{index}");
            transcript.entries.push(TranscriptEntry::Assistant {
                content: None,
                reasoning_content: Some(format!("旧推理-{index}")),
                tool_calls: vec![ModelToolCall {
                    id: call_id.clone(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(
                        json!({"path": format!("src/old-{index}.rs")}),
                    ),
                }],
            });
            transcript.entries.push(TranscriptEntry::Tool {
                call_id,
                name: "read_file".to_owned(),
                outcome: Box::new(ToolOutcome::success("可裁剪旧结果".repeat(2_000))),
            });
        }
        let raw_arguments = "{ \"z\" : 1, \"path\" : \"src/latest.rs\", \"a\" : 2 }";
        let exact_reasoning = "WIRE_REASONING_SENTINEL：必须原样重放";
        let exact_result = "WIRE_RESULT_SENTINEL".repeat(128);
        transcript.entries.push(TranscriptEntry::Assistant {
            content: None,
            reasoning_content: Some(exact_reasoning.to_owned()),
            tool_calls: vec![ModelToolCall {
                id: "wire-call".to_owned(),
                name: "read_file".to_owned(),
                arguments: ToolArguments::parse(raw_arguments),
            }],
        });
        transcript.entries.push(TranscriptEntry::Tool {
            call_id: "wire-call".to_owned(),
            name: "read_file".to_owned(),
            outcome: Box::new(ToolOutcome::success(exact_result.clone())),
        });

        let no_receipts = Vec::new();
        let input = ContextInput {
            transcript: &transcript,
            projection: None,
            task_contract: Some(&task_contract),
            workspace_state: &workspace,
            evidence_receipts: &no_receipts,
            last_completion_rejection: None,
            last_verifier_failure: None,
            last_verifier_failure_workspace: None,
            tools: &tools,
        };
        let ContextCompactionPreparation::Local { projection, .. } = prepare_compaction(
            input,
            ContextPolicy {
                hard_input_tokens: 6_000,
            },
        )
        .expect("deterministic compaction") else {
            panic!("long fixture must produce a local compaction");
        };
        let projected = effective_context(ContextInput {
            projection: Some(&projection),
            ..input
        })
        .expect("materialize compacted context");
        assert!(
            projected.messages.iter().all(|message| !matches!(
                message,
                ModelMessage::Tool { content, .. } if content.contains("可裁剪旧结果")
            )),
            "obsolete tool output should not survive the frozen projection"
        );

        let request = ModelRequest {
            run_id: RunId::from("run-wire-compaction"),
            parent_run_id: None,
            actor: AgentActor::default(),
            model: "deepseek-v4-pro".to_owned(),
            system_prompt: projected.system_prompt,
            messages: projected.messages,
            tools,
            reasoning_effort: ReasoningEffort::Max,
            max_output_tokens: Some(64),
            streaming: true,
            request_number: 1,
            attempt: 0,
        };
        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: true,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
        )
        .expect("plan official DeepSeek request");

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert_eq!(plan.url, "https://api.deepseek.com/beta/chat/completions");
        assert_eq!(
            plan.body["messages"][0],
            json!({"role": "system", "content": "稳定系统提示"})
        );
        assert_eq!(plan.body["tools"].as_array().map(Vec::len), Some(1));
        assert_eq!(plan.body["tools"][0]["function"]["strict"], true);
        let messages = plan.body["messages"]
            .as_array()
            .expect("wire messages are an array");
        let assistant_index = messages
            .iter()
            .position(|message| message["tool_calls"][0]["id"].as_str() == Some("wire-call"))
            .expect("selected assistant tool call reaches the wire");
        assert_eq!(
            messages[assistant_index]["reasoning_content"],
            exact_reasoning
        );
        assert_eq!(
            messages[assistant_index]["tool_calls"][0]["function"]["arguments"],
            raw_arguments
        );
        assert_eq!(messages[assistant_index + 1]["role"], "tool");
        assert_eq!(messages[assistant_index + 1]["tool_call_id"], "wire-call");
        assert_eq!(messages[assistant_index + 1]["content"], exact_result);
    }

    #[test]
    fn official_root_routes_compatible_strict_catalog_to_beta_chat() {
        let request = runtime_request(false);
        let plan = plan_runtime_chat(
            RuntimeChatPlanInput {
                root: "https://api.deepseek.com",
                strict_enabled: true,
                wire_model: request.model.clone(),
                max_tokens: 64,
            },
            &request,
        )
        .unwrap();

        assert_eq!(plan.surface, ApiSurface::StrictChat);
        assert_eq!(plan.url, "https://api.deepseek.com/beta/chat/completions");
        assert_eq!(plan.body["tools"][0]["function"]["strict"], true);
        assert_eq!(
            plan.tool_surface.as_ref().map(|decision| &decision.reason),
            Some(&ToolSurfaceReason::Compatible)
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
        )
        .unwrap();

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(
            plan.tool_surface.as_ref().map(|decision| &decision.reason),
            Some(&ToolSurfaceReason::IncompatibleCatalog {
                tool_name: "optional".to_owned(),
                issue: StrictSchemaIssue {
                    path: "$/required".to_owned(),
                    code: "all_properties_required".to_owned(),
                },
            })
        );
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
    fn strict_schema_diagnostics_are_stable_for_documented_boundaries() {
        for (schema, expected_path, expected_code) in [
            (
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }),
                "$/additionalProperties",
                "closed_object_required",
            ),
            (
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "minLength": 1}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
                "$/properties/path/minLength",
                "unsupported_keyword",
            ),
            (
                json!({
                    "type": "object",
                    "properties": {"items": {"type": "array", "items": {"type": "string"}, "minItems": 1}},
                    "required": ["items"],
                    "additionalProperties": false
                }),
                "$/properties/items/minItems",
                "unsupported_keyword",
            ),
            (
                json!({
                    "type": "object",
                    "properties": {"path": {"$ref": "#/$def/missing"}},
                    "required": ["path"],
                    "additionalProperties": false,
                    "$def": {"known": {"type": "string"}}
                }),
                "$/properties/path/$ref",
                "invalid_ref",
            ),
        ] {
            assert_eq!(
                strict_schema_issue(&schema),
                Some(StrictSchemaIssue {
                    path: expected_path.to_owned(),
                    code: expected_code.to_owned(),
                })
            );
        }

        let recursive = json!({
            "type": "object",
            "properties": {"node": {"$ref": "#/$def/node"}},
            "required": ["node"],
            "additionalProperties": false,
            "$def": {
                "node": {
                    "type": "object",
                    "properties": {"next": {"$ref": "#/$def/node"}},
                    "required": ["next"],
                    "additionalProperties": false
                }
            }
        });
        assert_eq!(strict_schema_issue(&recursive), None);
    }

    #[test]
    fn official_root_routes_fim_to_beta_completions() {
        let plan = plan_fim("https://api.deepseek.com", None, "fn main() {", "}", 64)
            .expect("valid official FIM plan");

        assert_eq!(plan.surface, ApiSurface::Fim);
        assert_eq!(plan.url, "https://api.deepseek.com/beta/completions");
        assert_eq!(plan.model, FIM_MODEL);
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
