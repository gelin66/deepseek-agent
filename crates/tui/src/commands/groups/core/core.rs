//! Core commands: help, clear, exit, model

use std::fmt::Write;
use std::path::PathBuf;

use crate::config::{
    ApiProvider, COMMON_DEEPSEEK_MODELS, normalize_custom_model_id,
    normalize_model_name_for_provider,
};
use crate::localization::{MessageId, tr};
use crate::route_runtime::resolve_route_candidate;
use crate::tui::app::{App, AppAction, AppMode, ReasoningEffort};
use crate::tui::views::{HelpView, ModalKind, SubAgentsView, subagent_view_agents};

use super::CommandResult;

/// Show help information
pub fn help(app: &mut App, topic: Option<&str>) -> CommandResult {
    if let Some(topic) = topic {
        // Show help for specific command
        if let Some(cmd) = crate::commands::get_command_info(topic) {
            let mut help = format!(
                "{}\n\n  {}\n\n  {} {}",
                cmd.name,
                cmd.description(),
                tr(MessageId::HelpUsageLabel),
                cmd.usage
            );
            if !cmd.aliases.is_empty() {
                let _ = write!(
                    help,
                    "\n  {} {}",
                    tr(MessageId::HelpAliasesLabel),
                    cmd.aliases.join(", ")
                );
            }
            return CommandResult::message(help);
        }
        return CommandResult::error(tr(MessageId::HelpUnknownCommand).replace("{topic}", topic));
    }

    // Show help overlay
    if app.view_stack.top_kind() != Some(ModalKind::Help) {
        app.view_stack.push(HelpView::new());
    }
    CommandResult::ok()
}

/// Exit the application
pub fn exit() -> CommandResult {
    CommandResult::action(AppAction::Quit)
}

/// Switch or view current model. With no argument, open the two-pane
/// picker (Pro/Flash + thinking effort) per #39 — gives users a discoverable
/// way to flip both knobs without memorising the docs.
pub fn model(app: &mut App, model_name: Option<&str>) -> CommandResult {
    if let Some(name) = model_name {
        // Manual Models.dev catalog refresh (#4187). Dispatched async so the
        // TUI event loop is not blocked; failure keeps prior/bundled rows.
        if name.trim().eq_ignore_ascii_case("refresh") {
            return CommandResult::action(AppAction::RefreshModelsDevCatalog);
        }
        if name.trim().eq_ignore_ascii_case("auto") {
            let old_model = app.model_display_label();
            let model_changed = !app.auto_model || app.model != "auto";
            app.auto_model = true;
            app.model = "auto".to_string();
            app.last_effective_model = None;
            app.reasoning_effort = ReasoningEffort::Auto;
            app.last_effective_reasoning_effort = None;
            app.active_route_limits = app.context_window_override_limits();
            app.update_model_compaction_budget();
            if model_changed {
                app.clear_model_scoped_telemetry();
            } else {
                app.session.last_prompt_tokens = None;
                app.session.last_completion_tokens = None;
                app.session.last_output_throughput = None;
            }
            app.provider_models
                .insert(app.api_provider.as_str().to_string(), "auto".to_string());
            let persist_warning =
                provider_model_selection_persist_warning(app.api_provider, "auto");
            let mut message = tr(MessageId::ModelChanged)
                .replace("{old}", &old_model)
                .replace("{new}", "auto");
            if let Some(warning) = persist_warning {
                message.push_str(&warning);
            }
            return CommandResult::with_message_and_action(
                message,
                AppAction::UpdateCompaction(app.compaction_config()),
            );
        }
        let model_id = if app.accepts_custom_model_ids() {
            let Some(model_id) = normalize_custom_model_id(name) else {
                return CommandResult::error(format!(
                    "Invalid model '{name}'. Expected a non-empty model ID."
                ));
            };
            model_id
        } else {
            let Some(model_id) = normalize_model_name_for_provider(app.api_provider, name) else {
                return CommandResult::error(format!(
                    "Invalid model '{name}'. Expected auto or a model for the active provider. Common DeepSeek models: {}",
                    COMMON_DEEPSEEK_MODELS.join(", ")
                ));
            };
            model_id
        };
        let strict_direct_custom_endpoint = app.accepts_custom_model_ids()
            && matches!(
                app.api_provider,
                ApiProvider::Deepseek | ApiProvider::DeepseekCN | ApiProvider::Zai
            );
        let route_limits = if strict_direct_custom_endpoint {
            None
        } else {
            match resolve_route_candidate(
                app.api_provider,
                Some(&model_id),
                None,
                None,
                app.active_context_window_override,
            ) {
                Ok(candidate) => Some(candidate.limits),
                Err(reason) => return CommandResult::error(reason),
            }
        };
        let old_model = app.model_display_label();
        let model_changed = app.auto_model || app.model != model_id;
        app.set_model_selection(model_id.clone());
        if let Some(limits) = route_limits {
            app.set_active_route_limits(limits);
        } else {
            app.active_route_limits = app.context_window_override_limits();
        }
        app.update_model_compaction_budget();
        if model_changed {
            app.clear_model_scoped_telemetry();
        } else {
            app.session.last_prompt_tokens = None;
            app.session.last_completion_tokens = None;
            app.session.last_output_throughput = None;
        }
        app.provider_models
            .insert(app.api_provider.as_str().to_string(), model_id.clone());
        let persist_warning = provider_model_selection_persist_warning(app.api_provider, &model_id);
        let mut message = tr(MessageId::ModelChanged)
            .replace("{old}", &old_model)
            .replace("{new}", &model_id);
        if let Some(warning) = persist_warning {
            message.push_str(&warning);
        }
        CommandResult::with_message_and_action(
            message,
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    } else {
        CommandResult::action(AppAction::OpenModelPicker)
    }
}

fn provider_model_selection_persist_warning(provider: ApiProvider, model: &str) -> Option<String> {
    crate::settings::Settings::persist_provider_model_selection(provider, model)
        .err()
        .map(|err| format!(" (not persisted: {err})"))
}

/// Fetch and list available models from the configured API endpoint.
pub fn models(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::FetchModels)
}

