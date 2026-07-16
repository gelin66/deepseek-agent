use std::num::NonZeroU32;

use async_trait::async_trait;
use codewhale_runtime::{
    ActorRequestAccounting, AgentActorKind, ApiSurface as RuntimeApiSurface, ModelAccounting,
    ModelErrorCategory, ModelPort, ModelPortError, ModelRequest, ModelStream, ModelStreamEvent,
    SurfaceUsage,
};

use crate::{
    ApiRequestActorSnapshot, ApiRequestBudgetError, ApiRequestBudgetSnapshot, ApiSurface,
    ApiUsageSnapshot, ChatPlanError, DeepSeekStream, DeepSeekTransport, DeepSeekTransportError,
    RuntimeChatPlanInput, SharedApiRequestBudget, plan_runtime_chat,
};

/// Product default for one official DeepSeek V4 Agent turn.
///
/// The provider fixture advertises a 384K hard output limit. The lower 256K
/// default is the existing CodeWhale Agent cost/latency policy; callers may
/// request less or explicitly raise it up to the provider ceiling.
pub const OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS: u32 = 262_144;
pub const OFFICIAL_V4_MAX_OUTPUT_TOKENS: u32 = 384_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialModelCapabilities {
    pub model: &'static str,
    pub default_output_tokens: u32,
    pub max_output_tokens: u32,
}

impl OfficialModelCapabilities {
    pub fn resolve_output_tokens(
        self,
        requested: Option<u32>,
    ) -> Result<u32, OfficialModelCapabilityError> {
        let requested = requested.unwrap_or(self.default_output_tokens);
        if requested == 0 || requested > self.max_output_tokens {
            return Err(OfficialModelCapabilityError::InvalidOutputTokens {
                model: self.model.to_string(),
                requested,
                max: self.max_output_tokens,
            });
        }
        Ok(requested)
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum OfficialModelCapabilityError {
    #[error(
        "unsupported official DeepSeek model {model}; expected deepseek-v4-pro or deepseek-v4-flash"
    )]
    UnsupportedModel { model: String },
    #[error("invalid max_output_tokens={requested} for {model}; expected 1..={max}")]
    InvalidOutputTokens {
        model: String,
        requested: u32,
        max: u32,
    },
}

pub fn official_model_capabilities(
    model: &str,
) -> Result<OfficialModelCapabilities, OfficialModelCapabilityError> {
    let model = match model {
        "deepseek-v4-pro" => "deepseek-v4-pro",
        "deepseek-v4-flash" => "deepseek-v4-flash",
        _ => {
            return Err(OfficialModelCapabilityError::UnsupportedModel {
                model: model.to_string(),
            });
        }
    };
    Ok(OfficialModelCapabilities {
        model,
        default_output_tokens: OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS,
        max_output_tokens: OFFICIAL_V4_MAX_OUTPUT_TOKENS,
    })
}

/// The sole production `ModelPort` for official DeepSeek root and child runs.
///
/// The transport is rebound to a per-run shared physical budget. Actor views
/// change attribution only; they never create another transport/runtime.
#[derive(Clone)]
pub struct DeepSeekModelPort {
    transport: DeepSeekTransport,
    accounting: SharedApiRequestBudget,
}

impl DeepSeekModelPort {
    #[must_use]
    pub fn new(transport: DeepSeekTransport, accounting: SharedApiRequestBudget) -> Self {
        Self {
            transport: transport
                .with_retries_disabled()
                .with_request_budget(accounting.clone()),
            accounting,
        }
    }

    fn transport_for(&self, actor: AgentActorKind) -> DeepSeekTransport {
        let accounting = match actor {
            AgentActorKind::Root => self.accounting.clone(),
            AgentActorKind::Child => self.accounting.for_child(),
        };
        self.transport.clone().with_request_budget(accounting)
    }

