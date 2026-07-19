//! Settings system - Persistent user preferences
//!
//! Settings are stored at ~/.codewhale/settings.toml, with legacy fallbacks.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{ApiProvider, normalize_model_name};
use crate::palette::{normalize_hex_rgb_color, normalize_theme_name};

/// User settings with defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Reduce status noise and collapse details more aggressively
    pub calm_mode: bool,
    /// Dense tool-run collapse mode: compact, expanded, or calm.
    pub tool_collapse_mode: String,
    /// Reduce decorative motion. This must never synthesize model text speed;
    /// streaming follows upstream deltas in both modes.
    pub low_motion: bool,
    /// Enable expressive live-state motion. This affects chrome and state
    /// affordances only; model text always follows upstream stream deltas.
    pub fancy_animations: bool,
    /// Background treatment: `ombre` paints the terminal-native water column;
    /// `flat` preserves all state marks on the theme's plain surface.
    pub ocean_treatment: String,
    /// Ocean Tasks / Runs / Workers rail placement: top, left, or right.
    /// The lower edge remains owned by the composer and phase footer.
    pub work_surface_placement: String,
    /// Enable terminal bracketed-paste mode. Default true. Disable if your
    /// terminal mishandles the `\e[?2004h` escape (rare; some legacy
    /// terminals over SSH+screen multiplex without the cap).
    pub bracketed_paste: bool,
    /// Maximum number of file-mention popup candidates retained before the
    /// composer renders its visible window. The widget paginates by terminal
    /// height, so this is a data-side cap rather than a visible-row budget.
    pub mention_menu_limit: usize,
    /// Maximum workspace depth for `@`-mention completion walks. `0` means
    /// unlimited depth; use with care in very large repositories.
    pub mention_walk_depth: usize,
    /// `@`-mention completion behavior: fuzzy workspace search or deterministic
    /// directory browser.
    pub mention_menu_behavior: String,
    /// Show thinking blocks from the model
    pub show_thinking: bool,
    /// Show detailed tool output
    pub show_tool_details: bool,
    /// Named UI theme. Accepts `"system"` (follow terminal background),
    /// `"dark"`, `"light"`, `"grayscale"`, or one of the community
    /// presets: `"catppuccin-mocha"`, `"tokyo-night"`, `"dracula"`,
    /// `"gruvbox-dark"`. The `background_color` setting still overrides the
    /// surface color on top of the resolved theme.
    pub theme: String,
    /// Optional main TUI background color as a 6-digit hex RGB value.
    pub background_color: Option<String>,
    /// Composer layout density: compact, comfortable, spacious
    pub composer_density: String,
    /// Show a border around the composer input area
    pub composer_border: bool,
    /// Transcript spacing rhythm: compact, comfortable, spacious
    pub transcript_spacing: String,
    /// Sidebar width as percentage of terminal width
    pub sidebar_width_percent: u16,
    /// Sidebar focus mode: pinned, auto, tasks, agents, context, hidden
    pub sidebar_focus: String,
    /// Migration marker for users who explicitly opt into idle auto-collapse.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sidebar_auto_collapse_opt_in: bool,
    /// Enable the session-context panel (#504). Shows working set, tokens,
    /// cost, MCP status, cycle count, and memory info.
    pub context_panel: bool,
    /// Cost display currency: usd or cny.
    pub cost_currency: String,
    /// Default provider override (e.g. "deepseek", "openai").
    pub default_provider: Option<String>,
    /// Default model to use
    pub default_model: Option<String>,
    /// Default reasoning effort selected from the TUI model picker.
    /// `None` falls back to `config.toml` and then the runtime default.
    pub reasoning_effort: Option<String>,
    /// Per-provider model overrides. Key is provider name (e.g. "openai"),
    /// value is the model id. Takes precedence over `default_model`.
    pub provider_models: Option<std::collections::HashMap<String, String>>,
    /// Header status indicator next to the effort chip. Cycles through a
    /// per-turn animation keyed off `App::turn_started_at`:
    /// - `"cw"` (default): static typographic CodeWhale mark.
    /// - `"whale"`: historical `🐳 → 🐋` 12-frame sequence
    ///   originally shipped in v0.3.5, removed in v0.8.x's "smoother TUI
    ///   streaming" pass, restored in v0.8.30. Idle frame is a steady `🐳`.
    /// - `"dots"`: the 6-frame geometric sequence (`◍ ◉ ◌ ◌ ◉ ◍`) that
    ///   replaced the whale during the dots era.
    /// - `"off"`: hide the indicator entirely.
    pub status_indicator: String,
    /// Whether to wrap each draw in DEC mode 2026 synchronized output
    /// (`\x1b[?2026h` … `\x1b[?2026l`). Synchronized output asks the
    /// terminal to defer rendering until the whole frame is staged so
    /// GPU-accelerated terminals (Ghostty, VS Code, Kitty, WezTerm)
    /// don't flash a blank intermediate frame.
    ///
    /// - `"auto"` (default): emit DEC 2026 unless an environment signal
    ///   says the active terminal mishandles it (currently Ptyxis 50.x
    ///   on VTE 0.84.x — see [`Settings::apply_env_overrides`]).
    /// - `"on"`: always emit DEC 2026 (override the auto opt-out).
    /// - `"off"`: never emit DEC 2026. Use this if your terminal flashes
    ///   the whole screen on every redraw — most often Ptyxis on
    ///   Ubuntu 26.04 today; historically also some legacy ssh+screen
    ///   stacks. The cost of `off` is brief tearing on terminals that
    ///   *do* support DEC 2026; it is purely a rendering-quality knob,
    ///   not a correctness one.
    pub synchronized_output: String,
    /// Prefer the external `pdftotext` binary (Poppler) over the bundled
    /// pure-Rust `pdf-extract` extractor for PDF reads in `read_file`.
    /// Pure-Rust extraction is the v0.8.32 default because it removes the
    /// install-poppler-first hurdle most users hit, but `pdftotext -layout`
    /// still wins for column-heavy or complex-table PDFs (academic papers
    /// laid out in two columns, financial filings, etc.). Set to `true` to
    /// route every PDF read through `pdftotext` instead — when the binary
    /// is missing in that mode the tool returns the structured
    /// `binary_unavailable` response with an install hint, matching the
    /// pre-v0.8.32 behavior.
    pub prefer_external_pdftotext: bool,
    /// Follow symbolic links during workspace file discovery walks (`@`-mention
    /// completion, fuzzy resolve, and the file-index builder). When `false`
    /// (default) symlinked directories are skipped, which keeps walks fast and
    /// avoids accidentally traversing into system paths. Set to `true` to
    /// support symlink-based multi-project workspaces where several project
    /// directories are symlinked into a single hub directory.
    ///
    /// **Note**: The walker has built-in cycle detection that skips already-
    /// visited real paths, so symlink loops (A→B→A) will not cause infinite
    /// recursion. However, enabling this on workspaces with symlinks that
    /// point to large directory trees (e.g. `/usr`, home directories) can
    /// significantly increase first-turn latency and memory usage.
    pub workspace_follow_symlinks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // #4095: default presentation is compact/calm; verbose detail is opt-in.
            calm_mode: true,
            tool_collapse_mode: "compact".to_string(),
            low_motion: false,
            fancy_animations: true,
            ocean_treatment: "ombre".to_string(),
            work_surface_placement: "top".to_string(),
            bracketed_paste: true,
            mention_menu_limit: 128,
            mention_walk_depth: 10,
            mention_menu_behavior: "fuzzy".to_string(),
            // Reasoning is useful when explicitly requested, but it should
            // never displace the actual conversation in the default TUI.
            show_thinking: false,
            show_tool_details: false,
            theme: "system".to_string(),
            background_color: None,
            composer_density: "comfortable".to_string(),
            composer_border: true,
            transcript_spacing: "comfortable".to_string(),
            sidebar_width_percent: 28,
            sidebar_focus: "auto".to_string(),
            sidebar_auto_collapse_opt_in: true,
            context_panel: false,
            cost_currency: "usd".to_string(),
            default_provider: None,
            default_model: None,
            reasoning_effort: None,
            provider_models: None,
            status_indicator: "cw".to_string(),
            synchronized_output: "auto".to_string(),
            prefer_external_pdftotext: false,
            workspace_follow_symlinks: false,
        }
    }
}

