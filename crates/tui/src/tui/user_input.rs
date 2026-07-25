//! Modal for request_user_input tool prompts.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Widget, Wrap};

use dse_localization::{MessageId, tr};
use dse_protocol::agent_runtime::{
    UserInputAnswer, UserInputQuestion, UserInputRequest,
    UserInteractionResponse as UserInputResponse,
};

use crate::palette;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent, render_modal_surface};

fn modal_block(title: &str) -> Block<'static> {
    Block::default()
        .title(Line::from(vec![Span::styled(
            title.to_string(),
            Style::default().fg(palette::DSE_ACCENT_PRIMARY).bold(),
        )]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::DSE_BG))
        .padding(Padding::uniform(1))
}

fn render_modal_chrome(area: Rect, popup_area: Rect, buf: &mut Buffer) {
    render_modal_surface(area, popup_area, buf);
}

fn push_option_lines(
    lines: &mut Vec<Line<'static>>,
    selected: bool,
    number: usize,
    label: String,
    description: String,
    ticked: bool,
) {
    let row_style = if selected {
        Style::default()
            .fg(palette::SELECTION_TEXT)
            .bg(palette::SELECTION_BG)
            .bold()
    } else {
        Style::default().fg(palette::TEXT_PRIMARY)
    };
    let detail_style = if selected {
        row_style
    } else {
        Style::default().fg(palette::TEXT_MUTED)
    };
    let prefix = if selected { ">" } else { " " };
    // Multi-select rows get a check-mark gutter when toggled into the pending
    // set, mirroring the affordance used in other multi-option pickers.
    let mark = if ticked { "✔ " } else { "  " };

    lines.push(Line::from(Span::styled(
        format!("{prefix}{mark}{number}) {label}"),
        row_style,
    )));
    lines.push(Line::from(Span::styled(
        format!("      {description}"),
        detail_style,
    )));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Selecting,
    OtherInput,
}

#[derive(Debug, Clone)]
pub struct UserInputView {
    tool_id: String,
    request: UserInputRequest,
    question_index: usize,
    selected: usize,
    mode: InputMode,
    other_input: String,
    answers: Vec<UserInputAnswer>,
    /// Indices toggled into the pending multi-select set for the current
    /// question. Only used when `question.multi_select` is true.
    multi_pending: Vec<usize>,
    validation_message: Option<String>,
}

impl UserInputView {
    pub fn new(tool_id: impl Into<String>, request: UserInputRequest) -> Self {
        Self {
            tool_id: tool_id.into(),
            request,
            question_index: 0,
            selected: 0,
            mode: InputMode::Selecting,
            other_input: String::new(),
            answers: Vec::new(),
            multi_pending: Vec::new(),
            validation_message: None,
        }
    }

    fn current_question(&self) -> &UserInputQuestion {
        &self.request.questions[self.question_index]
    }

    /// Whether the "Other" free-text row is offered for the current question.
    fn offers_other(&self) -> bool {
        self.current_question().allow_free_text
    }

    fn option_count(&self) -> usize {
        // Options + conditional "Other" row + conditional "Confirm" row.
        let mut count = self.current_question().options.len();
        count += usize::from(self.offers_other());
        count += usize::from(self.is_multi_select());
        count
    }

    fn is_other_selected(&self) -> bool {
        // "Other" sits immediately before the Confirm row when both exist, and
        // is last otherwise.
        let other_last = !self.is_multi_select();
        if other_last {
            self.offers_other() && self.selected + 1 == self.option_count()
        } else {
            self.offers_other() && self.selected + 2 == self.option_count()
        }
    }

    /// True when the multi-select "Confirm selection" row is highlighted.
    fn is_confirm_selected(&self) -> bool {
        self.is_multi_select() && self.selected + 1 == self.option_count()
    }

    fn is_multi_select(&self) -> bool {
        self.current_question().multi_select
    }

    fn toggle_pending(&mut self, index: usize) {
        self.validation_message = None;
        if let Some(pos) = self.multi_pending.iter().position(|i| *i == index) {
            self.multi_pending.remove(pos);
        } else {
            self.multi_pending.push(index);
        }
    }

    /// Build the answer(s) for the current question from a single selected
    /// option index (single-select and the confirm step of multi-select).
    fn answers_for_selection(&self, index: usize) -> Vec<UserInputAnswer> {
        let question = self.current_question();
        let option = &question.options[index];
        vec![UserInputAnswer {
            id: question.id.clone(),
            label: option.label.clone(),
            value: option.label.clone(),
        }]
    }

