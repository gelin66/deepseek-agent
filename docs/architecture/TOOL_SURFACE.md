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
| `apply_patch` | 全量预检 unified diff 或 `changes` 完整内容后修改工作区文件；逐文件原子发布，多文件普通失败回滚但跨文件 crash 非事务。 |
| `edit_file` | 对已读取且 byte-digest 仍 fresh 的单个文件执行一次唯一搜索替换。 |
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
判定调用是否合法，不能靠当前进程猜测或 TUI 私有缓存。State 不重复持久化 provider 派生
的 surface/fallback reason；唯一 DeepSeek planner 从重开的 exact `ModelRequestPrepared`
确定性重建完整 `RequestPlan`，`execution_fingerprint` 则绑定当时的 `strict_tools` policy，
配置漂移会在恢复边界 fail closed。

## 4. DeepSeek 协议语义

- 普通 Chat 与普通工具调用走官方标准 Chat surface。
- 只有整份请求目录都满足 strict schema 时，才使用 DeepSeek Beta Strict Function
  Calling；任一工具不兼容就整目录回退到普通工具调用，不能静默丢工具。
- M7-B 冻结的六个默认可执行 actor 目录在 Strict 候选下均仍选择 Standard Chat：root、
  coordinator 和普通 read-only child 首先受 `agent` required 语义阻断，depth-limit child
  受 `file_search` required 阻断，isolated Writer 受 `apply_patch.oneOf` 阻断。当前没有
  production Strict treatment surface，也没有用户 Strict 开关。
- fallback 必须保留工具数量、名称、顺序和完整 schema；不得通过删除 required/oneOf、
  nullable/sentinel 转换、第二份 wire catalog 或工具裁剪进入 Beta。
- Beta FIM 是独立的 Completions surface，不是工具目录成员。当前只有 DeepSeek request
  planner/accounting 基础，没有 production response parser、Host apply lifecycle 或
  canonical caller，因此没有 FIM 编辑工具或产品模式。
- 工具目录和稳定提示词前缀会影响 DeepSeek context cache，因此不增加无收益别名或
  每轮漂移的描述。
- `PromptCacheControl` 是 Host 的 block provenance，不是 DeepSeek wire 字段。当前 planner
  把 stable/volatile blocks 合成一个 system message；工具目录按固定字母顺序生成。M7-F
  不通过重排/复制工具、拆多个 system message 或删除 workspace/evidence facts猜测缓存收益。

## 5. 执行与证据边界

- `ProductionToolExecutor` 只按上述 11 个固定名称直接分派，未知名称返回不可用结果。
- 修改文件、可能产生副作用的完整 Shell、测试和 verifier 按运行策略请求审批；审批前
  不产生副作用。
- `exec_shell` schema 只有 `command`、可选 `timeout_ms` 和可选 `cwd`。旧后台、TTY、
  stdin 和 wait/interact alias 不在生产目录。
- 取消、超时、稳定 `failure_code`、invocation、transport、operation、重试建议、副作用
  状态、evidence、artifact 和 workspace revision 都进入 typed `ToolOutcome`，而不是藏在
  展示文本中。失败 code 覆盖 malformed/schema/invocation rejected/missing/invalid、unknown
  tool、workspace precondition/stale read、ambiguous edit、patch parse、operation/transport、
  side-effect ambiguous 和 verifier failure。
- 失败反馈由 canonical outcome 确定性生成：中文摘要说明失败与恢复动作，英文 code/字段
  保持稳定；成功结果不改写。Runtime 不自动重复可能有副作用的调用，恢复也不重复已经
  执行的工具或已提交的 outcome。
- `edit_file` 的 prior read 绑定 exact-byte SHA-256，并在原子 replacement 前再次校验原字节；
  check 与 rename 不是一个线性化 CAS，外部不守约 writer 仍可竞争该窗口；
  overlapping search、stale、not found 与 no-op 均 fail closed，成功替换保留 permissions。
- `apply_patch` 在写盘前拒绝 duplicate/resolved duplicate target、rename、hunk count
  mismatch、no-op、ambiguous fuzzy placement、`changes` 与 patch-only controls 混用，以及
  `path` 覆盖 `/dev/null` create/delete。delete-to-null 必须清空内容；create-from-null 与
  checked create 都不覆盖已有目标。multi-file 普通失败按已应用记录恢复原字节，
  rollback 不完整显式失败；它不声称提供跨文件 crash-atomic transaction。
- canonical Runtime 中已经 Started 的 `MayWrite` 即使最终 `NotApplied`、revision 未变，也会
  推进 workspace generation；direct tools fixture 没有 Runtime generation，不能据此恢复旧
  verifier evidence。
- 顶层 direct `git apply` 和 TUI-local eval/edit loop 已删除，不能绕过 production executor。
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
cargo test -p codewhale-tools --locked m7c_
cargo test -p codewhale-deepseek --locked strict_catalog_falls_back_atomically_without_losing_tools
cargo test -p codewhale-state --test run_store --locked sqlite_replay_matches_memory_and_survives_reopen
```

调用图审计还必须证明：

1. 固定工具定义只有 `crates/tools` 一个 owner；
2. `agent` 与 `request_user_input` 只有 `crates/runtime` 一个 owner；
3. TUI 没有第二套工具 executor、Store 或 completion 判定；
4. 被删除的旧工具名在生产 Rust 源码中没有注册或 dispatch 路径。