fn normalize_ocean_treatment(value: &str) -> &'static str {
    if value.trim().eq_ignore_ascii_case("flat") {
        "flat"
    } else {
        "ombre"
    }
}

fn normalize_work_surface_placement(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => "left",
        "right" => "right",
        _ => "top",
    }
}

impl Settings {
    /// Prompt-only projection consumed by the transport-neutral context owner.
    #[must_use]
    pub fn prompt_preferences(&self) -> codewhale_config::PromptPreferences {
        codewhale_config::PromptPreferences {
            show_thinking: self.show_thinking,
        }
    }

    /// Get the canonical settings file path.
    ///
    /// New writes should target `~/.codewhale/settings.toml`. Legacy
    /// DeepSeek-branded paths remain readable as fallbacks during load.
    pub fn path() -> Result<PathBuf> {
        codewhale_config::settings_path()
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Result<Self> {
        let mut settings = Self::load_persisted()?;
        settings.apply_env_overrides();
        Ok(settings)
    }

    /// Load the normalized values stored on disk without terminal/runtime
    /// overlays. Configuration editors use this path so a value labelled
    /// "saved" never silently reports a tmux, SSH, or accessibility override.
    pub(crate) fn load_persisted() -> Result<Self> {
        let source = codewhale_config::load_settings_source()?;
        let settings = match source.deserialize::<Settings>() {
            Ok(None) => Self::default(),
            Ok(Some(mut s)) => {
                s.composer_density = normalize_composer_density(&s.composer_density).to_string();
                s.transcript_spacing =
                    normalize_transcript_spacing(&s.transcript_spacing).to_string();
                s.tool_collapse_mode =
                    normalize_tool_collapse_mode(&s.tool_collapse_mode).to_string();
                s.sidebar_focus = normalize_sidebar_focus(&s.sidebar_focus).to_string();
                if s.sidebar_focus == "auto" && !s.sidebar_auto_collapse_opt_in {
                    // v0.8.62 wrote the surprising auto-collapse default into many
                    // full settings files. Treat unmarked saved "auto" as that
                    // legacy default so upgraded users get the sidebar back, while
                    // A persisted opt-in marker preserves an explicit `auto`
                    // choice from this release onward (#3328).
                    s.sidebar_focus = "pinned".to_string();
                }
                s.status_indicator = normalize_status_indicator(&s.status_indicator).to_string();
                s.ocean_treatment = normalize_ocean_treatment(&s.ocean_treatment).to_string();
                s.work_surface_placement =
                    normalize_work_surface_placement(&s.work_surface_placement).to_string();
                s.synchronized_output =
                    normalize_synchronized_output(&s.synchronized_output).to_string();
                s.background_color =
                    normalize_optional_background_color(s.background_color.as_deref());
                s.theme = normalize_settings_theme(&s.theme).to_string();
                s.default_model = s.default_model.as_deref().and_then(normalize_default_model);
                s.reasoning_effort = s
                    .reasoning_effort
                    .as_deref()
                    .and_then(|value| normalize_reasoning_effort_setting(value).ok().flatten());
                s
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to parse {} (using defaults): {e:#}",
                    source.read_path().display()
                );
                Self::default()
            }
        };
        Ok(settings)
    }

    /// Apply environment-driven overlays after disk load. Used for
    /// platform a11y signals that should ignore the user's saved
    /// preference (#450). The env values are consulted at startup;
    /// changing them mid-session has no effect because settings are
    /// only re-read on `Settings::load()`.
    pub fn apply_env_overrides(&mut self) {
        if env_truthy("NO_ANIMATIONS") {
            self.low_motion = true;
            self.fancy_animations = false;
        }
        // Termius (TERM_PROGRAM=Termius) and SSH sessions exhibit the
        // same 120-FPS flicker class as VS Code — the SSH round-trip
        // races ahead of what the remote renderer can flush, so rapid
        // cursor-positioning sequences cycle through input boxes.
        // Drop both to the 30 FPS low-motion cap. Harvested from
        // PR #1479 by @CrepuscularIRIS / autoghclaw (closes #1433).
        //
        // SSH_CLIENT is exported by sshd for every TCP SSH session;
        // SSH_TTY is exported only for interactive PTY logins, so we
        // check both so non-PTY-allocating tools (rsync wrappers, etc.)
        // still pick this up if they end up running the TUI.
        let term_is_termius = std::env::var("TERM_PROGRAM").as_deref() == Ok("Termius");
        let in_ssh_session = std::env::var_os("SSH_CLIENT").is_some_and(|v| !v.is_empty())
            || std::env::var_os("SSH_TTY").is_some_and(|v| !v.is_empty());
        if term_is_termius || in_ssh_session {
            self.low_motion = true;
            self.fancy_animations = false;
        }

        // Plain Windows PowerShell / cmd.exe under legacy ConHost exposes none
        // of the modern terminal markers below. Keep rendering calmer there:
        // lower the motion rate, disable animated chrome, and avoid DEC 2026
        // synchronized-output wrapping unless the user explicitly forced it on.
        if detected_legacy_windows_console_host() {
            self.low_motion = true;
            self.fancy_animations = false;
            if self.synchronized_output.eq_ignore_ascii_case("auto") {
                self.synchronized_output = "off".to_string();
            }
        }

        // Ptyxis 50.x (the new default terminal on Ubuntu 26.04) ships with
        // VTE 0.84.x which mishandles DEC mode 2026 synchronized output: the
        // begin/end pair is parsed but each wrapped frame still triggers a
        // full-viewport flash on the GPU compositor side, so any TUI that
        // uses DEC 2026 to avoid tearing instead gets visible flicker on
        // every redraw. gnome-terminal 3.58 on the same VTE renders cleanly,
        // so we can't broaden the opt-out to all VTE-based terminals —
        // only the Ptyxis-specific signals trigger it. Confirmed
        // user-visible regression starting with Ubuntu 26.04's default
        // terminal swap; cargo-installed binaries are not exempt because
        // the bug is in the terminal, not the binary.
        //
        // Only flip `auto` to `off`; respect an explicit `"on"` so users
        // who upgrade Ptyxis or want to confirm the fix landed upstream
        // can override the heuristic in persisted settings.toml.
        if self.synchronized_output.eq_ignore_ascii_case("auto") && detected_ptyxis_terminal() {
            self.synchronized_output = "off".to_string();
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let serialized = toml::to_string_pretty(self).context("Failed to serialize settings")?;
        let body = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read settings at {}", path.display()))?;
            codewhale_config::merge_and_preserve_comments(&serialized, &raw).unwrap_or_else(|e| {
                tracing::warn!("failed to merge settings comments, saving without them: {e:#}");
                serialized
            })
        } else {
            serialized
        };
        std::fs::write(&path, body)
            .with_context(|| format!("Failed to write settings to {}", path.display()))?;
        Ok(())
    }

    /// Persist the model for a specific provider.
    pub fn set_model_for_provider(&mut self, provider: &str, model: &str) {
        self.provider_models
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(provider.to_string(), model.to_string());
    }

    fn set_default_model(&mut self, value: &str) -> Result<()> {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "none" | "default" | "(default)"
            )
        {
            self.default_model = None;
            return Ok(());
        }

        let Some(model) = normalize_default_model(trimmed) else {
            anyhow::bail!(
                "Failed to update setting: invalid model '{value}'. Expected: auto, a DeepSeek model ID (for example deepseek-v4-pro, deepseek-v4-flash), or none/default."
            );
        };
        self.default_model = Some(model);
        Ok(())
    }

