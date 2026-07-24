# M10-D Failure-Directed Recovery（2026-07-24）

## 结论

M10-D 的正式决定是：

```text
reject_no_safe_independent_controller_delta
keep the existing owner-specific recovery mechanisms
do not add a second crates/app controller loop
do not read a credential or call the official API
advance to M10-E environment facts
```

只读 caller graph、current frozen trajectories 与 deterministic fault injection 共同证明：
当前可以安全动作的 typed failure 已由各自 canonical owner 完整处理；尚未覆盖的
`context_missing`、`reasoning_insufficient` 与泛 `environment_failure` 不是可重放的
typed causal facts。此时新增一个 `crates/app` 通用 failure→action controller，只会：

1. 重复 `ToolOutcome.model_content()`、Runtime verifier transition、model retry 或
   accounting stop；
2. 从关键词、模型自评或泛化 `operation_failed` 猜测原因；
3. 自动调用 read/edit/rollback，建立第二执行 loop 或产生重复副作用；
4. 在没有 measured missing-action loss 的情况下增加永久策略分支。

所以 M10-D 没有 production candidate，也没有付费 A/B。结论不是“恢复已经完美”，而是
“当前证据不支持一个独立通用控制器；下一步必须先由 M10-E 产生环境因果事实”。

## 切片契约

- 真实问题：判断 current canonical failure facts 是否暴露一个可由
  `crates/app` 单独修复的、已测量恢复损失；
- 验收：只用稳定 typed facts，不增加模型分类请求或第二 loop；同预算恢复率上升、
  重复副作用为零、false success 0、exact replay 一致，并能删除被替代的通用分支；
- owner（若准入）：`crates/app`；
- 必须保留：protocol 的 ToolOutcome feedback、tools 的失败因果/副作用、Runtime 的
  verifier/retry/budget/replay、DeepSeek transport/accounting、Harness 的 unknown billing
  stop；
- cutover：不存在独立 delta 时不创建 candidate，不制造需要稍后删除的生产分支。

机器可读矩阵见
[m10-d-failure-directed-recovery-audit-v1.json](../manifests/m10-d-failure-directed-recovery-audit-v1.json)。

## 当前 owner graph

当前不是“没有 recovery policy”，而是 recovery 已按事实 owner 分层：

- `crates/tools` 产生稳定 failure code、invocation/transport/operation/side-effect/retry
  与 revision/artifact；
- `ToolOutcome.model_content()` 为 root、read-only child、Writer 生成同一 typed 中文反馈，
  14 个 code 的 match 是编译期穷尽；
- `crates/runtime` 对 Host verifier failure 强制
  `failure -> effective workspace mutation -> pass`，相同 revision 禁止重验；
- Runtime 只在 response 没有 actionable output 且 replay-safe 时原子准备 model retry，
  crash/reopen 后只执行同一个 prepared retry；
- side-effect ambiguous、unknown billing 与 budget exhausted 都 fail closed；
- `crates/app` 已只在真实 completion rejection、Host verifier failure 或 prior child
  failure 存在时固定选择 Pro/max recheck；不进行任务难度分类；
- ContextBroker 从 canonical transcript/rejection/verifier failure 投影下一请求，不保存
  第二恢复状态。

把这些逻辑再汇总为一个 app map 不会替代旧路，只会形成冲突真相。

## Failure matrix

### 已由现有 owner 完整处理

- schema/arguments：`malformed_arguments`、`schema_validation`、`missing_field`、
  `invalid_field`、`patch_parse` 在执行前拒绝，要求修正参数；重复相同失败受 terminal
  request 约束；
- stale/conflict：`stale_read`、`ambiguous_edit`、`workspace_precondition` 与
  `patch_parse` 要求重读/缩窄，工具 freshness/atomicity 保证无盲写；
- verifier：`verifier_failed` 同时是 ToolFailureCode 与 typed CompletionRejection，
  Runtime 拒绝不修改就再次验证；
- incomplete stream：ModelResponseEvidence 决定 replay safety，retry decision 与 failed
  attempt 同事件提交，SIGKILL 后 exactly-once；
- read-only transport：workspace authority 把 side effect 证明为 not-applicable 后，下一
  模型 turn 可安全恢复；
- budget：typed terminal，永不伪装成功。

### 必须 fail closed

