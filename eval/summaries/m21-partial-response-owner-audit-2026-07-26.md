# M21 partial-response owner audit

Date: 2026-07-26

Owner decision: `no_local_defect_reproduced`

Runtime decision: `keep_fail_closed_stream_accounting`

Observed boundary:
`upstream_response_body_interruption_not_reconstructible`

Product capability delta: none

Paid successor: not admitted

## Scope

M21 audited only the TypeScript measurement interruption frozen by M20B. It did
not rerun M19 or M20B, complete a mate, read the Key, call the official
DeepSeek API, access external network or infer missing usage.

The immutable input journal remains
`eval/results/m20b-fixed-pro-reliability-7797ec9d7aaa-v1.jsonl`:

- mode: `0600`;
- bytes: 5,875,259;
- records: 14, with no partial tail;
- SHA-256:
  `cf25d9412fcfb8db3b50f1d5cfda0cf82d8a0d0786daa63fdd0469c468c639b7`.

The existing read-only trajectory report was recomputed twice. Both outputs
remained byte-identical with SHA-256
`49cd47bc6155662cc2235eacbe263b4cfbaa9215b6519a06248f03558c648107`.
No prompt, reasoning text, tool argument, credential or provider network detail
was copied into the committed fixture.

## Frozen observation

The second M20B arm's final event window is:

| Sequence | Event |
|---:|---|
| 2989 | model request prepared |
| 2990 | model request in flight |
| 2991 | one reasoning delta |
| 2992 | model request failed |
| 2993 | failed terminal |

The failure was committed 56 ms after the reasoning delta. That is far below
the Harness model-event idle limit of 120 seconds and the production transport
stream-idle limit of 900 seconds. It was not an idle-timeout transition.

The canonical failure and accounting facts were:

- code `deepseek_transport`, category `transport`, retryable;
- response headers and reasoning observed;
- no content, tool call, finish reason, `[DONE]` or usage observed;
- actionable partial output, therefore replay unsafe;
- retry decision `stop / actionable_output`;
- `started=6 / completed=6 / in_flight=0`;
- response count 6, usage-bearing response count 5;
- one incomplete response, no Runtime or transport retry;
- `billing_unknown=false / usage_incomplete=true / sealed=true`;
- no committed model response or completion proposal.

The preceding deterministic verifier had passed, but the Host correctly did not
convert that fact into completion. A verifier pass without the model's
completion proposal and latest revision-bound Host receipt remains
non-terminal evidence.

## Canonical call graph

The audited owners remain singular:

1. `crates/deepseek/src/transport.rs` owns the reqwest response byte stream,
   SSE parsing, finish/`[DONE]` contract and response-progress evidence.
2. `crates/deepseek/src/model_port.rs` and `accounting.rs` own stable error
   projection and response/usage/incomplete accounting.
3. `crates/runtime/src/agent.rs` owns replay safety, retry disposition,
   committed model responses and completion proposals.
4. `crates/state` owns exact RuntimeEvent persistence and reopen projection.

There is no alternate sender, parser, retry loop, completion authority or
accounting store.

## Deterministic replay

The committed redacted fixture freezes the observed M20B contract. A real local
TCP loopback server then:

1. accepted the production ChatCompletions request;
2. returned HTTP 200 and `text/event-stream`;
3. emitted one valid `reasoning_content` SSE frame;
4. closed the body before the declared `Content-Length`.

This exercised the actual reqwest byte stream, DeepSeek transport/parser and
model port. It produced exactly the frozen evidence:

- `deepseek_transport`;
- headers/reasoning true;
- finish/`[DONE]`/usage false;
- incomplete response 1;
- billing-unknown attempt 0;
- committed model response false.

The Runtime regression gave the request a retry budget of three and still
proved one physical call only. Partial reasoning is actionable output, so replay
is unsafe and the retry stops. The State regression persisted that exact
failure, retry decision and accounting through a SQLite close/reopen.

Existing process-level in-flight recovery continues to prove that crash/reopen
does not reissue an ambiguous physical model request.

## Decision

The deterministic local replay matches M20B. No failing test identified a
defect in the current transport, parser, accounting, Runtime or Store owner, so
M21 made no production correction and created no treatment branch.

The strongest supported statement is that the HTTP response body ended after
partial reasoning and before finish, `[DONE]` and usage. M21 cannot identify
which provider, proxy, OS or network component closed that body. The official
ChatCompletions contract provides no safe continuation token, reconstruction
of missing usage, or request-level reconciliation for this state.

The current behavior is retained:

- fail closed when finish/`[DONE]`/usage are incomplete;
- never fabricate usage;
- never auto-complete because an external verifier happened to pass;
- never blindly replay after content, reasoning or tool-call evidence;
- persist the exact interruption and accounting through reopen.

M19/M20B remain stopped. M21 does not admit a paid successor, quality aggregate,
retry policy, Auto, FIM, another Provider, another Runtime/Store or
multi-Writer.

## Evidence and local gates

Committed evidence:

- `eval/fixtures/m21-partial-response-interruption-v1.json`;
- `eval/manifests/m21-partial-response-owner-audit-v1.json`;
- a real loopback truncated-body regression in `dse-deepseek`;
- a no-retry/no-completion regression in `dse-runtime`;
- an exact SQLite reopen regression in `dse-state`.

All Cargo commands use:

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/dse-m21-target
```

The following local gates passed:

- `./scripts/dev-dse.sh focused`;
- `cargo fmt --all -- --check`;
- full `dse-deepseek`, `dse-runtime` and `dse-state` tests plus the
  `dse-deepseek` check;
- process-level SIGKILL/reopen and exec partial-SSE acceptance;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- JSON fixture/manifest parsing, two byte-identical trajectory reports and
  `git diff --check`.

Local success is sufficient for this commit; GitHub, push, remote CI and
release are out of scope.

## Official protocol boundary

The official contract was last revalidated by M20 on 2026-07-26:

- [DeepSeek API overview and base URL](https://api-docs.deepseek.com/api/deepseek-api)
- [ChatCompletions](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Error codes](https://api-docs.deepseek.com/quick_start/error_codes/)
- [Rate limits](https://api-docs.deepseek.com/quick_start/rate_limit/)
- [Updates](https://api-docs.deepseek.com/updates/)
- [V4 model identities](https://api-docs.deepseek.com/news/news260424/)

M21 performed no network revalidation because its contract was explicitly
local-only. It does not make a newer claim about mutable model names, pricing
or provider behavior.

## Non-conclusions

M21 does not prove:

- which upstream component ended the M20B response body;
- the missing sixth-response usage or billing amount;
- M20B's planned nine-arm baseline is complete;
- the TypeScript arm is a measurement-valid product success;
- a retry would reproduce the same output or be free of duplicate side effects;
- fixed-Pro quality, cost or reliability changed.