/// List Fleet worker status from the engine.
pub fn subagents(app: &mut App) -> CommandResult {
    if app.view_stack.top_kind() != Some(ModalKind::SubAgents) {
        let agents = subagent_view_agents(app, &app.subagent_cache);
        app.view_stack.push(SubAgentsView::new(agents));
    }
    app.status_message = Some(tr(MessageId::SubagentsFetching).to_string());
    CommandResult::action(AppAction::ListSubAgents)
}

/// Switch to a configured profile.
pub fn profile_switch(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let profile_name = match arg {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            return CommandResult::error(
                "Usage: /profile <name>\n\nSwitch to a named config profile. Profiles are defined in ~/.codewhale/config.toml under [profiles] sections.",
            );
        }
    };
    CommandResult::with_message_and_action(
        format!("Switching to profile '{profile_name}'..."),
        AppAction::SwitchProfile {
            profile: profile_name,
        },
    )
}

pub fn workspace_switch(app: &mut App, arg: Option<&str>) -> CommandResult {
    let Some(raw_path) = arg.map(str::trim).filter(|path| !path.is_empty()) else {
        return CommandResult::message(format!("Current workspace: {}", app.workspace.display()));
    };

    let expanded = match expand_workspace_path(raw_path) {
        Ok(path) => path,
        Err(message) => return CommandResult::error(message),
    };
    let candidate = if expanded.is_absolute() {
        expanded
    } else {
        app.workspace.join(expanded)
    };

    if !candidate.exists() {
        return CommandResult::error(format!("Workspace does not exist: {}", candidate.display()));
    }
    if !candidate.is_dir() {
        return CommandResult::error(format!(
            "Workspace is not a directory: {}",
            candidate.display()
        ));
    }

    let workspace = candidate.canonicalize().unwrap_or(candidate);
    CommandResult::with_message_and_action(
        format!("Switching workspace to {}...", workspace.display()),
        AppAction::SwitchWorkspace { workspace },
    )
}

fn expand_workspace_path(path: &str) -> Result<PathBuf, String> {
    if path == "~" {
        return dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home =
            dirs::home_dir().ok_or_else(|| "Could not resolve home directory".to_string())?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(path))
}

struct ProviderLinkInfo {
    key_url: Option<&'static str>,
    docs_url: &'static str,
    note: &'static str,
}

