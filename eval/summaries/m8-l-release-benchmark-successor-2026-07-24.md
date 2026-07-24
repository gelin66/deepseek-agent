# M8-L V13/V16 release benchmark successor

- 日期：2026-07-24
- 分支：`deepseek-agent`
- clean baseline：`4fef6a34e999a8d592629308cb1b5fd7017bbf18`
- baseline tree：`239316f1adda010c3ab684a7782d3128ae5fcf76`
- frozen manifest/Harness commit：`14319b11`
- protocol：Run API v11 / RuntimeEvent v17 / State v23 / exec-stream v3
- credential read / official API requests：false / 0

## 1. 结论

imported `352e86a6` 与 current 的 task/model/API surface 不是主要不可比项。exact imported
release binary 已能使用 official `/chat/completions` 与 `deepseek-v4-pro`；同一外部
fixture、deterministic verifier、binary/prompt/tool identity 与 wall-time boundary 也能
冻结。

不可比的是 imported 原生没有记录的物理事实：

- exec-stream v1 声明 `retry_count`，三个 production terminal 构造点却都写 `None`；
- 没有 `api_request_count`、started/in-flight、failed/incomplete response usage、
  root/child aggregate 或 cost completeness/bucket；
- 旧 Engine 内至少两层会重新发送相同 stream request，`TurnComplete` 只携带 observed
  aggregate usage；
- Engine/SessionManager 与旧 app-server 没有 canonical request ledger、
  `TaskContract`/`EvidenceReceipt` 或逐 actor SQLite reopen truth。

因此 imported/current paid A/B 在 Key/API 前判定：

```text
inadmissible_incomplete_baseline_accounting
```

不修改旧 revision，不建立 legacy adapter，不用外部价格估算补猜账单，也不把缺失字段当
零。这个结论不证明 current 比 imported 成功率更高、Token 更少、速度更快或费用更低。

ADR-0006 接受一个可删除、可复核的 release benchmark successor：

1. M5-A 12/12 qualified official DeepSeek arms 提供窄范围 real coding/false-success
   evidence；
2. exact current candidate 通过唯一 production chain 的 Git edit、verifier recovery、
   false-completion rejection、accounting/RequestPlan reopen 与 CLI resume regression；
3. login、interactive start、headless coding、run inspection、resume 五个共同 workflow
   的显式用户动作数为 `5 -> 5`。

V13/V16 因而关闭；current V1 matrix 为 15 pass / 1 blocked。V15 billing-provable
Simplified Chinese Agent prompt comparison 仍 blocked，V1 仍不可发布。

## 2. Frozen contract

[`m8-l-release-benchmark-successor-v1.json`](../manifests/m8-l-release-benchmark-successor-v1.json)
在 authority 变化前冻结：

- baseline revision/tree、Cargo.lock、Rust 1.97.0、Run/State/exec identities；
- imported tree、六个 source blob 与 M8-H exact release binary hashes；
- imported/current 逐 fact comparability matrix；
- M5-A summary/raw SHA-256、0600 mode、evaluation ID 与 aggregate；
- current 五个 exact Cargo test filter；
- five-workflow atomic action contract；
- `maximum_reruns=0`、credential/API/network=false、ignored 0600 single output；
- `reject imported paid A/B / keep successor / close V13+V16 / leave V15 blocked`。

Harness 不实现第二 model loop、tool、verifier、reducer 或 accounting projector。它只读取
immutable source/evidence，调用 production-owner 的 exact tests，并把 process verdict
写入一个不可覆盖的 private result。

## 3. Imported comparability

| Fact | imported | current | 可用于 paid pair |
|---|---|---|---|
| official Chat/model | `/chat/completions` + V4 Pro | 同 | 是 |
| external task/verifier | 可冻结 | 可冻结 | 是 |
| binary/prompt/tool identity | terminal 可投影 | canonical 可投影 | 是 |
| wall time | exec boundary | exec boundary | 是 |
| physical requests/retries | request count absent；retry null | root/child + retry typed | 否 |
| incomplete usage | absent | typed missing/incomplete | 否 |
| cost completeness | observed-turn scorecard only | canonical bucket/completeness | 否 |
| crash/reopen | Engine/session aggregate | RunStore exact ledger | 否 |

旧 scorecard 可以给已观察 turn 定价，但不能知道透明 retry 是否到达 provider、是否产生
usage，或 crash 时是否存在未完成 attempt。任何外部补账都需要把未知事实猜成零或已知值，
违反 evaluation 的 fail-closed accounting 规则。

## 4. Qualified real-model evidence

M5-A raw：

