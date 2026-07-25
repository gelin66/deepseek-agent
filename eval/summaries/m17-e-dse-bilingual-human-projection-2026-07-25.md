# M17-E DSE bilingual human projection cutover

Date: 2026-07-25
Decision: `accept_m17_e_bilingual_human_projection_cutover`
Code checkpoint: `6464fe1556390c1619a7a460349d35ce35b49443`
Code tree: `a4fc7f39c1c8bf8e7f2a8df5fc93967373385fec`

## Scope and decision

M17-E solves one bounded product problem: M17-D established a strict `en` /
`zh-Hans` language owner and startup contract, but retained CLI and TUI
surfaces still contained client-local Chinese or English human text. A locale
selector without complete real-entry projection would have been a false
bilingual claim.

The cutover is accepted. `crates/localization` remains the only message-catalog
owner, while the CLI and TUI remain thin projections of canonical app/runtime/
store facts. Every retained human-visible command, configuration, setup,
status, approval, cancellation, error, recovery, child/Writer and work-surface
message now consumes the process-frozen language. The app-server continues to
expose only canonical machine HTTP/SSE/stdio facts; its CLI-owned wrapper
errors are localized without changing the server protocol.

This slice does not select or translate the model-visible system prompt.
That is the separate M17-F paired experiment.

## Contract, owner and deleted path

- **Acceptance**: exact catalog key/placeholder parity; paired English/Chinese
  real-entry assertions; English narrow-terminal and Chinese CJK layout
  coverage; root/read-only child/Writer/recovery parity; byte-identical
  canonical machine output across locale; no route, request, accounting,
  protocol, State or Store change.
- **Single owner**: `crates/localization` owns product-language parsing and the
  two message catalogs. CLI/TUI modules only select message ids and pass typed
  facts.
- **Replaced path**: client-local hard-coded human language, private fallbacks
  and duplicate human formatters.
- **Cutover deletion**: the duplicate `McpAuthStatus` English `Display`
  formatter and retained CLI/TUI local language branches were removed when
  their callers moved to the catalog.

Catalog identities:

| Catalog | Keys | SHA-256 |
|---|---:|---|
| `en.json` | 768 | `8002ff24aa24977f12e5f02f1f9ff0ae325d15ecc9a7e4279fc1707bb1cfee27` |
| `zh-Hans.json` | 768 | `ce328fc30690ebac2c6cb965e76b221a1d8a0589ecbbe204f90fc30f355ae6c3` |

Both catalogs have the same key set and the same named-placeholder multiset for
every key.

## Human projection coverage

The accepted caller migration covers:

- CLI help, removed-command guidance, authentication/credential/model status,
  configuration and canonical Run list/resume errors;
- TUI setup, onboarding, Doctor, status, MCP/OAuth, sandbox, GitHub PR,
  headless execution, approval, clipboard, file mention, history, slash
  command, run presenter and work-surface messages;
- canonical root, read-only child, explicit Writer and typed recovery status
  projection;
- English 80-column multiline rendering and existing Chinese CJK 80/120-column
  width/wrap behavior;
- real `dse` / `dse-tui` process tests for both languages, including approval
  denial with zero write side effect.

## Exact non-localization allowlist

Locale does not translate or rewrite:

- commands, flags, tool names, model ids, paths, code, diffs, stdout, stderr,
  provider errors or stable failure codes;
- canonical JSON/NDJSON/HTTP/SSE, compact exec-stream terminal facts,
  RuntimeEvent, RequestPlan, route audit, tool catalog, budgets, accounting or
  RunStore state;
- the bilingual first-run picker shown before a locale has been resolved;
- model-facing PR-review prompts and generated skill/plugin templates, which
  remain stable code/model artifacts for the M17-F prompt experiment;
- historical `M7-A/M7-A2 DeepSeek Agent` titles, frozen schemas, hashes,
  manifests, summaries, raw journals and real historical paths.

The retained compact machine stream remains byte-stable even where its
historical terminal value is Chinese. That is a protocol fact, not a human
projection.

## Reproducible evidence

All Cargo commands used:

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/dse-m17-target
```

Passed:

- `cargo test -p dse-localization --locked`: `6/6`;
- `cargo test -p dse-tui --bin dse-tui --locked`: `803 passed`, `1` existing
  ignored;
- targeted English/Chinese approval PTY: `2/2`;
- canonical TUI PTY: `7/7`;
- CLI canonical Run integration: `6/6`, including exact en/zh machine JSON
  equality and paired human-list assertions;
- `./scripts/dev-dse.sh focused`;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`, including root/read-only child/Writer
  conformance, process crash/reopen, exec `25/25`, canonical TUI `18/18`,
  canonical PTY `7/7`, QA PTY `10/10`, release QA `5 passed` with one existing
  heavy test ignored, and run-surface parity `2/2`;
- `git diff --check`.

No credential was read, no official DeepSeek API or external network was used,
and no push, tag, release or public visibility action occurred. This is a
deterministic product-surface and invariance result, not a model-quality, Token,
latency or cost claim.

## Preserved architecture

- official DeepSeek ChatCompletions remains the sole production model
  transport;
- fixed root/Writer Pro-high, read-only child Flash-high and typed recheck
  Pro-max remain unchanged; Auto is absent;
- one `AgentRuntime`, one `RunStore`, Run API v12, RuntimeEvent v19, State v25
  and exec-stream v4 remain unchanged;
- DSE is the only current product identity; DSA remains only the superseded
  pre-public Git-history record caused by its conflict with the official
  DeepSeek Sparse Attention abbreviation.

## Next slice

M17-F must freeze the semantically paired English/Chinese DSE system prompts,
English/Chinese TaskContracts, immutable binary/catalog/verifier/schedule,
accounting and cost ceiling. After no-Key preflight, the formal fixed-Pro/high
2x2 matrix may run only through the canonical DeepSeek production path. Exactly
one prompt may survive; the loser, eval-only selector/assets and any temporary
dual-prompt branch must be physically deleted.
