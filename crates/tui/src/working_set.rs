//! Repo-aware workspace resolution and `@`-mention completion.
//!
//! This module resolves `@`-mentions and discovers completion candidates while
//! respecting workspace boundaries and repository ignore rules.

use crate::workspace_discovery::{
    DISCOVERY_ALWAYS_DIRS, path_is_excluded_from_discovery, should_skip_unignored_discovery_entry,
};
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Repo-aware resolver for `@`-mentions.
///
/// `cwd` is captured at construction; if the host's current directory changes
/// during a session, build a fresh `Workspace`. Fuzzy lookups are backed by a
/// lazy basename → paths index built once on first miss and reused for the
/// rest of the session — without it, every mis-typed mention triggered a full
/// `WalkBuilder` traversal up to the configured completion depth.
#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    cwd: Option<PathBuf>,
    file_index: OnceLock<HashMap<String, Vec<PathBuf>>>,
    completion_walk_depth: Option<usize>,
    /// Follow symbolic links during file discovery walks. When `true`,
    /// symlinked directories are traversed, enabling multi-project workspaces
    /// where project directories are symlinked into a hub directory.
    follow_links: bool,
}

struct SearchContext<'a> {
    needle: &'a str,
    limit: usize,
    prefix_hits: &'a mut Vec<String>,
    substring_hits: &'a mut Vec<String>,
    seen: &'a mut HashSet<PathBuf>,
}

impl SearchContext<'_> {
    fn is_full(&self) -> bool {
        self.prefix_hits.len() + self.substring_hits.len() >= self.limit
    }

    fn remember(&mut self, path: PathBuf) -> bool {
        self.seen.insert(path)
    }

    fn push_match(&mut self, candidate: String) {
        let lower = candidate.to_lowercase();
        if self.needle.is_empty() || lower.starts_with(self.needle) {
            self.prefix_hits.push(candidate);
        } else if lower.contains(self.needle) {
            self.substring_hits.push(candidate);
        }
    }
}

impl Workspace {
    /// Construct with an explicit cwd. Used by tests that need deterministic
    /// resolution against a known directory without depending on (and
    /// mutating) the process's real working directory.
    pub fn with_cwd(root: PathBuf, cwd: Option<PathBuf>) -> Self {
        Self::with_cwd_and_depth(root, cwd, DEFAULT_COMPLETIONS_WALK_DEPTH)
    }

    /// Construct with an explicit completion walk depth. A depth of `0`
    /// disables the depth limit for users with deeply nested workspaces.
    pub fn with_cwd_and_depth(root: PathBuf, cwd: Option<PathBuf>, walk_depth: usize) -> Self {
        Self::with_cwd_depth_and_follow_links(root, cwd, walk_depth, false)
    }

    /// Construct with an explicit completion walk depth and symlink-following
    /// preference. See [`Workspace::follow_links`].
    pub fn with_cwd_depth_and_follow_links(
        root: PathBuf,
        cwd: Option<PathBuf>,
        walk_depth: usize,
        follow_links: bool,
    ) -> Self {
        Self {
            root,
            cwd,
            file_index: OnceLock::new(),
            completion_walk_depth: normalize_completion_walk_depth(walk_depth),
            follow_links,
        }
    }

    /// Two-pass resolution: workspace, then cwd, then fuzzy fallback.
    pub fn resolve(&self, raw_path: &str) -> Result<PathBuf, PathBuf> {
        let path = expand_mention_home(raw_path);
        if path.is_absolute() {
            if path.exists() {
                return Ok(path);
            }
            return Err(path);
        }

        let ws_path = self.root.join(&path);
        if ws_path.exists() {
            return Ok(ws_path);
        }

        if let Some(cwd) = self.cwd.as_ref() {
            let cwd_path = cwd.join(&path);
            if cwd_path.exists() {
                return Ok(cwd_path);
            }
        }

        if let Some(fuzzy) = self.fuzzy_resolve(&path) {
            return Ok(fuzzy);
        }

        Err(ws_path)
    }

    fn fuzzy_resolve(&self, path: &Path) -> Option<PathBuf> {
        let needle = path.file_name()?.to_string_lossy().to_lowercase();
        if needle.is_empty() {
            return None;
        }

        let index = self.file_index.get_or_init(|| self.build_file_index());
        index.get(&needle).and_then(|paths| paths.first()).cloned()
    }

