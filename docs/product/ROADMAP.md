# DeepSeek Agent 开发路线图

> 文档类别：产品权威。仅定义实施顺序、迁移和删除点。

- 状态：执行中
- 当前阶段：M1 评测基线（M1-A/M1-B 已完成，M1-C 待完成）；下一工程切片为 M2-A production request planner
- 上次更新：2026-07-15

本文件是唯一执行路线。产品边界见 [PRODUCT_PLAN.md](PRODUCT_PLAN.md)，评测规则见
[EVALUATION.md](EVALUATION.md)。本文件可以根据开发证据调整顺序和实现细节，但不能
静默改变产品总纲中的固定架构决策。

## 1. 当前基线

- 导入基线：CodeWhale `352e86a611fdf3cd8bd27c36d24d482c06a71117`。
- 基线版本：workspace `0.8.68`。
- 当前真实 Agent loop 仍位于 `crates/tui`。
- `crates/core` 尚不是生产模型执行内核。
- 根 Agent 与子 Agent 仍有不同运行循环。
- M1-A 离线契约证据与生产工具目录测量已经完成。
- M1-B 官方 DeepSeek live canary 已通过 5/5，但仅属于协议兼容证据。
- 尚无导入基线与当前候选之间的真实编码任务 A/B，不能声称 Agent 能力提升。
- 当前已有一组 DeepSeek 协议、Agent 可靠性和独立 verify 实验 WIP。
- WIP 保存提交：`2ccccdd4`。
- 本地归档分支：`archive/pre-product-plan-20260715`。
- 该 WIP 尚未通过新的产品评测门禁，不能视为稳定能力。

## 2. 执行原则

每个开发切片必须包含：

1. 问题和验收条件；
2. 契约或回归测试；
3. 一条可运行的垂直实现；
4. 调用方迁移；
5. 旧路径删除；
6. 基准结果与文档更新。

禁止只新增抽象或新实现而长期不接管生产入口。

“垂直迁移”必须在同一切片内让一个真实生产入口改用新内核，并删除该入口的旧循环、
旧事件翻译或旧状态写入。临时兼容层最多跨一个里程碑；任何时刻都不能长期保留两套
生产 Agent loop 或两个可写状态真相。

## 3. 里程碑总览

| 里程碑 | 目标 | 状态 | 主要退出门槛 |
|---|---|---|---|
| M0 | 保护基线、整理仓库、建立唯一文档真相 | 已完成 | 工作区可追溯，产品方案落库，现有 WIP 被隔离说明 |
| M1 | 建立原始 DeepSeek 能力基准 | 进行中（M1-A/M1-B 已完成） | 真实编码 A/B 可重复测量成功率、假成功、Token、时间和成本 |
| M2 | 独立 DeepSeekBackend 与领域协议 | 待开始（M2-A 下一工程切片） | Production RequestPlan 接管真实请求，旧 DeepSeek 决策分支删除 |
| M3 | 最小 Headless AgentRuntime 垂直切片 | 待开始 | 可完成 read/edit/shell/verify/complete 真实任务 |
| M4 | 统一工具、事件、RunStore 和产品入口 | 待开始 | CLI/TUI/API 同事件，旧 core/bridge 路径删除 |
| M5 | RepoGraph、ContextBroker 和证据化完成 | 待开始 | 成功率或 Token 明显优于基线，假成功下降 |
| M6 | 统一多 Agent 与 worktree 生命周期 | 待开始 | 根/子 Agent 同内核，并行任务产生净收益 |
| M7 | DeepSeek 专项调优与产品清理 | 待开始 | 其他 Provider 和重复产品外壳被删除 |
| M8 | V1 本地产品化 | 待开始 | 自己的品牌、配置、CI、打包和开发流程完整 |

## 4. M0：仓库基线与整理

### 工作

