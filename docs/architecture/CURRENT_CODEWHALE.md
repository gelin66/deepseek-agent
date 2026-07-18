# Current CodeWhale Architecture

> 文档类别：迁移事实。只描述当前源码，不替代
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md)、
> [ROADMAP.md](../product/ROADMAP.md) 或 ADR。

- 快照日期：2026-07-19
- 导入基线：`352e86a611fdf3cd8bd27c36d24d482c06a71117`
- workspace version：`0.8.68`
- M4-B 被测代码：commit `a534a824670b60c807c5abf399ea8674d4beb527`，tree
  `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8`
- 当前阶段：M4-C 收尾；交互 TUI foreground 与 root/child projection 已迁移，当前
  canonical RuntimeEvent 为 v6、State schema 为 v10；隐藏 `workflow-tool` 第二模型循环
  及其私有状态/UI、ACP 独立模型/会话路径和 direct `review` 模型路径均已删除；
  child eager join 已进入 canonical Runtime；无生产构造入口但仍参与编译的旧 TUI
  SubAgent runtime/manager/registry 岛也已删除，最终完整门禁与其他旧编译岛复核仍待完成

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

旧 TUI `SubAgentRuntime`、`SubAgentManager`、`agents_*` 协调工具、私有 mailbox/
checkpoint/state、旧 child `ToolRegistry` 以及不可达 `/subagents` modal 也已物理删除。
Fleet 曾保留但从未接入的 `SharedSubAgentManager` 可选字段和 worker projection 同步删除；
Fleet 的真实执行仍是 `FleetExecutor -> codewhale exec`。只有 route、reasoning、prompt 和
全局 `FleetExecConfig` allow/deny 进入实际 argv；task-level role/tool scope 尚未执行，
所以仅为其声明 receipt 存在的 `WorkerRole`/`WorkerRuntimeProfile`/
`FleetWorkerRuntimeSpec` 已删除，`effective_permissions` 在 M6 enforced policy 接管前留空。

旧 TUI Goal/Hunt loop、私有 TaskContract/receipt/Goal completion store、Slop ledger 和
custom-command allowed-tools/pause 假状态也已物理删除。它们没有 canonical production
consumer，不能作为 M5 已有 evidence owner。M5 必须在 `protocol/runtime/state` 中新建唯一
TaskContract/EvidenceReceipt/Host completion 链路；历史测试只能作为反例参考，不能通过
adapter 恢复旧状态机。

WorkSurface 现在只投影 canonical child Agent，并保留 top/left/right 布局。旧键盘/鼠标
handler 从未接入生产事件循环，却生成不存在的 `/task` 与 `/jobs` 命令；该交互岛及其焦点、
选择、滚动、打开、停止和 hitbox 状态已物理删除。没有生产 writer、没有 RunStore 表或
RuntimeEvent 的 TUI-local Plan/Todo Store、`App.task_panel`、假工具及其 sidebar/footer/
transcript reader 也已删除。工具 Activity 继续读取 `run_presenter` 产生的 canonical
`GenericToolCell`，多 Agent 展示继续读取 canonical `child_agents`；`AppMode::Plan` 的只读
权限语义保留。M5 的 TaskContract/EvidenceReceipt 不通过恢复这些私有状态实现。
WorkSurface 不拥有 Runtime、Store、工具执行或 completion 判定。

旧 `ModePickerView` 与 `StatusPickerView` 没有生产构造或打开入口，只有模块内测试；两者及
其专属 View event 已删除。底层 `AppMode`、`StatusItem`、模式权限与 footer 状态投影仍由
现有真实调用方拥有。

