use super::{
    ASSISTANT_GLYPH, GenericToolCell, HistoryCell, REASONING_CURSOR, REASONING_OPENER,
    REASONING_RAIL, ToolCell, ToolStatus, TranscriptRenderOptions, USER_GLYPH,
    assistant_label_style_for, render_thinking, running_status_label_with_elapsed,
};
use crate::deepseek_theme::Theme;
use crate::palette;
use crate::tui::ui_text::{line_to_plain, slice_text, text_display_width};
use ratatui::style::Modifier;

// ---- elapsed-seconds badge for long-running tools ----
//
// Below 3s the label stays "running" — quick reads/greps shouldn't
// visually churn. From 3s onward the badge appears and ticks each
// second so the user can tell the call hasn't hung.
#[test]
fn summarize_tool_args_ignores_control_only_defaults() {
    let summary = super::summarize_tool_args(&serde_json::json!({
        "max_count": 15,
        "timeout_ms": 30_000
    }));

    assert_eq!(summary, None);
}

#[test]
fn summarize_tool_args_falls_back_to_meaningful_unknown_key() {
    let summary = super::summarize_tool_args(&serde_json::json!({
        "max_count": 15,
        "branch": "main"
    }));

    assert_eq!(summary.as_deref(), Some("branch: main"));
}

#[test]
fn compact_git_tool_header_names_tool_not_control_default() {
    let cell = GenericToolCell {
        name: "git_log".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("max_count: 15".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };

    let lines = cell.lines_with_mode(120, true, super::RenderMode::Live);
    let joined: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();

    assert_eq!(lines.len(), 1);
    assert!(
        joined.contains("read done · git_log"),
        "expected exact tool name in compact row: {joined:?}"
    );
    assert!(
        !joined.contains("max_count"),
        "control defaults should not become the visible tool summary: {joined:?}"
    );
}

#[test]
fn compact_unknown_tool_header_names_tool_not_control_default() {
    let cell = GenericToolCell {
        name: "future_private_tool".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("max_count: 15".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };

    let lines = cell.lines_with_mode(120, true, super::RenderMode::Live);
    let joined: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();

    assert_eq!(lines.len(), 1);
    assert!(
        joined.contains("tool done · future_private_tool"),
        "expected exact tool name in compact row: {joined:?}"
    );
    assert!(
        !joined.contains("max_count"),
        "control defaults should not become the visible tool summary: {joined:?}"
    );
}

#[test]
fn activity_group_renders_as_single_metadata_line() {
    let cell = GenericToolCell {
        name: "activity_group".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("Explored 2 files, 1 search".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };

    let lines = cell.lines_with_mode(120, true, super::RenderMode::Live);
    let joined: String = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect();

    assert_eq!(lines.len(), 1);
    assert_eq!(joined, "Explored 2 files, 1 search");
    assert!(!joined.contains("activity_group"));
}

// ---- Compact agent rendering ----
//
// The DelegateCard owns live state for spawned sub-agents; the
// generic tool block previously duplicated that signal at 3-4 lines
// per spawn. In live mode we now render a single compact line that
// points at the spawned agent id; transcript-mode replay keeps the
// full block so debug history is intact.

#[test]
fn extract_agent_id_pulls_id_from_json_output() {
    let output =
        r#"{"agent_id": "agent-abc12", "nickname": "Beluga", "model": "deepseek-v4-flash"}"#;
    assert_eq!(super::extract_agent_id(output), Some("agent-abc12"));
}

#[test]
fn extract_agent_id_handles_extra_whitespace() {
    let output = r#"{
        "agent_id"   :    "agent-xyz",
        "model": "x"
    }"#;
    assert_eq!(super::extract_agent_id(output), Some("agent-xyz"));
}

#[test]
fn extract_agent_id_returns_none_when_missing() {
    let output = r#"{"nickname": "Orca", "model": "x"}"#;
    assert!(super::extract_agent_id(output).is_none());
    assert!(super::extract_agent_id("(not json)").is_none());
    assert!(super::extract_agent_id("").is_none());
}

#[test]
fn extract_agent_id_returns_none_for_empty_id() {
    let output = r#"{"agent_id": "", "model": "x"}"#;
    assert!(super::extract_agent_id(output).is_none());
}

#[test]
fn agent_spawn_suppresses_generic_card_in_live_mode() {
    // #4133: spawn cards yield entirely to DelegateCard — no generic tool row.
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("prompt: do thing".to_string()),
        output: Some(
            r#"{"agent_id": "agent-abc12", "nickname": "Beluga", "model": "deepseek-v4-flash"}"#
                .to_string(),
        ),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert!(
        lines.is_empty(),
        "spawn generic tool card must be suppressed: {lines:?}"
    );
}

#[test]
fn agent_inspection_renders_single_compact_line_in_live_mode() {
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("action: peek agent_id: agent-abc12".to_string()),
        output: Some(
            r#"{"agent_id": "agent-abc12", "nickname": "Beluga", "model": "deepseek-v4-flash"}"#
                .to_string(),
        ),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "expected exactly 1 line, got {lines:?}");
    let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        rendered.contains("agent-abc12"),
        "expected agent id in header: {rendered:?}"
    );
    assert!(
        rendered.contains("checking"),
        "expected inspection status in header: {rendered:?}"
    );
    assert!(
        !rendered.contains("args"),
        "args should be hidden: {rendered:?}"
    );
}

#[test]
fn agent_pending_inspection_uses_fallback_token() {
    // Pending inspection (no agent_id yet) still renders a compact check line.
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("action: peek prompt: do thing".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "inspection must stay compact: {lines:?}");
    let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        rendered.contains("checking") || rendered.contains("subagent"),
        "{rendered:?}"
    );
    assert!(!rendered.contains('\u{2026}'), "{rendered:?}");
}

#[test]
fn agent_spawn_suppresses_generic_card_in_transcript_mode() {
    // #4133: spawn cards are suppressed in both Live and Transcript; DelegateCard
    // is the sole visible spawn artifact.
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("prompt: do thing".to_string()),
        output: Some(r#"{"agent_id": "agent-abc12", "model": "deepseek-v4-flash"}"#.to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Transcript);
    assert!(
        lines.is_empty(),
        "spawn generic tool card must be suppressed in transcript: {lines:?}"
    );
}

#[test]
fn other_tools_are_unaffected_by_agent_compact_path() {
    // Live-mode tool rows are compact by default; raw detail remains
    // available through the detail pager.
    let cell = GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("path: foo.rs".to_string()),
        output: Some("first line\nsecond line\nthird line".to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "live tools should use compact rows");
}

#[test]
fn agent_compact_header_omits_unknown_child_fallback() {
    // #4148: an inspection whose identity can't be resolved must not leak the
    // raw internal "unknown child" token into the default transcript.
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("action: peek agent_type: delegate".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "inspection must stay compact: {lines:?}");
    let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !rendered.contains("unknown child"),
        "raw fallback token must not leak: {rendered:?}"
    );
    assert!(
        rendered.contains("subagent"),
        "friendly fallback label should be shown: {rendered:?}"
    );
}

