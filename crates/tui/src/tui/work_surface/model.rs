use crate::tui::app::App;

/// Persisted work-surface placement. Bottom is deliberately absent: the
/// composer and phase footer own the shell's lower edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkSurfacePlacement {
    #[default]
    Top,
    Left,
    Right,
}

impl WorkSurfacePlacement {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Top,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkTone {
    Heading,
    Live,
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
    pub placement: WorkSurfacePlacement,
    pub(super) effective_placement: WorkSurfacePlacement,
    pub(super) latest_rows: Vec<WorkRow>,
}

impl Default for WorkSurfaceState {
    fn default() -> Self {
        Self::with_placement(WorkSurfacePlacement::Top)
    }
}

impl WorkSurfaceState {
    #[must_use]
    pub fn with_placement(placement: WorkSurfacePlacement) -> Self {
        Self {
            placement,
            effective_placement: placement,
            latest_rows: Vec::new(),
        }
    }
}

pub(super) fn project(app: &mut App) -> Vec<WorkRow> {
    let live = super::live_projection::LiveWorkProjection::from_app(app);
    let attention_hold = live
        .rows
        .iter()
        .any(|row| row.state == super::live_projection::LiveWorkState::Waiting);
    let mut rows = Vec::new();
    if live.counts.active > 0 || !live.rows.is_empty() {
        rows.push(section(
            "active",
            &format!(
                "Active {} · Tasks {} · Runs {} · Workers {}",
                live.counts.active, live.counts.tasks, live.counts.runs, live.counts.workers
            ),
            live.counts.active,
        ));
        rows.extend(live.rows.iter().map(|row| live_row(row, attention_hold)));
    }
    app.work_surface.latest_rows = rows.clone();
    rows
}

fn section(id: &str, label: &str, count: usize) -> WorkRow {
    WorkRow {
        id: format!("section:{id}"),
        mark: "▾",
        label: if label.chars().any(char::is_numeric) {
            label.to_string()
        } else {
            format!("{label} {count}")
        },
        tone: WorkTone::Heading,
    }
}

fn live_row(row: &super::live_projection::LiveWorkRow, attention_hold: bool) -> WorkRow {
    let (mark, tone) = match row.state {
        super::live_projection::LiveWorkState::Active if attention_hold => ("·", WorkTone::Muted),
        _ => match row.state {
            super::live_projection::LiveWorkState::Active => (
                "›",
                if row.kind == super::live_projection::LiveWorkKind::Worker {
                    WorkTone::Worker
                } else {
                    WorkTone::Live
                },
            ),
            super::live_projection::LiveWorkState::Waiting => ("◆", WorkTone::Attention),
            super::live_projection::LiveWorkState::Settled => match row.status.as_str() {
                "completed" | "success" | "done" => ("✓", WorkTone::Success),
                "failed" | "canceled" | "cancelled" | "interrupted" => ("✕", WorkTone::Attention),
                _ => ("☐", WorkTone::Muted),
            },
        },
    };
    WorkRow {
        id: row.identity.clone(),
        mark,
        label: row.label.clone(),
        tone,
    }
}
