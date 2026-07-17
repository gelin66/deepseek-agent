//! Diagnostic prompt source map for context pressure reports.
//!
//! The report is intentionally approximate for v0.8.59. It uses the same
//! conservative token heuristic as compaction and describes the runtime sources
//! CodeWhale already tracks, without claiming provider-tokenizer parity.

use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use codewhale_config::route::RouteLimits;

use crate::compaction::estimate_text_tokens_conservative;
use crate::config::{ApiProvider, Config, provider_capability};
use crate::prompts::{COMPACT_TEMPLATE, Personality};
use crate::route_budget::route_context_window_tokens;

#[derive(Debug, Clone, Serialize)]
pub struct PromptSourceMap {
    pub entries: Vec<SourceEntry>,
    pub total_estimated_tokens: usize,
    pub active_context_estimated_tokens: usize,
    pub context_window_tokens: Option<u32>,
    pub budget_used_percent: Option<f64>,
    pub generated_at: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceEntry {
    pub source_kind: SourceKind,
    pub label: String,
    pub source_path: Option<String>,
    pub activation_reason: ActivationReason,
    pub estimated_tokens: usize,
    pub counting_confidence: CountingConfidence,
    pub authority_tier: Option<u8>,
    pub truncation_reason: Option<String>,
}

impl SourceEntry {
    fn text(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        activation_reason: ActivationReason,
        text: &str,
        counting_confidence: CountingConfidence,
        authority_tier: Option<u8>,
    ) -> Self {
        Self::estimate(
            source_kind,
            label,
            source_path,
            activation_reason,
            estimate_text_tokens_conservative(text),
            counting_confidence,
            authority_tier,
        )
    }

    fn estimate(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        activation_reason: ActivationReason,
        estimated_tokens: usize,
        counting_confidence: CountingConfidence,
        authority_tier: Option<u8>,
    ) -> Self {
        Self {
            source_kind,
            label: label.into(),
            source_path,
            activation_reason,
            estimated_tokens,
            counting_confidence,
            authority_tier,
            truncation_reason: None,
        }
    }

    fn omitted(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        authority_tier: Option<u8>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            source_kind,
            label: label.into(),
            source_path,
            activation_reason: ActivationReason::Omitted,
            estimated_tokens: 0,
            counting_confidence: CountingConfidence::High,
            authority_tier,
            truncation_reason: Some(reason.into()),
        }
    }

