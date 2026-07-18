//! Read-only canonical child-Agent projection for the Ocean work surface.

use crate::tui::app::App;
use codewhale_protocol::agent_runtime::TerminalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveWorkState {
    Active,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LiveWorkRow {
    pub identity: String,
    pub state: LiveWorkState,
    pub status: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LiveWorkProjection {
    pub rows: Vec<LiveWorkRow>,
    pub active: usize,
}

impl LiveWorkProjection {
    #[must_use]
    pub(super) fn from_app(app: &App) -> Self {
        let mut rows = app
            .child_agents
            .rows()
            .iter()
            .enumerate()
            .map(|(index, child)| LiveWorkRow {
                identity: format!("worker:{}", child.child_run_id),
                state: if child.is_active() {
                    LiveWorkState::Active
                } else {
                    LiveWorkState::Settled
                },
                status: child_status(child.terminal.as_ref()).to_string(),
                label: format!("子 Agent {}", index + 1),
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            matches!(right.state, LiveWorkState::Active)
                .cmp(&matches!(left.state, LiveWorkState::Active))
                .then_with(|| left.identity.cmp(&right.identity))
        });
        let active = rows
            .iter()
            .filter(|row| row.state == LiveWorkState::Active)
            .count();
        Self { rows, active }
    }
}

fn child_status(terminal: Option<&TerminalState>) -> &'static str {
    match terminal {
        None => "running",
        Some(TerminalState::Completed { .. }) => "done",
        Some(TerminalState::Blocked { .. }) => "blocked",
        Some(TerminalState::Failed { .. }) => "failed",
        Some(TerminalState::Cancelled) => "canceled",
        Some(TerminalState::Interrupted) => "interrupted",
        Some(TerminalState::RecoveryRequired { .. }) => "recovery",
    }
}
