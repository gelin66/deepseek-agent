# Current CodeWhale Architecture

> 文档类别：迁移事实。只描述当前源码，不替代
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md)、
> [ROADMAP.md](../product/ROADMAP.md) 或 ADR。

- 快照日期：2026-07-17
- 导入基线：`352e86a611fdf3cd8bd27c36d24d482c06a71117`
- workspace version：`0.8.68`
- M4-B 被测代码：commit `a534a824670b60c807c5abf399ea8674d4beb527`，tree
  `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8`
- 当前阶段：M4-C C1 已冻结；C2 continuation/context projection 候选已通过验收、待
  review/commit 冻结，交互 TUI caller 尚未迁移

## 1. 当前结论

生产 Headless 与本地 API 已经共用一条 Agent 执行链：

```text
codewhale exec
  -> TUI 内的 exec 参数/NDJSON 投影
  -> crates/app::AgentApplication

codewhale app-server
  -> crates/app-server 的 HTTP/SSE/stdio framing
  -> crates/app::AgentApplication

AgentApplication
  -> production composition
  -> crates/runtime::AgentRuntime
       -> crates/deepseek::DeepSeekModelPort
       -> crates/tools::ProductionToolExecutor
       -> crates/state::StateStore as SQLite RunStore
       -> canonical StoredRuntimeEvent
```

这两条入口不再拥有各自的模型循环、工具目录、终态判断或持久状态。

交互 TUI 仍是明确的迁移例外：

```text
interactive TUI / TaskManager
  -> crates/tui legacy engine/session/task path
  -> RuntimeThreadManager / RuntimeThreadStore
```

该路径只服务交互 TUI，已不再服务 app-server。它是 M4-C 的替换和删除目标，不能作为
新调用方继续扩展。

## 2. 已统一的生产链

### Application service

`crates/app` 是 exec 与 app-server 的唯一 application composition owner：

- 组合官方 DeepSeek connection、credential 和 HTTP client；
- 组合 production prompt；
- 组合固定工具 executor 与本地执行策略；
- 打开同一种 SQLite `RunStore`；
- 绑定 physical request budget、model accounting 和 execution fingerprint；
- 维护轻量 process-local active control registry；
- 实现 start、continue、compact、list_roots、get、events、resume、steer、interrupt、
  cancel、resolve_interaction；
- start/continue/compact 通过 State schema v8 的 durable creation reservation 先绑定
  `request_id + command digest` 与唯一 reserved run ID；
- control command 只有在对应 `SteerQueued`、`ControlRequested` 或 `InteractionResolved`
  已提交到 `RunStore` 后才返回 accepted sequence；重复 `request_id` 按持久回执幂等处理。

active registry 只保存当前进程可投递的 control handle，不是第二个 lifecycle 或持久事实。
run projection、event、lease 和 terminal 都从 `RunStore` 读取。

### Agent runtime

`crates/runtime` 是 UI、HTTP、DeepSeek transport 和 SQLite 无关的唯一根/子 Agent 内核。
根 Agent 和 child Agent 使用同一个 `AgentRuntime` 与 conformance semantics。Runtime 负责：

- canonical transcript；
- continuation lineage 与 model-visible context projection；
- model/tool 循环；
- root/child budget；
- control command；
- durable approval、request-user-input、两阶段 steer 与 typed interrupt/cancel；
- crash-safe event 提交；
- terminal candidate 与 Host 接受边界。

Runtime 自带的内存 Store 只用于测试，不进入 production composition。

C2 候选把 continuation 与 recovery 分开：`resume` 继续同一个 run，`continue` 从一个终态
root 创建新的 root，并用 `continued_from_run_id` 记录 lineage；source 不被改写。完整
canonical transcript 仍 append-only，compaction 只替换每次请求的 model-visible projection。
当前会先本地裁剪旧的大型工具结果，必要时才发出计入预算和 accounting 的 tool-free 摘要
请求；prepared/in-flight/failed/committed 均是 RuntimeEvent v5 的持久事实。

### DeepSeek backend

`crates/deepseek` 是官方 DeepSeek 请求事实 owner：

- Standard Chat、Beta Strict Chat、FIM surface 规划；
- 确定性 `RequestPlan`；
- ordinary/non-streaming 与 SSE transport；
- reasoning/tool-call 历史回放；
- finish reason、typed error、retry 和 usage；
- root/child physical request attribution；
- auto-route classifier 与确定性 fallback；
- 官方模型 capability、output limit 和 pricing fixture。

普通工具调用不因存在工具就误走 Beta；只有整组 schema strict-compatible 时使用
Beta Strict Chat。FIM 仍是独立 Beta Completions surface。Context cache 由官方 Chat 的
稳定前缀自动触发，不存在手工 cache API。

### Tools

