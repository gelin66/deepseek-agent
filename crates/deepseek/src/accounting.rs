use std::fmt;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard};

use codewhale_runtime::Usage;
use serde_json::Value;

use crate::ApiSurface;

#[derive(Clone, Debug)]
pub struct SharedApiRequestBudget {
    state: Arc<Mutex<ApiRequestBudgetState>>,
    actor: ApiRequestActor,
}

#[derive(Debug)]
struct ApiRequestBudgetState {
    limit: NonZeroU32,
    started: u32,
    in_flight: u32,
    completed: u32,
    retry_attempts: u32,
    exhausted_denied: u32,
    sealed_denied: u32,
    sealed: bool,
    usage: Usage,
    usage_responses: u32,
    standard_chat_responses: u32,
    strict_chat_responses: u32,
    fim_responses: u32,
    usage_buckets: Vec<ApiUsageBucket>,
    responses_missing_usage: u32,
    incomplete_responses: u32,
    billing_unknown_attempts: u32,
    unpriced_usage_responses: u32,
    usage_records_after_seal: u32,
    cost_usd: f64,
    cost_cny: f64,
    root: ApiRequestActorCounters,
    child: ApiRequestActorCounters,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ApiRequestActorCounters {
    started: u32,
    in_flight: u32,
    completed: u32,
    retries: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiRequestActor {
    Root,
    Child,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiRequestKind {
    Inference,
    Control,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiRequestBudgetSnapshot {
    pub limit: u32,
    pub started: u32,
    pub in_flight: u32,
    pub completed: u32,
    pub retry_attempts: u32,
    pub exhausted_denied: u32,
    pub sealed_denied: u32,
    pub sealed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApiRequestActorSnapshot {
    pub root_started: u32,
    pub root_in_flight: u32,
    pub root_completed: u32,
    pub root_retries: u32,
    pub child_started: u32,
    pub child_in_flight: u32,
    pub child_completed: u32,
    pub child_retries: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiUsageBucket {
    pub model: String,
    pub surface: ApiSurface,
    pub response_count: u32,
    pub usage_responses: u32,
    pub usage: Usage,
    pub cost_usd: f64,
    pub cost_cny: f64,
}

/// Immutable projection of all provider-reported usage observed through the
/// shared request budget. Root, child, nested-child, background, and FIM
/// clients all clone the same owner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApiUsageSnapshot {
    pub usage: Usage,
    pub usage_responses: u32,
    pub standard_chat_responses: u32,
    pub strict_chat_responses: u32,
    pub fim_responses: u32,
    pub usage_buckets: Vec<ApiUsageBucket>,
    pub responses_missing_usage: u32,
    pub incomplete_responses: u32,
    pub billing_unknown_attempts: u32,
    pub unpriced_usage_responses: u32,
    pub usage_records_after_seal: u32,
    pub cost_usd: f64,
    pub cost_cny: f64,
}

impl ApiUsageSnapshot {
    pub fn usage_complete(&self) -> bool {
        self.responses_missing_usage == 0 && self.incomplete_responses == 0
    }

    pub fn cost_complete(&self) -> bool {
        self.usage_complete()
            && self.billing_unknown_attempts == 0
            && self.unpriced_usage_responses == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiRequestBudgetError {
    Exhausted { limit: u32, started: u32 },
    Sealed { limit: u32, started: u32 },
}

/// One admitted physical HTTP attempt. Dropping it records that the transport
/// future returned (success or failure), allowing Headless settlement to prove
/// that no admitted send is still in flight before sealing the budget.
#[derive(Debug)]
pub struct ApiRequestLease {
    budget: SharedApiRequestBudget,
    actor: ApiRequestActor,
    kind: ApiRequestKind,
    response_received: bool,
    billing_unknown: bool,
    usage_expected: bool,
    settled: bool,
}

/// Fail-closed owner for one HTTP-success inference response. Once response
/// headers have been accepted, every exit path must either commit the exact
/// provider usage or explicitly mark the body/stream incomplete. `Drop`
/// covers cancellation, consumer drop, and early `?` returns.
pub struct ApiResponseAccountingGuard {
    request_lease: Option<ApiRequestLease>,
    model: String,
    surface: ApiSurface,
    observed_usage: Option<Usage>,
    observed_wire_usage: Option<Value>,
    settled: bool,
}

impl ApiResponseAccountingGuard {
    pub fn new(
        request_lease: Option<ApiRequestLease>,
        model: impl Into<String>,
        surface: ApiSurface,
    ) -> Self {
        Self {
            request_lease,
            model: model.into(),
            surface,
            observed_usage: None,
            observed_wire_usage: None,
            settled: false,
        }
    }

    pub fn observe(&mut self, usage: &Usage, wire_usage: Option<&Value>) {
        self.observed_usage = Some(*usage);
        self.observed_wire_usage = wire_usage.cloned();
    }

    pub fn complete(&mut self, usage: Option<&Usage>, wire_usage: Option<&Value>) {
        if self.settled {
            return;
        }
        if let Some(usage) = usage {
            self.observe(usage, wire_usage);
        }
        if let Some(lease) = self.request_lease.as_mut() {
            lease.settle_inference_response(
                &self.model,
                self.surface,
                self.observed_usage.as_ref(),
                self.observed_wire_usage.as_ref(),
                false,
            );
        }
        self.settled = true;
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    pub fn incomplete(&mut self) {
        if self.settled {
            return;
        }
        if let Some(lease) = self.request_lease.as_mut() {
            lease.settle_inference_response(
                &self.model,
                self.surface,
                self.observed_usage.as_ref(),
                self.observed_wire_usage.as_ref(),
                true,
            );
        }
        self.settled = true;
    }
}

impl Drop for ApiResponseAccountingGuard {
    fn drop(&mut self) {
        self.incomplete();
    }
}

impl ApiRequestLease {
    /// Mark that the HTTP server returned response headers. Transport futures
    /// dropped before this point may have reached the provider but expose no
    /// billable usage, so they remain explicitly unknown.
    pub fn mark_response_received(&mut self) {
        self.response_received = true;
    }

    pub fn mark_billing_unknown(&mut self) {
        self.billing_unknown = true;
    }

    /// A successful inference response must be handed to an accounting guard.
    /// Keeping this bit on the lease makes a forgotten handoff fail closed
    /// instead of silently looking like a zero-token response.
    pub fn mark_usage_expected(&mut self) {
        if self.kind == ApiRequestKind::Inference {
            self.usage_expected = true;
        }
    }

    pub fn mark_retry_attempt(&mut self) {
        let mut state = self.budget.lock_state();
        state.retry_attempts = state.retry_attempts.saturating_add(1);
        let actor = actor_counters_mut(&mut state, self.actor);
        actor.retries = actor.retries.saturating_add(1);
    }

    fn settle_inference_response(
        &mut self,
        model: &str,
        surface: ApiSurface,
        usage: Option<&Usage>,
        wire_usage: Option<&Value>,
        incomplete: bool,
    ) {
        if self.settled {
            return;
        }
        let cost = crate::pricing::response_cost(model, usage);
        {
            let mut state = self.budget.lock_state();
            settle_request_lease_locked(&mut state, self);
            if state.sealed {
                state.usage_records_after_seal = state.usage_records_after_seal.saturating_add(1);
            } else {
                record_surface_response_locked(&mut state, model, surface);
                if usage.is_some() || !incomplete {
                    record_usage_response_locked(
                        &mut state, model, surface, usage, wire_usage, cost,
                    );
                }
                if incomplete {
                    state.incomplete_responses = state.incomplete_responses.saturating_add(1);
                }
            }
        }
        self.settled = true;
    }
}

impl Drop for ApiRequestLease {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        {
            let mut state = self.budget.lock_state();
            settle_request_lease_locked(&mut state, self);
            if self.usage_expected {
                if state.sealed {
                    state.usage_records_after_seal =
                        state.usage_records_after_seal.saturating_add(1);
                } else {
                    state.incomplete_responses = state.incomplete_responses.saturating_add(1);
                }
            }
        }
        self.settled = true;
    }
}

fn settle_request_lease_locked(state: &mut ApiRequestBudgetState, lease: &ApiRequestLease) {
    state.in_flight = state.in_flight.saturating_sub(1);
    state.completed = state.completed.saturating_add(1);
    let actor = actor_counters_mut(state, lease.actor);
    actor.in_flight = actor.in_flight.saturating_sub(1);
    actor.completed = actor.completed.saturating_add(1);
    debug_assert_actor_totals(state);
    if lease.kind == ApiRequestKind::Inference
        && (!lease.response_received || lease.billing_unknown)
    {
        state.billing_unknown_attempts = state.billing_unknown_attempts.saturating_add(1);
    }
}

impl fmt::Display for ApiRequestBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted { limit, started } => {
                write!(
                    formatter,
                    "DeepSeek API 请求预算已用尽（已发起：{started}，上限：{limit}）"
                )
            }
            Self::Sealed { limit, started } => write!(
                formatter,
                "DeepSeek API 请求预算已封存，不能再发起请求（已发起：{started}，上限：{limit}）"
            ),
        }
    }
}

impl std::error::Error for ApiRequestBudgetError {}

impl SharedApiRequestBudget {
    pub fn new(limit: NonZeroU32) -> Self {
        Self {
            state: Arc::new(Mutex::new(ApiRequestBudgetState {
                limit,
                started: 0,
                in_flight: 0,
                completed: 0,
                retry_attempts: 0,
                exhausted_denied: 0,
                sealed_denied: 0,
                sealed: false,
                usage: Usage::default(),
                usage_responses: 0,
                standard_chat_responses: 0,
                strict_chat_responses: 0,
                fim_responses: 0,
                usage_buckets: Vec::new(),
                responses_missing_usage: 0,
                incomplete_responses: 0,
                billing_unknown_attempts: 0,
                unpriced_usage_responses: 0,
                usage_records_after_seal: 0,
                cost_usd: 0.0,
                cost_cny: 0.0,
                root: ApiRequestActorCounters::default(),
                child: ApiRequestActorCounters::default(),
            })),
            actor: ApiRequestActor::Root,
        }
    }

    /// Track every physical request and provider usage without imposing a
    /// practical request cap. Headless uses the same owner in capped and
    /// uncapped runs so child-Agent/FIM accounting never disappears merely
    /// because the operator omitted `--max-api-requests`.
    pub fn tracking_only() -> Self {
        Self::new(NonZeroU32::MAX)
    }

    /// Return a child-Agent accounting view over the same admission owner.
    /// This changes attribution only; the request limit and admission policy
    /// remain shared with the root Agent.
    pub fn for_child(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            actor: ApiRequestActor::Child,
        }
    }

    pub fn try_reserve(&self) -> Result<ApiRequestLease, ApiRequestBudgetError> {
        self.try_reserve_kind(ApiRequestKind::Inference)
    }

    pub fn try_reserve_control(&self) -> Result<ApiRequestLease, ApiRequestBudgetError> {
        self.try_reserve_kind(ApiRequestKind::Control)
    }

    fn try_reserve_kind(
        &self,
        kind: ApiRequestKind,
    ) -> Result<ApiRequestLease, ApiRequestBudgetError> {
        let mut state = self.lock_state();

        if state.sealed {
            state.sealed_denied = state.sealed_denied.saturating_add(1);
            return Err(ApiRequestBudgetError::Sealed {
                limit: state.limit.get(),
                started: state.started,
            });
        }

        if state.started >= state.limit.get() {
            state.exhausted_denied = state.exhausted_denied.saturating_add(1);
            return Err(ApiRequestBudgetError::Exhausted {
                limit: state.limit.get(),
                started: state.started,
            });
        }

        state.started += 1;
        state.in_flight += 1;
        let actor = actor_counters_mut(&mut state, self.actor);
        actor.started += 1;
        actor.in_flight += 1;
        debug_assert_actor_totals(&state);
        Ok(ApiRequestLease {
            budget: self.clone(),
            actor: self.actor,
            kind,
            response_received: false,
            billing_unknown: false,
            usage_expected: false,
            settled: false,
        })
    }

    #[cfg(test)]
    pub fn seal_and_snapshot(&self) -> ApiRequestBudgetSnapshot {
        let mut state = self.lock_state();
        state.sealed = true;
        snapshot_of(&state)
    }

    /// Seal request admission and capture request plus usage accounting under
    /// one lock. Headless calls this only after the Engine settlement barrier.
    #[cfg(test)]
    pub fn seal_and_full_snapshot(&self) -> (ApiRequestBudgetSnapshot, ApiUsageSnapshot) {
        let mut state = self.lock_state();
        state.sealed = true;
        (snapshot_of(&state), usage_snapshot_of(&state))
    }

    /// Seal admission and capture total requests, actor attribution, and
    /// provider usage atomically for the terminal execution receipt.
    pub fn seal_and_accounting_snapshot(
        &self,
    ) -> (
        ApiRequestBudgetSnapshot,
        ApiRequestActorSnapshot,
        ApiUsageSnapshot,
    ) {
        let mut state = self.lock_state();
        state.sealed = true;
        (
            snapshot_of(&state),
            actor_snapshot_of(&state),
            usage_snapshot_of(&state),
        )
    }

    /// Capture request, actor, and usage accounting without sealing admission.
    /// Child runtimes use this read-only projection; the settled root runtime
    /// is the only caller allowed to use [`Self::seal_and_accounting_snapshot`].
    pub fn accounting_snapshot(
        &self,
    ) -> (
        ApiRequestBudgetSnapshot,
        ApiRequestActorSnapshot,
        ApiUsageSnapshot,
    ) {
        let state = self.lock_state();
        (
            snapshot_of(&state),
            actor_snapshot_of(&state),
            usage_snapshot_of(&state),
        )
    }

    /// Commit one fully parsed provider response. Missing official usage is
    /// retained as an explicit completeness failure rather than silently
    /// becoming a zero-token response.
    #[cfg(test)]
    pub fn record_usage_response(
        &self,
        model: &str,
        surface: ApiSurface,
        usage: Option<&Usage>,
        wire_usage: Option<&Value>,
    ) {
        let cost = crate::pricing::response_cost(model, usage);
        let mut state = self.lock_state();
        if state.sealed {
            state.usage_records_after_seal = state.usage_records_after_seal.saturating_add(1);
            return;
        }
        record_surface_response_locked(&mut state, model, surface);
        record_usage_response_locked(&mut state, model, surface, usage, wire_usage, cost);
    }

    /// Record an HTTP-success stream that never reached a valid provider
    /// terminal. Its billable usage is unknowable, so successful Headless
    /// receipts must treat the run as incomplete.
    #[cfg(test)]
    pub fn record_incomplete_response(&self) {
        let mut state = self.lock_state();
        if state.sealed {
            state.usage_records_after_seal = state.usage_records_after_seal.saturating_add(1);
            return;
        }
        state.incomplete_responses = state.incomplete_responses.saturating_add(1);
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> ApiRequestBudgetSnapshot {
        snapshot_of(&self.lock_state())
    }

    #[cfg(test)]
    pub fn actor_snapshot(&self) -> ApiRequestActorSnapshot {
        actor_snapshot_of(&self.lock_state())
    }

    pub fn usage_snapshot(&self) -> ApiUsageSnapshot {
        usage_snapshot_of(&self.lock_state())
    }

    fn lock_state(&self) -> MutexGuard<'_, ApiRequestBudgetState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn record_usage_response_locked(
    state: &mut ApiRequestBudgetState,
    model: &str,
    surface: ApiSurface,
    usage: Option<&Usage>,
    wire_usage: Option<&Value>,
    cost: Option<(f64, f64)>,
) {
    if let Some(usage) = usage {
        state.usage_responses = state.usage_responses.saturating_add(1);
        state.usage.add_assign(*usage);
        if !usage_contract_complete(usage, wire_usage) {
            state.responses_missing_usage = state.responses_missing_usage.saturating_add(1);
        }
        if let Some((usd, cny)) = cost {
            state.cost_usd += usd;
            state.cost_cny += cny;
        } else {
            state.unpriced_usage_responses = state.unpriced_usage_responses.saturating_add(1);
        }

        let bucket = usage_bucket_mut(state, model, surface);
        bucket.usage_responses = bucket.usage_responses.saturating_add(1);
        bucket.usage.add_assign(*usage);
        if let Some((usd, cny)) = cost {
            bucket.cost_usd += usd;
            bucket.cost_cny += cny;
        }
    } else {
        state.responses_missing_usage = state.responses_missing_usage.saturating_add(1);
    }
}

fn record_surface_response_locked(
    state: &mut ApiRequestBudgetState,
    model: &str,
    surface: ApiSurface,
) {
    match surface {
        ApiSurface::StandardChat => {
            state.standard_chat_responses = state.standard_chat_responses.saturating_add(1);
        }
        ApiSurface::StrictChat => {
            state.strict_chat_responses = state.strict_chat_responses.saturating_add(1);
        }
        ApiSurface::Fim => {
            state.fim_responses = state.fim_responses.saturating_add(1);
        }
    }
    let bucket = usage_bucket_mut(state, model, surface);
    bucket.response_count = bucket.response_count.saturating_add(1);
}

fn usage_bucket_mut<'a>(
    state: &'a mut ApiRequestBudgetState,
    model: &str,
    surface: ApiSurface,
) -> &'a mut ApiUsageBucket {
    if let Some(index) = state
        .usage_buckets
        .iter()
        .position(|bucket| bucket.surface == surface && bucket.model == model)
    {
        return &mut state.usage_buckets[index];
    }
    state.usage_buckets.push(ApiUsageBucket {
        model: model.to_string(),
        surface,
        response_count: 0,
        usage_responses: 0,
        usage: Usage::default(),
        cost_usd: 0.0,
        cost_cny: 0.0,
    });
    state
        .usage_buckets
        .last_mut()
        .expect("usage bucket was just inserted")
}

fn usage_contract_complete(usage: &Usage, wire_usage: Option<&Value>) -> bool {
    let Some(wire) = wire_usage.and_then(Value::as_object) else {
        return false;
    };
    let integer = |field: &str| wire.get(field).and_then(Value::as_u64);
    let (Some(prompt), Some(completion), Some(total), Some(hit), Some(miss)) = (
        integer("prompt_tokens"),
        integer("completion_tokens"),
        integer("total_tokens"),
        integer("prompt_cache_hit_tokens"),
        integer("prompt_cache_miss_tokens"),
    ) else {
        return false;
    };
    let reasoning = wire
        .get("completion_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64);

    prompt.checked_add(completion) == Some(total)
        && hit.checked_add(miss) == Some(prompt)
        && reasoning.is_none_or(|tokens| tokens <= completion)
        && usage.input_tokens == prompt
        && usage.output_tokens == completion
        && usage.cache_hit_tokens == hit
        && usage.cache_miss_tokens == miss
        && usage.reasoning_tokens == reasoning.unwrap_or(0)
}

fn snapshot_of(state: &ApiRequestBudgetState) -> ApiRequestBudgetSnapshot {
    debug_assert_actor_totals(state);
    ApiRequestBudgetSnapshot {
        limit: state.limit.get(),
        started: state.started,
        in_flight: state.in_flight,
        completed: state.completed,
        retry_attempts: state.retry_attempts,
        exhausted_denied: state.exhausted_denied,
        sealed_denied: state.sealed_denied,
        sealed: state.sealed,
    }
}

fn actor_snapshot_of(state: &ApiRequestBudgetState) -> ApiRequestActorSnapshot {
    debug_assert_actor_totals(state);
    ApiRequestActorSnapshot {
        root_started: state.root.started,
        root_in_flight: state.root.in_flight,
        root_completed: state.root.completed,
        root_retries: state.root.retries,
        child_started: state.child.started,
        child_in_flight: state.child.in_flight,
        child_completed: state.child.completed,
        child_retries: state.child.retries,
    }
}

fn actor_counters_mut(
    state: &mut ApiRequestBudgetState,
    actor: ApiRequestActor,
) -> &mut ApiRequestActorCounters {
    match actor {
        ApiRequestActor::Root => &mut state.root,
        ApiRequestActor::Child => &mut state.child,
    }
}

fn debug_assert_actor_totals(state: &ApiRequestBudgetState) {
    debug_assert_eq!(state.started, state.root.started + state.child.started);
    debug_assert_eq!(
        state.in_flight,
        state.root.in_flight + state.child.in_flight
    );
    debug_assert_eq!(
        state.completed,
        state.root.completed + state.child.completed
    );
}

fn usage_snapshot_of(state: &ApiRequestBudgetState) -> ApiUsageSnapshot {
    ApiUsageSnapshot {
        usage: state.usage,
        usage_responses: state.usage_responses,
        standard_chat_responses: state.standard_chat_responses,
        strict_chat_responses: state.strict_chat_responses,
        fim_responses: state.fim_responses,
        usage_buckets: state.usage_buckets.clone(),
        responses_missing_usage: state.responses_missing_usage,
        incomplete_responses: state.incomplete_responses,
        billing_unknown_attempts: state.billing_unknown_attempts,
        unpriced_usage_responses: state.unpriced_usage_responses,
        usage_records_after_seal: state.usage_records_after_seal,
        cost_usd: state.cost_usd,
        cost_cny: state.cost_cny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    fn successful_inference_lease(budget: &SharedApiRequestBudget) -> ApiRequestLease {
        let mut lease = budget.try_reserve().unwrap();
        lease.mark_response_received();
        lease.mark_usage_expected();
        lease
    }

    #[test]
    fn successful_inference_lease_stays_in_flight_across_guard_handoff() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
        let lease = successful_inference_lease(&budget);
        assert_eq!(budget.snapshot().in_flight, 1);

        let mut guard = ApiResponseAccountingGuard::new(
            Some(lease),
            "deepseek-v4-flash",
            ApiSurface::StandardChat,
        );
        assert_eq!(budget.snapshot().in_flight, 1);
        guard.complete(None, None);
        assert_eq!(budget.snapshot().in_flight, 0);
        assert_eq!(budget.snapshot().completed, 1);
        assert_eq!(budget.usage_snapshot().responses_missing_usage, 1);
    }

    #[test]
    fn forgotten_guard_is_incomplete_but_control_request_is_not_billable() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(2).unwrap());
        drop(successful_inference_lease(&budget));

        let mut control = budget.try_reserve_control().unwrap();
        control.mark_response_received();
        drop(control);

        let requests = budget.snapshot();
        let usage = budget.usage_snapshot();
        assert_eq!(requests.started, 2);
        assert_eq!(requests.completed, 2);
        assert_eq!(requests.in_flight, 0);
        assert_eq!(usage.incomplete_responses, 1);
        assert_eq!(usage.billing_unknown_attempts, 0);
        assert!(!usage.cost_complete());
    }

    #[test]
    fn concurrent_reservations_never_exceed_limit() {
        const CALLERS: usize = 32;
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        let barrier = Arc::new(Barrier::new(CALLERS + 1));
        let handles = (0..CALLERS)
            .map(|_| {
                let budget = budget.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    budget.try_reserve()
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    result.as_ref().err()
                        == Some(&ApiRequestBudgetError::Exhausted {
                            limit: 3,
                            started: 3,
                        })
                })
                .count(),
            CALLERS - 3
        );
        assert_eq!(
            budget.snapshot(),
            ApiRequestBudgetSnapshot {
                limit: 3,
                started: 3,
                in_flight: 3,
                completed: 0,
                retry_attempts: 0,
                exhausted_denied: (CALLERS - 3) as u32,
                sealed_denied: 0,
                sealed: false,
            }
        );
    }

