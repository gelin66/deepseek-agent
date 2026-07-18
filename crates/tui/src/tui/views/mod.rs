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

use crate::palette;
use crate::tools::UserInputResponse;
use crate::tui::approval::{ElevationOption, ReviewDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    Approval,
    Elevation,
    UserInput,
    Pager,
    LiveTranscript,
}

/// Clear and paint a modal popup with an opaque surface.
///
/// Older modals often called `Clear` only, which left reset-background blank
/// cells that could read as translucent on terminals with a non-default app
/// background. This helper makes the popup area explicit and keeps the small
/// shadow from inheriting stale transcript glyphs.
pub(crate) fn render_modal_surface(area: Rect, popup_area: Rect, buf: &mut Buffer) {
    let shadow_x = popup_area.x.saturating_add(1);
    let shadow_y = popup_area.y.saturating_add(1);
    let shadow_right = area.x.saturating_add(area.width);
    let shadow_bottom = area.y.saturating_add(area.height);
    let shadow_width = popup_area.width.min(shadow_right.saturating_sub(shadow_x));
    let shadow_height = popup_area
        .height
        .min(shadow_bottom.saturating_sub(shadow_y));

    if shadow_width > 0 && shadow_height > 0 {
        Block::default()
            .style(Style::default().bg(palette::SURFACE_ELEVATED))
            .render(
                Rect {
                    x: shadow_x,
                    y: shadow_y,
                    width: shadow_width,
                    height: shadow_height,
                },
                buf,
            );
    }

    Clear.render(popup_area, buf);
    Block::default()
        .style(Style::default().bg(palette::WHALE_BG))
        .render(popup_area, buf);
}

/// Paint a full-screen underwater instrument surface and return its body.
///
/// Secondary rooms use one title hairline and one bottom action rail instead
/// of a centered generic card. A one-cell outer margin is retained when the
/// terminal can afford it; compact panes use every cell.
pub(crate) fn render_underwater_surface(
    area: Rect,
    buf: &mut Buffer,
    title: impl Into<String>,
) -> Rect {
    let margin_x = u16::from(area.width >= 44);
    let margin_y = u16::from(area.height >= 14);
    let surface = Rect {
        x: area.x.saturating_add(margin_x),
        y: area.y.saturating_add(margin_y),
        width: area.width.saturating_sub(margin_x.saturating_mul(2)),
        height: area.height.saturating_sub(margin_y.saturating_mul(2)),
    };
    Clear.render(area, buf);
    Block::default()
        .style(Style::default().bg(palette::WHALE_BG))
        .render(area, buf);
    // Ratatui clips long block titles at the border edge without signalling
    // that anything is missing. Reserve the corner cells and semantic-ellipsis
    // the title so compact terminals still read as intentional instruments.
    let title_width = usize::from(surface.width.saturating_sub(4));
    let title = crate::tui::ui_text::semantic_truncate(&title.into(), title_width);
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(palette::WHALE_ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(palette::BORDER_COLOR))
        .style(Style::default().bg(palette::WHALE_BG))
        .padding(Padding::new(1, 1, 1, 1));
    let inner = block.inner(surface);
    block.render(surface, buf);
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

fn render_modal_backdrop(area: Rect, buf: &mut Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)]
                .set_symbol(" ")
                .set_style(Style::default().bg(palette::WHALE_BG));
        }
    }
}

