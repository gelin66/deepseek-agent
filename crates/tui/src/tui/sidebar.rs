//! Sidebar rendering — Pinned / Activity / Agents / Context panels.
//!
//! Extracted from `tui/ui.rs` (P1.2). The sidebar appears to the right of
//! the chat transcript when the available width allows it. Each section
//! reads from `App` snapshots; mutation lives in the main app loop.

use std::fmt::Write;
use std::time::Duration;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Widget,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::deepseek_theme::Theme;
use crate::localization::MessageId;
use crate::palette;
use codewhale_protocol::agent_runtime::TerminalState;

use super::app::{
    App, SidebarFocus, SidebarHoverRow, SidebarHoverSection, SidebarHoverState, SidebarRowAction,
};
use super::history::{GenericToolCell, HistoryCell, ToolStatus, summarize_tool_output};
use super::ui_text::truncate_line_to_width;

/// Tolerance for floating-point cost comparison in the sidebar breakdown.
/// Must be large enough that accumulated f64 error across hundreds of turns
/// does not prematurely hide the session+agents breakdown.
const COST_EQ_TOLERANCE: f64 = 1e-6;
const RECENT_TOOL_SCAN_LIMIT: usize = 24;
const ACTIVE_TOOL_COMPLETED_ROW_TTL: Duration = Duration::from_secs(8);
const ACTIVE_TOOL_STALE_RUNNING_ROW_TTL: Duration = Duration::from_secs(600);

/// The explicit Agents view remains available for settled children. Auto mode
/// only claims screen space while canonical child work is active.
pub(crate) fn agents_sidebar_surface_visible(app: &App) -> bool {
    match app.sidebar_focus {
        SidebarFocus::Hidden => false,
        SidebarFocus::Agents => true,
        SidebarFocus::Auto => app.child_agents.has_active(),
        _ => false,
    }
}

pub(crate) fn running_agent_count(app: &App) -> usize {
    app.child_agents.active_count()
}

pub fn render_sidebar(f: &mut Frame, area: Rect, app: &mut App) {
    // Clear hover state at the start of each render
    app.sidebar_hover = SidebarHoverState::default();
    if area.width < 20 || area.height < 3 {
        // Paint a styled block over the area so stale cells from a previous
        // (wider) frame don't persist as bleed-through artifacts (#400).
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut());
        return;
    }

    if app.sidebar_focus == SidebarFocus::Hidden {
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut());
        return;
    }

    match app.sidebar_focus {
        SidebarFocus::Auto => render_sidebar_auto(f, area, app),
        SidebarFocus::Pinned => render_sidebar_pinned(f, area, app),
        SidebarFocus::Tasks => render_sidebar_tasks(f, area, app),
        SidebarFocus::Agents => render_sidebar_subagents(f, area, app),
        SidebarFocus::Context => render_context_panel(f, area, app),
        SidebarFocus::Hidden => unreachable!("hidden sidebar returned before render dispatch"),
    }
}

/// Build the Auto-mode panel stack. Empty panels collapse to zero height so
/// non-empty ones get the full sidebar real estate.
fn render_sidebar_auto(f: &mut Frame, area: Rect, app: &mut App) {
    let visible = auto_sidebar_panels(auto_sidebar_state(app));
    render_sidebar_panel_stack(f, area, app, &visible);
}

/// Build the pinned panel stack. This uses the same content-sensitive panels
/// as Auto, but it never participates in idle auto-collapse.
fn render_sidebar_pinned(f: &mut Frame, area: Rect, app: &mut App) {
    let visible = auto_sidebar_panels(auto_sidebar_state(app));
    render_sidebar_panel_stack(f, area, app, &visible);
}

fn render_sidebar_panel_stack(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    visible: &[AutoSidebarPanel],
) {
    let constraints: Vec<Constraint> = match visible.len() {
        1 => vec![Constraint::Min(0)],
        2 => vec![Constraint::Percentage(50), Constraint::Min(0)],
        3 => vec![
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Min(0),
        ],
        4 => vec![
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Min(3),
        ],
        _ => vec![
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Min(6),
        ],
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (panel, rect) in visible.iter().zip(sections.iter()) {
        match panel {
            AutoSidebarPanel::Agents => render_sidebar_subagents(f, *rect, app),
            AutoSidebarPanel::Context => render_context_panel(f, *rect, app),
        }
    }
}

/// Compute the Auto-mode panel signals. Shared by `render_sidebar_auto` (which
/// panel boxes to show) and `sidebar_auto_idle` (whether to collapse the whole
/// sidebar to a full-width transcript).
fn auto_sidebar_state(app: &mut App) -> AutoSidebarState {
    AutoSidebarState {
        // Auto mode follows live canonical child work only. Settled children
        // remain available when the user explicitly opens the Agents panel.
        agents_empty: !app.child_agents.has_active(),
        context_enabled: app.context_panel,
    }
}

/// Auto-reveal: in Auto focus mode the sidebar collapses to nothing when there
/// is no active canonical child work or pinned context, so an
/// idle session gets a full-width transcript. Explicit focus and Hidden bypass
/// this (the former should always show, the latter is handled by the width helper).
pub(crate) fn sidebar_auto_idle(app: &mut App) -> bool {
    if app.sidebar_focus != SidebarFocus::Auto {
        return false;
    }
    let state = auto_sidebar_state(app);
    state.agents_empty && !state.context_enabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoSidebarPanel {
    Agents,
    Context,
}

#[derive(Debug, Clone, Copy)]
struct AutoSidebarState {
    agents_empty: bool,
    context_enabled: bool,
}

fn auto_sidebar_panels(state: AutoSidebarState) -> Vec<AutoSidebarPanel> {
    let mut visible = Vec::with_capacity(2);
    if !state.agents_empty {
        visible.push(AutoSidebarPanel::Agents);
    }
    if state.context_enabled {
        visible.push(AutoSidebarPanel::Context);
    }

    visible
}

fn render_sidebar_tasks(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let row_sets = task_panel_row_sets(app);
    let (lines, row_actions) =
        task_panel_rows(app, &row_sets, content_width.max(1), usable_rows.max(1));

    let full_texts = task_panel_hover_texts(app, &row_sets, usable_rows.max(1));
    // #4147: This panel renders live tools / background jobs, not durable task
    // state, so the user-facing label is "Activity" to match its contents and
    // avoid colliding with durable tasks. The internal identifiers keep the
    // "task_panel"/`SidebarFocus::Tasks` names (guard #4172).
    render_sidebar_section(f, area, "Activity", lines, full_texts, row_actions, app);
}

#[derive(Debug, Clone)]
struct SidebarToolRow {
    name: String,
    status: ToolStatus,
    summary: String,
    duration_ms: Option<u64>,
}

/// Row sets shared by the Tasks panel line renderer and hover-text builder.
///
/// Computed once per frame so the line and hover projections share the same
/// canonical tool snapshot.
struct TaskPanelRowSets {
    active: Vec<SidebarToolRow>,
    recent: Vec<SidebarToolRow>,
}

fn task_panel_row_sets(app: &App) -> TaskPanelRowSets {
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;
    let active = active_tool_rows(app);
    let recent = if explicit_tasks_focus {
        recent_tool_rows(app, 4)
    } else {
        Vec::new()
    };
    TaskPanelRowSets { active, recent }
}

#[cfg(test)]
fn task_panel_lines(app: &App, content_width: usize, max_rows: usize) -> Vec<Line<'static>> {
    task_panel_rows(app, &task_panel_row_sets(app), content_width, max_rows).0
}

/// Build the Activity panel lines together with a parallel per-line click-action
/// vector (#3028). Producing both in a single pass keeps the action indices
/// aligned with the rendered lines no matter how the layout evolves.
fn task_panel_rows(
    app: &App,
    row_sets: &TaskPanelRowSets,
    content_width: usize,
    max_rows: usize,
) -> (Vec<Line<'static>>, Vec<Option<SidebarRowAction>>) {
    let theme = &app.ui_theme;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let mut actions: Vec<Option<SidebarRowAction>> = Vec::with_capacity(max_rows.max(4));
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;

    if explicit_tasks_focus && app.runtime_turn_id.is_some() {
        let status = app
            .runtime_turn_status
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        // #3030: Use a stable turn number ("Turn 1") instead of the raw
        // UUID prefix.  The full UUID is preserved in the hover text
        // (task_panel_hover_texts) for inspection.
        let turn_label = if app.turn_counter > 0 {
            format!("Turn {} ({status})", app.turn_counter)
        } else {
            format!("Current turn ({status})")
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&turn_label, content_width.max(1)),
            Style::default().fg(theme.accent_primary),
        )));
    }

    let active_rows = &row_sets.active;
    if explicit_tasks_focus && !active_rows.is_empty() && lines.len() < max_rows {
        push_sidebar_label_theme(&mut lines, "Live tools", theme);
        push_tool_rows(&mut lines, active_rows, content_width, max_rows, theme);
    }

    actions.resize(lines.len(), None);

    if explicit_tasks_focus && lines.len() < max_rows {
        let recent_rows = &row_sets.recent;
        if !recent_rows.is_empty() {
            push_sidebar_label_theme(&mut lines, "Recent tools", theme);
            push_tool_rows(&mut lines, recent_rows, content_width, max_rows, theme);
        }
    }

    // Yank hint: surface the keyboard path for copying the focused task/turn ID.
    if lines.len() + 1 < max_rows
        && app.runtime_turn_id.is_some()
        && app.sidebar_focus == SidebarFocus::Tasks
    {
        lines.push(Line::from(Span::styled(
            "y → copy turn id  ·  Y → copy full status",
            Style::default()
                .fg(theme.text_dim)
                .add_modifier(ratatui::style::Modifier::ITALIC),
        )));
    }

    if lines.is_empty()
        || (lines.len() == 1 && app.runtime_turn_id.is_some() && active_rows.is_empty())
    {
        lines.push(Line::from(Span::styled(
            "No live tools",
            Style::default().fg(theme.text_muted),
        )));
    }

    // Backfill action slots for the trailing non-clickable lines (Recent
    // tools, yank hint, empty-state notice).
    actions.resize(lines.len(), None);
    (lines, actions)
}

fn task_panel_hover_texts(app: &App, row_sets: &TaskPanelRowSets, max_rows: usize) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;

    if explicit_tasks_focus && let Some(turn_id) = app.runtime_turn_id.as_ref() {
        let status = app.runtime_turn_status.as_deref().unwrap_or("unknown");
        texts.push(format!("turn {turn_id} ({status})"));
    }

    let active_rows = &row_sets.active;
    if explicit_tasks_focus && !active_rows.is_empty() && texts.len() < max_rows {
        texts.push("Live tools".to_string());
        push_tool_row_hover_texts(&mut texts, active_rows, max_rows);
    }

    if explicit_tasks_focus && texts.len() < max_rows {
        let recent_rows = &row_sets.recent;
        if !recent_rows.is_empty() {
            texts.push("Recent tools".to_string());
            push_tool_row_hover_texts(&mut texts, recent_rows, max_rows);
        }
    }

    if texts.len() + 1 < max_rows
        && app.runtime_turn_id.is_some()
        && app.sidebar_focus == SidebarFocus::Tasks
    {
        texts.push("y -> copy turn id  ·  Y -> copy full status".to_string());
    }

    if texts.is_empty()
        || (texts.len() == 1 && app.runtime_turn_id.is_some() && active_rows.is_empty())
    {
        texts.push("No live tools".to_string());
    }

    texts
}

fn push_sidebar_label_theme(lines: &mut Vec<Line<'static>>, label: &str, theme: &palette::UiTheme) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.accent_primary).bold(),
    )));
}

fn push_tool_row_hover_texts(texts: &mut Vec<String>, rows: &[SidebarToolRow], max_rows: usize) {
    for row in rows {
        if texts.len() >= max_rows {
            break;
        }
        let (marker, _) = tool_status_marker(row.status, &palette::UI_THEME);
        let label = if let Some(duration_ms) = row.duration_ms {
            format!("{marker} {} {}", row.name, format_duration_ms(duration_ms))
        } else {
            format!("{marker} {}", row.name)
        };
        texts.push(label);
        if !row.summary.trim().is_empty() && texts.len() < max_rows {
            texts.push(format!("  {}", row.summary));
        }
    }
}

