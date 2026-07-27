# DSE

[简体中文](README.zh-CN.md)

**DSE (DeepSeek Engineer)** is a Rust-native, local-first coding agent built
exclusively for the official DeepSeek API. Its CLI, interactive TUI, and local
Run API share one application service, one agent runtime, one tool catalog, and
one SQLite-backed source of truth.

> Release status: DSE is still on its pre-public development line. There are no
> official public binaries, tags, or releases yet. Build and test from source;
> do not treat a partial M17 checkpoint as a public V1.

## Why DSE

DSE is deliberately narrow:

- one official DeepSeek backend using OpenAI-format Chat Completions at
  `https://api.deepseek.com/chat/completions`;
- one `AgentRuntime` for the root agent, read-only children, and an explicitly
  admitted isolated Writer;
- one canonical event protocol and one `RunStore` for exact replay, resume,
  accounting, and crash recovery;
- Host-owned `TaskContract`, latest-revision `EvidenceReceipt`, and
  deterministic verification before completion;
- atomic, workspace-scoped tools with typed outcomes and retry/side-effect
  facts;
- complete `en` and `zh-Hans` human interfaces without a language-classifier
  request or translation model.

The current protocol identities are Run API v14, RuntimeEvent v21,
State schema v27, and exec-stream v4.

## Fixed model profiles

When the user does not explicitly select a model or reasoning effort, DSE uses
deterministic actor profiles:

| Actor or action | Model | Reasoning |
|---|---|---|
| Root agent | `deepseek-v4-pro` | `high` |
| Explicit isolated Writer | `deepseek-v4-pro` | `high` |
| Typed recovery, recheck, or rework | `deepseek-v4-pro` | `max` |
| Ordinary isolated read-only child | `deepseek-v4-flash` | `high` |

There is no model Auto mode, prompt classifier, dynamic router, fallback
provider, or extra routing request. Explicit Pro/Flash and reasoning choices
remain replayable user inputs.

`dse exec --auto` has an unrelated CLI meaning: it enables the non-interactive
tool-agent loop with the Agent decides permission preset. Host-classified
critical calls still require approval and therefore fail closed headlessly.
The interactive TUI defaults to Ask for approval; only explicit process-local
`--yolo` selects Full access. There is no Custom permission mode or persistent
permission configuration.

DeepSeek retired the old `deepseek-chat` and `deepseek-reasoner` aliases on
2026-07-24. DSE uses the current `deepseek-v4-pro` and
`deepseek-v4-flash` identifiers while keeping the official base URL. See the
[DeepSeek V4 release note](https://api-docs.deepseek.com/news/news260424/) and
[Chat Completions reference](https://api-docs.deepseek.com/api/create-chat-completion).

## Build from source

Requirements:

- Git;
- the Rust 1.97.0 toolchain pinned by `rust-toolchain.toml`;
- platform build dependencies required by Cargo packages;
- an official DeepSeek API key for live model requests.

Build the two shipped binaries:

```bash
cargo build -p dse-cli -p dse-tui --locked
```

Save a key without printing it:

```bash
dse login
```

Alternatively, provide the official environment variable only to the process
that needs it:

```bash
export DEEPSEEK_API_KEY='replace-with-your-key'
```

Never commit a key, `.env`, raw provider traffic, local Run state, or evaluation
raw data.

## Use DSE

Start the interactive product in English or Simplified Chinese:

```bash
dse --language en
dse --language zh-Hans
```

Run a one-response request:

```bash
dse exec "Explain the ownership of this module."
```

Run the canonical non-interactive coding loop:

```bash
dse exec --auto "Fix the failing tests and verify the result."
```

Start the same canonical Run API over stdio:

```bash
dse app-server --stdio
```

HTTP/SSE binds to loopback by default and requires authentication:

```bash
export DSE_APP_SERVER_TOKEN='replace-with-a-random-token'
dse app-server --host 127.0.0.1 --port 7878
```

Do not expose an unauthenticated local API on a non-loopback interface.

## Configuration

The canonical user configuration is `~/.dse/config.toml`. Override the DSE
state root with `DSE_HOME` or the config file with `DSE_CONFIG_PATH`. Start from
[config.example.toml](config.example.toml); environment-only users can consult
[.env.example](.env.example).

The default endpoint is `https://api.deepseek.com`, and the sender posts to
`/chat/completions`. DSE rejects model-provider selectors and foreign provider
tables. `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, and `DEEPSEEK_MODEL` retain
their official names; product-specific settings use `DSE_*`.

The human-interface language resolves once per process:

```text
--language
  -> [ui].language
  -> first-run bilingual choice
  -> en for a fresh non-interactive environment
```

Changing the UI language does not translate commands, paths, code, diffs,
machine JSON/NDJSON/HTTP/SSE, model ids, or stable error codes. The production
system prompt is a single evidence-selected Chinese-expression prompt that
instructs the agent to answer in the user's task language unless explicitly
asked otherwise.

## Local delivery lifecycle

Build a checksum-bound package from the locked source tree:

```bash
CARGO_TARGET_DIR=/private/tmp/dse-delivery-target \
  ./scripts/dse-delivery.sh package --output-dir dist
```

Install, verify, roll back, or uninstall under an explicit prefix:

```bash
artifact="$(find dist -maxdepth 1 -name '*.tar.gz' -type f -print -quit)"
./scripts/dse-delivery.sh install --artifact "$artifact" --prefix "$HOME/.local"
./scripts/dse-delivery.sh verify --prefix "$HOME/.local"
./scripts/dse-delivery.sh rollback --prefix "$HOME/.local"
./scripts/dse-delivery.sh uninstall --prefix "$HOME/.local"
```

The uninstaller manages only the DSE program links and immutable release
directories under the selected prefix. It does not delete `DSE_HOME`.

## Development

Run the public-repository and focused production gates:

```bash
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  ./scripts/dev-dse.sh focused
```

Before integrating Rust changes:

```bash
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  cargo test --workspace --locked
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the review contract,
[SECURITY.md](SECURITY.md) for private vulnerability reporting, and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community expectations.

## Architecture and product authority

These documents have distinct roles:

1. [Product plan](docs/product/PRODUCT_PLAN.md) — accepted scope and fixed
   architecture.
2. [Architecture decisions](docs/decisions/) — long-lived accepted decisions.
3. [Roadmap](docs/product/ROADMAP.md) — current migration and deletion order.
4. [Evaluation contract](docs/product/EVALUATION.md) — evidence required to
   keep a capability.
5. [Current architecture facts](docs/architecture/CURRENT_CODEWHALE.md) —
   implementation facts, including historical names where accuracy requires
   them.

The repository does not create a translated parallel roadmap or a second
product-state tracker.

## Deliberate non-goals

DSE does not provide:

- Anthropic Messages or a general provider ecosystem;
- a model Auto router or task-difficulty classifier;
- a production FIM editor or a second editing surface;
- a second Runtime, Store, tool catalog, or completion authority;
- default multi-Writer, swarm, cloud-agent platform, or plugin marketplace.

The sole Writer path is explicit-only and isolated in a Git worktree. Ordinary
read-only children may investigate with their fixed actor profile; the root
agent remains responsible for integration and verified completion.

## Source, license, and independence

DSE continues from the MIT-licensed CodeWhale source imported at commit
`352e86a611fdf3cd8bd27c36d24d482c06a71117`. The preserved license is in
[LICENSE](LICENSE), and provenance details are in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

DSE is an independent community project. It is not affiliated with, sponsored
by, or endorsed by DeepSeek. DeepSeek names, models, APIs, and trademarks
belong to their respective owners.