fn provider_link_info(provider_id: &str) -> ProviderLinkInfo {
    match provider_id {
        "deepseek" => ProviderLinkInfo {
            key_url: Some("https://platform.deepseek.com/api_keys"),
            docs_url: "https://api-docs.deepseek.com/",
            note: "Create an API key in the DeepSeek platform console.",
        },
        "nvidia-nim" => ProviderLinkInfo {
            key_url: Some("https://build.nvidia.com/settings/api-keys"),
            docs_url: "https://build.nvidia.com/explore/discover",
            note: "NVIDIA NIM keys are managed from the NVIDIA build console.",
        },
        "openai" => ProviderLinkInfo {
            key_url: Some("https://platform.openai.com/api-keys"),
            docs_url: "https://platform.openai.com/docs/api-reference",
            note: "Use this for OpenAI or compatible endpoints that share OpenAI-style auth.",
        },
        "atlascloud" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://atlascloud.ai/docs/en/api-keys",
            note: "Atlas Cloud documents API key creation in its API Keys guide.",
        },
        "wanjie-ark" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://platform.lingyiwanwu.com/docs",
            note: "Use the Wanjie/01.AI platform console for provider credentials.",
        },
        "volcengine" => ProviderLinkInfo {
            key_url: Some("https://console.volcengine.com/ark/apiKey"),
            docs_url: "https://www.volcengine.com/docs/82379/1541594",
            note: "Volcengine Ark API keys are managed in the Ark console.",
        },
        "openrouter" => ProviderLinkInfo {
            key_url: Some("https://openrouter.ai/settings/keys"),
            docs_url: "https://openrouter.ai/docs/api/reference/authentication",
            note: "OpenRouter keys can include app credit limits and model routing controls.",
        },
        "xiaomi-mimo" => ProviderLinkInfo {
            key_url: Some("https://platform.xiaomimimo.com/token-plan"),
            docs_url: "https://mimo.mi.com/docs/en-US/tokenplan/Token%20Plan/subscription",
            note: "Token Plan keys use the base URL shown on the Xiaomi MiMo Token Plan page.",
        },
        "novita" => ProviderLinkInfo {
            key_url: Some("https://novita.ai/en/settings/key-management"),
            docs_url: "https://novita.ai/docs/guides/quickstart",
            note: "Novita keys are managed from Key Management in account settings.",
        },
        "fireworks" => ProviderLinkInfo {
            key_url: Some("https://fireworks.ai/api-keys"),
            docs_url: "https://docs.fireworks.ai/getting-started/quickstart",
            note: "Create a Fireworks API key before exporting FIREWORKS_API_KEY.",
        },
        "siliconflow" => ProviderLinkInfo {
            key_url: Some("https://cloud.siliconflow.com/account/ak"),
            docs_url: "https://docs.siliconflow.com/en/userguide/quickstart",
            note: "Use the global SiliconFlow console unless your route is the China endpoint.",
        },
        "siliconflow-CN" => ProviderLinkInfo {
            key_url: Some("https://cloud.siliconflow.cn/account/ak"),
            docs_url: "https://docs.siliconflow.cn/en/userguide/quickstart",
            note: "Use the China SiliconFlow console for the China endpoint.",
        },
        "arcee" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.arcee.ai/other/create-your-first-api-key",
            note: "Arcee documents key creation from the platform API Keys page.",
        },
        "moonshot" => ProviderLinkInfo {
            key_url: Some("https://platform.kimi.ai/console/api-keys"),
            docs_url: "https://platform.kimi.ai/docs/api/overview",
            note: "Moonshot/Kimi keys are managed in the Kimi Open Platform console.",
        },
        "sglang" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.sglang.ai/",
            note: "Self-hosted SGLang usually needs a local base URL, not a hosted token.",
        },
        "vllm" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.vllm.ai/en/stable/serving/openai_compatible_server/",
            note: "Self-hosted vLLM usually needs a local base URL, not a hosted token.",
        },
        "ollama" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.ollama.com/api",
            note: "Local Ollama does not require an API key by default.",
        },
        "huggingface" => ProviderLinkInfo {
            key_url: Some("https://huggingface.co/settings/tokens"),
            docs_url: "https://huggingface.co/docs/hub/en/security-tokens",
            note: "Use a scoped Hugging Face access token.",
        },
        "together" => ProviderLinkInfo {
            key_url: Some("https://api.together.ai/settings/api-keys"),
            docs_url: "https://docs.together.ai/docs/api-keys-authentication",
            note: "Together API keys are project-scoped.",
        },
        "openai-codex" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://developers.openai.com/codex/",
            note: "This route uses Codex/ChatGPT auth instead of a normal provider API key.",
        },
        "anthropic" => ProviderLinkInfo {
            key_url: Some("https://console.anthropic.com/settings/keys"),
            docs_url: "https://docs.anthropic.com/en/api/overview",
            note: "Create Claude API keys from the Anthropic Console.",
        },
        "zai" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://docs.z.ai/api-reference/introduction",
            note: "Create or manage Z.ai API keys from the API Keys page linked in the docs.",
        },
        "stepfun" => ProviderLinkInfo {
            key_url: Some("https://platform.stepfun.ai/"),
            docs_url: "https://platform.stepfun.ai/docs/en/quickstart/overview",
            note: "Open Account Management > Interface Keys in the StepFun console.",
        },
        "minimax" => ProviderLinkInfo {
            key_url: Some(
                "https://platform.minimax.io/user-center/basic-information/interface-key",
            ),
            docs_url: "https://platform.minimax.io/docs/api-reference/api-overview",
            note: "MiniMax has separate pay-as-you-go API keys and Token Plan subscription keys.",
        },
        "deepinfra" => ProviderLinkInfo {
            key_url: Some("https://deepinfra.com/dash/api_keys"),
            docs_url: "https://docs.deepinfra.com/quickstart",
            note: "Create DeepInfra API keys from the dashboard.",
        },
        "qianfan" => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://cloud.baidu.com/doc/qianfan/index.html",
            note: "Create Baidu Qianfan API keys from the Qianfan console.",
        },
        _ => ProviderLinkInfo {
            key_url: None,
            docs_url: "https://codewhale.net/en/docs",
            note: "Use the provider console for credentials, then configure the matching env var.",
        },
    }
}

