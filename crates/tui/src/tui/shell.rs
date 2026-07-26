//! Canonical terminal-native grammar for the DSE main work surface.
//!
//! This module owns phase, responsive density, the empty-state composition,
//! and the compact header/footer fact budget. Product data still belongs to
//! [`App`]; this is only its terminal projection. Keeping these decisions in
//! one place prevents the default UI from duplicating the same fact across
//! several independent surfaces.

use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::tui::{app::App, run_presentation::RunPresentationPhase, views::ModalKind};
use dse_localization::{MessageId, tr};
use dse_protocol::agent_runtime::RunPermissionMode;

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

/// Perceptual session phase derived from canonical presentation and bounded
/// local interaction state. A footer cannot say `idle` while the transcript
/// is asking for approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPhase {
    Idle,
    Typing,
    Working,
    Approval,
    Done,
    Failed,
}

impl ShellPhase {
    #[must_use]
    pub fn from_app(app: &App) -> Self {
        if matches!(
            app.view_stack.top_kind(),
            Some(ModalKind::Approval | ModalKind::UserInput)
        ) {
            return Self::Approval;
        }
        match app.run_presentation.phase() {
            RunPresentationPhase::Thinking
            | RunPresentationPhase::Executing
            | RunPresentationPhase::Verifying
            | RunPresentationPhase::Reworking => return Self::Working,
            RunPresentationPhase::WaitingForUser => return Self::Approval,
            RunPresentationPhase::Completed => return Self::Done,
            RunPresentationPhase::Blocked
            | RunPresentationPhase::Failed
            | RunPresentationPhase::Cancelled
            | RunPresentationPhase::Interrupted
            | RunPresentationPhase::RecoveryRequired => return Self::Failed,
            RunPresentationPhase::Idle => {}
        }
        if app.is_loading {
            return Self::Working;
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
            Self::Approval => tr(MessageId::PhaseWaitingOnYou),
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
            Self::Approval => app.ui_theme.accent_action,
            Self::Failed => app.ui_theme.error_fg,
        }
    }
}

pub(crate) fn phase_marker(app: &App, phase: ShellPhase) -> (&'static str, Cow<'static, str>) {
    let label = if app.run_presentation.has_root()
        && !matches!(app.run_presentation.phase(), RunPresentationPhase::Idle)
    {
        Cow::Owned(app.run_presentation.phase().localized_label(app.language))
    } else {
        phase.label()
    };
    match phase {
        ShellPhase::Idle => ("·", label),
        ShellPhase::Typing => ("›", label),
        ShellPhase::Working => ("●", label),
        ShellPhase::Approval => ("◆", label),
        ShellPhase::Done => ("✓", label),
        ShellPhase::Failed => ("✕", label),
    }
}

