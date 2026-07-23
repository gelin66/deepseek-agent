//! TUI event loop and rendering logic for `DeepSeek` CLI.

use std::io::{self, Stdout, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use codewhale_app::AgentApplication;
use codewhale_protocol::agent_runtime::{
    ApprovalRisk, ReasoningEffort as RuntimeReasoningEffort, RunId, RunLimits, ToolPolicy,
    UserInteractionPrompt, UserInteractionResponse,
};
use codewhale_protocol::run_api::{RunProductControls, StartRunCommand};
use codewhale_protocol::task::TaskDefinition;
// On Windows the push/pop helpers write the escapes directly; crossterm's
// PushKeyboardEnhancementFlags / PopKeyboardEnhancementFlags commands are
// never referenced, so the imports are gated to avoid -D warnings failures.
#[cfg(not(windows))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout},
    prelude::Widget,
    style::Style,
    widgets::Block,
};
use tracing;
#[cfg(target_os = "windows")]
use windows::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode};

use crate::config::Config;
use crate::localization::{MessageId, tr};
use crate::palette;
use crate::prompts;
use crate::settings::Settings;
use crate::tui::color_compat::ColorCompatBackend;
use crate::tui::key_shortcuts;
use crate::tui::onboarding;
use crate::tui::pager::PagerView;
use crate::tui::run_client::{TuiRunClient, TuiRunClientError};
use crate::tui::run_presenter::{PresenterAction, present_effect};
use crate::tui::run_projection::CanonicalRunProjection;
use crate::tui::user_input::UserInputView;

use super::app::{App, OnboardingState, ReasoningEffort, StatusToastLevel, TuiOptions};
use super::approval::{ApprovalMode, ApprovalRequest, ApprovalView, ReviewDecision};
use super::canonical_commands::{self, CanonicalSlashCommand, CanonicalSlashParse};
use super::history::HistoryCell;
use super::slash_menu::{
    apply_slash_menu_selection, try_autocomplete_slash_command, visible_slash_menu_entries,
};
use super::views::{ModalKind, ViewEvent};
use super::widgets::{ChatWidget, ComposerWidget, Renderable};

// === Constants ===

/// Upper bound on slash-menu entries returned to the renderer. The composer's
/// render path already paginates with center-tracking (see
/// `widgets::ComposerWidget::render`), so this only needs to be high enough to
/// encompass the full filtered command list — never the visible-row budget.
/// Bumped from 6 to 128 to fix #64 (selection couldn't reach commands beyond
/// the visible window because the source list itself was capped).
const SLASH_MENU_LIMIT: usize = 128;
const MIN_CHAT_HEIGHT: u16 = 3;
const MIN_COMPOSER_HEIGHT: u16 = 2;
const UI_ACTIVE_POLL_MS: u64 = 24;
/// Ambient fish and the completion wake need a smoother cadence than the
/// deliberately legible status spinner. This remains modest enough for a
/// terminal renderer while avoiding the five-frame-per-second "jump" seen
/// whenever live status motion and ocean motion overlap.
pub(crate) const UI_UNDERWATER_ANIMATION_MS: u64 = 80;
const DEFAULT_TERMINAL_PROBE_TIMEOUT_MS: u64 = 500;

fn app_auto_approve_enabled(app: &App) -> bool {
    app.approval_mode == ApprovalMode::AutoApprove
}

type AppTerminal = Terminal<ColorCompatBackend<Stdout>>;

// Reset scroll region (`\x1b[r`), origin mode (`\x1b[?6l`), and home the cursor
// (`\x1b[H`) before letting ratatui's diff renderer repaint. The destructive
// `\x1b[2J\x1b[3J` pair was previously appended here to also wipe the visible
// screen and saved scrollback, but combined with the immediately-following
// `terminal.clear()` it produced a double-clear that several terminals
// (Ghostty, VSCode terminal, Win10 conhost) render as visible flicker on every
// TurnComplete / focus-gain / resize. The alt-screen buffer's double-buffering
// plus ratatui's `terminal.clear()` are sufficient to repaint cleanly.
const TERMINAL_ORIGIN_RESET: &[u8] = b"\x1b[r\x1b[?6l\x1b[H";
// Xterm alternate-scroll mode keeps wheel events inside the alternate-screen
// viewport when mouse capture is requested but unavailable or temporarily
// dropped. Leave it off with `--no-mouse-capture` so the host terminal owns
// raw mouse selection behavior end-to-end.
const ENABLE_ALT_SCROLL_MODE: &[u8] = b"\x1b[?1007h";
const DISABLE_ALT_SCROLL_MODE: &[u8] = b"\x1b[?1007l";
/// Begin synchronized update (DEC 2026): tell the terminal to defer
/// rendering until END_SYNC_UPDATE is received. Best-effort —
/// terminals that don't support this silently ignore the sequence.
/// Reduces flicker on GPU-accelerated terminals (Ghostty, VSCode
/// Terminal, Kitty, WezTerm) by batching ratatui's incremental
/// diff writes into a single frame.
const BEGIN_SYNC_UPDATE: &[u8] = b"\x1b[?2026h";
/// End synchronized update (DEC 2026): tell the terminal to render
/// the complete frame now.
const END_SYNC_UPDATE: &[u8] = b"\x1b[?2026l";
const TERMINAL_INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);

enum TerminalInputMessage {
    Event(Event),
    Error(io::Error),
}

struct TerminalInputPump {
    rx: std::sync::mpsc::Receiver<TerminalInputMessage>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TerminalInputPump {
    fn spawn() -> io::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("codewhale-terminal-input".to_string())
            .spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match event::poll(TERMINAL_INPUT_POLL_INTERVAL) {
                        Ok(true) => match event::read() {
                            Ok(event) => {
                                if tx.send(TerminalInputMessage::Event(event)).is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(TerminalInputMessage::Error(err));
                                break;
                            }
                        },
                        Ok(false) => {}
                        Err(err) => {
                            let _ = tx.send(TerminalInputMessage::Error(err));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            rx,
            stop,
            handle: Some(handle),
        })
    }

    fn recv_timeout(&self, timeout: Duration) -> io::Result<Option<Event>> {
        match self.rx.recv_timeout(timeout) {
            Ok(TerminalInputMessage::Event(event)) => Ok(Some(event)),
            Ok(TerminalInputMessage::Error(err)) => Err(err),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "终端输入线程已断开",
            )),
        }
    }
}

impl Drop for TerminalInputPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            #[cfg(target_os = "windows")]
            {
                drop(handle);
            }
            #[cfg(not(target_os = "windows"))]
            let _ = handle.join();
        }
    }
}

fn complete_trust_directory_onboarding(app: &mut App) -> Result<(), String> {
    onboarding::mark_trusted_at(app.config_path.as_deref(), &app.workspace)
        .map_err(|err| err.to_string())?;
    // Workspace trust permits loading and operating in this repository. It
    // does not silently grant unrestricted access outside the workspace.
    app.trust_mode = false;
    app.status_message = None;
    if app.onboarding_workspace_trust_gate {
        app.onboarding_workspace_trust_gate = false;
        app.onboarding = OnboardingState::None;
    } else {
        app.onboarding = OnboardingState::Tips;
    }
    Ok(())
}

fn back_from_api_key_onboarding(app: &mut App) {
    app.onboarding = OnboardingState::Welcome;
    app.api_key_input.clear();
    app.api_key_cursor = 0;
    app.status_message = None;
}

fn surface_prompt_override_notices(app: &mut App) {
    for notice in prompts::take_prompt_override_notices() {
        app.add_message(HistoryCell::System {
            content: prompt_override_warning(&notice),
        });
        app.push_status_toast(notice, StatusToastLevel::Warning, Some(12_000));
    }
}

fn prompt_override_warning(notice: &str) -> String {
    format!("警告：{notice}")
}

