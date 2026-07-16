//! System prompts for different modes.
//!
//! Prompts are assembled from composable layers loaded at compile time:
//!   constitution.md + personality overlay → message[0] (byte-stable).
//!   mode delta + tool taxonomy + approval policy → request-time runtime metadata.
//!
//! This keeps each concern in its own file and makes prompt tuning
//! a single-file operation.

use crate::project_context::{ProjectContext, load_project_context_with_parents};
use codewhale_config::PromptPreferences;
use codewhale_protocol::agent_runtime::{
    PromptCacheControl, SystemPrompt, SystemPromptBlock as SystemBlock,
};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone)]
pub struct PromptSessionContext<'a> {
    pub user_memory_block: Option<&'a str>,
    pub goal_objective: Option<&'a str>,
    pub project_context_pack_enabled: bool,
    /// Resolved BCP-47 locale tag for the `## Environment` block in
    /// the system prompt (e.g. `"en"`, `"zh-Hans"`, `"ja"`). The
    /// caller is responsible for resolving this from `Settings`; no
    /// disk I/O happens inside the prompt builder, so the workspace-
    /// static portion of the system prompt stays cache-friendly.
    pub locale_tag: &'a str,
    /// When true, a ## Language Output Requirement block is appended
    /// to the system prompt instructing the model to respond in
    /// the resolved session locale.
    pub translation_enabled: bool,
    /// Active model identifier. The bundled constitution is model-agnostic,
    /// but embedders may still provide a prompt override containing
    /// `{model_id}`. Defaults to `"codewhale"` when the caller doesn't supply one.
    pub model_id: &'a str,
    /// Route-effective context window, when known. Prompt composition no
    /// longer prints context-window facts, but the field remains part of the
    /// session context contract for embedders and future runtime metadata.
    pub context_window_override: Option<u32>,
    /// Whether the user-visible transcript renders thinking blocks.
    /// When false, the prompt should not spend localization pressure on
    /// `reasoning_content` the user will never see.
    pub show_thinking: bool,
    /// Optional output-verbosity mode. `concise` appends a short output
    /// discipline block; unset keeps the normal conversational prompt.
    pub verbosity: Option<&'a str>,
    /// Restrict skill discovery to CodeWhale-owned roots plus explicit
    /// `skills_dir` configuration.
    pub skills_scan_codewhale_only: bool,
    /// Caller-resolved shell binary. Host detection stays outside prompt
    /// composition so this crate has no presentation-process singleton.
    pub shell_binary: &'a str,
}

impl Default for PromptSessionContext<'_> {
    fn default() -> Self {
        Self {
            user_memory_block: None,
            goal_objective: None,
            project_context_pack_enabled: true,
            locale_tag: "en",
            translation_enabled: false,
            model_id: "codewhale",
            context_window_override: None,
            show_thinking: true,
            verbosity: None,
            skills_scan_codewhale_only: false,
            shell_binary: "sh",
        }
    }
}

/// Complete input for the canonical production system prompt.
#[derive(Debug)]
pub struct ProductionPromptRequest<'a> {
    pub workspace: &'a Path,
    pub model: &'a str,
    pub preferences: &'a PromptPreferences,
    pub instructions: &'a [InstructionSource],
    pub skills_dir: Option<&'a Path>,
    pub project_context_pack_enabled: bool,
    pub verbosity: Option<&'a str>,
    pub skills_scan_codewhale_only: bool,
    pub shell_binary: &'a str,
    pub tool_mode: bool,
}

/// Build the prompt shared by root and child runtimes, including the final
/// request-volatile execution-posture block.
#[must_use]
pub fn production_system_prompt(request: ProductionPromptRequest<'_>) -> SystemPrompt {
    let locale = request.preferences.resolved_locale();
    let mut prompt = system_prompt_for_mode_with_context_skills_and_session(
        request.workspace,
        None,
        request.skills_dir,
        Some(request.instructions),
        PromptSessionContext {
            user_memory_block: None,
            goal_objective: None,
            project_context_pack_enabled: request.project_context_pack_enabled,
            locale_tag: locale.tag(),
            translation_enabled: false,
            model_id: request.model,
            context_window_override: None,
            show_thinking: request.preferences.show_thinking,
            verbosity: request.verbosity,
            skills_scan_codewhale_only: request.skills_scan_codewhale_only,
            shell_binary: request.shell_binary,
        },
    );
    prompt.blocks.push(SystemBlock {
        text: if request.tool_mode {
            "你正在唯一 AgentRuntime 中执行编码任务。只使用本次请求实际提供的工具；先读取再修改，修改后运行最相关验证。`agent` 会启动同一 Runtime 的后台子 Agent，运行时会自动等待并把结构化结果回注；不要轮询或调用不存在的等待工具。".to_owned()
        } else {
            "本次是无工具执行。直接给出准确、简洁、可操作的最终答案，不要声称执行了文件或命令操作。".to_owned()
        },
        cache_control: PromptCacheControl::Volatile,
    });
    prompt
}

/// Conventional location for the structured session relay artifact (#32).
/// A previous session writes it on exit / `/compact`; the next session reads
/// it back on startup and prepends it to the system prompt so a fresh agent
/// doesn't have to re-discover open blockers from scratch.
pub const HANDOFF_RELATIVE_PATH: &str = ".codewhale/handoff.md";
/// Legacy handoff path for reading from existing installs.
const LEGACY_HANDOFF_RELATIVE_PATH: &str = ".deepseek/handoff.md";

/// Per-file size cap for `instructions = [...]` entries (#454). Mirrors
/// the existing project-context cap in `project_context::load_context_file`
/// so a malicious / oversized include can't blow the prompt budget on
/// its own. Files larger than this are truncated with an explicit `[…truncated: N bytes omitted]`
/// marker rather than skipped entirely so the model still sees the head.
const INSTRUCTIONS_FILE_MAX_BYTES: usize = 100 * 1024;

/// System prompt block appended when `translation_enabled` is true.
/// Instructs the model to respond in the resolved session locale for all
/// natural-language output — explanations, summaries, conversation.
/// Code identifiers, untranslatable technical terms, and explicitly
/// requested English code blocks are exempt.
fn translation_output_instruction(locale_tag: &str) -> String {
    let target_language = translation_target_language_for_tag(locale_tag);
    format!(
        "\
## Language Output Requirement\n\
\n\
The user requires all responses in {target_language}. \
Always respond in {target_language} — use natural, professional language for all \
explanations, code comments, summaries, and conversational turns. \
Only output English for:\n\
- Code identifiers (variable names, function names, file paths)\n\
- Technical terms that lack a standard translation in {target_language}\n\
- Code blocks the user explicitly requests in English\n\n\
This is a hard display requirement: the user does not read English, \
so any English prose in your response will block their decision-making."
    )
}

fn concise_output_discipline_instruction() -> &'static str {
    "\
## Concise Output Discipline

To minimize token usage and optimize speed:
- Output only direct, actionable code, technical steps, or final answers.
- Eliminate all conversational filler, fluff, introductions, transitions, or summarizing conclusions.
- Do NOT explain what you are about to do or what you have just completed.
- Do NOT provide conversational status updates before or after running tools.
- Keep explanations and comments extremely brief and technical, explaining only non-obvious reasoning."
}

fn is_concise_verbosity(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("concise"))
}

fn translation_target_language_for_tag(locale_tag: &str) -> &'static str {
    let normalized = locale_tag.trim().to_ascii_lowercase();
    if normalized.starts_with("ja") {
        "Japanese (日本語)"
    } else if normalized.starts_with("zh-hant")
        || normalized.contains("-tw")
        || normalized.contains("-hk")
        || normalized.contains("-mo")
    {
        "Traditional Chinese (繁體中文)"
    } else if normalized.starts_with("zh") {
        "Simplified Chinese (简体中文)"
    } else if normalized.starts_with("pt") {
        "Brazilian Portuguese (Português do Brasil)"
    } else if normalized.starts_with("vi") {
        "Vietnamese (Tiếng Việt)"
    } else {
        "English"
    }
}