#[test]
fn agent_compact_header_does_not_duplicate_delegate_verb() {
    // #4148: when the resolved identity collapses to the "delegate" verb, the
    // compact inspection header must not render a redundant "delegate · delegate".
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("action: peek role: delegate".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "inspection must stay compact: {lines:?}");
    let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !rendered.contains("delegate delegate"),
        "no adjacent duplicate: {rendered:?}"
    );
    assert_eq!(
        rendered.matches("delegate").count(),
        1,
        "verb must not be echoed by the summary: {rendered:?}"
    );
}

// ---- #403 concise todo / checklist update rendering ----
//
// The tool emits an "Updated todo #N to STATUS" leading line plus a
// JSON snapshot. The renderer should detect the prefix and produce
// a compact one-line state-change card instead of dumping the full
// item list every time.

#[test]
fn running_status_label_omits_elapsed_below_threshold() {
    assert_eq!(running_status_label_with_elapsed(0), "running");
    assert_eq!(running_status_label_with_elapsed(1), "running");
    assert_eq!(running_status_label_with_elapsed(2), "running");
}

#[test]
fn running_status_label_appends_elapsed_at_three_seconds() {
    assert_eq!(running_status_label_with_elapsed(3), "running (3s)");
    assert_eq!(running_status_label_with_elapsed(7), "running (7s)");
    assert_eq!(running_status_label_with_elapsed(120), "running (120s)");
}

#[test]
fn render_thinking_shows_full_reasoning_without_dead_affordance() {
    let lines = render_thinking(
        "Summary: First line\nSecond line\nThird line\nFourth line\nFifth line",
        80,
        false,
        false,
    );
    let text = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(text.contains("Fifth line"));
    assert!(!text.contains("Ctrl+O"));
    let header = lines
        .first()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default();
    assert!(
        header.starts_with(REASONING_OPENER),
        "header opens with the dotted opener: {header:?}"
    );
    assert!(
        header.contains("推理 已完成"),
        "header carries the reasoning title and done status: {header:?}"
    );
}

#[test]
fn reasoning_chrome_is_chinese_width_safe_and_preserves_model_text() {
    let raw_reasoning = "MODEL-RAW reasoning/live/done src/lib.rs read_file";

    let live = lines_text(&render_thinking(raw_reasoning, 120, true, true));
    assert!(live.contains("推理 进行中"), "{live}");
    assert!(live.contains(raw_reasoning), "{live}");

    let done = lines_text(&render_thinking(raw_reasoning, 120, false, true));
    assert!(done.contains("推理 已完成"), "{done}");
    assert!(done.contains(raw_reasoning), "{done}");

    let placeholder = lines_text(&render_thinking("", 40, true, true));
    assert!(placeholder.contains("推理中…"), "{placeholder}");

    let hidden = HistoryCell::Thinking {
        content: raw_reasoning.to_owned(),
        streaming: true,
    }
    .lines_with_options(
        18,
        TranscriptRenderOptions {
            show_thinking: false,
            low_motion: true,
            ..TranscriptRenderOptions::default()
        },
    );
    for line in &hidden {
        let plain = line_to_plain(line);
        assert!(
            text_display_width(&plain) <= 18,
            "Chinese reasoning chrome exceeded the terminal width: {plain:?}"
        );
    }
    assert!(
        !lines_text(&hidden).contains(raw_reasoning),
        "hidden reasoning must not expose the model body"
    );
}

#[test]
fn system_note_uses_chinese_title_and_preserves_canonical_content() {
    let raw_content = "event=child_finished path=src/lib.rs tool=read_file child=子一";
    let cell = HistoryCell::System {
        content: raw_content.to_owned(),
    };

    let live = cell.lines(160);
    assert_eq!(live[0].spans[0].content.as_ref(), "说明");
    assert!(lines_text(&live).contains(raw_content));

    let copied = cell.lines_with_copy_metadata(160, TranscriptRenderOptions::default());
    assert_eq!(copied[0].line.spans[0].content.as_ref(), "说明");
    assert!(
        lines_text(
            &copied
                .iter()
                .map(|rendered| rendered.line.clone())
                .collect::<Vec<_>>()
        )
        .contains(raw_content)
    );
}

#[test]
fn render_thinking_streaming_shows_live_content() {
    let lines = render_thinking(
        "Step 1: read the code\nStep 2: trace the call\nStep 3: form a hypothesis",
        80,
        true, // streaming
        true, // low_motion (no cursor noise to grep)
    );
    let text = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(
        text.contains("Step 3: form a hypothesis"),
        "the most recent thinking line must be visible during streaming, got: {text}"
    );
    // "推理中…" placeholder must not be the only thing rendered.
    assert!(
        !text.contains("推理中…"),
        "raw content present means the placeholder line should not be drawn, got: {text}"
    );
}

#[test]
fn render_hidden_streaming_thinking_shows_activity_without_content() {
    let cell = HistoryCell::Thinking {
        content: "private chain of thought that must not be shown".to_string(),
        streaming: true,
    };

    let lines = cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            show_thinking: false,
            low_motion: true,
            ..TranscriptRenderOptions::default()
        },
    );
    let text = lines_text(&lines);

    assert!(
        text.contains("推理内容已隐藏"),
        "hidden live thinking should still show progress: {text}"
    );
    assert!(
        !text.contains("private chain of thought"),
        "hidden live thinking must not reveal content: {text}"
    );
}

#[test]
fn render_hidden_completed_thinking_stays_hidden() {
    let cell = HistoryCell::Thinking {
        content: "completed hidden reasoning".to_string(),
        streaming: false,
    };

    let lines = cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            show_thinking: false,
            ..TranscriptRenderOptions::default()
        },
    );

    assert!(
        lines.is_empty(),
        "completed hidden thinking should stay out of the transcript"
    );
}

