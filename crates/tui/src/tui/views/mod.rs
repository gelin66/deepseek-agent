use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
};
use std::borrow::Cow;
use std::fmt;
use unicode_width::UnicodeWidthStr;

use dse_protocol::agent_runtime::UserInteractionResponse as UserInputResponse;

use crate::palette;
use crate::tui::approval::ReviewDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondarySurfaceKind {
    Approval,
    Permission,
    UserInput,
    Pager,
}

/// Bottom-anchored secondary surface. The transcript above remains visible.
#[must_use]
pub(crate) fn bottom_sheet_rect(area: Rect, desired_height: u16) -> Rect {
    let height = desired_height.clamp(1, area.height.max(1));
    Rect {
        x: area.x,
        y: area.bottom().saturating_sub(height),
        width: area.width,
        height,
    }
}

/// Paint a bottom sheet with one title rule and return its content body.
pub(crate) fn render_bottom_sheet(
    area: Rect,
    buf: &mut Buffer,
    desired_height: u16,
    title: impl Into<String>,
) -> Rect {
    let surface = bottom_sheet_rect(area, desired_height);
    Clear.render(surface, buf);
    Block::default()
        .style(Style::default().bg(palette::DSE_BG))
        .render(surface, buf);
    let title_width = usize::from(surface.width.saturating_sub(4));
    let title = crate::tui::ui_text::semantic_truncate(&title.into(), title_width);
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(palette::DSE_ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::DSE_BG))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(surface);
    block.render(surface, buf);
    inner
}

/// Paint the one full-screen reading room used by onboarding and pagers.
pub(crate) fn render_full_screen_room(
    area: Rect,
    buf: &mut Buffer,
    title: impl Into<String>,
) -> Rect {
    Clear.render(area, buf);
    Block::default()
        .style(Style::default().bg(palette::DSE_BG))
        .render(area, buf);
    let title_width = usize::from(area.width.saturating_sub(4));
    let title = crate::tui::ui_text::semantic_truncate(&title.into(), title_width);
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(palette::DSE_ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::DSE_BG))
        .padding(Padding::new(1, 1, 1, 1));
    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

/// Paint a scrollbar on the exact right edge of the panel it controls and
/// return the content rect with that rail reserved. Nothing is drawn when all
/// rows fit, so narrow surfaces do not spend a column on a fictional control.
pub(crate) fn render_panel_scroll_rail(
    area: Rect,
    buf: &mut Buffer,
    total_rows: usize,
    offset: usize,
    visible_rows: usize,
    focused: bool,
) -> Rect {
    if area.width < 2 || area.height == 0 || total_rows <= visible_rows.max(1) {
        return area;
    }
    let rail_x = area.right().saturating_sub(1);
    let rail_height = usize::from(area.height);
    let visible = visible_rows.max(1).min(total_rows);
    let thumb_height = ((rail_height * visible).div_ceil(total_rows)).clamp(1, rail_height);
    let max_offset = total_rows.saturating_sub(visible);
    let travel = rail_height.saturating_sub(thumb_height);
    let thumb_top = travel
        .saturating_mul(offset.min(max_offset))
        .checked_div(max_offset)
        .unwrap_or(0);
    let thumb_color = if focused {
        palette::TEXT_MUTED
    } else {
        palette::TEXT_DIM
    };
    for local_y in 0..area.height {
        let y = area.y.saturating_add(local_y);
        let local = usize::from(local_y);
        let is_thumb = local >= thumb_top && local < thumb_top + thumb_height;
        buf[(rail_x, y)]
            .set_symbol(if is_thumb { "█" } else { "│" })
            .set_style(Style::default().fg(if is_thumb {
                thumb_color
            } else {
                palette::BORDER_COLOR
            }));
    }
    Rect {
        width: area.width.saturating_sub(1),
        ..area
    }
}

fn render_surface_backdrop(area: Rect, buf: &mut Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)]
                .set_symbol(" ")
                .set_style(Style::default().bg(palette::DSE_BG));
        }
    }
}

/// A single key/label hint shown in a secondary surface's action footer.
///
/// Footers built from `ActionHint`s are laid out by [`action_footer_lines`],
/// which wraps to additional rows instead of letting an action run off the
/// right edge of the surface — the core overflow bug behind #3732. Use this for
/// action/navigation hints; truncate only identifiers/paths/hashes elsewhere.
pub(crate) struct ActionHint {
    key: Cow<'static, str>,
    label: Cow<'static, str>,
}