    fn plan(&self, request: &ModelRequest) -> Result<crate::RequestPlan, ModelPortError> {
        Self::plan_request(
            self.transport.config().endpoint.root(),
            self.transport.config().strict_tools,
            request,
        )
    }

    fn plan_request(
        root: &str,
        strict_enabled: bool,
        request: &ModelRequest,
    ) -> Result<crate::RequestPlan, ModelPortError> {
        let capability = official_model_capabilities(&request.model).map_err(capability_error)?;
        let max_tokens = capability
            .resolve_output_tokens(request.max_output_tokens)
            .map_err(capability_error)?;
        plan_runtime_chat(
            RuntimeChatPlanInput {
                root,
                strict_enabled,
                wire_model: capability.model.to_string(),
                max_tokens,
            },
            request,
        )
        .map_err(chat_plan_error)
    }
}

#[async_trait]
impl ModelPort for DeepSeekModelPort {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        let plan = self.plan(&request)?;
        let transport = self.transport_for(request.actor.kind);
        if request.streaming {
            let source = transport.stream(plan).await.map_err(transport_error)?;
            Ok(Box::new(TransportModelStream { source }))
        } else {
            let response = transport.complete(plan).await.map_err(transport_error)?;
            Ok(Box::new(CompletedModelStream {
                output: Some(response.output),
            }))
        }
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        let (requests, actors, usage) = if seal {
            self.accounting.seal_and_accounting_snapshot()
        } else {
            self.accounting.accounting_snapshot()
        };
        Ok(runtime_accounting(requests, actors, usage))
    }
}

struct TransportModelStream {
    source: DeepSeekStream,
}

#[async_trait]
impl ModelStream for TransportModelStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        use futures_util::StreamExt;
        self.source
            .next()
            .await
            .map(|event| event.map_err(transport_error))
    }
}

struct CompletedModelStream {
    output: Option<codewhale_runtime::ModelOutput>,
}

#[async_trait]
impl ModelStream for CompletedModelStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        self.output
            .take()
            .map(|output| Ok(ModelStreamEvent::Completed { output }))
    }
}

fn capability_error(error: OfficialModelCapabilityError) -> ModelPortError {
    ModelPortError::new(
        "deepseek_model_capability",
        ModelErrorCategory::Protocol,
        error.to_string(),
        false,
    )
}

fn chat_plan_error(error: ChatPlanError) -> ModelPortError {
    ModelPortError::new(
        "deepseek_history_replay_invalid",
        ModelErrorCategory::Protocol,
        error.to_string(),
        false,
    )
}

fn transport_error(error: DeepSeekTransportError) -> ModelPortError {
    let (code, category) = match &error {
        DeepSeekTransportError::RequestBudget(ApiRequestBudgetError::Exhausted { .. }) => (
            "deepseek_request_budget_exhausted",
            ModelErrorCategory::Protocol,
        ),
        DeepSeekTransportError::RequestBudget(ApiRequestBudgetError::Sealed { .. }) => (
            "deepseek_request_budget_sealed",
            ModelErrorCategory::Protocol,
        ),
        DeepSeekTransportError::ResponseHeaderTimeout { .. } => {
            ("deepseek_timeout", ModelErrorCategory::Timeout)
        }
        DeepSeekTransportError::Network(_) => ("deepseek_transport", ModelErrorCategory::Transport),
        DeepSeekTransportError::Http {
            status: 401 | 403, ..
        } => (
            "deepseek_authentication",
            ModelErrorCategory::Authentication,
        ),
        DeepSeekTransportError::Http { status: 429, .. } => {
            ("deepseek_rate_limited", ModelErrorCategory::RateLimit)
        }
        DeepSeekTransportError::Http { status, .. } if *status >= 500 => {
            ("deepseek_server", ModelErrorCategory::Service)
        }
        DeepSeekTransportError::StreamStall { .. } => {
            ("stream_stall", ModelErrorCategory::StreamStall)
        }
        DeepSeekTransportError::StreamOverflow { .. } => {
            ("stream_overflow", ModelErrorCategory::Protocol)
        }
        DeepSeekTransportError::StreamIncomplete => {
            ("deepseek_stream_incomplete", ModelErrorCategory::Protocol)
        }
        DeepSeekTransportError::InvalidConfig(_)
        | DeepSeekTransportError::InvalidPlan(_)
        | DeepSeekTransportError::Http { .. }
        | DeepSeekTransportError::InvalidJson(_)
        | DeepSeekTransportError::SseProvider(_)
        | DeepSeekTransportError::UnsupportedFinishReason(_)
        | DeepSeekTransportError::MissingField(_) => {
            ("deepseek_protocol", ModelErrorCategory::Protocol)
        }
    };
    ModelPortError::new(code, category, error.to_string(), error.retryable())
}

