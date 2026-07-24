# ADR-0008：固定 DeepSeek actor 路由并退休 Auto 产品语义

- 状态：已接受
- 日期：2026-07-24

## 决策

CodeWhale 不再提供或保留 `model=auto`、`reasoning=auto`、任务难度分类器或运行中动态
模型路由。production 只接受官方 `deepseek-v4-pro`、`deepseek-v4-flash` 与明确的
`off/low/medium/high/max` reasoning，按以下确定性规则冻结每个 Run：

1. 未显式指定模型的 root 固定为 `deepseek-v4-pro` + `high`；
2. 显式选择 Pro/Flash 和 reasoning 的 root 保持原值，其 child 精确继承，供用户明确
   选择和 fixed-Pro 同任务评测使用；
3. fixed actor profile 的普通 read-only child 固定为 Flash/high，并只提交 handoff；
4. fixed actor profile 的显式 isolated Writer 固定为 Pro/high；
5. 已有 typed recovery/recheck/rework 事实时，新 Run 固定为 Pro/max；
6. 一个 Run 的实际 model、reasoning、actor、route profile、policy version 和稳定
   reason code 在 `RunRequest`/RunStore 中不可变，重开时只验证，不重新决策。

`crates/app` 的 production composition 是固定 actor route policy 的唯一 owner。
`crates/deepseek` 只拥有官方模型能力、RequestPlan、transport/parser 和 accounting。
CLI、TUI、app-server/config 不得再次解释 Auto，也不得增加分类模型请求、关键词
heuristic、模型自评路由或第二 controller loop。

RuntimeEvent v18 使用中性的 `ModelRouteProfile::{Explicit, FixedActor}`，删除 caller
requested Auto mode/reasoning 的重复状态。State v24 直接退休无法无损重建 exact
RequestPlan 的 v23 materialized runs；只保留已能按 v18 命令契约直接反序列化的 pending
Start。迁移不保留 compatibility reader、dual write 或第二状态真相。

## 原因

- 产品目标是 fixed-Pro 根能力下的 verified task success 与恢复可靠性。为省成本增加
  classifier、策略分支和新请求会扩大失败面，且 M8-I/M9-A 从未得到默认准入证据。
- `ReasoningEffort::Auto` 在旧 wire contract 中代表省略 `reasoning_effort`。把历史值
  静默映射为 high/max 会改变 exact RequestPlan，不能诚实重放。
- actor authority、workspace access 和 typed recovery 已经是 Host 的确定性事实；它们
  足以选择固定 profile，不需要猜测自然语言任务难度。
- 显式 Pro/Flash 是用户控制和正式评测所需的真实 surface，不是 Auto。保留它们可继续
  构造同 binary fixed-Pro baseline，同时不引入隐式切换。
- M8-I/M9-A 的 manifest、summary 和 ignored raw 记录已经发生的历史事实。保留这些
  evidence 比改写历史更可审计；它们不再产生当前产品承诺。

## 后果

- 默认 root 与 Writer 都以 Pro 负责；普通 fixed-profile read-only child 可用 Flash
  调查；typed recovery/recheck 使用 Pro/max。
- config、CLI、TUI、Run API 当前输入和 help 不再接受或显示模型/推理 Auto。
  `codewhale exec --auto` 仍仅表示启用工具并自动批准，与模型路由无关。
- M8-I/M9-A 的历史结论保持原样，但任何 `hold_auto_default_admission` 或未来 Auto A/B
  不再是当前 roadmap。无需读取 Key 或重开 M9-A 来证明范围删除。
- 当前 fixed-Pro regression Harness 只读取中性的 `profile=explicit` route audit；
  官方 DeepSeek ChatCompletions sender、usage/cost accounting、唯一 AgentRuntime、
  RunStore 和工具目录不变。
- 未来计算量调整只有在新的长期产品决策中才能重新进入；不得以兼容、隐藏配置或实验
  开关恢复 Auto。
