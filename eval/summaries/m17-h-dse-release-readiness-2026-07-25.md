# M17-H DSE release-readiness

- Date: 2026-07-25
- Branch: `deepseek-agent`
- Local identity candidate: `a8c4bafab66a64bf27d1fccb44d86e1d9b66cc2a`
- Source tree: `e6d6ffde673d01912155dfadc29694009aa2df92`
- Product: `DSE` / `DeepSeek Engineer`
- Decision:
  `local_release_ready_remote_actions_pending_explicit_authorization_and_protection_capability`
- Credential/API use: none
- Remote mutations: none

## Problem and acceptance boundary

M17-G established the bilingual public-repository candidate, but it did not
prove that every active source identity had left the CodeWhale product, that a
package built from the exact current revision completed the local delivery
lifecycle, or that the private GitHub remote was ready for a protected public
release.

M17-H therefore separates two facts:

1. local source/release readiness can be established without credentials,
   DeepSeek traffic, or remote mutation;
2. push, remote CI, repository rename, visibility, protection, tag, and release
   remain separate external actions requiring explicit authorization.

The single owners remain the existing TUI/product surface, public-repository
checker, local delivery script, GitHub Actions workflow, and repository
settings. No release service, updater, compatibility reader, or second source
of truth was added.

## Active identity cutover

Commit `a8c4bafab` completed the final current-source cutover:

- deleted the tracked, no-consumer `.codewhale/constitution.json`;
- deliberately did **not** create `.dse/constitution.json` in this repository,
  because doing so would add an untested model-visible authority block after
  the M17-F prompt decision;
- retained ignored pre-DSE local paths only as inert user data; production has
  no reader or writer for them;
- renamed active TUI palette identifiers from the retired whale brand to DSE,
  while preserving every color value;
- removed the hidden `whale`, `whale-dark`, and `whale-light` theme aliases;
- replaced the idle whale silhouette with a responsive DSE wordmark and kept
  the existing dark/light, low-motion, terminal, and ASCII-safe behavior;
- changed non-frozen TUI path fixtures to DSE paths;
- extended `scripts/check-public-repository.py` so active crates fail on an
  unclassified CodeWhale/DSA identity or whale visual identity.

The exact crates allowlist contains only:

- negative assertions rejecting old CLI/prompt/config input;
- the State v25 migration comment and its pre-DSE materialized-run test;
- the canonical JSON v1 and M4-B recovery fixture identities;
- tests proving retired app-server dependencies and delivery paths stay absent.

It does not permit a current old binary, package, path, environment variable,
schema, media type, prompt marker, model request, help label, or TUI visual.
Historical M7-A/M7-A2 `DeepSeek Agent` titles, frozen eval schema/hash/
manifest/summary/raw, imported source paths, and Git history remain unchanged.

## Local validation

The identity candidate passed:

- `python3 scripts/check-public-repository.py`;
- `cargo fmt --all -- --check`;
- `cargo check -p dse-tui --locked`;
- palette, responsive empty-state, and ASCII-safe targeted TUI tests;
- `./scripts/dev-dse.sh focused`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `git diff --check`.

The focused and workspace suites cover the canonical root/read-only child/
Writer route, request-plan replay, pending Start, SIGKILL/reopen, exact
RunStore replay, accounting, CLI/TUI/app-server parity, and real PTY surfaces.
No Runtime, Store, protocol, official DeepSeek endpoint, fixed actor route, or
production prompt code changed.

## Secret, raw, license, and source audit

- `key.txt` is ignored and untracked.
- No tracked path exists under `eval/raw`, `target`, or `dist`, and no tracked
  database, log, key file, or release artifact was found.
- A boundary-aware credential-shape scan found only seven test/redaction
  fixture files in config, TUI acceptance, and eval support code. No value was
  printed or treated as a credential.
- All 269 ignored files currently under `eval/raw` are now mode `0600`.
  The audit found 258 old offline M1 logs at `0644` and changed permissions
  only; it did not read or rewrite their contents.