/// Run the interactive TUI event loop.
///
/// # Examples
///
/// ```ignore
/// # use crate::config::Config;
/// # use crate::tui::TuiOptions;
/// # async fn example(config: &Config, options: TuiOptions) -> anyhow::Result<()> {
/// crate::tui::run_tui(config, options).await
/// # }
/// ```
fn validate_interactive_tui_entry(config: &Config, options: &TuiOptions) -> Result<()> {
    config.validate()?;
    let configured_model = crate::resolve_interactive_deepseek_model(config)?;
    if options.model != configured_model {
        bail!(
            "交互式 Agent 模型配置不一致：配置解析为 {configured_model}，TUI 收到 {}。",
            options.model
        );
    }
    Ok(())
}

pub async fn run_tui(config: &Config, options: TuiOptions) -> Result<()> {
    validate_interactive_tui_entry(config, &options)?;
    // Onboarding may persist and install the official DeepSeek key into this
    // process-local view. The validated provider/model selection is immutable.
    let mut config = config.clone();
    let config = &mut config;
    let use_alt_screen = options.use_alt_screen;
    let use_mouse_capture = options.use_mouse_capture;
    let use_bracketed_paste = options.use_bracketed_paste;

    // Apply OSC 8 hyperlink toggle from config.
    //
    // #3029: OSC 8 hyperlinks are emitted out-of-band. Markdown wrapping keeps
    // visible spans and per-line targets in separate structures; each render
    // seam translates those targets into absolute `LinkRegion`s without ever
    // placing an escape byte in a ratatui buffer cell. `ColorCompatBackend`
    // then emits the OSC 8 escapes through its `Write` impl around the matching
    // cell runs. Hyperlinks are on by default for terminals that handle the OSC
    // terminator (`ESC \`) cleanly. Windows legacy consoles (conhost) still
    // mishandle the terminator, so the default stays off there; opt in via
    // `[tui] osc8_links = true` on any platform.
    let osc8_default_on = !cfg!(target_os = "windows");
    crate::tui::osc8::set_enabled(
        config
            .tui
            .as_ref()
            .and_then(|tui| tui.osc8_links)
            .unwrap_or(osc8_default_on),
    );

    // Terminal probe with timeout to prevent hanging on unresponsive terminals.
    //
    // The blocking task cannot be cancelled once the timeout fires, so a slow
    // `enable_raw_mode` may still succeed *after* we've bailed out, leaking
    // raw mode. Both sides run `raw_mode_probe_handshake`; whichever observes
    // the other's flag disables raw mode again.
    let probe_timeout = terminal_probe_timeout(config);
    let probe_abandoned = Arc::new(AtomicBool::new(false));
    let probe_enabled = Arc::new(AtomicBool::new(false));
    let task_abandoned = Arc::clone(&probe_abandoned);
    let task_enabled = Arc::clone(&probe_enabled);
    let enable_raw = tokio::task::spawn_blocking(move || {
        let result = enable_raw_mode().map_err(raw_mode_enable_error);
        if result.is_ok() && raw_mode_probe_handshake(&task_enabled, &task_abandoned) {
            // The probe timed out while we were blocked; the caller already
            // gave up, so undo the late enable instead of leaking raw mode.
            let _ = disable_raw_mode();
        }
        result
    });

    match tokio::time::timeout(probe_timeout, enable_raw).await {
        Ok(inner_result) => {
            inner_result??; // propagate both join and raw-mode errors
        }
        Err(_) => {
            if raw_mode_probe_handshake(&probe_abandoned, &probe_enabled) {
                // The blocking task finished enabling raw mode right as the
                // timeout fired and may have missed the abandoned flag.
                let _ = disable_raw_mode();
            }
            tracing::warn!(
                "终端探测在 {}ms 后超时，终端可能无响应",
                probe_timeout.as_millis()
            );
            return Err(terminal_probe_timeout_error(probe_timeout));
        }
    }

    #[cfg(target_os = "windows")]
    enable_windows_ime_console_mode();

    let mut stdout = io::stdout();
    // Initialize the file-backed TUI log and redirect raw stderr away from
    // the alt-screen for the lifetime of this guard. MUST run BEFORE
    // EnterAlternateScreen; otherwise logging between alt-screen entry and
    // redirect init leaks raw bytes into the TUI buffer, causing the "scroll
    // demon" on Windows (#1909) and garbled output on all platforms (#1085).
    // The guard is held until the function returns; dropping it after
    // LeaveAlternateScreen restores the original stderr handle/fd so shutdown
    // messages reach the user's terminal. We accept the init failing (e.g.,
    // read-only $HOME) and continue without the redirect rather than refusing
    // to start the TUI.
    let _tui_log_guard = match crate::runtime_log::init() {
        Ok(guard) => Some(guard),
        Err(err) => {
            tracing::warn!(target: "runtime_log", ?err, "TUI log init failed; stderr leaks may render as scroll-demon");
            None
        }
    };
    if use_alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
        // Windows also suppresses CodeWhale's own verbose CLI logger while
        // the alt-screen is active. The stderr redirect above catches raw
        // writes; this prevents the known verbose source at the origin.
        #[cfg(windows)]
        crate::logging::snapshot_verbose_state();
        #[cfg(windows)]
        crate::logging::set_verbose(false);
    }
    // Mouse capture, bracketed paste, focus events, and the Kitty
    // keyboard-protocol escape-disambiguation flag (#442). The setup is kept
    // in one helper so startup and mode-recovery tests exercise the same
    // sequence.
    //
    // Focus events are necessary for IME compositor re-activation on
    // macOS when the user switches away (Cmd+Tab) and returns. The Kitty
    // keyboard protocol opt-in is best-effort: terminals that don't
    // support it (iTerm2, Terminal.app, Windows 10 conhost) silently
    // discard the escape, while supporting terminals (Kitty, Ghostty,
    // Alacritty 0.13+, WezTerm, recent Konsole, recent xterm) report
    // unambiguous events for Option/Alt-modified keys and plain Esc.
    //
    // Only `DISAMBIGUATE_ESCAPE_CODES` is pushed — the higher tiers
    // (`REPORT_EVENT_TYPES`, `REPORT_ALL_KEYS_AS_ESCAPE_CODES`) emit
    // release events that the existing key handlers would mis-route
    // as duplicate presses.
    //
    // On Windows, crossterm's `PushKeyboardEnhancementFlags` command always
    // reports the terminal as unsupported (`is_ansi_code_supported` returns
    // false), so the escape is written directly instead. VSCode's integrated
    // terminal and Windows Terminal ≥1.17 honour the kitty keyboard protocol
    // and will correctly disambiguate Shift+Enter from plain Enter once this
    // sequence is received. Terminals that do not understand it silently
    // ignore it.
    recover_terminal_modes(&mut stdout, use_mouse_capture, use_bracketed_paste);
    let mut cleanup_guard = TerminalCleanupGuard {
        use_alt_screen,
        use_mouse_capture,
        use_bracketed_paste,
        defused: false,
    };
    let color_depth = palette::ColorDepth::detect();
    let palette_mode = palette::PaletteMode::detect();
    tracing::debug!(
        ?color_depth,
        ?palette_mode,
        "terminal color profile detected"
    );
    let backend = ColorCompatBackend::new(stdout, color_depth, palette_mode);
    let mut terminal = Terminal::new(backend)?;
    // At this point Settings hasn't loaded yet, so we can't read the
    // user's `synchronized_output` knob. Use the same env-based terminal
    // quirk detection that `Settings::apply_env_overrides` uses, so the
    // startup viewport reset matches what every later draw will do on
    // flicker-sensitive hosts. A user who has explicitly set
    // `synchronized_output = "on"` to override detection will get sync wrap
    // from the main draw loop onward; the one-time startup viewport reset
    // stays opt-out for them, which is the safe default because the cost is
    // at most brief tearing on the first frame.
    let sync_output_at_init = !crate::settings::detected_ptyxis_terminal()
        && !crate::settings::detected_legacy_windows_console_host();
    reset_terminal_viewport(&mut terminal, sync_output_at_init)?;
    let mut app = App::new(options.clone(), config);
    crate::startup_trace::mark("app_constructed");
    surface_prompt_override_notices(&mut app);

    let input = TerminalInputPump::spawn()?;
    if run_deepseek_onboarding_loop(&mut terminal, &mut app, config, &input).await? {
        return Ok(());
    }
    app.workspace = app.workspace.canonicalize().with_context(|| {
        tr(MessageId::CanonicalWorkspaceCanonicalizeFailed)
            .replace("{path}", &app.workspace.display().to_string())
    })?;
    if !app.workspace.is_dir() {
        anyhow::bail!(
            "{}",
            tr(MessageId::CanonicalWorkspaceNotDirectory)
                .replace("{path}", &app.workspace.display().to_string())
        );
    }
    let workspace_identity = app.workspace.display().to_string();
    let settings = Settings::load().unwrap_or_default();
    let application_config = crate::exec_runtime::production_application_config(
        config,
        &app.workspace,
        &settings,
        app.allow_shell,
        app_auto_approve_enabled(&app),
        app.trust_mode,
        None,
    )?;
    let application = Arc::new(AgentApplication::production(application_config)?);
    let (run_client, mut run_events) = TuiRunClient::new(application);

    let mut suppress_automatic_initial_submit = false;
    if let Some(resume_id) = options.resume_session_id.as_deref() {
        let run_id = if resume_id == "latest" {
            run_client
                .latest_root(workspace_identity.clone())
                .await?
                .map(|run| run.run_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(tr(MessageId::CanonicalNoRecoverableRun).into_owned())
                })?
        } else {
            RunId::from(resume_id)
        };
        let _ = run_client
            .attach_or_resume(run_id, Some(workspace_identity.clone()))
            .await?;
        app.is_loading = true;
    } else {
        match recover_creation_at_startup(&run_client, workspace_identity).await? {
            StartupCreationRecovery::None => {}
            StartupCreationRecovery::Recovered { run_id, active } => {
                suppress_automatic_initial_submit = true;
                app.is_loading = active;
                app.status_message = Some(
                    app.tr(MessageId::CanonicalRecoveringInterruptedRun)
                        .replace("{run_id}", &run_id.to_string()),
                );
            }
            StartupCreationRecovery::Warning(message) => {
                suppress_automatic_initial_submit = true;
                app.status_message = Some(message.clone());
                app.add_message(HistoryCell::System { content: message });
            }
        }
    }

    if suppress_automatic_initial_submit {
        // Keep a CLI-supplied initial prompt in the composer. Pressing Enter is
        // then a conscious new creation with a fresh request identity; startup
        // recovery never replays it implicitly.
        app.auto_submit_initial_input = false;
    } else if app.auto_submit_initial_input {
        app.auto_submit_initial_input = false;
        if !matches!(
            canonical_commands::parse(&app.input),
            CanonicalSlashParse::NotCommand
        ) {
            app.status_message = Some(
                app.tr(MessageId::CanonicalInitialCommandConfirmation)
                    .into_owned(),
            );
        } else if let Some(input) = app.submit_input() {
            let _ = run_client
                .submit(canonical_start_command(&app, config, input))
                .await?;
            app.is_loading = true;
        }
    }

    crate::startup_trace::log_summary();
    let result = run_canonical_event_loop(
        &mut terminal,
        &mut app,
        config,
        &run_client,
        &mut run_events,
        &input,
    )
    .await;

    cleanup_guard.defused = true;
    pop_keyboard_enhancement_flags(terminal.backend_mut());
    disable_alternate_scroll_mode(terminal.backend_mut());
    execute!(terminal.backend_mut(), DisableFocusChange)?;
    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        #[cfg(windows)]
        crate::logging::restore_verbose_state();
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    if use_bracketed_paste {
        disable_bracketed_paste_mode(terminal.backend_mut());
    }
    terminal.show_cursor()?;
    drop(terminal);

    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupCreationRecovery {
    None,
    Recovered { run_id: RunId, active: bool },
    Warning(String),
}

