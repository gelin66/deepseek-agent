//! Remaining sub-agent presentation queries for the TUI shell.

use crate::tools::subagent::SubAgentStatus;
use crate::tui::app::{App, SidebarFocus};
use crate::tui::history::{HistoryCell, SubAgentCell};
use crate::tui::widgets::agent_card::AgentLifecycle;

fn agents_panel_has_content(app: &App) -> bool {
    !app.subagent_cache.is_empty()
        || !app.agent_progress.is_empty()
        || active_fanout_counts(app).is_some()
        || foreground_rlm_running(app)
}

fn foreground_rlm_running(app: &App) -> bool {
    use crate::tui::history::{HistoryCell, ToolCell, ToolStatus};
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|entry| {
            matches!(
                entry,
                HistoryCell::Tool(ToolCell::Generic(generic))
                    if matches!(
                        generic.name.as_str(),
                        "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close" | "rlm"
                    ) && generic.status == ToolStatus::Running
            )
        })
    })
}

/// True when the Agents sidebar panel is on-screen and already owns fanout summary.
pub(super) fn agents_sidebar_surface_visible(app: &App) -> bool {
    match app.sidebar_focus {
        SidebarFocus::Hidden => false,
        SidebarFocus::Agents => true,
        SidebarFocus::Auto => agents_panel_has_content(app),
        _ => false,
    }
}

pub(super) fn running_agent_count(app: &App) -> usize {
    let mut ids: std::collections::HashSet<&str> =
        app.agent_progress.keys().map(String::as_str).collect();
    for agent in app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
    {
        ids.insert(agent.agent_id.as_str());
    }
    ids.len()
}

pub(super) fn active_fanout_counts(app: &App) -> Option<(usize, usize)> {
    // Read running count from the current slot states on the active
    // FanoutCard, if one exists. Used by `rlm` and any future multi-child
    // dispatch the parent agent makes via repeated `agent`.
    if let Some(idx) = app.last_fanout_card_index
        && let Some(HistoryCell::SubAgent(SubAgentCell::Fanout(card))) = app.history.get(idx)
    {
        let running = card
            .workers
            .iter()
            .filter(|slot| matches!(slot.status, AgentLifecycle::Running))
            .count();
        return Some((running, card.worker_count()));
    }
    None
}
