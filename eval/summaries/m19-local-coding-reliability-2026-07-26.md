# M19 DSE local coding reliability acquisition

Date: 2026-07-26

Acquisition decision: `stop_incomplete_accounting`

Product decision: `insufficient_repeated_current_loss`

Product delta: none

## Scope

M19 started a new position-1 fixed-Pro/high acquisition rather than completing,
mating or rerunning M18. Before credential access it:

1. corrected the outer watchdog so a timeout stops the credential-bearing
   process and reopens the same SQLite Store without a credential before
   temporary cleanup;
2. froze two independent TypeScript verifier-recovery tasks, one Rust
   cross-file debug task, one explicit isolated Writer task and one no-tool
   false-completion counterexample;
3. required the same stable loss to repeat across at least two independent task
   IDs before any production owner audit.

GitHub, remote CI, push, Auto, FIM, a second Provider/Runtime/Store and
multi-Writer were outside the slice.

## Frozen identity

- starting revision: `d3e19e26dfbd1c4f381b5634bca377b495788908`
- acquisition candidate:
  `93e3ee34dbcbe297666388498c43e081f32c8787`
- candidate tree: `4e00b3048491e9a414fa3a5a6a03ec017ccde5cd`
- live admission: `2fc945798`
- release binary: `/private/tmp/dse-m19-target/release/dse`
- binary version: `dse 0.8.68 (93e3ee34dbcb)`
- binary SHA-256:
  `44f9b558402cd5ee37886c8a5a31fb8b4cf24f7be7c086f7fa0c628645876868`
- contract manifest SHA-256:
  `4fe5860db9ed4241fe6c7f342d448a69d974a52e5106ab1e1875bba76c548c43`
- corrected Harness SHA-256:
  `f937e10ed8ffcc84872f3971ebaf1c269437eea243403e8782c452562efa1339`
- schedule SHA-256:
  `d592aeb95f0578587563375854249de48523c55dd20e048201fab70d3cba2556`
- TaskContract aggregate SHA-256:
  `a18b0b5e530e7eebea0929659212bd07cda35ae182b70cfc3ab5ed9d0b814b61`
- verifier-environment SHA-256:
  `0137309de9201422380264ffa1158e147e2da2e20d8f76d5ca28b27bfcea65ae`
- Run API v12, RuntimeEvent v19, State v25, exec-stream v4
- official model surface: `deepseek-v4-pro`, reasoning `high`, official
  OpenAI-format ChatCompletions sender
- planned arms: 15; maximum reruns: 0

The no-Key dry run initially rejected stale authority hashes before credential
access. A manifest-only correction bound the ROADMAP, EVALUATION and current
architecture bytes committed with M19; the rebuilt candidate then passed the
full credential-free preflight.

## Offline gates

The following passed locally before credential access:

- five initial deterministic verifiers fail without fixture mutation;
- the isolated reference patch makes all four positive tasks pass while the
  safety counterexample remains failing;
- deadline/accounting projection and exact
  `arm_started -> deadline_interruption_snapshot -> abort` journal order;
- four journal SIGKILL windows;
- M14 observer conformance 12/12;
- M16 acceptance-equivalence conformance 18/18;
- `./scripts/dev-dse.sh focused`;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --locked`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- immutable release build and the final no-Key Harness dry run;
- `git diff --check`.

One macOS Seatbelt test failed once in the first workspace run. The exact test,
the complete `dse-tools` crate and a second full workspace run all passed. No
source change or exception was used to hide the transient local sandbox
rejection.

## Formal acquisition

The existing Harness read the user-authorized `0600` Key only after admission.
It was not printed, logged or committed.

The first scheduled arm,
`typescript_forwarded_chain_recovery`, started one physical Pro/high request.
The canonical Store records:

- `model_request_prepared -> model_request_in_flight ->
  model_request_failed -> terminal`;
- stable terminal code `deepseek_transport`;
- no response header, content, reasoning, tool call, finish reason, usage or
  stream completion observed;
- root physical requests `started=1`, `completed=1`, `in_flight=0`;
- no runtime or transport retry;
- `billing_unknown=true`, `billing_unknown_attempts=1`;
- accounting sealed but not complete.

The failed terminal, canonical Store snapshot, credential-free exact reopen and
external verifier snapshot were all durable before the Harness emitted one
`accounting_incomplete` abort. Zero arms received a complete product label and
the remaining 14 scheduled arms were not started. The initial external verifier
failure is required fixture state, not a coding outcome.

The retained ignored `0600` journal:

- path:
  `eval/results/m19-local-reliability-93e3ee34dbcb-v1.jsonl`
- records: 8
- size: 67,073 bytes
- partial tail: 0 bytes
- SHA-256:
  `78f8d770a7eec8ec78219b36e0f176d49442872888bc4715cce2e5a08c890656`

## Read-only trajectory decision

The analysis manifest binds the exact journal and permits only the observed
`accounting_incomplete` abort. The read-only report was byte-identical across
two runs:

- report SHA-256:
  `e3930abe9efcf705039fe0d99986aa933ec552de153bf5c4c3896e43f44b3e69`
- canonical Store trajectories: 1
- complete arm results: 0
- measurement interruptions:
  `typescript_forwarded_chain_recovery=1`
- verified success: 0
- correct rejection: 0
- false success: 0
- current product losses: none
- repeated independent loss tasks: none
- candidate: `insufficient_repeated_current_loss`

The zero false-success count is only a fact about the one incomplete trajectory;
it is not a quality result for the planned suite.

## Decision and deletion

The acquisition decision is `stop_incomplete_accounting`. The pre-header
transport failure may or may not have been billed, and the official contract
does not make that physical request reconcilable. M19 therefore does not infer
cost, retry the request, complete a mate or start a later arm.

The product decision is `insufficient_repeated_current_loss`. There is no
accounting-complete coding outcome, much less the same stable loss across two
independent tasks. No Runtime, Store, tool, prompt, retry, recovery-controller
or route change is authorized. Fixed root/Writer Pro-high, fixed read-only
Flash-high, typed Pro-max recheck and the single official DeepSeek production
chain remain unchanged.

The watchdog correction is retained in the sole corrected Harness because it
has a direct crash/deadline evidence role and ensures future temporary cleanup
cannot erase a durable Store boundary. There is no experiment-only production
branch to delete.

## Official protocol revalidation

Reviewed 2026-07-26:

- [DeepSeek change log](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 release and model names](https://api-docs.deepseek.com/news/news260424/)
- [DeepSeek model and pricing contract](https://api-docs.deepseek.com/quick_start/pricing/)
- [ChatCompletions request and streaming contract](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking-mode replay contract](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool-call contract](https://api-docs.deepseek.com/guides/tool_calls/)

The current V4 product surface remains the official
`https://api.deepseek.com/chat/completions` endpoint with
`deepseek-v4-pro`; the retired legacy aliases are not the current model IDs.
M19 added no obsolete alias, alternate provider or compatibility transport.

## Non-conclusions and next evidence

M19 does not establish a fixed-Pro coding baseline, a TypeScript recovery loss,
a Writer loss, a cost result or a production improvement. It also does not
prove the first request was billed or unbilled.

Another paid coding campaign must not be a mechanical M19 rerun. Before a new
position-1 acquisition, a bounded credential-safe local transport viability
slice should distinguish local DNS/TLS/auth/connectivity failure from an
inference request and prove that the official endpoint can be reached without
creating a second sender or weakening `billing_unknown -> stop`. Only then may
a new immutable identity and new acquisition be considered.
