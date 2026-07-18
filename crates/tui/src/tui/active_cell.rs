//! Active in-flight tool/exec cell — single mutable group that buffers parallel
//! tool work for the current turn.
//!
//! ## Why
//!
//! When the model issues parallel tool calls in a single assistant turn (e.g.
//! two `read_file` and one `grep_files` running concurrently), naively
//! appending each tool start as its own history cell makes the transcript
//! "bounce" as completions arrive out of order. Codex's pattern is to keep all
//! in-flight tool work in ONE active cell that mutates in place; once the turn
//! resolves the active cell finalizes into the transcript.
//!
//! ## Contract
//!
//! - At most one [`ActiveCell`] per turn. It holds zero or more
//!   [`HistoryCell`]s that are still being mutated (status `Running`, output
//!   pending, etc.).
//! - The owning [`crate::tui::app::App`] renders the active cell's contents
//!   AFTER `App.history` so they appear at the live tail.
//! - Cell indices used by helpers like `tool_cells` address the virtual
//!   sequence `App.history ++ active_cell.entries`. Each
//!   entry's index is `App.history.len() + entry_offset`.
//! - When a tool completes whose `tool_id` does not match any active entry
//!   (orphan), the caller pushes a finalized standalone cell into `App.history`
//!   instead of mutating the active group. This keeps `active_cell` a stable
//!   reflection of what was actually started, and avoids merging unrelated
//!   tool work.
//! - On `TurnComplete` (or cancellation) the active cell is "flushed":
//!   in-progress entries are marked with the supplied terminal status, then
//!   every entry is appended to `App.history`.
//!
//! ## Revision counter
//!
//! Cells inside the active group mutate without changing pointer identity, so
//! the transcript cache cannot rely on enum-equality for invalidation. We
//! expose `revision()` and `bump_revision()`; the renderer combines this with
//! `App.history_version` when computing per-cell revisions for the cache.

use crate::tui::history::{HistoryCell, ToolStatus};

/// In-flight active cell: a sequence of mutable [`HistoryCell`] entries.
///
/// Conceptually a single "live tail" cell in the Codex sense: it appears as
/// one logical block at the end of the transcript, but internally it is
/// composed of one or more entries (each rendered as its own
/// [`HistoryCell`]). The reason we keep them as separate entries — rather
/// than fusing into a single conceptual block — is that the existing history
/// renderer already knows how to draw each canonical tool entry correctly.
/// Coalescing into a second render path would duplicate that logic.
#[derive(Debug, Clone, Default)]
pub struct ActiveCell {
    entries: Vec<HistoryCell>,
    /// Tool ids currently associated with this active cell. The map values are
    /// indices into [`Self::entries`].
    tool_to_entry: std::collections::HashMap<String, usize>,
    /// Bumped on every mutation. Used by the transcript cache to know that
    /// the active cell needs re-rendering even though its position in the
    /// virtual cell list is unchanged.
    revision: u64,
}

impl ActiveCell {
    /// Create an empty active cell.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries (each rendered as its own [`HistoryCell`]).
    #[must_use]
    #[allow(dead_code)] // Public surface used by tests and future renderers.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Whether the active cell has any entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Read-only access to the underlying entries (for rendering).
    #[must_use]
    pub fn entries(&self) -> &[HistoryCell] {
        &self.entries
    }

    /// Mutable access to a specific entry. Bumps the revision counter so the
    /// renderer knows the cached lines are stale.
    pub fn entry_mut(&mut self, index: usize) -> Option<&mut HistoryCell> {
        if index < self.entries.len() {
            self.bump_revision();
            self.entries.get_mut(index)
        } else {
            None
        }
    }

    /// Current revision counter. Wraps on overflow which is fine for cache
    /// invalidation; the chance of a wrap-around collision is astronomical
    /// over a single session and any miss only causes one extra re-render.
    #[must_use]
    #[allow(dead_code)] // Used by App::bump_active_cell_revision and future cache wiring.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Increment the revision counter. Call any time an entry is mutated.
    pub fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Add a tool entry to the active cell.
    ///
    /// Returns the entry index (which the caller can record in
    /// `tool_cells_in_active`).
    ///
    /// `tool_id` is registered for the new (or updated) entry so future
    /// completion lookups can find it.
    pub fn push_tool(&mut self, tool_id: impl Into<String>, cell: HistoryCell) -> usize {
        let tool_id = tool_id.into();
        let entry_idx = self.entries.len();
        self.entries.push(cell);
        self.tool_to_entry.insert(tool_id, entry_idx);
        self.bump_revision();
        entry_idx
    }

