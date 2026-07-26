//! Native presentation-token audit.

use ratatui::style::Color;

#[path = "../src/palette/mod.rs"]
#[allow(dead_code)]
mod palette;

#[test]
fn terminal_owns_base_surfaces_and_inherited_text() {
    assert_eq!(palette::DSE_BG, Color::Reset);
    assert_eq!(palette::DSE_PANEL, Color::Reset);
    assert_eq!(palette::SURFACE_ELEVATED, Color::Reset);
    assert_eq!(palette::COMPOSER_BG, Color::Reset);
    assert_eq!(palette::TEXT_BODY, Color::Reset);
    assert_eq!(palette::TEXT_MUTED, Color::Reset);
}

#[test]
fn semantic_states_remain_distinct_without_truecolor() {
    assert_eq!(palette::DSE_INFO, Color::Cyan);
    assert_eq!(palette::STATUS_SUCCESS, Color::Green);
    assert_eq!(palette::STATUS_WARNING, Color::Yellow);
    assert_eq!(palette::STATUS_ERROR, Color::Red);
    assert_ne!(palette::DSE_INFO, palette::STATUS_SUCCESS);
    assert_ne!(palette::STATUS_SUCCESS, palette::STATUS_WARNING);
    assert_ne!(palette::STATUS_WARNING, palette::STATUS_ERROR);
}
