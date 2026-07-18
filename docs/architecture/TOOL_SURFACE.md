# Canonical tool surface

> 文档类别：当前能力事实。目标架构以
> [`PRODUCT_PLAN.md`](../product/PRODUCT_PLAN.md) 为准；源码 owner 是
> `crates/tools/src/production.rs` 与 `crates/runtime/src/agent.rs`。

## 1. 设计原则

- 一个工具只有进入 `AgentRuntime::tool_definitions`、可由同一个
  `ProductionToolExecutor` 执行并产生 canonical `ToolOutcome`，才算生产能力。
- 专用工具必须比 `exec_shell` 提供更稳定的 schema、边界或证据；不保留同义别名。
- TUI 不拥有第二份 registry、handler 或工具状态；CLI、TUI 和 app-server 使用同一目录。
- 工具名和 JSON 字段保持英文稳定；给 DeepSeek 的描述、错误和用户界面使用简体中文。
- 旧 transcript 名称不做兼容注册。被替换的工具在切换后物理删除。

## 2. 固定生产目录

`crates/tools` 按名称排序并固定公开以下 11 个代码工具：

| 工具 | 当前职责 |
|---|---|
| `apply_patch` | 用 unified diff 或完整文件内容原子修改一个或多个工作区文件。 |
| `edit_file` | 对已读取的单个文件执行一次精确搜索替换。 |
| `exec_shell` | 在工作区同步执行一条有界命令，返回退出状态和输出。 |
| `file_search` | 按文件名或路径片段模糊查找工作区文件。 |
| `git_diff` | 读取未提交或已暂存的 Git 差异。 |
| `git_status` | 读取分支和工作区文件状态。 |
| `grep_files` | 用正则搜索工作区文本并返回结构化匹配。 |
| `list_dir` | 列出工作区内指定目录的直接子项。 |
| `read_file` | 读取 UTF-8 文本、PDF 或可由本地后端 OCR 的图片。 |
| `run_tests` | 在工作区运行 `cargo test` 并返回确定性结果。 |
| `run_verifiers` | 按项目类型运行确定性验证，产生 verifier evidence/artifact。 |

固定目录没有 `write_file`、`web_search`、`update_plan`、`work_update`、`todo_*`、
`checklist_*`、`handle_read`、后台 shell、GitHub、自动化、脚本插件或通用代码执行工具。
它们不是隐藏能力，也不会为旧 transcript 保留 alias。

## 3. Runtime 内建工具

`AgentRuntime` 在固定目录上按运行条件追加两个内建工具：

| 工具 | 出现条件 | owner |
|---|---|---|
| `agent` | 工具开启、策略允许且当前深度小于 `max_depth` | `crates/runtime` |
| `request_user_input` | 交互式 root 运行且策略允许 | `crates/runtime` |

`agent` 启动的 child 与 root 使用同一个 `AgentRuntime`。它不是第二套子 Agent
runtime。`request_user_input` 通过 canonical interaction、RuntimeEvent 和 RunStore
完成请求与响应，TUI 只投影和提交用户选择。

每次模型请求实际 advertised 的完整工具目录会持久化到 RunStore。恢复时按这份目录
判定调用是否合法，不能靠当前进程猜测或 TUI 私有缓存。

## 4. DeepSeek 协议语义

- 普通 Chat 与普通工具调用走官方标准 Chat surface。
- 只有整份请求目录都满足 strict schema 时，才使用 DeepSeek Beta Strict Function
  Calling；任一工具不兼容就整目录回退到普通工具调用，不能静默丢工具。
- Beta FIM 是独立的 Completions surface，不是工具目录成员。
- 工具目录和稳定提示词前缀会影响 DeepSeek context cache，因此不增加无收益别名或
  每轮漂移的描述。

## 5. 执行与证据边界

- `ProductionToolExecutor` 只按上述 11 个固定名称直接分派，未知名称返回不可用结果。
- 修改文件、可能产生副作用的完整 Shell、测试和 verifier 按运行策略请求审批；审批前
  不产生副作用。
- `exec_shell` schema 只有 `command`、可选 `timeout_ms` 和可选 `cwd`。旧后台、TTY、
  stdin 和 wait/interact alias 不在生产目录。
- 取消、超时、操作状态、重试建议、副作用状态、evidence 和 artifact 都进入 typed
  `ToolOutcome`，而不是藏在展示文本中。
- `run_verifiers` 是当前确定性验证入口。模型自评不是确定性证据，也不能单独令 Host
  接受完成。

## 6. MCP 当前边界

仓库仍有 MCP 配置、stdio/HTTP transport、OAuth、发现和 CLI 管理代码，但当前 canonical
`AgentRuntime` 的 model-visible 目录只来自固定 11 工具和两个条件内建工具。MCP 发现结果
尚未接入这条唯一执行链，因此不能把 MCP tool 当作已经可由当前 Agent 调用的能力。

如果后续保留 MCP，必须作为同一 `ToolExecutor`/`ToolOutcome`/RunStore 契约下的可测垂直
切片接入，不能恢复 TUI 私有模型循环或第二套 registry。

## 7. 真实性门禁

```bash
cargo test -p codewhale-tools --locked catalog_is_exact_chinese_and_description_free
cargo test -p codewhale-deepseek --locked strict_catalog_falls_back_atomically_without_losing_tools
cargo test -p codewhale-state --test run_store --locked sqlite_replay_matches_memory_and_survives_reopen
```

调用图审计还必须证明：

1. 固定工具定义只有 `crates/tools` 一个 owner；
2. `agent` 与 `request_user_input` 只有 `crates/runtime` 一个 owner；
3. TUI 没有第二套工具 executor、Store 或 completion 判定；
4. 被删除的旧工具名在生产 Rust 源码中没有注册或 dispatch 路径。
