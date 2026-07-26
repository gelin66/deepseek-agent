# ADR-0011：行为真相与 accounting 真相正交

- 状态：已接受
- 日期：2026-07-26

## 决策

DSE 的正式评测从同一 canonical observation 派生两个互不替代、互不推导的维度：

```text
behavior_status:
  verified_success
  correct_safety_rejection
  verified_product_failure
  measurement_interruption
  invalid

accounting_status:
  complete
  usage_incomplete
  billing_unknown
  unpriced
```

`behavior_status` 只由冻结任务身份、workspace outcome、external verifier、production
terminal、最新 workspace revision 的 Host `EvidenceReceipt`、actor/route contract 与
observer identity 决定。production 在冻结 deadline 内以 typed `failed`/`blocked`
terminal 结束且没有最新 receipt 时，只要上述观察均闭合，就属于
`verified_product_failure`；它不会因为 usage 缺失而变成测量中断。

`measurement_interruption` 只用于 Harness、机器、外部基础设施或人工中断导致 production
outcome 没有形成的情况。identity、task input、workspace、environment、route、evidence、
safety 或 observer 存在歧义时必须标记 `invalid`，不得推导产品质量。

`accounting_status` 只由 canonical physical request ledger、usage、pricing、sealed 与
billing-unknown facts 决定。`billing_unknown`、`usage_incomplete` 或 `unpriced` 继续在
下一次付费 request 前停止正式 campaign，并禁止 Token、费用、效率和完整 utility 聚合；
未知费用不得猜成零。只有 `accounting_status=complete` 的 observation 可进入 accounting
aggregate。

行为 aggregate 只接受前三个闭合的 behavior status；`measurement_interruption` 与
`invalid` 均不进入。完整产品准入仍要求预注册矩阵的行为质量门、false success 为零和
accounting 完整全部通过。这个决策允许从 accounting 中断前已经闭合的产品行为学习，但
不降低任何付费准入、费用或 A/B 公平性门。

`maximum_reruns=0`、不补 mate、不选择性续跑、不拼接历史 raw 保持不变。下一次付费请求
前的最坏费用 reservation 只证明授权上界，不能替代请求级实际结算。

## 原因

- M19 的 pre-header failure 与 M20B 的 partial-response failure 都形成了 exact production
  terminal、Store reopen 和 external verifier snapshot，但旧 analyzer 因没有完整
  `arm_result` 统一投影为 `measurement_incomplete`。这把 accounting stop 错当成行为
  结论。
- M18 的 outer deadline 没有 production terminal，确实只能是 measurement
  interruption。它与上述 typed production failure 不是同一事实。
- M9-E 已证明官方接口不能对 pre-header failure 提供逐物理请求结算真相。该限制要求成本
  fail closed，但不要求丢弃已经独立闭合的 task outcome。
- Host latest-revision completion、RunStore exact replay 与 external verifier 已提供行为
  判断所需的 canonical owner；不需要第二 Store、第二 observer truth 或 LLM judge。

## 实施与删除

- `docs/product/EVALUATION.md` 是规范 owner；
- `scripts/eval-m9b-fixed-pro-regression.py` 的现有 corrected Harness/analyzer 是唯一执行
  owner；
- analyzer 直接读取同一 hash-chained journal 中的 canonical Store、credential-free
  reopen、verifier snapshot 与 terminal，不回写 frozen evidence；
- 删除 `arm_result is None -> measurement_incomplete` 的旧 product-loss 分支，改为两个
  正交派生轴；
- 不增加 production protocol/state、RuntimeEvent、RunStore、model sender、tool surface
  或 compatibility writer。

已发生的 manifest、raw、summary 与历史决策保持 byte-unchanged。使用本 ADR 重新计算得到
的新 report 是当前分析规则下的派生结果，不改写当时的 acquisition/admission 结论。

## 被拒绝的替代方案

- **accounting 不完整就丢弃全部行为事实**：继续阻止 Hardness loss acquisition 学习，
  并把 production failure 与测量中断混为一谈。
- **有 external verifier pass 就视为成功**：越过 Host latest-revision receipt，会制造
  false success。
- **从余额差或 reservation 推算单请求费用**：官方契约不支持逐请求对账，违背 M9-E。
- **新增第二 evaluator 或行为 journal**：产生双写和第二状态真相；现有 journal 已包含
  所需 canonical facts。
