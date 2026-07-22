//! Compact transcript rendering for activity metadata cells.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::palette;

use super::{GenericToolCell, truncate_text};

pub(super) fn render_activity_group(cell: &GenericToolCell, width: u16) -> Vec<Line<'static>> {
    let summary = cell.input_summary.as_deref().unwrap_or("Updated metadata");
    let budget = usize::from(width).max(1);
    vec![Line::from(Span::styled(
        truncate_text(summary, budget),
        Style::default().fg(palette::TEXT_MUTED),
    ))]
}