旧 `ConfigView` 同样没有生产构造、打开入口或 canonical 命令；其 2,000 余行编辑/筛选/
渲染岛和专属消息已删除。底层配置仍从文件和环境加载，Doctor 指向实际配置文件。
旧 `ThemePickerView` 及其 `settings_picker` 框架也只有自测构造；两者、无消费者的
`ConfigUpdated` 事件与专属消息已删除。主题解析、持久化加载、Ocean 渲染与主题对比测试
继续使用同一组 `ThemeId`/`UiTheme`。
旧 `FeedbackPickerView` 也没有生产入口；它及其私有 command-palette 事件、失效的
`Ctrl+K` 帮助项已删除。输入 `/` 打开的 canonical slash menu 不受影响。
旧 `FilePickerView` 和 `file_picker_relevance` 只有自测与一个无人调用的 opener；modal、
事件和失效的 `Ctrl+P` 帮助项已删除。生产 `@mention` 菜单只通过 canonical key handler
补全 composer 文本，候选使用 `Workspace::completions` 的确定性排序；接受候选不会提交、
读文件或构造隐藏上下文，下一次 Enter 才原样发送 `@path`。无人消费的内联 renderer、假
pending-context 预览与 `file_frecency` 已删除，历史磁盘文件不做破坏性清理。
旧 `prompt_suggestion` 模块没有生产调用者，却直接请求任意 `/chat/completions` 并绕过
`AgentRuntime`、`RunStore` 与统一 accounting；该潜在第二模型请求路径、永不写入的 ghost
text 状态和配置已删除，composer 继续显示确定性的中文空输入提示。
旧 TUI `schema_sanitize` 只有自身测试且不在 production request path；这份重复 sanitizer 已
删除。DeepSeek Strict Function Calling 的整目录兼容性判断、原子 fallback 与 Beta Chat
路由仍由 `crates/deepseek` 唯一拥有。
同目录的 `schema_canonicalize` 也只有自身测试，从未处理 MCP 或 production 请求中的工具
schema；该 207 行假缓存优化已删除。真正的请求前缀稳定性只按 canonical DeepSeek 投影与
实际 cache 命中证据评估，不保留未接线的重复变换器。
旧 TUI `ResourceTelemetry` 整模块通过 `#[allow(dead_code)]` 隐藏且没有生产调用方；其
budget pressure、格式化和估算 token throughput 只由自身测试调用，App 中对应的
`last_output_throughput` 也只有初始化与清空、没有 producer 或 renderer。该模块和写空字段
已删除。canonical Runtime/RunStore 的 usage/accounting、DeepSeek usage 账本，以及 presenter
真实消费的 token/cache/reasoning/cost 状态不变。
旧 TUI `ContextBudget` 也只有自身测试和一个零调用的 `route_context_budget` wrapper，文件
注释明确其 engine/TUI consumers 从未接线。该 505 行 foundation 与 wrapper 已删除；仍有
真实调用方的 route context window、canonical DeepSeek output limit、`crates/context`
projection/compaction 和 Runtime/RunStore 的预算、恢复及 accounting 语义不变。
`route_runtime` 现只保留生产 `resolve_route_candidate` 与 context override：交互 TUI 用它
建立 active route limits，Fleet 用它生成 `FleetResolvedRoute` receipt。没有生产调用方的
`ResolvedRuntimeRoute` 配置快照包装、`resolve_runtime_route`、私有 base-URL 猜测器及其
五个自测已删除；全仓零调用的 `route_output_limit_tokens` wrapper 同步删除。
`known_route_limits`、`route_context_window_tokens`、pricing/provider-lake 和 Provider/M7
范围未改变。
旧 `workspace_context` 的 refresh/collect 链没有生产 caller，App 的 cache/cell/timestamp
始终保持空值；footer 与 empty-state 却把这个空值解释为非 Git 仓库。该模块及三个 ghost
字段已删除，`StatusItem::GitBranch` 断代替换为只读取真实 `App.workspace` 的
`StatusItem::Workspace`，旧 `git_branch` 配置键不设 alias。canonical Run workspace、
workspace guard、`git_status`/`git_diff`、Fleet branch 与 writer worktree 均未改变。
`TerminalInputPump` 现在只拥有真实的终端输入线程、poll/read、限时接收和 Drop 生命周期。
旧 Engine 的 heartbeat/liveness、child pause/ack、detached restart、pending queue/drain、
watchdog/recovery snapshot 与 pause/resume terminal helper 均无生产调用方并已删除。终端
Key/Paste/Mouse/Resize/Focus 仍进入 onboarding/canonical loop；canonical Run 事件继续由独立
的 `run_events.try_recv` 进入 `CanonicalRunProjection` 和 presenter。

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
请求实际 advertised tool catalog 拒绝未授权工具。该最终请求许可机制已有离线
conformance/Store replay 和真实 A/B；A/B 保留可靠性机制，但其原始实现的 multi
Token/时间/费用均回退，不能作为产品效率提升。

`528a72f2` 又把同一 `agent` 工具 batch 启动的 pending child 在下一次 root 模型请求前
eager join，使 durable `ChildFinished` 先于 root continuation，删除没有 handoff 新信息的
父模型等待轮。相对相邻基线 `f9dddd5d` 的 24-run、6/cell 精确 A/B 为 24/24 verified；
candidate multi 请求均值下降 `9.62%`，Token/费用均值下降 `8.27%/11.99%`，平均时间只变化
`-0.07%`。12/12 multi lifecycle/handoff 完整，但样本只覆盖一个固定任务，时间和费用的
成对结果各只有 2/6 更低，因此不能外推为普遍多 Agent 提速或成本优势。

