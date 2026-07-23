use codewhale_config::route::RouteLimits;

use crate::config::deepseek_capability;

/// Context window for a resolved runtime route.
///
/// Route/offering facts win when known; otherwise this falls back to the
/// existing provider+model capability matrix so startup and custom/local
/// routes keep their previous conservative behavior.
#[must_use]
pub(crate) fn route_context_window_tokens(model: &str, route_limits: Option<RouteLimits>) -> u32 {
    route_limits
        .and_then(|limits| limits.context_tokens)
        .and_then(|tokens| u32::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0)
        .unwrap_or_else(|| deepseek_capability(model).context_window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_override_uses_official_deepseek_context() {
        assert_eq!(
            route_context_window_tokens("deepseek-v4-pro", None),
            1_000_000
        );
    }
}