fn hidden_thinking_language_instruction(locale_tag: &str) -> String {
    let fallback_language = translation_target_language_for_tag(locale_tag);
    format!(
        "\
## Hidden Thinking Language\n\
\n\
The user has disabled thinking display (`show_thinking = false`). If you emit \
`reasoning_content`, keep that hidden internal thinking in English regardless \
of the latest user-message language or `## Environment.lang`; the user will \
not see it, so localizing hidden thinking only adds language switching.\n\
\n\
The final reply is still user-visible. Follow the normal `## Language` rule \
for the final reply: mirror the latest user message, and use \
{fallback_language} only when the user message is ambiguous. If the user \
explicitly asks for a different thinking language, follow that explicit request \
for the current turn."
    )
}

/// Render a `## Environment` block listing the resolved locale tag,
/// runtime version, host platform, login shell, and current working directory.
///
/// The block is appended to the workspace-static portion of the
/// system prompt (after mode prompt + project context, before
/// configured instructions / skills). `locale_tag` is resolved by the caller
/// from `Settings` so this function stays I/O-free.
fn render_environment_block(_workspace: &Path, locale_tag: &str, shell: &str) -> String {
    let codewhale_version = env!("CARGO_PKG_VERSION");
    let platform = std::env::consts::OS;

    // The workspace path (`pwd`) is intentionally delivered per-turn via the
    // `<turn_meta>` block (see `turn_metadata_block`) rather than embedded here.
    //
    // Rationale: when the workspace path changes between sessions (e.g. an
    // ephemeral per-session workspace), a volatile value inside the otherwise
    // static system prefix invalidates the inference server's prefix cache at
    // that exact point. The cache then only partially matches and the tail must
    // be re-prefilled from the divergence boundary. On backends that pair prefix
    // caching with speculative decoding, this partial re-prefill can perturb the
    // logits at the boundary enough to degrade structured tool-call emission
    // (the model regresses to bare text). Keeping the static system prefix
    // byte-identical across sessions lets the prefix cache be reused; the live
    // workspace path still reaches the model every turn through `turn_meta`.
    format!(
        "## Environment\n\
         \n\
         - lang: {locale_tag}\n\
         - codewhale_version: {codewhale_version}\n\
         - platform: {platform}\n\
         - shell: {shell}"
    )
}

/// Source for an `EngineConfig.instructions` entry. Either a disk file (loaded
/// at render time, original semantics) or an inline string (content baked into
/// `EngineConfig`, no disk I/O at render time).
///
/// The inline variant is useful for embedders that compute instructions at
/// runtime (e.g. rendering a template with workspace-specific substitutions)
/// and don't want to stage the content to a disk file just to satisfy a path
/// API. Staging adds two problems the inline path avoids:
///
///   1. The disk file looks like editable config but gets overwritten on
///      every launch — confusing for users browsing the install dir.
///   2. Multi-engine setups need per-engine paths to avoid `rehydrate`
///      reading another session's instructions; with inline sources the
///      content lives in the per-engine `EngineConfig` and the race
///      surface goes away.
///
/// `From<PathBuf>` is provided so existing callers passing `Vec<PathBuf>` can
/// keep working with a `.into()` upgrade at the call site.
#[derive(Debug, Clone)]
pub enum InstructionSource {
    /// Load this file from disk at prompt-render time. Original behavior:
    /// missing files are skipped with a warning, oversized files are
    /// truncated to `INSTRUCTIONS_FILE_MAX_BYTES` with an `[…elided]`
    /// marker.
    File(PathBuf),
    /// Use the provided string directly. `name` becomes the
    /// `<instructions source="…">` attribute (typically a synthetic
    /// identifier like `embedded:my-template` or a logical path).
    Inline { name: String, content: String },
}

impl From<PathBuf> for InstructionSource {
    fn from(path: PathBuf) -> Self {
        InstructionSource::File(path)
    }
}

impl From<&PathBuf> for InstructionSource {
    fn from(path: &PathBuf) -> Self {
        InstructionSource::File(path.clone())
    }
}

/// Render the `instructions = [...]` config array as a single
/// system-prompt block (#454). Each source is processed in declared order;
/// missing `File` sources are skipped with a tracing warning so a stale entry
/// doesn't fail the launch. Empty input (or all sources missing/empty)
/// returns `None` so callers append nothing.
fn render_instructions_block(sources: &[InstructionSource]) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();
    for source in sources {
        let (raw_source_name, raw_content): (String, String) = match source {
            InstructionSource::File(path) => match std::fs::read_to_string(path) {
                Ok(raw) => (path.display().to_string(), raw),
                Err(err) => {
                    tracing::warn!(
                        target: "instructions",
                        ?err,
                        ?path,
                        "skipping unreadable instructions file"
                    );
                    continue;
                }
            },
            InstructionSource::Inline { name, content } => (name.clone(), content.clone()),
        };
        let trimmed = raw_content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body = if trimmed.len() > INSTRUCTIONS_FILE_MAX_BYTES {
            let head_end = (0..=INSTRUCTIONS_FILE_MAX_BYTES)
                .rev()
                .find(|&i| trimmed.is_char_boundary(i))
                .unwrap_or(0);
            format!(
                "{}\n[…truncated: {} of {} bytes omitted — consider splitting this instructions file]",
                &trimmed[..head_end],
                trimmed.len() - head_end,
                trimmed.len()
            )
        } else {
            trimmed.to_string()
        };
        sections.push(format!(
            "<instructions source=\"{raw_source_name}\">\n{body}\n</instructions>"
        ));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

/// Read the workspace-local relay artifact, if present, and format it as a
/// system-prompt block. Returns `None` when the file is absent or empty so
/// callers can keep the default-uncluttered prompt for fresh workspaces.
fn load_handoff_block(workspace: &Path) -> Option<String> {
    let primary = workspace.join(HANDOFF_RELATIVE_PATH);
    let path = if primary.exists() {
        primary
    } else {
        workspace.join(LEGACY_HANDOFF_RELATIVE_PATH)
    };
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(format!(
        "## Previous Session Relay\n\nThe previous session in this workspace left a relay artifact at `{HANDOFF_RELATIVE_PATH}`. Consider it the first artifact to read on this turn — open blockers, in-flight changes, and recent decisions live there. Update or rewrite it before exiting if state changes materially.\n\n{trimmed}"
    ))
}

/// Load the structured user-global constitution, if present, and render it as
/// its own model-facing block.
fn load_user_constitution_block() -> Option<String> {
    if user_constitution_disabled_by_setup_state() {
        return None;
    }

    let path = match codewhale_config::UserConstitution::path() {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                target: "prompts",
                "could not resolve user-global constitution path: {err:#}"
            );
            return None;
        }
    };

    match codewhale_config::UserConstitution::load_from(&path) {
        codewhale_config::UserConstitutionLoad::Loaded(constitution) => {
            constitution.render_block(None)
        }
        codewhale_config::UserConstitutionLoad::Missing
        | codewhale_config::UserConstitutionLoad::Empty => None,
        codewhale_config::UserConstitutionLoad::Invalid(err) => {
            tracing::warn!(
                target: "prompts",
                "skipping invalid user-global constitution {}: {err}",
                path.display()
            );
            None
        }
        codewhale_config::UserConstitutionLoad::Unreadable(err) => {
            tracing::warn!(
                target: "prompts",
                "skipping unreadable user-global constitution {}: {err}",
                path.display()
            );
            None
        }
    }
}

fn user_constitution_disabled_by_setup_state() -> bool {
    match codewhale_config::SetupState::load() {
        Ok(Some(state)) => matches!(
            state.constitution_choice,
            codewhale_config::ConstitutionChoice::Bundled
                | codewhale_config::ConstitutionChoice::Deferred
                | codewhale_config::ConstitutionChoice::ExpertOverride
        ),
        Ok(None) => false,
        Err(err) => {
            tracing::warn!(
                target: "prompts",
                "could not resolve setup-state path while loading user constitution: {err:#}"
            );
            false
        }
    }
}