/// A single key/label hint shown in a modal's action footer.
///
/// Footers built from `ActionHint`s are laid out by [`action_footer_lines`],
/// which wraps to additional rows instead of letting an action run off the
/// right edge of the modal — the core overflow bug behind #3732. Use this for
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
                    .fg(palette::WHALE_INFO)
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
/// only happens at degenerate widths below the modal minimums). This is the
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
/// free-text modal footers.
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
/// Modals call this after painting their block so the footer reserves exactly
/// as many rows as it needs (bounded by the available height) and the body
/// fills the rest. Centralizing it keeps every modal's action row visible and
/// reachable at narrow widths.
pub(crate) fn render_modal_footer(inner: Rect, buf: &mut Buffer, hints: &[ActionHint]) -> Rect {
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
        tool_id: String,
        tool_name: String,
        decision: ReviewDecision,
        timed_out: bool,
        /// Exact-argument fingerprint, used to scope *denials* (#1617).
        approval_key: String,
        /// Lossy / arity-aware fingerprint, used to scope *approvals*.
        approval_grouping_key: String,
        /// Ask-only permission rules to append when the decision approves.
        persistent_ask_rules: Vec<codewhale_config::ToolAskRule>,
    },
    ElevationDecision {
        tool_id: String,
        tool_name: String,
        option: ElevationOption,
    },
    UserInputSubmitted {
        tool_id: String,
        response: UserInputResponse,
    },
    UserInputCancelled {
        tool_id: String,
    },
    SidebarAgentCancel {
        agent_id: String,
    },
    /// Emitted by the fleet setup Review step (`m`) to ask the configured
    /// model to draft the agent profile the wizard describes. The host
    /// performs the one-shot call, pushes the sanitized/bounded draft back
    /// into the wizard, and opens the rendered-TOML preview; on failure it
    /// reports why and the manual authoring flow stands. Nothing is
    /// persisted by this event.
    FleetProfileModelDraftRequested {
        role: String,
        /// Target model for the worker: a concrete model id, or "inherit".
        model: String,
        /// Canonical provider id for a concrete cross-provider route pick, or
        /// `None` for `inherit` (#4093). Carried so the model-drafted profile
        /// keeps the picked provider instead of collapsing to an ambiguous,
        /// provider-scoped profile — the exact bug #4093 fixes.
        provider: Option<String>,
        /// Canonical reasoning tier selected by the wizard, or `None` for
        /// inherit (#4137). Carried with the async draft for the same reason
        /// as `provider`: the ratified profile must preserve the operator's
        /// explicit choice, not whatever the model echoed.
        reasoning_effort: Option<String>,
    },
    /// Emitted by the fleet setup Review step after the user previewed a
    /// model-drafted profile and pressed the explicit ratify key. The host
    /// renders TOML deterministically from the validated draft and persists
    /// it atomically under `.codewhale/agents/`.
    FleetProfileDraftCommitRequested {
        draft: Box<crate::fleet::profile::FleetProfileDraft>,
    },
    /// Emitted by the pager (`c` / `y`) to copy its body to the system
    /// clipboard. The host handler writes via `app.clipboard` and surfaces a
    /// status message — modal views cannot reach `app` directly. `label` is
    /// the noun shown in the success / failure status.
    CopyToClipboard {
        text: String,
        label: String,
    },
}

#[derive(Debug, Clone)]
pub enum ViewAction {
    None,
    Close,
    Emit(ViewEvent),
    EmitAndClose(ViewEvent),
}

