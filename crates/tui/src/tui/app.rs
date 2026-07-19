//! Application state for the `DeepSeek` TUI.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use serde_json::Value;
use thiserror::Error;

use codewhale_config::route::RouteLimits;

use crate::config::{
    ApiProvider, Config, DEFAULT_TEXT_MODEL, SavedCredential, has_api_key, save_api_key,
    save_api_key_for,
};
use crate::localization::{MessageId, tr};
use crate::palette::{self, UiTheme};
use crate::pricing::{CostCurrency, CostEstimate};
use crate::settings::Settings;
use crate::tui::approval::ApprovalMode;
use crate::tui::child_agents::ChildAgents;
use crate::tui::clipboard::ClipboardHandler;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::scrolling::TranscriptScroll;
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::views::ViewStack;

// === Types ===

/// Durable permission baseline restored when the UI returns to Agent mode.
#[derive(Debug, Clone, Copy)]
struct ModeSessionPrefs {
    agent_allow_shell: bool,
    agent_trust_mode: bool,
    agent_approval_mode: ApprovalMode,
}

/// Permission fields projected from the visible mode and the Agent baseline.
#[derive(Debug, Clone, Copy)]
struct EffectiveModePolicy {
    allow_shell: bool,
    trust_mode: bool,
    approval_mode: ApprovalMode,
}

#[must_use]
fn base_policy_for_mode(mode: AppMode, prefs: &ModeSessionPrefs) -> EffectiveModePolicy {
    match mode {
        AppMode::Plan => EffectiveModePolicy {
            allow_shell: false,
            trust_mode: false,
            approval_mode: ApprovalMode::Suggest,
        },
        AppMode::Agent | AppMode::Auto | AppMode::Operate => EffectiveModePolicy {
            allow_shell: prefs.agent_allow_shell,
            trust_mode: prefs.agent_trust_mode,
            approval_mode: prefs.agent_approval_mode,
        },
        AppMode::Yolo => EffectiveModePolicy {
            allow_shell: true,
            trust_mode: true,
            approval_mode: ApprovalMode::Bypass,
        },
    }
}

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

/// Supported application modes for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Agent,
    #[allow(dead_code)]
    Auto,
    /// Legacy compatibility alias; resolves to [`Self::Agent`] + bypass approvals.
    Yolo,
    Plan,
    Operate,
}

/// Reasoning-effort tier, mirrored across DeepSeek and Codex effort pickers.
///
/// The config file accepts all five string values for forward-compat with
/// providers that expose the full spectrum; DeepSeek currently collapses
/// `Low`/`Medium` → `high`. OpenAI Codex normalizes inherited DeepSeek-only
/// `Off` to `Low` and displays/sends `Max` as `xhigh` at the provider
/// boundary. The default keyboard cycler walks the three DeepSeek-distinct
/// tiers: `Off` → `High` → `Max` → `Off`; provider-aware callers should use
/// [`ReasoningEffort::cycle_next_for_provider`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Auto,
    #[default]
    Max,
}

impl ReasoningEffort {
    /// Parse a config-file string into an effort tier. Unknown values fall
    /// back to the default (`Max`) rather than erroring out.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" | "false" => Self::Off,
            "low" | "minimal" => Self::Low,
            "medium" | "mid" => Self::Medium,
            "high" => Self::High,
            "auto" | "automatic" => Self::Auto,
            "max" | "maximum" | "xhigh" | "ultracode" => Self::Max,
            _ => Self::default(),
        }
    }

    #[must_use]
    pub fn from_setting_for_provider(value: &str, provider: ApiProvider) -> Self {
        Self::from_setting(value).normalize_for_provider(provider)
    }

    /// Canonical lowercase label used for config storage and UI hints.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Auto => "auto",
            Self::Max => "max",
        }
    }

    /// Short label for the header chip.
    #[must_use]
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "med",
            Self::High => "high",
            Self::Auto => "auto",
            Self::Max => "max",
        }
    }

    /// Provider-facing label for user-visible surfaces.
    #[must_use]
    pub fn display_label_for_provider(self, provider: ApiProvider) -> &'static str {
        match (provider, self.normalize_for_provider(provider)) {
            (ApiProvider::OpenaiCodex, Self::Low) => "low",
            (ApiProvider::OpenaiCodex, Self::Medium) => "medium",
            (ApiProvider::OpenaiCodex, Self::High) => "high",
            (ApiProvider::OpenaiCodex, Self::Max) => "xhigh",
            (_, effort) => effort.short_label(),
        }
    }

    /// Value forwarded to the engine/client. `None` means "provider default"
    /// (for `Off` we still emit `"off"` so the client can inject
    /// `thinking = {"type": "disabled"}`).
    #[must_use]
    pub fn api_value(self) -> Option<&'static str> {
        Some(self.as_setting())
    }

    #[must_use]
    pub fn normalize_for_provider(self, provider: ApiProvider) -> Self {
        if provider != ApiProvider::OpenaiCodex {
            return self;
        }
        match self {
            Self::Off => Self::Low,
            Self::Auto => Self::Medium,
            other => other,
        }
    }

    #[must_use]
    pub fn api_value_for_provider(self, provider: ApiProvider) -> Option<&'static str> {
        if provider != ApiProvider::OpenaiCodex {
            return self.api_value();
        }
        Some(match self.normalize_for_provider(provider) {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "xhigh",
            Self::Off => "low",
            Self::Auto => "medium",
        })
    }

    #[must_use]
    pub fn as_setting_for_provider(self, provider: ApiProvider) -> &'static str {
        self.api_value_for_provider(provider)
            .unwrap_or_else(|| self.as_setting())
    }

    /// Cycle through the three behaviorally distinct tiers.
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Off => Self::High,
            Self::Auto => Self::Off,
            Self::Low | Self::Medium | Self::High => Self::Max,
            Self::Max => Self::Off,
        }
    }

    #[must_use]
    pub fn cycle_next_for_provider(self, provider: ApiProvider) -> Self {
        if provider != ApiProvider::OpenaiCodex {
            return self.cycle_next();
        }
        match self.normalize_for_provider(provider) {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Max,
            Self::Max => Self::Low,
            Self::Off | Self::Auto => Self::Low,
        }
    }
}

/// Sidebar content focus mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarFocus {
    Auto,
    Pinned,
    Tasks,
    Agents,
    Context,
    Hidden,
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

impl SidebarFocus {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "pinned" | "visible" | "show" | "on" => Self::Pinned,
            // Persist/compat key remains "tasks"; user-facing panel is Activity (#4147/#4135).
            "tasks" | "activity" | "live" | "running" => Self::Tasks,
            "agents" | "subagents" | "sub-agents" => Self::Agents,
            "context" | "session" => Self::Context,
            "hidden" | "hide" | "closed" | "off" | "none" => Self::Hidden,
            _ => Self::Auto,
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pinned => "pinned",
            Self::Tasks => "tasks",
            Self::Agents => "agents",
            Self::Context => "context",
            Self::Hidden => "hidden",
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
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Expanded => "expanded",
            Self::Calm => "calm",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerHistorySearch {
    pre_search_input: String,
    pre_search_cursor: usize,
    query: String,
    selected: usize,
}

impl ComposerHistorySearch {
    fn new(pre_search_input: String, pre_search_cursor: usize) -> Self {
        Self {
            pre_search_input,
            pre_search_cursor,
            query: String::new(),
            selected: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputHistoryDraft {
    input: String,
    cursor: usize,
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
const MAX_DRAFT_HISTORY: usize = 50;

impl AppMode {
    /// Productive keyboard cycle: Plan -> Act -> Plan.
    ///
    /// `Auto` remains an internal variant while the real implementation is
    /// redesigned; do not expose it through user-facing mode selection (#3733).
    /// `Yolo` is kept for parse/back-compat only and is not in the Tab cycle.
    /// Operate remains parseable for restored sessions and compatibility, but
    /// user-facing selection must not offer a fail-closed mode whose control
    /// board and host-enforced workflow receipts are not shipped yet.
    pub const CYCLE: [Self; 2] = [Self::Plan, Self::Agent];

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" | "act" | "auto" | "1" => Some(Self::Agent),
            "plan" | "2" => Some(Self::Plan),
            "operate" | "operation" | "ops" | "3" => Some(Self::Operate),
            // Invisible one-way permission shorthand only — never a visible mode.
            "yolo" | "4" | "bypass" | "bypass-permissions" | "bypasspermissions" => {
                Some(Self::Yolo)
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        // Unreleased Multitask never shipped; normalize leftover settings to Operate.
        match value.trim().to_ascii_lowercase().as_str() {
            "multitask" | "multi" | "5" => Self::Operate,
            other => Self::parse(other).unwrap_or(Self::Agent),
        }
    }

    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Auto => "agent",
            // Write current permission vocabulary, not the legacy YOLO label.
            Self::Yolo => "agent",
            Self::Plan => "plan",
            Self::Operate => "operate",
        }
    }

    /// Short label used in the UI footer.
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Agent => "ACT",
            AppMode::Auto => "ACT",
            AppMode::Yolo => "ACT",
            AppMode::Plan => "PLAN",
            AppMode::Operate => "OPERATE",
        }
    }

    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            AppMode::Agent => "Act",
            AppMode::Auto => "Act",
            AppMode::Yolo => "Act",
            AppMode::Plan => "Plan",
            AppMode::Operate => "Operate",
        }
    }

    #[must_use]
    pub fn uses_agent_baseline(self) -> bool {
        matches!(self, Self::Agent | Self::Auto | Self::Operate)
    }

    /// Operate gets a higher parallel launch floor so background fan-out is
    /// not throttled to a single slot when config is low.
    #[must_use]
    pub fn mode_delegation_launch_floor(self) -> usize {
        match self {
            Self::Operate => 4,
            _ => 1,
        }
    }

    #[allow(dead_code)]
    /// Description shown in help or onboarding text.
    pub fn description(self) -> &'static str {
        match self {
            AppMode::Agent | AppMode::Auto => {
                "Act mode - direct work in the current session with tools"
            }
            AppMode::Yolo => "Act mode with Full Access (legacy compatibility setting)",
            AppMode::Plan => "Plan mode - research and design before implementing",
            AppMode::Operate => "Operate mode - coordinate a Fleet for multi-step work",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        let Some(index) = Self::CYCLE.iter().position(|mode| *mode == self) else {
            return Self::Agent;
        };
        Self::CYCLE[(index + 1) % Self::CYCLE.len()]
    }

    #[must_use]
    pub fn previous(self) -> Self {
        let Some(index) = Self::CYCLE.iter().position(|mode| *mode == self) else {
            return Self::Agent;
        };
        Self::CYCLE[(index + Self::CYCLE.len() - 1) % Self::CYCLE.len()]
    }
}

/// Configuration required to bootstrap the TUI.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TuiOptions {
    pub model: String,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config_profile: Option<String>,
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
    pub memory_path: PathBuf,
    #[allow(dead_code)]
    pub notes_path: PathBuf,
    #[allow(dead_code)]
    pub mcp_config_path: PathBuf,
    #[allow(dead_code)]
    pub use_memory: bool,
    /// Start in agent mode (defaults to agent; --yolo starts in YOLO)
    pub start_in_agent_mode: bool,
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
    /// Used by `codewhale pr <N>` (#451) to drop the model into a session
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
    /// Single-entry kill buffer for emacs-style `Ctrl+K` cut / `Ctrl+Y` yank.
    pub kill_buffer: String,
    /// When a large paste is consolidated at submit time, the file @mention
    /// is stored here so it can be appended to the submitted text without
    /// replacing the visible composer content (#3263).
    pub(crate) pending_paste_reference: Option<String>,
    /// When composer content is oversized, the full text is stored here
    /// while `self.input` shows a truncated preview. At submit time the
    /// full text is restored for model submission (#3263).
    pub(crate) oversized_paste_full_text: Option<String>,
    pub input_history: Vec<String>,
    pub draft_history: VecDeque<String>,
    pub clear_undo_buffer: Option<String>,
    pub history_index: Option<usize>,
    pub(crate) history_navigation_draft: Option<InputHistoryDraft>,
    pub composer_history_search: Option<ComposerHistorySearch>,
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
    /// When set, the cursor is the active end of a text selection and
    /// `selection_anchor` is the fixed end.  Both are char-indexed.
    /// `None` means no selection is active.
    pub selection_anchor: Option<usize>,
}