// ── Prompt layers loaded at compile time ──────────────────────────────

/// Core: task execution, tool-use rules, output format, toolbox reference,
/// "When NOT to use" guidance, sub-agent sentinel protocol.
///
/// This markdown is the single hand-maintained source of the constitutional
/// system prompt. The earlier YAML + Python-renderer generation pipeline
/// (`constitution.yaml` / `render_constitution.py`) was retired because it
/// had drifted from this file since the v4 "zero ceremony" adoption and the
/// renderer could no longer reproduce it byte-for-byte. The layered runtime
/// assembly composes this core with mode / approval / skills /
/// context-management / compaction / authority-recap layers at runtime (see
/// `system_prompt_for_mode_with_context_skills_and_session`). Edit this file
/// directly; `constitution_md_carries_required_structure` guards its skeleton.
pub const BASE_PROMPT: &str = include_str!("prompts/constitution.md");
/// Language mirroring law, split from the compact constitution in 0.9.0.
pub const LANGUAGE_PROMPT: &str = include_str!("prompts/language.md");
/// Terminal-facing output formatting law, split from the compact constitution.
pub const OUTPUT_PROMPT: &str = include_str!("prompts/output.md");

// ── Embedder prompt overrides ──
// Let an embedder replace these compile-time prompt constants at startup,
// so brand / slimming customizations live in the embedder crate instead of
// editing these files in-tree. Unset → the bundled constant (fully
// backward compatible). Intended to be set once at process start, before
// any engine spawns; later sets return the rejected override string.
static BASE_PROMPT_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_PREAMBLE_ZH_HANS_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_PREAMBLE_JA_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_PREAMBLE_PT_BR_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_PREAMBLE_VI_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_CLOSER_ZH_HANS_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_CLOSER_JA_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_CLOSER_PT_BR_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_CLOSER_VI_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static AUTHORITY_RECAP_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static STATIC_PROMPT_COMPOSER: std::sync::OnceLock<Box<StaticPromptComposer>> =
    std::sync::OnceLock::new();
static PROMPT_OVERRIDE_NOTICES: LazyLock<Mutex<Vec<String>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Context passed to an embedder-provided static prompt composer.
///
/// This hook only replaces the byte-stable base/personality prompt segment.
/// Mode deltas, approval policy, tool taxonomy, Core Execution, and the
/// Compaction Relay stay owned by CodeWhale's system prompt assembly.
#[non_exhaustive]
#[derive(Debug)]
pub struct StaticPromptCtx<'a> {
    /// Active model identifier after caller-side routing.
    pub model_id: &'a str,
    /// Personality overlay requested for the base static prompt.
    pub personality: Personality,
    /// Default base/personality prompt layers that would be used without an
    /// override.
    pub default_layers: &'a str,
}

/// Embedder hook for replacing CodeWhale's byte-stable base/personality prompt
/// segment.
pub type StaticPromptComposer = dyn Fn(&StaticPromptCtx<'_>) -> String + Send + Sync + 'static;

/// Replace `BASE_PROMPT` for all subsequent prompt composition. First call
/// wins; later calls return the rejected string. Set before spawning any
/// engine.
pub fn set_base_prompt_override(s: String) -> Result<(), String> {
    set_prompt_override(&BASE_PROMPT_OVERRIDE, s)
}

/// Replace the Simplified-Chinese locale preamble (`## 语言要求`).
pub fn set_locale_preamble_zh_hans_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_PREAMBLE_ZH_HANS_OVERRIDE, s)
}

/// Replace the Japanese locale preamble.
pub fn set_locale_preamble_ja_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_PREAMBLE_JA_OVERRIDE, s)
}

/// Replace the Brazilian-Portuguese locale preamble.
pub fn set_locale_preamble_pt_br_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_PREAMBLE_PT_BR_OVERRIDE, s)
}

/// Replace the Vietnamese locale preamble.
pub fn set_locale_preamble_vi_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_PREAMBLE_VI_OVERRIDE, s)
}

/// Replace the Simplified-Chinese locale closer (`## 语言再次提醒`).
pub fn set_locale_closer_zh_hans_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_CLOSER_ZH_HANS_OVERRIDE, s)
}

/// Replace the Japanese locale closer.
pub fn set_locale_closer_ja_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_CLOSER_JA_OVERRIDE, s)
}

/// Replace the Brazilian-Portuguese locale closer.
pub fn set_locale_closer_pt_br_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_CLOSER_PT_BR_OVERRIDE, s)
}

/// Replace the Vietnamese locale closer.
pub fn set_locale_closer_vi_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_CLOSER_VI_OVERRIDE, s)
}

/// Replace the trailing `## Authority Recap` block.
pub fn set_authority_recap_override(s: String) -> Result<(), String> {
    set_prompt_override(&AUTHORITY_RECAP_OVERRIDE, s)
}

/// Replace the byte-stable base/personality prompt segment for subsequent
/// prompt composition. First call wins; later calls return the rejected
/// composer so embedders can preserve ownership.
pub fn set_static_prompt_composer_override(
    f: Box<StaticPromptComposer>,
) -> Result<(), Box<StaticPromptComposer>> {
    set_static_prompt_composer(&STATIC_PROMPT_COMPOSER, f)
}

// ── Config-directory prompt overrides (issue #3638) ──
// Bridge the embedder override hooks above to a user-facing source: an
// optional file in the CodeWhale config directory. This lets users repurpose
// the TUI for non-software use cases (e.g. long-form writing) by swapping the
// constitutional base prompt, without editing in-tree files or shipping a
// custom embedder build.
//
// Scope is deliberately narrow: only the byte-stable base prompt segment is
// user-overridable. Mode deltas, approval policy, tool taxonomy, Core
// Execution, and the Compaction Relay stay owned by the runtime assembly (see
// `StaticPromptCtx`), so an override cannot strip safety-relevant guidance.
// A missing or empty file is a no-op — the bundled constant is used — so this
// is fully backward compatible.
//
// Because replacing the base prompt is a trust-boundary action (per maintainer
// review on #3638), the override file alone is NOT sufficient: the user must
// also set an explicit opt-in flag (`CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE`).
// This keeps replacing the global Constitution a deliberate, auditable act
// rather than something a stray file can do.

/// Relative path, under the config directory, of the optional base-prompt
/// (constitution) override file.
pub const CONSTITUTION_OVERRIDE_FILE: &str = "prompts/constitution.md";

/// Env flag that must be set (`1`/`true`/`on`/`yes`) to enable config-dir base
/// prompt overrides. Required in addition to the override file so the global
/// base prompt can never be replaced by file presence alone.
pub const BASE_PROMPT_OVERRIDE_OPT_IN_ENV: &str = "CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE";

/// Whether the user has explicitly opted in to base-prompt overrides.
pub fn base_prompt_override_opt_in() -> bool {
    match std::env::var(BASE_PROMPT_OVERRIDE_OPT_IN_ENV) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        Err(_) => false,
    }
}

/// Read an optional prompt-override file rooted at `config_dir`.
///
/// Returns the file contents when it exists and is non-empty after trimming;
/// otherwise `None` so the caller falls back to the embedded default. Pure
/// over `config_dir`, so it is unit-testable without touching the global
/// override cells.
fn read_prompt_override_file(config_dir: &Path, relative: &str) -> Option<String> {
    let path = config_dir.join(relative);
    let raw = std::fs::read_to_string(&path).ok()?;
    if raw.trim().is_empty() {
        tracing::warn!(
            target: "prompts",
            "ignoring empty prompt override file {}",
            path.display(),
        );
        return None;
    }
    tracing::info!(
        target: "prompts",
        "loaded prompt override from {}",
        path.display(),
    );
    Some(raw)
}

fn push_prompt_override_notice(message: String) {
    if let Ok(mut notices) = PROMPT_OVERRIDE_NOTICES.lock() {
        notices.push(message);
    }
}

pub fn take_prompt_override_notices() -> Vec<String> {
    PROMPT_OVERRIDE_NOTICES
        .lock()
        .map(|mut notices| std::mem::take(&mut *notices))
        .unwrap_or_default()
}