C2 把 continuation 与 recovery 分开：`resume` 继续同一个 run，`continue` 从一个终态
root 创建新的 root，并用 `continued_from_run_id` 记录 lineage；source 不被改写。完整
canonical transcript 仍 append-only，compaction 只替换每次请求的 model-visible projection。
当前会先本地裁剪旧的大型工具结果，必要时才发出计入预算和 accounting 的 tool-free 摘要
请求；prepared/in-flight/failed/committed 由 RuntimeEvent v5 引入，并继续由当前 v6
持久化。

### DeepSeek backend

`crates/deepseek` 是官方 DeepSeek 请求事实 owner：

- Standard Chat、Beta Strict Chat 与 FIM surface 规划；
- 确定性 `RequestPlan`；
- Chat ordinary/non-streaming 与 SSE transport；
- reasoning/tool-call 历史回放；
- finish reason、typed error、retry 和 usage；
- root/child physical request attribution；
- auto-route classifier 与确定性 fallback；
- 官方模型 capability、output limit 和 pricing fixture。

普通工具调用不因存在工具就误走 Beta；只有整组 schema strict-compatible 时使用
Beta Strict Chat。FIM 仍是独立 Beta Completions request-planning surface；当前没有
canonical production FIM 编辑调用方或完整 response parser，因此不能宣称事务性 FIM
编辑已经可用。Context cache 由官方 Chat 的稳定前缀自动触发，不存在手工 cache API。

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
side effect、evidence、artifact 和 workspace revision。旧 TUI child `ToolRegistry` 已
删除。TUI 下仍编译的其他宽工具实现不代表 canonical production catalog 会自动扩大，
其余无消费者模块按独立调用方切片继续清理。

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
- Work surface 只投影仍存在的 plan/todo 与 canonical child facts，不再投影 TUI 私有
  Goal/Hunt 或 custom-command pause 状态；canonical Run 的工具 allow-list 不从旧 UI 状态
  注入。

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

- `core/runtime_contract` 中从未编译、没有生产消费者的 10 个 speculative shadow 文件；
  `RuntimeEventKind` 现在只在 `crates/protocol` 定义。仍被 exec 输出、runtime 与 CLI
  presentation 消费的 typed `termination.rs` 保留并继续真实编译。
- TUI 无调用者的独立 `resume`/Agent-root picker wrapper、专属过滤 helper、projection public
  cursor getter，以及没有 producer/handler 的 Fleet model-draft ViewEvent/delivery cell；
  `attach_or_resume`、`latest_root`、canonical app Run API、内部投影游标与真实 Fleet 保留。
- 旧 foreground Engine、EventBroker 和 runtime-thread state owner；
- `SessionManager` 与旧 session/checkpoint helper；
- TUI child worker cache、mailbox reducer、fanout card 和第二展示真相；
- registry-driven slash command system（64 files，净删 25,516 行）；
- CodeWhale 自托管 MCP server 的两套实现与 `crates/mcp`；外部 MCP client 与 canonical
  app-server 保留；
- `codewhale serve --acp` 及其 1,210 行独立 session、stream 和 direct
  `DeepSeekClient` 路径；显式 `--prompt "serve --acp"` 仍只是普通 canonical Agent 输入。
- direct `review` completion、模型内 `ReviewTool`、私有 receipt 状态/文档和退役 review
  UI；canonical reviewer Agent profile 与普通自然语言代码审查任务保留。
- 退役 TUI model client、cache、mock、retry surface 及其旧请求/响应/SSE DTO；仍在
  `models.rs` 的类型只服务现有展示、计量、MCP/工具 schema 和模型元数据消费者，不再发送
  DeepSeek 请求。
- 没有当前事件循环消费者的 TUI frame limiter、frame requester、motion policy 及其
  `constrained_frame_rate` 假设置；实际事件轮询、动画 cadence 和 `low_motion` 路径保留。
- 无生产写入方的持久 composer stash、Doctor 假投影和不可达 Ctrl+S/`/stash` 产品声明；
  `App::stash_current_input_for_recovery` 的进程内撤销草稿不属于该旧文件能力，继续保留。
- 唯一构造 helper 自身无调用方、且所有事件都没有 canonical handler 的 TUI Setup Wizard；
  CLI `setup`、Doctor setup 诊断、`SetupState`/`UserConstitution` 和提示词上下文消费继续保留。
- 无生产按键入口的 Activity Detail/Turn Inspector、shell details 路由、composer 外部编辑器、
  假快捷键提示和只读不写的推理折叠/详情高亮状态；canonical 审批参数仍可由 `v` 打开本地
  通用分页器并复制，审批状态继续只由 canonical interaction/RunStore 管理。
