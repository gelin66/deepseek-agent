//! Color adaptation for terminal palette modes and the fixed native token set.

use ratatui::style::Color;

use super::themes::{ThemeId, UiTheme};
use super::tokens::*;

// M28 uses one terminal-native token set. Direct legacy palette constants are
// remapped here while their remaining render callers are migrated in M28-D/E.

/// Per-preset green accent used for things that semantically *should* stay
/// green even after theming (diff "+" lines, user-input body). Now delegates
/// to the active UiTheme's diff_added_fg.
#[must_use]
const fn theme_green(ui: &UiTheme) -> Color {
    ui.diff_added_fg
}

/// Per-preset dark-green diff-added background tint.
#[must_use]
const fn theme_diff_added_bg(ui: &UiTheme) -> Color {
    ui.diff_added_bg
}

/// Per-preset dark-red diff-deleted background tint.
#[must_use]
const fn theme_diff_deleted_bg(ui: &UiTheme) -> Color {
    ui.diff_deleted_bg
}

/// Remap a foreground color from the retired direct palette to the fixed
/// terminal-native roles.
#[must_use]
pub fn adapt_fg_for_theme(color: Color, _theme: ThemeId, ui: &UiTheme) -> Color {
    if color == TEXT_BODY || color == SELECTION_TEXT || color == Color::White {
        ui.text_body
    } else if color == TEXT_SECONDARY || color == TEXT_MUTED {
        ui.text_muted
    } else if color == TEXT_HINT || color == TEXT_DIM {
        ui.text_hint
    } else if color == TEXT_SOFT || color == TEXT_TOOL_OUTPUT {
        ui.text_soft
    } else if color == BORDER_COLOR {
        ui.border
    } else if color == TEXT_ACCENT || color == DSE_INFO || color == ACCENT_TOOL_LIVE {
        ui.status_working
    } else if color == TEXT_REASONING || color == ACCENT_REASONING_LIVE {
        ui.mode_plan
    } else if color == ACCENT_TOOL_ISSUE {
        ui.mode_yolo
    } else if color == STATUS_WARNING {
        ui.warning
    } else if color == STATUS_ERROR || color == DSE_ERROR {
        ui.error_fg
    } else if color == DIFF_ADDED || color == USER_BODY {
        theme_green(ui)
    } else if color == DSE_ACCENT_PRIMARY {
        ui.mode_agent
    } else {
        color
    }
}

/// Remap a background color from the retired direct palette to the fixed
/// terminal-native roles.
#[must_use]
pub fn adapt_bg_for_theme(color: Color, _theme: ThemeId, ui: &UiTheme) -> Color {
    if color == DSE_BG || color == BACKGROUND_DARK {
        ui.surface_bg
    } else if color == DSE_PANEL
        || color == COMPOSER_BG
        || color == SURFACE_PANEL
        || color == SURFACE_TOOL
    {
        ui.panel_bg
    } else if color == SURFACE_ELEVATED || color == SURFACE_TOOL_ACTIVE {
        ui.elevated_bg
    } else if color == SURFACE_REASONING
        || color == SURFACE_REASONING_TINT
        || color == SURFACE_REASONING_ACTIVE
    {
        ui.panel_bg
    } else if color == SURFACE_SUCCESS {
        ui.diff_added_bg
    } else if color == SURFACE_ERROR {
        ui.error_surface
    } else if color == SELECTION_BG {
        ui.selection_bg
    } else if color == DIFF_ADDED_BG {
        theme_diff_added_bg(ui)
    } else if color == DIFF_DELETED_BG {
        theme_diff_deleted_bg(ui)
    } else {
        color
    }
}

// === Terminal color-depth helpers ===

/// Terminal color depth, used to gate truecolor surfaces (e.g. reasoning bg
/// tints) on terminals that can't render them faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 16-color terminals (macOS Terminal.app default, dumb tmux setups).
    /// Background tints distort the named-palette mapping, so we drop them.
    Ansi16,
    /// 256-color terminals — RGB→256 fallback is faithful enough.
    Ansi256,
    /// True-color (24-bit) — render the palette verbatim.
    TrueColor,
}

impl ColorDepth {
    /// Detect the active terminal's color depth. Honors `COLORTERM`
    /// (truecolor / 24bit) first, then falls back to `TERM`. Defaults to
    /// `TrueColor` because most modern terminals support it; the conservative
    /// fallback is `Ansi16` so background tints disappear safely.
    #[must_use]
    pub fn detect() -> Self {
        if let Ok(ct) = std::env::var("COLORTERM") {
            let ct = ct.to_ascii_lowercase();
            if ct.contains("truecolor") || ct.contains("24bit") {
                return Self::TrueColor;
            }
        }
        if std::env::var_os("WT_SESSION").is_some() {
            return Self::TrueColor;
        }
        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
            let term_program = term_program.to_ascii_lowercase();
            if term_program.contains("iterm")
                || term_program.contains("wezterm")
                || term_program.contains("vscode")
                || term_program.contains("warp")
            {
                return Self::TrueColor;
            }
        }
        let term = std::env::var("TERM").unwrap_or_default();
        let term = term.to_ascii_lowercase();
        if term.contains("truecolor") || term.contains("24bit") {
            Self::TrueColor
        } else if term.contains("256") {
            Self::Ansi256
        } else if term.is_empty() || term == "dumb" {
            Self::Ansi16
        } else {
            // Unknown TERM strings should not receive 24-bit SGR by default.
            // Older macOS/remote terminals can render truecolor backgrounds as
            // bright cyan blocks; 256-color output is the safer compromise.
            Self::Ansi256
        }
    }
}

