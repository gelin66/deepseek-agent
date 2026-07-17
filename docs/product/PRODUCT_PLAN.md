# DeepSeek Agent 产品总纲

> 文档类别：产品权威。仅定义范围、原则与目标架构。

- 状态：已接受（V1 方向）
- 生效日期：2026-07-15
- 工作名称：DeepSeek Agent（正式品牌待定）
- 代码底座：CodeWhale `352e86a611fdf3cd8bd27c36d24d482c06a71117`

本文件是产品范围和目标架构的唯一总纲。开发顺序以
[ROADMAP.md](ROADMAP.md) 为准，能力取舍以 [EVALUATION.md](EVALUATION.md)
中的真实评测为准。当前源码事实记录在
[CURRENT_CODEWHALE.md](../architecture/CURRENT_CODEWHALE.md)，
它描述现状，不覆盖本文件中的目标方向。

当前实施阶段、已完成证据和未偿还证据债务只在
[ROADMAP.md](ROADMAP.md) 中维护。本总纲不复制随开发变化的里程碑状态，避免旧进度
覆盖路线图中的当前事实。

## 1. 产品结论

本仓库将演进为一个：

> Rust 原生、DeepSeek 专用、本地优先、支持单 Agent 与多 Agent、可恢复且可验证的编码 Agent 产品。

它不是继续维护上游 CodeWhale 的通用多模型发行版，也不是把 Cline、Codex、
Claude Code、Aider 或其他项目拼接进来。外部项目只提供能力参考；最终实现必须
在 CodeWhale 的 Rust 底座上形成一套统一逻辑。

产品保留三个入口：

1. 交互式 CLI/TUI。
2. Headless/NDJSON 本地自动化接口。
3. 多 Agent 团队执行。

三个入口必须使用同一个 Agent 内核、工具语义、事件协议和状态真相。

## 2. 北极星指标

所有新增、重构和删除决策都服务于：

```text
已验证任务成功率 / Token / 时间 / 代码复杂度
```

具体观察：

- verified success rate；
- false-success rate；
- wall time；
- 输入、输出和缓存 Token；
- API 成本；
- 工具与补丁失败率；
- 测试回归率；
- crash/resume 成功率；
- worktree 冲突率；
- 多 Agent 相对单 Agent 的净收益；
- 用户完成常见任务所需的配置和操作步骤。

代码更多、概念更多或架构图更复杂，都不构成能力提升。

## 3. 固定架构决策

以下是产品架构边界。除非真实评测和新的 ADR 证明必须改变，否则开发中不得漂移：

1. CodeWhale/Rust 是唯一源码和运行时底座。
2. DeepSeek 是唯一模型后端。
3. 只有一个 `AgentRuntime`。
4. 根 Agent 与子 Agent 运行同一个内核。
5. 只有一个 canonical `RuntimeEvent` 协议。
6. 只有一个 `RunStore` 持久化真相。
7. 多 Agent 是 `AgentRuntime × N + Orchestrator`，不是第二套系统。
8. 写 Agent 使用独立 worktree，并经过 diff、review、verify、merge、cleanup。
9. CLI、TUI、API 只发送命令和消费事件，不拥有模型循环。
10. 完成状态由最新工作区 revision 对应的证据决定，不由模型自报决定。
11. 新路径接管能力后必须删除旧路径。
12. 不引入 TypeScript sidecar、Cline runtime 或永久兼容桥。
13. 面向人的产品界面只使用简体中文，不提供语言配置、检测、切换或其他语言包；机器协议
    和技术标识保持稳定。
14. UI 消息目录与模型提示词组合是两个独立层次，不能用界面文案替代 Agent 提示词调优。

## 4. 第一性原理运行链

Agent 编码能力只有一条因果链：

```text
TaskContract
  -> ContextBundle
  -> AgentRuntime
  -> ToolOutcome
  -> EvidenceReceipt
  -> TerminalState
```

