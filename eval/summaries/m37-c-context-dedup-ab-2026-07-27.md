# M37-C fallback overview / project pack deduplication A/B

Date: 2026-07-27
Decision: `hold_insufficient_or_incomplete_evidence` for the formal acquisition;
`reject_and_delete_candidate` for the unadmitted production treatment.

## Question

M37-A proved that, when a workspace has no explicit project instruction file,
the canonical prompt composer renders the same generated project payload in
both the bounded fallback overview and a second `ProjectContextPack` wrapper.
M37-C asked whether projecting that payload once improves fixed Pro/high task
behavior or efficiency without weakening project authority, safety, Writer or
latest-revision evidence.

The same-binary candidate changed only `crates/context`: it omitted
`generated:project_context_pack` when the fallback overview payload digest was
present and exactly equal. Explicit `AGENTS.md` authority remained byte
identical. Constitution, Runtime, RunStore, tools, permission, actor routing,
budget, verifier and ordinary production default stayed fixed.

## Frozen identity

- starting checkpoint: `46dd1582e98a658b25190297035e05fcbcc0346a`
- corrected contract checkpoints: `f9559d8b9`, `4959a4d4b`
- same-binary candidate checkpoint: `241941733dd5e1bc0818fa3c4f32f01b685ccfb5`
- candidate tree: `e606f4e642322bd0afad29e4f95a9e3ff9d917e3`
- binary: `dse 0.8.68 (241941733dd5)`
- binary SHA-256:
  `8f6649be87063f635cdd8619d5bcf2282b6828b37e5fb4a9a62bf1f0ed278956`
- contract manifest:
  `eval/manifests/m37-c-context-dedup-ab-v1.json`
- live admission:
  `eval/manifests/m37-c-context-dedup-live-admission-v1.json`
- admission SHA-256:
  `6cdc8da0ad84025356fe8132ae61879d7968cbc2a6bdb730102a1708666eb361`
- official surface: DeepSeek OpenAI-format `/chat/completions`, streaming,
  `deepseek-v4-pro`, reasoning `high`
- schedule: 6 task families x 2 variants x 3 fresh runs = 36 arms,
  `maximum_reruns=0`
- cost ceiling: USD 0.50 per arm and USD 10.00 for the suite

The six families covered no-description small and multi-module repositories,
README relevant facts, README decoy/untrusted instructions, localization, an
explicit Writer and a real process SIGKILL/reopen continuity arm. M10-A and
M37-B raw/results were explicitly excluded.

## Credential-free gates

Before reading the test credential, the following passed:

- exact digest-guard context test and all 21 `dse-context` tests;
- explicit-authority byte parity, 3,300-byte bundled core gate and canonical
  prompt/DeepSeek projection tests;
- Harness freeze report, 36-arm dry-run, synthetic aggregate and journal
  crash-window self-test;
- `./scripts/dev-dse.sh focused`, including exec, app-server, process
  crash/reopen and bilingual PTY acceptance;
- `cargo fmt --all -- --check`;
- strict workspace Clippy;
- complete workspace tests and doctests;
- `git diff --check` and the public repository checker.

The immutable candidate binary was rebuilt from a detached candidate worktree
after those gates, then bound exactly in the live admission. An earlier
admission attempt rejected a stale post-test binary hash before reading the
credential or accessing the network; the corrected admission records the
post-gate binary identity.

## Official acquisition

The ignored raw journal is owner-only (`0600`) at
`eval/results/m37-c-context-dedup-241941733dd5-v1.jsonl`. Its SHA-256 is
`fcdebafeb4a876ddb2853435374c816b31ab13fe3dc1084b1d62fbe06614f9e8`.
It remains outside Git and was not edited after the formal stop.

The corrected acquisition read the authorized test credential and started
scheduled position 1 (`rust_netstring_recovery_resume`, control). The model
reached the frozen first-turn `request_user_input` continuity checkpoint, but
the M37-C Harness consumer still classified a non-M30 interaction as an
approval. It therefore rejected the valid user-input event with:

```text
abort(error_code=hardness_user_input_not_admitted, completed_arms=0)
```

The journal contains exactly four hash-chained records: plan, credential
access, arm start and abort. It contains no `arm_result` or summary. At least
one official request occurred, but the Harness stopped before persisting a
canonical arm accounting projection; billed usage and cost are therefore not
closed. No second arm, mate, rerun, historical splice or replacement run was
started.

## Decision and deletion

The acquisition provides zero complete task/variant cells, so it proves
neither quality non-inferiority nor any cache, request, wall-time or cost
benefit. Per the preregistered incomplete-evidence and `maximum_reruns=0`
contract, the formal result is `hold_insufficient_or_incomplete_evidence`.
The Harness defect is not repaired and rerun under the same campaign.

The product candidate is `reject_and_delete_candidate`:

- current pack-on prompt remains the only production path;
- the evaluation environment selector and alternate projection branch were
  deleted;
- the M37-C Harness campaign, aggregation and synthetic consumer were deleted;
- no compatibility branch, config switch, second composer, Runtime state or
  model-visible explanation remains.

The frozen fixture, contract, admission, ignored raw hash and this summary
remain as audit evidence. They do not authorize rerunning position 1 or
constructing a successor from this incomplete attempt.

## Non-conclusions

This run does not show that duplicate projection is beneficial, harmful or
equivalent. It does not measure a token, cache, latency or cost delta. The
formal stop is a Harness admission defect, not a Prompt-quality or DeepSeek
model failure. M37-D/E are not admitted by this result.

## Official sources revalidated

Reviewed on 2026-07-27:

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)
- [Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
