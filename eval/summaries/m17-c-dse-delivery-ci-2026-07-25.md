# M17-C DSE delivery and CI

## Decision

`keep_and_cut_over` for the single locked/offline delivery owner at current
checkpoint `fd23400ca`.

This is reproducible delivery and identity evidence. It is not a model-quality,
token, latency, cost, billing, public-release, or remote-CI result. No credential,
official DeepSeek API, external network request, push, tag, release, or remote
visibility mutation was used.

## Slice contract

1. **Real problem:** the current DSE product still depended on release scripts,
   artifacts, install roots, program links and CI commands owned by the retired
   CodeWhale identity.
2. **Acceptance:** one checksum-bound DSE artifact owner packages exactly two
   canonical binaries and supports offline install, verify, upgrade, rollback
   and data-preserving uninstall on macOS and Linux; current callers use it;
   old active paths have no consumer.
3. **Owner:** `scripts/dse-delivery.sh`; CI only calls this owner.
4. **Replaced path:** old delivery/dev/test filenames, delivery schema,
   CodeWhale artifacts/binaries, `lib/codewhale`, old program links and CI
   artifact names.
5. **Evidence:** deterministic fixture lifecycle, tamper and target rejection,
   real current-revision macOS source package, offline Linux fixture, hermetic
   TUI runner, focused/full workspace gates.
6. **Deleted at cutover:** old script names and install paths plus the
   no-consumer M8-L release evaluator/test. Frozen M8-L manifest, summary and
   result evidence remain historical and are not rewritten.

## Exact delivery contract

- schema: `dse.delivery.v1`
- product: `DSE`
- binaries: `dse,dse-tui`
- artifact root: `dse-<version>-<target>-<revision-prefix>`
- immutable install owner: `<prefix>/lib/dse`
- program links: `<prefix>/bin/dse`, `<prefix>/bin/dse-tui`
- build identity input: `DSE_BUILD_SHA`
- source mode: `locked-offline-source`
- pinned toolchain: Rust 1.97.0

The manifest binds source revision, source tree, Cargo.lock SHA-256, target,
toolchain, source mode and binary set. Internal checksums bind manifest,
LICENSE and both binaries; the sidecar binds the archive. The installer rejects
unsafe archive paths, non-canonical entries, target mismatch, changed archives,
changed inner binaries, foreign links and mutable identity collisions.

## Platform evidence

### macOS arm64 current revision

At clean checkpoint `fd23400ca`, the owner built the actual source with:

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/dse-m17-target
cargo build --release --locked --offline -p dse-cli -p dse-tui
```

The resulting package installed under an isolated prefix. Both binaries
reported `0.8.68 (fd23400cac1d)`, `verify` passed, and `uninstall` removed the
program links and `lib/dse` while preserving data. The installed manifest
reported:

```text
schema=dse.delivery.v1
product=DSE
source_mode=locked-offline-source
binaries=dse,dse-tui
```

The isolated release output and prefix were deleted after verification.

### Linux arm64 offline fixture

The same committed self-test ran in an already cached
`golang:1.26-bookworm` Linux arm64 image with:

```text
--pull never
--network none
source bind mounted read-only
```

It passed deterministic repeated package identity, v1 install, archive and
inner-binary tamper rejection, wrong-target rejection, v2 upgrade, v1 rollback,
verify and uninstall. No image pull or package installation occurred.

The GitHub Actions delivery matrix now calls the same owner on Linux and macOS.
It has not been pushed or executed remotely in this slice; private remote CI is
an explicit M17-H release gate.

## Current caller and deletion audit

Current callers use:

- `scripts/dev-dse.sh`
- `scripts/dse-delivery.sh`
- `scripts/test-dse-delivery.sh`
- the DSE-configured hermetic TUI runner
- DSE commands in README, CONTRIBUTING, AGENTS and docs index
- DSE artifact/prefix names in `.github/workflows/ci.yml`

Deleted current paths:

- `scripts/dev-codewhale.sh`
- `scripts/codewhale-delivery.sh`
- `scripts/test-codewhale-delivery.sh`
- `scripts/eval-m8l-release-benchmark.py`
- `scripts/test-eval-m8l-release-benchmark.py`
- active CodeWhale artifact, binary, link and `lib/codewhale` handling

Historical manifests and summaries that truthfully name their old runner or
artifact remain immutable evidence. Their executable runner is available from
the frozen tested revision; current production has no compatibility reader.

## Verification

Passed:

- shell syntax for Bash and POSIX hermetic scripts;
- YAML parse of `.github/workflows/ci.yml`;
- DSE delivery self-test on macOS arm64;
- DSE delivery self-test on Linux arm64 with network denied;
- one-run hermetic TUI suite: 798 pass / 1 ignored;
- `./scripts/dev-dse.sh focused`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `git diff --check`.

## Remaining boundary

M17-D/E own the bilingual human projection. M17-H owns private remote CI,
release-candidate secret/license/provenance audit, tag/release and any public
visibility change. This slice authorizes none of those external mutations.