- path：`eval/results/m5-completion-gate-ba25f836-vs-503d6294.json`
- mode：`0600`
- SHA-256：`42ac722d6f25abe6b066eee850ef5190b2e0367dfd91bed00472bad652e7b05c`
- model/surface：`deepseek-v4-flash` / Standard Chat
- schedule：2 scenarios × 2 revisions × 3 repetitions = 12/12 arms
- `product_metric_eligible=true`

| Cell | Result |
|---|---|
| coding baseline | 3/3 verified |
| coding candidate | 3/3 verified；0 false-success |
| forced false claim baseline | 3/3 false-success |
| forced false claim candidate | 3/3 correct rejection；0 false-success |

12/12 arm 的 measurement、contract、completion gate 与 accounting 都有效，总计
40 physical requests、144,903 tokens、USD 0.004110153。

边界：这只覆盖一个 frozen Python coding task 和一个 false-claim 反例，而且 candidate 是
current canonical chain 的祖先。它提供 real-model evidence，不能单独证明 exact current
source；因此 M8-L 还要求下面的 current production retention gates。

## 5. Exact current benchmark

| Case | Owner/test | 验收 |
|---|---|---|
| R01 production coding | app `m7c_production_loopback_verifier_failure_recovers` | 临时 Git repo，`before -> broken -> after`，verifier fail→pass，latest receipt，5 requests，reopen 0 request |
| R02 false success | runtime `model_completion_is_rejected_when_the_frozen_verifier_fails` | verifier fail 时拒绝 model completion |
| R03 replay/accounting | app `m7i_actor_request_plans_rebuild_from_v16_sqlite_events` | root/read-only/Writer RequestPlan/accounting exact reopen |
| R04 workflow surface | CLI `dispatcher_help_exposes_runs_and_removes_retired_top_level_commands` | 唯一 canonical `runs`，旧 session/thread/fork/run 不可见 |
| R05 recovery step | TUI exec `completed_exec_resume_replays_the_same_terminal_without_another_model_request` | 一次 resume 重放终态且不再请求模型 |

这些 gate 使用 local loopback 和真实临时 Git/SQLite/process path，只证明 exact current
production mechanism retention；它们不是 live DeepSeek quality arms。

## 6. Workflow steps

步骤单位是一次显式 command submission 或一次 TUI start：

| Workflow | imported | current |
|---|---:|---:|
| login | 1 | 1 |
| interactive start | 1 | 1 |
| headless coding | 1 | 1 |
| inspect local executions | 1 (`sessions`) | 1 (`runs`) |
| resume newest | 1 | 1 |
| total | 5 | 5 |

Runtime event、model request、内部 verifier step 不由用户逐项操作，不计入 user action。
Fleet/Lane/Thread/session metadata 是已删除的 optional/duplicate concepts，不是 required
imported steps，不能为了增加“兼容步骤”恢复。

## 7. Authority cutover

- ADR-0006 接受 release benchmark successor；
- PRODUCT_PLAN 仍有 16 项 completion definition，只把 V13 从不可计量的 imported
  superiority 收敛为 qualified real coding evidence + exact current retention；
- V16 明确以共同 workflow 的显式用户动作非增加为准；
- ROADMAP、EVALUATION 与 CURRENT_CODEWHALE 使用同一 15 pass / 1 blocked truth；
- production crates、DeepSeek ChatCompletions、默认 fixed Pro、Runtime、Store、protocol
  与工具目录均无变化。

## 8. 离线门禁

最终门禁使用：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8l-target
```

正式 result、candidate identity、五个 exact gate、focused、fmt、workspace Clippy/test 和
最终 cleanup 将在 clean candidate 上执行并在本摘要的 final checkpoint 中封存；在此之前
不读取 Key、不发 official request。

## 9. Keep / reject / hold

**Keep**

- qualified M5-A official DeepSeek evidence；
- exact current production release regression；
- five-workflow user-action contract；
- imported immutable black-box reference。

**Reject**

- 缺失 legacy accounting 的 paid imported/current A/B；
- 修改旧 revision、外部估算补账或把 unknown 当零；
- 为 benchmark 恢复旧 Runtime/Store/Fleet/Lane/Thread/session compatibility。

**Hold**

- V15 Simplified Chinese Agent prompt；
- Auto default admission 以及任何新的 model-visible optimization。

## 10. 非结论

M8-L 不证明：

- current 相对 imported 的 success、Token、cache、wall-time 或 API-cost delta；
- 一个 M5-A coding task 可以代表所有仓库、语言或 Agent lane；
- offline loopback 等于真实 DeepSeek coding quality；
- V15 已关闭或 V1 可以发布；
- Anthropic、旧 model alias、FIM、LLM router、Provider/模式/兼容层有重新进入理由。