- 没有 canonical 命令、按键处理或 Run 事件的 `/jobs` 作业中心、Ctrl+B/Ctrl+X 假控制、
  sidebar 点击动作和 TUI 私有 shell manager；同步 `exec_shell` 的命令/输出直接投影到
  canonical 工具卡，生产 executor 的取消、超时和进程树清理保留。
- 只有自测而无 production delta 写入方的旧 TUI `StreamingState`/`streaming_thinking`
  collector 及其影子 reasoning buffer；canonical reasoning/assistant 流继续由
  `run_presenter` 从 durable `RuntimeEvent` 直接投影，完成态不再被旧 duration 字段误投影为
  “空闲”，DeepSeek reasoning replay 不变。
- 无生产消费者的 TUI Goal/Hunt loop、私有 TaskContract/receipt/Goal 工具、Slop ledger、
  verifier preview config、假 custom-command pause/allowed-tools 状态及其 UI/文档；
  canonical Runtime terminal、RunStore 和确定性 `crates/tools::run_verifiers` 保留。
- 没有生产构造或写入方的 TUI-local Plan/Todo Store 与 `update_plan`/`todo_*` 假工具，以及
  只读取永久空状态的 WorkSurface/sidebar/footer 和旧 transcript/checklist 特判；
  `AppMode::Plan`、canonical root/child Run 投影和多 Agent 能力保留。
- 只有测试构造、没有 canonical producer、RunStore 表或 RuntimeEvent writer 的
  `App.task_panel`/`TaskPanelEntry` 及其 background shell reader；WorkSurface 现在只显示
  canonical child Agent，Activity 继续显示 canonical `GenericToolCell`。
- 没有 production 构造方的 TUI `ExecCell`/`ExecSource` 专用展示岛，以及只读取该旧类型的
  foreground shell chip、CI 状态猜测、命令时长和 live-output 分支；canonical
  `ToolPrepared`/`ToolOutcomeCommitted -> GenericToolCell` 投影仍完整承载 `exec_shell` 的
  命令、状态、输出、失败可见性、transcript 与 Activity 展示。
- 同样没有 canonical producer 的 `Exploring`/`PatchSummary`/`DiffPreview`/`Mcp`/
  `WebSearch` 五种 TUI 专用工具卡及其聚合、状态和 renderer；`ToolCell` 单变体兼容壳也已
  折叠为直接的 `HistoryCell::Tool(GenericToolCell)`。固定 11 工具现在只走这一展示模型，
  `git_diff`/`git_status` 也按真实名称获得 edit/read 语义；MCP transport 与 CLI 不依赖旧卡。
- 只服务已删除旧工具、没有生产 executor 或 registry 消费者的 TUI `ToolSpec`、
  `ToolContext`、`RuntimeToolServices` 与本地 `SandboxPolicy`；错误分类直接使用
  `codewhale_tools::ToolError`，固定目录、production context、sandbox 与 shell owner 不变。
- 无调用方的 TUI OpenAI/Anthropic message/tool DTO 与 MCP `to_api_tools` 广告器；它们未进入
  canonical `AgentRuntime`，删除不会改变 MCP 配置、transport、OAuth、发现或 CLI 管理。
  当前 model-visible 工具仍只由固定目录和 Runtime 条件内建工具产生。
- 顶层 `codewhale update` 与 CLI 自更新实现；仍有真实 TLS、MCP/OAuth、Fleet 和显式
  Doctor 消费者的 `crates/release` 保留，不属于本次删除。
- TUI 私有生命周期 shell-hook owner、`App.hooks`、`[hooks]` 配置和虚假参考文档已物理删除。
  切换后 `mode_change` shell hook 不再执行，受信任工作区中的 `.codewhale/hooks.toml`
  也不再读取；其余生命周期事件原本就没有 canonical production 派发。
  Runtime/tools/Run projection、MCP notification、panic hook、Fleet webhook 和桌面通知均未改变。
- 零反向依赖的通用 `crates/hooks` 及其 stdout/JSONL/webhook/Unix-socket fan-out 原型；删除前
  workspace Cargo 反向图只返回该 crate 自身。`codewhale-config` 的 `[hook_sinks]` typed
  schema 与 key-value API 分支同样没有 sink 注册方或生产读取方，现已连同只验证旧 TUI
  lifecycle `[hooks]` 透传的自测承诺删除。该切片没有生产行为损失，也不改变通用 unknown
  extras 处理；MCP protocol notification、panic hook、Fleet alerts/webhook、桌面通知和
  canonical Runtime/tools/RunStore 都由其他真实 owner 保留。