#[test]
fn render_thinking_streaming_keeps_the_full_visible_record() {
    let long = (1..=12)
        .map(|i| format!("Reasoning line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = render_thinking(&long, 80, true, true);
    let text = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(
        !text.contains("Ctrl+O"),
        "dead detail shortcut must not be advertised: {text}"
    );
    // The most recent line must be the visible tail (head dropped).
    assert!(
        text.contains("Reasoning line 12"),
        "tail line missing, got: {text}"
    );
    assert!(
        text.contains("Reasoning line 1"),
        "reasoning head must remain visible, got: {text}"
    );
}

// === Speaker glyph tests (v0.6.6 UI redesign) ===
//
// The literal "Assistant" / "You" labels are replaced by the calmer
// bullet/bar glyphs (`●` / `▎`). Only the assistant glyph pulses, and
// only while the cell is streaming — finished turns sit at the source
// sky color so the transcript reads as solid history.

#[test]
fn user_cell_renders_with_bar_glyph_not_literal_label() {
    let cell = HistoryCell::User {
        content: "hello".to_string(),
    };
    let lines = cell.lines(80);
    let head = &lines[0];
    assert_eq!(head.spans[0].content.as_ref(), USER_GLYPH);
    assert_eq!(head.spans[0].style.fg, Some(palette::USER_BODY));
    assert_eq!(head.style.bg, Some(palette::SURFACE_ELEVATED));
    assert_eq!(head.width(), 80);
    assert!(
        head.spans.iter().any(|span| span.style.bg.is_none()),
        "content spans should keep their own styles and inherit the line background"
    );
    // No "You" literal anywhere in the rendered head line.
    let visible: String = head
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(!visible.contains("You"), "user label dropped: {visible:?}");
    assert!(visible.contains("hello"));
}

#[test]
fn user_cell_wraps_fill_transcript_rows() {
    let cell = HistoryCell::User {
        content: "hello world this prompt wraps onto multiple transcript lines".to_string(),
    };
    let lines = cell.lines(18);

    assert!(lines.len() > 1, "expected wrapped user message");
    assert!(
        lines
            .iter()
            .all(|line| line.style.bg == Some(palette::SURFACE_ELEVATED)),
        "wrapped user message lines should keep the highlighted block background"
    );
    assert!(
        lines.iter().all(|line| line.width() == 18),
        "wrapped user message lines should fill the rendered row width"
    );
}

#[test]
fn user_transcript_lines_do_not_append_visual_padding() {
    let cell = HistoryCell::User {
        content: "hello".to_string(),
    };
    let lines = cell.transcript_lines(80);
    let head = &lines[0];
    let visible: String = head.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(visible, format!("{USER_GLYPH} hello"));
    assert!(head.width() < 80);
    assert_eq!(head.style.bg, None);
}

#[test]
fn user_cell_renders_plain_text_without_markdown_interpretation() {
    let cell = HistoryCell::User {
        content: "  # heading\n- item\n   \nhello    world".to_string(),
    };
    let visible: Vec<String> = cell.lines(80).iter().map(line_text).collect();

    assert_eq!(visible[0].trim_end(), format!("{USER_GLYPH}   # heading"));
    assert!(
        visible[1].trim_end().ends_with("- item"),
        "dash-prefixed text must remain literal: {visible:?}"
    );
    assert!(
        visible[2].ends_with("   "),
        "whitespace-only lines must survive: {visible:?}"
    );
    assert!(
        visible[3].trim_end().ends_with("hello    world"),
        "internal spacing must remain literal: {visible:?}"
    );
    assert!(
        !visible.iter().any(|line| line.contains('\u{2500}')),
        "plain user heading must not add markdown heading rule: {visible:?}"
    );
}

#[test]
fn assistant_cell_renders_with_bullet_glyph_not_literal_label() {
    let cell = HistoryCell::Assistant {
        content: "ready".to_string(),
        streaming: false,
    };
    let lines = cell.lines(80);
    let head = &lines[0];
    assert_eq!(head.spans[0].content.as_ref(), ASSISTANT_GLYPH);
    let visible: String = head
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        !visible.contains("Assistant"),
        "assistant label dropped: {visible:?}"
    );
    assert!(visible.contains("ready"));
    assert_ne!(head.style.bg, Some(palette::SURFACE_ELEVATED));
}

#[test]
fn copy_metadata_strips_tool_receipt_chrome_but_keeps_text() {
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "exec_shell".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("command: printf 'receipt'".to_string()),
        output: Some("receipt".to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));
    let rendered = cell.lines_with_copy_metadata(80, TranscriptRenderOptions::default());
    let header = rendered.first().expect("tool receipt header");
    assert!(
        header.copy_prefix_width >= 4,
        "missing status/family chrome width"
    );
    assert!(
        header
            .line
            .spans
            .iter()
            .any(|span| span.content.contains("receipt")),
        "receipt text must remain in the rendered copy source"
    );
    let header_text = line_to_plain(&ratatui::text::Line::from(
        header
            .line
            .spans
            .iter()
            .skip(1)
            .cloned()
            .collect::<Vec<_>>(),
    ));
    let copied = slice_text(
        &header_text,
        header.copy_prefix_width,
        text_display_width(&header_text),
    );
    assert!(
        !copied.contains('✓'),
        "status chrome leaked into copy: {copied:?}"
    );
    assert!(
        !copied.contains('●'),
        "family chrome leaked into copy: {copied:?}"
    );
    assert!(
        copied.contains("run done"),
        "receipt text was clipped: {copied:?}"
    );
}

#[test]
fn copy_metadata_tracks_wrapped_assistant_code_prefix_in_display_columns() {
    let cell = HistoryCell::Assistant {
        content: "```text\n  中文 = 1\n```".to_string(),
        streaming: false,
    };
    let rendered = cell.lines_with_copy_metadata(24, TranscriptRenderOptions::default());
    let code_line = rendered
        .iter()
        .find(|line| {
            line.line
                .spans
                .iter()
                .any(|span| span.content.contains("中文"))
        })
        .expect("wrapped fenced code line");
    assert_eq!(
        code_line.copy_prefix_width, 2,
        "code continuation prefix uses the role marker's two display columns"
    );
    let code = line_to_plain(&code_line.line);
    let copied = slice_text(
        &code,
        code_line.copy_prefix_width,
        text_display_width(&code),
    );
    assert!(
        copied.contains("中文 = 1"),
        "code text was clipped: {copied:?}"
    );
    assert!(
        copied.starts_with("    中文"),
        "code indentation or visual prefix was wrong: {copied:?}"
    );
}

#[test]
fn copy_metadata_keeps_fenced_code_indentation_after_prefix_removal() {
    let cell = HistoryCell::Assistant {
        content: "```rust\n    let answer = 42;\n```".to_string(),
        streaming: false,
    };
    let rendered = cell.lines_with_copy_metadata(40, TranscriptRenderOptions::default());
    let code_line = rendered
        .iter()
        .find(|line| {
            line.line
                .spans
                .iter()
                .any(|span| span.content.contains("answer"))
        })
        .expect("fenced code body");
    let text = line_to_plain(&code_line.line);
    let content = slice_text(
        &text,
        code_line.copy_prefix_width,
        text_display_width(&text),
    );
    assert!(
        content.contains("    let answer = 42;"),
        "code indentation was not preserved: {content:?}"
    );
    for glyph in ['╎', '▎', '●', '│', '┃'] {
        assert!(
            !content.contains(glyph),
            "decorative glyph leaked: {content:?}"
        );
    }
}

#[test]
fn whitespace_only_assistant_cell_renders_nothing() {
    // Regression: a stray newline/space streamed between reasoning and a
    // tool call produced a whitespace-only Assistant cell that rendered as
    // a bare, orphaned role glyph — the "blue dot with nothing after it"
    // artifact. It must collapse to zero lines instead.
    for content in ["", "   ", "\n", "\n\n", " \t \n"] {
        for streaming in [false, true] {
            let cell = HistoryCell::Assistant {
                content: content.to_string(),
                streaming,
            };
            assert!(
                cell.lines(80).is_empty(),
                "whitespace-only assistant content {content:?} (streaming={streaming}) \
                 must render no lines",
            );
        }
    }

    // Sanity: real prose still renders the role glyph as its first span.
    let cell = HistoryCell::Assistant {
        content: "hi".to_string(),
        streaming: false,
    };
    assert_eq!(
        cell.lines(80)[0].spans[0].content.as_ref(),
        ASSISTANT_GLYPH,
        "non-empty assistant content must still render the role glyph",
    );
}

