# M24 current local DSE V1 release candidate

- Date: 2026-07-26
- Branch: `deepseek-agent`
- Candidate revision:
  `92d8b84b3a79167b8d912365af8d1e808a94c2ed`
- Candidate tree: `ef5983a13b26770cb556970207f421778e5d5e2e`
- Product: `DSE` / `DeepSeek Engineer`
- Decision: `keep_local_v1_release_candidate_no_blocker`
- Production delta: none
- Credential/API/network/GitHub/push/release:
  `0 / 0 / 0 / 0 / 0 / 0`

## Problem and acceptance boundary

M17-H and M18 established the local DSE V1 lifecycle before M22 retained its
same-chunk streaming convergence and M23 closed without a product candidate.
Those later commits did not intentionally change delivery, but the exact
current source had not been rebound to the complete local release contract.

M24 therefore tested release reproducibility rather than adding a capability.
The existing owners remained:

- `scripts/dse-delivery.sh` for package/install/verify/rollback/uninstall;
- `scripts/test-dse-delivery.sh` for deterministic lifecycle and tamper cases;
- `scripts/check-public-repository.py` for the current public source boundary;
- the existing Rust conformance suites for Runtime, Store and client parity.

No second installer, release store, evaluator, compatibility path or
production branch was created. A production change was permitted only after a
reproducible release blocker; none was found.

## Frozen identity

The contract is
`eval/manifests/m24-local-release-candidate-v1.json`. It freezes:

```text
workspace version       0.8.68
host target             aarch64-apple-darwin
rustc / cargo           1.97.0 / 1.97.0
Run API                 v12
RuntimeEvent            v19
State schema            v25
exec-stream             v4
Cargo.lock SHA-256      33d367336b9e5bc32afe4cf3fb1694193036f288a623bc04b51ef8a2b9a05cf7
rust-toolchain SHA-256  d5de56d1101f9e3ba0d694a777665be841781229b16408c262294e4de9d2ca85
```

The detached checkout was clean and matched the frozen revision and tree
before any build or test. Cargo used `CARGO_INCREMENTAL=0`,
`CARGO_NET_OFFLINE=true`, `--locked` and the isolated
`/private/tmp/dse-m24-target`.

## Exact-source artifact

The existing delivery owner built:

```text
dse-0.8.68-aarch64-apple-darwin-92d8b84b3a79.tar.gz
```

Its archive contained exactly five canonical entries:

```text
manifest.tsv
LICENSE
SHA256SUMS
bin/dse
bin/dse-tui
```

The manifest recorded:

```text
schema          dse.delivery.v1
product         DSE
version         0.8.68
target          aarch64-apple-darwin
source_revision 92d8b84b3a79167b8d912365af8d1e808a94c2ed
source_tree     ef5983a13b26770cb556970207f421778e5d5e2e
source_mode     locked-offline-source
binaries        dse,dse-tui
```

Identity hashes and sizes:

| Item | SHA-256 | Bytes |
|---|---|---:|
| archive | `54578a5ddb2596b595553e3ef617708e927259dbe9b887b3fd58e7879f52b92a` | 17,177,858 |
| manifest | `de4eff192891e3e6fb897ea9bf269a6fae153839a3041f3d680a3bf2fd243863` | — |
| `dse` | `e2d197103af0c757c6b5eaec4c3c70c0054328417308465703d2742ae51c1ac0` | 15,595,184 |
| `dse-tui` | `0a58c9dcc8cc17250b111055ca262a6b2458eaeec0dffac17250acf2a3e1e447` | 21,712,544 |
| LICENSE | `91873e17f073f4dcddc63799a0a6fdeb44a281440b6c5e0b9d8ea2aa7f7ffd95` | — |

Every internal checksum and the outer checksum sidecar verified.

## Delivery lifecycle

The committed delivery self-test passed:

- deterministic same-input packaging;
- archive checksum and inner binary tamper rejection;
- wrong-target rejection;
- install and verify;
- upgrade and one-release rollback;
- uninstall and user-data preservation;
- no release-discovery or download command in the owner.

The current exact artifact was then exercised in a separate isolated prefix.
An earlier prebuilt fixture identity was installed first so the exact artifact
could be the real upgrade target. The active and previous manifest links were
checked at each transition:

```text
fixture install -> current exact upgrade -> fixture rollback
  -> current exact re-activation -> uninstall
```