- 没有构造方、不会读取图片数据的旧 TUI `ToolCell::ViewImage` 文本卡；canonical
  `read_file` 仍通过 `crates/tools` 的 macOS Vision/Tesseract 后端提供本地图片 OCR。
- 只有自测构造、没有生产打开入口的实时对话 overlay、其专用缓存和失效快捷键；主
  transcript 仍由 `CanonicalRunProjection`/presenter 与 `TranscriptViewCache` 实时驱动。
- App 状态始终为 `None`、没有生产 toggle/key handler 的 File Tree pane 及其后台目录扫描；
  `file_mention` 仅保留 composer 路径补全，不与旧 pane 或隐藏文件注入混为一体。
- 只有自身测试消费者、从未被 canonical prompt composition 调用的旧 TUI `memory.rs`；生产
  system prompt 仍由 `crates/context` 唯一构造，项目 instructions/skills/WorldState 不变。
- 没有任何生产 renderer 或 key handler 读取的静态 `keybindings.rs` 目录；帮助与按键行为
  只以真实 UI/PTY 契约为准，不再维护一份自测通过但不可见的快捷键表。
- 零调用方的旧 TUI MCP manager formatter/pager adapter，以及没有生产 writer 的 manager
  snapshot DTO、App cache、restart hint 和伪连接健康投影。`McpPool`、transport、OAuth、
  配置加载与顶层 `codewhale mcp` 的连接/工具发现仍是真实生产能力；当前 canonical Agent
  固定工具目录并未加载 MCP pool，也不存在 TUI `/mcp` manager 或 model-visible MCP 工具。
  私有 `mcp` 模块中八个没有生产消费者的 public wrapper、对应 dead-code allow 与无人读取的
  shutdown report 也已删除；真实 reload/reconnect/transport shutdown 生命周期不依赖它们。
- 零消费者的 `fast_hash` 类型别名与用户 regex LRU cache；真实正则消费者保留在各自 owner。
- 只有自身测试、App 只默认构造且从不读取的通用 Provider readiness snapshot；DeepSeek
  production transport 与 Doctor 明确探针继续分别承担请求和诊断职责。
- App 永远为 `None` 且没有生产 writer/handler 的旧 Decision Card overlay；结构化选择的
  唯一真实路径仍是 canonical `request_user_input` interaction。
- 零生产调用方的 TUI `arg_repair` 启发式修补器；正式路径由 DeepSeek transport 精确拼接
  SSE 参数，`ToolArguments` 同时保留 raw/parsed，Runtime 对畸形 JSON 产生 typed retry
  outcome 而不篡改原始调用。
- 只有自身测试的旧 `/models` 英文消息 formatter；TUI canonical 命令面没有模型列表或
  picker，运行模型仍由 DeepSeek 配置进入 canonical Run 请求。
- 从未被生产构造、没有事件 handler 的旧 `ElevationView`/widget/event 整岛；真实审批与
  结构化提问仍由 canonical interaction 承担，Run 级 `allow_sandbox_elevation` 策略保留。
- 未注册且零执行调用方的旧 TUI `RequestUserInputTool`/parser 与 prompt shadow；保留的
  UserInput modal 直接消费 protocol request/response，提交仍落入 canonical RunStore。
- 旧 `composer_ui` 键盘处理器岛；唯一真实使用的 slash-menu 选择已迁回 canonical 事件
  owner `ui.rs`，其余 escape/history/word-motion/newline helpers 没有生产调用方。
- TUI 私有 approval cache、exact/grouping key、永远未设置的 timeout/tick，以及没有
  canonical Runtime 消费者的“批准并保存询问规则”事件载荷和界面。当前审批事件只携带
  `interaction_id + decision`，由 `TuiRunClient` 调用 canonical `resolve_interaction` 或
  `cancel`；工具、风险、参数与 durable replay 仍由原真实链路承担。
- 交互循环中曾零调用的 `ViewStack::handle_mouse` 现已成为唯一 modal 鼠标入口；活动 modal
  优先消费点击/滚轮并阻止背景穿透，产生的事件仍交给现有 canonical view-event handler。
  无 modal 的 transcript 滚动路径保持不变，没有新增私有 interaction owner。
- `scrolling.rs` 的 `TranscriptScroll::anchor_for` 只有自身测试；rapid mouse acceleration
  类型只被 `ViewportState` 默认构造且没有生产 reader。两组叶子现已删除，真实滚动仍由
  canonical mouse 的固定三行 delta、`pending_scroll_delta` 和
  `TranscriptScroll::scrolled_by`/`resolve_top` 驱动；Pager 保持独立滚动处理。
