//! Skill discovery and registry used by the canonical prompt owner.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const MAX_SKILL_DESCRIPTION_CHARS: usize = 280;
const MAX_AVAILABLE_SKILLS_CHARS: usize = 12_000;
const MAX_SKILL_NAME_CHARS: usize = 64;
/// A discovered skill is admitted only when its complete UTF-8 definition fits
/// this Host-owned bound. `load_skill` therefore never truncates instructions.
pub const MAX_SKILL_SOURCE_BYTES: u64 = 128 * 1024;

// === Defaults ===

#[must_use]
pub fn default_skills_dir() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from("/tmp/dse/skills"),
        |p| p.join(".dse").join("skills"),
    )
}

/// Global agentskills.io-compatible skills directory (`~/.agents/skills`).
#[must_use]
pub fn agents_global_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|p| p.join(".agents").join("skills"))
}

// === Types ===

/// Session-time skill discovery scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillDiscoveryMode {
    /// Preserve the existing broad compatibility scan across DSE,
    /// agentskills.io, Claude, OpenCode, Cursor, and legacy DeepSeek roots.
    Compatible,
    /// Scan only DSE-owned roots. Callers that also pass an explicit
    /// `skills_dir` still get that directory because it is user configuration.
    DseOnly,
}

impl SkillDiscoveryMode {
    #[must_use]
    pub fn from_dse_only(value: bool) -> Self {
        if value {
            Self::DseOnly
        } else {
            Self::Compatible
        }
    }
}

/// Parsed representation of a SKILL.md definition.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    /// Default (language-neutral, usually English) description.
    pub description: String,
    /// Optional locale-specific descriptions, keyed by lowercased locale tag
    /// (e.g. `zh`, `zh-hant`, `ja`). Populated from `description_<tag>:`
    /// frontmatter keys so a skill author can ship a shorter, native-language
    /// description for non-English sessions (saves prompt tokens; see #3354).
    pub localized_descriptions: HashMap<String, String>,
    pub body: String,
    /// Hash and byte count of the complete admitted `SKILL.md` source. These
    /// are captured during discovery so later loads never race the filesystem.
    pub source_sha256: String,
    pub source_bytes: u64,
    /// On-disk path to the `SKILL.md` this was loaded from. The directory
    /// name can differ from the frontmatter `name` for community installs
    /// or manually-placed skills, so callers must use this rather than
    /// reconstructing `<dir>/<name>/SKILL.md`.
    pub path: PathBuf,
}

impl Skill {
    /// Pick the best description for a session `locale_tag`, falling back to the
    /// default `description` when no localized variant matches.
    ///
    /// Order: exact (lowercased) tag match, then the primary language subtag
    /// (so `en-us` → `en`, `pt-br` → `pt`, `zh-cn` → `zh`), then default.
    ///
    /// Chinese is the one place where the primary-subtag fallback would be
    /// *wrong*: Traditional and Simplified are written differently, so a
    /// Traditional tag (`zh-hant`, or the Traditional regions `zh-tw` / `zh-hk`
    /// / `zh-mo`) must NOT borrow a Simplified `description_zh`. Those match only
    /// an exact `description_zh-hant`-style key, else the default. Simplified
    /// tags (`zh`, `zh-hans`, `zh-cn`, …) still fold to `description_zh`.
    #[must_use]
    pub fn description_for_locale(&self, locale_tag: &str) -> &str {
        if self.localized_descriptions.is_empty() {
            return &self.description;
        }
        let normalized = locale_tag.trim().to_ascii_lowercase();
        if let Some(desc) = self.localized_descriptions.get(&normalized) {
            return desc;
        }
        if let Some((primary, _)) = normalized.split_once('-') {
            // Don't let a Traditional-Chinese session fall back to a Simplified
            // (`zh`) description — different written form, not just a region.
            let traditional_chinese = primary == "zh"
                && (normalized.contains("hant")
                    || normalized.ends_with("-tw")
                    || normalized.ends_with("-hk")
                    || normalized.ends_with("-mo"));
            if !traditional_chinese && let Some(desc) = self.localized_descriptions.get(primary) {
                return desc;
            }
        }
        &self.description
    }
}

/// Collection of discovered skills.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
    warnings: Vec<String>,
    /// Names that were ambiguous inside one discovery-precedence cell. A
    /// higher-precedence ambiguity blocks lower-precedence definitions instead
    /// of silently selecting one.
    ambiguous_names: HashSet<String>,
}