fn runtime_accounting(
    requests: ApiRequestBudgetSnapshot,
    actors: ApiRequestActorSnapshot,
    usage: ApiUsageSnapshot,
) -> ModelAccounting {
    let usage_complete = usage.usage_complete();
    let cost_complete = usage.cost_complete();
    let request_complete = requests.in_flight == 0 && requests.started == requests.completed;
    ModelAccounting {
        hard_request_limit: (requests.limit != u32::MAX).then_some(requests.limit),
        root: ActorRequestAccounting {
            started: u64::from(actors.root_started),
            completed: u64::from(actors.root_completed),
            in_flight: u64::from(actors.root_in_flight),
            retries: u64::from(actors.root_retries),
        },
        child: ActorRequestAccounting {
            started: u64::from(actors.child_started),
            completed: u64::from(actors.child_completed),
            in_flight: u64::from(actors.child_in_flight),
            retries: u64::from(actors.child_retries),
        },
        transport_retries: u64::from(requests.retry_attempts),
        runtime_retries: 0,
        sealed_denied: u64::from(requests.sealed_denied),
        exhausted_denied: u64::from(requests.exhausted_denied),
        budget_exhausted: requests.exhausted_denied > 0,
        sealed: requests.sealed,
        complete: request_complete
            && usage_complete
            && cost_complete
            && usage.usage_records_after_seal == 0,
        usage_complete,
        usage_missing: usage.responses_missing_usage > 0,
        usage_incomplete: usage.incomplete_responses > 0,
        billing_unknown: usage.billing_unknown_attempts > 0,
        unpriced: usage.unpriced_usage_responses > 0,
        usage_responses: u64::from(usage.usage_responses),
        usage_missing_responses: u64::from(usage.responses_missing_usage),
        incomplete_responses: u64::from(usage.incomplete_responses),
        billing_unknown_attempts: u64::from(usage.billing_unknown_attempts),
        unpriced_usage_responses: u64::from(usage.unpriced_usage_responses),
        records_after_seal: u64::from(usage.usage_records_after_seal),
        usage: usage.usage,
        surface_usage: usage
            .usage_buckets
            .into_iter()
            .map(|bucket| SurfaceUsage {
                surface: match bucket.surface {
                    ApiSurface::StandardChat => RuntimeApiSurface::StandardChat,
                    ApiSurface::StrictChat => RuntimeApiSurface::StrictChat,
                    ApiSurface::Fim => RuntimeApiSurface::Fim,
                },
                model: bucket.model,
                response_count: u64::from(bucket.response_count),
                usage_response_count: u64::from(bucket.usage_responses),
                usage: bucket.usage,
                cost_nanousd: to_nano_units(bucket.cost_usd),
                cost_nanocny: to_nano_units(bucket.cost_cny),
            })
            .collect(),
        cost_nanousd: to_nano_units(usage.cost_usd),
        cost_nanocny: to_nano_units(usage.cost_cny),
    }
}

#[must_use]
pub fn model_accounting_snapshot(accounting: &SharedApiRequestBudget) -> ModelAccounting {
    let (requests, actors, usage) = accounting.accounting_snapshot();
    runtime_accounting(requests, actors, usage)
}