    /// Push an entry with no tool id binding (used for non-tool grouping if
    /// ever needed). Currently unused; kept for symmetry with Codex which
    /// allows e.g. session-header cells to live in `active_cell`.
    #[allow(dead_code)]
    pub fn push_untracked(&mut self, cell: HistoryCell) -> usize {
        let entry_idx = self.entries.len();
        self.entries.push(cell);
        self.bump_revision();
        entry_idx
    }

    /// Look up the entry index that holds the given tool id.
    #[must_use]
    #[allow(dead_code)] // Reserved for the Codex-style "exec end target" lookup.
    pub fn entry_index_for_tool(&self, tool_id: &str) -> Option<usize> {
        self.tool_to_entry.get(tool_id).copied()
    }

    /// Remove the tool-id binding for an entry without removing the entry
    /// itself (the entry remains in the active group, presumably with its
    /// status updated).
    #[allow(dead_code)] // Reserved for cancellation paths that prune ids without flushing.
    pub fn forget_tool(&mut self, tool_id: &str) -> Option<usize> {
        self.tool_to_entry.remove(tool_id)
    }

    /// Drain every entry, returning them in insertion order. Resets internal
    /// state (revision is bumped via `bump_revision`).
    ///
    /// Callers use this on `TurnComplete` (or cancellation) to flush the
    /// active group into `App.history`.
    pub fn drain(&mut self) -> Vec<HistoryCell> {
        let entries = std::mem::take(&mut self.entries);
        self.tool_to_entry.clear();
        self.bump_revision();
        entries
    }

    /// Mark every still-running tool entry as `Failed` (used when the turn is
    /// cancelled mid-flight). Entries that already completed are left alone.
    ///
    /// `Failed` is the closest existing variant for "interrupted"; the cell's
    /// surrounding context (turn-status banner) tells the user it was a
    /// cancellation rather than a tool error.
    pub fn mark_in_progress_as_interrupted(&mut self) {
        for cell in &mut self.entries {
            mark_running_as_interrupted(cell);
        }
        self.bump_revision();
    }
}

fn mark_running_as_interrupted(cell: &mut HistoryCell) {
    let HistoryCell::Tool(tool) = cell else {
        return;
    };
    if tool.status == ToolStatus::Running {
        tool.status = ToolStatus::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::history::GenericToolCell;

    fn generic_cell(name: &str) -> HistoryCell {
        HistoryCell::Tool(GenericToolCell {
            name: name.to_string(),
            status: ToolStatus::Running,
            input_summary: None,
            output: None,
            prompts: None,
            output_summary: None,
            is_diff: false,
        })
    }

    #[test]
    fn push_tool_records_entry_and_revision_advances() {
        let mut cell = ActiveCell::new();
        let r0 = cell.revision();
        let idx = cell.push_tool("t1", generic_cell("exec_shell"));
        assert_eq!(idx, 0);
        assert_eq!(cell.entry_count(), 1);
        assert!(cell.revision() != r0);
        assert_eq!(cell.entry_index_for_tool("t1"), Some(0));
    }

    #[test]
    fn drain_resets_state_and_returns_in_order() {
        let mut cell = ActiveCell::new();
        cell.push_tool("a", generic_cell("exec_shell"));
        cell.push_tool("b", generic_cell("foo"));
        let drained = cell.drain();
        assert_eq!(drained.len(), 2);
        assert!(cell.is_empty());
        assert_eq!(cell.entry_index_for_tool("a"), None);
    }

    #[test]
    fn interrupt_marks_running_entries_failed() {
        let mut cell = ActiveCell::new();
        cell.push_tool("a", generic_cell("exec_shell"));
        cell.mark_in_progress_as_interrupted();
        let HistoryCell::Tool(tool) = &cell.entries()[0] else {
            panic!("expected canonical generic tool")
        };
        assert_eq!(tool.status, ToolStatus::Failed);
    }
}