    #[test]
    fn actor_scoped_views_share_admission_and_preserve_exact_totals() {
        let root = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        let child = root.for_child();
        let nested_child = child.for_child();
        assert!(Arc::ptr_eq(&root.state, &child.state));
        assert!(Arc::ptr_eq(&child.state, &nested_child.state));

        let mut root_lease = root.try_reserve().unwrap();
        let mut child_lease = child.clone().try_reserve().unwrap();
        let nested_child_lease = nested_child.try_reserve().unwrap();
        root_lease.mark_retry_attempt();
        child_lease.mark_retry_attempt();
        child_lease.mark_retry_attempt();

        let admitted = root.snapshot();
        let admitted_actors = root.actor_snapshot();
        assert_eq!(admitted.started, 3);
        assert_eq!(admitted_actors.root_started, 1);
        assert_eq!(admitted_actors.child_started, 2);
        assert_eq!(admitted.in_flight, 3);
        assert_eq!(admitted_actors.root_in_flight, 1);
        assert_eq!(admitted_actors.child_in_flight, 2);
        assert_eq!(admitted.completed, 0);
        assert_eq!(admitted_actors.root_completed, 0);
        assert_eq!(admitted_actors.child_completed, 0);
        assert_eq!(admitted.retry_attempts, 3);
        assert_eq!(admitted_actors.root_retries, 1);
        assert_eq!(admitted_actors.child_retries, 2);
        assert_eq!(
            admitted.started,
            admitted_actors.root_started + admitted_actors.child_started
        );
        assert_eq!(
            admitted.in_flight,
            admitted_actors.root_in_flight + admitted_actors.child_in_flight
        );

        drop(root_lease);
        drop(child_lease);
        let partially_settled = root.snapshot();
        let partially_settled_actors = root.actor_snapshot();
        assert_eq!(partially_settled.completed, 2);
        assert_eq!(partially_settled_actors.root_completed, 1);
        assert_eq!(partially_settled_actors.child_completed, 1);
        assert_eq!(partially_settled.in_flight, 1);
        assert_eq!(partially_settled_actors.root_in_flight, 0);
        assert_eq!(partially_settled_actors.child_in_flight, 1);

        drop(nested_child_lease);
        let settled = root.snapshot();
        let settled_actors = root.actor_snapshot();
        assert_eq!(settled.completed, 3);
        assert_eq!(settled_actors.root_completed, 1);
        assert_eq!(settled_actors.child_completed, 2);
        assert_eq!(settled.in_flight, 0);
        assert_eq!(
            settled.completed,
            settled_actors.root_completed + settled_actors.child_completed
        );
        assert_eq!(
            settled.in_flight,
            settled_actors.root_in_flight + settled_actors.child_in_flight
        );
    }

