# ADR-0003：多 Agent 复用 Runtime，并以 worktree 隔离写入

- 状态：已接受
- 日期：2026-07-15

## 决策

多 Agent 是核心能力。Orchestrator 调度多个相同的 `AgentRuntime`，通过一个
`TaskGraph`、结构化 `AgentOutcome` 和共享事件协议协作。所有写 Agent 使用独立
worktree，并完成 diff、review、verify、merge 和 cleanup。

## 原因

- 现有子 Agent 第二循环与根 Agent 在 streaming、工具、错误和终态上不一致；
- 共享 cwd 的并发写入无法形成可靠并行；
- Workflow、Fleet、Lane 和 Agent 多套产品概念增加认知和状态复杂度；
- 多 Agent 只有在隔离、证据和确定性收敛存在时才产生真实收益。

## 后果

- Agent 角色是 profile，不是不同 Runtime；
- 默认 supervisor tree，但引擎保留可配置深度；
- 模型侧收敛为一个 `agent` 工具；
- 有效的 DAG、ledger、lease、heartbeat、mailbox 和 worktree 能力迁入 Orchestrator；
- 替代完成后删除第二子循环和 Workflow/Fleet/Lane 重复外壳。