fn active_tool_rows(app: &App) -> Vec<SidebarToolRow> {
    let Some(active) = app.active_cell.as_ref() else {
        return Vec::new();
    };
    let mut rows: Vec<SidebarToolRow> = Vec::new();
    let mut stale_running: Vec<SidebarToolRow> = Vec::new();
    for (entry_idx, cell) in active.entries().iter().enumerate() {
        let Some(row) = sidebar_tool_row_from_cell(cell) else {
            continue;
        };
        match active_tool_row_visibility(app, entry_idx, &row) {
            ActiveToolRowVisibility::Visible => rows.push(row),
            ActiveToolRowVisibility::StaleRunning => stale_running.push(row),
            ActiveToolRowVisibility::Hidden => {}
        }
    }
    if !stale_running.is_empty() {
        rows.push(collapsed_stale_running_row(stale_running));
    }
    editorial_tool_rows(rows, usize::MAX, ToolRowOrder::OldestFirst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveToolRowVisibility {
    Visible,
    StaleRunning,
    Hidden,
}

fn active_tool_row_visibility(
    app: &App,
    entry_idx: usize,
    row: &SidebarToolRow,
) -> ActiveToolRowVisibility {
    if row.status == ToolStatus::Running {
        return if row
            .duration_ms
            .is_some_and(|ms| ms >= duration_ms(ACTIVE_TOOL_STALE_RUNNING_ROW_TTL))
        {
            ActiveToolRowVisibility::StaleRunning
        } else {
            ActiveToolRowVisibility::Visible
        };
    }

    let Some(completed_at) = app.active_tool_entry_completed_at.get(&entry_idx) else {
        return ActiveToolRowVisibility::Hidden;
    };
    if completed_at.elapsed() <= ACTIVE_TOOL_COMPLETED_ROW_TTL {
        ActiveToolRowVisibility::Visible
    } else {
        ActiveToolRowVisibility::Hidden
    }
}

fn collapsed_stale_running_row(rows: Vec<SidebarToolRow>) -> SidebarToolRow {
    let count = rows.len();
    let oldest_ms = rows
        .iter()
        .filter_map(|row| row.duration_ms)
        .max()
        .unwrap_or_default();
    let first_summary = rows
        .iter()
        .find_map(|row| (!row.summary.trim().is_empty()).then(|| row.summary.clone()))
        .unwrap_or_else(|| "工具仍在运行".to_string());
    SidebarToolRow {
        name: if count == 1 {
            "run".to_string()
        } else {
            format!("run x{count}")
        },
        status: ToolStatus::Running,
        summary: format!("long-running · {first_summary}"),
        duration_ms: (oldest_ms > 0).then_some(oldest_ms),
    }
}

fn recent_tool_rows(app: &App, limit: usize) -> Vec<SidebarToolRow> {
    let rows: Vec<SidebarToolRow> = app
        .history
        .iter()
        .rev()
        .filter_map(sidebar_tool_row_from_cell)
        .take(RECENT_TOOL_SCAN_LIMIT)
        .collect();
    editorial_tool_rows(rows, limit, ToolRowOrder::NewestFirst)
}

fn push_tool_rows(
    lines: &mut Vec<Line<'static>>,
    rows: &[SidebarToolRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) {
    for row in rows {
        if lines.len() >= max_rows {
            break;
        }
        let (marker, color) = tool_status_marker(row.status, theme);
        let label = if let Some(duration_ms) = row.duration_ms {
            format!("{marker} {} {}", row.name, format_duration_ms(duration_ms))
        } else {
            format!("{marker} {}", row.name)
        };
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&label, content_width),
            Style::default().fg(color),
        )));
        if !row.summary.trim().is_empty() && lines.len() < max_rows {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(&row.summary, content_width.saturating_sub(2).max(1))
                ),
                Style::default().fg(theme.text_dim),
            )));
        }
    }
}

fn sidebar_tool_row_from_cell(cell: &HistoryCell) -> Option<SidebarToolRow> {
    let HistoryCell::Tool(generic) = cell else {
        return None;
    };
    Some(SidebarToolRow {
        name: friendly_generic_tool_name(&generic.name).to_string(),
        status: generic.status,
        summary: generic_tool_sidebar_summary(generic),
        duration_ms: None,
    })
}

fn failure_summary_with_hint(summary: &str) -> String {
    if summary.trim().is_empty() {
        "执行失败".to_string()
    } else {
        summary.to_string()
    }
}

fn friendly_generic_tool_name(name: &str) -> &str {
    match name {
        "task_shell_start" => "start Bash",
        "task_shell_wait" => "wait Bash",
        "task_shell_write" => "write Bash",
        _ => name,
    }
}

fn generic_tool_sidebar_summary(generic: &GenericToolCell) -> String {
    match generic.name.as_str() {
        "task_shell_start" => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            "background Bash".to_string(),
        ]),
        "task_shell_wait" => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            generic.output_summary.clone().unwrap_or_default(),
        ]),
        _ => compact_join([
            generic.input_summary.clone().unwrap_or_default(),
            generic.output_summary.clone().unwrap_or_default(),
            generic
                .output
                .as_deref()
                .map(summarize_tool_output)
                .unwrap_or_default(),
        ]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolRowOrder {
    OldestFirst,
    NewestFirst,
}

fn editorial_tool_rows(
    rows: Vec<SidebarToolRow>,
    limit: usize,
    order_mode: ToolRowOrder,
) -> Vec<SidebarToolRow> {
    #[derive(Clone)]
    struct Candidate {
        rank: u8,
        order: usize,
        row: SidebarToolRow,
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut low_value_groups: Vec<(usize, SidebarToolRow, usize)> = Vec::new();
    let mut ci_poll_groups: Vec<(usize, SidebarToolRow, usize)> = Vec::new();
    let mut shell_wait_groups: Vec<(usize, SidebarToolRow, usize, String)> = Vec::new();
    let mut seen_success: Vec<String> = Vec::new();
    let mut seen_success_tool_names: Vec<String> = Vec::new();
    let mut seen_failures: Vec<String> = Vec::new();
    let mut visible_failure_count: usize = 0;
    const MAX_VISIBLE_FAILURES: usize = 2;

    for (order, mut row) in rows.into_iter().enumerate() {
        if row.status == ToolStatus::Failed {
            // Deduplicate failures for the same tool name: keep only the most
            // recent failure per tool. Fixes #1884 — stale failures from
            // tools that have since succeeded no longer crowd the sidebar.
            let fail_key = row.name.trim().to_ascii_lowercase();
            if order_mode == ToolRowOrder::NewestFirst
                && seen_success_tool_names.contains(&fail_key)
            {
                continue;
            }
            if seen_failures.contains(&fail_key) {
                continue;
            }
            seen_failures.push(fail_key);
            row.summary = failure_summary_with_hint(&row.summary);
        }

        if is_ci_poll_row(&row) {
            if let Some((_, grouped, count)) = ci_poll_groups
                .iter_mut()
                .find(|(_, grouped, _)| grouped.name == row.name)
            {
                *count += 1;
                if grouped.duration_ms.is_none() {
                    grouped.duration_ms = row.duration_ms;
                }
            } else {
                ci_poll_groups.push((order, row, 1));
            }
            continue;
        }

        if is_shell_wait_poll_row(&row) {
            let key = shell_wait_poll_key(&row);
            if let Some((_, grouped, count, _)) = shell_wait_groups
                .iter_mut()
                .find(|(_, _, _, existing_key)| existing_key == &key)
            {
                *count += 1;
                if !row.summary.trim().is_empty() {
                    grouped.summary = row.summary;
                }
            } else {
                shell_wait_groups.push((order, row, 1, key));
            }
            continue;
        }

        if is_low_value_tool(&row.name) && row.status == ToolStatus::Success {
            if let Some((_, grouped, count)) = low_value_groups
                .iter_mut()
                .find(|(_, grouped, _)| grouped.name == row.name)
            {
                *count += 1;
                if grouped.summary.trim().is_empty() && !row.summary.trim().is_empty() {
                    grouped.summary = row.summary;
                }
            } else {
                low_value_groups.push((order, row, 1));
            }
            continue;
        }

        let key = sidebar_row_identity(&row);
        if row.status == ToolStatus::Success && seen_success.iter().any(|seen| seen == &key) {
            continue;
        }
        if row.status == ToolStatus::Success {
            seen_success.push(key);
            let normalized = row.name.trim().to_ascii_lowercase();
            if !seen_success_tool_names.contains(&normalized) {
                seen_success_tool_names.push(normalized.clone());
            }

            // Active rows are oldest-first, so a success means any candidate
            // failure for the same tool is stale. Recent history rows are
            // newest-first; in that path the success is older than any
            // already-seen failure and must not remove it.
            if order_mode == ToolRowOrder::OldestFirst {
                let mut removed_visible_failures = 0usize;
                let mut removed_any_failure = false;
                candidates.retain(|c| {
                    let remove = c.row.status == ToolStatus::Failed
                        && c.row.name.trim().eq_ignore_ascii_case(&normalized);
                    if remove {
                        removed_any_failure = true;
                        if c.rank == 0 {
                            removed_visible_failures += 1;
                        }
                    }
                    !remove
                });
                if removed_any_failure {
                    seen_failures.retain(|seen| seen != &normalized);
                    visible_failure_count =
                        visible_failure_count.saturating_sub(removed_visible_failures);
                }
            }
        }

        // Cap visible failures at MAX_VISIBLE_FAILURES. Excess failures
        // get demoted to rank 3 so they don't crowd the top of the
        // sidebar. (#1884)
        let rank = if row.status == ToolStatus::Failed {
            if visible_failure_count >= MAX_VISIBLE_FAILURES {
                3
            } else {
                visible_failure_count += 1;
                0
            }
        } else {
            tool_row_rank(&row)
        };

        candidates.push(Candidate { rank, order, row });
    }

    for (order, mut row, count) in ci_poll_groups {
        if count > 1 {
            let command = row.name.clone();
            row.name = "等待 CI".to_string();
            row.summary = format!("{command} \u{00B7} 已合并 {count} 次轮询");
            row.status = ToolStatus::Running;
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row),
            order,
            row,
        });
    }

    for (order, mut row, count, key) in shell_wait_groups {
        if count > 1 {
            row.summary = compact_join([
                format!("{key} \u{00B7} {count} waits collapsed"),
                row.summary.clone(),
            ]);
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row),
            order,
            row,
        });
    }

    for (order, mut row, count) in low_value_groups {
        if count > 1 {
            row.name = format!("{} x{count}", row.name);
            if !row.summary.trim().is_empty() {
                row.summary = format!("latest: {}", row.summary);
            }
        }
        candidates.push(Candidate {
            rank: tool_row_rank(&row).saturating_add(1),
            order,
            row,
        });
    }

    candidates.sort_by_key(|candidate| (candidate.rank, candidate.order));
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.row)
        .collect()
}

fn sidebar_row_identity(row: &SidebarToolRow) -> String {
    format!(
        "{}\n{}",
        row.name.trim(),
        normalize_activity_text(row.summary.as_str())
    )
}

fn is_ci_poll_row(row: &SidebarToolRow) -> bool {
    row.name.starts_with("gh pr checks") || row.name.starts_with("gh run watch")
}

fn is_shell_wait_poll_row(row: &SidebarToolRow) -> bool {
    row.status == ToolStatus::Running
        && matches!(row.name.as_str(), "wait Bash" | "exec_shell_wait")
}

fn shell_wait_poll_key(row: &SidebarToolRow) -> String {
    const MARKER: &str = "task_id:";
    if let Some((_, rest)) = row.summary.split_once(MARKER) {
        let task_id = rest
            .trim_start()
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '\u{00B7}')
            .next()
            .unwrap_or_default()
            .trim();
        if !task_id.is_empty() {
            return task_id.to_string();
        }
    }

    normalize_activity_text(&row.name)
}

fn normalize_activity_text(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    crate::tui::osc8::strip_ansi_into(text, &mut cleaned);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tool_row_rank(row: &SidebarToolRow) -> u8 {
    match row.status {
        ToolStatus::Failed => 0,
        // A schema-hydrated deferred tool is not "run done" — it must be
        // retried — so it ranks with active work, not completed successes.
        ToolStatus::Running | ToolStatus::Hydrated => 1,
        ToolStatus::Success if is_low_value_tool(&row.name) => 3,
        ToolStatus::Success => 2,
    }
}

fn is_low_value_tool(name: &str) -> bool {
    let base = name.split_whitespace().next().unwrap_or(name);
    matches!(base, "read_file" | "grep_files" | "file_search" | "find")
}

fn compact_join(parts: impl IntoIterator<Item = String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|seen| seen == part) {
            out.push(part.to_string());
        }
    }
    out.join(" · ")
}

fn tool_status_marker(
    status: ToolStatus,
    theme: &palette::UiTheme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        ToolStatus::Running => ("[~]", theme.warning),
        ToolStatus::Success => ("[✓]", theme.success),
        ToolStatus::Hydrated => ("[~]", theme.warning),
        ToolStatus::Failed => ("[!]", theme.error_fg),
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let mut role_counts = std::collections::BTreeMap::new();
    for child in app.child_agents.rows() {
        let role = if child.depth > 1 {
            "子级 Agent"
        } else {
            "子 Agent"
        };
        *role_counts.entry(role.to_string()).or_insert(0) += 1;
    }

    let summary = SidebarSubagentSummary {
        cached_total: app.child_agents.rows().len(),
        cached_running: app.child_agents.active_count(),
        role_counts,
        ..SidebarSubagentSummary::default()
    };
    let rows = sidebar_agent_rows(app);
    let (lines, row_actions) = subagent_panel_rows(
        &summary,
        &rows,
        content_width,
        usable_rows.max(1),
        &app.ui_theme,
    );
    let full_texts = subagent_panel_hover_texts(&summary, &rows, usable_rows.max(1));

    render_sidebar_section(f, area, "Agents", lines, full_texts, row_actions, app);
}

