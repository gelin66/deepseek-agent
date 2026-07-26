use ratatui::{
    Frame,
    layout::Rect,
    prelude::Widget,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::palette;
use crate::tui::app::App;
use crate::tui::ui_text::truncate_line_to_width;
use dse_localization::MessageId;

use super::model::{WorkRow, WorkSurfaceLayout, WorkTone, project};

const SIDE_RAIL_MIN_HOST_WIDTH: u16 = 110;
const SIDE_RAIL_MIN_HOST_HEIGHT: u16 = 20;
const SIDE_RAIL_MIN_WIDTH: u16 = 26;
const SIDE_RAIL_MAX_WIDTH: u16 = 40;
const SIDE_RAIL_MIN_CHAT_WIDTH: u16 = 40;

fn responsive_layout(host_width: u16, host_height: u16) -> WorkSurfaceLayout {
    if host_width >= SIDE_RAIL_MIN_HOST_WIDTH && host_height >= SIDE_RAIL_MIN_HOST_HEIGHT {
        WorkSurfaceLayout::RightRail
    } else {
        WorkSurfaceLayout::TopStrip
    }
}

/// Responsive height for the bounded, read-only work projection.
pub fn height(app: &mut App, width: u16, terminal_height: u16) -> u16 {
    if project(app).is_empty() {
        app.work_surface.latest_rows.clear();
        return 0;
    }
    app.work_surface.layout = responsive_layout(width, terminal_height);
    if app.work_surface.layout == WorkSurfaceLayout::RightRail {
        return 0;
    }
    match terminal_height {
        0..=16 => 5,
        17..=23 => 6,
        _ => 8,
    }
}

/// Split the transcript slot for a side rail. Top placement consumes its own
/// vertical row before this point, so it returns the chat area unchanged.
pub fn split_chat(app: &mut App, area: Rect) -> (Rect, Option<Rect>) {
    if app.work_surface.latest_rows.is_empty()
        || app.work_surface.layout == WorkSurfaceLayout::TopStrip
    {
        return (area, None);
    }

    let proportional = area.width.saturating_mul(30) / 100;
    let rail_width = proportional
        .clamp(SIDE_RAIL_MIN_WIDTH, SIDE_RAIL_MAX_WIDTH)
        .min(area.width.saturating_sub(SIDE_RAIL_MIN_CHAT_WIDTH));
    if rail_width < SIDE_RAIL_MIN_WIDTH {
        app.work_surface.layout = WorkSurfaceLayout::TopStrip;
        return (area, None);
    }

    let chat_width = area.width.saturating_sub(rail_width);
    (
        Rect {
            width: chat_width,
            ..area
        },
        Some(Rect {
            x: area.x.saturating_add(chat_width),
            width: rail_width,
            ..area
        }),
    )
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let layout = app.work_surface.layout;
    let body_area = match layout {
        WorkSurfaceLayout::TopStrip => Rect {
            height: area.height.saturating_sub(1),
            ..area
        },
        WorkSurfaceLayout::RightRail => Rect {
            x: area.x.saturating_add(1),
            width: area.width.saturating_sub(1),
            ..area
        },
    };

    let mut rows = project(app);
    if body_area.height <= 4 {
        // A short terminal still shows the acceptance loop before optional
        // worker, permission, and persistence diagnostics.
        let mut compact = Vec::new();
        for id in [
            "section:task",
            "task:status",
            "task:changes",
            "task:verification",
        ] {
            if let Some(row) = rows.iter().find(|row| row.id == id) {
                compact.push(row.clone());
            }
        }
        for row in rows.iter().filter(|row| row.tone != WorkTone::Heading) {
            if compact.len() >= usize::from(body_area.height) {
                break;
            }
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
        .style(Style::default().bg(palette::DSE_BG))
        .render(area, frame.buffer_mut());

    let lines = rows
        .iter()
        .take(usize::from(body_area.height))
        .map(|row| {
            let compact_owner = if body_area.height <= 4 {
                row.id
                    .split_once(':')
                    .map(|(kind, _)| match kind {
                        "task" => format!("{} · ", app.tr(MessageId::SidebarTasksLabel)),
                        "worker" => format!("{} · ", app.tr(MessageId::SidebarWorkersLabel)),
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
            Line::from(Span::styled(format!("{prefix}{label}"), row_style(row)))
        })
        .collect::<Vec<_>>();

    Paragraph::new(lines).render(content_area, frame.buffer_mut());
    render_divider(frame, area, layout);
}

fn row_style(row: &WorkRow) -> Style {
    let fg = match row.tone {
        WorkTone::Heading => palette::DSE_ACCENT_PRIMARY,
        WorkTone::Active => palette::DSE_INFO,
        WorkTone::Attention => palette::STATUS_ERROR,
        WorkTone::Success => palette::STATUS_SUCCESS,
        WorkTone::Muted => palette::TEXT_MUTED,
        WorkTone::Worker => palette::DSE_INFO,
    };
    let style = Style::default().fg(fg).bg(palette::DSE_BG);
    if row.tone == WorkTone::Heading {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn render_divider(frame: &mut Frame, area: Rect, layout: WorkSurfaceLayout) {
    match layout {
        WorkSurfaceLayout::TopStrip => {
            let y = area.bottom().saturating_sub(1);
            for x in area.left()..area.right() {
                frame.buffer_mut()[(x, y)]
                    .set_symbol("─")
                    .set_fg(palette::BORDER_COLOR)
                    .set_bg(palette::DSE_BG);
            }
        }
        WorkSurfaceLayout::RightRail => {
            let x = area.left();
            for y in area.top()..area.bottom() {
                frame.buffer_mut()[(x, y)]
                    .set_symbol("│")
                    .set_fg(palette::BORDER_COLOR)
                    .set_bg(palette::DSE_BG);
            }
        }
    }
}
