//! Constitution-first setup wizard shell (#3404/#3794).
//!
//! This module owns the reusable setup shell: step ordering, navigation,
//! per-step status projection, and the v0.8.67 constitution checkpoint action.
//! Individual step contents can grow behind [`SetupWizardStep`] without
//! changing the navigation or commit contract.

use std::borrow::Cow;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use crate::config::{Config, has_api_key};
use crate::localization::{MessageId, tr};
use crate::palette;
use crate::prompts::{
    BASE_PROMPT_OVERRIDE_OPT_IN_ENV, CONSTITUTION_OVERRIDE_FILE, base_prompt_override_opt_in,
};
use crate::tui::app::App;
use crate::tui::onboarding;
use crate::tui::views::{
    ActionHint, ModalKind, ModalView, ViewAction, ViewEvent, render_modal_footer,
    render_panel_scroll_rail, render_underwater_surface,
};
use codewhale_config::{
    AutonomyPreference, ConstitutionAuthoring, ConstitutionChoice, ConstitutionSource,
    ConstitutionValidity, InheritedConfigFacts, RuntimePostureSource, SetupState, SetupStep,
    StepEntry, StepStatus, UserConstitution, UserConstitutionLoad,
    user_constitution::MAX_NOTES_LEN,
};

mod fleet_draft;
mod model_draft;
mod operate;
mod persistence;
mod provider;
mod tools_mcp;

pub(crate) use fleet_draft::{draft_fleet_profile_with_model, workspace_fingerprint};
pub(crate) use model_draft::draft_constitution_with_model;
use persistence::SetupPersistenceFacts;

/// Target lane for the once-per-version constitution checkpoint. The workspace
/// package remains 0.8.66 until release approval, so this cannot read
/// `CARGO_PKG_VERSION` yet.
pub const CONSTITUTION_CHECKPOINT_VERSION: &str = "0.8.67";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCommitKind {
    BundledConstitution,
    DeferredConstitution,
}

