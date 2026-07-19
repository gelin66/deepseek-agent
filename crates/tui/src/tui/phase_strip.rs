//! Live phase band for the underwater shell.
//!
//! The HTML reference attaches activity to the transcript and leaves the
//! composer as the final stable object. That means live phases
//! (working / waiting / approval / failed / done) render **above** the
//! composer, while idle and typing keep a quiet phase line beneath it.
//!
//! Classic shell keeps the legacy footer-below-composer order; this module
//! only decides Ocean placement and paints the one-line band.

use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::tui::{
    app::App,
    underwater::{ShellPhase, ShellTier, phase_marker},
};

/// Where the phase band sits relative to the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseStripPlacement {
    /// Live activity: phase sits on the transcript side of the prompt.
    AboveComposer,
    /// Idle / drafting: quiet phase under the prompt.
    BelowComposer,
}

impl PhaseStripPlacement {
    /// Live phases stay above the composer so the prompt is the bottom
    /// stable object. Idle and typing keep the quiet footer under `❯`.
    #[must_use]
    pub fn for_phase(phase: ShellPhase) -> Self {
        match phase {
            ShellPhase::Working | ShellPhase::Approval | ShellPhase::Failed | ShellPhase::Done => {
                Self::AboveComposer
            }
            ShellPhase::Idle | ShellPhase::Typing => Self::BelowComposer,
        }
    }

    #[must_use]
    pub fn is_above_composer(self) -> bool {
        matches!(self, Self::AboveComposer)
    }
}

