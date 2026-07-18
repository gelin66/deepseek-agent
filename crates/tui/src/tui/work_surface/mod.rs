//! Read-only work projection for tasks, tool runs and canonical child agents.
//! Runtime and stores remain the sole owners of all displayed facts.

mod live_projection;
mod model;
mod render;

pub use model::{WorkSurfacePlacement, WorkSurfaceState};
pub use render::{height, render, split_chat};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    use unicode_width::UnicodeWidthStr;

    use crate::config::Config;
    use crate::tui::app::{App, TaskPanelEntry, TaskPanelEntryKind, TuiOptions};

    fn app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("."),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: true,
            use_bracketed_paste: true,
            max_subagents: 4,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        App::new(options, &Config::default())
    }

    fn buffer_text(buf: &Buffer) -> String {
        let mut text = String::new();
        for y in buf.area.y..buf.area.bottom() {
            let mut x = buf.area.x;
            while x < buf.area.right() {
                let symbol = buf[(x, y)].symbol();
                text.push_str(symbol);
                x = x.saturating_add(UnicodeWidthStr::width(symbol).max(1) as u16);
            }
            text.push('\n');
        }
        text
    }

    fn add_task(app: &mut App, id: &str) {
        app.task_panel.push(TaskPanelEntry {
            id: id.to_string(),
            status: "running".to_string(),
            prompt_summary: format!("task {id}"),
            duration_ms: Some(1_000),
            kind: TaskPanelEntryKind::Background,
            stale: false,
            elapsed_since_output_ms: None,
            owner_agent_id: None,
            owner_agent_name: None,
        });
    }

    fn start_child(app: &mut App, child: &str) {
        use codewhale_protocol::agent_runtime::RunId;

        app.child_agents.begin_root(RunId("root-run".to_string()));
        app.child_agents.record_started(
            RunId("root-run".to_string()),
            format!("call-{child}"),
            RunId(child.to_string()),
            1,
        );
    }

    #[test]
    fn compact_surface_preserves_task_without_fake_controls() {
        let mut app = app();
        add_task(&mut app, "shell_compact");
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, frame.area(), &mut app))
            .expect("draw");
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("task shell_compact"), "{text}");
        for fake_control in ["[打开]", "[停止]", "确认", "正在停止"] {
            assert!(!text.contains(fake_control), "{fake_control}: {text}");
        }
    }

    #[test]
    fn waiting_row_freezes_other_live_marks() {
        let mut app = app();
        add_task(&mut app, "run");
        add_task(&mut app, "ask");
        app.task_panel[1].status = "waiting".to_string();
        let backend = TestBackend::new(100, 5);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, frame.area(), &mut app))
            .expect("draw");
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains('◆'), "waiting keeps a still attention mark");
        assert!(
            !text.contains('›'),
            "other live marks freeze under attention"
        );
    }

    #[test]
    fn canonical_children_render_without_a_second_snapshot() {
        let mut app = app();
        for index in 1..=3 {
            start_child(&mut app, &format!("agent_{index}"));
        }
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, frame.area(), &mut app))
            .expect("draw");
        let text = buffer_text(terminal.backend().buffer());
        assert_eq!(
            app.work_surface
                .latest_rows
                .iter()
                .filter(|row| row.id.starts_with("worker:"))
                .count(),
            3
        );
        assert!(text.contains("子 Agent 1"), "{text}");
        assert!(text.contains("子 Agent 3"), "{text}");
    }

    #[test]
    fn bounded_projection_is_deterministic_and_does_not_claim_scrollability() {
        let mut app = app();
        for id in ["one", "two", "three", "four", "five", "six"] {
            add_task(&mut app, id);
        }
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, frame.area(), &mut app))
            .expect("first draw");
        let first = buffer_text(terminal.backend().buffer());
        terminal
            .draw(|frame| super::render(frame, frame.area(), &mut app))
            .expect("second draw");
        let second = buffer_text(terminal.backend().buffer());

        assert_eq!(first, second);
        assert_eq!(app.work_surface.latest_rows.len(), 7);
        assert!(
            !first.contains('┃'),
            "read-only view must not show a fake thumb"
        );
    }

    #[test]
    fn left_and_right_placements_reserve_a_side_rail() {
        for (placement, expected_chat_x, expected_rail_x) in [
            (super::WorkSurfacePlacement::Left, 30, 0),
            (super::WorkSurfacePlacement::Right, 0, 70),
        ] {
            let mut app = app();
            add_task(&mut app, "rail");
            app.work_surface.placement = placement;
            assert_eq!(super::height(&mut app, 100, 24, false), 0);

            let area = ratatui::layout::Rect::new(0, 0, 100, 12);
            let (chat, rail) = super::split_chat(&mut app, area, false);
            let rail = rail.expect("side rail");
            assert_eq!(chat.x, expected_chat_x);
            assert_eq!(chat.width, 70);
            assert_eq!(rail.x, expected_rail_x);
            assert_eq!(rail.width, 30);

            let backend = TestBackend::new(100, 12);
            let mut terminal = Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| super::render(frame, rail, &mut app))
                .expect("draw");
            let divider_x = if placement == super::WorkSurfacePlacement::Left {
                rail.right().saturating_sub(1)
            } else {
                rail.x
            };
            assert_eq!(terminal.backend().buffer()[(divider_x, 0)].symbol(), "│");
        }
    }

    #[test]
    fn classic_and_narrow_layouts_keep_the_existing_top_surface() {
        let mut app = app();
        add_task(&mut app, "top");
        app.work_surface.placement = super::WorkSurfacePlacement::Right;

        assert_eq!(super::height(&mut app, 100, 24, true), 8);
        let area = ratatui::layout::Rect::new(0, 0, 100, 12);
        let (chat, rail) = super::split_chat(&mut app, area, true);
        assert_eq!(chat, area);
        assert!(rail.is_none());
        assert_eq!(
            app.work_surface.placement,
            super::WorkSurfacePlacement::Right,
            "fallback must not overwrite the saved preference"
        );

        assert_eq!(super::height(&mut app, 60, 16, false), 5);
        let narrow = ratatui::layout::Rect::new(0, 0, 60, 8);
        let (chat, rail) = super::split_chat(&mut app, narrow, false);
        assert_eq!(chat, narrow);
        assert!(rail.is_none());
    }
}