`crates/tools` 拥有 production 固定工具 catalog、schema、execution identity 和 handler。
当前 11 个 Host 工具为：

```text
apply_patch  edit_file    exec_shell   file_search
git_diff     git_status   grep_files   list_dir
read_file    run_tests    run_verifiers
```

Runtime 在允许的 depth/budget 内追加内建 `agent` control tool；它启动的 child 仍是同一个
`AgentRuntime`，不是另一套 swarm loop。

交互 root run 还会追加 Runtime 内建 `request_user_input`；child run 强制非交互。该能力
不增加固定 Host 工具数量，approval 也是工具执行前置协议，不是模型可见的新工具。

所有 Host handler 返回 canonical `ToolOutcome`，明确区分 invocation、operation、retry、
side effect、evidence、artifact 和 workspace revision。交互 TUI 的宽工具系统尚未迁移，
不代表 Headless production catalog 会自动扩大。

### State

`crates/state::StateStore` 实现 production SQLite `RunStore`：

- 当前 canonical RunStore schema 为 v8；
- append-only canonical event；
- reducer/snapshot/replay；
- continuation lineage 的快速 projection、workspace-scoped root 列表和原子 continuation
  创建；
- durable creation reservation：start/continue/compact 的同 ID 同 payload 重试只对应
  一个 reserved run ID，不同 payload 复用 ID 被拒绝；
- execution lease 与 epoch；
- pending model attempt 与 unknown billing；
- pending interaction、steer、terminal control 与携带规范化 payload 的 command receipt；
- terminal exactly-once；
- no-key terminal replay。

旧 thread/session/task tables 仍供未迁移交互路径使用。它们不是 app-server 的状态来源，
也不能与 canonical run 双写。

## 3. 当前入口

### `codewhale exec`

- 真实执行进入 `AgentApplication`；
- text/NDJSON、receipt 和 exit code 投影仍在 `crates/tui`；
- start/continue/list_roots/resume/events/cancel 均读写 canonical Run API；
- `codewhale exec --continue <PROMPT>` 查询精确 workspace 下最新 root；只有最新 root 已终态
  时才以新 prompt 创建新的 root continuation。若最新 root 未终态，必须显式使用
  `codewhale exec --resume <RUN_ID>` 恢复同一个 run；
- crash/reopen/resume、terminal-first signal 和 no-key replay 已有外部进程门禁。

顶层 `codewhale` 仍委托现有 TUI binary 处理 exec 参数与输出，这是进程入口复用，不是
第二个 Runtime。composition、model、tool 和 Store 已不在该 adapter 中重复。

### `codewhale app-server`

- CLI 直接构造 production `AgentApplication`；
- 默认 HTTP/SSE 监听 `127.0.0.1:7878`；
- `--stdio` 提供 newline Run envelope；
- HTTP/SSE/stdio 只使用 canonical Run DTO 与 StoredRuntimeEvent；
- Run API v3 提供 continuation、manual compact 和 root 列表；RuntimeEvent writer 为 v5，
  reader 接受 v4-v5；
- crate dependency tree 不含 `crates/core` 或 `crates/tui`；
- 不启动 sibling TUI process。

完整接口见 [RUNTIME_API.md](RUNTIME_API.md)。

### Interactive TUI

交互 TUI 尚未切到 `AgentApplication`：

- `crates/tui/src/core/engine/*` 仍有旧 turn loop；
- `crates/tui/src/compaction.rs` 与旧 Engine compaction event 仍是另一条交互投影路径；
- session、task 和 approval presentation 仍使用旧类型；
- `TaskManager` 仍消费 `RuntimeThreadManager/RuntimeThreadStore`；
- 交互 child-agent path 仍未通过与 canonical root/child 相同的 conformance suite；
- generic Provider/config/UI 仍未执行 DeepSeek-only 最终清理。

因此当前不能宣称三个产品入口已经完全统一。M4-C 必须把交互输入变成 application command，
把 UI 变成 RuntimeEvent projection，并删除旧 engine/runtime-thread 生产路径。

## 4. Crate responsibility snapshot

| Crate | 当前生产职责 | 当前迁移债务 |
|---|---|---|
| `protocol` | canonical request、command、event、outcome、terminal | 后续 TaskContract/EvidenceReceipt 扩展 |
| `runtime` | 唯一根/子 Agent loop 与 reducer | completion/evidence 的 M5 强化 |
| `deepseek` | 官方 DeepSeek planner/transport/parser/accounting | FIM 调优与定期官方复核 |
| `context` | production prompt/context 构建与最小 compaction projection | RepoGraph、evidence-aware compaction 与 A/B 在 M5 |
| `tools` | 固定 production tool catalog 与执行 | 编辑/FIM 协议 A/B |
| `state` | SQLite RunStore、lease、replay | 交互旧状态 M4-C 删除 |
| `app` | 唯一 production composition 与 Run command | 后续 orchestrator command |
| `app-server` | HTTP/SSE/stdio projection | 无独立业务状态 |
| `cli` | 顶层命令与 production config 解析 | DeepSeek-only 配置/中文 M7-M8 |
| `tui` | exec projection + 未迁移交互产品 | M4-C 主删除目标 |