impl ActionHint {
    pub(crate) fn new(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }

    /// Display columns this hint occupies: ` key ` (key padded by a space on
    /// each side) followed by the label.
    fn width(&self) -> usize {
        UnicodeWidthStr::width(self.key.as_ref()) + 2 + UnicodeWidthStr::width(self.label.as_ref())
    }

    fn spans(&self) -> [Span<'static>; 2] {
        [
            Span::styled(
                format!(" {} ", self.key),
                Style::default()
                    .fg(palette::DSE_INFO)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                self.label.clone().into_owned(),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]
    }
}

/// Lay out action hints into one or more lines that each fit within `width`.
///
/// Hints are packed greedily; when the next hint would overflow the current row
/// the layout starts a new row rather than truncating. No action is ever
/// dropped or clipped (a single hint wider than `width` is emitted alone, which
/// only happens at degenerate widths below the surface minimums). This is the
/// shared replacement for the single-line `title_bottom` footers that silently
/// pushed actions off-screen.
pub(crate) fn action_footer_lines(hints: &[ActionHint], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if hints.is_empty() || width == 0 {
        return Vec::new();
    }
    const GAP: usize = 1;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    for hint in hints {
        let hint_width = hint.width();
        let needed = if current.is_empty() {
            hint_width
        } else {
            current_width + GAP + hint_width
        };
        if !current.is_empty() && needed > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            current_width = 0;
        }
        if !current.is_empty() {
            current.push(Span::raw(" ".repeat(GAP)));
            current_width += GAP;
        }
        current.extend(hint.spans());
        current_width += hint_width;
    }
    if !current.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

/// Reserve `lines` worth of rows at the bottom of `inner`, paint them, and
/// return the content area that remains above. Shared by the action-hint and
/// free-text surface footers.
fn place_footer_lines(inner: Rect, buf: &mut Buffer, lines: Vec<Line<'static>>) -> Rect {
    if lines.is_empty() || inner.height == 0 {
        return inner;
    }
    let footer_height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(inner.height);
    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - footer_height,
        width: inner.width,
        height: footer_height,
    };
    Paragraph::new(lines).render(footer_area, buf);
    Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height - footer_height,
    }
}

/// Render a wrapping action footer anchored to the bottom of `inner` and
/// return the content area that remains above it.
///
/// Secondary surfaces call this after painting their block so the footer reserves exactly
/// as many rows as it needs (bounded by the available height) and the body
/// fills the rest. Centralizing it keeps every action row visible and
/// reachable at narrow widths.
pub(crate) fn render_action_footer(inner: Rect, buf: &mut Buffer, hints: &[ActionHint]) -> Rect {
    let lines = action_footer_lines(hints, inner.width);
    place_footer_lines(inner, buf, lines)
}

#[derive(Debug, Clone)]
pub enum ViewEvent {
    OpenTextPager {
        title: String,
        content: String,
    },
    ApprovalDecision {
        interaction_id: String,
        decision: ReviewDecision,
    },
    UserInputSubmitted {
        tool_id: String,
        response: UserInputResponse,
    },
    UserInputCancelled {
        tool_id: String,
    },
    /// Emitted by the pager (`c` / `y`) to copy its body to the system
    /// clipboard. The host handler writes via `app.clipboard` and surfaces a
    /// status message — secondary surfaces cannot reach `app` directly. `label` is
    /// the noun shown in the success / failure status.
    CopyToClipboard {
        text: String,
        label: String,
    },
    PermissionSelected {
        mode: dse_protocol::agent_runtime::RunPermissionMode,
    },
}

#[derive(Debug, Clone)]
pub enum ViewAction {
    None,
    Close,
    Emit(ViewEvent),
    EmitAndClose(ViewEvent),
}

pub trait SecondarySurface {
    fn kind(&self) -> SecondarySurfaceKind;
    fn handle_key(&mut self, key: KeyEvent) -> ViewAction;
    /// Returns `true` if the surface consumed the paste; `false` to let the
    /// host route the text elsewhere (e.g. drop it because a secondary surface is open,
    /// or insert it into the composer when no surface wants it). The default
    /// is `false` so surfaces that don't care about paste don't silently
    /// swallow Cmd-V.
    fn handle_paste(&mut self, _text: &str) -> bool {
        false
    }
    fn handle_mouse(&mut self, _mouse: MouseEvent) -> ViewAction {
        ViewAction::None
    }
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// The region this secondary surface actually paints within the full frame `area`.
    ///
    /// Defaults to the whole frame for full-screen rooms. Inline interruptions
    /// and bottom sheets override this so the transcript above stays visible.
    /// The returned rect MUST match the region the surface renders into, or the
    /// dim and the painted content will disagree.
    fn occupied_region(&self, area: Rect) -> Rect {
        area
    }
}