- `LICENSE` SHA-256 is
  `91873e17f073f4dcddc63799a0a6fdeb44a281440b6c5e0b9d8ea2aa7f7ffd95`,
  byte-identical to imported baseline
  `352e86a611fdf3cd8bd27c36d24d482c06a71117`.
- `THIRD_PARTY_NOTICES.md` and the English/Chinese README provenance retain
  the imported CodeWhale attribution without presenting it as current product
  identity.

## Locked/offline release lifecycle

The host is `aarch64-apple-darwin`; `rustc` and `cargo` are both 1.97.0.
The committed delivery self-test passed install, tamper rejection, upgrade,
rollback, verify, uninstall, and user-data preservation on macOS.

The exact clean source candidate then ran:

```text
cargo build --release --locked --offline -p dse-cli -p dse-tui
```

Its five-entry package contained only manifest, LICENSE, SHA256SUMS, `dse`,
and `dse-tui`. The manifest fixed:

```text
schema=dse.delivery.v1
product=DSE
version=0.8.68
target=aarch64-apple-darwin
source_revision=a8c4bafab66a64bf27d1fccb44d86e1d9b66cc2a
source_tree=e6d6ffde673d01912155dfadc29694009aa2df92
cargo_lock_sha256=33d367336b9e5bc32afe4cf3fb1694193036f288a623bc04b51ef8a2b9a05cf7
source_mode=locked-offline-source
binaries=dse,dse-tui
```

Archive SHA-256 was
`1f4beb04d2cb076d6c5ff224e3b46552de51b97cad13b4ec26c205fb84dd042b`.
The isolated install reported both binaries as
`0.8.68 (a8c4bafab66a)`, English and Chinese help projected the same command
contract, verify passed, uninstall removed only programs and `lib/dse`, and a
user-data marker remained byte-identical.

The same committed fixture lifecycle also passed in the cached
`golang:1.26-bookworm` Linux arm64 image with the source bind mounted
read-only, `--pull never`, and `--network none`. An exploratory stricter
read-only-container-root run rejected fixture executables in its `/tmp`
mount; that condition was not part of the frozen delivery contract and is not
used as a product regression or success claim.

## Private remote facts

Read-only GitHub and Git inspection found:

- origin: `https://github.com/gelin66/deepseek-agent.git`;
- repository: private, default branch `deepseek-agent`;
- remote/default SHA:
  `54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3`;
- local candidate is a descendant and is 621 commits ahead;
- the branch is not protected;
- the only remote Actions run is the 2026-07-15 old-SHA `Rust CI`, which
  failed at Clippy before workspace tests;
- the current DSE candidate has never run in private CI;
- both classic branch-protection and repository-ruleset read APIs returned
  HTTP 403 with GitHub's explicit requirement to upgrade to Pro or make the
  repository public;
- `gelin66/dse` returned 404 in the current viewer context. This proves no
  visible repository at that slug, not that a future rename is reserved or
  guaranteed.

No push, repository rename, protection/ruleset change, visibility change, tag,
release, or workspace-directory rename occurred.

## Decision and next authorization boundary

The local candidate is release-ready under the accepted DSE delivery contract.
M17 itself is not complete because private CI and the external release sequence
have not occurred.

The next safe order is:

1. obtain explicit authorization to push the current `deepseek-agent` branch
   to the existing private origin and wait for the complete current CI matrix;
2. choose how to satisfy protection: upgrade the account/repository plan while
   private, or explicitly accept a tightly bounded public-first window followed
   immediately by protection/ruleset configuration;
3. confirm `gelin66/dse` as the exact rename target;
4. separately authorize visibility change, tag, and GitHub release;
5. after release, install from the published artifact in a fresh environment
   and verify English/Chinese first run, real coding, resume, rollback, and
   uninstall.

Until those choices are explicit, the remote stays private and unchanged.
