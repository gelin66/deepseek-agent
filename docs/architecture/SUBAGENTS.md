# Canonical 多 Agent 现状

> 文档类别：当前能力说明。产品边界以
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md) 和 ADR 为准。

- 快照日期：2026-07-24
- 执行 owner：`crates/runtime::AgentRuntime`
- Writer 编排 owner：`crates/orchestrator::ProductionAgentOrchestrator`
- 持久事实 owner：canonical `RunStore`
- 模型侧入口：一个 `agent` 工具

## 当前执行链

根 Agent 和子 Agent 共用同一个 Runtime：

```text
root AgentRuntime
  -> agent tool
  -> read_only -----------> 同一 AgentRuntime -> child Run
  -> isolated_write
       -> ProductionAgentOrchestrator
       -> isolated worktree
       -> 同一 AgentRuntime -> Writer child Run
       -> Host diff/verify/integrate/root verify/cleanup
  -> canonical RuntimeEvent / RunStore
  -> parent handoff
```

`AgentRuntime` 在深度、并发和共享请求预算允许时追加内建 `agent` 工具。启动子
Agent 时会：

1. 先预留 child 并发许可和最终产物请求许可；
2. 把模型 intent 解析为 Host 冻结的 `AgentTask`；
3. read-only child 直接用同一个 `Arc<AgentRuntime>` 创建 child `RunRequest`；
4. isolated Writer 由唯一 Orchestrator 验证 clean Git/base/allowed paths，创建独立
   worktree 后再运行相同 `AgentRuntime`；
5. 在父 Agent 下一次模型请求前等待 child 结算；
6. Writer 由 Host 封存真实 diff/outcome，执行 worktree verifier、fast-forward
   integration、最新根 revision verifier 与 cleanup；
7. 持久化 canonical lifecycle 和结构化 handoff。

根与 child 共用模型端口、工具执行器、错误分类、请求预算、事件协议和 Store。
不存在第二个 child 模型循环、私有 completion 判定或 SubAgent JSON/JSONL 状态。

## 当前 `agent` 契约

当前 schema 支持：

- `prompt`：必填任务；
- `type`：角色标签，只改变任务侧重点；
- `workspace_access`：`read_only` 或显式 `isolated_write` intent；
- `allowed_paths`：isolated Writer 必填的工作区相对范围，只能收紧；
- `fork_context`：是否复制父 Agent 的规范化 transcript/projection；
- `allowed_tools`：只能缩小父工具权限；
- `max_steps`、`max_depth`、`wall_time_secs`：只能收紧父限制；
- `expected_artifact`：要求 child 返回的产物或证据。

默认仍为 read-only。角色标签不会获得写权限；只有显式 `isolated_write`、非空
`allowed_paths`、Host exact verifier、auto-approve 和唯一 Orchestrator 的 clean-Git/base
预检同时满足时，Host 才分配独立 worktree。Writer receipt 不能直接完成 root，集成后必须
在最新根 revision 重新签发 EvidenceReceipt。

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

它们分别控制是否暴露 `agent`、共享 child 并发上限和嵌套深度。DeepSeek endpoint、
model 与认证由唯一 DeepSeek config owner 解析；已经退休的 Provider、Fleet 和 Lane
配置会 fail closed。`max_concurrent` 仍允许多个只读 child；isolated Writer 限制为每个
root 最多一个，不能由该配置扩大为多 Writer。

## 唯一 TaskGraph 产品概念

TaskGraph 不是新的 DTO、模式或第二 scheduler。它是现有 canonical facts 的产品名称：

```text
AgentTask / AgentOutcome / parent_run_id
  -> AgentRuntime child lifecycle and shared budgets
  -> RunStore durable truth and reopen
  -> ProductionAgentOrchestrator only for explicit Writer Git side effects
```

`crates/protocol` 只表达 canonical Agent/Run facts；`crates/runtime` 推进 root 和 child；
`crates/state` 负责唯一持久真相；`crates/orchestrator` 只拥有 explicit Writer 的
worktree、diff、verify、integrate 和 cleanup。CLI、TUI 与 app-server 均投影这条链，
不存在远程 Fleet、Lane registry、第二 ledger、通用 DAG 或多 Writer 控制面。

## TUI 投影

交互 TUI 只从 canonical child events 投影：

- `CanonicalRunProjection`；
- `run_presenter`；
- `crates/tui/src/tui/child_agents.rs`；
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
- 旧 `/subagents` modal；
- Fleet 的协议、配置、ledger、lease/scheduler、SSH host、alerts、worker executor、
  `/fleet`、`codewhale fleet` 和 bundled `fleet-manager` skill；
- Lane crate、registry、tmux/inline process runtime、`lane-log-proxy` 与
  `codewhale lane`；
- setup-state 的 `OperateFleet` card、receipt flag 和 Doctor roster projection。

## Post-V1 证据准入能力

以下能力不得从旧实现恢复，也没有被 M6-B1 准入：

- canonical TaskGraph 上的 follow-up/wait/interrupt 等控制动作；
- 多 Writer 有界并发与可恢复冲突收敛；ADR-0005 已确认它不是 V1 门槛；
- 更广任务上的多 Agent 净收益证明。

M6-A 已完成单 Writer 的 `AgentTask`、结构化 Host-observed `AgentOutcome`、worktree
create/diff/verify/integrate/root verify/cleanup 和 crash/reopen。M6-B1 的 18 对 /
36 arms 同任务 A/B 已判定 `reject_and_rework`：Writer `4/18` verified、7
false-success，且 Token/费用分别比 single 高 35.5% / 52.5%。后续切片已完成 verifier、
actor capability 与 explicit-only admission 收敛；这不等于 Writer 已获得净收益，
也不允许扩到双 Writer。ADR-0005 以现有 explicit single Writer lifecycle 关闭 V08；
这不是 multi-Writer 已实现或 single Writer 已获净收益。重新出现可归因失败并通过正式
A/B 前，多 Writer、通用 DAG 和第二 scheduler 均不进入开发。
