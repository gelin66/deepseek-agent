# DSE

[English](README.md)

**DSE（DeepSeek Engineer）** 是一个 Rust 原生、本地优先、只接官方 DeepSeek API
的编码 Agent。CLI、交互 TUI 与本地 Run API 共用同一个 application service、同一个
Agent runtime、同一套工具目录和同一个 SQLite 持久真相。

> 发布状态：DSE 仍处于公开发布前的开发分支，目前没有官方公共二进制、tag 或 release。
> 请从源码构建和测试，不要把 M17 的局部检查点当成公开 V1。

## 为什么是 DSE

DSE 刻意保持边界收敛：

- 唯一模型后端是官方 DeepSeek，使用 OpenAI 格式 Chat Completions：
  `https://api.deepseek.com/chat/completions`；
- 根 Agent、只读子 Agent 与显式准入的隔离 Writer 共用一个 `AgentRuntime`；
- 一个 canonical event protocol 和一个 `RunStore` 负责精确 replay、resume、计费与
  crash recovery；
- Host-owned `TaskContract`、latest-revision `EvidenceReceipt` 与 deterministic
  verifier 共同决定完成；
- 原子、workspace-scoped 工具返回 typed outcome，以及 retry/side-effect 事实；
- `en` 与 `zh-Hans` 人类界面完整覆盖，不增加语言分类请求或翻译模型。

当前协议身份为 Run API v15、RuntimeEvent v22、State schema v28、exec-stream v6。

## 固定模型 profile

用户没有显式选择模型或 reasoning effort 时，DSE 使用确定性的 actor profile：

| Actor 或动作 | 模型 | Reasoning |
|---|---|---|
| 根 Agent | `deepseek-v4-pro` | `high` |
| 显式隔离 Writer | `deepseek-v4-pro` | `high` |
| Typed recovery、recheck 或 rework | `deepseek-v4-pro` | `max` |
| 普通隔离只读子 Agent | `deepseek-v4-flash` | `high` |

产品没有模型 Auto 模式、prompt classifier、dynamic router、fallback Provider 或额外的
routing request。用户显式选择的 Pro/Flash 与 reasoning 仍是可精确 replay 的输入。

`dse exec --auto` 是另一个 CLI 概念：它启用非交互工具 Agent loop，并使用“替我审批”
权限档位；Host 判定高风险的调用仍需批准，因此在 headless 中安全拒绝。交互 TUI 默认
“请求批准”，只有显式 process-local `--yolo` 选择“完全访问权限”。产品没有 Custom
权限模式或持久权限配置。

