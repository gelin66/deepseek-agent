//! Small string builders that compose status-bar / footer chips and
//! one-off informational messages.
//!
//! Each helper is a pure function over a small slice of `App` or
//! response data. Grouped here so the composer/footer renderer doesn't
//! need to scroll past their bodies, and so the labels can be unit
//! tested in isolation.

/// Render the response body for `/models` / `models list` — the current
/// model is starred and other available models follow underneath.
pub(super) fn available_models_message(current_model: &str, models: &[String]) -> String {
    let mut lines = vec![format!("Available models ({})", models.len())];
    for model in models {
        if model == current_model {
            lines.push(format!("* {model} (current)"));
        } else {
            lines.push(format!("  {model}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_models_message_marks_current_model() {
        let models = vec![
            "deepseek-v4-pro".to_string(),
            "deepseek-v4-flash".to_string(),
        ];
        let msg = available_models_message("deepseek-v4-pro", &models);
        assert!(msg.contains("* deepseek-v4-pro (current)"), "got: {msg}");
        assert!(msg.contains("  deepseek-v4-flash"), "got: {msg}");
        assert!(msg.starts_with("Available models (2)"), "got: {msg}");
    }
}