#[test]
fn assistant_cell_still_renders_markdown() {
    let cell = HistoryCell::Assistant {
        content: "# Heading\n\n- item".to_string(),
        streaming: false,
    };
    let visible: Vec<String> = cell.lines(80).iter().map(line_text).collect();

    assert!(
        visible[0].contains("Heading"),
        "assistant heading text should render: {visible:?}"
    );
    assert!(
        !visible[0].contains("# Heading"),
        "assistant heading should still be parsed as markdown: {visible:?}"
    );
    assert!(
        visible.iter().any(|line| line.contains('\u{2500}')),
        "assistant h1 markdown should still add a heading rule: {visible:?}"
    );
}

#[test]
fn assistant_code_block_lines_do_not_get_transcript_rail() {
    let cell = HistoryCell::Assistant {
        content: "SQL:\n```sql\nSELECT\nFROM customers\n```".to_string(),
        streaming: false,
    };
    let visible: Vec<String> = cell
        .lines(80)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();

    assert_eq!(visible[0], format!("{ASSISTANT_GLYPH} SQL:"));
    for line in visible
        .iter()
        .filter(|line| line.contains("SELECT") || line.contains("FROM customers"))
    {
        assert!(
            !line.contains('\u{258F}'),
            "code block line should not inherit the transcript rail: {line:?}"
        );
    }
}

/// Issue #1212 repro: a multi-line SQL fence rendered after a short
/// intro paragraph. Every code-block line — not just the first or last —
/// must avoid the `▏` rail.
#[test]
fn assistant_long_code_block_keeps_every_line_rail_free() {
    let cell = HistoryCell::Assistant {
        content: "Here's the query:\n```sql\nSELECT\n  c.customer_id,\n  c.name,\n  COUNT(o.order_id) AS order_count\nFROM customers c\nJOIN orders o ON c.customer_id = o.customer_id;\n```".to_string(),
        streaming: false,
    };
    let visible: Vec<String> = cell
        .lines(80)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();

    let code_markers = ["SELECT", "customer_id", "name,", "COUNT", "FROM", "JOIN"];
    for marker in code_markers {
        let line = visible
            .iter()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("expected code line containing {marker:?}"));
        assert!(
            !line.contains('\u{258F}'),
            "code block line containing {marker:?} must not have the transcript rail: {line:?}"
        );
    }
}

/// Edge case: a blank line inside a fence is still a code line; it must
/// not regress to the rail because the empty body falls through a
/// different wrap branch.
#[test]
fn assistant_code_block_blank_line_keeps_no_rail() {
    let cell = HistoryCell::Assistant {
        content: "```\nfn one() {}\n\nfn two() {}\n```".to_string(),
        streaming: false,
    };
    for line in cell.lines(80).iter().skip(1) {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{258F}'),
            "fence body line must stay rail-free: {text:?}"
        );
    }
}

/// Wrapped code lines (a single source line longer than the viewport)
/// emit multiple rendered lines from one `Block::Code`. None of them
/// should leak the rail.
#[test]
fn assistant_wrapped_code_lines_keep_no_rail() {
    let long = "let x = ".to_string() + &"abcdef ".repeat(40);
    let content = format!("```\n{long}\n```");
    let cell = HistoryCell::Assistant {
        content,
        streaming: false,
    };
    for line in cell.lines(40).iter().skip(1) {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains('\u{258F}'),
            "wrapped code line must stay rail-free: {text:?}"
        );
    }
}

#[test]
fn assistant_glyph_holds_full_brightness_when_idle() {
    // Idle (streaming=false) and low_motion both pin the colour to the
    // source sky — pulse only fires when actively streaming.
    let idle = assistant_label_style_for(false, false);
    let low_motion = assistant_label_style_for(true, true);
    assert_eq!(idle.fg, Some(palette::WHALE_INFO));
    assert_eq!(low_motion.fg, Some(palette::WHALE_INFO));
}

