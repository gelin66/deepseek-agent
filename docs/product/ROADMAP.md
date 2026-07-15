# DeepSeek Agent 开发路线图

> 文档类别：产品权威。仅定义实施顺序、迁移和删除点。

- 状态：执行中
- 当前阶段：M1 原始 DeepSeek 能力基准
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

## 3. 里程碑总览

| 里程碑 | 目标 | 状态 | 主要退出门槛 |
|---|---|---|---|
| M0 | 保护基线、整理仓库、建立唯一文档真相 | 已完成 | 工作区可追溯，产品方案落库，现有 WIP 被隔离说明 |
| M1 | 建立原始 DeepSeek 能力基准 | 进行中 | 可重复测量成功率、假成功、Token、时间和成本 |
| M2 | 独立 DeepSeekBackend 与领域协议 | 待开始 | Standard/Strict/FIM fixture 和 live canary 通过 |
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

### 工作

- 建立离线 protocol fixtures、固定仓库任务和结果格式。
- 用当前 CodeWhale + DeepSeek 跑出基线。
- 将现有 WIP 拆为四个可独立验证的切片：
  1. DeepSeek 协议；
  2. Agent 可靠性；
  3. verify 模型评审实验；
  4. 配置与本地开发支持。
- 独立评估当前 `verify` 工具；它是额外模型评审，不等同于测试证据。

### 退出门槛

- 基线结果可重复。
- 每项 WIP 有保留、重做、缩小或删除结论。
- `verify` 未经评测不得默认成为完成门禁。

## 6. M2：领域协议与 DeepSeekBackend

### 工作

- 在 `protocol` 定义 Task、Run、Turn、Event、ToolOutcome、Evidence 和 TerminalState。
- 建立版本化 NDJSON 事件。
- 抽取独立 DeepSeekBackend。
- 实现 Standard Chat、Beta Strict Chat、Beta FIM。
- 统一 reasoning replay、SSE、finish reason、usage、cache、retry 和 limits。
- 建立 fixture transport 和有费用上限的 live canary。

### 删除/替代

- 冻结旧通用 client，不再增加能力。
- 新 Backend 接管全部调用后删除旧 DeepSeek 路由分支和通用 Provider 选择路径。

### 退出门槛

- 协议 fixture 覆盖正常流、畸形流、工具循环、retry 和不完整终止。
- Strict 不兼容时普通工具调用保持可用。
- usage 无重复计算。
- Backend 不依赖 TUI、工具实现或调度器。

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

- 从 TUI Engine 提取 canonical transcript、request projection 和 turn loop。
- 建立 `ModelPort`、`ContextPort`、`ToolExecutor`、`RunStore` 等小端口。
- 实现 steer、interrupt、cancel、usage 和明确终态。
- 使用内存 Store 与 fixture Backend 完成 conformance 测试。

### 退出门槛

- Runtime 不依赖 TUI、HTTP 或具体数据库。
- 能在真实仓库完成一组基础 DeepSeek 编码任务。
- 没有引入新的通用 Provider SDK。

## 8. M4：工具、状态和入口统一

### 工作

- 将真实工具逐步迁入 `crates/tools`。
- 统一 `ToolOutcome`，区分调用成功、操作成功、验证成功。
- 建立 SQLite append-only RunStore、snapshot、replay、artifact。
- 先迁移 Headless CLI，再迁移 app-server，最后迁移 TUI。
- 统一 steer、resume、request_user_input 和 compaction 事件。

### 删除/替代

- 删除假的 `crates/core::Runtime::handle_prompt` 执行路径。
- 删除 app-server 启动 TUI 子进程的 bridge。
- 删除 TUI 内生产 turn loop。
- 逐个删除 runtime/session/task/fleet/lane 重复 JSON/JSONL 状态。

### 退出门槛

- CLI/TUI/API 对同一 fixture 产生相同事件和终态。
- 内存 Store 与 SQLite Store 重放一致。
- crash/resume 和 exactly-once completion 通过。

## 9. M5：RepoGraph 与证据化完成

### 工作

- 用 tree-sitter、ripgrep、LSP、包依赖和 git diff 建立增量 RepoGraph。
- 实现任务相关性和 Token 预算排序。
- 重构 compaction，保留 TaskContract、未决问题、当前 diff 和最新证据。
- 引入 `workspace_revision` 和 `EvidenceReceipt`。
- 提供可执行任意项目命令的显式 verify 入口，不写命令字符串猜测器。

### 退出门槛

- RepoGraph 相比当前 project map 提高成功率或减少 Token。
- 写操作会使旧证据失效。
- false-success 显著下降。

## 10. M6：统一多 Agent

### 工作

- 让子 Agent 运行相同 AgentRuntime。
- 实现 TaskGraph、预算、并发、mailbox、follow-up、wait、interrupt。
- 建立 `AgentTask/AgentOutcome`。
- 建立 worktree create、diff、review、verify、merge、conflict、cleanup。
- 将 Workflow DAG、Fleet ledger/lease 和 Lane worktree 的有效思想迁入一个 Orchestrator。

### 删除/替代

- 删除第二套 `run_subagent` 循环。
- 删除 Workflow/Fleet/Lane 重复用户概念和 scheduler。
- 删除 `workflow-js`。

### 退出门槛

- 根/子 Agent通过同一 conformance suite。
- 写 Agent 不共享 cwd。
- AgentOutcome 包含证据、文件、检查、未解决项和 usage。
- 适合并行的任务相对单 Agent 有可测净收益。

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
