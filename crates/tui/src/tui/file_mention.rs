//! `@path` parsing and deterministic workspace completion for the composer.
//!
//! Accepting a candidate only edits the ephemeral composer text. The
//! canonical request receives the exact `@path` text on the next submit;
//! this module never reads files or manufactures hidden model context.

use crate::tui::app::{App, MentionCompletionCache};
use crate::working_set::Workspace;

/// If the cursor sits inside a `@<partial>` token in the input, return the
/// byte offset where the `@` starts (so we can splice in a completion) and
/// the partial path the user has typed so far. The token stops at whitespace
/// or the end of input. Returns `None` when the cursor is outside any mention
/// or the token is empty (`@` with nothing after it).
pub fn partial_file_mention_at_cursor(input: &str, cursor_chars: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = input.chars().collect();
    if cursor_chars > chars.len() {
        return None;
    }
    // Walk left from the cursor until we find an `@` or a whitespace; if
    // whitespace comes first the cursor isn't inside a mention.
    let mut start_chars = cursor_chars;
    while start_chars > 0 {
        let prev = chars[start_chars - 1];
        if prev == '@' {
            start_chars -= 1;
            break;
        }
        if prev.is_whitespace() {
            return None;
        }
        start_chars -= 1;
    }
    if start_chars == cursor_chars || chars.get(start_chars) != Some(&'@') {
        return None;
    }
    // Confirm the `@` itself is at a valid mention boundary.
    if !is_file_mention_start(&chars, start_chars) {
        return None;
    }
    // Consume from the `@` to the next whitespace (the end of the token).
    let mut end_chars = start_chars + 1;
    while end_chars < chars.len() && !chars[end_chars].is_whitespace() {
        end_chars += 1;
    }
    let partial: String = chars[start_chars + 1..end_chars].iter().collect();
    let byte_start: usize = chars[..start_chars].iter().map(|c| c.len_utf8()).sum();
    Some((byte_start, partial))
}

/// Cwd-aware completion entry point. See [`Workspace::completions`] for the
/// ranking and display rules.
pub fn find_file_mention_completions(
    workspace: &Workspace,
    partial: &str,
    limit: usize,
) -> Vec<String> {
    let entries = workspace.completions(partial, limit);
    tracing::debug!(
        target: "codewhale_tui::file_mention",
        partial = %partial,
        workspace = %workspace.root.display(),
        cwd = ?std::env::current_dir().ok(),
        match_count = entries.len(),
        "file mention completion walk",
    );
    entries
}

/// Deterministic directory-browser completion entry point.
pub fn find_file_mention_browser_completions(
    workspace: &Workspace,
    partial: &str,
    limit: usize,
) -> Vec<String> {
    let entries = workspace.browser_completions(partial, limit);
    tracing::debug!(
        target: "codewhale_tui::file_mention",
        partial = %partial,
        workspace = %workspace.root.display(),
        cwd = ?std::env::current_dir().ok(),
        match_count = entries.len(),
        "file mention browser completion walk",
    );
    entries
}

