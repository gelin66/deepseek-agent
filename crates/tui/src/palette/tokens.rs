//! Fixed semantic colors inherited from the active terminal.

use ratatui::style::Color;

// The non-ratatui CLI projections use `colored::truecolor`, which accepts
// channel triples rather than `ratatui::Color`. They share these semantic
// roles instead of owning another palette.
pub const DSE_ACCENT_PRIMARY_RGB: (u8, u8, u8) = (53, 120, 229);
pub const DSE_INFO_RGB: (u8, u8, u8) = (79, 209, 197);
pub const DSE_ERROR_RGB: (u8, u8, u8) = (255, 92, 122);

pub const DSE_ACCENT_PRIMARY: Color = Color::Blue;
pub const DSE_INFO: Color = Color::Cyan;
pub const DSE_BG: Color = Color::Reset;
pub const DSE_PANEL: Color = Color::Reset;
pub const DSE_ERROR: Color = Color::Red;

pub const TEXT_BODY: Color = Color::Reset;
pub const TEXT_SECONDARY: Color = Color::Reset;
pub const TEXT_HINT: Color = Color::Reset;
pub const SELECTION_TEXT: Color = Color::White;
pub const TEXT_SOFT: Color = Color::Reset;
pub const TEXT_REASONING: Color = Color::Magenta;
pub const TEXT_PRIMARY: Color = TEXT_BODY;
pub const TEXT_MUTED: Color = TEXT_SECONDARY;
pub const TEXT_DIM: Color = TEXT_HINT;
pub const USER_BODY: Color = Color::Green;

pub const BORDER_COLOR: Color = Color::Reset;
pub const SURFACE_ELEVATED: Color = Color::Reset;
pub const SURFACE_REASONING_TINT: Color = Color::Reset;
pub const DIFF_ADDED_BG: Color = Color::Reset;
pub const DIFF_DELETED_BG: Color = Color::Reset;
pub const DIFF_ADDED: Color = Color::Green;
pub const ACCENT_REASONING_LIVE: Color = TEXT_REASONING;
pub const ACCENT_TOOL_LIVE: Color = Color::Cyan;
pub const ACCENT_TOOL_ISSUE: Color = Color::Red;
pub const STATUS_SUCCESS: Color = Color::Green;
pub const STATUS_WARNING: Color = Color::Yellow;
pub const STATUS_ERROR: Color = DSE_ERROR;
pub const SELECTION_BG: Color = Color::Blue;
pub const COMPOSER_BG: Color = DSE_PANEL;
