//! Three-row projection of the canonical Run permission preset.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use dse_localization::{MessageId, tr};
use dse_protocol::agent_runtime::RunPermissionMode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::palette;
use crate::tui::views::{
    ActionHint, ModalKind, ModalView, ViewAction, ViewEvent, render_modal_footer,
    render_modal_surface,
};

const MODES: [RunPermissionMode; 3] = [
    RunPermissionMode::Ask,
    RunPermissionMode::Agent,
    RunPermissionMode::FullAccess,
];

pub struct PermissionSelector {
    selected: usize,
    active_run: bool,
    row_hitboxes: RefCell<Vec<Rect>>,
}

impl PermissionSelector {
    #[must_use]
    pub fn new(current: RunPermissionMode, active_run: bool) -> Self {
        Self {
            selected: MODES.iter().position(|mode| *mode == current).unwrap_or(0),
            active_run,
            row_hitboxes: RefCell::new(Vec::new()),
        }
    }

    fn selected_mode(&self) -> RunPermissionMode {
        MODES[self.selected]
    }

    fn move_selection(&mut self, delta: isize) {
        let count = MODES.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
    }

    fn choose(&self) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::PermissionSelected {
            mode: self.selected_mode(),
        })
    }

    fn sheet(area: Rect) -> Rect {
        let height = area.height.min(10);
        Rect {
            x: area.x,
            y: area.bottom().saturating_sub(height),
            width: area.width,
            height,
        }
    }
}

impl ModalView for PermissionSelector {
    fn kind(&self) -> ModalKind {
        ModalKind::Permission
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::Enter => self.choose(),
            KeyCode::Esc => ViewAction::Close,
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
                let selected = self.row_hitboxes.borrow().iter().position(|rect| {
                    mouse.column >= rect.x
                        && mouse.column < rect.right()
                        && mouse.row >= rect.y
                        && mouse.row < rect.bottom()
                });
                if let Some(selected) = selected {
                    self.selected = selected;
                    self.choose()
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let sheet = Self::sheet(area);
        render_modal_surface(area, sheet, buf);
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .title(Line::from(Span::styled(
                format!(" {} ", tr(MessageId::PermissionSelectorTitle)),
                Style::default()
                    .fg(palette::DSE_ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().bg(palette::DSE_BG));
        let mut inner = block.inner(sheet);
        block.render(sheet, buf);
        inner = render_modal_footer(
            inner,
            buf,
            &[
                ActionHint::new("↑/↓", tr(MessageId::PermissionSelectorMove)),
                ActionHint::new("Enter", tr(MessageId::PermissionSelectorChoose)),
                ActionHint::new("Esc", tr(MessageId::PermissionSelectorClose)),
            ],
        );

        let note = if self.active_run {
            tr(MessageId::PermissionSelectorActiveRunNote)
        } else {
            tr(MessageId::PermissionSelectorNextRunNote)
        };
        if inner.height > 0 {
            Paragraph::new(Line::from(Span::styled(
                note,
                Style::default().fg(palette::TEXT_MUTED),
            )))
            .render(Rect { height: 1, ..inner }, buf);
            inner.y = inner.y.saturating_add(1);
            inner.height = inner.height.saturating_sub(1);
        }

        let mut hitboxes = Vec::new();
        for (index, mode) in MODES.iter().enumerate() {
            if index >= usize::from(inner.height) {
                break;
            }
            let y = inner
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            let selected = index == self.selected;
            let (name, description) = match mode {
                RunPermissionMode::Ask => (
                    tr(MessageId::PermissionModeAskName),
                    tr(MessageId::PermissionModeAskDescription),
                ),
                RunPermissionMode::Agent => (
                    tr(MessageId::PermissionModeAgentName),
                    tr(MessageId::PermissionModeAgentDescription),
                ),
                RunPermissionMode::FullAccess => (
                    tr(MessageId::PermissionModeFullAccessName),
                    tr(MessageId::PermissionModeFullAccessDescription),
                ),
            };
            let tone = if *mode == RunPermissionMode::FullAccess {
                palette::STATUS_WARNING
            } else if selected {
                palette::DSE_ACCENT_PRIMARY
            } else {
                palette::TEXT_PRIMARY
            };
            let marker = if selected { "✓" } else { " " };
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{marker} {name}"),
                    Style::default().fg(tone).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled("  ", Style::default()),
                Span::styled(description, Style::default().fg(palette::TEXT_MUTED)),
            ]))
            .render(
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
            hitboxes.push(Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            });
        }
        *self.row_hitboxes.borrow_mut() = hitboxes;
    }

    fn occupied_region(&self, area: Rect) -> Rect {
        Self::sheet(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};

    #[test]
    fn keyboard_and_mouse_emit_the_same_typed_selection() {
        let mut keyboard = PermissionSelector::new(RunPermissionMode::Ask, false);
        let _ = keyboard.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(matches!(
            keyboard.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ViewAction::EmitAndClose(ViewEvent::PermissionSelected {
                mode: RunPermissionMode::Agent
            })
        ));

        let mut mouse = PermissionSelector::new(RunPermissionMode::Ask, false);
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        mouse.render(area, &mut buffer);
        let row = mouse.row_hitboxes.borrow()[1];
        assert!(matches!(
            mouse.handle_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            }),
            ViewAction::EmitAndClose(ViewEvent::PermissionSelected {
                mode: RunPermissionMode::Agent
            })
        ));
    }
}