- 保存原始 CodeWhale 提交和现有 DeepSeek WIP。
- 建立 `PRODUCT_PLAN.md`、`ROADMAP.md`、`EVALUATION.md` 与核心 ADR。
- 重写根 README/AGENTS/CLAUDE/CONTRIBUTING，使其不再传播上游多 Provider 目标。
- 删除明确的版本 tracker、handoff 等工作状态残留。
- 给现有文档建立唯一索引和权威级别。
- 删除与 Rust Runtime 无编译依赖的网站、VS Code scaffold、npm 包装、云发布脚本、
  上游社区自动化、翻译和旧 dogfood/release 资料。
- 删除未被当前 Rust remote-setup 注册的 WeCom/Weixin 聊天桥。
- 盘点仍被 Runtime 引用的聊天桥、remote setup、通用 Provider 和旧 evidence。
- 不在本阶段破坏性移动 Agent 核心源码。

### 退出门槛

- 当前工作树没有来源不明的文件。
- 仓库入口只指向一套产品方案。
- WIP 和稳定能力明确区分。
- Markdown 链接和 `git diff --check` 通过。
- 清理没有影响 Cargo workspace metadata。

### 完成记录（2026-07-15）

- 原始导入、审计前 WIP 和归档分支均可追溯。
- 产品、架构、决策、参考与遗留文档已经分区，并建立唯一索引。
- 已移除与 Rust Agent 产品无编译依赖的上游网站、编辑器扩展、npm 包装、发布、社区与旧项目管理材料。
- `cargo metadata`、`cargo check`、Focused DeepSeek gate 和全 workspace tests 通过。
- Markdown 链接、Shell 语法、格式和 Git whitespace 检查通过。
- 本机 stable toolchain 未安装 Clippy component；CI 会安装并运行，不能把本地缺失记为代码通过。

## 5. M1：评测基线

### 已完成：M1-A 离线契约基线

- 当前提交在同一 Harness/manifest 下通过 41/41 离线用例。
- 导入提交在同一 Harness/manifest 下通过 12/12 个 `cross_revision` 可比较用例。
- 12 个跨提交用例包含生产 Engine 离线链路与多 Agent 契约，作为重构防回归底线。
- 另外 29 个 `candidate_only` 用例只证明候选契约通过，不证明能力提升。
- 完整证据、哈希和解释边界见
  [M1-A 离线契约基线](../../eval/summaries/m1-offline-baseline-2026-07-15.md)。
- [`scripts/measure-tool-catalog.py`](../../scripts/measure-tool-catalog.py) 已通过真实生产 Engine
  turn 测量完整目录、模型可见目录和确定性字节估算；该测量描述工具面规模，不是
  Provider Token、真实模型调用或能力提升证据。

M1-A 完成不等于 M1 完成：离线用例没有真实 DeepSeek Token、cache、成本和可验证任务
成功率，两套用例的总耗时也不可用于性能比较。

### 已完成：M1-B 官方 DeepSeek live canary

- 在干净提交 `366e8b5b37bedbbf3b1ebb326b14a70e885a57a2` 上，以 5 个受费用、Token、
  请求数和超时限制的真实请求通过 5/5。
- 覆盖 Standard Chat、Thinking tool call、`reasoning_content` exact replay、Beta Strict Chat
  和 Beta FIM；usage/cache 与 finish reason 均按各用例契约记录。
- 结果、哈希、usage 和费用口径见
  [M1-B DeepSeek live 协议 Canary](../../eval/summaries/m1-b-deepseek-live-2026-07-15.md)。
- 结果明确记录 `record_class=protocol_canary`、`product_metric_eligible=false` 和
  `verified_success=null`；它只证明当时官方 API 的 wire 契约兼容，不是编码任务成绩，
  也不证明当前生产 Client 已统一通过一个 RequestPlan 生成这些请求。

### 待完成

1. **M1-C 固定真实编码任务基线**：同任务、同仓库 revision、同预算、同确定性验收器，
   对比导入提交和候选提交的 verified success、false-success、Token、时间、费用和 diff。
2. **M1-D WIP 处置**：根据离线、live 协议和真实任务三层证据，对 DeepSeek 协议、
   Agent 可靠性、`verify` 和本地配置逐项给出保留、
   重做、缩小或删除结论。