/// Adapt a foreground color to the terminal's color depth.
///
/// On TrueColor, `color` passes through. On Ansi256 we let ratatui's renderer
/// down-convert (it does this already). On Ansi16 we strip RGB to a near
/// named color so semantic intent survives even on legacy terminals.
#[must_use]
pub fn adapt_color(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (_, ColorDepth::TrueColor) => color,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

/// Adapt a background color. On Ansi16 terminals background tints are noisy,
/// so we drop them to `Color::Reset` rather than attempt a coarse named-color
/// match — a quiet background reads cleaner than a wrong one.
#[must_use]
pub fn adapt_bg(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (_, ColorDepth::TrueColor) => color,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (_, ColorDepth::Ansi256) => color,
        (_, ColorDepth::Ansi16) => Color::Reset,
    }
}

/// Return the dedicated reasoning surface tint for terminals that can render
/// background colors faithfully. ANSI-16 terminals disable the tint because
/// the nearest named background is too coarse for this subtle treatment.
#[must_use]
pub fn reasoning_surface_tint(depth: ColorDepth) -> Option<Color> {
    match depth {
        ColorDepth::Ansi16 => None,
        _ => Some(adapt_bg(SURFACE_REASONING_TINT, depth)),
    }
}

/// Map an RGB triple to its closest ANSI-16 named color. Only used by
/// `adapt_color` on Ansi16 terminals; we lean on hue dominance + lightness so
/// brand colors land on the obviously-related named entry (sky → cyan, blue →
/// blue, red → red, etc.) rather than dithering around grey.
pub(crate) fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    let lum = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    if lum < 24 {
        return Color::Black;
    }
    if r > 220 && g > 220 && b > 220 {
        return Color::White;
    }
    let bright = lum > 144;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max.saturating_sub(min) < 16 {
        return if bright { Color::Gray } else { Color::DarkGray };
    }
    if r >= g && r >= b {
        if g > b + 24 {
            if bright {
                Color::LightYellow
            } else {
                Color::Yellow
            }
        } else if b > r.saturating_sub(24) {
            if bright {
                Color::LightMagenta
            } else {
                Color::Magenta
            }
        } else if bright {
            Color::LightRed
        } else {
            Color::Red
        }
    } else if g >= r && g >= b {
        if b > r + 24 {
            if bright {
                Color::LightCyan
            } else {
                Color::Cyan
            }
        } else if bright {
            Color::LightGreen
        } else {
            Color::Green
        }
    } else if r.saturating_add(48) >= b && r > g + 24 {
        if bright {
            Color::LightMagenta
        } else {
            Color::Magenta
        }
    } else if g.saturating_add(48) >= b && g > r + 24 {
        if bright {
            Color::LightCyan
        } else {
            Color::Cyan
        }
    } else if bright {
        Color::LightBlue
    } else {
        Color::Blue
    }
}

/// Map an RGB color to the nearest xterm 256-color palette index. We use only
/// the stable 6x6x6 cube and grayscale ramp (16..255), not the terminal's
/// user-configurable 0..15 colors.
pub(crate) fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    fn nearest_cube_level(channel: u8) -> usize {
        CUBE_LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| channel.abs_diff(**level))
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn dist_sq(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
        let dr = i32::from(a.0) - i32::from(b.0);
        let dg = i32::from(a.1) - i32::from(b.1);
        let db = i32::from(a.2) - i32::from(b.2);
        (dr * dr + dg * dg + db * db) as u32
    }

    let ri = nearest_cube_level(r);
    let gi = nearest_cube_level(g);
    let bi = nearest_cube_level(b);
    let cube_rgb = (CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]);
    let cube_index = 16 + (36 * ri) as u8 + (6 * gi) as u8 + bi as u8;

    let avg = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
    let gray_i = if avg <= 8 {
        0
    } else if avg >= 238 {
        23
    } else {
        ((u16::from(avg) - 8 + 5) / 10).min(23) as u8
    };
    let gray = 8 + 10 * gray_i;
    let gray_index = 232 + gray_i;

    if dist_sq((r, g, b), (gray, gray, gray)) < dist_sq((r, g, b), cube_rgb) {
        gray_index
    } else {
        cube_index
    }
}