    /// Persist a provider's model selection.
    ///
    /// `persist_as_default` controls the blast radius (#3227):
    ///
    /// - `false` (session-local, the default for `/model` and the model
    ///   picker): record the model only under that provider's scoped entry in
    ///   [`Self::provider_models`]. The shared `default_provider` and global
    ///   `default_model` are left untouched, so a model change in one terminal
    ///   no longer rewrites the global default that a second terminal reads on
    ///   startup. This is what stopped a GLM/Z.ai session from being dragged
    ///   onto a DeepSeek model (and vice-versa).
    /// - `true` (explicit "save as default"): also pin `default_provider`, and
    ///   for DeepSeek providers the global `default_model`, to this tuple.
    pub fn set_provider_model_selection(
        &mut self,
        provider: ApiProvider,
        model: &str,
        persist_as_default: bool,
    ) -> Result<()> {
        let model = model.trim();
        if model.is_empty() {
            anyhow::bail!("model cannot be empty");
        }
        self.set_model_for_provider(provider.as_str(), model);
        if persist_as_default {
            self.default_provider = Some(provider.as_str().to_string());
            if matches!(provider, ApiProvider::Deepseek | ApiProvider::DeepseekCN) {
                self.set_default_model(model)?;
            }
        }
        Ok(())
    }

    /// Load, update, and save a provider's model selection *without* touching
    /// the shared global default (the session-local path; see
    /// [`Self::set_provider_model_selection`]).
    pub fn persist_provider_model_selection(provider: ApiProvider, model: &str) -> Result<()> {
        let mut settings = Self::load()?;
        settings.set_provider_model_selection(provider, model, false)?;
        settings.save()
    }

