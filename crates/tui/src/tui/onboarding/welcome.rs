//! Welcome screen content for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::localization::MessageId;
use crate::palette;
use crate::tui::app::App;

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let steps = welcome_step_labels(app).join(" -> ");
    let version = app
        .tr(MessageId::OnboardWelcomeVersion)
        .replace("{version}", env!("CARGO_PKG_VERSION"));
    let next_steps = app
        .tr(MessageId::OnboardWelcomeSteps)
        .replace("{steps}", &steps);

    vec![
        Line::from(Span::styled(
            "codewhale",
            Style::default()
                .fg(palette::WHALE_ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            version,
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            app.tr(MessageId::OnboardWelcomeLead).to_string(),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            app.tr(MessageId::OnboardWelcomeSetupBlurb).to_string(),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            next_steps,
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            app.tr(MessageId::OnboardWelcomeDefaults).to_string(),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            app.tr(MessageId::OnboardWelcomeEnter)
                .replace("{key}", "Enter"),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            app.tr(MessageId::OnboardWelcomeExit).to_string(),
            Style::default().fg(palette::TEXT_MUTED),
        )),
    ]
}

fn welcome_step_labels(app: &App) -> Vec<String> {
    let mut steps = Vec::new();
    if app.onboarding_needs_api_key {
        steps.push(app.tr(MessageId::OnboardWelcomeStepApiKey).to_string());
    }
    if !app.trust_mode && super::needs_trust(&app.workspace) {
        steps.push(app.tr(MessageId::OnboardWelcomeStepTrust).to_string());
    }
    steps.push(app.tr(MessageId::OnboardWelcomeStepTips).to_string());
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
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

    fn body(app: &App) -> String {
        lines(app)
            .into_iter()
            .flat_map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.to_string())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn welcome_copy_describes_the_real_first_run_flow() {
        let mut app = test_app();
        app.onboarding_needs_api_key = false;
        app.trust_mode = true;
        let body = body(&app);

        assert!(body.contains("面向 DeepSeek 的本地编码 Agent"));
        assert!(body.contains("只检查必要的 API 密钥与工作区信任"));
        assert!(body.contains("只会显示下面这些页面"));
        assert!(body.contains("接下来：设置提示。"));
        assert!(body.contains("直接用自然语言描述要完成的任务"));
        for retired in ["/setup", "/constitution", "/provider", "/model", "Ctrl+K"] {
            assert!(
                !body.contains(retired),
                "欢迎页不得宣传不可用入口：{retired}"
            );
        }
        assert!(!body.contains("连接 API 密钥"));
        assert!(body.contains("按 Enter 继续。"));
    }

    #[test]
    fn welcome_steps_include_optional_api_key_and_trust_screens() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = test_app();
        app.workspace = tmp.path().to_path_buf();
        app.onboarding_needs_api_key = true;
        app.trust_mode = false;

        let body = body(&app);

        assert!(body.contains("接下来：连接 API 密钥 -> 信任工作区 -> 设置提示。"));
    }

    #[test]
    fn welcome_copy_uses_simplified_chinese_registry() {
        let mut app = test_app();
        app.onboarding_needs_api_key = false;
        app.trust_mode = true;

        let body = body(&app);

        assert!(body.contains("面向 DeepSeek 的本地编码 Agent"));
        assert!(body.contains("接下来：设置提示。"));
        assert!(!body.contains("Press Enter"));
    }
}
