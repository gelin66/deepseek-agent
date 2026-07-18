//! TUI rendering helpers for chat history and tool output.

use std::time::Instant;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::deepseek_theme::active_theme;
use crate::localization::{MessageId, tr};
use crate::palette;
use crate::tui::app::TranscriptSpacing;
use crate::tui::diff_render;

mod agent_activity;
mod archived_context;
mod constants;
mod message;
mod thinking;
mod tool_output;
mod tool_run;

use archived_context::render_archived_context;
use constants::{
    ASSISTANT_GLYPH, TOOL_CARD_SUMMARY_LINES, TOOL_DONE_SYMBOL, TOOL_FAILED_SYMBOL,
    TOOL_HEADER_SUMMARY_LIMIT, TOOL_OUTPUT_LINE_LIMIT, TRANSCRIPT_RAIL, USER_GLYPH,
};
use message::{
    RenderedTranscriptLine, assistant_label_style_for, message_body_style, render_message,
    render_message_with_metadata, render_user_message, system_body_style, system_label_style,
    tag_lines_without_links,
};
use thinking::{render_hidden_thinking_activity, render_thinking};
use tool_output::{render_tool_output_mode, wrap_text};

#[cfg(test)]
use agent_activity::extract_agent_id;
#[cfg(test)]
use tool_run::ToolRunActivitySummary;
pub use tool_run::{ToolRun, detect_tool_runs, tool_run_summary};

#[cfg(test)]
use thinking::{REASONING_CURSOR, REASONING_OPENER, REASONING_RAIL};
pub(crate) use tool_output::output_looks_like_diff;
pub use tool_output::{OutputRow, summarize_tool_args, summarize_tool_output};

/// Render mode controlling whether tool cells render their compact live form
/// or their uncapped transcript form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Live in-stream view: tool output may be summarized.
    Live,
    /// Full transcript view: every line of reasoning and tool output is
    /// emitted without caps.
    Transcript,
}

// === History Cells ===

/// Renderable history cell for user/assistant/system entries.
#[derive(Debug, Clone)]
pub enum HistoryCell {
    User {
        content: String,
    },
    Assistant {
        content: String,
        streaming: bool,
    },
    System {
        content: String,
    },
    Thinking {
        content: String,
        streaming: bool,
    },
    /// Archived context metadata or a local history-compaction placeholder.
    /// Rendered dimmed/italic with a level + range label.
    ArchivedContext {
        /// Seam level (1, 2, 3, or 0 for cycle-level).
        level: u8,
        /// Message range covered (e.g. "msg 0-128").
        range: String,
        /// Token estimate string (e.g. "~2500").
        tokens: String,
        /// Density label (e.g. "~2,500 tokens").
        density: String,
        /// Model that produced the summary.
        model: String,
        /// RFC 3339 timestamp.
        timestamp: String,
        /// The summary text content.
        summary: String,
    },
    Tool(GenericToolCell),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptRenderOptions {
    pub show_thinking: bool,
    pub show_tool_details: bool,
    pub calm_mode: bool,
    pub low_motion: bool,
    pub spacing: TranscriptSpacing,
}

impl Default for TranscriptRenderOptions {
    fn default() -> Self {
        Self {
            show_thinking: true,
            show_tool_details: true,
            calm_mode: false,
            low_motion: false,
            spacing: TranscriptSpacing::Comfortable,
        }
    }
}

impl HistoryCell {
    /// Render the cell into a set of terminal lines.
    ///
    /// This is the live-display path used by widgets that don't already pass
    /// `TranscriptRenderOptions`. Tool output is capped, but thinking is shown
    /// in full because callers using bare `lines()` historically expected the
    /// uncollapsed body. Callers that need configurable transcript rendering
    /// use `lines_with_options`.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            HistoryCell::User { content } => render_user_message(content, width),
            HistoryCell::Assistant { content, streaming } => render_message(
                ASSISTANT_GLYPH,
                assistant_label_style_for(*streaming, /*low_motion*/ false),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { content } => {
                if is_cycle_boundary(content) {
                    render_cycle_boundary(content, width)
                } else {
                    render_message(
                        &tr(MessageId::HistorySystemNoteLabel),
                        system_label_style(),
                        system_body_style(),
                        content,
                        width,
                    )
                }
            }
            HistoryCell::Thinking { content, streaming } => {
                render_thinking(content, width, *streaming, false)
            }
            HistoryCell::Tool(cell) => cell.lines_with_motion(width, false),
            HistoryCell::ArchivedContext { .. } => render_archived_context(self, width, false),
        }
    }

