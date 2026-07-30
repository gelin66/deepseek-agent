//! Workspace-scoped projection of canonical root Runs.
//!
//! The hub is a full-screen TUI room backed only by `ListRoots` + `Get`.
//! Selection emits an application intent; this view never owns lifecycle or
//! persistence state.

use std::borrow::Cow;
use std::cell::RefCell;

use chrono::{DateTime, SecondsFormat, Utc};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use dse_localization::{MessageId, ProductLanguage, tr_in};
use dse_protocol::agent_runtime::{RunId, TerminalState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::palette;
use crate::tui::run_client::TuiRootRun;
use crate::tui::ui_text::semantic_truncate;
use crate::tui::views::{
    ActionHint, SecondarySurface, SecondarySurfaceKind, ViewAction, ViewEvent,
    render_action_footer, render_full_screen_room, render_panel_scroll_rail,
};

pub struct RunHubView {
    workspace: String,
    roots: Vec<TuiRootRun>,
    selected: usize,
    language: ProductLanguage,
    row_hitboxes: RefCell<Vec<(Rect, usize)>>,
}

impl RunHubView {
    #[must_use]
    pub fn new(workspace: String, roots: Vec<TuiRootRun>, language: ProductLanguage) -> Self {
        Self {
            workspace,
            roots,
            selected: 0,
            language,
            row_hitboxes: RefCell::new(Vec::new()),
        }
    }

    fn item_count(&self) -> usize {
        self.roots.len() + 1
    }

    fn new_root_index(&self) -> usize {
        self.roots.len()
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.item_count() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
    }

    fn choose_selected(&self) -> ViewAction {
        if let Some(root) = self.roots.get(self.selected) {
            ViewAction::Emit(ViewEvent::RunHubOpen {
                run_id: root.summary.run_id.clone(),
            })
        } else {
            ViewAction::Emit(ViewEvent::RunHubNewRoot)
        }
    }

    fn row_text(&self, index: usize, width: usize) -> String {
        let selected = index == self.selected;
        let marker = if selected { "›" } else { " " };
        let text = if let Some(root) = self.roots.get(index) {
            let status = status_label(
                self.language,
                &root.run.completion,
                root.run.terminal.as_ref(),
            );
            let updated = format_updated_at(root.summary.updated_at_unix_ms);
            let objective = root
                .run
                .task_contract
                .as_ref()
                .map(|contract| contract.definition.objective.trim())
                .filter(|objective| !objective.is_empty())
                .map_or_else(
                    || tr_in(self.language, MessageId::RunHubUntitled).into_owned(),
                    ToOwned::to_owned,
                );
            let lineage = if root.summary.continued_from_run_id.is_some() {
                "↳ "
            } else {
                ""
            };
            let run_id = short_run_id(&root.summary.run_id);
            if width >= 96 {
                format!("{marker} {status}  {updated}  {run_id}  {lineage}{objective}")
            } else if width >= 58 {
                format!("{marker} {status}  {updated}  {lineage}{objective}")
            } else {
                format!("{marker} {status}  {lineage}{objective}")
            }
        } else {
            format!(
                "{marker} ＋ {} — {}",
                tr_in(self.language, MessageId::RunHubNewRoot),
                tr_in(self.language, MessageId::RunHubNewRootDescription)
            )
        };
        semantic_truncate(&text, width)
    }
}

impl SecondarySurface for RunHubView {
    fn kind(&self) -> SecondarySurfaceKind {
        SecondarySurfaceKind::RunHub
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::Home => {
                self.selected = 0;
                ViewAction::None
            }
            KeyCode::End => {
                self.selected = self.new_root_index();
                ViewAction::None
            }
            KeyCode::Char('n' | 'N') => ViewAction::Emit(ViewEvent::RunHubNewRoot),
            KeyCode::Enter => self.choose_selected(),
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => ViewAction::Close,
            _ => ViewAction::None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> ViewAction {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.move_selection(-1);
                ViewAction::None
            }
            MouseEventKind::ScrollDown => {
                self.move_selection(1);
                ViewAction::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let selected = self.row_hitboxes.borrow().iter().find_map(|(rect, index)| {
                    point_in_rect(mouse.column, mouse.row, *rect).then_some(*index)
                });
                if let Some(selected) = selected {
                    self.selected = selected;
                    self.choose_selected()
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut inner =
            render_full_screen_room(area, buf, tr_in(self.language, MessageId::RunHubTitle));
        inner = render_action_footer(
            inner,
            buf,
            &[
                ActionHint::new("↑/↓", tr_in(self.language, MessageId::RunHubMove)),
                ActionHint::new("Enter", tr_in(self.language, MessageId::RunHubOpen)),
                ActionHint::new("N", tr_in(self.language, MessageId::RunHubNewShortcut)),
                ActionHint::new("Esc", tr_in(self.language, MessageId::RunHubClose)),
            ],
        );

        if inner.height == 0 {
            self.row_hitboxes.borrow_mut().clear();
            return;
        }
        let workspace = tr_in(self.language, MessageId::RunHubWorkspace)
            .replace("{workspace}", &self.workspace);
        Paragraph::new(Line::from(Span::styled(
            semantic_truncate(&workspace, usize::from(inner.width)),
            Style::default().fg(palette::TEXT_MUTED),
        )))
        .render(Rect { height: 1, ..inner }, buf);
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);

        if inner.height > 0 {
            let count = tr_in(self.language, MessageId::RunHubCount)
                .replace("{count}", &self.roots.len().to_string());
            Paragraph::new(Line::from(Span::styled(
                count,
                Style::default().fg(palette::TEXT_DIM),
            )))
            .render(Rect { height: 1, ..inner }, buf);
            inner.y = inner.y.saturating_add(1);
            inner.height = inner.height.saturating_sub(1);
        }

        let visible_rows = usize::from(inner.height).max(1);
        let offset = self
            .selected
            .saturating_add(1)
            .saturating_sub(visible_rows)
            .min(self.item_count().saturating_sub(visible_rows));
        let rows =
            render_panel_scroll_rail(inner, buf, self.item_count(), offset, visible_rows, true);
        let mut hitboxes = Vec::new();
        for (visible_index, item_index) in (offset..self.item_count())
            .take(usize::from(rows.height))
            .enumerate()
        {
            let y = rows
                .y
                .saturating_add(u16::try_from(visible_index).unwrap_or(u16::MAX));
            let selected = item_index == self.selected;
            let tone = if item_index == self.new_root_index() || selected {
                palette::DSE_ACCENT_PRIMARY
            } else {
                status_tone(
                    &self.roots[item_index].run.completion,
                    self.roots[item_index].run.terminal.as_ref(),
                )
            };
            Paragraph::new(Line::from(Span::styled(
                self.row_text(item_index, usize::from(rows.width)),
                Style::default().fg(tone).add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            )))
            .render(
                Rect {
                    x: rows.x,
                    y,
                    width: rows.width,
                    height: 1,
                },
                buf,
            );
            hitboxes.push((
                Rect {
                    x: rows.x,
                    y,
                    width: rows.width,
                    height: 1,
                },
                item_index,
            ));
        }
        *self.row_hitboxes.borrow_mut() = hitboxes;
    }
}

fn status_label(
    language: ProductLanguage,
    completion: &dse_protocol::run_api::RunCompletion,
    terminal: Option<&TerminalState>,
) -> Cow<'static, str> {
    use dse_protocol::run_api::RunCompletion;
    match completion {
        RunCompletion::Answered { .. } => tr_in(language, MessageId::RunStatusAnswered),
        RunCompletion::HostAccepted { .. } => tr_in(language, MessageId::RunStatusHostAccepted),
        RunCompletion::VerifiedCompleted { .. } => {
            tr_in(language, MessageId::RunStatusVerifiedCompleted)
        }
        RunCompletion::Running if terminal.is_none() => {
            tr_in(language, MessageId::RunHubStatusActive)
        }
        RunCompletion::Running | RunCompletion::EndedWithoutCompletion => match terminal {
            None => tr_in(language, MessageId::RunHubStatusActive),
            Some(TerminalState::AwaitingHostAcceptance { .. }) => {
                tr_in(language, MessageId::RunTerminalAwaitingHostAcceptance)
            }
            Some(TerminalState::Completed { .. }) => {
                tr_in(language, MessageId::RunTerminalCompleted)
            }
            Some(TerminalState::Blocked { .. }) => tr_in(language, MessageId::RunTerminalBlocked),
            Some(TerminalState::Failed { .. }) => tr_in(language, MessageId::RunTerminalFailed),
            Some(TerminalState::Cancelled) => tr_in(language, MessageId::RunTerminalCancelled),
            Some(TerminalState::Interrupted) => tr_in(language, MessageId::RunTerminalInterrupted),
            Some(TerminalState::RecoveryRequired { .. }) => {
                tr_in(language, MessageId::RunTerminalRecoveryRequired)
            }
        },
    }
}

fn status_tone(
    completion: &dse_protocol::run_api::RunCompletion,
    terminal: Option<&TerminalState>,
) -> ratatui::style::Color {
    use dse_protocol::run_api::RunCompletion;
    match completion {
        RunCompletion::Answered { .. } => palette::STATUS_WARNING,
        RunCompletion::HostAccepted { .. } => palette::DSE_INFO,
        RunCompletion::VerifiedCompleted { .. } => palette::STATUS_SUCCESS,
        RunCompletion::Running | RunCompletion::EndedWithoutCompletion => match terminal {
            None => palette::DSE_INFO,
            Some(TerminalState::AwaitingHostAcceptance { .. }) => palette::STATUS_WARNING,
            Some(TerminalState::Completed { .. }) => palette::STATUS_SUCCESS,
            Some(TerminalState::Blocked { .. } | TerminalState::Interrupted) => {
                palette::STATUS_WARNING
            }
            Some(TerminalState::Failed { .. } | TerminalState::RecoveryRequired { .. }) => {
                palette::STATUS_ERROR
            }
            Some(TerminalState::Cancelled) => palette::TEXT_MUTED,
        },
    }
}

fn format_updated_at(unix_ms: u64) -> String {
    i64::try_from(unix_ms)
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| unix_ms.to_string())
}

