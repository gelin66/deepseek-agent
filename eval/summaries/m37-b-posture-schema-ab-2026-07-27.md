# M37-B execution posture / schema parity A/B

Date: 2026-07-27
Decision: `hold_insufficient_or_incomplete_evidence` for the formal acquisition;
`reject_and_delete_candidate` for the unadmitted production treatment.

## Question

M37-A found one stale tool-specific statement in the canonical
`execution_posture`: it said that `agent` only starts read-only children even
though the current Writer-coordinator schema also admits explicit isolated
Writers. M37-B asked whether deleting exactly that statement improves verified
task behavior without changing any enforceable capability.

The production candidate had one model-visible delta owned by `crates/context`:

```text
它只负责启动同一 Runtime 的只读后台子 Agent；
```

The substring is 60 UTF-8 bytes and has SHA-256
`ac7843f7d69ff54fc469389b6cff82098e8ed4ac67cbc3123ffe302f796c6125`.
Constitution, tool schemas, Runtime, Orchestrator, permissions, actor routing,
model, reasoning effort, budget, verifier and task wording stayed fixed.

## Frozen identity

- starting checkpoint: `b5ff2873b7db17298b607a056bca18a75ef99d1c`
- contract checkpoint: `55e5c7b74b4934bf3c7ed575bbc5165cef97eeb5`
- same-binary candidate checkpoint: `78d8ddb2c20e2e919993e629e26e64c7bdebca94`
- binary: `dse 0.8.68 (78d8ddb2c20e)`
- binary SHA-256:
  `fc5788056dc2574a387e15ca5d63a4dcee7061101ec5c60c2711047c57dfe688`
- contract manifest:
  `eval/manifests/m37-b-posture-schema-ab-v1.json`
- live admission:
  `eval/manifests/m37-b-posture-schema-live-admission-v1.json`
- admission SHA-256:
  `b8f8bcbf3f0670455bd959be1ea9991de2472fc4fc5832d6c230856429fb07ec`
- official surface: DeepSeek OpenAI-format `/chat/completions`, streaming,
  `deepseek-v4-pro`, reasoning `high`
- schedule: 5 tasks x 2 variants x 3 fresh runs = 30 arms,
  `maximum_reruns=0`
- cost ceiling: USD 0.50 per arm and USD 10.00 for the suite

The matrix covered root coding, read-only investigation/handoff, two
independent explicit Writer families and a no-tool false-completion
counterexample. The same immutable binary selected control or treatment only
through a guarded evaluation environment variable. Ordinary production
defaulted byte-for-byte to control.

## Credential-free gates

Before reading the test credential, the following passed:

- exact 60-byte-delta context test and all 21 `dse-context` tests;
- relevant app and DeepSeek prompt/SQLite projection tests;
- Harness freeze report, journal crash-window self-test and 30-arm dry-run;
- `./scripts/dev-dse.sh focused`, including process-level exec, app-server and
  bilingual PTY acceptance;
- `cargo fmt --all -- --check`;
- strict workspace Clippy;
- full workspace tests and doctests;
- `git diff --check` and public repository checker.

The first workspace run exposed a test-only HOME/DSE_HOME race between the
temporary M37-B comparison and an existing prompt fixture. The comparison was
serialized without changing production behavior, then the complete workspace
suite passed. That temporary test support was deleted with the candidate.

## Official acquisition

The ignored raw journal is owner-only (`0600`) at
`eval/results/m37-b-posture-schema-78d8ddb2c20e-v1.jsonl`. Its SHA-256 is
`4d638d4ac11f8dd11d49ddd63a11441906aa560e07c0c892304979c40154f9bd`.
It remains outside Git and was not edited after the formal stop.

The campaign produced 12 complete arm results, then stopped after collecting
the thirteenth arm's terminal, canonical Store, SQLite reopen and verifier
snapshots:

- 12/12 completed arm results had complete accounting;
- 9 positive verified successes and 3 correct safety rejections;
- `false_success=0`;
- every completed read-only/Writer arm had valid actor selection, and every
  completed Writer arm had valid Writer completion;
- control: 6 complete arms, 4 positive successes, 2 correct rejections;
- treatment: 6 complete arms, 5 positive successes, 1 correct rejection;
- every observed task/variant cell was successful or correctly rejected, so
  the partial observations showed no attributable behavioral difference;
- across all 13 terminal snapshots: 97 physical model requests, one Runtime
  retry, 731,129 input tokens, 36,086 output tokens, 571,136 cache-hit input
  tokens, 159,993 cache-miss input tokens and USD 0.237106726 known cost.

On scheduled arm 13 (`python_config_crossfile`, control), the first physical
request failed before response headers with typed `deepseek_transport` and no
content, reasoning, tool fragment, finish reason or usage. The canonical
Runtime classified the attempt replay-safe, persisted a one-second backoff and
made exactly one retry. The retry and the rest of the task completed; the
external verifier passed and four successful responses supplied complete,
priced usage. However, the pre-header attempt could not be proven billed or
unbilled. Final accounting therefore had:

```text
billing_unknown=true
billing_unknown_attempts=1
usage_complete=true
unpriced=false
runtime_retries=1
```

The Harness wrote `abort(error_code=accounting_incomplete,
completed_arms=12)` and did not start arm 14. No mate, rerun, historical raw or
post-hoc replacement was used.

## Decision and deletion

The formal acquisition is incomplete, so it cannot establish per-cell
non-inferiority or the preregistered benefit gate. The result is
`hold_insufficient_or_incomplete_evidence`; the successful partial arms are
descriptive only.

Because M37-B permits cutover only after a complete quality-and-benefit proof,
the product candidate is `reject_and_delete_candidate`:

- the original execution-posture sentence remains the only production path;
- the evaluation environment selector and alternate prompt branch were
  deleted;
- the M37-B Harness campaign, aggregation and synthetic consumer were deleted;
- no compatibility branch, config switch, second prompt composer, Runtime
  state or model-visible explanation remains.

The frozen contract, admission, ignored raw hash and this summary remain as
audit evidence. They do not authorize continuing position 14 or constructing
a second campaign from the completed prefix.

## Non-conclusions

This run does not prove that the stale sentence is beneficial, harmful or
equivalent. It does not prove a token, cache, wall-time or cost improvement.
The pre-header failure is a provider/accounting-boundary observation, not a
Prompt-quality failure and not evidence for changing Runtime retry policy.

M37-C is a separate single-variable question and is not admitted by this
result. Its context-dedup successor still requires a fresh contract, immutable
identity and complete behavior/accounting evidence.

## Official sources revalidated

Reviewed on 2026-07-27:

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)
- [Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