    pub fn lines_with_options(
        &self,
        width: u16,
        options: TranscriptRenderOptions,
    ) -> Vec<Line<'static>> {
        match self {
            HistoryCell::Thinking { streaming, .. } if !options.show_thinking => {
                if *streaming {
                    render_hidden_thinking_activity(width, options.low_motion)
                } else {
                    Vec::new()
                }
            }
            HistoryCell::Thinking { content, streaming } => {
                render_thinking(content, width, *streaming, options.low_motion)
            }
            HistoryCell::Tool(cell) if !options.show_tool_details && !cell.is_failed() => {
                let mut lines = cell.lines_with_motion(width, options.low_motion);
                if lines.len() > 2 {
                    lines.truncate(2);
                    lines.push(summary_notice_line(
                        "更多输出已折叠",
                        Style::default().fg(palette::TEXT_MUTED).italic(),
                    ));
                }
                lines
            }
            HistoryCell::Tool(cell) if options.calm_mode && !cell.is_failed() => {
                let mut lines = cell.lines_with_motion(width, options.low_motion);
                if lines.len() > TOOL_CARD_SUMMARY_LINES {
                    lines.truncate(TOOL_CARD_SUMMARY_LINES);
                    lines.push(summary_notice_line(
                        "更多输出已折叠",
                        Style::default().fg(palette::TEXT_MUTED).italic(),
                    ));
                }
                lines
            }
            HistoryCell::Tool(cell) => cell.lines_with_motion(width, options.low_motion),
            HistoryCell::User { content } => render_user_message(content, width),
            HistoryCell::Assistant { content, streaming } => render_message(
                ASSISTANT_GLYPH,
                assistant_label_style_for(*streaming, options.low_motion),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { .. } => self.lines(width),
            HistoryCell::ArchivedContext { .. } => {
                render_archived_context(self, width, options.low_motion)
            }
        }
    }

    pub(crate) fn lines_with_render_metadata(
        &self,
        width: u16,
        options: TranscriptRenderOptions,
    ) -> Vec<RenderedTranscriptLine> {
        match self {
            HistoryCell::User { content } => {
                tag_lines_without_links(render_user_message(content, width))
            }
            HistoryCell::Assistant { content, streaming } => render_message_with_metadata(
                ASSISTANT_GLYPH,
                assistant_label_style_for(*streaming, options.low_motion),
                message_body_style(),
                content,
                width,
            ),
            HistoryCell::System { content } if !is_cycle_boundary(content) => {
                render_message_with_metadata(
                    &tr(MessageId::HistorySystemNoteLabel),
                    system_label_style(),
                    system_body_style(),
                    content,
                    width,
                )
            }
            HistoryCell::Tool(_) => self
                .lines_with_options(width, options)
                .into_iter()
                .map(|line| RenderedTranscriptLine {
                    line,
                    links: Vec::new(),
                })
                .collect(),
            _ => tag_lines_without_links(self.lines_with_options(width, options)),
        }
    }

    /// Whether this cell is the continuation of a streaming assistant message.
    #[must_use]
    pub fn is_stream_continuation(&self) -> bool {
        matches!(
            self,
            HistoryCell::Assistant {
                streaming: true,
                ..
            }
        )
    }

    #[must_use]
    pub fn is_conversational(&self) -> bool {
        matches!(
            self,
            HistoryCell::User { .. } | HistoryCell::Assistant { .. } | HistoryCell::Thinking { .. }
        )
    }
}

// === Tool Cells ===

/// Overall status for a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Failed,
}