    fn advance_question(&mut self, new_answers: Vec<UserInputAnswer>) -> ViewAction {
        self.answers.extend(new_answers);
        if self.question_index + 1 >= self.request.questions.len() {
            let response = UserInputResponse::Answered {
                answers: self.answers.clone(),
            };
            return ViewAction::EmitAndClose(ViewEvent::UserInputSubmitted {
                tool_id: self.tool_id.clone(),
                response,
            });
        }
        self.question_index += 1;
        self.selected = 0;
        self.mode = InputMode::Selecting;
        self.other_input.clear();
        self.multi_pending.clear();
        self.validation_message = None;
        ViewAction::None
    }

    fn handle_selecting_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.option_count().saturating_sub(1));
                ViewAction::None
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let Some(number) = ch.to_digit(10) else {
                    return ViewAction::None;
                };
                if number == 0 {
                    return ViewAction::None;
                }
                let index = usize::try_from(number - 1).unwrap_or(usize::MAX);
                if index >= self.option_count() {
                    return ViewAction::None;
                }
                self.selected = index;
                self.activate_or_confirm_selection()
            }
            KeyCode::Char(' ') if self.is_multi_select() => {
                // Space toggles the highlighted option in the pending set
                // without leaving the picker (standard multi-select affordance).
                if !self.is_other_selected() {
                    self.toggle_pending(self.selected);
                }
                ViewAction::None
            }
            KeyCode::Enter => self.activate_or_confirm_selection(),
            KeyCode::Esc => ViewAction::EmitAndClose(ViewEvent::UserInputCancelled {
                tool_id: self.tool_id.clone(),
            }),
            _ => ViewAction::None,
        }
    }

    /// Resolve a digit/Enter activation for the currently highlighted row.
    ///
    /// - "Other" row → enter free-text input mode.
    /// - multi-select option → toggle into the pending set (Enter confirms on
    ///   the dedicated "Confirm" step; here it just toggles, like Space).
    /// - single-select option → submit immediately (legacy behavior).
    fn activate_or_confirm_selection(&mut self) -> ViewAction {
        if self.is_other_selected() {
            self.mode = InputMode::OtherInput;
            self.other_input.clear();
            return ViewAction::None;
        }
        if self.is_multi_select() {
            if self.is_confirm_selected() {
                if self.multi_pending.is_empty() {
                    self.validation_message =
                        Some(tr(MessageId::UserInputValidationSelect).into_owned());
                    return ViewAction::None;
                }
                let question = self.current_question();
                let answers: Vec<UserInputAnswer> = self
                    .multi_pending
                    .iter()
                    .filter_map(|i| question.options.get(*i))
                    .map(|opt| UserInputAnswer {
                        id: question.id.clone(),
                        label: opt.label.clone(),
                        value: opt.label.clone(),
                    })
                    .collect();
                return self.advance_question(answers);
            }
            // Enter/Space on a real option toggles it into the pending set.
            self.toggle_pending(self.selected);
            return ViewAction::None;
        }
        // Single-select: submit immediately.
        let answers = self.answers_for_selection(self.selected);
        self.advance_question(answers)
    }

    fn handle_other_input_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Selecting;
                self.other_input.clear();
                ViewAction::None
            }
            KeyCode::Enter => {
                if self.other_input.trim().is_empty() {
                    self.validation_message =
                        Some(tr(MessageId::UserInputValidationResponse).into_owned());
                    return ViewAction::None;
                }
                let question = self.current_question();
                let answer = UserInputAnswer {
                    id: question.id.clone(),
                    label: "Other".to_string(),
                    value: self.other_input.trim().to_string(),
                };
                // In multi-select mode a free-text "Other" is still a single
                // answer appended to whatever options were toggled.
                let mut answers: Vec<UserInputAnswer> = self
                    .multi_pending
                    .iter()
                    .filter_map(|i| question.options.get(*i))
                    .map(|opt| UserInputAnswer {
                        id: question.id.clone(),
                        label: opt.label.clone(),
                        value: opt.label.clone(),
                    })
                    .collect();
                answers.push(answer);
                self.advance_question(answers)
            }
            KeyCode::Backspace => {
                self.other_input.pop();
                self.validation_message = None;
                ViewAction::None
            }
            KeyCode::Char('h')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.other_input.pop();
                self.validation_message = None;
                ViewAction::None
            }
            KeyCode::Char(ch) => {
                if !ch.is_control() {
                    self.validation_message = None;
                    self.other_input.push(ch);
                }
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }
}