/// Load user prompt overrides from `config_dir` and install them through the
/// existing override hooks. Returns the names of the overrides that were
/// applied (for logging/diagnostics).
///
/// Call once at startup, before any engine spawns, because the underlying
/// override cells are first-call-wins. Missing files are a no-op, preserving
/// the bundled defaults.
pub fn load_config_dir_prompt_overrides(config_dir: &Path) -> Vec<&'static str> {
    let mut applied = Vec::new();
    if let Some(text) = read_prompt_override_file(config_dir, CONSTITUTION_OVERRIDE_FILE) {
        if !base_prompt_override_opt_in() {
            // A file exists but the user hasn't opted in. Don't silently
            // replace the base prompt — surface the gate instead.
            let warning = format!(
                "Custom Constitution override found at {}/{} but {} is not set; using the bundled Constitution. Set {}=1 to opt in.",
                config_dir.display(),
                CONSTITUTION_OVERRIDE_FILE,
                BASE_PROMPT_OVERRIDE_OPT_IN_ENV,
                BASE_PROMPT_OVERRIDE_OPT_IN_ENV,
            );
            tracing::warn!(
                target: "prompts",
                "{warning}",
            );
            push_prompt_override_notice(warning);
        } else if set_base_prompt_override(text).is_ok() {
            applied.push("constitution");
        }
    }
    applied
}

/// Resolve the CodeWhale config directory and load any prompt overrides found
/// there. Convenience wrapper around [`load_config_dir_prompt_overrides`] for
/// startup wiring; silently does nothing when the config home cannot be
/// resolved.
pub fn load_prompt_overrides_from_config_home() {
    let Ok(home) = codewhale_config::codewhale_home() else {
        return;
    };
    let applied = load_config_dir_prompt_overrides(&home);
    if !applied.is_empty() {
        tracing::info!(
            target: "prompts",
            "applied {} config-directory prompt override(s): {}",
            applied.len(),
            applied.join(", "),
        );
    }
}

fn set_prompt_override(cell: &std::sync::OnceLock<String>, s: String) -> Result<(), String> {
    cell.set(s)
}

fn set_static_prompt_composer(
    cell: &std::sync::OnceLock<Box<StaticPromptComposer>>,
    f: Box<StaticPromptComposer>,
) -> Result<(), Box<StaticPromptComposer>> {
    cell.set(f)
}

fn effective_prompt_override<'a>(
    cell: &'a std::sync::OnceLock<String>,
    fallback: &'static str,
) -> &'a str {
    cell.get().map(String::as_str).unwrap_or(fallback)
}

fn effective_base_prompt() -> &'static str {
    effective_prompt_override(&BASE_PROMPT_OVERRIDE, BASE_PROMPT)
}

fn effective_static_prompt_composer() -> Option<&'static StaticPromptComposer> {
    STATIC_PROMPT_COMPOSER.get().map(Box::as_ref)
}

fn effective_locale_preamble_zh_hans() -> &'static str {
    effective_prompt_override(&LOCALE_PREAMBLE_ZH_HANS_OVERRIDE, LOCALE_PREAMBLE_ZH_HANS)
}

fn effective_locale_preamble_ja() -> &'static str {
    effective_prompt_override(&LOCALE_PREAMBLE_JA_OVERRIDE, LOCALE_PREAMBLE_JA)
}

fn effective_locale_preamble_pt_br() -> &'static str {
    effective_prompt_override(&LOCALE_PREAMBLE_PT_BR_OVERRIDE, LOCALE_PREAMBLE_PT_BR)
}

fn effective_locale_preamble_vi() -> &'static str {
    effective_prompt_override(&LOCALE_PREAMBLE_VI_OVERRIDE, LOCALE_PREAMBLE_VI)
}

fn effective_locale_closer_zh_hans() -> &'static str {
    effective_prompt_override(&LOCALE_CLOSER_ZH_HANS_OVERRIDE, LOCALE_CLOSER_ZH_HANS)
}

fn effective_locale_closer_ja() -> &'static str {
    effective_prompt_override(&LOCALE_CLOSER_JA_OVERRIDE, LOCALE_CLOSER_JA)
}

fn effective_locale_closer_pt_br() -> &'static str {
    effective_prompt_override(&LOCALE_CLOSER_PT_BR_OVERRIDE, LOCALE_CLOSER_PT_BR)
}

fn effective_locale_closer_vi() -> &'static str {
    effective_prompt_override(&LOCALE_CLOSER_VI_OVERRIDE, LOCALE_CLOSER_VI)
}

fn effective_authority_recap() -> &'static str {
    effective_prompt_override(&AUTHORITY_RECAP_OVERRIDE, AUTHORITY_RECAP)
}

/// Optional locale-native reinforcement preamble prepended to the system
/// prompt when the user's UI locale is non-English.
///
/// `constitution.md` itself stays English (single source of truth, model is
/// natively multilingual, prefix-cache stable across users in the same
/// locale). For non-English locales we prepend a short locale-native
/// passage so the model's first exposure to the prompt overrides the
/// "match user message language" English directive with an explicit
/// "use {locale}" instruction in the user's own writing system. Reduces
/// the model's reliance on inferring intent from `## Environment.lang`
/// — which previously got overpowered by overwhelmingly English task
/// context, the symptom reported in #1118 and visible in the WeChat
/// screenshot that prompted this change.
///
/// The list is intentionally short (only locales the TUI ships UI
/// strings for: `zh-Hans`, `ja`, `pt-BR`). Other locales fall through
/// to `None` and get the English-only directive, which is the same
/// behavior as before this change.
///
/// ## Design philosophy: why a bookend, not a full translation
///
/// Community feedback on the WeChat thread that prompted this work
/// pointed out — correctly — that DeepSeek V4 is a Chinese-first
/// multilingual model, not an English-only model with multilingual
/// veneer. Its tokenizer is co-trained on Chinese; `你好` typically
/// encodes to ~1 token, not 2 — the "Chinese is expensive in tokens"
/// folk wisdom from Western-LLM commentary doesn't apply here.
///
/// The naïve translation of that argument would be: ship a fully
/// translated `constitution.md` per locale. We deliberately stop short of
/// that for v0.8.29. The reasons, ranked:
///
///   1. **Drift risk.** A 200+ line technical prompt has subtle
///      phrasing that drives subtle behavior. Every rule change has
///      to land in N translated copies, kept in lockstep. The class
///      of bug that arises (Chinese users see slightly different
///      agent behavior than English users) is hard to reproduce and
///      hard to triage from bug reports.
///   2. **Cache stability.** With one English `constitution.md` and a
///      per-locale preamble+closer, the largest cacheable chunk
///      (mode prompt + project context + environment) stays
///      byte-stable within a session and across users in the same
///      locale. A fully translated per-locale `constitution.md` keeps cache
///      per-locale but doesn't share with English users.
///   3. **Translation QA is expensive.** Each prompt-language pair
///      needs a native speaker reviewing tone, register, and rule
///      preservation. Getting it 95% right is bad, because the
///      missing 5% becomes silent behavior divergence.
///
/// What we DO instead — the bookend pattern @MuMu described from
/// their other project — is reinforce the locale directive in
/// native script at BOTH ends of the prompt. The opening anchors
/// behavior at session start; the closing reinforcement
/// (`locale_reinforcement_closer`) sits at the maximum-recency
/// position right before the user's next message. Empirically this
/// is sufficient to keep `reasoning_content` in the target locale
/// even as English code accumulates in context turn-over-turn.
///
/// If at some future point the bookend proves insufficient — or if
/// the maintenance cost of per-locale `constitution.md` files becomes
/// preferable to whatever's blocking it — full translation is the
/// natural next step. The locale tags here, the test invariants,
/// and the closer position would all carry over unchanged.
pub(crate) fn locale_reinforcement_preamble(locale_tag: &str) -> Option<&'static str> {
    match locale_tag {
        "zh-Hans" | "zh-CN" | "zh" => Some(effective_locale_preamble_zh_hans()),
        "ja" | "ja-JP" => Some(effective_locale_preamble_ja()),
        "pt-BR" | "pt" => Some(effective_locale_preamble_pt_br()),
        "vi" | "vi-VN" => Some(effective_locale_preamble_vi()),
        _ => None,
    }
}

