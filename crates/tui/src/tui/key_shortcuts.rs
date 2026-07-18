//! Keyboard predicate shared by the canonical onboarding input path.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
