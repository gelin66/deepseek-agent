use crate::config::deepseek_capability;

/// Context window from the canonical DeepSeek capability owner.
#[must_use]
pub(crate) fn route_context_window_tokens(model: &str) -> u32 {
    deepseek_capability(model).context_window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_override_uses_official_deepseek_context() {
        assert_eq!(route_context_window_tokens("deepseek-v4-pro"), 1_000_000);
    }
}