async fn recover_creation_at_startup(
    run_client: &TuiRunClient,
    workspace: String,
) -> Result<StartupCreationRecovery, TuiRunClientError> {
    match run_client.recover_pending_for_workspace(workspace).await {
        Ok(None) => Ok(StartupCreationRecovery::None),
        Ok(Some(run)) => Ok(StartupCreationRecovery::Recovered {
            run_id: run.run_id,
            active: run.terminal.is_none(),
        }),
        Err(TuiRunClientError::Application(error))
            if error
                .creation
                .as_deref()
                .is_some_and(|creation| creation.unknown_billing) =>
        {
            let creation = error
                .creation
                .as_deref()
                .expect("unknown-billing guard requires creation context");
            let run = error.run_id.as_ref().map_or_else(
                || tr(MessageId::CanonicalUnknownValue).into_owned(),
                ToString::to_string,
            );
            Ok(StartupCreationRecovery::Warning(
                tr(MessageId::CanonicalUnknownBillingCreation)
                    .replace("{creation}", &creation.creation_request_id)
                    .replace("{run}", &run),
            ))
        }
        Err(TuiRunClientError::AmbiguousPendingCreations {
            workspace,
            creation_request_ids,
        }) => {
            let count = creation_request_ids.len();
            let preview = creation_request_ids
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("、");
            let message_id = if count > 3 {
                MessageId::CanonicalAmbiguousPendingCreationsMore
            } else {
                MessageId::CanonicalAmbiguousPendingCreations
            };
            Ok(StartupCreationRecovery::Warning(
                tr(message_id)
                    .replace("{workspace}", &format!("{workspace:?}"))
                    .replace("{count}", &count.to_string())
                    .replace("{preview}", &preview),
            ))
        }
        Err(error) => Err(error),
    }
}

/// Complete first-run setup before constructing the sole production
/// `AgentApplication`.
///
/// This loop deliberately has no provider picker, model call, Engine command,
/// session writer, or runtime-thread owner. It persists only the official
/// DeepSeek key, workspace trust, and the onboarding marker.
async fn run_deepseek_onboarding_loop(
    terminal: &mut AppTerminal,
    app: &mut App,
    config: &mut Config,
    input: &TerminalInputPump,
) -> Result<bool> {
    while app.onboarding != OnboardingState::None {
        draw_app_frame_inner(terminal, app, true)?;
        let Some(event) = input.recv_timeout(Duration::from_millis(UI_ACTIVE_POLL_MS))? else {
            continue;
        };
        match event {
            Event::Paste(text) if app.onboarding == OnboardingState::ApiKey => {
                app.insert_api_key_str(&text);
                onboarding::sync_api_key_validation_status(app, false);
            }
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                if control && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
                    return Ok(true);
                }
                match key.code {
                    KeyCode::Esc if app.onboarding == OnboardingState::ApiKey => {
                        back_from_api_key_onboarding(app);
                    }
                    KeyCode::Esc if app.onboarding == OnboardingState::TrustDirectory => {
                        return Ok(true);
                    }
                    KeyCode::Enter => match app.onboarding {
                        OnboardingState::Welcome => {
                            onboarding::advance_onboarding_from_welcome(app);
                        }
                        OnboardingState::ApiKey => {
                            let key = app.api_key_input.trim().to_owned();
                            match onboarding::validate_api_key_for_onboarding(&key) {
                                onboarding::ApiKeyValidation::Reject(message) => {
                                    app.status_message = Some(message);
                                }
                                onboarding::ApiKeyValidation::Accept { warning } => {
                                    let config_path = app.config_path.as_deref();
                                    crate::config_persistence::persist_root_string_key(
                                        config_path,
                                        "api_key",
                                        &key,
                                    )?;
                                    config.api_key = Some(key);
                                    app.api_key_input.clear();
                                    app.api_key_cursor = 0;
                                    app.onboarding_needs_api_key = false;
                                    app.api_key_env_only = false;
                                    app.status_message = warning;
                                    onboarding::advance_onboarding_after_api_key(app);
                                }
                            }
                        }
                        OnboardingState::TrustDirectory => {
                            app.status_message =
                                Some(app.tr(MessageId::OnboardTrustConfirmHint).into_owned());
                        }
                        OnboardingState::Tips => {
                            app.finish_onboarding_without_feature_intro();
                        }
                        OnboardingState::None => {}
                    },
                    KeyCode::Char('y' | 'Y' | '1')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        if let Err(error) = complete_trust_directory_onboarding(app) {
                            app.status_message = Some(
                                app.tr(MessageId::OnboardTrustSaveFailed)
                                    .replace("{error}", &error),
                            );
                        }
                    }
                    KeyCode::Char('n' | 'N' | '2')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        return Ok(true);
                    }
                    KeyCode::Backspace if app.onboarding == OnboardingState::ApiKey => {
                        app.delete_api_key_char();
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    KeyCode::Char(character)
                        if app.onboarding == OnboardingState::ApiKey
                            && key_shortcuts::is_text_input_key(&key) =>
                    {
                        app.insert_api_key_char(character);
                        onboarding::sync_api_key_validation_status(app, false);
                    }
                    _ => {}
                }
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
            _ => {}
        }
        app.needs_redraw = true;
    }
    Ok(false)
}