/// Fixed one-row reservation for the phase band.
#[must_use]
pub fn height() -> u16 {
    1
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width + 1 > width {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    result.push('…');
    result
}

/// Compact elapsed-time detail for the phase band.
/// Kept quieter than the classic footer's verbose tool-status line so the
/// transcript owns the ledger and the strip only names the live pulse.
fn working_detail(app: &App) -> Option<String> {
    app.turn_started_at
        .map(|started| started.elapsed().as_secs())
        .filter(|secs| *secs > 0)
        .map(|secs| format!("{secs}s"))
}

/// Paint the one-line phase band. Owns phase, optional working detail, cost,
/// and detail-key hints — never route/context (header) or Tasks/Runs
/// (work surface).
pub fn render(area: Rect, buf: &mut Buffer, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let status_toast = app.active_status_toast();
    let phase = ShellPhase::from_app(app);
    let tier = ShellTier::for_chrome_width(area.width);
    Block::default()
        .style(Style::default().bg(app.ui_theme.footer_bg))
        .render(area, buf);

    let (marker, phase_label) = phase_marker(app, phase);
    let phase_style =
        Style::default()
            .fg(phase.color(app))
            .add_modifier(if phase == ShellPhase::Approval {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
    let mut left = vec![
        Span::styled(marker, phase_style),
        Span::raw(" "),
        Span::styled(phase_label.clone(), phase_style),
    ];

    if tier != ShellTier::Compact
        && phase == ShellPhase::Working
        && let Some(detail) = working_detail(app)
    {
        left.push(Span::styled(
            " · ",
            Style::default().fg(app.ui_theme.text_dim),
        ));
        left.push(Span::styled(
            detail,
            Style::default().fg(app.ui_theme.status_working),
        ));
    }

    if tier != ShellTier::Compact
        && phase != ShellPhase::Done
        && let Some(toast) = status_toast.filter(|toast| {
            !toast.text.trim().is_empty() && toast.text.trim() != phase_label.as_ref()
        })
    {
        left.push(Span::styled(
            " · ",
            Style::default().fg(app.ui_theme.text_dim),
        ));
        left.push(Span::styled(
            truncate_to_width(toast.text.trim(), 40),
            Style::default().fg(crate::tui::ui::status_color(toast.level)),
        ));
    }

    let cost = app.displayed_session_cost_for_currency(app.cost_currency);
    let chip = crate::route_billing::usage_chip(
        app.billing_presentation,
        app.api_provider,
        &app.model,
        cost,
        app.cost_currency,
        None,
    );
    if let crate::route_billing::UsageChip::Money(amount) = chip
        && tier != ShellTier::Compact
    {
        left.push(Span::styled(
            " · ",
            Style::default().fg(app.ui_theme.text_dim),
        ));
        left.push(Span::styled(
            amount,
            Style::default().fg(app.ui_theme.text_muted),
        ));
    }

    // Live phases keep the strip quiet so the ledger owns attention.
    // Idle/typing advertise only commands handled by the canonical foreground.
    // Compact terminals keep the discovery entry; wider tiers also expose the
    // manual compaction operation.
    let right_text: Cow<'static, str> = if PhaseStripPlacement::for_phase(phase).is_above_composer()
    {
        Cow::Borrowed("")
    } else {
        Cow::Borrowed(match tier {
            ShellTier::Compact => "/help",
            ShellTier::Normal | ShellTier::Wide => "/help · /compact",
        })
    };

    let right_width = right_text.width();
    let available = usize::from(area.width);
    let left_width = span_width(&left);
    if right_width > 0 && left_width + right_width < available {
        left.push(Span::raw(" ".repeat(available - left_width - right_width)));
        left.push(Span::styled(
            right_text.into_owned(),
            Style::default().fg(app.ui_theme.text_hint),
        ));
    }
    Paragraph::new(Line::from(left)).render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, tui::app::TuiOptions};
    use ratatui::{Terminal, backend::TestBackend};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn test_app() -> App {
        App::new(
            TuiOptions {
                model: "deepseek-v4-flash".to_string(),
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
            },
            &Config::default(),
        )
    }

    fn buffer_row_text(buf: &Buffer, area: Rect) -> String {
        let mut row = String::new();
        let mut x = area.left();
        while x < area.right() {
            let symbol = buf[(x, area.y)].symbol();
            row.push_str(symbol);
            x = x.saturating_add(UnicodeWidthStr::width(symbol).max(1) as u16);
        }
        row
    }

    #[test]
    fn live_phases_sit_above_composer_idle_stays_below() {
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Working),
            PhaseStripPlacement::AboveComposer
        );
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Approval),
            PhaseStripPlacement::AboveComposer
        );
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Failed),
            PhaseStripPlacement::AboveComposer
        );
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Done),
            PhaseStripPlacement::AboveComposer
        );
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Idle),
            PhaseStripPlacement::BelowComposer
        );
        assert_eq!(
            PhaseStripPlacement::for_phase(ShellPhase::Typing),
            PhaseStripPlacement::BelowComposer
        );
    }

    #[test]
    fn working_marker_uses_the_live_seafoam_role() {
        let app = test_app();
        assert_eq!(
            ShellPhase::Working.color(&app),
            app.ui_theme.accent_secondary
        );
        assert_ne!(ShellPhase::Working.color(&app), app.ui_theme.info);
    }

    #[test]
    fn working_band_shows_elapsed_time_without_key_chorus() {
        let mut app = test_app();
        app.is_loading = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(12));

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &mut app))
            .expect("draw");
        let area = Rect::new(0, 0, 80, 1);
        let text = buffer_row_text(terminal.backend().buffer(), area);
        assert!(text.contains("工作中"), "{text}");
        assert!(text.contains("12s"), "{text}");
        assert!(
            !text.contains("/help") && !text.contains("/compact"),
            "live phase strip stays quiet: {text}"
        );
    }

    #[test]
    fn idle_band_advertises_only_canonical_commands_by_width() {
        fn render_text(width: u16) -> String {
            let mut app = test_app();
            let backend = TestBackend::new(width, 1);
            let mut terminal = Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| render(frame.area(), frame.buffer_mut(), &mut app))
                .expect("draw");
            buffer_row_text(terminal.backend().buffer(), Rect::new(0, 0, width, 1))
        }

        let compact = render_text(40);
        assert!(compact.contains("/help"), "{compact}");
        assert!(!compact.contains("/compact"), "{compact}");

        for width in [80, 120] {
            let text = render_text(width);
            assert!(text.contains("/help"), "{text}");
            assert!(text.contains("/compact"), "{text}");
            assert!(!text.contains("Alt+V"), "{text}");
            assert!(!text.contains("F1"), "{text}");
        }
    }
}