#[derive(Default)]
pub struct ViewStack {
    views: Vec<Box<dyn SecondarySurface>>,
}

impl ViewStack {
    pub fn new() -> Self {
        Self { views: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn top_kind(&self) -> Option<SecondarySurfaceKind> {
        self.views.last().map(|view| view.kind())
    }

    pub fn push<V: SecondarySurface + 'static>(&mut self, view: V) {
        let kind = view.kind();
        let contract = crate::tui::surface_system::for_secondary_surface(kind);
        self.views.push(Box::new(view));
        tracing::debug!(
            target: "dse_tui::view_stack",
            action = "push",
            kind = ?kind,
            surface = ?contract.surface,
            targets = ?contract.targets,
            opener = contract.opener,
            state_source = contract.state_source,
            exit_action = contract.exit_action,
            legacy_deletion_point = contract.legacy_deletion_point,
            depth = self.views.len(),
            "view pushed"
        );
    }

    pub fn pop(&mut self) -> Option<Box<dyn SecondarySurface>> {
        let popped = self.views.pop();
        if let Some(view) = popped.as_ref() {
            tracing::debug!(target: "dse_tui::view_stack", action = "pop", kind = ?view.kind(), depth = self.views.len(), "view popped");
        }
        popped
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Dim each view's own occupied region rather than the whole frame, so
        // an inline surface (the approval prompt) leaves the transcript above it
        // visible instead of blacking out the screen. Full-screen rooms keep
        // the default `occupied_region` of the entire frame, so their backdrop
        // is unchanged.
        for view in &self.views {
            let region = view.occupied_region(area);
            crate::tui::osc8::overlay_frame_links(region, Vec::new());
            render_surface_backdrop(region, buf);
            view.render(area, buf);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<ViewEvent> {
        let action = self
            .views
            .last_mut()
            .map(|view| view.handle_key(key))
            .unwrap_or(ViewAction::None);
        self.apply_action(action)
    }

    pub fn handle_paste(&mut self, text: &str) -> bool {
        self.views
            .last_mut()
            .map(|view| view.handle_paste(text))
            .unwrap_or(false)
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<ViewEvent> {
        let action = self
            .views
            .last_mut()
            .map(|view| view.handle_mouse(mouse))
            .unwrap_or(ViewAction::None);
        self.apply_action(action)
    }

    fn apply_action(&mut self, action: ViewAction) -> Vec<ViewEvent> {
        let mut events = Vec::new();
        match action {
            ViewAction::None => {}
            ViewAction::Close => {
                if let Some(view) = self.views.pop() {
                    tracing::debug!(target: "dse_tui::view_stack", action = "close", kind = ?view.kind(), depth = self.views.len(), "view closed via action");
                }
            }
            ViewAction::Emit(event) => {
                events.push(event);
            }
            ViewAction::EmitAndClose(event) => {
                events.push(event);
                if let Some(view) = self.views.pop() {
                    tracing::debug!(target: "dse_tui::view_stack", action = "emit_and_close", kind = ?view.kind(), depth = self.views.len(), "view closed via action");
                }
            }
        }
        events
    }
}

impl fmt::Debug for ViewStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewStack")
            .field("len", &self.views.len())
            .field("top", &self.top_kind())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionHint, SecondarySurface, SecondarySurfaceKind, ViewAction, ViewStack,
        action_footer_lines, render_action_footer, render_full_screen_room,
    };
    use crate::palette;
    use crossterm::event::KeyEvent;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
    };
    #[test]
    fn action_footer_wraps_instead_of_overflowing() {
        let hints = [
            ActionHint::new("↑↓", "move"),
            ActionHint::new("a-z", "jump"),
            ActionHint::new("Enter", "apply"),
            ActionHint::new("R", "edit key"),
            ActionHint::new("M", "models"),
            ActionHint::new("Esc", "cancel"),
        ];

        // Wide enough for a single row.
        let wide = action_footer_lines(&hints, 120);
        assert_eq!(wide.len(), 1);
        assert!(wide[0].width() <= 120);

        // Narrow forces wrapping but never truncates: every action survives and
        // no produced line exceeds the available width.
        let narrow = action_footer_lines(&hints, 28);
        assert!(narrow.len() >= 2, "narrow footer should wrap to >1 row");
        for line in &narrow {
            assert!(
                line.width() <= 28,
                "wrapped footer row overflows: {} cols",
                line.width()
            );
        }
        let joined: String = narrow
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        for label in ["move", "jump", "apply", "edit key", "models", "cancel"] {
            assert!(joined.contains(label), "footer dropped action: {label}");
        }
    }

    #[test]
    fn render_action_footer_reserves_rows_and_returns_body() {
        let inner = Rect::new(2, 2, 40, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 44, 14));
        let hints = [
            ActionHint::new("Enter", "save"),
            ActionHint::new("Esc", "cancel"),
        ];
        let body = render_action_footer(inner, &mut buf, &hints);
        // The footer (a single row at this width) is reserved off the bottom and
        // the body fills the rows above it.
        assert_eq!(body.y, inner.y);
        assert_eq!(body.height, inner.height - 1);
        assert_eq!(body.y + body.height, inner.y + inner.height - 1);
    }