/// Viewport/scroll state — fields related to transcript scrolling and caching.
pub struct ViewportState {
    pub transcript_scroll: TranscriptScroll,
    pub pending_scroll_delta: i32,
    pub transcript_cache: TranscriptViewCache,
    pub transcript_scrollbar_dragging: bool,
    pub last_transcript_area: Option<Rect>,
    pub last_composer_area: Option<Rect>,
    pub last_transcript_top: usize,
    pub last_transcript_visible: usize,
    pub last_transcript_total: usize,
    pub last_transcript_padding_top: usize,
    pub jump_to_latest_button_area: Option<Rect>,
    /// Inner content rect of the composer (excluding border/padding),
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_content: Option<Rect>,
    /// Number of rendered text lines scrolled off the top of the composer,
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_scroll_offset: usize,
    /// Vertical padding above the first text line in the composer,
    /// stored at render time for mouse coordinate mapping.
    pub last_composer_top_padding: usize,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            transcript_scroll: TranscriptScroll::to_bottom(),
            pending_scroll_delta: 0,
            transcript_cache: TranscriptViewCache::new(),
            transcript_scrollbar_dragging: false,
            last_transcript_area: None,
            last_composer_area: None,
            last_transcript_top: 0,
            last_transcript_visible: 0,
            last_transcript_total: 0,
            last_transcript_padding_top: 0,
            jump_to_latest_button_area: None,
            last_composer_content: None,
            last_composer_scroll_offset: 0,
            last_composer_top_padding: 0,
        }
    }
}

/// Session cost and token telemetry state.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_cost: f64,
    pub session_cost_cny: f64,
    pub subagent_cost: f64,
    pub subagent_cost_cny: f64,
    pub subagent_cost_event_seqs: HashSet<u64>,
    pub displayed_cost_high_water: f64,
    pub displayed_cost_high_water_cny: f64,
    pub last_prompt_tokens: Option<u32>,
    pub last_completion_tokens: Option<u32>,
    pub last_prompt_cache_hit_tokens: Option<u32>,
    pub last_prompt_cache_miss_tokens: Option<u32>,
    pub last_reasoning_replay_tokens: Option<u32>,
    pub total_tokens: u32,
    pub total_conversation_tokens: u32,
    /// Accumulated token breakdown for the session.
    pub total_input_tokens: u32,
    pub total_cache_hit_tokens: u32,
    pub total_cache_miss_tokens: u32,
    pub total_output_tokens: u32,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            session_cost: 0.0,
            session_cost_cny: 0.0,
            subagent_cost: 0.0,
            subagent_cost_cny: 0.0,
            subagent_cost_event_seqs: HashSet::new(),
            displayed_cost_high_water: 0.0,
            displayed_cost_high_water_cny: 0.0,
            last_prompt_tokens: None,
            last_completion_tokens: None,
            last_prompt_cache_hit_tokens: None,
            last_prompt_cache_miss_tokens: None,
            last_reasoning_replay_tokens: None,
            total_tokens: 0,
            total_conversation_tokens: 0,
            total_input_tokens: 0,
            total_cache_hit_tokens: 0,
            total_cache_miss_tokens: 0,
            total_output_tokens: 0,
        }
    }
}

impl SessionState {
    /// Reset the accumulated token breakdown fields to zero.
    pub fn reset_token_breakdown(&mut self) {
        self.total_input_tokens = 0;
        self.total_cache_hit_tokens = 0;
        self.total_cache_miss_tokens = 0;
        self.total_output_tokens = 0;
    }
}

/// Evidence collected during a turn for the post-turn receipt.
#[derive(Debug, Clone)]
pub struct ToolEvidence {
    pub tool_name: String,
    pub summary: String,
}

/// Global UI state for the TUI.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub mode: AppMode,
    /// Composer sub-state (input, cursor, history, menus).
    pub composer: ComposerState,
    /// Viewport sub-state (scroll, cache, selection).
    pub viewport: ViewportState,
    /// Ocean work-surface state. Kept separate from transcript/sidebar state
    /// so the replacement shell can be removed or promoted as one unit.
    pub work_surface: crate::tui::work_surface::WorkSurfaceState,
    /// Session sub-state (cost, tokens, telemetry).
    pub session: SessionState,
    pub history: Vec<HistoryCell>,
    pub history_version: u64,
    /// Per-cell revision counter, kept in lockstep with `history`.
    pub history_revisions: Vec<u64>,
    /// Monotonic counter used to issue fresh per-cell revisions.
    pub next_history_revision: u64,
    pub is_loading: bool,
    /// Timestamp of the most recent Enter while the engine was busy.
    /// Used by `enter_with_double_tap()` to detect a double-tap within 500 ms.
    pub last_enter_instant: Option<Instant>,
    /// Degraded connectivity mode; new user inputs are queued for later retry.
    pub offline_mode: bool,
    /// Whether an `EngineEvent::Error` has already been posted for the
    /// current turn. Suppresses the redundant "Turn failed:" status line
    /// that `TurnComplete { error: .. }` would otherwise emit on top of
    /// the in-transcript error cell.
    pub turn_error_posted: bool,
    /// Legacy status text sink retained for compatibility with existing call sites.
    pub status_message: Option<String>,
    /// Recent status toasts (ephemeral, newest at back).
    pub status_toasts: VecDeque<StatusToast>,
    /// Sticky status toast used for important warnings/errors.
    pub sticky_status: Option<StatusToast>,
    /// Last status text already promoted from `status_message` into toast state.
    pub last_status_message_seen: Option<String>,
    pub model: String,
    /// When true, the model is auto-selected based on request complexity
    /// rather than using a fixed model. The `/model auto` command sets this.
    pub auto_model: bool,
    /// Current API provider (mirrors `Config::api_provider`).
    /// Updated by `/provider` switches so the UI/commands can read the
    /// active backend without re-deriving it from the live config.
    pub api_provider: ApiProvider,
    /// Resolved provider/model route limits for the active runtime route.
    pub active_route_limits: Option<RouteLimits>,
    /// Current reasoning-effort tier for DeepSeek thinking mode.
    /// Cycled via Shift+Tab; initialized from config at startup.
    pub reasoning_effort: ReasoningEffort,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config_profile: Option<String>,
    pub mcp_config_path: PathBuf,
    pub skills_dir: PathBuf,
    pub skills_scan_codewhale_only: bool,
    /// Path to the user-memory file (#489). Always populated; only
    /// consulted when `use_memory` is `true`.
    pub memory_path: PathBuf,
    /// Whether the user-memory feature is enabled (#489). Mirrors
    /// `Config::memory_enabled()` at app boot. Used by the `# foo`
    /// composer interception (also gated by `moraine_fallback`),
    /// the `/memory` slash command, and tool registration for
    /// `remember`.
    pub use_memory: bool,
    /// True when legacy memory push/inject behavior should stay disabled
    /// because Moraine pull/recall is the configured memory backend.
    pub moraine_fallback: bool,
    pub use_alt_screen: bool,
    pub use_mouse_capture: bool,
    /// When true, plain Up/Down on an empty composer scroll the transcript
    /// instead of navigating input history.  Defaults to `true` when mouse
    /// capture is off: terminals that convert mouse-wheel events to arrow-key
    /// sequences (e.g. Windows CMD without `WT_SESSION`) get page-scrolling
    /// without any explicit config (#1443).
    pub composer_arrows_scroll: bool,
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
    pub use_bracketed_paste: bool,
    pub calm_mode: bool,
    pub low_motion: bool,
    pub ocean_started_at: Instant,
    /// Start of the underwater shell's one-shot successful-turn exhale.
    /// Kept separate from the ambient ocean clock so completion can settle
    /// once without restarting or repainting the transcript field.
    pub ocean_completion_started_at: Option<Instant>,
    /// History length at the current turn boundary. Successful completion
    /// uses this stable index to settle only the receipts produced by that
    /// turn, never old transcript rows.
    pub ocean_turn_history_start: usize,
    /// First committed history cell participating in the current one-shot
    /// receipt-settle cascade.
    pub ocean_receipt_settle_start: Option<usize>,
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
    /// Route payment truth. Model pricing alone cannot distinguish metered
    /// API calls from OAuth or token-plan quota.
    pub billing_presentation: crate::route_billing::BillingPresentation,
    pub composer_density: ComposerDensity,
    pub composer_border: bool,
    /// Voice input state toggled by `/voice`.
    pub voice_enabled: bool,
    /// Auto-send after transcription when the transcript ends with an
    /// explicit send instruction ("send it" / "发送"). Toggled by `/voice-send`.
    pub voice_send_enabled: bool,
    /// AI-assisted dictation that sees the current composer text.
    /// Toggled by `/voice-control`.
    pub voice_control_enabled: bool,
    pub transcript_spacing: TranscriptSpacing,
    pub sidebar_width_percent: u16,
    pub sidebar_focus: SidebarFocus,
    /// Last known mouse position for tooltip placement.
    pub last_mouse_pos: Option<(u16, u16)>,
    /// Sidebar focus/hidden state changed and needs persistence.
    pub sidebar_focus_dirty: bool,
    /// Whether the session-context panel is enabled (#504).
    pub context_panel: bool,
    /// Minimum number of consecutive safe tool cells needed for auto-collapse.
    pub tool_collapse_threshold: usize,
    /// Tool runs the user explicitly expanded. Stores original history indices.
    pub expanded_tool_runs: HashSet<usize>,
    /// Current dense tool-run collapse behavior.
    pub tool_collapse_mode: ToolCollapseMode,
    pub max_input_history: usize,
    pub allow_shell: bool,
    pub verbosity: Option<String>,
    pub max_subagents: usize,
    /// Per-SSE-chunk idle timeout for streamed turns, in seconds.
    pub stream_chunk_timeout_secs: u64,
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
    pub onboarding_provider: ApiProvider,
    pub onboarding_workspace_trust_gate: bool,
    pub api_key_env_only: bool,
    pub api_key_input: String,
    pub api_key_cursor: usize,
    #[allow(dead_code)]
    pub yolo: bool,
    /// One-shot YOLO→Act+Bypass migration notice for this session (#0.8.68 M6).
    yolo_compat_notified: bool,
    /// One-shot Shift+Tab/Ctrl+T rebinding notice for this session (#0.8.68 M3).
    keybinding_migration_notified: bool,
    /// Durable Agent-era permission baseline that Plan/YOLO derive from and
    /// restore to (#3386). Refreshed from the live fields whenever the user
    /// leaves Agent mode; see [`base_policy_for_mode`] and `set_mode`.
    mode_prefs: ModeSessionPrefs,
    /// True when config/requirements supplied an approval policy. In that
    /// case the TUI-only Shift+Tab preference must not loosen it.
    approval_policy_locked: bool,
    /// True only when an organization requirements file owns approval policy.
    /// Unlike a user-owned config key, this source cannot be edited in-app.
    approval_policy_requirements_managed: bool,
    // Clipboard handler
    pub clipboard: ClipboardHandler,
    pub approval_mode: ApprovalMode,
    // Modal view stack (approval/help/etc.)
    pub view_stack: ViewStack,
    /// Trust mode - allow access outside workspace
    pub trust_mode: bool,
    /// Ordered footer items loaded from `tui.status_items` at startup. The
    /// renderer iterates this slice; no item is hardcoded in the footer path.
    pub status_items: Vec<crate::config::StatusItem>,
    /// Project documentation (AGENTS.md or CLAUDE.md)
    #[allow(dead_code)]
    pub project_doc: Option<String>,
    /// Number of MCP servers declared in the user's config at app boot.
    /// Used by passive UI projections; `0` hides the MCP status.
    pub mcp_configured_count: usize,
    /// Tool execution log
    pub tool_log: Vec<String>,
    /// Active skill to apply to next user message
    pub active_skill: Option<String>,
    /// Cached (name, description) pairs from the skill registry.
    /// Populated once at startup and refreshed on install/uninstall so
    /// the slash menu can show skills without filesystem I/O on every keystroke.
    pub cached_skills: Vec<(String, String)>,
    /// Canonical tool call cells by tool id. Prepared tools are written
    /// directly to `history`; committed outcomes update the indexed cell.
    pub tool_cells: HashMap<String, usize>,
    /// Tool calls that should be ignored by the UI
    pub ignored_tool_calls: HashSet<String>,
    /// Current streaming assistant cell
    pub streaming_message_index: Option<usize>,
    /// True after a local cancel key has been handled and before the engine's
    /// authoritative TurnComplete arrives. Stream events already queued for
    /// the cancelled turn are ignored so text does not keep appearing after
    /// Ctrl+C/Esc returns focus to the composer.
    pub suppress_stream_events_until_turn_complete: bool,
    /// Tool calls captured for the pending assistant message
    pub pending_tool_uses: Vec<(String, String, Value)>,
    /// User messages queued while a turn is running
    pub queued_messages: VecDeque<QueuedMessage>,
    /// Draft queued message being edited
    pub queued_draft: Option<QueuedMessage>,
    /// Legacy pending-steer bucket retained for session compatibility. New
    /// in-flight input uses Enter for same-turn steering and Tab for queued
    /// follow-ups; Esc only cancels the active turn.
    pub pending_steers: VecDeque<QueuedMessage>,
    /// Engine-rejected steers (e.g. a tool was already running and couldn't be
    /// cancelled cleanly). Surfaced in the pending-input preview so the user
    /// knows the steer was deferred to end-of-turn. Today no engine path
    /// produces these; the field is scaffolding for a future signalling
    /// channel and the bucket renders with a rejected-steer label when
    /// populated.
    pub rejected_steers: VecDeque<String>,
    /// Legacy resend flag for pending steer recovery.
    pub submit_pending_steers_after_interrupt: bool,
    /// Start time for current turn
    pub turn_started_at: Option<Instant>,
    /// Most recent engine event observed for the current turn. This is
    /// separate from `turn_started_at` because the latter drives elapsed-time
    /// UI and must not be reset during long but healthy turns.
    pub turn_last_activity_at: Option<Instant>,
    /// Sum of completed turn durations for this `App` instance (#448
    /// follow-up). Drives the footer's `worked Nh Mm` chip so the
    /// label reflects actual model work, not wall-clock since launch.
    /// Incremented on `TurnComplete` from the elapsed time of the
    /// just-finished turn. Resets per launch.
    pub cumulative_turn_duration: std::time::Duration,
    /// DeepSeek account balance, refreshed once per turn completion.
    /// Shared cell updated by background fetch tasks; read lock in the UI thread.
    pub balance_cell: std::sync::Arc<std::sync::Mutex<Option<crate::pricing::BalanceInfo>>>,
    /// Tracks whether the initial balance fetch has been attempted for this session.
    pub balance_initiated: bool,
    /// Timestamp of the last balance fetch, used to debounce rapid requests.
    pub last_balance_fetch: Option<std::time::Instant>,
    /// Current runtime turn id (if known).
    pub runtime_turn_id: Option<String>,
    /// Current runtime turn status (if known).
    pub runtime_turn_status: Option<String>,
    /// Monotonic turn counter for stable user-facing labels (#3030).
    /// Incremented each time a new turn starts; displayed as "Turn N".
    pub turn_counter: u64,
    /// When the UI accepted a user message but has not observed `TurnStarted` yet.
    pub dispatch_started_at: Option<Instant>,

    /// Whether the UI needs to be redrawn.
    pub needs_redraw: bool,
    /// When true, the next draw will be a full repaint (terminal clear +
    /// all cells redrawn) instead of a ratatui incremental diff. Used by
    /// theme switches where the diff engine may miss color-only changes
    /// in sidebar cells that were previously rendered with palette constants.
    pub force_next_full_repaint: bool,
    /// Whether context compaction is currently in progress.
    pub is_compacting: bool,
    /// Whether context purge is currently in progress.
    pub is_purging: bool,
    /// Set when the user scrolls up/down during a streaming turn so subsequent
    /// streamed chunks don't yank the view back to the live tail. Cleared
    /// when the user explicitly returns to bottom or the turn completes.
    pub user_scrolled_during_stream: bool,
    /// Timestamp of the last user message send (for brief visual feedback).
    pub last_send_at: Option<Instant>,
    /// Most recent user prompt accepted for an active engine turn. Ctrl+C can
    /// restore this into an empty composer after cancelling that turn.
    pub last_submitted_prompt: Option<String>,
    /// Startup prompt should be submitted automatically after the engine is ready.
    pub auto_submit_initial_input: bool,
    /// Two-tap quit confirmation. When set, a prior Ctrl+C in idle state has
    /// armed the quit shortcut; a second Ctrl+C before this `Instant` exits
    /// the app, while expiry silently re-arms the prompt for next time.
    /// Stays `None` while a turn is in flight or a modal/picker is open so
    /// Ctrl+C keeps its current "interrupt this turn" semantics in those
    /// states. See [`App::arm_quit`] / [`App::quit_is_armed`].
    pub quit_armed_until: Option<Instant>,

    // === Transcript filtering (#397) ===
    /// Transcript cells the user has collapsed (hidden from view).
    /// Stores **original** virtual cell indices (pre-filtering).
    pub collapsed_cells: HashSet<usize>,
    /// Mapping from filtered cell index → original virtual index.
    /// Populated during `ChatWidget::new` by filtering out collapsed cells.
    /// Used by `build_context_menu_entries` to convert line-meta indices
    /// back to original indices for the `HideCell` / `ShowCell` actions.
    pub collapsed_cell_map: Vec<usize>,

    /// Optional title shown in the composer border.
    pub session_title: Option<String>,

    /// Post-turn receipt rendered as transient composer chrome.
    /// Set when a turn completes; cleared when a new turn starts or after expiry.
    pub receipt_text: Option<String>,
    pub receipt_started_at: Option<Instant>,
    /// Tool evidence collected during the current turn for the receipt.
    pub tool_evidence: Vec<ToolEvidence>,
}

