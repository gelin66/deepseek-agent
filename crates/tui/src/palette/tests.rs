use ratatui::style::Color;

use super::{
    ColorDepth, DIFF_ADDED, DIFF_ADDED_BG, DSE_BG, DSE_INFO, TERMINAL_UI_THEME, TEXT_BODY, ThemeId,
    adapt_bg, adapt_bg_for_theme, adapt_color, adapt_fg_for_theme, normalize_hex_rgb_color,
    normalize_theme_name, parse_hex_rgb_color, reasoning_surface_tint,
};

#[test]
fn terminal_theme_is_the_only_native_surface_token_set() {
    assert_eq!(TERMINAL_UI_THEME.name, "terminal");
    assert_eq!(TERMINAL_UI_THEME.surface_bg, Color::Reset);
    assert_eq!(TERMINAL_UI_THEME.panel_bg, Color::Reset);
    assert_eq!(TERMINAL_UI_THEME.composer_bg, Color::Reset);
    assert_eq!(TERMINAL_UI_THEME.text_body, Color::Reset);
    assert_eq!(TERMINAL_UI_THEME.status_working, Color::Cyan);
    assert_eq!(TERMINAL_UI_THEME.success, Color::Green);
    assert_eq!(TERMINAL_UI_THEME.warning, Color::Yellow);
    assert_eq!(TERMINAL_UI_THEME.error_fg, Color::Red);
}

#[test]
fn fixed_token_adapter_maps_remaining_direct_palette_reads() {
    assert_eq!(
        adapt_bg_for_theme(DSE_BG, ThemeId::Terminal, &TERMINAL_UI_THEME),
        Color::Reset
    );
    assert_eq!(
        adapt_bg_for_theme(DIFF_ADDED_BG, ThemeId::Terminal, &TERMINAL_UI_THEME),
        Color::Reset
    );
    assert_eq!(
        adapt_fg_for_theme(TEXT_BODY, ThemeId::Terminal, &TERMINAL_UI_THEME),
        Color::Reset
    );
    assert_eq!(
        adapt_fg_for_theme(DSE_INFO, ThemeId::Terminal, &TERMINAL_UI_THEME),
        Color::Cyan
    );
    assert_eq!(
        adapt_fg_for_theme(DIFF_ADDED, ThemeId::Terminal, &TERMINAL_UI_THEME),
        Color::Green
    );
}

#[test]
fn retired_theme_names_normalize_to_the_single_native_surface_during_migration() {
    for name in [
        "system",
        "dark",
        "light",
        "grayscale",
        "tokyo-night",
        "terminal",
    ] {
        assert_eq!(normalize_theme_name(name), Some("terminal"));
    }
    assert_eq!(normalize_theme_name("whale"), None);
}

#[test]
fn hex_rgb_color_parser_accepts_hashless_and_normalizes() {
    assert_eq!(parse_hex_rgb_color("#1a1B26"), Some(Color::Rgb(26, 27, 38)));
    assert_eq!(parse_hex_rgb_color("1a1b26"), Some(Color::Rgb(26, 27, 38)));
    assert_eq!(
        normalize_hex_rgb_color("#1A1B26").as_deref(),
        Some("#1a1b26")
    );
    assert_eq!(parse_hex_rgb_color("#123"), None);
    assert_eq!(parse_hex_rgb_color("#zzzzzz"), None);
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
    assert!(matches!(
        reasoning_surface_tint(ColorDepth::Ansi256),
        Some(Color::Indexed(_))
    ));
}