/// Minimal projection of the data the sub-agent sidebar needs. Lifted out
/// of `render_sidebar_subagents` so the rendering can be snapshot-tested
/// without a full `App`.
#[derive(Debug, Clone, Default)]
pub struct SidebarSubagentSummary {
    pub cached_total: usize,
    pub cached_running: usize,
    pub progress_only_count: usize,
    pub fanout_total: Option<usize>,
    pub fanout_running: usize,
    pub role_counts: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
pub struct SidebarAgentRow {
    pub id: String,
    pub parent_run_id: Option<String>,
    pub spawn_depth: u32,
    pub name: String,
    pub role: String,
    pub model: Option<String>,
    pub status: String,
    pub objective: Option<String>,
    pub git_branch: Option<String>,
    pub progress: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: Option<u64>,
    pub expanded: bool,
}

fn sidebar_agent_rows(app: &App) -> Vec<SidebarAgentRow> {
    app.child_agents
        .rows()
        .iter()
        .enumerate()
        .map(|(index, child)| SidebarAgentRow {
            id: child.child_run_id.0.clone(),
            parent_run_id: Some(child.parent_run_id.0.clone()),
            spawn_depth: u32::from(child.depth),
            name: format!("子 Agent {}", index + 1),
            role: if child.depth > 1 {
                "子级 Agent".to_string()
            } else {
                "子 Agent".to_string()
            },
            model: None,
            status: canonical_child_status(child.terminal.as_ref()).to_string(),
            objective: None,
            git_branch: None,
            progress: child.handoff_content.clone(),
            steps_taken: 0,
            duration_ms: None,
            expanded: false,
        })
        .collect()
}

fn sort_sidebar_agent_rows_as_tree(rows: Vec<SidebarAgentRow>) -> Vec<SidebarAgentRow> {
    let known_ids: std::collections::HashSet<String> =
        rows.iter().map(|row| row.id.clone()).collect();
    let mut children: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    let mut roots = Vec::new();

    for (idx, row) in rows.iter().enumerate() {
        if let Some(parent) = row.parent_run_id.as_deref()
            && known_ids.contains(parent)
        {
            children.entry(parent.to_string()).or_default().push(idx);
            continue;
        }
        roots.push(idx);
    }

    fn push_tree(
        idx: usize,
        rows: &[SidebarAgentRow],
        children: &std::collections::HashMap<String, Vec<usize>>,
        seen: &mut std::collections::HashSet<usize>,
        order: &mut Vec<usize>,
    ) {
        if !seen.insert(idx) {
            return;
        }
        order.push(idx);
        if let Some(child_indices) = children.get(&rows[idx].id) {
            for child_idx in child_indices {
                push_tree(*child_idx, rows, children, seen, order);
            }
        }
    }

    let mut order = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for idx in roots {
        push_tree(idx, &rows, &children, &mut seen, &mut order);
    }
    for idx in 0..rows.len() {
        push_tree(idx, &rows, &children, &mut seen, &mut order);
    }

    // Materialize by move instead of cloning each row a second time (#3898):
    // `seen` guarantees every index lands in `order` exactly once, so each
    // slot is taken exactly once and no row is dropped.
    let mut slots: Vec<Option<SidebarAgentRow>> = rows.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|idx| slots[idx].take().expect("each row emitted exactly once"))
        .collect()
}

fn canonical_child_status(terminal: Option<&TerminalState>) -> &'static str {
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

/// Build sub-agent sidebar lines from summary + per-agent rows. Public
/// for the snapshot tests in this module.
#[cfg(test)]
pub fn subagent_panel_lines(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) -> Vec<Line<'static>> {
    subagent_panel_rows(summary, rows, content_width, max_rows, theme).0
}

/// Render an indented sidebar detail line that never exceeds `content_width`
/// display cells, counting the indent itself (#4094). The earlier inline
/// `format!("  {}", truncate(.., width - 2))` overflowed by the indent width at
/// very narrow terminals (`content_width < 3`, where `saturating_sub(2).max(1)`
/// still leaves room for a glyph that the 2-space prefix then pushes past the
/// column). This keeps the whole line — indent included — within the column.
fn indented_detail_line(indent: &str, body: &str, content_width: usize) -> String {
    let indent_width = unicode_width::UnicodeWidthStr::width(indent);
    if content_width <= indent_width {
        // No room for the indent; clip the body to the whole column so we never
        // overflow, even if that means dropping the indent at pathological widths.
        return truncate_line_to_width(body, content_width);
    }
    format!(
        "{indent}{}",
        truncate_line_to_width(body, content_width - indent_width)
    )
}

/// #4094: reference to a worker's transcript projection, surfaced as a
/// `handle_read` handle instead of dumping the (possibly huge) transcript
/// inline — the inline dump is the freeze/emptiness risk this issue tracks.
/// The child transcript is addressable as the `agent:<id>/full_transcript` var
/// handle (see `subagent_session_projection`); its JSON names the private
/// complete artifact, while clicking Open loads that artifact directly.
///
/// Returns `None` for workers that have not produced anything inspectable yet,
/// so an empty transcript is never advertised. This is the one place a raw
/// agent id is intentionally surfaced in the detail panel (cf. #3030): here it
/// is a functional, copyable handle on its own dedicated line, not incidental
/// id noise mixed into the dossier.
fn subagent_output_handle(row: &SidebarAgentRow) -> Option<String> {
    let has_output = sidebar_agent_status_is_terminal(row.status.as_str()) || row.steps_taken > 0;
    if !has_output {
        return None;
    }
    Some(format!("agent:{}/full_transcript", row.id))
}

/// Build the Agents panel lines together with a parallel per-line
/// click-action vector (#3028). Agent label rows open the Fleet worker status
/// view via `/fleet status`; header, role-mix, and detail lines are not
/// clickable.
fn subagent_panel_rows(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) -> (Vec<Line<'static>>, Vec<Option<SidebarRowAction>>) {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let mut actions: Vec<Option<SidebarRowAction>> = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0 && summary.progress_only_count == 0 && fanout_total == 0 {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(theme.text_muted),
        )));
        actions.push(None);
        return (lines, actions);
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    let header = if live_running > 0 {
        vec![
            Span::styled(
                format!("{live_running} running"),
                Style::default().fg(theme.accent_primary).bold(),
            ),
            Span::styled(format!(" / {total}"), Style::default().fg(theme.text_muted)),
        ]
    } else {
        vec![Span::styled(
            format!("{done} done"),
            Style::default().fg(theme.success),
        )]
    };
    // #4094: the running/done status is the single most useful line, so it must
    // never overflow the sidebar at narrow widths. When the two-tone header
    // fits it renders as-is; when the column is too narrow it collapses into one
    // truncated span so the status is clipped, never spilled past the column.
    let header_width: usize = header
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if header_width > content_width.max(1) {
        let flat: String = header.iter().map(|span| span.content.as_ref()).collect();
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&flat, content_width.max(1)),
            Style::default().fg(theme.text_muted),
        )));
    } else {
        lines.push(Line::from(header));
    }
    actions.push(None);

    if !summary.role_counts.is_empty() {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        let role_line = mix.join(" \u{00B7} ");
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(&role_line, content_width.max(1)),
            Style::default().fg(theme.text_dim),
        )));
        actions.push(None);
    }

    for row in rows {
        if lines.len() >= max_rows {
            break;
        }
        let (marker, color) = agent_status_marker(row.status.as_str(), theme);
        let tree_prefix = agent_tree_prefix(row);
        let label = format!(
            "{tree_prefix}{marker} {}",
            sidebar_agent_row_label(row, content_width.max(1))
        );
        let label = truncate_line_to_width(&label, content_width.max(1));
        lines.push(Line::from(Span::styled(label, Style::default().fg(color))));
        actions.push(Some(SidebarRowAction::InspectText {
            label: row.name.clone(),
            detail: agent_row_hover_text(row),
        }));

        // Auto-collapse finished sub-agents so the sidebar stays compact when
        // work is done or terminally stopped.
        if sidebar_agent_status_is_terminal(row.status.as_str()) && !row.expanded {
            continue;
        }

        if !row.expanded {
            continue;
        }

        if lines.len() >= max_rows {
            break;
        }
        // Expanded detail: a compact but never-empty dossier for the worker
        // (#4094). Status is always shown first so the expanded panel is never
        // blank while a worker is active; objective/elapsed/model/steps/
        // progress/branch follow when known. Raw ids stay out of the compact
        // line (#3030) — the full id remains available in the hover text.
        let mut detail_parts = Vec::new();
        detail_parts.push(row.status.clone());
        if let Some(objective) = row.objective.as_deref()
            && !objective.trim().is_empty()
        {
            detail_parts.push(summarize_tool_output(objective));
        }
        if let Some(model) = row.model.as_deref() {
            detail_parts.push(format!("model {model}"));
        }
        if let Some(duration) = row.duration_ms {
            detail_parts.push(format_duration_ms(duration));
        }
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(summarize_tool_output(progress));
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        lines.push(Line::from(Span::styled(
            indented_detail_line("  ", &detail_parts.join(" \u{00B7} "), content_width.max(1)),
            Style::default().fg(theme.text_dim),
        )));
        // Clicking the expanded dossier drills into the child's transcript
        // card in the detail pager (#2889 slice, dogfood A3). The label row
        // above keeps its expand/collapse toggle.
        actions.push(Some(SidebarRowAction::InspectText {
            label: row.name.clone(),
            detail: agent_row_hover_text(row),
        }));

        // #4094: hand the user a copyable bounded projection instead of
        // dumping the transcript inline — the inline dump is this issue's
        // freeze/emptiness risk. Clicking the row opens the complete private
        // artifact; handle_read exposes bounded slices and its artifact path.
        // Guarded by `max_rows` so the panel stays bounded, and width-clamped so
        // narrow terminals never overflow.
        if let Some(handle) = subagent_output_handle(row) {
            if lines.len() >= max_rows {
                break;
            }
            lines.push(Line::from(Span::styled(
                indented_detail_line(
                    "  ",
                    &format!("\u{25B8} complete chat: open \u{00B7} handle_read {handle}"),
                    content_width.max(1),
                ),
                Style::default().fg(theme.text_muted),
            )));
            actions.push(Some(SidebarRowAction::InspectText {
                label: row.name.clone(),
                detail: agent_row_hover_text(row),
            }));
        }
    }

    debug_assert_eq!(lines.len(), actions.len());
    (lines, actions)
}

fn agent_tree_prefix(row: &SidebarAgentRow) -> String {
    if row.parent_run_id.is_none() && row.spawn_depth <= 1 {
        return String::new();
    }
    let depth = row.spawn_depth.max(2).saturating_sub(2).min(6);
    format!("{}└─ ", "  ".repeat(depth as usize))
}

fn sidebar_agent_status_is_terminal(status: &str) -> bool {
    matches!(
        status,
        "done" | "blocked" | "canceled" | "failed" | "interrupted" | "recovery" | "budget"
    )
}

fn sidebar_agent_status_is_running(status: &str) -> bool {
    matches!(
        status,
        "running" | "queued" | "starting" | "waiting" | "model wait" | "tool"
    )
}

fn sidebar_agent_row_label(row: &SidebarAgentRow, max_width: usize) -> String {
    let detail = row
        .objective
        .as_deref()
        .filter(|objective| !objective.trim().is_empty())
        .map(summarize_tool_output)
        .or_else(|| {
            // Progress is only a live substitute for a missing objective;
            // terminal rows would resurface stale in-flight detail.
            if sidebar_agent_status_is_terminal(row.status.as_str()) {
                return None;
            }
            row.progress
                .as_deref()
                .filter(|progress| !progress.trim().is_empty())
                .map(summarize_tool_output)
        });
    match detail {
        Some(detail) => truncate_line_to_width(&format!("{} — {}", row.name, detail), max_width),
        None => truncate_line_to_width(&row.name, max_width),
    }
}

fn subagent_panel_hover_texts(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    max_rows: usize,
) -> Vec<String> {
    let mut texts = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0 && summary.progress_only_count == 0 && fanout_total == 0 {
        texts.push("No agents".to_string());
        return texts;
    }

    let (live_running, total) = if let Some(total) = summary.fanout_total {
        (summary.fanout_running, total)
    } else {
        (
            summary.cached_running + summary.progress_only_count,
            summary.cached_total + summary.progress_only_count,
        )
    };
    let done = total.saturating_sub(live_running);
    if live_running > 0 {
        texts.push(format!("{live_running} running / {total}"));
    } else {
        texts.push(format!("{done} done"));
    }

    if !summary.role_counts.is_empty() && texts.len() < max_rows {
        let mix: Vec<String> = summary
            .role_counts
            .iter()
            .map(|(role, count)| format!("{count} {role}"))
            .collect();
        texts.push(mix.join(" · "));
    }

    for row in rows {
        if texts.len() >= max_rows {
            break;
        }
        // The compact label row truncates aggressively, so its hover text
        // carries the full agent dossier: id, role, status, elapsed,
        // objective, branch, and untruncated progress (#3063).
        texts.push(agent_row_hover_text(row));

        if row.status == "done" {
            continue;
        }

        if texts.len() >= max_rows {
            break;
        }
        let mut detail_parts = Vec::new();
        detail_parts.push(row.id.clone());
        if row.steps_taken > 0 {
            detail_parts.push(format!("{} step(s)", row.steps_taken));
        }
        if let Some(progress) = row.progress.as_deref()
            && !progress.trim().is_empty()
        {
            detail_parts.push(progress.trim().to_string());
        }
        if let Some(branch) = row.git_branch.as_deref() {
            detail_parts.push(format!("branch {branch}"));
        }
        if let Some(duration) = row.duration_ms {
            detail_parts.push(format_duration_ms(duration));
        }
        texts.push(format!("  {}", detail_parts.join(" · ")));
    }

    texts
}

/// Full hover dossier for one Agents-panel label row (#3063). The compact
/// row only shows `marker role name`, so hovering reveals everything else
/// without spamming raw ids into the normal view.
fn agent_row_hover_text(row: &SidebarAgentRow) -> String {
    let (marker, _) = agent_status_marker(row.status.as_str(), &palette::UI_THEME);
    let mut text = format!(
        "{}{} {} {}",
        agent_tree_prefix(row),
        marker,
        row.role,
        row.name
    );
    let _ = write!(text, "\nid: {}", row.id);
    if let Some(parent) = row.parent_run_id.as_deref() {
        let _ = write!(text, "\nparent: {parent}");
    }
    if row.spawn_depth > 0 {
        let _ = write!(text, "\ndepth: {}", row.spawn_depth);
    }
    if let Some(model) = row.model.as_deref() {
        let _ = write!(text, "\nmodel: {model}");
    }
    let mut status_line = format!("status: {}", row.status);
    if let Some(duration) = row.duration_ms {
        let _ = write!(status_line, " · elapsed {}", format_duration_ms(duration));
    }
    if row.steps_taken > 0 {
        let _ = write!(status_line, " · {} step(s)", row.steps_taken);
    }
    let _ = write!(text, "\n{status_line}");
    if let Some(objective) = row.objective.as_deref() {
        let _ = write!(text, "\nobjective: {}", objective.trim());
    }
    if let Some(branch) = row.git_branch.as_deref() {
        let _ = write!(text, "\nbranch: {branch}");
    }
    if let Some(progress) = row.progress.as_deref()
        && !progress.trim().is_empty()
    {
        let _ = write!(text, "\nprogress: {}", progress.trim());
    }
    text
}