pub trait SetupWizardStep {
    fn id(&self) -> SetupStep;
    fn title_id(&self) -> MessageId;
    fn why_id(&self) -> MessageId;
    fn required(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StaticSetupStep {
    id: SetupStep,
    title_id: MessageId,
    why_id: MessageId,
    required: bool,
}

impl SetupWizardStep for StaticSetupStep {
    fn id(&self) -> SetupStep {
        self.id
    }

    fn title_id(&self) -> MessageId {
        self.title_id
    }

    fn why_id(&self) -> MessageId {
        self.why_id
    }

    fn required(&self) -> bool {
        self.required
    }
}

const STEP_SPECS: [StaticSetupStep; 7] = [
    StaticSetupStep {
        id: SetupStep::ProviderModel,
        title_id: MessageId::SetupStepProviderModelTitle,
        why_id: MessageId::SetupStepProviderModelWhy,
        required: true,
    },
    StaticSetupStep {
        id: SetupStep::TrustSandbox,
        title_id: MessageId::SetupStepTrustSandboxTitle,
        why_id: MessageId::SetupStepTrustSandboxWhy,
        required: true,
    },
    StaticSetupStep {
        id: SetupStep::Constitution,
        title_id: MessageId::SetupStepConstitutionTitle,
        why_id: MessageId::SetupStepConstitutionWhy,
        required: true,
    },
    StaticSetupStep {
        id: SetupStep::OperateFleet,
        title_id: MessageId::SetupStepOperateFleetTitle,
        why_id: MessageId::SetupStepOperateFleetWhy,
        required: false,
    },
    StaticSetupStep {
        id: SetupStep::ToolsMcp,
        title_id: MessageId::SetupStepToolsMcpTitle,
        why_id: MessageId::SetupStepToolsMcpWhy,
        required: false,
    },
    StaticSetupStep {
        id: SetupStep::Persistence,
        title_id: MessageId::SetupStepPersistenceTitle,
        why_id: MessageId::SetupStepPersistenceWhy,
        required: false,
    },
    StaticSetupStep {
        id: SetupStep::Verification,
        title_id: MessageId::SetupStepVerificationTitle,
        why_id: MessageId::SetupStepVerificationWhy,
        required: false,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupWizardView {
    state: SetupState,
    selected: usize,
    facts: SetupRuntimeFacts,
    guided_draft: GuidedConstitutionDraft,
    freeform_note: String,
    editing_freeform_note: bool,
    guided_preview_seen: bool,
    /// The keep-existing path mirrors the guided two-step: the first `K`
    /// opens the rendered preview of the existing file, the second completes
    /// the checkpoint without touching it.
    existing_preview_seen: bool,
    /// A model-drafted constitution awaiting ratification, installed by the
    /// host after a successful one-shot draft (already sanitized + bounded).
    /// Cleared whenever a guided answer changes so a stale draft can never be
    /// ratified against fresh answers.
    model_draft: Option<Box<UserConstitution>>,
    /// Display label of the model that authored `model_draft` (safe metadata,
    /// e.g. "GLM-5.2"), for provenance copy only.
    model_draft_label: Option<String>,
    runtime_preset: SetupRuntimePreset,
    runtime_preset_preview_seen: bool,
    body_scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupRuntimeFacts {
    provider: String,
    model: String,
    auth: String,
    health: String,
    provider_ready: bool,
    provider_result: String,
    work_intent: String,
    approval: String,
    shell: String,
    allow_shell_enabled: bool,
    trust: String,
    sandbox: String,
    sandbox_mode_value: String,
    network: String,
    network_default_value: String,
    runtime_result: String,
    operate_runtime_ready: bool,
    operate_runtime_result: String,
    fleet_roster_ready: bool,
    fleet_roster_result: String,
    operate_concurrency_result: String,
    operate_result: String,
    tools_mcp_servers_result: String,
    tools_mcp_skills_result: String,
    tools_mcp_tools_result: String,
    tools_mcp_plugins_result: String,
    tools_mcp_result: String,
    tools_mcp_needs_action: bool,
    tools_mcp_path_display: String,
    tools_mcp_skills_path_display: String,
    tools_mcp_plugins_path_display: String,
    persistence: SetupPersistenceFacts,
    default_mode: String,
    approval_policy_value: String,
    project_override_warning: Option<String>,
    constitution_autonomy: String,
    constitution_file: SetupConstitutionFileState,
    expert_override: SetupExpertOverrideState,
}

impl Default for SetupRuntimeFacts {
    fn default() -> Self {
        Self {
            provider: "not loaded".to_string(),
            model: "not loaded".to_string(),
            auth: "not checked".to_string(),
            health: "not checked".to_string(),
            provider_ready: false,
            provider_result: "provider/model not loaded".to_string(),
            work_intent: "not loaded".to_string(),
            approval: "not loaded".to_string(),
            shell: "not loaded".to_string(),
            allow_shell_enabled: false,
            trust: "not loaded".to_string(),
            sandbox: "not configured".to_string(),
            sandbox_mode_value: "default".to_string(),
            network: "not configured".to_string(),
            network_default_value: "prompt".to_string(),
            runtime_result: "runtime posture not loaded".to_string(),
            operate_runtime_ready: false,
            operate_runtime_result: "worker runtime not loaded".to_string(),
            fleet_roster_ready: false,
            fleet_roster_result: "Fleet roster not loaded".to_string(),
            operate_concurrency_result: "concurrency not loaded".to_string(),
            operate_result: "operate readiness not loaded".to_string(),
            tools_mcp_servers_result: "MCP config not loaded".to_string(),
            tools_mcp_skills_result: "skills dir not loaded".to_string(),
            tools_mcp_tools_result: "tools dir not loaded".to_string(),
            tools_mcp_plugins_result: "plugins dir not loaded".to_string(),
            tools_mcp_result: "tools/MCP not loaded".to_string(),
            tools_mcp_needs_action: false,
            tools_mcp_path_display: String::new(),
            tools_mcp_skills_path_display: String::new(),
            tools_mcp_plugins_path_display: String::new(),
            persistence: SetupPersistenceFacts::default(),
            default_mode: "agent".to_string(),
            approval_policy_value: "on-request".to_string(),
            project_override_warning: None,
            constitution_autonomy: "not loaded".to_string(),
            constitution_file: SetupConstitutionFileState::NotChecked,
            expert_override: SetupExpertOverrideState::NotChecked,
        }
    }
}

impl SetupRuntimeFacts {
    fn from_app_config(app: &App, config: &Config) -> Self {
        let expert_override = SetupExpertOverrideState::load();
        let readiness = crate::provider_readiness::resolve_for_model(
            config,
            app.api_provider,
            if app.auto_model { "auto" } else { &app.model },
            &app.provider_health,
        );
        // A failed observed check remains retryable in route pickers, but the
        // setup receipt must not certify it as healthy. Saved-unchecked and
        // local-unchecked are honest reviewed configuration states; an actual
        // session failure is NeedsAction until a later success replaces it.
        let provider_ready = readiness.can_attempt()
            && !matches!(
                &readiness,
                crate::provider_readiness::ResolvedProviderReadiness::SavedLastCheckFailed { .. }
            );
        let model = app.model_display_label();
        let provider = app.api_provider.display_name().to_string();
        let auth = readiness.label().into_owned();
        let health = if provider_ready {
            format!("{}; route can be attempted", readiness.label())
        } else if matches!(
            &readiness,
            crate::provider_readiness::ResolvedProviderReadiness::SavedLastCheckFailed { .. }
        ) {
            format!("{}; retry or open /provider", readiness.label())
        } else if app.api_provider == crate::config::ApiProvider::OpenaiCodex {
            format!("{}; run codex login or open /provider", readiness.label())
        } else if let Some(url) = app.api_provider.credential_url() {
            format!(
                "{}; credentials: {url}; open /provider to repair the route",
                readiness.label()
            )
        } else {
            format!("{}; open /provider to repair the route", readiness.label())
        };
        let provider_result = format!(
            "provider={}, model={}, auth={}, health={}",
            app.api_provider.as_str(),
            model,
            readiness.label(),
            if provider_ready {
                "attemptable"
            } else {
                "needs action"
            }
        );
        let shell = if app.allow_shell { "enabled" } else { "hidden" }.to_string();
        let trust = if app.trust_mode {
            "trusted workspace / writes allowed by posture"
        } else {
            "workspace trust not elevated"
        }
        .to_string();
        let sandbox = config
            .sandbox_mode
            .as_deref()
            .filter(|mode| !mode.trim().is_empty())
            .unwrap_or("default")
            .to_string();
        let sandbox_mode_value = sandbox.clone();
        let network_default_value = config
            .network
            .as_ref()
            .map_or("prompt".to_string(), |policy| policy.default.clone());
        let network = config
            .network
            .as_ref()
            .map_or("prompt by default".to_string(), |policy| {
                format!("default {}", policy.default)
            });
        let runtime_result = format!(
            "intent={}, approval={}, shell={}, trust={}, sandbox={}, network={}",
            app.mode.as_setting(),
            app.approval_mode
                .permission_chip_label()
                .to_ascii_lowercase(),
            if app.allow_shell { "enabled" } else { "hidden" },
            if app.trust_mode {
                "trusted"
            } else {
                "workspace"
            },
            sandbox,
            network
        );
        let operate = operate::SetupOperateFacts::from_app_config(app, config, provider_ready);
        let codewhale_home = setup_codewhale_home_dir();
        let persistence = SetupPersistenceFacts::from_app_config(app, config, &codewhale_home);
        let tools_mcp =
            tools_mcp::SetupToolsMcpFacts::from_app_config(app, config, &codewhale_home);
        let tools_mcp_servers_result = tools_mcp.servers_result;
        let tools_mcp_skills_result = tools_mcp.skills_result;
        let tools_mcp_tools_result = tools_mcp.tools_result;
        let tools_mcp_plugins_result = tools_mcp.plugins_result;
        let tools_mcp_result = tools_mcp.result;
        let tools_mcp_needs_action = tools_mcp.needs_action;
        let tools_mcp_path_display = tools_mcp.mcp_path_display;
        let tools_mcp_skills_path_display = tools_mcp.skills_path_display;
        let tools_mcp_plugins_path_display = tools_mcp.plugins_path_display;
        let constitution_autonomy = UserConstitution::load()
            .ok()
            .and_then(|load| {
                load.constitution().map(|constitution| {
                    autonomy_label(constitution.autonomy_preference).to_string()
                })
            })
            .unwrap_or_else(|| tr(MessageId::SetupAutonomyUnspecified).to_string());
        Self {
            provider,
            model,
            auth,
            health,
            provider_ready,
            provider_result,
            work_intent: app.mode.display_name().to_string(),
            approval: app
                .approval_mode
                .permission_chip_label()
                .to_ascii_lowercase(),
            shell,
            allow_shell_enabled: app.allow_shell,
            trust,
            sandbox,
            sandbox_mode_value,
            network,
            network_default_value,
            runtime_result,
            operate_runtime_ready: operate.runtime_ready,
            operate_runtime_result: operate.runtime_result,
            fleet_roster_ready: operate.roster_ready,
            fleet_roster_result: operate.roster_result,
            operate_concurrency_result: operate.concurrency_result,
            operate_result: operate.result,
            tools_mcp_servers_result,
            tools_mcp_skills_result,
            tools_mcp_tools_result,
            tools_mcp_plugins_result,
            tools_mcp_result,
            tools_mcp_needs_action,
            tools_mcp_path_display,
            tools_mcp_skills_path_display,
            tools_mcp_plugins_path_display,
            persistence,
            default_mode: app.mode.as_setting().to_string(),
            approval_policy_value: config
                .approval_policy
                .as_deref()
                .filter(|policy| !policy.trim().is_empty())
                .unwrap_or("on-request")
                .to_string(),
            project_override_warning: project_runtime_override_warning(&app.workspace),
            constitution_autonomy,
            constitution_file: SetupConstitutionFileState::load(),
            expert_override,
        }
    }
}

fn setup_codewhale_home_dir() -> std::path::PathBuf {
    codewhale_config::codewhale_home().unwrap_or_else(|_| {
        dirs::home_dir().map_or_else(
            || std::path::PathBuf::from(".codewhale"),
            |home| home.join(".codewhale"),
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetupRuntimePreset {
    AskFirst,
    #[default]
    NormalAgent,
    HighTrustLocal,
}

impl SetupRuntimePreset {
    const ALL: [Self; 3] = [Self::AskFirst, Self::NormalAgent, Self::HighTrustLocal];

    fn from_key(key: char) -> Option<Self> {
        match key {
            '1' => Some(Self::AskFirst),
            '2' => Some(Self::NormalAgent),
            '3' => Some(Self::HighTrustLocal),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::AskFirst => "ask-first",
            Self::NormalAgent => "normal-agent",
            Self::HighTrustLocal => "high-trust-local",
        }
    }

    fn title_id(self) -> MessageId {
        match self {
            Self::AskFirst => MessageId::SetupRuntimePresetAskFirstTitle,
            Self::NormalAgent => MessageId::SetupRuntimePresetNormalAgentTitle,
            Self::HighTrustLocal => MessageId::SetupRuntimePresetHighTrustTitle,
        }
    }

    fn description_id(self) -> MessageId {
        match self {
            Self::AskFirst => MessageId::SetupRuntimePresetAskFirstDescription,
            Self::NormalAgent => MessageId::SetupRuntimePresetNormalAgentDescription,
            Self::HighTrustLocal => MessageId::SetupRuntimePresetHighTrustDescription,
        }
    }

    pub fn default_mode(self) -> &'static str {
        match self {
            Self::AskFirst => "plan",
            Self::NormalAgent | Self::HighTrustLocal => "agent",
        }
    }

    pub fn permission_posture(self) -> &'static str {
        match self {
            Self::AskFirst | Self::NormalAgent => "ask",
            Self::HighTrustLocal => "full-access",
        }
    }

    pub fn approval_policy(self) -> Option<&'static str> {
        match self {
            Self::AskFirst | Self::NormalAgent => Some("on-request"),
            // Full Access lives in TUI settings; it is intentionally not a
            // top-level approval_policy value.
            Self::HighTrustLocal => None,
        }
    }

    pub fn allow_shell(self) -> bool {
        match self {
            Self::AskFirst => false,
            Self::NormalAgent | Self::HighTrustLocal => true,
        }
    }

    pub fn sandbox_mode(self) -> &'static str {
        match self {
            Self::AskFirst => "read-only",
            Self::NormalAgent | Self::HighTrustLocal => "workspace-write",
        }
    }

    pub fn result_summary(self) -> String {
        let approval = self
            .approval_policy()
            .unwrap_or("unset (Full Access saved in TUI settings)");
        format!(
            "preset={}, default_mode={}, permission_posture={}, approval_policy={}, allow_shell={}, sandbox_mode={}, network=unchanged, trust=unchanged",
            self.id(),
            self.display_mode(),
            self.permission_posture(),
            approval,
            self.allow_shell(),
            self.sandbox_mode()
        )
    }

    fn display_mode(self) -> &'static str {
        match self {
            Self::AskFirst => "plan",
            Self::NormalAgent => "act",
            Self::HighTrustLocal => "act + full-access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupConstitutionFileState {
    NotChecked,
    Missing,
    Loaded,
    Empty,
    Invalid,
    Unreadable,
    PathError,
}

impl SetupConstitutionFileState {
    fn load() -> Self {
        match UserConstitution::path() {
            Ok(path) => Self::from_load(&UserConstitution::load_from(&path)),
            Err(_) => Self::PathError,
        }
    }

    fn from_load(load: &UserConstitutionLoad) -> Self {
        match load {
            UserConstitutionLoad::Missing => Self::Missing,
            UserConstitutionLoad::Empty => Self::Empty,
            UserConstitutionLoad::Invalid(_) => Self::Invalid,
            UserConstitutionLoad::Unreadable(_) => Self::Unreadable,
            UserConstitutionLoad::Loaded(_) => Self::Loaded,
        }
    }

    fn label(self, choice: ConstitutionChoice) -> Cow<'static, str> {
        let id = match self {
            Self::NotChecked => MessageId::SetupConstitutionFileNotChecked,
            Self::Missing => MessageId::SetupConstitutionFileMissing,
            Self::Loaded if choice == ConstitutionChoice::GuidedCustom => {
                MessageId::SetupConstitutionFileLoadedSelected
            }
            Self::Loaded if choice.is_explicit() => MessageId::SetupConstitutionFileLoadedInactive,
            Self::Loaded => MessageId::SetupConstitutionFileLoadedUnselected,
            Self::Empty => MessageId::SetupConstitutionFileEmpty,
            Self::Invalid => MessageId::SetupConstitutionFileInvalid,
            Self::Unreadable => MessageId::SetupConstitutionFileUnreadable,
            Self::PathError => MessageId::SetupConstitutionFilePathError,
        };
        tr(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupExpertOverrideState {
    NotChecked,
    Missing,
    Active,
    Disabled,
    Empty,
    Unreadable,
    PathError,
}

impl SetupExpertOverrideState {
    fn load() -> Self {
        let Some(path) = expert_override_path() else {
            return Self::PathError;
        };
        match std::fs::read_to_string(&path) {
            Ok(raw) if raw.trim().is_empty() => Self::Empty,
            Ok(_) if base_prompt_override_opt_in() => Self::Active,
            Ok(_) => Self::Disabled,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(_) => Self::Unreadable,
        }
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::NotChecked => tr(MessageId::SetupExpertOverrideNotChecked),
            Self::Missing => tr(MessageId::SetupExpertOverrideMissing),
            Self::Active => tr(MessageId::SetupExpertOverrideActive),
            Self::Disabled => tr(MessageId::SetupExpertOverrideDisabled)
                .replace("{env}", BASE_PROMPT_OVERRIDE_OPT_IN_ENV)
                .into(),
            Self::Empty => tr(MessageId::SetupExpertOverrideEmpty),
            Self::Unreadable => tr(MessageId::SetupExpertOverrideUnreadable),
            Self::PathError => tr(MessageId::SetupExpertOverridePathError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuidedConstitutionDraft {
    purpose: GuidedPurpose,
    autonomy: AutonomyPreference,
    evidence: GuidedEvidence,
    communication: GuidedCommunication,
    privacy: GuidedPrivacy,
    principles: GuidedPrinciples,
}

impl Default for GuidedConstitutionDraft {
    fn default() -> Self {
        Self {
            purpose: GuidedPurpose::Coding,
            autonomy: AutonomyPreference::Balanced,
            evidence: GuidedEvidence::TestsAndReceipts,
            communication: GuidedCommunication::Concise,
            privacy: GuidedPrivacy::StandardCare,
            principles: GuidedPrinciples::ScopedChanges,
        }
    }
}

impl GuidedConstitutionDraft {
    fn cycle(&mut self, key: char) -> bool {
        match key {
            '1' => self.purpose = self.purpose.next(),
            '2' => self.autonomy = next_guided_autonomy(self.autonomy),
            '3' => self.evidence = self.evidence.next(),
            '4' => self.communication = self.communication.next(),
            '5' => self.privacy = self.privacy.next(),
            '6' => self.principles = self.principles.next(),
            _ => return false,
        }
        true
    }

    #[cfg(test)]
    fn to_constitution(self) -> UserConstitution {
        self.to_constitution_with_freeform(None)
    }

    fn to_constitution_with_freeform(self, freeform_note: Option<&str>) -> UserConstitution {
        let mut notes = self.notes();
        if let Some(note) = freeform_note.map(str::trim).filter(|note| !note.is_empty()) {
            let own_words = format!(
                "\n用户自由原则：{}",
                bounded_freeform_note(note, MAX_NOTES_LEN)
            );
            notes.push_str(&own_words);
        }
        UserConstitution {
            about: Some(self.purpose.about().to_string()),
            working_style: vec![
                self.purpose.working_style().to_string(),
                self.communication.working_style().to_string(),
                self.evidence.working_style().to_string(),
                self.privacy.working_style().to_string(),
            ],
            priorities: vec![
                authority_priority().to_string(),
                autonomy_priority(self.autonomy).to_string(),
                self.privacy.escalation_rule().to_string(),
            ],
            autonomy_preference: self.autonomy,
            notes: Some(notes),
            ..UserConstitution::default()
        }
    }

    fn notes(self) -> String {
        let notes = tr(MessageId::SetupGuidedNotes);
        notes
            .replace("{purpose}", &self.purpose.label())
            .replace("{initiative}", autonomy_label(self.autonomy))
            .replace("{evidence}", &self.evidence.label())
            .replace("{communication}", self.communication.label())
            .replace("{privacy}", self.privacy.label())
            .replace("{principles}", self.principles.label())
            .replace("{notes}", self.principles.note())
            .to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidedPurpose {
    Coding,
    Research,
    Operations,
    Mixed,
}

impl GuidedPurpose {
    fn next(self) -> Self {
        match self {
            Self::Coding => Self::Research,
            Self::Research => Self::Operations,
            Self::Operations => Self::Mixed,
            Self::Mixed => Self::Coding,
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Coding => tr(MessageId::SetupGuidedPurposeCoding),
            Self::Research => tr(MessageId::SetupGuidedPurposeResearch),
            Self::Operations => tr(MessageId::SetupGuidedPurposeOperations),
            Self::Mixed => tr(MessageId::SetupGuidedPurposeMixed),
        }
    }

    fn about(self) -> Cow<'static, str> {
        match self {
            Self::Coding => tr(MessageId::SetupGuidedPurposeAboutCoding),
            Self::Research => tr(MessageId::SetupGuidedPurposeAboutResearch),
            Self::Operations => tr(MessageId::SetupGuidedPurposeAboutOperations),
            Self::Mixed => tr(MessageId::SetupGuidedPurposeAboutMixed),
        }
    }

    fn working_style(self) -> Cow<'static, str> {
        match self {
            Self::Coding => tr(MessageId::SetupGuidedStyleCoding),
            Self::Research => tr(MessageId::SetupGuidedStyleResearch),
            Self::Operations => tr(MessageId::SetupGuidedStyleOperations),
            Self::Mixed => tr(MessageId::SetupGuidedStyleMixed),
        }
    }

    fn as_prompt_value(self) -> &'static str {
        match self {
            Self::Coding => "coding workbench",
            Self::Research => "research synthesis",
            Self::Operations => "operations helper",
            Self::Mixed => "mixed workbench",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidedEvidence {
    Assumptions,
    TestsAndReceipts,
    ReleaseReceipts,
}

impl GuidedEvidence {
    fn next(self) -> Self {
        match self {
            Self::Assumptions => Self::TestsAndReceipts,
            Self::TestsAndReceipts => Self::ReleaseReceipts,
            Self::ReleaseReceipts => Self::Assumptions,
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Assumptions => tr(MessageId::SetupGuidedEvidenceAssumptions),
            Self::TestsAndReceipts => tr(MessageId::SetupGuidedEvidenceTestsAndReceipts),
            Self::ReleaseReceipts => tr(MessageId::SetupGuidedEvidenceReleaseReceipts),
        }
    }

    fn working_style(self) -> &'static str {
        match self {
            Self::Assumptions => "在宣称完成前总结假设、未知和剩余风险。",
            Self::TestsAndReceipts => "在能降低不确定性时，用命令、测试、截图或引用给出具体验证。",
            Self::ReleaseReceipts => "对重要结论和发布证据标注文件、命令、截图、CI 或来源。",
        }
    }

    fn as_prompt_value(self) -> &'static str {
        match self {
            Self::Assumptions => "state assumptions",
            Self::TestsAndReceipts => "tests & receipts",
            Self::ReleaseReceipts => "release receipts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidedCommunication {
    Concise,
    Teaching,
    Direct,
}

impl GuidedCommunication {
    fn next(self) -> Self {
        match self {
            Self::Concise => Self::Teaching,
            Self::Teaching => Self::Direct,
            Self::Direct => Self::Concise,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Concise => "简洁",
            Self::Teaching => "教学式",
            Self::Direct => "直接",
        }
    }

    fn working_style(self) -> &'static str {
        match self {
            Self::Concise => "保持更新简洁，并只解释重要取舍。",
            Self::Teaching => "解释关键推理和取舍，让用户能理解系统。",
            Self::Direct => "直接说明阻塞、风险和不确定性，避免装饰性文案。",
        }
    }

    fn as_prompt_value(self) -> &'static str {
        match self {
            Self::Concise => "concise",
            Self::Teaching => "teaching",
            Self::Direct => "direct",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidedPrivacy {
    StandardCare,
    StrictBoundaries,
    ProjectLocal,
}

impl GuidedPrivacy {
    fn next(self) -> Self {
        match self {
            Self::StandardCare => Self::StrictBoundaries,
            Self::StrictBoundaries => Self::ProjectLocal,
            Self::ProjectLocal => Self::StandardCare,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::StandardCare => "标准保护",
            Self::StrictBoundaries => "严格边界",
            Self::ProjectLocal => "项目内记忆",
        }
    }

    fn working_style(self) -> &'static str {
        match self {
            Self::StandardCare => "保护密钥、用户文件、Git 历史、生产系统、成本、隐私和时间。",
            Self::StrictBoundaries => {
                "把密钥、个人数据、凭据、生产状态、资金和发布动作视为先确认边界。"
            }
            Self::ProjectLocal => "项目特定上下文留在项目内，除非明确要求，否则不要写入记忆。",
        }
    }

    fn escalation_rule(self) -> &'static str {
        match self {
            Self::StandardCare => "遇到破坏性、高成本、凭据、发布、法律或安全风险操作时先询问。",
            Self::StrictBoundaries => {
                "在读取或传播敏感信息、触碰生产系统、花费资金或发布内容前停止并询问。"
            }
            Self::ProjectLocal => {
                "需要跨项目记忆、复制项目细节或引用旧交接时，先确认这些上下文仍适用。"
            }
        }
    }

    fn as_prompt_value(self) -> &'static str {
        match self {
            Self::StandardCare => "standard care",
            Self::StrictBoundaries => "strict boundaries",
            Self::ProjectLocal => "project-local memory",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuidedPrinciples {
    ScopedChanges,
    UserVoice,
    ReversibleOps,
}

impl GuidedPrinciples {
    fn next(self) -> Self {
        match self {
            Self::ScopedChanges => Self::UserVoice,
            Self::UserVoice => Self::ReversibleOps,
            Self::ReversibleOps => Self::ScopedChanges,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ScopedChanges => "小范围改动",
            Self::UserVoice => "保留用户语气",
            Self::ReversibleOps => "可逆步骤",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::ScopedChanges => {
                "自由原则：优先采用小范围、可审查的改动；除非明确要求，不做无关重构。"
            }
            Self::UserVoice => "自由原则：保留用户的语气、品牌和约束；不把偏好推断成权限扩大。",
            Self::ReversibleOps => "自由原则：先选择可逆步骤、检查点和回滚说明，再进行高影响操作。",
        }
    }

    fn as_prompt_value(self) -> &'static str {
        match self {
            Self::ScopedChanges => "scoped changes",
            Self::UserVoice => "user voice",
            Self::ReversibleOps => "reversible steps",
        }
    }
}

fn next_guided_autonomy(preference: AutonomyPreference) -> AutonomyPreference {
    match preference {
        AutonomyPreference::Unspecified | AutonomyPreference::Cautious => {
            AutonomyPreference::Balanced
        }
        AutonomyPreference::Balanced => AutonomyPreference::Autonomous,
        AutonomyPreference::Autonomous => AutonomyPreference::Cautious,
    }
}

fn autonomy_label(preference: AutonomyPreference) -> &'static str {
    match preference {
        AutonomyPreference::Cautious => "谨慎",
        AutonomyPreference::Balanced => "平衡",
        AutonomyPreference::Autonomous => "积极主动",
        AutonomyPreference::Unspecified => "未指定",
    }
}

fn autonomy_prompt_value(preference: AutonomyPreference) -> &'static str {
    match preference {
        AutonomyPreference::Unspecified => "unspecified",
        AutonomyPreference::Cautious => "cautious",
        AutonomyPreference::Balanced => "balanced",
        AutonomyPreference::Autonomous => "autonomous",
    }
}

fn autonomy_priority(preference: AutonomyPreference) -> &'static str {
    match preference {
        AutonomyPreference::Cautious => "在编辑文件、运行命令或产品选择不明确前，倾向先停下询问。",
        AutonomyPreference::Balanced => {
            "清晰低风险任务可直接行动；遇到风险、破坏性或歧义时先确认。"
        }
        AutonomyPreference::Autonomous => {
            "可批量处理安全的常规工作，但遇到破坏性、凭据、发布、高成本、法律或安全风险时停止询问。"
        }
        AutonomyPreference::Unspecified => "未选择常设主动性偏好。",
    }
}

fn authority_priority() -> &'static str {
    "当前用户请求和实时工具证据优先于记忆、陈旧交接和猜测。"
}

fn bounded_freeform_note(input: &str, max_chars: usize) -> String {
    input
        .chars()
        .filter_map(|ch| {
            if ch == '\t' {
                Some(' ')
            } else if ch == '\n' || !ch.is_control() {
                Some(ch)
            } else {
                None
            }
        })
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn compact_freeform_preview(note: &str) -> String {
    let compact = note.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = compact.chars().take(96).collect::<String>();
    if compact.chars().count() > 96 {
        preview.push_str("...");
    }
    preview
}

fn freeform_note_line(note: &str, editing: bool) -> Line<'static> {
    let preview = compact_freeform_preview(note);
    let text = match (editing, preview.is_empty()) {
        (true, true) => "F 自由原则：正在编辑 - 输入或粘贴有界原则，Enter 完成".to_string(),
        (true, false) => format!("F 自由原则：正在编辑 - {preview}"),
        (false, true) => "F 自由原则：按 F 输入或粘贴自己的有界原则".to_string(),
        (false, false) => format!("F 自由原则：{preview}"),
    };
    let style = if editing || !preview.is_empty() {
        Style::default().fg(palette::WHALE_ACCENT_PRIMARY)
    } else {
        Style::default().fg(palette::TEXT_MUTED)
    };
    Line::from(Span::styled(text, style))
}

impl SetupWizardView {
    #[cfg(test)]
    #[must_use]
    pub fn new(state: SetupState) -> Self {
        let selected = initial_step_index(&state);
        Self {
            state,
            selected,
            facts: SetupRuntimeFacts::default(),
            guided_draft: GuidedConstitutionDraft::default(),
            freeform_note: String::new(),
            editing_freeform_note: false,
            guided_preview_seen: false,
            existing_preview_seen: false,
            model_draft: None,
            model_draft_label: None,
            runtime_preset: SetupRuntimePreset::default(),
            runtime_preset_preview_seen: false,
            body_scroll: 0,
        }
    }

    #[must_use]
    pub fn new_for_app(app: &App, config: &Config) -> Self {
        Self::new_with_facts(
            load_setup_state_for_app(app, config),
            SetupRuntimeFacts::from_app_config(app, config),
        )
    }

    #[must_use]
    pub fn new_for_app_at(app: &App, config: &Config, step: SetupStep) -> Self {
        Self::new_at_with_facts(
            load_setup_state_for_app(app, config),
            step,
            SetupRuntimeFacts::from_app_config(app, config),
        )
    }

    #[cfg(test)]
    #[must_use]
    pub fn state(&self) -> &SetupState {
        &self.state
    }

    #[must_use]
    pub fn selected_step(&self) -> SetupStep {
        STEP_SPECS[self.selected].id()
    }

    fn selected_spec(&self) -> &'static dyn SetupWizardStep {
        &STEP_SPECS[self.selected]
    }

    fn new_with_facts(state: SetupState, facts: SetupRuntimeFacts) -> Self {
        let selected = initial_step_index(&state);
        Self {
            state,
            selected,
            facts,
            guided_draft: GuidedConstitutionDraft::default(),
            freeform_note: String::new(),
            editing_freeform_note: false,
            guided_preview_seen: false,
            existing_preview_seen: false,
            model_draft: None,
            model_draft_label: None,
            runtime_preset: SetupRuntimePreset::default(),
            runtime_preset_preview_seen: false,
            body_scroll: 0,
        }
    }

    fn new_at_with_facts(state: SetupState, step: SetupStep, facts: SetupRuntimeFacts) -> Self {
        Self {
            state,
            selected: visible_step_index(step),
            facts,
            guided_draft: GuidedConstitutionDraft::default(),
            freeform_note: String::new(),
            editing_freeform_note: false,
            guided_preview_seen: false,
            existing_preview_seen: false,
            model_draft: None,
            model_draft_label: None,
            runtime_preset: SetupRuntimePreset::default(),
            runtime_preset_preview_seen: false,
            body_scroll: 0,
        }
    }

    fn move_next(&mut self) {
        self.selected = (self.selected + 1).min(STEP_SPECS.len().saturating_sub(1));
        self.body_scroll = 0;
    }

    fn move_back(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.body_scroll = 0;
    }

    fn commit_selected_status(
        &mut self,
        status: StepStatus,
        message_id: MessageId,
        advance: bool,
    ) -> ViewAction {
        let spec = self.selected_spec();
        let result = match status {
            StepStatus::Skipped => Some("skipped by user"),
            StepStatus::NeedsAction => Some("retry requested; needs action"),
            _ => None,
        };
        let mut entry = StepEntry::new(status, spec.required(), CONSTITUTION_CHECKPOINT_VERSION);
        if let Some(result) = result {
            entry = entry.with_result(result);
        }
        let mut state = self.state.clone();
        state.set_step(spec.id(), entry);
        self.state = state.clone();
        if advance {
            self.move_next();
        }
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(message_id).to_string(),
        })
    }

    fn commit_provider_model_review(&mut self) -> ViewAction {
        let status = provider::step_status(self.facts.provider_ready);
        let mut state = self.state.clone();
        state.set_step(
            SetupStep::ProviderModel,
            provider::step_entry(
                self.facts.provider_ready,
                CONSTITUTION_CHECKPOINT_VERSION,
                self.facts.provider_result.clone(),
            ),
        );
        self.state = state.clone();
        self.move_next();
        let message_id = if status == StepStatus::Verified {
            MessageId::SetupProviderModelReviewed
        } else {
            MessageId::SetupProviderModelNeedsActionSaved
        };
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(message_id).to_string(),
        })
    }

    fn commit_runtime_posture_review(&mut self) -> ViewAction {
        let mut state = self.state.clone();
        state.runtime_posture_source = RuntimePostureSource::Confirmed;
        state.set_step(
            SetupStep::TrustSandbox,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(self.facts.runtime_result.clone()),
        );
        self.state = state.clone();
        self.move_next();
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(MessageId::SetupRuntimePostureReviewed).to_string(),
        })
    }

    fn operate_fleet_facts_ready(&self) -> bool {
        // Provider, capacity, and roster facts are configuration snapshots,
        // not proof of dispatch and terminal receipts. This release must never
        // persist an Operate-ready claim from those facts alone.
        false
    }

    fn commit_operate_fleet_review(&mut self) -> ViewAction {
        let status = if self.operate_fleet_facts_ready() {
            StepStatus::Verified
        } else {
            StepStatus::NeedsAction
        };
        let mut state = self.state.clone();
        state.set_step(
            SetupStep::OperateFleet,
            StepEntry::new(status, false, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(self.facts.operate_result.clone()),
        );
        self.state = state.clone();
        self.move_next();
        let message_id = if status == StepStatus::Verified {
            MessageId::SetupOperateReviewed
        } else {
            MessageId::SetupOperateNeedsActionSaved
        };
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(message_id).to_string(),
        })
    }

    fn commit_tools_mcp_review(&mut self) -> ViewAction {
        // Optional step: empty/off inventories settle as Optional; broken
        // configured tools record NeedsAction without blocking first-run.
        let status = if self.facts.tools_mcp_needs_action {
            StepStatus::NeedsAction
        } else if self.facts.tools_mcp_result.contains("overall=off") {
            StepStatus::Optional
        } else {
            StepStatus::Verified
        };
        let mut state = self.state.clone();
        state.set_step(
            SetupStep::ToolsMcp,
            StepEntry::new(status, false, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(self.facts.tools_mcp_result.clone()),
        );
        self.state = state.clone();
        self.move_next();
        let message_id = if status == StepStatus::NeedsAction {
            MessageId::SetupToolsMcpNeedsActionSaved
        } else {
            MessageId::SetupToolsMcpReviewed
        };
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(message_id).to_string(),
        })
    }

