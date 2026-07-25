# ADR-0007：V1 固定中文 prompt 以可追溯基线证据验收

- 状态：已接受
- 日期：2026-07-24

> 范围说明：本 ADR 继续作为旧 fixed-Chinese baseline 的 immutable identity、历史
> release evidence 与 rollback 事实；其固定中文长期产品方向已被
> [ADR-0010](0010-bilingual-product-and-prompt-admission.md) 取代。

## 决策

V1 把当前固定简体中文 Agent prompt 作为已经接受的产品语言约束，而不是一个仍须不断
发明 treatment 才能发布的优化候选。固定基线按以下 evidence chain 验收：

1. `crates/context/src/prompts/constitution.md` 是唯一 bundled production
   constitution；其完整 SHA-256、源码 revision/tree、编译期 owner 和 whole-release
   rollback identity 必须可复核。
2. 至少一份合格的官方 DeepSeek coding/false-success 评测必须使用与当前 byte-identical
   的 constitution，并完整记录 immutable binary、请求、usage、cost、deterministic
   verifier 与 `product_metric_eligible=true`。
3. exact current source 必须通过 prompt provenance、root/read-only child/explicit
   Writer、verifier rejection/recovery、physical accounting、RequestPlan 与 SQLite reopen
   的 production conformance，以及 locked/offline release 安装、验证、卸载和用户数据保留。
4. 已拒绝或因 unknown billing 停止且从未接管的 M8-D/M8-M prompt treatment 保持
   `hold`。它们的 frozen manifest/summary/raw 继续可审计，但未接管的候选不构成发布依赖。
5. 任何新的模型可见 prompt 语义候选，仍须在接管前通过自己的 current
   same-revision、immutable-binary、同任务、计费可证明 A/B，并同时满足
   `PRODUCT_PLAN` 6.1 的不回归和明确净收益门槛。

M8-N 接受以下现有证据：

- current constitution SHA-256 为
  `39f2eeb30519e143eed2d4c627fcb97d323c6b95ad9060816a93b72ea994d409`；
  它在 M5-A baseline/candidate、M6-A Writer canary、M8-M immutable binary 和 M8-N
  clean baseline 中 byte-identical；
- M5-A 的 12/12 official DeepSeek arms 全部
  `product_metric_eligible`，覆盖固定 coding task 与 forced-false-claim 反例；coding
  两侧均 3/3 verified，candidate false-success 为 0，false-claim candidate 3/3 正确
  拒绝，40 次请求、144,903 tokens 和 `$0.004110153` accounting 完整；
- M8-M stopped suite 中 current baseline 的 t1、t3、t5 三个 exact-current diagnostic
  observations 均 measurement-valid、verified 且 false-success 为 0；candidate 的
  unknown billing 仍使该 suite 不具备 treatment 产品指标资格；
- M6-A 使用同一 constitution 的 explicit Writer canary 完成 7/7 usage-bearing
  responses、集成、latest-root receipt 和 cleanup；它只作为机制证据；
- M8-N 的 exact-current gates 与现有 delivery owner负责当前源码 retention 和
  release rollback。

完整冻结输入见
[`m8-n-v15-release-scope-successor-v1.json`](../../eval/manifests/m8-n-v15-release-scope-successor-v1.json)。

## 原因

- `PRODUCT_PLAN` 6.1 约束的是“改变提示词语义的候选”：只有同任务 A/B 无能力回归且有
  明确净收益，候选才可以接管 production。它不要求为已经固定的产品语言基线永久生成
  新候选。
- 旧 V15 的绝对措辞把“固定中文基线的版本、回滚、真实能力和 current retention 证据”
  与“未来语义 treatment 的接管规则”混成一项，导致两个从未接管的候选因第三方
  unknown billing 成为无限发布依赖。
- M8-D v1-v5 和 M8-M 都正确 fail closed；它们不能续跑、补 mate、拼样或重写为质量结论。
  继续用同一个未采用候选重复消费 API，不会增加当前 shipped baseline 的可归因证据。
- 当前 constitution 在合格 M5-A、Writer canary 和 exact-current production/release
  checkpoints 之间 byte-identical。用不可变身份连接窄范围 live evidence 与 current
  deterministic retention，比把未知账单猜成 0 或伪造第三个候选更可审计。
- M8-N 没有 material model-visible delta，因此 live A/B 为
  `inadmissible_no_material_treatment`：不读取 Key，不调用官方 API，不访问外部网络。

## 后果

- V15 改为“固定中文 prompt 有 immutable 版本/回滚身份、合格官方 DeepSeek
  coding/false-success evidence 和 exact-current production retention；未来语义候选仍须
  A/B 后才能接管”。
- M8-N 离线门禁和 release lifecycle 通过后，current V1 matrix 从
  `15 pass / 1 blocked` 收敛为 `16 pass / 0 blocked`，发布决策为 `V1 可发布`。这只表示
  release-ready，不执行 push、发布或远端变更。
- `crates/context` 继续是唯一 production prompt owner；prompt rollback 继续使用现有
  immutable whole-release rollback，不增加 prompt version store、selector、mode 或
  compatibility branch。
- 失去消费者的 M8-D candidate-only app test 和 current-tree fixture 删除；通用的
  app-server/exec/TUI prompt override loader consistency 保留。冻结 Git history、
  manifests、summaries 和 `0600` raw 不改写。
- 本决策不改写为中文 prompt 优于英文 prompt。`b088fd13` 的 full Chinese treatment
  仍为拒绝，M8-D/M8-M candidate 仍为 hold；它们不证明更好、更差或等价。
- 本决策在接受时不改变默认 fixed Pro 或当时的 Auto admission；Auto 产品方向随后由
  [ADR-0008](0008-fixed-deepseek-routing-and-auto-retirement.md) supersede 并删除。official DeepSeek
  ChatCompletions、AgentRuntime、RunStore、工具目录或协议版本，也不准入 Anthropic、
  FIM、Provider、第二 transport、第二 Runtime 或第二 Store。
