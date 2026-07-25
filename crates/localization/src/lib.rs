//! Bilingual human-message registry for DSE product strings.
//!
//! Machine-facing identifiers, protocol values, paths, and raw tool output do
//! not pass through this registry.
use std::{borrow::Cow, fmt, str::FromStr, sync::OnceLock};

rust_i18n::i18n!("locales", fallback = "en");

// Keep Cargo's dependency graph aware of catalog-only edits. The proc macro
// loads this file at compile time, but an explicit include is what guarantees a
// changed message rebuilds every consumer without requiring a clean target.
pub const ENGLISH_CATALOG_SOURCE: &str = include_str!("../locales/en.json");
pub const SIMPLIFIED_CHINESE_CATALOG_SOURCE: &str = include_str!("../locales/zh-Hans.json");

/// The complete product-language surface admitted by ADR-0010.
///
/// This affects only human-facing projections. Protocol values, persisted
/// machine facts, model selection, prompts, paths, code, diffs, and raw tool
/// output remain language-independent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductLanguage {
    #[default]
    English,
    SimplifiedChinese,
}

impl ProductLanguage {
    pub const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-Hans",
        }
    }

    pub const fn catalog_source(self) -> &'static str {
        match self {
            Self::English => ENGLISH_CATALOG_SOURCE,
            Self::SimplifiedChinese => SIMPLIFIED_CHINESE_CATALOG_SOURCE,
        }
    }
}

impl fmt::Display for ProductLanguage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.tag())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedProductLanguage {
    value: String,
}

impl UnsupportedProductLanguage {
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for UnsupportedProductLanguage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported product language {:?}; expected en or zh-Hans",
            self.value
        )
    }
}

impl std::error::Error for UnsupportedProductLanguage {}

impl FromStr for ProductLanguage {
    type Err = UnsupportedProductLanguage;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "en" => Ok(Self::English),
            "zh-Hans" => Ok(Self::SimplifiedChinese),
            _ => Err(UnsupportedProductLanguage {
                value: value.to_owned(),
            }),
        }
    }
}

static PROCESS_LANGUAGE: OnceLock<ProductLanguage> = OnceLock::new();

/// Freezes the human projection language for this process.
///
/// Repeating the same choice is harmless. Attempting to change it is rejected
/// so a resumed run cannot mix human projections or create per-run locale
/// state.
pub fn set_process_language(language: ProductLanguage) -> Result<(), ProductLanguage> {
    match PROCESS_LANGUAGE.set(language) {
        Ok(()) => Ok(()),
        Err(_) if process_language() == language => Ok(()),
        Err(_) => Err(process_language()),
    }
}

/// Returns the frozen process language.
///
/// Entry points resolve every fresh non-interactive process to English before
/// rendering. The pre-resolution fallback remains Simplified Chinese so an
/// existing local installation and catalog-only callers never silently change
/// language before startup has inspected persisted state.
pub fn process_language() -> ProductLanguage {
    PROCESS_LANGUAGE
        .get()
        .copied()
        .unwrap_or(ProductLanguage::SimplifiedChinese)
}

#[must_use]
pub fn process_language_is_set() -> bool {
    PROCESS_LANGUAGE.get().is_some()
}