    fn build_file_index(&self) -> HashMap<String, Vec<PathBuf>> {
        let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut total: usize = 0;
        let builder =
            discovery_walk_builder(&self.root, self.completion_walk_depth, self.follow_links);

        for entry in builder.build().flatten() {
            if total >= FILE_INDEX_MAX_ENTRIES {
                tracing::warn!(
                    target: "working_set",
                    limit = FILE_INDEX_MAX_ENTRIES,
                    "file-index discovery hit the entry cap; truncating to keep first-turn latency bounded (#697)"
                );
                return index;
            }
            if entry
                .file_type()
                .is_some_and(|ft| ft.is_file() || ft.is_dir())
            {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                index
                    .entry(name)
                    .or_default()
                    .push(entry.path().to_path_buf());
                total += 1;
            }
        }

        // Also index AI-tool dot-directories with gitignore disabled.
        for dir_name in DISCOVERY_ALWAYS_DIRS {
            if total >= FILE_INDEX_MAX_ENTRIES {
                break;
            }
            let dot_dir = self.root.join(dir_name);
            if !dot_dir.is_dir() {
                continue;
            }
            let mut dot_builder = WalkBuilder::new(&dot_dir);
            dot_builder
                .hidden(true)
                .follow_links(self.follow_links)
                .git_ignore(false)
                .ignore(false);
            if let Some(depth) = child_completion_walk_depth(self.completion_walk_depth) {
                dot_builder.max_depth(Some(depth));
            }
            for entry in dot_builder.build().flatten() {
                if total >= FILE_INDEX_MAX_ENTRIES {
                    break;
                }
                // Exclude machine-generated bulk (e.g. .deepseek/snapshots/).
                if path_is_excluded_from_discovery(&self.root, entry.path()) {
                    continue;
                }
                if entry
                    .file_type()
                    .is_some_and(|ft| ft.is_file() || ft.is_dir())
                {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    index
                        .entry(name)
                        .or_default()
                        .push(entry.path().to_path_buf());
                    total += 1;
                }
            }
        }

        // Beyond the curated dot-dir whitelist above, also index any explicit
        // hidden/ignored path the user might `@`-mention (e.g. a project's
        // own `.generated/specs/`). `local_reference_paths` walks with
        // gitignore disabled but still honors `.deepseekignore`.
        for path in local_reference_paths(
            &self.root,
            LOCAL_REFERENCE_SCAN_LIMIT,
            self.completion_walk_depth,
            self.follow_links,
        ) {
            if total >= FILE_INDEX_MAX_ENTRIES {
                break;
            }
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
            else {
                continue;
            };
            index.entry(name).or_default().push(path);
            total += 1;
        }
        index
    }

    /// Walk the workspace (and the recorded `cwd` when it diverges) and
    /// return relative paths whose representation matches `partial`.
    ///
    /// Ranking: a candidate matches when its case-insensitive display string
    /// starts with `partial` (prefix hit) or contains it as a substring; prefix
    /// hits sort first so `docs/de` lands `docs/deepseek_v4.pdf` ahead of any
    /// path that merely shares those bytes.
    ///
    /// Display strings are workspace-relative for files under `root`, and
    /// cwd-relative for files only under the recorded `cwd` — so what the user
    /// Tab-completes matches what their shell would have shown them.
    ///
    /// Honors `.gitignore`, `.git/info/exclude`, `.ignore`, and
    /// `.deepseekignore`. Capped at `limit` results.
    #[must_use]
    pub fn completions(&self, partial: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        let needle = partial.to_lowercase();
        let mut prefix_hits: Vec<String> = Vec::new();
        let mut substring_hits: Vec<String> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // Walk the recorded cwd first when it diverges from the workspace
        // root, so cwd-relative entries appear ahead of duplicates surfaced by
        // the workspace walk.
        {
            let mut ctx = SearchContext {
                needle: &needle,
                limit,
                prefix_hits: &mut prefix_hits,
                substring_hits: &mut substring_hits,
                seen: &mut seen,
            };

            let cwd_diverges = self
                .cwd
                .as_deref()
                .map(|c| c != self.root.as_path())
                .unwrap_or(false);
            if cwd_diverges && let Some(cwd) = self.cwd.as_deref() {
                walk_for_completions(
                    cwd,
                    cwd,
                    &mut ctx,
                    self.completion_walk_depth,
                    self.follow_links,
                );
                add_local_reference_completions(
                    cwd,
                    cwd,
                    &mut ctx,
                    self.completion_walk_depth,
                    self.follow_links,
                );
            }
            walk_for_completions(
                &self.root,
                &self.root,
                &mut ctx,
                self.completion_walk_depth,
                self.follow_links,
            );
            add_local_reference_completions(
                &self.root,
                &self.root,
                &mut ctx,
                self.completion_walk_depth,
                self.follow_links,
            );
        }

        prefix_hits.sort();
        substring_hits.sort();
        prefix_hits.extend(substring_hits);
        prefix_hits.truncate(limit);
        prefix_hits
    }

    /// One full completion walk with no needle: every discoverable display
    /// string from the workspace walk plus the divergent-cwd walk (and the
    /// always-discoverable AI dot-directories), deduped, in walk order.
    /// Pair with [`rank_completion_candidates`] so the composer can filter
    /// per keystroke without re-walking the filesystem (#3757).
    ///
    /// Needle-gated local path-reference completions are NOT included;
    /// callers must fall back to [`Workspace::completions`] for path-like
    /// needles (starting with `.` or containing a separator).
    #[must_use]
    pub fn completion_candidates(&self) -> Vec<String> {
        let mut prefix_hits: Vec<String> = Vec::new();
        let mut substring_hits: Vec<String> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        {
            let mut ctx = SearchContext {
                needle: "",
                limit: usize::MAX,
                prefix_hits: &mut prefix_hits,
                substring_hits: &mut substring_hits,
                seen: &mut seen,
            };
            let cwd_diverges = self
                .cwd
                .as_deref()
                .map(|c| c != self.root.as_path())
                .unwrap_or(false);
            if cwd_diverges && let Some(cwd) = self.cwd.as_deref() {
                walk_for_completions(
                    cwd,
                    cwd,
                    &mut ctx,
                    self.completion_walk_depth,
                    self.follow_links,
                );
            }
            walk_for_completions(
                &self.root,
                &self.root,
                &mut ctx,
                self.completion_walk_depth,
                self.follow_links,
            );
        }
        // Empty needle routes everything into prefix_hits.
        prefix_hits
    }

