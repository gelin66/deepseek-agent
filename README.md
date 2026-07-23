# CodeWhale

一个面向官方 DeepSeek API 的 Rust-native、本地优先编码 Agent。

目标不是继续扩展通用模型兼容，也不是把多个 Agent 项目拼接在一起；目标是形成一套
统一、可恢复、可验证、支持单 Agent 与多 Agent 的 Rust 运行时，并让 CLI、TUI 和
Headless API 共用它。

exec、app-server 与交互 TUI 已统一到
`AgentApplication -> AgentRuntime -> RunStore`。产品二进制固定为 `codewhale` 与
`codewhale-tui`，产品状态只写入 `~/.codewhale`（或显式 `CODEWHALE_HOME`）。

## 从这里开始

- [产品总纲](docs/product/PRODUCT_PLAN.md)
- [开发路线图](docs/product/ROADMAP.md)
- [能力评测规范](docs/product/EVALUATION.md)
- [M1 可执行评测](eval/README.md)
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

- exec、app-server 与交互 TUI 使用同一 production application/runtime/store；
- 根 Agent、只读子 Agent和隔离 Writer 使用同一个 `AgentRuntime`；
- 单 Writer 由唯一 Orchestrator 完成 worktree、diff、verify、integrate 和 cleanup；
- 固定生产工具目录、typed `ToolOutcome`、TaskContract、EvidenceReceipt 与 Host 终态；
- DeepSeek Standard Chat、reasoning/tool history exact replay、完整 stream/usage evidence；
- hard-limit 本地上下文压缩、持久恢复、approval、steer、cancel 和 canonical Run API；
- Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2。

DeepSeek Beta Strict planner 仍保留，但 M7-B 证明六个默认可执行 actor 的完整工具目录均会
无损回退 Standard Chat，因此 Strict 当前不是生产默认，也没有用户开关。Beta FIM 只有
独立 request-planning/transport 基础，没有 canonical production 编辑调用方；下一切片将
独立比较 `apply_patch`、`edit_file` 与 FIM，而不是恢复旧 `FimEditTool`。

## 本地开发与交付

仓库使用 `rust-toolchain.toml` 固定 Rust 1.97.0。源码构建：

```bash
cargo build -p codewhale-cli -p codewhale-tui --locked
```

API Key 只放在环境或系统凭据存储中：

```bash
export DEEPSEEK_API_KEY='your-key'
```

当前配置样例：

```text
config.example.toml
```

运行当前本地入口：

```bash
cargo run -p codewhale-cli --locked --
cargo run -p codewhale-cli --locked -- exec --auto "inspect this repository"
```

Focused 检查：

```bash
./scripts/dev-codewhale.sh focused
```

生成 checksum-bound 本地包（命令只使用已锁定、已缓存依赖，不访问网络）：

```bash
CARGO_TARGET_DIR=/private/tmp/codewhale-delivery-target \
  ./scripts/codewhale-delivery.sh package --output-dir dist
```

安装、验证、回滚与卸载：

```bash
artifact="$(find dist -maxdepth 1 -name '*.tar.gz' -type f -print -quit)"
./scripts/codewhale-delivery.sh install --artifact "$artifact" --prefix "$HOME/.local"
./scripts/codewhale-delivery.sh verify --prefix "$HOME/.local"
./scripts/codewhale-delivery.sh rollback --prefix "$HOME/.local"
./scripts/codewhale-delivery.sh uninstall --prefix "$HOME/.local"
```

安装器只管理指定 prefix 下的两个程序和 immutable release 目录；卸载不会读取或删除
`CODEWHALE_HOME`。完整离线生命周期自测：

```bash
./scripts/test-codewhale-delivery.sh
```

每项 DeepSeek 能力按 [Roadmap](docs/product/ROADMAP.md) 独立冻结、评测和取舍；协议 canary
不等于真实编码收益，缺少 treatment surface 时不会为了运行 A/B 而读取 Key 或调用 API。

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

M0-M7 的主要 canonical 迁移与专项评测已完成，当前里程碑是 M8 V1 产品化。具体状态和
下一步只在 [ROADMAP.md](docs/product/ROADMAP.md) 更新，不再创建平行的版本 tracker
或 handoff 文件。

## 来源与许可

本项目基于 MIT 许可的 CodeWhale 源码继续开发。许可证见 [LICENSE](LICENSE)。
CodeWhale 和其他 Agent 项目提供了重要参考，但本产品独立开发，且不隶属于任何模型
提供商。
