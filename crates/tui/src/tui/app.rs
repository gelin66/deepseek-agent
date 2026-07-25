//! Application state for the `DeepSeek` TUI.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ratatui::layout::Rect;

use crate::config::{Config, has_api_key};
use crate::palette::{self, UiTheme};
use crate::pricing::CostCurrency;
use crate::settings::Settings;
use crate::tui::approval::ApprovalMode;
use crate::tui::child_agents::ChildAgents;
use crate::tui::clipboard::ClipboardHandler;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::scrolling::TranscriptScroll;
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::views::ViewStack;
use dse_localization::{MessageId, tr};

// === Types ===

/// State machine for onboarding new users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingState {
    Welcome,
    ApiKey,
    TrustDirectory,
    Tips,
    None,
}

pub(crate) fn resolve_skills_dir(
    workspace: &Path,
    global_skills_dir: &Path,
    config: &Config,
) -> PathBuf {
    if config.skills_config().scan_codewhale_only() {
        if config.skills_dir.is_some() {
            return global_skills_dir.to_path_buf();
        }
        if let Some(codewhale_skills_dir) =
            crate::skill_context::codewhale_workspace_skills_dir(workspace)
        {
            return codewhale_skills_dir;
        }
        return global_skills_dir.to_path_buf();
    }

    let agents_skills_dir = workspace.join(".agents").join("skills");
    if agents_skills_dir.exists() {
        return agents_skills_dir;
    }

    let local_skills_dir = workspace.join("skills");
    if local_skills_dir.exists() {
        return local_skills_dir;
    }

    if config.skills_dir.is_none()
        && let Some(global_agents) = crate::skill_context::agents_global_skills_dir()
        && global_agents.exists()
    {
        return global_agents;
    }

    global_skills_dir.to_path_buf()
}

fn initial_onboarding_state(
    skip_onboarding: bool,
    was_onboarded: bool,
    needs_api_key: bool,
    needs_workspace_trust: bool,
) -> OnboardingState {
    if skip_onboarding || (was_onboarded && !needs_api_key && !needs_workspace_trust) {
        return OnboardingState::None;
    }

    if was_onboarded && needs_api_key {
        OnboardingState::ApiKey
    } else if was_onboarded && needs_workspace_trust {
        OnboardingState::TrustDirectory
    } else {
        OnboardingState::Welcome
    }
}

fn onboarding_is_workspace_trust_gate(
    skip_onboarding: bool,
    was_onboarded: bool,
    needs_api_key: bool,
    needs_workspace_trust: bool,
) -> bool {
    !skip_onboarding && was_onboarded && !needs_api_key && needs_workspace_trust
}

/// Reasoning-effort tier projected into the canonical DeepSeek request plan.
///
/// The retained UI labels collapse onto the supported DeepSeek planning
/// choices before the request reaches the backend.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    #[default]
    High,
    Max,
}

impl ReasoningEffort {
    /// Parse a config-file string into an effort tier. Unknown values fall
    /// back to the default (`High`) rather than erroring out.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" | "false" => Self::Off,
            "low" | "minimal" => Self::Low,
            "medium" | "mid" => Self::Medium,
            "high" => Self::High,
            "max" | "maximum" | "xhigh" | "ultracode" => Self::Max,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerDensity {
    Compact,
    Comfortable,
    Spacious,
}

impl ComposerDensity {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSpacing {
    Compact,
    Comfortable,
    Spacious,
}

impl TranscriptSpacing {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }
}

/// Controls how dense tool-call runs are collapsed in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCollapseMode {
    /// Collapse qualifying tool runs by default.
    Compact,
    /// Never collapse tool runs automatically.
    Expanded,
    /// Collapse only when calm mode is active.
    Calm,
}

impl ToolCollapseMode {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "expanded" | "off" | "none" => Self::Expanded,
            "calm" | "calm-mode" | "calm_only" | "calm-only" => Self::Calm,
            // `collapsed`/`collapse` are issue #3256's preferred names for the
            // default; treat them like the canonical `compact`.
            _ => Self::Compact,
        }
    }

    #[must_use]
    pub fn is_active(self, calm_mode: bool) -> bool {
        match self {
            Self::Compact => true,
            Self::Expanded => false,
            Self::Calm => calm_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct StatusToast {
    pub text: String,
    pub level: StatusToastLevel,
    pub created_at: Instant,
    pub ttl_ms: Option<u64>,
}

impl StatusToast {
    #[must_use]
    pub fn new(text: impl Into<String>, level: StatusToastLevel, ttl_ms: Option<u64>) -> Self {
        Self {
            text: text.into(),
            level,
            created_at: Instant::now(),
            ttl_ms,
        }
    }

    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        self.ttl_ms
            .is_some_and(|ttl| now.duration_since(self.created_at).as_millis() >= u128::from(ttl))
    }
}

pub(crate) fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| text.len())
}

fn remove_char_at(text: &mut String, char_index: usize) -> bool {
    let start = byte_index_at_char(text, char_index);
    if start >= text.len() {
        return false;
    }
    let ch = text[start..].chars().next().unwrap();
    let end = start + ch.len_utf8();
    text.replace_range(start..end, "");
    true
}

