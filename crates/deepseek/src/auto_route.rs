//! DeepSeek-only automatic model selection.
//!
//! The classifier is one ordinary, non-streaming official Chat request. It
//! shares the caller's `DeepSeekTransport`, so routing, root turns, and child
//! turns consume one physical request ledger. Transport and classifier
//! failures fall back to a deterministic caller-provided selection; a sealed
//! or exhausted hard request budget remains a typed failure.

use std::time::Duration;

use codewhale_runtime::ReasoningEffort;
use serde_json::{Value, json};

use crate::{
    ApiRequestBudgetError, ChatPlanInput, DeepSeekTransport, DeepSeekTransportError, ReasoningMode,
    ResponseMode, official_model_capabilities, plan_chat,
};

pub const DEEPSEEK_AUTO_ROUTE_PRO_MODEL: &str = "deepseek-v4-pro";
pub const DEEPSEEK_AUTO_ROUTE_FLASH_MODEL: &str = "deepseek-v4-flash";

const CLASSIFIER_OUTPUT_TOKENS: u32 = 128;
const CLASSIFIER_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepSeekAutoRouteSource {
    FlashClassifier,
    Heuristic,
}

impl DeepSeekAutoRouteSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FlashClassifier => "flash-router",
            Self::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeepSeekAutoRouteSelection {
    model: &'static str,
    reasoning_effort: Option<ReasoningEffort>,
    source: DeepSeekAutoRouteSource,
}

impl DeepSeekAutoRouteSelection {
    #[must_use]
    pub fn model(self) -> &'static str {
        self.model
    }

    #[must_use]
    pub fn reasoning_effort(self) -> Option<ReasoningEffort> {
        self.reasoning_effort
    }

    #[must_use]
    pub fn source(self) -> DeepSeekAutoRouteSource {
        self.source
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeepSeekAutoRouteFallback {
    model: &'static str,
    reasoning_effort: Option<ReasoningEffort>,
}

impl DeepSeekAutoRouteFallback {
    #[must_use]
    pub fn for_request(latest_request: &str, reasoning_effort: Option<ReasoningEffort>) -> Self {
        Self {
            model: heuristic_model(latest_request),
            reasoning_effort,
        }
    }

    pub fn new(
        model: &str,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<Self, crate::OfficialModelCapabilityError> {
        let capability = official_model_capabilities(model)?;
        Ok(Self {
            model: capability.model,
            reasoning_effort,
        })
    }

    fn selection(self) -> DeepSeekAutoRouteSelection {
        DeepSeekAutoRouteSelection {
            model: self.model,
            reasoning_effort: self.reasoning_effort,
            source: DeepSeekAutoRouteSource::Heuristic,
        }
    }
}

fn heuristic_model(input: &str) -> &'static str {
    let lower = input.to_lowercase();
    if COMPLEX_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(keyword))
        || input.chars().count() > 500
    {
        DEEPSEEK_AUTO_ROUTE_PRO_MODEL
    } else {
        DEEPSEEK_AUTO_ROUTE_FLASH_MODEL
    }
}

const COMPLEX_KEYWORDS: &[&str] = &[
    "refactor",
    "architecture",
    "design",
    "debug",
    "security",
    "review",
    "audit",
    "migrate",
    "optimize",
    "rewrite",
    "implement",
    "analyze",
    "重构",
    "架构",
    "设计",
    "调试",
    "安全",
    "审查",
    "审计",
    "迁移",
    "优化",
    "重写",
    "实现",
    "分析",
    "重構",
    "架構",
    "設計",
    "調試",
    "審查",
    "審計",
    "遷移",
    "優化",
    "重寫",
    "實現",
];