impl ModalView for UserInputView {
    fn kind(&self) -> ModalKind {
        ModalKind::UserInput
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match self.mode {
            InputMode::Selecting => self.handle_selecting_key(key),
            InputMode::OtherInput => self.handle_other_input_key(key),
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let question = self.current_question();
        let total = self.request.questions.len();
        let header = format!(
            " {} ({}/{}) ",
            question.header,
            self.question_index + 1,
            total
        );

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![Span::styled(
            tr(MessageId::UserInputActionRequired).into_owned(),
            Style::default().fg(palette::DSE_INFO).bold(),
        )]));
        lines.push(Line::from(vec![
            Span::styled(
                question.header.clone(),
                Style::default().fg(palette::TEXT_PRIMARY).bold(),
            ),
            Span::styled(
                format!(
                    "  {}",
                    tr(MessageId::UserInputQuestionProgress)
                        .replace("{current}", &(self.question_index + 1).to_string())
                        .replace("{total}", &total.to_string())
                ),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            question.question.clone(),
            Style::default().fg(palette::TEXT_PRIMARY).bold(),
        )]));
        lines.push(Line::from(""));

        for (idx, option) in question.options.iter().enumerate() {
            let number = idx + 1;
            let ticked = self.is_multi_select() && self.multi_pending.contains(&idx);
            push_option_lines(
                &mut lines,
                self.selected == idx,
                number,
                option.label.clone(),
                option.description.clone(),
                ticked,
            );
        }

        // The free-text "Other" row is now conditional on allow_free_text.
        if self.offers_other() {
            let other_index = question.options.len();
            let other_number = other_index + 1;
            push_option_lines(
                &mut lines,
                self.selected == other_index,
                other_number,
                tr(MessageId::UserInputOther).into_owned(),
                tr(MessageId::UserInputOtherDescription).into_owned(),
                false,
            );
        }

        // Multi-select gets a dedicated "Confirm selection" row after the
        // options (and after "Other" when present). Selecting and pressing
        // Enter on it flushes the pending set as the question's answers.
        if self.is_multi_select() {
            let confirm_index = self.option_count().saturating_sub(1);
            let confirm_number = confirm_index + 1;
            push_option_lines(
                &mut lines,
                self.selected == confirm_index,
                confirm_number,
                tr(MessageId::UserInputConfirmSelection).into_owned(),
                tr(MessageId::UserInputSubmitSelected)
                    .replace("{count}", &self.multi_pending.len().to_string()),
                false,
            );
        }