/// Resolves the fixed startup precedence without reading config, terminal, or
/// filesystem state inside the localization owner.
///
/// `defer_fresh_to_first_run_choice` is true only for a fresh interactive TUI
/// that is about to present the bilingual first-run choice.
#[must_use]
pub fn resolve_product_language(
    explicit: Option<ProductLanguage>,
    persisted: Option<ProductLanguage>,
    existing_local_installation: bool,
    defer_fresh_to_first_run_choice: bool,
) -> Option<ProductLanguage> {
    explicit
        .or(persisted)
        .or_else(|| existing_local_installation.then_some(ProductLanguage::SimplifiedChinese))
        .or_else(|| (!defer_fresh_to_first_run_choice).then_some(ProductLanguage::English))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageId {
    CliAbout,
    CliHelpTemplate,
    CliArgumentError,
    CliCommandDoctor,
    CliCommandRuns,
    CliCommandResume,
    CliCommandInit,
    CliCommandSetup,
    CliCommandExec,
    CliCommandMcp,
    CliCommandFeatures,
    CliCommandCompletions,
    CliCommandLogin,
    CliCommandLogout,
    CliCommandAuth,
    CliCommandConfig,
    CliCommandModel,
    CliCommandSandbox,
    CliCommandAppServer,
    CliCommandCompletion,
    CliCommandSessionDiagnostics,
    CliCommandPr,
    CliCommandExecPolicy,
    CliArgVerbosity,
    CliArgWorkspace,
    CliArgContinue,
    CliArgPrompt,
    CliArgApiKey,
    CliArgJson,
    CliArgLimit,
    CliArgHelp,
    CliArgVersion,
    CliArgEnableFeature,
    CliArgDisableFeature,
    CliArgMaxSubagents,
    CliArgConfig,
    CliArgLanguage,
    CliArgVerbose,
    CliArgProfile,
    CliArgResume,
    CliArgMouseCapture,
    CliArgNoMouseCapture,
    CliArgSkipOnboarding,
    CliArgNoProjectConfig,
    CliArgModel,
    CliArgOutputMode,
    CliArgLogLevel,
    CliArgTelemetry,
    CliArgApprovalPolicy,
    CliArgSandboxMode,
    CliArgBaseUrl,
    CliArgReasoningEffort,
    CliArgAuto,
    CliArgSandbox,
    CliArgAllowSandboxElevation,
    CliArgOutputFormat,
    CliArgAllowedTools,
    CliArgDisallowedTools,
    CliArgMaxTurns,
    CliArgMaxApiRequests,
    CliArgMaxRuntimeSecs,
    CliArgAppendSystemPrompt,
    CliExecAfterHelp,
    CliArgStdio,
    CliArgHost,
    CliArgPort,
    CliArgAuthToken,
    CliArgInsecureNoAuth,
    CliArgCorsOrigin,
    CliArgMaxBodyBytes,
    CliArgTransportMaxRetries,
    CliAppServerAfterHelp,
    DoctorTitle,
    DoctorSectionVersion,
    DoctorSectionDelivery,
    DoctorSectionConfiguration,
    DoctorSectionStateRoot,
    DoctorSectionSetupState,
    DoctorSectionApiKeys,
    DoctorSectionApiConnectivity,
    DoctorSectionMcpServers,
    DoctorSectionSkills,
    DoctorSectionPlugins,
    DoctorSectionToolDependencies,
    DoctorSectionTerminalQuirks,
    DoctorSectionPlatform,
    DoctorInstalledBuild,
    DoctorUpdateDiscoveryDisabled,
    DoctorConfigFound,
    DoctorConfigNotFound,
    DoctorWorkspace,
    DoctorStateActive,
    DoctorSetupSource,
    DoctorFirstRun,
    DoctorUpdateCheckpoint,
    DoctorConstitutionAutonomy,
    DoctorRuntimePosture,
    DoctorNextActions,
    DoctorReady,
    DoctorNeedsAction,
    DoctorRequired,
    DoctorOptional,
    DoctorUnversioned,
    DoctorNoResult,
    DoctorDeepseekKeyState,
    DoctorCredentialPrecedence,
    DoctorActiveKeySource,
    DoctorActiveKeyMissing,
    DoctorSaveKeyHint,
    DoctorProvider,
    DoctorBaseUrl,
    DoctorModel,
    DoctorConnectivityScope,
    DoctorTestingConnection,
    DoctorConnectionSuccessful,
    DoctorConnectionFailed,
    DoctorConnectionSkipped,
    DoctorMcpConfigMissing,
    DoctorProjectMcpConfigMissing,
    DoctorMcpMergedCount,
    DoctorMcpInitHint,
    DoctorPluginsMissing,
    DoctorPluginsHint,
    DoctorTerminalNoOverrides,
    DoctorOs,
    DoctorArch,
    DoctorSandboxAvailable,
    DoctorSandboxUnavailable,
    DoctorComplete,
    DoctorSourceConfig,
    DoctorSourceDefault,
    DoctorSourceInteractiveDefault,
    DoctorWorkspaceTrusted,
    DoctorWorkspaceNotTrusted,
    DoctorSearchProvider,
    DoctorSearchProviderSwitchHint,
    DoctorSourcePersisted,
    DoctorSourceDerived,
    DoctorAutonomyUnspecified,
    DoctorAutonomyCautious,
    DoctorAutonomyBalanced,
    DoctorAutonomyAutonomous,
    DoctorDirectoryFound,
    DoctorDirectoryMissing,
    DoctorSelectedSkillsDirectory,
    DoctorSkillsSetupHint,
    DoctorPluginsFound,
    DoctorPythonAvailable,
    DoctorPythonMissing,
    DoctorToolNotAdvertised,
    DoctorInstallDependencyHint,
    DoctorPandocAvailable,
    DoctorPandocMissing,
    DoctorOcrAvailable,
    DoctorTesseractAvailable,
    DoctorPdftotextAvailable,
    DoctorPdftotextMissing,
    DoctorPdftotextOptionalHint,
    DoctorInvalidApiKey,
    DoctorRejectedKeyFromKeyring,
    DoctorInspectCredentialSources,
    DoctorRejectedKeyFromEnv,
    DoctorSaveConfigKeyOverridesEnv,
    DoctorApiKeyPermissionDenied,
    DoctorDnsFailure,
    DoctorConnectFailure,
    DoctorRawError,
    DoctorTimeout,
    DoctorTimeoutOfficialHint,
    DoctorTimeoutFixtureHint,
    DoctorTimeoutReportHint,
    DoctorTlsVerificationEnforced,
    DoctorMcpConfigFound,
    DoctorProjectMcpConfigFound,
    DoctorMcpServerDisabled,
    DoctorMcpConfigParseError,
    DoctorMcpNoCommand,
    DoctorMcpHttpServer,
    DoctorMcpEmptyCommand,
    DoctorMcpCommandNotFound,
    DoctorMcpRelativeCommand,
    DoctorMcpRelativeArg,
    DoctorMcpStdioServer,
    DoctorSkillWorkspaceLabel,
    DoctorSkillAgentsLabel,
    DoctorSkillGlobalAgentsLabel,
    DoctorSkillGlobalLabel,
    DoctorSkillOpencodeLabel,
    DoctorSkillClaudeLabel,
    DoctorPythonMacInstall,
    DoctorPythonLinuxInstall,
    DoctorPythonWindowsInstall,
    DoctorPythonOtherInstall,
    DoctorPandocLinuxInstall,
    DoctorPandocOtherInstall,
    DoctorOcrVisionFallback,
    DoctorTesseractOptionalMissing,
    DoctorOcrNotAdvertised,
    DoctorTesseractLinuxInstall,
    DoctorTesseractOtherInstall,
    DoctorPdfExternalActive,
    DoctorPdfExternalHint,
    DoctorPdfExternalMissing,
    DoctorPdfExternalFallback,
    DoctorPopplerMacInstall,
    DoctorPopplerLinuxInstall,
    DoctorPopplerWindowsInstall,
    DoctorTerminalLowMotion,
    DoctorTerminalSshLowMotion,
    DoctorTerminalPtyxis,
    DoctorTerminalLegacyWindows,
    DoctorSetupInconsistent,
    UserInputActionRequired,
    UserInputQuestionProgress,
    UserInputOther,
    UserInputOtherDescription,
    UserInputConfirmSelection,
    UserInputSubmitSelected,
    UserInputCustomResponse,
    UserInputTypeResponse,
    UserInputValidationSelect,
    UserInputValidationResponse,
    UserInputSubmit,
    UserInputBack,
    UserInputMove,
    UserInputToggle,
    UserInputToggleConfirm,
    UserInputCancel,
    UserInputQuickPick,
    UserInputDigit,
    TuiOnboardingMarkFailed,
    TuiPasteDirectoryFailed,
    TuiPasteWriteFailed,
    ExecOutputFailed,
    ExecRuntimeFailed,
    CliErrorPrefix,
    CliCausedByPrefix,
    CliContinueInteractiveConflict,
    CliExecFlagPlacement,
    CliReadApiKeyFailed,
    CliConfigKeyNotFound,
    CliConfigSet,
    CliConfigUnset,
    CliAppServerRuntimeFailed,
    CliAppServerStdioFailed,
    CliAppServerHttpFailed,
    CliAppServerOfficialEndpointOnly,
    CliPromptPreferencesFailed,
    CliTuiNoExitCode,
    CliCurrentExecutableFailed,
    CliEmptyApiKey,
    ComposerPlaceholder,
    CmdCostDescription,
    CmdExitDescription,
    CmdHelpDescription,
    CmdCostReport,
    // Canonical foreground slash-command presentation.
    CanonicalCommandRequired,
    CanonicalCommandUnknown,
    CanonicalCommandNoArguments,
    CanonicalCommandListTitle,
    CanonicalCommandAliases,
    FooterWorkspacePrefix,
    // Onboarding screens — welcome.
    OnboardPanelTitle,
    OnboardStepProgress,
    OnboardHomeDirectoryNotFound,
    OnboardWelcomeVersion,
    OnboardWelcomeLead,
    OnboardWelcomeSetupBlurb,
    OnboardWelcomeSteps,
    OnboardWelcomeStepApiKey,
    OnboardWelcomeStepTrust,
    OnboardWelcomeStepTips,
    OnboardWelcomeDefaults,
    OnboardWelcomeEnter,
    OnboardWelcomeExit,
    OnboardApiKeyTitle,
    OnboardApiKeyStep1,
    OnboardApiKeyStep2,
    OnboardApiKeySavedHint,
    OnboardApiKeyFormatHint,
    OnboardApiKeyPlaceholder,
    OnboardApiKeyLabel,
    OnboardApiKeyFooter,
    OnboardApiKeyEmpty,
    OnboardApiKeyWhitespace,
    OnboardApiKeyShortWarning,
    OnboardApiKeyUnusualWarning,
    // Onboarding screens — workspace trust prompt.
    OnboardTrustTitle,
    OnboardTrustQuestion,
    OnboardTrustLocationPrefix,
    OnboardTrustRiskHint,
    OnboardTrustEffectHint,
    OnboardTrustFooterPrefix,
    OnboardTrustFooterMiddle,
    OnboardTrustFooterSuffix,
    OnboardTrustConfirmHint,
    OnboardTrustSaveFailed,
    // Onboarding screens — final tips screen.
    OnboardTipsTitle,
    OnboardTipsLine1,
    OnboardTipsLine2,
    OnboardTipsLine3,
    OnboardTipsLine4,
    OnboardTipsFooterEnter,
    OnboardTipsFooterAction,
    // Retained canonical TUI foreground.
    CanonicalWorkspaceCanonicalizeFailed,
    CanonicalWorkspaceNotDirectory,
    CanonicalNoRecoverableRun,
    CanonicalRecoveringInterruptedRun,
    CanonicalInitialCommandConfirmation,
    CanonicalUnknownValue,
    CanonicalAmbiguousPendingCreations,
    CanonicalAmbiguousPendingCreationsMore,
    CanonicalCancelAwaitTerminal,
    CanonicalWaitTerminalBeforeExit,
    CanonicalInterruptAccepted,
    CanonicalWaitTerminal,
    CanonicalCancelBeforeExit,
    CanonicalHelpShown,
    CanonicalCostShown,
    CanonicalSteerSubmitFailed,
    CanonicalWaitBeforeNextInput,
    CanonicalRunSubmitFailed,
    CanonicalLegacyActionUnavailable,
    CanonicalMismatchedInteractionReceipt,
    // Approval dialog — risk badges, category labels, field labels, options.
    ApprovalRiskReview,
    ApprovalRiskElevated,
    ApprovalRiskDestructive,
    ApprovalCategorySafe,
    ApprovalCategoryFileWrite,
    ApprovalCategoryShell,
    ApprovalCategoryNetwork,
    ApprovalCategoryMcpRead,
    ApprovalCategoryMcpAction,
    ApprovalCategoryAgent,
    ApprovalCategoryUnknown,
    ApprovalFieldType,
    ApprovalFieldAbout,
    ApprovalFieldImpact,
    ApprovalFieldParams,
    ApprovalOptionApproveOnce,
    ApprovalOptionDeny,
    ApprovalOptionAbortTurn,
    ApprovalControlsHint,
    ApprovalIntentLabel,
    ApprovalMoreLines,
    ApprovalEmptyContent,
    ApprovalMorePatchLines,
    ApprovalMoreFiles,
    ApprovalUnknownFile,
    // Approval dialog — localized descriptions.
    ApprovalDescSafe,
    ApprovalDescFileWrite,
    ApprovalDescShell,
    ApprovalDescNetwork,
    ApprovalDescMcpRead,
    ApprovalDescMcpAction,
    ApprovalDescAgent,
    ApprovalDescUnknown,
    // Approval impact summaries.
    ApprovalImpactSafe,
    ApprovalImpactFileWrite,
    ApprovalImpactShell,
    ApprovalImpactNetwork,
    ApprovalImpactMcpRead,
    ApprovalImpactMcpAction,
    ApprovalImpactAgent,
    ApprovalImpactUnknown,
    // Approval detail labels.
    ApprovalLabelCommand,
    ApprovalLabelDir,
    ApprovalLabelFile,
    ApprovalLabelPreview,
    ApprovalLabelProposedContent,
    ApprovalLabelReplaceThis,
    ApprovalLabelWithThis,
    ApprovalLabelReplacementContent,
    ApprovalLabelPath,
    ApprovalLabelTarget,
    ApprovalLabelInput,
    ApprovalLabelAction,
    ApprovalLabelType,
    ApprovalLabelPrompt,
    // Approval header labels.
    ApprovalLabelAbout,
    ApprovalLabelImpact,
    // Underwater shell phase words (footer status band).
    PhaseIdle,
    PhaseDraft,
    PhaseWorking,
    PhaseWaitingOnYou,
    PhaseDone,
    PhaseFailed,
    // Underwater header chips: mode and permission words.
    ChipPermissionAsk,
    ChipPermissionAutoApprove,
    // Underwater post-launch empty state.
    EmptyStateMcpLabel,
    SidebarWorkersLabel,
    // Sidebar work strip.
    SidebarTasksLabel,
    // Composer slash menu.
    ComposerSlashMenuHint,
    // Canonical transcript chrome.
    HistoryReasoningTitle,
    HistoryReasoningPlaceholder,
    HistoryReasoningHiddenActivity,
    HistoryReasoningStatusLive,
    HistoryReasoningStatusDone,
    HistorySystemNoteLabel,
    // Canonical exec terminal summaries.
    ExecModelRequestBudgetExhausted,
    ExecApiRequestBudgetExhausted,
    ExecTurnBudgetExhausted,
    ExecToolBudgetExhausted,
    ExecDepthLimitExceeded,
    ExecTimeout,
    ExecIncompleteModelStream,
    ExecContextLimitExceeded,
    ExecEmptyModelOutput,
    ExecOutputLimit,
    ExecContentFiltered,
    ExecInsufficientSystemResource,
    ExecOutputWriteFailed,
    ExecOutputQueueClosed,
    ExecOutputAcknowledgementDropped,
    ExecToolStarted,
    ExecToolCompleted,
    ExecToolFailed,
}

#[cfg(test)]
pub const ALL_MESSAGE_IDS: &[MessageId] = &[
    MessageId::CliAbout,
    MessageId::CliHelpTemplate,
    MessageId::CliArgumentError,
    MessageId::CliCommandDoctor,
    MessageId::CliCommandRuns,
    MessageId::CliCommandResume,
    MessageId::CliCommandInit,
    MessageId::CliCommandSetup,
    MessageId::CliCommandExec,
    MessageId::CliCommandMcp,
    MessageId::CliCommandFeatures,
    MessageId::CliCommandCompletions,
    MessageId::CliCommandLogin,
    MessageId::CliCommandLogout,
    MessageId::CliCommandAuth,
    MessageId::CliCommandConfig,
    MessageId::CliCommandModel,
    MessageId::CliCommandSandbox,
    MessageId::CliCommandAppServer,
    MessageId::CliCommandCompletion,
    MessageId::CliCommandSessionDiagnostics,
    MessageId::CliCommandPr,
    MessageId::CliCommandExecPolicy,
    MessageId::CliArgVerbosity,
    MessageId::CliArgWorkspace,
    MessageId::CliArgContinue,
    MessageId::CliArgPrompt,
    MessageId::CliArgApiKey,
    MessageId::CliArgJson,
    MessageId::CliArgLimit,
    MessageId::CliArgHelp,
    MessageId::CliArgVersion,
    MessageId::CliArgEnableFeature,
    MessageId::CliArgDisableFeature,
    MessageId::CliArgMaxSubagents,
    MessageId::CliArgConfig,
    MessageId::CliArgLanguage,
    MessageId::CliArgVerbose,
    MessageId::CliArgProfile,
    MessageId::CliArgResume,
    MessageId::CliArgMouseCapture,
    MessageId::CliArgNoMouseCapture,
    MessageId::CliArgSkipOnboarding,
    MessageId::CliArgNoProjectConfig,
    MessageId::CliArgModel,
    MessageId::CliArgOutputMode,
    MessageId::CliArgLogLevel,
    MessageId::CliArgTelemetry,
    MessageId::CliArgApprovalPolicy,
    MessageId::CliArgSandboxMode,
    MessageId::CliArgBaseUrl,
    MessageId::CliArgReasoningEffort,
    MessageId::CliArgAuto,
    MessageId::CliArgSandbox,
    MessageId::CliArgAllowSandboxElevation,
    MessageId::CliArgOutputFormat,
    MessageId::CliArgAllowedTools,
    MessageId::CliArgDisallowedTools,
    MessageId::CliArgMaxTurns,
    MessageId::CliArgMaxApiRequests,
    MessageId::CliArgMaxRuntimeSecs,
    MessageId::CliArgAppendSystemPrompt,
    MessageId::CliExecAfterHelp,
    MessageId::CliArgStdio,
    MessageId::CliArgHost,
    MessageId::CliArgPort,
    MessageId::CliArgAuthToken,
    MessageId::CliArgInsecureNoAuth,
    MessageId::CliArgCorsOrigin,
    MessageId::CliArgMaxBodyBytes,
    MessageId::CliArgTransportMaxRetries,
    MessageId::CliAppServerAfterHelp,
    MessageId::DoctorTitle,
    MessageId::DoctorSectionVersion,
    MessageId::DoctorSectionDelivery,
    MessageId::DoctorSectionConfiguration,
    MessageId::DoctorSectionStateRoot,
    MessageId::DoctorSectionSetupState,
    MessageId::DoctorSectionApiKeys,
    MessageId::DoctorSectionApiConnectivity,
    MessageId::DoctorSectionMcpServers,
    MessageId::DoctorSectionSkills,
    MessageId::DoctorSectionPlugins,
    MessageId::DoctorSectionToolDependencies,
    MessageId::DoctorSectionTerminalQuirks,
    MessageId::DoctorSectionPlatform,
    MessageId::DoctorInstalledBuild,
    MessageId::DoctorUpdateDiscoveryDisabled,
    MessageId::DoctorConfigFound,
    MessageId::DoctorConfigNotFound,
    MessageId::DoctorWorkspace,
    MessageId::DoctorStateActive,
    MessageId::DoctorSetupSource,
    MessageId::DoctorFirstRun,
    MessageId::DoctorUpdateCheckpoint,
    MessageId::DoctorConstitutionAutonomy,
    MessageId::DoctorRuntimePosture,
    MessageId::DoctorNextActions,
    MessageId::DoctorReady,
    MessageId::DoctorNeedsAction,
    MessageId::DoctorRequired,
    MessageId::DoctorOptional,
    MessageId::DoctorUnversioned,
    MessageId::DoctorNoResult,
    MessageId::DoctorDeepseekKeyState,
    MessageId::DoctorCredentialPrecedence,
    MessageId::DoctorActiveKeySource,
    MessageId::DoctorActiveKeyMissing,
    MessageId::DoctorSaveKeyHint,
    MessageId::DoctorProvider,
    MessageId::DoctorBaseUrl,
    MessageId::DoctorModel,
    MessageId::DoctorConnectivityScope,
    MessageId::DoctorTestingConnection,
    MessageId::DoctorConnectionSuccessful,
    MessageId::DoctorConnectionFailed,
    MessageId::DoctorConnectionSkipped,
    MessageId::DoctorMcpConfigMissing,
    MessageId::DoctorProjectMcpConfigMissing,
    MessageId::DoctorMcpMergedCount,
    MessageId::DoctorMcpInitHint,
    MessageId::DoctorPluginsMissing,
    MessageId::DoctorPluginsHint,
    MessageId::DoctorTerminalNoOverrides,
    MessageId::DoctorOs,
    MessageId::DoctorArch,
    MessageId::DoctorSandboxAvailable,
    MessageId::DoctorSandboxUnavailable,
    MessageId::DoctorComplete,
    MessageId::DoctorSourceConfig,
    MessageId::DoctorSourceDefault,
    MessageId::DoctorSourceInteractiveDefault,
    MessageId::DoctorWorkspaceTrusted,
    MessageId::DoctorWorkspaceNotTrusted,
    MessageId::DoctorSearchProvider,
    MessageId::DoctorSearchProviderSwitchHint,
    MessageId::DoctorSourcePersisted,
    MessageId::DoctorSourceDerived,
    MessageId::DoctorAutonomyUnspecified,
    MessageId::DoctorAutonomyCautious,
    MessageId::DoctorAutonomyBalanced,
    MessageId::DoctorAutonomyAutonomous,
    MessageId::DoctorDirectoryFound,
    MessageId::DoctorDirectoryMissing,
    MessageId::DoctorSelectedSkillsDirectory,
    MessageId::DoctorSkillsSetupHint,
    MessageId::DoctorPluginsFound,
    MessageId::DoctorPythonAvailable,
    MessageId::DoctorPythonMissing,
    MessageId::DoctorToolNotAdvertised,
    MessageId::DoctorInstallDependencyHint,
    MessageId::DoctorPandocAvailable,
    MessageId::DoctorPandocMissing,
    MessageId::DoctorOcrAvailable,
    MessageId::DoctorTesseractAvailable,
    MessageId::DoctorPdftotextAvailable,
    MessageId::DoctorPdftotextMissing,
    MessageId::DoctorPdftotextOptionalHint,
    MessageId::DoctorInvalidApiKey,
    MessageId::DoctorRejectedKeyFromKeyring,
    MessageId::DoctorInspectCredentialSources,
    MessageId::DoctorRejectedKeyFromEnv,
    MessageId::DoctorSaveConfigKeyOverridesEnv,
    MessageId::DoctorApiKeyPermissionDenied,
    MessageId::DoctorDnsFailure,
    MessageId::DoctorConnectFailure,
    MessageId::DoctorRawError,
    MessageId::DoctorTimeout,
    MessageId::DoctorTimeoutOfficialHint,
    MessageId::DoctorTimeoutFixtureHint,
    MessageId::DoctorTimeoutReportHint,
    MessageId::DoctorTlsVerificationEnforced,
    MessageId::DoctorMcpConfigFound,
    MessageId::DoctorProjectMcpConfigFound,
    MessageId::DoctorMcpServerDisabled,
    MessageId::DoctorMcpConfigParseError,
    MessageId::DoctorMcpNoCommand,
    MessageId::DoctorMcpHttpServer,
    MessageId::DoctorMcpEmptyCommand,
    MessageId::DoctorMcpCommandNotFound,
    MessageId::DoctorMcpRelativeCommand,
    MessageId::DoctorMcpRelativeArg,
    MessageId::DoctorMcpStdioServer,
    MessageId::DoctorSkillWorkspaceLabel,
    MessageId::DoctorSkillAgentsLabel,
    MessageId::DoctorSkillGlobalAgentsLabel,
    MessageId::DoctorSkillGlobalLabel,
    MessageId::DoctorSkillOpencodeLabel,
    MessageId::DoctorSkillClaudeLabel,
    MessageId::DoctorPythonMacInstall,
    MessageId::DoctorPythonLinuxInstall,
    MessageId::DoctorPythonWindowsInstall,
    MessageId::DoctorPythonOtherInstall,
    MessageId::DoctorPandocLinuxInstall,
    MessageId::DoctorPandocOtherInstall,
    MessageId::DoctorOcrVisionFallback,
    MessageId::DoctorTesseractOptionalMissing,
    MessageId::DoctorOcrNotAdvertised,
    MessageId::DoctorTesseractLinuxInstall,
    MessageId::DoctorTesseractOtherInstall,
    MessageId::DoctorPdfExternalActive,
    MessageId::DoctorPdfExternalHint,
    MessageId::DoctorPdfExternalMissing,
    MessageId::DoctorPdfExternalFallback,
    MessageId::DoctorPopplerMacInstall,
    MessageId::DoctorPopplerLinuxInstall,
    MessageId::DoctorPopplerWindowsInstall,
    MessageId::DoctorTerminalLowMotion,
    MessageId::DoctorTerminalSshLowMotion,
    MessageId::DoctorTerminalPtyxis,
    MessageId::DoctorTerminalLegacyWindows,
    MessageId::DoctorSetupInconsistent,
    MessageId::UserInputActionRequired,
    MessageId::UserInputQuestionProgress,
    MessageId::UserInputOther,
    MessageId::UserInputOtherDescription,
    MessageId::UserInputConfirmSelection,
    MessageId::UserInputSubmitSelected,
    MessageId::UserInputCustomResponse,
    MessageId::UserInputTypeResponse,
    MessageId::UserInputValidationSelect,
    MessageId::UserInputValidationResponse,
    MessageId::UserInputSubmit,
    MessageId::UserInputBack,
    MessageId::UserInputMove,
    MessageId::UserInputToggle,
    MessageId::UserInputToggleConfirm,
    MessageId::UserInputCancel,
    MessageId::UserInputQuickPick,
    MessageId::UserInputDigit,
    MessageId::TuiOnboardingMarkFailed,
    MessageId::TuiPasteDirectoryFailed,
    MessageId::TuiPasteWriteFailed,
    MessageId::ExecOutputFailed,
    MessageId::ExecRuntimeFailed,
    MessageId::CliErrorPrefix,
    MessageId::CliCausedByPrefix,
    MessageId::CliContinueInteractiveConflict,
    MessageId::CliExecFlagPlacement,
    MessageId::CliReadApiKeyFailed,
    MessageId::CliConfigKeyNotFound,
    MessageId::CliConfigSet,
    MessageId::CliConfigUnset,
    MessageId::CliAppServerRuntimeFailed,
    MessageId::CliAppServerStdioFailed,
    MessageId::CliAppServerHttpFailed,
    MessageId::CliAppServerOfficialEndpointOnly,
    MessageId::CliPromptPreferencesFailed,
    MessageId::CliTuiNoExitCode,
    MessageId::CliCurrentExecutableFailed,
    MessageId::CliEmptyApiKey,
    MessageId::ComposerPlaceholder,
    MessageId::CmdCostDescription,
    MessageId::CmdExitDescription,
    MessageId::CmdHelpDescription,
    MessageId::CmdCostReport,
    MessageId::CanonicalCommandRequired,
    MessageId::CanonicalCommandUnknown,
    MessageId::CanonicalCommandNoArguments,
    MessageId::CanonicalCommandListTitle,
    MessageId::CanonicalCommandAliases,
    MessageId::FooterWorkspacePrefix,
    MessageId::OnboardPanelTitle,
    MessageId::OnboardStepProgress,
    MessageId::OnboardHomeDirectoryNotFound,
    MessageId::OnboardWelcomeVersion,
    MessageId::OnboardWelcomeLead,
    MessageId::OnboardWelcomeSetupBlurb,
    MessageId::OnboardWelcomeSteps,
    MessageId::OnboardWelcomeStepApiKey,
    MessageId::OnboardWelcomeStepTrust,
    MessageId::OnboardWelcomeStepTips,
    MessageId::OnboardWelcomeDefaults,
    MessageId::OnboardWelcomeEnter,
    MessageId::OnboardWelcomeExit,
    MessageId::OnboardApiKeyTitle,
    MessageId::OnboardApiKeyStep1,
    MessageId::OnboardApiKeyStep2,
    MessageId::OnboardApiKeySavedHint,
    MessageId::OnboardApiKeyFormatHint,
    MessageId::OnboardApiKeyPlaceholder,
    MessageId::OnboardApiKeyLabel,
    MessageId::OnboardApiKeyFooter,
    MessageId::OnboardApiKeyEmpty,
    MessageId::OnboardApiKeyWhitespace,
    MessageId::OnboardApiKeyShortWarning,
    MessageId::OnboardApiKeyUnusualWarning,
    MessageId::OnboardTrustTitle,
    MessageId::OnboardTrustQuestion,
    MessageId::OnboardTrustLocationPrefix,
    MessageId::OnboardTrustRiskHint,
    MessageId::OnboardTrustEffectHint,
    MessageId::OnboardTrustFooterPrefix,
    MessageId::OnboardTrustFooterMiddle,
    MessageId::OnboardTrustFooterSuffix,
    MessageId::OnboardTrustConfirmHint,
    MessageId::OnboardTrustSaveFailed,
    MessageId::OnboardTipsTitle,
    MessageId::OnboardTipsLine1,
    MessageId::OnboardTipsLine2,
    MessageId::OnboardTipsLine3,
    MessageId::OnboardTipsLine4,
    MessageId::OnboardTipsFooterEnter,
    MessageId::OnboardTipsFooterAction,
    MessageId::CanonicalWorkspaceCanonicalizeFailed,
    MessageId::CanonicalWorkspaceNotDirectory,
    MessageId::CanonicalNoRecoverableRun,
    MessageId::CanonicalRecoveringInterruptedRun,
    MessageId::CanonicalInitialCommandConfirmation,
    MessageId::CanonicalUnknownValue,
    MessageId::CanonicalAmbiguousPendingCreations,
    MessageId::CanonicalAmbiguousPendingCreationsMore,
    MessageId::CanonicalCancelAwaitTerminal,
    MessageId::CanonicalWaitTerminalBeforeExit,
    MessageId::CanonicalInterruptAccepted,
    MessageId::CanonicalWaitTerminal,
    MessageId::CanonicalCancelBeforeExit,
    MessageId::CanonicalHelpShown,
    MessageId::CanonicalCostShown,
    MessageId::CanonicalSteerSubmitFailed,
    MessageId::CanonicalWaitBeforeNextInput,
    MessageId::CanonicalRunSubmitFailed,
    MessageId::CanonicalLegacyActionUnavailable,
    MessageId::CanonicalMismatchedInteractionReceipt,
    MessageId::ApprovalRiskReview,
    MessageId::ApprovalRiskElevated,
    MessageId::ApprovalRiskDestructive,
    MessageId::ApprovalCategorySafe,
    MessageId::ApprovalCategoryFileWrite,
    MessageId::ApprovalCategoryShell,
    MessageId::ApprovalCategoryNetwork,
    MessageId::ApprovalCategoryMcpRead,
    MessageId::ApprovalCategoryMcpAction,
    MessageId::ApprovalCategoryAgent,
    MessageId::ApprovalCategoryUnknown,
    MessageId::ApprovalFieldType,
    MessageId::ApprovalFieldAbout,
    MessageId::ApprovalFieldImpact,
    MessageId::ApprovalFieldParams,
    MessageId::ApprovalOptionApproveOnce,
    MessageId::ApprovalOptionDeny,
    MessageId::ApprovalOptionAbortTurn,
    MessageId::ApprovalControlsHint,
    MessageId::ApprovalIntentLabel,
    MessageId::ApprovalMoreLines,
    MessageId::ApprovalEmptyContent,
    MessageId::ApprovalMorePatchLines,
    MessageId::ApprovalMoreFiles,
    MessageId::ApprovalUnknownFile,
    MessageId::ApprovalDescSafe,
    MessageId::ApprovalDescFileWrite,
    MessageId::ApprovalDescShell,
    MessageId::ApprovalDescNetwork,
    MessageId::ApprovalDescMcpRead,
    MessageId::ApprovalDescMcpAction,
    MessageId::ApprovalDescAgent,
    MessageId::ApprovalDescUnknown,
    MessageId::ApprovalImpactSafe,
    MessageId::ApprovalImpactFileWrite,
    MessageId::ApprovalImpactShell,
    MessageId::ApprovalImpactNetwork,
    MessageId::ApprovalImpactMcpRead,
    MessageId::ApprovalImpactMcpAction,
    MessageId::ApprovalImpactAgent,
    MessageId::ApprovalImpactUnknown,
    MessageId::ApprovalLabelCommand,
    MessageId::ApprovalLabelDir,
    MessageId::ApprovalLabelFile,
    MessageId::ApprovalLabelPreview,
    MessageId::ApprovalLabelProposedContent,
    MessageId::ApprovalLabelReplaceThis,
    MessageId::ApprovalLabelWithThis,
    MessageId::ApprovalLabelReplacementContent,
    MessageId::ApprovalLabelPath,
    MessageId::ApprovalLabelTarget,
    MessageId::ApprovalLabelInput,
    MessageId::ApprovalLabelAction,
    MessageId::ApprovalLabelType,
    MessageId::ApprovalLabelPrompt,
    MessageId::ApprovalLabelAbout,
    MessageId::ApprovalLabelImpact,
    MessageId::PhaseIdle,
    MessageId::PhaseDraft,
    MessageId::PhaseWorking,
    MessageId::PhaseWaitingOnYou,
    MessageId::PhaseDone,
    MessageId::PhaseFailed,
    MessageId::ChipPermissionAsk,
    MessageId::ChipPermissionAutoApprove,
    MessageId::EmptyStateMcpLabel,
    MessageId::SidebarWorkersLabel,
    MessageId::SidebarTasksLabel,
    MessageId::ComposerSlashMenuHint,
    MessageId::HistoryReasoningTitle,
    MessageId::HistoryReasoningPlaceholder,
    MessageId::HistoryReasoningHiddenActivity,
    MessageId::HistoryReasoningStatusLive,
    MessageId::HistoryReasoningStatusDone,
    MessageId::HistorySystemNoteLabel,
    MessageId::ExecModelRequestBudgetExhausted,
    MessageId::ExecApiRequestBudgetExhausted,
    MessageId::ExecTurnBudgetExhausted,
    MessageId::ExecToolBudgetExhausted,
    MessageId::ExecDepthLimitExceeded,
    MessageId::ExecTimeout,
    MessageId::ExecIncompleteModelStream,
    MessageId::ExecContextLimitExceeded,
    MessageId::ExecEmptyModelOutput,
    MessageId::ExecOutputLimit,
    MessageId::ExecContentFiltered,
    MessageId::ExecInsufficientSystemResource,
    MessageId::ExecOutputWriteFailed,
    MessageId::ExecOutputQueueClosed,
    MessageId::ExecOutputAcknowledgementDropped,
    MessageId::ExecToolStarted,
    MessageId::ExecToolCompleted,
    MessageId::ExecToolFailed,
];

pub fn tr_in(language: ProductLanguage, id: MessageId) -> Cow<'static, str> {
    rust_i18n::t!(format!("{id:?}"), locale = language.tag())
}