impl SkillRegistry {
    /// Maximum directory-traversal depth when discovering skills.
    ///
    /// Defends against pathological configurations (e.g. a user pointing
    /// `skills_dir` at `~`) without artificially limiting realistic
    /// vendored layouts like `<root>/<org>/<repo>/<skill>/SKILL.md`.
    const MAX_DISCOVERY_DEPTH: usize = 8;

    /// Discover skills from the given directory.
    ///
    /// The search walks `dir` recursively: any directory that contains a
    /// `SKILL.md` is loaded as a single skill, and the walk does **not**
    /// descend further into that directory: `SKILL.md` and its companion
    /// files form that skill's boundary. This lets users organize
    /// skills by vendor / category — e.g.
    /// `<root>/<vendor>/<skill>/SKILL.md` — instead of being forced into
    /// a flat `<root>/<skill>/SKILL.md` layout.
    ///
    /// Hidden subdirectories (names starting with `.`) below the root
    /// are skipped to avoid descending into VCS / cache trees like
    /// `.git/`. The provided `dir` itself is always honored, even if
    /// hidden — that's what the user explicitly configured.
    /// Symlinked directories are followed only when their canonical target
    /// remains inside the configured discovery root, with canonical path
    /// tracking plus [`Self::MAX_DISCOVERY_DEPTH`] keeping the walk finite.
    #[must_use]
    pub fn discover(dir: &Path) -> Self {
        let mut registry = Self::default();
        let Ok(canonical_dir) = fs::canonicalize(dir) else {
            return registry;
        };
        if !canonical_dir.is_dir() {
            return registry;
        }

        let mut visited = HashSet::new();
        Self::discover_recursive(dir, &canonical_dir, 0, &mut registry, &mut visited);
        registry
            .skills
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        registry
    }

