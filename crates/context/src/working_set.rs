//! Deterministic, budgeted repository-region selection.
//!
//! The selector is a read-only ContextBroker observation. It derives a compact
//! map from the current workspace and canonical TaskDefinition, but it does
//! not become durable state: callers persist only the exact rendered prompt in
//! the canonical RunRequest. It never calls a model, mutates the workspace, or
//! replaces the canonical search/read tools.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use codewhale_protocol::task::TaskDefinition;
use ignore::{DirEntry, WalkBuilder};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

const POLICY_VERSION: &str = "m10b-working-set-v1";
const MIN_REGION_SCORE: u32 = 120;
const CONTEXT_RADIUS_LINES: usize = 8;
const DEFAULT_REGION_LINES: usize = 24;

static PATH_WITH_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (?P<path>
            (?:[\p{L}\p{N}_@.+-]+/)*
            [\p{L}\p{N}_@.+-]+\.[\p{L}\p{N}_+-]+
        )
        (?::(?P<line>[1-9][0-9]{0,8}))?
        ",
    )
    .expect("working-set path regex is valid")
});
static TERM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{L}_][\p{L}\p{N}_-]{2,}").expect("working-set term regex is valid")
});
static IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        (?:
          (?:from|import|mod|use|require)\s*[\(\s]*
          |(?:src|href)\s*=\s*
        )
        ["']?(?P<target>[.:/\p{L}\p{N}_@+-]+)["']?
        "#,
    )
    .expect("working-set import regex is valid")
});

const STOP_TERMS: &[&str] = &[
    "after", "before", "change", "code", "file", "fix", "from", "into", "issue", "make", "must",
    "only", "should", "src", "task", "test", "tests", "that", "the", "then", "this", "with",
    "修改", "修复", "任务", "文件", "代码", "测试", "需要", "应该", "确保",
];

const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "go.sum",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Makefile",
];

const IGNORED_COMPONENTS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".codewhale-worktrees",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
];

/// Hard limits for one working-set observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkingSetBudget {
    pub max_regions: usize,
    pub max_total_lines: usize,
    pub max_region_lines: usize,
    pub max_scanned_files: usize,
    pub max_scanned_bytes: usize,
    pub max_file_bytes: usize,
    pub max_rendered_chars: usize,
}

impl Default for WorkingSetBudget {
    fn default() -> Self {
        Self {
            max_regions: 8,
            max_total_lines: 160,
            max_region_lines: 32,
            max_scanned_files: 4_096,
            max_scanned_bytes: 16 * 1024 * 1024,
            max_file_bytes: 512 * 1024,
            max_rendered_chars: 6_000,
        }
    }
}

impl WorkingSetBudget {
    fn validate(self) -> Result<Self, WorkingSetError> {
        if self.max_regions == 0
            || self.max_total_lines == 0
            || self.max_region_lines == 0
            || self.max_scanned_files == 0
            || self.max_scanned_bytes == 0
            || self.max_file_bytes == 0
            || self.max_rendered_chars < 256
            || self.max_region_lines > self.max_total_lines
        {
            return Err(WorkingSetError::InvalidBudget);
        }
        Ok(self)
    }
}

/// Borrowed inputs for one deterministic selection.
#[derive(Debug, Clone, Copy)]
pub struct WorkingSetRequest<'a> {
    pub workspace: &'a Path,
    pub task: &'a TaskDefinition,
    /// Optional Host-observed Git/status paths. The selector treats these as
    /// evidence but does not run a second Git implementation itself.
    pub changed_paths: &'a [String],
    pub budget: WorkingSetBudget,
}

/// Stable reason codes used by offline metrics and model-visible projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSetReason {
    ExplicitPath,
    StackTrace,
    ChangedPath,
    SymbolMatch,
    PathTerm,
    ContentTerm,
    ManifestEntry,
    TestAdjacency,
    ImportAdjacency,
}

/// One ranked, freshness-bound repository region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkingSetRegion {
    pub rank: usize,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: u32,
    pub reasons: Vec<WorkingSetReason>,
    pub evidence: Vec<String>,
    pub sha256: String,
    pub expand_hint: String,
}