- `TaskContract`：目标、约束、验收条件和非目标。
- `ContextBundle`：与当前任务相关且受 Token 预算约束的代码和状态。
- `AgentRuntime`：唯一模型/工具循环。
- `ToolOutcome`：区分传输、操作、证据和重试语义的真实工具结果。
- `EvidenceReceipt`：绑定最新 workspace revision 的检查证据。
- `TerminalState`：Completed、Blocked、Failed、Cancelled 等类型化终态。

计划、任务板、工作流、多 Agent 和 UI 都只能是这条链路的投影或编排，不能产生
另一套事实系统。

## 5. 目标架构

```text
CLI / TUI / Local API
          |
   Application Service
          |
 Orchestrator / TaskGraph
          |
    AgentRuntime x N
     /    |    |    \
DeepSeek Context Tools RunStore
Backend  Broker       EventLog
                \
          Verifier / Evidence

Writing agents -> WorkspaceLane / Worktree
```

### 5.1 最终模块职责

| 模块 | 唯一职责 | 禁止依赖 |
|---|---|---|
| `protocol` | 领域类型、命令、事件和结果 | UI、HTTP、SQLite、Provider |
| `runtime` | 单 Agent 循环、transcript、tool replay、终态候选 | TUI、具体 DeepSeek HTTP、具体数据库、多 Agent 产品逻辑 |
| `deepseek` | Standard Chat、Strict Chat、FIM、SSE、reasoning、usage | TUI、工具实现、调度器 |
| `context` | RepoGraph、working set、request projection、compaction | UI、Provider 传输 |
| `tools` | 工具目录、执行、编辑、进程、验证适配 | TUI、模型协议 |
| `state` | append-only events、snapshot、replay、artifact | UI、模型调用 |
| `orchestrator` | TaskGraph、Agent 调度、预算、mailbox、worktree | TUI、DeepSeek wire format |
| `app` | 唯一 composition root 和产品级命令 | 业务状态复制 |
| `cli/tui/app-server` | 输入、输出与事件投影 | 模型循环、独立状态机 |

表中的模块名首先表示唯一职责，不要求立即创建同名 crate。先调整职责，最后再统一
crate 名称；不能为了目录好看而一次性搬动整个仓库。下一切片 M2-A 的 DeepSeek
request planner 直接进入现有生产 `crates/tui/src/client/`，不得先创建无人调用的新 crate。

### 5.2 真正可替换的端口

只在预计会独立变化的边界使用小型 Rust trait：

- `ModelPort`；
- `ContextPort`；
- `ToolExecutor`；
- `RunStore`；
- `Verifier`；
- `AgentControlPort`；
- `WorkspaceLane`；
- `RuntimeEventSink`。

内部结构保持具体直接。禁止为每个对象制造 Manager、Factory、Service 和 Adapter。

## 6. DeepSeek 原生能力

模型层必须显式区分：

```rust
enum ApiSurface {
    StandardChat,
    StrictChat,
    Fim,
}
```

固定语义：

- 普通 Chat 和普通工具调用走标准 Chat API；
- 只有整组工具都满足 strict schema 时才走 Beta Strict Chat；
- 任一工具不兼容 strict 时整组退回普通工具调用，不得丢工具；
- FIM 独立走 Beta Completions；
- Context Cache 是普通 Chat 的自动能力；
- assistant reasoning/tool-call 历史按 DeepSeek 协议精确回放；
- SSE、finish reason、usage、cache、retry 和 limits 由一个 Backend 统一负责。

每次模型调用必须先产生一个确定性的 `RequestPlan`，一次性固定 surface、endpoint、
wire model、streaming、reasoning replay 与工具 strict 状态；发送层不得再次猜测或改写。
M2-A 只在现有生产 Client 内接管这段预检并复用当前 transport、retry、SSE parser 和
Agent loop，不创建第二个 Runtime。待真实调用路径完成迁移并删除旧分支后，再决定是否
值得抽成独立 crate。

模型名、上下文上限、价格和 Beta 状态属于可变化能力，必须从官方协议 fixture 和
定期 canary 中验证，不能散落为永久业务假设。

