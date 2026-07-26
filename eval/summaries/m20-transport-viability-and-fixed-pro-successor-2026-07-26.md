# M20 transport viability and fresh fixed-Pro successor

Date: 2026-07-26

Transport decision: `viable_for_bounded_successor`

Acquisition decision: `stop_incomplete_accounting`

Product decision: `insufficient_repeated_current_loss`

Product capability delta: none

## Scope and cutover

M20 started from M19's first-request pre-header `deepseek_transport` and
`billing_unknown=true`. It did not rerun M19, complete a mate or infer billing.
The first vertical slice audited the existing local Doctor path and found that
it sent an untracked one-token ChatCompletions request outside
`AgentRuntime`/`RunStore` accounting.

That old path was physically replaced by one Host diagnostic call to the
official authenticated `GET /user/balance` endpoint. The probe:

- reuses the canonical DeepSeek connection configuration, HTTP client, TLS,
  bearer authorization, User-Agent and timeout;
- performs no inference, consumes no model-request budget and has no retry;
- validates only `is_available` and the `balance_infos` shape;
- never exposes balance values or credentials;
- states that success proves only official host and credential reachability,
  not Chat inference, Agent success or request-level billing.

There is still one production Chat sender:
`https://api.deepseek.com/chat/completions`, using current V4 model IDs.
M20 added no Provider, sender, Runtime, Store, retry loop or compatibility path.

## Local gates and identity

All gates were local; GitHub, push, remote CI and release were not used.

The Doctor cutover passed:

- focused production checks;
- `cargo fmt --all -- --check`;
- targeted DeepSeek/TUI tests and checks;
- strict workspace Clippy;
- full workspace tests;
- offline timeout, malformed-shape, 401 redaction and no-retry fixtures;
- `git diff --check`.

Every Cargo command used `CARGO_INCREMENTAL=0` and
`CARGO_TARGET_DIR=/private/tmp/dse-m20-target`.

The non-inference viability candidate was
`698e41a912d1aa23ec9c7bf897e9b53f8b900ef5`. Its immutable
`dse-tui 0.8.68 (698e41a912d1)` binary had SHA-256
`ee07cf3d1e5486f16b0792bc0e0e97910042513bb1202062c4ebbba926daaffa`.
The existing corrected Harness ran the direct production Doctor caller three
times with:

- outcomes: `reachable`, `reachable`, `reachable`;
- durations: 554 ms, 384 ms, 387 ms;
- model requests: 0;
- known API cost: USD 0;
- maximum reruns: 0.

The retained ignored `0600` viability journal is
`eval/results/m20-transport-viability-698e41a912d1-v1.jsonl`: 6 records,
4,291 bytes, SHA-256
`03f18114d69ba7a1d33558d36aa39798bf72705e77374c789cb8e6cda2a89e94`.
It retains hashes and safe derived facts, not response text, balances or the
Key.

This proves DNS/TCP/TLS/HTTP/auth reachability collectively for those three
probes. It does not isolate each network layer, exercise Chat inference or
resolve pre-header request billing.

## Fresh fixed-Pro successor

Only after the 3/3 viability result, M20 froze a new acquisition identity:

- contract commit: `7797ec9d7aaa6de13ce7a4af25787b249175aeeb`;
- candidate tree: `d9cc7562d45ac0eefad8f9314893d11d06d9bb29`;
- live admission commit: `00a10a196`;
- immutable binary: `dse 0.8.68 (7797ec9d7aaa)`;
- binary SHA-256:
  `89e69ffbbd3ddeed66caa850826e183e4f7320fc9252bb74e472ec59229222f1`;
- manifest SHA-256:
  `407ac536631485f51bd457c43ba640e214e2e3ea0eb8c7083adc5961cf6ca49a`;
- Harness SHA-256:
  `299b554cdc76346df8273c86530ecd47298fa8067f3b839a9ed20c1f7d239bb4`;
- schedule SHA-256:
  `fee6ec6e291349e208cb195a6b19dae22cc59e7eb2b2c015abcdb913fd0333c5`;
- TaskContract aggregate SHA-256:
  `46b78571ad422a83d9f893999eaec96bd1c1bdba1753339c09a29e973d924495`.

The new matrix was three unrelated tasks × three repetitions: Python
scope-token recovery, TypeScript request-budget recovery and a no-tool
false-completion counterexample. It used fixed
`deepseek-v4-pro/high`, official ChatCompletions, one immutable binary,
deterministic verifier, exact Store reopen and `maximum_reruns=0`. Its fixtures
and prompts were not M19 tasks, and no historical raw was input.

Before credential access:

- all three initial verifiers failed without fixture mutation;
- one isolated reference patch made both positive tasks pass while safety
  remained failing;