`verify` 仍是额外模型评审实验，不等同于测试证据；未经真实缺陷检出率和误报率评测，
不得默认成为完成门禁。

### 退出门槛

- 离线契约与 live 协议基线已经可重复；真实任务 A/B 仍须可重复。
- 真实任务能测量 verified success、false-success、输入/输出/cache Token、时间和成本。
- 每项 WIP 有保留、重做、缩小或删除结论。
- `verify` 未经评测不得默认成为完成门禁。

## 6. M2：领域协议与 DeepSeekBackend

### M2-A 下一切片：production DeepSeek request planner

- 不新建 crate；在现有生产 `crates/tui/src/client/` 内建立最小
  `ApiSurface::{StandardChat, StrictChat, Fim}` 与 `RequestPlan`。
- `RequestPlan` 一次性决定 surface、endpoint、wire model、streaming、reasoning replay、
  工具/strict 状态和请求 body，发送层不得再次改写。
- 首先接管 `codewhale exec`、TUI 和子 Agent 共用的真实 DeepSeek streaming Client 路径；
  Standard/Strict 使用现有 Chat 响应与 SSE parser，FIM 保留独立 Beta Completions 语义。
- Strict 只有在整组函数 schema 兼容时启用；任一不兼容即原子退回 `StandardChat`，
  保留全部普通工具调用。
- 复用现有 HTTP transport、retry、usage parser、SSE decoder 和 Agent loop；本切片禁止
  新 Runtime、第二套 Client 主循环或大范围目录搬迁。
- 用纯 planner 单测、WireMock 生产路径测试和现有 5/5 live canary 复核切换结果。

### 同切片删除/替代

- 生产路径切换时删除它原有的 DeepSeek URL、Beta route、strict flag 保留/剥离和 FIM
  endpoint 决策分支；不能让新旧 planner 并存。
- 保留仍被未迁移调用方或其他 Provider 使用的通用 transport；不得为了目录纯度扩大删除范围。
- 不新增独立 `deepseek` crate。只有当生产调用方已经迁移、旧分支已经删除且依赖方向明确后，
  才评估物理抽取。

### 后续工作

- 在 `protocol` 定义 Task、Run、Turn、Event、ToolOutcome、Evidence 和 TerminalState。
- 建立版本化 NDJSON 事件。
- 在生产迁移中逐步形成独立 `DeepSeekBackend` 职责；后续层不得再次猜 URL、删除工具或改变 surface。
- 统一 reasoning replay、SSE、finish reason、usage、cache、retry 和 limits。
- 将现有官方 live canary 固化为 Backend 变更后的受限回归，不把它当作编码 benchmark。

### 删除/替代

- 冻结旧通用 client，不再增加能力。
- 每迁移一个真实调用方，同一切片删除它对旧 URL、序列化、
  parser、retry 和 usage 分支的依赖。
- 新 Backend 接管全部调用后删除旧 DeepSeek 路由分支和通用 Provider 选择路径。

### 退出门槛

- `RequestPlan` 已由真实生产调用路径消费，不是无人调用的新抽象。
- 协议 fixture 覆盖正常流、畸形流、工具循环、retry 和不完整终止。
- Production planner 切换后的 Standard、Thinking exact replay、Strict 和 FIM canary 仍通过。
- Strict 不兼容时普通工具调用保持可用。
- usage 无重复计算。
- Backend 不依赖 TUI、工具实现或调度器。
- 没有新增第二个 Runtime、第二个生产 Client loop 或仅为未来准备的新 crate。

## 7. M3：最小 AgentRuntime

### 垂直链路

```text
Headless Task
  -> DeepSeek
  -> read/search
  -> patch/shell
  -> verify
  -> complete candidate
  -> RuntimeEvent
```

### 工作

