# ADR-0006：V1 使用可复核 release benchmark，不伪造不可计量的 imported A/B

- 状态：已接受
- 日期：2026-07-24

## 决策

V1 不再要求对 imported `352e86a6` 产生一个旧臂无法原生计量的付费“优于”结论。
imported revision 继续作为 immutable black-box workflow reference；只有两臂都能原生提供
同口径 physical request、usage、retry、cache、wall-time、cost 与 reopen truth 时，才允许
跨 revision 付费 A/B。

V1 的 coding release benchmark 改为一条可复核 evidence chain：

1. 至少一份正式官方 DeepSeek coding evaluation 必须冻结 task、fixture、immutable
   binary、model/surface、deterministic verifier、request/usage/cost 与 false-success，
   并且 `product_metric_eligible=true`。
2. exact current candidate 必须通过
   `AgentApplication -> AgentRuntime -> RunStore` 的临时 Git workspace、有效 edit、
   verifier failure/recovery、latest-revision receipt、false-completion rejection、
   root/read-only/Writer RequestPlan/accounting 与 SQLite reopen 回归。
3. 以后任何改变 model-visible prompt、model、reasoning、tool catalog、budget、routing
   或 DeepSeek surface 的候选，仍须用 current same-revision immutable control/treatment
   独立评测；本 ADR 不允许用祖先结果替代新 treatment 的产品证据。

M8-L 接受 M5-A 的 12/12 official DeepSeek arms 作为第 1 项的窄范围 real-model evidence：
固定 coding task 两侧均 3/3 verified，candidate false-success 为 0；强制假完成反例从
baseline 3/3 false-success 变为 candidate 3/3 correct rejection，12/12 accounting
完整。M8-L current benchmark 负责第 2 项的 exact-source retention。两者组合不产生
“current 比 imported 成功率更高/更便宜”的结论。

V1 workflow-step gate 只计算完成同一用户目标所需的显式用户动作（一次 command
submission 或一次 TUI start），不把 Runtime event、model request 或内部 verifier step
算成用户动作。冻结共同流程为 login、interactive start、headless coding、run inspection
与 resume；imported/current 都是每项 1 次、总计 `5 -> 5`。已删除的 Fleet、Lane、Thread
和 session metadata 不是 required imported steps，也不恢复为 compatibility workflow。

完整冻结输入见
[`m8-l-release-benchmark-successor-v1.json`](../../eval/manifests/m8-l-release-benchmark-successor-v1.json)。

## 原因

- imported exec-stream v1 声明 `retry_count`，但三个 production terminal 构造点都固定为
  `None`，也没有 `api_request_count`、started/in-flight ledger、cost completeness/bucket
  或 canonical root/child aggregate。
- 同一旧 Engine 至少有两层会重新发送相同 `create_message_stream` request 的透明 retry；
  `TurnComplete` 和 session persistence 只保留已观察到的 aggregate usage。单独的
  scorecard 可以给已观察 turn 定价，但不能重建未观察 attempt、incomplete response 或
  crash window。
- exact imported release binary 已证明能走相同 official
  `/chat/completions` 与 `deepseek-v4-pro`，外部 task fixture 和 deterministic verifier
  也能保持一致。因此 blocker 是 baseline accounting 缺失，不是 Harness 应通过估算、
  修改旧 revision 或 compatibility adapter 掩盖的问题。
- M5-A 已提供小而完整的 real-model coding/false-success evidence；current production
  又能在 exact revision 对相同 completion/evidence owner 做确定性 retention。把两种证据
  明确分层，比重新消费 Key 后得到不可解释的旧臂成本更可审计。
- M8-G/J 已删除 Fleet/Lane/legacy Thread truth，并证明 current common CLI workflow
  没有增加用户动作。为了旧概念恢复额外命令会增加产品复杂度，而不是改善 V16。

## 后果

- V13 改为“合格的真实官方 DeepSeek coding evidence + exact current release benchmark
  retention”，不再要求不可计量的 imported superiority claim。
- V16 由五个共同 workflow 的 atomic action comparison 与 current process gates验收。
- V13/V16 关闭后 current V1 matrix 为 15 pass / 1 blocked；V15 billing-provable
  Simplified Chinese Agent prompt comparison 仍 blocked，因此 V1 仍不可发布。
- imported `352e86a6`、M5-A raw 和全部 frozen historical evidence 保持 immutable；
  不读取 Key、不调用 official API、不修改旧 revision、不外部补账。
- production model、ChatCompletions sender/parser、AgentRuntime、RunStore、工具目录、
  protocol version 与默认 fixed Pro 均不改变。
- 本决策不证明 current 相对 imported 的 success、Token、wall-time 或 API-cost delta，
  也不把 loopback 当作 live model quality。
