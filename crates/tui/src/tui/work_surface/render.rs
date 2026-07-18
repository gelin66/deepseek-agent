use ratatui::{
    Frame,
    layout::Rect,
    prelude::Widget,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::localization::MessageId;
use crate::tui::app::App;
use crate::tui::ui_text::truncate_line_to_width;

use super::model::{WorkRow, WorkSurfacePlacement, WorkTone, project};

const SIDE_RAIL_MIN_HOST_WIDTH: u16 = 72;
const SIDE_RAIL_MIN_WIDTH: u16 = 26;
const SIDE_RAIL_MAX_WIDTH: u16 = 40;
const SIDE_RAIL_MIN_CHAT_WIDTH: u16 = 40;

fn effective_placement(
    configured: WorkSurfacePlacement,
    host_width: u16,
    classic_shell: bool,
) -> WorkSurfacePlacement {
    if classic_shell || host_width < SIDE_RAIL_MIN_HOST_WIDTH {
        WorkSurfacePlacement::Top
    } else {
        configured
    }
}

/// Responsive height for the bounded, read-only work projection.
pub fn height(app: &mut App, width: u16, terminal_height: u16, classic_shell: bool) -> u16 {
    if project(app).is_empty() {
        app.work_surface.latest_rows.clear();
        return 0;
    }
    app.work_surface.effective_placement =
        effective_placement(app.work_surface.placement, width, classic_shell);
    if app.work_surface.effective_placement != WorkSurfacePlacement::Top {
        return 0;
    }
    match terminal_height {
        0..=12 => 3,
        13..=16 => 5,
        17..=23 => 6,
        _ => 8,
    }
}

/// Split the transcript slot for a side rail. Top placement consumes its own
/// vertical row before this point, so it returns the chat area unchanged.
pub fn split_chat(app: &mut App, area: Rect, classic_shell: bool) -> (Rect, Option<Rect>) {
    let placement = effective_placement(app.work_surface.placement, area.width, classic_shell);
    app.work_surface.effective_placement = placement;
    if app.work_surface.latest_rows.is_empty() || placement == WorkSurfacePlacement::Top {
        return (area, None);
    }

    let proportional = area.width.saturating_mul(30) / 100;
    let rail_width = proportional
        .clamp(SIDE_RAIL_MIN_WIDTH, SIDE_RAIL_MAX_WIDTH)
        .min(area.width.saturating_sub(SIDE_RAIL_MIN_CHAT_WIDTH));
    if rail_width < SIDE_RAIL_MIN_WIDTH {
        app.work_surface.effective_placement = WorkSurfacePlacement::Top;
        return (area, None);
    }

    let chat_width = area.width.saturating_sub(rail_width);
    match placement {
        WorkSurfacePlacement::Left => (
            Rect {
                x: area.x.saturating_add(rail_width),
                width: chat_width,
                ..area
            },
            Some(Rect {
                width: rail_width,
                ..area
            }),
        ),
        WorkSurfacePlacement::Right => (
            Rect {
                width: chat_width,
                ..area
            },
            Some(Rect {
                x: area.x.saturating_add(chat_width),
                width: rail_width,
                ..area
            }),
        ),
        WorkSurfacePlacement::Top => (area, None),
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let placement = app.work_surface.effective_placement;
    let body_area = match placement {
        WorkSurfacePlacement::Top => Rect {
            height: area.height.saturating_sub(1),
            ..area
        },
        WorkSurfacePlacement::Left => Rect {
            width: area.width.saturating_sub(1),
            ..area
        },
        WorkSurfacePlacement::Right => Rect {
            x: area.x.saturating_add(1),
            width: area.width.saturating_sub(1),
            ..area
        },
    };

    let mut rows = project(app);
    if body_area.height <= 2 {
        // On a two-row surface, prefer actual work over section headings.
        let mut compact = Vec::new();
        for prefix in ["task:", "worker:"] {
            if let Some(row) = rows.iter().find(|row| row.id.starts_with(prefix)) {
                compact.push(row.clone());
            }
        }
        for row in rows.iter().filter(|row| row.tone != WorkTone::Heading) {
            if !compact.iter().any(|candidate| candidate.id == row.id) {
                compact.push(row.clone());
            }
        }
        rows = compact;
    }

    let inset = u16::from(body_area.width >= 60);
    let content_area = Rect {
        x: body_area.x.saturating_add(inset),
        y: body_area.y,
        width: body_area.width.saturating_sub(inset.saturating_mul(2)),
        height: body_area.height,
    };

    Block::default()
        .style(Style::default().bg(app.ui_theme.surface_bg))
        .render(area, frame.buffer_mut());

    let lines = rows
        .iter()
        .take(usize::from(body_area.height))
        .map(|row| {
            let compact_owner = if body_area.height <= 2 {
                row.id
                    .split_once(':')
                    .map(|(kind, _)| match kind {
                        "task" => format!("{} · ", app.tr(MessageId::SidebarTasksLabel)),
                        "worker" => format!("{} · ", app.tr(MessageId::FleetRosterWorkers)),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let prefix = if row.tone == WorkTone::Heading {
                format!("{} ", row.mark)
            } else {
                format!("{compact_owner}{} ", row.mark)
            };
            let label_width = usize::from(content_area.width)
                .saturating_sub(UnicodeWidthStr::width(prefix.as_str()))
                .max(1);
            let label = truncate_line_to_width(&row.label, label_width);
            Line::from(Span::styled(
                format!("{prefix}{label}"),
                row_style(app, row),
            ))
        })
        .collect::<Vec<_>>();

    Paragraph::new(lines).render(content_area, frame.buffer_mut());
    render_divider(frame, area, placement, app);
}

fn row_style(app: &App, row: &WorkRow) -> Style {
    let fg = match row.tone {
        WorkTone::Heading => app.ui_theme.accent_primary,
        WorkTone::Live => app.ui_theme.status_working,
        WorkTone::Attention => app.ui_theme.error_fg,
        WorkTone::Success => app.ui_theme.success,
        WorkTone::Muted => app.ui_theme.text_muted,
        WorkTone::Worker => app.ui_theme.info,
    };
    let style = Style::default().fg(fg).bg(app.ui_theme.surface_bg);
    if row.tone == WorkTone::Heading {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn render_divider(frame: &mut Frame, area: Rect, placement: WorkSurfacePlacement, app: &App) {
    match placement {
        WorkSurfacePlacement::Top => {
            let y = area.bottom().saturating_sub(1);
            for x in area.left()..area.right() {
                frame.buffer_mut()[(x, y)]
                    .set_symbol("─")
                    .set_fg(app.ui_theme.border)
                    .set_bg(app.ui_theme.surface_bg);
            }
        }
        WorkSurfacePlacement::Left | WorkSurfacePlacement::Right => {
            let x = if placement == WorkSurfacePlacement::Left {
                area.right().saturating_sub(1)
            } else {
                area.left()
            };
            for y in area.top()..area.bottom() {
                frame.buffer_mut()[(x, y)]
                    .set_symbol("│")
                    .set_fg(app.ui_theme.border)
                    .set_bg(app.ui_theme.surface_bg);
            }
        }
    }
}
