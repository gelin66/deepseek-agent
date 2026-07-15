# DeepSeek Agent（工作名称）

一个以 CodeWhale Rust 源码为底座、面向官方 DeepSeek API 的本地编码 Agent 产品。

目标不是继续扩展通用模型兼容，也不是把多个 Agent 项目拼接在一起；目标是形成一套
统一、可恢复、可验证、支持单 Agent 与多 Agent 的 Rust 运行时，并让 CLI、TUI 和
Headless API 共用它。

> 当前处于产品基线与架构迁移阶段。现有可执行文件、配置目录和部分文档仍使用
> `codewhale` 名称；生产 Agent loop 也仍位于 `crates/tui`。仓库不会把目标架构写成
> 已经完成的能力。

## 从这里开始

- [产品总纲](docs/product/PRODUCT_PLAN.md)
- [开发路线图](docs/product/ROADMAP.md)
- [能力评测规范](docs/product/EVALUATION.md)
- [文档入口](docs/README.md)
- [当前实现架构](docs/architecture/CURRENT_CODEWHALE.md)

这几份文档的职责不同：产品总纲定义固定方向，Roadmap 记录可调整的执行顺序，
Evaluation 决定能力是否值得保留，当前架构文档只描述尚未迁移的源码事实。

## 产品目标

```text
用户任务
  -> 精准代码上下文
  -> AgentRuntime
  -> 工具和工作区
  -> 最新验证证据
  -> 明确终态
```

最终产品只保留三个入口：

1. 交互式 CLI/TUI；
2. Headless/NDJSON 本地自动化；
3. 多 Agent 团队任务。

它们必须共享：

- 一个 DeepSeekBackend；
- 一个 AgentRuntime；
- 一个 RuntimeEvent 协议；
- 一个 RunStore；
- 一个 TaskGraph/Orchestrator；
- 一套 ToolOutcome 和 EvidenceReceipt 语义。

## 当前已有能力

CodeWhale 底座已经包含大量真实能力：

- 流式 Agent、工具调用、steer、cancel、compaction 和恢复；
- 文件读写、patch、shell、git、LSP 和测试工具；
- Skills、MCP、hooks 和本地 runtime API；
- 子 Agent、预算、mailbox、checkpoint 和 worktree 基础；
- DeepSeek reasoning replay、Strict Function Calling、FIM 和 cache 相关实现。

当前主要问题不是功能数量，而是这些能力分散在 TUI、core、subagent、Workflow、
Fleet、Lane 和多套状态系统中。开发路线会逐条迁移并删除旧路径，而不是继续叠加。

## 当前本地开发

要求 Rust 1.88 或更高版本。当前二进制名称在产品化里程碑前仍保持 CodeWhale：

```bash
rustup default stable
cargo build -p codewhale-cli -p codewhale-tui --locked
```

API Key 只放在环境或系统凭据存储中：

```bash
export DEEPSEEK_API_KEY='your-key'
```

当前 DeepSeek 配置样例：

```text
config.deepseek-agent.example.toml
```

运行当前本地入口：

```bash
cargo run -p codewhale-cli --locked --
cargo run -p codewhale-cli --locked -- exec --auto "inspect this repository"
```

Focused 检查：

```bash
./scripts/dev-deepseek-agent.sh focused
```

当前 DeepSeek WIP 已被保存，但尚未通过新的产品评测门禁。它包含协议、Agent
可靠性和独立模型评审实验，后续会按 [Roadmap](docs/product/ROADMAP.md) 分开验证。

## 开发原则

- 真实可用性优先于功能数量；
- 每次只迁移一条完整能力链；
- 新路径接管后删除旧路径；
- 只在真正会变化的边界使用小型 trait；
- 多 Agent 复用同一个 Runtime；
- 写 Agent 使用独立 worktree；
- 完成依赖最新证据，不依赖模型自报；
- 没有基准收益的功能缩小、推迟或删除；
- 不引入其他模型、TypeScript sidecar、云平台或插件市场。

## 项目状态

M0 仓库整理和文档基线已经完成，当前里程碑是 M1：建立可重复的 DeepSeek 能力基准。具体状态和下一步只
在 [ROADMAP.md](docs/product/ROADMAP.md) 更新，不再创建平行的版本 tracker 或 handoff 文件。

## 来源与许可

本项目基于 MIT 许可的 CodeWhale 源码继续开发。许可证见 [LICENSE](LICENSE)。
CodeWhale 和其他 Agent 项目提供了重要参考，但本产品独立开发，且不隶属于任何模型
提供商。
