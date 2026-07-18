use codewhale_config::route::RouteLimits;

use crate::config::{ApiProvider, provider_capability};

/// Preserve only route limits that came from a concrete offering.
#[must_use]
pub(crate) fn known_route_limits(limits: RouteLimits) -> Option<RouteLimits> {
    limits.has_known_limit().then_some(limits)
}

/// Context window for a resolved runtime route.
///
/// Route/offering facts win when known; otherwise this falls back to the
/// existing provider+model capability matrix so startup and custom/local
/// routes keep their previous conservative behavior.
#[must_use]
pub(crate) fn route_context_window_tokens(
    provider: ApiProvider,
    model: &str,
    route_limits: Option<RouteLimits>,
) -> u32 {
    route_limits
        .and_then(|limits| limits.context_tokens)
        .and_then(|tokens| u32::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0)
        .unwrap_or_else(|| provider_capability(provider, model).context_window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_missing_route_metadata_uses_provider_context_floor() {
        assert_eq!(
            route_context_window_tokens(ApiProvider::OpenaiCodex, "gpt-5.5", None),
            128_000
        );
    }
}
