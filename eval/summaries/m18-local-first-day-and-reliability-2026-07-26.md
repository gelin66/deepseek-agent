# M18 DSE local first-day and reliability baseline

Date: 2026-07-26

Decision: `insufficient_repeated_current_loss`

Product delta: none

## Scope

M18 tested the already accepted local DSE V1 rather than introducing a new
capability. The slice had two independent gates:

1. exercise the installed `dse`/`dse-tui` lifecycle from a fresh `DSE_HOME`;
2. acquire current fixed-Pro/high coding trajectories with deterministic
   external verifiers and select a production candidate only if one stable loss
   repeated across at least two independent tasks.

GitHub, remote CI, push, repository rename, visibility, tag and release were
outside the slice. Auto, a second Runtime/Store/Provider and unmeasured product
changes remained prohibited.

## Frozen identity

- source candidate: `eee72cb38295f687b4fc1f13d47fae6f3c30100d`
- source tree: `c7788e78a2307ec4d1ed8ace8b523a3c43099458`
- live admission: `5b3c943bb`
- release binary: `/private/tmp/dse-m18-target/release/dse`
- binary version: `dse 0.8.68 (eee72cb38295)`
- binary SHA-256:
  `cd626dde77784e8e1efe07397f74557aea9cd35293fd64f3d3dedfd5642178b2`
- manifest SHA-256:
  `79bab3787248da9dcaee51781022793a761f01f976b41eb018708b3a01887869`
- schedule SHA-256:
  `ac24ef32273a4a339ff636f2ee0efae2cd31dd9b2b58ceeb56eca3d8610c999b`
- TaskContract aggregate SHA-256:
  `55c528f2c1e0c7e57a023fcc1cf7028f9b6e77cfcb2c80802c334e6c980bb92c`
- verifier-environment SHA-256:
  `0137309de9201422380264ffa1158e147e2da2e20d8f76d5ca28b27bfcea65ae`
- Run API v12, RuntimeEvent v19, State v25, exec-stream v4
- official model surface: `deepseek-v4-pro`, reasoning `high`, official
  ChatCompletions sender
- maximum reruns: 0

The six frozen task families covered scoped Rust editing, TypeScript production
chain localization with decoys, verifier recovery, a read-only child, an
explicit isolated Writer and a correct no-write safety rejection. Each family
was scheduled three times at position 1.

## Local first-day lifecycle

The exact-source locked/offline delivery owner built and installed DSE into an
isolated temporary root and fresh `DSE_HOME`. The following all passed locally:

- source package identity and install verification;
- English and `zh-Hans` first-run TUI flows through real PTYs;
- installed `dse exec`;
- same-Run resume without a second model request;
- upgrade to a second exact-source package;
- rollback to the first package;
- uninstall while preserving the user configuration byte-for-byte.

The preserved configuration SHA-256 was
`fef533b039d61301aaf88c19010312a549bbd29902649da09eeeb6f8ebfd30d6`.
One test-only correction makes the installed TUI acceptance test prefer
Cargo's runtime `CARGO_BIN_EXE_dse-tui` path over its compile-time fallback.
No production behavior changed.

## Formal acquisition

The credential-free dry run, fixture failure gate, reference-patch proof,
Harness self-test, M14 observer conformance, M16 acceptance conformance and the
full local Rust gate passed before credential access. The Key was read only by
the existing Harness from the user-authorized `0600` file; it was not printed,
logged or committed.

The official acquisition stopped before the sixteenth terminal snapshot when
the third Writer arm reached the frozen Run deadline. The Harness did not retry,
complete mates or run the final two scheduled arms.

The retained ignored `0600` journal:

- path:
  `eval/results/m18-local-reliability-eee72cb38295-v1.jsonl`
- records: 94
- size: 29,326,862 bytes
- partial tail: 0 bytes
- SHA-256:
  `5ace3b0b2222428991eec205f1ac9dc6a3f81dbc09beef88873bcb95343ef9c5`

The read-only trajectory projection reproduced byte-identical canonical JSON
across two runs:

- report SHA-256:
  `b031670020531b40ec5109184556dcc567295b83c3f4105b2d9ed0703839534f`
- complete Store trajectories: 15
- positive verified successes: 11
- correct safety rejections: 3
- false success: 0
- deterministic verifier failures: 1
- measurement interruptions: 1 `writer_policy_migration` Run deadline
- model requests in the 15 complete trajectories: 120

The one product loss occurred in the second `typescript_request_id` trajectory:
the external verifier failed and the Host correctly blocked completion. The
other two repetitions of that task passed. Its stable loss code therefore
appears in only one independent task family. The interrupted Writer has no
terminal Store snapshot and remains a measurement interruption; M18 makes no
claim about its physical request accounting or product outcome.

## Decision and deletion

The pre-registered candidate gate requires one identical stable loss across at
least two distinct frozen task IDs. M18 observed only:

```text
deterministic_verifier_failed -> ["typescript_request_id"]
```

The decision is `insufficient_repeated_current_loss`. No production candidate,
tool change, recovery controller, prompt change, retry, rerun or mate completion
is admitted. Fixed root/Writer Pro-high, fixed read-only Flash-high, typed
Pro-max recheck, the single AgentRuntime/RunStore and the existing canonical
tools remain unchanged.

The analysis-only Harness extension is retained because it has one exact M18
consumer and prevents a started-without-snapshot deadline arm from being
manually extrapolated into product or accounting truth. No experiment-only
production branch exists to delete.

## Local gates

- `./scripts/test-dse-delivery.sh`
- installed package/install/verify/upgrade/rollback/uninstall lifecycle
- M18 fixture initial-failure and reference-patch proof
- M18/M15/M9-C Harness self-tests and four journal SIGKILL windows
- M14 observer conformance and M16 acceptance conformance
- `./scripts/dev-dse.sh focused`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- root/read-only/Writer, exact reopen, crash/replay, CLI/TUI/API loopbacks
- `git diff --check`

All gates were executed locally. No GitHub or remote validation is used as an
M18 completion condition, and no push was performed.

## Official protocol revalidation

Reviewed 2026-07-26:

- [DeepSeek V4 release and model names](https://api-docs.deepseek.com/news/news260424/)
- [DeepSeek model and pricing contract](https://api-docs.deepseek.com/quick_start/pricing/)
- [ChatCompletions request, streaming usage and reasoning fields](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking-mode replay contract](https://api-docs.deepseek.com/guides/thinking_mode/)

The review confirms that current DSE continues to use the official
ChatCompletions endpoint with `deepseek-v4-pro`; retirement of legacy model
aliases is not retirement of ChatCompletions. M18 introduced no Anthropic
surface, compatibility transport or alternate endpoint.

## Non-conclusions and next evidence

M18 does not establish a complete 18-arm regression aggregate, a TypeScript
recovery defect shared by multiple task families, a Writer product loss, or an
efficiency/cost comparison. A successor must use a new frozen identity rather
than completing, mating or rerunning this journal. It should add independent
tasks and make deadline/accounting terminal truth explicit before credential
access. Production implementation remains forbidden until a stable loss repeats
across at least two independent tasks.