/// Canonical TUI projection for every runtime tool invocation.
#[derive(Debug, Clone)]
pub struct GenericToolCell {
    pub name: String,
    pub status: ToolStatus,
    pub input_summary: Option<String>,
    pub output: Option<String>,
    /// Optional list of per-child prompts. When populated (by any future
    /// fan-out tool), each prompt is shown on its own indented row instead
    /// of the inline `args:` summary. `None` for ordinary tools.
    pub prompts: Option<Vec<String>>,
    // --- Pre-computed render cache (populated once at cell creation) ---
    /// Cached output summary — avoids re-parsing JSON every frame.
    pub output_summary: Option<String>,
    /// Whether the output looks like a unified diff (cached after first check).
    pub is_diff: bool,
}

fn should_show_raw_tool_name(
    name: &str,
    family: crate::tui::widgets::tool_card::ToolFamily,
    mode: RenderMode,
) -> bool {
    matches!(mode, RenderMode::Transcript)
        || matches!(family, crate::tui::widgets::tool_card::ToolFamily::Generic)
        || name.starts_with("mcp_")
}

impl GenericToolCell {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.status == ToolStatus::Success
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.status == ToolStatus::Running
    }

    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.status == ToolStatus::Failed
    }

    /// Whether this cell should stay visible inside a dense tool run.
    #[must_use]
    pub fn is_collapsible_guard(&self) -> bool {
        self.is_running()
            || self.is_failed()
            || tool_run::tool_name_is_collapse_guard(&self.name)
            || self.is_diff
    }

    pub fn lines_with_motion(&self, width: u16, low_motion: bool) -> Vec<Line<'static>> {
        self.lines_with_mode(width, low_motion, RenderMode::Live)
    }

    /// Render the canonical tool cell into lines.
    ///
    /// `mode` controls multi-line output handling: `Live` caps at
    /// `TOOL_OUTPUT_LINE_LIMIT` rows with a "+N more" affordance;
    /// `Transcript` emits the full output.
    pub fn lines_with_mode(
        &self,
        width: u16,
        low_motion: bool,
        mode: RenderMode,
    ) -> Vec<Line<'static>> {
        if self.name == "activity_group" {
            return agent_activity::render_activity_group(self, width);
        }

        // Sub-agent launch already gets a dedicated `DelegateCard`
        // that owns the live action tree, status, and final summary (#4133).
        // Spawns therefore render nothing here in either mode — one visible
        // artifact per delegated unit. Inspection/join calls (peek/status/
        // wait) stay as a single compact line (#4112 dogfood A5).
        if self.name == "agent" {
            if agent_activity::is_agent_inspection(self) {
                return agent_activity::render_agent_compact(self, low_motion);
            }
            // Spawn / start / run: suppress the tool card entirely.
            return Vec::new();
        }

        // A call to a tool that doesn't exist carries exactly one useful
        // fact: the catalog error. The full name:/args:/result: block turns
        // each model slip into a four-line card (dogfood A5) — collapse it
        // to a single header line in both render modes.
        if self.status == ToolStatus::Failed
            && let Some(output) = self.output.as_deref()
            && output.contains("is not available in the current tool catalog")
        {
            let family = crate::tui::widgets::tool_card::tool_family_for_name(&self.name);
            let summary = truncate_text(output.trim(), 200);
            return wrap_card_rail(vec![render_tool_header_with_family_and_summary(
                family,
                Some(summary.as_str()),
                tool_status_label(self.status),
                self.status,
                None,
                low_motion,
            )]);
        }

        // Live mode stays calm: successful tool calls collapse to one header
        // line, and non-read in-flight tools do the same. Failures keep their
        // body visible because error output is the useful part.
        if matches!(mode, RenderMode::Live) {
            let family = crate::tui::widgets::tool_card::tool_family_for_name(&self.name);
            let is_read_family = matches!(
                family,
                crate::tui::widgets::tool_card::ToolFamily::Read
                    | crate::tui::widgets::tool_card::ToolFamily::Find
            );
            let should_collapse = self.status == ToolStatus::Success
                || (self.status != ToolStatus::Failed && !is_read_family);
            if should_collapse {
                let header_summary = crate::tui::widgets::tool_card::tool_header_summary_for_name(
                    &self.name,
                    self.input_summary.as_deref(),
                );
                return wrap_card_rail(vec![render_tool_header_with_family_and_summary(
                    family,
                    header_summary.as_deref(),
                    tool_status_label(self.status),
                    self.status,
                    None,
                    low_motion,
                )]);
            }
        }

        let mut lines = Vec::new();
        // Map the actual tool name (e.g. `agent`, `apply_patch`) to a
        // family rather than the catch-all `"Tool"` title — this is what
        // gives a `GenericToolCell` the right verb glyph (◐ delegate, ⋮⋮
        // fanout, etc.) instead of falling back to the neutral bullet.
        let family = crate::tui::widgets::tool_card::tool_family_for_name(&self.name);
        let header_summary = crate::tui::widgets::tool_card::tool_header_summary_for_name(
            &self.name,
            self.input_summary.as_deref(),
        );
        lines.push(render_tool_header_with_family_and_summary(
            family,
            header_summary.as_deref(),
            tool_status_label(self.status),
            self.status,
            None,
            low_motion,
        ));
        if should_show_raw_tool_name(&self.name, family, mode) {
            lines.extend(render_compact_kv(
                "name",
                &self.name,
                tool_value_style(),
                width,
            ));
        }

        // Prefer per-prompt rows over the generic args summary when the tool
        // exposes a list of child prompts. One row per child with a `[i]`
        // index makes the fan-out legible without expanding JSON.
        let show_prompts = matches!(self.status, ToolStatus::Running) || self.output.is_none();
        if show_prompts
            && let Some(prompts) = self.prompts.as_ref()
            && !prompts.is_empty()
        {
            for (idx, prompt) in prompts.iter().enumerate() {
                let label = if idx == 0 { "prompts" } else { "" };
                let value = format!("[{idx}] {}", truncate_text(prompt.trim(), 200));
                lines.extend(render_card_detail_line(
                    if label.is_empty() { None } else { Some(label) },
                    &value,
                    tool_value_style(),
                    width,
                ));
            }
        } else {
            let show_args = matches!(self.status, ToolStatus::Running | ToolStatus::Failed)
                || self.output.is_none();
            if show_args && let Some(summary) = self.input_summary.as_ref() {
                lines.extend(render_compact_kv(
                    "args",
                    summary,
                    tool_value_style(),
                    width,
                ));
            }
        }

        if let Some(output) = self.output.as_ref() {
            if self.is_diff {
                let diff_summary = diff_render::diff_summary_label(output);
                lines.push(render_tool_header_with_family_and_summary(
                    crate::tui::widgets::tool_card::ToolFamily::Patch,
                    diff_summary.as_deref(),
                    tool_status_label(self.status),
                    self.status,
                    None,
                    low_motion,
                ));
                lines.extend(diff_render::render_diff(output, width));
            } else {
                let output_mode =
                    if matches!(mode, RenderMode::Live) && self.status == ToolStatus::Failed {
                        RenderMode::Transcript
                    } else {
                        mode
                    };
                lines.extend(render_tool_output_mode(
                    output,
                    width,
                    TOOL_OUTPUT_LINE_LIMIT,
                    output_mode,
                ));
            }
        }
        wrap_card_rail(lines)
    }
}