    /// Deterministic directory-browser completions for `@` mentions.
    ///
    /// Unlike [`Workspace::completions`], this mode does not fuzzy-rank across
    /// the full workspace. It locks onto the directory part of `partial` and
    /// returns only that directory's immediate children in case-insensitive
    /// alphabetical order.
    #[must_use]
    pub fn browser_completions(&self, partial: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }

        let normalized = partial.replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/');
        let (dir_part, name_part) = match trimmed.rsplit_once('/') {
            Some((dir, name)) => (dir.trim_end_matches('/'), name),
            None => ("", trimmed),
        };
        let Some(safe_dir_part) = browser_completion_dir_part(dir_part) else {
            return Vec::new();
        };
        let dir = if safe_dir_part.as_os_str().is_empty() {
            self.root.clone()
        } else {
            self.root.join(&safe_dir_part)
        };
        if !dir.is_dir() {
            return Vec::new();
        }
        let display_dir_part = safe_dir_part.to_string_lossy().replace('\\', "/");

        let show_hidden = name_part.starts_with('.');
        let needle = name_part.to_lowercase();
        let mut entries = Vec::new();

        let mut builder = WalkBuilder::new(&dir);
        builder
            .hidden(!show_hidden)
            .follow_links(self.follow_links)
            .max_depth(Some(1));
        let _ = builder.add_custom_ignore_filename(".deepseekignore");

        for entry in builder.build().flatten() {
            let path = entry.path();
            if path == dir || path_is_excluded_from_discovery(&self.root, path) {
                continue;
            }
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() && !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if !needle.is_empty() && !name.to_lowercase().starts_with(&needle) {
                continue;
            }
            let mut candidate = if display_dir_part.is_empty() {
                name.to_string()
            } else {
                format!("{display_dir_part}/{name}")
            };
            if file_type.is_dir() {
                candidate.push('/');
            }
            entries.push(candidate);
        }

        entries.sort_by_key(|entry| entry.to_lowercase());
        entries.truncate(limit);
        entries
    }
}

fn browser_completion_dir_part(dir_part: &str) -> Option<PathBuf> {
    let mut safe = PathBuf::new();
    for component in Path::new(dir_part).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => safe.push(part),
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    Some(safe)
}

/// Default directory depth walked when surfacing file-mention completions.
/// Set high enough that conventionally nested source trees (Java/.NET/web
/// projects routinely reach 7-9 levels) stay reachable, while a `0` override
/// removes the limit entirely. Keeps Tab snappy in deep monorepos via the
/// `.gitignore`-aware walk and per-keypress candidate caps (#2488).
pub const DEFAULT_COMPLETIONS_WALK_DEPTH: usize = 10;

fn normalize_completion_walk_depth(depth: usize) -> Option<usize> {
    if depth == 0 { None } else { Some(depth) }
}

fn child_completion_walk_depth(depth: Option<usize>) -> Option<usize> {
    depth.map(|depth| depth.saturating_sub(1))
}

/// Hard cap on the number of `(file or directory)` entries indexed by
/// [`Workspace::build_file_index`]. The fuzzy-resolve index is a
/// convenience for [`Workspace::fuzzy_resolve`]; missing entries fall
/// back to literal-path resolution. Capping here keeps the first
/// `fuzzy_resolve` call bounded on huge workspaces (#697 reported a
/// ~10s hang on the first turn). For typical projects 50K is well
/// above the actual entry count and the cap is a no-op.
const FILE_INDEX_MAX_ENTRIES: usize = 50_000;

/// Configure a `WalkBuilder` for workspace discovery: hidden files,
/// depth-limited, custom `.deepseekignore` honored, and gitignore overrides
/// for AI-tool dot-directories so `@`-completion finds them even when
/// they're gitignored. Symlink following is controlled by `follow_links`.
fn discovery_walk_builder(
    root: &Path,
    max_depth: Option<usize>,
    follow_links: bool,
) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(true).follow_links(follow_links);
    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth));
    }
    let _ = builder.add_custom_ignore_filename(".deepseekignore");
    builder
}