    fn discover_recursive(
        dir: &Path,
        canonical_root: &Path,
        depth: usize,
        registry: &mut Self,
        visited: &mut HashSet<PathBuf>,
    ) {
        if depth > Self::MAX_DISCOVERY_DEPTH {
            return;
        }
        if !Self::mark_discovered_dir(dir, visited) {
            return;
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                // Only surface a warning for the user-provided root
                // (depth == 0). Nested permission errors are usually
                // noise (e.g. a stray `.Trash` inside someone's
                // `~/.agents/skills`).
                if depth == 0 {
                    registry.push_warning(format!("无法读取技能目录 {}：{err}", dir.display()));
                }
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            // Skip hidden subdirectories. Common offenders are `.git`,
            // `.cache`, `.Trash`. The provided root itself is exempt:
            // the user explicitly pointed `skills_dir` at it and we
            // never filter it (it's passed directly to this function,
            // not iterated). This check applies to *children* of the
            // current directory at every depth — including depth 0,
            // because a `.git/` right next to the skills we want is
            // exactly the kind of noise we must not descend into.
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }

            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_dir() {
                continue;
            }
            let Ok(canonical_path) = fs::canonicalize(&path) else {
                continue;
            };
            if !canonical_path.starts_with(canonical_root) {
                registry.push_warning(format!(
                    "技能路径 {} 解析到发现根目录之外，已拒绝。",
                    path.display()
                ));
                continue;
            }

            let skill_path = path.join("SKILL.md");
            match Self::read_skill_source(&skill_path, canonical_root) {
                Ok(Some((content, canonical_source))) => {
                    match Self::parse_skill(&skill_path, &content) {
                        Ok(mut skill) => {
                            if !Self::mark_discovered_dir(&path, visited) {
                                continue;
                            }
                            skill.path = canonical_source;
                            skill.source_bytes = u64::try_from(content.len()).unwrap_or(u64::MAX);
                            skill.source_sha256 = sha256(content.as_bytes());
                            registry.normalize_skill_name(&mut skill, &skill_path);
                            // Two definitions in the same precedence cell that
                            // normalize to one exact name are ambiguous. Remove the
                            // first and block the name so neither can be advertised
                            // or loaded.
                            let shadowed_index = registry
                                .skills
                                .iter()
                                .position(|candidate| candidate.name == skill.name);
                            if let Some(existing_index) = shadowed_index {
                                let existing = registry.skills.remove(existing_index);
                                registry.ambiguous_names.insert(skill.name.clone());
                                registry.push_warning(format!(
                                    "技能名 `{}` 在 {} 与 {} 之间存在歧义，均未加载。",
                                    skill.name,
                                    existing.path.display(),
                                    skill.path.display()
                                ));
                            } else if registry.ambiguous_names.contains(&skill.name) {
                                registry.push_warning(format!(
                                    "技能名 `{}` 已存在歧义，{} 未加载。",
                                    skill.name,
                                    skill.path.display()
                                ));
                            } else {
                                registry.skills.push(skill);
                            }
                            // This directory IS a skill. Don't descend further:
                            // any nested `SKILL.md` would be a fixture or
                            // example bundled with the parent skill, not a
                            // separately-installable skill.
                            continue;
                        }
                        Err(reason) => {
                            if !Self::mark_discovered_dir(&path, visited) {
                                continue;
                            }
                            registry.push_warning(format!(
                                "无法解析 {}：{reason}",
                                skill_path.display()
                            ));
                            // Still treat this directory as "claimed" — a
                            // malformed SKILL.md shouldn't cause us to
                            // double-load nested fixtures as skills.
                            continue;
                        }
                    }
                }
                Err(reason) => {
                    if !Self::mark_discovered_dir(&path, visited) {
                        continue;
                    }
                    registry.push_warning(format!("无法读取 {}：{reason}", skill_path.display()));
                    continue;
                }
                Ok(None) => {
                    // No SKILL.md here — recurse to look for nested
                    // skill directories (e.g. `<vendor>/<skill>/SKILL.md`).
                }
            }

            Self::discover_recursive(&path, canonical_root, depth + 1, registry, visited);
        }
    }

    fn mark_discovered_dir(dir: &Path, visited: &mut HashSet<PathBuf>) -> bool {
        let key = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        visited.insert(key)
    }

    fn read_skill_source(
        path: &Path,
        canonical_root: &Path,
    ) -> Result<Option<(String, PathBuf)>, String> {
        let canonical_source = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if !canonical_source.starts_with(canonical_root) {
            return Err("文件解析到发现根目录之外".to_owned());
        }
        if canonical_source.to_str().is_none() {
            return Err("文件路径不是有效 UTF-8，无法提供精确来源".to_owned());
        }
        let mut file = match fs::File::open(&canonical_source) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("不是普通文件".to_owned());
        }
        if metadata.len() > MAX_SKILL_SOURCE_BYTES {
            return Err(format!(
                "文件大小 {} bytes 超过上限 {MAX_SKILL_SOURCE_BYTES} bytes",
                metadata.len()
            ));
        }

        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len().min(MAX_SKILL_SOURCE_BYTES)).unwrap_or_default(),
        );
        file.by_ref()
            .take(MAX_SKILL_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SKILL_SOURCE_BYTES {
            return Err(format!("读取内容超过上限 {MAX_SKILL_SOURCE_BYTES} bytes"));
        }
        String::from_utf8(bytes)
            .map(|content| Some((content, canonical_source)))
            .map_err(|error| {
                format!(
                    "不是有效 UTF-8（offset {}）",
                    error.utf8_error().valid_up_to()
                )
            })
    }

    fn push_warning(&mut self, warning: String) {
        tracing::warn!(target: "skills", "{warning}");
        self.warnings.push(warning);
    }

    fn normalize_skill_name(&mut self, skill: &mut Skill, skill_path: &Path) {
        let normalized = normalize_skill_name_for_lookup(&skill.name);
        if normalized != skill.name || !is_valid_skill_name(&skill.name) {
            let original = skill.name.clone();
            skill.name = normalized;
            self.push_warning(format!(
                "{} 中的技能名 `{original}` 不能安全用作命令名，已改用 `{}`。",
                skill_path.display(),
                skill.name
            ));
        }
    }

    pub(crate) fn parse_skill(_path: &Path, content: &str) -> std::result::Result<Skill, String> {
        let trimmed = content.trim_start();

        // Try to parse frontmatter block first. If absent, fall back to
        // extracting the first `# Heading` as the skill name so that plain
        // Markdown files (no `---` fence) are accepted instead of rejected.
        if trimmed.starts_with("---") {
            let start = content
                .find("---")
                .ok_or_else(|| "缺少 frontmatter 起始分隔符".to_string())?;
            let rest = &content[start + 3..];
            let end = rest
                .find("---")
                .ok_or_else(|| "缺少 frontmatter 结束分隔符".to_string())?;
            let frontmatter = &rest[..end];
            let body = &rest[end + 3..];

            let mut metadata = HashMap::new();
            let lines: Vec<&str> = frontmatter.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                let raw = lines[i];
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    i += 1;
                    continue;
                }
                if let Some((key, value)) = line.split_once(':') {
                    let value = value.trim();
                    // Check for YAML block scalar indicators: > (folded), | (literal),
                    // optionally with chomping: >-, >+, |-, |+
                    let is_block_scalar = matches!(value, ">" | "|" | ">-" | ">+" | "|-" | "|+");
                    if is_block_scalar {
                        let is_folded = value.starts_with('>');
                        let chomp = if value.ends_with('-') {
                            "strip"
                        } else if value.ends_with('+') {
                            "keep"
                        } else {
                            "clip"
                        };
                        // Determine the base indentation from the key line
                        let base_indent = raw.len() - raw.trim_start().len();
                        let mut block_lines: Vec<&str> = Vec::new();
                        let mut content_indent: Option<usize> = None;
                        i += 1;
                        while i < lines.len() {
                            let raw_line = lines[i];
                            if raw_line.trim().is_empty() {
                                // Empty lines are part of the block
                                block_lines.push("");
                                i += 1;
                                continue;
                            }
                            let line_indent = raw_line.len() - raw_line.trim_start().len();
                            if line_indent > base_indent {
                                // Track content indent from the first non-empty
                                // line so we strip only that one level of
                                // leading whitespace, preserving any deeper
                                // relative indentation (YAML §8.1.2).
                                if content_indent.is_none() {
                                    content_indent = Some(line_indent);
                                }
                                block_lines.push(raw_line);
                                i += 1;
                            } else {
                                break;
                            }
                        }
                        let content_indent = content_indent.unwrap_or(base_indent);
                        // Strip only the content indent from each non-empty
                        // line so nested indentation survives.
                        let block_lines: Vec<&str> = block_lines
                            .iter()
                            .map(|raw| {
                                if raw.is_empty() {
                                    ""
                                } else {
                                    let indent = raw.len() - raw.trim_start().len();
                                    let strip = std::cmp::min(indent, content_indent);
                                    &raw[strip..]
                                }
                            })
                            .collect();
                        // Apply chomping to trailing empty lines before folding.
                        // Chomping operates on the raw block_lines (before join), so
                        // strip / keep / clip behave per the YAML spec.
                        let block_lines = if matches!(chomp, "strip") {
                            // strip: remove all trailing empty lines
                            let mut lines = block_lines;
                            while lines.last().is_some_and(|s| s.is_empty()) {
                                lines.pop();
                            }
                            lines
                        } else if matches!(chomp, "keep") {
                            // keep: no modification
                            block_lines
                        } else {
                            // clip: keep at most one trailing empty line
                            let mut lines = block_lines;
                            while lines.len() >= 2
                                && lines[lines.len() - 1].is_empty()
                                && lines[lines.len() - 2].is_empty()
                            {
                                lines.pop();
                            }
                            lines
                        };
                        let description = if is_folded {
                            // Folded: join non-empty lines with spaces; empty
                            // lines become paragraph breaks.
                            let mut result = String::new();
                            let mut pending_space = false;
                            for line in &block_lines {
                                if line.is_empty() {
                                    result.push('\n');
                                    pending_space = false;
                                } else {
                                    if pending_space {
                                        result.push(' ');
                                    }
                                    result.push_str(line);
                                    pending_space = true;
                                }
                            }
                            result
                        } else {
                            // Literal: join with newlines.
                            block_lines.join("\n")
                        };
                        metadata.insert(key.trim().to_ascii_lowercase(), description);
                    } else {
                        let unquoted = match value {
                            v if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
                                || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2) =>
                            {
                                &v[1..v.len() - 1]
                            }
                            _ => value,
                        };
                        metadata.insert(key.trim().to_ascii_lowercase(), unquoted.to_string());
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }

            let name = metadata
                .get("name")
                .filter(|name| !name.is_empty())
                .cloned()
                .ok_or_else(|| "缺少必需的 frontmatter 字段：name".to_string())?;

            let description = metadata.get("description").cloned().unwrap_or_default();

            // Collect `description_<tag>:` frontmatter keys (already lowercased
            // above) into locale-specific descriptions, e.g. `description_zh`.
            let localized_descriptions = metadata
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_prefix("description_")
                        .filter(|tag| !tag.is_empty())
                        .map(|tag| (tag.to_string(), value.clone()))
                })
                .collect();

            return Ok(Skill {
                name,
                description,
                localized_descriptions,
                body: body.trim().to_string(),
                source_sha256: String::new(),
                source_bytes: 0,
                // Filled in by `discover` after parse succeeds; default to an
                // empty path so direct constructors (e.g. tests) compile.
                path: PathBuf::new(),
            });
        }

        // Graceful degradation: no frontmatter fence found.
        // Extract the first `# Heading` as the skill name.
        let heading_re = regex::Regex::new(r"(?m)^#\s+(.+)$").expect("static regex is valid");
        let name = heading_re
            .captures(content)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "没有 frontmatter，也没有可用作技能名的 `# 标题`".to_string())?;

        Ok(Skill {
            name,
            description: String::new(),
            localized_descriptions: HashMap::new(),
            body: content.trim().to_string(),
            source_sha256: String::new(),
            source_bytes: 0,
            path: PathBuf::new(),
        })
    }

    /// Lookup a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        let normalized = normalize_skill_name_for_lookup(name);
        self.skills.iter().find(|s| s.name == normalized)
    }

    /// Lookup only the exact model-visible name. This is the production load
    /// contract; convenience normalization remains limited to management code.
    #[must_use]
    pub fn get_exact(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    /// Stable identity of the complete immutable discovery snapshot, including
    /// source provenance and warnings that explain fail-closed exclusions.
    #[must_use]
    pub fn snapshot_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        let mut skills = self.skills.iter().collect::<Vec<_>>();
        skills.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.path.cmp(&right.path))
        });
        for skill in skills {
            hash_frame(&mut hasher, skill.name.as_bytes());
            hash_frame(&mut hasher, stable_path(&skill.path).as_bytes());
            hash_frame(&mut hasher, skill.source_sha256.as_bytes());
            hash_frame(&mut hasher, &skill.source_bytes.to_le_bytes());
            hash_frame(&mut hasher, skill.description.as_bytes());
            let mut localized = skill.localized_descriptions.iter().collect::<Vec<_>>();
            localized.sort_by_key(|(locale, _)| *locale);
            for (locale, description) in localized {
                hash_frame(&mut hasher, locale.as_bytes());
                hash_frame(&mut hasher, description.as_bytes());
            }
        }
        let mut warnings = self.warnings.iter().collect::<Vec<_>>();
        warnings.sort();
        for warning in warnings {
            hash_frame(&mut hasher, warning.as_bytes());
        }
        let mut ambiguous = self.ambiguous_names.iter().collect::<Vec<_>>();
        ambiguous.sort();
        for name in ambiguous {
            hash_frame(&mut hasher, name.as_bytes());
        }
        prefixed_hex(&hasher.finalize())
    }

    /// Return all loaded skills.
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// Parse or I/O warnings encountered while discovering skills.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Check whether any skills were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Return the number of loaded skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }
}