当前 5/5 live canary 是上述 wire 契约的外部事实基线，且记录明确标记为
`product_metric_eligible=false`、`verified_success=null`。M2-A 切换生产请求路径后必须
重跑它，但无论通过多少次都不能替代 M1-C 的真实编码任务验收。

截至 2026-07-15，M2-A 已完成上述 `RequestPlan` 的生产 Client 代码接入和单元回归；
production-path 的 sender/FIM 垂直测试及切换后的 live canary 仍是未完成门禁。验收前
不得把“代码已接入”写成“Backend 已完成”或“真实链路已验证”。

### 6.1 中文原生交互与 Agent 提示词

中文原生产品分为两个解耦层次：

1. **人类界面消息目录**：唯一 `zh-Hans` 目录覆盖 CLI/TUI、帮助、配置向导、Doctor、
   错误与恢复提示、上下文状态、多 Agent 进度以及产品文档。产品不保存 locale，不读取
   `LANG/LC_*` 决定界面语言，也不提供语言选择、运行时切换、其他语言包或模型后处理翻译。
   根 Agent、子 Agent、恢复会话和 Headless 文本输出使用同一固定中文投影。
2. **模型提示词组合**：DeepSeek 的自然语言系统提示固定使用简洁中文；工具名、协议字段、
   路径、代码和原始输出保持机器契约。规划、工具策略、失败恢复、上下文压缩和子 Agent
   协作等能力提示仍按语义独立设计、版本化和评测，不能与 UI 消息目录或 Runtime 分叉绑定。

以下内容保持原样，不因汉化而改变契约：命令和 flags、工具名、JSON Schema 与 API 字段、
模型 ID、文件路径、代码、diff、stdout/stderr 以及上游原始日志。面向用户的错误应提供
中文摘要和可执行建议，同时保留可展开的原始技术细节。

固定中文是产品语言契约，不作为能力提升结论。任何改变规划、工具策略、失败恢复、压缩或
多 Agent 协作语义的提示词候选，必须在相同任务、仓库 revision、模型、工具面和预算下与
当前生产提示做 A/B，至少记录 verified success、工具错误率、模型轮次、Token、时延和
成本。只有能力不回归且至少一个关键效率或结果指标有明确净提升时才能接管生产路径。

## 7. 工具与完成语义

默认模型工具面保持精简，目标入口为：

```text
read
search
apply_patch
shell
git
verify
request_user_input
agent
complete_task
```

Skills 和 MCP 只先暴露元数据，需要时再加载完整定义。

`ToolOutcome` 必须至少表达：

```text
transport_status
operation_status
retryability
workspace_revision
evidence
artifacts
```

工具函数返回不等于操作成功，操作成功也不等于任务已验证。`complete_task` 只提交
完成候选；最终终态由 Runtime 根据 `TaskContract` 和最新证据判断。

## 8. Context 与 RepoGraph

第一版使用确定性的代码图，不先建设向量数据库：

- tree-sitter 符号和签名；
- ripgrep 文本引用；
- LSP definition/reference 补强；
- 文件和包依赖；
- git diff 与工作集；
- 测试影响；
- 任务相关性与 Token 预算排序。

Context compaction 必须保留任务目标、用户最新约束、当前变更、未解决问题、最新
证据以及 tool-call/result 原子性。只有评测证明确定性 RepoGraph 不足时，才考虑
本地 embedding 或混合检索。

## 9. 多 Agent 产品

多 Agent 是核心能力，保留可配置深度，但产品默认采用 supervisor tree：

- 1 个 Integrator；
- 按需要启动 Explorer、Implementer、Reviewer、Verifier；
- 只读 Agent 可共享只读工作区；
- 写 Agent 必须获得独立 worktree；
- Agent 数量、预算和深度由任务与评测决定。

所有角色都是同一个 Runtime 的 profile。模型侧只需要一个 `agent` 工具，通过
`spawn/followup/send/list/wait/interrupt/cancel` 操作。