    #[test]
    fn sealed_budget_rejects_new_reservations() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        budget.try_reserve().unwrap();

        assert_eq!(
            budget.seal_and_snapshot(),
            ApiRequestBudgetSnapshot {
                limit: 3,
                started: 1,
                in_flight: 0,
                completed: 1,
                retry_attempts: 0,
                exhausted_denied: 0,
                sealed_denied: 0,
                sealed: true,
            }
        );
        assert!(matches!(
            budget.try_reserve(),
            Err(ApiRequestBudgetError::Sealed {
                limit: 3,
                started: 1,
            })
        ));
        assert_eq!(budget.snapshot().sealed_denied, 1);
        assert_eq!(budget.snapshot().exhausted_denied, 0);
    }

    #[test]
    fn child_accounting_snapshot_is_atomic_without_sealing_root_admission() {
        let root = SharedApiRequestBudget::new(NonZeroU32::new(2).unwrap());
        let child = root.for_child();
        let lease = child.try_reserve().unwrap();
        drop(lease);

        let (requests, actors, usage) = child.accounting_snapshot();
        assert!(!requests.sealed);
        assert_eq!(requests.started, 1);
        assert_eq!(actors.child_started, 1);
        assert_eq!(actors.child_completed, 1);
        assert_eq!(usage.incomplete_responses, 0);

        assert!(root.try_reserve().is_ok());
    }

    #[test]
    fn using_last_slot_succeeds_before_exhaustion_is_observed() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());

        budget.try_reserve().unwrap();
        budget.try_reserve().unwrap();
        let final_reservation = budget.try_reserve().unwrap();

        let snapshot = budget.snapshot();
        assert_eq!(snapshot.started, snapshot.limit);
        assert_eq!(snapshot.in_flight, 1);
        assert_eq!(snapshot.completed, 2);
        assert_eq!(snapshot.exhausted_denied, 0);
        assert_eq!(snapshot.sealed_denied, 0);
        assert!(matches!(
            budget.try_reserve(),
            Err(ApiRequestBudgetError::Exhausted {
                limit: 3,
                started: 3,
            })
        ));
        assert_eq!(budget.snapshot().exhausted_denied, 1);
        assert_eq!(budget.snapshot().sealed_denied, 0);
        drop(final_reservation);
        assert_eq!(budget.snapshot().in_flight, 0);
        assert_eq!(budget.snapshot().completed, 3);
    }

    #[test]
    fn shared_usage_ledger_aggregates_chat_and_fim_with_exact_cost() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        let chat = Usage {
            input_tokens: 1_000,
            output_tokens: 100,
            cache_hit_tokens: 250,
            cache_miss_tokens: 750,
            reasoning_tokens: 50,
            ..Usage::default()
        };
        let fim = Usage {
            input_tokens: 500,
            output_tokens: 50,
            cache_hit_tokens: 0,
            cache_miss_tokens: 500,
            ..Usage::default()
        };
        let chat_wire = serde_json::json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 100,
            "total_tokens": 1_100,
            "prompt_cache_hit_tokens": 250,
            "prompt_cache_miss_tokens": 750,
            "completion_tokens_details": { "reasoning_tokens": 50 }
        });
        let fim_wire = serde_json::json!({
            "prompt_tokens": 500,
            "completion_tokens": 50,
            "total_tokens": 550,
            "prompt_cache_hit_tokens": 0,
            "prompt_cache_miss_tokens": 500
        });
        budget.record_usage_response(
            "deepseek-v4-flash",
            ApiSurface::StandardChat,
            Some(&chat),
            Some(&chat_wire),
        );
        budget.record_usage_response(
            "deepseek-v4-pro",
            ApiSurface::Fim,
            Some(&fim),
            Some(&fim_wire),
        );

        let (_, usage) = budget.seal_and_full_snapshot();
        assert_eq!(usage.usage.input_tokens, 1_500);
        assert_eq!(usage.usage.output_tokens, 150);
        assert_eq!(usage.usage.reasoning_tokens, 50);
        assert_eq!(usage.usage_responses, 2);
        assert_eq!(usage.standard_chat_responses, 1);
        assert_eq!(usage.strict_chat_responses, 0);
        assert_eq!(usage.fim_responses, 1);
        assert_eq!(usage.usage_buckets.len(), 2);
        assert!(usage.usage_complete());
        assert!(usage.cost_complete());
        let expected =
            crate::pricing::calculate_turn_cost_estimate("deepseek-v4-flash", &chat).unwrap();
        let expected_fim =
            crate::pricing::calculate_turn_cost_estimate("deepseek-v4-pro", &fim).unwrap();
        assert!((usage.cost_usd - expected.usd - expected_fim.usd).abs() < 1e-12);
        assert!((usage.cost_cny - expected.cny - expected_fim.cny).abs() < 1e-12);
    }

    #[test]
    fn official_request_plan_surface_drives_exact_accounting_bucket() {
        use crate::{ChatPlanInput, ReasoningMode, ResponseMode, plan_chat};

        let plan = plan_chat(
            "https://api.deepseek.com",
            false,
            ChatPlanInput {
                model: "deepseek-v4-flash".to_string(),
                messages: Vec::new(),
                max_tokens: 32,
                tools: None,
                tool_choice: None,
                reasoning: ReasoningMode::Off,
                response_mode: ResponseMode::Streaming,
                temperature: None,
                top_p: None,
            },
        )
        .expect("valid exact replay history");
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(plan.surface, ApiSurface::StandardChat);

        let usage = Usage {
            input_tokens: 12,
            output_tokens: 3,
            cache_hit_tokens: 2,
            cache_miss_tokens: 10,
            ..Usage::default()
        };
        let wire = serde_json::json!({
            "prompt_tokens": 12,
            "completion_tokens": 3,
            "total_tokens": 15,
            "prompt_cache_hit_tokens": 2,
            "prompt_cache_miss_tokens": 10
        });
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
        budget.record_usage_response(&plan.model, plan.surface, Some(&usage), Some(&wire));

        let snapshot = budget.usage_snapshot();
        assert!(snapshot.usage_complete());
        assert!(snapshot.cost_complete());
        assert_eq!(snapshot.standard_chat_responses, 1);
        assert_eq!(snapshot.strict_chat_responses, 0);
        assert_eq!(snapshot.fim_responses, 0);
        assert_eq!(snapshot.usage_buckets.len(), 1);
        assert_eq!(snapshot.usage_buckets[0].surface, plan.surface);
        assert_eq!(snapshot.usage_buckets[0].model, plan.model);
    }

    #[test]
    fn missing_incomplete_and_post_seal_usage_are_never_silent() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        drop(budget.try_reserve().unwrap());
        budget.record_usage_response("deepseek-v4-flash", ApiSurface::StandardChat, None, None);
        budget.record_incomplete_response();
        let (_, before) = budget.seal_and_full_snapshot();
        assert!(!before.usage_complete());
        assert!(!before.cost_complete());
        assert_eq!(before.responses_missing_usage, 1);
        assert_eq!(before.incomplete_responses, 1);
        assert_eq!(before.billing_unknown_attempts, 1);
        assert_eq!(before.standard_chat_responses, 1);
        assert_eq!(before.usage_buckets.len(), 1);
        assert_eq!(before.usage_buckets[0].response_count, 1);
        assert_eq!(before.usage_buckets[0].usage_responses, 0);

        budget.record_usage_response(
            "deepseek-v4-flash",
            ApiSurface::StandardChat,
            Some(&Usage::default()),
            None,
        );
        let state = budget.lock_state();
        assert_eq!(state.usage_records_after_seal, 1);
        assert_eq!(state.usage_responses, 0);
        assert_eq!(state.standard_chat_responses, 1);
    }

    #[test]
    fn deepseek_wire_contract_cannot_be_reconstructed_from_normalized_usage() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
        let normalized = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cache_hit_tokens: 25,
            cache_miss_tokens: 75,
            ..Usage::default()
        };
        let missing_official_fields = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        });

        budget.record_usage_response(
            "deepseek-v4-pro",
            ApiSurface::StandardChat,
            Some(&normalized),
            Some(&missing_official_fields),
        );

        let usage = budget.usage_snapshot();
        assert_eq!(usage.responses_missing_usage, 1);
        assert!(!usage.usage_complete());
    }

    #[test]
    fn unknown_model_is_explicitly_unpriced_even_with_complete_wire_usage() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cache_hit_tokens: 25,
            cache_miss_tokens: 75,
            ..Usage::default()
        };
        let wire = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_cache_hit_tokens": 25,
            "prompt_cache_miss_tokens": 75
        });

        budget.record_usage_response(
            "deepseek-future-unknown",
            ApiSurface::StandardChat,
            Some(&usage),
            Some(&wire),
        );

        let snapshot = budget.usage_snapshot();
        assert!(snapshot.usage_complete());
        assert_eq!(snapshot.unpriced_usage_responses, 1);
        assert!(!snapshot.cost_complete());
    }

    #[test]
    fn response_guard_marks_consumer_drop_incomplete_exactly_once() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(2).unwrap());
        {
            let _guard = ApiResponseAccountingGuard::new(
                Some(successful_inference_lease(&budget)),
                "deepseek-v4-pro",
                ApiSurface::StandardChat,
            );
            assert_eq!(budget.snapshot().in_flight, 1);
        }
        assert_eq!(budget.snapshot().in_flight, 0);
        assert_eq!(budget.usage_snapshot().incomplete_responses, 1);

        let mut completed = ApiResponseAccountingGuard::new(
            Some(successful_inference_lease(&budget)),
            "deepseek-v4-pro",
            ApiSurface::StrictChat,
        );
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 2,
            cache_hit_tokens: 0,
            cache_miss_tokens: 10,
            ..Usage::default()
        };
        let wire = serde_json::json!({
            "prompt_tokens": 10,
            "completion_tokens": 2,
            "total_tokens": 12,
            "prompt_cache_hit_tokens": 0,
            "prompt_cache_miss_tokens": 10
        });
        completed.complete(Some(&usage), Some(&wire));
        drop(completed);

        let snapshot = budget.usage_snapshot();
        assert_eq!(snapshot.incomplete_responses, 1);
        assert_eq!(snapshot.usage_responses, 1);
        assert_eq!(snapshot.standard_chat_responses, 1);
        assert_eq!(snapshot.strict_chat_responses, 1);
    }

    #[test]
    fn incomplete_stream_preserves_usage_observed_before_disconnect() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(1).unwrap());
        let usage = Usage {
            input_tokens: 12,
            output_tokens: 4,
            cache_hit_tokens: 2,
            cache_miss_tokens: 10,
            ..Usage::default()
        };
        let wire = serde_json::json!({
            "prompt_tokens": 12,
            "completion_tokens": 4,
            "total_tokens": 16,
            "prompt_cache_hit_tokens": 2,
            "prompt_cache_miss_tokens": 10
        });
        {
            let mut guard = ApiResponseAccountingGuard::new(
                Some(successful_inference_lease(&budget)),
                "deepseek-v4-flash",
                ApiSurface::StandardChat,
            );
            guard.observe(&usage, Some(&wire));
        }

        let snapshot = budget.usage_snapshot();
        assert_eq!(snapshot.incomplete_responses, 1);
        assert_eq!(snapshot.usage_responses, 1);
        assert_eq!(snapshot.usage.input_tokens, 12);
        assert_eq!(snapshot.usage.output_tokens, 4);
        assert!(snapshot.cost_usd > 0.0);
        assert!(!snapshot.usage_complete());
        assert_eq!(budget.snapshot().in_flight, 0);
    }
}