fn short_run_id(run_id: &RunId) -> String {
    run_id.0.chars().take(12).collect()
}

fn point_in_rect(column: u16, row: u16, rect: Rect) -> bool {
    column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom()
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyModifiers};
    use dse_protocol::agent_runtime::TerminalState;
    use dse_protocol::run_api::{RootRunSummary, RunCompletion, RunView};
    use dse_protocol::task::{
        AcceptanceId, AcceptanceSatisfaction, CompletionCandidate, CompletionCandidateId,
        CompletionDecision, EvidenceReceiptId, HostAcceptanceReceipt, HostAcceptanceReceiptId,
        TaskContract, TaskDefinition, TaskGenerationId, WorkspaceRevision, WorkspaceState,
    };

    use super::*;

    fn root(run_id: &str, objective: &str, terminal: Option<TerminalState>) -> TuiRootRun {
        let completion = match &terminal {
            Some(TerminalState::AwaitingHostAcceptance { candidate }) => RunCompletion::Answered {
                candidate: candidate.clone(),
                current_workspace_state: candidate.workspace_state.clone(),
            },
            Some(TerminalState::Completed { decision, .. })
                if decision.satisfied.iter().any(|criterion| {
                    matches!(criterion, AcceptanceSatisfaction::Evidence { .. })
                }) =>
            {
                RunCompletion::VerifiedCompleted {
                    decision: decision.clone(),
                }
            }
            Some(TerminalState::Completed { message, decision }) => {
                let receipt_id = decision
                    .satisfied
                    .iter()
                    .find_map(|satisfaction| match satisfaction {
                        AcceptanceSatisfaction::Host { receipt_id, .. } => Some(receipt_id.clone()),
                        AcceptanceSatisfaction::Evidence { .. } => None,
                    })
                    .expect("Host-completed fixture receipt");
                let candidate = CompletionCandidate {
                    id: decision.candidate_id.clone(),
                    generation_id: decision.generation_id.clone(),
                    message: message.clone(),
                    workspace_state: decision.workspace_state.clone(),
                };
                RunCompletion::HostAccepted {
                    candidate,
                    receipt: HostAcceptanceReceipt {
                        id: receipt_id,
                        candidate_id: decision.candidate_id.clone(),
                        generation_id: decision.generation_id.clone(),
                        workspace_state: decision.workspace_state.clone(),
                    },
                    current_workspace_state: decision.workspace_state.clone(),
                }
            }
            Some(_) => RunCompletion::EndedWithoutCompletion,
            None => RunCompletion::Running,
        };
        TuiRootRun {
            summary: RootRunSummary {
                run_id: RunId::from(run_id),
                continued_from_run_id: None,
                workspace: "/workspace/project".to_owned(),
                last_sequence: 4,
                terminal: terminal.is_some(),
                created_at_unix_ms: 1_700_000_000_000,
                updated_at_unix_ms: 1_700_000_123_000,
            },
            run: RunView {
                run_id: RunId::from(run_id),
                parent_run_id: None,
                continued_from_run_id: None,
                model: "deepseek-v4-flash".to_owned(),
                task_contract: Some(TaskContract {
                    generation_id: TaskGenerationId::from("task-1"),
                    definition: TaskDefinition::host(objective),
                }),
                workspace: "/workspace/project".to_owned(),
                last_sequence: 4,
                completion,
                terminal,
                usage: Default::default(),
                accounting: Default::default(),
                runtime_model_requests: 1,
                runtime_retries: 0,
                tool_calls: 0,
                local_turns: 1,
            },
        }
    }

    fn buffer_text(buffer: &Buffer) -> String {
        (buffer.area.y..buffer.area.bottom())
            .map(|y| {
                (buffer.area.x..buffer.area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn completion_truth_has_distinct_labels_and_tones() {
        let workspace_state = WorkspaceState {
            generation: 1,
            revision: WorkspaceRevision::Known {
                sha256: "sha256:hub".to_owned(),
            },
        };
        let candidate = CompletionCandidate {
            id: CompletionCandidateId::from("candidate"),
            generation_id: TaskGenerationId::from("generation"),
            message: "answer".to_owned(),
            workspace_state: workspace_state.clone(),
        };
        let decision = |satisfied| CompletionDecision {
            candidate_id: candidate.id.clone(),
            generation_id: candidate.generation_id.clone(),
            workspace_state: workspace_state.clone(),
            satisfied: vec![satisfied],
        };
        let answered = RunCompletion::Answered {
            candidate: candidate.clone(),
            current_workspace_state: workspace_state.clone(),
        };
        let host = RunCompletion::HostAccepted {
            candidate: candidate.clone(),
            receipt: HostAcceptanceReceipt {
                id: HostAcceptanceReceiptId::from("host-acceptance:fixture"),
                candidate_id: candidate.id.clone(),
                generation_id: candidate.generation_id.clone(),
                workspace_state: workspace_state.clone(),
            },
            current_workspace_state: workspace_state.clone(),
        };
        let verified = RunCompletion::VerifiedCompleted {
            decision: decision(AcceptanceSatisfaction::Evidence {
                acceptance_id: AcceptanceId::from("tests"),
                receipt_id: EvidenceReceiptId::from("evidence:fixture"),
            }),
        };

        assert_eq!(
            status_label(ProductLanguage::English, &answered, None),
            tr_in(ProductLanguage::English, MessageId::RunStatusAnswered)
        );
        assert_eq!(
            status_label(ProductLanguage::English, &host, None),
            tr_in(ProductLanguage::English, MessageId::RunStatusHostAccepted)
        );
        assert_eq!(
            status_label(ProductLanguage::English, &verified, None),
            tr_in(
                ProductLanguage::English,
                MessageId::RunStatusVerifiedCompleted
            )
        );
        assert_eq!(status_tone(&answered, None), palette::STATUS_WARNING);
        assert_eq!(status_tone(&host, None), palette::DSE_INFO);
        assert_eq!(status_tone(&verified, None), palette::STATUS_SUCCESS);
    }

    #[test]
    fn keyboard_and_mouse_emit_the_same_canonical_run_choice() {
        let roots = vec![root("run-active", "审计当前项目", None)];
        let mut keyboard = RunHubView::new(
            "/workspace/project".to_owned(),
            roots.clone(),
            ProductLanguage::English,
        );
        assert!(matches!(
            keyboard.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ViewAction::Emit(ViewEvent::RunHubOpen { run_id }) if run_id == RunId::from("run-active")
        ));

        let mut mouse = RunHubView::new(
            "/workspace/project".to_owned(),
            roots,
            ProductLanguage::English,
        );
        let area = Rect::new(0, 0, 100, 24);
        let mut buffer = Buffer::empty(area);
        mouse.render(area, &mut buffer);
        let row = mouse.row_hitboxes.borrow()[0].0;
        assert!(matches!(
            mouse.handle_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            }),
            ViewAction::Emit(ViewEvent::RunHubOpen { run_id }) if run_id == RunId::from("run-active")
        ));
    }

    #[test]
    fn new_root_is_a_visible_keyboard_and_mouse_action() {
        let mut view = RunHubView::new(
            "/workspace/project".to_owned(),
            vec![root(
                "run-terminal",
                "完成旧任务",
                Some(TerminalState::Blocked {
                    reason: "fixture".to_owned(),
                }),
            )],
            ProductLanguage::SimplifiedChinese,
        );
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            ViewAction::None
        ));
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ViewAction::Emit(ViewEvent::RunHubNewRoot)
        ));

        let area = Rect::new(0, 0, 80, 18);
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        let text = buffer_text(&buffer);
        assert!(
            text.replace(' ', "").contains("新建运行"),
            "missing localized new action: {text}"
        );
        let new_row = view
            .row_hitboxes
            .borrow()
            .iter()
            .find(|(_, index)| *index == view.new_root_index())
            .expect("new root row")
            .0;
        assert!(matches!(
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: new_row.x,
                row: new_row.y,
                modifiers: KeyModifiers::NONE,
            }),
            ViewAction::Emit(ViewEvent::RunHubNewRoot)
        ));
    }

    #[test]
    fn responsive_rows_keep_status_time_and_cjk_objective_bounded() {
        let view = RunHubView::new(
            "/workspace/项目".to_owned(),
            vec![root(
                "run-terminal-long-identity",
                "修复数据库迁移并验证恢复链路",
                Some(TerminalState::Blocked {
                    reason: "fixture".to_owned(),
                }),
            )],
            ProductLanguage::SimplifiedChinese,
        );
        assert!(view.row_text(0, 100).contains("2023-11-14T22:15:23Z"));
        for width in [24usize, 48, 80, 120] {
            let row = view.row_text(0, width);
            assert!(crate::tui::ui_text::text_display_width(&row) <= width);
            assert!(!row.contains('\u{fffd}'));
        }
    }
}