    fn preview_tools_mcp_on_ramp(&self) -> ViewAction {
        ViewAction::Emit(ViewEvent::OpenTextPager {
            title: tr(MessageId::SetupToolsMcpPreviewTitle).to_string(),
            content: tools_mcp_on_ramp_text(&self.facts),
        })
    }

    fn commit_persistence_review(&mut self) -> ViewAction {
        let mut state = self.state.clone();
        state.set_step(
            SetupStep::Persistence,
            StepEntry::new(StepStatus::Verified, false, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(self.facts.persistence.result.clone()),
        );
        self.state = state.clone();
        self.move_next();
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(MessageId::SetupPersistenceReviewed).to_string(),
        })
    }

    fn select_runtime_preset(&mut self, key: char) -> ViewAction {
        if let Some(preset) = SetupRuntimePreset::from_key(key)
            && preset != self.runtime_preset
        {
            self.runtime_preset = preset;
            self.runtime_preset_preview_seen = false;
        }
        ViewAction::None
    }

    fn preview_runtime_preset(&mut self) -> ViewAction {
        self.runtime_preset_preview_seen = true;
        ViewAction::Emit(ViewEvent::OpenTextPager {
            title: tr(MessageId::SetupRuntimePresetPreviewTitle).to_string(),
            content: runtime_preset_preview_text(self.runtime_preset, &self.facts),
        })
    }

    fn commit_runtime_preset(&mut self) -> ViewAction {
        if !self.runtime_preset_preview_seen {
            return self.preview_runtime_preset();
        }

        let mut state = self.state.clone();
        state.runtime_posture_source = RuntimePostureSource::Confirmed;
        state.set_step(
            SetupStep::TrustSandbox,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(self.runtime_preset.result_summary()),
        );
        self.state = state.clone();
        self.move_next();
        ViewAction::Emit(ViewEvent::SetupRuntimePresetApplyRequested {
            preset: self.runtime_preset,
            state,
            message: tr(MessageId::SetupRuntimePresetApplied).to_string(),
        })
    }

    fn commit_setup_report(&mut self) -> ViewAction {
        let mut state = self.state.clone();
        let status = if setup_report_ready(&state) {
            StepStatus::Verified
        } else {
            StepStatus::NeedsAction
        };
        state.set_step(
            SetupStep::Verification,
            StepEntry::new(status, false, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(setup_report_result(&state, &self.facts)),
        );
        self.state = state.clone();
        ViewAction::Emit(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(MessageId::SetupReportRecorded).to_string(),
        })
    }

    fn commit_guided_constitution(&mut self) -> ViewAction {
        if !self.guided_preview_seen {
            return self.preview_guided_constitution();
        }

        let (constitution, authoring) = match self.model_draft.as_deref() {
            // Model drafts arrive sanitized + bounded from the untrusted-JSON
            // gate; ratify exactly what was previewed.
            Some(draft) => (draft.clone(), ConstitutionAuthoring::ModelDrafted),
            None => (
                self.guided_draft
                    .to_constitution_with_freeform(self.freeform_note_for_draft()),
                ConstitutionAuthoring::Guided,
            ),
        };
        let mut state = self.state.clone();
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::GuidedCustom,
        );
        state.constitution_source = ConstitutionSource::UserGlobal;
        state.constitution_validity = ConstitutionValidity::Valid;
        state.constitution_authoring = Some(authoring);
        state.constitution_preview_hash = Some(constitution.preview_hash());
        state.constitution_preview_version =
            state.constitution_preview_version.saturating_add(1).max(1);
        let hash = state
            .constitution_preview_hash
            .as_deref()
            .unwrap_or("unknown");
        let result = match authoring {
            ConstitutionAuthoring::ModelDrafted => format!(
                "model-drafted constitution ratified ({}) preview_hash={hash}",
                self.model_draft_label.as_deref().unwrap_or("model")
            ),
            ConstitutionAuthoring::Guided => {
                format!("guided custom constitution preview_hash={hash}")
            }
        };
        state.set_step(
            SetupStep::Constitution,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(result),
        );
        self.state = state.clone();
        ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            message: tr(MessageId::SetupCheckpointDoneGuided).to_string(),
        })
    }

    fn preview_guided_constitution(&mut self) -> ViewAction {
        self.guided_preview_seen = true;
        let (constitution, provenance) = match self.model_draft.as_deref() {
            Some(draft) => (
                draft.clone(),
                DraftProvenance::Model(
                    self.model_draft_label
                        .clone()
                        .unwrap_or_else(|| "model".to_string()),
                ),
            ),
            None => (
                self.guided_draft
                    .to_constitution_with_freeform(self.freeform_note_for_draft()),
                DraftProvenance::Guided,
            ),
        };
        ViewAction::Emit(ViewEvent::OpenTextPager {
            title: ratification_preview_title().to_string(),
            content: constitution_ratification_text(&constitution, &provenance),
        })
    }

    fn cycle_guided_answer(&mut self, key: char) -> ViewAction {
        if self.guided_draft.cycle(key) {
            self.guided_preview_seen = false;
            // Answers changed under the draft: the model draft is stale law
            // and must be re-drafted or replaced by the guided rendering.
            self.model_draft = None;
            self.model_draft_label = None;
        }
        ViewAction::None
    }

    /// `A` on the constitution step: ask the first configured model to draft.
    /// Requires a ready provider route; otherwise the key is inert and the
    /// deterministic guided flow stands untouched.
    fn request_model_draft(&self) -> ViewAction {
        if !self.facts.provider_ready {
            return ViewAction::None;
        }
        ViewAction::Emit(ViewEvent::SetupConstitutionModelDraftRequested {
            draft: self.guided_draft,
            freeform_note: self.freeform_note_for_draft().map(str::to_string),
        })
    }

    fn toggle_freeform_edit(&mut self) -> ViewAction {
        if self.selected_step() == SetupStep::Constitution {
            self.editing_freeform_note = !self.editing_freeform_note;
        }
        ViewAction::None
    }

    fn freeform_note_for_draft(&self) -> Option<&str> {
        let note = self.freeform_note.trim();
        (!note.is_empty()).then_some(note)
    }

    fn append_freeform_note_text(&mut self, text: &str) {
        let mut next = self.freeform_note.clone();
        next.push_str(text);
        self.freeform_note = bounded_freeform_note(&next, MAX_NOTES_LEN);
        self.guided_preview_seen = false;
        self.model_draft = None;
        self.model_draft_label = None;
    }

    fn handle_freeform_note_key(&mut self, key: KeyEvent) -> Option<ViewAction> {
        if self.selected_step() != SetupStep::Constitution || !self.editing_freeform_note {
            return None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.editing_freeform_note = false;
                Some(ViewAction::None)
            }
            KeyCode::Backspace => {
                self.freeform_note.pop();
                self.guided_preview_seen = false;
                self.model_draft = None;
                self.model_draft_label = None;
                Some(ViewAction::None)
            }
            KeyCode::Char(c) if key.modifiers.is_empty() => {
                let mut buf = [0; 4];
                self.append_freeform_note_text(c.encode_utf8(&mut buf));
                Some(ViewAction::None)
            }
            _ => Some(ViewAction::None),
        }
    }

    /// Install a model-drafted constitution (already sanitized + bounded by
    /// the untrusted-JSON gate) and return the `(title, content)` of the
    /// ratification preview the host must open in the same breath — that is
    /// what satisfies the preview gate. Ratifying still takes the explicit
    /// `G` keypress afterwards.
    #[must_use]
    pub(crate) fn install_model_draft(
        &mut self,
        constitution: Box<UserConstitution>,
        model_label: String,
    ) -> (String, String) {
        let content = constitution_ratification_text(
            &constitution,
            &DraftProvenance::Model(model_label.clone()),
        );
        self.model_draft = Some(constitution);
        self.model_draft_label = Some(model_label);
        self.guided_preview_seen = true;
        (ratification_preview_title().to_string(), content)
    }

    fn commit_constitution(&self, kind: SetupCommitKind) -> ViewAction {
        let choice = match kind {
            SetupCommitKind::BundledConstitution => ConstitutionChoice::Bundled,
            SetupCommitKind::DeferredConstitution => ConstitutionChoice::Deferred,
        };
        let mut state = self.state.clone();
        state.complete_constitution_checkpoint(CONSTITUTION_CHECKPOINT_VERSION, choice);
        state.constitution_source = ConstitutionSource::Bundled;
        state.constitution_validity = ConstitutionValidity::Unknown;
        state.constitution_authoring = None;
        state.constitution_preview_hash = None;
        state.set_step(
            SetupStep::Constitution,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result(match kind {
                    SetupCommitKind::BundledConstitution => "bundled/default constitution",
                    SetupCommitKind::DeferredConstitution => "checkpoint deferred; bundled applies",
                }),
        );
        let message_id = match kind {
            SetupCommitKind::BundledConstitution => MessageId::SetupCheckpointDoneBundled,
            SetupCommitKind::DeferredConstitution => MessageId::SetupCheckpointDeferred,
        };
        ViewAction::EmitAndClose(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(message_id).to_string(),
        })
    }

    /// Complete the checkpoint by keeping the existing valid
    /// `constitution.json` exactly as it stands (#3794). First `K` previews
    /// the rendered law; second `K` records the choice. The file is never
    /// rewritten — only `setup_state.json` changes, through the same commit
    /// event as every other completion.
    fn commit_keep_existing_constitution(&mut self) -> ViewAction {
        if self.facts.constitution_file != SetupConstitutionFileState::Loaded {
            return ViewAction::None;
        }
        // Re-read the live file so a stale card cannot ratify a file that
        // has since become invalid; any non-loaded state leaves the key inert.
        let Ok(load) = UserConstitution::load() else {
            return ViewAction::None;
        };
        let Some(constitution) = load.constitution() else {
            return ViewAction::None;
        };
        if !self.existing_preview_seen {
            self.existing_preview_seen = true;
            let content = constitution_ratification_text(constitution, &DraftProvenance::Existing);
            return ViewAction::Emit(ViewEvent::OpenTextPager {
                title: ratification_preview_title().to_string(),
                content,
            });
        }
        let mut state = self.state.clone();
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::GuidedCustom,
        );
        state.constitution_source = ConstitutionSource::UserGlobal;
        state.constitution_validity = ConstitutionValidity::Valid;
        state.constitution_preview_hash = Some(constitution.preview_hash());
        state.set_step(
            SetupStep::Constitution,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION)
                .with_result("existing constitution kept unchanged"),
        );
        ViewAction::EmitAndClose(ViewEvent::SetupStateCommitRequested {
            state,
            message: tr(MessageId::SetupCheckpointDoneKept).to_string(),
        })
    }

    fn status_label(&self, status: StepStatus) -> Cow<'static, str> {
        tr(match status {
            StepStatus::NotStarted => MessageId::SetupStatusNotStarted,
            StepStatus::Recommended => MessageId::SetupStatusRecommended,
            StepStatus::Optional => MessageId::SetupStatusOptional,
            StepStatus::Deferred => MessageId::SetupStatusDeferred,
            StepStatus::InProgress => MessageId::SetupStatusInProgress,
            StepStatus::NeedsAction => MessageId::SetupStatusNeedsAction,
            StepStatus::Verified => MessageId::SetupStatusVerified,
            StepStatus::Skipped => MessageId::SetupStatusSkipped,
            StepStatus::Failed => MessageId::SetupStatusFailed,
        })
    }
}

