//! Legacy direct color constants retained only while M28 migrates every
//! renderer to [`super::TERMINAL_UI_THEME`].

use ratatui::style::Color;

pub const DSE_BG_RGB: (u8, u8, u8) = (10, 17, 32);
pub const DSE_PANEL_RGB: (u8, u8, u8) = (22, 34, 56);
pub const DSE_ELEVATED_RGB: (u8, u8, u8) = (36, 52, 78);
pub const DSE_SELECTION_RGB: (u8, u8, u8) = (40, 56, 84);
pub const DSE_TEXT_BODY_RGB: (u8, u8, u8) = (246, 242, 232);
pub const DSE_TEXT_SOFT_RGB: (u8, u8, u8) = (217, 224, 234);
pub const DSE_TEXT_MUTED_RGB: (u8, u8, u8) = (169, 180, 199);
pub const DSE_TEXT_HINT_RGB: (u8, u8, u8) = (138, 150, 174);
pub const DSE_ACCENT_PRIMARY_RGB: (u8, u8, u8) = (246, 196, 83);
pub const DSE_ACCENT_SECONDARY_RGB: (u8, u8, u8) = (79, 209, 197);
pub const DSE_ERROR_RGB: (u8, u8, u8) = (255, 92, 122);
pub const DSE_ERROR_SURFACE_RGB: (u8, u8, u8) = (42, 18, 26);
pub const DSE_WARNING_RGB: (u8, u8, u8) = (240, 160, 48);
pub const DSE_SUCCESS_RGB: (u8, u8, u8) = (155, 214, 111);
pub const DSE_INFO_RGB: (u8, u8, u8) = (106, 174, 242);
pub const DSE_BORDER_RGB: (u8, u8, u8) = (52, 88, 145);
pub const DSE_REASONING_TEXT_RGB: (u8, u8, u8) = (224, 153, 72);
pub const DSE_REASONING_SURFACE_RGB: (u8, u8, u8) = (42, 34, 24);
pub const DSE_REASONING_TINT_RGB: (u8, u8, u8) = (24, 36, 52);
pub const DSE_DIFF_ADDED_RGB: (u8, u8, u8) = (87, 199, 133);
pub const DSE_DIFF_ADDED_BG_RGB: (u8, u8, u8) = (18, 42, 34);
pub const DSE_DIFF_DELETED_BG_RGB: (u8, u8, u8) = (42, 18, 26);
pub const DSE_TOOL_LIVE_RGB: (u8, u8, u8) = (140, 190, 238);
pub const DSE_TOOL_ISSUE_RGB: (u8, u8, u8) = (198, 150, 160);
pub const DSE_TOOL_OUTPUT_RGB: (u8, u8, u8) = (194, 208, 224);
pub const DSE_TOOL_SURFACE_RGB: (u8, u8, u8) = (28, 40, 62);
pub const DSE_TOOL_ACTIVE_RGB: (u8, u8, u8) = (38, 54, 80);

pub const DSE_ACCENT_PRIMARY: Color = Color::Rgb(
    DSE_ACCENT_PRIMARY_RGB.0,
    DSE_ACCENT_PRIMARY_RGB.1,
    DSE_ACCENT_PRIMARY_RGB.2,
);
pub const DSE_INFO: Color = Color::Rgb(DSE_INFO_RGB.0, DSE_INFO_RGB.1, DSE_INFO_RGB.2);
pub const DSE_BG: Color = Color::Rgb(DSE_BG_RGB.0, DSE_BG_RGB.1, DSE_BG_RGB.2);
pub const DSE_PANEL: Color = Color::Rgb(DSE_PANEL_RGB.0, DSE_PANEL_RGB.1, DSE_PANEL_RGB.2);
pub const DSE_ERROR: Color = Color::Rgb(DSE_ERROR_RGB.0, DSE_ERROR_RGB.1, DSE_ERROR_RGB.2);

pub const TEXT_BODY: Color = Color::Rgb(
    DSE_TEXT_BODY_RGB.0,
    DSE_TEXT_BODY_RGB.1,
    DSE_TEXT_BODY_RGB.2,
);
pub const TEXT_SECONDARY: Color = Color::Rgb(
    DSE_TEXT_MUTED_RGB.0,
    DSE_TEXT_MUTED_RGB.1,
    DSE_TEXT_MUTED_RGB.2,
);
pub const TEXT_HINT: Color = Color::Rgb(
    DSE_TEXT_HINT_RGB.0,
    DSE_TEXT_HINT_RGB.1,
    DSE_TEXT_HINT_RGB.2,
);
pub const TEXT_ACCENT: Color = Color::Rgb(
    DSE_ACCENT_SECONDARY_RGB.0,
    DSE_ACCENT_SECONDARY_RGB.1,
    DSE_ACCENT_SECONDARY_RGB.2,
);
pub const SELECTION_TEXT: Color = TEXT_BODY;
pub const TEXT_SOFT: Color = Color::Rgb(
    DSE_TEXT_SOFT_RGB.0,
    DSE_TEXT_SOFT_RGB.1,
    DSE_TEXT_SOFT_RGB.2,
);
pub const TEXT_REASONING: Color = Color::Rgb(
    DSE_REASONING_TEXT_RGB.0,
    DSE_REASONING_TEXT_RGB.1,
    DSE_REASONING_TEXT_RGB.2,
);
pub const TEXT_PRIMARY: Color = TEXT_BODY;
pub const TEXT_MUTED: Color = TEXT_SECONDARY;
pub const TEXT_DIM: Color = TEXT_HINT;
pub const USER_BODY: Color = Color::Rgb(74, 222, 128);