/// Walk the AI-tool dot-directories (`.deepseek/`, `.cursor/`, `.claude/`,
/// `.agents/`) with gitignore disabled so their contents are discoverable
/// even when the project's `.gitignore` / `.ignore` excludes them.
fn walk_always_discoverable_dirs(
    walk_root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    for dir_name in DISCOVERY_ALWAYS_DIRS {
        let dot_dir = walk_root.join(dir_name);
        if !dot_dir.is_dir() {
            continue;
        }
        let mut builder = WalkBuilder::new(&dot_dir);
        builder
            .hidden(true)
            .follow_links(follow_links)
            .git_ignore(false)
            .ignore(false);
        if let Some(depth) = max_depth {
            builder.max_depth(Some(depth.saturating_sub(1)));
        }
        for entry in builder.build().flatten() {
            if ctx.is_full() {
                break;
            }
            let path = entry.path();
            // Exclude machine-generated bulk (e.g. .deepseek/snapshots/)
            // even though gitignore is disabled for this walk.
            if path_is_excluded_from_discovery(walk_root, path) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(display_root) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() {
                continue;
            }
            let abs = path.to_path_buf();
            if !ctx.remember(abs) {
                continue;
            }
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            let candidate = if is_dir {
                format!("{rel_str}/")
            } else {
                rel_str.clone()
            };
            ctx.push_match(candidate);
        }
    }
}

fn walk_for_completions(
    walk_root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    let builder = discovery_walk_builder(walk_root, max_depth, follow_links);

    for entry in builder.build().flatten() {
        if ctx.is_full() {
            break;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(display_root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() {
            continue;
        }
        // Dedup across the (cwd, workspace) double-walk by absolute path; we
        // want the cwd-relative display when both walks see the same file.
        let abs = path.to_path_buf();
        if !ctx.remember(abs) {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
        let candidate = if is_dir {
            format!("{rel_str}/")
        } else {
            rel_str.clone()
        };
        ctx.push_match(candidate);
    }

    // Also walk the AI-tool dot-directories with gitignore disabled so
    // `.deepseek/`, `.cursor/`, etc. are always discoverable.
    walk_always_discoverable_dirs(walk_root, display_root, ctx, max_depth, follow_links);
}

const LOCAL_REFERENCE_SCAN_LIMIT: usize = 4096;

fn add_local_reference_completions(
    root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    if !should_try_local_reference_completion(ctx.needle) {
        return;
    }

    for path in local_reference_paths(root, LOCAL_REFERENCE_SCAN_LIMIT, max_depth, follow_links) {
        if ctx.is_full() {
            break;
        }
        let Ok(rel) = path.strip_prefix(display_root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() || !ctx.remember(path.clone()) {
            continue;
        }
        ctx.push_match(rel_str);
    }
}

/// Rank pre-collected completion candidates for `partial` the same way
/// [`Workspace::completions`] ranks live walk hits: case-insensitive prefix
/// matches first, then substring matches, each bucket alphabetical, truncated
/// to `limit` (#3757).
#[must_use]
pub fn rank_completion_candidates(
    candidates: &[String],
    partial: &str,
    limit: usize,
) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let needle = partial.to_lowercase();
    let mut prefix_hits: Vec<String> = Vec::new();
    let mut substring_hits: Vec<String> = Vec::new();
    for candidate in candidates {
        let lower = candidate.to_lowercase();
        if needle.is_empty() || lower.starts_with(&needle) {
            prefix_hits.push(candidate.clone());
        } else if lower.contains(&needle) {
            substring_hits.push(candidate.clone());
        }
    }
    prefix_hits.sort();
    substring_hits.sort();
    prefix_hits.extend(substring_hits);
    prefix_hits.truncate(limit);
    prefix_hits
}

fn should_try_local_reference_completion(needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    // A bare separator or dot isn't an actionable path yet. Without this
    // guard, a single `@/` keystroke triggers a `LOCAL_REFERENCE_SCAN_LIMIT`
    // (4096-path) walk on the UI thread for #1921 — on WSL2 with a
    // `/mnt/c/...` workspace each entry crosses Windows-host I/O and the
    // composer appears frozen for seconds to minutes.
    if matches!(needle, "/" | "\\" | "." | "..") {
        return false;
    }
    needle.starts_with('.') || needle.contains('/') || needle.contains('\\')
}

fn local_reference_paths(
    root: &Path,
    limit: usize,
    max_depth: Option<usize>,
    follow_links: bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(follow_links)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false);
    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth));
    }
    let _ = builder.add_custom_ignore_filename(".deepseekignore");
    let root_for_filter = root.to_path_buf();
    builder.filter_entry(move |entry| {
        !should_skip_unignored_discovery_entry(&root_for_filter, entry.path())
    });

    for entry in builder.build().flatten() {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        if path == root {
            continue;
        }
        if entry
            .file_type()
            .is_some_and(|ft| ft.is_file() || ft.is_dir())
        {
            out.push(path.to_path_buf());
        }
    }
    out
}

impl Clone for Workspace {
    fn clone(&self) -> Self {
        // Don't carry the cached file_index — clones get a fresh OnceLock so
        // they don't pin a stale snapshot of the previous owner's tree.
        Self {
            root: self.root.clone(),
            cwd: self.cwd.clone(),
            file_index: OnceLock::new(),
            completion_walk_depth: self.completion_walk_depth,
            follow_links: self.follow_links,
        }
    }
}

