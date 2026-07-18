//! Parsing and rendering for archived-context transcript cells.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

use super::{HistoryCell, TRANSCRIPT_RAIL};

/// Render an `<archived_context>` block with dimmed/italic styling.
pub(super) fn render_archived_context(
    cell: &HistoryCell,
    width: u16,
    _low_motion: bool,
) -> Vec<Line<'static>> {
    let HistoryCell::ArchivedContext {
        level,
        range,
        tokens,
        density,
        model,
        timestamp,
        summary,
    } = cell
    else {
        return Vec::new();
    };

    let body = if summary.is_empty() {
        "(no summary)".to_string()
    } else {
        summary.clone()
    };

    let label = format!("Context L{level}");
    let label_style = Style::default()
        .fg(palette::TEXT_DIM)
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().fg(palette::TEXT_DIM).italic();

    let content_width = width.saturating_sub(4).max(1);

    let mut lines = Vec::new();

    let range_display = if range.is_empty() {
        String::new()
    } else {
        range.to_string()
    };
    let mut header = format!("{label}  {range_display}");
    if !tokens.is_empty() {
        header.push_str(&format!("  {tokens}"));
    }
    if !density.is_empty() && density != tokens {
        header.push_str(&format!("  {density}"));
    }
    lines.push(Line::from(Span::styled(header, label_style)));

    let model_display = if model.is_empty() {
        String::new()
    } else {
        format!("via {model}")
    };
    let ts_display = if timestamp.is_empty() {
        String::new()
    } else {
        timestamp.clone()
    };
    let mut sub = String::new();
    if !model_display.is_empty() {
        sub.push_str(&model_display);
    }
    if !ts_display.is_empty() {
        if !sub.is_empty() {
            sub.push_str(" · ");
        }
        sub.push_str(&ts_display);
    }
    if !sub.is_empty() {
        lines.push(Line::from(Span::styled(
            sub,
            Style::default().fg(palette::TEXT_MUTED),
        )));
    }

    let rendered = crate::tui::markdown_render::render_markdown(&body, content_width, body_style);
    for (idx, line) in rendered.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = vec![Span::styled(
                TRANSCRIPT_RAIL.to_string(),
                Style::default().fg(palette::TEXT_DIM),
            )];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        } else {
            let mut spans = vec![Span::raw("  ")];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::from(""));

    lines
}