pub trait ModalView: std::any::Any {
    fn kind(&self) -> ModalKind;
    fn handle_key(&mut self, key: KeyEvent) -> ViewAction;
    /// Returns `true` if the modal consumed the paste; `false` to let the
    /// host route the text elsewhere (e.g. drop it because a modal is open,
    /// or insert it into the composer when no modal wants it). The default
    /// is `false` so modals that don't care about paste don't silently
    /// swallow Cmd-V.
    fn handle_paste(&mut self, _text: &str) -> bool {
        false
    }
    fn handle_mouse(&mut self, _mouse: MouseEvent) -> ViewAction {
        ViewAction::None
    }
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// The region this modal actually paints within the full frame `area`.
    ///
    /// Defaults to the whole frame, which is the legacy full-screen overlay
    /// behaviour every picker/menu still relies on. Inline modals (the
    /// approval prompt) override this to return a bottom-anchored band so the
    /// backdrop only dims their strip and the transcript above stays visible.
    /// The returned rect MUST match the region the modal renders into, or the
    /// dim and the painted content will disagree.
    fn occupied_region(&self, area: Rect) -> Rect {
        area
    }
    fn tick(&mut self) -> ViewAction {
        ViewAction::None
    }
    /// Erased downcast hook for views that need a typed reference back from
    /// the boxed trait object (e.g. the live transcript overlay needs `&mut`
    /// access from outside the trait so it can refresh its snapshot of the
    /// app's transcript state right before render).
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[derive(Default)]
pub struct ViewStack {
    views: Vec<Box<dyn ModalView>>,
}

impl ViewStack {
    pub fn new() -> Self {
        Self { views: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn top_kind(&self) -> Option<ModalKind> {
        self.views.last().map(|view| view.kind())
    }

    pub fn push<V: ModalView + 'static>(&mut self, view: V) {
        let kind = view.kind();
        self.views.push(Box::new(view));
        tracing::debug!(target: "codewhale_tui::view_stack", action = "push", kind = ?kind, depth = self.views.len(), "view pushed");
    }

    /// Push an already-boxed view back onto the stack. Used by call sites
    /// that pop a view, mutate it externally, and need to restore it without
    /// the generic `push` re-boxing dance.
    pub fn push_boxed(&mut self, view: Box<dyn ModalView>) {
        let kind = view.kind();
        self.views.push(view);
        tracing::debug!(target: "codewhale_tui::view_stack", action = "push_boxed", kind = ?kind, depth = self.views.len(), "view pushed");
    }

    pub fn pop(&mut self) -> Option<Box<dyn ModalView>> {
        let popped = self.views.pop();
        if let Some(view) = popped.as_ref() {
            tracing::debug!(target: "codewhale_tui::view_stack", action = "pop", kind = ?view.kind(), depth = self.views.len(), "view popped");
        }
        popped
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Dim each view's own occupied region rather than the whole frame, so
        // an inline modal (the approval prompt) leaves the transcript above it
        // visible instead of blacking out the screen. Full-screen modals keep
        // the default `occupied_region` of the entire frame, so their backdrop
        // is unchanged.
        for view in &self.views {
            let region = view.occupied_region(area);
            crate::tui::osc8::overlay_frame_links(region, Vec::new());
            render_modal_backdrop(region, buf);
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

    pub fn tick(&mut self) -> Vec<ViewEvent> {
        let action = self
            .views
            .last_mut()
            .map(|view| view.tick())
            .unwrap_or(ViewAction::None);
        self.apply_action(action)
    }

    fn apply_action(&mut self, action: ViewAction) -> Vec<ViewEvent> {
        let mut events = Vec::new();
        match action {
            ViewAction::None => {}
            ViewAction::Close => {
                if let Some(view) = self.views.pop() {
                    tracing::debug!(target: "codewhale_tui::view_stack", action = "close", kind = ?view.kind(), depth = self.views.len(), "view closed via action");
                }
            }
            ViewAction::Emit(event) => {
                events.push(event);
            }
            ViewAction::EmitAndClose(event) => {
                events.push(event);
                if let Some(view) = self.views.pop() {
                    tracing::debug!(target: "codewhale_tui::view_stack", action = "emit_and_close", kind = ?view.kind(), depth = self.views.len(), "view closed via action");
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
        ActionHint, ModalKind, ModalView, ViewAction, ViewStack, action_footer_lines,
        render_modal_footer, render_underwater_surface,
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
    fn render_modal_footer_reserves_rows_and_returns_body() {
        let inner = Rect::new(2, 2, 40, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 44, 14));
        let hints = [
            ActionHint::new("Enter", "save"),
            ActionHint::new("Esc", "cancel"),
        ];
        let body = render_modal_footer(inner, &mut buf, &hints);
        // The footer (a single row at this width) is reserved off the bottom and
        // the body fills the rows above it.
        assert_eq!(body.y, inner.y);
        assert_eq!(body.height, inner.height - 1);
        assert_eq!(body.y + body.height, inner.y + inner.height - 1);
    }

    #[test]
    fn underwater_surface_ellipsizes_narrow_titles() {
        let area = Rect::new(0, 0, 24, 8);
        let mut buf = Buffer::empty(area);
        render_underwater_surface(area, &mut buf, "Help — Concepts, commands, and keybindings");
        let top = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            top.contains('…'),
            "narrow title should signal truncation: {top}"
        );
    }

    /// A modal that doesn't override `handle_paste` must report
    /// "not consumed" so the host can fall through to the composer.
    /// Regression: views/mod.rs previously inverted the boolean, swallowing
    /// every Cmd-V while any modal was on top.
    #[test]
    fn default_modal_does_not_consume_paste() {
        let mut stack = ViewStack::new();
        stack.push(BareModal);
        assert!(!stack.handle_paste("hello"));
        assert_eq!(stack.top_kind(), Some(ModalKind::Pager));
    }

    struct BareModal;

    impl ModalView for BareModal {
        fn kind(&self) -> ModalKind {
            ModalKind::Pager
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

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn view_stack_paints_opaque_backdrop_before_modal() {
        let area = Rect::new(0, 0, 24, 8);
        let modal_x = area.x + area.width / 2;
        let modal_y = area.y + area.height / 2;
        let mut buf = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)]
                    .set_symbol("X")
                    .set_style(Style::default().fg(Color::Red).bg(Color::Blue));
            }
        }

        let mut stack = ViewStack::new();
        stack.push(BareModal);
        stack.render(area, &mut buf);

        assert_eq!(buf[(modal_x, modal_y)].symbol(), "M");
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if x == modal_x && y == modal_y {
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
                    palette::WHALE_BG,
                    "backdrop at ({x},{y}) must be opaque"
                );
            }
        }
    }

    #[test]
    fn view_stack_masks_links_behind_opaque_modals() {
        let area = Rect::new(0, 0, 24, 8);
        crate::tui::osc8::set_frame_links(vec![crate::tui::osc8::LinkRegion {
            row: 3,
            col_start: 2,
            col_end: 18,
            target: "https://example.invalid/under-modal".to_string(),
        }]);
        let mut stack = ViewStack::new();
        stack.push(BareModal);
        stack.render(area, &mut Buffer::empty(area));
        assert!(crate::tui::osc8::take_frame_links().is_empty());
    }
}
