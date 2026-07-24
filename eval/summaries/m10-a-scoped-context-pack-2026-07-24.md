# M10-A scoped context pack（2026-07-24）

## 结论

M10-A 的正式决定是：

```text
stop_incomplete_accounting
do not admit pack_off
keep the fixed pack_on production prompt
delete the pack_off user/config/Harness treatment path
keep the derived prompt ledger and frozen evidence
do not proceed to nested scoped-rules treatment
```

正式 A/B 从新的 manifest、live admission、candidate binary、position 1 和空 raw
开始。预注册 36 arms 中有 21 个形成完整 `arm_result`；第 22 个
`writer_migration / pack_off` 在第一个模型响应已收到 headers 和一段 reasoning、但尚未
收到 trusted finish、`[DONE]` 或 usage 时发生 typed `deepseek_transport`。
Runtime 正确保存 `retry_safe=false`、`usage_incomplete=true` 并停止，Harness 没有重跑、
补 mate、续跑、resample 或拼接。

因此不存在完整 6 tasks × 2 variants × 3 runs 矩阵，`product_metric_eligible=false`。
不能对 verified success 非劣、10% cache-miss 降幅、时间或费用作产品结论。production
默认保持 project context pack；未准入的 pack-off 产品和 runner 双轨物理删除。

## 冻结身份

- candidate revision：
  `14c922de30669777e9a7a83e176b4649074f563b`；
- candidate tree：
  `c5a9ecb19ad9684806fe2fe3a01b93351b4d037f`；
- live admission commit：`5df74561`；
- candidate binary：`codewhale 0.8.68 (14c922de3066)`；
- binary SHA-256：
  `ba37eb5b49e0c451fb77947241a88051b0c2c9af269dcbec8a70c2eea985ed2d`；
- binary size：79,087,944 bytes；
- M10-A manifest SHA-256：
  `d50a181919a4306d1b9a88f0974c7972296e7035a92d9b1b5a0e8aa58d53cd86`；
- inherited M9-B manifest SHA-256：
  `00fc21663441dafc28370002305f19ed2a212c119fbe38140abbac6fad89f0c5`；
- frozen Harness SHA-256：
  `6887eedae3dc5fcdeb37717707268b832cecd6023957795fadf48864f9b7d3e2`；
- live admission SHA-256：
  `6cdd1fee5adcd9e66fa86a92e4c637338a629cd365ce306df1028742388d0c15`；
- schedule SHA-256：
  `3d2c0112d67968c34e2c8f1a94c7a0de61dc447477d0e6e60f3c70dc383e5bbd`；
- task-contract SHA-256：
  `9bf479bbe6b866ace512cdfbd7c634454eff2260e73b9ec7f42338037a200648`；
- Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3。

两个 variant 使用同一 binary、official DeepSeek OpenAI-format ChatCompletions
`POST /chat/completions`、`deepseek-v4-pro`、reasoning `high`、TaskContract、工具、
verifier、预算和交错 schedule。唯一 delta 是隔离 config home 中的：

```text
pack_on  -> context.project_pack=true
pack_off -> context.project_pack=false
```

transport/runtime retry 与 `maximum_reruns` 均为 0；每 arm 的 known cost ceiling 为
`$0.08`，suite ceiling 为 `$2.88`。

## 离线门禁

读取 Key 前通过：

- manifest/inherited sections、fixtures、acceptance IDs、36-arm balanced schedule、
  TaskContract 与所有 identity hash；
- pack-on/off system-prompt marker 与唯一 fallback overview；
- 0600 exclusive hash-chain journal、tamper rejection 和四个 SIGKILL window；
- same-binary config parity、invalid config fail-closed；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- fixed root/read-only/Writer route、latest-revision verifier、RunStore reopen、
  process crash/recovery、CLI/TUI/API parity；
- immutable candidate build、credential-free dry-run 和 `git diff --check`。

第一次 workspace test 中一个 Writer cleanup crash-marker 等待超时；相同测试隔离通过，
完整 39-case process suite 通过 38/0/1，随后新的 workspace 全量测试通过。没有 production
改动介于这些验证之间。

## 正式停止证据

ignored raw：

- `eval/results/m10-a-scoped-context-pack-14c922de3066-v1.jsonl`；
- mode `0600`；
- 31,169,005 bytes；
- 134 个完整 hash-chained records、无 partial tail；
- raw SHA-256：
  `0c81935b2e6c4afb2e80c76d0a3746689e068a90c17ee16644176b8d710a0d66`；
- final record SHA-256：
  `0e4aa3ebdf7f482443991fe60c3824aadf41749ef89a9599f091c93bbfc36ae0`。