`crates/core` 已删除。它原有的 fake `handle_prompt` 从未是 production Agent 能力；app-server
迁移后没有保留兼容 crate 或空壳。

## 5. 已删除的旧产品路径

M4-B 已物理删除：

- app-server fake core runtime；
- `/prompt`、raw chat proxy、direct tool invoke；
- RuntimeBridge、TUI child process、事件翻译和私有 seq/thread map；
- TUI runtime HTTP/mobile server 与 mobile page；
- `serve --http/--mobile`、`app-server --http/--mobile` alias；
- remote-setup bundle generator；
- Tencent Lighthouse deploy scripts/units；
- Feishu、Telegram 和 bridge-core Node 产品链；
- 对应的 QR、CORS 和孤儿 runtime-api config 依赖。

删除这些外围产品不会删除 `agent` 多智能体能力。它们是旧 chat/cloud 控制面，不是
`AgentRuntime × N + Orchestrator` 的目标多 Agent 架构。

## 6. 当前验证事实

M4-B 提交中的离线验收已经证明：

- exec、HTTP/SSE、stdio 对同一个 read-only tool fixture 产生相同的 12 个 normalized
  canonical event、terminal、usage 和 accounting；
- HTTP SSE 的每个 `id` 等于 Store sequence，`data` 是完整 Store event；stdio 也
  0 丢失、0 虚构；
- 外部 app-server 进程 `SIGKILL` 后恢复同一 run，前缀不变、epoch 提升、event id 唯一、
  terminal 恰好一个；
- live owner 并发 resume 被拒绝；terminal 可由第三个无 Key 进程原样重放；
- app-server 不依赖 core/tui，生产调用图没有旧 bridge 符号。

完整 workspace 门禁、冻结 release build 和官方 DeepSeek canary 均已通过。精确结果、
二进制摘要、费用和非结论见
[M4-B 本地 API 证据汇总](../../eval/summaries/m4-b-local-api-2026-07-17.md)；不能从一次
链路 canary 推断编码能力或效率提升。

M4-C C1 实现提交为 `1d127b78`。conformance/Store replay 已证明 interaction ordering、
幂等响应和同 run 恢复状态机；外部监督进程 `SIGKILL` 测试已覆盖 `InteractionRequested`、
`InteractionResolved`/before-tool-start、`SteerQueued`/before-applied、
`ControlRequested`/tool-in-flight 与 `SteerApplied`/next-model-not-prepared 窗口。focused、
all-target check、全仓 clippy 和 workspace tests 均通过。该冻结不代表 TUI 已切换，也不构成
编码能力或效率提升证据。

M4-C C2 当前是已通过验收但未 commit 冻结的候选：Run API v3、RuntimeEvent writer
v5/read v4-v5、State schema v8，以及 continuation、root list、manual/automatic context
projection 已进入 exec/app-server 的 canonical 链路。focused、workspace Clippy
`-D warnings`、串行完整 workspace tests、内存/SQLite parity，以及由外部监督进程
`SIGKILL` 的 compaction prepared/in-flight/committed 恢复矩阵均通过。费用受限的官方
DeepSeek production sender canary 以 6/6 请求覆盖 Standard、Thinking/tool-history replay、
Beta Strict 与 FIM，完整 usage、无 transport retry，费用为 `USD 0.0000969904`；该 canary
不包含 compaction on/off 收益对照，且 `product_metric_eligible=false`。

交互 TUI 仍使用旧 engine/session/task/runtime-thread 与旧 compaction 路径，因此不能把该
候选记录为三个入口切换完成。当前证据只证明协议、lineage、持久恢复、accounting 与官方
surface 兼容；尚无 compaction on/off 真实 A/B，不能声称 Token、成本或任务成功率改善。

## 7. 明确非结论

当前源码不证明：

- 交互 TUI 已统一；
- C2 已完成冻结，或旧 TUI compaction/runtime-thread 路径已删除；
- 当前 compaction 已证明节省 Token、降低成本或提高任务成功率；
- Provider 清理、全面汉化或中文 Agent prompt A/B 已完成；
- RepoGraph、EvidenceReceipt、writer-worktree Orchestrator 已完成；
- transport 迁移本身提升了真实编码成功率；
- 单次 live canary 可以成为产品指标。

这些能力只能按 ROADMAP 的后续切片实现，并按 EVALUATION 的同任务、同预算、重复 A/B
决定保留或删除。
