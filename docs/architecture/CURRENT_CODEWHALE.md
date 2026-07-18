# Current CodeWhale Architecture

> 文档类别：迁移事实。只描述当前源码，不替代
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md)、
> [ROADMAP.md](../product/ROADMAP.md) 或 ADR。

- 快照日期：2026-07-18
- 导入基线：`352e86a611fdf3cd8bd27c36d24d482c06a71117`
- workspace version：`0.8.68`
- M4-B 被测代码：commit `a534a824670b60c807c5abf399ea8674d4beb527`，tree
  `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8`
- 当前阶段：M4-C 收尾；交互 TUI foreground 与 root/child projection 已迁移，当前
  canonical RuntimeEvent 为 v6、State schema 为 v10；隐藏 `workflow-tool` 第二模型循环
  及其私有状态/UI 已删除，最终集成门禁待通过

## 1. 当前结论

Headless、本地 API 与交互 TUI foreground 已共用一条 Agent 执行链：

```text
codewhale exec --------\
app-server -------------+-> AgentApplication -> AgentRuntime
interactive TUI --------/          |                |
  TuiRunClient                     |                +-> DeepSeekModelPort
  CanonicalRunProjection           |                +-> ProductionToolExecutor
  run_presenter                    +----------------> SQLite RunStore
```

这三条入口不再拥有各自的模型循环、工具目录、终态判断或持久状态。交互 TUI 只提交
canonical Run command，并从 durable event 投影 root/child 状态。

旧生产例外 `workflow -> workflow-tool -> WorkflowTool -> SubAgentRuntime ->
DeepSeekClient` 已物理删除；同时删除 Workflow/Workflow-JS crate、私有 JSON/JSONL
写入链、TUI 面板/事件/审批/触发和专属 SubAgent adapter。没有建立兼容桥或双写。旧命令
会在配置解析、TUI、Store 和模型初始化前 fail closed；显式
`--prompt "workflow ..."` 仍是合法自然语言输入。

因此当前生产可达的根/子 Agent 模型循环只剩 canonical `AgentRuntime`。尚未实现的
DAG、writer worktree 和 merge 能力属于 M6 唯一 Orchestrator，而不是保留旧 Workflow
Runtime 的理由。

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
- start/continue/compact 通过 State schema v10 的 durable creation reservation 先绑定
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

当前 RuntimeEvent v6 将逻辑模型请求预算和物理 API 请求预算分开：Runtime 在进入
ModelPort 前拒绝第 N+1 个逻辑请求时持久化
`model_request_budget_exceeded`；只有 DeepSeek 物理 admission 实际拒绝请求并使
`exhausted_denied > 0` 时才持久化 `api_request_budget_exceeded`。`started == limit`
不等于物理耗尽，Runtime 不为证明耗尽而故意发送额外请求。旧泛化
`request_budget_exceeded` 不再接受。

Runtime 还为每个 root/child 从共享逻辑预算预留一个可退还的最终请求许可。descendant 与
child 必须先 join，随后各自以 `tools=[]` 发出最后请求；没有最终容量时不得先提交假的
`ChildStarted`。自动 compaction 在尚未触及硬上下文限制时不能消耗最后许可，恢复则按该次
请求实际 advertised tool catalog 拒绝未授权工具。该机制已有离线 conformance/Store replay
证据，但尚未通过新的真实 multi A/B，不能宣称提升了产品成功率或效率。

C2 把 continuation 与 recovery 分开：`resume` 继续同一个 run，`continue` 从一个终态
root 创建新的 root，并用 `continued_from_run_id` 记录 lineage；source 不被改写。完整
canonical transcript 仍 append-only，compaction 只替换每次请求的 model-visible projection。
当前会先本地裁剪旧的大型工具结果，必要时才发出计入预算和 accounting 的 tool-free 摘要
请求；prepared/in-flight/failed/committed 由 RuntimeEvent v5 引入，并继续由当前 v6
持久化。

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
side effect、evidence、artifact 和 workspace revision。TUI 下仍编译的宽工具实现只服务
隐藏旧路径或已无消费者，不代表 canonical production catalog 会自动扩大。

### State

`crates/state::StateStore` 实现 production SQLite `RunStore`：

- 当前 canonical RunStore schema 为 v10；
- 当前 canonical RuntimeEvent writer/reader 为 v6；
- append-only canonical event；
- reducer/snapshot/replay；
- continuation lineage 的快速 projection、workspace-scoped root 列表和原子 continuation
  创建；
- durable creation reservation：start/continue/compact 的同 ID 同 payload 重试只对应
  一个 reserved run ID，不同 payload 复用 ID 被拒绝；
- 最近一次模型请求实际 advertised tool catalog 的持久化与 replay 重建；
- execution lease 与 epoch；
- pending model attempt 与 unknown billing；
- pending interaction、steer、terminal control 与携带规范化 payload 的 command receipt；
- terminal exactly-once；
- no-key terminal replay。

旧 thread/message/goal tables 仍被 legacy `thread` CLI 等外围路径消费，不再服务交互 TUI
foreground。旧 Workflow/SubAgent JSON/JSONL 写入链已随隐藏执行路径删除，没有迁为
`RunStore` 双写。

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
- 当前 Run API v4 在 v3 的 continuation、manual compact 和 root list 上增加 durable
  creation-intent list/recover；当前 RuntimeEvent writer/reader 为 v6；
- crate dependency tree 不含 `crates/core` 或 `crates/tui`；
- 不启动 sibling TUI process。

完整接口见 [RUNTIME_API.md](RUNTIME_API.md)。

### Interactive TUI

交互 foreground 已切到 `AgentApplication`：

