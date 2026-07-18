//! Documentation-only catalog of every user-facing keybinding.
//!
//! This module is the *single source of truth* for what shortcuts the help
//! overlay renders. The actual key handlers live in `tui/ui.rs` (and a few
//! sibling modules); they read keys directly off the crossterm event stream
//! and intentionally do **not** consult this catalog. The catalog exists so
//! that:
//!
//! 1. The help overlay (`tui/views/help.rs`) does not have to maintain a
//!    parallel list that silently rots when a handler is added or moved.
//! 2. New contributors have one place to look when answering "which keys are
//!    bound, and where do they go?"
//!
//! When you add or change a binding in `ui.rs`, **add or update the matching
//! entry here**. The compile-only side-effect of forgetting is a stale help
//! screen; there is no runtime crash, so the discipline lives in code review.
//!
//! Entries are grouped by `KeybindingSection`. The `chord` field is a
//! human-readable string formatted exactly the way it should appear in help —
//! we avoid storing `KeyBinding` values directly because many shortcuts are
//! pairs (`↑/↓`) or families (`1-8`) that don't map cleanly to a single
//! chord.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingSection {
    Navigation,
    Editing,
    Submission,
    Modes,
    Clipboard,
    Help,
}

impl KeybindingSection {
    pub fn label(self) -> Cow<'static, str> {
        use crate::localization::{MessageId, tr};
        let id = match self {
            Self::Navigation => MessageId::HelpSectionNavigation,
            Self::Editing => MessageId::HelpSectionEditing,
            Self::Submission => MessageId::HelpSectionActions,
            Self::Modes => MessageId::HelpSectionModes,
            Self::Clipboard => MessageId::HelpSectionClipboard,
            Self::Help => MessageId::HelpSectionHelp,
        };
        tr(id)
    }

    /// Stable ordering for help rendering — matches the variant declaration
    /// order; explicit so adding a section forces a deliberate placement.
    pub fn rank(self) -> u8 {
        match self {
            Self::Navigation => 0,
            Self::Editing => 1,
            Self::Submission => 2,
            Self::Modes => 3,
            Self::Clipboard => 4,
            Self::Help => 5,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KeybindingEntry {
    pub chord: &'static str,
    pub description_id: crate::localization::MessageId,
    pub section: KeybindingSection,
}

/// Canonical list of keybindings shown in the help overlay.
///
/// Strings are written in the same notation the existing help screen uses so
/// readers can cross-reference with documentation: `Ctrl+X`, `Alt+X`,
/// `Shift+X`, `↑/↓`, `PgUp/PgDn`, etc. Help renderers may apply per-platform
/// substitutions (e.g. `⌥` for Alt on macOS) at render time, but the catalog
/// itself stores the portable form.
pub const KEYBINDINGS: &[KeybindingEntry] = &[
    // --- Navigation ---
    KeybindingEntry {
        chord: "↑ / ↓",
        description_id: crate::localization::MessageId::KbScrollTranscript,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Alt+↑ / Alt+↓",
        description_id: crate::localization::MessageId::KbScrollTranscriptAlt,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Shift+↑ / Shift+↓",
        description_id: crate::localization::MessageId::KbBrowseHistory,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "PgUp / PgDn",
        description_id: crate::localization::MessageId::KbScrollPage,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Ctrl+Home / Ctrl+End",
        description_id: crate::localization::MessageId::KbJumpTopBottom,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Alt+G / Alt+Shift+G",
        description_id: crate::localization::MessageId::KbJumpTopBottomEmpty,
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Alt+[ / Alt+]",
        description_id: crate::localization::MessageId::KbJumpToolBlocks,
        section: KeybindingSection::Navigation,
    },
    // --- Editing ---
    KeybindingEntry {
        chord: "← / →",
        description_id: crate::localization::MessageId::KbMoveCursor,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Home / End",
        description_id: crate::localization::MessageId::KbJumpLineStartEnd,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+A / Ctrl+E",
        description_id: crate::localization::MessageId::KbJumpLineStartEnd,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Backspace / Delete",
        description_id: crate::localization::MessageId::KbDeleteChar,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+U",
        description_id: crate::localization::MessageId::KbClearDraft,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Alt+R",
        description_id: crate::localization::MessageId::KbSearchHistory,
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+J / Alt+Enter / Shift+Enter",
        description_id: crate::localization::MessageId::KbInsertNewline,
        section: KeybindingSection::Editing,
    },
    // --- Submission / actions ---
    KeybindingEntry {
        chord: "Enter",
        description_id: crate::localization::MessageId::KbSendDraft,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Esc",
        description_id: crate::localization::MessageId::KbCloseMenu,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+C",
        description_id: crate::localization::MessageId::KbCancelOrExit,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+D",
        description_id: crate::localization::MessageId::KbExitEmpty,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+K",
        description_id: crate::localization::MessageId::KbCommandPalette,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+P",
        description_id: crate::localization::MessageId::KbFuzzyFilePicker,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+Shift+T",
        description_id: crate::localization::MessageId::KbLiveTranscript,
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+T",
        description_id: crate::localization::MessageId::KbCycleThinking,
        section: KeybindingSection::Modes,
    },
    // --- Modes ---
    KeybindingEntry {
        chord: "Tab",
        description_id: crate::localization::MessageId::KbCompleteCycleModes,
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Shift+Tab",
        description_id: crate::localization::MessageId::KbCyclePermissions,
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Alt+P / Alt+A / Alt+Y",
        description_id: crate::localization::MessageId::KbAltJumpPlanAgentYolo,
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Alt+! / Alt+@ / Alt+# / Alt+$ / Alt+0 / Ctrl+Alt+0",
        description_id: crate::localization::MessageId::KbFocusSidebar,
        section: KeybindingSection::Modes,
    },
    // --- Clipboard ---
    KeybindingEntry {
        chord: "Ctrl+V",
        description_id: crate::localization::MessageId::KbPasteAttach,
        section: KeybindingSection::Clipboard,
    },
    KeybindingEntry {
        chord: "Ctrl+Shift+C",
        description_id: crate::localization::MessageId::KbCopySelection,
        section: KeybindingSection::Clipboard,
    },
    KeybindingEntry {
        chord: "Right click",
        description_id: crate::localization::MessageId::KbContextMenu,
        section: KeybindingSection::Clipboard,
    },
    KeybindingEntry {
        chord: "@path",
        description_id: crate::localization::MessageId::KbAttachPath,
        section: KeybindingSection::Clipboard,
    },
    // --- Help ---
    KeybindingEntry {
        // F1 is primary (with /help); Ctrl+/ is the secondary fallback.
        // Alt+? stays an unadvertised handler (TUI-DOG-003).
        chord: "F1 / Ctrl+/",
        description_id: crate::localization::MessageId::KbHelpOverlay,
        section: KeybindingSection::Help,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty_and_sections_have_entries() {
        assert!(KEYBINDINGS.iter().any(|entry| !entry.chord.is_empty()));
        // Every declared section should appear in the catalog at least once,
        // otherwise the help overlay would render an empty heading.
        let sections = [
            KeybindingSection::Navigation,
            KeybindingSection::Editing,
            KeybindingSection::Submission,
            KeybindingSection::Modes,
            KeybindingSection::Clipboard,
            KeybindingSection::Help,
        ];
        for section in sections {
            assert!(
                KEYBINDINGS.iter().any(|entry| entry.section == section),
                "no entries for section {section:?}"
            );
        }
    }

    #[test]
    fn help_advertises_f1_and_ctrl_slash_never_alt_question() {
        // TUI-DOG-003: Alt+? is not advertised anywhere; F1 (with /help) is
        // primary and Ctrl+/ is the secondary fallback.
        assert!(
            KEYBINDINGS.iter().any(|entry| {
                entry.section == KeybindingSection::Help
                    && entry.chord.contains("F1")
                    && entry.chord.contains("Ctrl+/")
            }),
            "help must document F1 with the Ctrl+/ fallback"
        );
        assert!(
            KEYBINDINGS
                .iter()
                .all(|entry| !entry.chord.contains("Alt+?")),
            "Alt+? must not be advertised in the help catalog"
        );
    }

    #[test]
    fn transcript_navigation_catalog_does_not_advertise_bare_typing_keys() {
        for stale in [
            "g / G",
            "[ / ]",
            "l",
            "?",
            "Ctrl+↑ / Ctrl+↓",
            "v",
            "v / Alt+V",
        ] {
            assert!(
                KEYBINDINGS.iter().all(|entry| entry.chord != stale),
                "stale handler-free chord remains documented: {stale}"
            );
        }
        for wired in ["Alt+G / Alt+Shift+G", "Alt+[ / Alt+]"] {
            assert!(
                KEYBINDINGS.iter().any(|entry| entry.chord == wired),
                "wired transcript shortcut missing from help: {wired}"
            );
        }
    }

    #[test]
    fn section_rank_is_a_total_order() {
        let sections = [
            KeybindingSection::Navigation,
            KeybindingSection::Editing,
            KeybindingSection::Submission,
            KeybindingSection::Modes,
            KeybindingSection::Clipboard,
            KeybindingSection::Help,
        ];
        let mut ranks: Vec<u8> = sections.iter().map(|s| s.rank()).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), sections.len(), "ranks must be unique");
    }
}