DeepSeek 已于 2026-07-24 下线旧的 `deepseek-chat` 与 `deepseek-reasoner` alias。
DSE 使用当前的 `deepseek-v4-pro`、`deepseek-v4-flash` 模型 ID，官方 base URL 不变。
参见 [DeepSeek V4 发布说明](https://api-docs.deepseek.com/zh-cn/news/news260424/) 与
[Chat Completions API](https://api-docs.deepseek.com/zh-cn/api/create-chat-completion)。

## 从源码构建

需要：

- Git；
- `rust-toolchain.toml` 固定的 Rust 1.97.0；
- Cargo package 所需的平台构建依赖；
- 发起真实模型请求时使用的官方 DeepSeek API Key。

构建两个发布二进制：

```bash
cargo build -p dse-cli -p dse-tui --locked
```

保存 Key 且不打印内容：

```bash
dse login
```

也可以只向需要的进程提供官方环境变量：

```bash
export DEEPSEEK_API_KEY='replace-with-your-key'
```

绝不提交 Key、`.env`、原始 Provider 流量、本地 Run state 或评测 raw。

## 使用 DSE

以英文或简体中文启动交互产品：

```bash
dse --language en
dse --language zh-Hans
```

执行单次模型回答：

```bash
dse exec "解释这个模块的所有权。"
```

执行 canonical 非交互编码 loop：

```bash
dse exec --auto "修复失败测试并验证结果。"
```

通过 stdio 启动同一个 canonical Run API：

```bash
dse app-server --stdio
```

HTTP/SSE 默认绑定 loopback，并要求认证：

```bash
export DSE_APP_SERVER_TOKEN='replace-with-a-random-token'
dse app-server --host 127.0.0.1 --port 7878
```

不要在非 loopback 接口暴露无认证的本地 API。

## 配置

canonical 用户配置位于 `~/.dse/config.toml`。用 `DSE_HOME` 覆盖 DSE 状态根目录，或用
`DSE_CONFIG_PATH` 覆盖配置文件。可从 [config.example.toml](config.example.toml)
开始；只使用环境变量时可参考 [.env.example](.env.example)。

默认 endpoint 是 `https://api.deepseek.com`，sender 向 `/chat/completions` 发送请求。
DSE 拒绝模型 Provider selector 与外部 Provider table。`DEEPSEEK_API_KEY`、
`DEEPSEEK_BASE_URL`、`DEEPSEEK_MODEL` 保持官方命名，产品设置使用 `DSE_*`。

人类界面语言在每个进程中只解析一次：

```text
--language
  -> [ui].language
  -> 首次运行双语选择
  -> fresh noninteractive 环境使用 en
```

切换 UI 语言不会翻译命令、路径、代码、diff、机器 JSON/NDJSON/HTTP/SSE、模型 ID 或
稳定错误码。production 只保留一个经证据选择的中文表达 system prompt；除非用户显式
指定，否则它要求 Agent 使用用户当前任务语言回答。

## 本地交付生命周期

从 locked source tree 构建 checksum-bound 包：

```bash
CARGO_TARGET_DIR=/private/tmp/dse-delivery-target \
  ./scripts/dse-delivery.sh package --output-dir dist
```

在显式 prefix 下安装、验证、回滚或卸载：

```bash
artifact="$(find dist -maxdepth 1 -name '*.tar.gz' -type f -print -quit)"
./scripts/dse-delivery.sh install --artifact "$artifact" --prefix "$HOME/.local"
./scripts/dse-delivery.sh verify --prefix "$HOME/.local"
./scripts/dse-delivery.sh rollback --prefix "$HOME/.local"
./scripts/dse-delivery.sh uninstall --prefix "$HOME/.local"
```

卸载器只管理所选 prefix 下的 DSE program link 与 immutable release directory，不删除
`DSE_HOME`。

## 开发

运行公共仓库与 focused production 门禁：

```bash
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  ./scripts/dev-dse.sh focused
```

集成 Rust 修改前运行：

```bash
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=/private/tmp/dse-development-target \
  cargo test --workspace --locked
```

审查契约见 [CONTRIBUTING.md](CONTRIBUTING.md)，私密漏洞报告见
[SECURITY.md](SECURITY.md)，社区行为约定见 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

## 架构与产品权威

以下文档职责不同：

1. [产品总纲](docs/product/PRODUCT_PLAN.md)——已接受范围与固定架构；
2. [架构决策](docs/decisions/)——长期已接受决策；
3. [路线图](docs/product/ROADMAP.md)——当前迁移与删除顺序；
4. [评测契约](docs/product/EVALUATION.md)——保留能力所需证据；
5. [当前架构事实](docs/architecture/CURRENT_CODEWHALE.md)——实现事实；需要准确描述历史时
   会保留旧名称。

仓库不创建翻译版平行 Roadmap 或第二产品状态 tracker。

## 明确不做

DSE 不提供：

- Anthropic Messages 或通用 Provider 生态；
- 模型 Auto router 或任务难度 classifier；
- production FIM 编辑器或第二编辑面；
- 第二 Runtime、Store、工具目录或 completion authority；
- 默认 multi-Writer、swarm、云 Agent 平台或插件市场。

唯一 Writer 路径是 explicit-only，并隔离在 Git worktree 中。普通只读子 Agent 可以按
固定 actor profile 调查；根 Agent 仍负责集成与 verified completion。

## 来源、许可与独立性

DSE 基于在 commit `352e86a611fdf3cd8bd27c36d24d482c06a71117` 导入的 MIT
许可 CodeWhale 源码继续开发。保留的许可证见 [LICENSE](LICENSE)，来源细节见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

DSE 是独立社区项目，不隶属于 DeepSeek，也未获得 DeepSeek 的赞助或背书。DeepSeek
名称、模型、API 与商标归各自权利人所有。
