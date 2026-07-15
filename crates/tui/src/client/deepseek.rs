//! Pure request planning for the official DeepSeek OpenAI-format API.

use std::fmt;

use serde_json::{Value, json};

use crate::config::{ApiProvider, wire_model_for_provider};
use crate::models::{MessageRequest, Tool};

pub(crate) const FIM_MODEL: &str = "deepseek-v4-pro";

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

    let mut planned_tools = tools.map(<[Tool]>::to_vec);
    let strict_requested = strict_enabled
        && planned_tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty());
    let strict_compatible = strict_requested
        && planned_tools.as_mut().is_some_and(|tools| {
            crate::tools::schema_sanitize::prepare_tools_for_strict_mode(tools)
        });

    if !strict_compatible && let Some(tools) = planned_tools.as_mut() {
        clear_strict(tools);
    }

    let surface = if strict_compatible {
        ApiSurface::StrictChat
    } else {
        ApiSurface::StandardChat
    };
    Some(ToolPlan {
        surface,
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
) -> Option<RequestPlan> {
    let root = official_root(provider, base_url)?;
    let wire_model = wire_model_for_provider(provider, &request.model);
    // Prompt projection must use the same canonical model identity that will
    // be sent on the wire. Compact aliases such as `pro` and `flash` do not
    // themselves carry V4 reasoning semantics; using the raw alias here would
    // discard exact reasoning history before the request is serialized.
    let mut wire_request = request.clone();
    wire_request.model.clone_from(&wire_model);
    let messages =
        super::chat::build_chat_messages_for_request_and_provider(&wire_request, provider);
    let tool_plan = plan_tools(
        provider,
        base_url,
        path_suffix,
        strict_enabled,
        request.tools.as_deref(),
    )?;
    let mut body = json!({
        "model": wire_model,
        "messages": messages,
        "max_tokens": request.max_tokens,
        "stream": response_mode == ResponseMode::Streaming,
    });
    if response_mode == ResponseMode::Streaming {
        body["stream_options"] = json!({"include_usage": true});
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = request.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(tools) = tool_plan.tools.as_deref() {
        body["tools"] = Value::Array(tools.iter().map(super::chat::tool_to_chat).collect());
    }
    if super::chat::should_send_tool_choice_for_chat(provider, request.reasoning_effort.as_deref())
        && let Some(choice) = request.tool_choice.as_ref()
        && let Some(mapped) = super::chat::map_tool_choice_for_chat(choice)
    {
        body["tool_choice"] = mapped;
    }
    super::apply_reasoning_effort(&mut body, request.reasoning_effort.as_deref(), provider);
    let reasoning_replay_tokens = super::chat::sanitize_thinking_mode_messages(
        &mut body,
        &wire_model,
        request.reasoning_effort.as_deref(),
        provider,
    );

    Some(RequestPlan {
        surface: tool_plan.surface,
        url: chat_url(root, tool_plan.surface),
        model: wire_model,
        body,
        response_mode,
        reasoning_replay_tokens,
    })
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
    let version = if surface == ApiSurface::StrictChat {
        "beta"
    } else {
        "v1"
    };
    format!("{root}/{version}/chat/completions")
}

fn clear_strict(tools: &mut [Tool]) {
    for tool in tools {
        tool.strict = None;
    }
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
        .expect("official plan");

        assert_eq!(plan.body["tool_choice"], "auto");
        assert_eq!(plan.body["thinking"]["type"], "disabled");
    }

    #[test]
    fn compact_model_alias_replays_exact_reasoning_with_canonical_wire_model() {
        let mut request = request(Some(vec![compatible_tool("lookup")]));
        request.model = "pro".to_string();
        request.messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "inspect the exact call history".to_string(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call-alias".to_string(),
                        name: "lookup".to_string(),
                        input: json!({"value": "history"}),
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
            true,
            &request,
            ResponseMode::Streaming,
        )
        .expect("official plan");

        assert_eq!(plan.model, "deepseek-v4-pro");
        assert_eq!(plan.body["model"], "deepseek-v4-pro");
        assert_eq!(
            plan.body["messages"][0]["reasoning_content"],
            "inspect the exact call history"
        );
        assert_ne!(
            plan.body["messages"][0]["reasoning_content"],
            "(reasoning omitted)"
        );
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
        .expect("official plan");

        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.surface, prefix_plan.surface);
        assert_eq!(plan.url, "https://api.deepseek.com/v1/chat/completions");
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