The exact active manifest matched the frozen revision and tree. Both installed
binaries reported `0.8.68 (92d8b84b3a79)`. English and `zh-Hans` installed help
projected the same command contract. Uninstall removed the two program links
and `lib/dse`; the isolated `DSE_HOME` marker remained byte-identical with
SHA-256
`33d367336b9e5bc32afe4cf3fb1694193036f288a623bc04b51ef8a2b9a05cf7`.

The prior prebuilt identity is only a delivery-mechanism fixture. It is not a
historical DSE release or a second product artifact.

## Runtime, Store and surface evidence

`./scripts/dev-dse.sh focused` passed from the clean checkout. Its retained
coverage included:

- canonical tools: 347 passed, one external crash helper ignored;
- DeepSeek transport/planner/accounting: 58 passed, the frozen benchmark
  ignored;
- Runtime conformance: 83 passed;
- application: 56 passed, one external process helper ignored;
- app-server: 23 passed;
- exec terminal acceptance: 25 passed;
- canonical TUI run acceptance: 18 passed;
- real TUI PTY acceptance: 7 passed.

The focused evidence covers fixed root Pro/high, read-only Flash/high, explicit
Writer Pro/high and typed Pro/max recovery; exact RequestPlan and accounting
rebuild; pending Start; no-credential terminal replay; process SIGKILL/reopen;
M21 partial-response fail-closed; M22 same-chunk convergence; and
CLI/TUI/HTTP/SSE/stdio parity.

The strict workspace Clippy gate and complete workspace test both passed.
Notable full-suite projections included:

- app-server process crash recovery: 3 passed, one process child ignored;
- localization: 6 passed;
- explicit Writer orchestration: 25 passed;
- TUI unit surface: 803 passed, two deliberate heavy/helper cases ignored;
- release runtime QA: 5 passed, one explicit 32-worker stress case ignored;
- canonical Run surface parity: 2 passed;
- real bilingual PTY: 7 passed.

Ignored tests retain their documented explicit-run/helper roles; no unexpected
skip or zero-match focused filter occurred.

## Repository, secret, license and provenance audit

- `scripts/check-public-repository.py` passed.
- `key.txt` remained ignored and was not read.
- No tracked `key.txt`, `eval/raw`, target, dist, release artifact, database or
  log was present.
- The existing 269 ignored raw files in the primary workspace were all mode
  `0600`; their contents were not read or rewritten.
- The localization catalogs had 776/776 exact keys and identical named
  placeholders.
- LICENSE was byte-identical to imported baseline
  `352e86a611fdf3cd8bd27c36d24d482c06a71117`.
- English and Chinese README provenance and `THIRD_PARTY_NOTICES.md` retained
  the imported CodeWhale attribution without restoring an active old product
  identity.
- The active-source CodeWhale/DSA allowlist and current DSE identity gate
  passed.

## Gates

All commands ran locally:

```text
python3 scripts/check-public-repository.py
CARGO_NET_OFFLINE=true ./scripts/test-dse-delivery.sh
CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=/private/tmp/dse-m24-target \
  ./scripts/dse-delivery.sh package ...
./scripts/dse-delivery.sh install/verify/rollback/uninstall ...
CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=/private/tmp/dse-m24-target \
  ./scripts/dev-dse.sh focused
CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=/private/tmp/dse-m24-target \
  cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=/private/tmp/dse-m24-target \
  cargo test --workspace --locked
cargo fmt --all -- --check
git diff --check
```

No GitHub operation, remote CI, push, tag, release, official DeepSeek request or
credential read occurred.

## Decision, deletion and non-conclusions

The decision is `keep_local_v1_release_candidate_no_blocker`.

There is no production fix, compatibility branch or old production path to
keep or delete. The only new tracked files are the frozen M24 contract,
authority updates and this summary. The temporary detached checkout, Cargo
target, artifacts, install prefix, inspection directory and user-data fixture
are deleted after the conclusion is committed.

M24 proves current local release reproducibility. It does not prove:

- remote CI, repository protection, public visibility, tag or release;
- official DeepSeek reachability or coding quality;
- a complete M23 Hardness baseline, high/max benefit or a selected candidate;
- verified-success, Token, time or cost improvement;
- a reason to restore Auto, Anthropic Messages, FIM, multi-Writer, a second
  Provider/Runtime/Store or a second delivery owner.

Future product work still requires a new repeated, attributable current loss.
M24 is only the exact current local V1 release regression anchor.
