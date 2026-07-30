use crate::tui::app::App;
use dse_localization::MessageId;
use dse_protocol::agent_runtime::RunPermissionMode;

use crate::tui::run_presentation::{RunPresentationPhase, VerificationPresentation};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum WorkSurfaceLayout {
    #[default]
    TopStrip,
    RightRail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkTone {
    Heading,
    Active,
    Attention,
    Success,
    Muted,
    Worker,
}

#[derive(Debug, Clone)]
pub(super) struct WorkRow {
    pub id: String,
    pub mark: &'static str,
    pub label: String,
    pub tone: WorkTone,
}

/// Read-only projection state. Runtime and RunStore remain the owners of all
/// facts displayed here.
#[derive(Debug, Clone)]
pub struct WorkSurfaceState {
    pub(super) layout: WorkSurfaceLayout,
    pub(super) latest_rows: Vec<WorkRow>,
}

impl Default for WorkSurfaceState {
    fn default() -> Self {
        Self {
            layout: WorkSurfaceLayout::TopStrip,
            latest_rows: Vec::new(),
        }
    }
}

pub(super) fn project(app: &mut App) -> Vec<WorkRow> {
    let live = super::live_projection::LiveWorkProjection::from_app(app);
    let mut rows = Vec::new();
    if app.run_presentation.has_root() {
        let heading = app
            .run_presentation
            .objective()
            .map(objective_summary)
            .filter(|objective| !objective.is_empty())
            .map_or_else(
                || app.tr(MessageId::SidebarTasksLabel).into_owned(),
                |objective| {
                    app.tr(MessageId::WorkRunHeading)
                        .replace("{objective}", &objective)
                },
            );
        rows.push(section("task", &heading));
        rows.extend(root_rows(app, &live));
    }
    if !live.rows.is_empty() {
        rows.extend(live.rows.iter().map(live_row));
    }
    app.work_surface.latest_rows = rows.clone();
    rows
}

fn objective_summary(objective: &str) -> String {
    objective.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn root_rows(app: &App, live: &super::live_projection::LiveWorkProjection) -> Vec<WorkRow> {
    let phase = app.run_presentation.phase();
    let status_tone = if phase.is_success() {
        WorkTone::Success
    } else if phase.needs_attention() {
        WorkTone::Attention
    } else if phase.is_active() {
        WorkTone::Active
    } else {
        WorkTone::Muted
    };
    let status_mark = match phase {
        RunPresentationPhase::Completed => "✓",
        RunPresentationPhase::HostAccepted => "○",
        RunPresentationPhase::WaitingForUser => "◆",
        phase if phase.needs_attention() => "✕",
        phase if phase.is_active() => "●",
        _ => "○",
    };
    let status = app
        .tr(MessageId::WorkStatusRow)
        .replace("{status}", &phase.localized_label(app.language));

    let changed_file_count = app.run_presentation.confirmed_changed_file_count();
    let changes = if changed_file_count > 0 {
        app.tr(MessageId::WorkChangesFiles)
            .replace("{count}", &changed_file_count.to_string())
    } else if app.run_presentation.workspace_change_confirmed() {
        app.tr(MessageId::WorkChangesConfirmed).into_owned()
    } else {
        app.tr(MessageId::WorkChangesNone).into_owned()
    };
    let change_tone = if app.run_presentation.workspace_change_confirmed() {
        WorkTone::Success
    } else {
        WorkTone::Muted
    };

    let verification = app.run_presentation.verification();
    let verification_text = verification.localized_label(
        app.language,
        app.run_presentation.satisfied_acceptance_count(),
        app.run_presentation.acceptance_total(),
    );
    let verification_tone = match verification {
        VerificationPresentation::Passed => WorkTone::Success,
        VerificationPresentation::Failed => WorkTone::Attention,
        VerificationPresentation::Preparing | VerificationPresentation::Running => WorkTone::Active,
        VerificationPresentation::NotStarted => WorkTone::Muted,
    };
    let verification_mark = match verification {
        VerificationPresentation::Passed => "✓",
        VerificationPresentation::Failed => "✕",
        VerificationPresentation::Preparing | VerificationPresentation::Running => "●",
        VerificationPresentation::NotStarted => "○",
    };

    let permission = match app
        .run_presentation
        .permission_mode()
        .unwrap_or(app.permission_mode)
    {
        RunPermissionMode::Ask => app.tr(MessageId::ChipPermissionAsk).into_owned(),
        RunPermissionMode::Agent => app.tr(MessageId::ChipPermissionAgent).into_owned(),
        RunPermissionMode::FullAccess => app.tr(MessageId::ChipPermissionFullAccess).into_owned(),
    };

    vec![
        task_row("status", status_mark, status, status_tone),
        task_row(
            "changes",
            "Δ",
            app.tr(MessageId::WorkChangesRow)
                .replace("{changes}", &changes),
            change_tone,
        ),
        task_row(
            "verification",
            verification_mark,
            app.tr(MessageId::WorkVerificationRow)
                .replace("{verification}", &verification_text),
            verification_tone,
        ),
        task_row(
            "agents",
            "◇",
            app.tr(MessageId::WorkAgentsRow)
                .replace("{active}", &live.active.to_string())
                .replace("{total}", &live.rows.len().to_string()),
            if live.active > 0 {
                WorkTone::Worker
            } else {
                WorkTone::Muted
            },
        ),
        task_row(
            "permission",
            "◆",
            app.tr(MessageId::WorkPermissionRow)
                .replace("{permission}", &permission),
            WorkTone::Muted,
        ),
        task_row(
            "run-store",
            "↻",
            app.tr(MessageId::WorkRunStoreRecoverable).into_owned(),
            WorkTone::Muted,
        ),
    ]
}

fn task_row(id: &str, mark: &'static str, label: String, tone: WorkTone) -> WorkRow {
    WorkRow {
        id: format!("task:{id}"),
        mark,
        label,
        tone,
    }
}

fn section(id: &str, label: &str) -> WorkRow {
    WorkRow {
        id: format!("section:{id}"),
        mark: "▾",
        label: label.to_string(),
        tone: WorkTone::Heading,
    }
}

fn live_row(row: &super::live_projection::LiveWorkRow) -> WorkRow {
    let (mark, tone) = match row.state {
        super::live_projection::LiveWorkState::Active => ("›", WorkTone::Worker),
        super::live_projection::LiveWorkState::Settled => match row.status.as_str() {
            "done" => ("✓", WorkTone::Success),
            "failed" | "canceled" | "interrupted" | "blocked" | "recovery" => {
                ("✕", WorkTone::Attention)
            }
            _ => ("☐", WorkTone::Muted),
        },
    };
    WorkRow {
        id: row.identity.clone(),
        mark,
        label: row.label.clone(),
        tone,
    }
}
