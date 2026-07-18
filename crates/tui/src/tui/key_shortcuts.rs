//! Keyboard-shortcut predicates and platform-specific labels.
//!
//! These helpers normalise the cross-platform variations between
//! `Ctrl+…` (Linux/Windows) and `Cmd+…` (macOS), legacy `Ctrl+H`-as-
//! backspace handling, and the macOS Option-Latin-character escapes.
//! Centralising them
//! keeps the composer / transcript event loops in `ui.rs` short and
//! lets us add a new platform without touching the call sites.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn has_control_like_modifier(modifiers: KeyModifiers) -> bool {
    has_control_like_modifier_for_platform(modifiers, cfg!(target_os = "macos"))
}

pub(super) fn has_control_like_modifier_for_platform(
    modifiers: KeyModifiers,
    is_macos: bool,
) -> bool {
    modifiers.contains(KeyModifiers::CONTROL)
        || (is_macos && modifiers.contains(KeyModifiers::SUPER))
}

/// Copy-to-clipboard: `Cmd+C` on macOS or `Ctrl+Shift+C` elsewhere.
pub(super) fn is_copy_shortcut(key: &KeyEvent) -> bool {
    let is_c = matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));
    if !is_c {
        return false;
    }

    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT)
}

/// Paste-from-clipboard: `Cmd+V` (macOS), `Ctrl+V` (Linux/Windows), or
/// the legacy raw `\u{16}` ETX byte some terminals emit.
pub(super) fn is_paste_shortcut(key: &KeyEvent) -> bool {
    let is_v = matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'));
    let is_legacy_ctrl_v = matches!(key.code, KeyCode::Char('\u{16}'));
    if !is_v && !is_legacy_ctrl_v {
        return false;
    }

    if is_legacy_ctrl_v {
        return true;
    }

    // Cmd+V on macOS
    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    // Ctrl+V on Linux/Windows
    key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Whether the key event represents a user typing a printable
/// character into the composer (no modifier that would turn it into
/// a shortcut).
pub(super) fn is_text_input_key(key: &KeyEvent) -> bool {
    if matches!(key.code, KeyCode::Char(c) if c.is_control()) {
        return false;
    }

    !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SUPER)
}

/// `Ctrl+H` is the legacy ASCII backspace many terminals still emit
/// when the user presses Backspace. Disallows Alt/Super so it doesn't
/// shadow window-management combos.
pub(super) fn is_ctrl_h_backspace(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('h'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SUPER)
}
