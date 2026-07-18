//! Sidebar rendering — Pinned / Activity / Agents / Context panels.
//!
//! Extracted from `tui/ui.rs` (P1.2). The sidebar appears to the right of
//! the chat transcript when the available width allows it. Each section
//! reads from `App` snapshots; mutation lives in the main app loop.

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

use super::app::{App, SidebarFocus};
use super::history::{GenericToolCell, HistoryCell, ToolStatus, summarize_tool_output};
use super::ui_text::truncate_line_to_width;

/// Tolerance for floating-point cost comparison in the sidebar breakdown.
/// Must be large enough that accumulated f64 error across hundreds of turns
/// does not prematurely hide the session+agents breakdown.
const COST_EQ_TOLERANCE: f64 = 1e-6;
const RECENT_TOOL_SCAN_LIMIT: usize = 24;

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
    let lines = task_panel_rows(app, &row_sets, content_width.max(1), usable_rows.max(1));
    // #4147: This panel renders live tools / background jobs, not durable task
    // state, so the user-facing label is "Activity" to match its contents and
    // avoid colliding with durable tasks. The internal identifiers keep the
    // "task_panel"/`SidebarFocus::Tasks` names (guard #4172).
    render_sidebar_section(f, area, "Activity", lines, app);
}

#[derive(Debug, Clone)]
struct SidebarToolRow {
    name: String,
    status: ToolStatus,
    summary: String,
    duration_ms: Option<u64>,
}

/// Canonical tool rows used by the Activity panel renderer.
struct TaskPanelRowSets {
    recent: Vec<SidebarToolRow>,
}

fn task_panel_row_sets(app: &App) -> TaskPanelRowSets {
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;
    let recent = if explicit_tasks_focus {
        recent_tool_rows(app, 4)
    } else {
        Vec::new()
    };
    TaskPanelRowSets { recent }
}

#[cfg(test)]
fn task_panel_lines(app: &App, content_width: usize, max_rows: usize) -> Vec<Line<'static>> {
    task_panel_rows(app, &task_panel_row_sets(app), content_width, max_rows)
}

/// Build the visible Activity panel lines.
fn task_panel_rows(
    app: &App,
    row_sets: &TaskPanelRowSets,
    content_width: usize,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let theme = &app.ui_theme;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));
    let explicit_tasks_focus = app.sidebar_focus == SidebarFocus::Tasks;

    if explicit_tasks_focus && app.runtime_turn_id.is_some() {
        let status = app
            .runtime_turn_status
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        // #3030: Use a stable turn number ("Turn 1") instead of the raw UUID.
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

    if lines.is_empty() || (lines.len() == 1 && app.runtime_turn_id.is_some()) {
        lines.push(Line::from(Span::styled(
            "No live tools",
            Style::default().fg(theme.text_muted),
        )));
    }

    lines
}

fn push_sidebar_label_theme(lines: &mut Vec<Line<'static>>, label: &str, theme: &palette::UiTheme) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.accent_primary).bold(),
    )));
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
        ToolStatus::Running => 1,
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

fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let usable_rows = area.height.saturating_sub(3) as usize;
    let summary = sidebar_subagent_summary(app);
    let rows = sidebar_agent_rows(app);
    let lines = subagent_panel_rows(
        &summary,
        &rows,
        content_width,
        usable_rows.max(1),
        &app.ui_theme,
    );

    render_sidebar_section(f, area, "Agents", lines, app);
}

fn sidebar_subagent_summary(app: &App) -> SidebarSubagentSummary {
    let mut role_counts = std::collections::BTreeMap::new();
    for child in app.child_agents.rows() {
        let role = if child.depth > 1 {
            "子级 Agent"
        } else {
            "子 Agent"
        };
        *role_counts.entry(role.to_string()).or_insert(0) += 1;
    }

    SidebarSubagentSummary {
        cached_total: app.child_agents.rows().len(),
        cached_running: app.child_agents.active_count(),
        role_counts,
        ..SidebarSubagentSummary::default()
    }
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
    pub parent_run_id: Option<String>,
    pub spawn_depth: u32,
    pub name: String,
    pub status: String,
}