impl ModalView for SetupWizardView {
    fn kind(&self) -> ModalKind {
        ModalKind::SetupWizard
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        if let Some(action) = self.handle_freeform_note_key(key) {
            return action;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Left | KeyCode::Char('b') => {
                self.move_back();
                ViewAction::None
            }
            KeyCode::Right | KeyCode::Char('n') => {
                self.move_next();
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.body_scroll = self.body_scroll.saturating_sub(8);
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.body_scroll = self.body_scroll.saturating_add(8);
                ViewAction::None
            }
            KeyCode::Up => {
                self.move_back();
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_next();
                ViewAction::None
            }
            KeyCode::Char('s') => {
                self.commit_selected_status(StepStatus::Skipped, MessageId::SetupStepSkipped, true)
            }
            KeyCode::Char('r') if self.selected_step() == SetupStep::ToolsMcp => {
                self.preview_tools_mcp_on_ramp()
            }
            KeyCode::Char('r') => self.commit_selected_status(
                StepStatus::NeedsAction,
                MessageId::SetupStepRetryRecorded,
                false,
            ),
            KeyCode::Char('g') if self.selected_step() == SetupStep::Constitution => {
                self.commit_guided_constitution()
            }
            KeyCode::Char('p') if self.selected_step() == SetupStep::ProviderModel => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenProviderRequested)
            }
            KeyCode::Char('m') if self.selected_step() == SetupStep::ProviderModel => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenModelRequested)
            }
            KeyCode::Char('p') if self.selected_step() == SetupStep::OperateFleet => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenProviderRequested)
            }
            KeyCode::Char('f') if self.selected_step() == SetupStep::OperateFleet => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenFleetRequested)
            }
            KeyCode::Char('m') if self.selected_step() == SetupStep::TrustSandbox => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenModeRequested)
            }
            KeyCode::Char('c') if self.selected_step() == SetupStep::TrustSandbox => {
                ViewAction::EmitAndClose(ViewEvent::SetupOpenConfigRequested)
            }
            KeyCode::Char(key @ ('1' | '2' | '3'))
                if self.selected_step() == SetupStep::TrustSandbox =>
            {
                self.select_runtime_preset(key)
            }
            KeyCode::Char('a') if self.selected_step() == SetupStep::TrustSandbox => {
                self.commit_runtime_preset()
            }
            KeyCode::Char(key @ ('1' | '2' | '3' | '4' | '5' | '6'))
                if self.selected_step() == SetupStep::Constitution =>
            {
                self.cycle_guided_answer(key)
            }
            KeyCode::Char('a') if self.selected_step() == SetupStep::Constitution => {
                self.request_model_draft()
            }
            KeyCode::Char('f') if self.selected_step() == SetupStep::Constitution => {
                self.toggle_freeform_edit()
            }
            KeyCode::Char('k') if self.selected_step() == SetupStep::Constitution => {
                self.commit_keep_existing_constitution()
            }
            KeyCode::Char('u') => self.commit_constitution(SetupCommitKind::BundledConstitution),
            KeyCode::Char('d') => self.commit_constitution(SetupCommitKind::DeferredConstitution),
            KeyCode::Enter if self.selected_step() == SetupStep::Constitution => {
                self.commit_constitution(SetupCommitKind::BundledConstitution)
            }
            KeyCode::Enter if self.selected_step() == SetupStep::ProviderModel => {
                self.commit_provider_model_review()
            }
            KeyCode::Enter if self.selected_step() == SetupStep::TrustSandbox => {
                self.commit_runtime_posture_review()
            }
            KeyCode::Enter if self.selected_step() == SetupStep::OperateFleet => {
                self.commit_operate_fleet_review()
            }
            KeyCode::Enter if self.selected_step() == SetupStep::ToolsMcp => {
                self.commit_tools_mcp_review()
            }
            KeyCode::Enter if self.selected_step() == SetupStep::Persistence => {
                self.commit_persistence_review()
            }
            KeyCode::Enter if self.selected_step() == SetupStep::Verification => {
                self.commit_setup_report()
            }
            KeyCode::Enter => {
                self.move_next();
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn handle_paste(&mut self, text: &str) -> bool {
        if self.selected_step() != SetupStep::Constitution {
            return false;
        }
        self.append_freeform_note_text(text);
        true
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let progress = format!(
            "{} {}/{}",
            tr(MessageId::SetupWizardProgress),
            self.selected + 1,
            STEP_SPECS.len()
        );
        let inner = render_underwater_surface(
            area,
            buf,
            format!("{} · {progress}", tr(MessageId::SetupWizardTitle)),
        );
        let mut hints = vec![
            ActionHint::new("B", tr(MessageId::SetupActionBack).to_string()),
            ActionHint::new("N", tr(MessageId::SetupActionContinue).to_string()),
            ActionHint::new("S", tr(MessageId::SetupActionSkip).to_string()),
            ActionHint::new("R", tr(MessageId::SetupActionRetry).to_string()),
            ActionHint::new("PgUp/Dn", tr(MessageId::SetupActionScrollBody).to_string()),
        ];
        if self.selected_step() == SetupStep::Constitution {
            hints.push(ActionHint::new(
                "1-6",
                tr(MessageId::SetupActionTuneGuided).to_string(),
            ));
            if self.facts.provider_ready {
                hints.push(ActionHint::new(
                    "A",
                    tr(MessageId::SetupActionModelDraft).to_string(),
                ));
            }
            hints.push(ActionHint::new(
                "G",
                tr(MessageId::SetupActionGuided).to_string(),
            ));
            hints.push(ActionHint::new(
                "F",
                tr(MessageId::SetupActionFreeform).to_string(),
            ));
            if self.facts.constitution_file == SetupConstitutionFileState::Loaded {
                hints.push(ActionHint::new(
                    "K",
                    tr(MessageId::SetupActionKeepExisting).to_string(),
                ));
            }
        } else if self.selected_step() == SetupStep::ProviderModel {
            hints.push(ActionHint::new(
                "P",
                tr(MessageId::SetupActionProvider).to_string(),
            ));
            hints.push(ActionHint::new(
                "M",
                tr(MessageId::SetupActionModel).to_string(),
            ));
        } else if self.selected_step() == SetupStep::OperateFleet {
            hints.push(ActionHint::new(
                "P",
                tr(MessageId::SetupActionProvider).to_string(),
            ));
            hints.push(ActionHint::new(
                "F",
                tr(MessageId::SetupActionFleet).to_string(),
            ));
        } else if self.selected_step() == SetupStep::TrustSandbox {
            hints.push(ActionHint::new(
                "1-3",
                tr(MessageId::SetupActionRuntimePreset).to_string(),
            ));
            hints.push(ActionHint::new(
                "A",
                tr(MessageId::SetupActionApplyRuntimePreset).to_string(),
            ));
            hints.push(ActionHint::new(
                "M",
                tr(MessageId::SetupActionMode).to_string(),
            ));
            hints.push(ActionHint::new(
                "C",
                tr(MessageId::SetupActionConfig).to_string(),
            ));
        }
        hints.extend([
            ActionHint::new("U", tr(MessageId::SetupActionUseBundled).to_string()),
            ActionHint::new("D", tr(MessageId::SetupActionDefer).to_string()),
            ActionHint::new("Esc", tr(MessageId::SetupActionCancel).to_string()),
        ]);
        let content_area = render_modal_footer(inner, buf, &hints);
        let spec = self.selected_spec();
        let mut lines = vec![
            Line::from(Span::styled(
                tr(spec.title_id()).to_string(),
                Style::default()
                    .fg(palette::WHALE_INFO)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::raw(tr(spec.why_id()).to_string())),
            Line::from(""),
        ];
        lines.extend(self.selected_step_detail_lines());
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            tr(MessageId::SetupWizardWhy).to_string(),
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines.push(Line::from(""));
        for (idx, step) in STEP_SPECS.iter().enumerate() {
            let selected = idx == self.selected;
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::TEXT_MUTED)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(tr(step.title_id()).to_string(), style),
                Span::raw("  "),
                Span::styled(
                    self.status_label(self.state.status(step.id())).to_string(),
                    Style::default().fg(palette::WHALE_ACCENT_PRIMARY),
                ),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::raw(
            tr(MessageId::SetupCheckpointLayerOrder).to_string(),
        )));
        let wrap_width = usize::from(content_area.width).max(1);
        let visual_rows: usize = lines
            .iter()
            .map(|line| line.width().div_ceil(wrap_width).max(1))
            .sum();
        let visible_rows = usize::from(content_area.height).max(1);
        let max_scroll = visual_rows.saturating_sub(visible_rows);
        let scroll = self.body_scroll.min(max_scroll);
        let content_area =
            render_panel_scroll_rail(content_area, buf, visual_rows, scroll, visible_rows, true);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0))
            .render(content_area, buf);
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl SetupWizardView {
    fn selected_step_detail_lines(&self) -> Vec<Line<'static>> {
        match self.selected_step() {
            SetupStep::ProviderModel => self.provider_model_detail_lines(),
            SetupStep::TrustSandbox => self.runtime_posture_detail_lines(),
            SetupStep::Constitution => self.constitution_detail_lines(),
            SetupStep::OperateFleet => self.operate_fleet_detail_lines(),
            SetupStep::ToolsMcp => self.tools_mcp_detail_lines(),
            SetupStep::Persistence => self.persistence_detail_lines(),
            SetupStep::Verification => self.verification_detail_lines(),
        }
    }

    fn provider_model_detail_lines(&self) -> Vec<Line<'static>> {
        vec![
            self.detail_row(MessageId::SetupCardRouteLabel, &self.facts.provider),
            self.detail_row(MessageId::SetupCardModelLabel, &self.facts.model),
            self.detail_row(MessageId::SetupCardAuthLabel, &self.facts.auth),
            self.detail_row(MessageId::SetupCardHealthLabel, &self.facts.health),
            Line::from(Span::styled(
                tr(if self.facts.provider_ready {
                    MessageId::SetupProviderModelReadyHint
                } else {
                    MessageId::SetupProviderModelNeedsActionHint
                })
                .to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            )),
        ]
    }

    fn constitution_detail_lines(&self) -> Vec<Line<'static>> {
        let choice = constitution_choice_label(self.state.constitution_choice);
        let source = constitution_source_label(self.state.constitution_source);
        let validity = constitution_validity_label(self.state.constitution_validity);
        let source_state = format!("{source}; validity {validity}");
        let existing_file = self
            .facts
            .constitution_file
            .label(self.state.constitution_choice);
        let expert_override = self.facts.expert_override.label();
        let preview = self
            .state
            .constitution_preview_hash
            .as_deref()
            .unwrap_or("not accepted yet")
            .to_string();
        let mut lines = vec![
            self.detail_row(MessageId::SetupConstitutionChoiceLabel, choice),
            self.detail_row(MessageId::SetupConstitutionSourceLabel, &source_state),
            self.detail_row(MessageId::SetupConstitutionPreviewLabel, &preview),
            self.detail_row(MessageId::SetupConstitutionExistingLabel, &existing_file),
            self.detail_row(
                MessageId::SetupConstitutionExpertOverrideLabel,
                &expert_override,
            ),
            Line::from(Span::styled(
                tr(MessageId::SetupConstitutionGuidedAnswersHint).to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            )),
            self.guided_answer_pair(
                (
                    "1",
                    MessageId::SetupConstitutionPurposeLabel,
                    &self.guided_draft.purpose.label(),
                ),
                (
                    "2",
                    MessageId::SetupConstitutionAutonomyLabel,
                    autonomy_label(self.guided_draft.autonomy),
                ),
            ),
            self.guided_answer_pair(
                (
                    "3",
                    MessageId::SetupConstitutionEvidenceLabel,
                    &self.guided_draft.evidence.label(),
                ),
                (
                    "4",
                    MessageId::SetupConstitutionCommunicationLabel,
                    self.guided_draft.communication.label(),
                ),
            ),
            self.guided_answer_single(
                "5",
                MessageId::SetupConstitutionPrivacyLabel,
                self.guided_draft.privacy.label(),
            ),
            self.guided_answer_single(
                "6",
                MessageId::SetupConstitutionPrinciplesLabel,
                self.guided_draft.principles.label(),
            ),
            freeform_note_line(&self.freeform_note, self.editing_freeform_note),
        ];
        if self.facts.constitution_file == SetupConstitutionFileState::Loaded {
            lines.push(Line::from(Span::styled(
                keep_existing_invitation_line(),
                Style::default().fg(palette::WHALE_ACCENT_PRIMARY),
            )));
        }
        if let Some(label) = self
            .model_draft_label
            .as_deref()
            .filter(|_| self.model_draft.is_some())
        {
            lines.push(Line::from(Span::styled(
                model_draft_ready_line(label),
                Style::default().fg(palette::WHALE_ACCENT_PRIMARY),
            )));
        } else if self.facts.provider_ready {
            lines.push(Line::from(Span::styled(
                model_draft_invitation_line(&self.facts.model),
                Style::default().fg(palette::WHALE_ACCENT_PRIMARY),
            )));
        }
        lines.push(Line::from(Span::styled(
            tr(MessageId::SetupConstitutionGuidedHint).to_string(),
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines
    }

    fn runtime_posture_detail_lines(&self) -> Vec<Line<'static>> {
        let project_override = self
            .facts
            .project_override_warning
            .clone()
            .unwrap_or_else(|| tr(MessageId::SetupRuntimeProjectOverrideNone).to_string());
        let mut lines = vec![
            self.detail_row(MessageId::SetupCardIntentLabel, &self.facts.work_intent),
            self.detail_row(MessageId::SetupCardApprovalLabel, &self.facts.approval),
            self.detail_row(MessageId::SetupCardShellLabel, &self.facts.shell),
            self.detail_row(MessageId::SetupCardTrustLabel, &self.facts.trust),
            self.detail_row(MessageId::SetupCardSandboxLabel, &self.facts.sandbox),
            self.detail_row(MessageId::SetupCardNetworkLabel, &self.facts.network),
            self.detail_row(
                MessageId::SetupRuntimePresetSelectedLabel,
                &runtime_preset_summary(self.runtime_preset),
            ),
            self.detail_row(
                MessageId::SetupRuntimePresetDiffLabel,
                &runtime_preset_inline_diff(self.runtime_preset, &self.facts),
            ),
            self.detail_row(
                MessageId::SetupRuntimeProjectOverrideLabel,
                &project_override,
            ),
            Line::from(Span::styled(
                tr(MessageId::SetupRuntimePostureBoundary).to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            )),
            Line::from(Span::styled(
                tr(MessageId::SetupRuntimePresetSafetyFloor).to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            )),
            self.setup_review_hint_line(MessageId::SetupRuntimePostureReviewHint),
            Line::from(Span::styled(
                tr(MessageId::SetupRuntimePresetApplyHint).to_string(),
                Style::default().fg(palette::TEXT_MUTED),
            )),
        ];
        for (idx, preset) in SetupRuntimePreset::ALL.iter().enumerate() {
            let marker = if *preset == self.runtime_preset {
                ">"
            } else {
                " "
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {}. {}", idx + 1, runtime_preset_summary(*preset)),
                Style::default().fg(if *preset == self.runtime_preset {
                    palette::TEXT_PRIMARY
                } else {
                    palette::TEXT_MUTED
                }),
            )));
        }
        lines
    }

    fn operate_fleet_detail_lines(&self) -> Vec<Line<'static>> {
        let route = format!("{} / {}", self.facts.provider, self.facts.model);
        let readiness = self.ready_label(self.operate_fleet_facts_ready());
        vec![
            self.detail_row(MessageId::SetupCardRouteLabel, &route),
            self.detail_row(MessageId::SetupCardAuthLabel, &self.facts.auth),
            self.detail_row(
                MessageId::SetupOperateRuntimeLabel,
                &self.facts.operate_runtime_result,
            ),
            self.detail_row(
                MessageId::SetupOperateRosterLabel,
                &self.facts.fleet_roster_result,
            ),
            self.detail_row(
                MessageId::SetupOperateConcurrencyLabel,
                &self.facts.operate_concurrency_result,
            ),
            self.detail_row(MessageId::SetupOperateReadinessLabel, &readiness),
            self.setup_review_hint_line(MessageId::SetupOperateReviewHint),
        ]
    }

    fn tools_mcp_detail_lines(&self) -> Vec<Line<'static>> {
        vec![
            self.detail_row(
                MessageId::SetupToolsMcpServersLabel,
                &self.facts.tools_mcp_servers_result,
            ),
            self.detail_row(
                MessageId::SetupToolsMcpSkillsLabel,
                &self.facts.tools_mcp_skills_result,
            ),
            self.detail_row(
                MessageId::SetupToolsMcpToolsLabel,
                &self.facts.tools_mcp_tools_result,
            ),
            self.detail_row(
                MessageId::SetupToolsMcpPluginsLabel,
                &self.facts.tools_mcp_plugins_result,
            ),
            self.setup_review_hint_line(MessageId::SetupToolsMcpReviewHint),
        ]
    }

    fn persistence_detail_lines(&self) -> Vec<Line<'static>> {
        vec![
            self.detail_row(
                MessageId::SetupPersistenceHomeLabel,
                &self.facts.persistence.home_result,
            ),
            self.detail_row(
                MessageId::SetupPersistenceConfigLabel,
                &self.facts.persistence.config_result,
            ),
            self.detail_row(
                MessageId::SetupPersistenceStateLabel,
                &self.facts.persistence.state_result,
            ),
            self.detail_row(
                MessageId::SetupPersistenceConstitutionLabel,
                &self.facts.persistence.constitution_result,
            ),
            self.detail_row(
                MessageId::SetupPersistenceMemoryLabel,
                &self.facts.persistence.memory_result,
            ),
            self.detail_row(
                MessageId::SetupPersistenceNotesLabel,
                &self.facts.persistence.notes_result,
            ),
            self.setup_review_hint_line(MessageId::SetupPersistenceReviewHint),
        ]
    }

    fn verification_detail_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            self.detail_row(
                MessageId::SetupReportFirstRunLabel,
                &self.ready_label(self.state.first_run_ready()),
            ),
            self.detail_row(
                MessageId::SetupReportUpdateLabel,
                &self.ready_label(self.state.update_ready(CONSTITUTION_CHECKPOINT_VERSION)),
            ),
            self.detail_row(
                MessageId::SetupReportOperateLabel,
                &self.ready_label(self.state.operate_ready()),
            ),
            self.detail_row(
                MessageId::SetupReportSourceLabel,
                &self.state_source_label(),
            ),
            self.detail_row(
                MessageId::SetupReportAutonomyLabel,
                &self.facts.constitution_autonomy,
            ),
            self.detail_row(
                MessageId::SetupReportRuntimePostureLabel,
                &self.facts.runtime_result,
            ),
            Line::from(""),
            Line::from(Span::styled(
                tr(MessageId::SetupReportRowsLabel).to_string(),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            )),
        ];

        for spec in STEP_SPECS {
            let step = spec.id();
            let entry = self.state.steps.get(&step);
            let required = entry.map_or(spec.required(), |entry| entry.required);
            let required_label = if required {
                tr(MessageId::SetupReportRequired)
            } else {
                tr(MessageId::SetupReportOptional)
            };
            let mut value = format!(
                "{} ({})",
                self.status_label(self.state.status(step)),
                required_label
            );
            if let Some(version) = entry.and_then(|entry| entry.version.as_deref()) {
                value.push_str(&format!(" · {version}"));
            }
            if let Some(result) = entry.and_then(|entry| entry.result.as_deref()) {
                value.push_str(&format!(" · {result}"));
            }
            lines.push(self.detail_row(spec.title_id(), &value));
        }

        lines.push(Line::from(""));
        let next_action = tr(self.next_action_id()).to_string();
        lines.push(self.detail_row(MessageId::SetupReportNextActionLabel, &next_action));
        lines
    }

    fn setup_review_hint_line(&self, hint_id: MessageId) -> Line<'static> {
        let hint = tr(hint_id).to_string();
        Line::from(Span::styled(hint, Style::default().fg(palette::TEXT_MUTED)))
    }

    fn ready_label(&self, ready: bool) -> String {
        if ready {
            tr(MessageId::SetupReportReady).to_string()
        } else {
            tr(MessageId::SetupStatusNeedsAction).to_string()
        }
    }

    fn state_source_label(&self) -> String {
        if self.state.inherited {
            tr(MessageId::SetupReportInherited).to_string()
        } else {
            tr(MessageId::SetupReportPersisted).to_string()
        }
    }

    fn next_action_id(&self) -> MessageId {
        if !self.state.update_ready(CONSTITUTION_CHECKPOINT_VERSION) {
            return MessageId::SetupReportNextActionConstitution;
        }
        if !matches!(
            self.state.status(SetupStep::ProviderModel),
            StepStatus::Verified | StepStatus::NeedsAction
        ) {
            return MessageId::SetupReportNextActionProvider;
        }
        if !self.state.runtime_posture_source.is_reviewed() {
            return MessageId::SetupReportNextActionRuntime;
        }
        if !self.state.first_run_ready() {
            return MessageId::SetupReportNextActionRequired;
        }
        if !self.state.operate_ready() {
            return MessageId::SetupReportNextActionOperate;
        }
        MessageId::SetupReportNextActionNone
    }

    fn detail_row(&self, label: MessageId, value: &str) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{} ", tr(label)),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(value.to_string()),
        ])
    }

    fn guided_answer_pair(
        &self,
        left: (&str, MessageId, &str),
        right: (&str, MessageId, &str),
    ) -> Line<'static> {
        let label_style = Style::default()
            .fg(palette::TEXT_MUTED)
            .add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled(format!("{} {} ", left.0, tr(left.1)), label_style),
            Span::raw(left.2.to_string()),
            Span::styled("  ·  ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(format!("{} {} ", right.0, tr(right.1)), label_style),
            Span::raw(right.2.to_string()),
        ])
    }

    fn guided_answer_single(&self, key: &str, label: MessageId, value: &str) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{key} {} ", tr(label)),
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(value.to_string()),
        ])
    }
}

fn setup_report_ready(state: &SetupState) -> bool {
    state.first_run_ready() || state.update_ready(CONSTITUTION_CHECKPOINT_VERSION)
}

fn runtime_preset_summary(preset: SetupRuntimePreset) -> String {
    format!(
        "{} - {}",
        tr(preset.title_id()),
        tr(preset.description_id())
    )
}

fn runtime_preset_inline_diff(preset: SetupRuntimePreset, facts: &SetupRuntimeFacts) -> String {
    runtime_preset_diff_rows(preset, facts).join("; ")
}