fn agent_status_marker(
    status: &str,
    theme: &palette::UiTheme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        "running" => ("[~]", theme.warning),
        "done" => ("[✓]", theme.success),
        "blocked" | "recovery" => ("[!]", theme.warning),
        "failed" => ("[!]", theme.error_fg),
        "canceled" | "interrupted" => ("[-]", theme.text_muted),
        _ => ("[ ]", theme.text_muted),
    }
}

/// Session-context panel (#504) — consolidated session state overview.
///
/// Surfaces at-a-glance: working set, token usage / context %, running
/// cost, MCP server count, cycle count, and memory
/// file size + mtime. Each section is a compact one-liner so the panel
/// reads as a dashboard rather than a scrolling list.
fn render_context_panel(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let theme = &app.ui_theme;
    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(usize::from(area.height).max(4));

    // ── Working set ──────────────────────────────────────────────
    let ws_name = app
        .workspace
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("(root)")
        .to_string();
    let workspace_label = format!("{}：{ws_name}", app.tr(MessageId::FooterWorkspacePrefix));
    lines.push(Line::from(Span::styled(
        truncate_line_to_width(&workspace_label, content_width.max(1)),
        Style::default().fg(theme.accent_primary).bold(),
    )));

    // ── Token usage ──────────────────────────────────────────────
    // Context % is disclosed in the header; the sidebar keeps the raw token
    // counts for at-a-glance reference without duplicating the bar.
    let total_tokens = app.session.total_conversation_tokens;
    let window = crate::route_budget::route_context_window_tokens(
        app.api_provider,
        app.effective_model_for_budget(),
        app.active_route_limits,
    );
    lines.push(Line::from(Span::styled(
        format!("context: {total_tokens}/{window} tokens"),
        Style::default().fg(theme.text_muted),
    )));

    // ── Session cost ─────────────────────────────────────────────
    let cost_line = context_panel_cost_line(app);
    lines.push(Line::from(Span::styled(
        cost_line,
        Style::default().fg(theme.text_muted),
    )));

    // ── MCP servers ──────────────────────────────────────────────
    if app.mcp_configured_count > 0 {
        lines.push(Line::from(Span::styled(
            format!("mcp: {} server(s)", app.mcp_configured_count),
            Style::default().fg(theme.text_muted),
        )));
    }

    // ── Memory ───────────────────────────────────────────────────
    if app.use_memory {
        let size_hint = std::fs::metadata(&app.memory_path)
            .map(|m| m.len())
            .map(|bytes| {
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else if bytes >= 1024 {
                    format!("{:.1} KB", bytes as f64 / 1024.0)
                } else {
                    format!("{bytes} B")
                }
            })
            .unwrap_or_else(|_| "—".to_string());
        lines.push(Line::from(Span::styled(
            format!("memory: {} ({})", app.memory_path.display(), size_hint),
            Style::default().fg(theme.text_muted),
        )));
    }

    render_sidebar_section(f, area, "Session", lines, Vec::new(), Vec::new(), app);
}

fn context_panel_cost_line(app: &App) -> String {
    let displayed_total = app.displayed_session_cost_for_currency(app.cost_currency);
    let chip = crate::route_billing::usage_chip(
        app.billing_presentation,
        app.api_provider,
        &app.model,
        displayed_total,
        app.cost_display_currency(app.cost_currency),
        None,
    );
    match &chip {
        crate::route_billing::UsageChip::Money(_)
            if crate::route_billing::has_priced_metered_basis(
                app.billing_presentation,
                app.api_provider,
                &app.model,
            ) =>
        {
            let session_cost = app.session_cost_for_currency(app.cost_currency);
            let agent_cost = app.subagent_cost_for_currency(app.cost_currency);
            let real_total = session_cost + agent_cost;
            // Only show the additive breakdown when it matches the displayed
            // total; when the high-water mark is in effect (post-reconciliation),
            // the breakdown would not sum to the displayed value (#244).
            if (displayed_total - real_total).abs() < COST_EQ_TOLERANCE {
                format!(
                    "cost: {} (session {} + agents {})",
                    app.format_cost_amount(displayed_total),
                    app.format_cost_amount(session_cost),
                    app.format_cost_amount(agent_cost)
                )
            } else {
                crate::route_billing::format_usage_line(&chip)
            }
        }
        _ => crate::route_billing::format_usage_line(&chip),
    }
}

fn spans_to_text(spans: &[Span<'_>]) -> String {
    let mut s = String::new();
    for span in spans {
        s.push_str(span.content.as_ref());
    }
    s
}

fn render_sidebar_section(
    f: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    full_texts: Vec<String>,
    row_actions: Vec<Option<SidebarRowAction>>,
    app: &mut App,
) {
    if area.width < 4 || area.height < 3 {
        // Clear stale cells before bailing out (#400).
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(area, f.buffer_mut());
        return;
    }

    let theme = Theme::for_palette_mode(app.ui_theme.mode);

    // Record hover metadata for mouse tooltip support.
    let padding = theme.section_padding;
    let content_area = Rect {
        x: area.x + 1 + padding.left,
        y: area.y + 1 + padding.top,
        width: area.width.saturating_sub(2 + padding.left + padding.right),
        height: area.height.saturating_sub(2 + padding.top + padding.bottom),
    };
    let display_texts: Vec<String> = lines
        .iter()
        .map(|line| spans_to_text(&line.spans))
        .collect();
    let hover_texts: Vec<String> = display_texts
        .iter()
        .enumerate()
        .map(|(idx, display)| {
            full_texts
                .get(idx)
                .filter(|text| !text.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| display.clone())
        })
        .collect();
    let rows = sidebar_hover_rows(content_area, &display_texts, &hover_texts, &row_actions);
    app.sidebar_hover.sections.push(SidebarHoverSection {
        content_area,
        lines: hover_texts,
        rows,
    });
    // Truncate the panel title so it always fits within the section width
    // even after a resize. The title occupies up to 4 chars of border chrome
    // (two spaces + one space on each side), so the max title length is
    // area.width.saturating_sub(4) when borders are enabled.
    let max_title_width = area.width.saturating_sub(4).max(1) as usize;
    let display_title = truncate_line_to_width(title, max_title_width);

    // Constrain lines to the visible section area so a Paragraph wrap
    // overflow can't write cells outside the Block bounds (#400). The
    // border + padding consume 2 rows; budget the rest for content.
    let visible_content_rows = area
        .height
        .saturating_sub(2) // top + bottom border
        .saturating_sub(theme.section_padding.top + theme.section_padding.bottom)
        as usize;
    let lines: Vec<Line<'static>> =
        if lines.len() > visible_content_rows && visible_content_rows > 0 {
            lines.into_iter().take(visible_content_rows).collect()
        } else {
            lines
        };

    let section = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(Line::from(vec![Span::styled(
                format!(" {display_title} "),
                Style::default().fg(theme.section_title_color).bold(),
            )]))
            .borders(theme.section_borders)
            .border_type(theme.section_border_type)
            .border_style(Style::default().fg(theme.section_border_color))
            .style(Style::default().bg(theme.section_bg))
            .padding(theme.section_padding),
    );

    f.render_widget(section, area);
}