/// Show provider dashboard, token, and docs links.
pub fn deepseek_links(app: &mut App) -> CommandResult {
    let active_provider = app.api_provider.as_str();
    let mut message = format!(
        "{}\n─────────────────────────────\n",
        tr(MessageId::LinksTitle)
    );

    for provider in codewhale_config::provider::providers_sorted_for_display() {
        let links = provider_link_info(provider.id());
        let active_marker = if provider.id() == active_provider {
            " <- current"
        } else {
            ""
        };
        let _ = writeln!(
            message,
            "\n{} ({}){}",
            provider.display_name(),
            provider.id(),
            active_marker
        );
        if let Some(key_url) = links.key_url {
            let _ = writeln!(message, "{} `{}`", tr(MessageId::LinksDashboard), key_url);
        } else {
            let _ = writeln!(message, "{} {}", tr(MessageId::LinksDashboard), links.note);
        }
        let _ = writeln!(
            message,
            "{}      `{}`",
            tr(MessageId::LinksDocs),
            links.docs_url
        );
        let env_vars = provider.env_vars();
        if env_vars.is_empty() {
            let _ = writeln!(message, "Env: none");
        } else {
            let _ = writeln!(message, "Env: {}", env_vars.join(", "));
        }
    }

    let _ = writeln!(message, "\n{}", tr(MessageId::LinksTip));
    CommandResult::message(message)
}