fn sidebar_agent_rows(app: &App) -> Vec<SidebarAgentRow> {
    app.child_agents
        .rows()
        .iter()
        .enumerate()
        .map(|(index, child)| SidebarAgentRow {
            parent_run_id: Some(child.parent_run_id.0.clone()),
            spawn_depth: u32::from(child.depth),
            name: format!("子 Agent {}", index + 1),
            status: canonical_child_status(child.terminal.as_ref()).to_string(),
        })
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
    subagent_panel_rows(summary, rows, content_width, max_rows, theme)
}

/// Build the visible Agents panel lines.
fn subagent_panel_rows(
    summary: &SidebarSubagentSummary,
    rows: &[SidebarAgentRow],
    content_width: usize,
    max_rows: usize,
    theme: &palette::UiTheme,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_rows.max(4));

    let fanout_total = summary.fanout_total.unwrap_or(0);
    if summary.cached_total == 0 && summary.progress_only_count == 0 && fanout_total == 0 {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(theme.text_muted),
        )));
        return lines;
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
    }

    lines
}

fn agent_tree_prefix(row: &SidebarAgentRow) -> String {
    if row.parent_run_id.is_none() && row.spawn_depth <= 1 {
        return String::new();
    }
    let depth = row.spawn_depth.max(2).saturating_sub(2).min(6);
    format!("{}└─ ", "  ".repeat(depth as usize))
}