- `TuiRunClient` 提交 start/resume/continue/compact/steer/interrupt/cancel 和 interaction
  command；
- `CanonicalRunProjection` 与 presenter 只从 `RunStore` event 投影 root/child 进度、终态和
  durable outcome；
- 旧 foreground Engine、EventBroker、runtime-thread owner、`SessionManager`、child display
  cache 和 registry-driven slash command system 已删除；
- slash command 只剩统一的 `help/compact/cost/exit` canonical contract；
- 退役的 `crates/tui/src/compaction.rs`、`seam_manager.rs` 以及不再生效的 TUI
  `auto_compact` 开关/阈值状态均已删除；真正的 compaction 位于
  `crates/context + crates/runtime + crates/app`，手动 `/compact` 仍提交 canonical command；
- generic Provider/config/UI 仍未执行 DeepSeek-only 最终清理。

因此三个保留 foreground 入口与所有生产可达根/子 Agent 模型循环已经统一；最终 M4
集成门禁仍需确认完整调用图和回归。

## 4. Crate responsibility snapshot

| Crate | 当前生产职责 | 当前迁移债务 |
|---|---|---|
| `protocol` | canonical request、command、event、outcome、terminal | 后续 TaskContract/EvidenceReceipt 扩展 |
| `runtime` | 唯一根/子 Agent loop 与 reducer | completion/evidence 的 M5 强化 |
| `deepseek` | 官方 DeepSeek planner/transport/parser/accounting | FIM 调优与定期官方复核 |
| `context` | production prompt/context 构建与最小 compaction projection | RepoGraph、evidence-aware compaction 与 A/B 在 M5 |
| `tools` | 固定 production tool catalog 与执行 | 编辑/FIM 协议 A/B |
| `state` | SQLite RunStore、lease、replay | legacy thread tables 删除 |
| `app` | 唯一 production composition 与 Run command | 后续 orchestrator command |
| `app-server` | HTTP/SSE/stdio projection | 无独立业务状态 |
| `cli` | 顶层命令与 production config 解析 | DeepSeek-only 配置/中文 M7-M8 |
| `tui` | exec/interactive canonical projection + Provider 遗留 | 删除退役 context 实现和非 DeepSeek 产品面 |

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

M4-C foreground 切换后还已物理删除：

- 旧 foreground Engine、EventBroker 和 runtime-thread state owner；
- `SessionManager` 与旧 session/checkpoint helper；
- TUI child worker cache、mailbox reducer、fanout card 和第二展示真相；
- registry-driven slash command system（64 files，净删 25,516 行）；
- CodeWhale 自托管 MCP server 的两套实现与 `crates/mcp`；外部 MCP client、ACP 与
  canonical app-server 保留。
- 顶层 `codewhale update` 与 CLI 自更新实现；TUI 启动时版本检查和仍被 TUI/hooks 使用的
  `crates/release` 保留，不属于本次删除。

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
all-target check、全仓 clippy 和 workspace tests 均通过。该冻结本身不构成编码能力或
效率提升证据。

M4-C C2 已冻结为提交 `4a3311ac`：Run API v3、RuntimeEvent writer
v5/read v4-v5；后续提交 `35fc3cc4` 的 durable run creation delivery 把当前 Run API
提升到 v4、State schema 提升到 v9。continuation、root list、manual/automatic context
projection 已进入 exec/app-server 的 canonical 链路。focused、workspace Clippy
`-D warnings`、串行完整 workspace tests、内存/SQLite parity，以及由外部监督进程
`SIGKILL` 的 compaction prepared/in-flight/committed 恢复矩阵均通过。费用受限的官方
DeepSeek production sender canary 以 6/6 请求覆盖 Standard、Thinking/tool-history replay、
Beta Strict 与 FIM，完整 usage、无 transport retry，费用为 `USD 0.0000969904`；该 canary
不包含 compaction on/off 收益对照，且 `product_metric_eligible=false`。

此后交互 TUI foreground 与 child projection 已完成 canonical 切换：canonical Run 20/20、
canonical PTY 5/5、run presenter 13/13、canonical commands 5/5。最终请求许可机制通过
Runtime conformance 51/51 与 State `run_store` 18/18；State schema 已升至 v10，
RuntimeEvent 仍为 v6。严格 workspace clippy 当前仍被遗留 TUI 无消费者代码阻断，告警数量
随构建目标不同；不得压制，应继续删除。

当前证据证明三个 foreground 入口已统一，也证明协议、lineage、持久恢复、accounting 与
官方 surface 兼容；hidden workflow 第二循环已物理删除，但尚无终局许可机制的真实
multi A/B 或 compaction on/off A/B，不能声称 Token、成本或任务成功率改善。

## 7. 明确非结论

当前源码不证明：

- 当前 compaction 已证明节省 Token、降低成本或提高任务成功率；
- Provider 清理或全面汉化已完成；
- 当前中文 Agent prompt 已获得能力提升；首个正式 A/B 及后续 v2/v3 收敛 canary 均未通过，
  v3 的 multi child 两次用满 4 轮并把成功率降为 `1/3`，见
  [正式 A/B](../../eval/summaries/prompt-chinese-ab-2026-07-18.md) 和
  [收敛 canary](../../eval/summaries/prompt-convergence-canaries-2026-07-18.md)；
- RepoGraph、EvidenceReceipt、writer-worktree Orchestrator 已完成；
- 最终请求许可已经提高 multi verified success、降低 Token 或减少费用；
- transport 迁移本身提升了真实编码成功率；
- 单次 live canary 可以成为产品指标。

这些能力只能按 ROADMAP 的后续切片实现，并按 EVALUATION 的同任务、同预算、重复 A/B
决定保留或删除。
