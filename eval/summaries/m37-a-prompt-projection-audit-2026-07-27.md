# M37-A model-visible prompt projection audit

## Decision

`keep_current_no_material_loss`

M37-A keeps the derived projection audit and 3,300-byte no-growth gate. It
does not change model-visible bytes and does not admit M37-B, M37-C, M37-D, or
M37-E.

## Identity and scope

- baseline: `4057028967ce62ec72eb60b17eb20456741732c2`
- implementation: `5fb2bc971315a00e32c703fe0807dd6199abfba2`
- implementation tree: `443f7bcf45de4e8b80f1df13bb71eba18d1b948d`
- fixture:
  `eval/fixtures/m37-a-prompt-projection-audit-v1.json`
  (`sha256:18b99ff46ce7273c5717ab665dfbf4d07e33e2571f8c53269f31a9657781bb43`)
- manifest:
  `eval/manifests/m37-a-prompt-projection-audit-v1.json`
  (`sha256:029e90b96d8434e4fee044f4ce77a06c1cab949ef12429034d09a45b11d13fbd`)
- credential read: false
- official DeepSeek requests: 0
- production prompt treatment: none

The single owner is `crates/context`. The old path replaced is the incomplete
shape of the existing read-only `PromptContextLedger`; no compatibility
ledger, Prompt service, Store, evaluator, or second composer remains.

## What the ledger now proves

Every ordered canonical fragment exposes:

```text
fragment_id
source
owner
authority_class
trust_class
scope
stability
sha256
payload_sha256
bytes
estimated_tokens
duplicate_of / duplicate_kind
model_visible
tool_schema_claims
```

The same derived object freezes summed block size, final DeepSeek system-message
size/hash, stable-prefix boundary/hash, and bundled core budget. A real
DeepSeek planner test proves that the ledger's joined bytes are the only wire
system message. Ordinary `production_system_prompt` and audited composition
remain field-for-field equal.

## Frozen current distribution

The deterministic fixtures use the same model, locale, shell identity, prompt
composer, and production project-context/skills paths:

| Fixture | Assembled bytes | Estimated tokens | Current duplicate |
|---|---:|---:|---|
| no `AGENTS.md` small repo | 5,283 | 2,063 | overview → pack same payload |
| rules-heavy | 5,198 | 1,995 | none |
| skills-heavy, 20 metadata entries | 9,225 | 3,802 | none |
| medium repo, 40 source files | 9,013 | 4,161 | overview → pack same payload |
| large repo, 260 source files | 18,963 | 9,735 | overview → pack same payload |

These values are current deterministic observations, not a universal hard
limit. The bundled core is still:

```text
constitution.md  2,629 bytes
output.md          360 bytes
language.md        311 bytes
total            3,300 bytes
```

The concatenated bundled-core SHA-256 is
`638f9a675f178df0fac58404102fa9b8af4e9b57d095329b4fef644416be4a67`.
No core file changed from the baseline.

## Current debt findings

1. When no project instruction source exists, generated bounded overview and
   project context pack wrap the same JSON payload. The audit records
   `project_context -> project_context_pack` as `same_payload_wrapper`; it does
   not delete either projection.
2. The execution posture claims that an available `agent` only creates
   read-only children. Actual actor-scoped parity is:

| Actor | Actual `agent.workspace_access` | Parity |
|---|---|---|
| root | `read_only` | match |
| Writer coordinator | `read_only`, `isolated_write` | mismatch |
| read-only child | `read_only` | match |
| explicit Writer | tool unavailable | not applicable |

This identifies the real source/caller boundary for a possible M37-B
treatment. M37-A does not edit the sentence or schema.

## Offline coverage

- no/with `AGENTS.md`, DSE and compatibility rules ordering;
- explicit configured-instruction order;
- explicit constitution override in an isolated process, including its
  user-authority provenance;
- metadata-only skills with bodies excluded;
- small, medium, and bounded large project packs;
- exact-byte duplicate and same-payload-wrapper relations;
- real root, Writer coordinator, read-only child, and explicit Writer catalogs;
- DeepSeek's single joined system message;
- root/read-only/Writer `ModelRequest`, ledger, RequestPlan, and SQLite reopen.

The app reopen tests retain the same canonical Runtime, State store, tool
catalog, actor authority, and request planner. There is no test-only alternate
prompt composition.

## Validation

The final implementation tree passed:

- `cargo fmt --all -- --check`;
- `cargo test -p dse-context --locked` (`20 passed`);
- `cargo check -p dse-context --locked`;
- `cargo test -p dse-deepseek --locked` (`61 passed`, `1 ignored`);
- `cargo test -p dse-app --locked` (`57 passed`, `1 ignored`);
- `./scripts/dev-dse.sh focused`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `git diff --check`.

The first focused run saw one pre-header transport timing failure in the
unchanged `established_sse_without_events_hits_typed_stream_stall` process
test. Its isolated rerun passed, and a complete second focused run plus the
final workspace suite both passed the same test. No production behavior was
changed to mask that transient.

## Complexity and deletion

The implementation commit is `+1,233/-37` across code, tests, fixture, and
manifest. Most added lines are typed audit facts and deterministic coverage.
Production request callers continue using `production_system_prompt`; the
catalog-aware API is explicit read-only audit input. The former ledger shape
was replaced in place, so there is no dual reader or compatibility state.

## Non-conclusions

- This does not prove that removing project pack improves task quality or
  efficiency.
- This does not prove that deleting the stale posture sentence helps Writer
  selection or completion.
- The deterministic token estimate is not provider billing.
- The fixture distribution is not a universal context budget.
- No Prompt, Writer recovery, tool schema, Runtime, RunStore, permission, TUI,
  model, or DeepSeek API treatment was evaluated.
- The single M36 Writer loss remains M36-A2 evidence and was not rewritten as
  a Prompt failure.