/// Locale-native closing reinforcement appended to the very end of the
/// system prompt — the bookend MuMu described in the WeChat thread that
/// prompted #1118 follow-up work.
///
/// The opening preamble alone is not enough: as the model accumulates
/// English context turn-over-turn (code, error logs, search results,
/// file listings), the recency bias of the transformer's attention
/// drifts thinking back toward English even when the user keeps writing
/// in their own language. A closing native-script reinforcement sits at
/// the position closest to the user's next message — where attention
/// weight is highest — and re-asserts the language rule right before
/// the model generates `reasoning_content` for the turn.
///
/// Like the opening preamble, English (and unknown) locales return
/// `None` and the system prompt is byte-identical to the pre-bookend
/// behavior.
pub(crate) fn locale_reinforcement_closer(locale_tag: &str) -> Option<&'static str> {
    match locale_tag {
        "zh-Hans" | "zh-CN" | "zh" => Some(effective_locale_closer_zh_hans()),
        "ja" | "ja-JP" => Some(effective_locale_closer_ja()),
        "pt-BR" | "pt" => Some(effective_locale_closer_pt_br()),
        "vi" | "vi-VN" => Some(effective_locale_closer_vi()),
        _ => None,
    }
}

const LOCALE_PREAMBLE_ZH_HANS: &str = "## 语言要求\n\n\
你正在 codewhale 中运行。无论任务上下文（代码、错误日志、文件名）\
是英文，无论系统提示的其余部分是英文，你都必须用简体中文进行 \
`reasoning_content`（内部思考）和最终回复。代码、文件路径、工具名称\
（例如 `read_file`、`exec_shell`）、环境变量、命令行参数和 URL \
保持原样 —— 只有自然语言散文要切换到简体中文。\n\n\
如果用户在会话中切换到另一种语言，从下一轮开始跟随切换。\
如果用户明确要求（例如 \"think in English\"），则覆盖此规则。";

const LOCALE_PREAMBLE_JA: &str = "## 言語要件\n\n\
codewhale を実行しています。タスクコンテキスト（コード、エラーログ、\
ファイル名）が英語であっても、システムプロンプトの他の部分が英語で\
あっても、`reasoning_content`（内部思考）と最終的な返信は日本語で\
行ってください。コード、ファイルパス、ツール名（例：`read_file`、\
`exec_shell`）、環境変数、コマンドライン引数、URL は元のまま —— \
自然言語の文章のみ日本語に切り替えます。\n\n\
ユーザーがセッション中に別の言語に切り替えた場合は、次のターンから\
それに従ってください。ユーザーが明示的に要求した場合（例：\
\"think in English\"）はこのルールを上書きします。";

const LOCALE_PREAMBLE_PT_BR: &str = "## Requisito de Idioma\n\n\
Você está rodando dentro do codewhale. Escreva tanto \
`reasoning_content` (seu pensamento interno) quanto a resposta final \
em português do Brasil, mesmo quando o contexto da tarefa (código, \
logs de erro, nomes de arquivos) estiver em inglês e mesmo quando o \
resto do system prompt for em inglês. Mantenha código, caminhos de \
arquivos, nomes de ferramentas (por exemplo `read_file`, \
`exec_shell`), variáveis de ambiente, flags de linha de comando e \
URLs no formato original — apenas a prosa em linguagem natural muda \
para português do Brasil.\n\n\
Se o usuário mudar de idioma no meio da sessão, mude no próximo turno. \
Se o usuário pedir explicitamente (por exemplo, \"think in English\"), \
isso sobrescreve esta regra.";

// ── Closing bookends (appended to the very end of the system prompt) ──

const LOCALE_CLOSER_ZH_HANS: &str = "## 语言再次提醒\n\n\
**重要：你的 `reasoning_content`（内部思考）和最终回复必须保持简体中文。** \
无论你在这次会话中读到了多少英文代码、错误日志或文档，无论项目上下文 \
是英文，思考过程不能漂移到英文。这是会话级硬性要求 —— 用户的语言决定 \
你的语言，与上下文中累积的英文内容无关。除非用户明确要求切换（例如 \
\"think in English\"），否则继续用简体中文思考和回答。";

const LOCALE_CLOSER_JA: &str = "## 言語再確認\n\n\
**重要：`reasoning_content`（内部思考）と最終的な返信は日本語で行ってください。** \
このセッションで読み込んだ英語のコード、エラーログ、ドキュメントの量に \
関係なく、プロジェクトコンテキストが英語であっても、思考プロセスを \
英語に逸らさないでください。これはセッションレベルの厳格な要件であり、 \
ユーザーの言語があなたの言語を決定します。ユーザーが明示的に切り替えを \
要求しない限り（例：\"think in English\"）、日本語で思考し、回答し続けて \
ください。";

const LOCALE_CLOSER_PT_BR: &str = "## Reforço de Idioma\n\n\
**Importante: seu `reasoning_content` (pensamento interno) e a resposta \
final devem permanecer em português do Brasil.** Independentemente de \
quanto código em inglês, logs de erro ou documentação você ler nesta \
sessão, e independentemente de o contexto do projeto ser em inglês, o \
processo de pensamento não pode derivar para o inglês. Este é um \
requisito rígido em nível de sessão — o idioma do usuário define seu \
idioma. A menos que o usuário peça explicitamente a troca (por exemplo, \
\"think in English\"), continue pensando e respondendo em português do \
Brasil.";

const LOCALE_PREAMBLE_VI: &str = "## Yêu cầu ngôn ngữ\n\n\
Bạn đang chạy trong codewhale. Cho dù ngữ cảnh tác vụ (mã nguồn, nhật ký lỗi, tên tệp) \
là tiếng Anh, cho dù phần còn lại của system prompt là tiếng Anh, bạn đều phải sử dụng \
tiếng Việt cho phần `reasoning_content` (suy nghĩ nội bộ) và câu trả lời cuối cùng. Các từ \
mã nguồn, đường dẫn tệp, tên công cụ (ví dụ `read_file`, `exec_shell`), biến môi trường, \
tham số dòng lệnh và URL giữ nguyên dạng gốc —— chỉ các văn bản giải thích bằng ngôn ngữ \
tự nhiên mới được chuyển sang tiếng Việt.\n\n\
Nếu người dùng chuyển sang ngôn ngữ khác trong phiên làm việc, hãy chuyển theo từ lượt tiếp theo. \
Nếu người dùng yêu cầu rõ ràng (ví dụ \"think in English\"), hãy ghi đè quy tắc này.";

const LOCALE_CLOSER_VI: &str = "## Nhắc nhở ngôn ngữ một lần nữa\n\n\
**Quan trọng: phần `reasoning_content` (suy nghĩ nội bộ) và phản hồi cuối cùng của bạn phải được viết bằng tiếng Việt.** \
Dù bạn có đọc bao nhiêu mã nguồn tiếng Anh, nhật ký lỗi hay tài liệu trong phiên làm việc này, và dù ngữ cảnh \
dự án có là tiếng Anh, quá trình suy nghĩ của bạn cũng không được chuyển sang tiếng Anh. Đây là yêu cầu cứng \
ở cấp phiên làm việc —— ngôn ngữ của người dùng quyết định ngôn ngữ của bạn, không phụ thuộc vào nội dung tiếng Anh \
tích lũy trong ngữ cảnh. Trừ khi người dùng yêu cầu rõ ràng việc chuyển đổi (ví dụ \"think in English\"), \
hãy tiếp tục suy nghĩ và trả lời bằng tiếng Việt.";

/// Personality overlays — voice and tone.
pub const CALM_PERSONALITY: &str = include_str!("prompts/personalities/calm.md");
pub const PLAYFUL_PERSONALITY: &str = include_str!("prompts/personalities/playful.md");