#[test]
fn assistant_glyph_pulses_when_streaming_and_motion_allowed() {
    // The streaming path runs through `pulse_brightness`, which yields
    // an RGB colour scaled within 30%..100% of the source. Sample twice
    // — at least one of the samples must fall below 100% brightness, or
    // the test wouldn't be exercising the pulse at all. (We can't pin
    // the value because the function reads SystemTime::now().)
    use ratatui::style::Color;
    let mut saw_dimmed = false;
    for _ in 0..50 {
        if let Some(Color::Rgb(_, _, b)) = assistant_label_style_for(true, false).fg {
            let Color::Rgb(_, _, src_b) = palette::WHALE_INFO else {
                panic!("WHALE_INFO must be RGB");
            };
            if b < src_b {
                saw_dimmed = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        saw_dimmed,
        "expected the streaming pulse to dip below source brightness at least once",
    );
}

// === Tool-card verb-glyph tests (v0.6.6 UI redesign) ===

#[test]
fn generic_exec_shell_header_uses_run_family_and_command_summary() {
    let cell = GenericToolCell {
        name: "exec_shell".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("command: cargo test --workspace --all-features".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };

    let header = &cell.lines_with_mode(80, true, super::RenderMode::Live)[0];
    let visible: String = header
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        visible.contains('\u{25B6}'),
        "Run glyph `▶` present: {visible:?}"
    );
    assert!(visible.contains("run running"));
    assert!(
        visible.contains("cargo test"),
        "running shell header must identify the command being executed: {visible:?}"
    );
    assert!(
        !visible.contains("Shell"),
        "old `Shell` literal is gone: {visible:?}"
    );
    assert!(!visible.contains("Ctrl+B"));
    assert!(!visible.contains("/jobs"));

    let transcript_visible: String = HistoryCell::Tool(ToolCell::Generic(cell))
        .transcript_lines(80)[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        transcript_visible.contains("cargo test"),
        "transcript must preserve the running command: {transcript_visible:?}"
    );
    assert!(!transcript_visible.contains("Ctrl+B"));
    assert!(!transcript_visible.contains("/jobs"));
}

#[test]
fn generic_tool_cell_picks_family_from_tool_name() {
    // Use an inspection call so the compact Delegate header still renders;
    // spawn cards are suppressed entirely (#4133).
    let cell = GenericToolCell {
        name: "agent".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("action: peek foo".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);
    assert_eq!(lines.len(), 1, "inspection must stay compact: {lines:?}");
    let header_visible: String = lines[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    // agent → Delegate family (◐ delegate).
    assert!(
        header_visible.contains('\u{25D0}'),
        "Delegate glyph `◐`: {header_visible:?}"
    );
    assert!(
        header_visible.contains(" delegate "),
        "verb label `delegate`: {header_visible:?}"
    );
}

#[test]
fn exploring_card_search_reads_as_find_not_read() {
    // #4145: a completed grep grouped under the exploration card must not
    // render `read done · Searching …`; the header verb has to agree with the
    // `Searching for …` label.
    let cell = super::ExploringCell {
        entries: vec![super::ExploringEntry {
            label: "Searching for `TranscriptScroll`".to_string(),
            status: ToolStatus::Success,
        }],
    };
    let header: String = cell.lines_with_motion(80, true)[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        header.contains("find done"),
        "search card header should read `find done`: {header:?}"
    );
    assert!(
        !header.contains("read done"),
        "search card must not pair `read done` with a search label: {header:?}"
    );
    assert!(
        header.contains("Searching for `TranscriptScroll`"),
        "search label should remain intact: {header:?}"
    );
}

#[test]
fn exploring_card_read_keeps_read_verb() {
    // The fix only re-verbs search-only cards — a plain read stays `read`.
    let cell = super::ExploringCell {
        entries: vec![super::ExploringEntry {
            label: "Reading src/foo.rs".to_string(),
            status: ToolStatus::Success,
        }],
    };
    let header: String = cell.lines_with_motion(80, true)[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        header.contains("read done"),
        "read card header should read `read done`: {header:?}"
    );
}

// === Reasoning treatment tests (v0.6.6 UI redesign) ===

#[test]
fn render_thinking_uses_dotted_opener_in_header() {
    let lines = render_thinking("Step one\nStep two", 80, false, true);
    let header = &lines[0];
    // First span carries `…` followed by a space.
    assert!(
        header.spans[0].content.starts_with(REASONING_OPENER),
        "header opener: {:?}",
        header.spans[0].content
    );
}

#[test]
fn render_thinking_body_lines_use_dashed_rail_and_italic() {
    let lines = render_thinking(
        "concrete reasoning content",
        80,
        /*streaming*/ false,
        /*low_motion*/ true,
    );
    // Header is index 0; first body line is index 1.
    assert!(lines.len() >= 2, "expected at least one body line");
    let body = &lines[1];
    assert_eq!(
        body.spans[0].content.as_ref(),
        REASONING_RAIL,
        "body rail must be the dashed `╎ ` glyph"
    );
    // The body span should carry italic.
    let italic_seen = body
        .spans
        .iter()
        .skip(1)
        .any(|span| span.style.add_modifier.contains(Modifier::ITALIC));
    assert!(italic_seen, "body content should carry italic modifier");
}

#[test]
fn render_thinking_streaming_appends_cursor_when_motion_allowed() {
    let lines = render_thinking(
        "ongoing reasoning...",
        80,
        /*streaming*/ true,
        /*low_motion*/ false,
    );
    // Last line is the most recent body line — cursor lives there.
    let last = lines.last().expect("body line present");
    let last_span = last.spans.last().expect("trailing span present");
    assert!(
        last_span.content.contains(REASONING_CURSOR),
        "expected trailing cursor `▎` on last streaming body line, got {:?}",
        last_span.content
    );
}

#[test]
fn render_thinking_streaming_omits_cursor_when_low_motion() {
    let lines = render_thinking(
        "ongoing reasoning...",
        80,
        /*streaming*/ true,
        /*low_motion*/ true,
    );
    let last = lines.last().expect("body line present");
    let visible: String = last
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(
        !visible.contains(REASONING_CURSOR),
        "low_motion must suppress the streaming cursor: {visible:?}"
    );
}

// === Theme parity tests ===
//
// These lock the visible color/style choices for one plan cell and one
// tool cell against `deepseek_theme::Theme::dark()`. The render path is
// unchanged in shape; the assertions just guarantee a future skin swap
// (or accidental drift) is caught here instead of at runtime.

#[test]
fn generic_exec_shell_failed_status_renders_with_dark_theme_tokens() {
    let theme = Theme::dark();
    let cell = GenericToolCell {
        name: "exec_shell".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: false".to_string()),
        output: Some("boom".to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };

    let lines = cell.lines_with_mode(80, true, super::RenderMode::Live);

    let header = &lines[0];
    let symbol_span = &header.spans[1];
    let glyph_span = &header.spans[2];
    let title_span = &header.spans[3];
    let state_span = &header.spans[5];

    assert_eq!(
        symbol_span.style.fg,
        Some(theme.tool_failed_accent),
        "failed exec_shell header symbol should use the dark theme failed accent"
    );
    // exec_shell is family Run → glyph `▶ ` and verb `run`.
    assert!(
        glyph_span.content.starts_with('\u{25B6}'),
        "Run family glyph: {:?}",
        glyph_span.content
    );
    assert_eq!(
        title_span.content.as_ref(),
        "run",
        "exec_shell routes to Run family → 'run' verb",
    );
    assert_eq!(title_span.style.fg, Some(theme.tool_title_color));
    assert!(title_span.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(state_span.content.as_ref(), "issue");
    assert_eq!(state_span.style.fg, Some(theme.tool_failed_accent));
}

// === display_lines (lines_with_options) vs transcript_lines parity ===
//
// These lock the contract for CX#8: live view keeps reasoning compact
// and caps tool output, transcript view shows the full body. Completed
// reasoning without an explicit Summary stays out of the main flow so it
// cannot masquerade as user text.

fn line_text(line: &ratatui::text::Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn lines_text(lines: &[ratatui::text::Line<'static>]) -> String {
    lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
}

#[test]
fn long_thinking_display_preserves_the_canonical_reasoning_body() {
    let body = "First paragraph lede.\n\
                Second sentence of the first paragraph.\n\n\
                Second paragraph: deeper analysis follows.\n\
                More detail in paragraph two.\n\n\
                Third paragraph: even more reasoning.\n\
                With another line.\n\n\
                Fourth paragraph: the conclusion.\n\
                And one more line for good measure.";
    let cell = HistoryCell::Thinking {
        content: body.to_string(),
        streaming: false,
    };

    let live = cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            low_motion: true,
            ..TranscriptRenderOptions::default()
        },
    );
    let transcript = cell.transcript_lines(80);

    let live_text = lines_text(&live);
    let transcript_text = lines_text(&transcript);

    assert!(
        transcript_text.contains("First paragraph lede"),
        "transcript thinking must keep the lede"
    );
    assert!(
        live_text.contains("First paragraph lede"),
        "live thinking should preview completed reasoning: {live_text}"
    );
    assert!(
        transcript_text.contains("Fourth paragraph"),
        "transcript thinking must keep the full body"
    );
    assert!(
        live_text.contains("Fourth paragraph"),
        "live thinking must keep the full body when reasoning is enabled"
    );
    assert!(
        !live_text.contains("Ctrl+O"),
        "live thinking must not advertise a missing detail command"
    );
    assert!(
        !transcript_text.contains("Ctrl+O"),
        "transcript thinking must not include the dead affordance"
    );
}

#[test]
fn completed_short_thinking_without_summary_stays_visible_in_live_view() {
    // The reasoning rail and tint distinguish this from the user's prompt;
    // the useful body remains inline.
    let cell = HistoryCell::Thinking {
        content: "One brief reasoning step.".to_string(),
        streaming: false,
    };

    let live = cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            low_motion: true,
            ..TranscriptRenderOptions::default()
        },
    );
    let transcript = cell.transcript_lines(80);

    let live_text = lines_text(&live);
    let transcript_text = lines_text(&transcript);

    assert!(
        live_text.contains("One brief reasoning step."),
        "live thinking must preview short completed reasoning: {live_text}"
    );
    assert!(
        transcript_text.contains("One brief reasoning step."),
        "transcript thinking must keep the full reasoning body"
    );
    assert!(
        !live_text.contains("Ctrl+O"),
        "complete short reasoning should not need the detail affordance: {live_text}"
    );
}

#[test]
fn completed_reasoning_preserves_model_text_without_a_shadow_expanded_copy() {
    let cell = HistoryCell::Thinking {
        content: "I will call refresh_catalog_cache to refresh the model list.".to_string(),
        streaming: false,
    };

    let live = cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            low_motion: true,
            ..TranscriptRenderOptions::default()
        },
    );
    let live_text = lines_text(&live);
    assert!(
        live_text.contains("refresh_catalog_cache"),
        "enabled reasoning must preserve the model response exactly: {live_text}"
    );
    assert!(
        live_text.contains("refresh the model list"),
        "surrounding prose must still read: {live_text}"
    );
    assert!(
        !live_text.contains("Ctrl+O"),
        "reasoning must not advertise a missing expanded copy: {live_text}"
    );

    // Transcript / pager / clipboard keeps the full, un-redacted body.
    assert!(
        lines_text(&cell.transcript_lines(80)).contains("refresh_catalog_cache"),
        "transcript must keep the full identifier"
    );
}

#[test]
fn generic_tool_cell_renders_prompts_as_indexed_rows() {
    // When prompts are populated by a fan-out tool, each child shows on
    // its own row instead of the inline `args:` summary so the user can
    // read what each child was asked.
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("prompts: <3 items>".to_string()),
        output: None,
        prompts: Some(vec![
            "Summarize the README".to_string(),
            "List the public types in client.rs".to_string(),
            "Diff this commit against main".to_string(),
        ]),
        output_summary: None,
        is_diff: false,
    }));
    let text = lines_text(&cell.lines(80));

    assert!(text.contains("[0] Summarize the README"));
    assert!(text.contains("[1] List the public types in client.rs"));
    assert!(text.contains("[2] Diff this commit against main"));
    // The inline args summary must not also be emitted — we replaced it
    // with the per-child rows.
    assert!(
        !text.contains("args: prompts:"),
        "inline `args:` summary must be suppressed when per-prompt rows render"
    );
}

