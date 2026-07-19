//! Coherent shell grammar for the underwater TUI.
//!
//! This module owns phase, responsive density, the empty-state composition,
//! and the compact header/footer fact budget. Product data still belongs to
//! [`App`]; this is only its terminal projection. Keeping these decisions in
//! one place prevents the default UI from drifting back into a header +
//! sidebar + dashboard + footer composition with four owners for one fact.

use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::localization::{MessageId, tr};
use crate::tui::{
    app::{App, AppMode},
    approval::ApprovalMode,
    views::ModalKind,
};

/// Responsive density tier. It changes how much truth is shown, never the
/// underlying state grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellTier {
    Compact,
    Normal,
    Wide,
}

impl ShellTier {
    #[must_use]
    pub fn for_area(area: Rect) -> Self {
        if area.width < 60 || area.height < 16 {
            Self::Compact
        } else if area.width < 110 || area.height < 30 {
            Self::Normal
        } else {
            Self::Wide
        }
    }

    #[must_use]
    pub fn for_chrome_width(width: u16) -> Self {
        if width < 60 {
            Self::Compact
        } else if width < 110 {
            Self::Normal
        } else {
            Self::Wide
        }
    }
}

/// Perceptual session phase. Every treatment reads from this same enum so a
/// footer cannot say `idle` while the transcript is asking for approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPhase {
    Idle,
    Typing,
    Working,
    Waiting,
    Approval,
    Done,
    Failed,
}

const WORKING_BUBBLE_FRAMES: [&str; 8] = ["⠀", "⢀", "⣀", "⣄", "⣤", "⣦", "⣶", "⣿"];
const COMPLETION_BREATH_MS: u128 = 800;
const COMPLETION_RELEASE_MS: u128 = 560;

impl ShellPhase {
    #[must_use]
    pub fn from_app(app: &App) -> Self {
        if matches!(
            app.view_stack.top_kind(),
            Some(ModalKind::Approval | ModalKind::UserInput)
        ) {
            return Self::Approval;
        }
        if app.turn_error_posted
            || matches!(app.runtime_turn_status.as_deref(), Some("failed" | "error"))
        {
            return Self::Failed;
        }
        if app.is_loading || matches!(app.runtime_turn_status.as_deref(), Some("in_progress")) {
            return Self::Working;
        }
        if matches!(app.runtime_turn_status.as_deref(), Some("completed")) {
            return Self::Done;
        }
        if !app.input.is_empty() {
            return Self::Typing;
        }
        Self::Idle
    }

    #[must_use]
    pub fn label(self) -> Cow<'static, str> {
        match self {
            Self::Idle => tr(MessageId::PhaseIdle),
            Self::Typing => tr(MessageId::PhaseDraft),
            Self::Working => tr(MessageId::PhaseWorking),
            Self::Waiting | Self::Approval => tr(MessageId::PhaseWaitingOnYou),
            Self::Done => tr(MessageId::PhaseDone),
            Self::Failed => tr(MessageId::PhaseFailed),
        }
    }

    #[must_use]
    pub fn color(self, app: &App) -> Color {
        match self {
            Self::Idle => app.ui_theme.text_muted,
            Self::Done => app.ui_theme.success,
            Self::Typing => app.ui_theme.accent_primary,
            Self::Working => app.ui_theme.status_working,
            Self::Waiting | Self::Approval => app.ui_theme.accent_action,
            Self::Failed => app.ui_theme.error_fg,
        }
    }
}

fn completion_elapsed_ms(app: &App) -> Option<u128> {
    if app.low_motion || !app.fancy_animations {
        return None;
    }
    app.ocean_completion_started_at
        .map(|started| started.elapsed().as_millis())
        .filter(|elapsed| *elapsed < COMPLETION_BREATH_MS)
}

pub(crate) fn phase_marker(app: &App, phase: ShellPhase) -> (&'static str, Cow<'static, str>) {
    match phase {
        ShellPhase::Idle => ("·", phase.label()),
        ShellPhase::Typing => ("›", phase.label()),
        ShellPhase::Working => {
            let frame = if app.low_motion || !app.fancy_animations {
                WORKING_BUBBLE_FRAMES[4]
            } else {
                let elapsed = app.turn_started_at.map_or_else(
                    || app.ocean_started_at.elapsed(),
                    |started| started.elapsed(),
                );
                let index = (elapsed.as_millis() / 300) as usize % WORKING_BUBBLE_FRAMES.len();
                WORKING_BUBBLE_FRAMES[index]
            };
            (frame, phase.label())
        }
        ShellPhase::Waiting | ShellPhase::Approval => ("◆", phase.label()),
        ShellPhase::Done => match completion_elapsed_ms(app) {
            Some(elapsed) if elapsed < COMPLETION_RELEASE_MS => {
                let index = ((elapsed / 140) as usize + 4).min(WORKING_BUBBLE_FRAMES.len() - 1);
                (WORKING_BUBBLE_FRAMES[index], tr(MessageId::PhaseFinishing))
            }
            _ => ("✓", phase.label()),
        },
        ShellPhase::Failed => ("✕", phase.label()),
    }
}