fn normalize_paste_text(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

fn sanitize_api_key_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn strip_raw_mouse_report_runs(input: &str, cursor: usize) -> Option<(String, usize)> {
    // First pass: strip the well-defined control-sequence fragment
    // shapes that crossterm sometimes hands us as `Char(c)` keystrokes
    // when its event reader is interrupted mid-sequence during dense
    // streaming output (#1915). This covers OSC 8 hyperlink fragments
    // (`]8;;URL`, including the closing `]8;;`) and Kitty keyboard
    // protocol fragments (`[?…u`, `[>…u`, `[?u`).
    let (after_fragments, after_fragments_cursor, fragments_changed) =
        strip_control_sequence_fragments(input, cursor);

    // Second pass: the existing run-based filter handles SGR mouse
    // reports (`[<35;44;18M`) and the multi-terminator burst shape
    // (`5;46;18M;48;18M`) introduced in e63a4ba4a. It operates on a
    // narrow char set so it can't be confused with user-typed text.
    let chars: Vec<char> = after_fragments.chars().collect();
    let mut output = String::with_capacity(after_fragments.len());
    let mut new_cursor = 0usize;
    let mut changed = fragments_changed;
    let mut index = 0usize;

    while index < chars.len() {
        if is_raw_mouse_report_run_char(chars[index]) {
            let start = index;
            while index < chars.len() && is_raw_mouse_report_run_char(chars[index]) {
                index += 1;
            }
            let run = &chars[start..index];
            if let Some(keep) = raw_mouse_report_keep_mask(run) {
                changed = true;
                for (offset, ch) in run.iter().copied().enumerate() {
                    if !keep[offset] {
                        continue;
                    }
                    if start + offset < cursor {
                        new_cursor += 1;
                    }
                    output.push(ch);
                }
                continue;
            }
            for (offset, ch) in run.iter().copied().enumerate() {
                if start + offset < after_fragments_cursor {
                    new_cursor += 1;
                }
                output.push(ch);
            }
            continue;
        }

        if index < after_fragments_cursor {
            new_cursor += 1;
        }
        output.push(chars[index]);
        index += 1;
    }

    changed.then(|| {
        let cursor = new_cursor.min(char_count(&output));
        (output, cursor)
    })
}

fn is_raw_mouse_report_run_char(ch: char) -> bool {
    matches!(ch, '\x1b' | '[' | '<' | ';' | ':' | 'M' | 'm') || ch.is_ascii_digit()
}

fn looks_like_raw_mouse_report_run(run: &[char]) -> bool {
    if run.len() < 5 {
        return false;
    }
    let has_separator = run.iter().any(|ch| matches!(ch, ';' | ':'));
    let terminators = run.iter().filter(|ch| matches!(ch, 'M' | 'm')).count();
    if !has_separator || terminators == 0 {
        return false;
    }
    has_sgr_mouse_marker(run) || terminators >= 2
}

fn has_sgr_mouse_marker(run: &[char]) -> bool {
    run.windows(2).any(|window| window == ['[', '<'])
}

fn raw_mouse_report_keep_mask(run: &[char]) -> Option<Vec<bool>> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut index = 0usize;

    while index < run.len() {
        let (start, body_start) = if run[index] == '\x1b'
            && run.get(index + 1) == Some(&'[')
            && run.get(index + 2) == Some(&'<')
        {
            (index, index + 3)
        } else if run[index] == '[' && run.get(index + 1) == Some(&'<') {
            (index, index + 2)
        } else {
            index += 1;
            continue;
        };

        let mut end = body_start;
        let mut has_digit = false;
        let mut has_separator = false;
        let mut matched = false;
        while end < run.len() {
            match run[end] {
                '0'..='9' => {
                    has_digit = true;
                    end += 1;
                }
                ';' | ':' => {
                    has_separator = true;
                    end += 1;
                }
                'M' | 'm' if has_digit && has_separator => {
                    ranges.push((start, end + 1));
                    index = end + 1;
                    matched = true;
                    break;
                }
                _ => break,
            }
        }
        if !matched {
            index = index.saturating_add(1);
        }
    }

    if ranges.is_empty() {
        if looks_like_raw_mouse_report_run(run) {
            return Some(vec![false; run.len()]);
        }
        return None;
    }

    ranges.sort_unstable_by_key(|(start, _)| *start);
    let first_start = ranges[0].0;
    let mut prefix_start = first_start;
    while prefix_start > 0 && is_raw_mouse_report_fragment_char(run[prefix_start - 1]) {
        prefix_start -= 1;
    }
    if prefix_start < first_start
        && looks_like_raw_mouse_report_fragment(&run[prefix_start..first_start])
    {
        ranges.push((prefix_start, first_start));
    }

    let last_end = ranges.iter().map(|(_, end)| *end).max().unwrap_or_default();
    if last_end < run.len() && looks_like_raw_mouse_report_fragment(&run[last_end..]) {
        ranges.push((last_end, run.len()));
    }

    ranges.sort_unstable_by_key(|(start, _)| *start);
    let mut keep = vec![true; run.len()];
    for (start, end) in ranges {
        for slot in keep.iter_mut().take(end.min(run.len())).skip(start) {
            *slot = false;
        }
    }
    Some(keep)
}

fn is_raw_mouse_report_fragment_char(ch: char) -> bool {
    matches!(ch, ';' | ':' | 'M' | 'm') || ch.is_ascii_digit()
}

fn looks_like_raw_mouse_report_fragment(run: &[char]) -> bool {
    if run.len() < 4 {
        return false;
    }
    run.iter().any(|ch| ch.is_ascii_digit())
        && run.iter().any(|ch| matches!(ch, ';' | ':'))
        && run.iter().any(|ch| matches!(ch, 'M' | 'm'))
}

/// Scan `input` for control-sequence fragment shapes (#1915) — OSC 8
/// hyperlinks and Kitty keyboard protocol responses — and excise each
/// match. Returns `(output, new_cursor, changed)`. Cursor positions
/// inside an excised fragment are moved to the fragment's start.
///
/// The match shapes are deliberately narrow so legitimate text like
/// `[is this ok?]` or a typed URL survives untouched:
///
/// - **OSC 8**: `(\x1b?)] 8 ; ...` consuming everything up to the
///   first BEL (`\x07`), `\x1b\\`, lone `\\`, or the next `\x1b]8;`
///   block — terminator characters are optional because crossterm may
///   have already consumed them.
/// - **Kitty CSI**: `(\x1b?) [ (? | > | < | =) ... u` — the
///   private-parameter prefix is what distinguishes a Kitty response
///   from a user-typed `[…u` (which is exceedingly rare and would
///   need an explicit private-parameter byte to be a real CSI).
fn strip_control_sequence_fragments(input: &str, cursor: usize) -> (String, usize, bool) {
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::with_capacity(input.len());
    let mut new_cursor = 0usize;
    let mut changed = false;
    let mut index = 0usize;

    while index < chars.len() {
        if let Some(end) = match_osc8_fragment(&chars, index) {
            // The excised span contributes nothing to `output`, so
            // `new_cursor` simply doesn't tick for any of those
            // characters. A cursor that was inside the span ends up at
            // the fragment's start position in the rewritten input,
            // which matches the existing run-stripper's behavior.
            index = end;
            changed = true;
            continue;
        }

        if let Some(end) = match_kitty_csi_fragment(&chars, index) {
            index = end;
            changed = true;
            continue;
        }

        if index < cursor {
            new_cursor += 1;
        }
        output.push(chars[index]);
        index += 1;
    }

    let cursor = new_cursor.min(char_count(&output));
    (output, cursor, changed)
}

/// If an OSC 8 hyperlink fragment starts at `chars[start]`, return its
/// end index (exclusive). The leading `ESC` is optional because
/// crossterm's event parser often consumes it before reclassifying the
/// tail as keystrokes.
fn match_osc8_fragment(chars: &[char], start: usize) -> Option<usize> {
    let body_start = if chars.get(start) == Some(&'\x1b')
        && chars.get(start + 1) == Some(&']')
        && chars.get(start + 2) == Some(&'8')
        && chars.get(start + 3) == Some(&';')
    {
        start + 4
    } else if chars.get(start) == Some(&']')
        && chars.get(start + 1) == Some(&'8')
        && chars.get(start + 2) == Some(&';')
    {
        start + 3
    } else {
        return None;
    };

    // After `]8;` we expect the OSC 8 payload: an optional second `;`
    // (params separator), then the URL (or empty for the closing
    // wrapper), then a terminator. We deliberately stop at the first
    // ASCII whitespace so a typed `]8;` followed by real prose can't
    // swallow the user's words — real OSC 8 URLs don't contain spaces.
    let mut end = body_start;
    while end < chars.len() {
        let ch = chars[end];
        // BEL terminator.
        if ch == '\x07' {
            return Some(end + 1);
        }
        // `ESC \\` string terminator (ST).
        if ch == '\x1b' && chars.get(end + 1) == Some(&'\\') {
            return Some(end + 2);
        }
        // Lone `\\` — crossterm sometimes delivers ST with the leading
        // ESC already consumed, leaving just `\\` as a Char keystroke.
        if ch == '\\' {
            return Some(end + 1);
        }
        // Start of the next OSC 8 wrapper (closing `]8;;` glued to the
        // body) — close the current fragment here so the next iteration
        // matches that one separately.
        if ch == '\x1b' && chars.get(end + 1) == Some(&']') {
            return Some(end);
        }
        if ch == ']' && chars.get(end + 1) == Some(&'8') && chars.get(end + 2) == Some(&';') {
            return Some(end);
        }
        if ch.is_whitespace() {
            // We never crossed a terminator, so this isn't a real
            // fragment — give up rather than eat user prose.
            return None;
        }
        end += 1;
    }

    // Reached end of input without a terminator or whitespace. Treat as
    // a fragment in flight (its tail will arrive on a later keystroke
    // and get filtered then).
    Some(end)
}