#[test]
fn generic_tool_cell_falls_back_to_args_when_prompts_none() {
    // Non-fan-out tools keep the existing `args:` summary so behavior
    // doesn't drift for everything else.
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "file_search".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("query: foo".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));
    let text = lines_text(&cell.lines(80));
    assert!(text.contains("query: foo"));
}

#[test]
fn known_generic_tool_hides_raw_name_in_live_mode() {
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "run_verifiers".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("profile: auto, level: quick".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let text = lines_text(&cell.lines(80));
    assert!(text.contains("verify running"), "{text}");
    assert!(
        !text.contains("name: run_verifiers"),
        "live card should not spend a row on internal tool id: {text}"
    );
    assert!(
        !text.contains("run_verifiers"),
        "known tool id should not leak into compact live card: {text}"
    );
}

#[test]
fn known_generic_tool_keeps_raw_name_in_transcript_mode() {
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "run_verifiers".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("profile: auto, level: quick".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let text = lines_text(&cell.transcript_lines(80));
    assert!(text.contains("verify running"), "{text}");
    assert!(
        text.contains("name: run_verifiers"),
        "transcript replay should preserve exact tool id: {text}"
    );
}

#[test]
fn unknown_generic_tool_keeps_raw_name_in_live_mode() {
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "future_private_tool".to_string(),
        status: ToolStatus::Running,
        input_summary: Some("query: foo".to_string()),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let text = lines_text(&cell.lines(80));
    // Unknown/Generic tools collapse to a single header line in live mode.
    assert!(
        !text.is_empty(),
        "collapsed header must still render: {text}"
    );
}

#[test]
fn generic_tool_cell_preserves_multi_line_output_in_transcript() {
    // Repro for #80: a `git diff --stat`-shaped tool result should keep
    // its newlines on the transcript surface — one file per row, not
    // squashed into a single line.
    let diff_stat = "Cargo.lock                |  1 +\n\
                     crates/cli/Cargo.toml     |  1 +\n\
                     crates/cli/src/main.rs    | 47 ++++++\n\
                     crates/config/src/lib.rs  | 27 ++++\n\
                     crates/tui/src/mcp.rs     | 384 +++++";

    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("command: git diff --stat".to_string()),
        output: Some(diff_stat.to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let transcript_text = lines_text(&cell.transcript_lines(80));

    // Each file path must appear on its own row in the transcript.
    for needle in [
        "Cargo.lock",
        "crates/cli/Cargo.toml",
        "crates/cli/src/main.rs",
        "crates/config/src/lib.rs",
        "crates/tui/src/mcp.rs",
    ] {
        assert!(
            transcript_text.contains(needle),
            "transcript missing '{needle}': {transcript_text}"
        );
    }
    // The pre-fix bug: result line containing
    // "Cargo.lock | 1 + crates/cli/Cargo.toml" — joined into one row.
    // With the fix, the diff-stat pipes are still present per-line, but
    // adjacent file paths are on separate rendered rows. Assert that the
    // first file's line ends before the second begins.
    let lines: Vec<&str> = transcript_text.lines().collect();
    let cargo_lock_line = lines
        .iter()
        .find(|l| l.contains("Cargo.lock"))
        .expect("Cargo.lock row must exist");
    assert!(
        !cargo_lock_line.contains("crates/cli/Cargo.toml"),
        "Cargo.lock row must not also contain the second file: {cargo_lock_line}"
    );
}

#[test]
fn generic_tool_cell_expands_failed_multi_line_output_in_live() {
    // Failed tools should auto-expand in live mode so the command/input summary
    // and full error output remain immediately visible.
    let total = 30usize;
    let output = (0..total)
        .map(|i| format!("row {i:02}: payload"))
        .collect::<Vec<_>>()
        .join("\n");

    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: ls".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let live = cell.lines_with_options(80, TranscriptRenderOptions::default());
    let transcript = cell.transcript_lines(80);
    let live_text = lines_text(&live);
    let transcript_text = lines_text(&transcript);

    assert!(live_text.contains("command: ls"), "{live_text}");
    assert!(
        !live_text.contains("已省略"),
        "failed output must not be hidden behind an omission marker: {live_text}"
    );
    assert!(transcript_text.contains("row 29"));
    assert!(live_text.contains("row 29"));
}

#[test]
fn generic_tool_failed_output_live_renders_card_rail() {
    let output = (0..24usize)
        .map(|i| format!("line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: noisy".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let live_text = lines_text(&cell.lines_with_options(80, TranscriptRenderOptions::default()));

    // Card-rail wrapping: first line starts with ╭, last with ╰.
    assert!(
        live_text.starts_with('\u{256D}'),
        "live view must start with card-rail top glyph ╭: {live_text}"
    );
    assert!(!live_text.contains("已省略"), "{live_text}");
    assert!(live_text.contains("line 00"));
    assert!(live_text.contains("line 23"));
}

#[test]
fn hidden_tool_details_keeps_failed_generic_output_expanded() {
    let output = (0..30usize)
        .map(|i| format!("row {i:02}: payload"))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: noisy".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let live_text = lines_text(&cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            show_tool_details: false,
            ..TranscriptRenderOptions::default()
        },
    ));

    assert!(
        !live_text.contains("已省略") && !live_text.contains("details"),
        "failed output must not be hidden behind a details affordance: {live_text}"
    );
    assert!(live_text.contains("row 29"), "{live_text}");
}

#[test]
fn calm_mode_keeps_failed_generic_output_expanded() {
    let output = (0..30usize)
        .map(|i| format!("row {i:02}: payload"))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: noisy".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let live_text = lines_text(&cell.lines_with_options(
        80,
        TranscriptRenderOptions {
            calm_mode: true,
            ..TranscriptRenderOptions::default()
        },
    ));

    assert!(
        !live_text.contains("已省略") && !live_text.contains("details"),
        "failed output must not be hidden behind a details affordance: {live_text}"
    );
    assert!(live_text.contains("row 29"), "{live_text}");
}