- `ColorCompatBackend` 的 forced/cached size override 字段和 setter 没有任何生产 writer，
  仅由同文件 3 个测试构造虚假尺寸路径，现已删除。`Backend::size()` 直接读取真实
  Crossterm backend；保留的颜色深度适配、palette/theme 更新与 OSC8 link 发送继续由原 owner 执行。
- transcript `selection.rs` 的 selection/autoscroll 状态没有任何生产事件 writer，只会默认构造，
  再被自动滚动 guard 与 renderer 读取；该模块、viewport 字段、resize clear、着色 helper 和专属自测
  现已删除。composer `selection_anchor`、菜单/审批 palette 状态、Pager 系统复制及 canonical scroll 均未改变。
- sidebar 的旧 hover/click 元数据只有 renderer producer，没有事件 handler、tooltip 或
  popover consumer；`SidebarHoverState`/section/row/action、每帧全文克隆、tooltip shadow
  和从未构造的 `SidebarAgentCancel` 已物理删除。可见 Activity/Agents/Session 行继续直接
  渲染，`last_sidebar_area`/resize handle、canonical child/Fleet、modal 鼠标与 transcript
  滚动保持原生产调用链。
- canonical TUI presenter 曾在 `GenericToolCell` 之外重复写入完整工具参数/输出到
  `ToolDetailRecord`，但该详情图没有任何生产 renderer、交互 handler 或其他语义 reader；
  只有历史前缀重键、active flush 搬运和自身 replay 测试维护这份影子状态。该结构、两个
  App map 及重复 producer 已删除；`tool_cells` 仍负责 prepared→outcome 的真实 history
  原位更新，`HistoryCell::Tool(GenericToolCell)` 仍承担 live/replay/transcript/Activity
  展示，canonical `RuntimeEvent`/`ToolOutcome`、`ActiveCell` 与 child/Fleet 均未改变。当前
  定向证据为 presenter 13/13、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check、
  fmt 与 diff-check。
- 没有生产构造者的 TUI `AutoReviewPolicy`、动态 allow/block 配置、私有审计事件和重复的
  shell/action 风险分类。production `crates/tools` 直接在 canonical
  `ToolApprovalPrompt::risk` 中给出 `Routine`/`Elevated`/`Critical`，TUI 只负责穷尽投影与
  展示，不再根据工具名或参数重算风险。
- 旧 Engine 的 post-edit LSP 集成。`crates/tui/src/lsp` 中的 `LspManager`、stdio transport、
  `publishDiagnostics` parser、diagnostic renderer 和 language registry 没有任何生产构造方，
  所有 manager 调用都仅属于自测；该四文件编译岛、TUI/`codewhale-config` 的重复
  `[lsp]` schema、侧栏虚假 `lsp: on/off` 状态和专属文档已同步删除。MCP transport 及
  `notifications/initialized` 不在该路径，继续保留。M5 若用 LSP 补强 RepoGraph，必须在
  canonical context/tools 路径重新实现并评测，不恢复 TUI Engine 兼容层。
- 旧 TUI side-git snapshot 岛及 `[snapshots]` 配置。它没有 production snapshot writer、
  restore/list UI、`/restore` handler 或 `revert_turn` 工具；唯一真实调用方是交互启动时对旧
  仓库执行保留期 prune。该 janitor 随整岛删除后，不再自动清理
  `~/.codewhale/snapshots`/`~/.deepseek/snapshots` 中的历史 side-git 数据，也不会在切换时
  自动删除用户文件。Runtime `RunSnapshot`/`RunReplay`、State `agent_run_snapshots`、canonical
  reducer/crash replay、工具 artifact 和 Fleet checkpoint 是不同 owner，均未改变。
- 旧 `setup --clean` JSON cleanup 产品面。`SessionManager` 删除后，
  `sessions/checkpoints/latest.json` 与 `offline_queue.json` 已没有 production writer/reader；
  `CleanPlan` 只在显式 CLI cleanup 及自身测试内闭环。删除后程序不再列出或移除既有历史
  文件，用户需要时只能手工处理。系统 skills 的生产自动安装仍保留；无调用的 uninstall/
  bundled-name helper 同步删除。Setup `--force`、SQLite RunStore/`agent_run_snapshots`、crash
  replay、进程内 busy-message queue、Fleet ledger/checkpoint 和 constitution checkpoint 均不经
  该路径。
- 没有生产命令、canonical Runtime 调用方或模型可见入口的 community skill installer。
  `crates/tui/tests/skill_cli.rs` 只是用 `#[path]` 重新编译该源文件，不是 CLI 验收；现已连同
  registry URL/安装大小 schema、伪配置说明和 `tar`/`flate2` 直接依赖物理删除。TUI
  和 `codewhale-config` 都不再将两个旧键建模为受支持 schema；config crate 只保持 TUI
  `[skills]` 表的通用 extra 往返，其中当前唯一有产品语义的键是 `scan_codewhale_only`。系统
  `install_system_skills`、版本 marker、local skill discovery 与 prompt
  注入继续工作；仅宣称不存在 `/skill install/update/trust/uninstall` 的 bundled
  `skill-installer` 不再进入新安装，既有用户目录不会被程序主动删除。
