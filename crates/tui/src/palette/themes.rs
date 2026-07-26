//! The single terminal-native surface token set.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTheme {
    pub name: &'static str,
    pub surface_bg: Color,
    pub panel_bg: Color,
    pub elevated_bg: Color,
    pub composer_bg: Color,
    pub selection_bg: Color,
    pub header_bg: Color,
    pub footer_bg: Color,
    pub text_dim: Color,
    pub text_hint: Color,
    pub text_muted: Color,
    pub text_body: Color,
    pub text_soft: Color,
    pub border: Color,
    pub accent_primary: Color,
    pub accent_secondary: Color,
    pub accent_action: Color,
    pub error_fg: Color,
    pub error_hover: Color,
    pub error_surface: Color,
    pub error_border: Color,
    pub error_text: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub mode_agent: Color,
    pub mode_yolo: Color,
    pub mode_plan: Color,
    pub mode_operate: Color,
    pub status_ready: Color,
    pub status_working: Color,
    pub status_warning: Color,
    pub diff_added_fg: Color,
    pub diff_deleted_fg: Color,
    pub diff_added_bg: Color,
    pub diff_deleted_bg: Color,
    pub tool_running: Color,
    pub tool_success: Color,
    pub tool_failed: Color,
}

pub const TERMINAL_UI_THEME: UiTheme = UiTheme {
    name: "terminal",
    surface_bg: Color::Reset,
    panel_bg: Color::Reset,
    elevated_bg: Color::Reset,
    composer_bg: Color::Reset,
    selection_bg: Color::Reset,
    header_bg: Color::Reset,
    footer_bg: Color::Reset,
    text_dim: Color::Reset,
    text_hint: Color::Reset,
    text_muted: Color::Reset,
    text_body: Color::Reset,
    text_soft: Color::Reset,
    border: Color::Reset,
    accent_primary: Color::Blue,
    accent_secondary: Color::Cyan,
    accent_action: Color::Yellow,
    error_fg: Color::Red,
    error_hover: Color::Red,
    error_surface: Color::Reset,
    error_border: Color::Red,
    error_text: Color::Red,
    warning: Color::Yellow,
    success: Color::Green,
    info: Color::Cyan,
    mode_agent: Color::Blue,
    mode_yolo: Color::Red,
    mode_plan: Color::Magenta,
    mode_operate: Color::Cyan,
    status_ready: Color::DarkGray,
    status_working: Color::Cyan,
    status_warning: Color::Yellow,
    diff_added_fg: Color::Green,
    diff_deleted_fg: Color::Red,
    diff_added_bg: Color::Reset,
    diff_deleted_bg: Color::Reset,
    tool_running: Color::Cyan,
    tool_success: Color::Green,
    tool_failed: Color::Red,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Terminal,
}

/// Temporary settings parser retained until M28-E deletes the retired display
/// settings. Production rendering always resolves to [`TERMINAL_UI_THEME`].
#[must_use]
pub fn normalize_theme_name(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "terminal" | "transparent" | "inherit" => Some("terminal"),
        "system" | "default" | "dark" | "light" | "grayscale" | "black-white" | "mono"
        | "solarized" | "solarized-light" | "catppuccin-mocha" | "tokyo-night" | "dracula"
        | "gruvbox-dark" | "claude" | "matrix" => Some("terminal"),
        _ => None,
    }
}

#[must_use]
pub fn parse_hex_rgb_color(value: &str) -> Option<Color> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if value.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&value[0..2], 16).ok()?;
    let green = u8::from_str_radix(&value[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color::Rgb(red, green, blue))
}

#[must_use]
pub fn normalize_hex_rgb_color(value: &str) -> Option<String> {
    parse_hex_rgb_color(value).map(hex_rgb_string)
}

#[must_use]
pub fn hex_rgb_string(color: Color) -> String {
    match color {
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        _ => String::new(),
    }
}