#[test]
fn generic_tool_success_live_collapses_output_transcript_keeps_it() {
    let output = (0..24usize)
        .map(|i| format!("row {i:02}: payload"))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Success,
        input_summary: Some("path: crates/tui/src/main.rs".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));

    let live_text = lines_text(&cell.lines_with_options(80, TranscriptRenderOptions::default()));
    let transcript_text = lines_text(&cell.transcript_lines(80));

    assert!(
        !live_text.contains("row 00"),
        "successful generic tool output should be hidden live: {live_text}"
    );
    assert!(
        !live_text.contains("已省略"),
        "collapsed success should not spend a row on an omission marker: {live_text}"
    );
    assert!(transcript_text.contains("row 00"));
    assert!(transcript_text.contains("row 23"));
}

#[test]
fn tool_output_live_preserves_error_card_rail() {
    let output = [
        "start",
        "still starting",
        "middle noise 1",
        "fatal: failed to read /tmp/deepseek/config.toml",
        "middle noise 2",
        "see https://example.test/build/log for details",
        "middle noise 3",
        "almost done",
        "final line",
    ]
    .join("\n");
    let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "read_file".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("command: tool".to_string()),
        output: Some(output),
        prompts: None,
        output_summary: Some("Error: failed to read config".to_string()),
        is_diff: false,
    }));

    let live_text = lines_text(&cell.lines_with_options(80, TranscriptRenderOptions::default()));

    assert!(
        !live_text.contains("已省略"),
        "failed output must not be hidden behind an omission marker: {live_text}"
    );
    assert!(
        live_text.contains("Error:") || live_text.contains("fatal:"),
        "live summary should capture error text: {live_text}"
    );
    assert!(live_text.contains("final line"), "{live_text}");
}

// === ErrorEnvelope severity → cell color tests (#66) ===

/// Snapshot: an `Error`-severity cell uses the red status palette token
/// for both the leading "Error" label glyph and the body. This is the
/// load-bearing visual signal that distinguishes an error cell from a
/// neutral system note.
#[test]
fn error_severity_cell_renders_in_red() {
    let cell = HistoryCell::Error {
        message: "Authentication failed: invalid API key".to_string(),
        severity: crate::error_taxonomy::ErrorSeverity::Error,
    };
    let lines = cell.lines(80);
    assert!(
        !lines.is_empty(),
        "error cell must render at least one line"
    );

    let head = &lines[0];
    let label_span = &head.spans[0];
    assert_eq!(label_span.content.as_ref(), "Error");
    assert_eq!(label_span.style.fg, Some(palette::STATUS_ERROR));
    assert!(label_span.style.add_modifier.contains(Modifier::BOLD));

    // The body carries the error message and is rendered in the same red.
    let body_text = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(body_text.contains("Authentication failed"));
    // Find a span whose text contains "Authentication" and verify its color.
    let body_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("Authentication"))
        .expect("error body span must exist");
    assert_eq!(body_span.style.fg, Some(palette::STATUS_ERROR));
}

/// `Warning`-severity uses amber, not red — distinguishes a transient
/// retry hiccup from a hard failure.
#[test]
fn warning_severity_cell_renders_in_amber() {
    let cell = HistoryCell::Error {
        message: "Stream stalled: no data received for 60s, closing stream".to_string(),
        severity: crate::error_taxonomy::ErrorSeverity::Warning,
    };
    let lines = cell.lines(80);
    let label_span = &lines[0].spans[0];
    assert_eq!(label_span.content.as_ref(), "Warn");
    assert_eq!(label_span.style.fg, Some(palette::STATUS_WARNING));
}

/// `Critical` severity collapses to the same red as `Error` — both flip
/// offline mode and both should read as the loudest signal in the
/// transcript.
#[test]
fn critical_severity_cell_renders_in_red() {
    let cell = HistoryCell::Error {
        message: "API key expired".to_string(),
        severity: crate::error_taxonomy::ErrorSeverity::Critical,
    };
    let lines = cell.lines(80);
    let label_span = &lines[0].spans[0];
    assert_eq!(label_span.content.as_ref(), "Error");
    assert_eq!(label_span.style.fg, Some(palette::STATUS_ERROR));
}

/// `Info` severity stays neutral / dim so it doesn't draw the eye away
/// from real failures sitting alongside it in the transcript.
#[test]
fn info_severity_cell_renders_in_dim() {
    let cell = HistoryCell::Error {
        message: "Reconnected".to_string(),
        severity: crate::error_taxonomy::ErrorSeverity::Info,
    };
    let lines = cell.lines(80);
    let label_span = &lines[0].spans[0];
    assert_eq!(label_span.content.as_ref(), "Info");
    assert_eq!(label_span.style.fg, Some(palette::TEXT_DIM));
}

fn success_generic_tool(name: &str) -> HistoryCell {
    HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status: ToolStatus::Success,
        input_summary: Some(format!("args for {name}")),
        output: Some(format!("output for {name}")),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }))
}

fn failed_generic_tool(name: &str) -> HistoryCell {
    HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status: ToolStatus::Failed,
        input_summary: None,
        output: Some("failed".to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }))
}

fn running_generic_tool(name: &str) -> HistoryCell {
    HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status: ToolStatus::Running,
        input_summary: None,
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }))
}

fn shell_tool(command: &str) -> HistoryCell {
    HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: "exec_shell".to_string(),
        status: ToolStatus::Success,
        input_summary: Some(format!("command: {command}")),
        output: Some("ok".to_string()),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }))
}

#[test]
fn detect_tool_runs_finds_contiguous_successful_safe_tools() {
    let history = vec![
        HistoryCell::User {
            content: "go".to_string(),
        },
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
        success_generic_tool("web_search"),
        HistoryCell::Assistant {
            content: "done".to_string(),
            streaming: false,
        },
    ];

    let runs = super::detect_tool_runs(&history, 3);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start, 1);
    assert_eq!(runs[0].count, 3);
    assert_eq!(
        runs[0].tool_families,
        vec!["read_file", "list_dir", "web_search"]
    );
    assert_eq!(runs[0].activity.files, 2);
    assert_eq!(runs[0].activity.searches, 1);
}

