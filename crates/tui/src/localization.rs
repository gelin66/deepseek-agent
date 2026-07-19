//! Simplified Chinese message registry for user-facing TUI strings.
//!
//! Machine-facing identifiers, protocol values, paths, and raw tool output do
//! not pass through this registry.
use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageId {
    ComposerPlaceholder,
    HistorySearchPlaceholder,
    HistorySearchTitle,
    HistoryHintMove,
    HistoryHintAccept,
    HistoryHintRestore,
    HistoryNoMatches,
    CommandPaletteTitle,
    CommandPaletteSubtitle,
    HelpTitle,
    HelpSubtitle,
    HelpFilterPlaceholder,
    HelpFilterPrefix,
    HelpNoMatches,
    HelpSlashCommands,
    HelpKeybindings,
    HelpFooterTypeFilter,
    HelpFooterMove,
    HelpFooterJump,
    HelpFooterClose,
    CmdAnchorDescription,
    CmdChangeDescription,
    CmdChangeHeader,
    CmdChangePreviousVersion,
    CmdBalanceDescription,
    CmdCompactDescription,
    CmdAuthDescription,
    CmdConstitutionDescription,
    CmdCostDescription,
    CmdDiffDescription,
    CmdEditDescription,
    CmdExitDescription,
    CmdExportDescription,
    CmdHfDescription,
    CmdHelpDescription,
    CmdProfileDescription,
    CmdHomeDescription,
    CmdHooksDescription,
    CmdAgentDescription,
    CmdInitDescription,
    CmdJobsDescription,
    CmdLinksDescription,
    CmdLoadDescription,
    CmdLogoutDescription,
    CmdMcpDescription,
    CmdMemoryDescription,
    CmdPluginDescription,
    CmdPluginNoneFound,
    CmdPluginNotFound,
    CmdPluginListHeader,
    CmdPluginDetailDescription,
    CmdPluginDetailSchema,
    CmdPluginDetailApproval,
    CmdPluginDetailPath,
    CmdModelDescription,
    CmdModelsDescription,
    CmdModelDbDescription,
    CmdNetworkDescription,
    CmdNoteDescription,
    CmdProviderDescription,
    CmdQueueDescription,
    CmdQueueUsage,
    CmdQueueDraftHeader,
    CmdQueueNoMessages,
    CmdQueueListHeader,
    CmdQueueTip,
    CmdQueueAlreadyEditing,
    CmdQueueNotFound,
    CmdQueueEditingStatus,
    CmdQueueEditingMessage,
    CmdQueueDropped,
    CmdQueueAlreadyEmpty,
    CmdQueueCleared,
    CmdQueueMissingIndex,
    CmdQueueIndexPositive,
    CmdQueueIndexMin,
    CmdRetryDescription,
    CmdSaveDescription,
    CmdForkDescription,
    CmdNewDescription,
    CmdSessionsDescription,
    CmdSettingsDescription,
    CmdSidebarDescription,
    CmdSkillDescription,
    CmdSkillsDescription,
    CmdFleetDescription,
    CmdSetupDescription,
    CmdSubagentsDescription,
    CmdTrustDescription,
    CmdShareDescription,
    CmdWorkspaceDescription,
    CmdVerboseDescription,
    CmdCostReport,
    // Canonical foreground slash-command presentation.
    CanonicalCommandRequired,
    CanonicalCommandUnknown,
    CanonicalCommandNoArguments,
    CanonicalCommandListTitle,
    CanonicalCommandAliases,
    FooterAgentSingular,
    FooterAgentsPlural,
    HeaderAgentsChip,
    FooterWorking,
    FooterBalancePrefix,
    FooterWorkspacePrefix,
    HelpSectionActions,
    HelpSectionClipboard,
    HelpSectionEditing,
    HelpSectionHelp,
    HelpSectionModes,
    HelpSectionNavigation,
    HelpSectionSessions,
    KbScrollTranscript,
    KbNavigateHistory,
    KbScrollTranscriptAlt,
    KbBrowseHistory,
    KbScrollPage,
    KbJumpTopBottom,
    KbJumpTopBottomEmpty,
    KbJumpToolBlocks,
    KbMoveCursor,
    KbJumpLineStartEnd,
    KbDeleteChar,
    KbClearDraft,
    KbSearchHistory,
    KbInsertNewline,
    KbSendDraft,
    KbCloseMenu,
    KbCancelOrExit,
    KbExitEmpty,
    KbCompleteCycleModes,
    KbCycleThinking,
    KbCyclePermissions,
    KbAltJumpPlanAgentYolo,
    KbFocusSidebar,
    KbCopySelection,
    KbContextMenu,
    KbHelpOverlay,
    KbToggleHelp,
    KbToggleHelpSlash,
    HelpUsageLabel,
    HelpAliasesLabel,
    SettingsTitle,
    SettingsConfigFile,
    ClearConversation,
    ClearConversationBusy,
    ModelChanged,
    LinksTitle,
    LinksDashboard,
    LinksDocs,
    LinksTip,
    SubagentsFetching,
    HelpUnknownCommand,
    HomeDashboardTitle,
    HomeModel,
    HomeMode,
    HomeWorkspace,
    HomeHistory,
    HomeTokens,
    HomeQueued,
    HomeSubagents,
    HomeSkill,
    HomeQuickActions,
    HomeQuickLinks,
    HomeQuickSkills,
    HomeQuickSettings,
    HomeQuickModel,
    HomeQuickSubagents,
    HomeQuickHelp,
    HomeModeTips,
    HomeAgentModeTip,
    HomeAgentModeReviewTip,
    HomeAgentModeYoloTip,
    HomeYoloModeTip,
    HomeYoloModeCaution,
    HomePlanModeTip,
    HomePlanModeChecklistTip,
    HomeOperateModeTip,
    HomeOperateModeFleetTip,
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
    OnboardApiKeyLocalHint,
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
    CanonicalUnknownBillingCreation,
    CanonicalAmbiguousPendingCreations,
    CanonicalAmbiguousPendingCreationsMore,
    CanonicalCancelAwaitTerminal,
    CanonicalWaitTerminalBeforeExit,
    CanonicalInterruptAccepted,
    CanonicalWaitTerminal,
    CanonicalNoTerminalRunToCompact,
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
    ApprovalBlockTitle,
    ApprovalControlsHint,
    ApprovalChooseHint,
    ApprovalChooseAction,
    ApprovalIntentLabel,
    ApprovalMoreLines,
    ApprovalEmptyContent,
    ApprovalMorePatchLines,
    ApprovalMoreFiles,
    ApprovalUnknownFile,
    // Voice commands (/voice, /voice-send, /voice-control)
    CmdVoiceDescription,
    CmdVoiceSendDescription,
    CmdVoiceControlDescription,
    VoiceEnabled,
    VoiceDisabled,
    VoiceSendEnabled,
    VoiceSendDisabled,
    VoiceControlEnabled,
    VoiceControlDisabled,
    VoiceErrNoAuth,
    VoiceErrNoRecorder,
    VoiceErrNetwork,
    VoiceErrEmptySend,
    VoiceErrTooShort,
    VoiceRecording,
    VoiceProcessing,
    VoiceTranscribed,
    // Footer chips.
    FooterWorkedChip,
    // Fleet setup wizard.
    FleetDraftTitle,
    FleetDraftHeader,
    FleetPreviewHeader,
    // Remote setup on-ramp.
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
    // Setup wizard — constitution file state.
    // Setup wizard — expert override state.
    // Setup wizard — autonomy fallback.
    // Setup wizard — purpose labels.
    // Setup wizard — purpose about descriptions.
    // Setup wizard — working style descriptions.
    // Setup wizard — evidence labels.
    // Setup wizard — guided answer notes.
    // Underwater shell phase words (footer status band).
    PhaseIdle,
    PhaseDraft,
    PhaseWorking,
    PhaseWaitingOnYou,
    PhaseDone,
    PhaseFailed,
    PhaseFinishing,
    // Underwater header chips: mode and permission words.
    ChipModeAct,
    ChipModePlan,
    ChipModeOperate,
    ChipPermissionReadOnly,
    ChipPermissionAsk,
    ChipPermissionAuto,
    ChipPermissionFullAccess,
    ChipPermissionNever,
    // Underwater footer right-hand hint words (keys stay literal in code).
    FooterHintKeys,
    FooterHintOutput,
    // Underwater post-launch empty state.
    EmptyStateMcpLabel,
    EmptyStateFleetLabel,
    EmptyStateFleetSetupLabel,
    // Session picker surface.
    SessionsSurfaceTitle,
    SessionsPaneTitle,
    SessionsHistoryPaneTitle,
    SessionsActionResume,
    SessionsActionSearch,
    SessionsActionSort,
    SessionsActionRename,
    SessionsActionAllWorkspaces,
    SessionsActionDelete,
    SessionsActionClose,
    SessionsScopeSortHeader,
    SessionsEmptyTitle,
    SessionsEmptyHint,
    SessionsShowingAllWorkspaces,
    SessionsScopedToWorkspace,
    SessionsNewTitlePrompt,
    SessionsDeletePrompt,
    SessionsConfirmDelete,
    SessionsNewSessionTitle,
    // Model picker route surface.
    RouteSurfaceTitle,
    RouteBrowseCatalog,
    RouteActionType,
    RouteActionSearchAnyModel,
    RoutePanelHeader,
    RouteProviderLabel,
    RouteModelFirstAtomic,
    // Fleet roster room.
    FleetRosterHeaderLabel,
    FleetRosterTabRoster,
    FleetRosterTabSetup,
    FleetRosterWorkers,
    FleetRosterMembersCount,
    FleetRosterOperatorFirst,
    FleetRosterOperatorRow,
    FleetReadyNotice,
    /// Sticky error when Fleet profile save cannot prove collision safety.
    FleetProfileIdentityVerifyFailed,
    /// Sticky error when the drafted profile id collides with another file.
    FleetProfileIdConflict,
    /// Sticky error when the drafted profile pins an unconfigured provider.
    FleetProfileProviderUnconfigured,
    // Sidebar work strip.
    SidebarTasksLabel,
    SidebarDestructiveArmed,
    // Composer slash menu.
    ComposerSlashMenuHint,
    // Approval modal — repository law band.
    ApprovalRepoLawBadge,
    ApprovalRepoLawTitle,
    ApprovalRepoLawWarning,
    ApprovalRepoLawRuleLabel,
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

#[allow(dead_code)]
pub const ALL_MESSAGE_IDS: &[MessageId] = &[
    MessageId::ComposerPlaceholder,
    MessageId::HistorySearchPlaceholder,
    MessageId::HistorySearchTitle,
    MessageId::HistoryHintMove,
    MessageId::HistoryHintAccept,
    MessageId::HistoryHintRestore,
    MessageId::HistoryNoMatches,
    MessageId::CommandPaletteTitle,
    MessageId::CommandPaletteSubtitle,
    MessageId::HelpTitle,
    MessageId::HelpSubtitle,
    MessageId::HelpFilterPlaceholder,
    MessageId::HelpFilterPrefix,
    MessageId::HelpNoMatches,
    MessageId::HelpSlashCommands,
    MessageId::HelpKeybindings,
    MessageId::HelpFooterTypeFilter,
    MessageId::HelpFooterMove,
    MessageId::HelpFooterJump,
    MessageId::HelpFooterClose,
    MessageId::CmdAnchorDescription,
    MessageId::CmdBalanceDescription,
    MessageId::CmdCompactDescription,
    MessageId::CmdAuthDescription,
    MessageId::CmdConstitutionDescription,
    MessageId::CmdCostDescription,
    MessageId::CmdDiffDescription,
    MessageId::CmdEditDescription,
    MessageId::CmdExitDescription,
    MessageId::CmdExportDescription,
    MessageId::CmdForkDescription,
    MessageId::CmdHfDescription,
    MessageId::CmdHelpDescription,
    MessageId::CmdProfileDescription,
    MessageId::CmdHomeDescription,
    MessageId::CmdHooksDescription,
    MessageId::CmdAgentDescription,
    MessageId::CmdInitDescription,
    MessageId::CmdJobsDescription,
    MessageId::CmdLinksDescription,
    MessageId::CmdLoadDescription,
    MessageId::CmdLogoutDescription,
    MessageId::CmdMcpDescription,
    MessageId::CmdPluginDescription,
    MessageId::CmdPluginNoneFound,
    MessageId::CmdPluginNotFound,
    MessageId::CmdPluginListHeader,
    MessageId::CmdPluginDetailDescription,
    MessageId::CmdPluginDetailSchema,
    MessageId::CmdPluginDetailApproval,
    MessageId::CmdPluginDetailPath,
    MessageId::CmdMemoryDescription,
    MessageId::CmdModelDescription,
    MessageId::CmdModelsDescription,
    MessageId::CmdModelDbDescription,
    MessageId::CmdNetworkDescription,
    MessageId::CmdNoteDescription,
    MessageId::CmdProviderDescription,
    MessageId::CmdQueueDescription,
    MessageId::CmdQueueUsage,
    MessageId::CmdQueueDraftHeader,
    MessageId::CmdQueueNoMessages,
    MessageId::CmdQueueListHeader,
    MessageId::CmdQueueTip,
    MessageId::CmdQueueAlreadyEditing,
    MessageId::CmdQueueNotFound,
    MessageId::CmdQueueEditingStatus,
    MessageId::CmdQueueEditingMessage,
    MessageId::CmdQueueDropped,
    MessageId::CmdQueueAlreadyEmpty,
    MessageId::CmdQueueCleared,
    MessageId::CmdQueueMissingIndex,
    MessageId::CmdQueueIndexPositive,
    MessageId::CmdQueueIndexMin,
    MessageId::CmdRetryDescription,
    MessageId::CmdSaveDescription,
    MessageId::CmdNewDescription,
    MessageId::CmdSessionsDescription,
    MessageId::CmdSettingsDescription,
    MessageId::CmdSidebarDescription,
    MessageId::CmdSkillDescription,
    MessageId::CmdSkillsDescription,
    MessageId::CmdFleetDescription,
    MessageId::CmdSetupDescription,
    MessageId::CmdSubagentsDescription,
    MessageId::CmdTrustDescription,
    MessageId::CmdShareDescription,
    MessageId::CmdWorkspaceDescription,
    MessageId::CmdVerboseDescription,
    MessageId::CmdChangeDescription,
    MessageId::CmdChangeHeader,
    MessageId::CmdChangePreviousVersion,
    MessageId::CmdCostReport,
    MessageId::CanonicalCommandRequired,
    MessageId::CanonicalCommandUnknown,
    MessageId::CanonicalCommandNoArguments,
    MessageId::CanonicalCommandListTitle,
    MessageId::CanonicalCommandAliases,
    MessageId::FooterAgentSingular,
    MessageId::FooterAgentsPlural,
    MessageId::HeaderAgentsChip,
    MessageId::FooterWorking,
    MessageId::FooterBalancePrefix,
    MessageId::FooterWorkspacePrefix,
    MessageId::HelpSectionActions,
    MessageId::HelpSectionClipboard,
    MessageId::HelpSectionEditing,
    MessageId::HelpSectionHelp,
    MessageId::HelpSectionModes,
    MessageId::HelpSectionNavigation,
    MessageId::HelpSectionSessions,
    MessageId::KbScrollTranscript,
    MessageId::KbNavigateHistory,
    MessageId::KbScrollTranscriptAlt,
    MessageId::KbBrowseHistory,
    MessageId::KbScrollPage,
    MessageId::KbJumpTopBottom,
    MessageId::KbJumpTopBottomEmpty,
    MessageId::KbJumpToolBlocks,
    MessageId::KbMoveCursor,
    MessageId::KbJumpLineStartEnd,
    MessageId::KbDeleteChar,
    MessageId::KbClearDraft,
    MessageId::KbSearchHistory,
    MessageId::KbInsertNewline,
    MessageId::KbSendDraft,
    MessageId::KbCloseMenu,
    MessageId::KbCancelOrExit,
    MessageId::KbExitEmpty,
    MessageId::KbCompleteCycleModes,
    MessageId::KbCycleThinking,
    MessageId::KbCyclePermissions,
    MessageId::KbAltJumpPlanAgentYolo,
    MessageId::KbFocusSidebar,
    MessageId::KbCopySelection,
    MessageId::KbContextMenu,
    MessageId::KbHelpOverlay,
    MessageId::KbToggleHelp,
    MessageId::KbToggleHelpSlash,
    MessageId::HelpUsageLabel,
    MessageId::HelpAliasesLabel,
    MessageId::SettingsTitle,
    MessageId::SettingsConfigFile,
    MessageId::ClearConversation,
    MessageId::ClearConversationBusy,
    MessageId::ModelChanged,
    MessageId::LinksTitle,
    MessageId::LinksDashboard,
    MessageId::LinksDocs,
    MessageId::LinksTip,
    MessageId::SubagentsFetching,
    MessageId::HelpUnknownCommand,
    MessageId::HomeDashboardTitle,
    MessageId::HomeModel,
    MessageId::HomeMode,
    MessageId::HomeWorkspace,
    MessageId::HomeHistory,
    MessageId::HomeTokens,
    MessageId::HomeQueued,
    MessageId::HomeSubagents,
    MessageId::HomeSkill,
    MessageId::HomeQuickActions,
    MessageId::HomeQuickLinks,
    MessageId::HomeQuickSkills,
    MessageId::HomeQuickSettings,
    MessageId::HomeQuickModel,
    MessageId::HomeQuickSubagents,
    MessageId::HomeQuickHelp,
    MessageId::HomeModeTips,
    MessageId::HomeAgentModeTip,
    MessageId::HomeAgentModeReviewTip,
    MessageId::HomeAgentModeYoloTip,
    MessageId::HomeYoloModeTip,
    MessageId::HomeYoloModeCaution,
    MessageId::HomePlanModeTip,
    MessageId::HomePlanModeChecklistTip,
    MessageId::HomeOperateModeTip,
    MessageId::HomeOperateModeFleetTip,
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
    MessageId::OnboardApiKeyLocalHint,
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
    MessageId::CanonicalUnknownBillingCreation,
    MessageId::CanonicalAmbiguousPendingCreations,
    MessageId::CanonicalAmbiguousPendingCreationsMore,
    MessageId::CanonicalCancelAwaitTerminal,
    MessageId::CanonicalWaitTerminalBeforeExit,
    MessageId::CanonicalInterruptAccepted,
    MessageId::CanonicalWaitTerminal,
    MessageId::CanonicalNoTerminalRunToCompact,
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
    MessageId::ApprovalBlockTitle,
    MessageId::ApprovalControlsHint,
    MessageId::ApprovalChooseHint,
    MessageId::ApprovalChooseAction,
    MessageId::ApprovalIntentLabel,
    MessageId::ApprovalMoreLines,
    MessageId::ApprovalEmptyContent,
    MessageId::ApprovalMorePatchLines,
    MessageId::ApprovalMoreFiles,
    MessageId::ApprovalUnknownFile,
    MessageId::CmdVoiceDescription,
    MessageId::CmdVoiceSendDescription,
    MessageId::CmdVoiceControlDescription,
    MessageId::VoiceEnabled,
    MessageId::VoiceDisabled,
    MessageId::VoiceSendEnabled,
    MessageId::VoiceSendDisabled,
    MessageId::VoiceControlEnabled,
    MessageId::VoiceControlDisabled,
    MessageId::VoiceErrNoAuth,
    MessageId::VoiceErrNoRecorder,
    MessageId::VoiceErrNetwork,
    MessageId::VoiceErrEmptySend,
    MessageId::VoiceErrTooShort,
    MessageId::VoiceRecording,
    MessageId::VoiceProcessing,
    MessageId::VoiceTranscribed,
    MessageId::FooterWorkedChip,
    MessageId::FleetDraftTitle,
    MessageId::FleetDraftHeader,
    MessageId::FleetPreviewHeader,
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
    MessageId::PhaseFinishing,
    MessageId::ChipModeAct,
    MessageId::ChipModePlan,
    MessageId::ChipModeOperate,
    MessageId::ChipPermissionReadOnly,
    MessageId::ChipPermissionAsk,
    MessageId::ChipPermissionAuto,
    MessageId::ChipPermissionFullAccess,
    MessageId::ChipPermissionNever,
    MessageId::FooterHintKeys,
    MessageId::FooterHintOutput,
    MessageId::EmptyStateMcpLabel,
    MessageId::EmptyStateFleetLabel,
    MessageId::EmptyStateFleetSetupLabel,
    MessageId::SessionsSurfaceTitle,
    MessageId::SessionsPaneTitle,
    MessageId::SessionsHistoryPaneTitle,
    MessageId::SessionsActionResume,
    MessageId::SessionsActionSearch,
    MessageId::SessionsActionSort,
    MessageId::SessionsActionRename,
    MessageId::SessionsActionAllWorkspaces,
    MessageId::SessionsActionDelete,
    MessageId::SessionsActionClose,
    MessageId::SessionsScopeSortHeader,
    MessageId::SessionsEmptyTitle,
    MessageId::SessionsEmptyHint,
    MessageId::SessionsShowingAllWorkspaces,
    MessageId::SessionsScopedToWorkspace,
    MessageId::SessionsNewTitlePrompt,
    MessageId::SessionsDeletePrompt,
    MessageId::SessionsConfirmDelete,
    MessageId::SessionsNewSessionTitle,
    MessageId::RouteSurfaceTitle,
    MessageId::RouteBrowseCatalog,
    MessageId::RouteActionType,
    MessageId::RouteActionSearchAnyModel,
    MessageId::RoutePanelHeader,
    MessageId::RouteProviderLabel,
    MessageId::RouteModelFirstAtomic,
    MessageId::FleetRosterHeaderLabel,
    MessageId::FleetRosterTabRoster,
    MessageId::FleetRosterTabSetup,
    MessageId::FleetRosterWorkers,
    MessageId::FleetRosterMembersCount,
    MessageId::FleetRosterOperatorFirst,
    MessageId::FleetRosterOperatorRow,
    MessageId::FleetReadyNotice,
    MessageId::FleetProfileIdentityVerifyFailed,
    MessageId::FleetProfileIdConflict,
    MessageId::FleetProfileProviderUnconfigured,
    MessageId::SidebarTasksLabel,
    MessageId::SidebarDestructiveArmed,
    MessageId::ComposerSlashMenuHint,
    MessageId::ApprovalRepoLawBadge,
    MessageId::ApprovalRepoLawTitle,
    MessageId::ApprovalRepoLawWarning,
    MessageId::ApprovalRepoLawRuleLabel,
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

pub fn tr(id: MessageId) -> Cow<'static, str> {
    rust_i18n::t!(format!("{id:?}"), locale = "zh-Hans")
}

#[allow(dead_code)]
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_string();
    }

    let ellipsis_width = '…'.width().unwrap_or(1);
    if max_width <= ellipsis_width {
        return "…".to_string();
    }

    let limit = max_width - ellipsis_width;
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget, Wrap},
    };

    pub fn missing_message_ids() -> Vec<MessageId> {
        ALL_MESSAGE_IDS
            .iter()
            .copied()
            .filter(|id| tr(*id).eq(&format!("{id:?}")))
            .collect()
    }

    fn message_source() -> &'static str {
        include_str!("../locales/zh-Hans.json")
    }

    #[test]
    fn message_pack_has_no_missing_core_messages() {
        assert!(missing_message_ids().is_empty());
    }

    fn raw_message_keys() -> std::collections::BTreeSet<String> {
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(message_source())
            .expect("zh-Hans message catalog should parse")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn message_id_list_and_catalog_stay_in_exact_sync() {
        let catalog = raw_message_keys();
        let ids: std::collections::BTreeSet<String> =
            ALL_MESSAGE_IDS.iter().map(|id| format!("{id:?}")).collect();
        assert_eq!(
            ids.len(),
            ALL_MESSAGE_IDS.len(),
            "ALL_MESSAGE_IDS contains duplicates"
        );
        let unlisted: Vec<_> = catalog.difference(&ids).collect();
        assert!(
            unlisted.is_empty(),
            "zh-Hans keys absent from ALL_MESSAGE_IDS: {unlisted:?}"
        );
        let missing: Vec<_> = ids.difference(&catalog).collect();
        assert!(
            missing.is_empty(),
            "ALL_MESSAGE_IDS entries without a zh-Hans string: {missing:?}"
        );
    }

    #[test]
    fn setup_strings_are_explicitly_present() {
        let setup_keys = ALL_MESSAGE_IDS
            .iter()
            .map(|id| format!("{id:?}"))
            .filter(|id| id.starts_with("Setup"))
            .collect::<Vec<_>>();

        let messages =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(message_source())
                .expect("zh-Hans message catalog");
        for key in setup_keys {
            assert!(messages.contains_key(&key), "zh-Hans should define {key}");
        }
    }

    #[test]
    fn provider_description_is_present() {
        let description = tr(MessageId::CmdProviderDescription);
        assert!(!description.is_empty());
        assert!(!description.contains("codewhale |"));
    }

    #[test]
    fn width_truncation_handles_cjk_rtl_indic_and_latin_samples() {
        let samples = [
            ("zh-Hans", "输入以筛选配置"),
            ("ar", "تصفية الإعدادات"),
            ("hi", "सेटिंग खोजें"),
            ("pt-BR", "configurações filtradas"),
        ];

        for (tag, sample) in samples {
            let truncated = truncate_to_width(sample, 12);
            assert!(
                truncated.width() <= 12,
                "{tag} sample overflowed: {truncated:?}"
            );
        }
    }

    #[test]
    fn planned_script_samples_render_in_narrow_terminal_buffer() {
        let samples = [
            ("CJK", "输入以筛选配置"),
            ("RTL", "تصفية الإعدادات"),
            ("Indic", "सेटिंग खोजें"),
            ("Latin Global South", "configurações filtradas"),
        ];

        for (label, sample) in samples {
            let area = Rect::new(0, 0, 18, 4);
            let mut buf = Buffer::empty(area);
            Paragraph::new(sample)
                .wrap(Wrap { trim: false })
                .render(area, &mut buf);
            let dump = buffer_text(&buf, area);

            assert!(
                dump.chars().any(|ch| !ch.is_whitespace()),
                "{label} sample produced an empty render"
            );
        }
    }

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn visible_row_text(buf: &Buffer, area: Rect, y: u16) -> String {
        let mut out = String::new();
        let mut skip_cells = 0usize;
        for x in area.left()..area.right() {
            if skip_cells > 0 {
                skip_cells -= 1;
                continue;
            }
            let symbol = buf[(x, y)].symbol();
            out.push_str(symbol);
            skip_cells = UnicodeWidthStr::width(symbol).saturating_sub(1);
        }
        out
    }

    // --- Unicode / CJK / terminal-width QA (issue #3488) -------------------
    // `truncate_to_width` is the localization-layer truncation helper. These
    // verify it clips by display width (never byte/char count), preserves
    // semantic prefixes, never splits a grapheme cluster, and that mixed
    // English/CJK rows wrap inside a narrow (40-col) and medium (80-col)
    // terminal buffer without overflowing the column.

    #[test]
    fn truncate_to_width_clips_cjk_by_display_width_and_keeps_prefix_intact() {
        // Each Han glyph is two columns. A 12-column budget fits the six-glyph
        // title exactly, so no truncation/ellipsis happens and the prefix survives.
        let title = "项目报告结果"; // 12 columns
        assert_eq!(truncate_to_width(title, 12), title);

        // Oversized: clip on a whole-glyph boundary, append the ellipsis, and
        // stay within the budget by display width.
        let out = truncate_to_width("数据库迁移任务结果", 7); // 10 glyphs = 20 cols
        assert!(
            UnicodeWidthStr::width(out.as_str()) <= 7,
            "{out:?} overflowed"
        );
        assert!(out.ends_with('…'), "expected ellipsis, got {out:?}");
        assert!(!out.contains('\u{FFFD}'), "split a wide glyph: {out:?}");
        // The kept body is whole wide glyphs (each two columns) — never a half cell.
        let body = out.strip_suffix('…').unwrap_or(&out);
        assert!(
            body.chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                .sum::<usize>()
                <= 6,
            "body exceeded budget-minus-ellipsis: {out:?}"
        );

        // A semantic ASCII prefix (e.g. a status verb) survives when it fits.
        let row = "running 数据库迁移任务结果预览测试";
        let out = truncate_to_width(row, 16);
        assert!(
            out.starts_with("running"),
            "semantic prefix dropped: {out:?}"
        );
        assert!(UnicodeWidthStr::width(out.as_str()) <= 16);
        assert!(!out.contains('\u{FFFD}'));
    }

    #[test]
    fn truncate_to_width_never_splits_combining_marks_or_emoji() {
        // Combining mark (U+0301) and ZWJ are zero-width; they must not be
        // counted as columns and must never be cut mid-cluster into U+FFFD.
        let cafe = "cafe\u{0301}"; // "café", 4 columns
        assert_eq!(truncate_to_width(cafe, 10), cafe);
        let out = truncate_to_width("cafe\u{0301} overflow here", 6);
        assert!(UnicodeWidthStr::width(out.as_str()) <= 6);
        assert!(!out.contains('\u{FFFD}'));

        // Emoji is two columns; truncation lands on a cluster boundary.
        let out = truncate_to_width("\u{1F433}\u{1F433}\u{1F433} whales everywhere", 5);
        assert!(UnicodeWidthStr::width(out.as_str()) <= 5);
        assert!(!out.contains('\u{FFFD}'));
    }

    #[test]
    fn narrow_and_medium_terminal_wraps_mixed_width_rows_without_overflow() {
        // Issue #3488 acceptance: at a 40-col (narrow, macOS-Terminal-like) and
        // 80-col (medium) terminal, mixed English/CJK task titles and transcript
        // lines must (a) truncate to the column by display width, and (b) wrap
        // inside the buffer so no rendered row exceeds the terminal width.
        let fixtures = [
            "Task: 数据库迁移任务 — verify provider routing for issue #3488",
            "抹香鲸 is running codex/issue-3439-zhipu-glm-fixture @ issue-3439",
            "满員電車🫠 — full-width punctuation：『』【】 mixes with ASCII ids",
        ];

        for width in [40usize, 80] {
            // (a) The truncation helper clips by display width.
            for fixture in fixtures {
                let out = truncate_to_width(fixture, width);
                assert!(
                    UnicodeWidthStr::width(out.as_str()) <= width,
                    "width={width}: truncated row overflowed: {out:?}"
                );
                assert!(
                    !out.contains('\u{FFFD}'),
                    "width={width}: split a glyph: {out:?}"
                );
            }

            // (b) Wrapping the full mixed-width line inside a buffer of `width`
            // columns never lets a rendered row exceed the terminal width.
            for fixture in fixtures {
                let area = Rect::new(0, 0, width as u16, 6);
                let mut buf = Buffer::empty(area);
                Paragraph::new(fixture)
                    .wrap(Wrap { trim: false })
                    .render(area, &mut buf);
                let mut saw_text = false;
                for (row_idx, y) in (area.top()..area.bottom()).enumerate() {
                    let row = visible_row_text(&buf, area, y);
                    let trimmed = row.trim_end_matches('\u{0}').trim_end();
                    assert!(
                        UnicodeWidthStr::width(trimmed) <= width,
                        "width={width} row {row_idx}: wrapped row overflowed ({} cols): {trimmed:?}",
                        UnicodeWidthStr::width(trimmed)
                    );
                    saw_text |= trimmed.chars().any(|ch| !ch.is_whitespace());
                }
                assert!(
                    saw_text,
                    "width={width}: mixed fixture produced an empty render"
                );
            }
        }
    }
}
