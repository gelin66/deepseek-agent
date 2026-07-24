# M10-B budgeted Working-Set（2026-07-24）

## 结论

M10-B 的正式决定是：

```text
reject_quality_veto_and_delete
stop_incomplete_accounting
do not admit the budgeted Working-Set selector
restore the single fixed-pack production prompt path
delete the selector, treatment surface, fixture, benchmark, and Harness branch
preserve frozen evidence
advance to M10-C Acceptance Progress
```

正式 fixed-Pro A/B 从新的 manifest、live admission、immutable candidate binary、
position 1 和空 raw 开始。前 5 个 arms 都形成完整 `arm_result`；其中 4 个 verified
success，1 个 `readonly_investigation / working_set_on` 违反冻结 actor contract 并形成
false-success label。预注册 admission 要求绝对 `false_success=0`，因此候选已经触发不可
抵消的质量 veto。

第 6 个 `safety_false_completion / working_set_on` 在 response headers/usage 前发生
typed `deepseek_transport`。canonical accounting 保存 `billing_unknown=true` 并停止，
没有重跑、补 mate、续跑、resample 或拼接。矩阵不完整且 task/variant 不平衡，因此
不能比较 discovery、Token、时间或费用。质量否决与计费停止是两个独立事实。

## 切片契约

- 真实问题：root 在开始有效修改前可能进行宽泛、重复的仓库 discovery；
- acceptance：fixed-Pro end-task verified success 非劣、false success 0、关键 stratum
  无 treatment-only failure，并在完整 accounting 下使有效修改前调用数或至少一项效率
  指标取得稳定净改善；
- owner：`crates/context` 的既有 ContextBroker 路径；
- treatment：有界确定性 ranked regions，真实 caller 位于现有
  `AgentApplication` composition；
- 被替代旧路径：只有准入后才替代启动时宽泛 discovery；底层 `rg`/read 工具不替代；
- cutover：质量、accounting 或净收益任一失败即完整删除 treatment，不保留模式、兼容
  reader、第二 store 或永久双轨。

## 冻结身份

- candidate revision：
  `d79ebf43b0d9b86a46f33131b4c733a325507af0`；
- candidate tree：
  `55d9bb0c16efb29b3c0d74c3f6db9b66db4af986`；
- live admission commit：`2ec2e8a1`；
- candidate binary：`codewhale 0.8.68 (d79ebf43b0d9)`；
- binary SHA-256：
  `f5e6afea1013dd5f89c9fecf95fb84cb906030b005a231270ef26eed9fdf3c8a`；
- binary size：79,842,072 bytes；
- Harness SHA-256：
  `6b4f07ddbd91881d2b6cf67760a1a991b72670049af63edb07b2c728a03f82ca`；
- M10-B manifest SHA-256：
  `e2a5e8914452adcf82a616d52f054aa1332b63d76357364c608b4310c5c21190`；
- live admission SHA-256：
  `c5dafa8c976d55a69f4288e404117c9caffa5fe5f395aa17634895c47eb8e4d9`；
- schedule SHA-256：
  `474760a5d8325b31ab52cd2dd09588633f59a2dfa58588c094374b3af48412c4`；
- task-contracts SHA-256：
  `0aa456731701c3717ed534d9405e64e49986165bc91eda01da6b3f2ff83aed2b`；
- Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3。

两个 variant 使用同一 binary、official DeepSeek OpenAI-format ChatCompletions
`POST /chat/completions`、`deepseek-v4-pro`、reasoning `high`、fixed project pack、
TaskContract、工具、预算、verifier 与交错 schedule。唯一 delta 是隔离 config home
中的：

```text
working_set_off -> context.working_set=false
working_set_on  -> context.working_set=true
```

6 tasks × 2 variants × 3 runs 共 36 个预注册 arms；transport/runtime retry 与
`maximum_reruns` 均为 0。每 arm known-cost ceiling 为 `$0.08`，suite ceiling 为
`$2.88`。

## 离线门禁与机制基线

读取 Key 前通过：

- default M9-C 与 M10-B Harness self-test；
- manifest/admission/fixture/TaskContract/36-arm schedule/identity hash；
- 0600 exclusive hash-chain journal、tamper rejection 与四个 SIGKILL window；
- 8-task 真实临时 Git repo localization benchmark；
- root/read-only/Writer、latest-revision evidence、RunStore exact reopen、
  process crash/recovery、CLI/TUI/API parity；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- immutable candidate build、credential-free dry-run 与 `git diff --check`。

已删除 candidate 的 offline localization 结果是 Recall@5 `1.00`、mean precision@5
`0.881`、median first relevant rank `1`、negative task regions `0`，相同输入两次输出
byte-equivalent。这个结果只证明 selector 机制，不证明 end-task 能力或效率。

## 正式 raw

ignored raw：

- `eval/results/m10-b-working-set-d79ebf43b0d9-v1.jsonl`；
- mode `0600`；
- 7,445,467 bytes；
- 38 个完整 hash-chained records、无 partial tail；
- raw SHA-256：
  `69b44fa8ebcc8555fe102369bbf4fc25c66149aa713a8d4fa2157578f87da1d0`；
- final record SHA-256：
  `f8342ed848e7c4b23fdf87a35b73c152d018b38a45a1d1da4c40eb8d4982f492`。