/// If a private-parameter CSI fragment starts at `chars[start]`, return its
/// end index (exclusive). Shape: `(ESC)? [ (? | > | < | =) [0-9;:]* <final>`
/// where `<final>` is any ASCII letter. This covers the Kitty keyboard
/// protocol (`…u`) *and* the DEC private mode set/reset sequences a terminal
/// emits during a session — bracketed paste (`[?2004h`/`[?2004l`), mouse
/// capture (`[?1000h`), focus reporting (`[?1004h`), and synchronized output
/// (`[?2026h`). Those end in `h`/`l`, not `u`, so the old `u`-only terminator
/// let the leading `[` leak into the composer during dense streaming (#2592,
/// regression of #1915). The private-parameter byte (`?`, `>`, `<`, `=`) is
/// what keeps this distinct from text the user might plausibly type.
fn match_kitty_csi_fragment(chars: &[char], start: usize) -> Option<usize> {
    let after_csi = if chars.get(start) == Some(&'\x1b') && chars.get(start + 1) == Some(&'[') {
        start + 2
    } else if chars.get(start) == Some(&'[') {
        start + 1
    } else {
        return None;
    };

    let priv_byte = chars.get(after_csi)?;
    if !matches!(priv_byte, '?' | '>' | '<' | '=') {
        return None;
    }

    let mut end = after_csi + 1;
    let mut saw_param = false;
    while end < chars.len() {
        let ch = chars[end];
        if ch.is_ascii_digit() || ch == ';' || ch == ':' {
            saw_param = true;
            end += 1;
            continue;
        }
        // Final byte. The Kitty keyboard protocol ends in `u` and is valid
        // with no parameters (`[?u`). DEC private mode set/reset ends in
        // `h`/`l` and always carries a numeric mode — bracketed paste
        // (`[?2004h`/`l`), mouse capture (`[?1000h`), focus reporting
        // (`[?1004h`), synchronized output (`[?2026h`). Require a parameter
        // before `h`/`l` so ordinary text like `[?help]` is left untouched.
        return match ch {
            'u' => Some(end + 1),
            'h' | 'l' if saw_param => Some(end + 1),
            _ => None,
        };
    }
    None
}

const MAX_SUBMITTED_INPUT_CHARS: usize = 16_000;
/// Maximum characters displayed in the composer for oversized input.
/// Beyond this, the text is truncated for rendering but the full content
/// is preserved for model submission (#3263).
const MAX_COMPOSER_DISPLAY_CHARS: usize = 4_000;
/// Configuration required to bootstrap the TUI.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TuiOptions {
    pub model: String,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub allow_shell: bool,
    /// Use the alternate screen buffer (fullscreen TUI).
    pub use_alt_screen: bool,
    /// Capture mouse input for internal scrolling/selection.
    pub use_mouse_capture: bool,
    /// Enable terminal bracketed-paste mode (OSC `?2004h` / `?2004l`). Defaults
    /// on; settable via `bracketed_paste = false` in `settings.toml` for the
    /// rare terminal that mishandles it.
    pub use_bracketed_paste: bool,
    /// Maximum number of concurrent sub-agents.
    pub max_subagents: usize,
    #[allow(dead_code)]
    pub skills_dir: PathBuf,
    #[allow(dead_code)]
    pub mcp_config_path: PathBuf,
    /// Skip onboarding screens
    pub skip_onboarding: bool,
    /// Auto-approve tool executions (yolo mode)
    pub yolo: bool,
    /// Resume a previous session by ID
    pub resume_session_id: Option<String>,
    /// Pre-populate the composer with this text when the TUI starts.
    /// Used by `deepseek pr <N>` (#451) to drop the model into a
    /// session with the PR context already typed — the user can edit
    /// before sending or hit Enter to fire as-is.
    pub initial_input: Option<InitialInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialInput {
    /// Pre-populate the composer and wait for the user to press Enter.
    ///
    /// Used by `dse pr <N>` (#451) to drop the model into a session
    /// with the PR context already typed so the user can edit before sending.
    Prefill(String),
    /// Pre-populate the composer, submit it once startup is ready, then keep
    /// the interactive session open for follow-up messages (#2370).
    Submit(String),
}

// === Sub-state structs for App field organization (#377) ===

/// Cached @-mention completion results to avoid re-walking the filesystem when
/// the cursor moves inside the same mention token.
#[derive(Debug, Clone)]
pub struct MentionCompletionCache {
    /// Workspace root used for this completion walk.
    pub workspace: PathBuf,
    /// Process cwd captured for cwd-relative completion entries.
    pub cwd: Option<PathBuf>,
    /// The partial text after `@` that triggered this completion.
    pub partial: String,
    /// Candidate limit used for this completion walk.
    pub limit: usize,
    /// Workspace depth limit used for this completion walk. Included so live
    /// config changes invalidate cached popup results.
    pub walk_depth: usize,
    /// Completion behavior used for this walk. Included so live config changes
    /// invalidate cached popup results.
    pub behavior: String,
    /// Whether symlink following was enabled for this completion walk.
    /// Included so live config changes invalidate cached popup results.
    pub follow_links: bool,
    /// Cached completion entries.
    pub entries: Vec<String>,
}

/// Cached full candidate walk for @-mention completions. One workspace walk
/// serves every subsequent keystroke of the same mention token — the
/// per-keystroke synchronous re-walk was the dominant composer latency on
/// large repos (#3757). Path-like partials (containing `/` or starting with
/// `.`) bypass this cache because local path-reference completions are
/// needle-dependent.
#[derive(Debug, Clone)]
pub struct MentionCandidateCache {
    pub workspace: PathBuf,
    pub cwd: Option<PathBuf>,
    pub walk_depth: usize,
    pub follow_links: bool,
    pub collected_at: std::time::Instant,
    pub candidates: Vec<String>,
}

/// Composer input state — grouped fields for the text input area.
#[derive(Default)]
pub struct ComposerState {
    /// Current composer text content.
    pub input: String,
    /// Cursor position within `input` (in characters).
    pub cursor_position: usize,
    /// When a large paste is consolidated at submit time, the file @mention
    /// is stored here so it can be appended to the submitted text without
    /// replacing the visible composer content (#3263).
    pub(crate) pending_paste_reference: Option<String>,
    /// When composer content is oversized, the full text is stored here
    /// while `self.input` shows a truncated preview. At submit time the
    /// full text is restored for model submission (#3263).
    pub(crate) oversized_paste_full_text: Option<String>,
    pub slash_menu_selected: usize,
    pub slash_menu_hidden: bool,
    pub mention_menu_selected: usize,
    pub mention_menu_hidden: bool,
    /// Cached @-mention completions to avoid re-walking the filesystem when
    /// the cursor moves inside the same mention token.
    pub mention_completion_cache: Option<MentionCompletionCache>,
    /// Cached full candidate list so successive keystrokes inside one mention
    /// token filter in memory instead of re-walking the workspace (#3757).
    pub mention_candidate_cache: Option<MentionCandidateCache>,
}

