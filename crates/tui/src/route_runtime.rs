use codewhale_config::route::{
    LogicalModelRef, ReadyRouteCandidate, RouteLimits, RouteRequest, RouteResolver, WireModelId,
};

use crate::codex_model_cache::{CodexModelCacheFreshness, model_roster};
use crate::config::ApiProvider;

pub(crate) fn resolve_route_candidate(
    provider: ApiProvider,
    model_selector: Option<&str>,
    saved_provider_model: Option<&str>,
    base_url_override: Option<String>,
    context_window_override: Option<u32>,
) -> Result<ReadyRouteCandidate, String> {
    let route_request = RouteRequest {
        explicit_provider: provider.kind(),
        model_selector: model_selector.map(|model| LogicalModelRef::from(model.to_string())),
        saved_provider_model: saved_provider_model
            .map(|model| WireModelId::from(model.to_string())),
        base_url_override,
    };
    let mut candidate = RouteResolver::new()
        .resolve(&route_request)
        .map_err(|err| err.to_string())?;
    apply_context_window_override(&mut candidate.limits, context_window_override);
    if provider == ApiProvider::OpenaiCodex {
        // Models.dev describes the public API offering, not the account-scoped
        // ChatGPT OAuth route. Strip API-only limits, then carry the fresh
        // Codex roster's per-model context into every runtime consumer.
        candidate.limits.input_tokens = None;
        candidate.limits.output_tokens = None;
        if context_window_override.is_none() {
            let roster = model_roster();
            candidate.limits.context_tokens = if roster.freshness == CodexModelCacheFreshness::Fresh
            {
                roster
                    .metadata_for(candidate.wire_model_id.as_str())
                    .and_then(|metadata| metadata.context_window)
                    .map(u64::from)
            } else {
                None
            };
        }
    }
    Ok(candidate)
}

fn apply_context_window_override(limits: &mut RouteLimits, context_window: Option<u32>) {
    if let Some(context_window) = context_window.filter(|window| *window > 0) {
        limits.context_tokens = Some(u64::from(context_window));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_route_uses_fresh_account_context_and_drops_api_only_limits() {
        let _lock = crate::test_support::lock_test_env();
        let codex_home = tempfile::tempdir().expect("Codex home");
        let _home = crate::test_support::EnvVarGuard::set("CODEX_HOME", codex_home.path());
        std::fs::write(
            codex_home.path().join("models_cache.json"),
            serde_json::to_vec(&serde_json::json!({
                "fetched_at": chrono::Utc::now(),
                "models": [{
                    "slug": crate::config::DEFAULT_OPENAI_CODEX_MODEL,
                    "priority": 1,
                    "context_window": 128000,
                    "supported_reasoning_levels": [{"effort": "high"}]
                }]
            }))
            .expect("serialize cache"),
        )
        .expect("write cache");

        let candidate = resolve_route_candidate(
            ApiProvider::OpenaiCodex,
            Some(crate::config::DEFAULT_OPENAI_CODEX_MODEL),
            None,
            None,
            None,
        )
        .expect("Codex route");

        assert_eq!(candidate.limits.context_tokens, Some(128_000));
        assert_eq!(candidate.limits.input_tokens, None);
        assert_eq!(candidate.limits.output_tokens, None);
        assert_eq!(
            crate::route_budget::route_context_window_tokens(
                ApiProvider::OpenaiCodex,
                crate::config::DEFAULT_OPENAI_CODEX_MODEL,
                Some(candidate.limits),
            ),
            128_000
        );
    }
}