/// Mode deltas — permissions, workflow expectations, mode-specific rules.
pub const AGENT_MODE: &str = include_str!("prompts/modes/agent.md");
pub const PLAN_MODE: &str = include_str!("prompts/modes/plan.md");
pub const YOLO_MODE: &str = include_str!("prompts/modes/yolo.md");
pub const OPERATE_MODE: &str = include_str!("prompts/modes/operate.md");

/// Approval-policy overlays — whether tool calls are auto-approved,
/// require confirmation, or are blocked.
pub const AUTO_APPROVAL: &str = include_str!("prompts/approvals/auto.md");
pub const SUGGEST_APPROVAL: &str = include_str!("prompts/approvals/suggest.md");
pub const NEVER_APPROVAL: &str = include_str!("prompts/approvals/never.md");

/// Shell policy guidance for `allow_shell=false`. Referenced from the
/// Runtime Policy Reference so the model can adapt without mutating the
/// static system-prompt prefix (preserves DeepSeek prefix cache across
/// shell-access toggles).
pub const SHELL_POLICY_DISABLED: &str = "Shell tools unavailable. For mandatory-use items referencing \
`exec_shell`, use `code_execution` (Python sandbox). For GitHub triage, use \
`github_issue_context` / `github_pr_context` as primary route.";

/// Compaction relay template — written into the system prompt so the
/// model knows the format to use when writing `.codewhale/handoff.md`.
pub const COMPACT_TEMPLATE: &str = include_str!("prompts/compact.md");

/// Goal continuation audit template — injected by the engine when a runtime
/// goal is active and the assistant tries to end a turn without closing it.
pub const GOAL_CONTINUATION_PROMPT: &str = include_str!("prompts/continuation.md");

/// Memory hygiene guidance — appended to the system prompt only when the
/// session has a non-empty user-memory block. Steers the model toward
/// writing durable memories as declarative facts ("User prefers concise
/// responses") rather than imperatives ("Always respond concisely"),
/// because imperatives get re-read as directives in later sessions and
/// can override the user's current request (#725).
pub const MEMORY_GUIDANCE: &str = include_str!("prompts/memory_guidance.md");

/// Lean execution layer shared by the default agent runtime. Product/UI
/// tutorials remain outside the model-facing coding contract.
pub const CORE_EXECUTION_PROFILE_PROMPT: &str = include_str!("prompts/core_execution.md");

// ── Legacy prompt constants (kept for backwards compatibility) ────────

/// Legacy base prompt (agent.txt — now decomposed into constitution.md + overlays).
/// Still available for callers that haven't migrated to the layered API.
pub const AGENT_PROMPT: &str = include_str!("prompts/agent.txt");

// ── Personality selection ─────────────────────────────────────────────

/// Which personality overlay to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    /// Cool, spatial, reserved — the default.
    Calm,
    /// Warm, energetic, playful — alternative for fun mode.
    Playful,
}

impl Personality {
    /// Resolve from the `calm_mode` settings flag.
    /// When `calm_mode` is true → Calm; when false → Playful (future).
    /// For now, always returns Calm — Playful is wired but opt-in.
    #[must_use]
    pub fn from_settings(calm_mode: bool) -> Self {
        if calm_mode {
            Self::Calm
        } else {
            // Future: when playful mode is exposed in settings, return Playful here.
            // For now, calm is the only default.
            Self::Calm
        }
    }
}

// ── Composition ───────────────────────────────────────────────────────

/// Compose the full system prompt in deterministic order:
///   1. tool taxonomy  — compact hints generated from the eager core tools
///   2. constitution.md — core identity, toolbox, execution contract
///   3. personality    — voice and tone overlay
///   4. mode delta     — mode-specific permissions and workflow
///   5. approval policy — tool-approval behavior
///
/// Each layer is separated by a blank line for readability in the
/// rendered prompt (the model sees them as contiguous sections).
/// Substitute the model id for embedder-supplied prompt overrides that still
/// template it. The bundled constitution is deliberately model-agnostic and
/// carries no model-fact placeholders.
fn apply_model_template(
    prompt: &str,
    model_id: &str,
    _context_window_override: Option<u32>,
) -> String {
    prompt.replace("{model_id}", model_id)
}

/// Authority recap block — appended at the end of the system prompt,
/// just before the user's first message. Uses recency bias constructively:
/// this is the last thing the model reads before generating, so it
/// reinforces the Constitutional hierarchy without occupying cache-stable
/// prefix space.
const AUTHORITY_RECAP: &str = "\
## Authority Recap

CodeWhale's constitution governs your behavior. Ground truth underlies the
whole list: the user may override a fact, but no one may invent one. When
guidance conflicts, the user's request this turn outranks this constitution,
which outranks nearest-scope project law and instructions, which outrank
standing user-global preferences, which outrank memory and previous-session
handoffs. When in doubt, consult ### Whose word wins.";

pub fn compose_prompt(personality: Personality) -> String {
    compose_prompt_with_approval_model_and_shell(personality, "codewhale")
}

pub fn compose_prompt_with_approval_model_and_shell(
    personality: Personality,
    model_id: &str,
) -> String {
    let default_layers = compose_default_static_layers(personality, model_id);
    apply_static_prompt_composer(
        effective_static_prompt_composer(),
        personality,
        model_id,
        &default_layers,
    )
}

fn compose_default_static_layers(_personality: Personality, model_id: &str) -> String {
    // Personality is folded into the constitutional preamble/articles — no
    // separate overlay is appended. Language and output rules are split into
    // their own static segments so the 0.9.0 constitution stays compact.
    let layers = format!(
        "{}\n\n{}\n\n{}",
        effective_base_prompt().trim(),
        LANGUAGE_PROMPT.trim(),
        OUTPUT_PROMPT.trim()
    );
    apply_model_template(&layers, model_id, None)
}

fn apply_static_prompt_composer(
    composer: Option<&StaticPromptComposer>,
    personality: Personality,
    model_id: &str,
    default_layers: &str,
) -> String {
    match composer {
        Some(composer) => composer(&StaticPromptCtx {
            model_id,
            personality,
            default_layers,
        }),
        None => default_layers.to_string(),
    }
}

// The full base prompt is always used; effective tool availability is enforced
// by the tool catalog and execution layer rather than by mutating message[0].

// ── Public API ────────────────────────────────────────────────────────

/// Get the system prompt for a specific mode with project context.
pub fn system_prompt_for_mode_with_context(
    workspace: &Path,
    working_set_summary: Option<&str>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_and_skills(workspace, working_set_summary, None, None, None)
}

/// Get the system prompt for a specific mode with project and skills context.
///
/// **Volatile-content-last invariant.** Blocks are appended in order from
/// most-static to most-volatile so DeepSeek's KV prefix cache hits the
/// longest possible byte prefix turn-over-turn:
///
///   1. mode prompt (compile-time constant)
///   2. project context / fallback (workspace-static)
///   3. skills block (skills-dir-static)
///   4. `## Core Execution` (compile-time constant)
///   5. compaction relay template (compile-time constant)
///   6. relay block — file-backed; rewritten by `/compact` and on exit
///
/// Anything appended after a volatile block forfeits the cache for the rest
/// of the request. New blocks belong above the relay boundary unless they
/// themselves are turn-volatile. Working-set metadata is now injected into the
/// latest user message as per-turn metadata instead of this system prompt.
pub fn system_prompt_for_mode_with_context_and_skills(
    workspace: &Path,
    working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    user_memory_block: Option<&str>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_skills_and_session(
        workspace,
        working_set_summary,
        skills_dir,
        instructions,
        PromptSessionContext {
            user_memory_block,
            goal_objective: None,
            project_context_pack_enabled: true,
            locale_tag: "en",
            translation_enabled: false,
            model_id: "codewhale",
            context_window_override: None,
            show_thinking: true,
            verbosity: None,
            skills_scan_codewhale_only: false,
            shell_binary: "sh",
        },
    )
}