/// Message queued while the engine is busy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub display: String,
    pub skill_instruction: Option<String>,
}

/// How a freshly-typed user input should be sent.
///
/// Picked by [`App::decide_submit_disposition`] when the user hits Enter on a
/// non-empty composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitDisposition {
    /// Engine idle and online: send immediately.
    Immediate,
    /// Park on `queued_messages` (offline, or engine busy — #382).
    Queue,
    /// Explicit steer via Ctrl+Enter (#382). Not returned by `decide_submit_disposition`.
    #[allow(dead_code)]
    Steer,
    /// Park on `queued_messages` for dispatch after TurnComplete.
    /// Legacy path; #382 unified busy states under `Queue`.
    #[allow(dead_code)]
    QueueFollowUp,
}

impl QueuedMessage {
    pub fn new(display: String, skill_instruction: Option<String>) -> Self {
        Self {
            display,
            skill_instruction,
        }
    }

    #[allow(dead_code)] // Tests and queue helpers use the display-only form; send path resolves @mentions.
    pub fn content(&self) -> String {
        if let Some(skill_instruction) = self.skill_instruction.as_ref() {
            format!(
                "{skill_instruction}\n\n---\n\nUser request: {}",
                self.display
            )
        } else {
            self.display.clone()
        }
    }
}

// === Errors ===