- Harness/self-test, four journal SIGKILL windows, M14 observer 12/12 and M16
  acceptance equivalence 18/18 passed;
- immutable binary build and no-Key dry-run passed;
- worktree and all frozen hashes were exact.

## Formal acquisition result

The first arm, `python_scope_token_recovery`, completed:

- verified success: true;
- false success: false;
- physical requests: 8;
- usage responses: 8;
- `billing_unknown=false`, `usage_complete=true`, accounting sealed;
- known cost: USD 0.015759093;
- external verifier and latest Host receipt passed after the required
  fail-write-pass sequence.

The second arm, `typescript_request_budget_recovery`, reached a verifier-passing
workspace, but its sixth physical response ended as typed
`deepseek_transport`. Canonical terminal, Store and credential-free reopen
agree:

- terminal state: failed, code `deepseek_transport`;
- physical requests: `started=6 / completed=6 / in_flight=0`;
- response count: 6;
- usage-bearing responses: 5;
- incomplete responses: 1;
- no Runtime or transport retry;
- `billing_unknown=false`, `usage_incomplete=true`,
  `usage_complete=false`, accounting sealed;
- known usage cost recorded before the incomplete response:
  USD 0.009581600.

Because one response lacked complete usage, the arm is measurement-ineligible
even though its external verifier passed. The Harness wrote one
`accounting_incomplete` abort after 1 complete arm. The remaining seven arms
were not started, and no request was retried or mated.

The ignored `0600` journal is
`eval/results/m20b-fixed-pro-reliability-7797ec9d7aaa-v1.jsonl`: 14 records,
5,875,259 bytes, no partial tail, SHA-256
`cf25d9412fcfb8db3b50f1d5cfda0cf82d8a0d0786daa63fdd0469c468c639b7`.
The Key was read only after admission and was not printed, logged or committed.

## Read-only trajectory decision

The exact raw journal was validated twice without credential or network access.
Both reports were byte-identical, SHA-256
`49cd47bc6155662cc2235eacbe263b4cfbaa9215b6519a06248f03558c648107`:

- canonical Store trajectories: 2;
- complete arm results: 1;
- verified success: 1;
- correct rejection: 0;
- false success: 0;
- TypeScript measurement interruptions: 1;
- current product losses: none;
- repeated independent loss tasks: none;
- candidate: `insufficient_repeated_current_loss`.

The report observes one same-actor, same-mutation-epoch duplicate `read_file`.
That single frequency fact has no causal product loss and does not authorize a
tool or context treatment.

## Decision

The non-inference official endpoint was locally reachable and therefore
permitted exactly one fresh fixed-Pro acquisition. The successor then stopped
correctly on incomplete usage. This is stronger evidence than M19's zero-arm
pre-header stop, but it is not a complete baseline or a product comparison.

No production capability candidate is admitted. The current fixed root/Writer
Pro-high, read-only Flash-high, typed Pro-max recheck, canonical
AgentRuntime/RunStore/tools and official ChatCompletions sender remain
unchanged. The account probe cutover is retained because it deleted an
untracked inference side effect and provides a bounded diagnostic fact.

Further paid acquisition must not mechanically rerun M19 or M20B. A next slice
must first identify a materially different, bounded evidence question that can
stop safely on incomplete usage; otherwise local/offline engineering continues
without making cost or aggregate quality claims.

## Official protocol review

Reviewed 2026-07-26:

- [DeepSeek API overview and current base URL](https://api-docs.deepseek.com/api/deepseek-api)
- [Authenticated account balance endpoint](https://api-docs.deepseek.com/api/get-user-balance/)
- [Current ChatCompletions contract](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Error codes](https://api-docs.deepseek.com/quick_start/error_codes/)
- [Rate limits](https://api-docs.deepseek.com/quick_start/rate_limit/)
- [Current model pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Change log](https://api-docs.deepseek.com/updates/)
- [V4 model identities](https://api-docs.deepseek.com/news/news260424/)

The legacy `deepseek-chat`/`deepseek-reasoner` aliases were retired on
2026-07-24; official OpenAI-format ChatCompletions was not retired. No official
health endpoint, request-level billing export, settlement bound or pre-header
request reconciliation contract was found.

## Non-conclusions

M20 does not prove:

- `/user/balance` success means Chat inference will complete;
- the account endpoint can reconcile a physical Chat request or its billing;
- M20B's planned 9-arm fixed-Pro baseline is complete;
- the TypeScript verifier-passing workspace is a verified product success;
- one complete Python arm establishes general quality, cost or reliability;
- transport migration improved coding success;
- retries, Auto, FIM, another Provider, another Runtime/Store or multi-Writer
  should be added.