fn render_compact_kv(label: &str, value: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    render_card_detail_line(Some(label.trim_end_matches(':')), value, style, width)
}

/// Wrap rendered tool-card lines with card-rail glyphs (╭ │ ╰).
/// First non-empty line gets `╭`, middle lines get `│`, last line gets `╰`.
/// Single-line cards get a single `─` prefix.
fn wrap_card_rail(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let n = lines.len();
    if n == 0 {
        return lines;
    }
    if n == 1 {
        lines[0].spans.insert(0, Span::raw("─ "));
        return lines;
    }
    for (i, line) in lines.iter_mut().enumerate() {
        let rail = if i == 0 {
            "\u{256D} " // ╭
        } else if i == n - 1 {
            "\u{2570} " // ╰
        } else {
            "\u{2502} " // │
        };
        line.spans.insert(0, Span::raw(rail));
    }
    lines
}

/// Detect whether a system message is a cycle-boundary announcement
/// (e.g. `─── cycle 0 → 1  (briefing: 2500 tokens) ───`).
fn is_cycle_boundary(content: &str) -> bool {
    content.contains("cycle")
}

/// Render a cycle-boundary system message with distinct visual styling (#395):
/// full-width line with primary accent text and bold weight, plus a thin
/// horizontal rule above for visual separation.
fn render_cycle_boundary(content: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default()
        .fg(palette::WHALE_ACCENT_PRIMARY)
        .add_modifier(Modifier::BOLD);
    let rule_style = Style::default().fg(palette::TEXT_DIM);
    let content_width = usize::from(width.saturating_sub(2).max(1));
    let mut lines = Vec::new();
    // Thin horizontal rule above for visual separation
    if width >= 4 {
        let rule = "\u{2500}".repeat(content_width);
        lines.push(Line::from(Span::styled(format!("  {rule}"), rule_style)));
    }
    // Cycle boundary text — just the content, full-width
    let rendered =
        crate::tui::markdown_render::render_markdown(content, content_width as u16, style);
    for line in rendered {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(line.spans);
        lines.push(Line::from(spans));
    }
    if lines.len() == 1 && width >= 4 {
        // Only the rule was added (unlikely), but add at least a spacer
        lines.push(Line::from(""));
    }
    lines
}