#[test]
fn detect_tool_runs_honors_threshold_and_boundaries() {
    let short = vec![
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
    ];
    assert!(super::detect_tool_runs(&short, 3).is_empty());

    let with_assistant_boundary = vec![
        success_generic_tool("read_file"),
        HistoryCell::Assistant {
            content: "pause".to_string(),
            streaming: false,
        },
        success_generic_tool("list_dir"),
        success_generic_tool("web_search"),
    ];
    assert!(super::detect_tool_runs(&with_assistant_boundary, 3).is_empty());
}

#[test]
fn detect_tool_runs_keeps_failed_running_and_shell_cells_visible() {
    let history = vec![
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
        failed_generic_tool("web_search"),
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
        running_generic_tool("web_search"),
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
        shell_tool("rm -rf target"),
        success_generic_tool("read_file"),
        success_generic_tool("list_dir"),
        success_generic_tool("web_search"),
    ];

    let runs = super::detect_tool_runs(&history, 3);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start, 9);
    assert_eq!(runs[0].count, 3);
}

#[test]
fn detect_tool_runs_summarizes_safe_command_tools() {
    let history = vec![
        success_generic_tool("run_tests"),
        success_generic_tool("run_verifiers"),
        success_generic_tool("validate_data"),
    ];

    let runs = super::detect_tool_runs(&history, 3);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start, 0);
    assert_eq!(runs[0].count, 3);
    assert_eq!(runs[0].activity.commands, 3);
    assert_eq!(
        runs[0].tool_families,
        vec!["run_tests", "run_verifiers", "validate_data"]
    );
    assert_eq!(
        super::tool_run_summary(&runs[0]),
        "Ran 3 commands: run_tests, run_verifiers, validate_data"
    );
}

#[test]
fn tool_run_summary_reports_compact_success_group() {
    let run = super::ToolRun {
        start: 4,
        count: 5,
        tool_families: vec!["read_file".to_string(), "list_dir".to_string()],
        activity: super::ToolRunActivitySummary {
            files: 4,
            searches: 1,
            ..Default::default()
        },
    };

    let summary = super::tool_run_summary(&run);

    assert_eq!(summary, "Explored 4 files, 1 search: read_file, list_dir");
}

#[test]
fn tool_run_summary_keeps_git_history_tools_visible() {
    let history = vec![
        success_generic_tool("git_log"),
        success_generic_tool("git_show"),
        success_generic_tool("git_blame"),
    ];

    let runs = super::detect_tool_runs(&history, 3);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].activity.files, 3);
    assert_eq!(
        super::tool_run_summary(&runs[0]),
        "Explored 3 files: git_log, git_show, git_blame"
    );
}

#[test]
fn tool_run_summary_lists_only_command_families_for_command_clause() {
    let run = super::ToolRun {
        start: 4,
        count: 4,
        tool_families: vec![
            "read_file".to_string(),
            "run_tests".to_string(),
            "validate_data".to_string(),
        ],
        activity: super::ToolRunActivitySummary {
            files: 2,
            commands: 2,
            ..Default::default()
        },
    };

    assert_eq!(
        super::tool_run_summary(&run),
        "Explored 2 files: read_file, ran 2 commands: run_tests, validate_data"
    );
}

#[test]
fn tool_run_summary_uses_metadata_fallback_for_unknown_groups() {
    let run = super::ToolRun {
        start: 4,
        count: 2,
        tool_families: vec!["session_sync".to_string()],
        activity: super::ToolRunActivitySummary {
            other: 2,
            ..Default::default()
        },
    };

    assert_eq!(super::tool_run_summary(&run), "Updated metadata");
}

// ---- #4112 / dogfood A5: transcript noise ----

fn agent_cell(
    action_summary: Option<&str>,
    status: ToolStatus,
    output: Option<&str>,
) -> GenericToolCell {
    GenericToolCell {
        name: "agent".to_string(),
        status,
        input_summary: action_summary.map(str::to_string),
        output: output.map(str::to_string),
        prompts: None,
        output_summary: None,
        is_diff: false,
    }
}

fn joined_lines(cell: &GenericToolCell, mode: super::RenderMode) -> String {
    cell.lines_with_mode(120, true, mode)
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
        .collect()
}

#[test]
fn unknown_tool_failure_collapses_to_one_line() {
    let cell = GenericToolCell {
        name: "item".to_string(),
        status: ToolStatus::Failed,
        input_summary: Some("status: pending".to_string()),
        output: Some(
            "Tool 'item' is not available in the current tool catalog. \
             Checklist entries are not separate tool calls."
                .to_string(),
        ),
        prompts: None,
        output_summary: None,
        is_diff: false,
    };
    for mode in [super::RenderMode::Live, super::RenderMode::Transcript] {
        let lines = cell.lines_with_mode(120, true, mode);
        assert_eq!(
            lines.len(),
            1,
            "unknown-tool failure should be a single header line in {mode:?}: {lines:?}"
        );
        let joined = joined_lines(&cell, mode);
        assert!(
            joined.contains("Tool 'item' is not available"),
            "the catalog error is the useful part: {joined:?}"
        );
        assert!(
            !joined.contains("name: item"),
            "no name:/args:/result: block for unknown tools: {joined:?}"
        );
    }
}

#[test]
fn agent_peek_renders_checked_not_done() {
    let cell = agent_cell(
        Some("action: peek agent_id: agent_scout_1"),
        ToolStatus::Success,
        Some(r#"{"agent_id":"agent_scout_1","status":"running"}"#),
    );
    let joined = joined_lines(&cell, super::RenderMode::Live);
    assert!(
        joined.contains("checked") && joined.contains("agent_scout_1"),
        "peek should read as a check, not a completed delegate: {joined:?}"
    );
    assert!(
        !joined.contains("delegate done"),
        "peek must not draw the spawn-completion line: {joined:?}"
    );
}

#[test]
fn agent_wait_renders_waited_label() {
    let cell = agent_cell(
        Some("action: wait"),
        ToolStatus::Success,
        Some(r#"{"action":"wait","settled":[{"agent_id":"agent_scout_1"}]}"#),
    );
    let joined = joined_lines(&cell, super::RenderMode::Live);
    assert!(
        joined.contains("waited"),
        "wait cells should read as a join: {joined:?}"
    );
}

#[test]
fn agent_inspection_stays_compact_in_transcript_mode() {
    let cell = agent_cell(
        Some("action: status agent_id: agent_scout_1"),
        ToolStatus::Success,
        Some(r#"{"agent_id":"agent_scout_1","status":"running","terminal":false}"#),
    );
    let lines = cell.lines_with_mode(120, true, super::RenderMode::Transcript);
    assert_eq!(
        lines.len(),
        1,
        "status checks should not dump full projections in the pager: {lines:?}"
    );
}

#[test]
fn agent_spawn_suppresses_generic_card_in_favor_of_delegate_card() {
    let cell = agent_cell(
        Some("prompt: map the repo"),
        ToolStatus::Success,
        Some(r#"{"agent_id":"agent_scout_1","status":"running"}"#),
    );
    for mode in [super::RenderMode::Live, super::RenderMode::Transcript] {
        let lines = cell.lines_with_mode(120, true, mode);
        assert!(
            lines.is_empty(),
            "spawn generic tool card must yield to DelegateCard (#4133): {mode:?} {lines:?}"
        );
    }
}