- 以真实 `codewhale exec` 为第一个生产调用方，不先铺设无人使用的新框架。
- 从 TUI Engine 提取唯一 `AgentRuntime`、canonical transcript、request projection 和 turn loop。
- 建立 `ModelPort`、`ContextPort`、`ToolExecutor` 和 `RunStore` 等小端口；首个切片使用
  `DeepSeekBackend`、内存 `RunStore`、fixture Backend 和最小真实工具集。
- 所有运行进度只发出 canonical `RuntimeEvent`；Headless 入口只发送命令和消费事件。
- 实现 steer、interrupt、cancel、usage 和明确终态。
- 使用内存 Store 与 fixture Backend 完成 conformance 测试。

### 删除/替代

- `codewhale exec` 切换到 `AgentRuntime` 的同一切片，删除它原有的 `spawn_engine`/TUI Engine
  生产入口，不保留第二套 Headless loop。
- 不新增第二个 runtime store、event enum 或 completion 判定器。

### 退出门槛

- Runtime 不依赖 TUI、HTTP 或具体数据库。
- 能在真实仓库完成一组基础 DeepSeek 编码任务。
- `codewhale exec` 只有一条生产 Agent loop，并能重放同一 `RuntimeEvent` 序列。
- 没有引入新的通用 Provider SDK。

## 8. M4：工具、状态和入口统一

### 工作

- 将真实工具逐步迁入 `crates/tools`。
- 统一 `ToolOutcome`，区分调用成功、操作成功、验证成功。
- 扩展现有 `crates/state` 实现唯一 SQLite append-only `RunStore`、snapshot、replay 和 artifact，
  不新建并行数据库真相。
- 先迁移 Headless CLI，再迁移 app-server，最后迁移 TUI。
- 三个入口只使用同一个 `AgentRuntime`、`RuntimeEvent` 和 `RunStore`，统一 steer、resume、
  request_user_input、compaction 与 completion 事件。

### 删除/替代

- Headless 切换时删除假的 `crates/core::Runtime::handle_prompt` 与残留旧 exec 路径。
- app-server 切换时删除启动 TUI 子进程的 bridge、`RuntimeBridge`、`monitor_turn` 和
  `RuntimeThreadStore` 等事件/状态翻译层。
- TUI 切换时删除 TUI 内生产 turn loop，只保留交互和 `RuntimeEvent` 投影。
- 每个入口切换时同步删除对应 runtime/session/task/fleet/lane 重复 JSON/JSONL 写入。

### 退出门槛

- CLI/TUI/API 对同一 fixture 产生相同事件和终态。
- 内存 Store 与 SQLite Store 重放一致。
- crash/resume 和 exactly-once completion 通过。
- 不存在第二个生产 loop、可写 Store 或入口私有 completion 语义。

## 9. M5：RepoGraph 与证据化完成

### 工作

- 用 tree-sitter、ripgrep、LSP、包依赖和 git diff 建立增量 RepoGraph。
- 在统一 Runtime 上实现 `ContextBroker`，按任务相关性、证据新鲜度和 Token 预算选择上下文。
- 重构 compaction，保留 TaskContract、未决问题、当前 diff 和最新证据。
- 引入 `workspace_revision` 和 `EvidenceReceipt`。
- 提供可执行任意项目命令的显式 verify 入口，不写命令字符串猜测器。
- 让 `ToolOutcome`、证据失效和 completion 判定都经过同一 Runtime/RunStore；先完成这条
  单 Agent 证据链，再让多 Agent 复用，避免把不可靠终态并行放大。

### 退出门槛

- RepoGraph 相比当前 project map 提高成功率或减少 Token。
- 写操作会使旧证据失效。
- false-success 显著下降。

## 10. M6：统一多 Agent

多 Agent 是必须保留的产品能力。本阶段不是删除智能体，而是删除多套重复内核和产品外壳。

### 工作

- 让根 Agent 与每个子 Agent 都运行相同 `AgentRuntime`、发出相同 `RuntimeEvent`、写入同一
  `RunStore` 契约；差异只来自 TaskContract、预算、权限和 workspace。