    /// Load, update, and save a provider/model tuple as the global default
    /// (the explicit "save as default" path).
    #[allow(dead_code)] // wired to an explicit save-as-default action in a later UX pass (#3227).
    pub fn persist_provider_model_selection_as_default(
        provider: ApiProvider,
        model: &str,
    ) -> Result<()> {
        let mut settings = Self::load()?;
        settings.set_provider_model_selection(provider, model, true)?;
        settings.save()
    }

    /// Resolved boolean for whether the renderer should wrap each frame in
    /// DEC mode 2026 synchronized output. `auto` and `on` enable; `off`
    /// disables. The `auto` → `off` flip for known-bad terminals happens
    /// earlier in [`Self::apply_env_overrides`]; this method only inspects
    /// the final state.
    #[must_use]
    pub fn synchronized_output_enabled(&self) -> bool {
        !self.synchronized_output.eq_ignore_ascii_case("off")
    }

    /// Runtime bracketed-paste mode after terminal-host quirks are applied.
    ///
    /// This deliberately does not mutate [`Settings::bracketed_paste`]:
    /// `apply_env_overrides()` can run before saving settings, and a legacy
    /// conhost runtime fallback must not permanently disable bracketed paste
    /// when the same config is later used in Windows Terminal or another
    /// modern terminal.
    #[must_use]
    pub fn effective_bracketed_paste(&self) -> bool {
        self.bracketed_paste && !detected_legacy_windows_console_host()
    }
}

fn normalize_default_model(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        Some("auto".to_string())
    } else {
        normalize_model_name(trimmed)
    }
}

fn normalize_reasoning_effort_setting(value: &str) -> Result<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "default" | "(default)" | "config" | "configured" | "unset"
        )
    {
        return Ok(None);
    }

    let normalized = match trimmed.to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" | "false" => "off",
        "low" | "minimal" => "low",
        "medium" | "mid" => "medium",
        "high" => "high",
        "auto" | "automatic" => "auto",
        "max" | "maximum" | "xhigh" | "ultracode" => "max",
        _ => {
            anyhow::bail!(
                "Failed to update setting: invalid reasoning_effort '{value}'. Expected: auto, off, low, medium, high, max, xhigh, ultracode, or default."
            );
        }
    };
    Ok(Some(normalized.to_string()))
}

fn normalize_composer_density(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_transcript_spacing(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_tool_collapse_mode(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "collapsed" | "collapse" | "default" | "on" | "true" => "compact",
        "expanded" | "expand" | "off" | "none" | "false" => "expanded",
        "calm" | "calm_mode" | "calm-mode" | "calm_only" | "calm-only" => "calm",
        _ => value,
    }
}

/// Normalize the `status_indicator` header chip setting. Accepts the
/// canonical names plus common aliases ("none"/"hidden" → "off",
/// "dot" → "dots"). Unknown values fall through unchanged so the parser
/// in `update_setting` can surface a clear error.
fn normalize_status_indicator(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "cw" | "mark" | "text" => "cw",
        "whale" | "🐳" | "🐋" => "whale",
        "dots" | "dot" => "dots",
        "off" | "none" | "hidden" | "false" => "off",
        _ => value,
    }
}

/// Normalize the `synchronized_output` setting. Accepts the canonical
/// `"auto"` / `"on"` / `"off"` plus the usual truthy/falsey spellings.
/// Unknown values fall through unchanged so the parser in `set` can
/// surface a clear error.
fn normalize_synchronized_output(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" | "default" => "auto",
        "on" | "true" | "yes" | "1" | "enabled" => "on",
        "off" | "false" | "no" | "0" | "disabled" => "off",
        _ => value,
    }
}

fn normalize_settings_theme(value: &str) -> &'static str {
    normalize_theme_name(value).unwrap_or("system")
}

/// Returns `true` when the active terminal is Ptyxis (the new default
/// terminal on Ubuntu 26.04). Used by [`Settings::apply_env_overrides`]
/// to flip `synchronized_output` from `auto` to `off` so DEC mode 2026
/// flicker on Ptyxis 50.x + VTE 0.84.x stops at the source.
///
/// We deliberately keep this narrow:
///
/// - `TERM_PROGRAM` matches `ptyxis` case-insensitively (the value
///   Ptyxis sets when it forwards a process-launch context).
/// - `PTYXIS_VERSION` is set to any non-empty value (the binary's
///   own version probe, present whether or not `TERM_PROGRAM` made it
///   into the child environment).
///
/// Either signal is sufficient. We do *not* trigger on `VTE_VERSION`
/// alone because gnome-terminal 3.58 ships with the same VTE 0.84.x
/// and renders cleanly — broadening the heuristic would regress every
/// gnome-terminal user.
pub fn detected_ptyxis_terminal() -> bool {
    if let Ok(program) = std::env::var("TERM_PROGRAM")
        && program.trim().to_ascii_lowercase().contains("ptyxis")
    {
        return true;
    }
    matches!(std::env::var("PTYXIS_VERSION"), Ok(v) if !v.trim().is_empty())
}

/// Returns `true` for the unmarked Windows console-host path used by plain
/// PowerShell / cmd.exe. Modern Windows terminals set at least one marker that
/// lets us keep the richer rendering path.
pub fn detected_legacy_windows_console_host() -> bool {
    cfg!(windows)
        && legacy_windows_console_host_env([
            std::env::var_os("WT_SESSION").as_deref(),
            std::env::var_os("ConEmuPID").as_deref(),
            std::env::var_os("TERM_PROGRAM").as_deref(),
            std::env::var_os("WEZTERM_EXECUTABLE").as_deref(),
            std::env::var_os("WEZTERM_PANE").as_deref(),
            std::env::var_os("ALACRITTY_WINDOW_ID").as_deref(),
            std::env::var_os("ANSICON").as_deref(),
            std::env::var_os("TERM").as_deref(),
        ])
}