        if self.mode == InputMode::OtherInput {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("> {}", tr(MessageId::UserInputCustomResponse)),
                    Style::default().fg(palette::TEXT_PRIMARY).bold(),
                ),
                Span::raw(" "),
                Span::styled(
                    if self.other_input.is_empty() {
                        tr(MessageId::UserInputTypeResponse).into_owned()
                    } else {
                        self.other_input.clone()
                    },
                    Style::default().fg(palette::DSE_ACCENT_PRIMARY),
                ),
            ]));
        }

        if let Some(message) = &self.validation_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                message.clone(),
                Style::default().fg(palette::STATUS_ERROR).bold(),
            )));
        }

        lines.push(Line::from(""));
        if self.mode == InputMode::OtherInput {
            lines.push(Line::from(vec![
                Span::styled("Enter", Style::default().fg(palette::DSE_INFO).bold()),
                Span::styled(
                    format!(" {}", tr(MessageId::UserInputSubmit)),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
                Span::raw("  "),
                Span::styled("Esc", Style::default().fg(palette::DSE_INFO).bold()),
                Span::styled(
                    format!(" {}", tr(MessageId::UserInputBack)),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
            ]));
        } else {
            let opt_count = self.option_count();
            let quick_pick_label = if opt_count <= 9 {
                format!("1-{opt_count}")
            } else {
                tr(MessageId::UserInputDigit).into_owned()
            };
            if self.is_multi_select() {
                lines.push(Line::from(vec![
                    Span::styled(
                        quick_pick_label,
                        Style::default().fg(palette::DSE_INFO).bold(),
                    ),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputMove)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Space", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputToggle)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Enter", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputToggleConfirm)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Esc", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputCancel)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(
                        quick_pick_label,
                        Style::default().fg(palette::DSE_INFO).bold(),
                    ),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputQuickPick)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Up/Down", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputMove)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Enter", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputConfirmSelection)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled("Esc", Style::default().fg(palette::DSE_INFO).bold()),
                    Span::styled(
                        format!(" {}", tr(MessageId::UserInputCancel)),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(modal_block(&header));

        let popup_area = centered_rect(82, 68, area);
        render_modal_chrome(area, popup_area, buf);
        paragraph.render(popup_area, buf);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dse_protocol::agent_runtime::UserInputOption;
    use unicode_width::UnicodeWidthStr;

    fn render_view(view: &UserInputView, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);

        (0..height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < width {
                    let symbol = buf[(x, y)].symbol();
                    row.push_str(symbol);
                    x = x.saturating_add(
                        u16::try_from(UnicodeWidthStr::width(symbol).max(1)).unwrap_or(u16::MAX),
                    );
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample_view() -> UserInputView {
        UserInputView::new(
            "tool-1",
            UserInputRequest {
                questions: vec![UserInputQuestion {
                    header: "Confirm".to_string(),
                    id: "confirm".to_string(),
                    question: "What should happen next?".to_string(),
                    options: vec![
                        UserInputOption {
                            label: "Ship it".to_string(),
                            description: "Proceed with the current change set".to_string(),
                        },
                        UserInputOption {
                            label: "Revise it".to_string(),
                            description: "Return to editing before continuing".to_string(),
                        },
                    ],
                    allow_free_text: true,
                    multi_select: false,
                }],
            },
        )
    }

    #[test]
    fn user_input_modal_calls_out_required_action_and_controls() {
        let rendered = render_view(&sample_view(), 110, 36);

        assert!(rendered.contains("需要你的操作"));
        assert!(rendered.contains("第 1/1 个问题"));
        assert!(rendered.contains("快速选择"));
        assert!(rendered.contains("其他"));
        assert!(rendered.contains("输入自定义回答"));
        // Model-provided question and option text remains raw.
        assert!(rendered.contains("What should happen next?"));
        assert!(rendered.contains("Proceed with the current change set"));
    }

    #[test]
    fn user_input_modal_renders_custom_response_state() {
        let mut view = sample_view();
        view.selected = 2;
        view.mode = InputMode::OtherInput;
        view.other_input = "Need one more pass".to_string();

        let rendered = render_view(&view, 110, 36);

        assert!(rendered.contains("自定义回答"));
        assert!(rendered.contains("Need one more pass"));
        assert!(rendered.contains("Enter"));
        assert!(rendered.contains("提交"));
    }

    #[test]
    fn user_input_modal_hides_other_row_when_free_text_disabled() {
        // Issue #3102: allow_free_text=false must NOT render the Host-owned
        // free-text pseudo-option.
        let mut view = sample_view();
        view.request.questions[0].allow_free_text = false;
        // Reset selection to a valid option index (no Other row to land on).
        view.selected = 0;

        let rendered = render_view(&view, 110, 36);
        assert!(
            !rendered.contains("输入自定义回答"),
            "Other row should be hidden when allow_free_text is false"
        );
        assert!(!rendered.contains("\n其他\n"));
    }

    #[test]
    fn user_input_modal_renders_multi_select_ticks_and_confirm() {
        // Issue #3102: multi_select=true renders a check-mark gutter on
        // toggled options plus a trailing Host-owned confirmation row.
        let mut view = sample_view();
        view.request.questions[0].multi_select = true;
        view.request.questions[0].allow_free_text = false;
        // Toggle the first option into the pending set.
        view.multi_pending.push(0);
        // Highlight the confirm row (last selectable row).
        view.selected = view.option_count() - 1;

        let rendered = render_view(&view, 120, 40);
        assert!(rendered.contains("✔"), "toggled option shows a check mark");
        assert!(
            rendered.contains("确认选择"),
            "multi-select renders a confirm row"
        );
        assert!(
            rendered.contains("3) 确认选择"),
            "confirm row keeps its real selectable index"
        );
        assert!(
            rendered.contains("提交已选择的 1 项"),
            "localized multi-select summary missing:\n{rendered}"
        );
        assert!(rendered.contains("切换选择"));
    }

    #[test]
    fn user_input_modal_respects_80_and_120_column_frames() {
        for width in [80, 120] {
            let rendered = render_view(&sample_view(), width, 40);
            assert!(
                rendered
                    .lines()
                    .all(|line| UnicodeWidthStr::width(line) <= usize::from(width)),
                "request_user_input exceeded its {width}-column frame"
            );
            assert!(rendered.contains("需要你的操作"));
            assert!(rendered.contains("What should happen next?"));
        }
    }

    #[test]
    fn empty_multi_select_stays_open_and_shows_validation() {
        let mut view = sample_view();
        view.request.questions[0].multi_select = true;
        view.request.questions[0].allow_free_text = false;
        view.selected = view.option_count() - 1;

        let action = view.handle_key(KeyEvent::from(KeyCode::Enter));

        assert!(matches!(action, ViewAction::None));
        assert!(view.answers.is_empty());
        assert_eq!(
            view.validation_message.as_deref(),
            Some("请至少选择一个选项后再确认")
        );
        assert!(render_view(&view, 120, 40).contains('请'));
    }

    #[test]
    fn empty_free_text_stays_open_and_shows_validation() {
        let mut view = sample_view();
        view.mode = InputMode::OtherInput;
        view.other_input = "   ".to_string();

        let action = view.handle_key(KeyEvent::from(KeyCode::Enter));

        assert!(matches!(action, ViewAction::None));
        assert!(view.answers.is_empty());
        assert_eq!(view.mode, InputMode::OtherInput);
        assert_eq!(
            view.validation_message.as_deref(),
            Some("请输入内容后再确认")
        );
        assert!(render_view(&view, 120, 40).contains('请'));
    }
}