/// Viewport/scroll state — fields related to transcript scrolling and caching.
pub struct ViewportState {
    pub transcript_scroll: TranscriptScroll,
    pub pending_scroll_delta: i32,
    pub transcript_cache: TranscriptViewCache,
    pub last_transcript_area: Option<Rect>,
    pub last_transcript_top: usize,
    pub last_transcript_visible: usize,
    pub last_transcript_total: usize,
    pub jump_to_latest_button_area: Option<Rect>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            transcript_scroll: TranscriptScroll::to_bottom(),
            pending_scroll_delta: 0,
            transcript_cache: TranscriptViewCache::new(),
            last_transcript_area: None,
            last_transcript_top: 0,
            last_transcript_visible: 0,
            last_transcript_total: 0,
            jump_to_latest_button_area: None,
        }
    }
}

/// Session facts consumed by the TUI.
///
/// The canonical RunStore owns the full token and accounting ledger. The TUI
/// keeps only the latest prompt size needed by the context meter and the
/// aggregate costs it actually renders.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Canonical aggregate cost for all root and child model responses.
    pub total_cost_usd: f64,
    pub total_cost_cny: f64,
    pub last_prompt_tokens: Option<u32>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            total_cost_usd: 0.0,
            total_cost_cny: 0.0,
            last_prompt_tokens: None,
        }
    }
}

/// Global UI state for the TUI.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    /// Composer sub-state (input, cursor, history, menus).
    pub composer: ComposerState,
    /// Viewport sub-state (scroll, cache, selection).
    pub viewport: ViewportState,
    /// Canonical root/child work-surface projection, kept separate from the
    /// transcript so each fact has one presentation owner.
    pub work_surface: crate::tui::work_surface::WorkSurfaceState,
    /// Session sub-state (cost, tokens, telemetry).
    pub session: SessionState,
    pub history: Vec<HistoryCell>,
    /// Per-cell revision counter, kept in lockstep with `history`.
    pub history_revisions: Vec<u64>,
    /// Monotonic counter used to issue fresh per-cell revisions.
    pub next_history_revision: u64,
    pub is_loading: bool,
    /// Legacy status text sink retained for compatibility with existing call sites.
    pub status_message: Option<String>,
    /// Recent status toasts (ephemeral, newest at back).
    pub status_toasts: VecDeque<StatusToast>,
    /// Sticky status toast used for important warnings/errors.
    pub sticky_status: Option<StatusToast>,
    /// Last status text already promoted from `status_message` into toast state.
    pub last_status_message_seen: Option<String>,
    pub model: String,
    /// Current reasoning-effort tier for DeepSeek thinking mode.
    /// Cycled via Ctrl+T; initialized from config at startup.
    pub reasoning_effort: ReasoningEffort,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub use_mouse_capture: bool,
    /// Data-side cap for the `@`-mention popup. The renderer still limits the
    /// visible rows to available terminal height.
    pub mention_menu_limit: usize,
    /// Maximum workspace depth for `@`-mention completion walks. `0` means
    /// unlimited depth.
    pub mention_walk_depth: usize,
    /// `@`-mention completion behavior: fuzzy workspace search or deterministic
    /// directory browser.
    pub mention_menu_behavior: String,
    /// Follow symbolic links during workspace file discovery walks.
    /// When `true`, symlinked directories are traversed, enabling
    /// multi-project workspaces.
    pub workspace_follow_symlinks: bool,
    pub calm_mode: bool,
    pub low_motion: bool,
    pub ocean_started_at: Instant,
    /// Enables the authored underwater phase and ambient motion system.
    pub fancy_animations: bool,
    /// Typed appearance treatment; appearance is independent from motion
    /// settings, and every underwater treatment keeps ambient life.
    pub ocean_treatment: crate::tui::ocean::OceanTreatment,
    /// Whether the renderer should wrap each frame in DEC mode 2026
    /// synchronized output. Resolved from `Settings::synchronized_output`
    /// at construction; `auto`/`on` → `true`, `off` → `false`. The Ptyxis
    /// auto-detect path in `Settings::apply_env_overrides` flips `auto`
    /// to `off` before App is built, so by the time we read this flag in
    /// the draw loop the decision is already made. See the
    /// `Settings::synchronized_output` doc for the user-facing knob.
    pub synchronized_output_enabled: bool,
    /// Header status-indicator chip mode. `"cw"` is the static default;
    /// `"whale"` and `"dots"` preserve the animated legacy choices, while
    /// `"off"` hides the chip. Loaded from settings.
    pub status_indicator: String,
    pub show_thinking: bool,
    pub show_tool_details: bool,
    pub cost_currency: CostCurrency,
    pub composer_density: ComposerDensity,
    pub composer_border: bool,
    pub transcript_spacing: TranscriptSpacing,
    /// Minimum number of consecutive safe tool cells needed for auto-collapse.
    pub tool_collapse_threshold: usize,
    /// Current dense tool-run collapse behavior.
    pub tool_collapse_mode: ToolCollapseMode,
    pub allow_shell: bool,
    pub max_subagents: usize,
    /// Ephemeral projection of canonical root/child runtime events.
    pub child_agents: ChildAgents,
    pub ui_theme: UiTheme,
    /// Active named theme. Drives the cell-level color remap in
    /// `tui::color_compat::ColorCompatBackend` so community presets
    /// (Catppuccin, Tokyo Night, Dracula, Gruvbox) propagate to every
    /// render site, not just the handful that read `app.ui_theme`.
    pub theme_id: palette::ThemeId,
    // Onboarding
    pub onboarding: OnboardingState,
    pub onboarding_needs_api_key: bool,
    pub onboarding_workspace_trust_gate: bool,
    pub api_key_env_only: bool,
    pub api_key_input: String,
    pub api_key_cursor: usize,
    // Clipboard handler
    pub clipboard: ClipboardHandler,
    pub approval_mode: ApprovalMode,
    // Modal view stack (approval/help/etc.)
    pub view_stack: ViewStack,
    /// Trust mode - allow access outside workspace
    pub trust_mode: bool,
    /// Number of MCP servers declared in the user's config at app boot.
    /// Used by passive UI projections; `0` hides the MCP status.
    pub mcp_configured_count: usize,
    /// Cached (name, description) pairs from the skill registry.
    /// Populated once at startup and refreshed on install/uninstall so
    /// the slash menu can show skills without filesystem I/O on every keystroke.
    pub cached_skills: Vec<(String, String)>,
    /// Canonical tool call cells by tool id. Prepared tools are written
    /// directly to `history`; committed outcomes update the indexed cell.
    pub tool_cells: HashMap<String, usize>,
    /// Current streaming assistant cell
    pub streaming_message_index: Option<usize>,
    /// Start time for current turn
    pub turn_started_at: Option<Instant>,
    /// Current runtime turn status (if known).
    pub runtime_turn_status: Option<String>,
    /// Whether the UI needs to be redrawn.
    pub needs_redraw: bool,
    /// Set when the user scrolls up/down during a streaming turn so subsequent
    /// streamed chunks don't yank the view back to the live tail. Cleared
    /// when the user explicitly returns to bottom or the turn completes.
    pub user_scrolled_during_stream: bool,
    /// Startup prompt should be submitted automatically after the engine is ready.
    pub auto_submit_initial_input: bool,
    // === Transcript filtering (#397) ===
    /// Transcript cells the user has collapsed (hidden from view).
    /// Stores **original** virtual cell indices (pre-filtering).
    pub collapsed_cells: HashSet<usize>,
    /// Mapping from filtered cell index → original virtual index.
    /// Populated during `ChatWidget::new` by filtering out collapsed cells.
    /// Used by `build_context_menu_entries` to convert line-meta indices
    /// back to original indices for the `HideCell` / `ShowCell` actions.
    pub collapsed_cell_map: Vec<usize>,
}