fn sidebar_hover_rows(
    content_area: Rect,
    display_texts: &[String],
    hover_texts: &[String],
    row_actions: &[Option<SidebarRowAction>],
) -> Vec<SidebarHoverRow> {
    display_texts
        .iter()
        .zip(hover_texts.iter())
        .enumerate()
        .map(|(idx, (display_text, full_text))| {
            let row_y = content_area.y.saturating_add(idx as u16);
            let display_width = unicode_width::UnicodeWidthStr::width(display_text.as_str());
            let full_width = unicode_width::UnicodeWidthStr::width(full_text.as_str());
            let click_action = row_actions.get(idx).and_then(|a| a.clone());
            SidebarHoverRow {
                row_y,
                display_text: display_text.clone(),
                full_text: full_text.clone(),
                detail: None,
                is_truncated: display_width > content_area.width as usize
                    || full_width > content_area.width as usize
                    || display_text != full_text,
                click_action,
                stop_action: None,
                stop_zone_start_col: None,
                stop_zone_end_col: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_TOOL_COMPLETED_ROW_TTL, AutoSidebarPanel, AutoSidebarState, SidebarAgentRow,
        SidebarFocus, SidebarHoverRow, SidebarHoverSection, SidebarHoverState,
        SidebarSubagentSummary, SidebarToolRow, ToolRowOrder, agent_row_hover_text,
        auto_sidebar_panels, context_panel_cost_line, editorial_tool_rows, normalize_activity_text,
        render_sidebar, sidebar_agent_rows, sidebar_hover_rows, sort_sidebar_agent_rows_as_tree,
        subagent_output_handle, subagent_panel_hover_texts, subagent_panel_lines,
        subagent_panel_rows, task_panel_hover_texts, task_panel_lines, task_panel_row_sets,
        task_panel_rows,
    };
    use crate::config::Config;
    use crate::palette;
    use crate::tui::active_cell::ActiveCell;
    use crate::tui::app::{App, SidebarRowAction, TuiOptions};
    use crate::tui::history::{GenericToolCell, HistoryCell, ToolStatus};
    use ratatui::{Terminal, backend::TestBackend, text::Line};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("."),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        App::new(options, &Config::default())
    }

    fn start_child(app: &mut App, parent: &str, child: &str, depth: u8) {
        use codewhale_protocol::agent_runtime::RunId;

        app.child_agents.begin_root(RunId("root-run".to_string()));
        app.child_agents.record_started(
            RunId(parent.to_string()),
            format!("call-{child}"),
            RunId(child.to_string()),
            depth,
        );
    }

    fn sidebar_tool_row(name: &str, status: ToolStatus) -> SidebarToolRow {
        SidebarToolRow {
            name: name.to_string(),
            status,
            summary: String::new(),
            duration_ms: None,
        }
    }

    fn lines_to_text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn context_panel_cost_line_shows_na_for_unpriced_zero_cost_model() {
        let mut app = create_test_app();
        app.model = "unknown-provider/unknown-model".to_string();
        app.billing_presentation = crate::route_billing::BillingPresentation::Metered;

        assert_eq!(context_panel_cost_line(&app), "cost: unknown");
    }

    #[test]
    fn context_panel_cost_line_does_not_inherit_api_pricing_for_codex_oauth() {
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::OpenaiCodex;
        app.model = "gpt-5.5".to_string();
        app.billing_presentation =
            crate::route_billing::BillingPresentation::Subscription("Codex OAuth quota");
        app.accrue_session_cost_estimate(crate::pricing::CostEstimate::usd_only(12.34));

        let line = context_panel_cost_line(&app);
        assert_eq!(line, "usage: Codex OAuth quota");
        assert!(!line.contains('$'), "OAuth must not invent dollars: {line}");
    }

    #[test]
    fn context_panel_cost_line_marks_unpriced_metered_as_unknown() {
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::NvidiaNim;
        app.model = "deepseek-ai/deepseek-v4-pro".to_string();
        app.billing_presentation = crate::route_billing::BillingPresentation::Metered;

        assert_eq!(context_panel_cost_line(&app), "cost: unknown");
    }

    #[test]
    fn context_panel_cost_line_uses_usd_for_usd_only_model_in_cny_mode() {
        let mut app = create_test_app();
        app.model = "kimi-k2.6".to_string();
        // This test is about METERED currency rendering; pin the route class
        // and a metered provider so the session default (which may be a
        // subscription/OAuth route with no API pricing basis) cannot change
        // what is under test (TUI-DOG-010).
        app.api_provider = crate::config::ApiProvider::Moonshot;
        app.billing_presentation = crate::route_billing::BillingPresentation::Metered;
        app.cost_currency = crate::pricing::CostCurrency::Cny;
        app.accrue_session_cost_estimate(crate::pricing::CostEstimate::usd_only(0.42));

        let line = context_panel_cost_line(&app);

        assert!(line.contains("$0.42"), "expected USD amount, got {line:?}");
        assert!(
            !line.contains('¥'),
            "must not render CNY zero, got {line:?}"
        );
    }

    #[test]
    fn editorial_rows_keep_newer_failure_when_older_success_is_seen_later() {
        let rows = vec![
            sidebar_tool_row("gh issue create", ToolStatus::Failed),
            sidebar_tool_row("gh issue create", ToolStatus::Success),
        ];

        let rendered = editorial_tool_rows(rows, 4, ToolRowOrder::NewestFirst);

        assert!(
            rendered
                .iter()
                .any(|row| row.name == "gh issue create" && row.status == ToolStatus::Failed),
            "newest-first rows must keep a failure newer than a later-seen success: {rendered:?}"
        );
    }

    #[test]
    fn normalize_activity_text_strips_ansi_before_collapsing_text() {
        let text = normalize_activity_text("running \x1b[48;2;10;17;32mtool\x1b[0m now");
        assert_eq!(text, "running tool now");
        assert!(!text.contains("48;2"));
    }

    #[test]
    fn editorial_rows_hide_older_failure_after_newer_success() {
        let rows = vec![
            sidebar_tool_row("gh issue create", ToolStatus::Success),
            sidebar_tool_row("gh issue create", ToolStatus::Failed),
        ];

        let rendered = editorial_tool_rows(rows, 4, ToolRowOrder::NewestFirst);

        assert!(
            !rendered
                .iter()
                .any(|row| row.name == "gh issue create" && row.status == ToolStatus::Failed),
            "newest-first rows should hide stale failures older than success: {rendered:?}"
        );
    }

    #[test]
    fn editorial_rows_reclaim_failure_slot_after_oldest_first_success() {
        let rows = vec![
            sidebar_tool_row("gh issue create", ToolStatus::Failed),
            sidebar_tool_row("grep_files", ToolStatus::Failed),
            sidebar_tool_row("gh issue create", ToolStatus::Success),
            sidebar_tool_row("cargo test", ToolStatus::Failed),
        ];

        let rendered = editorial_tool_rows(rows, 2, ToolRowOrder::OldestFirst);

        assert_eq!(
            rendered
                .iter()
                .filter(|row| row.status == ToolStatus::Failed)
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            vec!["grep_files", "cargo test"],
            "success should clear its stale failure and free a visible failure slot"
        );
    }

    #[test]
    fn auto_sidebar_shows_canonical_agents_when_child_work_is_active() {
        let panels = auto_sidebar_panels(AutoSidebarState {
            agents_empty: false,
            context_enabled: false,
        });

        assert_eq!(panels, vec![AutoSidebarPanel::Agents]);
    }

    #[test]
    fn auto_sidebar_returns_no_panels_when_idle() {
        let panels = auto_sidebar_panels(AutoSidebarState {
            agents_empty: true,
            context_enabled: false,
        });

        assert!(panels.is_empty());
    }

    #[test]
    fn pinned_sidebar_renders_agents_section_when_subagents_are_active() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Pinned;
        start_child(&mut app, "root-run", "child-active-1", 1);

        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render_sidebar(frame, frame.area(), &mut app))
            .expect("draw sidebar");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(
            rendered.contains("Agents"),
            "pinned sidebar must surface active sub-agents: {rendered:?}"
        );
        assert!(
            rendered.contains('子') && rendered.contains("Agent 1"),
            "pinned sidebar should render the child agent label: {rendered:?}"
        );
    }

    #[test]
    fn tasks_panel_renders_active_tool_rows_before_background_empty_state() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        active.push_tool(
            "tool-1",
            HistoryCell::Tool(GenericToolCell {
                name: "agent".to_string(),
                status: ToolStatus::Running,
                input_summary: Some("agent_id: agent_af58ba3a".to_string()),
                output: None,
                prompts: None,
                output_summary: None,
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);
        app.runtime_turn_id = Some("turn_abcdef123456".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));

        assert!(text[0].contains("turn "));
        assert!(text[0].contains("in_progress"));
        assert!(
            text.iter().any(|line| line == "Live tools"),
            "live section missing: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("[~] agent")),
            "active agent row missing: {text:?}"
        );
        assert!(
            !text.iter().any(|line| line.contains("No active tasks")),
            "old empty state should not render during active tools: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_renders_recent_completed_tool_rows() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.history.push(HistoryCell::Tool(GenericToolCell {
            name: "read_file".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("codewhale-tui/CHANGELOG.md".to_string()),
            output: Some("done".to_string()),
            prompts: None,
            output_summary: Some("Reading CHANGELOG.md".to_string()),
            is_diff: false,
        }));

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));

        assert!(
            text.iter().any(|line| line == "Recent tools"),
            "recent section missing: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("[✓] read_file")),
            "recent read_file row missing: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_expires_completed_active_tool_rows() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        active.push_tool(
            "tool-1",
            HistoryCell::Tool(GenericToolCell {
                name: "read_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("src/main.rs".to_string()),
                output: Some("done".to_string()),
                prompts: None,
                output_summary: Some("done".to_string()),
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);
        let expired_at = instant_older_than(ACTIVE_TOOL_COMPLETED_ROW_TTL + Duration::from_secs(1));
        app.active_tool_entry_completed_at.insert(0, expired_at);

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));

        assert!(
            !text.iter().any(|line| line.contains("[✓] read_file")),
            "expired completed active row should leave the sidebar: {text:?}"
        );
    }

    fn instant_older_than(age: Duration) -> Instant {
        if let Some(instant) = Instant::now().checked_sub(age) {
            return instant;
        }

        let instant = Instant::now();
        std::thread::sleep(age);
        instant
    }

    #[test]
    fn tasks_panel_lingers_fresh_completed_active_tool_rows() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        active.push_tool(
            "tool-1",
            HistoryCell::Tool(GenericToolCell {
                name: "read_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("src/main.rs".to_string()),
                output: Some("done".to_string()),
                prompts: None,
                output_summary: Some("done".to_string()),
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);
        app.active_tool_entry_completed_at.insert(0, Instant::now());

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));

        assert!(
            text.iter().any(|line| line.contains("[✓] read_file")),
            "fresh completed active row should linger briefly: {text:?}"
        );
    }

    #[test]
    fn task_panel_rows_and_hover_share_one_snapshot() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("turn_abcdef123456".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());
        app.turn_counter = 3;
        let mut active = ActiveCell::new();
        active.push_tool(
            "tool-1",
            HistoryCell::Tool(GenericToolCell {
                name: "exec_shell".to_string(),
                status: ToolStatus::Running,
                input_summary: Some("cargo test --workspace".to_string()),
                output: None,
                prompts: None,
                output_summary: None,
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);

        let row_sets = task_panel_row_sets(&app);
        let (lines, actions) = task_panel_rows(&app, &row_sets, 80, 12);
        let hover = task_panel_hover_texts(&app, &row_sets, 12);
        let text = lines_to_text(&lines);

        assert_eq!(lines.len(), actions.len(), "actions align with lines");
        let tool_idx = hover
            .iter()
            .position(|line| line.contains("cargo test --workspace"))
            .unwrap_or_else(|| panic!("canonical tool detail missing: {hover:?}"));
        assert!(
            text[tool_idx].contains("cargo test --workspace"),
            "line and hover projections must align at the tool detail row: {text:?} / {hover:?}"
        );
        assert!(
            text[0].starts_with("Turn 3"),
            "line shows stable turn label: {:?}",
            text[0]
        );
        assert!(
            hover[0].contains("turn_abcdef123456"),
            "hover carries the full turn id: {:?}",
            hover[0]
        );
    }

    #[test]
    fn tasks_panel_keeps_model_reasoning_in_transcript_only() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("turn_reasoning".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());
        app.history.push(HistoryCell::Thinking {
            content: "private chain of thought".to_string(),
            streaming: true,
        });

        let text = lines_to_text(&task_panel_lines(&app, 96, 12));

        assert!(
            !text.iter().any(|line| {
                line.contains("Model reasoning")
                    || line.contains("private chain of thought")
                    || line.contains("thinking")
            }),
            "reasoning belongs to transcript detail, not Activity: {text:?}"
        );
    }

    #[test]
    fn activity_actions_align_with_canonical_tool_rows() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("0196f0a3-aaaa-bbbb-cccc-ddddeeee0000".to_string());
        let mut active = ActiveCell::new();
        active.push_tool(
            "shell-1",
            HistoryCell::Tool(GenericToolCell {
                name: "exec_shell".to_string(),
                status: ToolStatus::Running,
                input_summary: Some("sleep 600".to_string()),
                output: None,
                prompts: None,
                output_summary: None,
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);

        let (lines, actions) = task_panel_rows(&app, &task_panel_row_sets(&app), 96, 16);
        let text = lines_to_text(&lines);
        assert_eq!(
            lines.len(),
            actions.len(),
            "actions must stay index-aligned with lines: {text:?}"
        );

        // Turn label and live-tool rows are not clickable.
        assert!(actions[0].is_none(), "turn label row has no action");
        let live_idx = text
            .iter()
            .position(|line| line == "Live tools")
            .expect("live tools header");
        assert!(actions[live_idx].is_none());
        let tool_idx = text
            .iter()
            .position(|line| line.contains("exec_shell"))
            .expect("canonical tool row");
        assert!(actions[tool_idx].is_none());
    }

    #[test]
    fn subagent_panel_actions_mark_agent_rows_with_role_mix_header() {
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("worker".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_0123456789".to_string(),
            model: None,
            parent_run_id: None,
            spawn_depth: 1,
            name: "investigator".to_string(),
            role: "worker".to_string(),
            status: "running".to_string(),
            objective: None,
            git_branch: None,
            progress: Some("scanning".to_string()),
            steps_taken: 2,
            duration_ms: Some(1_000),
            expanded: true,
        }];

        let (lines, actions) = subagent_panel_rows(&summary, &rows, 48, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert_eq!(lines.len(), actions.len());

        assert!(actions[0].is_none(), "count header has no action");
        assert!(actions[1].is_none(), "role-mix header has no action");
        let agent_idx = text
            .iter()
            .position(|line| line.contains("investigator"))
            .expect("agent label row");
        assert!(matches!(
            actions[agent_idx],
            Some(SidebarRowAction::InspectText { ref label, .. })
                if label == "investigator"
        ));
        assert!(
            !text[agent_idx].ends_with("[x]"),
            "canonical child rows do not expose a fake direct stop target: {text:?}"
        );
        assert!(matches!(
            actions[agent_idx + 1],
            Some(SidebarRowAction::InspectText { ref label, .. })
                if label == "investigator"
        ));
    }

    #[test]
    fn subagent_panel_actions_skip_role_mix_slot_for_progress_only_agents() {
        // Progress-only agents have no cached role counts, so there is no
        // role-mix line — the first agent row sits directly under the count
        // header and must still resolve to /fleet status (#3028 audit fix).
        let summary = SidebarSubagentSummary {
            progress_only_count: 1,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_fedcba987654".to_string(),
            model: None,
            parent_run_id: None,
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "explorer".to_string(),
            status: "running".to_string(),
            objective: None,
            git_branch: None,
            progress: Some("reading".to_string()),
            steps_taken: 1,
            duration_ms: None,
            expanded: false,
        }];

        let (lines, actions) = subagent_panel_rows(&summary, &rows, 48, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert_eq!(lines.len(), actions.len());

        assert!(actions[0].is_none(), "count header has no action");
        let agent_idx = text
            .iter()
            .position(|line| line.contains("scout"))
            .expect("agent label row");
        assert_eq!(
            agent_idx, 1,
            "no role-mix line should be emitted without role counts: {text:?}"
        );
        assert!(matches!(
            actions[agent_idx],
            Some(SidebarRowAction::InspectText { ref label, .. })
                if label == "scout"
        ));
    }

    #[test]
    fn subagent_panel_collapses_terminal_non_done_rows() {
        let summary = SidebarSubagentSummary {
            cached_total: 3,
            cached_running: 0,
            ..SidebarSubagentSummary::default()
        };
        let rows = ["canceled", "failed", "interrupted"]
            .into_iter()
            .enumerate()
            .map(|(idx, status)| SidebarAgentRow {
                id: format!("agent_terminal_{idx}"),
                model: None,
                parent_run_id: None,
                spawn_depth: 1,
                name: format!("worker-{idx}"),
                role: "explore".to_string(),
                status: status.to_string(),
                objective: None,
                git_branch: None,
                progress: Some(format!("{status} with a long stale-looking detail")),
                steps_taken: 7,
                duration_ms: Some(1_000),
                expanded: false,
            })
            .collect::<Vec<_>>();

        let (lines, _) = subagent_panel_rows(&summary, &rows, 72, 10, &palette::UI_THEME);
        let text = lines_to_text(&lines);

        assert!(
            text.iter().any(|line| line.contains("3 done")),
            "terminal summary remains visible: {text:?}"
        );
        for idx in 0..3 {
            assert!(
                text.iter()
                    .any(|line| line.contains(&format!("worker-{idx}"))),
                "terminal worker label remains visible: {text:?}"
            );
        }
        assert!(
            !text.iter().any(|line| line.contains("step(s)")),
            "terminal rows should not keep noisy detail lines: {text:?}"
        );
        assert!(
            !text
                .iter()
                .any(|line| line.contains("stale-looking detail")),
            "terminal rows should hide stale progress details: {text:?}"
        );
    }

    #[test]
    fn subagent_panel_cancelled_rows_are_visibly_terminal_and_not_cancelable() {
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 0,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_cancelled".to_string(),
            model: None,
            parent_run_id: None,
            spawn_depth: 1,
            name: "worker-cancelled".to_string(),
            role: "worker".to_string(),
            status: "canceled".to_string(),
            objective: None,
            git_branch: None,
            progress: Some("cancelled by user".to_string()),
            steps_taken: 2,
            duration_ms: Some(2_000),
            expanded: false,
        }];

        let (lines, actions) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        let agent_idx = text
            .iter()
            .position(|line| line.contains("worker-cancelled"))
            .expect("cancelled agent row");

        assert!(
            text[agent_idx].contains("[-]"),
            "cancelled row should render with the terminal marker: {text:?}"
        );
        assert!(
            !text[agent_idx].ends_with("[x]"),
            "cancelled row must not show the inline stop target: {text:?}"
        );
        assert!(matches!(
            actions[agent_idx],
            Some(SidebarRowAction::InspectText { ref label, .. })
                if label == "worker-cancelled"
        ));
        assert!(
            actions
                .iter()
                .flatten()
                .all(|action| !action.is_cancel_action()),
            "terminal agent rows should not expose cancel actions: {actions:?}"
        );
    }

    #[test]
    fn sort_sidebar_agent_rows_as_tree_emits_each_row_exactly_once() {
        let agent_row = |id: &str, parent: Option<&str>| SidebarAgentRow {
            id: id.to_string(),
            parent_run_id: parent.map(str::to_string),
            spawn_depth: 1,
            name: id.to_string(),
            role: "explore".to_string(),
            model: None,
            status: "running".to_string(),
            objective: None,
            git_branch: None,
            progress: None,
            steps_taken: 1,
            duration_ms: None,
            expanded: false,
        };
        // Parent + child + a two-node parent cycle: the cycle has no root, so
        // only the orphan sweep reaches it. Every row must still come out
        // exactly once (the move-based materialization panics on a double
        // take and this pins the drop case too).
        let rows = vec![
            agent_row("agent_parent", None),
            agent_row("agent_child", Some("agent_parent")),
            agent_row("agent_cycle_a", Some("agent_cycle_b")),
            agent_row("agent_cycle_b", Some("agent_cycle_a")),
        ];

        let sorted = sort_sidebar_agent_rows_as_tree(rows);

        assert_eq!(sorted.len(), 4, "no rows dropped or duplicated");
        let mut ids: Vec<&str> = sorted.iter().map(|row| row.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "agent_child",
                "agent_cycle_a",
                "agent_cycle_b",
                "agent_parent"
            ]
        );
        assert_eq!(sorted[0].id, "agent_parent");
        assert_eq!(sorted[1].id, "agent_child");
    }

    #[test]
    fn subagent_sidebar_orders_and_indents_nested_children() {
        let rows = vec![
            SidebarAgentRow {
                id: "agent_grandchild".to_string(),
                model: None,
                parent_run_id: Some("agent_parent".to_string()),
                spawn_depth: 2,
                name: "nested-reader".to_string(),
                role: "explore".to_string(),
                status: "done".to_string(),
                objective: None,
                git_branch: None,
                progress: None,
                steps_taken: 1,
                duration_ms: Some(250),
                expanded: false,
            },
            SidebarAgentRow {
                id: "agent_parent".to_string(),
                model: None,
                parent_run_id: None,
                spawn_depth: 1,
                name: "nested-parent".to_string(),
                role: "explore".to_string(),
                status: "running".to_string(),
                objective: None,
                git_branch: None,
                progress: Some("waiting on child".to_string()),
                steps_taken: 2,
                duration_ms: Some(500),
                expanded: false,
            },
        ];
        let sorted = sort_sidebar_agent_rows_as_tree(rows);
        assert_eq!(sorted[0].id, "agent_parent");
        assert_eq!(sorted[1].id, "agent_grandchild");
        assert_eq!(sorted.len(), 2, "tree sort must not drop or duplicate rows");

        let summary = SidebarSubagentSummary {
            cached_total: 2,
            cached_running: 1,
            ..SidebarSubagentSummary::default()
        };
        let (lines, _) = subagent_panel_rows(&summary, &sorted, 64, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        let parent_idx = text
            .iter()
            .position(|line| line.contains("nested-parent"))
            .expect("parent row");
        let child_idx = text
            .iter()
            .position(|line| line.contains("nested-reader"))
            .expect("child row");
        assert!(
            parent_idx < child_idx,
            "parent must render before child: {text:?}"
        );
        assert!(
            text[child_idx].contains("└─"),
            "nested child should render with a tree branch marker: {text:?}"
        );

        let hover = agent_row_hover_text(&sorted[1]);
        assert!(hover.contains("parent: agent_parent"));
        assert!(hover.contains("depth: 2"));
    }

    #[test]
    fn subagent_sidebar_orders_and_indents_canonical_children() {
        let mut app = create_test_app();
        start_child(&mut app, "root-run", "agent_parent", 1);
        start_child(&mut app, "agent_parent", "agent_child", 2);

        let rows = sidebar_agent_rows(&app);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "agent_parent");
        assert_eq!(rows[1].id, "agent_child");
        assert_eq!(rows[1].parent_run_id.as_deref(), Some("agent_parent"));
        assert_eq!(rows[1].spawn_depth, 2);
        assert_eq!(rows[1].role, "子级 Agent");

        let summary = SidebarSubagentSummary {
            cached_total: 2,
            cached_running: 2,
            ..SidebarSubagentSummary::default()
        };
        let (lines, _) = subagent_panel_rows(&summary, &rows, 64, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        let parent_idx = text
            .iter()
            .position(|line| line.contains("子 Agent 1"))
            .expect("live parent row");
        let child_idx = text
            .iter()
            .position(|line| line.contains("子 Agent 2"))
            .expect("live child row");
        assert!(
            parent_idx < child_idx,
            "live parent must render before child: {text:?}"
        );
        assert!(
            text[child_idx].contains("└─"),
            "live child should render with a tree branch marker: {text:?}"
        );

        let hover = agent_row_hover_text(&rows[1]);
        assert!(hover.contains("parent: agent_parent"));
        assert!(hover.contains("depth: 2"));
    }

    #[test]
    fn tasks_panel_collapses_repeated_low_value_recent_tools_after_failures() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            app.history.push(HistoryCell::Tool(GenericToolCell {
                name: "read_file".to_string(),
                status: ToolStatus::Success,
                input_summary: Some(path.to_string()),
                output: Some("ok".to_string()),
                prompts: None,
                output_summary: None,
                is_diff: false,
            }));
        }
        app.history.push(HistoryCell::Tool(GenericToolCell {
            name: "file_search".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("pattern: src/*.rs".to_string()),
            output: Some("src/main.rs".to_string()),
            prompts: None,
            output_summary: None,
            is_diff: false,
        }));
        app.history.push(HistoryCell::Tool(GenericToolCell {
            name: "grep_files".to_string(),
            status: ToolStatus::Failed,
            input_summary: Some("pattern: canonical projection".to_string()),
            output: Some("regex parse error".to_string()),
            prompts: None,
            output_summary: Some("regex parse error".to_string()),
            is_diff: false,
        }));

        let text = lines_to_text(&task_panel_lines(&app, 80, 12));
        let failed_index = text
            .iter()
            .position(|line| line.contains("[!] grep_files"))
            .expect("failed grep row should stay visible");
        let read_group_index = text
            .iter()
            .position(|line| line.contains("[✓] read_file x3"))
            .expect("repeated read_file rows should collapse");

        assert!(
            failed_index < read_group_index,
            "failure should sort above low-value success noise: {text:?}"
        );
        assert_eq!(
            text.iter()
                .filter(|line| line.contains("[✓] read_file"))
                .count(),
            1,
            "read_file should render once after grouping: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("regex parse error")),
            "failure detail should remain visible: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_failed_shell_rows_keep_the_real_failure_summary() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.history.push(HistoryCell::Tool(GenericToolCell {
            name: "exec_shell".to_string(),
            status: ToolStatus::Failed,
            input_summary: Some("command: cargo test -p codewhale-tui".to_string()),
            output: Some("test failed".to_string()),
            prompts: None,
            output_summary: Some("test failed".to_string()),
            is_diff: false,
        }));

        let text = lines_to_text(&task_panel_lines(&app, 80, 8));

        assert!(
            text.iter().any(|line| line.contains("[!] exec_shell")),
            "failed canonical shell tool should keep its status: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("cargo test")),
            "failed canonical shell tool should keep its command: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("test failed")),
            "failed row should keep the real failure summary: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_uses_plain_names_for_shell_background_helpers() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        active.push_tool(
            "shell-wait",
            HistoryCell::Tool(GenericToolCell {
                name: "task_shell_wait".to_string(),
                status: ToolStatus::Running,
                input_summary: Some("task_id: shell_33a08c3c".to_string()),
                output: None,
                prompts: None,
                output_summary: None,
                is_diff: false,
            }),
        );
        app.active_cell = Some(active);

        let text = lines_to_text(&task_panel_lines(&app, 80, 6));

        assert!(
            text.iter().any(|line| line.contains("[~] wait Bash")),
            "shell helper should render as a user-facing activity: {text:?}"
        );
        assert!(
            !text.iter().any(|line| line.contains("task_shell_wait")),
            "internal helper name should not leak into sidebar: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_collapses_repeated_shell_waits_for_same_job() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        for id in ["shell-wait-1", "shell-wait-2"] {
            active.push_tool(
                id,
                HistoryCell::Tool(GenericToolCell {
                    name: "task_shell_wait".to_string(),
                    status: ToolStatus::Running,
                    input_summary: Some("task_id: shell_33a08c3c".to_string()),
                    output: None,
                    prompts: None,
                    output_summary: Some("Background task running (no new output).".to_string()),
                    is_diff: false,
                }),
            );
        }
        app.active_cell = Some(active);

        let text = lines_to_text(&task_panel_lines(&app, 100, 8));

        assert_eq!(
            text.iter()
                .filter(|line| line.contains("[~] wait Bash"))
                .count(),
            1,
            "duplicate waits for the same shell job should collapse: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("2 waits collapsed")),
            "collapsed row should explain why only one wait is visible: {text:?}"
        );
    }

    #[test]
    fn tasks_panel_collapses_repeated_shell_waits_without_task_marker() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        let mut active = ActiveCell::new();
        for (id, summary) in [
            ("shell-wait-1", "Background task running (no new output)."),
            ("shell-wait-2", "Still running after 10s."),
        ] {
            active.push_tool(
                id,
                HistoryCell::Tool(GenericToolCell {
                    name: "task_shell_wait".to_string(),
                    status: ToolStatus::Running,
                    input_summary: None,
                    output: None,
                    prompts: None,
                    output_summary: Some(summary.to_string()),
                    is_diff: false,
                }),
            );
        }
        app.active_cell = Some(active);

        let text = lines_to_text(&task_panel_lines(&app, 100, 8));

        assert_eq!(
            text.iter()
                .filter(|line| line.contains("[~] wait Bash"))
                .count(),
            1,
            "same wait helper without task markers should still collapse: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("2 waits collapsed")),
            "collapsed no-marker row should show the wait count: {text:?}"
        );
    }

    #[test]
    fn navigator_empty_state_says_no_agents() {
        let summary = SidebarSubagentSummary::default();
        let lines = subagent_panel_lines(&summary, &[], 32, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert_eq!(text, vec!["No agents".to_string()]);
    }

    #[test]
    fn agents_panel_running_state_renders_count_role_and_rows() {
        // Two general agents (one running, one done) + one explore (running).
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("general".to_string(), 2);
        role_counts.insert("explore".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 3,
            cached_running: 2,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let rows = vec![
            SidebarAgentRow {
                id: "agent_a5e674dc".to_string(),
                model: None,
                parent_run_id: None,
                spawn_depth: 1,
                name: "check-docs-mcp".to_string(),
                role: "explore".to_string(),
                status: "running".to_string(),
                objective: None,
                git_branch: Some("feature/docs".to_string()),
                progress: Some("step 2/3: running tool 'read_file'".to_string()),
                steps_taken: 2,
                duration_ms: Some(22_000),
                expanded: true,
            },
            SidebarAgentRow {
                id: "agent_850aa63f".to_string(),
                model: None,
                parent_run_id: None,
                spawn_depth: 1,
                name: "check-install-docs".to_string(),
                role: "general".to_string(),
                status: "done".to_string(),
                objective: None,
                git_branch: None,
                progress: Some("SUMMARY: docs checked".to_string()),
                steps_taken: 5,
                duration_ms: Some(21_000),
                expanded: false,
            },
        ];
        let text = lines_to_text(&subagent_panel_lines(
            &summary,
            &rows,
            64,
            12,
            &palette::UI_THEME,
        ));
        assert!(text[0].contains("2 running"), "header: {:?}", text[0]);
        assert!(text[0].contains("/ 3"), "total in header: {:?}", text[0]);
        assert!(
            text[1].contains("1 explore") && text[1].contains("2 general"),
            "role mix line: {:?}",
            text[1]
        );
        assert!(
            text.iter().any(|l| l.contains("[~] check-docs-mcp")),
            "running row missing: {text:?}",
        );
        assert!(
            text.iter().any(|l| l.contains("step 2/3")),
            "progress detail missing: {text:?}",
        );
        let wide_text = lines_to_text(&subagent_panel_lines(
            &summary,
            &rows,
            96,
            12,
            &palette::UI_THEME,
        ));
        assert!(
            wide_text.iter().any(|l| l.contains("branch feature/docs")),
            "branch detail missing at wide width: {wide_text:?}",
        );
    }

    #[test]
    fn navigator_uses_fanout_total_when_fanout_has_seeded_slots() {
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            progress_only_count: 0,
            fanout_total: Some(6),
            fanout_running: 1,
            role_counts: std::collections::BTreeMap::new(),
        };

        let text = lines_to_text(&subagent_panel_lines(
            &summary,
            &[],
            64,
            8,
            &palette::UI_THEME,
        ));

        assert!(text[0].contains("1 running"), "header: {:?}", text[0]);
        assert!(text[0].contains("/ 6"), "fanout total: {:?}", text[0]);
    }

    #[test]
    fn navigator_settled_state_says_done() {
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("general".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 0,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let text = lines_to_text(&subagent_panel_lines(
            &summary,
            &[],
            32,
            8,
            &palette::UI_THEME,
        ));
        assert!(text[0].contains("1 done"), "settled header: {:?}", text[0]);
    }

    #[test]
    fn navigator_truncates_long_role_mix_to_content_width() {
        // Build a wide role mix; assert it doesn't blow past content_width.
        let mut role_counts = std::collections::BTreeMap::new();
        for role in ["general", "explore", "plan", "review", "custom", "extra"] {
            role_counts.insert(role.to_string(), 1);
        }
        let summary = SidebarSubagentSummary {
            cached_total: 6,
            cached_running: 6,
            progress_only_count: 0,
            fanout_total: None,
            fanout_running: 0,
            role_counts,
        };
        let lines = subagent_panel_lines(&summary, &[], 16, 8, &palette::UI_THEME);
        let role_line: &str = lines[1]
            .spans
            .first()
            .map(|s| s.content.as_ref())
            .unwrap_or("");
        assert!(
            role_line.chars().count() <= 16,
            "role line {role_line:?} exceeded content_width"
        );
    }

    // ---- Sidebar hover tooltip tests ----

    #[test]
    fn sidebar_hover_state_default_is_empty() {
        let state = SidebarHoverState::default();
        assert!(state.sections.is_empty());
    }

    #[test]
    fn sidebar_hover_section_stores_lines() {
        use ratatui::layout::Rect;
        let section = SidebarHoverSection {
            content_area: Rect::new(1, 1, 38, 8),
            lines: vec!["line 1".to_string(), "line 2".to_string()],
            rows: vec![],
        };
        assert_eq!(section.lines.len(), 2);
        assert_eq!(section.lines[0], "line 1");
        assert!(section.content_area.x > 0);
    }

    #[test]
    fn hover_line_matching_respects_content_area_offset() {
        use ratatui::layout::Rect;
        let section = SidebarHoverSection {
            content_area: Rect::new(62, 2, 36, 6),
            lines: vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string(),
            ],
            rows: vec![],
        };

        // Mouse within content area, first line
        let line_idx = (2u16.saturating_sub(section.content_area.y)) as usize;
        assert_eq!(section.lines[line_idx], "first");

        // Mouse within content area, second line
        let line_idx = (3u16.saturating_sub(section.content_area.y)) as usize;
        assert_eq!(section.lines[line_idx], "second");

        // Mouse outside content area (above) — row < content_area.y
        assert!((1u16) < section.content_area.y);
    }

    #[test]
    fn sidebar_hover_rows_mark_source_text_diff_as_truncated() {
        use ratatui::layout::Rect;
        let display = vec!["[~] agent imple…".to_string()];
        let full = vec!["[~] agent implementation-worker-for-sidebar-detail-popover".to_string()];
        let rows = sidebar_hover_rows(Rect::new(62, 5, 16, 4), &display, &full, &[]);

        let expected = SidebarHoverRow {
            row_y: 5,
            display_text: display[0].clone(),
            full_text: full[0].clone(),
            detail: None,
            is_truncated: true,
            click_action: None,
            stop_action: None,
            stop_zone_start_col: None,
            stop_zone_end_col: None,
        };
        assert_eq!(rows, vec![expected]);
    }

    #[test]
    fn sidebar_hover_rows_do_not_invent_a_stop_zone_from_text() {
        use ratatui::layout::Rect;

        let display = vec!["cargo test [x]".to_string(), "  running 1.00s".to_string()];
        let full = display.clone();
        let actions = vec![None, None];

        let rows = sidebar_hover_rows(Rect::new(60, 5, 20, 4), &display, &full, &actions);

        assert!(rows.iter().all(|row| row.click_action.is_none()));
        assert!(rows.iter().all(|row| row.stop_action.is_none()));
        assert!(rows.iter().all(|row| row.stop_zone_start_col.is_none()));
        assert!(rows.iter().all(|row| row.stop_zone_end_col.is_none()));
    }

    #[test]
    fn subagent_hover_text_preserves_full_agent_id_and_progress() {
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("worker".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let long_id = "019e9142-83f6-7713-87f1-28902e74bf05";
        let long_progress =
            "currently reviewing sidebar hover popover wrapping and hitbox metadata";
        let rows = vec![SidebarAgentRow {
            id: long_id.to_string(),
            model: None,
            parent_run_id: None,
            spawn_depth: 1,
            name: "sidebar-detail-worker-with-long-name".to_string(),
            role: "worker".to_string(),
            status: "running".to_string(),
            objective: None,
            git_branch: Some("codex/sidebar-hover".to_string()),
            progress: Some(long_progress.to_string()),
            steps_taken: 9,
            duration_ms: Some(12_345),
            expanded: false,
        }];

        let hover = subagent_panel_hover_texts(&summary, &rows, 5);
        assert!(
            hover.iter().any(|line| line.contains(long_id)),
            "hover text should include the full agent id: {hover:?}"
        );
        assert!(
            hover.iter().any(|line| line.contains(long_progress)),
            "hover text should include the full progress before popover wrapping: {hover:?}"
        );
    }

    #[test]
    fn subagent_expanded_detail_line_shows_model() {
        // #D (0.8.67 dogfood): the model each worker runs on must be visible
        // in the expanded detail line so Hunter can tell per-agent routes apart.
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_model_detail".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "model-worker".to_string(),
            role: "worker".to_string(),
            model: Some("kimi-k2.6".to_string()),
            status: "running".to_string(),
            objective: None,
            git_branch: None,
            progress: Some("working".to_string()),
            steps_taken: 3,
            duration_ms: Some(1_000),
            expanded: true,
        }];

        let (lines, _) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert!(
            text.iter().any(|line| line.contains("model kimi-k2.6")),
            "expanded detail line should surface the agent model: {text:?}"
        );
    }

    #[test]
    fn subagent_expanded_dossier_rows_register_read_only_inspection() {
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_drill".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "worker".to_string(),
            model: Some("deepseek-v4-flash".to_string()),
            status: "running".to_string(),
            objective: Some("map the repo".to_string()),
            git_branch: None,
            progress: Some("step 3".to_string()),
            steps_taken: 3,
            duration_ms: Some(2_000),
            expanded: true,
        }];

        let (lines, actions) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        assert_eq!(lines.len(), actions.len());
        assert!(
            actions.iter().any(|action| matches!(
                action,
                Some(SidebarRowAction::InspectText { label, .. }) if label == "scout"
            )),
            "canonical child rows should remain inspectable: {actions:?}"
        );
    }

    #[test]
    fn subagent_expanded_detail_never_blank_for_sparse_worker() {
        // #4094: expanding a running worker must show real activity, not a
        // bare status string. A freshly-spawned worker with an objective and
        // elapsed time but no model/steps/progress/branch previously rendered
        // an essentially blank detail line.
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_sparse".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "worker".to_string(),
            model: None,
            status: "running".to_string(),
            objective: Some("Audit TUI input-pump path for starvation".to_string()),
            git_branch: None,
            progress: None,
            steps_taken: 0,
            duration_ms: Some(4_000),
            expanded: true,
        }];

        let (lines, _) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        // The expanded detail line (the indented second row) carries the
        // objective and elapsed time, not just "running". Elapsed time is
        // unique to the detail line, so key off it.
        let detail = text
            .iter()
            .find(|line| line.contains("4.0s"))
            .expect("expanded detail should surface elapsed time: {text:?}");
        assert!(
            detail.contains("running"),
            "detail should carry the status: {detail:?}"
        );
        assert!(
            detail.contains("Audit TUI input-pump"),
            "detail should surface the worker objective: {detail:?}"
        );
    }

    #[test]
    fn subagent_expanded_detail_shows_status_when_all_fields_empty() {
        // #4094: a progress-only worker with no objective/model/duration must
        // still render a non-empty detail line (the status), never a blank row.
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_progress_only".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "child".to_string(),
            role: "agent".to_string(),
            model: None,
            status: "tool".to_string(),
            objective: None,
            git_branch: None,
            progress: None,
            steps_taken: 0,
            duration_ms: None,
            expanded: true,
        }];

        let (lines, _) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        // No line should be blank, and at least one carries the status.
        assert!(
            text.iter().all(|line| !line.trim().is_empty()),
            "no expanded detail line should be blank: {text:?}"
        );
        assert!(
            text.iter().any(|line| line.trim().starts_with("tool")),
            "expanded detail should show the status when no other fields exist: {text:?}"
        );
    }

    #[test]
    fn subagent_panel_stays_bounded_with_many_expanded_agents() {
        // #4094 freeze guard: opening details on many concurrent running
        // workers during active streaming must keep rendering bounded and
        // width-safe (never overflow, never hang). Reaching the assertions
        // proves the render path returns promptly under load.
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("worker".to_string(), 25);
        let summary = SidebarSubagentSummary {
            cached_total: 25,
            cached_running: 25,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows: Vec<SidebarAgentRow> = (0..25)
            .map(|i| SidebarAgentRow {
                id: format!("agent_{i}"),
                parent_run_id: None,
                spawn_depth: 1,
                name: format!("worker-{i}"),
                role: "worker".to_string(),
                model: Some("deepseek-v4-flash".to_string()),
                status: "running".to_string(),
                objective: Some(format!(
                    "Investigate sub-system {i} for the v0.8.68 stopship fix"
                )),
                git_branch: None,
                progress: Some(format!("step {i}: finished tool 'grep_files'")),
                steps_taken: i + 1,
                duration_ms: Some(1_000 + u64::from(i) * 500),
                expanded: true,
            })
            .collect();

        let content_width = 28usize;
        let max_rows = 6usize;
        let (lines, actions) =
            subagent_panel_rows(&summary, &rows, content_width, max_rows, &palette::UI_THEME);

        // Header + role-mix precede the per-agent loop, which is capped by
        // max_rows, so the total stays small and actions stay parallel.
        assert!(
            lines.len() <= max_rows + 2,
            "panel must stay bounded under many expanded agents: {} lines",
            lines.len()
        );
        assert_eq!(
            lines.len(),
            actions.len(),
            "lines and actions must stay parallel"
        );
        // Narrow-width readability: no rendered line overflows content_width.
        for line in &lines {
            let display = lines_to_text(std::slice::from_ref(line));
            let width = display
                .first()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.as_str()))
                .unwrap_or(0);
            assert!(
                width <= content_width,
                "line overflows narrow content_width {content_width}: {width} cells"
            );
        }
    }

    /// Display width of a single rendered sidebar line, styling stripped.
    fn subagent_line_width(line: &Line<'static>) -> usize {
        lines_to_text(std::slice::from_ref(line))
            .first()
            .map(|s| unicode_width::UnicodeWidthStr::width(s.as_str()))
            .unwrap_or(0)
    }

    /// Summary for a single cached worker with an explicit running count.
    fn single_worker_summary(running: usize) -> SidebarSubagentSummary {
        SidebarSubagentSummary {
            cached_total: 1,
            cached_running: running,
            ..SidebarSubagentSummary::default()
        }
    }

    #[test]
    fn subagent_expanded_detail_renders_many_tool_calls_without_overflow() {
        // #4094 item 1: a single worker that has fired many tool calls must
        // render correctly — non-empty, carrying the live tool-call trail plus
        // step count, width-bounded, and with a handle to inspect the rest —
        // never a panic, an overflow, or a blank panel.
        let summary = single_worker_summary(1);
        let rows = vec![SidebarAgentRow {
            id: "agent_busy".to_string(),
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "worker".to_string(),
            model: Some("deepseek-v4-flash".to_string()),
            status: "running".to_string(),
            objective: Some("Sweep the TUI for the v0.8.68 stopship".to_string()),
            // Latest entry in a long live-activity trail: tool name + status.
            progress: Some("step 247: finished tool grep_files ok".to_string()),
            steps_taken: 247,
            duration_ms: Some(96_000),
            expanded: true,
            ..SidebarAgentRow::default()
        }];

        // Wide render: the tool-call trail and step count are both visible.
        let (wide, wide_actions) = subagent_panel_rows(&summary, &rows, 200, 8, &palette::UI_THEME);
        let wide_text = lines_to_text(&wide);
        assert_eq!(
            wide.len(),
            wide_actions.len(),
            "lines/actions stay parallel"
        );
        assert!(
            wide_text.iter().all(|line| !line.trim().is_empty()),
            "no rendered line should be blank under many tool calls: {wide_text:?}"
        );
        let detail = wide_text
            .iter()
            .find(|line| line.contains("247 step(s)"))
            .expect("many-tool-call detail should surface the step count");
        assert!(
            detail.contains("grep_files"),
            "detail should carry the recent tool-call name/status: {detail:?}"
        );
        assert!(
            wide_text
                .iter()
                .any(|line| line.contains("handle_read agent:agent_busy/full_transcript")),
            "a busy worker needs a handle to inspect the fuller trail: {wide_text:?}"
        );

        // Narrow render of the same busy worker: bounded, no overflow, no panic.
        let content_width = 24usize;
        let (narrow, narrow_actions) =
            subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
        assert_eq!(narrow.len(), narrow_actions.len());
        for line in &narrow {
            assert!(
                subagent_line_width(line) <= content_width,
                "many-tool-call line overflows narrow width {content_width}: {} cells",
                subagent_line_width(line)
            );
        }
    }

    #[test]
    fn subagent_detail_readable_and_bounded_across_narrow_widths() {
        // #4094 item 2: the detail panel must stay readable at narrow widths —
        // the status verb stays visible at a usable-narrow column, and no line
        // (header, role-mix, label, dossier, or handle) overflows the column,
        // even at pathological single-cell widths.
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("worker".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "agent_narrow".to_string(),
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "worker".to_string(),
            model: Some("deepseek-v4-flash".to_string()),
            status: "running".to_string(),
            objective: Some("Audit the input pump for starvation under fan-out".to_string()),
            progress: Some("step 9: finished tool read_file ok".to_string()),
            steps_taken: 9,
            duration_ms: Some(12_000),
            expanded: true,
            ..SidebarAgentRow::default()
        }];

        for content_width in [1usize, 2, 3, 5, 8, 12, 16, 20, 24, 32, 48] {
            let (lines, actions) =
                subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            assert_eq!(lines.len(), actions.len(), "width {content_width}");
            for line in &lines {
                assert!(
                    subagent_line_width(line) <= content_width,
                    "line overflows content_width {content_width}: {} cells",
                    subagent_line_width(line)
                );
            }
        }

        // At a usable-narrow width the status verb must remain legible.
        let (lines, _) = subagent_panel_rows(&summary, &rows, 24, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert!(
            text.iter().any(|line| line.contains("running")),
            "status verb must remain visible at narrow width 24: {text:?}"
        );
    }

    #[test]
    fn subagent_status_matrix_renders_marker_verb_and_style() {
        // #4094 item 3: explicit running/done/failed (+ terminal) state matrix.
        // Each status must render its status marker, its status verb, and the
        // color that signals the state, so the panel is trustworthy at a glance.
        let theme = &palette::UI_THEME;
        let cases = [
            ("running", "[~]", theme.warning),
            ("done", "[\u{2713}]", theme.success),
            ("failed", "[!]", theme.error_fg),
            ("canceled", "[-]", theme.text_muted),
            ("interrupted", "[-]", theme.text_muted),
        ];
        for (status, marker, expected_color) in cases {
            let running = usize::from(status == "running");
            let summary = single_worker_summary(running);
            let rows = vec![SidebarAgentRow {
                id: "agent_matrix".to_string(),
                spawn_depth: 1,
                name: "scout".to_string(),
                role: "worker".to_string(),
                status: status.to_string(),
                objective: Some("Trace the input pump".to_string()),
                steps_taken: 3,
                duration_ms: Some(2_500),
                expanded: true,
                ..SidebarAgentRow::default()
            }];

            let (lines, _) = subagent_panel_rows(&summary, &rows, 48, 8, theme);
            let text = lines_to_text(&lines);

            // The label line carries the status marker in the state color.
            let marker_idx = text
                .iter()
                .position(|line| line.contains(marker))
                .unwrap_or_else(|| {
                    panic!("status {status} should render marker {marker}: {text:?}")
                });
            assert_eq!(
                lines[marker_idx].spans.first().map(|s| s.style.fg),
                Some(Some(expected_color)),
                "status {status} label should use its state color"
            );

            // The dossier line surfaces the status verb.
            assert!(
                text.iter()
                    .any(|line| line.trim_start().starts_with(status)),
                "status {status} detail should surface the verb: {text:?}"
            );
        }
    }

    #[test]
    fn subagent_completed_worker_surfaces_output_handle_not_inline_dump() {
        // #4094 item 4: a completed worker shows a bounded preview of its final
        // summary plus a copyable handle to the *full* output transcript,
        // instead of dumping the transcript inline (the freeze/emptiness risk).
        let summary = single_worker_summary(0);
        let rows = vec![SidebarAgentRow {
            id: "agent_7f3c".to_string(),
            spawn_depth: 1,
            name: "scout".to_string(),
            role: "worker".to_string(),
            model: Some("deepseek-v4-flash".to_string()),
            status: "done".to_string(),
            objective: Some("Audit TUI input path".to_string()),
            progress: Some("Wrote findings and staged a patch".to_string()),
            steps_taken: 12,
            duration_ms: Some(42_000),
            expanded: true,
            ..SidebarAgentRow::default()
        }];

        let (lines, actions) = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);

        // A handle line references the documented full-transcript var handle.
        let handle_line = text
            .iter()
            .find(|line| line.contains("handle_read"))
            .expect("completed worker should surface a full-output handle");
        assert!(
            handle_line.contains("agent:agent_7f3c/full_transcript"),
            "handle should reference the worker transcript: {handle_line:?}"
        );
        // The bounded preview (objective/summary) is still shown inline — the
        // handle augments, it does not replace, the summary.
        assert!(
            text.iter()
                .any(|line| line.contains("Audit TUI input path")),
            "bounded preview of the summary must remain: {text:?}"
        );
        // Lines and actions stay parallel with the extra handle line; the
        // handle line doubles as the drill-in affordance (#2889 slice).
        assert_eq!(lines.len(), actions.len());
        let handle_idx = text
            .iter()
            .position(|line| line.contains("handle_read"))
            .unwrap();
        assert!(matches!(
            actions[handle_idx],
            Some(SidebarRowAction::InspectText { ref label, .. })
                if label == "scout"
        ));
    }

    #[test]
    fn subagent_output_handle_gated_on_inspectable_output() {
        // #4094 item 4: the handle only appears once there is something to
        // inspect — a fresh, zero-step, non-terminal worker advertises no
        // handle (so we never point at an empty transcript), while a running
        // worker with steps does get the "inspect more" affordance.
        let fresh = SidebarAgentRow {
            id: "agent_fresh".to_string(),
            name: "scout".to_string(),
            role: "worker".to_string(),
            status: "starting".to_string(),
            steps_taken: 0,
            expanded: true,
            ..SidebarAgentRow::default()
        };
        assert!(
            subagent_output_handle(&fresh).is_none(),
            "a zero-step non-terminal worker must not advertise a handle"
        );

        let working = SidebarAgentRow {
            steps_taken: 4,
            status: "running".to_string(),
            ..fresh.clone()
        };
        assert_eq!(
            subagent_output_handle(&working).as_deref(),
            Some("agent:agent_fresh/full_transcript"),
            "a running worker with steps should expose the inspect-more handle"
        );

        // A terminal worker exposes the handle even with zero recorded steps.
        let failed_immediately = SidebarAgentRow {
            steps_taken: 0,
            status: "failed".to_string(),
            ..fresh.clone()
        };
        assert_eq!(
            subagent_output_handle(&failed_immediately).as_deref(),
            Some("agent:agent_fresh/full_transcript"),
            "a terminal worker should expose its transcript handle"
        );
    }

    #[test]
    fn agent_row_hover_includes_model() {
        // #D (0.8.67 dogfood): the hover dossier should also carry the model.
        let row = SidebarAgentRow {
            id: "agent_model_hover".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "hover-worker".to_string(),
            role: "worker".to_string(),
            model: Some("deepseek-v4".to_string()),
            status: "running".to_string(),
            objective: None,
            git_branch: None,
            progress: Some("reading".to_string()),
            steps_taken: 1,
            duration_ms: Some(500),
            expanded: false,
        };
        let hover = agent_row_hover_text(&row);
        assert!(
            hover.contains("model: deepseek-v4"),
            "hover dossier should include the model line: {hover:?}"
        );
    }

    #[test]
    fn subagent_label_hover_carries_full_agent_dossier() {
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("worker".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            id: "019e9142-83f6-7713-87f1-28902e74bf05".to_string(),
            model: None,
            parent_run_id: None,
            spawn_depth: 1,
            name: "doc-checker".to_string(),
            role: "worker".to_string(),
            status: "running".to_string(),
            objective: Some("Verify install docs against the release notes".to_string()),
            git_branch: Some("codex/doc-check".to_string()),
            progress: Some("step 2/3: running tool 'read_file'".to_string()),
            steps_taken: 2,
            duration_ms: Some(22_000),
            expanded: false,
        }];

        let hover = subagent_panel_hover_texts(&summary, &rows, 6);
        let label = hover
            .iter()
            .find(|text| text.contains("doc-checker"))
            .expect("label hover row should exist");

        assert!(
            label.contains("id: 019e9142-83f6-7713-87f1-28902e74bf05"),
            "label hover should carry the full id: {label:?}"
        );
        assert!(
            label.contains("status: running") && label.contains("elapsed"),
            "label hover should carry status and elapsed time: {label:?}"
        );
        assert!(
            label.contains("objective: Verify install docs against the release notes"),
            "label hover should carry the objective: {label:?}"
        );
        assert!(
            label.contains("branch: codex/doc-check"),
            "label hover should carry the branch: {label:?}"
        );
        assert!(
            label.contains("progress: step 2/3: running tool 'read_file'"),
            "label hover should carry untruncated progress: {label:?}"
        );
    }

    // ── #3030: stable labels instead of raw internal ids ───────────────────

    #[test]
    fn tasks_panel_shows_stable_turn_label_not_uuid() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("0196f0a3-1111-2222-3333-444455556666".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());
        app.turn_counter = 3;

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));
        assert!(
            text[0].contains("Turn 3 (in_progress)"),
            "compact row must show the stable turn label: {text:?}"
        );
        assert!(
            !text[0].contains("0196f0a3"),
            "raw turn UUID must stay out of the compact row: {text:?}"
        );

        let hover = task_panel_hover_texts(&app, &task_panel_row_sets(&app), 8);
        assert!(
            hover[0].contains("0196f0a3-1111-2222-3333-444455556666"),
            "full turn UUID must remain available in hover text: {hover:?}"
        );
    }

    #[test]
    fn tasks_panel_turn_label_falls_back_before_first_counted_turn() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("0196f0a3-1111-2222-3333-444455556666".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());
        app.turn_counter = 0;

        let text = lines_to_text(&task_panel_lines(&app, 64, 8));
        assert!(
            text[0].contains("Current turn (in_progress)"),
            "zero counter falls back to a generic label: {text:?}"
        );
    }

    // --- Unicode / CJK / terminal-width QA (issue #3488) -------------------
    // The sub-agent overlay renders CJK display names next to ASCII ids,
    // numeric columns (step count, elapsed), status verbs, and branch lines.
    // These guard that a CJK name never shifts the status columns, corrupts the
    // panel border, or hides the running/completed state (#3488 dogfood case:
    // a worker named 抹香鲸).

    /// Build the exact dogfood fixture: a CJK-named running implementer with a
    /// mixed English/CJK objective, a long branch, step count, and elapsed time.
    fn cjk_running_implementer_row() -> SidebarAgentRow {
        SidebarAgentRow {
            id: "agent_e0b2dcf1".to_string(),
            parent_run_id: None,
            spawn_depth: 1,
            name: "抹香鲸".to_string(),
            role: "implementer".to_string(),
            model: Some("glm-5.2".to_string()),
            status: "running".to_string(),
            objective: Some(
                "QUESTION: Add Zhipu GLM as a first-class provider-scoped model (issue #3439)"
                    .to_string(),
            ),
            git_branch: Some("codex/issue-3439-zhipu-glm-fixture".to_string()),
            progress: Some("step 10: finished tool edit_file ok".to_string()),
            steps_taken: 10,
            duration_ms: Some(124_838),
            expanded: true,
        }
    }

    #[test]
    fn subagent_panel_cjk_display_name_keeps_columns_and_state_at_narrow_and_medium_widths() {
        let summary = single_worker_summary(1);
        let rows = vec![cjk_running_implementer_row()];

        // Across pathological single-cell widths up through a medium terminal,
        // every rendered line (count header, role-mix, label, dossier, handle)
        // must stay within the column budget by *display* width and never split
        // a wide glyph into a replacement char — which is what would corrupt the
        // panel border or visually drift the status columns.
        for content_width in [1usize, 2, 3, 5, 8, 12, 16, 20, 24, 40, 80] {
            let (lines, actions) =
                subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            assert_eq!(lines.len(), actions.len(), "width {content_width}");
            for line in &lines {
                assert!(
                    subagent_line_width(line) <= content_width,
                    "width {content_width}: line overflows by display width ({} cells)",
                    subagent_line_width(line)
                );
                let text = lines_to_text(std::slice::from_ref(line)).join("");
                assert!(
                    !text.contains('\u{FFFD}'),
                    "width {content_width}: wide glyph split during truncation: {text:?}"
                );
            }
        }

        // At medium/usable widths the CJK name must not hide the running state:
        // the status marker `[~]`, the compact stop target `[x]`, and the CJK
        // display name all survive, and the row still resolves to its agent id.
        for content_width in [40usize, 80] {
            let (lines, actions) =
                subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            let text = lines_to_text(&lines);

            let label_idx = text
                .iter()
                .position(|line| line.contains("抹香鲸"))
                .unwrap_or_else(|| {
                    panic!("width {content_width}: CJK display name dropped: {text:?}")
                });
            assert!(
                text[label_idx].contains("[~]"),
                "width {content_width}: running marker hidden by CJK name: {text:?}"
            );
            assert!(
                !text[label_idx].ends_with("[x]"),
                "width {content_width}: canonical row must not expose direct stop: {text:?}"
            );
            assert!(
                !text[label_idx].contains('\u{FFFD}'),
                "width {content_width}: CJK name split: {text:?}"
            );
            assert!(
                matches!(
                    actions[label_idx],
                    Some(SidebarRowAction::InspectText { ref label, .. })
                        if label == "抹香鲸"
                ),
                "width {content_width}: CJK row must still resolve to its agent id"
            );
        }
    }

    #[test]
    fn subagent_panel_mixed_ascii_cjk_objective_truncates_on_glyph_boundary() {
        // A long objective mixing ASCII (provider name, issue number) with CJK
        // and full-width punctuation. Truncation must land on a whole-glyph
        // boundary by display width, preserving the leading status marker
        // prefix and never emitting U+FFFD.
        let summary = single_worker_summary(1);
        let rows = vec![SidebarAgentRow {
            id: "agent_cjk_obj".to_string(),
            spawn_depth: 1,
            name: "抹香鲸".to_string(),
            role: "implementer".to_string(),
            status: "running".to_string(),
            objective: Some(
                "将智谱 GLM 添加为 provider-scoped provider，覆盖 issue #3439 的全部断言"
                    .to_string(),
            ),
            git_branch: Some("codex/issue-3439".to_string()),
            steps_taken: 4,
            duration_ms: Some(88_000),
            expanded: true,
            ..SidebarAgentRow::default()
        }];

        for content_width in [12usize, 20, 28, 40, 80] {
            let (lines, _) =
                subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            for line in &lines {
                assert!(
                    subagent_line_width(line) <= content_width,
                    "width {content_width}: objective line overflowed ({} cells)",
                    subagent_line_width(line)
                );
                let text = lines_to_text(std::slice::from_ref(line)).join("");
                assert!(
                    !text.contains('\u{FFFD}'),
                    "width {content_width}: mixed objective split a wide glyph: {text:?}"
                );
            }
        }

        // The label keeps its semantic status-marker prefix across widths.
        let (lines, _) = subagent_panel_rows(&summary, &rows, 40, 8, &palette::UI_THEME);
        let label_text = lines_to_text(&lines);
        let label = label_text
            .iter()
            .find(|line| line.contains("抹香鲸"))
            .expect("CJK name present at medium width");
        assert!(
            label.contains("[~]"),
            "status marker prefix must survive truncation: {label:?}"
        );
        assert!(!label.contains('\u{FFFD}'));
    }
}
