//! Ephemeral read model for canonical child-agent presentation.
//!
//! Rows are rebuilt from canonical runtime events and transcript entries. They
//! are never persisted and cannot drive execution or lifecycle decisions.

use codewhale_protocol::agent_runtime::{AgentOutcome, RunId, TerminalState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildAgentRow {
    pub parent_run_id: RunId,
    pub call_id: String,
    pub child_run_id: RunId,
    pub depth: u8,
    pub terminal: Option<TerminalState>,
    pub handoff_content: Option<String>,
}

impl ChildAgentRow {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.terminal.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildAgents {
    root_run_id: Option<RunId>,
    rows: Vec<ChildAgentRow>,
}

impl ChildAgents {
    #[must_use]
    pub fn rows(&self) -> &[ChildAgentRow] {
        &self.rows
    }

    pub fn active_rows(&self) -> impl Iterator<Item = &ChildAgentRow> {
        self.rows.iter().filter(|row| row.is_active())
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_rows().count()
    }

    #[must_use]
    pub fn has_active(&self) -> bool {
        self.rows.iter().any(ChildAgentRow::is_active)
    }

    pub(super) fn begin_root(&mut self, root_run_id: RunId) {
        if self.root_run_id.as_ref() != Some(&root_run_id) {
            self.root_run_id = Some(root_run_id);
            self.rows.clear();
        }
    }

    pub(super) fn record_started(
        &mut self,
        parent_run_id: RunId,
        call_id: String,
        child_run_id: RunId,
        depth: u8,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.child_run_id == child_run_id)
        {
            row.parent_run_id = parent_run_id;
            row.call_id = call_id;
            row.depth = depth;
            row.terminal = None;
            row.handoff_content = None;
            return;
        }
        self.rows.push(ChildAgentRow {
            parent_run_id,
            call_id,
            child_run_id,
            depth,
            terminal: None,
            handoff_content: None,
        });
    }

    pub(super) fn record_finished(
        &mut self,
        parent_run_id: RunId,
        call_id: String,
        outcome: &AgentOutcome,
        handoff_content: &str,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.child_run_id == outcome.run_id)
        {
            row.parent_run_id = parent_run_id;
            row.call_id = call_id;
            row.terminal = Some(outcome.terminal.clone());
            row.handoff_content = nonempty(handoff_content);
            return;
        }
        self.rows.push(ChildAgentRow {
            parent_run_id,
            call_id,
            child_run_id: outcome.run_id.clone(),
            depth: 0,
            terminal: Some(outcome.terminal.clone()),
            handoff_content: nonempty(handoff_content),
        });
    }
}

fn nonempty(content: &str) -> Option<String> {
    (!content.trim().is_empty()).then(|| content.to_owned())
}