#[derive(Debug, Clone, Copy)]
pub struct DeepSeekAutoRouteInput<'a> {
    pub latest_request: &'a str,
    pub recent_context: &'a str,
    pub session_mode: &'a str,
    pub selected_model_mode: &'a str,
    pub selected_thinking_mode: &'a str,
    pub fallback: DeepSeekAutoRouteFallback,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeepSeekAutoRouteError {
    #[error("DeepSeek auto-route request budget rejected the physical request: {0}")]
    RequestBudget(#[source] ApiRequestBudgetError),
}

/// Resolve an automatic DeepSeek model selection through the supplied
/// per-run transport.
///
/// Only the two official V4 Agent models can leave this boundary. Network,
/// HTTP, timeout, response-shape, and classifier-value failures return the
/// deterministic fallback. Hard budget exhaustion/sealing is never hidden by
/// that fallback.
pub async fn resolve_deepseek_auto_route(
    transport: &DeepSeekTransport,
    input: DeepSeekAutoRouteInput<'_>,
) -> Result<DeepSeekAutoRouteSelection, DeepSeekAutoRouteError> {
    let fallback = input.fallback.selection();
    let Some(plan) = classifier_plan(transport, input) else {
        return Ok(fallback);
    };

    let response = match tokio::time::timeout(CLASSIFIER_TIMEOUT, transport.complete(plan)).await {
        Ok(Ok(response)) => response,
        Ok(Err(DeepSeekTransportError::RequestBudget(error))) => {
            return Err(DeepSeekAutoRouteError::RequestBudget(error));
        }
        Ok(Err(_)) | Err(_) => return Ok(fallback),
    };

    Ok(parse_recommendation(&response.output.content).unwrap_or(fallback))
}

fn classifier_plan(
    transport: &DeepSeekTransport,
    input: DeepSeekAutoRouteInput<'_>,
) -> Option<crate::RequestPlan> {
    plan_chat(
        transport.config().endpoint.root(),
        false,
        ChatPlanInput {
            model: DEEPSEEK_AUTO_ROUTE_FLASH_MODEL.to_owned(),
            messages: vec![
                json!({
                    "role": "system",
                    "content": classifier_system_prompt(),
                }),
                json!({
                    "role": "user",
                    "content": classifier_user_prompt(input),
                }),
            ],
            max_tokens: CLASSIFIER_OUTPUT_TOKENS,
            response_mode: ResponseMode::NonStreaming,
            tools: None,
            tool_choice: None,
            reasoning: ReasoningMode::Off,
            temperature: Some(0.0),
            top_p: None,
        },
    )
    .ok()
}

fn classifier_system_prompt() -> String {
    format!(
        "You are the codewhale model-routing classifier. Return only compact JSON: \
{{\"provider\":\"deepseek\",\"model\":\"<model>\",\"thinking\":\"off|high|max\"}}.\n\
Choose only provider/model pairs present in the inventory JSON. Use off only for trivial no-tool answers, \
high for ordinary reasoning, and max for agentic, coding, multi-file, release, architecture, debugging, \
security, tool-heavy, or uncertain work.\n\nInventory JSON:\n{}",
        json!({
            "active_provider": "deepseek",
            "router_provider": "deepseek",
            "router_model": DEEPSEEK_AUTO_ROUTE_FLASH_MODEL,
            "candidates": [
                {"provider": "deepseek", "model": DEEPSEEK_AUTO_ROUTE_PRO_MODEL},
                {"provider": "deepseek", "model": DEEPSEEK_AUTO_ROUTE_FLASH_MODEL},
            ],
        })
    )
}

fn classifier_user_prompt(input: DeepSeekAutoRouteInput<'_>) -> String {
    format!(
        "Session mode: {}\nSelected model mode: {}\nSelected thinking mode: {}\n\nRecent context:\n{}\n\nLatest user request:\n{}\n\nReturn JSON only.",
        input.session_mode,
        input.selected_model_mode,
        input.selected_thinking_mode,
        if input.recent_context.trim().is_empty() {
            "No prior context."
        } else {
            input.recent_context
        },
        truncate(input.latest_request, 4_000)
    )
}

fn parse_recommendation(raw: &str) -> Option<DeepSeekAutoRouteSelection> {
    let json = extract_first_json_object(raw)?;
    let value: Value = serde_json::from_str(json).ok()?;
    if value.get("provider").and_then(Value::as_str) != Some("deepseek") {
        return None;
    }
    let model = match value.get("model").and_then(Value::as_str)? {
        DEEPSEEK_AUTO_ROUTE_PRO_MODEL => DEEPSEEK_AUTO_ROUTE_PRO_MODEL,
        DEEPSEEK_AUTO_ROUTE_FLASH_MODEL => DEEPSEEK_AUTO_ROUTE_FLASH_MODEL,
        _ => return None,
    };
    let reasoning_effort = value
        .get("thinking")
        .or_else(|| value.get("reasoning_effort"))
        .or_else(|| value.get("effort"))
        .and_then(Value::as_str)
        .and_then(parse_reasoning_effort);
    Some(DeepSeekAutoRouteSelection {
        model,
        reasoning_effort,
        source: DeepSeekAutoRouteSource::FlashClassifier,
    })
}

fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" | "false" => Some(ReasoningEffort::Off),
        "low" | "minimal" | "medium" | "mid" | "high" => Some(ReasoningEffort::High),
        "max" | "maximum" | "xhigh" | "ultracode" => Some(ReasoningEffort::Max),
        _ => None,
    }
}