- 建立唯一 `Orchestrator`，只负责 TaskGraph、预算、并发、mailbox、follow-up、wait、
  interrupt 和结果汇聚，不拥有第二套模型/工具循环。
- 建立 `AgentTask/AgentOutcome`。
- 建立 worktree create、diff、review、verify、merge、conflict、cleanup。
- 模型侧只保留一个 `agent` 工具入口；生命周期与调度语义是其参数/事件，不扩张为多组工具。
- 将现有 SubAgent、Workflow DAG、Fleet ledger/lease 和 Lane worktree 中经过测试的能力逐项
  迁入 `Orchestrator`，每迁完一个生产调用方就删除对应旧入口和重复状态写入。

### 删除/替代

- 子 Agent 切换到 `AgentRuntime` 时删除第二套 `run_subagent` 循环。
- 能力迁入并有回归测试后，删除 Workflow/Fleet/Lane 重复用户概念、scheduler 和状态真相。
- 删除不再承载独有能力的 `workflow-js`，不删除已迁入统一 Orchestrator 的智能体能力。

### 退出门槛

- 根/子 Agent通过同一 conformance suite。
- 写 Agent 不共享 cwd。
- AgentOutcome 包含证据、文件、检查、未解决项和 usage。
- worktree 的 create、diff、review、verify、merge/conflict 和 cleanup 生命周期可恢复。
- 固定任务 A/B 证明适合并行的任务相对单 Agent 在 verified success、时间、Token、成本和
  冲突率综合后有可测净收益；无收益时自动退回单 Agent。

## 11. M7：专项调优与外围清理

### 调优

- `apply_patch/search-replace/FIM` A/B；
- thinking、上下文预算和压缩策略；
- stable prefix/cache；
- 并行只读工具；
- Agent 数量和预算；
- 只有基准证明需要时才加入 embedding。

### 剩余清理

- 其他 Provider、模型目录、定价和别名；
- 旧 updater、release 和品牌耦合；
- 当前仍被 remote-setup 引用的 Telegram、Feishu 和 bridge-core；
- 腾讯云/CNB 等云部署；
- 当前 Rust remote-setup 调用面；
- 遗留 evidence 和最终不再需要的导入资产；
- 无接线 stub、兼容别名和永久临时适配层。

清理必须先通过依赖盘点；Cargo 核心能力不得因外围删除而退化。

## 12. M8：V1 产品化

- 正式产品名、二进制名、配置目录和 User-Agent；
- 自己的 origin/upstream 远程策略；
- DeepSeek-only 配置向导；
- 本地开发、安装、卸载和数据迁移；
- 精确 Rust toolchain；
- 自己的 CI、版本、changelog 和发布流程；
- 架构依赖门禁和长期 benchmark。

## 13. 当前源码迁移表

| 当前实现 | 目标归属 | 替代后删除 |
|---|---|---|
| `crates/tui/src/core/engine/*` | `runtime` | TUI 生产循环 |
| `client.rs`、`client/chat.rs` | `deepseek` | 通用 Provider/DeepSeek 混合 client |
| `project_context`、`working_set`、`compaction` | `context` | 浅层 project map 和重复投影 |
| `tui/src/tools/*` | `tools` | TUI 工具业务逻辑 |
| `runtime_threads`、各 JSON store、Fleet ledger | `state` | 多状态真相 |
| `subagent`、Workflow、Fleet、Lane | `orchestrator` | 第二循环和重复产品外壳 |
| `app-server` bridge | `app + app-server` | TUI 子进程桥 |
| `crates/core` 脚手架 | 真实 runtime/application 职责 | 假 `handle_prompt` 路径 |

## 14. 调整机制

里程碑结束时只允许三种结论：

- **保留**：真实评测有净收益，复杂度合理；
- **重做/缩小**：方向有价值，实现或默认策略不合理；
- **删除/推迟**：没有收益、明显负优化或不属于产品范围。

改变固定架构边界必须新增 ADR，说明问题、证据、替代方案、迁移和删除影响。
不能用临时开发困难作为恢复多 Provider、多 Runtime 或多状态真相的理由。