/// Resolve the `@`-mention completion popup contents for the current
/// composer state. Returns an empty `Vec` when:
///
/// - The popup is suppressed (`app.mention_menu_hidden`).
/// - The cursor is not inside an `@<partial>` token.
/// - The workspace walk produced no candidates.
///
/// Mirrors `visible_slash_menu_entries` so the composer widget can treat
/// both menus identically (one `Vec<String>` of entries, one selected index).
///
/// The canonical key handler pairs this with
/// [`apply_mention_menu_selection`] for the Up/Down/Enter/Tab flow.
#[must_use]
pub fn visible_mention_menu_entries(app: &mut App, limit: usize) -> Vec<String> {
    if app.mention_menu_hidden {
        return Vec::new();
    }
    let Some((_byte_start, partial)) =
        partial_file_mention_at_cursor(&app.input, app.cursor_position)
    else {
        return Vec::new();
    };
    if limit == 0 {
        return Vec::new();
    }

    let workspace = app.workspace.clone();
    let cwd = std::env::current_dir().ok();
    let walk_depth = app.mention_walk_depth;
    let behavior = app.mention_menu_behavior.clone();
    let follow_links = app.workspace_follow_symlinks;
    if let Some(ref cache) = app.composer.mention_completion_cache
        && cache.workspace == workspace
        && cache.cwd == cwd
        && cache.partial == partial
        && cache.limit == limit
        && cache.walk_depth == walk_depth
        && cache.behavior == behavior
        && cache.follow_links == follow_links
    {
        return cache.entries.clone();
    }

    // Fast path (#3757): for non-path-like partials the candidate set is
    // needle-independent, so one cached walk serves every keystroke of the
    // mention token and ranking happens in memory. Path-like partials fall
    // through to the live walk because local path-reference completions are
    // needle-gated (see `should_try_local_reference_completion`).
    let path_like = partial.starts_with('.') || partial.contains('/') || partial.contains('\\');
    if behavior != "browser" && !path_like {
        const CANDIDATE_TTL: std::time::Duration = std::time::Duration::from_secs(4);
        let fresh = app
            .composer
            .mention_candidate_cache
            .as_ref()
            .is_some_and(|c| {
                c.workspace == workspace
                    && c.cwd == cwd
                    && c.walk_depth == walk_depth
                    && c.follow_links == follow_links
                    && c.collected_at.elapsed() < CANDIDATE_TTL
            });
        if !fresh {
            let ws = Workspace::with_cwd_depth_and_follow_links(
                workspace.clone(),
                cwd.clone(),
                walk_depth,
                follow_links,
            );
            app.composer.mention_candidate_cache = Some(crate::tui::app::MentionCandidateCache {
                workspace: workspace.clone(),
                cwd: cwd.clone(),
                walk_depth,
                follow_links,
                collected_at: std::time::Instant::now(),
                candidates: ws.completion_candidates(),
            });
        }
        let ranked = match app.composer.mention_candidate_cache.as_ref() {
            Some(cache) => {
                crate::working_set::rank_completion_candidates(&cache.candidates, &partial, limit)
            }
            None => Vec::new(),
        };
        let entries = ranked;
        app.composer.mention_completion_cache = Some(MentionCompletionCache {
            workspace,
            cwd,
            partial,
            limit,
            walk_depth,
            behavior,
            follow_links,
            entries: entries.clone(),
        });
        return entries;
    }

    let ws = Workspace::with_cwd_depth_and_follow_links(
        workspace.clone(),
        cwd.clone(),
        walk_depth,
        app.workspace_follow_symlinks,
    );
    let entries = if behavior == "browser" {
        find_file_mention_browser_completions(&ws, &partial, limit)
    } else {
        find_file_mention_completions(&ws, &partial, limit)
    };

    app.composer.mention_completion_cache = Some(MentionCompletionCache {
        workspace,
        cwd,
        partial,
        limit,
        walk_depth,
        behavior,
        follow_links,
        entries: entries.clone(),
    });

    entries
}

/// Apply the currently selected `@`-mention popup entry to the composer
/// input, splicing it in place of the `@<partial>` token at the cursor.
/// Returns `true` if a substitution occurred.
///
/// Designed to be invoked by the same keybinding that drives
/// `apply_slash_menu_selection` (Enter / Tab); the caller is responsible
/// for choosing which menu is "active" based on cursor context.
pub fn apply_mention_menu_selection(app: &mut App, entries: &[String]) -> bool {
    if entries.is_empty() {
        return false;
    }
    let Some((byte_start, partial)) =
        partial_file_mention_at_cursor(&app.input, app.cursor_position)
    else {
        return false;
    };
    let selected_idx = app
        .mention_menu_selected
        .min(entries.len().saturating_sub(1));
    let replacement = &entries[selected_idx];
    replace_file_mention(app, byte_start, &partial, replacement);
    // The completed path would otherwise remain an exact menu match and make
    // the next Enter accept it again instead of sending the request.
    app.mention_menu_hidden = true;
    app.mention_menu_selected = 0;
    app.status_message = Some(format!("已补全 @{replacement}"));
    true
}

/// Splice a completion into the input, replacing the `@<partial>` token at
/// `byte_start` with `@<replacement>`. Cursor moves to the end of the new
/// token so further keystrokes extend (or escape via space) naturally.
fn replace_file_mention(app: &mut App, byte_start: usize, partial: &str, replacement: &str) {
    let original_token_len = '@'.len_utf8() + partial.len();
    let original_token_end = byte_start + original_token_len;
    let mut new_input =
        String::with_capacity(app.input.len() - original_token_len + 1 + replacement.len());
    new_input.push_str(&app.input[..byte_start]);
    new_input.push('@');
    new_input.push_str(replacement);
    if original_token_end < app.input.len() {
        new_input.push_str(&app.input[original_token_end..]);
    }
    let new_cursor_chars =
        app.input[..byte_start].chars().count() + 1 + replacement.chars().count();
    app.input = new_input;
    app.cursor_position = new_cursor_chars;
}

fn is_file_mention_start(chars: &[char], idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    chars
        .get(idx.saturating_sub(1))
        .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '"' | '\''))
}

pub fn longest_common_prefix<'a>(values: &[&'a str]) -> &'a str {
    let Some(first) = values.first().copied() else {
        return "";
    };
    let mut end = first.len();

    for value in values.iter().skip(1) {
        while end > 0 && !value.starts_with(&first[..end]) {
            end -= 1;
            // Ensure we land on a valid UTF-8 char boundary.
            while end > 0 && !first.is_char_boundary(end) {
                end -= 1;
            }
        }
        if end == 0 {
            return "";
        }
    }

    &first[..end]
}
