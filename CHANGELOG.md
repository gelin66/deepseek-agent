# Changelog

This changelog starts with the independent DeepSeek Agent product line. The
imported CodeWhale history remains available in Git and is not duplicated here.

## [Unreleased]

### Changed

- Established the Rust + DeepSeek-only product scope and target architecture.
- Replaced overlapping product plans with one product plan, one roadmap, one
  evaluation contract, and three accepted ADRs.
- Preserved the pre-audit DeepSeek work as an explicitly unverified WIP before
  repository cleanup.
- Removed imported website, VS Code scaffold, npm publishing wrappers, cloud
  release automation, translated marketing READMEs, version trackers, and
  historical dogfood/release documents from the active development tree.

### Current migration status

- The production Agent loop still lives in `crates/tui`.
- DeepSeek protocol and Agent-reliability WIP still require slice-by-slice
  evaluation.
- Remote setup/chat bridges and generic Provider code remain only because the
  current Rust runtime still references them; their removal has an explicit
  migration point in `docs/product/ROADMAP.md`.

## [0.8.68] - 2026-07-13

Imported CodeWhale source baseline used to start this product line. Its detailed
upstream release history remains available in Git; this entry is retained
because the current binary exposes versioned release notes.

## [0.8.67] - 2026-07-06

Previous imported CodeWhale release boundary. Retained only so the current
release-note command can navigate one version backward during migration.