子 Agent 必须返回结构化结果：

```text
summary
evidence
changed_files
checks
unresolved
artifacts
usage
terminal_state
```

Workflow、Fleet 和 Lane 中有效的 DAG、ledger、lease、heartbeat、worktree 思想会
迁入一个 Orchestrator；旧产品外壳在替代完成后删除。

## 10. 状态和恢复

最终只保留：

```text
runs
events
snapshots
artifacts
```

SQLite append-only event log 是唯一事实源。TaskGraph、Agent 状态、plan、usage、
diff 和 UI 状态通过 reducer 生成投影。大日志和 diff 使用内容寻址 artifact，数据库
保存引用。

必须支持：

- 单调事件序列；
- snapshot + replay；
- crash resume；
- schema migration；
- exactly-once terminal completion；
- artifact 清理策略。

## 11. 防止成为缝合怪

1. 外部能力先还原为它解决的问题，再决定 Rust 中的唯一归属。
2. 一个问题只能有一个 owner：一个循环、一个事件协议、一个状态真相、一个调度器。
3. 新能力必须写明替代和删除对象。
4. 临时兼容层最多跨一个里程碑，并在 Roadmap 中注明删除点。
5. 没有真实 DeepSeek 评测收益的功能不进入稳定核心。
6. 不采用强制六阶段、无限递归、自由聊天式 swarm 或默认双模型调用。
7. 不因“以后可能需要”提前建设通用 Provider、云平台或插件市场。
8. 产品复杂度留在内部；用户只看到任务、进度、diff、证据、成本和结果。

## 12. 固定方向与灵活实现

可以根据开发证据调整：

- crate 拆分和命名时机；
- RepoGraph 的具体实现；
- compaction 策略；
- 默认工具、Agent 数量和预算；
- Strict/FIM 的任务路由；
- snapshot 间隔；
- UI 交互和配置细节；
- 是否加入 embedding。

不能在没有 ADR 和评测证据的情况下改变第 3 节中的架构边界。灵活调整应遵循：

```text
真实问题 -> 证据 -> 最小方案 -> 垂直实现 -> 基准对照 -> 保留/修改/删除
```

## 13. 明确非目标

- 其他模型和 Provider 扩展；
- Cline/TypeScript runtime；
- 云 VM、Kubernetes、远程 Fleet；
- Hub、社交桥、定时任务和社区网站；
- 模型/插件市场；
- 企业 RBAC/SSO；
- 默认长期向量记忆；
- 每次工具调用全仓 checkpoint；
- 与 Agent 编码能力无关的 UI 重设计。

## 14. V1 完成定义

V1 必须同时满足：

- 只有 DeepSeekBackend；
- 只有一个 AgentRuntime；
- 根 Agent 和子 Agent通过同一 conformance suite；
- 只有一个 canonical RuntimeEvent；
- 只有一个 RunStore；
- 只有一个 TaskGraph 产品概念；
- CLI/TUI/API 是同一 Runtime 的薄客户端；
- 多写 Agent 有完整 worktree/review/verify/merge/cleanup；
- DeepSeek Standard、Strict 和 FIM 路由准确；
- RepoGraph 能处理跨文件代码理解；
- 完成状态依赖最新 EvidenceReceipt；
- 其他 Provider、旧 updater、重复状态和重复运行路径已清除；
- 真实评测证明产品优于导入时的 CodeWhale 基线；
- 保留的人类交互链路只使用 `zh-Hans`，不存在语言状态、选择器或其他语言包，也没有未列入
  技术白名单的英文泄漏；
- 中文原生 Agent 提示词通过同任务 A/B，版本可追溯、可回滚且没有能力回归；
- 使用步骤没有因为架构重构而变复杂。

## 15. 来源与许可

本产品基于 MIT 许可的 CodeWhale 源码继续开发。保留 `LICENSE`、必要来源说明和
第三方许可义务。外部 Agent 项目只作为架构和能力研究参考，默认不复制其源码。
