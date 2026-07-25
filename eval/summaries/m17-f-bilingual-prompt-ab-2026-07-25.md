# M17-F DSE bilingual prompt 2x2 decision

Date: 2026-07-25
Decision: `retain_chinese_block1_quality_veto`

## Problem and owner

The previous prompt experiments mixed language, structure, length, and content.
M17-F isolated one variable: the expression language of DSE-owned system-prompt
text. `crates/context` remained the sole production prompt owner, and the
corrected fixed-Pro Harness remained the sole evaluator. The experiment did not
add a product mode, language classifier, translation request, second Runtime,
second Store, or alternate model route.

## Frozen identity

- starting revision: `5b5f7b4d8b27c0707888aaee60f8320af39b1d71`;
- contract/assets commit: `5a18d2376`;
- formal candidate: `73d02d05e36dc6da58814b2b3183195aac6f4930`;
- candidate tree: `9b5d708ee3550a34c18dd52be7ce6542d1bbcc7c`;
- live admission commit: `2cb98d18b`;
- cutover commit: `c1856fa4b`;
- binary: `dse 0.8.68 (73d02d05e36d)`;
- binary SHA-256:
  `adc519c179b2f3480619f8c46e19a8c02599cd4ed32a9bc4464630e91aed2a17`;
- model/surface: `deepseek-v4-pro`, `high`, streaming official
  `POST https://api.deepseek.com/chat/completions`;
- schedule: 8 task families × 2 task languages × 2 prompt languages, block 1
  positions 1–32; block 2 was pre-frozen but conditional;
- maximum reruns: `0`;
- schedule SHA-256:
  `869434cd0d78d12b103bff5f00806f53ed47a5de703278d23acccaa0d0f5d352`;
- task-contract SHA-256:
  `03cd40da1653fb2e999885351c35baefa767eaf9436da0276b2bf9dc51a5b8cf`;
- frozen Chinese prompt normalized SHA-256:
  `a91799031d8f430945e98871f19d3cefd0496834304b4af0ff04197944ab1bdb`.

Before credential access, the fixture/tree/reference proof, balanced schedule,
prompt hashes, exact binary, route, actor lanes, verifier environment,
hash-chained journal crash windows, focused checks, fmt, strict Clippy, and
workspace tests passed. The first live entry attempt stopped before journal
claim, Key access, or network with `output_scope_invalid`; the Harness was
corrected so only M17-F could claim its manifest-bound ignored `eval/raw`
directory, then the binary and admission were refrozen.

## Formal result

The campaign completed block 1 and stopped at its pre-registered quality gate:

| Fact | Result |
|---|---:|
| Measurement-valid arms | 32 / 32 |
| Positive verified success | 26 |
| Correct safety rejection | 4 |
| False success | 0 |
| Physical requests | 263 |
| Input tokens | 1,914,399 |
| Output tokens | 159,125 |
| Cache-hit input tokens | 1,209,344 |
| Cache-miss input tokens | 705,055 |
| Known cost | USD 0.511237723 |
| Aggregate wall time | 2,754,758 ms |
| Reruns | 0 |

Both prompt languages produced 6/7 verified successes on English tasks and 7/7
on Chinese tasks, and both correctly rejected both safety-task languages.
Aggregate equality was not enough for admission: the English prompt had one
pre-registered treatment-only loss on
`rust_scoped_rules:en:verified_success`, while false success, route identity,
actor-lane identity, prompt identity, and accounting remained valid. Therefore
`english_noninferior=false`; the Harness wrote
`retain_chinese_block1_quality_veto` and did not run block 2.

The ignored `0600` journal contains 195 complete SHA-256-chained records, no
partial tail, and file SHA-256
`ecdb74675207081104c09f3be0910f3d646ab0e94b0e8e458543935f915500d5`.
It is not committed and was inspected only through the Harness chain reader and
aggregate fields.

## Cutover and deletion

Production keeps one Chinese-expression system prompt and now instructs the
Agent to answer in the user's current task language unless explicitly
overridden. The production prompt fixture exactly matches the frozen Chinese
normalized hash above.

The cutover physically deleted:

- both temporary `DSE_M17F_*` selector variables and all selector branches;
- the English prompt candidate;
- translated project-context, skills, environment, and truncation scaffolding;
- all six tracked prompt experiment assets;
- the M17-F-only campaign from the corrected Harness.

The frozen contract, admission, this summary, ignored `0600` raw, and Git
history remain the audit trail. There is no dual production prompt,
compatibility reader, Auto route, classifier, or translation model.

## Non-conclusions

- This does not prove that Chinese prompts are universally better than English
  prompts.
- It does prove that this English candidate failed the pre-registered
  non-inferiority gate and cannot replace the current prompt.
- Token, time, or cost differences cannot offset the treatment-only task loss.
- The 32 unexecuted block-2 arms must not be supplemented, rerun, or represented
  as observed data.

## Official protocol review

Revalidated on 2026-07-25:

- <https://api-docs.deepseek.com/api/create-chat-completion/>
- <https://api-docs.deepseek.com/quick_start/pricing/>
- <https://api-docs.deepseek.com/guides/thinking_mode/>
- <https://api-docs.deepseek.com/guides/tool_calls/>
- <https://api-docs.deepseek.com/news/news260424/>

The official surface remained ChatCompletions. The 2026-07-24 retirement
concerned legacy model aliases, not the `/chat/completions` endpoint.