fn runtime_preset_preview_text(preset: SetupRuntimePreset, facts: &SetupRuntimeFacts) -> String {
    let mut lines = vec![
        tr(MessageId::SetupRuntimePresetPreviewTitle).to_string(),
        runtime_preset_summary(preset),
        String::new(),
        tr(MessageId::SetupRuntimePresetDiffLabel).to_string(),
    ];
    lines.extend(
        runtime_preset_diff_rows(preset, facts)
            .into_iter()
            .map(|row| format!("- {row}")),
    );
    lines.extend([
        String::new(),
        tr(MessageId::SetupRuntimePostureBoundary).to_string(),
        tr(MessageId::SetupRuntimePresetSafetyFloor).to_string(),
        tr(MessageId::SetupRuntimePresetApplyHint).to_string(),
    ]);
    lines.join("\n")
}

fn runtime_preset_diff_rows(preset: SetupRuntimePreset, facts: &SetupRuntimeFacts) -> Vec<String> {
    let approval_target = preset.approval_policy().map_or_else(
        || "removed; Full Access comes from settings.permission_posture".to_string(),
        ToString::to_string,
    );
    let mut rows = vec![
        format!(
            "settings.default_mode: {} -> {}",
            facts.default_mode,
            preset.display_mode()
        ),
        format!(
            "settings.permission_posture: -> {}",
            preset.permission_posture()
        ),
        format!(
            "config.approval_policy: {} -> {}",
            facts.approval_policy_value, approval_target
        ),
        format!(
            "config.allow_shell: {} -> {}",
            facts.allow_shell_enabled,
            preset.allow_shell()
        ),
        format!(
            "config.sandbox_mode: {} -> {}",
            facts.sandbox_mode_value,
            preset.sandbox_mode()
        ),
        format!(
            "config.network.default: {} -> unchanged",
            facts.network_default_value
        ),
        format!("workspace trust: {} -> unchanged", facts.trust),
    ];
    if let Some(warning) = facts.project_override_warning.as_deref() {
        rows.push(format!("project override warning: {warning}"));
    }
    rows
}

fn project_runtime_override_warning(workspace: &Path) -> Option<String> {
    let project = codewhale_config::load_project_config(workspace)?;
    let mut fields = Vec::new();
    if let Some(policy) = project.approval_policy.as_deref() {
        fields.push(format!("approval_policy={policy}"));
    }
    if let Some(mode) = project.sandbox_mode.as_deref() {
        fields.push(format!("sandbox_mode={mode}"));
    }
    if fields.is_empty() {
        return None;
    }
    Some(format!(
        "此工作区的项目配置包含 {}。预设会保存用户默认值；项目配置仍可在此工作区收紧运行姿态。",
        fields.join(", ")
    ))
}

fn setup_report_result(state: &SetupState, facts: &SetupRuntimeFacts) -> String {
    format!(
        "first_run={}, update={}, operate={}, constitution={:?}, autonomy={}, posture={:?}, runtime={}, operate_fleet={}",
        if state.first_run_ready() {
            "ready"
        } else {
            "needs_action"
        },
        if state.update_ready(CONSTITUTION_CHECKPOINT_VERSION) {
            "ready"
        } else {
            "needs_action"
        },
        if state.operate_ready() {
            "ready"
        } else {
            "needs_action"
        },
        state.constitution_choice,
        facts.constitution_autonomy,
        state.runtime_posture_source,
        facts.runtime_result,
        facts.operate_result
    )
}

fn tools_mcp_on_ramp_text(facts: &SetupRuntimeFacts) -> String {
    let tools_facts = tools_mcp::SetupToolsMcpFacts {
        servers_result: facts.tools_mcp_servers_result.clone(),
        skills_result: facts.tools_mcp_skills_result.clone(),
        tools_result: facts.tools_mcp_tools_result.clone(),
        plugins_result: facts.tools_mcp_plugins_result.clone(),
        result: facts.tools_mcp_result.clone(),
        overall_status: if facts.tools_mcp_needs_action {
            tools_mcp::InventoryStatus::NeedsConfig
        } else if facts.tools_mcp_result.contains("overall=off") {
            tools_mcp::InventoryStatus::Off
        } else {
            tools_mcp::InventoryStatus::Healthy
        },
        needs_action: facts.tools_mcp_needs_action,
        mcp_path_display: facts.tools_mcp_path_display.clone(),
        skills_path_display: facts.tools_mcp_skills_path_display.clone(),
        plugins_path_display: facts.tools_mcp_plugins_path_display.clone(),
    };
    tools_mcp::on_ramp_text(&tools_facts)
}

#[cfg(test)]
#[must_use]
fn guided_constitution_template() -> UserConstitution {
    GuidedConstitutionDraft::default().to_constitution()
}

/// Who authored the draft being previewed for ratification.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DraftProvenance {
    /// Rendered deterministically from the guided answers.
    Guided,
    /// Drafted by the named model, then sanitized and bounded by CodeWhale.
    Model(String),
    /// The user's existing `constitution.json`, shown unchanged for the
    /// keep-existing checkpoint completion (#3794).
    Existing,
}

fn ratification_preview_title() -> &'static str {
    "用户宪法 — 批准前草案"
}

/// The ratification artifact shown in the pager: provenance, what a
/// constitution is, the exact block that will be injected (byte-identical to
/// prompt assembly's rendering), its authority boundaries, and how to ratify
/// or amend. Only the scaffold differs between guided and model drafts — the
/// law itself always comes from the same renderer.
fn constitution_ratification_text(
    constitution: &UserConstitution,
    provenance: &DraftProvenance,
) -> String {
    const RULE: &str = "──────────────────────────────────────────────────────";
    let rendered = constitution
        .render_block(None)
        .unwrap_or_else(|| "结构化宪法为空。".to_string());
    let layer_order = tr(MessageId::SetupCheckpointLayerOrder);
    let drafted_by = match provenance {
        DraftProvenance::Model(label) => {
            format!("由 {label} 根据你的引导式答案起草，并已由 CodeWhale 完成结构校验与边界限制。")
        }
        DraftProvenance::Guided => "由你的引导式答案确定性生成。".to_string(),
        DraftProvenance::Existing => {
            "你现有的宪法，读取自 constitution.json——原样展示，未做任何修改。".to_string()
        }
    };
    let ratify_how = match provenance {
        DraftProvenance::Existing => {
            "这已是你现行的准则。关闭此预览后按 K 保留并完成检查点——文件不会被修改。\
             之后可随时用 /constitution 或 /setup 修订。"
        }
        _ => {
            "未经你确认，任何内容都不会成为准则。关闭此预览后按 G 批准并保存；\
             之后可随时用 /constitution 或 /setup 修订。"
        }
    };
    format!(
        "CODEWHALE · 用户宪法\n{RULE}\n\n{drafted_by}\n\n\
         这是 CodeWhale 与你协作的长期准则。像优秀的宪法一样：足够简短因而可用，由持久原则而非详尽规则构成，并且可以随你修订。\
         它界定权力与边界，而非裁决每个具体决定；它让协作跨会话延续——但它不是记忆，它承载的是原则，而非历史。\n\n\
         {rendered}\n\n\
         权限层级\n{layer_order}\n你的直接指令始终高于本文件。\n\n\
         它不能做什么\n\
         它只提供行为指导，不能授予或更改审批策略、沙箱、Shell、网络、信任、MCP 权限、默认模式、发布或支出权限——这些始终由你在运行时掌控。\n\n\
         精简核心与可选模块\n\
         内置核心始终生效。本草案只保存你的用户全局长期偏好。执行/编排等重型教义位于模式提示词或未来的可选模块中；此预览不会启用模块或更改其配置。\n\n\
         批准\n{ratify_how}"
    )
}

/// Card line inviting the user to let their configured model draft the law.
fn model_draft_invitation_line(model_label: &str) -> String {
    format!("A {model_label} 起草，你批准。未经确认不会保存。")
}

/// Card line offering to keep an existing valid constitution unchanged.
fn keep_existing_invitation_line() -> &'static str {
    "K 保留现有宪法——先查看，再保留，文件不变。"
}

/// Card line shown while a model draft awaits ratification.
fn model_draft_ready_line(model_label: &str) -> String {
    format!("{model_label} 的草案待批准——按 G 查看并批准；按 1-6 会丢弃草案。")
}

/// Host-facing status line after a successful model draft.
pub(crate) fn model_draft_ready_message(model_label: &str) -> String {
    format!("{model_label} 已起草你的宪法。请查看预览，然后按 G 批准。")
}

/// Host-facing status line when model drafting fails or is unavailable. The
/// guided deterministic draft always remains the standing fallback.
pub(crate) fn model_draft_failed_message(model_label: &str, reason: &str) -> String {
    format!("{model_label} 未能完成起草（{reason}）。引导式草案仍然有效——按 G 预览并批准。")
}

fn constitution_choice_label(choice: ConstitutionChoice) -> &'static str {
    match choice {
        ConstitutionChoice::Unset => "unset",
        ConstitutionChoice::Bundled => "bundled/default",
        ConstitutionChoice::GuidedCustom => "guided custom",
        ConstitutionChoice::ExpertOverride => "expert override",
        ConstitutionChoice::Deferred => "deferred",
    }
}

fn constitution_source_label(source: ConstitutionSource) -> &'static str {
    match source {
        ConstitutionSource::Bundled => "bundled",
        ConstitutionSource::UserGlobal => "user-global constitution.json",
        ConstitutionSource::ExpertOverride => "expert full Markdown override",
    }
}

fn constitution_validity_label(validity: ConstitutionValidity) -> &'static str {
    match validity {
        ConstitutionValidity::Unknown => "unknown",
        ConstitutionValidity::Valid => "valid",
        ConstitutionValidity::Invalid => "invalid",
        ConstitutionValidity::Empty => "empty",
        ConstitutionValidity::Unreadable => "unreadable",
    }
}

pub fn persist_user_constitution_choice(
    constitution: &UserConstitution,
    state: &SetupState,
) -> anyhow::Result<()> {
    let constitution_path = UserConstitution::path()?;
    let setup_state_path = SetupState::path()?;
    let mut transaction = codewhale_config::persistence::SetupTransaction::new();
    transaction.stage_json(constitution_path, &constitution.bounded())?;
    transaction.stage_json(setup_state_path, state)?;
    transaction.commit()
}

#[must_use]
pub fn should_open_update_checkpoint(app: &App, config: &Config) -> bool {
    let state = load_setup_state_for_app(app, config);
    state.needs_constitution_checkpoint(CONSTITUTION_CHECKPOINT_VERSION)
}

pub fn defer_update_checkpoint_for_app(app: &App, config: &Config) -> anyhow::Result<SetupState> {
    let mut state = load_setup_state_for_app(app, config);
    if !state.needs_constitution_checkpoint(CONSTITUTION_CHECKPOINT_VERSION) {
        return Ok(state);
    }
    state.complete_constitution_checkpoint(
        CONSTITUTION_CHECKPOINT_VERSION,
        ConstitutionChoice::Deferred,
    );
    state.constitution_source = ConstitutionSource::Bundled;
    state.constitution_validity = ConstitutionValidity::Unknown;
    state.constitution_authoring = None;
    state.constitution_preview_hash = None;
    state.set_step(
        SetupStep::Constitution,
        StepEntry::new(StepStatus::Deferred, true, CONSTITUTION_CHECKPOINT_VERSION)
            .with_result("checkpoint deferred; bundled applies"),
    );
    state.save()?;
    Ok(state)
}

#[must_use]
pub fn load_setup_state_for_app(app: &App, config: &Config) -> SetupState {
    if let Ok(Some(state)) = SetupState::load() {
        return state;
    }
    SetupState::derive_inherited(&inherited_facts_for_app(app, config))
}

pub(crate) fn record_provider_model_setup_state_for_app(
    app: &App,
    config: &Config,
) -> anyhow::Result<SetupState> {
    let facts = SetupRuntimeFacts::from_app_config(app, config);
    let mut state = load_setup_state_for_app(app, config);
    state.set_step(
        SetupStep::ProviderModel,
        provider::step_entry(
            facts.provider_ready,
            CONSTITUTION_CHECKPOINT_VERSION,
            facts.provider_result,
        ),
    );
    state.save()?;
    Ok(state)
}

#[must_use]
fn inherited_facts_for_app(app: &App, config: &Config) -> InheritedConfigFacts {
    let user_constitution = UserConstitution::load().ok();
    let user_constitution_validity = user_constitution.as_ref().map_or(
        ConstitutionValidity::Unknown,
        UserConstitutionLoad::validity,
    );
    let has_user_constitution = user_constitution
        .as_ref()
        .is_some_and(|loaded| !matches!(loaded, UserConstitutionLoad::Missing));
    let expert_override = SetupExpertOverrideState::load();
    InheritedConfigFacts {
        has_provider_route: !config.default_model().trim().is_empty(),
        has_credentials_or_local_runtime: has_api_key(config),
        trust_chosen: app.trust_mode || !onboarding::needs_trust(&app.workspace),
        has_expert_override: expert_override.is_active(),
        has_user_constitution,
        user_constitution_validity,
    }
}

fn expert_override_path() -> Option<std::path::PathBuf> {
    codewhale_config::codewhale_home()
        .ok()
        .map(|home| home.join(Path::new(CONSTITUTION_OVERRIDE_FILE)))
}

#[must_use]
fn initial_step_index(state: &SetupState) -> usize {
    if state.needs_constitution_checkpoint(CONSTITUTION_CHECKPOINT_VERSION) {
        return step_index(SetupStep::Constitution);
    }
    STEP_SPECS
        .iter()
        .position(|step| {
            step.required()
                && !matches!(
                    state.status(step.id()),
                    StepStatus::Verified
                        | StepStatus::NeedsAction
                        | StepStatus::Deferred
                        | StepStatus::Optional
                        | StepStatus::Skipped
                )
        })
        .unwrap_or_else(|| step_index(SetupStep::Verification))
}

#[must_use]
fn step_index(step: SetupStep) -> usize {
    STEP_SPECS
        .iter()
        .position(|spec| spec.id() == step)
        .expect("all setup-state steps should have wizard specs")
}