- 旧 `workspace-trust.json` 外部路径快照的 `add/remove` 及原子写入只有自身测试调用，
  现已物理删除。保留的 production 路径只用 `WorkspaceTrust::load_for` 读取已有文件，
  再把 canonical paths 交给 `ProductionToolConfig`；`permits` 只保留为读取契约测试。这与
  onboarding/MCP 仍在真实读写的 `[projects].trust_level` 是两条独立路径，后者及
  approval、Runtime 权限、project trust 均未改变；既有用户 trust 文件不会被自动清理。
- TUI startup version checker 及 `[update]` schema。`spawn_startup_version_check` 在全仓只有
  定义，既没有启动调用，也没有 task join、toast 或 renderer 消费者；其余 release JSON、
  asset completeness 和自制 semver helper 只由这条死链及自身测试引用。删除因此没有运行时
  行为损失，旧 `[update]` 表不再由 typed config 消费。Doctor 仍显式执行 release 诊断；
  `crates/release` 的平台 HTTP/TLS builder 继续服务 MCP/OAuth/Fleet alerts 与配置网络调用，
  均未随幽灵启动检查删除。
- pre-session Launch menu 的 `LaunchState`、action/handler、renderer/hitbox、`launch_screen`
  设置和专属本地化。生产启动曾构造该状态并同步执行 `git rev-parse`，随后在第一帧前无条件
  将其隐藏；没有 launch action 进入事件循环、Run command、Lane 或 Fleet。删除因此只移除
  不可达 UI 和无效启动子进程，零生产行为损失。首启 onboarding、canonical TUI Run、CLI
  resume/continue、underwater shell/ocean 与 Fleet/Lane/worktree 均不经过该旧外壳。
- TUI desktop notifications 模块及其 `[notifications]`/`tui.notification_condition` schema、
  专属本地化和 Windows Audio/Debug/UI features。OSC/BEL/macOS 通知、声音、terminal
  title/taskbar 及配置入口没有模块外生产调用，故删除为零生产行为损失；唯一真实消费者是
  footer 对 `humanize_duration` 的调用，该 helper 和边界测试现由 footer 直接拥有。MCP
  `notifications/initialized`/`notifications/progress`、Fleet alerts/webhooks、canonical
  active/run status 和 panic hook 均为独立生产路径，未被删除。
- 系统剪贴板 read/image 与伪 composer attachment 状态。生产只有 Pager copy 经
  `ClipboardHandler::write_text` 写系统剪贴板；图片读取/PNG 落盘、App attachment
  插入/选择/移除、手写 `[Attached ...]` parser 和对应 UI/文档均没有生产入口，删除为零
  生产行为损失。arboard 现关闭默认 image feature，TUI direct `image` dependency 已删除；
  terminal `Event::Paste`/onboarding、文本 writer、普通 `@mention`、canonical `read_file`
  OCR 和现役 composer 文本编辑均保持原 owner。旧 rapid-key paste-burst handler 从未进入
  canonical Key 事件分支，App 只会轮询无法由生产输入激活的默认状态；该两模块、状态、
  设置/别名和自证测试现已物理删除。无 bracketed marker 的字节按普通按键直接进入
  composer，不再虚构可区分快速键入与裸 paste；真实 bracketed mode 启停/恢复、API-key
  paste、CRLF/裸 CR 归一化、超大文本处理与 Pager copy 均不经过已删除 fallback。
- `key_shortcuts` 中零调用的 copy/paste/control-like/Ctrl-H predicates；保留首启输入所需
  的 `is_text_input_key`，真实 paste/copy 仍由 terminal event 与 Pager local event 承担。
- 零生产消费者的通用 `[vision_model]`/`image_analyze` 配置与 feature；正式模型面仍只有
  DeepSeek，图像文本提取继续由 `crates/tools` 的本地 `read_file` OCR backend 承担。
- 首启状态机不可达的 Provider 选择页与 picker memory；当前真实首启只包含 Welcome、
  DeepSeek API Key、工作区信任和 Tips，不再编译一页无法到达的多 Provider UI。
- 零生产调用方的 provider-lake picker/dashboard 查询面；保留的 catalog lookup 仍服务现有
  pricing 与尚待 M7 收敛的 route metadata，canonical Fleet 和 DeepSeek transport 不读取
  已删除的 configured-provider/model-list API。