record set 是 1 plan、1 credential access、22 arm started、22 terminal、22 canonical
Store、22 credential-free SQLite reopen、22 verifier、21 arm result 和 1 abort；没有
summary。

21 个完整 observations 的描述性合计是：

| variant | arms | verified | correct rejection | frozen raw false-success label | requests | known cost |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `pack_on` | 11 | 9 | 1 | 1 | 72 | `$0.095994292` |
| `pack_off` | 10 | 8 | 2 | 0 | 54 | `$0.077778957` |

合计 126 个完整 response/usage、input 552,333、output 51,217、cache hit 347,136、
cache miss 205,197、reasoning 28,956、reasoning replay 70,595、known cost
`$0.173773249`、aggregate arm wall 1,110,877 ms。

这些 arms 数量不平衡，缺少后续预注册 cells，且第 22 个 physical response usage
不完整；上表不是 variant 比较或产品指标。部分样本的 median cache-miss input 是
`pack_on=9,512`、`pack_off=9,585`，既没有达到预注册 10% 降幅，也不能从 stopped
campaign 推导负向结论。

第 22 个 scheduled arm 是第 4 轮 `writer_migration / pack_off`。第一个 root request
发生：

```text
response_headers_received = true
reasoning_observed = true
content_observed = false
finish_reason_observed = false
stream_done_received = false
usage_received = false
retry_safe = false
retry disposition = stop
root started/completed/in_flight = 1/1/0
child started/completed/in_flight = 0/0/0
billing_unknown = false
usage_complete = false
usage_incomplete = true
accounting complete = false
sealed = true
```

这不是 Writer 机制、worktree 或 verifier failure；Writer child 尚未创建。response
headers 使该 attempt 不属于 M9-C 的 pre-header `billing_unknown` 窗口，但缺失 usage
仍使完整成本与 Token 真相不可得。abort 为 `accounting_incomplete`，
`completed_arms=21`。

## frozen false-success label 的观察器纠错

frozen raw 把 arm 2 `root_recovery / pack_on` 标为 1 个 false success。逐事件取证证明
这是 Harness observer 的 false-positive label，不是 production Stop Gate 漏放：

- model 先通过 `run_verifiers` 得到稳定 `verifier_failed`；
- 随后只修改 `retry_delay.py`；
- canonical Host 在 completion proposal 后运行同一冻结 verifier；
- committed `EvidenceReceipt` 的 policy 为 `failed_write_pass`，lineage 精确绑定失败
  revision、有效 mutation 和 latest-revision final pass；
- terminal completed、expected file 精确、external verifier passed、route/context/
  accounting/SQLite reopen 全部有效。

旧 `root_lane_audit` 却额外要求 model 自己在 mutation 后再调用一次 `run_verifiers`。
这与冻结 objective 的“最后提出完成，由 Host 形成最终通过证据”冲突，也重复了 canonical
Host receipt 的权威。raw 不改写；post-decision Harness 改为接受：

```text
model verifier failure -> applied mutation -> committed Host verification pass
```

并增加正向与缺失 Host pass 的负向 self-test。这个纠错不使 stopped M10-A 获得产品
资格，也不用于补算或重标 frozen raw。

## Cutover 与删除

M10-A 没有达到 admission gate。decision cutover 因而：

- 固定 production prompt 继续生成 project context pack；
- 删除 `context.project_pack` config/example/reference、CLI/TUI/app 接线、
  `ProductionPromptConfig`/`ProductionPromptRequest` bool 和 pack-off assembly 分支；
- 旧 config key fail closed，不保留 compatibility reader、dual behavior 或用户模式；
- 从唯一 fixed-Pro Harness 删除 M10-A flag、variant schedule/config、prompt audit、
  treatment aggregate 与 admission 分支；
- 保留 frozen manifest、live admission、本 summary、ignored 0600 raw 和对应 Git
  history；
- 保留低开销 derived prompt fragment ledger，以及证明 fallback overview/pack 重复的
  offline characterization；它们没有生产 prompt byte 或状态副作用。

不继续 nested scoped-rules treatment，因为其前置 pack-off admission 未满足。下一条
独立产品切片是 M10-B Budgeted Working-Set Selector，继续使用固定 pack-on、fixed Pro、
单变量、external verifier 和 complete-accounting gate。

## 官方复核

复核日期为 2026-07-24：

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)；
- [DeepSeek change log](https://api-docs.deepseek.com/updates/)；
- [current models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)。

CodeWhale 继续使用 official DeepSeek OpenAI-format ChatCompletions。M10-A 没有
Anthropic Messages、FIM、Auto、Provider、第二 Runtime/Store 或第二 evaluator。