fn is_valid_skill_name(name: &str) -> bool {
    let char_count = name.chars().count();
    char_count > 0
        && char_count <= MAX_SKILL_NAME_CHARS
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn normalize_skill_name_for_lookup(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;

    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() && out.len() < MAX_SKILL_NAME_CHARS {
                out.push('-');
            }
            pending_dash = false;
            if out.len() < MAX_SKILL_NAME_CHARS {
                out.push(ch.to_ascii_lowercase());
            }
        } else {
            pending_dash = true;
        }

        if out.len() >= MAX_SKILL_NAME_CHARS {
            break;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        "skill".to_string()
    } else {
        out
    }
}

/// Resolve every candidate skills directory for a workspace, in
/// precedence order — most specific first. Used for session-time
/// skill discovery so the model sees skills that originated in
/// other AI-tool conventions installed in the same workspace
/// (#432).
///
/// Precedence (first match wins on name conflicts):
///
/// 1. `<workspace>/.agents/skills` — deepseek-native convention.
/// 2. `<workspace>/skills` — flat, project-local.
/// 3. `<workspace>/.opencode/skills` — OpenCode interop.
/// 4. `<workspace>/.claude/skills` — Claude Code interop.
/// 5. `<workspace>/.cursor/skills` — Cursor interop.
/// 6. `<workspace>/.dse/skills` — DSE workspace skills.
/// 7. [`agents_global_skills_dir`] — agentskills.io global.
/// 8. `~/.claude/skills` — Claude-ecosystem global (#902).
/// 9. `~/.dse/skills` — DSE global, primary install target.
///
/// Only directories that exist on disk are returned — callers don't
/// need to filter further. Returns an empty vec when nothing is
/// installed (the system-prompt skills block is then suppressed).
#[must_use]
pub fn skills_directories_for_mode(workspace: &Path, mode: SkillDiscoveryMode) -> Vec<PathBuf> {
    let home = dirs::home_dir();
    skills_directories_with_home_and_mode(workspace, home.as_deref(), mode)
}

