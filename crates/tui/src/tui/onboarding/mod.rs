//! Onboarding flow rendering and helpers.

pub mod api_key;
pub mod trust_directory;
pub mod welcome;

use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::localization::{MessageId, tr};
use crate::palette;
use crate::tui::app::{App, OnboardingState};

const ONBOARDED_MARKER_FILE: &str = ".onboarded";

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().style(Style::default().bg(palette::WHALE_BG));
    f.render_widget(block, area);

    const TOP_MARGIN: u16 = 2;
    let content_width = 76.min(area.width.saturating_sub(4));
    let content_height = 20.min(area.height.saturating_sub(TOP_MARGIN + 2));
    let content_area = Rect {
        x: (area.width.saturating_sub(content_width)) / 2,
        y: TOP_MARGIN,
        width: content_width,
        height: content_height,
    };

    let lines = match app.onboarding {
        OnboardingState::Welcome => welcome::lines(app),
        OnboardingState::ApiKey => api_key::lines(app),
        OnboardingState::TrustDirectory => trust_directory::lines(app),
        OnboardingState::Tips => tips_lines(app),
        OnboardingState::None => Vec::new(),
    };

    if !lines.is_empty() {
        let mut panel = Block::default()
            .title(Line::from(Span::styled(
                app.tr(MessageId::OnboardPanelTitle).to_string(),
                Style::default()
                    .fg(palette::WHALE_ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::WHALE_PANEL))
            .padding(Padding::new(2, 2, 1, 1));
        if !app.onboarding_workspace_trust_gate {
            let (step, total) = onboarding_step(app);
            panel = panel.title_bottom(Line::from(Span::styled(
                app.tr(MessageId::OnboardStepProgress)
                    .replace("{step}", &step.to_string())
                    .replace("{total}", &total.to_string()),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        let inner = panel.inner(content_area);
        f.render_widget(panel, content_area);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(paragraph, inner);
    }
}

fn onboarding_step(app: &App) -> (usize, usize) {
    let needs_trust = needs_trust_at(app.config_path.as_deref(), &app.workspace);
    // Welcome + Tips are always shown.
    let mut total = 2;
    if app.onboarding_needs_api_key {
        total += 1;
    }
    if needs_trust {
        total += 1;
    }

    let step = match app.onboarding {
        OnboardingState::Welcome => 1,
        OnboardingState::ApiKey => 2,
        OnboardingState::TrustDirectory => {
            if app.onboarding_needs_api_key {
                3
            } else {
                2
            }
        }
        OnboardingState::Tips => total,
        OnboardingState::None => total,
    };

    (step, total)
}

pub fn tips_lines(app: &App) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};

    let commands_line = app
        .tr(MessageId::OnboardTipsLine2)
        .replace("{help}", "/help")
        .replace("{compact}", "/compact");
    let cost_line = app
        .tr(MessageId::OnboardTipsLine3)
        .replace("{cost}", "/cost");
    let exit_line = app
        .tr(MessageId::OnboardTipsLine4)
        .replace("{exit}", "/exit")
        .replace("{quit}", "/quit");

    vec![
        Line::from(Span::styled(
            app.tr(MessageId::OnboardTipsTitle).to_string(),
            Style::default()
                .fg(palette::WHALE_INFO)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(app.tr(MessageId::OnboardTipsLine1).to_string())),
        Line::from(Span::raw(commands_line)),
        Line::from(Span::raw(cost_line)),
        Line::from(Span::raw(exit_line)),
        Line::from(vec![
            Span::styled(
                app.tr(MessageId::OnboardTipsFooterEnter)
                    .replace("{key}", "Enter"),
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.tr(MessageId::OnboardTipsFooterAction).to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]),
    ]
}

pub fn default_marker_path() -> Option<PathBuf> {
    crate::config::effective_home_dir().map(|home| marker_path_with_home(&home))
}

fn marker_path_with_home(home: &Path) -> PathBuf {
    home.join(".codewhale").join(ONBOARDED_MARKER_FILE)
}

pub fn is_onboarded() -> bool {
    default_marker_path().is_some_and(|path| path.exists())
}

pub fn mark_onboarded() -> std::io::Result<PathBuf> {
    let home = crate::config::effective_home_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            tr(MessageId::OnboardHomeDirectoryNotFound).into_owned(),
        )
    })?;
    mark_onboarded_at_home(&home)
}

fn mark_onboarded_at_home(home: &Path) -> std::io::Result<PathBuf> {
    let path = marker_path_with_home(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

pub fn needs_trust(workspace: &Path) -> bool {
    needs_trust_at(None, workspace)
}

pub fn needs_trust_at(config_path: Option<&Path>, workspace: &Path) -> bool {
    !crate::config::is_workspace_trusted_at(config_path, workspace)
}

pub fn mark_trusted_at(config_path: Option<&Path>, workspace: &Path) -> anyhow::Result<PathBuf> {
    crate::config::save_workspace_trust_at(config_path, workspace)
}

// ── API key validation and state-machine transitions ─────────────────

/// Result of inspecting an API-key string entered during onboarding.
///
/// `Accept` always lets the user proceed; the optional `warning` is shown
/// as a non-blocking status message (short keys, unusual formats, etc.).
/// `Reject` blocks the keystroke flow until the user fixes the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyValidation {
    Accept { warning: Option<String> },
    Reject(String),
}

/// Validate an API key entered during onboarding. Whitespace-only or
/// whitespace-containing keys are rejected; short or hyphen-less keys
/// are accepted with a warning so unusual provider key formats still
/// work.
#[must_use]
pub fn validate_api_key_for_onboarding(api_key: &str) -> ApiKeyValidation {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return ApiKeyValidation::Reject(tr(MessageId::OnboardApiKeyEmpty).into_owned());
    }
    if trimmed.contains(char::is_whitespace) {
        return ApiKeyValidation::Reject(tr(MessageId::OnboardApiKeyWhitespace).into_owned());
    }
    if trimmed.len() < 16 {
        return ApiKeyValidation::Accept {
            warning: Some(tr(MessageId::OnboardApiKeyShortWarning).into_owned()),
        };
    }
    if !trimmed.contains('-') {
        return ApiKeyValidation::Accept {
            warning: Some(tr(MessageId::OnboardApiKeyUnusualWarning).into_owned()),
        };
    }
    ApiKeyValidation::Accept { warning: None }
}

/// Leave the welcome screen and route directly to the next required step.
pub fn advance_onboarding_from_welcome(app: &mut App) {
    app.status_message = None;
    if app.onboarding_needs_api_key {
        app.onboarding = OnboardingState::ApiKey;
    } else if needs_trust_at(app.config_path.as_deref(), &app.workspace) {
        app.onboarding = OnboardingState::TrustDirectory;
    } else {
        app.onboarding = OnboardingState::Tips;
    }
}

pub fn advance_onboarding_after_api_key(app: &mut App) {
    app.status_message = None;
    if needs_trust_at(app.config_path.as_deref(), &app.workspace) {
        app.onboarding = OnboardingState::TrustDirectory;
    } else {
        app.onboarding = OnboardingState::Tips;
    }
}

/// Re-validate the current `api_key_input` and project the result onto
/// `app.status_message`. `show_empty_error` reports the "cannot be empty"
/// message even when the input has not been touched yet (used right
/// before submission); otherwise an empty input clears the status bar.
pub fn sync_api_key_validation_status(app: &mut App, show_empty_error: bool) {
    if app.api_key_input.trim().is_empty() && !show_empty_error {
        app.status_message = None;
        return;
    }

    match validate_api_key_for_onboarding(&app.api_key_input) {
        ApiKeyValidation::Accept { warning } => {
            app.status_message = warning;
        }
        ApiKeyValidation::Reject(message) => {
            app.status_message = Some(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::localization::tr;
    use crate::tui::app::{App, TuiOptions};
    use crate::tui::canonical_commands::command_infos;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn test_app() -> App {
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

    fn flattened(lines: Vec<ratatui::text::Line<'static>>) -> String {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn tips_copy_only_advertises_canonical_commands() {
        let app = test_app();
        let body = flattened(tips_lines(&app));

        let advertised = body
            .split_whitespace()
            .filter(|word| word.starts_with('/'))
            .map(|word| {
                word.trim_matches(|ch: char| {
                    !ch.is_ascii_alphanumeric() && !matches!(ch, '/' | '-' | '_')
                })
                .to_string()
            })
            .collect::<HashSet<_>>();
        let canonical = command_infos()
            .iter()
            .flat_map(|info| {
                std::iter::once(format!("/{}", info.name))
                    .chain(info.aliases.iter().map(|alias| format!("/{alias}")))
            })
            .collect::<HashSet<_>>();

        assert_eq!(advertised, canonical);
        assert!(body.contains("直接用自然语言描述任务"));
        for retired in ["/setup", "/constitution", "/provider", "/model", "Ctrl+K"] {
            assert!(
                !body.contains(retired),
                "首次启动页不得宣传不可用入口：{retired}"
            );
        }
        assert!(body.contains("按 Enter 进入任务输入区"));
    }

    #[test]
    fn visible_permission_labels_are_simplified_chinese() {
        assert_eq!(tr(MessageId::ChipPermissionAsk), "询问");
        assert_eq!(tr(MessageId::ChipPermissionAuto), "自动审查");
        assert_eq!(tr(MessageId::ChipPermissionFullAccess), "完全访问");
        assert_eq!(tr(MessageId::ChipPermissionNever), "从不询问");
    }

    #[test]
    fn fresh_install_marker_path_uses_codewhale_not_legacy() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let expected = tmp.path().join(".codewhale").join(ONBOARDED_MARKER_FILE);
        assert_eq!(marker_path_with_home(tmp.path()), expected);

        let written = mark_onboarded_at_home(tmp.path()).expect("mark onboarded");
        assert_eq!(written, expected);
        assert!(expected.exists());
        assert!(
            !tmp.path().join(".deepseek").exists(),
            "fresh onboarding must not recreate the legacy .deepseek dir"
        );
    }

    #[test]
    fn existing_legacy_marker_is_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = tmp.path().join(".deepseek").join(ONBOARDED_MARKER_FILE);
        std::fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("mkdir legacy");
        std::fs::write(&legacy, "").expect("seed legacy marker");

        let primary = tmp.path().join(".codewhale").join(ONBOARDED_MARKER_FILE);
        assert_eq!(marker_path_with_home(tmp.path()), primary);
        assert_eq!(
            mark_onboarded_at_home(tmp.path()).expect("mark onboarded"),
            primary
        );
        assert!(primary.is_file());
    }

    #[test]
    fn codewhale_marker_wins_over_legacy_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join(".codewhale").join(ONBOARDED_MARKER_FILE);
        let legacy = tmp.path().join(".deepseek").join(ONBOARDED_MARKER_FILE);
        for marker in [&primary, &legacy] {
            std::fs::create_dir_all(marker.parent().expect("marker parent")).expect("mkdir");
            std::fs::write(marker, "").expect("seed marker");
        }

        assert_eq!(marker_path_with_home(tmp.path()), primary);
    }

    #[test]
    fn validate_rejects_empty_or_whitespace() {
        assert!(matches!(
            validate_api_key_for_onboarding(""),
            ApiKeyValidation::Reject(_)
        ));
        assert!(matches!(
            validate_api_key_for_onboarding("   "),
            ApiKeyValidation::Reject(_)
        ));
        assert!(matches!(
            validate_api_key_for_onboarding("sk live abc"),
            ApiKeyValidation::Reject(_)
        ));
    }

    #[test]
    fn validate_warns_on_short_or_no_hyphen_keys_but_accepts() {
        match validate_api_key_for_onboarding("abc123") {
            ApiKeyValidation::Accept { warning: Some(_) } => {}
            _ => panic!("expected accept-with-warning"),
        }
        match validate_api_key_for_onboarding("abcdefghijklmnop") {
            ApiKeyValidation::Accept { warning: Some(_) } => {}
            _ => panic!("expected accept-with-warning"),
        }
    }

    #[test]
    fn validate_accepts_well_formed_key() {
        assert_eq!(
            validate_api_key_for_onboarding("sk-1234567890abcdef"),
            ApiKeyValidation::Accept { warning: None }
        );
    }
}