fn canonical_start_command(app: &App, config: &Config, input: String) -> StartRunCommand {
    let reasoning_effort = match app.reasoning_effort {
        ReasoningEffort::Off => RuntimeReasoningEffort::Off,
        ReasoningEffort::Low => RuntimeReasoningEffort::Low,
        ReasoningEffort::Medium => RuntimeReasoningEffort::Medium,
        ReasoningEffort::High => RuntimeReasoningEffort::High,
        ReasoningEffort::Auto => RuntimeReasoningEffort::Auto,
        ReasoningEffort::Max => RuntimeReasoningEffort::Max,
    };
    let mut limits = RunLimits::default();
    let subagents = crate::exec_runtime::runtime_subagent_limits(config, app.max_subagents);
    limits.max_depth = subagents.max_depth;
    limits.max_concurrent_children = subagents.max_concurrent_children;
    StartRunCommand {
        task: TaskDefinition::host(input),
        workspace: app.workspace.display().to_string(),
        model: (!app.auto_model).then(|| app.model.clone()),
        reasoning_effort,
        max_output_tokens: Some(384_000),
        max_api_requests: None,
        streaming: true,
        tool_policy: ToolPolicy {
            enabled: true,
            allowed: None,
            denied: Vec::new(),
        },
        limits,
        controls: RunProductControls {
            write_execution_mode: Default::default(),
            auto_approve: app_auto_approve_enabled(app),
            trust_mode: app.trust_mode,
            allow_sandbox_elevation: false,
            interactive: true,
            sandbox: config.sandbox_mode.clone(),
        },
    }
}

#[allow(clippy::too_many_lines)]
async fn run_canonical_event_loop(
    terminal: &mut AppTerminal,
    app: &mut App,
    config: &Config,
    run_client: &TuiRunClient,
    run_events: &mut tokio::sync::mpsc::Receiver<
        codewhale_protocol::agent_runtime::StoredRuntimeEvent,
    >,
    input: &TerminalInputPump,
) -> Result<()> {
    let mut projection = CanonicalRunProjection::new();
    let mut presented_interaction_id = None;
    let mut exit_after_terminal = false;
    let mut last_frame = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

    loop {
        while let Ok(stored) = run_events.try_recv() {
            for effect in projection
                .apply(stored)
                .map_err(canonical_projection_error)?
            {
                if let Some(action) = present_effect(app, effect) {
                    apply_presenter_action(app, action, &mut presented_interaction_id);
                }
            }
            app.needs_redraw = true;
        }

        let now = Instant::now();

        let snapshot = run_client.snapshot().await;
        if exit_after_terminal && snapshot.current_active_root.is_none() && !app.is_loading {
            return Ok(());
        }

        if app.needs_redraw
            || now.saturating_duration_since(last_frame)
                >= Duration::from_millis(UI_UNDERWATER_ANIMATION_MS)
        {
            draw_app_frame_inner(terminal, app, false)?;
            app.needs_redraw = false;
            last_frame = now;
        }

        let Some(event) = input.recv_timeout(Duration::from_millis(UI_ACTIVE_POLL_MS))? else {
            continue;
        };
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if handle_canonical_key(app, config, run_client, key, &mut exit_after_terminal)
                    .await?
                {
                    return Ok(());
                }
            }
            Event::Paste(text) => {
                if app.view_stack.is_empty() {
                    app.insert_paste_text(&text);
                } else {
                    let _ = app.view_stack.handle_paste(&text);
                }
                app.needs_redraw = true;
            }
            Event::Mouse(mouse) => {
                let view_events = route_canonical_mouse_event(app, mouse);
                handle_canonical_view_events(
                    app,
                    run_client,
                    view_events,
                    &mut exit_after_terminal,
                )
                .await?;
                app.needs_redraw = true;
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {
                app.needs_redraw = true;
            }
            _ => {}
        }
    }
}

fn route_canonical_mouse_event(app: &mut App, mouse: MouseEvent) -> Vec<ViewEvent> {
    if !app.view_stack.is_empty() {
        return app.view_stack.handle_mouse(mouse);
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => app.scroll_up(3),
        MouseEventKind::ScrollDown => app.scroll_down(3),
        _ => {}
    }
    Vec::new()
}

fn select_previous_slash_menu_entry(app: &mut App, entry_count: usize) {
    if entry_count == 0 {
        return;
    }
    let selected = app.slash_menu_selected.min(entry_count.saturating_sub(1));
    app.slash_menu_selected = (selected + entry_count - 1) % entry_count;
}

fn select_next_slash_menu_entry(app: &mut App, entry_count: usize) {
    if entry_count == 0 {
        return;
    }
    let selected = app.slash_menu_selected.min(entry_count.saturating_sub(1));
    app.slash_menu_selected = (selected + 1) % entry_count;
}

/// Handle one key while the canonical composer is showing an `@path` menu.
///
/// This path only edits ephemeral composer state. It never submits a Run,
/// reads the selected file, or expands local content into the request.
fn handle_open_mention_menu_key(app: &mut App, key: &KeyEvent, entries: &[String]) -> bool {
    if entries.is_empty() {
        return false;
    }
    match key.code {
        KeyCode::Enter
            if !key.modifiers.contains(KeyModifiers::SHIFT)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            crate::tui::file_mention::apply_mention_menu_selection(app, entries)
        }
        KeyCode::Tab => crate::tui::file_mention::apply_mention_menu_selection(app, entries),
        KeyCode::Up if key.modifiers.is_empty() => {
            app.mention_menu_selected = app.mention_menu_selected.saturating_sub(1);
            true
        }
        KeyCode::Down if key.modifiers.is_empty() => {
            app.mention_menu_selected =
                (app.mention_menu_selected + 1).min(entries.len().saturating_sub(1));
            true
        }
        KeyCode::Esc => {
            app.mention_menu_hidden = true;
            app.mention_menu_selected = 0;
            true
        }
        _ => false,
    }
}

