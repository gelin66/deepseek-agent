//! Native presentation-token audit.

use ratatui::style::Color;

#[path = "../src/palette/mod.rs"]
#[allow(dead_code)]
mod palette;

#[test]
fn terminal_owns_base_surfaces_and_inherited_text() {
    let theme = palette::TERMINAL_UI_THEME;
    assert_eq!(theme.name, "terminal");
    assert_eq!(theme.surface_bg, Color::Reset);
    assert_eq!(theme.panel_bg, Color::Reset);
    assert_eq!(theme.elevated_bg, Color::Reset);
    assert_eq!(theme.composer_bg, Color::Reset);
    assert_eq!(theme.text_body, Color::Reset);
    assert_eq!(theme.text_muted, Color::Reset);
}

#[test]
fn semantic_states_remain_distinct_without_truecolor() {
    let theme = palette::TERMINAL_UI_THEME;
    assert_eq!(theme.status_working, Color::Cyan);
    assert_eq!(theme.success, Color::Green);
    assert_eq!(theme.warning, Color::Yellow);
    assert_eq!(theme.error_fg, Color::Red);
    assert_ne!(theme.status_working, theme.success);
    assert_ne!(theme.success, theme.warning);
    assert_ne!(theme.warning, theme.error_fg);
}

#[test]
fn direct_palette_adapter_resolves_to_the_native_owner() {
    let theme = palette::TERMINAL_UI_THEME;
    assert_eq!(
        palette::adapt_bg_for_theme(palette::DSE_BG, palette::ThemeId::Terminal, &theme),
        theme.surface_bg
    );
    assert_eq!(
        palette::adapt_fg_for_theme(palette::DSE_INFO, palette::ThemeId::Terminal, &theme),
        theme.status_working
    );
    assert_eq!(
        palette::adapt_fg_for_theme(palette::STATUS_ERROR, palette::ThemeId::Terminal, &theme,),
        theme.error_fg
    );
}