pub fn system_prompt_for_mode_with_context_skills_and_session(
    workspace: &Path,
    _working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    session_context: PromptSessionContext<'_>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_skills_session_and_approval(
        workspace,
        _working_set_summary,
        skills_dir,
        instructions,
        session_context,
    )
}

pub fn system_prompt_for_mode_with_context_skills_session_and_approval(
    workspace: &Path,
    _working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    session_context: PromptSessionContext<'_>,
) -> SystemPrompt {
    let default_layers = apply_model_template(
        effective_base_prompt().trim(),
        session_context.model_id,
        session_context.context_window_override,
    );
    let mode_prompt = apply_static_prompt_composer(
        effective_static_prompt_composer(),
        Personality::Calm,
        session_context.model_id,
        &default_layers,
    );

    // Load project context from workspace
    let project_context = load_project_context_with_parents(workspace);

    // 0. Locale-native reinforcement preamble (#1118 follow-up). When the
    // user's UI locale is non-English we prepend a short native-script
    // passage so the model's first exposure to the prompt is an explicit
    // "think and reply in {locale}" directive in the user's own writing
    // system — defeats the "task context is English, so the model thinks
    // in English even though `lang: zh-Hans` is set" failure mode that
    // PR #1398 partially addressed. English (and unknown) locales get
    // `None` and keep the previous behavior unchanged.
    let preamble = if session_context.show_thinking {
        locale_reinforcement_preamble(session_context.locale_tag)
    } else {
        None
    };

    // 1–2. Mode prompt + project context.
    // `load_project_context_with_parents` generates an in-memory bounded
    // overview when no context file exists, so the fallback should usually be
    // available without writing project-local files.
    let mut full_prompt = if let Some(project_block) = project_context.as_system_block() {
        format!("{mode_prompt}\n\n{project_block}")
    } else {
        // Extremely unlikely: context generation failed (e.g. filesystem error).
        // Use mode prompt alone rather than panic.
        tracing::warn!("No project context available and auto-generation failed");
        mode_prompt
    };

    if let Some(preamble) = preamble {
        full_prompt = format!("{preamble}\n\n{full_prompt}");
    }

    if let Some(user_constitution_block) = load_user_constitution_block() {
        full_prompt = format!("{full_prompt}\n\n{user_constitution_block}");
    }

    if session_context.project_context_pack_enabled
        && let Some(pack) = crate::project_context::generate_project_context_pack(workspace)
    {
        full_prompt = format!("{full_prompt}\n\n{pack}");
    }

    // 2.3a. Translation output instruction — when enabled, instruct
    // the model to respond in the resolved session locale. Stays
    // above the volatile-content boundary because it's a per-session
    // flag, not a per-turn one: enabling `/translate` is a session
    // toggle, so the prompt-prefix bytes don't drift turn-over-turn.
    if session_context.translation_enabled {
        full_prompt = format!(
            "{full_prompt}\n\n{}",
            translation_output_instruction(session_context.locale_tag)
        );
    }

    if is_concise_verbosity(session_context.verbosity) {
        full_prompt = format!(
            "{full_prompt}\n\n{}",
            concise_output_discipline_instruction()
        );
    }

    // 3. Skills block. #432: default discovery walks every compatible
    // workspace/global skill directory so skills installed for other AI-tool
    // conventions show up in the catalogue. Users can opt into a CodeWhale-only
    // scan with `[skills] scan_codewhale_only = true`. When an explicit
    // `skills_dir` is configured, union it with the workspace view instead of
    // treating it as a fallback; the workspace view often returns Some and
    // would otherwise shadow the configured directory entirely.
    let skill_discovery_mode = crate::skills::SkillDiscoveryMode::from_codewhale_only(
        session_context.skills_scan_codewhale_only,
    );
    let skills_block = match skills_dir {
        Some(dir) => {
            crate::skills::render_available_skills_context_for_workspace_and_dir_with_mode(
                workspace,
                dir,
                skill_discovery_mode,
                session_context.locale_tag,
            )
        }
        None => crate::skills::render_available_skills_context_for_workspace_with_mode(
            workspace,
            skill_discovery_mode,
            session_context.locale_tag,
        ),
    };
    if let Some(block) = skills_block {
        full_prompt = format!("{full_prompt}\n\n{block}");
    }

    // 4. Lean, runtime-only coding discipline. Context pressure, prompt-cache
    // accounting, footer presentation, and automatic compaction are host
    // responsibilities; teaching their UI to the model dilutes the task.
    full_prompt.push_str("\n\n");
    full_prompt.push_str(CORE_EXECUTION_PROFILE_PROMPT.trim());

    // 5. Compaction relay template — so the model knows the format to use
    //    when writing `.codewhale/handoff.md` on exit / `/compact`.
    full_prompt.push_str("\n\n");
    full_prompt.push_str(COMPACT_TEMPLATE);

    // ── Volatile-content boundary → WorldState fragments ──────────────────
    // Constitution (`full_prompt`) stays the cache-stable Blocks[0] prefix.
    // Everything below drifts mid-session and is assembled as marked
    // WorldState fragments so an env/memory/goal/handoff change can
    // `render_diff` without rebuilding unrelated material.

    // Workspace fragment: environment + mid-session memory/goal facts.
    let mut workspace_parts = vec![render_environment_block(
        workspace,
        session_context.locale_tag,
        session_context.shell_binary,
    )];
    if let Some(memory_block) = session_context.user_memory_block
        && !memory_block.trim().is_empty()
    {
        workspace_parts.push(format!("{memory_block}\n\n{MEMORY_GUIDANCE}"));
    }
    if let Some(goal_objective) = session_context.goal_objective
        && !goal_objective.trim().is_empty()
    {
        workspace_parts.push(format!(
            "## Current Goal\n\n<session_goal>\n{}\n</session_goal>",
            goal_objective.trim()
        ));
    }
    let workspace_body = workspace_parts.join("\n\n");

    // Permissions fragment: configured `instructions = [...]` files (#454).
    let permissions_body = instructions.and_then(render_instructions_block);

    // Route fragment: active model / verbosity / translation posture.
    let route_body = render_route_fragment(&session_context);

    // Token-budget / continuity fragment: prior-session handoff relay.
    let token_budget_body = load_handoff_block(workspace);

    let world_state = world_state_from_session_facts(
        Some(workspace_body.as_str()),
        permissions_body.as_deref(),
        Some(route_body.as_str()),
        None, // AgentTopology is updated by runtime callers when available.
        None, // Skills stay in the constitution prefix (skills-dir-static).
        token_budget_body.as_deref(),
    );

    let mut blocks = crate::model_context::WorldStateSnapshot {
        constitution: full_prompt,
        world_state,
    }
    .to_system_blocks();

    // Trailers keep recency bias after WorldState: authority, then locale.
    blocks.push(SystemBlock {
        text: effective_authority_recap().trim().to_string(),
        cache_control: PromptCacheControl::Volatile,
    });
    if let Some(closer) = session_context
        .show_thinking
        .then(|| locale_reinforcement_closer(session_context.locale_tag))
        .flatten()
    {
        blocks.push(SystemBlock {
            text: closer.trim().to_string(),
            cache_control: PromptCacheControl::Volatile,
        });
    } else if !session_context.show_thinking {
        blocks.push(SystemBlock {
            text: hidden_thinking_language_instruction(session_context.locale_tag)
                .trim()
                .to_string(),
            cache_control: PromptCacheControl::Volatile,
        });
    }

    SystemPrompt { blocks }
}