/// Errors that can occur while submitting API keys during onboarding.
#[derive(Debug, Error)]
pub enum ApiKeyError {
    /// The provided API key was empty.
    #[error("Failed to save API key: API key cannot be empty")]
    Empty,
    /// Persisting the API key failed.
    #[error("Failed to save API key: {source}")]
    SaveFailed { source: anyhow::Error },
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

fn default_composer_arrows_scroll(use_mouse_capture: bool) -> bool {
    default_composer_arrows_scroll_for_platform(use_mouse_capture, cfg!(windows))
}

fn default_composer_arrows_scroll_for_platform(use_mouse_capture: bool, _is_windows: bool) -> bool {
    !use_mouse_capture
}

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
            config_profile,
            allow_shell,
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            max_subagents,
            skills_dir: global_skills_dir,
            memory_path,
            notes_path: _,
            mcp_config_path,
            use_memory,
            start_in_agent_mode,
            skip_onboarding,
            yolo,
            resume_session_id: _,
            initial_input,
        } = options;

        // Start from disk-only preferences so one-time migrations can never
        // persist terminal/environment overlays such as NO_ANIMATIONS. Apply
        // those overlays only after any normalized settings write succeeds.
        let mut settings = Settings::load_persisted().unwrap_or_else(|_| Settings::default());
        let legacy_yolo_default = settings.legacy_yolo_default_detected();
        let legacy_yolo_full_access = if legacy_yolo_default {
            let control = config.approval_policy_control(
                config_path.as_deref(),
                config_profile.as_deref(),
                &workspace,
            );
            match control {
                crate::config::ApprovalPolicyControl::Unset => {
                    if let Err(error) = settings.save() {
                        tracing::warn!(
                            "failed to normalize legacy YOLO settings; retrying next launch: {error:#}"
                        );
                    }
                    true
                }
                crate::config::ApprovalPolicyControl::RootConfig => {
                    let active_config_path =
                        crate::config::resolve_load_config_path(config_path.clone());
                    match crate::config_persistence::persist_unset_root_key(
                        active_config_path.as_deref(),
                        "approval_policy",
                    ) {
                        Ok(_) => {
                            if let Err(error) = settings.save() {
                                tracing::warn!(
                                    "removed legacy approval_policy but could not normalize settings; retrying next launch: {error:#}"
                                );
                            }
                            true
                        }
                        Err(error) => {
                            tracing::warn!(
                                "could not migrate legacy YOLO approval policy; keeping the controlling policy: {error:#}"
                            );
                            false
                        }
                    }
                }
                source => {
                    tracing::warn!(
                        "legacy YOLO setting was not allowed to override {}",
                        source.label()
                    );
                    false
                }
            }
        } else {
            false
        };
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
        let tui_prefs_warning = crate::settings::TuiPrefs::path().ok().and_then(|p| {
            if p.exists() {
                std::fs::read_to_string(&p).ok().and_then(|raw| {
                    ::toml::from_str::<::toml::Value>(&raw)
                        .err()
                        .map(|e| format!("⚠ tui.toml is malformed — using defaults ({e})"))
                })
            } else {
                None
            }
        });

        let provider = config.api_provider();
        let mut effective_auth_config = config.clone();
        effective_auth_config.provider = Some(provider.as_str().to_string());
        // Authentication follows the already validated entry configuration.
        // Saved UI preferences cannot change the production model backend.
        let needs_api_key = !has_api_key(&effective_auth_config);
        let api_key_env_only =
            crate::config::active_provider_uses_env_only_api_key(&effective_auth_config);
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
        let sidebar_width_percent = settings.sidebar_width_percent;
        let sidebar_focus = SidebarFocus::from_setting(&settings.sidebar_focus);
        let max_input_history = settings.max_input_history;
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
        let auto_model = model.trim().eq_ignore_ascii_case("auto");
        let active_context_window_override = config.context_window_for_provider_config(provider);
        let active_route_limits = if auto_model {
            active_context_window_override.map(|window| RouteLimits {
                context_tokens: Some(u64::from(window)),
                ..RouteLimits::default()
            })
        } else {
            let saved_provider_model = config
                .provider_config_for(provider)
                .and_then(|provider| provider.model.as_deref());
            crate::route_runtime::resolve_route_candidate(
                provider,
                Some(&model),
                saved_provider_model,
                Some(effective_auth_config.deepseek_base_url()),
                active_context_window_override,
            )
            .ok()
            .and_then(|candidate| crate::route_budget::known_route_limits(candidate.limits))
        };
        let configured_reasoning_effort = settings
            .reasoning_effort
            .as_deref()
            .or_else(|| config.reasoning_effort());
        let reasoning_effort = if auto_model {
            ReasoningEffort::Auto
        } else {
            configured_reasoning_effort.map_or_else(ReasoningEffort::default, |s| {
                ReasoningEffort::from_setting_for_provider(s, provider)
            })
        };

        // Resolve the saved mode separately from the permission posture.
        let preferred_mode = AppMode::from_setting(&settings.default_mode);
        let yolo_compat = yolo || (preferred_mode == AppMode::Yolo && !start_in_agent_mode);
        let initial_mode = if yolo_compat || start_in_agent_mode {
            AppMode::Agent
        } else {
            preferred_mode
        };
        let needs_workspace_trust = !yolo_compat
            && crate::tui::onboarding::needs_trust_at(config_path.as_deref(), &workspace);
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

        // Durable Agent-era permission baseline (#3386). Plan/YOLO derive from
        // and restore to this. Legacy Auto inputs parse to Agent; if an older
        // caller still constructs `AppMode::Auto` directly, it projects through
        // the Agent baseline instead of enabling a fourth runtime posture. When
        // the user starts in YOLO the live shell flag is force-enabled below, so
        // the baseline shell value is taken from the interactive default (the
        // pre-mode Agent surface) rather than the YOLO-forced live mirror;
        // otherwise it mirrors the resolved `allow_shell` option, which already
        // carries that same interactive default. Using `interactive_allow_shell()`
        // here keeps the Agent baseline identical regardless of launch mode, so
        // a YOLO -> Agent downshift exposes shell (approval-gated) exactly as
        // documented, while an explicit `allow_shell = false` still hides it.
        // Trust is never part of the Agent baseline (it is YOLO-only authority).
        // Approval mirrors the configured policy.
        let explicit_approval_mode = (!legacy_yolo_full_access)
            .then_some(config.approval_policy.as_deref())
            .flatten()
            .and_then(ApprovalMode::from_config_value);
        let approval_policy_locked =
            !legacy_yolo_full_access && config.approval_policy_is_managed();
        let approval_policy_requirements_managed = config.approval_policy_is_requirements_managed();
        let saved_permission_posture = if approval_policy_locked {
            None
        } else {
            settings
                .permission_posture
                .as_deref()
                .and_then(ApprovalMode::from_config_value)
        };
        let configured_approval_mode = explicit_approval_mode
            .or(saved_permission_posture)
            .unwrap_or_default();
        let mode_prefs = ModeSessionPrefs {
            agent_allow_shell: if yolo_compat || matches!(initial_mode, AppMode::Yolo) {
                config.interactive_allow_shell()
            } else {
                allow_shell
            },
            agent_trust_mode: false,
            // The YOLO-compat launch elevates the *live* approval mirror to
            // Bypass below; the durable Agent baseline keeps the configured
            // policy so a YOLO -> Agent downshift restores it.
            agent_approval_mode: configured_approval_mode,
        };
        let allow_shell = allow_shell || yolo_compat || matches!(initial_mode, AppMode::Yolo);

        let skills_scan_codewhale_only = config.skills_config().scan_codewhale_only();
        let skills_dir = resolve_skills_dir(&workspace, &global_skills_dir, config);
        let cached_skills =
            Self::discover_cached_skills(&workspace, &skills_dir, skills_scan_codewhale_only);

        let input_history = crate::composer_history::load_history();
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
        let mut app = Self {
            mode: initial_mode,
            composer: ComposerState {
                input: initial_input_text,
                cursor_position: initial_input_cursor,
                kill_buffer: String::new(),
                pending_paste_reference: None,
                oversized_paste_full_text: None,
                input_history,
                draft_history: VecDeque::new(),
                clear_undo_buffer: None,
                history_index: None,
                history_navigation_draft: None,
                composer_history_search: None,
                slash_menu_selected: 0,
                slash_menu_hidden: false,
                mention_menu_selected: 0,
                mention_menu_hidden: false,
                mention_completion_cache: None,
                mention_candidate_cache: None,
                selection_anchor: None,
            },
            viewport: ViewportState::default(),
            work_surface: crate::tui::work_surface::WorkSurfaceState::with_placement(
                work_surface_placement,
            ),
            session: SessionState::default(),
            history: Vec::new(),
            history_version: 0,
            history_revisions: Vec::new(),
            next_history_revision: 1,
            is_loading: false,
            last_enter_instant: None,
            offline_mode: false,
            turn_error_posted: false,
            // Surface parse warnings so the user knows their config file is
            // broken instead of silently losing all settings.
            status_message: settings_parse_warning.or(tui_prefs_warning),
            status_toasts: VecDeque::new(),
            sticky_status: None,
            last_status_message_seen: None,
            model,
            auto_model,
            api_provider: provider,
            active_route_limits,
            reasoning_effort,
            workspace,
            config_path,
            config_profile,
            mcp_config_path: mcp_config_path.clone(),
            skills_dir,
            skills_scan_codewhale_only,
            memory_path,
            use_memory,
            moraine_fallback: config.moraine_fallback(),
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            calm_mode,
            low_motion,
            ocean_started_at: Instant::now(),
            ocean_completion_started_at: None,
            ocean_turn_history_start: 0,
            ocean_receipt_settle_start: None,
            fancy_animations,
            ocean_treatment,
            synchronized_output_enabled,
            status_indicator,
            show_thinking,
            show_tool_details,
            cost_currency,
            billing_presentation: crate::route_billing::for_route(config, provider),
            composer_density,
            composer_border,
            voice_enabled: false,
            voice_send_enabled: false,
            voice_control_enabled: false,
            transcript_spacing,
            sidebar_width_percent,
            sidebar_focus,
            last_mouse_pos: None,
            sidebar_focus_dirty: false,
            context_panel: settings.context_panel,
            tool_collapse_threshold: 3,
            expanded_tool_runs: HashSet::new(),
            tool_collapse_mode: ToolCollapseMode::from_setting(&settings.tool_collapse_mode),
            max_input_history,
            allow_shell,
            verbosity: config.verbosity.clone(),
            max_subagents,
            stream_chunk_timeout_secs: config.stream_chunk_timeout_secs(),
            child_agents: ChildAgents::default(),
            ui_theme,
            theme_id,
            onboarding,
            onboarding_needs_api_key: needs_api_key,
            onboarding_provider: provider,
            onboarding_workspace_trust_gate,
            api_key_env_only,
            api_key_input: String::new(),
            api_key_cursor: 0,
            yolo: yolo_compat,
            yolo_compat_notified: false,
            keybinding_migration_notified: false,
            mode_prefs,
            approval_policy_locked,
            approval_policy_requirements_managed,
            clipboard: ClipboardHandler::new(),
            approval_mode: if yolo_compat || matches!(initial_mode, AppMode::Yolo) {
                ApprovalMode::Bypass
            } else {
                configured_approval_mode
            },
            view_stack: ViewStack::new(),
            trust_mode: yolo_compat || initial_mode == AppMode::Yolo,
            status_items: config
                .tui
                .as_ref()
                .and_then(|tui| tui.status_items.clone())
                .unwrap_or_else(crate::config::StatusItem::default_footer),
            project_doc: None,
            // Read the MCP config once at boot to know how many servers the
            // user declared. Errors fall through to zero so a missing or
            // malformed config simply hides the passive UI projections.
            mcp_configured_count,
            tool_log: Vec::new(),
            active_skill: None,
            cached_skills,
            tool_cells: HashMap::new(),
            ignored_tool_calls: HashSet::new(),
            streaming_message_index: None,
            suppress_stream_events_until_turn_complete: false,
            pending_tool_uses: Vec::new(),
            queued_messages: VecDeque::new(),
            queued_draft: None,
            pending_steers: VecDeque::new(),
            rejected_steers: VecDeque::new(),
            submit_pending_steers_after_interrupt: false,
            turn_started_at: None,
            turn_last_activity_at: None,
            cumulative_turn_duration: std::time::Duration::ZERO,
            balance_cell: std::sync::Arc::new(std::sync::Mutex::new(None)),
            balance_initiated: false,
            last_balance_fetch: None,
            runtime_turn_id: None,
            runtime_turn_status: None,
            turn_counter: 0,
            dispatch_started_at: None,
            needs_redraw: true,
            force_next_full_repaint: false,
            is_compacting: false,
            is_purging: false,
            user_scrolled_during_stream: false,
            last_send_at: None,
            last_submitted_prompt: None,
            auto_submit_initial_input,
            quit_armed_until: None,
            collapsed_cells: HashSet::new(),
            collapsed_cell_map: Vec::new(),
            composer_arrows_scroll: config
                .tui
                .as_ref()
                .and_then(|tui| tui.composer_arrows_scroll)
                .unwrap_or_else(|| default_composer_arrows_scroll(use_mouse_capture)),
            mention_menu_limit: settings.mention_menu_limit,
            mention_walk_depth: settings.mention_walk_depth,
            mention_menu_behavior: settings.mention_menu_behavior.clone(),
            workspace_follow_symlinks: settings.workspace_follow_symlinks,
            session_title: None,
            receipt_text: None,
            receipt_started_at: None,
            tool_evidence: Vec::new(),
        };
        if yolo_compat {
            app.notify_yolo_compat_once();
        }
        app
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

    pub fn refresh_skill_cache(&mut self) {
        let skills_dir = self.skills_dir.clone();
        self.cached_skills = Self::discover_cached_skills(
            &self.workspace,
            &skills_dir,
            self.skills_scan_codewhale_only,
        );
    }

    pub fn submit_api_key(&mut self) -> Result<SavedCredential, ApiKeyError> {
        let key = self.api_key_input.trim().to_string();
        if key.is_empty() {
            return Err(ApiKeyError::Empty);
        }

        let saved = if matches!(
            self.onboarding_provider,
            ApiProvider::Deepseek | ApiProvider::DeepseekCN
        ) {
            save_api_key(&key).map_err(|source| ApiKeyError::SaveFailed { source })?
        } else {
            let path = save_api_key_for(self.onboarding_provider, &key)
                .map_err(|source| ApiKeyError::SaveFailed { source })?;
            SavedCredential::ConfigFile(path)
        };
        self.api_key_input.clear();
        self.api_key_cursor = 0;
        self.onboarding_needs_api_key = false;
        self.api_key_env_only = false;
        Ok(saved)
    }

    pub fn finish_onboarding_without_feature_intro(&mut self) {
        self.onboarding = OnboardingState::None;
        if let Err(err) = crate::tui::onboarding::mark_onboarded() {
            self.status_message = Some(format!("Failed to mark onboarding: {err}"));
        }
        self.needs_redraw = true;
    }

    /// Mark the first-run follow-up as seen without inserting a transcript
    /// message. The empty underwater launch surface owns setup guidance; a
    /// synthetic history cell would hide that surface before the user sends
    /// anything.
    pub fn maybe_show_feature_intro(&mut self) {
        if self.onboarding != OnboardingState::None {
            return;
        }
        // Never claim "setup is ready" when auth is still missing — e.g.
        // `--skip-onboarding` with no API key (#3985). Leave the flag unset so
        // the tip can appear after the user finishes provider setup.
        if self.onboarding_needs_api_key {
            return;
        }
        let mut settings = Settings::load_persisted().unwrap_or_default();
        if settings.feature_intro_shown {
            return;
        }
        settings.feature_intro_shown = true;
        if let Err(err) = settings.save() {
            self.status_message = Some(format!("Failed to save feature-intro flag: {err}"));
            // Still show the nudge; the flag write may simply retry next launch.
        }
        self.status_message = Some(self.tr(MessageId::FleetReadyNotice).into_owned());
        self.needs_redraw = true;
    }

    pub fn set_mode(&mut self, mode: AppMode) -> bool {
        let requested_mode = mode;
        let mode = match mode {
            AppMode::Yolo => AppMode::Agent,
            other => other,
        };
        let yolo_compat = requested_mode == AppMode::Yolo;
        let previous_mode = self.mode;
        if previous_mode == mode && !yolo_compat && !self.yolo {
            return false;
        }

        self.mode = mode;
        // Mode chip lives in the header — skip redundant status/toast copy.

        // Mode cycling is untangled from permission policy (#3386). The user
        // only edits the durable permission surface while in Agent mode, so
        // refresh the baseline from the live mirrors whenever we leave Agent —
        // before any transient Plan/YOLO policy overwrites them. This subsumes
        // the old per-mode `YoloRestoreState`/`PlanRestoreState` snapshots:
        // cross-mode hops (Plan -> YOLO, YOLO -> Plan) do not touch the baseline,
        // so YOLO's elevated authority never bleeds into the restored Agent
        // surface (#3279).
        if previous_mode.uses_agent_baseline() && !self.yolo {
            self.mode_prefs = ModeSessionPrefs {
                agent_allow_shell: self.allow_shell,
                agent_trust_mode: self.trust_mode,
                agent_approval_mode: self.approval_mode,
            };
        }

        if yolo_compat {
            // Transient full-access mirrors for legacy YOLO entry points; do not
            // persist trust/shell elevation into the durable Agent baseline.
            self.allow_shell = true;
            self.trust_mode = true;
            self.approval_mode = ApprovalMode::Bypass;
            self.yolo = true;
            self.notify_yolo_compat_once();
        } else {
            let policy = base_policy_for_mode(mode, &self.mode_prefs);
            self.allow_shell = policy.allow_shell;
            self.trust_mode = policy.trust_mode;
            self.approval_mode = policy.approval_mode;
            self.yolo = matches!(policy.approval_mode, ApprovalMode::Bypass);
        }

        self.needs_redraw = true;
        true
    }

    fn notify_yolo_compat_once(&mut self) {
        if self.yolo_compat_notified {
            return;
        }
        self.yolo_compat_notified = true;
        // Per-install suppression: check the persisted flag so the toast
        // appears exactly once across sessions, not every launch.
        if let Ok(settings) = crate::settings::Settings::load()
            && settings.yolo_deprecation_shown
        {
            return;
        }
        // Persist the flag best-effort; toast still fires even if the write
        // fails (retries on the next attempt).
        if let Ok(mut settings) = crate::settings::Settings::load_persisted() {
            settings.yolo_deprecation_shown = true;
            let _ = settings.save();
        }
        self.push_status_toast(
            "Legacy full-access mode is deprecated — use Act + Full Access (Shift+Tab)".to_string(),
            StatusToastLevel::Warning,
            Some(8_000),
        );
    }

    /// One-release migration notice for the Shift+Tab/Ctrl+T rebinding: users
    /// pressing Shift+Tab expecting the old thinking cycle land here first.
    fn notify_keybinding_migration_once(&mut self) {
        if self.keybinding_migration_notified {
            return;
        }
        self.keybinding_migration_notified = true;
        self.push_status_toast(
            "Shift+Tab now cycles permissions — reasoning effort moved to Ctrl+T".to_string(),
            StatusToastLevel::Info,
            Some(8_000),
        );
    }

    /// Whether mode/thinking selection is locked because a turn is in flight.
    ///
    /// While `is_loading`, the model/permission surface the engine is acting on
    /// must not shift underneath it, so user-initiated mode and thinking changes
    /// are refused (#2982). Returns true (and posts a concise status message) if
    /// the change should be rejected — the caller leaves the selection unchanged
    /// so the chip "twitches" back instead of moving.
    fn reject_setting_change_while_busy(&mut self, what: &str) -> bool {
        if self.is_loading {
            self.status_message = Some(format!(
                "{what} is locked while a turn is running — press Esc to interrupt first"
            ));
            self.needs_redraw = true;
            true
        } else {
            false
        }
    }

    /// Cycle through productive modes: Plan → Act → Plan.
    pub fn cycle_mode(&mut self) {
        if self.reject_setting_change_while_busy("Mode") {
            return;
        }
        let next = self.mode.next();
        let _ = self.set_mode(next);
    }

    /// Cycle through modes in reverse.
    #[allow(dead_code)]
    pub fn cycle_mode_reverse(&mut self) {
        if self.reject_setting_change_while_busy("Mode") {
            return;
        }
        let next = self.mode.previous();
        let _ = self.set_mode(next);
    }

    /// Cycle reasoning-effort through the active provider's distinct tiers.
    pub fn cycle_effort(&mut self) {
        if self.reject_setting_change_while_busy("Thinking") {
            return;
        }
        self.reasoning_effort = self
            .reasoning_effort
            .cycle_next_for_provider(self.api_provider);
        self.needs_redraw = true;
        // Effort chip in the header is canonical — no duplicate toast.
    }

    /// Cycle the durable Agent permission posture: Ask → Auto-Review → Bypass.
    pub fn cycle_approval_posture(&mut self) -> bool {
        if self.reject_setting_change_while_busy("Permissions") {
            return false;
        }
        if self.mode == AppMode::Plan {
            self.push_status_toast(
                "Plan is Read Only; switch to Act to change permissions".to_string(),
                StatusToastLevel::Info,
                Some(5_000),
            );
            self.needs_redraw = true;
            return false;
        }
        if self.approval_policy_locked() {
            self.push_status_toast(
                "Permissions are controlled by config or managed requirements".to_string(),
                StatusToastLevel::Warning,
                Some(6_000),
            );
            self.needs_redraw = true;
            return false;
        }
        let next = self.mode_prefs.agent_approval_mode.cycle_permission_next();
        let persisted = match next {
            ApprovalMode::Suggest => "ask",
            ApprovalMode::Auto => "auto-review",
            ApprovalMode::Bypass => "full-access",
            ApprovalMode::Never => "never",
        };
        let persistence_result = (|| -> anyhow::Result<()> {
            let mut settings = Settings::load_persisted()?;
            settings.permission_posture = Some(persisted.to_string());
            settings.save()
        })();
        if let Err(err) = persistence_result {
            self.push_status_toast(
                format!("Permissions were not changed: could not save TUI posture ({err})"),
                StatusToastLevel::Warning,
                Some(8_000),
            );
            self.needs_redraw = true;
            return false;
        }
        self.set_agent_approval_posture(next);
        self.needs_redraw = true;
        // Footer permission chip is canonical — no status toast for the new
        // value, only the one-shot rebinding notice.
        self.notify_keybinding_migration_once();
        true
    }

    /// Replace the complete durable Act baseline and project it onto the live
    /// runtime when the current mode uses that baseline. Keeping these three
    /// fields together prevents setup presets from updating a live mirror while
    /// leaving the next Plan → Act transition stale.
    pub fn set_agent_runtime_baseline(
        &mut self,
        allow_shell: bool,
        trust_mode: bool,
        approval_mode: ApprovalMode,
    ) {
        self.mode_prefs = ModeSessionPrefs {
            agent_allow_shell: allow_shell,
            agent_trust_mode: trust_mode,
            agent_approval_mode: approval_mode,
        };
        if self.mode.uses_agent_baseline() {
            let policy = base_policy_for_mode(self.mode, &self.mode_prefs);
            self.allow_shell = policy.allow_shell;
            self.trust_mode = policy.trust_mode;
            self.approval_mode = policy.approval_mode;
            self.yolo = matches!(policy.approval_mode, ApprovalMode::Bypass);
        }
    }

    #[must_use]
    pub(crate) fn agent_trust_baseline(&self) -> bool {
        self.mode_prefs.agent_trust_mode
    }

    /// Update the durable Act shell choice without disturbing trust or
    /// approval. The live mirror changes only while Act owns the runtime.
    pub fn set_agent_shell_access(&mut self, allow_shell: bool) {
        self.set_agent_runtime_baseline(
            allow_shell,
            self.mode_prefs.agent_trust_mode,
            self.mode_prefs.agent_approval_mode,
        );
    }

    /// Update the durable Act approval choice without changing its saved shell
    /// or trust choices. Plan remains read-only.
    pub fn set_agent_approval_posture(&mut self, next: ApprovalMode) {
        self.set_agent_runtime_baseline(
            self.mode_prefs.agent_allow_shell,
            self.mode_prefs.agent_trust_mode,
            next,
        );
    }

    #[must_use]
    pub fn approval_policy_locked(&self) -> bool {
        self.approval_policy_locked
    }

    #[cfg(test)]
    #[must_use]
    pub fn approval_policy_requirements_managed(&self) -> bool {
        self.approval_policy_requirements_managed
    }

    /// Session transitions must never detach live runtime producers. Late
    /// engine, compaction, purge, or background-task events could otherwise
    /// contaminate the replacement session after clear/load/new.
    #[must_use]
    pub fn session_transition_blocked(&self) -> bool {
        self.is_loading
            || self.runtime_turn_status.as_deref() == Some("in_progress")
            || self.is_compacting
            || self.is_purging
    }

    /// Whether the interface is asking the user to make a decision. Ambient
    /// motion yields across the whole frame while this is true; freezing one
    /// task marker still leaves distracting movement in peripheral vision.
    #[must_use]
    pub fn attention_hold_active(&self) -> bool {
        !self.view_stack.is_empty()
    }

    pub fn mark_approval_policy_locked(&mut self) {
        self.approval_policy_locked = true;
    }

    pub fn clear_saved_approval_policy_lock(&mut self) {
        if !self.approval_policy_requirements_managed {
            self.approval_policy_locked = false;
        }
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
        self.history_version = self.history_version.wrapping_add(1);

        // Bound history length: when the soft cap fires, fold the oldest
        // batch into a single ArchivedContext placeholder.
        self.maybe_fold_history();
        if self.viewport.transcript_scroll.is_at_tail() && !self.user_scrolled_during_stream {
            self.scroll_to_bottom();
        }
    }

    /// Add `delta` to the parent-turn session cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    #[allow(dead_code)]
    pub fn accrue_session_cost(&mut self, delta: f64) {
        self.accrue_session_cost_estimate(CostEstimate::usd_only(delta));
    }

    /// Add a dual-currency parent-turn cost estimate.
    pub fn accrue_session_cost_estimate(&mut self, estimate: CostEstimate) {
        self.session.session_cost += estimate.usd;
        self.session.session_cost_cny += estimate.cny;
        self.refresh_displayed_cost_high_water();
    }

    /// Add `delta` to the running sub-agent cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    #[allow(dead_code)]
    pub fn accrue_subagent_cost(&mut self, delta: f64) {
        self.accrue_subagent_cost_estimate(CostEstimate::usd_only(delta));
    }

    /// Add a dual-currency sub-agent/background cost estimate.
    pub fn accrue_subagent_cost_estimate(&mut self, estimate: CostEstimate) {
        self.session.subagent_cost += estimate.usd;
        self.session.subagent_cost_cny += estimate.cny;
        self.refresh_displayed_cost_high_water();
    }

    /// Recompute the displayed cost high-water mark. Called any time a cost
    /// counter is mutated; never decreases.
    pub fn refresh_displayed_cost_high_water(&mut self) {
        let current = self.session.session_cost + self.session.subagent_cost;
        if current > self.session.displayed_cost_high_water {
            self.session.displayed_cost_high_water = current;
        }
        let current_cny = self.session.session_cost_cny + self.session.subagent_cost_cny;
        if current_cny > self.session.displayed_cost_high_water_cny {
            self.session.displayed_cost_high_water_cny = current_cny;
        }
    }

    /// Read the visible session+sub-agent cost. Guaranteed monotonic across
    /// reconciliation events (cache adjustments, provisional → final swaps)
    /// for the lifetime of one session (#244).
    #[allow(dead_code)]
    pub fn displayed_session_cost(&self) -> f64 {
        self.displayed_session_cost_for_currency(CostCurrency::Usd)
    }

    /// Read the visible session+sub-agent cost in the chosen currency.
    pub fn displayed_session_cost_for_currency(&self, currency: CostCurrency) -> f64 {
        match self.cost_display_currency(currency) {
            CostCurrency::Usd => {
                let current = self.session.session_cost + self.session.subagent_cost;
                current.max(self.session.displayed_cost_high_water)
            }
            CostCurrency::Cny => {
                let current = self.session.session_cost_cny + self.session.subagent_cost_cny;
                current.max(self.session.displayed_cost_high_water_cny)
            }
        }
    }

    pub fn session_cost_for_currency(&self, currency: CostCurrency) -> f64 {
        match self.cost_display_currency(currency) {
            CostCurrency::Usd => self.session.session_cost,
            CostCurrency::Cny => self.session.session_cost_cny,
        }
    }

    pub fn subagent_cost_for_currency(&self, currency: CostCurrency) -> f64 {
        match self.cost_display_currency(currency) {
            CostCurrency::Usd => self.session.subagent_cost,
            CostCurrency::Cny => self.session.subagent_cost_cny,
        }
    }

    pub fn format_cost_amount(&self, amount: f64) -> String {
        crate::pricing::format_cost_amount(amount, self.cost_display_currency(self.cost_currency))
    }

    pub fn format_cost_amount_precise(&self, amount: f64) -> String {
        crate::pricing::format_cost_amount_precise(
            amount,
            self.cost_display_currency(self.cost_currency),
        )
    }

    pub(crate) fn cost_display_currency(&self, currency: CostCurrency) -> CostCurrency {
        if currency == CostCurrency::Cny
            && self.session.session_cost_cny == 0.0
            && self.session.subagent_cost_cny == 0.0
            && self.session.displayed_cost_high_water_cny == 0.0
            && (self.session.session_cost > 0.0
                || self.session.subagent_cost > 0.0
                || self.session.displayed_cost_high_water > 0.0)
        {
            CostCurrency::Usd
        } else {
            currency
        }
    }

    /// Estimated cost saved by the last turn's cache-hit tokens in the
    /// configured display currency.  Returns `None` when the model's pricing
    /// is unknown or there were no cache hits.
    pub fn last_turn_cache_savings(&self) -> Option<f64> {
        let hit_tokens = self.session.last_prompt_cache_hit_tokens?;
        let estimate = crate::pricing::calculate_cache_savings_for_provider(
            self.api_provider,
            &self.model,
            hit_tokens,
        )?;
        Some(match self.cost_currency {
            crate::pricing::CostCurrency::Usd => estimate.usd,
            crate::pricing::CostCurrency::Cny if estimate.cny == 0.0 && estimate.usd > 0.0 => {
                estimate.usd
            }
            crate::pricing::CostCurrency::Cny => estimate.cny,
        })
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
        self.history_version = self.history_version.wrapping_add(1);
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
        self.expanded_tool_runs = std::mem::take(&mut self.expanded_tool_runs)
            .into_iter()
            .filter_map(|idx| if idx >= n { Some(idx - n) } else { None })
            .collect();
        self.collapsed_cell_map.clear();
    }

    pub fn mark_history_updated(&mut self) {
        self.history_version = self.history_version.wrapping_add(1);
        // Resync per-cell revisions to history.len(). This is the
        // "I-don't-know-which-cell-changed" path: if cells were appended in
        // bulk (e.g. session resume, compaction), every new cell gets a
        // fresh revision; if cells were removed, drop trailing revs. We
        // intentionally do NOT bump revisions for indices that already had
        // one — the cache will reuse those. Callers that mutate a specific
        // cell's content must call `bump_history_cell(idx)` instead.
        self.resync_history_revisions();
        self.needs_redraw = true;
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
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Append a single history cell, allocating a fresh per-cell revision.
    /// Equivalent to `add_message` but exposed as a generic alias so call
    /// sites currently doing `app.history.push(...)` followed by
    /// `app.mark_history_updated()` can collapse to one helper.
    pub fn push_history_cell(&mut self, cell: HistoryCell) {
        let rev = self.fresh_history_revision();
        self.history.push(cell);
        self.history_revisions.push(rev);
        self.history_version = self.history_version.wrapping_add(1);
        self.maybe_fold_history();
        self.needs_redraw = true;
    }

    /// Append a batch of history cells, allocating fresh revisions.
    pub fn extend_history<I>(&mut self, cells: I)
    where
        I: IntoIterator<Item = HistoryCell>,
    {
        for cell in cells {
            let rev = self.fresh_history_revision();
            self.history.push(cell);
            self.history_revisions.push(rev);
        }
        self.maybe_fold_history();
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Clear the ephemeral display projection and its side indexes.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_revisions.clear();
        self.collapsed_cells.clear();
        self.expanded_tool_runs.clear();
        self.collapsed_cell_map.clear();
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Pop the trailing history cell, keeping revisions in sync.
    pub fn pop_history(&mut self) -> Option<HistoryCell> {
        let cell = self.history.pop();
        if cell.is_some() {
            self.history_revisions.pop();
            self.expanded_tool_runs
                .retain(|idx| *idx < self.history.len());
            self.history_version = self.history_version.wrapping_add(1);
            self.needs_redraw = true;
        }
        cell
    }

    #[must_use]
    pub fn tool_collapse_active(&self) -> bool {
        self.tool_collapse_threshold > 0 && self.tool_collapse_mode.is_active(self.calm_mode)
    }

    #[must_use]
    pub fn tool_run_start_for_history_index(&self, index: usize) -> Option<usize> {
        if !self.tool_collapse_active() {
            return None;
        }
        if index >= self.history.len() {
            return None;
        }
        crate::tui::history::detect_tool_runs(&self.history, self.tool_collapse_threshold)
            .into_iter()
            .find(|run| index >= run.start && index < run.start.saturating_add(run.count))
            .map(|run| run.start)
    }

    pub fn toggle_tool_run_expansion_at(&mut self, index: usize) -> bool {
        let Some(start) = self.tool_run_start_for_history_index(index) else {
            return false;
        };
        if self.expanded_tool_runs.remove(&start) {
            self.status_message = Some("Tool group collapsed".to_string());
        } else {
            self.expanded_tool_runs.insert(start);
            self.status_message = Some("Tool group expanded".to_string());
        }
        self.mark_history_updated();
        true
    }

    #[must_use]
    pub fn original_cell_index_for_rendered(&self, rendered_index: usize) -> usize {
        self.collapsed_cell_map
            .get(rendered_index)
            .copied()
            .unwrap_or(rendered_index)
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

    /// How long the "press Ctrl+C again to quit" prompt stays armed before it
    /// silently expires.
    pub const QUIT_CONFIRMATION_WINDOW: Duration = Duration::from_secs(2);

    /// Arm the quit confirmation timer. The next Ctrl+C within
    /// [`Self::QUIT_CONFIRMATION_WINDOW`] should exit the app cleanly. Call this only
    /// from idle state — while a turn is in flight or a modal is open Ctrl+C
    /// retains its existing "interrupt this turn" / "close modal" semantics.
    pub fn arm_quit(&mut self) {
        self.quit_armed_until = Some(Instant::now() + Self::QUIT_CONFIRMATION_WINDOW);
        self.needs_redraw = true;
    }

    /// Whether the quit timer is currently armed (i.e. a prior Ctrl+C set it
    /// and it hasn't expired yet).
    pub fn quit_is_armed(&self) -> bool {
        self.quit_armed_until
            .map(|deadline| Instant::now() < deadline)
            .unwrap_or(false)
    }

    /// Clear the quit-armed timer. Call when expiry is detected on a tick or
    /// when the user takes any other action that should disarm the prompt
    /// (typing, sending a message, etc.).
    pub fn disarm_quit(&mut self) {
        if self.quit_armed_until.is_some() {
            self.quit_armed_until = None;
            self.needs_redraw = true;
        }
    }

    /// Tick called from the redraw loop. Lets time-based UI state (the
    /// quit-armed prompt) expire even when no input event is delivered.
    pub fn tick_quit_armed(&mut self) {
        if let Some(deadline) = self.quit_armed_until
            && Instant::now() >= deadline
        {
            self.quit_armed_until = None;
            self.needs_redraw = true;
        }
    }

    pub const RECEIPT_VISIBLE_DURATION: Duration = Duration::from_secs(8);

    pub fn set_receipt_text(&mut self, text: impl Into<String>) {
        self.receipt_text = Some(text.into());
        self.receipt_started_at = Some(Instant::now());
        self.needs_redraw = true;
    }

    pub fn clear_receipt(&mut self) {
        if self.receipt_text.is_some() || self.receipt_started_at.is_some() {
            self.receipt_text = None;
            self.receipt_started_at = None;
            self.needs_redraw = true;
        }
    }

    pub fn active_receipt_text(&self) -> Option<&str> {
        let receipt = self.receipt_text.as_deref()?;
        let started = self.receipt_started_at?;
        (started.elapsed() <= Self::RECEIPT_VISIBLE_DURATION).then_some(receipt)
    }

    /// Tick called from the redraw loop so transient receipts leave the UI
    /// without waiting for the next keypress.
    pub fn tick_receipt(&mut self) {
        if self
            .receipt_started_at
            .is_some_and(|started| started.elapsed() > Self::RECEIPT_VISIBLE_DURATION)
        {
            self.clear_receipt();
        }
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

    pub fn set_sidebar_focus(&mut self, focus: SidebarFocus) {
        if self.sidebar_focus != focus {
            self.sidebar_focus = focus;
            self.sidebar_focus_dirty = true;
        }
        self.needs_redraw = true;
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

    /// Up to `limit` currently-active toasts, most recent last (so a stacked
    /// renderer iterating top-to-bottom shows the freshest message at the
    /// bottom, like a chat log). Drains expired toasts off the front as a
    /// side effect — same cleanup as `active_status_toast` so callers see a
    /// consistent queue. Whalescale#439.
    pub fn active_status_toasts(&mut self, limit: usize) -> Vec<StatusToast> {
        self.sync_status_message_to_toasts();
        let now = Instant::now();
        while self
            .status_toasts
            .front()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.status_toasts.pop_front();
            self.needs_redraw = true;
        }
        if self
            .sticky_status
            .as_ref()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.sticky_status = None;
            self.needs_redraw = true;
        }

        let mut out: Vec<StatusToast> = Vec::with_capacity(limit);
        if let Some(sticky) = self.sticky_status.clone() {
            out.push(sticky);
        }
        let take = limit.saturating_sub(out.len());
        let queued: Vec<StatusToast> = self
            .status_toasts
            .iter()
            .rev()
            .take(take)
            .cloned()
            .collect();
        // Iterate in queue order (oldest of the visible window first) so the
        // stacked renderer feels chronological — most recent at the bottom.
        for toast in queued.into_iter().rev() {
            out.push(toast);
        }
        out
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

    /// Handle terminal resize event.
    pub fn handle_resize(&mut self, _width: u16, _height: u16) {
        let preserved_scroll = (!self.viewport.transcript_scroll.is_at_tail())
            .then_some(self.viewport.last_transcript_top);
        self.viewport.transcript_cache = TranscriptViewCache::new();

        if let Some(top) = preserved_scroll {
            self.viewport.transcript_scroll = TranscriptScroll::at_line(top);
        }

        self.viewport.pending_scroll_delta = 0;

        self.viewport.last_transcript_area = None;
        self.viewport.last_transcript_top = 0;
        // Seed visible height from the resize event so paging keys use a
        // useful page size immediately, before the next render updates it.
        self.viewport.last_transcript_visible = (_height as usize).saturating_sub(2).max(1);
        self.viewport.last_transcript_total = 0;
        self.viewport.last_transcript_padding_top = 0;
        self.viewport.jump_to_latest_button_area = None;

        self.mark_history_updated();
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
        self.delete_selection();
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
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        self.delete_selection();
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
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        if self.delete_selection() {
            return;
        }
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
        self.clear_input_history_navigation();
        self.auto_expand_oversized_paste();
        if self.delete_selection() {
            return;
        }
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
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
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

    /// Delete from the cursor to the start of the line.
    pub fn delete_to_start_of_line(&mut self) {
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
        if self.cursor_position == 0 {
            return;
        }

        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        // Find the start of the current line (last newline or start of string)
        let line_start = self.input[..cursor_byte]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);

        if line_start < cursor_byte {
            self.input.replace_range(line_start..cursor_byte, "");
            self.cursor_position = char_count(&self.input[..line_start]);
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    /// Delete the word after the cursor.
    pub fn delete_word_forward(&mut self) {
        self.clear_input_history_navigation();
        if self.delete_selection() {
            return;
        }
        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        if cursor_byte >= self.input.len() {
            return;
        }

        let mut word_end = cursor_byte;
        while word_end < self.input.len() {
            let Some(ch) = self.input[word_end..].chars().next() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            word_end += ch.len_utf8();
        }

        while word_end < self.input.len() {
            let Some(ch) = self.input[word_end..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            word_end += ch.len_utf8();
        }

        if cursor_byte < word_end {
            self.input.replace_range(cursor_byte..word_end, "");
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    /// Cut from the cursor to the end of the current logical line into the
    /// kill buffer. If the cursor is already at end-of-line and a trailing
    /// newline exists, that newline is consumed so repeated invocations
    /// continue to make progress (matching emacs/codex semantics).
    ///
    /// Returns `true` when bytes were moved into the kill buffer.
    pub fn kill_to_end_of_line(&mut self) -> bool {
        self.clear_input_history_navigation();
        if let Some((start, end)) = self.selection_range() {
            let sb = byte_index_at_char(&self.input, start);
            let eb = byte_index_at_char(&self.input, end);
            self.kill_buffer = self.input[sb..eb].to_string();
            self.delete_selection();
            return true;
        }
        let total_chars = char_count(&self.input);
        let cursor = self.cursor_position.min(total_chars);
        let start_byte = byte_index_at_char(&self.input, cursor);

        // Find the byte offset of the next '\n' (relative to the whole string)
        // or the end of the buffer if no newline exists at/after the cursor.
        let eol_byte = self.input[start_byte..]
            .find('\n')
            .map(|rel| start_byte + rel)
            .unwrap_or_else(|| self.input.len());

        let end_byte = if start_byte == eol_byte {
            // Cursor is at EOL — consume the newline itself if one is there.
            if eol_byte < self.input.len() {
                eol_byte + 1
            } else {
                return false;
            }
        } else {
            eol_byte
        };

        let removed: String = self.input[start_byte..end_byte].to_string();
        if removed.is_empty() {
            return false;
        }

        self.kill_buffer = removed;
        self.input.replace_range(start_byte..end_byte, "");
        // Cursor stays at the same character index (start of removed range).
        self.cursor_position = cursor;
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    /// Insert the contents of the kill buffer at the cursor, advancing it.
    /// The kill buffer is left intact so multiple yanks duplicate the text.
    /// Returns `true` if any text was inserted.
    pub fn yank(&mut self) -> bool {
        if self.kill_buffer.is_empty() {
            return false;
        }
        self.delete_selection();
        self.clear_input_history_navigation();
        let text = self.kill_buffer.clone();
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, &text);
        self.cursor_position = cursor + char_count(&text);
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
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

    /// In a multiline composer, jump to the start of the current line.
    /// On single-line input this is equivalent to `move_cursor_start`.
    pub fn move_cursor_line_start(&mut self) {
        let byte_pos = byte_index_at_char(&self.input, self.cursor_position);
        let before = &self.input[..byte_pos];
        if let Some(last_nl_byte) = before.rfind('\n') {
            // Position after the '\n' (start of the current line).
            self.cursor_position = char_count(&self.input[..=last_nl_byte]);
        } else {
            self.cursor_position = 0;
        }
        self.needs_redraw = true;
    }

    /// In a multiline composer, jump to the end of the current line
    /// (just before the next `\n` or at the end of input).
    /// On single-line input this is equivalent to `move_cursor_end`.
    pub fn move_cursor_line_end(&mut self) {
        let search_start = byte_index_at_char(&self.input, self.cursor_position);
        if let Some(offset) = self.input[search_start..].find('\n') {
            self.cursor_position = char_count(&self.input[..search_start + offset]);
        } else {
            self.cursor_position = char_count(&self.input);
        }
        self.needs_redraw = true;
    }

    /// Move forward one word. Skips over the current word then any trailing
    /// whitespace to land on the first character of the next word.
    pub fn move_cursor_word_forward(&mut self) {
        let text = self.input.clone();
        let total = char_count(&text);
        let mut pos = self.cursor_position;
        if pos >= total {
            return;
        }
        // Skip non-whitespace (current word).
        while pos < total {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            pos += 1;
        }
        // Skip whitespace.
        while pos < total {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            pos += 1;
        }
        self.cursor_position = pos;
        self.needs_redraw = true;
    }

    /// Move backward one word. Skips leading whitespace then the preceding
    /// word to land on its first character.
    pub fn move_cursor_word_backward(&mut self) {
        let text = self.input.clone();
        let mut pos = self.cursor_position;
        if pos == 0 {
            return;
        }
        // Step back one so we're not already at the word start.
        pos -= 1;
        // Skip whitespace.
        while pos > 0 {
            let byte = byte_index_at_char(&text, pos);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            pos -= 1;
        }
        // Skip non-whitespace.
        while pos > 0 {
            let byte = byte_index_at_char(&text, pos - 1);
            let ch = text[byte..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            pos -= 1;
        }
        self.cursor_position = pos;
        self.needs_redraw = true;
    }

    // === Selection helpers ===

    /// Return the (start, end) of the active selection, or `None`.
    /// `start` is inclusive, `end` is exclusive; both are char indices.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let total = char_count(&self.input);
        let anchor = self.selection_anchor?.min(total);
        let cursor = self.cursor_position.min(total);
        if anchor == cursor {
            return None;
        }
        Some(if anchor < cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        })
    }

    /// Return the selected text, or empty string if no selection.
    pub fn selected_text(&self) -> String {
        self.selection_range()
            .map(|(s, e)| {
                let sb = byte_index_at_char(&self.input, s);
                let eb = byte_index_at_char(&self.input, e);
                self.input[sb..eb].to_string()
            })
            .unwrap_or_default()
    }

    /// Delete the selected text, place cursor at the start of the deleted range.
    /// Returns true if a selection was deleted.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let sb = byte_index_at_char(&self.input, start);
        let eb = byte_index_at_char(&self.input, end);
        self.input.replace_range(sb..eb, "");
        self.cursor_position = start;
        self.selection_anchor = None;
        self.clear_input_history_navigation();
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    /// Clear the selection without moving the cursor.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    pub fn clear_input(&mut self) {
        self.clear_input_history_navigation();
        self.input.clear();
        self.cursor_position = 0;
        // Prevent stale oversized-paste state from leaking when the user
        // clears the composer or navigates to a different input (#3263).
        self.pending_paste_reference = None;
        self.oversized_paste_full_text = None;
        self.selection_anchor = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.needs_redraw = true;
    }

    pub fn clear_input_recoverable(&mut self) {
        self.stash_current_input_for_recovery();
        self.clear_input();
    }

    pub fn stash_current_input_for_recovery(&mut self) {
        // Before stashing, expand any truncated paste so the saved draft
        // contains the full text, not the truncated preview (#3263).
        self.auto_expand_oversized_paste();
        let draft = self.input.clone();
        if draft.trim().is_empty() {
            self.clear_undo_buffer = None;
            return;
        }
        self.clear_undo_buffer = Some(draft.clone());
        self.remember_draft_for_recovery(draft);
    }

    fn remember_draft_for_recovery(&mut self, draft: String) {
        if draft.trim().is_empty() {
            return;
        }
        self.draft_history.retain(|existing| existing != &draft);
        self.draft_history.push_back(draft);
        while self.draft_history.len() > MAX_DRAFT_HISTORY {
            let _ = self.draft_history.pop_front();
        }
    }

    pub fn start_history_search(&mut self) {
        if self.composer_history_search.is_some() {
            return;
        }
        // Expand any truncated paste first so the history search seed
        // contains the full text, not the truncated preview (#3263).
        self.auto_expand_oversized_paste();
        self.composer_history_search = Some(ComposerHistorySearch::new(
            self.input.clone(),
            self.cursor_position,
        ));
        self.slash_menu_hidden = true;
        self.mention_menu_hidden = true;
        self.status_message = Some("History search: type to filter, Enter accepts".to_string());
        self.needs_redraw = true;
    }

    pub fn is_history_search_active(&self) -> bool {
        self.composer_history_search.is_some()
    }

    pub fn history_search_query(&self) -> Option<&str> {
        self.composer_history_search
            .as_ref()
            .map(|search| search.query.as_str())
    }

    pub fn history_search_selected_index(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(0, |search| search.selected)
    }

    pub fn composer_display_input(&self) -> &str {
        self.history_search_query().unwrap_or(&self.input)
    }

    pub fn composer_display_cursor(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(self.cursor_position, |search| char_count(&search.query))
    }

    pub fn history_search_matches(&self) -> Vec<String> {
        let Some(query) = self.history_search_query() else {
            return Vec::new();
        };
        self.history_search_matches_for_query(query)
    }

    fn history_search_matches_for_query(&self, query: &str) -> Vec<String> {
        let normalized_query = query.trim().to_lowercase();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut matches = Vec::new();

        for candidate in self
            .draft_history
            .iter()
            .rev()
            .chain(self.input_history.iter().rev())
        {
            if candidate.trim().is_empty() || !seen.insert(candidate.as_str()) {
                continue;
            }
            if normalized_query.is_empty() || candidate.to_lowercase().contains(&normalized_query) {
                matches.push(candidate.clone());
            }
        }

        matches
    }

    fn clamp_history_search_selection(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let selected = search.selected;
        let query = search.query.clone();
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = if match_count == 0 {
                0
            } else {
                selected.min(match_count.saturating_sub(1))
            };
        }
    }

    pub fn history_search_insert_char(&mut self, ch: char) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push(ch);
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push_str(&normalize_paste_text(text));
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_backspace(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.pop();
            search.selected = 0;
            self.needs_redraw = true;
        }
        self.clamp_history_search_selection();
    }

    pub fn history_search_select_previous(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = search.selected.saturating_sub(1);
            self.needs_redraw = true;
        }
    }

    pub fn history_search_select_next(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let query = search.query.clone();
        let selected = search.selected;
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut()
            && match_count > 0
        {
            search.selected = (selected + 1).min(match_count.saturating_sub(1));
            self.needs_redraw = true;
        }
    }

    pub fn accept_history_search(&mut self) -> bool {
        let Some(search) = self.composer_history_search.take() else {
            return false;
        };
        let matches = self.history_search_matches_for_query(&search.query);
        if let Some(selected) = matches
            .get(search.selected.min(matches.len().saturating_sub(1)))
            .cloned()
        {
            self.input = selected;
            self.cursor_position = char_count(&self.input);
            self.history_index = None;
            self.status_message = Some("History match inserted into composer".to_string());
            self.needs_redraw = true;
            true
        } else {
            self.composer_history_search = Some(search);
            self.status_message = Some("No history matches".to_string());
            self.needs_redraw = true;
            false
        }
    }

    pub fn cancel_history_search(&mut self) {
        let Some(search) = self.composer_history_search.take() else {
            return;
        };
        self.input = search.pre_search_input;
        self.cursor_position = search.pre_search_cursor.min(char_count(&self.input));
        self.status_message = Some("History search canceled".to_string());
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
        if !super::canonical_commands::looks_like_command_input(&input) {
            self.input_history.push(input.clone());
            if self.max_input_history == 0 {
                self.input_history.clear();
            } else if self.input_history.len() > self.max_input_history {
                let excess = self.input_history.len() - self.max_input_history;
                self.input_history.drain(0..excess);
            }
            // Mirror to the persisted cross-session history (#366) so
            // arrow-up recall works across restarts. Best-effort write —
            // see `composer_history::append_history` for failure modes.
            crate::composer_history::append_history(&input);
        }
        self.history_index = None;
        self.history_navigation_draft = None;
        self.clear_input();
        Some(input)
    }

    pub fn restore_last_submitted_prompt_if_empty(&mut self) -> bool {
        if !self.input.is_empty() {
            return false;
        }
        let Some(prompt) = self
            .last_submitted_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
        else {
            return false;
        };

        self.input = prompt.to_string();
        self.cursor_position = char_count(&self.input);
        self.history_index = None;
        self.history_navigation_draft = None;
        self.needs_redraw = true;
        true
    }

    /// Restore the last cleared input if the composer is empty.
    /// Returns `true` if the input was restored.
    pub fn restore_last_cleared_input_if_empty(&mut self) -> bool {
        if !self.input.is_empty() {
            return false;
        }
        let Some(saved) = self.clear_undo_buffer.take().filter(|s| !s.is_empty()) else {
            return false;
        };

        self.input = saved;
        self.cursor_position = char_count(&self.input);
        self.history_index = None;
        self.history_navigation_draft = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.needs_redraw = true;
        self.clear_undo_buffer = None;
        true
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
                format!("Failed to create paste directory: {e}"),
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
                format!("Failed to write paste file: {e}"),
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

    pub fn queue_message(&mut self, message: QueuedMessage) {
        self.queued_messages.push_back(message);
    }

    pub fn pop_queued_message(&mut self) -> Option<QueuedMessage> {
        self.queued_messages.pop_front()
    }

    pub fn remove_queued_message(&mut self, index: usize) -> Option<QueuedMessage> {
        self.queued_messages.remove(index)
    }

    pub fn queued_message_count(&self) -> usize {
        self.queued_messages.len()
    }

    /// Pop the most-recently queued message back into the composer for editing
    /// (issue #85 — ↑ affordance). The popped message is parked in
    /// [`Self::queued_draft`] so the next Enter re-queues it carrying its
    /// original skill instruction. No-op if the composer already has typed
    /// content or a draft is already being edited — surfacing the affordance
    /// would be ambiguous in either case.
    ///
    /// Returns `true` when the composer state was mutated.
    pub fn pop_last_queued_into_draft(&mut self) -> bool {
        if !self.input.is_empty() || self.queued_draft.is_some() {
            return false;
        }
        let Some(msg) = self.queued_messages.pop_back() else {
            return false;
        };
        self.input = msg.display.clone();
        self.cursor_position = char_count(&self.input);
        self.queued_draft = Some(msg);
        self.needs_redraw = true;
        true
    }

    /// Stop editing a queued follow-up and put the original queued message back
    /// at the tail where [`Self::pop_last_queued_into_draft`] took it from.
    pub fn cancel_queued_draft_edit(&mut self) -> bool {
        let Some(draft) = self.queued_draft.take() else {
            return false;
        };
        self.queued_messages.push_back(draft);
        self.clear_input_recoverable();
        self.needs_redraw = true;
        true
    }

    /// Park a legacy pending steer. New keyboard handling routes running-turn
    /// drafts through Enter (same-turn steer) or Tab (next-turn follow-up).
    #[allow(dead_code)]
    pub fn push_pending_steer(&mut self, message: QueuedMessage) {
        self.pending_steers.push_back(message);
        self.submit_pending_steers_after_interrupt = true;
        self.needs_redraw = true;
    }

    /// Drain the pending-steer queue and clear the resend flag. Returns the
    /// messages in submit order (oldest first).
    pub fn drain_pending_steers(&mut self) -> Vec<QueuedMessage> {
        self.submit_pending_steers_after_interrupt = false;
        if self.pending_steers.is_empty() {
            return Vec::new();
        }
        self.needs_redraw = true;
        self.pending_steers.drain(..).collect()
    }

    /// Decide how to route a fresh composer submit.
    ///
    /// v0.8.68: streaming output queues. Busy-but-waiting turns steer so
    /// Enter can amend the active turn before output starts. A double-tap
    /// Enter within 500 ms triggers Steer while streaming; Ctrl+Enter forces
    /// Steer in all busy states.
    ///
    /// Truth table:
    ///   offline=F, busy=F → Immediate
    ///   offline=F, busy=T, streaming=F → Steer
    ///   offline=F, busy=T, streaming=T → Queue (double-tap → Steer)
    ///   offline=T, busy=* → Queue
    #[must_use]
    pub fn decide_submit_disposition(&self) -> SubmitDisposition {
        if self.offline_mode {
            return SubmitDisposition::Queue;
        }
        if !self.is_loading {
            return SubmitDisposition::Immediate;
        }
        if self.streaming_message_index.is_none() {
            return SubmitDisposition::Steer;
        }
        // Streaming: queue the message. Double-tap Enter within 500 ms
        // triggers Steer via enter_with_double_tap(); see the ui.rs submit
        // handler.
        SubmitDisposition::Queue
    }

    /// Process an Enter keypress with double-tap steering detection.
    ///
    /// When the engine is busy, the first Enter queues the message. A second
    /// Enter within 500 ms triggers Steer (interrupt the current turn to
    /// inject the new instruction immediately). When idle, Enter submits
    /// immediately.
    #[must_use]
    pub fn enter_with_double_tap(&mut self) -> Option<SubmitDisposition> {
        let disposition = self.decide_submit_disposition();
        match disposition {
            SubmitDisposition::Queue => {
                if let Some(instant) = self.last_enter_instant
                    && instant.elapsed() < Duration::from_millis(500)
                {
                    self.last_enter_instant = None;
                    return Some(SubmitDisposition::Steer);
                }
                self.last_enter_instant = Some(Instant::now());
                Some(SubmitDisposition::Queue)
            }
            other => {
                self.last_enter_instant = None;
                Some(other)
            }
        }
    }

    /// Mark the in-flight streaming Assistant cell as interrupted: prepend
    /// `[interrupted]` to whatever streamed so far (so the user can see what
    /// was salvaged) and flip `streaming` off so the spinner halts. No-op if
    /// no Assistant cell is currently streaming.
    ///
    /// Deliberate divergence from openai/codex which discards partial output
    /// on abort — V4 thinking is expensive and the user usually wants to see
    /// what the model produced before steering.
    pub fn finalize_streaming_assistant_as_interrupted(&mut self) {
        let Some(index) = self.streaming_message_index.take() else {
            return;
        };
        if let Some(HistoryCell::Assistant { content, streaming }) = self.history.get_mut(index) {
            *streaming = false;
            if content.is_empty() {
                *content = "[interrupted]".to_string();
            } else if !content.starts_with("[interrupted]") {
                content.insert_str(0, "[interrupted] ");
            }
        }
        self.bump_history_cell(index);
    }

    pub fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        if self.history_index.is_none() {
            // Expand truncated paste first so the saved draft contains the
            // full text instead of the truncated preview (#3263).
            self.auto_expand_oversized_paste();
            self.history_navigation_draft = Some(InputHistoryDraft {
                input: self.input.clone(),
                cursor: self.cursor_position,
            });
        }
        let new_index = match self.history_index {
            None => self.input_history.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.history_index = Some(new_index);
        self.input = self.input_history[new_index].clone();
        self.cursor_position = char_count(&self.input);
        self.selection_anchor = None;
        self.slash_menu_hidden = false;
    }

    pub fn history_down(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {}
            Some(i) => {
                if i + 1 < self.input_history.len() {
                    self.history_index = Some(i + 1);
                    self.input = self.input_history[i + 1].clone();
                    self.cursor_position = char_count(&self.input);
                    self.selection_anchor = None;
                    self.slash_menu_hidden = false;
                } else {
                    self.history_index = None;
                    if let Some(draft) = self.history_navigation_draft.take() {
                        self.input = draft.input;
                        self.cursor_position = draft.cursor.min(char_count(&self.input));
                        self.selection_anchor = None;
                        self.slash_menu_hidden = false;
                        self.needs_redraw = true;
                    } else {
                        self.clear_input();
                    }
                }
            }
        }
    }

    fn clear_input_history_navigation(&mut self) {
        self.history_index = None;
        self.history_navigation_draft = None;
    }

    pub fn effective_model_for_budget(&self) -> &str {
        if self.auto_model && self.model.eq_ignore_ascii_case("auto") {
            return DEFAULT_TEXT_MODEL;
        }
        &self.model
    }

    pub fn model_display_label(&self) -> String {
        if self.auto_model {
            if !self.model.eq_ignore_ascii_case("auto") {
                return format!("auto: {}", self.model);
            }
            return "auto".to_string();
        }
        self.model.clone()
    }

    pub fn reasoning_effort_display_label(&self) -> String {
        if self.auto_model {
            if self.reasoning_effort != ReasoningEffort::Auto {
                return format!(
                    "auto: {}",
                    self.reasoning_effort
                        .display_label_for_provider(self.api_provider)
                );
            }
            return "auto".to_string();
        }
        if self.reasoning_effort == ReasoningEffort::Auto {
            return "auto".to_string();
        }
        self.reasoning_effort
            .display_label_for_provider(self.api_provider)
            .to_string()
    }
}

#[cfg(test)]
mod tests;