fn visible_step_index(step: SetupStep) -> usize {
    STEP_SPECS
        .iter()
        .position(|spec| spec.id() == step)
        .unwrap_or_else(|| step_index(SetupStep::Constitution))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn setup_test_options(workspace: std::path::PathBuf) -> crate::tui::app::TuiOptions {
        crate::tui::app::TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace,
            config_path: None,
            config_profile: None,
            allow_shell: true,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: std::path::PathBuf::from("."),
            memory_path: std::path::PathBuf::from("memory.md"),
            notes_path: std::path::PathBuf::from("notes.txt"),
            mcp_config_path: std::path::PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: true,
            skip_onboarding: false,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        }
    }

    #[test]
    fn visible_release_rail_includes_supported_optional_steps() {
        let steps = STEP_SPECS.iter().map(|step| step.id()).collect::<Vec<_>>();

        assert_eq!(
            steps,
            vec![
                SetupStep::ProviderModel,
                SetupStep::TrustSandbox,
                SetupStep::Constitution,
                SetupStep::OperateFleet,
                SetupStep::ToolsMcp,
                SetupStep::Persistence,
                SetupStep::Verification,
            ]
        );
        assert_eq!(
            SetupWizardView::new_at_with_facts(
                SetupState::default(),
                SetupStep::ToolsMcp,
                SetupRuntimeFacts::default(),
            )
            .selected_step(),
            SetupStep::ToolsMcp
        );
    }

    #[test]
    fn wizard_resumes_at_constitution_checkpoint_when_update_incomplete() {
        let state = SetupState::default();

        let view = SetupWizardView::new(state);

        assert_eq!(view.selected_step(), SetupStep::Constitution);
    }

    #[test]
    fn bundled_constitution_commit_marks_checkpoint_complete() {
        let mut view = SetupWizardView::new(SetupState::default());

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::EmitAndClose(ViewEvent::SetupStateCommitRequested { state, message }) =
            action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.constitution_checkpoint_completed_for.as_deref(),
            Some(CONSTITUTION_CHECKPOINT_VERSION)
        );
        assert_eq!(state.constitution_choice, ConstitutionChoice::Bundled);
        assert_eq!(state.status(SetupStep::Constitution), StepStatus::Verified);
        assert!(message.contains("宪法检查点已完成"));
    }

    #[test]
    fn back_keys_return_to_previous_step_and_clamp_at_first() {
        let mut view = SetupWizardView::new(SetupState::default());
        assert_eq!(view.selected_step(), SetupStep::Constitution);

        let action = view.handle_key(key(KeyCode::Right));
        assert!(matches!(action, ViewAction::None));
        assert_eq!(view.selected_step(), SetupStep::OperateFleet);

        let action = view.handle_key(key(KeyCode::Char('b')));
        assert!(matches!(action, ViewAction::None));
        assert_eq!(view.selected_step(), SetupStep::Constitution);

        for _ in 0..STEP_SPECS.len() {
            view.handle_key(key(KeyCode::Left));
        }
        assert_eq!(view.selected_step(), SetupStep::ProviderModel);
    }

    #[test]
    fn cancel_closes_without_commit_event() {
        let mut view = SetupWizardView::new(SetupState::default());

        let action = view.handle_key(key(KeyCode::Esc));

        assert!(matches!(action, ViewAction::Close));
    }

    #[test]
    fn skip_and_retry_emit_setup_state_commits() {
        let mut view = SetupWizardView::new(SetupState::default());

        let action = view.handle_key(key(KeyCode::Char('s')));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected skipped setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::Constitution), StepStatus::Skipped);
        assert!(message.contains("已保存跳过此设置步骤"));
        assert_eq!(view.selected_step(), SetupStep::OperateFleet);

        let action = view.handle_key(key(KeyCode::Char('r')));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected retry setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::OperateFleet),
            StepStatus::NeedsAction
        );
        assert!(message.contains("已标记此设置步骤待重试"));
    }

    #[test]
    fn completed_checkpoint_resumes_to_first_required_gap() {
        let mut state = SetupState::default();
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::Bundled,
        );

        let view = SetupWizardView::new(state);

        assert_eq!(view.selected_step(), SetupStep::ProviderModel);
    }

    #[test]
    fn setup_copy_is_simplified_chinese() {
        assert!(tr(MessageId::SetupWizardTitle).contains("设置"));
        assert!(tr(MessageId::SetupCheckpointDoneBundled).contains("宪法"));
    }

    #[test]
    fn guided_constitution_requires_preview_before_save() {
        let mut view = SetupWizardView::new(SetupState::default());

        let action = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::Emit(ViewEvent::OpenTextPager { title, content }) = action else {
            panic!("expected guided constitution preview event");
        };
        assert!(title.contains("批准前草案"));
        assert!(content.contains("<codewhale_user_constitution"));
        assert!(content.contains("按 G 批准并保存"));
        assert!(content.contains("精简核心与可选模块"));
        assert!(content.contains("内置核心始终生效"));
        assert!(content.contains("不会启用模块"));
        assert_eq!(view.state().constitution_choice, ConstitutionChoice::Unset);

        let action = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            message,
        }) = action
        else {
            panic!("expected guided constitution commit event");
        };
        assert_eq!(
            constitution.autonomy_preference,
            AutonomyPreference::Balanced
        );
        assert_eq!(state.constitution_choice, ConstitutionChoice::GuidedCustom);
        assert_eq!(state.constitution_source, ConstitutionSource::UserGlobal);
        assert_eq!(state.constitution_validity, ConstitutionValidity::Valid);
        assert_eq!(
            state.constitution_preview_hash.as_deref(),
            Some(constitution.preview_hash().as_str())
        );
        assert_eq!(state.status(SetupStep::Constitution), StepStatus::Verified);
        assert_eq!(state.runtime_posture_source, RuntimePostureSource::Unset);
        assert!(message.contains("宪法"));
    }

    #[test]
    fn ratification_preview_explains_reduced_core_modules() {
        let constitution = GuidedConstitutionDraft::default().to_constitution();
        let content = constitution_ratification_text(&constitution, &DraftProvenance::Guided);

        assert!(content.contains("精简核心"));
        assert!(content.contains("模块"));
        assert!(content.contains("不会启用"));
        assert!(content.contains("不能授予或更改审批策略、沙箱、Shell、网络、信任、MCP 权限"));
        assert!(content.contains("发布或支出权限"));
    }

    #[test]
    fn guided_constitution_key_is_contextual_to_constitution_step() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            SetupRuntimeFacts::default(),
        );

        let action = view.handle_key(key(KeyCode::Char('g')));

        assert!(matches!(action, ViewAction::None));
        assert_eq!(view.selected_step(), SetupStep::ProviderModel);
        assert_eq!(view.state().constitution_choice, ConstitutionChoice::Unset);
    }

    #[test]
    fn provider_model_step_hands_off_to_existing_route_surfaces() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            SetupRuntimeFacts::default(),
        );

        let provider_action = view.handle_key(key(KeyCode::Char('p')));
        assert!(matches!(
            provider_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenProviderRequested)
        ));

        let model_action = view.handle_key(key(KeyCode::Char('m')));
        assert!(matches!(
            model_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenModelRequested)
        ));
    }

    #[test]
    fn provider_model_detail_lines_show_credential_url_for_missing_hosted_provider() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        let codewhale_home = tmp.path().join(".codewhale");
        let _home = crate::test_support::EnvVarGuard::set("HOME", tmp.path());
        let _userprofile = crate::test_support::EnvVarGuard::set("USERPROFILE", tmp.path());
        let _codewhale_home =
            crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", &codewhale_home);
        let _deepseek_key = crate::test_support::EnvVarGuard::remove("DEEPSEEK_API_KEY");
        let _nim_key = crate::test_support::EnvVarGuard::remove("NVIDIA_API_KEY");
        let _nim_alt_key = crate::test_support::EnvVarGuard::remove("NVIDIA_NIM_API_KEY");
        let config = Config {
            provider: Some("nvidia-nim".to_string()),
            ..Config::default()
        };
        let app = App::new(setup_test_options(workspace), &config);
        let facts = SetupRuntimeFacts::from_app_config(&app, &config);
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            facts,
        );

        let text = lines_to_text(view.provider_model_detail_lines());

        assert!(text.contains("NVIDIA NIM"), "{text}");
        assert!(text.contains("credentials: https://build.nvidia.com/settings/api-keys"));
    }

    #[test]
    fn provider_model_detail_lines_keep_codex_oauth_url_free() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        let codewhale_home = tmp.path().join(".codewhale");
        let _home = crate::test_support::EnvVarGuard::set("HOME", tmp.path());
        let _userprofile = crate::test_support::EnvVarGuard::set("USERPROFILE", tmp.path());
        let _codewhale_home =
            crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", &codewhale_home);
        let _openai_codex_key =
            crate::test_support::EnvVarGuard::remove("OPENAI_CODEX_ACCESS_TOKEN");
        let _codex_key = crate::test_support::EnvVarGuard::remove("CODEX_ACCESS_TOKEN");
        let config = Config {
            provider: Some("openai-codex".to_string()),
            ..Config::default()
        };
        let app = App::new(setup_test_options(workspace), &config);
        let facts = SetupRuntimeFacts::from_app_config(&app, &config);
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            facts,
        );

        let text = lines_to_text(view.provider_model_detail_lines());

        assert!(text.contains("codex login"), "{text}");
        assert!(!text.contains("credentials:"), "{text}");
    }

    #[test]
    fn provider_model_detail_lines_cover_deepseek_cn_and_local_boundaries() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        let codewhale_home = tmp.path().join(".codewhale");
        let _home = crate::test_support::EnvVarGuard::set("HOME", tmp.path());
        let _userprofile = crate::test_support::EnvVarGuard::set("USERPROFILE", tmp.path());
        let _codewhale_home =
            crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", &codewhale_home);
        let _deepseek_key = crate::test_support::EnvVarGuard::remove("DEEPSEEK_API_KEY");
        let _deepseek_source = crate::test_support::EnvVarGuard::remove("DEEPSEEK_API_KEY_SOURCE");

        let cn_config = Config {
            provider: Some("deepseek-cn".to_string()),
            ..Config::default()
        };
        let cn_app = App::new(setup_test_options(workspace.clone()), &cn_config);
        let cn_view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            SetupRuntimeFacts::from_app_config(&cn_app, &cn_config),
        );
        let cn_text = lines_to_text(cn_view.provider_model_detail_lines());
        assert!(cn_text.contains("DeepSeek (legacy alias)"), "{cn_text}");
        assert!(
            cn_text.contains("credentials: https://platform.deepseek.com/api_keys"),
            "{cn_text}"
        );
        assert!(cn_text.contains("missing key"), "{cn_text}");

        let local_config = Config {
            provider: Some("ollama".to_string()),
            ..Config::default()
        };
        let local_app = App::new(setup_test_options(workspace), &local_config);
        let local_view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            SetupRuntimeFacts::from_app_config(&local_app, &local_config),
        );
        let local_text = lines_to_text(local_view.provider_model_detail_lines());
        assert!(local_text.contains("Ollama"), "{local_text}");
        assert!(local_text.contains("local · not checked"), "{local_text}");
        assert!(!local_text.contains("credentials:"), "{local_text}");
    }

    #[test]
    fn runtime_posture_step_hands_off_to_mode_and_config_surfaces() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            SetupRuntimeFacts::default(),
        );

        let mode_action = view.handle_key(key(KeyCode::Char('m')));
        assert!(matches!(
            mode_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenModeRequested)
        ));

        let config_action = view.handle_key(key(KeyCode::Char('c')));
        assert!(matches!(
            config_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenConfigRequested)
        ));
    }

    #[test]
    fn operate_fleet_step_hands_off_to_provider_and_fleet_surfaces() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::OperateFleet,
            SetupRuntimeFacts::default(),
        );

        let provider_action = view.handle_key(key(KeyCode::Char('p')));
        assert!(matches!(
            provider_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenProviderRequested)
        ));

        let fleet_action = view.handle_key(key(KeyCode::Char('f')));
        assert!(matches!(
            fleet_action,
            ViewAction::EmitAndClose(ViewEvent::SetupOpenFleetRequested)
        ));
    }

    #[test]
    fn guided_constitution_answers_shape_preview_and_saved_payload() {
        let mut view = SetupWizardView::new(SetupState::default());
        for key_char in ['1', '2', '3', '4', '5', '6'] {
            assert!(matches!(
                view.handle_key(key(KeyCode::Char(key_char))),
                ViewAction::None
            ));
        }

        let action = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = action else {
            panic!("expected tuned guided constitution preview event");
        };
        assert!(content.contains("实时资料、引用证据"));
        assert!(content.contains("积极主动"));
        assert!(content.contains("发布证据"));
        assert!(content.contains("解释关键推理和取舍"));
        assert!(content.contains("敏感信息"));
        assert!(content.contains("保留用户语气"));
        assert!(content.contains("保留用户的语气"));

        let action = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            ..
        }) = action
        else {
            panic!("expected tuned guided constitution commit event");
        };
        assert_eq!(
            constitution.autonomy_preference,
            AutonomyPreference::Autonomous
        );
        let body = constitution.render_body();
        assert!(body.contains("实时资料、引用证据"));
        assert!(body.contains("发布证据"));
        assert!(body.contains("解释关键推理和取舍"));
        assert!(body.contains("敏感信息"));
        assert!(body.contains("保留用户的语气"));
        assert_eq!(
            state.constitution_preview_hash.as_deref(),
            Some(constitution.preview_hash().as_str())
        );
    }

    #[test]
    fn constitution_detail_lines_explain_reduced_core_and_modules_boundary() {
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            SetupRuntimeFacts::default(),
        );

        let text = lines_to_text(view.constitution_detail_lines());

        assert!(text.contains("只保存用户全局偏好"));
        assert!(text.contains("内置核心始终生效"));
        assert!(text.contains("模式提示词"));
        assert!(text.contains("未来可选模块"));
    }

    #[test]
    fn freeform_note_previews_saves_and_stays_advisory() {
        let mut view = SetupWizardView::new(SetupState::default());

        let first_preview = view.handle_key(key(KeyCode::Char('g')));
        assert!(matches!(
            first_preview,
            ViewAction::Emit(ViewEvent::OpenTextPager { .. })
        ));
        assert!(view.handle_paste(
            "Prefer reversible demos; do not treat shell unrestricted as permission."
        ));

        let second_preview = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = second_preview else {
            panic!("freeform note should force a fresh preview");
        };
        assert!(content.contains("用户自由原则"));
        assert!(content.contains("Prefer reversible demos"));
        assert!(content.contains("不会改变审批、沙箱、Shell"));

        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            ..
        }) = action
        else {
            panic!("expected guided constitution commit event");
        };
        let body = constitution.render_body();
        assert!(body.contains("用户自由原则"));
        assert!(body.contains("Prefer reversible demos"));
        assert_eq!(
            state.constitution_authoring,
            Some(ConstitutionAuthoring::Guided)
        );
        assert_eq!(state.runtime_posture_source, RuntimePostureSource::Unset);
    }

    #[test]
    fn changing_guided_answer_requires_fresh_preview() {
        let mut view = SetupWizardView::new(SetupState::default());

        let first_preview = view.handle_key(key(KeyCode::Char('g')));
        assert!(matches!(
            first_preview,
            ViewAction::Emit(ViewEvent::OpenTextPager { .. })
        ));

        assert!(matches!(
            view.handle_key(key(KeyCode::Char('6'))),
            ViewAction::None
        ));
        let second_preview = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = second_preview else {
            panic!("changed guided answer should preview again before saving");
        };
        assert!(content.contains("保留用户的语气"));

        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            ..
        }) = action
        else {
            panic!("expected save after fresh preview");
        };
        assert_eq!(
            constitution.autonomy_preference,
            AutonomyPreference::Balanced
        );
        assert!(constitution.render_body().contains("保留用户的语气"));
    }

    fn ready_facts(model: &str) -> SetupRuntimeFacts {
        SetupRuntimeFacts {
            provider_ready: true,
            model: model.to_string(),
            ..SetupRuntimeFacts::default()
        }
    }

    fn first_run_ready_state() -> SetupState {
        let mut state = SetupState::default();
        state.set_step(
            SetupStep::ProviderModel,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION),
        );
        state.runtime_posture_source = RuntimePostureSource::Confirmed;
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::Bundled,
        );
        state.set_step(
            SetupStep::Constitution,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION),
        );
        state
    }

    fn sample_model_draft() -> Box<UserConstitution> {
        Box::new(UserConstitution {
            about: Some("A GLM-5.2 user shipping Rust.".to_string()),
            working_style: vec!["Keep diffs scoped.".to_string()],
            priorities: vec!["Evidence over vibes.".to_string()],
            autonomy_preference: AutonomyPreference::Balanced,
            notes: Some("Advisory only.".to_string()),
            ..UserConstitution::default()
        })
    }

    #[test]
    fn model_draft_key_is_inert_without_a_ready_provider() {
        // Fallback contract: no route, no drafting offer — the deterministic
        // guided flow stands untouched.
        let mut view = SetupWizardView::new(SetupState::default());
        assert_eq!(view.selected_step(), SetupStep::Constitution);

        let action = view.handle_key(key(KeyCode::Char('a')));

        assert!(matches!(action, ViewAction::None));
        assert_eq!(view.state().constitution_choice, ConstitutionChoice::Unset);
    }

    #[test]
    fn model_draft_key_requests_drafting_with_current_answers() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );
        // Tune one answer first: the request must carry the tuned draft.
        assert!(matches!(
            view.handle_key(key(KeyCode::Char('2'))),
            ViewAction::None
        ));
        assert!(view.handle_paste("Prefer demos before durable rewrites."));

        let action = view.handle_key(key(KeyCode::Char('a')));

        let ViewAction::Emit(ViewEvent::SetupConstitutionModelDraftRequested {
            draft,
            freeform_note,
        }) = action
        else {
            panic!("expected model draft request event");
        };
        assert_eq!(draft.autonomy, AutonomyPreference::Autonomous);
        assert_eq!(
            freeform_note.as_deref(),
            Some("Prefer demos before durable rewrites.")
        );
        // The wizard stays open (Emit, not EmitAndClose) and nothing commits.
        assert_eq!(view.state().constitution_choice, ConstitutionChoice::Unset);
    }

    #[test]
    fn installed_model_draft_previews_then_ratifies_with_provenance() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );

        let (title, content) =
            view.install_model_draft(sample_model_draft(), "GLM-5.2".to_string());
        assert!(title.contains("批准前草案"));
        assert!(content.contains("由 GLM-5.2 根据你的引导式答案起草"));
        assert!(content.contains("A GLM-5.2 user shipping Rust."));
        assert!(content.contains("<codewhale_user_constitution"));

        // The install satisfied the preview gate; G ratifies the model draft.
        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            message,
        }) = action
        else {
            panic!("expected ratification commit event");
        };
        assert_eq!(constitution, *sample_model_draft());
        assert_eq!(state.constitution_choice, ConstitutionChoice::GuidedCustom);
        assert_eq!(
            state.constitution_authoring,
            Some(ConstitutionAuthoring::ModelDrafted)
        );
        assert_eq!(
            state.constitution_preview_hash.as_deref(),
            Some(constitution.preview_hash().as_str())
        );
        let step = state.steps.get(&SetupStep::Constitution).expect("step");
        let result = step.result.as_deref().expect("result");
        assert!(result.contains("model-drafted constitution ratified (GLM-5.2)"));
        assert!(message.contains("宪法已批准"));
    }

    #[test]
    fn deterministic_ratification_records_guided_authoring() {
        let mut view = SetupWizardView::new(SetupState::default());

        view.handle_key(key(KeyCode::Char('g')));
        let action = view.handle_key(key(KeyCode::Char('g')));

        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested { state, .. }) =
            action
        else {
            panic!("expected guided commit event");
        };
        assert_eq!(
            state.constitution_authoring,
            Some(ConstitutionAuthoring::Guided)
        );
    }

    #[test]
    fn cycling_answers_discards_the_model_draft() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );
        let _ = view.install_model_draft(sample_model_draft(), "GLM-5.2".to_string());

        // Changing any answer makes the model draft stale law.
        assert!(matches!(
            view.handle_key(key(KeyCode::Char('1'))),
            ViewAction::None
        ));

        // The next G must preview afresh — and preview the guided rendering,
        // not the discarded model draft.
        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = action else {
            panic!("stale draft should force a fresh preview");
        };
        assert!(content.contains("由你的引导式答案确定性生成"));
        assert!(!content.contains("由 GLM-5.2"));

        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested { state, .. }) =
            action
        else {
            panic!("expected guided commit after discard");
        };
        assert_eq!(
            state.constitution_authoring,
            Some(ConstitutionAuthoring::Guided)
        );
    }

    #[test]
    fn freeform_note_discards_the_model_draft() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );
        let _ = view.install_model_draft(sample_model_draft(), "GLM-5.2".to_string());

        assert!(view.handle_paste("Prefer local examples before broad rewrites."));

        let action = view.handle_key(key(KeyCode::Char('g')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = action else {
            panic!("changed freeform note should force a fresh guided preview");
        };
        assert!(content.contains("由你的引导式答案确定性生成"));
        assert!(content.contains("Prefer local examples"));
        assert!(!content.contains("由 GLM-5.2"));
    }

    #[test]
    fn constitution_card_gates_the_model_draft_invitation() {
        // No ready provider: no invitation (and the blocker-size layout holds).
        let not_ready = SetupWizardView::new(SetupState::default());
        let text = lines_to_text(not_ready.constitution_detail_lines());
        assert!(!text.contains("起草，你批准"));
        assert!(!text.contains("草案待批准"));

        // Ready provider: the invitation names the first configured model.
        let ready = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );
        let text = lines_to_text(ready.constitution_detail_lines());
        assert!(text.contains("GLM-5.2 起草，你批准"));

        // Installed draft: the card flips to the awaiting-ratification line.
        let mut with_draft = ready.clone();
        let _ = with_draft.install_model_draft(sample_model_draft(), "GLM-5.2".to_string());
        let text = lines_to_text(with_draft.constitution_detail_lines());
        assert!(text.contains("GLM-5.2 的草案待批准"));
        assert!(!text.contains("GLM-5.2 起草，你批准"));
    }

    #[test]
    fn model_drafted_commit_round_trips_through_the_setup_transaction() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let _home = crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", tmp.path());

        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            ready_facts("GLM-5.2"),
        );
        let _ = view.install_model_draft(sample_model_draft(), "GLM-5.2".to_string());
        let ViewAction::EmitAndClose(ViewEvent::SetupConstitutionCommitRequested {
            constitution,
            state,
            ..
        }) = view.handle_key(key(KeyCode::Char('g')))
        else {
            panic!("expected ratification commit event");
        };

        persist_user_constitution_choice(&constitution, &state).expect("persist");

        let loaded = UserConstitution::load().expect("load constitution");
        let loaded = loaded.constitution().expect("valid constitution");
        assert_eq!(loaded.render_body(), constitution.render_body());
        let loaded_state = SetupState::load().expect("load state").expect("state");
        assert_eq!(
            loaded_state.constitution_authoring,
            Some(ConstitutionAuthoring::ModelDrafted)
        );
        assert_eq!(
            loaded_state.constitution_preview_hash.as_deref(),
            Some(constitution.preview_hash().as_str())
        );
    }

    #[test]
    fn guided_constitution_template_uses_simplified_chinese() {
        let body = guided_constitution_template().render_body();

        assert!(body.contains("重证据"));
        assert!(body.contains("当前用户请求和实时工具证据"));
        assert!(!body.contains("A CodeWhale user who wants"));
        assert!(!body.contains("Guided answers:"));
    }

    #[test]
    fn ratification_preview_uses_rendered_block_and_layer_order() {
        let draft = GuidedConstitutionDraft::default();
        let content =
            constitution_ratification_text(&draft.to_constitution(), &DraftProvenance::Guided);
        assert!(content.contains("<codewhale_user_constitution"));
        assert!(content.contains("权限层级"));
        assert!(content.contains("按 G 批准并保存"));
        // Framing: powers and limits, not case-by-case; continuity, not memory.
        assert!(content.contains("它界定权力与边界"));
        assert!(content.contains("但它不是记忆"));
        assert!(content.contains("精简核心与可选模块"));
    }

    #[test]
    fn ratification_preview_states_authority_boundaries_and_provenance() {
        let draft = GuidedConstitutionDraft::default();
        let constitution = draft.to_constitution();

        let guided = constitution_ratification_text(&constitution, &DraftProvenance::Guided);
        assert!(guided.contains("权限层级"));
        assert!(guided.contains("它不能做什么"));
        assert!(guided.contains("不能授予或更改审批策略"));
        assert!(guided.contains("未经你确认"));
        assert!(guided.contains("确定性生成"));

        let drafted = constitution_ratification_text(
            &constitution,
            &DraftProvenance::Model("GLM-5.2".to_string()),
        );
        assert!(drafted.contains("由 GLM-5.2 根据你的引导式答案起草"));
        assert!(drafted.contains("结构校验与边界限制"));
    }

    #[test]
    fn guided_constitution_detail_lines_show_simplified_chinese_answers() {
        let view = SetupWizardView::new(SetupState::default());
        let text = lines_to_text(view.constitution_detail_lines());
        assert!(text.contains("用途："));
        assert!(text.contains("编码工作台"));
        assert!(text.contains("主动性："));
        assert!(text.contains("平衡"));
        assert!(text.contains("原则："));
        assert!(text.contains("小范围改动"));
        assert!(!text.contains("Purpose:"));
    }

    #[test]
    fn constitution_file_state_labels_existing_override_states() {
        assert!(
            SetupConstitutionFileState::Missing
                .label(ConstitutionChoice::Bundled)
                .contains("未找到 constitution.json")
        );
        assert!(
            SetupConstitutionFileState::Loaded
                .label(ConstitutionChoice::GuidedCustom)
                .contains("已存在并已选择")
        );
        assert!(
            SetupConstitutionFileState::Loaded
                .label(ConstitutionChoice::Bundled)
                .contains("不生效")
        );
        assert!(
            SetupConstitutionFileState::Invalid
                .label(ConstitutionChoice::Unset)
                .contains("无效")
        );
        assert!(
            SetupConstitutionFileState::Unreadable
                .label(ConstitutionChoice::Unset)
                .contains("无法读取")
        );
        assert!(
            SetupConstitutionFileState::PathError
                .label(ConstitutionChoice::Unset)
                .contains("CODEWHALE_HOME")
        );
    }

    #[test]
    fn expert_override_state_requires_content_and_opt_in() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let _home = crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", tmp.path());
        let _opt_in = crate::test_support::EnvVarGuard::remove(BASE_PROMPT_OVERRIDE_OPT_IN_ENV);

        assert_eq!(
            SetupExpertOverrideState::load(),
            SetupExpertOverrideState::Missing
        );

        let path = tmp.path().join(CONSTITUTION_OVERRIDE_FILE);
        std::fs::create_dir_all(path.parent().expect("override parent")).expect("override parent");
        std::fs::write(&path, "\n  \n").expect("write empty override");
        assert_eq!(
            SetupExpertOverrideState::load(),
            SetupExpertOverrideState::Empty
        );

        std::fs::write(&path, "# Expert override\n").expect("write override");
        assert_eq!(
            SetupExpertOverrideState::load(),
            SetupExpertOverrideState::Disabled
        );
        assert!(!SetupExpertOverrideState::Disabled.is_active());
        assert!(
            SetupExpertOverrideState::Disabled
                .label()
                .contains(BASE_PROMPT_OVERRIDE_OPT_IN_ENV)
        );

        // SAFETY: the process-wide test env mutex is held by `_guard`.
        unsafe { std::env::set_var(BASE_PROMPT_OVERRIDE_OPT_IN_ENV, "1") };
        assert_eq!(
            SetupExpertOverrideState::load(),
            SetupExpertOverrideState::Active
        );
        assert!(SetupExpertOverrideState::Active.is_active());
    }

    #[test]
    fn constitution_detail_lines_show_existing_file_state() {
        let mut state = SetupState {
            constitution_choice: ConstitutionChoice::Bundled,
            constitution_source: ConstitutionSource::Bundled,
            constitution_validity: ConstitutionValidity::Valid,
            ..SetupState::default()
        };
        let facts = SetupRuntimeFacts {
            constitution_file: SetupConstitutionFileState::Loaded,
            ..SetupRuntimeFacts::default()
        };
        let view =
            SetupWizardView::new_at_with_facts(state.clone(), SetupStep::Constitution, facts);

        let text = lines_to_text(view.constitution_detail_lines());
        assert!(text.contains("来源： bundled; validity valid"));
        assert!(text.contains("现有文件："));
        assert!(text.contains("当前记录选择使其不生效"));
        assert!(text.contains("专家覆盖："));
        assert!(text.contains("尚未检查"));

        state.constitution_choice = ConstitutionChoice::GuidedCustom;
        state.constitution_source = ConstitutionSource::UserGlobal;
        let view = SetupWizardView::new_at_with_facts(
            state,
            SetupStep::Constitution,
            SetupRuntimeFacts {
                constitution_file: SetupConstitutionFileState::Loaded,
                ..SetupRuntimeFacts::default()
            },
        );
        let text = lines_to_text(view.constitution_detail_lines());
        assert!(text.contains("现有文件："));
        assert!(text.contains("已存在并已选择"));
        assert!(text.contains("专家覆盖："));
    }

    #[test]
    fn setup_wizard_is_usable_and_opaque_at_blocker_sizes() {
        use crate::tui::views::ViewStack;
        use ratatui::{buffer::Buffer, layout::Rect};
        use unicode_width::UnicodeWidthStr;

        const BLOCKER_SIZES: [(u16, u16); 4] = [(80, 24), (100, 30), (120, 32), (160, 40)];
        for (w, h) in BLOCKER_SIZES {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            for y in 0..h {
                for x in 0..w {
                    buf[(x, y)].set_symbol("X");
                }
            }
            let mut stack = ViewStack::new();
            stack.push(SetupWizardView::new_at_with_facts(
                SetupState::default(),
                SetupStep::Constitution,
                SetupRuntimeFacts {
                    constitution_file: SetupConstitutionFileState::Loaded,
                    ..SetupRuntimeFacts::default()
                },
            ));
            stack.render(area, &mut buf);

            let rows: Vec<String> = (0..h)
                .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
                .collect();
            let text = rows.join("\n");

            for label in [
                // Ratatui stores the continuation cell of each wide CJK glyph
                // as a blank symbol, so buffer text contains these spaces.
                "设 置",
                "选 择 ：",
                "现 有 文 件 ：",
                "用 途 ：",
                "预 览 /批 准",
                "使 用 内 置",
                "取 消",
            ] {
                assert!(text.contains(label), "{w}x{h}: missing '{label}'\n{text}");
            }
            assert!(
                !text.contains('X'),
                "{w}x{h}: background bleed-through into setup modal"
            );
            assert!(
                [palette::WHALE_BG, palette::WHALE_PANEL].contains(&buf[(w / 2, h / 2)].bg),
                "{w}x{h}: modal interior must be opaque"
            );
            for y in 0..h {
                let mut x = 0;
                let mut display_width = 0;
                while x < w {
                    let symbol_width = UnicodeWidthStr::width(buf[(x, y)].symbol()).max(1);
                    display_width += symbol_width;
                    x = x.saturating_add(symbol_width as u16);
                }
                assert!(
                    display_width <= usize::from(w),
                    "{w}x{h}: row {y} overflows width ({display_width})"
                );
            }
        }
    }

    #[test]
    fn persist_user_constitution_choice_writes_constitution_and_state() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let _home = crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", tmp.path());
        let constitution = guided_constitution_template();
        let mut state = SetupState::default();
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::GuidedCustom,
        );
        state.constitution_source = ConstitutionSource::UserGlobal;
        state.constitution_validity = ConstitutionValidity::Valid;
        state.constitution_preview_hash = Some(constitution.preview_hash());
        state.set_step(
            SetupStep::Constitution,
            StepEntry::new(StepStatus::Verified, true, CONSTITUTION_CHECKPOINT_VERSION),
        );

        persist_user_constitution_choice(&constitution, &state).expect("persist constitution");

        let loaded_constitution = UserConstitution::load().expect("load constitution");
        assert!(matches!(
            loaded_constitution,
            UserConstitutionLoad::Loaded(_)
        ));
        let loaded_state = SetupState::load()
            .expect("load setup state")
            .expect("setup state");
        assert_eq!(
            loaded_state.constitution_choice,
            ConstitutionChoice::GuidedCustom
        );
        assert_eq!(
            loaded_state
                .constitution_checkpoint_completed_for
                .as_deref(),
            Some(CONSTITUTION_CHECKPOINT_VERSION)
        );
    }

    #[test]
    fn keep_existing_constitution_previews_then_completes_without_rewriting() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let _home = crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", tmp.path());

        // An existing valid custom constitution from a prior version.
        let existing = guided_constitution_template();
        persist_user_constitution_choice(&existing, &SetupState::default())
            .expect("write existing constitution");
        let path = UserConstitution::path().expect("constitution path");
        let bytes_before = std::fs::read(&path).expect("existing file bytes");

        let facts = SetupRuntimeFacts {
            constitution_file: SetupConstitutionFileState::Loaded,
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            facts,
        );

        // The card offers the keep path.
        let text = lines_to_text(view.constitution_detail_lines());
        assert!(text.contains("K 保留现有宪法"), "{text}");

        // First K previews the existing law, unchanged, with keep wording.
        let action = view.handle_key(key(KeyCode::Char('k')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { title, content }) = action else {
            panic!("expected keep-existing preview event");
        };
        assert!(title.contains("批准前草案"));
        assert!(content.contains("原样展示，未做任何修改"), "{content}");
        assert!(content.contains("按 K 保留"), "{content}");
        assert!(
            content.contains("<codewhale_user_constitution"),
            "{content}"
        );

        // Second K completes the checkpoint without touching the file.
        let action = view.handle_key(key(KeyCode::Char('k')));
        let ViewAction::EmitAndClose(ViewEvent::SetupStateCommitRequested { state, message }) =
            action
        else {
            panic!("expected keep-existing commit event");
        };
        assert_eq!(state.constitution_choice, ConstitutionChoice::GuidedCustom);
        assert_eq!(state.constitution_source, ConstitutionSource::UserGlobal);
        assert_eq!(state.constitution_validity, ConstitutionValidity::Valid);
        assert_eq!(
            state.constitution_checkpoint_completed_for.as_deref(),
            Some(CONSTITUTION_CHECKPOINT_VERSION)
        );
        assert_eq!(
            state.constitution_preview_hash.as_deref(),
            Some(existing.preview_hash().as_str())
        );
        assert_eq!(state.status(SetupStep::Constitution), StepStatus::Verified);
        assert!(message.contains("已保留现有宪法"), "{message}");

        let bytes_after = std::fs::read(&path).expect("file bytes after keep");
        assert_eq!(bytes_before, bytes_after, "keep must not rewrite the file");
    }

    #[test]
    fn keep_key_is_inert_without_a_valid_existing_constitution() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let _home = crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", tmp.path());

        for file_state in [
            SetupConstitutionFileState::Missing,
            SetupConstitutionFileState::Invalid,
            SetupConstitutionFileState::Empty,
        ] {
            let facts = SetupRuntimeFacts {
                constitution_file: file_state,
                ..SetupRuntimeFacts::default()
            };
            let mut view = SetupWizardView::new_at_with_facts(
                SetupState::default(),
                SetupStep::Constitution,
                facts,
            );
            let text = lines_to_text(view.constitution_detail_lines());
            assert!(
                !text.contains("K 保留现有宪法"),
                "{file_state:?} must not offer keep: {text}"
            );
            assert!(
                matches!(view.handle_key(key(KeyCode::Char('k'))), ViewAction::None),
                "{file_state:?} must leave K inert"
            );
        }
    }

    #[test]
    fn provider_model_review_records_ready_route_and_continues() {
        let facts = SetupRuntimeFacts {
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            auth: "present".to_string(),
            health: "ready".to_string(),
            provider_ready: true,
            provider_result:
                "provider=deepseek, model=deepseek-v4-pro, auth=present/local, health=not checked"
                    .to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::ProviderModel), StepStatus::Verified);
        assert_eq!(view.selected_step(), SetupStep::TrustSandbox);
        assert!(message.contains("已记录服务商/模型就绪状态"));
    }

    #[test]
    fn provider_model_review_records_missing_auth_as_needs_action() {
        let facts = SetupRuntimeFacts {
            provider_ready: false,
            provider_result:
                "provider=deepseek, model=deepseek-v4-pro, auth=missing, health=needs action"
                    .to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::ProviderModel),
            StepStatus::NeedsAction
        );
        assert!(message.contains("服务商/模型仍需操作"));
    }

    #[test]
    fn observed_provider_failure_records_needs_action_not_verified() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let codewhale_home = tmp.path().join(".codewhale");
        let _home = crate::test_support::EnvVarGuard::set("HOME", tmp.path());
        let _userprofile = crate::test_support::EnvVarGuard::set("USERPROFILE", tmp.path());
        let _codewhale_home =
            crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", &codewhale_home);
        let _deepseek_env = crate::test_support::EnvVarGuard::remove("DEEPSEEK_API_KEY");
        let config = Config {
            api_key: Some("saved-deepseek-key".to_string()),
            ..Default::default()
        };
        let mut app = App::new(setup_test_options(workspace), &config);
        app.api_provider = crate::config::ApiProvider::Deepseek;
        app.model = "deepseek-v4-pro".to_string();
        app.provider_health.record_failure_message(
            &config,
            crate::config::ApiProvider::Deepseek,
            "deepseek-v4-pro",
            crate::error_taxonomy::ErrorCategory::Authentication,
            "credential rejected",
        );

        let facts = SetupRuntimeFacts::from_app_config(&app, &config);
        assert!(!facts.provider_ready);
        assert!(facts.auth.contains("last check failed"), "{}", facts.auth);
        assert!(facts.provider_result.contains("health=needs action"));

        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::ProviderModel,
            facts,
        );
        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, .. }) =
            view.handle_key(key(KeyCode::Enter))
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::ProviderModel),
            StepStatus::NeedsAction
        );
    }

    #[test]
    fn runtime_posture_review_confirms_without_config_mutation() {
        let facts = SetupRuntimeFacts {
            runtime_result: "intent=agent, approval=suggest, shell=enabled, trust=workspace, sandbox=default, network=prompt by default".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::TrustSandbox), StepStatus::Verified);
        assert_eq!(
            state.runtime_posture_source,
            RuntimePostureSource::Confirmed
        );
        assert!(message.contains("已复核运行姿态"));
        assert_eq!(view.selected_step(), SetupStep::Constitution);
    }

    #[test]
    fn runtime_posture_review_result_redacts_secret_config() {
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        let codewhale_home = tmp.path().join(".codewhale");
        let _home = crate::test_support::EnvVarGuard::set("HOME", tmp.path());
        let _userprofile = crate::test_support::EnvVarGuard::set("USERPROFILE", tmp.path());
        let _codewhale_home =
            crate::test_support::EnvVarGuard::set("CODEWHALE_HOME", &codewhale_home);

        let mut config = Config {
            api_key: Some("sk-runtime-posture-secret".to_string()),
            sandbox_api_key: Some("sandbox-runtime-secret".to_string()),
            approval_policy: Some("on-request".to_string()),
            sandbox_mode: Some("workspace-write".to_string()),
            ..Config::default()
        };
        config.default_text_model = Some("deepseek-v4-pro".to_string());
        let app = App::new(setup_test_options(workspace), &config);
        let facts = SetupRuntimeFacts::from_app_config(&app, &config);
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, .. }) = action else {
            panic!("expected runtime posture commit event");
        };
        let result = state
            .steps
            .get(&SetupStep::TrustSandbox)
            .and_then(|entry| entry.result.as_deref())
            .expect("runtime posture result");
        assert!(result.contains("intent=agent"), "{result}");
        assert!(result.contains("sandbox=workspace-write"), "{result}");
        for forbidden in [
            "sk-runtime-posture-secret",
            "sandbox-runtime-secret",
            "api_key",
            "sandbox_api_key",
            "secret",
        ] {
            assert!(
                !result.contains(forbidden),
                "runtime posture result leaked {forbidden}: {result}"
            );
        }
    }

    #[test]
    fn runtime_posture_skip_records_posture_specific_state() {
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            SetupRuntimeFacts::default(),
        );

        let action = view.handle_key(key(KeyCode::Char('s')));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected runtime posture skip commit event");
        };
        let entry = state
            .steps
            .get(&SetupStep::TrustSandbox)
            .expect("trust/sandbox step entry");
        assert_eq!(entry.status, StepStatus::Skipped);
        assert!(entry.required);
        assert_eq!(entry.result.as_deref(), Some("skipped by user"));
        assert_eq!(state.runtime_posture_source, RuntimePostureSource::Unset);
        assert!(message.contains("已保存跳过此设置步骤"));
        assert_eq!(view.selected_step(), SetupStep::Constitution);
    }

    #[test]
    fn runtime_posture_detail_lines_show_preset_diff() {
        let facts = SetupRuntimeFacts {
            default_mode: "agent".to_string(),
            approval_policy_value: "on-request".to_string(),
            allow_shell_enabled: true,
            sandbox_mode_value: "workspace-write".to_string(),
            network_default_value: "prompt".to_string(),
            trust: "workspace trust not elevated".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            facts,
        );

        let text = lines_to_text(view.runtime_posture_detail_lines());

        assert!(text.contains("所选预设："));
        assert!(text.contains("普通 Agent"));
        assert!(text.contains("settings.default_mode: agent -> act"));
        assert!(text.contains("config.allow_shell: true -> true"));
        assert!(text.contains("安全底线："));
        assert!(text.contains("按 A 预览"));
    }

    #[test]
    fn runtime_posture_detail_lines_warn_about_project_overrides() {
        let tmp = tempfile::TempDir::new().expect("workspace");
        let project_dir = tmp.path().join(codewhale_config::CODEWHALE_APP_DIR);
        std::fs::create_dir_all(&project_dir).expect("project config dir");
        std::fs::write(
            project_dir.join("config.toml"),
            "approval_policy = \"never\"\nsandbox_mode = \"read-only\"\n",
        )
        .expect("project config");
        let warning = project_runtime_override_warning(tmp.path()).expect("project warning");
        let facts = SetupRuntimeFacts {
            project_override_warning: Some(warning),
            ..SetupRuntimeFacts::default()
        };
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            facts,
        );

        let text = lines_to_text(view.runtime_posture_detail_lines());

        assert!(text.contains("项目覆盖："));
        assert!(text.contains("approval_policy=never"));
        assert!(text.contains("sandbox_mode=read-only"));
        assert!(text.contains("此工作区的项目配置包含"));
        assert!(text.contains("项目配置仍可在此工作区收紧运行姿态"));
    }

    #[test]
    fn operate_fleet_detail_lines_show_read_only_facts() {
        let facts = SetupRuntimeFacts {
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            auth: "present".to_string(),
            provider_ready: true,
            operate_runtime_ready: true,
            operate_runtime_result: "worker runtime enabled for deepseek; max_subagents=4, launch_concurrency=2, admission=6".to_string(),
            fleet_roster_ready: true,
            fleet_roster_result: "3 Fleet members (1 config/workspace)".to_string(),
            operate_concurrency_result:
                "configured launch_concurrency=2; max_subagents=4; admission=6; plan limit not probed"
                    .to_string(),
            ..SetupRuntimeFacts::default()
        };
        let view = SetupWizardView::new_at_with_facts(
            first_run_ready_state(),
            SetupStep::OperateFleet,
            facts,
        );

        let text = lines_to_text(view.operate_fleet_detail_lines());

        assert!(text.contains("Worker 运行时："));
        assert!(text.contains("worker runtime enabled for deepseek"));
        assert!(text.contains("Fleet 成员表："));
        assert!(text.contains("3 Fleet members"));
        assert!(text.contains("plan limit not probed"));
        assert!(text.contains("按 Enter 记录当前 Operate/Fleet 事实"));
    }

    #[test]
    fn operate_fleet_review_records_needs_action_without_receipt_capability() {
        let facts = SetupRuntimeFacts {
            provider_ready: true,
            operate_runtime_ready: true,
            fleet_roster_ready: true,
            operate_result:
                "provider=ready, runtime=ready, roster=ready, concurrency=configured launch_concurrency=2; max_subagents=4; admission=6; plan limit not probed"
                    .to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            first_run_ready_state(),
            SetupStep::OperateFleet,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::OperateFleet),
            StepStatus::NeedsAction
        );
        assert!(!state.operate_ready());
        let result = state
            .steps
            .get(&SetupStep::OperateFleet)
            .and_then(|entry| entry.result.as_deref())
            .expect("operate result");
        assert!(result.contains("plan limit not probed"), "{result}");
        assert!(message.contains("Operate/Fleet 仍需操作"));
        assert_eq!(view.selected_step(), SetupStep::ToolsMcp);
    }

    #[test]
    fn tools_mcp_detail_lines_show_read_only_inventory_facts() {
        let facts = SetupRuntimeFacts {
            tools_mcp_servers_result: "healthy — 2 configured (2 healthy, 0 needs_config, 0 off; global present at /tmp/mcp.json; project missing at /tmp/project/.codewhale/mcp.json); healthy: docs, search".to_string(),
            tools_mcp_skills_result: "healthy — 3 discovered, 3 on disk at /tmp/skills".to_string(),
            tools_mcp_tools_result: "healthy — 1 entries, 0 script-plugin tools at /tmp/tools".to_string(),
            tools_mcp_plugins_result: "off — nothing configured yet (missing at /tmp/plugins); optional".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let view =
            SetupWizardView::new_at_with_facts(SetupState::default(), SetupStep::ToolsMcp, facts);

        let text = lines_to_text(view.tools_mcp_detail_lines());

        assert!(text.contains("MCP 服务器："));
        assert!(text.contains("healthy"));
        assert!(text.contains("/tmp/mcp.json"));
        assert!(text.contains("/tmp/project/.codewhale/mcp.json"));
        assert!(text.contains("技能："));
        assert!(text.contains("/tmp/skills"));
        assert!(text.contains("工具目录："));
        assert!(text.contains("插件："));
        assert!(text.contains("按 Enter 记录当前工具/MCP 事实"));
        assert!(text.contains("按 R 查看安全引导"));
    }

    #[test]
    fn tools_mcp_review_records_optional_snapshot_when_empty() {
        let facts = SetupRuntimeFacts {
            tools_mcp_result:
                "mcp=off, skills=off, tools=off, plugins=off, overall=off, mode=read_only_safe_probe"
                    .to_string(),
            tools_mcp_needs_action: false,
            ..SetupRuntimeFacts::default()
        };
        let mut view =
            SetupWizardView::new_at_with_facts(SetupState::default(), SetupStep::ToolsMcp, facts);

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::ToolsMcp), StepStatus::Optional);
        let entry = state
            .steps
            .get(&SetupStep::ToolsMcp)
            .expect("tools/mcp setup entry");
        assert!(!entry.required);
        assert!(
            entry
                .result
                .as_deref()
                .is_some_and(|result| result.contains("mode=read_only_safe_probe"))
        );
        assert!(message.contains("已记录工具/MCP 就绪状态"));
        assert_eq!(view.selected_step(), SetupStep::Persistence);
    }

    #[test]
    fn tools_mcp_review_records_needs_action_for_broken_config() {
        let facts = SetupRuntimeFacts {
            tools_mcp_result:
                "mcp=needs_config, skills=off, tools=off, plugins=off, overall=needs_config, mode=read_only_safe_probe"
                    .to_string(),
            tools_mcp_needs_action: true,
            ..SetupRuntimeFacts::default()
        };
        let mut view =
            SetupWizardView::new_at_with_facts(SetupState::default(), SetupStep::ToolsMcp, facts);

        let action = view.handle_key(key(KeyCode::Enter));
        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::ToolsMcp), StepStatus::NeedsAction);
        assert!(
            !state
                .steps
                .get(&SetupStep::ToolsMcp)
                .expect("entry")
                .required
        );
        assert!(message.contains("工具/MCP 仍需处理"));
        // Optional step still advances; first-run is not blocked.
        assert_eq!(view.selected_step(), SetupStep::Persistence);
    }

    #[test]
    fn tools_mcp_on_ramp_preview_is_safe() {
        let facts = SetupRuntimeFacts {
            tools_mcp_servers_result: "off — nothing configured".into(),
            tools_mcp_skills_result: "off — missing".into(),
            tools_mcp_tools_result: "off — missing".into(),
            tools_mcp_plugins_result: "off — missing".into(),
            tools_mcp_path_display: "~/.codewhale/mcp.json".into(),
            tools_mcp_skills_path_display: "~/.codewhale/skills".into(),
            tools_mcp_plugins_path_display: "~/.codewhale/plugins".into(),
            ..SetupRuntimeFacts::default()
        };
        let mut view =
            SetupWizardView::new_at_with_facts(SetupState::default(), SetupStep::ToolsMcp, facts);

        let action = view.handle_key(key(KeyCode::Char('r')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { title, content }) = action else {
            panic!("expected on-ramp pager, got {action:?}");
        };
        assert!(title.to_ascii_lowercase().contains("tool") || title.contains("MCP"));
        assert!(content.contains("/mcp") || content.contains("mcp init"));
        assert!(!content.contains("sk-"));
    }

    #[test]
    fn persistence_detail_lines_show_read_only_path_facts() {
        let facts = SetupRuntimeFacts {
            persistence: SetupPersistenceFacts {
                home_result: "explicit CODEWHALE_HOME at /tmp/cw-home (present)".to_string(),
                config_result: "/tmp/cw-home/config.toml (present)".to_string(),
                state_result: "/tmp/cw-home/setup_state.json (missing)".to_string(),
                constitution_result: "/tmp/cw-home/constitution.json (present)".to_string(),
                memory_result: "/tmp/cw-home/memory.md (missing)".to_string(),
                notes_result: "/tmp/cw-home/notes.md (exists-not-file)".to_string(),
                result: "home_source=explicit, home=present, config=present, setup_state=missing, constitution=present, memory=missing, notes=exists-not-file, mode=read_only_review".to_string(),
            },
            ..SetupRuntimeFacts::default()
        };
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Persistence,
            facts,
        );

        let text = lines_to_text(view.persistence_detail_lines());

        assert!(text.contains("Home："));
        assert!(text.contains("explicit CODEWHALE_HOME"));
        assert!(text.contains("/tmp/cw-home/config.toml"));
        assert!(text.contains("/tmp/cw-home/setup_state.json (missing)"));
        assert!(text.contains("宪法："));
        assert!(text.contains("记忆："));
        assert!(text.contains("笔记："));
        assert!(text.contains("按 Enter 记录此路径快照"));
    }

    #[test]
    fn persistence_review_records_optional_snapshot() {
        let facts = SetupRuntimeFacts {
            persistence: SetupPersistenceFacts {
                result: "home_source=explicit, home=present, config=present, setup_state=missing, constitution=present, memory=missing, notes=missing, mode=read_only_review".to_string(),
                ..SetupPersistenceFacts::default()
            },
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Persistence,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::Persistence), StepStatus::Verified);
        let entry = state
            .steps
            .get(&SetupStep::Persistence)
            .expect("persistence setup entry");
        assert!(!entry.required);
        assert!(
            entry
                .result
                .as_deref()
                .is_some_and(|result| result.contains("mode=read_only_review"))
        );
        assert!(message.contains("已记录持久化路径"));
        assert_eq!(view.selected_step(), SetupStep::Verification);
    }

    #[test]
    fn operate_fleet_review_records_needs_action_until_first_run_ready() {
        let facts = SetupRuntimeFacts {
            provider_ready: true,
            operate_runtime_ready: true,
            fleet_roster_ready: true,
            operate_result:
                "provider=ready, runtime=ready, roster=ready, concurrency=plan limit not probed"
                    .to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::OperateFleet,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::OperateFleet),
            StepStatus::NeedsAction
        );
        assert!(!state.operate_ready());
        assert!(message.contains("Operate/Fleet 仍需操作"));
    }

    #[test]
    fn runtime_posture_preset_requires_preview_before_apply() {
        let facts = SetupRuntimeFacts {
            default_mode: "agent".to_string(),
            approval_policy_value: "never".to_string(),
            allow_shell_enabled: false,
            sandbox_mode_value: "read-only".to_string(),
            network_default_value: "deny".to_string(),
            trust: "workspace trust not elevated".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::TrustSandbox,
            facts,
        );

        assert!(matches!(
            view.handle_key(key(KeyCode::Char('3'))),
            ViewAction::None
        ));
        let preview = view.handle_key(key(KeyCode::Char('a')));
        let ViewAction::Emit(ViewEvent::OpenTextPager { content, .. }) = preview else {
            panic!("first apply should preview the exact diff");
        };
        assert!(content.contains("运行姿态预设预览"));
        assert!(content.contains("settings.default_mode: agent -> act + full-access"));
        assert!(content.contains(
            "config.approval_policy: never -> removed; Full Access comes from settings.permission_posture"
        ));
        assert!(content.contains("settings.permission_posture: -> full-access"));
        assert!(content.contains("config.network.default: deny -> unchanged"));

        let action = view.handle_key(key(KeyCode::Char('a')));
        let ViewAction::Emit(ViewEvent::SetupRuntimePresetApplyRequested {
            preset,
            state,
            message,
        }) = action
        else {
            panic!("second apply should request preset persistence");
        };
        assert_eq!(preset, SetupRuntimePreset::HighTrustLocal);
        assert_eq!(state.status(SetupStep::TrustSandbox), StepStatus::Verified);
        assert_eq!(
            state.runtime_posture_source,
            RuntimePostureSource::Confirmed
        );
        assert!(
            state
                .steps
                .get(&SetupStep::TrustSandbox)
                .and_then(|entry| entry.result.as_deref())
                .is_some_and(|result| {
                    result.contains("preset=high-trust-local")
                        && result.contains("default_mode=act + full-access")
                        && result.contains("network=unchanged")
                })
        );
        assert!(message.contains("已应用运行姿态预设"));
        assert_eq!(view.selected_step(), SetupStep::Constitution);
    }

    #[test]
    fn verification_report_records_needs_action_until_checkpoint_complete() {
        let facts = SetupRuntimeFacts {
            constitution_autonomy: "balanced".to_string(),
            runtime_result: "intent=agent, approval=suggest".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Verification,
            facts,
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, message }) = action
        else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(
            state.status(SetupStep::Verification),
            StepStatus::NeedsAction
        );
        assert!(
            state
                .steps
                .get(&SetupStep::Verification)
                .and_then(|entry| entry.result.as_deref())
                .is_some_and(|result| {
                    result.contains("update=needs_action")
                        && result.contains("operate=needs_action")
                        && result.contains("autonomy=balanced")
                        && result.contains("runtime=intent=agent, approval=suggest")
                })
        );
        assert!(message.contains("设置报告已记录"));
    }

    #[test]
    fn verification_report_records_ready_after_bundled_checkpoint() {
        let mut state = SetupState::default();
        state.complete_constitution_checkpoint(
            CONSTITUTION_CHECKPOINT_VERSION,
            ConstitutionChoice::Bundled,
        );
        let mut view = SetupWizardView::new_at_with_facts(
            state,
            SetupStep::Verification,
            SetupRuntimeFacts::default(),
        );

        let action = view.handle_key(key(KeyCode::Enter));

        let ViewAction::Emit(ViewEvent::SetupStateCommitRequested { state, .. }) = action else {
            panic!("expected setup-state commit event");
        };
        assert_eq!(state.status(SetupStep::Verification), StepStatus::Verified);
        assert!(
            state
                .steps
                .get(&SetupStep::Verification)
                .and_then(|entry| entry.result.as_deref())
                .is_some_and(|result| {
                    result.contains("update=ready") && result.contains("operate=needs_action")
                })
        );
    }

    #[test]
    fn verification_detail_lines_show_next_action() {
        let facts = SetupRuntimeFacts {
            constitution_autonomy: "balanced".to_string(),
            runtime_result: "intent=agent, approval=suggest".to_string(),
            ..SetupRuntimeFacts::default()
        };
        let view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Verification,
            facts,
        );

        let text = lines_to_text(view.verification_detail_lines());

        assert!(text.contains("首次运行："));
        assert!(text.contains("更新检查点："));
        assert!(text.contains("Operate/Fleet："));
        assert!(text.contains("宪法主动性："));
        assert!(text.contains("balanced"));
        assert!(text.contains("运行时姿态："));
        assert!(text.contains("intent=agent, approval=suggest"));
        assert!(text.contains("完成宪法检查点"));
    }

    #[test]
    fn setup_wizard_body_scroll_resets_on_step_change() {
        let mut view = SetupWizardView::new(SetupState::default());
        view.body_scroll = 12;
        view.move_next();
        assert_eq!(view.body_scroll, 0, "step change should reset body scroll");
        view.body_scroll = 5;
        view.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert!(view.body_scroll >= 5);
        view.move_back();
        assert_eq!(view.body_scroll, 0);
    }

    #[test]
    fn setup_wizard_page_down_clamps_scroll_at_80x24() {
        use ratatui::text::{Line, Span};

        let mut view = SetupWizardView::new_at_with_facts(
            SetupState::default(),
            SetupStep::Constitution,
            SetupRuntimeFacts {
                constitution_file: SetupConstitutionFileState::Loaded,
                ..SetupRuntimeFacts::default()
            },
        );
        let wrap_width = 76usize;
        let visible_rows = 10usize;
        let mut lines = view.constitution_detail_lines();
        lines.extend(std::iter::repeat_n(
            Line::from(Span::raw("x".repeat(wrap_width))),
            40,
        ));
        let visual_rows: usize = lines
            .iter()
            .map(|line| line.width().div_ceil(wrap_width).max(1))
            .sum();
        let max_scroll = visual_rows.saturating_sub(visible_rows);
        assert!(max_scroll > 0, "fixture should overflow a small viewport");

        for _ in 0..32 {
            view.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        }
        assert!(
            view.body_scroll >= max_scroll.saturating_sub(8),
            "page down should reach the scroll ceiling"
        );

        let clamped = view.body_scroll.min(max_scroll);
        assert_eq!(
            clamped, max_scroll,
            "render path should clamp overshoot to max scroll"
        );
    }

    fn lines_to_text(lines: Vec<Line<'static>>) -> String {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
