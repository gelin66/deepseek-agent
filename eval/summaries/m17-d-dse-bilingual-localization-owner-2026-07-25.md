# M17-D DSE bilingual localization owner

Date: 2026-07-25
Decision: `accept_localization_owner_and_startup_contract`
Code checkpoint: `68f3aa7390d0ed1bdb1c265dfe9d40c4ead50a4c`

## Scope and decision

M17-D solves one bounded problem: the release-ready DSE checkpoint had a sole
`zh-Hans` catalog and implicit language assumptions, so a later English product
projection would have had no single owner or replayable startup rule.

The accepted owner is `crates/localization`. It admits exactly:

```text
ProductLanguage = en | zh-Hans

explicit --language
  -> persisted [ui].language
  -> old local DSE installation keeps zh-Hans
  -> fresh interactive TUI bilingual choice
  -> fresh noninteractive English
```

Language is frozen once per process and affects only human projection. It does
not enter TaskContract, model selection, prompt selection, Run API, RuntimeEvent,
RunStore, tool facts, request accounting, or actor routing. There is no language
classifier, translation model, additional DeepSeek request, per-Run locale, or
compatibility locale ecosystem.

M17-D does **not** claim that all CLI/TUI/app-server strings are bilingual.
Migrating every remaining human projection and deleting client-local hard-coded
language is the separate M17-E cutover.

## Accepted implementation

- `en.json` and `zh-Hans.json` each contain 417 exact-matching message ids.
- Every id has the same named-placeholder multiset in both catalogs.
- The strict parser rejects aliases such as `en-US` and `zh-hans`.
- `dse config get/set/unset/list ui.language` uses the canonical ConfigStore;
  project config and profiles cannot override the root UI language.
- `dse` and `dse-tui` scan only `--language` and `--config` before localized
  Clap rendering, then parse the normal command once.
- Fresh interactive TUI selection is bilingual before fullscreen setup and
  persists through the same ConfigStore. Default Enter selects English.
- Existing local DSE installations identified by `.onboarded` remain
  `zh-Hans` when they have no persisted language.
- CLI forwarding preserves an explicit language when it launches `dse-tui`.
- Historical Chinese PTY fixtures now pass `--language zh-Hans` explicitly;
  the first-run fixture selects Chinese and proves persisted restart behavior.

Catalog identities:

| Catalog | Keys | SHA-256 |
|---|---:|---|
| `en.json` | 417 | `55b5e84da9425106e3cb666b66a3f67687bc85d4ad39a02be519f517f4217cd7` |
| `zh-Hans.json` | 417 | `af3b6c8654aec7e7f680be4ce7392f7fafb7c9acbba2b4eaa781d416f98f74c6` |

## Reproducible evidence

All Cargo commands used:

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/dse-m17-target
```

Passed:

- `cargo fmt --all -- --check`;
- localization/config/CLI targeted tests:
  `6 + 77 + 14` unit tests and `6` CLI integration tests;
- TUI unit target: `802 passed`, `1` existing ignored;
- canonical TUI PTY: `6/6`;
- QA PTY: `9/9`;
- release runtime QA: `5 passed`, `1` existing heavy-fanout ignored;
- real `dse`/`dse-tui` process gate:
  `fresh=en`, explicit/persisted/migrated=`zh-Hans`, unknown locale rejected,
  exact temporary-directory cleanup;
- `./scripts/dev-dse.sh focused`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `git diff --check`.

The first focused attempt observed one timing-sensitive no-event SSE test return
the generic network error before its expected stall code. The exact test then
passed, and the same test passed in two later full invocations, including the
fully green focused run and the final workspace run. No localization caller
touches the DeepSeek transport. Separately, full gates exposed three groups of
PTY fixtures that still assumed fresh Chinese; they were migrated to explicit
language inputs, after which their complete original runtime assertions passed.

No credential was read, no official DeepSeek API was called, and no network,
push, tag, release, or public visibility action occurred.

## Cutover deletion and preserved boundaries

Deleted/replaced:

- the fixed sole-`zh-Hans` localization contract and sole-catalog assertion;
- implicit fresh-Chinese CLI/TUI startup behavior;
- PTY tests whose expected language came from ambient startup state;
- scattered startup precedence decisions outside the localization owner.

Preserved:

- official DeepSeek ChatCompletions as the only production model transport;
- fixed root/Writer Pro-high, read-only child Flash-high, typed recheck Pro-max;
- Run API v12, RuntimeEvent v19, State v25, exec-stream v4;
- exact canonical JSON/NDJSON/HTTP/SSE and RunStore replay facts;
- historical `M7-A/M7-A2 DeepSeek Agent` titles and all frozen manifest,
  schema, hash, summary, raw, and real-path evidence;
- Git history commit `83d05775`; DSA remains a superseded pre-public planning
  record because of the official DeepSeek Sparse Attention abbreviation.

## Next slice

M17-E must migrate every retained human-visible CLI, TUI and app-server
projection to this owner, add paired English/Chinese golden and PTY coverage,
and prove that switching locale changes no machine output, route, request,
accounting, or Store fact. Only after M17-E may the repository claim complete
product-surface bilingual support.