fn sidebar_agent_row_label(row: &SidebarAgentRow, max_width: usize) -> String {
    truncate_line_to_width(&row.name, max_width)
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

    render_sidebar_section(f, area, "Session", lines, app);
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

fn render_sidebar_section(
    f: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
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
#[cfg(test)]
mod tests {
    use super::{
        AutoSidebarPanel, AutoSidebarState, SidebarAgentRow, SidebarFocus, SidebarSubagentSummary,
        SidebarToolRow, ToolRowOrder, auto_sidebar_panels, canonical_child_status,
        context_panel_cost_line, editorial_tool_rows, normalize_activity_text, render_sidebar,
        sidebar_agent_rows, sidebar_subagent_summary, subagent_panel_lines, subagent_panel_rows,
        task_panel_lines, task_panel_row_sets, task_panel_rows,
    };
    use crate::config::Config;
    use crate::palette;
    use crate::tui::app::{App, TuiOptions};
    use crate::tui::history::{GenericToolCell, HistoryCell, ToolStatus};
    use codewhale_protocol::agent_runtime::{
        AgentOutcome, ModelAccounting, RecoveryAmbiguity, RecoveryAmbiguityPhase, RunId,
        RuntimeFailure, TerminalState,
    };
    use ratatui::{Terminal, backend::TestBackend, text::Line};
    use std::path::PathBuf;

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
        app.child_agents.begin_root(RunId("root-run".to_string()));
        app.child_agents.record_started(
            RunId(parent.to_string()),
            format!("call-{child}"),
            RunId(child.to_string()),
            depth,
        );
    }

    fn finish_child(app: &mut App, parent: &str, child: &str, terminal: TerminalState) {
        app.child_agents.record_finished(
            RunId(parent.to_string()),
            format!("call-{child}"),
            &AgentOutcome {
                run_id: RunId(child.to_string()),
                parent_run_id: Some(RunId(parent.to_string())),
                terminal,
                accounting: ModelAccounting::default(),
                runtime_model_requests: 1,
                runtime_retries: 0,
                tool_calls: 0,
            },
            "canonical handoff",
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
    fn task_panel_rows_keep_stable_turn_and_tool_detail() {
        let mut app = create_test_app();
        app.sidebar_focus = SidebarFocus::Tasks;
        app.runtime_turn_id = Some("turn_abcdef123456".to_string());
        app.runtime_turn_status = Some("in_progress".to_string());
        app.turn_counter = 3;
        app.add_message(HistoryCell::Tool(GenericToolCell {
            name: "exec_shell".to_string(),
            status: ToolStatus::Running,
            input_summary: Some("cargo test --workspace".to_string()),
            output: None,
            prompts: None,
            output_summary: None,
            is_diff: false,
        }));

        let row_sets = task_panel_row_sets(&app);
        let lines = task_panel_rows(&app, &row_sets, 80, 12);
        let text = lines_to_text(&lines);

        let tool_idx = text
            .iter()
            .position(|line| line.contains("cargo test --workspace"))
            .unwrap_or_else(|| panic!("canonical tool detail missing: {text:?}"));
        assert!(
            text[tool_idx].contains("cargo test --workspace"),
            "tool detail remains visible in the Activity panel: {text:?}"
        );
        assert!(
            text[0].starts_with("Turn 3"),
            "line shows stable turn label: {:?}",
            text[0]
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
    fn subagent_terminal_rows_stay_compact() {
        let summary = SidebarSubagentSummary {
            cached_total: 3,
            cached_running: 0,
            ..SidebarSubagentSummary::default()
        };
        let rows = ["canceled", "failed", "interrupted"]
            .into_iter()
            .enumerate()
            .map(|(idx, status)| SidebarAgentRow {
                parent_run_id: None,
                spawn_depth: 1,
                name: format!("子 Agent {}", idx + 1),
                status: status.to_string(),
            })
            .collect::<Vec<_>>();

        let lines = subagent_panel_rows(&summary, &rows, 72, 10, &palette::UI_THEME);
        let text = lines_to_text(&lines);

        assert!(
            text.iter().any(|line| line.contains("3 done")),
            "terminal summary remains visible: {text:?}"
        );
        for idx in 0..3 {
            assert!(
                text.iter()
                    .any(|line| line.contains(&format!("子 Agent {}", idx + 1))),
                "terminal worker label remains visible: {text:?}"
            );
        }
        assert_eq!(text.len(), 4, "header plus three compact rows: {text:?}");
    }

    #[test]
    fn subagent_panel_cancelled_rows_are_visibly_terminal_and_not_cancelable() {
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 0,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            parent_run_id: None,
            spawn_depth: 1,
            name: "子 Agent 1".to_string(),
            status: "canceled".to_string(),
        }];

        let lines = subagent_panel_rows(&summary, &rows, 72, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        let agent_idx = text
            .iter()
            .position(|line| line.contains("子 Agent 1"))
            .expect("cancelled agent row");

        assert!(
            text[agent_idx].contains("[-]"),
            "cancelled row should render with the terminal marker: {text:?}"
        );
        assert!(
            !text[agent_idx].ends_with("[x]"),
            "cancelled row must not show the inline stop target: {text:?}"
        );
    }

    #[test]
    fn subagent_sidebar_projects_event_order_and_indents_canonical_children() {
        let mut app = create_test_app();
        start_child(&mut app, "root-run", "agent_parent", 1);
        start_child(&mut app, "agent_parent", "agent_child", 2);

        let rows = sidebar_agent_rows(&app);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "子 Agent 1");
        assert_eq!(rows[1].name, "子 Agent 2");
        assert_eq!(rows[1].parent_run_id.as_deref(), Some("agent_parent"));
        assert_eq!(rows[1].spawn_depth, 2);

        let summary = SidebarSubagentSummary {
            cached_total: 2,
            cached_running: 2,
            ..SidebarSubagentSummary::default()
        };
        let lines = subagent_panel_rows(&summary, &rows, 64, 8, &palette::UI_THEME);
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
        app.add_message(HistoryCell::Tool(GenericToolCell {
            name: "task_shell_wait".to_string(),
            status: ToolStatus::Running,
            input_summary: Some("task_id: shell_33a08c3c".to_string()),
            output: None,
            prompts: None,
            output_summary: None,
            is_diff: false,
        }));

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
        for _ in 0..2 {
            app.add_message(HistoryCell::Tool(GenericToolCell {
                name: "task_shell_wait".to_string(),
                status: ToolStatus::Running,
                input_summary: Some("task_id: shell_33a08c3c".to_string()),
                output: None,
                prompts: None,
                output_summary: Some("Background task running (no new output).".to_string()),
                is_diff: false,
            }));
        }

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
        for summary in [
            "Background task running (no new output).",
            "Still running after 10s.",
        ] {
            app.add_message(HistoryCell::Tool(GenericToolCell {
                name: "task_shell_wait".to_string(),
                status: ToolStatus::Running,
                input_summary: None,
                output: None,
                prompts: None,
                output_summary: Some(summary.to_string()),
                is_diff: false,
            }));
        }

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
    fn agents_panel_running_state_renders_count_roles_and_compact_rows() {
        let mut app = create_test_app();
        start_child(&mut app, "root-run", "agent-one", 1);
        start_child(&mut app, "root-run", "agent-two", 1);
        start_child(&mut app, "agent-one", "agent-nested", 2);
        finish_child(
            &mut app,
            "root-run",
            "agent-two",
            TerminalState::Completed {
                message: "完成".to_string(),
            },
        );
        let summary = sidebar_subagent_summary(&app);
        let rows = sidebar_agent_rows(&app);
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
            text[1].contains("2 子 Agent") && text[1].contains("1 子级 Agent"),
            "role mix line: {:?}",
            text[1]
        );
        assert!(
            text.iter().any(|line| line.contains("[~] 子 Agent 1")),
            "running direct-child row missing: {text:?}",
        );
        assert!(
            text.iter().any(|line| line.contains("[✓] 子 Agent 2")),
            "completed direct-child row missing: {text:?}",
        );
        assert!(
            text.iter().any(|line| line.contains("└─ [~] 子 Agent 3")),
            "running nested-child row missing: {text:?}",
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
        role_counts.insert("子 Agent".to_string(), 1);
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
        // The two role labels produced by canonical child depth stay bounded.
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("子 Agent".to_string(), 123);
        role_counts.insert("子级 Agent".to_string(), 456);
        let summary = SidebarSubagentSummary {
            cached_total: 579,
            cached_running: 579,
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

    #[test]
    fn subagent_panel_stays_bounded_with_many_agents() {
        // Many concurrent canonical rows must remain bounded and width-safe.
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("子 Agent".to_string(), 25);
        let summary = SidebarSubagentSummary {
            cached_total: 25,
            cached_running: 25,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows: Vec<SidebarAgentRow> = (0..25)
            .map(|i| SidebarAgentRow {
                parent_run_id: None,
                spawn_depth: 1,
                name: format!("worker-{i}"),
                status: "running".to_string(),
            })
            .collect();

        let content_width = 28usize;
        let max_rows = 6usize;
        let lines =
            subagent_panel_rows(&summary, &rows, content_width, max_rows, &palette::UI_THEME);

        // Header + role-mix precede the per-agent loop, which is capped by
        // max_rows, so the total stays small.
        assert!(
            lines.len() <= max_rows + 2,
            "panel must stay bounded under many agents: {} lines",
            lines.len()
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
    fn subagent_rows_are_bounded_across_narrow_widths() {
        // Canonical status rows stay within even pathological narrow widths.
        let mut role_counts = std::collections::BTreeMap::new();
        role_counts.insert("子 Agent".to_string(), 1);
        let summary = SidebarSubagentSummary {
            cached_total: 1,
            cached_running: 1,
            role_counts,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![SidebarAgentRow {
            spawn_depth: 1,
            name: "子 Agent 1".to_string(),
            status: "running".to_string(),
            ..SidebarAgentRow::default()
        }];

        for content_width in [1usize, 2, 3, 5, 8, 12, 16, 20, 24, 32, 48] {
            let lines = subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            for line in &lines {
                assert!(
                    subagent_line_width(line) <= content_width,
                    "line overflows content_width {content_width}: {} cells",
                    subagent_line_width(line)
                );
            }
        }

        // At a usable-narrow width the canonical marker remains legible.
        let lines = subagent_panel_rows(&summary, &rows, 24, 8, &palette::UI_THEME);
        let text = lines_to_text(&lines);
        assert!(
            text.iter().any(|line| line.contains("[~] 子 Agent 1")),
            "status marker must remain visible at narrow width 24: {text:?}"
        );
    }

    #[test]
    fn subagent_status_matrix_renders_marker_and_style() {
        // Every TerminalState passes through the canonical child projection
        // before rendering its marker and state color.
        let theme = &palette::UI_THEME;
        let cases = vec![
            (None, "running", "[~]", theme.warning),
            (
                Some(TerminalState::Completed {
                    message: "完成".to_string(),
                }),
                "done",
                "[\u{2713}]",
                theme.success,
            ),
            (
                Some(TerminalState::Blocked {
                    reason: "等待输入".to_string(),
                }),
                "blocked",
                "[!]",
                theme.warning,
            ),
            (
                Some(TerminalState::Failed {
                    failure: RuntimeFailure::EmptyModelOutput,
                }),
                "failed",
                "[!]",
                theme.error_fg,
            ),
            (
                Some(TerminalState::Cancelled),
                "canceled",
                "[-]",
                theme.text_muted,
            ),
            (
                Some(TerminalState::Interrupted),
                "interrupted",
                "[-]",
                theme.text_muted,
            ),
            (
                Some(TerminalState::RecoveryRequired {
                    ambiguity: RecoveryAmbiguity {
                        phase: RecoveryAmbiguityPhase::ChildRun,
                        action_id: "child-run".to_string(),
                        message: "需要恢复".to_string(),
                    },
                }),
                "recovery",
                "[!]",
                theme.warning,
            ),
        ];
        for (terminal, expected_status, marker, expected_color) in cases {
            let status = canonical_child_status(terminal.as_ref());
            assert_eq!(status, expected_status);
            let running = usize::from(status == "running");
            let summary = single_worker_summary(running);
            let rows = vec![SidebarAgentRow {
                spawn_depth: 1,
                name: "子 Agent 1".to_string(),
                status: status.to_string(),
                ..SidebarAgentRow::default()
            }];

            let lines = subagent_panel_rows(&summary, &rows, 48, 8, theme);
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
        }
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

    // --- Canonical Chinese labels / terminal-width QA ----------------------

    #[test]
    fn canonical_child_rows_keep_cjk_columns_and_state_across_widths() {
        let summary = SidebarSubagentSummary {
            cached_total: 2,
            cached_running: 2,
            ..SidebarSubagentSummary::default()
        };
        let rows = vec![
            SidebarAgentRow {
                parent_run_id: Some("root-run".to_string()),
                spawn_depth: 1,
                name: "子 Agent 1".to_string(),
                status: "running".to_string(),
            },
            SidebarAgentRow {
                parent_run_id: Some("agent_parent".to_string()),
                spawn_depth: 2,
                name: "子 Agent 2".to_string(),
                status: "running".to_string(),
            },
        ];

        // Every compact line remains within the display-width budget and never
        // splits a Chinese glyph into a replacement character.
        for content_width in [1usize, 2, 3, 5, 8, 12, 16, 20, 24, 40, 80] {
            let lines = subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
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

        // At usable widths both canonical Chinese labels and the nested tree
        // prefix remain visible beside the running marker.
        for content_width in [40usize, 80] {
            let lines = subagent_panel_rows(&summary, &rows, content_width, 8, &palette::UI_THEME);
            let text = lines_to_text(&lines);

            let root = text
                .iter()
                .find(|line| line.contains("子 Agent 1"))
                .unwrap_or_else(|| panic!("width {content_width}: root label missing: {text:?}"));
            let child = text
                .iter()
                .find(|line| line.contains("子 Agent 2"))
                .unwrap_or_else(|| panic!("width {content_width}: child label missing: {text:?}"));
            assert!(
                root.contains("[~]") && child.contains("[~]") && child.contains("└─"),
                "width {content_width}: canonical marker/tree prefix missing: {text:?}"
            );
            assert!(
                !root.ends_with("[x]") && !child.ends_with("[x]"),
                "width {content_width}: canonical row must not expose direct stop: {text:?}"
            );
            assert!(!root.contains('\u{FFFD}') && !child.contains('\u{FFFD}'));
        }
    }
}