pub const BORDER_COLOR: Color = Color::Rgb(DSE_BORDER_RGB.0, DSE_BORDER_RGB.1, DSE_BORDER_RGB.2);
pub const BACKGROUND_DARK: Color = DSE_BG;
pub const SURFACE_PANEL: Color = DSE_PANEL;
pub const SURFACE_ELEVATED: Color =
    Color::Rgb(DSE_ELEVATED_RGB.0, DSE_ELEVATED_RGB.1, DSE_ELEVATED_RGB.2);
pub const SURFACE_REASONING: Color = Color::Rgb(
    DSE_REASONING_SURFACE_RGB.0,
    DSE_REASONING_SURFACE_RGB.1,
    DSE_REASONING_SURFACE_RGB.2,
);
pub const SURFACE_REASONING_TINT: Color = Color::Rgb(
    DSE_REASONING_TINT_RGB.0,
    DSE_REASONING_TINT_RGB.1,
    DSE_REASONING_TINT_RGB.2,
);
pub const SURFACE_REASONING_ACTIVE: Color = Color::Rgb(58, 46, 32);
pub const SURFACE_TOOL: Color = Color::Rgb(
    DSE_TOOL_SURFACE_RGB.0,
    DSE_TOOL_SURFACE_RGB.1,
    DSE_TOOL_SURFACE_RGB.2,
);
pub const SURFACE_TOOL_ACTIVE: Color = Color::Rgb(
    DSE_TOOL_ACTIVE_RGB.0,
    DSE_TOOL_ACTIVE_RGB.1,
    DSE_TOOL_ACTIVE_RGB.2,
);
pub const SURFACE_SUCCESS: Color = Color::Rgb(18, 42, 37);
pub const SURFACE_ERROR: Color = Color::Rgb(
    DSE_ERROR_SURFACE_RGB.0,
    DSE_ERROR_SURFACE_RGB.1,
    DSE_ERROR_SURFACE_RGB.2,
);
pub const DIFF_ADDED_BG: Color = Color::Rgb(
    DSE_DIFF_ADDED_BG_RGB.0,
    DSE_DIFF_ADDED_BG_RGB.1,
    DSE_DIFF_ADDED_BG_RGB.2,
);
pub const DIFF_DELETED_BG: Color = Color::Rgb(
    DSE_DIFF_DELETED_BG_RGB.0,
    DSE_DIFF_DELETED_BG_RGB.1,
    DSE_DIFF_DELETED_BG_RGB.2,
);
pub const DIFF_ADDED: Color = Color::Rgb(
    DSE_DIFF_ADDED_RGB.0,
    DSE_DIFF_ADDED_RGB.1,
    DSE_DIFF_ADDED_RGB.2,
);
pub const ACCENT_REASONING_LIVE: Color = TEXT_REASONING;
pub const ACCENT_TOOL_LIVE: Color = Color::Rgb(
    DSE_TOOL_LIVE_RGB.0,
    DSE_TOOL_LIVE_RGB.1,
    DSE_TOOL_LIVE_RGB.2,
);
pub const ACCENT_TOOL_ISSUE: Color = Color::Rgb(
    DSE_TOOL_ISSUE_RGB.0,
    DSE_TOOL_ISSUE_RGB.1,
    DSE_TOOL_ISSUE_RGB.2,
);
pub const TEXT_TOOL_OUTPUT: Color = Color::Rgb(
    DSE_TOOL_OUTPUT_RGB.0,
    DSE_TOOL_OUTPUT_RGB.1,
    DSE_TOOL_OUTPUT_RGB.2,
);

pub const STATUS_SUCCESS: Color =
    Color::Rgb(DSE_SUCCESS_RGB.0, DSE_SUCCESS_RGB.1, DSE_SUCCESS_RGB.2);
pub const STATUS_WARNING: Color =
    Color::Rgb(DSE_WARNING_RGB.0, DSE_WARNING_RGB.1, DSE_WARNING_RGB.2);
pub const STATUS_ERROR: Color = DSE_ERROR;
pub const SELECTION_BG: Color = Color::Rgb(
    DSE_SELECTION_RGB.0,
    DSE_SELECTION_RGB.1,
    DSE_SELECTION_RGB.2,
);
pub const COMPOSER_BG: Color = DSE_PANEL;