/// Complete bounded selector output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkingSetProjection {
    pub policy_version: String,
    pub task_sha256: String,
    pub observation_sha256: String,
    pub scanned_files: usize,
    pub scanned_bytes: usize,
    pub scan_truncated: bool,
    pub regions: Vec<WorkingSetRegion>,
}

impl WorkingSetProjection {
    /// Render the compact model-visible map. It contains no source bytes.
    #[must_use]
    pub fn render_prompt_block(&self, max_chars: usize) -> Option<String> {
        if self.regions.is_empty() || max_chars < 256 {
            return None;
        }
        let mut rendered = format!(
            "## Host 预算化工作集\n\n\
             这是按当前 TaskContract 和工作区确定性派生的候选地图，不是完成证据。\
             修改前仍须读取文件；digest 只绑定本次观察。\n\
             policy={} task={} observation={}\n",
            self.policy_version, self.task_sha256, self.observation_sha256
        );
        let mut emitted = 0_usize;
        for region in &self.regions {
            let reasons = region
                .reasons
                .iter()
                .map(|reason| {
                    serde_json::to_string(reason)
                        .expect("working-set reason serializes")
                        .trim_matches('"')
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join(",");
            let evidence = region.evidence.join(",");
            let line = format!(
                "\n{}. path={} range={}-{} score={} reason=[{}] evidence=[{}] \
                 digest={} expand={}",
                region.rank,
                region.path,
                region.start_line,
                region.end_line,
                region.score,
                reasons,
                evidence,
                region.sha256,
                region.expand_hint
            );
            if rendered.len().saturating_add(line.len()) > max_chars {
                break;
            }
            rendered.push_str(&line);
            emitted += 1;
        }
        if emitted == 0 { None } else { Some(rendered) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkingSetError {
    #[error("working-set workspace is not a canonical directory")]
    InvalidWorkspace,
    #[error("working-set budget is invalid")]
    InvalidBudget,
    #[error("working-set task is invalid: {0}")]
    InvalidTask(String),
    #[error("working-set observation digest failed: {0}")]
    Digest(String),
}

#[derive(Debug)]
struct QueryFacts {
    task_text: String,
    task_sha256: String,
    path_hints: BTreeMap<String, Option<usize>>,
    terms: Vec<String>,
    manifest_requested: bool,
    changed_paths: BTreeSet<String>,
}

#[derive(Debug)]
struct FileObservation {
    path: String,
    content: String,
    sha256: String,
    lines: usize,
}

#[derive(Debug, Default)]
struct Candidate {
    score: u32,
    line: Option<usize>,
    reasons: BTreeSet<WorkingSetReason>,
    evidence: BTreeSet<String>,
}

/// Select a deterministic, bounded set of repository regions.
pub fn select_working_set(
    request: WorkingSetRequest<'_>,
) -> Result<WorkingSetProjection, WorkingSetError> {
    request
        .task
        .validate()
        .map_err(WorkingSetError::InvalidTask)?;
    let budget = request.budget.validate()?;
    let workspace = request
        .workspace
        .canonicalize()
        .map_err(|_| WorkingSetError::InvalidWorkspace)?;
    if !workspace.is_dir() {
        return Err(WorkingSetError::InvalidWorkspace);
    }

    let query = query_facts(request.task, request.changed_paths);
    let (files, scanned_bytes, scan_truncated) = scan_workspace(&workspace, budget);
    let mut candidates = BTreeMap::<String, Candidate>::new();

    for file in &files {
        score_file(file, &query, &mut candidates);
    }
    apply_test_adjacency(&files, &mut candidates);
    apply_import_adjacency(&files, &mut candidates);

    let mut ranked = candidates
        .into_iter()
        .filter(|(_, candidate)| candidate.score >= MIN_REGION_SCORE)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(path, candidate)| (Reverse(candidate.score), path.clone()));

    let by_path = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    let mut regions = Vec::new();
    let mut remaining_lines = budget.max_total_lines;
    for (path, candidate) in ranked {
        if regions.len() >= budget.max_regions || remaining_lines == 0 {
            break;
        }
        let Some(file) = by_path.get(path.as_str()) else {
            continue;
        };
        let (start_line, end_line) = region_range(
            candidate.line,
            file.lines,
            budget.max_region_lines.min(remaining_lines),
        );
        if end_line < start_line {
            continue;
        }
        remaining_lines = remaining_lines.saturating_sub(end_line - start_line + 1);
        let rank = regions.len() + 1;
        regions.push(WorkingSetRegion {
            rank,
            path: path.clone(),
            start_line,
            end_line,
            score: candidate.score,
            reasons: candidate.reasons.into_iter().collect(),
            evidence: candidate.evidence.into_iter().collect(),
            sha256: file.sha256.clone(),
            expand_hint: format!(
                "read_file path={path} start_line={start_line} end_line={end_line}"
            ),
        });
    }

    let observation_sha256 = projection_digest(
        &query.task_sha256,
        files.len(),
        scanned_bytes,
        scan_truncated,
        &regions,
    )?;
    Ok(WorkingSetProjection {
        policy_version: POLICY_VERSION.to_owned(),
        task_sha256: query.task_sha256,
        observation_sha256,
        scanned_files: files.len(),
        scanned_bytes,
        scan_truncated,
        regions,
    })
}

fn query_facts(task: &TaskDefinition, changed_paths: &[String]) -> QueryFacts {
    let task_text = task.model_message();
    let task_sha256 = sha256(task_text.as_bytes());
    let mut path_hints = BTreeMap::new();
    for captures in PATH_WITH_LINE_RE.captures_iter(&task_text) {
        let Some(path) = captures.name("path") else {
            continue;
        };
        if let Some(normalized) = normalize_relative(path.as_str()) {
            let line = captures
                .name("line")
                .and_then(|value| value.as_str().parse::<usize>().ok());
            path_hints.insert(normalized, line);
        }
    }

    let term_text = PATH_WITH_LINE_RE.replace_all(&task_text, " ");
    let mut terms = TERM_RE
        .find_iter(&term_text)
        .map(|value| value.as_str().to_lowercase())
        .filter(|term| term.chars().count() >= 3 && !STOP_TERMS.contains(&term.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| (Reverse(term.chars().count()), term.clone()));
    terms.truncate(24);

    let lowered = task_text.to_lowercase();
    let manifest_requested = [
        "cargo",
        "dependency",
        "dependencies",
        "feature",
        "manifest",
        "package",
        "workspace",
        "构建",
        "依赖",
        "工作区",
        "清单",
    ]
    .iter()
    .any(|term| lowered.contains(term));
    let changed_paths = changed_paths
        .iter()
        .filter_map(|path| normalize_relative(path))
        .collect();
    QueryFacts {
        task_text,
        task_sha256,
        path_hints,
        terms,
        manifest_requested,
        changed_paths,
    }
}

fn scan_workspace(
    workspace: &Path,
    budget: WorkingSetBudget,
) -> (Vec<FileObservation>, usize, bool) {
    let mut builder = WalkBuilder::new(workspace);
    builder
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .sort_by_file_path(|left, right| left.cmp(right))
        .filter_entry(allowed_entry);

    let mut files = Vec::new();
    let mut scanned_bytes = 0_usize;
    let mut scan_truncated = false;
    for result in builder.build() {
        let Ok(entry) = result else {
            continue;
        };
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if files.len() >= budget.max_scanned_files || scanned_bytes >= budget.max_scanned_bytes {
            scan_truncated = true;
            break;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(size) = usize::try_from(metadata.len()) else {
            continue;
        };
        if size == 0
            || size > budget.max_file_bytes
            || scanned_bytes.saturating_add(size) > budget.max_scanned_bytes
        {
            continue;
        }
        let Ok(mut file) = crate::safe_read::open_no_follow(entry.path()) else {
            continue;
        };
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let mut bytes = Vec::with_capacity(size);
        if file
            .by_ref()
            .take(
                u64::try_from(budget.max_file_bytes)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() != size
        {
            continue;
        }
        if bytes.contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let Some(path) = entry
            .path()
            .strip_prefix(workspace)
            .ok()
            .and_then(normalize_path)
        else {
            continue;
        };
        scanned_bytes = scanned_bytes.saturating_add(content.len());
        files.push(FileObservation {
            path,
            sha256: sha256(content.as_bytes()),
            lines: content.lines().count().max(1),
            content,
        });
    }
    (files, scanned_bytes, scan_truncated)
}

fn allowed_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    !entry.path().components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();
        IGNORED_COMPONENTS.contains(&name.as_ref())
    })
}

fn score_file(
    file: &FileObservation,
    query: &QueryFacts,
    candidates: &mut BTreeMap<String, Candidate>,
) {
    let path_lower = file.path.to_lowercase();
    let basename = Path::new(&file.path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(file.path.as_str())
        .to_lowercase();
    let stem = Path::new(&file.path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut candidate = Candidate::default();

    for (hint, line) in &query.path_hints {
        let hint_lower = hint.to_lowercase();
        if path_lower == hint_lower || path_lower.ends_with(&format!("/{hint_lower}")) {
            candidate.score += 1_000;
            candidate.line = *line;
            candidate.reasons.insert(if line.is_some() {
                WorkingSetReason::StackTrace
            } else {
                WorkingSetReason::ExplicitPath
            });
            candidate.evidence.insert(format!(
                "{}:{}",
                if line.is_some() {
                    "stack_path"
                } else {
                    "task_path"
                },
                hint
            ));
        } else if basename == hint_lower {
            candidate.score += 700;
            candidate.reasons.insert(WorkingSetReason::ExplicitPath);
            candidate.evidence.insert(format!("task_basename:{hint}"));
        }
    }

    if query.changed_paths.contains(&file.path) {
        candidate.score += 650;
        candidate.reasons.insert(WorkingSetReason::ChangedPath);
        candidate
            .evidence
            .insert(format!("changed_path:{}", file.path));
    }

    let content_lower = file.content.to_lowercase();
    for term in &query.terms {
        let path_match = path_lower
            .split(['/', '.', '_', '-'])
            .any(|part| part == term)
            || basename == *term
            || stem == *term;
        if path_match {
            candidate.score += if stem == *term { 260 } else { 140 };
            candidate.reasons.insert(WorkingSetReason::PathTerm);
            candidate.evidence.insert(format!("path_term:{term}"));
        }
        if let Some(byte_index) = content_lower.find(term) {
            let strong_symbol = term.contains('_')
                || query.task_text.contains(term) && term.chars().any(char::is_uppercase)
                || (!term.is_ascii() && term.chars().count() >= 4);
            candidate.score += if strong_symbol { 220 } else { 55 };
            candidate.line = candidate
                .line
                .or_else(|| Some(line_number_at(&file.content, byte_index)));
            candidate.reasons.insert(if strong_symbol {
                WorkingSetReason::SymbolMatch
            } else {
                WorkingSetReason::ContentTerm
            });
            candidate.evidence.insert(format!("content_term:{term}"));
        }
    }

    if query.manifest_requested
        && MANIFEST_NAMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&basename))
    {
        candidate.score += 320;
        candidate.reasons.insert(WorkingSetReason::ManifestEntry);
        candidate.evidence.insert(format!("manifest:{}", file.path));
    }

    if candidate.score > 0 {
        candidates.insert(file.path.clone(), candidate);
    }
}

fn apply_test_adjacency(files: &[FileObservation], candidates: &mut BTreeMap<String, Candidate>) {
    let scored = candidates
        .iter()
        .filter(|(_, candidate)| candidate.score >= MIN_REGION_SCORE)
        .map(|(path, candidate)| (path.clone(), candidate.score))
        .collect::<Vec<_>>();
    for (path, source_score) in scored {
        let source_stem = normalized_test_stem(&path);
        if source_stem.is_empty() {
            continue;
        }
        for file in files {
            if file.path == path || normalized_test_stem(&file.path) != source_stem {
                continue;
            }
            if !(is_test_path(&path) || is_test_path(&file.path)) {
                continue;
            }
            let candidate = candidates.entry(file.path.clone()).or_default();
            candidate.score = candidate
                .score
                .max(source_score.saturating_sub(40).max(180));
            candidate.reasons.insert(WorkingSetReason::TestAdjacency);
            candidate.evidence.insert(format!("adjacent_to:{path}"));
        }
    }
}

fn apply_import_adjacency(files: &[FileObservation], candidates: &mut BTreeMap<String, Candidate>) {
    let by_suffix = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<Vec<_>>();
    let seeds = candidates
        .iter()
        .filter(|(_, candidate)| candidate.score >= 220)
        .map(|(path, candidate)| (path.clone(), candidate.score))
        .collect::<Vec<_>>();
    for (seed_path, seed_score) in seeds {
        let Some(seed) = files.iter().find(|file| file.path == seed_path) else {
            continue;
        };
        for captures in IMPORT_RE.captures_iter(&seed.content) {
            let Some(target) = captures.name("target") else {
                continue;
            };
            let target = target.as_str().trim_matches('.');
            if target.len() < 3 {
                continue;
            }
            let target_path = target.replace("::", "/").replace('.', "/");
            let target_stem = Path::new(&target_path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            for (path, _) in &by_suffix {
                let stem = Path::new(path)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                if stem != target_stem || *path == seed_path {
                    continue;
                }
                let candidate = candidates.entry((*path).to_owned()).or_default();
                candidate.score = candidate.score.max(seed_score.saturating_sub(80).max(160));
                candidate.reasons.insert(WorkingSetReason::ImportAdjacency);
                candidate
                    .evidence
                    .insert(format!("imported_from:{seed_path}"));
            }
        }
    }
}

fn normalized_test_stem(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    stem.strip_prefix("test_")
        .or_else(|| stem.strip_suffix("_test"))
        .or_else(|| stem.strip_suffix(".test"))
        .unwrap_or(stem.as_str())
        .to_owned()
}

fn is_test_path(path: &str) -> bool {
    let lowered = path.to_lowercase();
    lowered.contains("/test")
        || lowered.starts_with("test")
        || lowered.contains(".test.")
        || lowered.contains("_test.")
        || lowered.contains("test_")
}

fn region_range(line: Option<usize>, file_lines: usize, max_lines: usize) -> (usize, usize) {
    if file_lines == 0 || max_lines == 0 {
        return (1, 0);
    }
    let max_lines = max_lines.min(file_lines);
    let Some(line) = line.map(|value| value.clamp(1, file_lines)) else {
        return (1, DEFAULT_REGION_LINES.min(max_lines));
    };
    let mut start = line.saturating_sub(CONTEXT_RADIUS_LINES).max(1);
    let mut end = (line + CONTEXT_RADIUS_LINES).min(file_lines);
    if end - start + 1 > max_lines {
        end = start + max_lines - 1;
    } else if end - start + 1 < max_lines {
        start = end.saturating_sub(max_lines - 1).max(1);
    }
    (start, end)
}

fn line_number_at(content: &str, byte_index: usize) -> usize {
    content[..byte_index.min(content.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn normalize_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.is_empty() {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn normalize_relative(raw: &str) -> Option<String> {
    let normalized = raw.replace('\\', "/");
    let path = PathBuf::from(normalized.trim_matches(['`', '"', '\'', '(', ')', '[', ']']));
    if path.is_absolute() {
        return None;
    }
    normalize_path(&path)
}

fn projection_digest(
    task_sha256: &str,
    scanned_files: usize,
    scanned_bytes: usize,
    scan_truncated: bool,
    regions: &[WorkingSetRegion],
) -> Result<String, WorkingSetError> {
    #[derive(Serialize)]
    struct DigestInput<'a> {
        policy_version: &'static str,
        task_sha256: &'a str,
        scanned_files: usize,
        scanned_bytes: usize,
        scan_truncated: bool,
        regions: &'a [WorkingSetRegion],
    }
    serde_json::to_vec(&DigestInput {
        policy_version: POLICY_VERSION,
        task_sha256,
        scanned_files,
        scanned_bytes,
        scan_truncated,
        regions,
    })
    .map(|bytes| sha256(&bytes))
    .map_err(|error| WorkingSetError::Digest(error.to_string()))
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut rendered = String::with_capacity("sha256:".len() + digest.len() * 2);
    rendered.push_str("sha256:");
    for byte in digest {
        write!(&mut rendered, "{byte:02x}").expect("writing to String cannot fail");
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn task(objective: &str) -> TaskDefinition {
        TaskDefinition::host(objective)
    }

    fn write(workspace: &Path, path: &str, content: &str) {
        let path = workspace.join(path);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, content).expect("write fixture");
    }

    #[test]
    fn explicit_path_symbol_and_test_adjacency_are_ranked_deterministically() {
        let workspace = tempfile::tempdir().expect("workspace");
        write(
            workspace.path(),
            "src/retry/backoff.rs",
            "pub fn bounded_retry_delay(attempt: u32) -> u64 {\n    attempt as u64\n}\n",
        );
        write(
            workspace.path(),
            "tests/backoff_test.rs",
            "use retry::backoff::bounded_retry_delay;\n",
        );
        write(
            workspace.path(),
            "src/unrelated.rs",
            "pub fn unrelated() {}\n",
        );
        let definition = task("修复 src/retry/backoff.rs:2 的 bounded_retry_delay，并更新对应测试");
        let request = || WorkingSetRequest {
            workspace: workspace.path(),
            task: &definition,
            changed_paths: &[],
            budget: WorkingSetBudget::default(),
        };
        let first = select_working_set(request()).expect("first selection");
        let second = select_working_set(request()).expect("second selection");
        assert_eq!(first, second);
        assert_eq!(first.regions[0].path, "src/retry/backoff.rs");
        assert!(
            first.regions[0]
                .reasons
                .contains(&WorkingSetReason::StackTrace)
        );
        assert!(
            first
                .regions
                .iter()
                .any(|region| region.path == "tests/backoff_test.rs")
        );
        assert!(
            first
                .render_prompt_block(WorkingSetBudget::default().max_rendered_chars)
                .expect("rendered map")
                .contains("digest=sha256:")
        );
    }

    #[test]
    fn changed_path_is_typed_evidence_and_unrelated_task_abstains() {
        let workspace = tempfile::tempdir().expect("workspace");
        write(
            workspace.path(),
            "src/config/tenant.rs",
            "pub fn tenant_policy() {}\n",
        );
        write(workspace.path(), "src/net.rs", "pub fn slow_start() {}\n");
        let changed = ["src/config/tenant.rs".to_owned()];
        let changed_task = task("调查最近未提交的 tenant policy 回归");
        let projection = select_working_set(WorkingSetRequest {
            workspace: workspace.path(),
            task: &changed_task,
            changed_paths: &changed,
            budget: WorkingSetBudget::default(),
        })
        .expect("changed selection");
        assert_eq!(projection.regions[0].path, "src/config/tenant.rs");
        assert!(
            projection.regions[0]
                .reasons
                .contains(&WorkingSetReason::ChangedPath)
        );

        let unrelated = task("只解释 TCP 慢启动原理，不调查或修改仓库");
        let abstained = select_working_set(WorkingSetRequest {
            workspace: workspace.path(),
            task: &unrelated,
            changed_paths: &[],
            budget: WorkingSetBudget::default(),
        })
        .expect("abstention");
        assert!(abstained.regions.is_empty());
    }

    #[test]
    fn traversal_hints_symlinks_binary_and_budget_overflow_do_not_escape() {
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        write(outside.path(), "secret.rs", "pub fn secret() {}\n");
        write(workspace.path(), "src/visible.rs", "pub fn visible() {}\n");
        fs::write(workspace.path().join("binary.bin"), b"\0secret").expect("binary");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            workspace.path().join("src/escape.rs"),
        )
        .expect("symlink");

        let definition = task("修复 ../secret.rs 和 visible");
        let projection = select_working_set(WorkingSetRequest {
            workspace: workspace.path(),
            task: &definition,
            changed_paths: &["../secret.rs".to_owned()],
            budget: WorkingSetBudget {
                max_regions: 1,
                max_total_lines: 1,
                max_region_lines: 1,
                ..WorkingSetBudget::default()
            },
        })
        .expect("bounded selection");
        assert_eq!(projection.regions.len(), 1);
        assert_eq!(projection.regions[0].path, "src/visible.rs");
        assert_eq!(
            (
                projection.regions[0].start_line,
                projection.regions[0].end_line
            ),
            (1, 1)
        );
        assert!(
            projection
                .regions
                .iter()
                .all(|region| !region.path.contains("secret"))
        );
    }
}
