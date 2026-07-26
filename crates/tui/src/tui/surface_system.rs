//! Canonical topology for every production-reachable TUI surface.
//!
//! This module owns presentation placement only. It deliberately carries no
//! Run, permission, evidence, focus, or persistence state. Those facts remain
//! with their canonical owners and are merely projected into one of the four
//! containers accepted by ADR-0013.

use super::{app::OnboardingState, views::SecondarySurfaceKind};

/// The complete container grammar accepted by ADR-0013.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceContainer {
    MainWorkSurface,
    BottomSheet,
    FullScreenRoom,
    InlineApproval,
}

/// Every distinct production entry point from first start through reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProductionSurface {
    Main,
    OnboardingWelcome,
    OnboardingApiKey,
    OnboardingTrust,
    OnboardingTips,
    SlashMenu,
    MentionMenu,
    PermissionSelector,
    UserInput,
    Approval,
    Pager,
}

/// Auditable presentation contract for one reachable surface.
///
/// The string fields name code owners and actions, not user-facing copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceContract {
    pub surface: ProductionSurface,
    pub opener: &'static str,
    pub state_source: &'static str,
    pub targets: &'static [SurfaceContainer],
    pub exit_action: &'static str,
    pub legacy_deletion_point: &'static str,
}

const MAIN: &[SurfaceContainer] = &[SurfaceContainer::MainWorkSurface];
const SHEET: &[SurfaceContainer] = &[SurfaceContainer::BottomSheet];
const ROOM: &[SurfaceContainer] = &[SurfaceContainer::FullScreenRoom];
const INLINE: &[SurfaceContainer] = &[SurfaceContainer::InlineApproval];
const ADAPTIVE_INPUT: &[SurfaceContainer] = &[
    SurfaceContainer::BottomSheet,
    SurfaceContainer::FullScreenRoom,
];

const CONTRACTS: &[SurfaceContract] = &[
    SurfaceContract {
        surface: ProductionSurface::Main,
        opener: "canonical event loop after onboarding",
        state_source: "CanonicalRunPresentation + App composer/viewport",
        targets: MAIN,
        exit_action: "canonical Ctrl-C/Ctrl-D or /exit",
        legacy_deletion_point: "deleted in M28-B",
    },
    SurfaceContract {
        surface: ProductionSurface::OnboardingWelcome,
        opener: "initial_onboarding_state",
        state_source: "OnboardingState::Welcome",
        targets: ROOM,
        exit_action: "Enter advances; canonical exit cancels",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::OnboardingApiKey,
        opener: "welcome advance when credential is absent",
        state_source: "OnboardingState::ApiKey + masked App input",
        targets: ROOM,
        exit_action: "Enter validates/saves; Esc returns",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::OnboardingTrust,
        opener: "workspace trust gate",
        state_source: "OnboardingState::TrustDirectory",
        targets: ROOM,
        exit_action: "explicit trust continues; deny/Esc exits",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::OnboardingTips,
        opener: "last required onboarding step",
        state_source: "OnboardingState::Tips",
        targets: ROOM,
        exit_action: "Enter completes onboarding",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::SlashMenu,
        opener: "composer input begins with /",
        state_source: "canonical command registry + composer selection",
        targets: MAIN,
        exit_action: "Enter dispatches; Esc closes",
        legacy_deletion_point: "none; retain composer-attached menu",
    },
    SurfaceContract {
        surface: ProductionSurface::MentionMenu,
        opener: "composer contains an active @ query",
        state_source: "deterministic workspace mention completion",
        targets: MAIN,
        exit_action: "Enter inserts; Esc closes",
        legacy_deletion_point: "none; retain composer-attached menu",
    },
    SurfaceContract {
        surface: ProductionSurface::PermissionSelector,
        opener: "/permissions or permission chip",
        state_source: "RunPermissionMode + active-run freeze fact",
        targets: SHEET,
        exit_action: "Enter selects for next Run; Esc closes",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::UserInput,
        opener: "canonical pending request_user_input interaction",
        state_source: "UserInteractionRequest + local bounded response draft",
        targets: ADAPTIVE_INPUT,
        exit_action: "Enter submits; Esc cancels",
        legacy_deletion_point: "deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::Approval,
        opener: "canonical pending approval interaction",
        state_source: "ApprovalRequest + Host risk/authority facts",
        targets: INLINE,
        exit_action: "approve/deny/abort decision",
        legacy_deletion_point: "detached path deleted in M28-D",
    },
    SurfaceContract {
        surface: ProductionSurface::Pager,
        opener: "/help, /cost, or canonical detail event",
        state_source: "immutable text/log/diff/evidence projection",
        targets: ROOM,
        exit_action: "Esc/q closes and restores main surface",
        legacy_deletion_point: "deleted in M28-D",
    },
];