/// Permission chip words map directly from the canonical typed permission state,
/// so rewording the localized labels cannot change approval behavior.
fn permission_label(app: &App) -> Cow<'static, str> {
    let mode = app
        .run_presentation
        .permission_mode()
        .unwrap_or(app.permission_mode);
    match mode {
        RunPermissionMode::Ask => tr(MessageId::ChipPermissionAsk),
        RunPermissionMode::Agent => tr(MessageId::ChipPermissionAgent),
        RunPermissionMode::FullAccess => tr(MessageId::ChipPermissionFullAccess),
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

/// Render the one-line shell header. Route, permission, active-agent
/// count, and context each have exactly one owner here.
pub fn render_header(area: Rect, buf: &mut Buffer, app: &App) {
    app.permission_chip_hitbox.set(None);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let tier = ShellTier::for_chrome_width(area.width);
    Block::default()
        .style(Style::default().bg(app.ui_theme.header_bg))
        .render(area, buf);

    let route_label = format!(
        "{} · {}",
        crate::config::DEEPSEEK_DISPLAY_NAME,
        app.model_display_label()
    );
    let mut left = vec![
        Span::styled(
            "DSE",
            Style::default()
                .fg(app.ui_theme.accent_primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(route_label, Style::default().fg(app.ui_theme.text_muted)),
    ];
    let mut permission_hitbox = None;
    if tier != ShellTier::Compact {
        left.push(Span::styled(
            " · ",
            Style::default().fg(app.ui_theme.text_dim),
        ));
        let permission = permission_label(app);
        let permission_x = area
            .x
            .saturating_add(u16::try_from(span_width(&left)).unwrap_or(u16::MAX));
        let permission_width =
            u16::try_from(unicode_width::UnicodeWidthStr::width(permission.as_ref()))
                .unwrap_or(u16::MAX);
        permission_hitbox = Some(Rect::new(permission_x, area.y, permission_width, 1));
        left.push(Span::styled(
            permission,
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
            format!("v{}", env!("DSE_BUILD_VERSION")),
            Style::default().fg(app.ui_theme.text_hint),
        ));
    }

    let available = usize::from(area.width);
    let right_width = span_width(&right);
    let left_budget = available.saturating_sub(right_width + usize::from(right_width > 0));
    if span_width(&left) > left_budget {
        permission_hitbox = None;
        left = vec![
            Span::styled(
                "DSE",
                Style::default()
                    .fg(app.ui_theme.accent_primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                truncate_to_width(&app.model_display_label(), left_budget.saturating_sub(8)),
                Style::default().fg(app.ui_theme.text_muted),
            ),
        ];
    }
    let left_width = span_width(&left);
    let gap = available.saturating_sub(left_width + right_width);
    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    let title_area = Rect { height: 1, ..area };
    Paragraph::new(Line::from(left)).render(title_area, buf);
    app.permission_chip_hitbox.set(permission_hitbox);
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
/// Placement (above vs below the composer) is owned by
/// [`crate::tui::phase_strip`]; this entry point paints the canonical band.
pub fn render_footer(area: Rect, buf: &mut Buffer, app: &mut App) {
    crate::tui::phase_strip::render(area, buf, app);
}

/// Build the quiet idle composition. Commands remain discoverable through the
/// canonical composer menu; the empty transcript carries no illustration or
/// invented repository state.
pub fn empty_state_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let width = usize::from(area.width);
    let tier = ShellTier::for_area(area);
    let mut lines = vec![Line::from(""); usize::from(area.height / 3)];

    let workspace = crate::utils::display_path(&app.workspace);
    let workspace = format!("{}：{workspace}", tr(MessageId::FooterWorkspacePrefix));
    let context = if tier == ShellTier::Compact {
        format!("dse · {workspace}")
    } else {
        format!(
            "dse · {workspace} · {} {}",
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
                language: dse_localization::ProductLanguage::SimplifiedChinese,
                workspace: PathBuf::from("."),
                config_path: None,
                allow_shell: false,
                use_alt_screen: true,
                use_mouse_capture: false,
                use_bracketed_paste: true,
                max_subagents: 1,
                skills_dir: PathBuf::from("."),
                mcp_config_path: PathBuf::from("mcp.json"),
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

        app.is_loading = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_millis(1_250));
        let (working, label) = phase_marker(&app, ShellPhase::from_app(&app));
        assert_eq!(working, "●");
        assert_eq!(label, "工作中");

        app.low_motion = true;
        app.turn_started_at = Some(Instant::now() - Duration::from_secs(9));
        assert_eq!(phase_marker(&app, ShellPhase::Working).0, "●");

        app.is_loading = false;
        let (marker, label) = phase_marker(&app, ShellPhase::Failed);
        assert_eq!(marker, "✕");
        assert_eq!(label, "失败");
    }

    #[test]
    fn attention_and_failure_keep_distinct_semantic_hues() {
        let app = test_app();
        assert_eq!(ShellPhase::Approval.color(&app), app.ui_theme.accent_action);
        assert_eq!(ShellPhase::Failed.color(&app), app.ui_theme.error_fg);
        assert_ne!(
            ShellPhase::Approval.color(&app),
            ShellPhase::Failed.color(&app)
        );
    }

    #[test]
    fn phase_labels_are_simplified_chinese() {
        assert_eq!(ShellPhase::Idle.label(), "空闲");
        assert_eq!(ShellPhase::Working.label(), "工作中");
        assert_eq!(ShellPhase::Done.label(), "完成");
    }
}