fn status_symbol(
    started_at: Option<Instant>,
    status: ToolStatus,
    low_motion: bool,
    family: crate::tui::widgets::tool_card::ToolFamily,
) -> String {
    match status {
        ToolStatus::Running if family == crate::tui::widgets::tool_card::ToolFamily::Verify => {
            crate::tui::spinner::verification_tick_frame(started_at, low_motion).to_string()
        }
        ToolStatus::Running => {
            crate::tui::spinner::braille_spinner_frame(started_at, low_motion).to_string()
        }
        ToolStatus::Success => TOOL_DONE_SYMBOL.to_string(),
        ToolStatus::Failed => TOOL_FAILED_SYMBOL.to_string(),
    }
}

fn summary_notice_line(text: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            TRANSCRIPT_RAIL.to_string(),
            Style::default().fg(palette::TEXT_DIM),
        ),
        Span::styled(text.to_string(), style),
    ])
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_len.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn render_tool_header_with_family_and_summary(
    family: crate::tui::widgets::tool_card::ToolFamily,
    summary: Option<&str>,
    state: &str,
    status: ToolStatus,
    started_at: Option<Instant>,
    low_motion: bool,
) -> Line<'static> {
    // For long-running tools, append elapsed seconds so the user can see the
    // call isn't stuck. Threshold matches the eye's "did this hang?" reflex
    // — under 3s we stay quiet so quick reads/greps don't visually churn.
    let state_owned: String = if state == "running"
        && status == ToolStatus::Running
        && let Some(started) = started_at
    {
        running_status_label_with_elapsed(started.elapsed().as_secs())
    } else {
        state.to_string()
    };

    let glyph = crate::tui::widgets::tool_card::family_glyph(family);
    let verb = crate::tui::widgets::tool_card::family_label(family);

    let mut spans = vec![
        Span::styled(
            format!("{} ", status_symbol(started_at, status, low_motion, family)),
            Style::default().fg(tool_state_color(status)),
        ),
        Span::styled(
            format!("{glyph} "),
            Style::default().fg(tool_state_color(status)),
        ),
        Span::styled(verb.to_string(), tool_title_style()),
        Span::styled(" ", Style::default()),
        Span::styled(state_owned, tool_status_style(status)),
    ];

    // #4148: don't let the summary echo the verb it sits next to — an
    // identity/summary that resolves to the family word itself would render a
    // duplicate like "delegate · delegate". When the summary collapses to the
    // verb, the verb already carries the signal, so drop the redundant tail.
    if let Some(summary) = summary
        .and_then(normalize_header_summary)
        .filter(|summary| !summary.eq_ignore_ascii_case(verb))
    {
        spans.push(Span::styled(" · ", Style::default().fg(palette::TEXT_DIM)));
        spans.push(Span::styled(
            truncate_text(&summary, TOOL_HEADER_SUMMARY_LIMIT),
            Style::default().fg(palette::TEXT_MUTED),
        ));
    }

    Line::from(spans)
}