pub(crate) fn contracts() -> &'static [SurfaceContract] {
    CONTRACTS
}

fn contract(surface: ProductionSurface) -> &'static SurfaceContract {
    contracts()
        .iter()
        .find(|contract| contract.surface == surface)
        .expect("every production surface has one topology contract")
}

pub(crate) fn for_secondary_surface(kind: SecondarySurfaceKind) -> &'static SurfaceContract {
    contract(match kind {
        SecondarySurfaceKind::Approval => ProductionSurface::Approval,
        SecondarySurfaceKind::Permission => ProductionSurface::PermissionSelector,
        SecondarySurfaceKind::UserInput => ProductionSurface::UserInput,
        SecondarySurfaceKind::Pager => ProductionSurface::Pager,
    })
}

pub(crate) fn for_onboarding(state: OnboardingState) -> Option<&'static SurfaceContract> {
    let surface = match state {
        OnboardingState::Welcome => ProductionSurface::OnboardingWelcome,
        OnboardingState::ApiKey => ProductionSurface::OnboardingApiKey,
        OnboardingState::TrustDirectory => ProductionSurface::OnboardingTrust,
        OnboardingState::Tips => ProductionSurface::OnboardingTips,
        OnboardingState::None => return None,
    };
    Some(contract(surface))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_reachable_surface_has_one_complete_contract() {
        let expected = [
            ProductionSurface::Main,
            ProductionSurface::OnboardingWelcome,
            ProductionSurface::OnboardingApiKey,
            ProductionSurface::OnboardingTrust,
            ProductionSurface::OnboardingTips,
            ProductionSurface::SlashMenu,
            ProductionSurface::MentionMenu,
            ProductionSurface::PermissionSelector,
            ProductionSurface::UserInput,
            ProductionSurface::Approval,
            ProductionSurface::Pager,
        ];
        let actual = contracts()
            .iter()
            .map(|contract| contract.surface)
            .collect::<HashSet<_>>();

        assert_eq!(actual, expected.into_iter().collect());
        assert_eq!(
            actual.len(),
            contracts().len(),
            "duplicate surface contract"
        );
        for contract in contracts() {
            assert!(!contract.opener.trim().is_empty());
            assert!(!contract.state_source.trim().is_empty());
            assert!(!contract.targets.is_empty());
            assert!(!contract.exit_action.trim().is_empty());
            assert!(!contract.legacy_deletion_point.trim().is_empty());
        }
    }

    #[test]
    fn secondary_surface_and_onboarding_variants_are_exhaustively_reachable() {
        assert_eq!(
            for_secondary_surface(SecondarySurfaceKind::Approval).surface,
            ProductionSurface::Approval
        );
        assert_eq!(
            for_secondary_surface(SecondarySurfaceKind::Permission).surface,
            ProductionSurface::PermissionSelector
        );
        assert_eq!(
            for_secondary_surface(SecondarySurfaceKind::UserInput).surface,
            ProductionSurface::UserInput
        );
        assert_eq!(
            for_secondary_surface(SecondarySurfaceKind::Pager).surface,
            ProductionSurface::Pager
        );

        assert_eq!(
            for_onboarding(OnboardingState::Welcome).map(|contract| contract.surface),
            Some(ProductionSurface::OnboardingWelcome)
        );
        assert_eq!(
            for_onboarding(OnboardingState::ApiKey).map(|contract| contract.surface),
            Some(ProductionSurface::OnboardingApiKey)
        );
        assert_eq!(
            for_onboarding(OnboardingState::TrustDirectory).map(|contract| contract.surface),
            Some(ProductionSurface::OnboardingTrust)
        );
        assert_eq!(
            for_onboarding(OnboardingState::Tips).map(|contract| contract.surface),
            Some(ProductionSurface::OnboardingTips)
        );
        assert_eq!(for_onboarding(OnboardingState::None), None);
    }

    #[test]
    fn target_contract_bans_legacy_container_grammar() {
        let accepted = contracts()
            .iter()
            .flat_map(|contract| contract.targets.iter().copied())
            .collect::<HashSet<_>>();
        assert_eq!(
            accepted,
            [
                SurfaceContainer::MainWorkSurface,
                SurfaceContainer::BottomSheet,
                SurfaceContainer::FullScreenRoom,
                SurfaceContainer::InlineApproval,
            ]
            .into_iter()
            .collect()
        );
        for contract in contracts() {
            let target = format!("{:?}", contract.targets).to_ascii_lowercase();
            for retired in ["underwater", "ocean", "centered", "card", "genericmodal"] {
                assert!(
                    !target.contains(retired),
                    "legacy container admitted by {:?}: {retired}",
                    contract.surface
                );
            }
        }
    }
}
