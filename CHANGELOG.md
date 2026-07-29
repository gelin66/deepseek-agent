# Changelog

This changelog starts with the independent DeepSeek Agent product line. The
imported CodeWhale history remains available in Git and is not duplicated here.

## [Unreleased]

This section summarizes user-visible development since the imported baseline.
The canonical status, tested revision, and keep/delete evidence remain in the
[Roadmap](docs/product/ROADMAP.md),
[Evaluation contract](docs/product/EVALUATION.md), and frozen `eval/` records.

### Added

- One Rust-native DeepSeek Agent production loop shared by CLI, TUI, and the
  local Run API, with typed outcomes, SQLite replay/reopen, recovery, and
  Host-accepted latest-revision completion.
- Repository-scoped engineering tools, deterministic application probing, and
  one explicit isolated Writer worktree path that converges through
  read/edit/verify/integrate receipts.
- Canonical public HTTP(S) `web_fetch`, one Host-owned `web_search`, and a
  Rust-native semantic browser with scoped public interaction.
- Project-isolated managed browser sessions, Host-owned credential handling,
  and controlled workspace upload plus isolated/scanned download promotion.
- Complete English and Simplified Chinese human-facing interfaces.

### Changed

- Consolidated product truth into one Product Plan, one Roadmap, one Evaluation
  contract, accepted ADRs, and owner-scoped current architecture facts.
- Replaced unconditional repository bootstrap and duplicated validation lists
  with bounded owner routing and one risk-tier executable gate.
- Replaced legacy per-action browser paths with one semantic interaction
  surface and replaced mechanical first-N observations with deterministic
  task-cue priority and bounded action diffs.

### Removed

- Imported multi-provider routing, model Auto, chat bridges, cloud control
  surfaces, second runtime paths, and other product identities outside the
  DeepSeek-only DSE architecture.
- Imported website/release scaffolding, obsolete marketing translations,
  duplicate roadmaps, compatibility trackers, and stale public screenshots.

No public DSE release has been cut from these changes yet.

## [0.8.68] - 2026-07-13

Imported CodeWhale source baseline used to start this product line. Its detailed
upstream release history remains available in Git; this entry is retained
because the current binary exposes versioned release notes.

## [0.8.67] - 2026-07-06

Previous imported CodeWhale release boundary. Retained only so the current
release-note command can navigate one version backward during migration.