/// Flatten a system prompt to joined text (tests + debug inspectors).
#[must_use]
pub fn system_prompt_flat_text(prompt: &SystemPrompt) -> String {
    prompt
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_route_fragment(session_context: &PromptSessionContext<'_>) -> String {
    let verbosity = session_context
        .verbosity
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    format!(
        "model: {}\nverbosity: {}\ntranslation: {}\nshow_thinking: {}",
        session_context.model_id.trim(),
        verbosity,
        if session_context.translation_enabled {
            "on"
        } else {
            "off"
        },
        if session_context.show_thinking {
            "on"
        } else {
            "off"
        },
    )
}

/// Assemble a cache-stable constitution prefix with a typed WorldState layer.
///
/// This is the Codex-parity assembly point: constitution stays byte-stable for
/// prefix caching; volatile concerns live in `WorldState` fragments with
/// markers, caps, and `render_diff` retain-unchanged behavior. Callers that
/// still need a flat string can use [`WorldStateSnapshot::render_text`].
pub fn system_prompt_with_world_state(
    constitution: impl Into<String>,
    world_state: crate::model_context::WorldState,
) -> SystemPrompt {
    let snapshot = crate::model_context::WorldStateSnapshot {
        constitution: constitution.into(),
        world_state,
    };
    SystemPrompt {
        blocks: snapshot.to_system_blocks(),
    }
}

/// Build a WorldState from the common volatile session facts.
///
/// Does not load constitution — callers keep that as the stable base.
pub fn world_state_from_session_facts(
    workspace_body: Option<&str>,
    permissions_body: Option<&str>,
    route_body: Option<&str>,
    agent_topology_body: Option<&str>,
    skills_tools_body: Option<&str>,
    token_budget_body: Option<&str>,
) -> crate::model_context::WorldState {
    let mut state = crate::model_context::WorldState::new();
    if let Some(body) = workspace_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_workspace(body);
    }
    if let Some(body) = permissions_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_permissions(body);
    }
    if let Some(body) = route_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_route(body);
    }
    if let Some(body) = agent_topology_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_agent_topology(body);
    }
    if let Some(body) = skills_tools_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_skills_tools(body);
    }
    if let Some(body) = token_budget_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_token_budget(body);
    }
    state
}

/// Build a system prompt with explicit project context
pub fn build_system_prompt(base: &str, project_context: Option<&ProjectContext>) -> SystemPrompt {
    let full_prompt =
        match project_context.and_then(super::project_context::ProjectContext::as_system_block) {
            Some(project_block) => format!("{}\n\n{}", base.trim(), project_block),
            None => base.trim().to_string(),
        };
    SystemPrompt::from_text(full_prompt)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;
    use sha2::{Digest, Sha256};

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: this crate has one environment-mutating test and does
            // not start threads while the scoped fixture is active.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: see `EnvGuard::set`; restoration happens before the
            // single test exits.
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn normalized_fixture_text(text: &str) -> String {
        text.replace(
            &format!("- platform: {}", std::env::consts::OS),
            "- platform: <fixture-os>",
        )
    }

    #[test]
    fn production_prompt_fixture_freezes_body_order_and_cache_boundary() {
        let fixture = std::env::temp_dir().join("codewhale-context-production-prompt-fixture");
        let _ = fs::remove_dir_all(&fixture);
        let home = fixture.join("home");
        let workspace = fixture.join("workspace");
        fs::create_dir_all(home.join(".codewhale/skills")).expect("home skills");
        fs::create_dir_all(workspace.join(".codewhale/skills/frozen-skill"))
            .expect("workspace skill");
        fs::create_dir_all(workspace.join("src")).expect("source dir");
        fs::write(
            workspace.join("AGENTS.md"),
            "# Fixture law\n\nKeep the frozen prompt byte-identical.\n",
        )
        .expect("AGENTS.md");
        fs::write(
            workspace.join("README.md"),
            "# Frozen prompt fixture\n\nA deterministic project context pack.\n",
        )
        .expect("README");
        fs::write(workspace.join("src/lib.rs"), "pub fn fixture() {}\n").expect("source");
        fs::write(
            workspace.join(".codewhale/skills/frozen-skill/SKILL.md"),
            "---\nname: frozen-skill\ndescription: Deterministic fixture skill\n---\nUse it.\n",
        )
        .expect("skill");
        fs::create_dir_all(workspace.join(".codewhale")).expect("codewhale dir");
        fs::write(
            workspace.join(HANDOFF_RELATIVE_PATH),
            "# Relay\n\nContinue the frozen fixture.\n",
        )
        .expect("handoff");
        let configured = workspace.join("configured.md");
        fs::write(&configured, "Configured instruction body.\n").expect("configured instruction");

        let _home = EnvGuard::set("HOME", &home);
        let _codewhale_home = EnvGuard::set("CODEWHALE_HOME", &home.join(".codewhale"));
        let preferences = PromptPreferences {
            locale_setting: "zh-Hans".to_owned(),
            show_thinking: false,
        };
        let instructions = vec![
            InstructionSource::File(configured),
            InstructionSource::Inline {
                name: "cli:append-system-prompt".to_owned(),
                content: "CLI appended instruction body.".to_owned(),
            },
        ];

        let prompt = production_system_prompt(ProductionPromptRequest {
            workspace: &workspace,
            model: "deepseek-v4-pro",
            preferences: &preferences,
            instructions: &instructions,
            skills_dir: Some(&workspace.join(".codewhale/skills")),
            project_context_pack_enabled: true,
            verbosity: Some("concise"),
            skills_scan_codewhale_only: true,
            shell_binary: "/fixture/bin/zsh",
            tool_mode: true,
        });

        assert_eq!(prompt.blocks.len(), 8);
        assert_eq!(prompt.blocks[0].cache_control, PromptCacheControl::Stable);
        assert!(
            prompt
                .blocks
                .iter()
                .skip(1)
                .all(|block| block.cache_control == PromptCacheControl::Volatile)
        );
        assert!(prompt.blocks[0].text.contains("# Fixture law"));
        assert!(prompt.blocks[0].text.contains("frozen-skill"));
        assert!(prompt.blocks[1].text.contains("/fixture/bin/zsh"));
        assert!(
            prompt.blocks[2]
                .text
                .contains("Configured instruction body.")
        );
        assert!(
            prompt.blocks[2]
                .text
                .contains("CLI appended instruction body.")
        );
        assert!(prompt.blocks[3].text.contains("model: deepseek-v4-pro"));
        assert!(
            prompt.blocks[4]
                .text
                .contains("Continue the frozen fixture.")
        );
        assert!(prompt.blocks[5].text.starts_with("## Authority Recap"));
        assert!(
            prompt.blocks[6]
                .text
                .starts_with("## Hidden Thinking Language")
        );
        assert!(prompt.blocks[7].text.starts_with("你正在唯一 AgentRuntime"));

        let block_hashes = prompt
            .blocks
            .iter()
            .map(|block| sha256(normalized_fixture_text(&block.text).as_bytes()))
            .collect::<Vec<_>>();
        assert_eq!(
            block_hashes,
            [
                "b5b624482ecad9315e1d75cbba95bfd233db6e47ee2f0e65bd831884f6f9d784",
                "657021acf824946ccfaf5f7445fe894c19b4631b55a52f1bb63885231b5d7964",
                "6fea08828ee251fd8682f0987f4beaf95bd9dc28a4f4dd80606709b0af41099f",
                "6546204f20f95702891a51e577213f9ff3d55c05358d3b9539b658cba392882f",
                "4c80b2b5e829efd024a0d6665b949a58a70e33909d3b544dd2d0f4c5e7ae6c14",
                "525923116dacb0c1014dfc9ea028e69391ec011c2bf4d8ebddd994cefedea3e8",
                "4c3451c16a53fd13f7be517c523d7fa22c9d71ffa638b6f4bf32033d5a17d8e0",
                "c31d4de13a28a0ba5d9a01b0c09b37e8336ed6db67bf12d1b0b9adc6e72c87b5",
            ]
        );
        let normalized_prompt = prompt
            .blocks
            .iter()
            .map(|block| {
                format!(
                    "{:?}\0{}",
                    block.cache_control,
                    normalized_fixture_text(&block.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\0\0");
        assert_eq!(
            sha256(normalized_prompt.as_bytes()),
            "eb6300fe6821019fb7e0a247950a65ef3b6f880e199bb5f8e3252e6f696df7a6"
        );
        fs::remove_dir_all(&fixture).expect("remove fixture");
    }
}
