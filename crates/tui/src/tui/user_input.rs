//! Secondary surface for request_user_input tool prompts.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget, Wrap};
use unicode_width::UnicodeWidthStr;

use dse_localization::{MessageId, tr};
use dse_protocol::agent_runtime::{
    UserInputAnswer, UserInputQuestion, UserInputRequest,
    UserInteractionResponse as UserInputResponse,
};

use crate::palette;
use crate::tui::views::{
    ActionHint, SecondarySurface, SecondarySurfaceKind, ViewAction, ViewEvent, bottom_sheet_rect,
    render_action_footer, render_bottom_sheet, render_full_screen_room, render_panel_scroll_rail,
};

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
    row_hitboxes: RefCell<Vec<(usize, Rect)>>,
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
            row_hitboxes: RefCell::new(Vec::new()),
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

    fn move_selection(&mut self, delta: isize) {
        let count = self.option_count() as isize;
        if count > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        }
    }

    fn estimated_content_rows(&self, width: u16) -> usize {
        let width = usize::from(width.saturating_sub(4).max(1));
        let question = self.current_question();
        let wrapped = |value: &str| UnicodeWidthStr::width(value).div_ceil(width).max(1);
        let mut rows = 5 + wrapped(&question.question);
        for option in &question.options {
            rows += 1 + wrapped(&option.description);
        }
        if self.offers_other() {
            rows += 2;
        }
        if self.is_multi_select() {
            rows += 2;
        }
        if self.mode == InputMode::OtherInput {
            rows += 2;
        }
        if self.validation_message.is_some() {
            rows += 2;
        }
        rows
    }

    fn sheet_height(&self, area: Rect) -> u16 {
        let desired = u16::try_from(self.estimated_content_rows(area.width).saturating_add(4))
            .unwrap_or(u16::MAX);
        let maximum = area.height.saturating_mul(2).div_ceil(3).max(8);
        desired.min(maximum).min(area.height)
    }

    fn uses_full_screen_room(&self, area: Rect) -> bool {
        let sheet = self.sheet_height(area);
        let required = u16::try_from(self.estimated_content_rows(area.width).saturating_add(4))
            .unwrap_or(u16::MAX);
        required > sheet || area.height < 14
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

impl SecondarySurface for UserInputView {
    fn kind(&self) -> SecondarySurfaceKind {
        SecondarySurfaceKind::UserInput
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match self.mode {
            InputMode::Selecting => self.handle_selecting_key(key),
            InputMode::OtherInput => self.handle_other_input_key(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> ViewAction {
        if self.mode != InputMode::Selecting {
            return ViewAction::None;
        }
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
                let selected = self.row_hitboxes.borrow().iter().find_map(|(index, rect)| {
                    rect.contains(ratatui::layout::Position::new(mouse.column, mouse.row))
                        .then_some(*index)
                });
                if let Some(selected) = selected {
                    self.selected = selected;
                    self.activate_or_confirm_selection()
                } else {
                    ViewAction::None
                }
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let question = self.current_question();
        let total = self.request.questions.len();
        let title = format!(
            "{} ({}/{})",
            question.header,
            self.question_index + 1,
            total
        );

        let mut lines: Vec<Line> = Vec::new();
        let mut selectable_lines = Vec::new();
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
            selectable_lines.push((idx, lines.len()));
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
            selectable_lines.push((other_index, lines.len()));
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
            selectable_lines.push((confirm_index, lines.len()));
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

        let mut inner = if self.uses_full_screen_room(area) {
            render_full_screen_room(area, buf, title)
        } else {
            render_bottom_sheet(area, buf, self.sheet_height(area), title)
        };
        let hints = if self.mode == InputMode::OtherInput {
            vec![
                ActionHint::new("Enter", tr(MessageId::UserInputSubmit)),
                ActionHint::new("Esc", tr(MessageId::UserInputBack)),
            ]
        } else {
            let opt_count = self.option_count();
            let quick_pick_label = if opt_count <= 9 {
                format!("1-{opt_count}")
            } else {
                tr(MessageId::UserInputDigit).into_owned()
            };
            if self.is_multi_select() {
                vec![
                    ActionHint::new(quick_pick_label, tr(MessageId::UserInputMove)),
                    ActionHint::new("Space", tr(MessageId::UserInputToggle)),
                    ActionHint::new("Enter", tr(MessageId::UserInputToggleConfirm)),
                    ActionHint::new("Esc", tr(MessageId::UserInputCancel)),
                ]
            } else {
                vec![
                    ActionHint::new(quick_pick_label, tr(MessageId::UserInputQuickPick)),
                    ActionHint::new("↑/↓", tr(MessageId::UserInputMove)),
                    ActionHint::new("Enter", tr(MessageId::UserInputConfirmSelection)),
                    ActionHint::new("Esc", tr(MessageId::UserInputCancel)),
                ]
            }
        };
        inner = render_action_footer(inner, buf, &hints);
        let visible_rows = usize::from(inner.height);
        let selected_line = selectable_lines
            .iter()
            .find_map(|(index, line)| (*index == self.selected).then_some(*line))
            .unwrap_or(0);
        let scroll = selected_line
            .saturating_sub(visible_rows.saturating_sub(2))
            .min(lines.len().saturating_sub(visible_rows));
        let content = render_panel_scroll_rail(inner, buf, lines.len(), scroll, visible_rows, true);
        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));
        paragraph.render(content, buf);

        let hitboxes = selectable_lines
            .into_iter()
            .filter_map(|(index, line)| {
                let visible = line.checked_sub(scroll)?;
                if visible >= usize::from(content.height) {
                    return None;
                }
                Some((
                    index,
                    Rect {
                        x: content.x,
                        y: content
                            .y
                            .saturating_add(u16::try_from(visible).unwrap_or(u16::MAX)),
                        width: content.width,
                        height: 2.min(
                            content
                                .height
                                .saturating_sub(u16::try_from(visible).unwrap_or(u16::MAX)),
                        ),
                    },
                ))
            })
            .collect();
        *self.row_hitboxes.borrow_mut() = hitboxes;
    }

    fn occupied_region(&self, area: Rect) -> Rect {
        if self.uses_full_screen_room(area) {
            area
        } else {
            bottom_sheet_rect(area, self.sheet_height(area))
        }
    }
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
    fn user_input_sheet_calls_out_required_action_and_controls() {
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
    fn user_input_sheet_renders_custom_response_state() {
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
    fn user_input_sheet_hides_other_row_when_free_text_disabled() {
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
    fn user_input_sheet_renders_multi_select_ticks_and_confirm() {
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
    fn user_input_sheet_respects_80_and_120_column_frames() {
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

    #[test]
    fn short_input_is_a_bottom_sheet_and_long_input_uses_the_same_full_screen_room() {
        let area = Rect::new(0, 0, 100, 32);
        let short = sample_view();
        let sheet = short.occupied_region(area);
        assert_eq!(sheet.x, 0);
        assert_eq!(sheet.width, area.width);
        assert!(sheet.y > 0, "short input must leave transcript visible");
        assert_eq!(sheet.bottom(), area.bottom());

        let mut long = sample_view();
        long.request.questions[0].question = "long bounded question ".repeat(120);
        assert_eq!(long.occupied_region(area), area);
        let rendered = render_view(&long, area.width, area.height);
        assert!(rendered.contains("Confirm"));
        assert!(rendered.contains("需要你的操作"));
    }

    #[test]
    fn keyboard_and_mouse_choose_the_same_user_input_answer() {
        let mut keyboard = sample_view();
        let _ = keyboard.handle_key(KeyEvent::from(KeyCode::Down));
        let keyboard_action = keyboard.handle_key(KeyEvent::from(KeyCode::Enter));

        let mut mouse = sample_view();
        let area = Rect::new(0, 0, 100, 32);
        mouse.render(area, &mut Buffer::empty(area));
        let row = mouse
            .row_hitboxes
            .borrow()
            .iter()
            .find_map(|(index, rect)| (*index == 1).then_some(*rect))
            .expect("second answer hitbox");
        let mouse_action = mouse.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.x,
            row: row.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });

        assert_eq!(
            format!("{keyboard_action:?}"),
            format!("{mouse_action:?}"),
            "keyboard and mouse must emit the same canonical answer"
        );
    }
}
