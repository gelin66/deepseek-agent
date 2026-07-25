# M17-G DSE bilingual public repository

- Date: 2026-07-25
- Identity-marker correction: `389aac8960687be4d0a458b7171d6583286ac0e2`
- Nested-help localization correction:
  `de6bc7004dcdf61b7d316c771317560353e39dc9`
- Public-repository candidate:
  `85e241223b38482e15944706c0536294828cb765`
- Decision: `keep_public_repository_candidate`; M17-H release actions remain
  unexecuted and separately authorized

## Problem, owner, and replaced path

The root README still described an older Chinese-only CodeWhale checkpoint,
while contribution, security, governance, configuration, and operations
references either omitted DSE's current product boundary or retained stale
current-product facts. That made a locally verified product unsafe to present
as a public repository.

The single owner for this slice is the root public documentation plus GitHub
governance. It replaces stale README/reference claims and two no-consumer
reference pages; it does not create a translated Roadmap, Evaluation tree,
architecture authority, provider abstraction, or release controller.

## Cutover

- `README.md` is the canonical English public entry and
  `README.zh-CN.md` is a complete Simplified Chinese entry. Both state the
  official DeepSeek ChatCompletions endpoint, fixed actor routes, single
  Runtime/Store, explicit isolated Writer, current protocol versions, single
  Chinese-expression production prompt, and pre-public status.
- `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`,
  `THIRD_PARTY_NOTICES.md`, `CODEOWNERS`, English-first issue forms, and the
  pull-request template now freeze contribution, review, provenance, secret,
  and disclosure boundaries.
- `.env.example`, configuration, MCP, operations, accessibility, and sandbox
  references describe current DSE paths and commands. The stale keybinding and
  duplicate provider reference pages were deleted after their consumers were
  removed.
- `scripts/check-public-repository.py` is the deterministic offline owner for
  required public files, current README facts, local links, bash block syntax,
  issue-template shape, public credential patterns, retired current identities,
  and the exact historical M7-A/M7-A2 `DeepSeek Agent` titles. It runs from
  `scripts/dev-dse.sh` and GitHub Actions.
- The sandbox reference now distinguishes actual enforcement from policy
  intent: macOS Seatbelt and Linux bubblewrap are the current enforcing local
  paths, an isolated Writer fails closed without one, Landlock/seccomp are not
  wired into spawned Linux children, and Windows exposes no local OS sandbox.

Two audit findings were corrected in their existing owners before this public
commit:

1. `389aac896` changed active model-visible context markers from `cw:ctx` to
   `dse:ctx`, the TUI header mark to `DSE`, and removed the legacy whale status
   option.
2. `de6bc7004` made nested `auth` and `model set` help consume the existing
   localization owner instead of leaking derive-time Chinese or English text.
   The current catalogs contain 776 exact-matching keys. Their current SHA-256
   values are `fb0b4fa20ae488fa537446bfd6c0a68f06f72548b6baed491f83db698c6350c1`
   and `2f40b61d60ee199d56b0a3cfdd4fb5020472eca82345dec75ad39eab230fb845`.

The `dse:ctx` marker correction changes the current fully assembled prompt
fixture hash to
`d7746692db36eea33da0305553499b708a4d8b2b7d9688633aba1673142d49c9`.
It does not change the M17-F Chinese constitution, prompt-language winner, tool
contract, permission, completion, or multi-agent clauses. The frozen M17-F
tested hash
`a91799031d8f430945e98871f19d3cefd0496834304b4af0ff04197944ab1bdb`
remains immutable evaluation evidence.

## Identity and provenance audit

- DSE / DeepSeek Engineer is the only current public product identity.
- DSA remains only in the superseded pre-public planning record
  `83d05775`; it was replaced because the official DeepSeek V4 release uses
  DSA for DeepSeek Sparse Attention.
- The M7-A and M7-A2 Roadmap/Evaluation link labels and frozen summary titles
  remain exactly `DeepSeek Agent`. No eval manifest, fixture, schema, raw,
  summary, or hash was changed by the public candidate.
- CodeWhale remains only where source provenance, imported history, a frozen
  identity, or a real historical path requires it.
- `LICENSE` SHA-256 is
  `91873e17f073f4dcddc63799a0a6fdeb44a281440b6c5e0b9d8ea2aa7f7ffd95`,
  byte-identical to imported baseline
  `352e86a611fdf3cd8bd27c36d24d482c06a71117`.
- The tracked-tree credential-shape audit found only existing test/redaction
  fixtures. The current public-file gate found no credential, private-key, or
  active API-key assignment.

The official DeepSeek V4 release note and Chat Completions reference were
rechecked on 2026-07-25:

- <https://api-docs.deepseek.com/news/news260424/>
- <https://api-docs.deepseek.com/api/create-chat-completion/>

They confirm that the base URL remains unchanged, current model identifiers
are `deepseek-v4-pro` and `deepseek-v4-flash`, the old
`deepseek-chat`/`deepseek-reasoner` aliases retired after 2026-07-24 15:59 UTC,
and the OpenAI-format API path is `/chat/completions`.

## Verification

The candidate passed without reading a Key or calling the DeepSeek API:

- public-repository checker, Markdown local links, bash block syntax, GitHub
  YAML parse, Python syntax, shell syntax, and `git diff --check`;
- isolated real-binary smoke for `dse`/`dse-tui` version, English and Chinese
  root help, nested credential help, config path, model list, MCP list, and
  sandbox help; the temporary DSE home remained file-empty and was removed;
- `./scripts/dev-dse.sh focused`, including tools 347 pass / 1 ignored,
  DeepSeek 51 pass, Runtime conformance 82 pass, app 55 pass / 1 ignored,
  app-server 23 pass, exec 25 pass, canonical TUI 18 pass, and PTY 7 pass;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`, including State replay, process
  crash/reopen 38 pass / 1 ignored, TUI 803 pass / 1 ignored, exec, PTY,
  delivery-related, and Run API parity suites.

All Cargo commands used `CARGO_INCREMENTAL=0` and
`CARGO_TARGET_DIR=/private/tmp/dse-m17-target`.

## Decision and non-conclusions

The public-repository candidate is retained. It establishes accurate local
public material and deterministic offline review gates; it does not prove that
a remote CI run passed, that branch protection exists, or that DSE is publicly
released. No push, remote rename, visibility change, tag, release, credential
read, model request, or workspace-directory move occurred.

M17-H must next audit the remaining active-source identity allowlist, run the
locked/offline release lifecycle on the exact candidate, inspect remote/slug
state, and obtain explicit authorization before each external mutation.