/// Show home dashboard with stats and quick actions
pub fn home_dashboard(app: &mut App) -> CommandResult {
    let mut stats = String::new();

    // Basic info
    let _ = writeln!(stats, "{}", tr(MessageId::HomeDashboardTitle));
    let _ = writeln!(stats, "============================================");

    // Model & mode
    let _ = writeln!(stats, "{}      {}", tr(MessageId::HomeModel), app.model);
    let _ = writeln!(
        stats,
        "{}       {}",
        tr(MessageId::HomeMode),
        app.mode.label()
    );
    let _ = writeln!(
        stats,
        "{}  {}",
        tr(MessageId::HomeWorkspace),
        app.workspace.display()
    );

    // Session stats
    let history_count = app.history.len();
    let total_tokens = app.session.total_conversation_tokens;
    let queued_messages = app.queued_messages.len();
    let _ = writeln!(
        stats,
        "{}    {} messages",
        tr(MessageId::HomeHistory),
        history_count
    );
    let _ = writeln!(
        stats,
        "{}     {} (session)",
        tr(MessageId::HomeTokens),
        total_tokens
    );
    if queued_messages > 0 {
        let _ = writeln!(
            stats,
            "{}     {} messages",
            tr(MessageId::HomeQueued),
            queued_messages
        );
    }

    // Fleet role workers
    let subagent_count = app.subagent_cache.len();
    if subagent_count > 0 {
        let _ = writeln!(
            stats,
            "{} {} active",
            tr(MessageId::HomeSubagents),
            subagent_count
        );
    }

    // Active skill
    if let Some(skill) = &app.active_skill {
        let _ = writeln!(
            stats,
            "{}      {} (active)",
            tr(MessageId::HomeSkill),
            skill
        );
    }

    // Quick actions section
    let _ = writeln!(stats, "\n{}", tr(MessageId::HomeQuickActions));
    let _ = writeln!(stats, "--------------------------------------------");
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickLinks));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickSkills));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickConfig));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickSettings));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickModel));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickSubagents));
    let _ = writeln!(stats, "{}", tr(MessageId::HomeQuickHelp));

    // Mode-specific tips
    let _ = writeln!(stats, "\n{}", tr(MessageId::HomeModeTips));
    let _ = writeln!(stats, "--------------------------------------------");
    match app.mode {
        AppMode::Agent | AppMode::Auto => {
            let _ = writeln!(stats, "{}", tr(MessageId::HomeAgentModeTip));
            let _ = writeln!(stats, "{}", tr(MessageId::HomeAgentModeReviewTip));
            let _ = writeln!(stats, "{}", tr(MessageId::HomeAgentModeYoloTip));
        }
        AppMode::Yolo => {
            // Compatibility residual: YOLO is invisible Act + Full Access.
            let _ = writeln!(stats, "{}", tr(MessageId::HomeYoloModeTip));
            let _ = writeln!(stats, "{}", tr(MessageId::HomeYoloModeCaution));
        }
        AppMode::Operate => {
            let _ = writeln!(stats, "{}", tr(MessageId::HomeOperateModeTip));
            let _ = writeln!(stats, "{}", tr(MessageId::HomeOperateModeFleetTip));
        }
        AppMode::Plan => {
            let _ = writeln!(stats, "{}", tr(MessageId::HomePlanModeTip));
            let _ = writeln!(stats, "{}", tr(MessageId::HomePlanModeChecklistTip));
        }
    }

    CommandResult::message(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, AppMode, TuiOptions};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use tempfile::{TempDir, tempdir};

    struct SettingsPathGuard {
        _tmp: TempDir,
        previous: Option<OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl SettingsPathGuard {
        fn new() -> Self {
            let lock = crate::test_support::lock_test_env();
            let tmp = TempDir::new().expect("settings tempdir");
            let config_path = tmp.path().join(".deepseek").join("config.toml");
            std::fs::create_dir_all(config_path.parent().expect("config parent"))
                .expect("config dir");
            let previous = std::env::var_os("DEEPSEEK_CONFIG_PATH");
            // Safety: test-only environment mutation guarded by a global mutex.
            unsafe {
                std::env::set_var("DEEPSEEK_CONFIG_PATH", &config_path);
            }
            Self {
                _tmp: tmp,
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for SettingsPathGuard {
        fn drop(&mut self) {
            // Safety: test-only environment mutation guarded by a global mutex.
            unsafe {
                if let Some(previous) = self.previous.take() {
                    std::env::set_var("DEEPSEEK_CONFIG_PATH", previous);
                } else {
                    std::env::remove_var("DEEPSEEK_CONFIG_PATH");
                }
            }
        }
    }

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("/tmp/test-workspace"),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("/tmp/test-skills"),
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
        let mut app = App::new(options, &Config::default());
        app.api_provider = crate::config::ApiProvider::Deepseek;
        app.model = "deepseek-v4-pro".to_string();
        app.auto_model = false;
        app.model_ids_passthrough = false;
        app
    }

    #[test]
    fn test_help_unknown_command() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("nonexistent"));
        assert!(result.message.is_some());
        assert!(result.message.unwrap().contains("未知命令"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_help_known_command() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("queue"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("queue"));
        assert!(msg.contains("查看或编辑已排队的消息"));
        assert!(msg.contains("用法： /queue [list|send <n>|edit <n>|drop <n>|clear]"));
    }

    #[test]
    fn test_help_config_topic_uses_interactive_editor_text() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("config"));
        let msg = result.message.expect("help topic should return message");
        assert!(msg.contains("config"));
        assert!(msg.contains("查看与修改设置"));
        assert!(msg.contains("用法： /config"));
    }

    #[test]
    fn test_help_links_topic_shows_aliases() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("links"));
        let msg = result.message.expect("help topic should return message");
        assert!(msg.contains("links"));
        assert!(msg.contains("显示服务商令牌、控制台与文档链接"));
        assert!(msg.contains("用法： /links"));
        assert!(msg.contains("别名： dashboard, api"));
    }

    #[test]
    fn test_help_memory_topic_shows_usage_and_description() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("memory"));
        let msg = result.message.expect("help topic should return message");
        assert!(msg.contains("memory"));
        assert!(msg.contains("持久用户记忆文件"));
        assert!(msg.contains("用法： /memory [show|path|clear|edit|help]"));
    }

    #[test]
    fn test_help_pushes_overlay() {
        let mut app = create_test_app();
        assert_ne!(app.view_stack.top_kind(), Some(ModalKind::Help));
        let result = help(&mut app, None);
        assert_eq!(result.message, None);
        assert_eq!(result.action, None);
        assert_eq!(app.view_stack.top_kind(), Some(ModalKind::Help));
    }

    #[test]
    fn test_help_does_not_duplicate_overlay() {
        let mut app = create_test_app();
        help(&mut app, None);
        let initial_kind = app.view_stack.top_kind();
        help(&mut app, None);
        assert_eq!(app.view_stack.top_kind(), initial_kind);
    }

    #[test]
    fn test_exit_returns_quit_action() {
        let result = exit();
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::Quit)));
    }

    #[test]
    fn workspace_without_arg_shows_current_workspace() {
        let mut app = create_test_app();
        let result = workspace_switch(&mut app, None);
        let msg = result.message.expect("workspace should be shown");
        assert!(msg.contains("Current workspace:"));
        assert!(msg.contains("/tmp/test-workspace"));
        assert!(result.action.is_none());
    }

    #[test]
    fn workspace_existing_absolute_dir_returns_switch_action() {
        let mut app = create_test_app();
        let dir = tempdir().expect("temp dir");
        let result = workspace_switch(&mut app, Some(dir.path().to_str().unwrap()));
        assert!(matches!(
            result.action,
            Some(AppAction::SwitchWorkspace { workspace }) if workspace == dir.path().canonicalize().unwrap()
        ));
    }

    #[test]
    fn workspace_relative_dir_resolves_from_current_workspace() {
        let root = tempdir().expect("temp dir");
        let child = root.path().join("child");
        std::fs::create_dir(&child).expect("child dir");
        let mut app = create_test_app();
        app.workspace = root.path().to_path_buf();

        let result = workspace_switch(&mut app, Some("child"));
        assert!(matches!(
            result.action,
            Some(AppAction::SwitchWorkspace { workspace }) if workspace == child.canonicalize().unwrap()
        ));
    }

    #[test]
    fn workspace_rejects_missing_path() {
        let mut app = create_test_app();
        let result = workspace_switch(&mut app, Some("definitely-missing"));
        assert!(result.is_error);
        assert!(result.message.unwrap().contains("does not exist"));
    }

    #[test]
    fn workspace_rejects_file_path() {
        let root = tempdir().expect("temp dir");
        let file = root.path().join("file.txt");
        std::fs::write(&file, "not a directory").expect("test file");
        let mut app = create_test_app();

        let result = workspace_switch(&mut app, Some(file.to_str().unwrap()));
        assert!(result.is_error);
        assert!(result.message.unwrap().contains("not a directory"));
    }

    #[test]
    fn test_model_change_updates_state() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        let old_model = app.model.clone();
        let result = model(&mut app, Some("deepseek-v4-flash"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains(&old_model));
        assert!(msg.contains("deepseek-v4-flash"));
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
        assert_eq!(app.model, "deepseek-v4-flash");
        assert_eq!(app.session.last_prompt_tokens, None);
        assert_eq!(app.session.last_completion_tokens, None);
    }

    #[test]
    fn model_command_persists_active_provider_model_scoped() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();

        let result = model(&mut app, Some("deepseek-v4-flash"));

        assert!(result.message.is_some());
        assert_eq!(
            app.provider_models.get("deepseek").map(String::as_str),
            Some("deepseek-v4-flash")
        );
        let settings = crate::settings::Settings::load().expect("load settings");
        // #3227: `/model` is session-local. It records the model under the
        // provider-scoped entry only; it must NOT rewrite the shared global
        // `default_provider`/`default_model` that other terminals read on
        // startup.
        assert_eq!(
            settings
                .provider_models
                .as_ref()
                .and_then(|models| models.get("deepseek"))
                .map(String::as_str),
            Some("deepseek-v4-flash")
        );
        assert_eq!(settings.default_provider.as_deref(), None);
        assert_eq!(settings.default_model.as_deref(), None);
    }

    #[test]
    fn model_command_does_not_mutate_shared_default_provider() {
        // Regression for #3227: a `/model` change on a non-default provider
        // must not drag the global `default_provider` onto it. Here the saved
        // default is DeepSeek; selecting a model while the session is on Z.ai
        // changes only Z.ai's scoped model.
        let _settings = SettingsPathGuard::new();
        {
            let seed = crate::settings::Settings {
                default_provider: Some("deepseek".to_string()),
                ..Default::default()
            };
            seed.save().expect("seed settings");
        }
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::Zai;
        app.model_ids_passthrough = false;
        app.model = crate::config::DEFAULT_ZAI_MODEL.to_string();
        app.auto_model = false;

        let result = model(&mut app, Some("GLM-5.2"));
        assert!(result.message.is_some(), "expected a model-changed message");
        assert!(!result.is_error, "GLM-5.2 is valid on Z.ai");

        let settings = crate::settings::Settings::load().expect("load settings");
        // The shared default provider is untouched.
        assert_eq!(settings.default_provider.as_deref(), Some("deepseek"));
        // Only Z.ai's scoped entry changed.
        assert_eq!(
            settings
                .provider_models
                .as_ref()
                .and_then(|models| models.get("zai"))
                .map(String::as_str),
            Some("GLM-5.2")
        );
    }

    #[test]
    fn two_sessions_keep_independent_provider_model_routes() {
        // #3227: two App instances sharing one settings/config path. A is on
        // Z.ai/GLM; B switches to DeepSeek and picks a DeepSeek model. B must
        // build a DeepSeek route (not Z.ai + a DeepSeek model), A must stay on
        // Z.ai/GLM, and neither session's `/model` may flip the shared global
        // default provider out from under the other.
        let _settings = SettingsPathGuard::new();

        // Terminal A: Z.ai / GLM.
        let mut app_a = create_test_app();
        app_a.api_provider = crate::config::ApiProvider::Zai;
        app_a.model_ids_passthrough = false;
        app_a.model = crate::config::DEFAULT_ZAI_MODEL.to_string();
        app_a.auto_model = false;
        let result_a = model(&mut app_a, Some("GLM-5.2"));
        assert!(!result_a.is_error, "GLM-5.2 is valid on Z.ai");
        assert_eq!(app_a.api_provider, crate::config::ApiProvider::Zai);
        assert_eq!(app_a.model, "GLM-5.2");

        // Terminal B: DeepSeek / deepseek-v4-flash.
        let mut app_b = create_test_app();
        app_b.api_provider = crate::config::ApiProvider::Deepseek;
        app_b.model_ids_passthrough = false;
        app_b.model = "deepseek-v4-pro".to_string();
        app_b.auto_model = false;
        let result_b = model(&mut app_b, Some("deepseek-v4-flash"));
        assert!(!result_b.is_error, "deepseek-v4-flash is valid on DeepSeek");

        // B's route is a coherent DeepSeek route — never Z.ai + a DeepSeek model.
        assert_eq!(app_b.api_provider, crate::config::ApiProvider::Deepseek);
        assert_eq!(app_b.model, "deepseek-v4-flash");

        // A is untouched by B's selection — still Z.ai / GLM.
        assert_eq!(app_a.api_provider, crate::config::ApiProvider::Zai);
        assert_eq!(app_a.model, "GLM-5.2");

        // Shared settings: per-provider scoped models recorded for both, and
        // the global default provider was never flipped by either `/model`.
        let settings = crate::settings::Settings::load().expect("load settings");
        assert_eq!(settings.default_provider.as_deref(), None);
        let provider_models = settings.provider_models.expect("provider_models");
        assert_eq!(
            provider_models.get("zai").map(String::as_str),
            Some("GLM-5.2")
        );
        assert_eq!(
            provider_models.get("deepseek").map(String::as_str),
            Some("deepseek-v4-flash")
        );
    }

    #[test]
    fn model_command_rejects_model_foreign_to_active_provider() {
        // #3227: a DeepSeek model id requested while the session is on Z.ai is
        // rejected locally with a precise diagnostic, before any network call.
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::Zai;
        app.model_ids_passthrough = false;
        app.model = crate::config::DEFAULT_ZAI_MODEL.to_string();
        app.auto_model = false;
        app.provider_models.clear();

        let result = model(&mut app, Some("deepseek-v4-pro"));

        assert!(result.is_error, "expected a local rejection");
        let msg = result.message.expect("error message");
        assert!(msg.contains("deepseek-v4-pro"), "names the model: {msg}");
        assert!(msg.contains("zai"), "names the provider: {msg}");
        // The session route is unchanged — still Z.ai / GLM.
        assert_eq!(app.api_provider, crate::config::ApiProvider::Zai);
        assert_eq!(app.model, crate::config::DEFAULT_ZAI_MODEL);
    }

    #[test]
    fn test_model_auto_enables_auto_thinking() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        app.reasoning_effort = ReasoningEffort::Off;

        let result = model(&mut app, Some("auto"));

        assert!(result.message.is_some());
        assert!(app.auto_model);
        assert_eq!(app.model, "auto");
        assert_eq!(app.reasoning_effort, ReasoningEffort::Auto);
        assert!(app.last_effective_model.is_none());
        assert!(app.last_effective_reasoning_effort.is_none());
    }

    #[test]
    fn test_model_change_accepts_future_deepseek_model() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        let result = model(&mut app, Some("deepseek-v4"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("deepseek-v4"));
        assert_eq!(app.model, "deepseek-v4");
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
    }

    #[test]
    fn test_model_change_accepts_custom_id_for_openai_compatible_provider() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::Openai;
        app.model_ids_passthrough = true;

        let result = model(&mut app, Some("opencode-go/glm-5.1"));

        assert!(result.message.is_some());
        assert_eq!(app.model, "opencode-go/glm-5.1");
        assert!(!app.auto_model);
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
    }

    #[test]
    fn test_model_change_accepts_custom_id_for_custom_base_url() {
        let _settings = SettingsPathGuard::new();
        let mut app = create_test_app();
        app.model_ids_passthrough = true;

        let result = model(&mut app, Some("opencode-go/kimi-k2.6"));

        assert!(result.message.is_some());
        assert_eq!(app.model, "opencode-go/kimi-k2.6");
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
    }

    #[test]
    fn test_model_change_rejects_invalid_model() {
        let mut app = create_test_app();
        let result = model(&mut app, Some("gpt-4"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Invalid model"));
        assert!(msg.contains("active provider"));
        assert!(msg.contains("deepseek-v4-pro"));
        assert!(msg.contains("deepseek-v4-flash"));
        assert!(result.action.is_none());
    }

    #[test]
    fn model_command_rejects_saved_model_from_other_provider() {
        let mut app = create_test_app();
        app.api_provider = crate::config::ApiProvider::Deepseek;
        app.provider_models
            .insert("moonshot".to_string(), "kimi-k2.6".to_string());

        let result = model(&mut app, Some("kimi-k2.6"));

        let message = result.message.expect("invalid model message");
        assert!(message.contains("Invalid model"));
        assert!(message.contains("active provider"));
        assert!(result.action.is_none());
        assert_eq!(app.api_provider, crate::config::ApiProvider::Deepseek);
        assert_eq!(app.model, "deepseek-v4-pro");
    }

    #[test]
    fn test_model_without_args_opens_picker() {
        let mut app = create_test_app();
        let result = model(&mut app, None);
        assert_eq!(result.message, None);
        assert_eq!(result.action, Some(AppAction::OpenModelPicker));
    }

    #[test]
    fn test_models_triggers_fetch_action() {
        let mut app = create_test_app();
        let result = models(&mut app);
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::FetchModels)));
    }

    #[test]
    fn model_refresh_dispatches_models_dev_catalog_action() {
        let mut app = create_test_app();
        let result = model(&mut app, Some("refresh"));
        assert!(result.message.is_none());
        assert!(matches!(
            result.action,
            Some(AppAction::RefreshModelsDevCatalog)
        ));
    }

    #[test]
    fn test_subagents_pushes_view_and_sets_status() {
        let mut app = create_test_app();
        let result = subagents(&mut app);
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::ListSubAgents)));
        assert_eq!(app.view_stack.top_kind(), Some(ModalKind::SubAgents));
        assert_eq!(
            app.status_message,
            Some("正在获取 Fleet 工作器状态...".to_string())
        );
    }

    #[test]
    fn test_deepseek_links() {
        let mut app = create_test_app();
        let result = deepseek_links(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("服务商链接"));
        assert!(msg.contains("DeepSeek (deepseek) <- current"));
        assert!(msg.contains("https://platform.deepseek.com/api_keys"));
        assert!(msg.contains("Xiaomi MiMo (xiaomi-mimo)"));
        assert!(msg.contains("https://platform.xiaomimimo.com/token-plan"));
        assert!(msg.contains("Baidu Qianfan (qianfan)"));
        assert!(msg.contains("https://cloud.baidu.com/doc/qianfan/index.html"));
        assert!(msg.contains("OPENAI_API_KEY"));
        assert!(msg.contains("XIAOMI_MIMO_TOKEN_PLAN_API_KEY"));
        assert!(!msg.contains("https://codewhale.dev/docs/providers"));
        assert!(result.action.is_none());
    }

    #[test]
    fn provider_links_emit_urls_as_inline_code_for_narrow_transcripts() {
        let mut app = create_test_app();
        let result = deepseek_links(&mut app);
        let msg = result.message.expect("links should return a message");

        assert!(msg.contains("`https://platform.openai.com/api-keys`"));
        assert!(
            msg.contains(
                "`https://platform.minimax.io/user-center/basic-information/interface-key`"
            )
        );

        for line in msg.lines().filter(|line| line.contains("http")) {
            let Some(url_start) = line.find("http") else {
                continue;
            };
            assert!(
                line[..url_start].ends_with('`') && line[url_start..].contains('`'),
                "provider URL should be inline-code wrapped so narrow TUI renders do not emit oversized OSC8 link payloads: {line}"
            );
        }
    }

    #[test]
    fn provider_link_fallback_uses_current_codewhale_docs() {
        let links = provider_link_info("unknown-provider");

        assert_eq!(links.docs_url, "https://codewhale.net/en/docs");
        assert_eq!(links.key_url, None);
    }

    #[test]
    fn test_home_dashboard_includes_all_sections() {
        let mut app = create_test_app();
        app.session.total_conversation_tokens = 1234;
        let result = home_dashboard(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("codewhale 主面板"));
        assert!(msg.contains("模型："));
        assert!(msg.contains("模式："));
        assert!(msg.contains("工作区："));
        assert!(msg.contains("历史："));
        assert!(msg.contains("令牌："));
        assert!(msg.contains("快捷操作"));
        assert!(msg.contains("模式提示"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_home_dashboard_shows_queued_when_present() {
        let mut app = create_test_app();
        app.queued_messages
            .push_back(crate::tui::app::QueuedMessage::new(
                "test".to_string(),
                None,
            ));
        let result = home_dashboard(&mut app);
        let msg = result.message.unwrap();
        assert!(msg.contains("队列："));
    }

    #[test]
    fn test_home_dashboard_mode_tips_for_each_mode() {
        let modes = [
            AppMode::Agent,
            AppMode::Auto,
            AppMode::Yolo,
            AppMode::Plan,
            AppMode::Operate,
        ];
        for mode in modes {
            let mut app = create_test_app();
            app.mode = mode;
            let result = home_dashboard(&mut app);
            let msg = result.message.unwrap();
            assert!(msg.contains("模式提示"), "模式 {mode:?} 缺少提示");
        }
    }

    #[test]
    fn test_home_dashboard_quick_actions_reflect_links_and_config_and_hide_removed_commands() {
        let mut app = create_test_app();
        let result = home_dashboard(&mut app);
        let msg = result
            .message
            .expect("home dashboard should return message");
        assert!(msg.contains("/links      - 控制台与 API 链接"));
        assert!(msg.contains("/config      - 查看与修改设置"));
        assert!(
            !msg.lines()
                .any(|line| line.trim_start().starts_with("/set "))
        );
        assert!(!msg.contains("/codewhale"));
    }

    #[test]
    fn home_dashboard_localizes_in_zh_hans() {
        let mut app = create_test_app();
        let result = home_dashboard(&mut app);
        let msg = result
            .message
            .expect("home dashboard should return message");
        assert!(msg.contains("主面板"), "missing zh-Hans title:\n{msg}");
        assert!(msg.contains("模型"), "missing zh-Hans model label:\n{msg}");
        assert!(
            msg.contains("快捷操作"),
            "missing zh-Hans quick actions:\n{msg}"
        );
        assert!(
            msg.contains("模式提示"),
            "missing zh-Hans mode tips:\n{msg}"
        );
    }
}