fn extract_first_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then_some(&raw[start..=end])
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::num::NonZeroU32;
    use std::sync::mpsc;
    use std::thread;

    use super::*;
    use crate::{
        DeepSeekConnectionConfig, DeepSeekCredential, DeepSeekEndpoint, SharedApiRequestBudget,
        TransportRetryPolicy,
    };

    fn fallback() -> DeepSeekAutoRouteFallback {
        DeepSeekAutoRouteFallback::new(DEEPSEEK_AUTO_ROUTE_FLASH_MODEL, Some(ReasoningEffort::High))
            .expect("canonical fallback")
    }

    fn input() -> DeepSeekAutoRouteInput<'static> {
        DeepSeekAutoRouteInput {
            latest_request: "refactor this module",
            recent_context: "No prior context.",
            session_mode: "agent",
            selected_model_mode: "auto",
            selected_thinking_mode: "auto",
            fallback: fallback(),
        }
    }

    #[test]
    fn parser_accepts_only_canonical_deepseek_routes() {
        let selected = parse_recommendation(
            r#"route: {"provider":"deepseek","model":"deepseek-v4-pro","thinking":"max"}"#,
        )
        .expect("canonical selection");
        assert_eq!(selected.model, DEEPSEEK_AUTO_ROUTE_PRO_MODEL);
        assert_eq!(selected.reasoning_effort, Some(ReasoningEffort::Max));

        for rejected in [
            r#"{"provider":"deepseek","model":"deepseek-chat","thinking":"high"}"#,
            r#"{"provider":"deepseek","model":"deepseek-v4pro","thinking":"high"}"#,
            r#"{"provider":"openrouter","model":"deepseek-v4-pro","thinking":"high"}"#,
        ] {
            assert!(
                parse_recommendation(rejected).is_none(),
                "accepted {rejected}"
            );
        }
    }

    #[tokio::test]
    async fn classifier_uses_flash_non_streaming_plan_and_shared_ledger() {
        let response = json!({
            "id": "route-1",
            "model": DEEPSEEK_AUTO_ROUTE_FLASH_MODEL,
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": r#"{"provider":"deepseek","model":"deepseek-v4-pro","thinking":"max"}"#,
                },
            }],
            "usage": {"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9},
        });
        let (transport, budget, request_rx, server) = fixture_transport(response, 2);

        let selected = resolve_deepseek_auto_route(&transport, input())
            .await
            .expect("route selection");
        assert_eq!(selected.model, DEEPSEEK_AUTO_ROUTE_PRO_MODEL);
        assert_eq!(selected.reasoning_effort, Some(ReasoningEffort::Max));
        assert_eq!(selected.source, DeepSeekAutoRouteSource::FlashClassifier);

        let body = request_rx.recv().expect("captured classifier request");
        assert_eq!(body["model"], DEEPSEEK_AUTO_ROUTE_FLASH_MODEL);
        assert_eq!(body["max_tokens"], CLASSIFIER_OUTPUT_TOKENS);
        assert_eq!(body["stream"], false);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("tools").is_none());
        server.join().expect("fixture server");

        let requests = budget.snapshot();
        let usage = budget.usage_snapshot();
        assert_eq!(requests.started, 1);
        assert_eq!(requests.completed, 1);
        assert_eq!(usage.usage_responses, 1);
        assert_eq!(usage.usage.input_tokens, 7);
        assert_eq!(usage.usage.output_tokens, 2);
    }

    #[tokio::test]
    async fn invalid_classifier_value_falls_back_without_erasing_physical_attempt() {
        let response = json!({
            "id": "route-2",
            "model": DEEPSEEK_AUTO_ROUTE_FLASH_MODEL,
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": r#"{"provider":"deepseek","model":"deepseek-v4pro","thinking":"max"}"#,
                },
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 1, "total_tokens": 6},
        });
        let (transport, budget, _request_rx, server) = fixture_transport(response, 2);

        let selected = resolve_deepseek_auto_route(&transport, input())
            .await
            .expect("invalid classifier value uses fallback");
        assert_eq!(selected.model, DEEPSEEK_AUTO_ROUTE_FLASH_MODEL);
        assert_eq!(selected.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(selected.source, DeepSeekAutoRouteSource::Heuristic);
        server.join().expect("fixture server");
        assert_eq!(budget.snapshot().started, 1);
        assert_eq!(budget.snapshot().completed, 1);
    }

    #[tokio::test]
    async fn hard_budget_exhaustion_and_sealing_remain_typed_failures() {
        for sealed in [false, true] {
            let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
            if sealed {
                budget.seal_and_snapshot();
            } else {
                drop(budget.try_reserve().expect("consume only request"));
            }
            let transport = fixture_connection("http://127.0.0.1:9/v1", budget.clone());
            let error = resolve_deepseek_auto_route(&transport, input())
                .await
                .expect_err("hard budget rejection must not use heuristic fallback");
            assert!(matches!(error, DeepSeekAutoRouteError::RequestBudget(_)));
            let snapshot = budget.snapshot();
            assert_eq!(snapshot.started, u32::from(!sealed));
            if sealed {
                assert_eq!(snapshot.sealed_denied, 1);
            } else {
                assert_eq!(snapshot.exhausted_denied, 1);
            }
        }
    }

    fn fixture_transport(
        response: Value,
        request_limit: u32,
    ) -> (
        DeepSeekTransport,
        SharedApiRequestBudget,
        mpsc::Receiver<Value>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept classifier request");
            let body = read_request_body(&mut stream);
            request_tx.send(body).expect("send captured request");
            let payload = serde_json::to_vec(&response).expect("serialize fixture response");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .expect("write response headers");
            stream.write_all(&payload).expect("write response body");
        });
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(request_limit).unwrap());
        let root = format!("http://{address}/v1");
        (
            fixture_connection(&root, budget.clone()),
            budget,
            request_rx,
            server,
        )
    }

    fn fixture_connection(root: &str, budget: SharedApiRequestBudget) -> DeepSeekTransport {
        let _ = rustls::crypto::ring::default_provider().install_default();
        DeepSeekConnectionConfig {
            endpoint: DeepSeekEndpoint::loopback_fixture(root).expect("loopback root"),
            strict_tools: false,
            response_header_timeout: Duration::from_secs(1),
            stream_idle_timeout: Duration::from_secs(1),
            retry: TransportRetryPolicy::disabled(),
        }
        .bind(
            reqwest::Client::new(),
            DeepSeekCredential::new("fixture-key").expect("fixture credential"),
            budget,
        )
        .expect("fixture transport")
    }

    fn read_request_body(stream: &mut std::net::TcpStream) -> Value {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).expect("read request");
            assert!(read > 0, "request ended before headers");
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("content length"))
                })
            })
            .expect("content-length header");
        while bytes.len() - header_end < content_length {
            let read = stream.read(&mut chunk).expect("read request body");
            assert!(read > 0, "request body ended early");
            bytes.extend_from_slice(&chunk[..read]);
        }
        serde_json::from_slice(&bytes[header_end..header_end + content_length])
            .expect("request JSON")
    }
}