pub fn tr(id: MessageId) -> Cow<'static, str> {
    tr_in(process_language(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn missing_message_ids(language: ProductLanguage) -> Vec<MessageId> {
        ALL_MESSAGE_IDS
            .iter()
            .copied()
            .filter(|id| tr_in(language, *id).eq(&format!("{id:?}")))
            .collect()
    }

    #[test]
    fn message_pack_has_no_missing_core_messages() {
        for language in ProductLanguage::ALL {
            assert!(
                missing_message_ids(language).is_empty(),
                "{language} has missing messages"
            );
        }
    }

    fn raw_messages(language: ProductLanguage) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(language.catalog_source())
            .unwrap_or_else(|error| panic!("{language} message catalog should parse: {error}"))
    }

    fn raw_message_keys(language: ProductLanguage) -> std::collections::BTreeSet<String> {
        raw_messages(language).keys().cloned().collect()
    }

    #[test]
    fn message_id_list_and_catalog_stay_in_exact_sync() {
        let ids: std::collections::BTreeSet<String> =
            ALL_MESSAGE_IDS.iter().map(|id| format!("{id:?}")).collect();
        assert_eq!(
            ids.len(),
            ALL_MESSAGE_IDS.len(),
            "ALL_MESSAGE_IDS contains duplicates"
        );
        for language in ProductLanguage::ALL {
            let catalog = raw_message_keys(language);
            let unlisted: Vec<_> = catalog.difference(&ids).collect();
            assert!(
                unlisted.is_empty(),
                "{language} keys absent from ALL_MESSAGE_IDS: {unlisted:?}"
            );
            let missing: Vec<_> = ids.difference(&catalog).collect();
            assert!(
                missing.is_empty(),
                "ALL_MESSAGE_IDS entries without a {language} string: {missing:?}"
            );
        }
    }

    #[test]
    fn bilingual_product_has_exactly_the_admitted_catalogs() {
        let locale_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut catalogs = std::fs::read_dir(locale_dir)
            .expect("read fixed locale directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        catalogs.sort();
        assert_eq!(
            catalogs
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["en.json", "zh-Hans.json"]
        );
    }

    #[test]
    fn catalogs_have_identical_named_placeholders() {
        fn placeholders(value: &str) -> Vec<&str> {
            let mut result = Vec::new();
            let mut rest = value;
            while let Some(open) = rest.find('{') {
                let after_open = &rest[open + 1..];
                let Some(close) = after_open.find('}') else {
                    break;
                };
                let name = &after_open[..close];
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                {
                    result.push(name);
                }
                rest = &after_open[close + 1..];
            }
            result.sort_unstable();
            result
        }

        let english = raw_messages(ProductLanguage::English);
        let chinese = raw_messages(ProductLanguage::SimplifiedChinese);
        for (key, english_value) in english {
            let english_value = english_value
                .as_str()
                .unwrap_or_else(|| panic!("en {key} must be a string"));
            let chinese_value = chinese[&key]
                .as_str()
                .unwrap_or_else(|| panic!("zh-Hans {key} must be a string"));
            assert_eq!(
                placeholders(english_value),
                placeholders(chinese_value),
                "placeholder mismatch for {key}"
            );
        }
    }

    #[test]
    fn language_parser_is_strict_and_default_is_english() {
        assert_eq!(
            "en".parse::<ProductLanguage>(),
            Ok(ProductLanguage::English)
        );
        assert_eq!(
            "zh-Hans".parse::<ProductLanguage>(),
            Ok(ProductLanguage::SimplifiedChinese)
        );
        assert!("en-US".parse::<ProductLanguage>().is_err());
        assert!("zh-hans".parse::<ProductLanguage>().is_err());
        assert_eq!(ProductLanguage::default(), ProductLanguage::English);
    }

    #[test]
    fn startup_resolution_is_explicit_then_persisted_then_migration_then_fresh() {
        assert_eq!(
            resolve_product_language(
                Some(ProductLanguage::English),
                Some(ProductLanguage::SimplifiedChinese),
                true,
                false,
            ),
            Some(ProductLanguage::English)
        );
        assert_eq!(
            resolve_product_language(None, Some(ProductLanguage::SimplifiedChinese), false, false,),
            Some(ProductLanguage::SimplifiedChinese)
        );
        assert_eq!(
            resolve_product_language(None, None, true, false),
            Some(ProductLanguage::SimplifiedChinese)
        );
        assert_eq!(
            resolve_product_language(None, None, false, false),
            Some(ProductLanguage::English)
        );
        assert_eq!(resolve_product_language(None, None, false, true), None);
    }
}