fn skills_directories_with_home_and_mode(
    workspace: &Path,
    home_dir: Option<&Path>,
    mode: SkillDiscoveryMode,
) -> Vec<PathBuf> {
    let mut candidates = match mode {
        SkillDiscoveryMode::Compatible => vec![
            workspace.join(".agents").join("skills"),
            workspace.join("skills"),
            workspace.join(".opencode").join("skills"),
            workspace.join(".claude").join("skills"),
            workspace.join(".cursor").join("skills"),
            workspace.join(".dse").join("skills"),
        ],
        SkillDiscoveryMode::DseOnly => dse_workspace_skills_dir(workspace).into_iter().collect(),
    };
    if let Some(home) = home_dir {
        match mode {
            SkillDiscoveryMode::Compatible => {
                candidates.push(home.join(".agents").join("skills"));
                candidates.push(home.join(".claude").join("skills"));
                candidates.push(home.join(".dse").join("skills"));
            }
            SkillDiscoveryMode::DseOnly => {
                candidates.push(home.join(".dse").join("skills"));
            }
        }
    } else {
        candidates.push(PathBuf::from("/tmp/dse/skills"));
    }
    existing_skill_dirs(candidates)
}

pub fn dse_workspace_skills_dir(workspace: &Path) -> Option<PathBuf> {
    let skills_dir = workspace.join(".dse").join("skills");
    let canonical_workspace = fs::canonicalize(workspace).ok()?;
    let canonical_skills = fs::canonicalize(&skills_dir).ok()?;
    (canonical_skills.is_dir() && canonical_skills.starts_with(canonical_workspace))
        .then_some(skills_dir)
}