- 只由自身测试调用的 TUI `is_key_file`/`summarize_project`/`project_tree` 浅层 project-map
  helpers；生产上下文仍由 `crates/context`、显式文件工具与 canonical transcript 负责，
  没有为尚未开始的 M5 RepoGraph/ContextBroker 保留兼容层。
- 零生产调用方的 TUI `open_url` 及其平台 browser-command 构造器；当前没有外链打开交互，
  OAuth/MCP transport、终端复制和 DeepSeek HTTP 请求不依赖该 helper。
- 零调用的 TUI `record_caught_panic`、`ensure_dir`、`pretty_json`、`url_encode` 与
  `estimate_message_chars`；真实使用的 supervised panic dump、写入、路径显示和计数 writer
  继续由各自生产调用方保留。
- 从未进入固定 production catalog 的旧 TUI `js_execution`、Node resolver 和 Doctor 假
  注册提示；显式配置的 Node MCP server 与 canonical `run_verifiers` 的 Node 项目检查不经
  过该旧模型工具，继续保留。
- 从未接入 production catalog 的 TUI script-command plugin ToolSpec、复制扫描器的假 E2E、
  `[tools.plugin_dir]`/`[tools.overrides]` 与 `setup --tools`/Doctor 脚手架；canonical 固定工具
  目录不再被配置宣称可替换，真实 MCP、skills 和现有 plugins 目录逻辑不变。
- 只有自身测试、没有生产读取方的 TUI `large_output_router`、`[workshop]` 配置、
  `ToolContext` router/vars 字段与未接线的 V4-Flash synthesis 声明；canonical artifact/
  evidence owner 不在该路径，旧 spillover writer 仍待独立调用图切片。
- 没有生产注册者或 producer、唯一 store 只由默认 `ToolContext` 空建的 TUI
  `handle_read`/`VarHandle` 原型与 process-local store；canonical artifact、RuntimeEvent、
  RunStore 不经该路径，最后仅被它使用的计数 writer 也已删除。
- 零执行调用方的旧 TUI tool-output spillover/retrieval 链、只服务该链的私有 artifact 文件、
  Doctor/启动 janitor 投影和始终为 `None` 的 history 字段；canonical `ToolOutcome.artifacts`、
  verifier artifact、RuntimeEvent 和 RunStore 不依赖这套旧文件缓存，继续保留。

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
Runtime conformance/State replay；eager join 后 Runtime conformance 为 53/53，State
`run_store` 为 18/18。focused 的 TUI 过滤器现只覆盖 canonical Run 投影、命令、本地
approval、Fleet 和 DeepSeek Doctor，并继续对零匹配 fail closed；旧 memory/schema/client/
stream 测试已退出该门禁。State schema 已升至 v10，RuntimeEvent 仍为 v6。严格 workspace
clippy 当前仍被遗留 TUI 无消费者代码阻断，告警数量随构建目标不同；不得压制，应继续删除。

当前证据证明三个 foreground 入口已统一，也证明协议、lineage、持久恢复、accounting 与
官方 surface 兼容；hidden workflow、ACP 和 direct review 模型路径已物理删除。最终请求
许可和 eager join 均已有真实 multi A/B：前者的原始实现保留可靠性但 multi 效率未通过，
后者在一个固定任务中降低请求且不回归 handoff。完整 eager-join 身份、四 cell 与 pair
边界见
[子 Agent eager join 精确 A/B](../../eval/summaries/eager-join-exact-ab-2026-07-18.md)。
compaction on/off A/B、其他旧编译岛复核和 M4 完整门禁仍未完成。

## 7. 明确非结论

当前源码不证明：

- 当前 compaction 已证明节省 Token、降低成本或提高任务成功率；
- Provider 清理或全面汉化已完成；
- 当前中文 Agent prompt 已获得能力提升；首个正式 A/B 及后续 v2/v3 收敛 canary 均未通过，
  v3 的 multi child 两次用满 4 轮并把成功率降为 `1/3`，见
  [正式 A/B](../../eval/summaries/prompt-chinese-ab-2026-07-18.md) 和
  [收敛 canary](../../eval/summaries/prompt-convergence-canaries-2026-07-18.md)；
- RepoGraph、EvidenceReceipt、writer-worktree Orchestrator 已完成；
- eager join 已在广泛任务上提高 multi verified success、降低 Token/费用或缩短时间；
- transport 迁移本身提升了真实编码成功率；
- 单次 live canary 可以成为产品指标。

这些能力只能按 ROADMAP 的后续切片实现，并按 EVALUATION 的同任务、同预算、重复 A/B
决定保留或删除。
