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
    let mut rows = Vec::new();
    if !live.rows.is_empty() {
        rows.push(section(
            "agents",
            &format!("Agents {} active · {} total", live.active, live.rows.len()),
        ));
        rows.extend(live.rows.iter().map(live_row));
    }
    app.work_surface.latest_rows = rows.clone();
    rows
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