// === Deref to ComposerState for backward compat ===

impl std::ops::Deref for App {
    type Target = ComposerState;
    fn deref(&self) -> &Self::Target {
        &self.composer
    }
}

impl std::ops::DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.composer
    }
}

// === App State ===

impl App {
    pub fn tr(&self, id: MessageId) -> Cow<'static, str> {
        tr(id)
    }

    #[allow(clippy::too_many_lines)]
    pub fn new(options: TuiOptions, config: &Config) -> Self {
        let TuiOptions {
            model,
            workspace,
            config_path,
            allow_shell,
            use_alt_screen: _,
            use_mouse_capture,
            use_bracketed_paste: _,
            max_subagents,
            skills_dir: global_skills_dir,
            mcp_config_path,
            skip_onboarding,
            yolo,
            resume_session_id: _,
            initial_input,
        } = options;

        // Start from disk-only preferences, then apply terminal/environment
        // overlays such as NO_ANIMATIONS without persisting them.
        let mut settings = Settings::load_persisted().unwrap_or_else(|_| Settings::default());
        settings.apply_env_overrides();
        // If settings.toml exists on disk but couldn't be parsed (we fell back
        // to defaults), surface a warning in the TUI so the user knows their
        // file is broken instead of silently losing all settings.
        let settings_parse_warning = crate::settings::Settings::path().ok().and_then(|p| {
            if p.exists() {
                std::fs::read_to_string(&p).ok().and_then(|raw| {
                    ::toml::from_str::<::toml::Value>(&raw)
                        .err()
                        .map(|e| format!("⚠ settings.toml is malformed — using defaults ({e})"))
                })
            } else {
                None
            }
        });
        // Authentication follows the already validated entry configuration.
        // Saved UI preferences cannot change the production model backend.
        let needs_api_key = !has_api_key(config);
        let api_key_env_only = crate::config::uses_env_only_api_key(config);
        let was_onboarded = crate::tui::onboarding::is_onboarded();
        let calm_mode = settings.calm_mode;
        let low_motion = settings.low_motion;
        let fancy_animations = settings.fancy_animations;
        let ocean_treatment = crate::tui::ocean::OceanTreatment::parse(&settings.ocean_treatment);
        let work_surface_placement =
            crate::tui::work_surface::WorkSurfacePlacement::parse(&settings.work_surface_placement);
        let synchronized_output_enabled = settings.synchronized_output_enabled();
        let status_indicator = settings.status_indicator.clone();
        let show_thinking = settings.show_thinking;
        let show_tool_details = settings.show_tool_details;
        let cost_currency =
            CostCurrency::from_setting(&settings.cost_currency).unwrap_or(CostCurrency::Usd);
        let composer_density = ComposerDensity::from_setting(&settings.composer_density);
        let composer_border = settings.composer_border;
        let transcript_spacing = TranscriptSpacing::from_setting(&settings.transcript_spacing);
        // Resolve the named theme from settings; unknown values were already
        // normalised to "system" in Settings::load. The background_color
        // setting still overlays on top.
        let theme_id =
            palette::ThemeId::from_name(&settings.theme).unwrap_or(palette::ThemeId::System);
        let mut ui_theme = theme_id.ui_theme();
        if let Some(background) = settings
            .background_color
            .as_deref()
            .and_then(palette::parse_hex_rgb_color)
        {
            ui_theme = ui_theme.with_background_color(background);
        }
        let configured_reasoning_effort = settings
            .reasoning_effort
            .as_deref()
            .or_else(|| config.reasoning_effort());
        let reasoning_effort = configured_reasoning_effort
            .map_or_else(ReasoningEffort::default, ReasoningEffort::from_setting);

        let needs_workspace_trust =
            !yolo && crate::tui::onboarding::needs_trust_at(config_path.as_deref(), &workspace);
        let onboarding = initial_onboarding_state(
            skip_onboarding,
            was_onboarded,
            needs_api_key,
            needs_workspace_trust,
        );
        let onboarding_workspace_trust_gate = onboarding_is_workspace_trust_gate(
            skip_onboarding,
            was_onboarded,
            needs_api_key,
            needs_workspace_trust,
        );

        // Approval has one persistent owner (`Config::approval_policy`) and
        // exactly two canonical behaviors. `--yolo` is an explicit startup
        // override; trust and shell authority remain independent controls.
        let configured_approval_mode = config
            .approval_policy
            .as_deref()
            .and_then(ApprovalMode::from_config_value)
            .unwrap_or_default();
        let allow_shell = allow_shell || yolo;

        let skills_scan_codewhale_only = config.skills_config().scan_codewhale_only();
        let skills_dir = resolve_skills_dir(&workspace, &global_skills_dir, config);
        let cached_skills =
            Self::discover_cached_skills(&workspace, &skills_dir, skills_scan_codewhale_only);

        let (initial_input_text, initial_input_cursor, auto_submit_initial_input) =
            match initial_input {
                // #451: pre-populate the composer when invoked via
                // `deepseek pr <N>` (or any future caller that wants to
                // drop the model into a session with context already
                // typed). Cursor lands at the end so Enter sends as-is.
                Some(InitialInput::Prefill(text)) if !text.is_empty() => {
                    let cursor = text.chars().count();
                    (text, cursor, false)
                }
                Some(InitialInput::Submit(text)) if !text.is_empty() => {
                    let cursor = text.chars().count();
                    (text, cursor, true)
                }
                _ => (String::new(), 0, false),
            };
        let mcp_configured_count =
            crate::mcp::load_config_with_workspace(&mcp_config_path, &workspace)
                .map(|cfg| cfg.servers.len())
                .unwrap_or(0);
        Self {
            composer: ComposerState {
                input: initial_input_text,
                cursor_position: initial_input_cursor,
                pending_paste_reference: None,
                oversized_paste_full_text: None,
                slash_menu_selected: 0,
                slash_menu_hidden: false,
                mention_menu_selected: 0,
                mention_menu_hidden: false,
                mention_completion_cache: None,
                mention_candidate_cache: None,
            },
            viewport: ViewportState::default(),
            work_surface: crate::tui::work_surface::WorkSurfaceState::with_placement(
                work_surface_placement,
            ),
            session: SessionState::default(),
            history: Vec::new(),
            history_revisions: Vec::new(),
            next_history_revision: 1,
            is_loading: false,
            // Surface parse warnings so the user knows their config file is
            // broken instead of silently losing all settings.
            status_message: settings_parse_warning,
            status_toasts: VecDeque::new(),
            sticky_status: None,
            last_status_message_seen: None,
            model,
            reasoning_effort,
            workspace,
            config_path,
            use_mouse_capture,
            calm_mode,
            low_motion,
            ocean_started_at: Instant::now(),
            fancy_animations,
            ocean_treatment,
            synchronized_output_enabled,
            status_indicator,
            show_thinking,
            show_tool_details,
            cost_currency,
            composer_density,
            composer_border,
            transcript_spacing,
            tool_collapse_threshold: 3,
            tool_collapse_mode: ToolCollapseMode::from_setting(&settings.tool_collapse_mode),
            allow_shell,
            max_subagents,
            child_agents: ChildAgents::default(),
            ui_theme,
            theme_id,
            onboarding,
            onboarding_needs_api_key: needs_api_key,
            onboarding_workspace_trust_gate,
            api_key_env_only,
            api_key_input: String::new(),
            api_key_cursor: 0,
            clipboard: ClipboardHandler::new(),
            approval_mode: if yolo {
                ApprovalMode::AutoApprove
            } else {
                configured_approval_mode
            },
            view_stack: ViewStack::new(),
            trust_mode: yolo,
            // Read the MCP config once at boot to know how many servers the
            // user declared. Errors fall through to zero so a missing or
            // malformed config simply hides the passive UI projections.
            mcp_configured_count,
            cached_skills,
            tool_cells: HashMap::new(),
            streaming_message_index: None,
            turn_started_at: None,
            runtime_turn_status: None,
            needs_redraw: true,
            user_scrolled_during_stream: false,
            auto_submit_initial_input,
            collapsed_cells: HashSet::new(),
            collapsed_cell_map: Vec::new(),
            mention_menu_limit: settings.mention_menu_limit,
            mention_walk_depth: settings.mention_walk_depth,
            mention_menu_behavior: settings.mention_menu_behavior.clone(),
            workspace_follow_symlinks: settings.workspace_follow_symlinks,
        }
    }

    fn discover_cached_skills(
        workspace: &std::path::Path,
        skills_dir: &std::path::Path,
        scan_codewhale_only: bool,
    ) -> Vec<(String, String)> {
        crate::skill_context::discover_for_workspace_and_dir_with_mode(
            workspace,
            skills_dir,
            crate::skill_context::SkillDiscoveryMode::from_codewhale_only(scan_codewhale_only),
        )
        .list()
        .iter()
        .map(|s| (s.name.clone(), s.description.clone()))
        .collect()
    }

    pub fn finish_onboarding_without_feature_intro(&mut self) {
        self.onboarding = OnboardingState::None;
        if let Err(err) = crate::tui::onboarding::mark_onboarded() {
            self.status_message =
                Some(tr(MessageId::TuiOnboardingMarkFailed).replace("{error}", &err.to_string()));
        }
        self.needs_redraw = true;
    }

    /// Whether the interface is asking the user to make a decision. Ambient
    /// motion yields across the whole frame while this is true; freezing one
    /// task marker still leaves distracting movement in peripheral vision.
    #[must_use]
    pub fn attention_hold_active(&self) -> bool {
        !self.view_stack.is_empty()
    }

    /// Soft cap on [`Self::history`] length. When history exceeds this count,
    /// the oldest cells are folded into a single placeholder to bound memory
    /// and render cost (#399 S2). The cap is generous — 5000 cells is more
    /// than enough to keep the visible transcript intact across sessions.
    pub const HISTORY_SOFT_CAP: usize = 5_000;

    /// Number of oldest cells to fold when the soft cap fires. Folding in
    /// batches amortizes the cost instead of triggering on every push.
    const HISTORY_FOLD_BATCH: usize = 1_000;

    pub fn add_message(&mut self, msg: HistoryCell) {
        let rev = self.fresh_history_revision();
        self.history.push(msg);
        self.history_revisions.push(rev);

        // Bound history length: when the soft cap fires, fold the oldest
        // batch into a single ArchivedContext placeholder.
        self.maybe_fold_history();
        if self.viewport.transcript_scroll.is_at_tail() && !self.user_scrolled_during_stream {
            self.scroll_to_bottom();
        }
    }

    /// Read the canonical root-and-child accounting total in the chosen
    /// display currency. `run_presenter` projects the aggregate Run ledger
    /// into these two fields; the TUI does not maintain a second child ledger.
    pub fn total_cost_for_currency(&self, currency: CostCurrency) -> f64 {
        match self.cost_display_currency(currency) {
            CostCurrency::Usd => self.session.total_cost_usd,
            CostCurrency::Cny => self.session.total_cost_cny,
        }
    }

    pub fn format_cost_amount_precise(&self, amount: f64) -> String {
        crate::pricing::format_cost_amount_precise(
            amount,
            self.cost_display_currency(self.cost_currency),
        )
    }

    pub(crate) fn cost_display_currency(&self, currency: CostCurrency) -> CostCurrency {
        if currency == CostCurrency::Cny
            && self.session.total_cost_cny == 0.0
            && self.session.total_cost_usd > 0.0
        {
            CostCurrency::Usd
        } else {
            currency
        }
    }

    /// Fold the oldest [`Self::HISTORY_FOLD_BATCH`] cells into a single
    /// `ArchivedContext` placeholder when history exceeds the soft cap.
    /// Called from [`Self::add_message`]; the caller is responsible for
    /// also removing the folded range from any auxiliary per-cell maps.
    fn maybe_fold_history(&mut self) {
        if self.history.len() <= Self::HISTORY_SOFT_CAP {
            return;
        }

        let fold_count = Self::HISTORY_FOLD_BATCH.min(self.history.len());
        // Don't fold into the very last cell(s) — keep a buffer of
        // non-folded cells so the visible transcript tail stays intact.
        let keep_tail = Self::HISTORY_SOFT_CAP.saturating_sub(Self::HISTORY_FOLD_BATCH);
        if self.history.len().saturating_sub(fold_count) < keep_tail {
            return;
        }

        // Gather the range of cell indices we are folding.
        let folded: Vec<HistoryCell> = self.history.drain(..fold_count).collect();
        let folded_revs: Vec<u64> = self.history_revisions.drain(..fold_count).collect();
        let _ = folded_revs; // revisions are discarded with the cells

        // Shift all per-cell index maps down by `fold_count`.
        self.shift_history_maps_down(fold_count);

        // Build a single placeholder cell summarizing the folded range.
        let total_folded = folded.len();
        let summary = format!(
            "{total_folded} older transcript cells folded to bound memory. \
             Use /sessions to load a prior session snapshot if needed."
        );
        let placeholder = HistoryCell::ArchivedContext {
            level: 0,
            range: format!("cells 0-{}", total_folded.saturating_sub(1)),
            tokens: String::new(),
            density: String::new(),
            model: String::new(),
            timestamp: String::new(),
            summary,
        };

        // Insert the placeholder at the front.
        let rev = self.fresh_history_revision();
        self.history.insert(0, placeholder);
        self.history_revisions.insert(0, rev);
        self.needs_redraw = true;
    }

    /// Shift all per-cell index maps down by `n` after removing the first
    /// `n` history cells. Every map key >= n is mapped to key - n; keys < n
    /// are dropped.
    fn shift_history_maps_down(&mut self, n: usize) {
        // tool_cells: HashMap<String, usize>
        self.tool_cells.retain(|_, idx| {
            if *idx >= n {
                *idx -= n;
                true
            } else {
                false
            }
        });

        // collapsed_cells
        self.collapsed_cells = std::mem::take(&mut self.collapsed_cells)
            .into_iter()
            .filter_map(|idx| if idx >= n { Some(idx - n) } else { None })
            .collect();
        self.collapsed_cell_map.clear();
    }

    /// Issue a fresh, monotonically increasing revision counter for a new
    /// history cell. Wrapping is acceptable — collisions are astronomically
    /// rare and at worst trigger one extra re-render.
    fn fresh_history_revision(&mut self) -> u64 {
        let rev = self.next_history_revision;
        self.next_history_revision = self.next_history_revision.wrapping_add(1);
        rev
    }

    /// Bring `history_revisions` back into shape (`history_revisions.len() ==
    /// history.len()`). Pushes fresh revs for newly appended cells, truncates
    /// for cells that were removed. **Does not** invalidate existing entries.
    pub fn resync_history_revisions(&mut self) {
        if self.history_revisions.len() < self.history.len() {
            let needed = self.history.len() - self.history_revisions.len();
            for _ in 0..needed {
                let rev = self.fresh_history_revision();
                self.history_revisions.push(rev);
            }
        } else if self.history_revisions.len() > self.history.len() {
            self.history_revisions.truncate(self.history.len());
        }
    }

    /// Bump the revision counter of a single history cell so the transcript
    /// cache re-renders it on the next frame. Use this whenever a cell's
    /// content (e.g. a streaming Assistant body) is mutated in place.
    pub fn bump_history_cell(&mut self, idx: usize) {
        // Resync first in case callers mutated `history` directly without
        // pushing through `add_message`. After resync, the index is valid
        // (or out of bounds — in which case there's nothing to bump).
        self.resync_history_revisions();
        if let Some(rev) = self.history_revisions.get_mut(idx) {
            let new_rev = self.next_history_revision;
            self.next_history_revision = self.next_history_revision.wrapping_add(1);
            *rev = new_rev;
        }
        self.needs_redraw = true;
    }

    /// Clear the ephemeral display projection and its side indexes.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_revisions.clear();
        self.collapsed_cells.clear();
        self.collapsed_cell_map.clear();
        self.needs_redraw = true;
    }

    /// Pop the trailing history cell, keeping revisions in sync.
    pub fn pop_history(&mut self) -> Option<HistoryCell> {
        let cell = self.history.pop();
        if cell.is_some() {
            self.history_revisions.pop();
            self.needs_redraw = true;
        }
        cell
    }

    #[must_use]
    pub fn tool_collapse_active(&self) -> bool {
        self.tool_collapse_threshold > 0 && self.tool_collapse_mode.is_active(self.calm_mode)
    }

    pub fn push_status_toast(
        &mut self,
        text: impl Into<String>,
        level: StatusToastLevel,
        ttl_ms: Option<u64>,
    ) {
        let toast = StatusToast::new(text, level, ttl_ms);
        self.status_toasts.push_back(toast);
        while self.status_toasts.len() > 24 {
            self.status_toasts.pop_front();
        }
        self.needs_redraw = true;
    }

    pub fn set_sticky_status(
        &mut self,
        text: impl Into<String>,
        level: StatusToastLevel,
        ttl_ms: Option<u64>,
    ) {
        self.sticky_status = Some(StatusToast::new(text, level, ttl_ms));
        self.needs_redraw = true;
    }

    pub fn clear_sticky_status(&mut self) {
        self.sticky_status = None;
    }

    pub fn close_slash_menu(&mut self) {
        self.slash_menu_hidden = true;
        self.needs_redraw = true;
    }

    fn classify_status_text(text: &str) -> (StatusToastLevel, Option<u64>, bool) {
        let lower = text.to_ascii_lowercase();
        let has = |needle: &str| lower.contains(needle);

        if has("offline mode") || has("context critical") {
            return (StatusToastLevel::Warning, None, true);
        }
        if has("error")
            || has("failed")
            || has("denied")
            || has("timeout")
            || has("aborted")
            || has("critical")
        {
            return (StatusToastLevel::Error, Some(15_000), true);
        }
        // A success keyword under a negation ("not saved", "no longer
        // found", "could not enable") is a failure the coarse keyword match
        // would otherwise paint green. Guard it: negated success degrades to
        // a neutral Info toast rather than a misleading Success.
        let negated = has("not ")
            || has("no longer")
            || has("no ")
            || has("could not")
            || has("couldn't")
            || has("cannot")
            || has("can't")
            || has("unable");
        if !negated
            && (has("saved")
                || has("loaded")
                || has("queued")
                || has("found")
                || has("enabled")
                || has("completed"))
        {
            return (StatusToastLevel::Success, Some(5_000), false);
        }
        if has("cancelled") || has("canceled") || has("warning") {
            return (StatusToastLevel::Warning, Some(5_000), false);
        }
        (StatusToastLevel::Info, Some(4_000), false)
    }

    fn is_mode_switch_status_message(message: &str) -> bool {
        message.starts_with("Switched to ") && message.ends_with(" mode")
    }

    pub fn sync_status_message_to_toasts(&mut self) {
        let current = self.status_message.clone();
        if self.last_status_message_seen == current {
            return;
        }
        self.last_status_message_seen = current.clone();

        let Some(message) = current else {
            return;
        };
        if message.trim().is_empty() {
            return;
        }
        if Self::is_mode_switch_status_message(&message) {
            return;
        }

        let (level, ttl_ms, sticky) = Self::classify_status_text(&message);
        if sticky {
            self.set_sticky_status(message, level, ttl_ms);
        } else {
            if matches!(level, StatusToastLevel::Success)
                && self
                    .sticky_status
                    .as_ref()
                    .is_some_and(|toast| matches!(toast.level, StatusToastLevel::Error))
            {
                self.clear_sticky_status();
            }
            self.push_status_toast(message, level, ttl_ms);
        }
    }

    pub fn active_status_toast(&mut self) -> Option<StatusToast> {
        self.sync_status_message_to_toasts();
        let now = Instant::now();
        let mut removed = false;

        while self
            .status_toasts
            .front()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.status_toasts.pop_front();
            removed = true;
        }

        if self
            .sticky_status
            .as_ref()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.sticky_status = None;
            removed = true;
        }

        if removed {
            self.needs_redraw = true;
        }

        self.sticky_status
            .clone()
            .or_else(|| self.status_toasts.back().cloned())
    }

    pub fn transcript_render_options(&self) -> TranscriptRenderOptions {
        TranscriptRenderOptions {
            show_thinking: self.show_thinking,
            show_tool_details: self.show_tool_details,
            calm_mode: self.calm_mode,
            low_motion: self.low_motion,
            spacing: self.transcript_spacing,
        }
    }

    /// When the user starts editing a truncated oversized paste, restore the
    /// full text so they can see and edit the complete content (#3263).
    fn auto_expand_oversized_paste(&mut self) {
        if let Some(full) = self.oversized_paste_full_text.take() {
            self.input = full;
            // Clamp cursor to the new length instead of resetting to 0,
            // so the user's position in the truncated preview is preserved.
            self.cursor_position = self.cursor_position.min(char_count(&self.input));
        }
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.auto_expand_oversized_paste();
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, text);
        self.cursor_position = cursor + char_count(text);
        self.strip_raw_mouse_reports_from_input();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    pub fn insert_paste_text(&mut self, text: &str) {
        let normalized = normalize_paste_text(text);
        if !normalized.is_empty() {
            self.insert_str(&normalized);
        }
        // Large pasted input stays editable and visible until submit. The
        // submit-time safety net consolidates oversized composer content into
        // an @paste-...md mention before dispatch, so no path silently
        // truncates user input.
        // self.consolidate_large_input_if_oversized(); // deferred to submit time
    }

    pub fn insert_api_key_char(&mut self, c: char) {
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert(byte_index, c);
        self.api_key_cursor = cursor + 1;
    }

    pub fn insert_api_key_str(&mut self, text: &str) {
        let sanitized = sanitize_api_key_text(text);
        if sanitized.is_empty() {
            return;
        }
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert_str(byte_index, &sanitized);
        self.api_key_cursor = cursor + char_count(&sanitized);
    }

    pub fn delete_api_key_char(&mut self) {
        if self.api_key_cursor == 0 {
            return;
        }
        let target = self.api_key_cursor.saturating_sub(1);
        if remove_char_at(&mut self.api_key_input, target) {
            self.api_key_cursor = target;
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.viewport.pending_scroll_delta =
            self.viewport.pending_scroll_delta.saturating_sub(delta);
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.viewport.pending_scroll_delta =
            self.viewport.pending_scroll_delta.saturating_add(delta);
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.viewport.transcript_scroll = TranscriptScroll::to_bottom();
        self.viewport.pending_scroll_delta = 0;
        self.viewport.jump_to_latest_button_area = None;
        self.user_scrolled_during_stream = false;
        self.needs_redraw = true;
    }

    pub fn insert_char(&mut self, c: char) {
        self.auto_expand_oversized_paste();
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert(byte_index, c);
        self.cursor_position = cursor + 1;
        self.strip_raw_mouse_reports_from_input();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    fn strip_raw_mouse_reports_from_input(&mut self) {
        if let Some((input, cursor_position)) =
            strip_raw_mouse_report_runs(&self.input, self.cursor_position)
        {
            self.input = input;
            self.cursor_position = cursor_position;
        }
    }

    pub fn delete_char(&mut self) {
        self.auto_expand_oversized_paste();
        if self.cursor_position == 0 {
            return;
        }
        let target = self.cursor_position.saturating_sub(1);
        let removed = remove_char_at(&mut self.input, target);
        if removed {
            self.cursor_position = target;
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    pub fn delete_char_forward(&mut self) {
        self.auto_expand_oversized_paste();
        if self.input.is_empty() {
            return;
        }
        let target = self.cursor_position;
        let removed = remove_char_at(&mut self.input, target);
        if !removed {
            self.cursor_position = char_count(&self.input);
        }
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    /// Delete the word before the cursor.
    pub fn delete_word_backward(&mut self) {
        if self.cursor_position == 0 {
            return;
        }

        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        let mut word_start = cursor_byte;

        while word_start > 0 {
            let Some((prev, ch)) = self.input[..word_start].char_indices().next_back() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            word_start = prev;
        }

        while word_start > 0 {
            let Some((prev, ch)) = self.input[..word_start].char_indices().next_back() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            word_start = prev;
        }

        if word_start < cursor_byte {
            self.input.replace_range(word_start..cursor_byte, "");
            self.cursor_position = char_count(&self.input[..word_start]);
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
        self.needs_redraw = true;
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < char_count(&self.input) {
            self.cursor_position += 1;
            self.needs_redraw = true;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
        self.needs_redraw = true;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = char_count(&self.input);
        self.needs_redraw = true;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
        // Prevent stale oversized-paste state from leaking when the user
        // clears the composer or navigates to a different input (#3263).
        self.pending_paste_reference = None;
        self.oversized_paste_full_text = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.needs_redraw = true;
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            return None;
        }
        // Enforce the safety cap at submit time for every input path. This
        // keeps bracketed pastes fully visible and editable in the composer,
        // then writes oversized content to a workspace paste file before
        // dispatch (#553, #3263).
        self.consolidate_large_input_if_oversized();
        // If consolidation created a paste file, restore the full text and
        // append the @mention so the model can read the complete content
        // while the composer stays editable (#3263).
        let mut input = self
            .oversized_paste_full_text
            .take()
            .unwrap_or_else(|| self.input.clone());
        if let Some(reference) = self.pending_paste_reference.take() {
            if !input.is_empty() && !input.ends_with('\n') {
                input.push('\n');
            }
            input.push_str(&reference);
        }
        self.clear_input();
        Some(input)
    }

    /// Submit the current composer input when Enter is pressed.
    ///
    /// Literal newlines from a terminal paste arrive inside `Event::Paste`
    /// and are inserted by [`Self::insert_paste_text`]. Ordinary Enter key
    /// events keep their unambiguous submit meaning.
    pub fn handle_composer_enter(&mut self) -> Option<String> {
        self.submit_input()
    }

    /// Public wrapper around [`Self::consolidate_large_input`] that no-ops
    /// when the current input fits inside the safety cap. Both the paste-
    /// insert path (visible-before-submit) and the submit-time safety net
    /// route through here, so the cap is enforced exactly once even when
    /// both paths fire on the same buffer.
    fn consolidate_large_input_if_oversized(&mut self) {
        if char_count(&self.input) > MAX_SUBMITTED_INPUT_CHARS {
            self.consolidate_large_input();
        }
    }

    /// When the composer input exceeds [`MAX_SUBMITTED_INPUT_CHARS`], write
    /// the full content to a timestamped paste file under
    /// `.codewhale/pastes/` and replace `self.input` with an `@`-mention
    /// pointing at it so the model can read the full content via the
    /// normal file-mention resolution path (#553).
    fn consolidate_large_input(&mut self) {
        let full_input = std::mem::take(&mut self.input);
        self.cursor_position = 0;

        let now = chrono::Local::now();
        let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let filename = format!("paste-{}-{}.md", now.format("%Y-%m-%d-%H%M%S"), suffix);
        let rel_path = format!(".codewhale/pastes/{filename}");

        let pastes_dir = self.workspace.join(".codewhale/pastes");
        if let Err(e) = std::fs::create_dir_all(&pastes_dir) {
            // Fallback: keep a truncated version so we don't lose the
            // user's input entirely when the filesystem is unhappy.
            self.input = full_input.chars().take(MAX_SUBMITTED_INPUT_CHARS).collect();
            self.cursor_position = char_count(&self.input);
            self.push_status_toast(
                tr(MessageId::TuiPasteDirectoryFailed).replace("{error}", &e.to_string()),
                StatusToastLevel::Error,
                Some(8_000),
            );
            return;
        }

        let file_path = self.workspace.join(&rel_path);
        if let Err(e) = std::fs::write(&file_path, &full_input) {
            self.input = full_input.chars().take(MAX_SUBMITTED_INPUT_CHARS).collect();
            self.cursor_position = char_count(&self.input);
            self.push_status_toast(
                tr(MessageId::TuiPasteWriteFailed).replace("{error}", &e.to_string()),
                StatusToastLevel::Error,
                Some(8_000),
            );
            return;
        }

        // Keep a truncated preview in the composer so the user can still
        // select, copy, and edit it, while the full text is stored for
        // model submission. The @mention is appended at submit time (#3263).
        self.pending_paste_reference = Some(format!("@{rel_path}"));
        self.oversized_paste_full_text = Some(full_input.clone());
        let display_chars = char_count(&full_input).min(MAX_COMPOSER_DISPLAY_CHARS);
        let mut truncated: String = full_input.chars().take(display_chars).collect();
        if char_count(&full_input) > MAX_COMPOSER_DISPLAY_CHARS {
            truncated.push_str("\n\n---\n(content truncated for display — start typing to expand; full text sent to model)");
        }
        self.input = truncated;
        self.cursor_position = 0;
        self.push_status_toast(
            "Large paste backed up to file — the model will receive the full content.",
            StatusToastLevel::Info,
            Some(5_000),
        );
    }

    pub fn effective_model_for_budget(&self) -> &str {
        &self.model
    }

    pub fn model_display_label(&self) -> String {
        self.model.clone()
    }
}

#[cfg(test)]
mod tests;
