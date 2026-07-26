use ratatui::style::Color;

use super::{
    ColorDepth, DIFF_ADDED, DIFF_ADDED_BG, DSE_BG, DSE_INFO, STATUS_ERROR, STATUS_SUCCESS,
    STATUS_WARNING, TEXT_BODY, adapt_bg, adapt_color, reasoning_surface_tint,
};

#[test]
fn fixed_tokens_are_the_only_native_surface_owner() {
    assert_eq!(DSE_BG, Color::Reset);
    assert_eq!(TEXT_BODY, Color::Reset);
    assert_eq!(DIFF_ADDED_BG, Color::Reset);
    assert_eq!(DSE_INFO, Color::Cyan);
    assert_eq!(DIFF_ADDED, Color::Green);
    assert_eq!(STATUS_SUCCESS, Color::Green);
    assert_eq!(STATUS_WARNING, Color::Yellow);
    assert_eq!(STATUS_ERROR, Color::Red);
}

#[test]
fn color_depth_preserves_semantics_across_terminal_capabilities() {
    let rgb = Color::Rgb(53, 120, 229);
    assert_eq!(adapt_color(rgb, ColorDepth::TrueColor), rgb);
    assert!(matches!(
        adapt_color(rgb, ColorDepth::Ansi256),
        Color::Indexed(_)
    ));
    assert_eq!(
        adapt_color(Color::Rgb(255, 92, 122), ColorDepth::Ansi16),
        Color::LightRed
    );
    assert_eq!(
        adapt_bg(Color::Rgb(24, 36, 52), ColorDepth::Ansi16),
        Color::Reset
    );
}

#[test]
fn subtle_reasoning_surface_is_disabled_on_ansi16() {
    assert!(reasoning_surface_tint(ColorDepth::Ansi16).is_none());
    assert!(reasoning_surface_tint(ColorDepth::TrueColor).is_some());
    assert_eq!(
        reasoning_surface_tint(ColorDepth::Ansi256),
        Some(Color::Reset)
    );
}