/// Recreate the remaining physical request budget from persisted accounting.
/// The returned boolean is true when the original hard budget is exhausted.
#[must_use]
pub fn resume_api_request_budget(accounting: &ModelAccounting) -> (SharedApiRequestBudget, bool) {
    let Some(limit) = accounting.hard_request_limit else {
        return (SharedApiRequestBudget::tracking_only(), false);
    };
    let physical_started = u32::try_from(accounting.total_started()).unwrap_or(u32::MAX);
    let remaining = limit.saturating_sub(physical_started);
    (
        NonZeroU32::new(remaining).map_or_else(
            SharedApiRequestBudget::tracking_only,
            SharedApiRequestBudget::new,
        ),
        remaining == 0,
    )
}

fn to_nano_units(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        (value * 1_000_000_000.0).round().min(u64::MAX as f64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codewhale_runtime::{AgentActor, ReasoningEffort, RunId, SystemPrompt};

    #[test]
    fn official_capabilities_are_exact_and_aliases_fail_closed() {
        for model in ["deepseek-v4-pro", "deepseek-v4-flash"] {
            let capability = official_model_capabilities(model).expect("official model");
            assert_eq!(capability.model, model);
            assert_eq!(
                capability.resolve_output_tokens(None).unwrap(),
                OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS
            );
            assert_eq!(
                capability
                    .resolve_output_tokens(Some(OFFICIAL_V4_MAX_OUTPUT_TOKENS))
                    .unwrap(),
                OFFICIAL_V4_MAX_OUTPUT_TOKENS
            );
        }
        for unsupported in [
            "deepseek-chat",
            "deepseek-reasoner",
            "pro",
            "flash",
            "deepseek/deepseek-v4-pro",
            "DeepSeek-V4-Pro",
        ] {
            assert!(official_model_capabilities(unsupported).is_err());
        }
    }

    #[test]
    fn output_tokens_must_stay_inside_the_official_fixture() {
        let capability = official_model_capabilities("deepseek-v4-pro").unwrap();
        assert!(capability.resolve_output_tokens(Some(0)).is_err());
        assert!(
            capability
                .resolve_output_tokens(Some(OFFICIAL_V4_MAX_OUTPUT_TOKENS + 1))
                .is_err()
        );
    }

    #[test]
    fn resume_budget_uses_persisted_physical_attempts() {
        let mut accounting = ModelAccounting {
            hard_request_limit: Some(9),
            ..ModelAccounting::default()
        };
        accounting.root.started = 3;
        accounting.child.started = 2;
        let (budget, exhausted) = resume_api_request_budget(&accounting);
        assert!(!exhausted);
        assert_eq!(budget.accounting_snapshot().0.limit, 4);

        accounting.hard_request_limit = Some(5);
        assert!(resume_api_request_budget(&accounting).1);
    }

    fn request(model: &str) -> ModelRequest {
        ModelRequest {
            run_id: RunId::new(),
            parent_run_id: None,
            model: model.to_string(),
            system_prompt: SystemPrompt::default(),
            messages: Vec::new(),
            tools: Vec::new(),
            reasoning_effort: ReasoningEffort::Auto,
            max_output_tokens: None,
            streaming: false,
            actor: AgentActor::default(),
            request_number: 1,
            attempt: 0,
        }
    }

    #[test]
    fn model_port_plan_owns_none_default_and_rejects_aliases_before_http() {
        let root = "http://127.0.0.1:9/v1";
        let plan = DeepSeekModelPort::plan_request(root, false, &request("deepseek-v4-pro"))
            .expect("official request plan");
        assert_eq!(
            plan.body["max_tokens"],
            OFFICIAL_V4_AGENT_DEFAULT_OUTPUT_TOKENS
        );
        assert!(DeepSeekModelPort::plan_request(root, false, &request("deepseek-chat")).is_err());
    }
}