fn existing_skill_dirs(candidates: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in candidates {
        let Ok(canonical_path) = fs::canonicalize(&path) else {
            continue;
        };
        if canonical_path.is_dir() && seen.insert(canonical_path) {
            out.push(path);
        }
    }
    out
}

/// Walk every candidate skills directory for a workspace and merge
/// the discovered skills into a single registry. Name conflicts are
/// resolved with first-match-wins precedence per
/// [`skills_directories_for_mode`].
///
/// Warnings from each scanned directory accumulate so the model
/// (and the user via `/skill list`) can see why a skill didn't
/// load.
#[must_use]
pub fn discover_in_workspace_with_mode(
    workspace: &Path,
    mode: SkillDiscoveryMode,
) -> SkillRegistry {
    discover_from_directories(skills_directories_for_mode(workspace, mode))
}

/// Discover skills from the workspace search set plus the configured install
/// directory. Workspace-local directories keep their normal precedence; a
/// custom configured directory is inserted before global defaults when it is
/// outside that set so explicit configuration cannot be buried by large global
/// libraries.
#[must_use]
pub fn discover_for_workspace_and_dir_with_mode(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> SkillRegistry {
    let dirs = skill_directories_for_workspace_and_dir(workspace, skills_dir, mode);
    discover_from_directories(dirs)
}

#[must_use]
pub fn skill_directories_for_workspace_and_dir(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> Vec<PathBuf> {
    let mut dirs = skills_directories_for_mode(workspace, mode);
    insert_configured_skills_dir(&mut dirs, workspace, skills_dir);
    dirs
}

fn insert_configured_skills_dir(dirs: &mut Vec<PathBuf>, workspace: &Path, skills_dir: &Path) {
    if !skills_dir.is_dir() || dirs.iter().any(|p| paths_refer_to_same_dir(p, skills_dir)) {
        return;
    }

    let workspace_root = fs::canonicalize(workspace).ok();
    let insert_at = workspace_root
        .as_ref()
        .and_then(|root| {
            dirs.iter()
                .position(|dir| fs::canonicalize(dir).map_or(true, |dir| !dir.starts_with(root)))
        })
        .unwrap_or(dirs.len());
    dirs.insert(insert_at, skills_dir.to_path_buf());
}

fn paths_refer_to_same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub fn discover_from_directories(dirs: impl IntoIterator<Item = PathBuf>) -> SkillRegistry {
    let mut merged = SkillRegistry::default();
    for dir in dirs {
        let registry = SkillRegistry::discover(&dir);
        for name in &registry.ambiguous_names {
            if merged.skills.iter().all(|skill| skill.name != *name) {
                merged.ambiguous_names.insert(name.clone());
            }
        }
        for skill in registry.skills {
            if merged.ambiguous_names.contains(&skill.name) {
                merged.push_warning(format!(
                    "技能 `{}`（{}）被更高优先级的歧义定义阻止。",
                    skill.name,
                    skill.path.display()
                ));
            } else if let Some(existing) = merged.skills.iter().find(|s| s.name == skill.name) {
                merged.push_warning(format!(
                    "技能 `{}`（{}）被同名技能 {} 覆盖。",
                    skill.name,
                    skill.path.display(),
                    existing.path.display()
                ));
            } else {
                merged.skills.push(skill);
            }
        }
        for warning in registry.warnings {
            merged.warnings.push(warning);
        }
    }
    merged
}

/// Render the system-prompt skills block from every workspace
/// candidate directory plus the global default (#432). Wraps
/// [`discover_in_workspace_with_mode`] for callers (e.g. `prompts.rs`) that
/// only have the workspace path to hand.
#[must_use]
pub fn render_available_skills_context_for_workspace_with_mode(
    workspace: &Path,
    mode: SkillDiscoveryMode,
) -> Option<String> {
    let registry = discover_in_workspace_with_mode(workspace, mode);
    render_available_skills_context(&registry)
}

#[must_use]
pub fn render_available_skills_context_for_workspace_and_dir_with_mode(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> Option<String> {
    let registry = discover_for_workspace_and_dir_with_mode(workspace, skills_dir, mode);
    render_available_skills_context(&registry)
}

pub fn render_available_skills_context(registry: &SkillRegistry) -> Option<String> {
    if registry.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str("## 技能\n");
    out.push_str(
        "技能是 Host 在本次运行开始时发现并冻结的本地操作说明。下面只列出本次运行\
真实可加载的精确名称和说明；需要正文时调用 `load_skill`。\n\n",
    );
    out.push_str("### 可用技能\n");

    let mut omitted = 0usize;
    for skill in registry.list() {
        let description = truncate_for_prompt(
            skill.description_for_locale("zh-Hans"),
            MAX_SKILL_DESCRIPTION_CHARS,
        );
        let line = if description.is_empty() {
            format!("- {}\n", skill.name)
        } else {
            format!("- {}: {}\n", skill.name, description)
        };

        if out.chars().count() + line.chars().count() > MAX_AVAILABLE_SKILLS_CHARS {
            omitted += 1;
        } else {
            out.push_str(&line);
        }
    }

    if omitted > 0 {
        out.push_str(&format!(
            "- … 另有 {omitted} 个技能因提示词预算限制而省略。\n"
        ));
    }

    out.push_str(
        "\n### 使用规则\n\
- 任务匹配时，用目录中显示的精确名称调用 `load_skill`；不要用 `read_file` 猜测 Skill 路径。\n\
- 用户点名技能（`$SkillName` 或自然语言）或任务明显匹配描述时使用；下一轮未再次提及时不要自动沿用。\n\
- `load_skill` 拒绝名称时，说明该 Skill 本次不可用，并使用最佳替代方案继续；不要猜测别名或路径。\n\
- 未经用户明确要求或信任，不要执行社区技能附带的脚本。\n",
    );

    Some(out)
}

fn stable_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn hash_frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(bytes);
}

fn sha256(bytes: &[u8]) -> String {
    prefixed_hex(&Sha256::digest(bytes))
}

fn prefixed_hex(bytes: &[u8]) -> String {
    let digest = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let single_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        return single_line;
    }

    let mut truncated = single_line
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, directory: &str, name: &str, body: &str) -> PathBuf {
        let skill_dir = root.join(directory);
        fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        fs::write(
            &path,
            format!("---\nname: {name}\ndescription: Exact test skill\n---\n\n{body}\n"),
        )
        .unwrap();
        path
    }

    #[test]
    fn exact_lookup_and_render_hide_paths_and_require_load_tool() {
        let temp = tempfile::tempdir().unwrap();
        write_skill(temp.path(), "one", "exact-skill", "Do the exact work.");
        let registry = SkillRegistry::discover(temp.path());

        assert!(registry.get_exact("exact-skill").is_some());
        assert!(registry.get_exact("Exact Skill").is_none());
        assert!(registry.get("Exact Skill").is_some());
        let rendered = render_available_skills_context(&registry).unwrap();
        assert!(rendered.contains("- exact-skill: Exact test skill"));
        assert!(rendered.contains("`load_skill`"));
        assert!(!rendered.contains(&temp.path().display().to_string()));
        assert!(!rendered.contains("`/skill"));
        assert!(rendered.contains("不要用 `read_file` 猜测"));
    }

    #[test]
    fn oversized_and_non_utf8_sources_are_not_advertised() {
        let oversized = tempfile::tempdir().unwrap();
        let oversized_dir = oversized.path().join("large");
        fs::create_dir_all(&oversized_dir).unwrap();
        fs::write(
            oversized_dir.join("SKILL.md"),
            vec![b'x'; usize::try_from(MAX_SKILL_SOURCE_BYTES + 1).unwrap()],
        )
        .unwrap();
        let invalid = oversized.path().join("invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join("SKILL.md"), [0xff, 0xfe, 0xfd]).unwrap();

        let registry = SkillRegistry::discover(oversized.path());
        assert!(registry.is_empty());
        assert_eq!(registry.warnings().len(), 2);
        assert!(
            registry
                .warnings()
                .iter()
                .any(|warning| warning.contains("超过上限"))
        );
        assert!(
            registry
                .warnings()
                .iter()
                .any(|warning| warning.contains("UTF-8"))
        );
    }

    #[test]
    fn normalized_collision_fails_closed_and_blocks_lower_precedence() {
        let high = tempfile::tempdir().unwrap();
        let low = tempfile::tempdir().unwrap();
        write_skill(high.path(), "a", "My Skill", "first");
        write_skill(high.path(), "b", "my_skill", "second");
        write_skill(low.path(), "only", "my-skill", "lower");

        let registry =
            discover_from_directories([high.path().to_path_buf(), low.path().to_path_buf()]);
        assert!(registry.get_exact("my-skill").is_none());
        assert!(registry.is_empty());
        assert!(
            registry
                .warnings()
                .iter()
                .any(|warning| warning.contains("存在歧义"))
        );
        assert!(
            registry
                .warnings()
                .iter()
                .any(|warning| warning.contains("更高优先级的歧义定义"))
        );
    }

    #[test]
    fn admitted_snapshot_is_complete_immutable_and_change_sensitive() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(temp.path(), "stable", "stable-skill", "first body");
        let first = SkillRegistry::discover(temp.path());
        let first_hash = first.snapshot_sha256();
        let admitted = first.get_exact("stable-skill").unwrap();
        assert_eq!(admitted.body, "first body");
        assert!(!admitted.source_sha256.is_empty());
        assert!(admitted.source_bytes <= MAX_SKILL_SOURCE_BYTES);

        fs::write(
            path,
            "---\nname: stable-skill\ndescription: Exact test skill\n---\n\nsecond body\n",
        )
        .unwrap();
        let second = SkillRegistry::discover(temp.path());
        assert_eq!(first.get_exact("stable-skill").unwrap().body, "first body");
        assert_eq!(
            second.get_exact("stable-skill").unwrap().body,
            "second body"
        );
        assert_ne!(first_hash, second.snapshot_sha256());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_and_source_cannot_escape_discovery_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_skill(outside.path(), "escaped", "escaped-skill", "secret");
        symlink(
            outside.path().join("escaped"),
            root.path().join("escaped-dir"),
        )
        .unwrap();

        let inside = root.path().join("inside");
        fs::create_dir_all(&inside).unwrap();
        symlink(
            outside.path().join("escaped/SKILL.md"),
            inside.join("SKILL.md"),
        )
        .unwrap();

        let registry = SkillRegistry::discover(root.path());
        assert!(registry.is_empty());
        assert!(registry.warnings().len() >= 2);
        assert!(
            registry
                .warnings()
                .iter()
                .all(|warning| warning.contains("之外"))
        );
    }
}