fn legacy_windows_console_host_env(markers: [Option<&std::ffi::OsStr>; 8]) -> bool {
    fn has_value(value: Option<&std::ffi::OsStr>) -> bool {
        value.is_some_and(|v| !v.is_empty())
    }

    markers.into_iter().all(|value| !has_value(value))
}

fn normalize_optional_background_color(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| normalize_background_color_setting(raw).ok().flatten())
}

fn normalize_background_color_setting(value: &str) -> Result<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "default" | "none" | "reset" | "off"
        )
    {
        return Ok(None);
    }

    normalize_hex_rgb_color(trimmed).map(Some).ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to update setting: invalid background_color '{value}'. Expected #RRGGBB, RRGGBB, or default."
        )
    })
}

fn normalize_sidebar_focus(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "pinned" | "visible" | "show" | "on" => "pinned",
        "tasks" | "activity" | "live" | "running" => "tasks",
        "agents" | "subagents" | "sub-agents" => "agents",
        "context" | "session" => "context",
        "hidden" | "hide" | "closed" | "off" | "none" => "hidden",
        _ => "auto",
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Resolve an environment variable as a boolean. Recognises the
/// common truthy spellings (`1`, `true`, `yes`, `on`) case-
/// insensitively. Used by [`Settings::apply_env_overrides`] for
/// platform a11y signals like `NO_ANIMATIONS`.
fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicit animated baseline for env-force tests (#4095 flipped defaults to calm).
    fn animated_settings() -> Settings {
        Settings {
            calm_mode: false,
            low_motion: false,
            fancy_animations: true,
            show_tool_details: true,
            transcript_spacing: "comfortable".to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn default_settings_use_comfortable_transcript_spacing() {
        let settings = Settings::default();
        assert!(settings.calm_mode);
        assert!(!settings.show_tool_details);
        assert!(!settings.low_motion);
        assert!(settings.fancy_animations);
        assert_eq!(settings.transcript_spacing, "comfortable");
        assert_eq!(settings.tool_collapse_mode, "compact");
        // Thinking is opt-in so the transcript stays focused on the chat.
        assert!(!settings.show_thinking);
    }

    #[test]
    fn default_settings_show_footer_water_strip() {
        let settings = Settings::default();
        assert!(
            settings.fancy_animations,
            "underwater presentation is the default"
        );
        assert!(!settings.low_motion);
        assert_eq!(settings.transcript_spacing, "comfortable");
    }

    #[test]
    fn default_settings_keep_the_water_field_open_until_inspection_is_needed() {
        let settings = Settings::default();
        assert_eq!(settings.sidebar_focus, "auto");
        assert!(settings.sidebar_auto_collapse_opt_in);
    }

    /// Tests that mutate process-global `NO_ANIMATIONS` serialise
    /// through this guard so the cargo parallel runner doesn't
    /// observe interleaved overrides. Uses the process-wide test env
    /// lock so this serializes with the TERM_PROGRAM tests too —
    /// otherwise a `NO_ANIMATIONS=1` leak from this test family can
    /// flip a concurrent `TERM_PROGRAM=iTerm` test's `low_motion`
    /// assertion through the shared `apply_env_overrides` path.
    fn no_animations_test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::lock_test_env()
    }

    #[test]
    fn no_animations_env_forces_low_motion_on() {
        let _g = no_animations_test_guard();
        // SAFETY: tests in this group serialise through the guard.
        unsafe {
            std::env::set_var("NO_ANIMATIONS", "1");
        }
        let mut settings = animated_settings();
        assert!(!settings.low_motion, "default is animated");
        assert!(settings.fancy_animations, "default shows the water strip");
        settings.apply_env_overrides();
        assert!(settings.low_motion, "NO_ANIMATIONS=1 forces low_motion");
        assert!(
            !settings.fancy_animations,
            "NO_ANIMATIONS=1 keeps fancy off"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
        }
    }

    #[test]
    fn no_animations_env_overrides_user_opt_in() {
        let _g = no_animations_test_guard();
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("NO_ANIMATIONS", "true");
        }
        // User had explicitly opted into fancy animations on disk.
        let mut settings = Settings {
            fancy_animations: true,
            ..Settings::default()
        };
        settings.apply_env_overrides();
        assert!(
            !settings.fancy_animations,
            "platform NO_ANIMATIONS overrides user-opt-in fancy_animations"
        );
        assert!(settings.low_motion);
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
        }
    }

    #[test]
    fn no_animations_env_recognises_truthy_spellings_only() {
        let _g = no_animations_test_guard();
        let prev_wt_session = std::env::var_os("WT_SESSION");
        let prev_term_program = std::env::var_os("TERM_PROGRAM");
        let prev_ssh_client = std::env::var_os("SSH_CLIENT");
        let prev_ssh_tty = std::env::var_os("SSH_TTY");

        // The test is about NO_ANIMATIONS only. On Windows CI, an unmarked
        // console host now independently enables low_motion, so mark the host
        // as non-legacy while checking falsy spellings.
        // Termius and SSH also force low_motion, so clear those signals.
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::remove_var("TERM_PROGRAM");
            std::env::remove_var("SSH_CLIENT");
            std::env::remove_var("SSH_TTY");
        }
        #[cfg(windows)]
        unsafe {
            std::env::set_var("WT_SESSION", "test");
        }
        for truthy in ["1", "true", "True", "YES", "on"] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("NO_ANIMATIONS", truthy);
            }
            let mut s = animated_settings();
            s.apply_env_overrides();
            assert!(s.low_motion, "{truthy:?} should be truthy");
        }
        for falsy in ["0", "false", "no", "off", ""] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("NO_ANIMATIONS", falsy);
            }
            let mut s = animated_settings();
            s.apply_env_overrides();
            assert!(!s.low_motion, "{falsy:?} should be falsy");
        }
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
            match prev_wt_session {
                Some(v) => std::env::set_var("WT_SESSION", v),
                None => std::env::remove_var("WT_SESSION"),
            }
            match prev_term_program {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
            match prev_ssh_client {
                Some(v) => std::env::set_var("SSH_CLIENT", v),
                None => std::env::remove_var("SSH_CLIENT"),
            }
            match prev_ssh_tty {
                Some(v) => std::env::set_var("SSH_TTY", v),
                None => std::env::remove_var("SSH_TTY"),
            }
        }
    }

    /// Serialise tests that mutate `TERM_PROGRAM` through this guard.
    /// Uses the process-wide test env lock so this serializes not just
    /// with itself but with every other env-mutating test in the suite
    /// — otherwise a concurrent test that calls `animated_settings()`
    /// can read whatever value our two `set_var`s have raced into the
    /// env at that instant.
    fn term_program_test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::lock_test_env()
    }

    #[test]
    fn ordinary_term_program_does_not_force_low_motion() {
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        let prev_ssh_client = std::env::var_os("SSH_CLIENT");
        let prev_ssh_tty = std::env::var_os("SSH_TTY");
        // SAFETY: serialised by the guard. Clear SSH_* so a real
        // SSH session running the test suite doesn't make this
        // assertion trivially fail — the SSH path is exercised
        // separately by `ssh_session_forces_low_motion_on`.
        unsafe {
            std::env::remove_var("SSH_CLIENT");
            std::env::remove_var("SSH_TTY");
        }
        for program in ["iTerm.app", "Apple_Terminal", "WezTerm", "xterm-256color"] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("TERM_PROGRAM", program);
            }
            let mut s = animated_settings();
            s.apply_env_overrides();
            assert!(
                !s.low_motion,
                "TERM_PROGRAM={program:?} should not force low_motion"
            );
        }
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
            if let Some(v) = prev_ssh_client {
                std::env::set_var("SSH_CLIENT", v);
            }
            if let Some(v) = prev_ssh_tty {
                std::env::set_var("SSH_TTY", v);
            }
        }
    }

    #[test]
    fn termius_term_program_forces_low_motion_on() {
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "Termius");
        }
        let mut settings = animated_settings();
        assert!(!settings.low_motion, "default is animated");
        settings.apply_env_overrides();
        assert!(
            settings.low_motion,
            "TERM_PROGRAM=Termius must enable low_motion to prevent flickering (#1433)"
        );
        assert!(
            !settings.fancy_animations,
            "TERM_PROGRAM=Termius must disable fancy_animations"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
        }
    }

    #[test]
    fn legacy_windows_console_host_detects_unmarked_shell() {
        assert!(legacy_windows_console_host_env([
            None, None, None, None, None, None, None, None
        ]));
    }

    #[test]
    fn legacy_windows_console_host_excludes_modern_terminal_markers() {
        use std::ffi::OsStr;

        let marker = Some(OsStr::new("1"));
        assert!(!legacy_windows_console_host_env([
            marker, None, None, None, None, None, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, marker, None, None, None, None, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, marker, None, None, None, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, None, marker, None, None, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, None, None, marker, None, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, None, None, None, marker, None, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, None, None, None, None, marker, None
        ]));
        assert!(!legacy_windows_console_host_env([
            None, None, None, None, None, None, None, marker
        ]));
    }

    #[cfg(windows)]
    #[test]
    fn unmarked_windows_console_forces_calm_rendering() {
        let _g = term_program_test_guard();
        let vars = [
            "WT_SESSION",
            "ConEmuPID",
            "TERM_PROGRAM",
            "WEZTERM_EXECUTABLE",
            "WEZTERM_PANE",
            "ALACRITTY_WINDOW_ID",
            "ANSICON",
            "TERM",
            "SSH_CLIENT",
            "SSH_TTY",
            "NO_ANIMATIONS",
            "PTYXIS_VERSION",
        ];
        let prev: Vec<_> = vars
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();

        // SAFETY: serialised by the guard.
        unsafe {
            for name in vars {
                std::env::remove_var(name);
            }
        }

        let mut settings = animated_settings();
        assert!(!settings.low_motion, "default is animated");
        assert!(settings.fancy_animations, "default shows the water strip");
        assert_eq!(settings.synchronized_output, "auto");
        settings.apply_env_overrides();
        assert!(settings.low_motion);
        assert!(!settings.fancy_animations);
        assert!(
            settings.bracketed_paste,
            "env-only conhost fallback must not persistently mutate bracketed_paste (#1102)"
        );
        assert!(
            !settings.effective_bracketed_paste(),
            "legacy Windows console hosts do not support crossterm bracketed paste (#1102)"
        );
        assert_eq!(settings.synchronized_output, "off");

        // SAFETY: cleanup under the guard.
        unsafe {
            for (name, value) in prev {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn ssh_session_forces_low_motion_on() {
        let _g = term_program_test_guard();
        let prev_client = std::env::var_os("SSH_CLIENT");
        let prev_tty = std::env::var_os("SSH_TTY");
        let prev_term_program = std::env::var_os("TERM_PROGRAM");
        for (var, val) in [
            ("SSH_CLIENT", "192.168.1.100 50000 22"),
            ("SSH_TTY", "/dev/pts/0"),
        ] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::remove_var("SSH_CLIENT");
                std::env::remove_var("SSH_TTY");
                // Clear TERM_PROGRAM so the test isolates the SSH signal
                // — otherwise a leaked `TERM_PROGRAM=vscode` from a
                // concurrent test would already have forced low_motion
                // and the SSH-only assertion below would be a tautology.
                std::env::remove_var("TERM_PROGRAM");
                std::env::set_var(var, val);
            }
            let mut s = Settings::default();
            s.apply_env_overrides();
            assert!(
                s.low_motion,
                "{var}={val:?} must enable low_motion to prevent flickering in SSH sessions (#1433)"
            );
            assert!(
                !s.fancy_animations,
                "{var}={val:?} must disable fancy_animations in SSH sessions (#1433)"
            );
        }
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("SSH_CLIENT");
            std::env::remove_var("SSH_TTY");
            if let Some(v) = prev_client {
                std::env::set_var("SSH_CLIENT", v);
            }
            if let Some(v) = prev_tty {
                std::env::set_var("SSH_TTY", v);
            }
            match prev_term_program {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // synchronized_output / Ptyxis flicker detection
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn synchronized_output_defaults_to_auto_and_resolves_to_enabled() {
        let s = Settings::default();
        assert_eq!(s.synchronized_output, "auto");
        assert!(
            s.synchronized_output_enabled(),
            "auto must keep DEC 2026 on so terminals that support it stay tear-free"
        );
    }

    #[test]
    fn synchronized_output_off_disables_dec_2026() {
        let s = Settings {
            synchronized_output: "off".to_string(),
            ..Settings::default()
        };
        assert!(!s.synchronized_output_enabled());
    }

    #[test]
    fn synchronized_output_on_keeps_dec_2026_enabled() {
        let s = Settings {
            synchronized_output: "on".to_string(),
            ..Settings::default()
        };
        assert!(s.synchronized_output_enabled());
    }

    #[test]
    fn ptyxis_term_program_flips_synchronized_output_off() {
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        let prev_ptyxis = std::env::var_os("PTYXIS_VERSION");
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "Ptyxis");
            std::env::remove_var("PTYXIS_VERSION");
        }
        let mut s = Settings::default();
        assert_eq!(s.synchronized_output, "auto");
        s.apply_env_overrides();
        assert_eq!(
            s.synchronized_output, "off",
            "Ptyxis 50.x mishandles DEC 2026 — auto must flip to off so VTE 0.84 stops flickering"
        );
        assert!(
            !s.synchronized_output_enabled(),
            "resolved boolean must agree with stored string"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
            match prev_ptyxis {
                Some(v) => std::env::set_var("PTYXIS_VERSION", v),
                None => std::env::remove_var("PTYXIS_VERSION"),
            }
        }
    }

    #[test]
    fn ptyxis_version_env_alone_flips_synchronized_output_off() {
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        let prev_ptyxis = std::env::var_os("PTYXIS_VERSION");
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::remove_var("TERM_PROGRAM");
            std::env::set_var("PTYXIS_VERSION", "50.1");
        }
        let mut s = Settings::default();
        s.apply_env_overrides();
        assert_eq!(
            s.synchronized_output, "off",
            "PTYXIS_VERSION alone is sufficient — Ptyxis sets this even when TERM_PROGRAM isn't propagated"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
            match prev_ptyxis {
                Some(v) => std::env::set_var("PTYXIS_VERSION", v),
                None => std::env::remove_var("PTYXIS_VERSION"),
            }
        }
    }

    #[test]
    fn ptyxis_does_not_override_user_explicit_on() {
        // Users who set `synchronized_output = "on"` (e.g. to confirm a
        // Ptyxis upgrade fixed it) must keep DEC 2026 even on Ptyxis.
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "ptyxis");
        }
        let mut s = Settings {
            synchronized_output: "on".to_string(),
            ..Settings::default()
        };
        s.apply_env_overrides();
        assert_eq!(
            s.synchronized_output, "on",
            "explicit user override must beat the Ptyxis env heuristic"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
        }
    }

    #[test]
    fn ptyxis_does_not_override_user_explicit_off() {
        // A user with `synchronized_output = "off"` on a non-Ptyxis
        // terminal stays off after env detection (no-op flip).
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "xterm-256color");
        }
        let mut s = Settings {
            synchronized_output: "off".to_string(),
            ..Settings::default()
        };
        s.apply_env_overrides();
        assert_eq!(s.synchronized_output, "off");
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
        }
    }

    #[test]
    fn non_ptyxis_term_programs_keep_synchronized_output_auto() {
        let _g = term_program_test_guard();
        let prev = std::env::var_os("TERM_PROGRAM");
        let prev_ptyxis = std::env::var_os("PTYXIS_VERSION");
        // SAFETY: clean slate so non-Ptyxis programs don't see a leaked
        // PTYXIS_VERSION from another test.
        unsafe {
            std::env::remove_var("PTYXIS_VERSION");
        }
        for program in [
            "iTerm.app",
            "Apple_Terminal",
            "WezTerm",
            "xterm-256color",
            "gnome-terminal-server",
            // The Ghostty / VS Code paths force low_motion but must NOT
            // disable DEC 2026 — they handle synchronized output cleanly.
            "ghostty",
            "vscode",
        ] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("TERM_PROGRAM", program);
            }
            let mut s = Settings::default();
            s.apply_env_overrides();
            assert_eq!(
                s.synchronized_output, "auto",
                "TERM_PROGRAM={program:?} must not opt out of DEC 2026"
            );
            assert!(
                s.synchronized_output_enabled(),
                "resolved boolean for {program:?} must stay enabled"
            );
        }
        // SAFETY: cleanup under the guard.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("TERM_PROGRAM", v),
                None => std::env::remove_var("TERM_PROGRAM"),
            }
            match prev_ptyxis {
                Some(v) => std::env::set_var("PTYXIS_VERSION", v),
                None => std::env::remove_var("PTYXIS_VERSION"),
            }
        }
    }

    /// Serialise tests that mutate `DEEPSEEK_CONFIG_PATH` through this guard
    /// so the parallel test runner doesn't observe interleaved env values.
    fn config_path_test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::lock_test_env()
    }

    struct EnvVarRestore {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests using this helper hold config_path_test_guard.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests using this helper hold config_path_test_guard.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            // SAFETY: tests using this helper hold config_path_test_guard.
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn settings_path_defaults_to_codewhale_home_for_new_writes() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _config_override = EnvVarRestore::remove("DEEPSEEK_CONFIG_PATH");
        let _codewhale_home = EnvVarRestore::set("CODEWHALE_HOME", tmp.path().join(".codewhale"));
        let _home = EnvVarRestore::set("HOME", tmp.path());

        let got = Settings::path().expect("settings path");

        assert_eq!(got, tmp.path().join(".codewhale").join("settings.toml"));
    }

    #[test]
    fn settings_path_prefers_codewhale_home_even_when_legacy_exists() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy_dir = tmp.path().join(".deepseek");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        std::fs::write(legacy_dir.join("settings.toml"), "low_motion = true\n")
            .expect("legacy settings");
        let _config_override = EnvVarRestore::remove("DEEPSEEK_CONFIG_PATH");
        let _codewhale_home = EnvVarRestore::set("CODEWHALE_HOME", tmp.path().join(".codewhale"));
        let _home = EnvVarRestore::set("HOME", tmp.path());

        let got = Settings::path().expect("settings path");

        assert_eq!(got, tmp.path().join(".codewhale").join("settings.toml"));
    }

    #[test]
    fn settings_load_migrates_legacy_deepseek_home_into_codewhale_home_without_explicit_home() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join(".codewhale").join("settings.toml");
        let legacy_dir = tmp.path().join(".deepseek");
        let legacy_home = legacy_dir.join("settings.toml");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        std::fs::write(&legacy_home, "low_motion = true\n").expect("legacy settings");
        let _config_override = EnvVarRestore::remove("DEEPSEEK_CONFIG_PATH");
        let _codewhale_home = EnvVarRestore::remove("CODEWHALE_HOME");
        let _home = EnvVarRestore::set("HOME", tmp.path());

        let loaded = Settings::load_persisted().expect("load persisted settings");

        assert!(loaded.low_motion, "legacy settings should still be read");
        assert!(
            primary.exists(),
            "settings load should migrate to primary path"
        );
    }

    #[test]
    fn settings_load_ignores_legacy_files_when_codewhale_home_is_explicit() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let explicit_home = tmp.path().join("isolated-codewhale");
        let legacy_dir = tmp.path().join(".deepseek");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        std::fs::write(
            legacy_dir.join("settings.toml"),
            "theme = \"dracula\"\ncomposer_density = \"spacious\"\nsidebar_width_percent = 42\n",
        )
        .expect("legacy settings");
        let _config_override = EnvVarRestore::remove("DEEPSEEK_CONFIG_PATH");
        let _codewhale_home = EnvVarRestore::set("CODEWHALE_HOME", &explicit_home);
        let _home = EnvVarRestore::set("HOME", tmp.path());

        let loaded = Settings::load().expect("load settings");

        assert_eq!(
            loaded.theme, "system",
            "explicit CODEWHALE_HOME must not inherit ambient legacy settings"
        );
        assert_eq!(
            loaded.composer_density, "comfortable",
            "explicit CODEWHALE_HOME must not inherit ambient legacy settings"
        );
        assert_eq!(
            loaded.sidebar_width_percent, 28,
            "explicit CODEWHALE_HOME must not inherit ambient legacy settings"
        );
        assert!(
            !explicit_home.join("settings.toml").exists(),
            "ambient legacy settings must not be migrated into explicit CODEWHALE_HOME"
        );
    }

    #[test]
    fn settings_load_migrates_legacy_saved_auto_sidebar_focus_to_pinned() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let settings_path = tmp.path().join("settings.toml");
        std::fs::write(&settings_path, "sidebar_focus = \"auto\"\n").expect("settings");
        let _config_override =
            EnvVarRestore::set("DEEPSEEK_CONFIG_PATH", tmp.path().join("config.toml"));

        let loaded = Settings::load().expect("load settings");

        assert_eq!(loaded.sidebar_focus, "pinned");
        assert!(!loaded.sidebar_auto_collapse_opt_in);
    }

    #[test]
    fn settings_load_preserves_explicit_auto_sidebar_opt_in() {
        let _g = config_path_test_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let settings_path = tmp.path().join("settings.toml");
        std::fs::write(
            &settings_path,
            "sidebar_focus = \"auto\"\nsidebar_auto_collapse_opt_in = true\n",
        )
        .expect("settings");
        let _config_override =
            EnvVarRestore::set("DEEPSEEK_CONFIG_PATH", tmp.path().join("config.toml"));

        let loaded = Settings::load().expect("load settings");

        assert_eq!(loaded.sidebar_focus, "auto");
        assert!(loaded.sidebar_auto_collapse_opt_in);
    }

    #[test]
    fn settings_save_preserves_comments() {
        let _g = config_path_test_guard();
        let tmp = std::env::temp_dir().join("dst_settings_comment_test");
        std::fs::create_dir_all(&tmp).unwrap();
        let config_file = tmp.join("config.toml");
        // SAFETY: test-only env mutation guarded by config_path_test_guard.
        unsafe {
            std::env::set_var("DEEPSEEK_CONFIG_PATH", config_file.to_str().unwrap());
        }

        // settings.toml lives next to config.toml
        let settings_path = tmp.join("settings.toml");
        std::fs::write(
            &settings_path,
            "# my setting\ncost_currency = \"usd\"\n# trailing\n",
        )
        .unwrap();

        // Load the existing file so we have a real struct to modify.
        let mut settings = Settings::load().expect("load settings");
        settings.cost_currency = "cny".to_string();
        settings.save().expect("save should succeed");

        let body = std::fs::read_to_string(&settings_path).expect("read settings.toml");
        assert!(body.contains("# my setting"), "comment lost: {body}");
        assert!(body.contains("# trailing"), "trailing lost: {body}");
        assert!(body.contains("cny"), "new value not written: {body}");

        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("DEEPSEEK_CONFIG_PATH");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