fn mode_label(mode: AppMode) -> Cow<'static, str> {
    match mode {
        AppMode::Agent | AppMode::Auto | AppMode::Yolo => tr(MessageId::ChipModeAct),
        AppMode::Plan => tr(MessageId::ChipModePlan),
        AppMode::Operate => tr(MessageId::ChipModeOperate),
    }
}

/// Permission chip words. This maps from the typed [`ApprovalMode`] state —
/// never from the English `permission_chip_label()` strings — so localizing
/// (or rewording) the upstream chip labels can never silently break the chip.
fn permission_label(app: &App) -> Cow<'static, str> {
    if app.mode == AppMode::Plan {
        return tr(MessageId::ChipPermissionReadOnly);
    }
    match app.approval_mode {
        ApprovalMode::Suggest => tr(MessageId::ChipPermissionAsk),
        ApprovalMode::Auto => tr(MessageId::ChipPermissionAuto),
        // Keep the effective permission explicit. `bypass` is an
        // implementation detail and, more importantly, can imply that
        // repository law no longer applies. Full Access never bypasses
        // constitution rules.
        ApprovalMode::Bypass => tr(MessageId::ChipPermissionFullAccess),
        ApprovalMode::Never => tr(MessageId::ChipPermissionNever),
    }
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

fn compact_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Render the one-line shell header. Route, mode, permission, active-agent
/// count, and context each have exactly one owner here.
pub fn render_header(area: Rect, buf: &mut Buffer, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let tier = ShellTier::for_chrome_width(area.width);
    Block::default()
        .style(Style::default().bg(app.ui_theme.header_bg))
        .render(area, buf);

    let route_label = format!(
        "{} · {}",
        app.api_provider.display_name(),
        app.model_display_label()
    );
    let mut left = vec![
        Span::styled(
            "cw",
            Style::default()
                .fg(app.ui_theme.accent_primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(route_label, Style::default().fg(app.ui_theme.text_muted)),
        Span::styled(" · ", Style::default().fg(app.ui_theme.text_dim)),
        Span::styled(
            mode_label(app.mode),
            Style::default().fg(match app.mode {
                AppMode::Plan => app.ui_theme.mode_plan,
                AppMode::Operate => app.ui_theme.mode_operate,
                _ => app.ui_theme.mode_agent,
            }),
        ),
    ];
    if tier != ShellTier::Compact {
        // The Underwater shell owns its header rather than delegating to the
        // classic renderer, so render the selected status mark here too.
        // "cw" is already the leading brand mark; the other choices deserve
        // their visible indicator beside it.
        if let Some(indicator) = crate::tui::widgets::header_status_indicator_frame(
            (!app.low_motion && app.fancy_animations)
                .then_some(app.turn_started_at)
                .flatten(),
            &app.status_indicator,
        )
        .filter(|indicator| *indicator != "cw")
        {
            left.push(Span::raw(" "));
            left.push(Span::styled(
                indicator,
                Style::default()
                    .fg(app.ui_theme.info)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        left.push(Span::styled(
            " · ",
            Style::default().fg(app.ui_theme.text_dim),
        ));
        left.push(Span::styled(
            permission_label(app),
            Style::default().fg(app.ui_theme.text_muted),
        ));
    }

    let mut right = Vec::new();
    if tier != ShellTier::Compact
        && let Some((used, max, percent)) = crate::tui::ui::context_usage_snapshot(app)
    {
        let filled = ((percent / 100.0) * 5.0).ceil().clamp(0.0, 5.0) as usize;
        right.push(Span::styled(
            format!(
                "{}/{} [{}{}] {:.0}%",
                compact_tokens(used),
                compact_tokens(i64::from(max)),
                "▰".repeat(filled),
                "▱".repeat(5usize.saturating_sub(filled)),
                percent
            ),
            Style::default().fg(app.ui_theme.info),
        ));
    }
    if tier == ShellTier::Wide {
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
        right.push(Span::styled(
            format!("v{}", env!("DEEPSEEK_BUILD_VERSION")),
            Style::default().fg(app.ui_theme.text_hint),
        ));
    }

    let available = usize::from(area.width);
    let right_width = span_width(&right);
    let left_budget = available.saturating_sub(right_width + usize::from(right_width > 0));
    if span_width(&left) > left_budget {
        left = vec![
            Span::styled(
                "cw",
                Style::default()
                    .fg(app.ui_theme.accent_primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                truncate_to_width(&app.model_display_label(), left_budget.saturating_sub(7)),
                Style::default().fg(app.ui_theme.text_muted),
            ),
            Span::styled(" · ", Style::default().fg(app.ui_theme.text_dim)),
            Span::styled(
                mode_label(app.mode),
                Style::default().fg(app.ui_theme.accent_primary),
            ),
        ];
    }
    let left_width = span_width(&left);
    let gap = available.saturating_sub(left_width + right_width);
    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    let title_area = Rect { height: 1, ..area };
    Paragraph::new(Line::from(left)).render(title_area, buf);
    if area.height > 1 {
        let rule_area = Rect {
            y: area.y.saturating_add(1),
            height: 1,
            ..area
        };
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(usize::from(area.width)),
            Style::default().fg(app.ui_theme.border),
        )))
        .render(rule_area, buf);
    }
}

/// Render the fixed one-line phase band.
///
/// Ocean placement (above vs below the composer) is owned by
/// [`crate::tui::phase_strip`]; this entry point only paints the band so
/// classic callers and tests keep a stable name.
pub fn render_footer(area: Rect, buf: &mut Buffer, app: &mut App) {
    crate::tui::phase_strip::render(area, buf, app);
}

/// Build the idle composition: one brand mark and one context line.
/// Commands are discovered only through the canonical composer menu.
pub fn empty_state_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let width = usize::from(area.width);
    let tier = ShellTier::for_area(area);
    let mut lines = vec![Line::from(""); usize::from(area.height / 4)];
    if tier != ShellTier::Compact && area.height >= 14 && area.width >= 28 {
        let mark = [
            vec![Span::styled(
                "   ˚",
                Style::default().fg(app.ui_theme.accent_secondary),
            )],
            vec![Span::styled(
                " ▗▄▄▄▄▄▄▄▄▄▄▄▄▄▖    ▚▞",
                Style::default().fg(app.ui_theme.accent_primary),
            )],
            vec![
                Span::styled("▐██", Style::default().fg(app.ui_theme.accent_primary)),
                Span::styled("·", Style::default().fg(app.ui_theme.text_body)),
                Span::styled(
                    "████████████▙▄▄▄▞",
                    Style::default().fg(app.ui_theme.accent_primary),
                ),
            ],
            vec![Span::styled(
                " ▝▀▀▀▀▀▀▀▀▀▀▀▀▀▘",
                Style::default().fg(app.ui_theme.accent_primary),
            )],
        ];
        for row in mark {
            let row_width = span_width(&row);
            let inset = " ".repeat(width.saturating_sub(row_width) / 2);
            let mut spans = vec![Span::raw(inset)];
            spans.extend(row);
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));
    }

    let workspace = crate::utils::display_path(&app.workspace);
    let workspace = format!("{}：{workspace}", tr(MessageId::FooterWorkspacePrefix));
    let context = if tier == ShellTier::Compact {
        format!("codewhale · {workspace}")
    } else {
        format!(
            "codewhale · {workspace} · {} {}",
            tr(MessageId::EmptyStateMcpLabel),
            app.mcp_configured_count
        )
    };
    let context = truncate_to_width(&context, width);
    let inset = " ".repeat(width.saturating_sub(context.width()) / 2);
    lines.push(Line::from(Span::styled(
        format!("{inset}{context}"),
        Style::default().fg(app.ui_theme.text_muted),
    )));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, tui::app::TuiOptions};
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
                start_in_agent_mode: true,
                skip_onboarding: true,
                yolo: false,
                resume_session_id: None,
                initial_input: None,
            },
            &Config::default(),
        )
    }

    #[test]
    fn empty_state_uses_workspace_without_fabricated_git_state() {
        let mut app = test_app();
        app.workspace = PathBuf::from("/tmp/真实项目");

        for area in [Rect::new(0, 0, 40, 8), Rect::new(0, 0, 100, 20)] {
            let text = empty_state_lines(&app, area)
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>();

            assert!(text.contains("工作区"), "missing Chinese label: {text}");
            assert!(text.contains("真实项目"), "missing workspace path: {text}");
            assert!(!text.contains("无 git"), "{text}");
            assert!(!text.to_ascii_lowercase().contains("branch"), "{text}");
        }
    }

    fn footer_text(app: &mut App) -> String {
        let area = Rect::new(0, 0, 100, 1);
        let mut buf = Buffer::empty(area);
        render_footer(area, &mut buf, app);
        (0..area.width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    /// The footer consumes the toast system, not the legacy status sink: an
    /// informational acknowledgement must leave on its own instead of
    /// becoming permanent idle chrome.
    #[test]
    fn footer_notices_expire_instead_of_becoming_permanent_chrome() {
        let mut app = test_app();
        app.status_message = Some("Auto-compaction enabled".to_string());

        let fresh = footer_text(&mut app);
        assert!(
            fresh.contains("Auto-compaction enabled"),
            "a fresh notice should surface once: {fresh}"
        );

        for toast in &mut app.status_toasts {
            toast.created_at = Instant::now() - Duration::from_secs(60);
        }
        let later = footer_text(&mut app);
        assert!(
            !later.contains("Auto-compaction"),
            "an informational acknowledgement must expire without user action: {later}"
        );
        let later_compact: String = later.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            later_compact.contains("空闲"),
            "the stable phase fact survives the expiry: {later}"
        );
    }

    /// Errors are sticky: they outlive the informational TTL window and stay
    /// until their own resolution window passes.
    #[test]
    fn footer_errors_outlive_informational_acknowledgements() {
        let mut app = test_app();
        app.status_message = Some("Provider request failed: timeout".to_string());

        let fresh = footer_text(&mut app);
        assert!(fresh.contains("failed"), "error notice missing: {fresh}");

        if let Some(sticky) = app.sticky_status.as_mut() {
            sticky.created_at = Instant::now() - Duration::from_secs(6);
        } else {
            panic!("an error must be promoted to the sticky slot");
        }
        let held = footer_text(&mut app);
        assert!(
            held.contains("failed"),
            "errors must hold past the informational window: {held}"
        );
    }

    #[test]
    fn phase_markers_make_motion_and_attention_explicit() {
        let mut app = test_app();

        app.runtime_turn_status = Some("in_progress".to_string());
        app.turn_started_at = Some(Instant::now() - Duration::from_millis(1_250));
        let (working, label) = phase_marker(&app, ShellPhase::from_app(&app));
        assert_eq!(working, WORKING_BUBBLE_FRAMES[4]);
        assert_eq!(label, "工作中");

        app.low_motion = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(9));
        assert_eq!(
            phase_marker(&app, ShellPhase::Working).0,
            WORKING_BUBBLE_FRAMES[4]
        );

        app.runtime_turn_status = Some("failed".to_string());
        let (marker, label) = phase_marker(&app, ShellPhase::from_app(&app));
        assert_eq!(marker, "✕");
        assert_eq!(label, "失败");
    }

    #[test]
    fn attention_and_failure_keep_distinct_semantic_hues() {
        let app = test_app();
        assert_eq!(ShellPhase::Waiting.color(&app), app.ui_theme.accent_action);
        assert_eq!(ShellPhase::Approval.color(&app), app.ui_theme.accent_action);
        assert_eq!(ShellPhase::Failed.color(&app), app.ui_theme.error_fg);
        assert_ne!(
            ShellPhase::Waiting.color(&app),
            ShellPhase::Failed.color(&app)
        );
    }

    #[test]
    fn completion_releases_once_then_settles_to_checkmark() {
        let mut app = test_app();
        app.runtime_turn_status = Some("completed".to_string());
        app.low_motion = false;
        app.fancy_animations = true;
        app.ocean_completion_started_at = Some(Instant::now() - Duration::from_millis(120));

        let (marker, label) = phase_marker(&app, ShellPhase::from_app(&app));
        assert_ne!(marker, "✓");
        assert_eq!(label, "收尾中");

        app.ocean_completion_started_at = Some(Instant::now() - Duration::from_millis(700));
        let (marker, label) = phase_marker(&app, ShellPhase::Done);
        assert_eq!(marker, "✓");
        assert_eq!(label, "完成");

        app.low_motion = true;
        app.ocean_completion_started_at = Some(Instant::now());
        let (marker, label) = phase_marker(&app, ShellPhase::Done);
        assert_eq!(marker, "✓");
        assert_eq!(label, "完成");
    }

    #[test]
    fn phase_labels_are_simplified_chinese() {
        assert_eq!(ShellPhase::Idle.label(), "空闲");
        assert_eq!(ShellPhase::Working.label(), "工作中");
        assert_eq!(ShellPhase::Done.label(), "完成");
    }
}