    #[test]
    fn full_screen_room_ellipsizes_narrow_titles() {
        let area = Rect::new(0, 0, 24, 8);
        let mut buf = Buffer::empty(area);
        render_full_screen_room(area, &mut buf, "Help — Concepts, commands, and keybindings");
        let top = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            top.contains('…'),
            "narrow title should signal truncation: {top}"
        );
    }

    /// A secondary surface that doesn't override `handle_paste` must report
    /// "not consumed" so the host can fall through to the composer.
    /// Regression: views/mod.rs previously inverted the boolean, swallowing
    /// every Cmd-V while any surface was on top.
    #[test]
    fn default_secondary_surface_does_not_consume_paste() {
        let mut stack = ViewStack::new();
        stack.push(BareSurface);
        assert!(!stack.handle_paste("hello"));
        assert_eq!(stack.top_kind(), Some(SecondarySurfaceKind::Pager));
    }

    struct BareSurface;

    impl SecondarySurface for BareSurface {
        fn kind(&self) -> SecondarySurfaceKind {
            SecondarySurfaceKind::Pager
        }

        fn handle_key(&mut self, _key: KeyEvent) -> ViewAction {
            ViewAction::None
        }

        fn render(&self, area: Rect, buf: &mut Buffer) {
            let x = area.x + area.width / 2;
            let y = area.y + area.height / 2;
            buf[(x, y)]
                .set_symbol("M")
                .set_style(Style::default().fg(Color::White).bg(Color::Red));
        }
    }

    #[test]
    fn view_stack_paints_opaque_backdrop_before_surface() {
        let area = Rect::new(0, 0, 24, 8);
        let surface_x = area.x + area.width / 2;
        let surface_y = area.y + area.height / 2;
        let mut buf = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)]
                    .set_symbol("X")
                    .set_style(Style::default().fg(Color::Red).bg(Color::Blue));
            }
        }

        let mut stack = ViewStack::new();
        stack.push(BareSurface);
        stack.render(area, &mut buf);

        assert_eq!(buf[(surface_x, surface_y)].symbol(), "M");
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if x == surface_x && y == surface_y {
                    continue;
                }
                let cell = &buf[(x, y)];
                assert_eq!(
                    cell.symbol(),
                    " ",
                    "stale glyph at ({x},{y}) must be cleared"
                );
                assert_eq!(
                    cell.bg,
                    palette::DSE_BG,
                    "backdrop at ({x},{y}) must be opaque"
                );
            }
        }
    }

    #[test]
    fn view_stack_masks_links_behind_opaque_surfaces() {
        let area = Rect::new(0, 0, 24, 8);
        crate::tui::osc8::set_frame_links(vec![crate::tui::osc8::LinkRegion {
            row: 3,
            col_start: 2,
            col_end: 18,
            target: "https://example.invalid/under-surface".to_string(),
        }]);
        let mut stack = ViewStack::new();
        stack.push(BareSurface);
        stack.render(area, &mut Buffer::empty(area));
        assert!(crate::tui::osc8::take_frame_links().is_empty());
    }
}