async fn handle_canonical_key(
    app: &mut App,
    config: &Config,
    run_client: &TuiRunClient,
    key: KeyEvent,
    exit_after_terminal: &mut bool,
) -> Result<bool> {
    if !app.view_stack.is_empty() {
        let events = app.view_stack.handle_key(key);
        handle_canonical_view_events(app, run_client, events, exit_after_terminal).await?;
        app.needs_redraw = true;
        return Ok(false);
    }

    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if control && matches!(key.code, KeyCode::Char('d') | KeyCode::Char('D')) {
        if run_client.snapshot().await.current_active_root.is_some() {
            run_client.cancel().await?;
            *exit_after_terminal = true;
            app.status_message = Some(app.tr(MessageId::CanonicalCancelAwaitTerminal).into_owned());
            app.needs_redraw = true;
            return Ok(false);
        }
        if app.is_loading {
            app.status_message = Some(
                app.tr(MessageId::CanonicalWaitTerminalBeforeExit)
                    .into_owned(),
            );
            app.needs_redraw = true;
            return Ok(false);
        }
        return Ok(true);
    }
    if control && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
        if run_client.snapshot().await.current_active_root.is_some() {
            run_client.interrupt().await?;
            app.status_message = Some(app.tr(MessageId::CanonicalInterruptAccepted).into_owned());
        } else if !app.input.is_empty() {
            app.clear_input();
        } else if app.is_loading {
            app.status_message = Some(app.tr(MessageId::CanonicalWaitTerminal).into_owned());
        } else {
            return Ok(true);
        }
        app.needs_redraw = true;
        return Ok(false);
    }
    let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
    let mention_menu_limit = app.mention_menu_limit;
    let mention_menu_entries =
        crate::tui::file_mention::visible_mention_menu_entries(app, mention_menu_limit);
    if handle_open_mention_menu_key(app, &key, &mention_menu_entries) {
        app.needs_redraw = true;
        return Ok(false);
    }
    match key.code {
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.insert_char('\n');
        }
        KeyCode::Enter => {
            if !slash_menu_entries.is_empty() {
                let _ = apply_slash_menu_selection(app, &slash_menu_entries);
            }
            let Some(input) = app.handle_composer_enter() else {
                return Ok(false);
            };
            match canonical_commands::parse(&input) {
                CanonicalSlashParse::Command(CanonicalSlashCommand::Exit) => {
                    if run_client.snapshot().await.current_active_root.is_some() {
                        run_client.cancel().await?;
                        *exit_after_terminal = true;
                        app.status_message =
                            Some(app.tr(MessageId::CanonicalCancelBeforeExit).into_owned());
                        return Ok(false);
                    }
                    if app.is_loading {
                        app.status_message = Some(
                            app.tr(MessageId::CanonicalWaitTerminalBeforeExit)
                                .into_owned(),
                        );
                        return Ok(false);
                    }
                    return Ok(true);
                }
                CanonicalSlashParse::Command(CanonicalSlashCommand::Help) => {
                    app.add_message(HistoryCell::System {
                        content: canonical_commands::help_text(),
                    });
                    app.status_message = Some(app.tr(MessageId::CanonicalHelpShown).into_owned());
                }
                CanonicalSlashParse::Command(CanonicalSlashCommand::Cost) => {
                    let total = app.total_cost_for_currency(app.cost_currency);
                    let content = tr(MessageId::CmdCostReport)
                        .replace("{cost}", &app.format_cost_amount_precise(total));
                    app.add_message(HistoryCell::System { content });
                    app.status_message = Some(app.tr(MessageId::CanonicalCostShown).into_owned());
                }
                CanonicalSlashParse::Error(message) => {
                    app.insert_str(&input);
                    app.status_message = Some(message);
                }
                CanonicalSlashParse::NotCommand => {
                    let snapshot = run_client.snapshot().await;
                    if snapshot.current_active_root.is_some() {
                        match run_client.steer(input.clone()).await {
                            Ok(_) => {}
                            Err(error) => {
                                app.insert_str(&input);
                                app.status_message = Some(
                                    app.tr(MessageId::CanonicalSteerSubmitFailed)
                                        .replace("{error}", &error.to_string()),
                                );
                            }
                        }
                    } else if app.is_loading {
                        app.insert_str(&input);
                        app.status_message =
                            Some(app.tr(MessageId::CanonicalWaitBeforeNextInput).into_owned());
                    } else {
                        match run_client
                            .submit(canonical_start_command(app, config, input.clone()))
                            .await
                        {
                            Ok(_) => app.is_loading = true,
                            Err(error) => {
                                app.insert_str(&input);
                                app.status_message = Some(
                                    app.tr(MessageId::CanonicalRunSubmitFailed)
                                        .replace("{error}", &error.to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Tab => {
            let _ = try_autocomplete_slash_command(app);
        }
        KeyCode::Up if !slash_menu_entries.is_empty() => {
            select_previous_slash_menu_entry(app, slash_menu_entries.len());
        }
        KeyCode::Down if !slash_menu_entries.is_empty() => {
            select_next_slash_menu_entry(app, slash_menu_entries.len());
        }
        KeyCode::Esc if !slash_menu_entries.is_empty() => {
            app.close_slash_menu();
        }
        KeyCode::Esc => {
            if !app.input.is_empty() {
                app.clear_input();
            } else if run_client.snapshot().await.current_active_root.is_some() {
                run_client.interrupt().await?;
                app.status_message =
                    Some(app.tr(MessageId::CanonicalInterruptAccepted).into_owned());
            } else if app.is_loading {
                app.status_message = Some(app.tr(MessageId::CanonicalWaitTerminal).into_owned());
            } else {
                return Ok(true);
            }
        }
        KeyCode::Backspace => app.delete_char(),
        KeyCode::Delete => app.delete_char_forward(),
        KeyCode::Left => app.move_cursor_left(),
        KeyCode::Right => app.move_cursor_right(),
        KeyCode::Home => app.move_cursor_start(),
        KeyCode::End => app.move_cursor_end(),
        KeyCode::PageUp => app.scroll_up(12),
        KeyCode::PageDown => app.scroll_down(12),
        KeyCode::Up if app.input.is_empty() => app.scroll_up(3),
        KeyCode::Down if app.input.is_empty() => app.scroll_down(3),
        KeyCode::Char('a') if control => app.move_cursor_start(),
        KeyCode::Char('e') if control => app.move_cursor_end(),
        KeyCode::Char('w') if control => app.delete_word_backward(),
        KeyCode::Char(character)
            if !control
                && !key.modifiers.contains(KeyModifiers::SUPER)
                && !character.is_control() =>
        {
            app.insert_char(character);
        }
        _ => {}
    }
    app.needs_redraw = true;
    Ok(false)
}

async fn handle_canonical_view_events(
    app: &mut App,
    run_client: &TuiRunClient,
    events: Vec<ViewEvent>,
    exit_after_terminal: &mut bool,
) -> Result<()> {
    for event in events {
        let Some(event) = handle_canonical_local_view_event(app, event) else {
            continue;
        };
        match event {
            ViewEvent::ApprovalDecision {
                interaction_id,
                decision,
            } => match decision {
                ReviewDecision::Approved => {
                    run_client
                        .resolve_interaction(
                            codewhale_protocol::agent_runtime::InteractionId::from(interaction_id),
                            UserInteractionResponse::Approved,
                        )
                        .await?;
                }
                ReviewDecision::Denied => {
                    run_client
                        .resolve_interaction(
                            codewhale_protocol::agent_runtime::InteractionId::from(interaction_id),
                            UserInteractionResponse::Denied { reason: None },
                        )
                        .await?;
                }
                ReviewDecision::Abort => {
                    run_client.cancel().await?;
                    *exit_after_terminal = false;
                }
            },
            ViewEvent::UserInputSubmitted { tool_id, response } => {
                run_client
                    .resolve_interaction(
                        codewhale_protocol::agent_runtime::InteractionId::from(tool_id),
                        response,
                    )
                    .await?;
            }
            ViewEvent::UserInputCancelled { tool_id } => {
                run_client
                    .resolve_interaction(
                        codewhale_protocol::agent_runtime::InteractionId::from(tool_id),
                        UserInteractionResponse::Cancelled,
                    )
                    .await?;
            }
            _ => {
                app.status_message = Some(
                    app.tr(MessageId::CanonicalLegacyActionUnavailable)
                        .into_owned(),
                );
            }
        }
    }
    Ok(())
}

/// Handle modal events that only affect the local TUI projection. These do
/// not create Runtime events or durable state; approval remains pending while
/// its full arguments are inspected in the pager.
fn handle_canonical_local_view_event(app: &mut App, event: ViewEvent) -> Option<ViewEvent> {
    match event {
        ViewEvent::OpenTextPager { title, content } => {
            let width = app
                .viewport
                .last_transcript_area
                .map(|area| area.width)
                .unwrap_or(80)
                .saturating_sub(2);
            app.view_stack
                .push(PagerView::from_text(title, &content, width));
            None
        }
        ViewEvent::CopyToClipboard { text, label } => {
            app.status_message = Some(match app.clipboard.write_text(&text) {
                Ok(()) => format!("{label}已复制到剪贴板"),
                Err(error) => format!("{label}复制失败：{error}"),
            });
            None
        }
        event => Some(event),
    }
}

fn apply_presenter_action(
    app: &mut App,
    action: PresenterAction,
    presented_interaction_id: &mut Option<codewhale_protocol::agent_runtime::InteractionId>,
) {
    match action {
        PresenterAction::ShowInteraction(request) => {
            let interaction_id = request.interaction_id.clone();
            *presented_interaction_id = Some(interaction_id.clone());
            match request.prompt {
                UserInteractionPrompt::Approval { prompt, arguments } => {
                    let approval = ApprovalRequest::new_with_intent(
                        &interaction_id.0,
                        &request.tool_name,
                        &arguments,
                        project_approval_risk(prompt.risk),
                        Some(&prompt.title),
                    );
                    app.view_stack.push(ApprovalView::new(approval));
                }
                UserInteractionPrompt::UserInput { request } => {
                    app.view_stack
                        .push(UserInputView::new(interaction_id.0, request));
                }
            }
        }
        PresenterAction::InteractionResolved { interaction_id, .. } => {
            if presented_interaction_id.as_ref() != Some(&interaction_id) {
                app.status_message = Some(
                    app.tr(MessageId::CanonicalMismatchedInteractionReceipt)
                        .replace("{interaction_id}", &interaction_id.0),
                );
                return;
            }
            *presented_interaction_id = None;
            if matches!(
                app.view_stack.top_kind(),
                Some(ModalKind::Approval | ModalKind::UserInput)
            ) {
                let _ = app.view_stack.pop();
            }
        }
    }
}

fn project_approval_risk(risk: ApprovalRisk) -> super::approval::ApprovalStakes {
    match risk {
        ApprovalRisk::Routine => super::approval::ApprovalStakes::Routine,
        ApprovalRisk::Elevated => super::approval::ApprovalStakes::Elevated,
        ApprovalRisk::Critical => super::approval::ApprovalStakes::Critical,
    }
}

/// One side of the raw-mode probe abandonment handshake between the startup
/// probe timeout and the blocking `enable_raw_mode` task finishing late.
///
/// Each side publishes its own flag (`publish`), then checks whether the
/// other side's flag (`check`) is already up; a `true` return means this
/// side must disable raw mode again. `SeqCst` ordering guarantees that when
/// both sides run, at least one observes the other's flag, so a raw-mode
/// enable landing after the probe timeout is always undone. Both sides
/// observing each other is fine — a duplicate `disable_raw_mode` is a no-op.
fn raw_mode_probe_handshake(publish: &AtomicBool, check: &AtomicBool) -> bool {
    publish.store(true, Ordering::SeqCst);
    check.load(Ordering::SeqCst)
}

fn raw_mode_enable_error(error: io::Error) -> anyhow::Error {
    anyhow::anyhow!("启用终端 raw mode 失败：{error}")
}

fn terminal_probe_timeout_error(timeout: Duration) -> anyhow::Error {
    anyhow::anyhow!("终端探测在 {}ms 后超时", timeout.as_millis())
}

fn canonical_projection_error(error: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("canonical TUI 事件投影失败：{error}")
}

fn terminal_probe_timeout(config: &Config) -> Duration {
    let timeout_ms = config
        .tui
        .as_ref()
        .and_then(|tui| tui.terminal_probe_timeout_ms)
        .unwrap_or(DEFAULT_TERMINAL_PROBE_TIMEOUT_MS)
        .clamp(100, 5_000);
    Duration::from_millis(timeout_ms)
}

struct TerminalCleanupGuard {
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
    defused: bool,
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        if self.defused {
            return;
        }

        let mut stdout = io::stdout();
        pop_keyboard_enhancement_flags(&mut stdout);
        disable_alternate_scroll_mode(&mut stdout);
        let _ = execute!(stdout, DisableFocusChange);
        let _ = disable_raw_mode();
        if self.use_alt_screen {
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
        if self.use_mouse_capture {
            let _ = execute!(stdout, DisableMouseCapture);
        }
        if self.use_bracketed_paste {
            disable_bracketed_paste_mode(&mut stdout);
        }
        let _ = execute!(stdout, crossterm::cursor::Show);
    }
}

fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Clear entire area with the configured app background.
    let background = Block::default().style(Style::default().bg(app.ui_theme.surface_bg));
    f.render_widget(background, size);

    // Show onboarding screen if needed
    if app.onboarding != OnboardingState::None {
        onboarding::render(f, size, app);
        return;
    }

    let header_height = if size.height < 16 { 1 } else { 2 };
    let footer_height = crate::tui::phase_strip::height();
    let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
    let mention_menu_limit = app.mention_menu_limit;
    let mention_menu_entries =
        crate::tui::file_mention::visible_mention_menu_entries(app, mention_menu_limit);
    if !mention_menu_entries.is_empty() && app.mention_menu_selected >= mention_menu_entries.len() {
        app.mention_menu_selected = mention_menu_entries.len().saturating_sub(1);
    }
    let top_work_strip_height = super::work_surface::height(app, size.width, size.height);

    // Defensive two-pass layout: pin the header to the absolute top row,
    // then split the remaining body area for chat / composer / footer. This
    // guarantees the header is never vertically centered
    // regardless of ratatui Flex defaults or terminal size.
    // Fixes #1834 — macOS terminal title centering.
    let (header_area, body_area) = {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .flex(ratatui::layout::Flex::Start)
            .constraints([Constraint::Length(header_height), Constraint::Min(1)])
            .split(size);
        (split[0], split[1])
    };

    let body_height = body_area.height;
    let composer_max_height = body_height
        .saturating_sub(MIN_CHAT_HEIGHT + footer_height + top_work_strip_height)
        .max(MIN_COMPOSER_HEIGHT);
    let composer_height = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        composer_widget.desired_height(size.width)
    };

    // Ocean live phases put the phase strip above the composer so activity
    // stays attached to the transcript and the prompt is the final bottom
    // object. Idle/typing keep a quiet phase under the prompt.
    let phase = crate::tui::underwater::ShellPhase::from_app(app);
    let phase_above =
        crate::tui::phase_strip::PhaseStripPlacement::for_phase(phase).is_above_composer();
    let (composer_slot, footer_slot, tail_constraints) = if phase_above {
        (
            3,
            2,
            [
                Constraint::Length(footer_height),
                Constraint::Length(composer_height),
            ],
        )
    } else {
        (
            2,
            3,
            [
                Constraint::Length(composer_height),
                Constraint::Length(footer_height),
            ],
        )
    };

    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .flex(ratatui::layout::Flex::Start)
        .constraints([
            Constraint::Length(top_work_strip_height), // Tasks + Runs above transcript
            Constraint::Min(1),                        // Chat area
            tail_constraints[0],
            tail_constraints[1],
        ])
        .split(body_area);

    let (work_chat_area, side_work_area) = super::work_surface::split_chat(app, body_chunks[1]);

    if top_work_strip_height > 0 {
        super::work_surface::render(f, body_chunks[0], app);
    } else if let Some(work_area) = side_work_area {
        super::work_surface::render(f, work_area, app);
    }

    crate::tui::underwater::render_header(header_area, f.buffer_mut(), app);

    // Render the transcript. The canonical work surface owns task and worker
    // facts, Fleet owns `/fleet`, and dense context owns its inspector.
    let shell_ocean;
    {
        // Defensive backstop (#400): fill the entire body area with ink
        // background before any sub-widgets render, so cells that end up
        // uncovered by layout splits after a resize don't retain stale content
        // from a previous frame.
        Block::default()
            .style(Style::default().bg(app.ui_theme.surface_bg))
            .render(work_chat_area, f.buffer_mut());

        let chat_widget = ChatWidget::new(app, work_chat_area).with_ocean_viewport(size);
        shell_ocean = chat_widget.ocean_column();
        let buf = f.buffer_mut();
        chat_widget.render(work_chat_area, buf);
    }

    // Render composer
    let cursor_pos = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        let buf = f.buffer_mut();
        composer_widget.render(body_chunks[composer_slot], buf);
        composer_widget.cursor_pos(body_chunks[composer_slot])
    };
    if let Some(cursor_pos) = cursor_pos {
        f.set_cursor_position(cursor_pos);
    }

    crate::tui::underwater::render_footer(body_chunks[footer_slot], f.buffer_mut(), app);

    // The underwater shell is one water column, not a stack of independently
    // shaded panels. Continue the transcript's absolute-row ramp through each
    // ordinary shell surface after its foreground has rendered. Semantic
    // backgrounds such as selection, hover, errors, and code blocks do not
    // match these base colors and therefore remain intact.
    if let Some(column) = shell_ocean {
        column.paint_matching(header_area, f.buffer_mut(), app.ui_theme.header_bg);
        if top_work_strip_height > 0 {
            column.paint_matching(body_chunks[0], f.buffer_mut(), app.ui_theme.surface_bg);
        }
        if let Some(side_area) = side_work_area {
            column.paint_matching(side_area, f.buffer_mut(), app.ui_theme.surface_bg);
        }
        column.paint_matching(work_chat_area, f.buffer_mut(), app.ui_theme.surface_bg);
        column.paint_matching(body_chunks[2], f.buffer_mut(), app.ui_theme.surface_bg);
        column.paint_matching(body_chunks[3], f.buffer_mut(), app.ui_theme.surface_bg);
        column.paint_matching(
            body_chunks[composer_slot],
            f.buffer_mut(),
            app.ui_theme.composer_bg,
        );
        column.paint_matching(
            body_chunks[footer_slot],
            f.buffer_mut(),
            app.ui_theme.footer_bg,
        );
    }
    if !app.view_stack.is_empty() {
        let buf = f.buffer_mut();
        app.view_stack.render(size, buf);
    }
}

/// Draw a complete application frame, optionally with a full viewport reset.
///
/// When `full_repaint` is true, the terminal scroll margins and origin mode
/// are reset, the screen is cleared, ratatui's buffer is emptied, and then
/// the full UI is drawn — all within a single DEC 2026 synchronized-update
/// batch so GPU-accelerated terminals (Ghostty, VS Code, Kitty) render one
/// complete frame instead of a blank intermediate frame followed by the UI.
///
/// When `full_repaint` is false, only the diff from the previous draw is
/// written (normal incremental update path).
fn draw_app_frame_inner(
    terminal: &mut AppTerminal,
    app: &mut App,
    full_repaint: bool,
) -> Result<()> {
    terminal.backend_mut().set_palette_mode(app.ui_theme.mode);
    terminal.backend_mut().set_theme(app.theme_id, app.ui_theme);
    // DEC 2026 wrapping is on by default but can be turned off for
    // terminals that mishandle it (Ptyxis 50.x + VTE 0.84.x flashes the
    // whole viewport on every wrapped frame instead of deferring as the
    // standard requires). Settings::synchronized_output_enabled resolves
    // the user's setting against the Ptyxis env auto-detect.
    let wrap_in_sync_update = app.synchronized_output_enabled;
    if wrap_in_sync_update {
        let _ = terminal.backend_mut().write_all(BEGIN_SYNC_UPDATE);
    }

    // Run fallible draw operations in a closure so END_SYNC_UPDATE is
    // always sent even if an intermediate step fails. Without this, a
    // failing `?` would return early and leave the terminal stuck in
    // synchronized-update mode (screen frozen).
    let result = (|| -> Result<()> {
        if full_repaint {
            terminal.backend_mut().write_all(TERMINAL_ORIGIN_RESET)?;
            terminal.clear()?;
        }
        terminal.draw(|f| render(f, app))?;
        Ok(())
    })();

    // Always end the synchronized update, regardless of success or failure.
    if wrap_in_sync_update {
        let _ = terminal.backend_mut().write_all(END_SYNC_UPDATE);
    }
    let _ = terminal.backend_mut().flush();
    result
}

fn reset_terminal_viewport(terminal: &mut AppTerminal, sync_output_enabled: bool) -> Result<()> {
    // Reset scroll margins and origin mode before clearing. Some interactive
    // child processes leave DECSTBM/DECOM behind; if ratatui's diff renderer
    // then writes "row 0", terminals can place it relative to the leaked
    // scroll region and the whole viewport appears shifted down. We
    // deliberately do *not* emit CSI 2J/3J here — see TERMINAL_ORIGIN_RESET
    // for why; the immediately-following ratatui `terminal.clear()` flushes a
    // single clear via the diff renderer, which the alt-screen buffer absorbs
    // without visible flicker on the affected terminals.
    //
    // Wrap the reset+clear sequence in DEC 2026 synchronized-output mode
    // (`\x1b[?2026h` … `\x1b[?2026l`) so GPU-accelerated terminals
    // (Ghostty, VSCode, Kitty, WezTerm) defer rendering until the whole
    // frame is staged. Terminals that don't support it silently ignore.
    // The wrap is opt-out via `synchronized_output = "off"` for terminals
    // that mishandle the sequence (Ptyxis 50.x on VTE 0.84.x flashes the
    // whole viewport on each wrapped frame).
    if sync_output_enabled {
        let _ = terminal.backend_mut().write_all(BEGIN_SYNC_UPDATE);
    }

    let result = (|| -> Result<()> {
        terminal.backend_mut().write_all(TERMINAL_ORIGIN_RESET)?;
        terminal.clear()?;
        Ok(())
    })();

    // Always end the synchronized update, regardless of success or failure.
    if sync_output_enabled {
        let _ = terminal.backend_mut().write_all(END_SYNC_UPDATE);
    }
    let _ = terminal.backend_mut().flush();
    result
}

fn push_keyboard_enhancement_flags<W: Write>(writer: &mut W) {
    // crossterm's PushKeyboardEnhancementFlags command unconditionally
    // returns Unsupported on Windows (is_ansi_code_supported() == false), so
    // the ANSI escape is written directly on that platform. Modern Windows
    // terminals (VSCode integrated terminal, Windows Terminal ≥1.17) honour
    // the kitty keyboard protocol but crossterm's event reader does not
    // decode CSI u sequences on Windows (issue #1599). Write \033[>0u to
    // probe the protocol without enabling any flags — Enter stays as \n.
    #[cfg(windows)]
    {
        if let Err(err) = write!(writer, "\x1b[>0u").and_then(|()| writer.flush()) {
            tracing::debug!(
                target: "kitty_keyboard",
                ?err,
                "PushKeyboardEnhancementFlags direct write failed on Windows"
            );
        }
    }
    #[cfg(not(windows))]
    if let Err(err) = execute!(
        writer,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    ) {
        tracing::debug!(
            target: "kitty_keyboard",
            ?err,
            "PushKeyboardEnhancementFlags ignored (terminal lacks support)"
        );
    }
}

pub(crate) fn pop_keyboard_enhancement_flags<W: Write>(writer: &mut W) {
    // Mirror of push_keyboard_enhancement_flags: crossterm's
    // PopKeyboardEnhancementFlags also has is_ansi_code_supported() == false
    // on Windows, so write the pop escape directly to restore the terminal to
    // its pre-launch keyboard mode.
    // pub(crate) so the panic hook in main.rs can also call the Windows-aware
    // path instead of using the raw crossterm
    // execute!() macro which silently no-ops on Windows.
    #[cfg(windows)]
    {
        if let Err(err) = write!(writer, "\x1b[<1u").and_then(|()| writer.flush()) {
            tracing::debug!(
                target: "kitty_keyboard",
                ?err,
                "PopKeyboardEnhancementFlags direct write failed on Windows"
            );
        }
    }
    #[cfg(not(windows))]
    let _ = execute!(writer, PopKeyboardEnhancementFlags);
}

fn set_alternate_scroll_mode<W: Write>(writer: &mut W, enabled: bool) {
    let sequence = if enabled {
        ENABLE_ALT_SCROLL_MODE
    } else {
        DISABLE_ALT_SCROLL_MODE
    };
    if let Err(err) = writer.write_all(sequence).and_then(|()| writer.flush()) {
        tracing::debug!(
            ?err,
            enabled,
            "alternate-scroll terminal mode change ignored"
        );
    }
}

fn enable_alternate_scroll_mode<W: Write>(writer: &mut W) {
    set_alternate_scroll_mode(writer, true);
}

pub(crate) fn disable_alternate_scroll_mode<W: Write>(writer: &mut W) {
    set_alternate_scroll_mode(writer, false);
}

/// Best-effort terminal restoration for emergency exit paths
/// (panic hook, signal handlers). Mirrors the normal teardown in
/// `run_event_loop` but tolerates any subset of modes not actually being
/// active — every step is discarded on failure so a half-initialized TUI
/// (e.g. SIGINT during startup before `EnterAlternateScreen`) still gets
/// raw mode + kitty keyboard flags cleared, which is what causes the
/// `^[[>5u` shell pollution reported in #1583.
pub fn emergency_restore_terminal() {
    let mut stdout = std::io::stdout();
    pop_keyboard_enhancement_flags(&mut stdout);
    disable_alternate_scroll_mode(&mut stdout);
    let _ = execute!(stdout, DisableFocusChange);
    disable_bracketed_paste_mode(&mut stdout);
    let _ = execute!(stdout, DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen);
}

/// On Windows, ensure the console input handle has `ENABLE_WINDOW_INPUT`
/// (0x0008) set. crossterm's `enable_raw_mode()` removes this flag, which
/// breaks IME composition (Chinese/Japanese/Korean input methods cannot
/// commit characters) on some Windows configurations (e.g. Windows Terminal
/// in conhost compatibility mode, or the legacy console with VT input).
///
/// Best-effort and idempotent. Silently ignored if the console handle or
/// mode query fails.
#[cfg(target_os = "windows")]
fn enable_windows_ime_console_mode() {
    use windows::Win32::System::Console::CONSOLE_MODE;
    const ENABLE_WINDOW_INPUT: CONSOLE_MODE = CONSOLE_MODE(0x0008);

    // SAFETY: Win32 console API is safe to call from any thread.
    // Failures (console handle invalid, mode query fails) are silently
    // ignored — this is a best-effort IME compatibility tweak.
    unsafe {
        let Ok(handle) = GetStdHandle(windows::Win32::System::Console::STD_INPUT_HANDLE) else {
            return;
        };
        let mut mode = CONSOLE_MODE(0);
        if GetConsoleMode(handle, &mut mode).is_err() {
            return;
        }
        if mode.0 & ENABLE_WINDOW_INPUT.0 == 0 {
            let _ = SetConsoleMode(handle, mode | ENABLE_WINDOW_INPUT);
        }
    }
}

/// Re-establish terminal mode flags. Idempotent and best-effort: each
/// underlying flag is silently discarded by terminals that don't support
/// it, and a single flag's failure doesn't prevent later flags from being
/// attempted.
///
/// **Canonical location for terminal-mode setup.** New mouse, paste, keyboard
/// or focus flags belong here so startup and terminal-mode tests stay aligned.
/// Raw mode and the alternate screen are established separately by `run_tui`.
///
pub(crate) fn recover_terminal_modes<W: Write>(
    writer: &mut W,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
) {
    #[cfg(target_os = "windows")]
    enable_windows_ime_console_mode();

    pop_keyboard_enhancement_flags(writer);
    push_keyboard_enhancement_flags(writer);
    if use_mouse_capture {
        enable_alternate_scroll_mode(writer);
        if let Err(err) = execute!(writer, EnableMouseCapture) {
            tracing::debug!(?err, "EnableMouseCapture ignored");
        }
    } else {
        disable_alternate_scroll_mode(writer);
    }
    if use_bracketed_paste {
        try_enable_bracketed_paste_mode(writer);
    }
    if let Err(err) = execute!(writer, EnableFocusChange) {
        tracing::debug!(?err, "EnableFocusChange ignored");
    }
}

fn try_enable_bracketed_paste_mode<W: Write>(writer: &mut W) -> bool {
    match execute!(writer, EnableBracketedPaste) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(?err, "EnableBracketedPaste ignored");
            false
        }
    }
}

pub(crate) fn disable_bracketed_paste_mode<W: Write>(writer: &mut W) {
    if let Err(err) = execute!(writer, DisableBracketedPaste) {
        tracing::debug!(?err, "DisableBracketedPaste ignored");
    }
}

pub(crate) fn status_color(level: StatusToastLevel) -> ratatui::style::Color {
    match level {
        StatusToastLevel::Info => palette::WHALE_INFO,
        StatusToastLevel::Success => palette::STATUS_SUCCESS,
        StatusToastLevel::Warning => palette::STATUS_WARNING,
        StatusToastLevel::Error => palette::STATUS_ERROR,
    }
}

pub(crate) fn context_usage_snapshot(app: &App) -> Option<(i64, u32, f64)> {
    let max = crate::route_budget::route_context_window_tokens(app.effective_model_for_budget());
    let max_i64 = i64::from(max);
    let used = app
        .session
        .last_prompt_tokens
        .map(i64::from)
        .map(|tokens| tokens.max(0).min(max_i64))?;

    let max_f64 = f64::from(max);
    let used_f64 = used as f64;
    let percent = ((used_f64 / max_f64) * 100.0).clamp(0.0, 100.0);
    Some((used, max, percent))
}

#[cfg(test)]
mod localized_canonical_surface_tests {
    use super::*;

    #[test]
    fn framework_error_prefixes_are_simplified_chinese_and_keep_raw_details() {
        let raw_mode = raw_mode_enable_error(io::Error::other("raw-detail")).to_string();
        assert_eq!(raw_mode, "启用终端 raw mode 失败：raw-detail");

        let timeout = terminal_probe_timeout_error(Duration::from_millis(1250)).to_string();
        assert_eq!(timeout, "终端探测在 1250ms 后超时");

        let projection = canonical_projection_error("sequence-detail").to_string();
        assert_eq!(projection, "canonical TUI 事件投影失败：sequence-detail");

        let warning = prompt_override_warning("override-detail");
        assert_eq!(warning, "警告：override-detail");

        for text in [&raw_mode, &timeout, &projection, &warning] {
            assert!(!text.contains("Warning:"));
            assert!(!text.contains("failed:"));
            assert!(!text.contains("timed out"));
        }
    }
}

#[cfg(test)]
mod tests;