fn normalize_header_summary(summary: &str) -> Option<String> {
    let normalized = summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Build the "running" label with an elapsed-seconds badge for long-running
/// tools. Below 3s the badge is suppressed to avoid visual churn for tools
/// that resolve in milliseconds; at 3s and beyond the badge appears and ticks
/// every second the tool stays in flight.
pub(crate) fn running_status_label_with_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 3 {
        "running".to_string()
    } else {
        format!("running ({elapsed_secs}s)")
    }
}

fn render_card_detail_line(
    label: Option<&str>,
    value: &str,
    value_style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let label_text = label.map(|text| format!("{text}:"));
    let prefix_width = UnicodeWidthStr::width(TRANSCRIPT_RAIL)
        + label_text.as_deref().map_or(0, UnicodeWidthStr::width)
        + usize::from(label.is_some());
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);

    let mut lines = Vec::new();
    for (idx, part) in wrap_text(value, content_width).into_iter().enumerate() {
        let mut spans = vec![Span::styled(
            TRANSCRIPT_RAIL.to_string(),
            Style::default().fg(palette::TEXT_DIM),
        )];
        if idx == 0 {
            if let Some(label_text) = label_text.as_deref() {
                spans.push(Span::styled(
                    label_text.to_string(),
                    tool_detail_label_style(),
                ));
                spans.push(Span::raw(" "));
            }
        } else if let Some(label_text) = label_text.as_deref() {
            spans.push(Span::raw(
                " ".repeat(UnicodeWidthStr::width(label_text) + 1),
            ));
        }
        spans.push(Span::styled(part, value_style));
        lines.push(Line::from(spans));
    }
    lines
}

fn render_card_detail_line_single(
    label: Option<&str>,
    value: &str,
    value_style: Style,
) -> Line<'static> {
    let label_text = label.map(|text| format!("{text}:"));
    let mut spans = vec![Span::styled(
        TRANSCRIPT_RAIL.to_string(),
        Style::default().fg(palette::TEXT_DIM),
    )];
    if let Some(label_text) = label_text {
        spans.push(Span::styled(label_text, tool_detail_label_style()));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(value.to_string(), value_style));
    Line::from(spans)
}

fn tool_title_style() -> Style {
    active_theme().tool_title_style()
}

fn tool_status_style(status: ToolStatus) -> Style {
    active_theme().tool_status_style(status)
}

fn tool_detail_label_style() -> Style {
    active_theme().tool_label_style()
}

fn tool_state_color(status: ToolStatus) -> Color {
    active_theme().tool_status_color(status)
}

fn tool_status_label(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running => "running",
        ToolStatus::Success => "done",
        ToolStatus::Failed => "issue",
    }
}

fn tool_value_style() -> Style {
    active_theme().tool_value_style()
}

/// Heuristic check whether a string looks like a file path (contains a
/// directory separator or a known source file extension).
fn looks_like_file_path(s: &str) -> bool {
    if s.contains('/') || s.contains('\\') {
        return true;
    }
    // Check for a known file extension
    if let Some((_, ext)) = s.rsplit_once('.') {
        let ext = ext.trim();
        matches!(
            ext,
            "rs" | "toml"
                | "md"
                | "sh"
                | "py"
                | "js"
                | "ts"
                | "json"
                | "yaml"
                | "yml"
                | "css"
                | "html"
                | "go"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "java"
                | "kt"
                | "swift"
                | "rb"
                | "php"
                | "lua"
                | "zig"
                | "mod"
                | "sum"
                | "lock"
                | "txt"
                | "ini"
                | "cfg"
                | "conf"
                | "env"
                | "gitignore"
                | "dockerfile"
                | "sql"
                | "r"
                | "ex"
                | "exs"
                | "vue"
                | "svelte"
                | "tsx"
                | "jsx"
                | "scss"
                | "sass"
                | "less"
                | "gradle"
                | "properties"
                | "xml"
                | "proto"
                | "nix"
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests;