- `side_effect_ambiguous`：没有安全重放许可；
- `transport_billing_unknown`：M9-E 已证明 official contract 无逐 pre-header attempt
  对账能力，formal campaign 必须停止；
- budget exhausted：冻结预算内已无 action capacity。

### 当前没有 canonical 因果事实

- `context_missing`：它是对模型认知状态的推断；M10-B 的 deterministic Working-Set
  替代已被质量 veto 删除；
- `reasoning_insufficient`：不能由关键词、模型自评或重复调用判定；只有 completion
  rejection/prior child failure 等 typed fact 才允许 Pro/max；
- `environment_failure`：当前泛 `operation_failed` 不能区分工具缺失、错误命令、服务
  未启动、权限、网络或产品代码失败；这正是 M10-E 的 ProjectEnvironmentProfile 缺口；
- semantically wrong file：若路径在权限范围内，只能由 TaskContract/verifier 判断；
  通用自动 rollback 会误伤用户或其他合法修改。

## Current trajectory evidence

审计三个 frozen 0600 raw：

| raw | typed-failure arms | 结果 |
| --- | ---: | --- |
| M9-C fixed-Pro | 2 × verifier_failed | 2 verified，0 false success |
| M10-A scoped context | 5 × verifier_failed | 实际 5 均恢复；1 个 frozen label 已证明是 observer false positive |
| M10-B Working-Set | 1 × verifier_failed + 1 × workspace_precondition | verifier 恢复；另一 arm 文件/verifier 通过但违反冻结 child 参数契约 |

合计 9 个带 typed tool failure 的 arm：8 个 verifier failure 已由 current path 完成恢复；
1 个 workspace precondition arm 的产品否决来自 `agent` 参数不匹配，不是缺少 failure
action。没有观察到一个“存在安全 typed action、但 current owner 没有执行/反馈”的样本。

raw identities：

- M9-C：
  `c73a01a4934b82b3f4a4042791aae89e5da56feaea1ffbea188a80d4e6e05b77`；
- M10-A：
  `0c81935b2e6c4afb2e80c76d0a3746689e068a90c17ee16644176b8d710a0d66`；
- M10-B：
  `69b44fa8ebcc8555fe102369bbf4fc25c66149aa713a8d4fa2157578f87da1d0`。

它们只用于 loss audit，不拼成新的 treatment/control 产品比较。

## Deterministic fault injection

在 current `85d6995c`、Run API v12、RuntimeEvent v18、State v24、exec-stream v3 上通过：

- stable typed 中文 tool failure feedback；
- root/read-only/Writer 相同 schema correction；
- read-only executor transport failure 下一 turn 恢复；
- verifier failure 只投影一次、修改后通过；
- no-output replay-safe model retry；
- atomic retry decision 的 crash/reopen exactly-once；
- 重复相同 correctable failure 受 terminal request 限制；
- mandatory context 超限在任何 model request 前失败；
- tools production failure matrix 与 edit failure code/atomicity；
- fixed actor route 只由 actor authority 与 typed recovery facts 决定。

这些测试不是产品收益 A/B；它们证明新增通用 controller 没有可以替代的缺失机制。

## Cutover

M10-D 没有写 production 代码，因此：

- 不新增 app recovery enum/map/store/event/config/mode；
- 不新增分类模型请求、关键词 detector、自动 rollback 或第二 controller loop；
- 不移动 protocol/tools/runtime/deepseek 的现有 owner；
- 不删除当前 model feedback、verifier transition、safe retry、fixed Pro/max recheck 或
  billing stop；
- 只保留 manifest、summary 与权威文档中的否决事实。

Key 未读取，official API requests=0，raw 未创建。

## 非结论

M10-D 不证明：

- 所有恢复场景都已覆盖或 current model 永远会正确行动；
- `workspace_precondition` 等价于 wrong-file scope；
- generic `operation_failed` 足以驱动环境修复；
- 应自动重试 unsafe transport、放宽 budget 或 unknown billing stop；
- M10-E 一定能形成可准入的环境候选；
- 应恢复 Auto、FIM、Anthropic Messages、第二 Provider/Runtime/Store 或 multi-Writer。

下一独立切片为 M10-E：先建立 deterministic ProjectEnvironmentProfile 与 task-scoped
runtime artifact loss audit。只有环境事实能稳定区分“修环境”与“改产品代码”时，才允许
未来增加对应的窄 recovery action。