fn expand_mention_home(path: &str) -> PathBuf {
    if path == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn workspace_resolve_respects_cwd_and_workspace() {
        let tmp = TempDir::new().unwrap();

        let sub = tmp.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let bar = sub.join("bar.txt");
        std::fs::write(&bar, "bar").unwrap();

        let nested = tmp.path().join("nested/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let file_md = nested.join("file.md");
        std::fs::write(&file_md, "md").unwrap();

        // Construct with an explicit cwd so the test doesn't race with other
        // tests that mutate the real process cwd.
        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), Some(sub.clone()));

        // #101 repro #1: @bar.txt with cwd=sub MUST resolve via the cwd pass,
        // never to the bogus workspace path tmp/bar.txt (which doesn't exist).
        let res1 = ws.resolve("bar.txt").unwrap();
        assert_eq!(
            res1.canonicalize().unwrap_or(res1.clone()),
            bar.canonicalize().unwrap_or(bar.clone())
        );
        let wrong = tmp.path().join("bar.txt");
        assert_ne!(res1, wrong, "must not have routed to workspace fallback");

        // #101 repro #2: @nested/deep/file.md falls through to workspace root.
        let res2 = ws.resolve("nested/deep/file.md").unwrap();
        assert_eq!(
            res2.canonicalize().unwrap_or(res2),
            file_md.canonicalize().unwrap_or(file_md)
        );
    }

    /// Negative test (#101): a truly missing path returns `Err` with a path
    /// that callers can show to the user as a signal of failure.
    #[test]
    fn workspace_resolve_returns_err_for_truly_missing_path() {
        let tmp = TempDir::new().unwrap();
        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), Some(tmp.path().to_path_buf()));

        let res = ws.resolve("does/not/exist.txt");
        assert!(res.is_err(), "expected Err for missing path, got: {res:?}");
    }

    /// `Workspace::completions` returns workspace-relative entries for files
    /// under the root, and cwd-relative entries when the cwd-only file lives
    /// outside the workspace tree. Honors `.gitignore`.
    #[test]
    fn workspace_completions_walk_surfaces_workspace_and_cwd() {
        let tmp = TempDir::new().unwrap();
        // Two trees: a workspace under `ws/` and a cwd under `cwd/` that is
        // NOT inside the workspace, so the two walks are disjoint and we can
        // assert each branch contributed.
        let ws_root = tmp.path().join("ws");
        let cwd_root = tmp.path().join("cwd");
        std::fs::create_dir_all(&ws_root).unwrap();
        std::fs::create_dir_all(&cwd_root).unwrap();
        std::fs::write(ws_root.join("alpha.txt"), "a").unwrap();
        std::fs::write(cwd_root.join("alphabeta.txt"), "b").unwrap();

        let ws = Workspace::with_cwd(ws_root.clone(), Some(cwd_root.clone()));
        let entries = ws.completions("alpha", 16);
        assert!(
            entries.iter().any(|e| e == "alpha.txt"),
            "expected workspace entry alpha.txt; got: {entries:?}",
        );
        assert!(
            entries.iter().any(|e| e == "alphabeta.txt"),
            "expected cwd entry alphabeta.txt; got: {entries:?}",
        );
    }

    #[test]
    fn workspace_completions_honor_configured_walk_depth() {
        let tmp = TempDir::new().unwrap();
        // Sits at component depth 12, past the default walk depth (10) but
        // within the explicit deeper walk (16) below.
        let deep_dir = tmp.path().join("a/b/c/d/e/f/g/h/i/j/k");
        std::fs::create_dir_all(&deep_dir).unwrap();
        std::fs::write(deep_dir.join("target.txt"), "target").unwrap();

        let default_ws = Workspace::with_cwd(tmp.path().to_path_buf(), None);
        let default_entries = default_ws.completions("target", 16);
        assert!(
            !default_entries
                .iter()
                .any(|entry| entry.ends_with("target.txt")),
            "default depth should keep very deep entries out of the hot completion path: {default_entries:?}",
        );

        let deep_ws = Workspace::with_cwd_and_depth(tmp.path().to_path_buf(), None, 16);
        let deep_entries = deep_ws.completions("target", 16);
        assert!(
            deep_entries
                .iter()
                .any(|entry| entry.ends_with("target.txt")),
            "configured deeper walk should surface the nested file: {deep_entries:?}",
        );

        let unlimited_ws = Workspace::with_cwd_and_depth(tmp.path().to_path_buf(), None, 0);
        let unlimited_entries = unlimited_ws.completions("target", 16);
        assert!(
            unlimited_entries
                .iter()
                .any(|entry| entry.ends_with("target.txt")),
            "depth 0 should disable the completion walk depth limit: {unlimited_entries:?}",
        );
    }

    #[test]
    fn browser_completions_show_only_immediate_children() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/nested")).unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "lib").unwrap();
        std::fs::write(tmp.path().join("src/nested/deep.rs"), "deep").unwrap();
        std::fs::write(tmp.path().join("README.md"), "readme").unwrap();

        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), None);

        let root_entries = ws.browser_completions("", 16);
        assert_eq!(root_entries, vec!["README.md", "src/"]);

        let src_entries = ws.browser_completions("src/", 16);
        assert_eq!(src_entries, vec!["src/lib.rs", "src/nested/"]);
        assert!(
            !src_entries.iter().any(|entry| entry.ends_with("deep.rs")),
            "browser mode must not walk past immediate children: {src_entries:?}",
        );
    }

    #[test]
    fn browser_completions_hide_dot_entries_until_dot_query() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agents")).unwrap();
        std::fs::write(tmp.path().join(".env"), "secret-ish fixture").unwrap();
        std::fs::write(tmp.path().join("app.rs"), "app").unwrap();

        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), None);

        let default_entries = ws.browser_completions("", 16);
        assert_eq!(default_entries, vec!["app.rs"]);

        let dot_entries = ws.browser_completions(".", 16);
        assert_eq!(dot_entries, vec![".agents/", ".env"]);
    }

    #[test]
    fn browser_completions_reject_path_escape_segments() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let sibling = tmp.path().join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(workspace.join("inside.rs"), "inside").unwrap();
        std::fs::write(sibling.join("secret.rs"), "outside").unwrap();

        let ws = Workspace::with_cwd(workspace, None);

        assert_eq!(ws.browser_completions("", 16), vec!["inside.rs"]);
        assert!(
            ws.browser_completions("../", 16).is_empty(),
            "browser mode must not list workspace siblings",
        );
        assert!(
            ws.browser_completions("../outside", 16).is_empty(),
            "browser mode must not complete names from outside the workspace",
        );
    }

    #[test]
    fn workspace_completions_surface_explicit_hidden_and_ignored_paths() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".deepseek/\n.generated/\n").unwrap();
        std::fs::write(
            tmp.path().join(".deepseekignore"),
            ".generated/specs/secrets.env\n",
        )
        .unwrap();
        let deepseek_commands = tmp.path().join(".deepseek").join("commands");
        let generated_specs = tmp.path().join(".generated").join("specs");
        std::fs::create_dir_all(&deepseek_commands).unwrap();
        std::fs::create_dir_all(&generated_specs).unwrap();
        std::fs::write(deepseek_commands.join("start-task.md"), "start").unwrap();
        std::fs::write(generated_specs.join("device-layout.md"), "layout").unwrap();
        std::fs::write(generated_specs.join("secrets.env"), "secret").unwrap();

        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), Some(tmp.path().to_path_buf()));

        let start_entries = ws.completions(".deepseek/commands", 16);
        assert!(
            start_entries
                .iter()
                .any(|e| e == ".deepseek/commands/start-task.md"),
            "expected explicitly addressed hidden command file in completions: {start_entries:?}",
        );

        let generated_entries = ws.completions(".generated/specs", 16);
        assert!(
            generated_entries
                .iter()
                .any(|e| e == ".generated/specs/device-layout.md"),
            "expected explicitly addressed ignored user folder in completions: {generated_entries:?}",
        );
        assert!(
            !generated_entries
                .iter()
                .any(|e| e == ".generated/specs/secrets.env"),
            ".deepseekignore entries must not be reintroduced by local fallback: {generated_entries:?}",
        );
    }

    #[test]
    fn workspace_completions_skip_hidden_worktrees_and_build_bulk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n.generated/\n").unwrap();

        std::fs::create_dir_all(root.join(".worktrees/release/src")).unwrap();
        std::fs::write(
            root.join(".worktrees/release/src/worktree-only.rs"),
            "fn main() {}",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".worktrees/release/target/debug")).unwrap();
        std::fs::write(
            root.join(".worktrees/release/target/debug/generated.o"),
            "object",
        )
        .unwrap();

        std::fs::create_dir_all(root.join(".claude/worktrees/agent/src")).unwrap();
        std::fs::write(
            root.join(".claude/worktrees/agent/src/agent-only.md"),
            "agent note",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/commands")).unwrap();
        std::fs::write(root.join(".claude/commands/keep.md"), "command").unwrap();

        std::fs::create_dir_all(root.join(".generated/specs")).unwrap();
        std::fs::write(root.join(".generated/specs/device-layout.md"), "layout").unwrap();

        let ws = Workspace::with_cwd(root.to_path_buf(), Some(root.to_path_buf()));

        let worktree_entries = ws.completions(".worktrees", 32);
        assert!(
            worktree_entries
                .iter()
                .all(|entry| !entry.starts_with(".worktrees/")),
            "hidden release worktrees must stay out of completions: {worktree_entries:?}",
        );

        let claude_worktree_entries = ws.completions(".claude/worktrees", 32);
        assert!(
            claude_worktree_entries
                .iter()
                .all(|entry| !entry.starts_with(".claude/worktrees/")),
            ".claude/worktrees must stay out of completions: {claude_worktree_entries:?}",
        );

        let generated_entries = ws.completions(".generated/specs", 32);
        assert!(
            generated_entries
                .iter()
                .any(|entry| entry == ".generated/specs/device-layout.md"),
            "explicit user-generated hidden folders should still complete: {generated_entries:?}",
        );

        let command_entries = ws.completions(".claude/commands", 32);
        assert!(
            command_entries
                .iter()
                .any(|entry| entry == ".claude/commands/keep.md"),
            "normal .claude command files should still complete: {command_entries:?}",
        );

        assert!(
            ws.resolve("worktree-only.rs").is_err(),
            "fuzzy resolution must not index files from hidden release worktrees"
        );
        assert!(
            ws.resolve("agent-only.md").is_err(),
            "fuzzy resolution must not index files from .claude/worktrees"
        );
        assert!(ws.resolve("keep.md").is_ok());
    }

    #[test]
    fn fuzzy_index_resolves_hidden_and_ignored_files_except_deepseekignored() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".generated/\n").unwrap();
        std::fs::write(
            tmp.path().join(".deepseekignore"),
            ".generated/specs/secrets.env\n",
        )
        .unwrap();
        let generated_specs = tmp.path().join(".generated").join("specs");
        std::fs::create_dir_all(&generated_specs).unwrap();
        std::fs::write(generated_specs.join("device-layout.md"), "layout").unwrap();
        std::fs::write(generated_specs.join("secrets.env"), "secret").unwrap();

        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), None);
        let resolved = ws.resolve("device-layout.md").unwrap();

        assert!(resolved.ends_with(".generated/specs/device-layout.md"));
        assert!(
            ws.resolve("secrets.env").is_err(),
            "basename fuzzy resolution must honor .deepseekignore"
        );
        assert!(
            ws.resolve(".generated/specs/secrets.env").is_ok(),
            "exact user-specified paths should still resolve"
        );
    }

    #[test]
    fn fuzzy_index_finds_files_and_directories() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b/target_dir")).unwrap();
        std::fs::write(tmp.path().join("a/b/needle.rs"), "fn main(){}").unwrap();

        let ws = Workspace::with_cwd(tmp.path().to_path_buf(), None);

        // Basename-only mention triggers fuzzy fallback for both files and dirs.
        let f = ws.resolve("needle.rs").unwrap();
        assert!(f.ends_with("a/b/needle.rs"));
        let d = ws.resolve("target_dir").unwrap();
        assert!(d.ends_with("a/b/target_dir"));

        // Index was populated exactly once (subsequent lookups reuse it).
        assert!(ws.file_index.get().is_some());
    }

    /// Regression: `@`-mention completion must discover files inside
    /// `.deepseek/`, `.cursor/`, `.claude/`, `.agents/` even when
    /// those directories are excluded by `.gitignore` (or `.ignore`).
    /// The `discovery_walk_builder` override un-ignores them.
    #[test]
    fn completions_discovers_files_inside_gitignored_dot_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // `.ignore` works even outside a git repo; use it to simulate
        // a project that gitignores its AI-tool dot-directories.
        std::fs::write(
            root.join(".ignore"),
            ".deepseek/\n.cursor/\n.claude/\n.agents/\n",
        )
        .unwrap();

        // Create files inside each dot-dir.
        std::fs::create_dir_all(root.join(".deepseek/commands")).unwrap();
        std::fs::write(root.join(".deepseek/commands/build.md"), "build cmd").unwrap();
        std::fs::create_dir_all(root.join(".cursor/commands")).unwrap();
        std::fs::write(root.join(".cursor/commands/run.md"), "run cmd").unwrap();
        std::fs::create_dir_all(root.join(".claude/commands")).unwrap();
        std::fs::write(root.join(".claude/commands/test.md"), "test cmd").unwrap();
        std::fs::create_dir_all(root.join(".agents/skills/example")).unwrap();
        std::fs::write(
            root.join(".agents/skills/example/SKILL.md"),
            "name: example\n",
        )
        .unwrap();

        let ws = Workspace::with_cwd(root.to_path_buf(), None);

        // Completions should find entries inside the dot-dirs.
        {
            let entries = ws.completions("build", 16);
            assert!(
                entries.iter().any(|e| e.contains("build.md")),
                "expected build.md in completions although .deepseek/ is ignored; got: {entries:?}"
            );
        }
        {
            let entries = ws.completions("run", 16);
            assert!(
                entries.iter().any(|e| e.contains("run.md")),
                "expected run.md from .cursor/; got: {entries:?}"
            );
        }
        {
            let entries = ws.completions("test", 16);
            assert!(
                entries.iter().any(|e| e.contains("test.md")),
                "expected test.md from .claude/; got: {entries:?}"
            );
        }

        // Fuzzy resolution should also work.
        let f = ws.resolve("build.md").unwrap();
        assert!(f.ends_with("build.md"));
        let f2 = ws.resolve("SKILL.md").unwrap();
        assert!(f2.ends_with("SKILL.md"));
    }

    /// Regression: the dot-dir walk must NOT index `.deepseek/snapshots/`,
    /// which is the snapshot side repo that can grow to hundreds of GB.
    /// Indexing it would re-create the same OOM/hang that #1112 was built
    /// to prevent.
    #[test]
    fn dot_dir_walk_excludes_snapshot_side_repo() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create a snapshot-like directory tree.
        std::fs::create_dir_all(root.join(".deepseek/snapshots/deadbeef/deadbeef/.git/objects"))
            .unwrap();
        std::fs::write(
            root.join(".deepseek/snapshots/deadbeef/deadbeef/.git/objects/snapshot.pack"),
            b"fake pack data",
        )
        .unwrap();
        // Also create a legitimate file in .deepseek/ that should be found.
        std::fs::create_dir_all(root.join(".deepseek/commands")).unwrap();
        std::fs::write(root.join(".deepseek/commands/build.md"), "build cmd").unwrap();

        let ws = Workspace::with_cwd(root.to_path_buf(), None);

        // Searching for "build" must find build.md.
        let entries = ws.completions("build", 16);
        assert!(
            entries.iter().any(|e| e.contains("build.md")),
            "build.md must still be found; got: {entries:?}"
        );
        // Searching for "snapshot" must NOT return snapshot files.
        let snap_entries = ws.completions("snapshot", 16);
        assert!(
            !snap_entries.iter().any(|e| e.contains("snapshot")),
            "snapshot files must NOT appear in completions; got: {snap_entries:?}"
        );

        // Fuzzy index must also exclude snapshots.
        let f = ws.resolve("build.md").unwrap();
        assert!(f.ends_with("build.md"));
        // snapshot.pack should NOT resolve.
        let result = ws.resolve("snapshot.pack");
        assert!(
            result.is_err(),
            "snapshot.pack must not resolve via fuzzy index"
        );
    }

    /// Regression for #1921 — typing `@/` (or `@.`) must NOT trigger the
    /// `local_reference_paths` walk, which scans up to
    /// `LOCAL_REFERENCE_SCAN_LIMIT` paths on the UI thread. On WSL2 with a
    /// `/mnt/c/...` workspace this hangs the composer for seconds to minutes.
    #[test]
    fn should_try_local_reference_completion_skips_bare_separators_and_dots() {
        // The trigger gate must reject bare separators/dots.
        assert!(!should_try_local_reference_completion("/"));
        assert!(!should_try_local_reference_completion("\\"));
        assert!(!should_try_local_reference_completion("."));
        assert!(!should_try_local_reference_completion(".."));
        // Empty string was already rejected; keep that.
        assert!(!should_try_local_reference_completion(""));

        // Actionable references must still trigger.
        assert!(should_try_local_reference_completion("./foo"));
        assert!(should_try_local_reference_completion("../bar"));
        assert!(should_try_local_reference_completion(".env"));
        assert!(should_try_local_reference_completion("path/"));
        assert!(should_try_local_reference_completion("path/to/file"));
        assert!(should_try_local_reference_completion("/usr"));
    }

    #[test]
    fn cached_candidates_rank_like_live_completions() {
        // #3757: the composer caches one full candidate walk and ranks per
        // keystroke in memory; the ranked result must match what the live
        // walk would return for non-path-like needles.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/mention.rs"), "// m").unwrap();
        std::fs::write(root.join("README.md"), "# readme").unwrap();
        std::fs::write(root.join("Makefile"), "all:").unwrap();

        let ws = Workspace::with_cwd(root.to_path_buf(), None);
        let candidates = ws.completion_candidates();
        assert!(
            candidates.iter().any(|c| c == "src/main.rs"),
            "{candidates:?}"
        );

        for needle in ["ma", "readme", "men", ""] {
            let live = ws.completions(needle, 16);
            let ranked = rank_completion_candidates(&candidates, needle, 16);
            assert_eq!(ranked, live, "needle {needle:?}");
        }

        // Limit truncation applies after prefix/substring bucketing.
        let ranked = rank_completion_candidates(&candidates, "ma", 1);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].to_lowercase().starts_with("ma"), "{ranked:?}");
    }

    /// Regression for #1921 — `completions("/", N)` must return without
    /// invoking `local_reference_paths`, even on a workspace large enough
    /// to expose the original 4096-path walk. We can't assert "doesn't
    /// touch the disk", but we can assert the call completes promptly and
    /// stays within the requested limit.
    #[test]
    fn completions_for_bare_slash_does_not_trigger_local_reference_walk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Lay out enough files that a runaway walk would be visibly slow,
        // but the bounded path returns near-instantly. Depth-1 entries are
        // enough; we don't need to stress the filesystem.
        for i in 0..40 {
            std::fs::write(root.join(format!("file_{i}.txt")), "x").unwrap();
        }
        let ws = Workspace::with_cwd(root.to_path_buf(), None);

        let start = std::time::Instant::now();
        let entries = ws.completions("/", 64);
        let elapsed = start.elapsed();

        // Behavioral assertions:
        // 1. The call returns within a generous bound. Real freezes on
        //    WSL2 were tens of seconds; a 2s budget is comfortable for a
        //    40-file tmp dir on any CI host.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "completions(\"/\") took too long: {elapsed:?} (likely re-introduced #1921)"
        );
        // 2. Results stay within the requested cap.
        assert!(entries.len() <= 64);
    }
}
