# Canonical 多 Agent 现状

> 文档类别：当前能力说明。产品边界以
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md) 和 ADR 为准。

- 快照日期：2026-07-18
- 执行 owner：`crates/runtime::AgentRuntime`
- 持久事实 owner：canonical `RunStore`
- 模型侧入口：一个 `agent` 工具

## 当前执行链

根 Agent 和子 Agent 共用同一个 Runtime：

```text
root AgentRuntime
  -> agent tool
  -> Arc<AgentRuntime>::launch_child
  -> child Run
  -> ChildStarted / ChildFinished
  -> RunStore
  -> parent handoff
```

`AgentRuntime` 在深度、并发和共享请求预算允许时追加内建 `agent` 工具。启动子
Agent 时会：

1. 先预留 child 并发许可和最终产物请求许可；
2. 持久化 `ChildStarted`；
3. 用同一个 `Arc<AgentRuntime>` 创建 child `RunRequest`；
4. 在父 Agent 下一次模型请求前等待 child 结算；
5. 持久化 `ChildFinished` 并把结构化 handoff 回注父 transcript。

根与 child 共用模型端口、工具执行器、错误分类、请求预算、事件协议和 Store。
不存在第二个 child 模型循环、私有 completion 判定或 SubAgent JSON/JSONL 状态。

## 当前 `agent` 契约

当前 schema 支持：

- `prompt`：必填任务；
- `type`：角色标签，只改变任务侧重点；
- `fork_context`：是否复制父 Agent 的规范化 transcript/projection；
- `allowed_tools`：只能缩小父工具权限；
- `max_steps`、`max_depth`、`wall_time_secs`：只能收紧父限制；
- `expected_artifact`：要求 child 返回的产物或证据。

在 M6 的 `WorkspaceLane` 接管 writer worktree 前，canonical child 强制只读。角色标签
不会改变这一安全边界，也不会选择另一套 Runtime。

模型目前只有一个 `agent` 启动入口。`list/send/followup/wait/interrupt/cancel` 将来只能
成为同一 `agent` 契约和唯一 Orchestrator 的动作，不能恢复
`agents_list`、`agents_message`、`agents_followup`、`agents_interrupt`、
`agents_wait` 五套旧工具。

## 配置

canonical Run 的核心控制是：

```toml
[subagents]
enabled = true
max_concurrent = 4
max_depth = 3
```

它们分别控制是否暴露 `agent`、共享 child 并发上限和嵌套深度。当前 Config resolver
仍接受部分导入期 provider/top-level fallback 字段；这些旧配置待独立删除，不构成第二
Runtime 的保留理由。

## Fleet 与角色 profile

Fleet 的真实 worker 由 `fleet::executor` 启动为：

```text
FleetExecutor -> codewhale exec --output-format stream-json
```

因此 Fleet worker 最终仍进入 canonical `AgentApplication -> AgentRuntime`。Fleet
ledger 目前仍是待迁入 Orchestrator 的产品债务，但不拥有模型循环。

Fleet 任务/profile 中的 role、loadout 和 model intent 只参与 prompt 与 route 投影；
解析后的 route、reasoning tier 以及全局 `FleetExecConfig` allow/deny 会进入实际 exec
命令。task-level role/tool scope 目前没有进入 exec argv，因此 Fleet receipt 的
`effective_permissions` 固定留空，不能把声明配置写成已执行权限。仅当 M6 的唯一
Orchestrator 把 task policy 真正传入并观测执行后，才能由 enforced policy owner 填写该字段。

## TUI 投影

交互 TUI 只从 canonical child events 投影：

- `CanonicalRunProjection`；
- `run_presenter`；
- `tui/child_agents.rs`；
- sidebar 中的 child rows。

旧 `/subagents` modal、legacy child DTO、mailbox cache 和刷新事件已经删除。TUI 不再
维护第二份 child 生命周期。

## 已删除路径

以下实现已经物理删除，不能恢复兼容桥：

- `SubAgentRuntime` / `SubAgentManager`；
- TUI 旧 `AgentTool` 与协调工具；
- TUI 旧 `ToolRegistry` child recursion；
- `.codewhale/state/subagents.v1.json`；
- child JSONL transcript/checkpoint/mailbox；
- `workflow-tool -> WorkflowTool -> SubAgentRuntime -> DeepSeekClient`；
- Fleet 到 `SharedSubAgentManager` 的可选死接线；
- 仅为虚假权限 receipt 服务的 `WorkerRole`、`WorkerRuntimeProfile` 和
  `FleetWorkerRuntimeSpec`；
- 旧 `/subagents` modal。

## 未完成能力

以下属于 M6，不应从旧实现恢复：

- 唯一 Orchestrator 的 task graph、mailbox 和控制动作；
- writing Agent worktree create/diff/review/verify/merge/cleanup；
- 可恢复的 writer 冲突收敛；
- 结构化 `AgentOutcome`；
- 多 Agent 相对单 Agent 的广泛真实任务净收益评测。
