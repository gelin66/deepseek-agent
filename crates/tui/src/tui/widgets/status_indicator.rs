use std::time::Instant;

const STATUS_INDICATOR_FRAME_MS: u128 = 420;
const STATUS_INDICATOR_WHALE_FRAMES: &[&str] = &[
    "🐳", "🐳.", "🐳..", "🐳...", "🐳..", "🐳.", "🐋", "🐋.", "🐋..", "🐋...", "🐋..", "🐋.",
];
const STATUS_INDICATOR_DOT_FRAMES: &[&str] = &["◍", "◉", "◌", "◌", "◉", "◍"];

/// Resolve the current status mark used by the canonical underwater header.
#[must_use]
pub fn header_status_indicator_frame(
    turn_started_at: Option<Instant>,
    mode: &str,
) -> Option<&'static str> {
    if matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "cw" | "mark" | "text"
    ) {
        return Some("cw");
    }
    let frames: &[&str] = match mode.trim().to_ascii_lowercase().as_str() {
        "off" | "none" | "hidden" | "false" => return None,
        "dots" | "dot" => STATUS_INDICATOR_DOT_FRAMES,
        "whale" | "🐳" | "🐋" => STATUS_INDICATOR_WHALE_FRAMES,
        _ => return Some("cw"),
    };
    let elapsed_ms = turn_started_at
        .map(|started_at| started_at.elapsed().as_millis())
        .unwrap_or(0);
    let index = (elapsed_ms / STATUS_INDICATOR_FRAME_MS) as usize % frames.len();
    Some(frames[index])
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::header_status_indicator_frame;

    #[test]
    fn canonical_names_select_visible_or_hidden_marks() {
        assert_eq!(header_status_indicator_frame(None, "cw"), Some("cw"));
        assert_eq!(header_status_indicator_frame(None, "whale"), Some("🐳"));
        assert_eq!(header_status_indicator_frame(None, "dots"), Some("◍"));
        assert_eq!(header_status_indicator_frame(None, "off"), None);
        assert_eq!(header_status_indicator_frame(None, "unknown"), Some("cw"));
    }

    #[test]
    fn active_mark_advances_from_turn_time() {
        let started_at = Instant::now() - Duration::from_millis(420);
        assert_eq!(
            header_status_indicator_frame(Some(started_at), "dots"),
            Some("◉")
        );
    }
}