record set 是 1 plan、1 credential access、6 arm started、6 terminal、6 canonical
Store、6 credential-free SQLite reopen、6 verifier、5 arm result 与 1 abort；没有
summary。abort 为 `accounting_incomplete`，`completed_arms=5`。

## 五个 measurement-valid arms

| task / variant | verified | frozen false success | pre-edit discovery | total discovery | requests | cache-miss input | known cost | wall ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `root_single / off` | 1 | 0 | 2 | 2 | 5 | 7,370 | `$0.005357866` | 34,548 |
| `root_migration / on` | 1 | 0 | 3 | 5 | 5 | 12,230 | `$0.006782810` | 24,241 |
| `root_recovery / off` | 1 | 0 | 1 | 1 | 4 | 4,794 | `$0.003465906` | 25,356 |
| `readonly_investigation / on` | 0 | 1 | 10 | 10 | 8 | 23,489 | `$0.019377365` | 105,868 |
| `writer_migration / off` | 1 | 0 | 5 | 9 | 10 | 14,691 | `$0.018062766` | 79,916 |

描述性总计是 5 arms、4 verified、1 frozen false success、32 requests、input 149,230、
output 12,294、cache hit 86,656、cache miss 62,574、reasoning 5,627、reasoning replay
15,919、known cost `$0.053046713`、aggregate wall 269,929 ms。

off 有 3 个不同任务 arms，on 有 2 个不同任务 arms；任务和数量都不平衡。任何 off/on
分组统计都不是 variant comparison。

## 质量 veto

`readonly_investigation / working_set_on` 的文件结果本身通过冻结 external verifier：

- exact changed file 是 `compatibility.py`；
- child 保持 read-only，root 完成写入；
- Host committed latest-revision receipt；
- terminal 为 completed；
- route、response usage、accounting、Store 与 SQLite reopen 完整。

但冻结 lane contract 要求 root 创建 read-only child 时完整提交
`wall_time_secs=180`。该调用省略了这一字段，所以 actor call keys/arguments 不相等，
`actor_contract_valid=false`，arm 的 `verified_success=false`。terminal 仍 completed，
因此 frozen label 定义产生 `false_success=true`。

这应精确称为“actor-contract nonconformance / frozen false-success label”，而不是声称
目标文件内容错误。它仍然是 measurement-valid 的正式质量失败；预注册 gate 要求
false success 绝对为零，不能由后续成功或成本改善抵消。

## 第六 arm 与计费停止

第六个 scheduled arm 是 `safety_false_completion / working_set_on`。第一个 root
physical request 在 response headers 与 usage 前失败：

```text
terminal failure = deepseek_transport
surface usage = empty
root started/completed/in_flight = 1/1/0
billing_unknown_attempts = 1
billing_unknown = true
usage_complete = false
accounting complete = false
sealed = true
```

Harness 按冻结规则写入 abort 后停止，没有重发 physical attempt。由于 official
contract 不能把该 pre-header attempt 与逐请求账单真相对账，完整 suite 的 Token/费用
与 control/treatment 比较均不可得。

## Cutover 与删除

质量 gate 已失败，所以 M10-B 不接管：

- 删除 `crates/context::working_set`、no-follow safe-read helper、对应 dependency、
  tests 与 benchmark；
- 删除 `AgentApplication` task-aware selector caller、volatile WorldState fragment 和
  prompt marker；
- 删除 `[context].working_set` config、CLI/TUI/API 接线与 parity tests；旧 key
  fail closed；
- 从唯一 corrected fixed-Pro Harness 删除 M10-B flag、variant/config/prompt observer、
  discovery aggregate、admission path 与 current consumer；
- 删除 exact fixture；它仍可从 candidate Git revision 复核；
- production 恢复 fixed project pack、无 Working-Set map 的单路径，不保留
  compatibility reader、第二 store、RepoGraph、embedding、LLM reranker 或模式。

保留 frozen formal manifest、live admission、localization v1/v2 manifests、ignored
0600 raw、Git history 与本 summary。它们是已发生实验的不可变审计证据，不是 current
production 或 evaluator consumer。

下一独立切片是 M10-C Acceptance Progress：只从 canonical TaskContract、ToolOutcome、
workspace revision、verifier observation 与 EvidenceReceipt 派生执行中验收状态，不
新建 plan store、Goal/Hunt、update-plan 工具或第二完成权威。

## 非结论

M10-B 不证明：

- selector 比原路径更快、更省 Token、更便宜或更可靠；
- working-set off 优于 on，或相反；
- offline localization 指标会转化为 end-task verified success；
- 第六个 physical attempt 已计费或未计费；
- 应恢复 FIM、Auto、Anthropic Messages、第二 Provider、RepoGraph service、第二
  Runtime/Store 或额外 evaluator。

## 官方复核

复核日期为 2026-07-24：

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)；
- [DeepSeek change log](https://api-docs.deepseek.com/updates/)；
- [current models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)。

CodeWhale 继续使用 official DeepSeek OpenAI-format ChatCompletions
`POST /chat/completions`。旧模型 alias 退役不等于 ChatCompletions 下线；M10-B 没有
引入 Anthropic Messages、FIM、Auto、Provider、第二 Runtime/Store 或第二 evaluator。