    fn diagnostic(
        source_kind: SourceKind,
        label: impl Into<String>,
        source_path: Option<String>,
        activation_reason: ActivationReason,
        detail: impl Into<String>,
        estimated_tokens: usize,
        authority_tier: Option<u8>,
    ) -> Self {
        Self {
            source_kind,
            label: label.into(),
            source_path,
            activation_reason,
            estimated_tokens,
            counting_confidence: CountingConfidence::High,
            authority_tier,
            truncation_reason: Some(detail.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Constitution,
    RepoConstitution,
    ProjectContext,
    ProjectContextWarning,
    ProjectContextPack,
    SkillsBlock,
    ContextManagement,
    CompactionRelayTemplate,
    RuntimePolicy,
    UserMemory,
    HandoffRelay,
    ModelProviderFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationReason {
    AlwaysOn,
    FilePresent,
    ConfigEnabled,
    RuntimeState,
    Omitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountingConfidence {
    High,
    Approximate,
}

struct ReportBuilder {
    entries: Vec<SourceEntry>,
}

impl ReportBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn push(&mut self, entry: SourceEntry) {
        self.entries.push(entry);
    }

    fn finish(
        self,
        provider: ApiProvider,
        model: &str,
        route_limits: Option<RouteLimits>,
        active_context_estimated_tokens: usize,
        note: impl Into<String>,
    ) -> PromptSourceMap {
        let total_estimated_tokens = self
            .entries
            .iter()
            .map(|entry| entry.estimated_tokens)
            .sum();
        // Overlay the resolved route's context window when known, falling back
        // to the provider+model capability matrix (route_context_window_tokens
        // always yields a concrete value, so this is never None at runtime).
        let context_window_tokens =
            Some(route_context_window_tokens(provider, model, route_limits));
        let budget_used_percent = context_window_tokens.map(|window| {
            ((active_context_estimated_tokens as f64 / f64::from(window)) * 100.0).clamp(0.0, 100.0)
        });
        PromptSourceMap {
            entries: self.entries,
            total_estimated_tokens,
            active_context_estimated_tokens,
            context_window_tokens,
            budget_used_percent,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            note: note.into(),
        }
    }
}

pub fn build_headless_context_report(config: &Config, workspace: &Path) -> PromptSourceMap {
    let model = config.default_model();
    let global_skills_dir = config.skills_dir();
    let selected_skills_dir =
        crate::tui::app::resolve_skills_dir(workspace, &global_skills_dir, config);
    let mut builder = base_source_entries(&model, workspace, Some(&selected_skills_dir));
    let memory_path = config.memory_path();
    let memory_enabled = config.memory_enabled();
    let moraine_fallback = config.moraine_fallback();

    // TODO(v0.8.71): remove legacy memory push/inject when Moraine recall stable; see #3490, #3495
    if let Some(memory_block) =
        crate::memory::compose_block(memory_enabled && !moraine_fallback, &memory_path)
    {
        builder.push(SourceEntry::text(
            SourceKind::UserMemory,
            "User memory",
            Some(memory_path.display().to_string()),
            ActivationReason::ConfigEnabled,
            &memory_block,
            CountingConfidence::High,
            Some(6),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::UserMemory,
            "User memory",
            Some(memory_path.display().to_string()),
            Some(6),
            if moraine_fallback && memory_enabled {
                "disabled by moraine_fallback"
            } else {
                "disabled, missing, or empty"
            },
        ));
    }

    builder.push(SourceEntry::text(
        SourceKind::ModelProviderFact,
        format!("Provider facts ({})", config.api_provider().as_str()),
        None,
        ActivationReason::RuntimeState,
        &format!(
            "provider: {}\nmodel: {}\ncontext_window: {}",
            config.api_provider().as_str(),
            model,
            // Route limits aren't resolved in the headless doctor path, so report
            // the provider+model capability window (route overlay is unavailable).
            provider_capability(config.api_provider(), &model).context_window
        ),
        CountingConfidence::Approximate,
        None,
    ));

    let active_context_estimated_tokens = builder
        .entries
        .iter()
        .map(|entry| entry.estimated_tokens)
        .sum();
    builder.finish(
        config.api_provider(),
        &model,
        // Route limits aren't resolved in the headless doctor path.
        None,
        active_context_estimated_tokens,
        "Headless diagnostic source map. Conversation, tool results, and live TUI state are unavailable in doctor mode.",
    )
}

fn base_source_entries(model: &str, workspace: &Path, skills_dir: Option<&Path>) -> ReportBuilder {
    let mut builder = ReportBuilder::new();

    let constitution =
        crate::prompts::compose_prompt_with_approval_model_and_shell(Personality::Calm, model);
    builder.push(SourceEntry::text(
        SourceKind::Constitution,
        "Constitution and static prompt",
        Some("crates/tui/src/prompts/constitution.md".to_string()),
        ActivationReason::AlwaysOn,
        &constitution,
        CountingConfidence::High,
        Some(1),
    ));

    let project_context = crate::project_context::load_project_context_with_parents(workspace);
    if let Some(block) = project_context.constitution_block.as_deref() {
        builder.push(SourceEntry::text(
            SourceKind::RepoConstitution,
            "Repository constitution",
            project_context
                .constitution_source_path
                .as_ref()
                .map(|path| path.display().to_string()),
            ActivationReason::FilePresent,
            block,
            CountingConfidence::High,
            Some(4),
        ));
    }

    if let Some(content) = project_context.instructions.as_deref() {
        let source = project_context
            .source_path
            .as_ref()
            .map_or_else(|| "project".to_string(), |p| p.display().to_string());
        let mut block = format!(
            "<project_instructions source=\"{source}\">\n{content}\n</project_instructions>"
        );
        // Include rules in the report when present
        if let Some(rules) = &project_context.rules_block {
            block.push('\n');
            block.push_str(rules);
        }
        builder.push(SourceEntry::text(
            SourceKind::ProjectContext,
            "Project instructions",
            project_context
                .source_path
                .as_ref()
                .map(|path| path.display().to_string()),
            ActivationReason::FilePresent,
            &block,
            CountingConfidence::High,
            Some(5),
        ));
    } else if let Some(rules) = &project_context.rules_block {
        // Rules exist without main instructions
        builder.push(SourceEntry::text(
            SourceKind::ProjectContext,
            "Project rules",
            None::<String>,
            ActivationReason::FilePresent,
            rules,
            CountingConfidence::High,
            Some(5),
        ));
    }

    if project_context.constitution_block.is_none() && project_context.instructions.is_none() {
        builder.push(SourceEntry::omitted(
            SourceKind::ProjectContext,
            "Project context and repository instructions",
            Some(workspace.display().to_string()),
            Some(5),
            "no project context block available",
        ));
    }
    if !project_context.warnings.is_empty() {
        let warnings = project_context.warnings.join("\n");
        let estimated_tokens = estimate_text_tokens_conservative(&warnings);
        builder.push(SourceEntry::diagnostic(
            SourceKind::ProjectContextWarning,
            "Project context warnings",
            Some(workspace.display().to_string()),
            ActivationReason::RuntimeState,
            warnings,
            estimated_tokens,
            Some(4),
        ));
    }

    if let Some(pack) = crate::project_context::generate_project_context_pack(workspace) {
        builder.push(SourceEntry::text(
            SourceKind::ProjectContextPack,
            "Project context pack",
            Some(workspace.display().to_string()),
            ActivationReason::RuntimeState,
            &pack,
            CountingConfidence::Approximate,
            Some(5),
        ));
    }

    let skills_block = match skills_dir {
        Some(dir) => crate::skill_context::render_available_skills_context_for_workspace_and_dir(
            workspace, dir,
        ),
        None => crate::skill_context::render_available_skills_context_for_workspace(workspace),
    };
    if let Some(block) = skills_block {
        builder.push(SourceEntry::text(
            SourceKind::SkillsBlock,
            "Available skills",
            skills_dir.map(|path| path.display().to_string()),
            ActivationReason::FilePresent,
            &block,
            CountingConfidence::High,
            Some(5),
        ));
    } else {
        builder.push(SourceEntry::omitted(
            SourceKind::SkillsBlock,
            "Available skills",
            skills_dir.map(|path| path.display().to_string()),
            Some(5),
            "no skills discovered",
        ));
    }

    builder.push(SourceEntry::estimate(
        SourceKind::ContextManagement,
        "Context management guidance",
        None,
        ActivationReason::AlwaysOn,
        430,
        CountingConfidence::Approximate,
        Some(3),
    ));
    builder.push(SourceEntry::text(
        SourceKind::CompactionRelayTemplate,
        "Compaction relay template",
        Some("crates/tui/src/prompts/compact.md".to_string()),
        ActivationReason::AlwaysOn,
        COMPACT_TEMPLATE,
        CountingConfidence::High,
        Some(3),
    ));
    builder.push(SourceEntry::estimate(
        SourceKind::RuntimePolicy,
        "Runtime policy reference",
        None,
        ActivationReason::AlwaysOn,
        650,
        CountingConfidence::Approximate,
        Some(3),
    ));

    add_handoff_entry(&mut builder, workspace);
    builder
}

fn add_handoff_entry(builder: &mut ReportBuilder, workspace: &Path) {
    let primary = workspace.join(crate::prompts::HANDOFF_RELATIVE_PATH);
    let legacy = workspace.join(".deepseek/handoff.md");
    let path = if primary.exists() { primary } else { legacy };
    let Some(raw) = std::fs::read_to_string(&path)
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    else {
        builder.push(SourceEntry::omitted(
            SourceKind::HandoffRelay,
            "Previous session relay",
            Some(
                workspace
                    .join(crate::prompts::HANDOFF_RELATIVE_PATH)
                    .display()
                    .to_string(),
            ),
            Some(6),
            "no relay artifact found",
        ));
        return;
    };

    builder.push(SourceEntry::text(
        SourceKind::HandoffRelay,
        "Previous session relay",
        Some(path.display().to_string()),
        ActivationReason::FilePresent,
        &raw,
        CountingConfidence::High,
        Some(6),
    ));
}

pub fn context_report_json(report: &PromptSourceMap) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|err| {
        format!("{{\"error\":\"failed to serialize context report: {err}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn context_report_surfaces_repo_constitution_source_and_warnings() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".git")).expect("mkdir .git");
        fs::create_dir(tmp.path().join(".codewhale")).expect("mkdir .codewhale");
        fs::write(
            tmp.path().join(".codewhale").join("constitution.json"),
            r#"{
                "schema_version": 1,
                "authority": ["current user request"],
                "branch_policy": "v0.8.53 work targets the codex/v0.8.53 integration branch, not main"
            }"#,
        )
        .expect("write constitution");

        let report = build_headless_context_report(&Config::default(), tmp.path());
        assert!(
            report.entries.iter().any(|entry| {
                entry.source_kind == SourceKind::RepoConstitution
                    && entry.source_path.as_deref().is_some_and(|path| {
                        path.replace('\\', "/")
                            .ends_with(".codewhale/constitution.json")
                    })
            }),
            "repo constitution source should be an explicit source-map entry: {:?}",
            report.entries
        );
        assert!(
            report.entries.iter().any(|entry| {
                entry.source_kind == SourceKind::ProjectContextWarning
                    && entry
                        .truncation_reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("branch_policy appears stale"))
                    && entry.estimated_tokens > 0
            }),
            "repo constitution warnings should be explicit source-map entries: {:?}",
            report.entries
        );

        let json = context_report_json(&report);
        assert!(json.contains("\"repo_constitution\""));
        assert!(json.contains("branch_policy appears stale"));
    }

    #[test]
    fn context_report_marks_whale_md_ignored_without_loading_body() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("WHALE.md"), "SECRET_LEGACY_WHALE_BODY").expect("write whale");

        let report = build_headless_context_report(&Config::default(), tmp.path());
        assert!(
            report.entries.iter().any(|entry| {
                entry.source_kind == SourceKind::ProjectContextWarning
                    && entry
                        .truncation_reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("WHALE.md is ignored"))
            }),
            "ignored WHALE.md should be visible as a migration warning: {:?}",
            report.entries
        );
        assert!(
            !context_report_json(&report).contains("SECRET_LEGACY_WHALE_BODY"),
            "ignored WHALE.md body must not enter context report"
        );
    }

    #[test]
    fn headless_context_report_omits_legacy_memory_when_moraine_fallback_enabled() {
        let tmp = tempdir().expect("tempdir");
        let memory_path = tmp.path().join("memory.md");
        fs::write(&memory_path, "private legacy memory").expect("write memory");
        let mut config: Config = toml::from_str(
            r#"
            [memory]
            enabled = true
            moraine_fallback = true
            "#,
        )
        .expect("parse config");
        config.memory_path = Some(memory_path.to_string_lossy().into_owned());

        let report = build_headless_context_report(&config, tmp.path());
        let memory_entry = report
            .entries
            .iter()
            .find(|entry| entry.source_kind == SourceKind::UserMemory)
            .expect("user memory source entry");

        assert_eq!(memory_entry.activation_reason, ActivationReason::Omitted);
        assert_eq!(
            memory_entry.truncation_reason.as_deref(),
            Some("disabled by moraine_fallback")
        );
        assert!(!context_report_json(&report).contains("private legacy memory"));
    }

    #[test]
    fn finish_reflects_route_context_window_over_model_default() {
        // deepseek-v4-pro defaults to a 1M window; a resolved route advertising a
        // smaller window must win in the report's context_window_tokens.
        let route_window = 128_000u64;
        let model_default = crate::models::context_window_for_model("deepseek-v4-pro")
            .expect("model has a default window");
        assert_ne!(
            u64::from(model_default),
            route_window,
            "test fixture must differ from the model default to be meaningful"
        );

        let limits = RouteLimits {
            context_tokens: Some(route_window),
            input_tokens: None,
            output_tokens: None,
        };
        let builder = ReportBuilder::new();
        let report = builder.finish(
            ApiProvider::Deepseek,
            "deepseek-v4-pro",
            Some(limits),
            10_000,
            "test",
        );

        assert_eq!(report.context_window_tokens, Some(route_window as u32));
        // Budget percent is computed against the route window, not the default.
        let expected = (10_000.0 / route_window as f64) * 100.0;
        let actual = report.budget_used_percent.expect("window known");
        assert!(
            (actual - expected).abs() < 1e-6,
            "got {actual}, want {expected}"
        );
    }
}
