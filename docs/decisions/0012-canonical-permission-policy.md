# ADR-0012：唯一类型化权限策略与 Codex 式选择器

- 状态：已接受
- 日期：2026-07-26

## 真实问题

当前权限链只有 `auto_approve: bool`：

```text
Config.approval_policy
  -> TUI ApprovalMode::Ask / AutoApprove
  -> RunProductControls.auto_approve
  -> RunEnvironment.auto_approve
  -> ProductionToolContext.auto_approve
  -> approval_prompt Some / None
```

这条链可重放，但只能表达“所有 eligible 工具都问”或“所有 eligible 工具都不问”。它不能
表达用户真正需要的三个稳定档位：工作区内正常编码不中断、边界或风险操作才询问、明确的
完全访问。TUI 若只增加三个菜单项，会让界面承诺底层没有的语义。

仓库还存在第二组更丰富的 `dse-execpolicy` 类型，但当前 production 只消费 TUI 本地解析
出的 Shell allow/deny 快照；CLI `execpolicy check` 与真实 Agent 工具执行不是同一决策链。
不能把两套策略直接并列保留。

## 决策

DSE 建立一个冻结到每个 Run、由 Host 强制、可重放的类型化权限策略。TUI、CLI、API 和
配置只负责选择或提交策略；最终工具授权由 `crates/tools` 在副作用前决定，Runtime 只
持久化并执行该决定。

### 1. Canonical policy

`crates/protocol` 定义唯一 durable contract：

```rust
enum RunPermissionMode {
    Ask,
    Agent,
    FullAccess,
}
```

字段名可在实现前按现有协议风格微调，但产品只保留这三个 variant；不得退回 bool、
自由字符串、TUI 私有模式或任意字段组合。显式 deny 和 Host 不可绕过的安全不变量始终
优先；模型的 intent 或自报风险只能用于说明，不参与放行。

工具侧返回唯一类型化结果：

```rust
ToolAuthorizationDecision::Allow
ToolAuthorizationDecision::Ask(ToolApprovalPrompt)
ToolAuthorizationDecision::Deny(ToolPermissionRejection)
```

该决定必须绑定 exact tool name、arguments digest、workspace revision、匹配规则和风险。
批准只授权这一次 exact invocation，不建立隐式 session allow-list，不扩大后续命令、
路径或网络范围。

### 2. 三个产品档位

界面只提供以下三行：

| 中文 | English | 工作区常规读写/验证 | 外部文件/网络 | Host 判定高风险 |
|---|---|---:|---:|---:|
| 请求批准 | Ask for approval | Allow | Ask | Ask |
| 替我审批 | Agent decides | Allow | Allow | Ask |
| 完全访问权限 | Full access | Allow | Allow | Allow |

“替我审批”中的风险判断必须来自 Host 的确定性工具分类、路径解析、Shell safety 和显式
execpolicy 规则，不允许模型自己批准自己的调用。“完全访问权限”不再弹动态审批，但仍受
操作系统权限、显式 deny 和不可绕过安全不变量约束，界面不得宣称能越过这些边界。

请求批准的产品含义是：工作区内的常规读写、diff、测试和 verifier 不逐次打断；访问工作区
外文件、网络或高风险操作才询问。它不是当前“每次编辑都问”的旧 `Ask`。

### 3. 一次性、最小授权

`Ask` 后的批准只能为当前 invocation 生成一次性 scoped execution authority：

- 外部文件批准只放行本次已解析的 canonical path；
- 网络批准只放行本次 invocation 的确定目标或受控进程；
- high-risk 批准不自动授予任意外部路径或后续 Shell 前缀；
- arguments、workspace revision 或 execution fingerprint 变化后必须重新决策；
- 无法证明副作用尚未发生时不得自动重试或扩大 sandbox。

若当前 macOS/Linux backend 无法对某类外部或网络访问实施 scoped authority，该类调用在
`Ask` 下必须 fail closed，不能用全局 `danger-full-access` 冒充一次性批准。实现者必须先
证明执行层可约束，再开放对应文案和路径。

### 4. 唯一 owner 与现有 execpolicy 收敛

| 事实 | 唯一 owner |
|---|---|
| durable policy DTO、authorization event DTO | `crates/protocol` |
| allow/ask/deny 规则解析与匹配 | `crates/execpolicy` |
| invocation 分类、规则合并、最终 authorization decision、scoped authority | `crates/tools` |
| prepared/approval/started/outcome 的顺序与 durable interaction | `crates/runtime` |
| root policy composition、resume fingerprint、Run API | `crates/app` |
| 选择器、状态文案、鼠标/键盘投影 | `crates/tui` |
| exact policy 与 authorization replay | `crates/state` / `RunStore` |

`dse-execpolicy` 的规则匹配进入 production 工具链；TUI 本地
`ProductionExecPolicySnapshot`、重复 allow/deny matcher 和只服务诊断的平行 decision
语义在 cutover 时删除。`dse execpolicy check` 与真实 production 使用同一规则引擎，
但最终工具风险分类仍由 `crates/tools` 拥有。

### 5. 选择与持久化

不新增 `[permissions]`、Custom 档位、自由组合字段或 TUI 规则编辑器。交互 TUI 默认
`Ask`；用户选择只改变当前进程后续新 Run 的 mode，不静默写入全局配置。`FullAccess`
必须由用户在本次会话显式选择。

CLI 只通过已有明确入口映射三个 mode：

- 普通交互 TUI：`Ask`；
- `dse exec --auto`：`Agent`；非交互遇到需要 Ask 的高风险 invocation 时 fail closed；
- `--yolo` 或 TUI 明确选择完全访问：`FullAccess`。

旧 `approval_policy = "on-request|auto"`、`auto_approve`、`trust_mode`、
`allow_sandbox_elevation` 和由用户字符串控制的 `sandbox_mode` 在完成 cutover 后不再作为
并列产品真相。DSE 尚未公开发布，因此不保留 alias、双读、双写或隐藏 Custom；失去 owner
的配置键给出明确删除提示后物理删除。

已有 `execpolicy.toml` 只保留显式 allow/deny 规则职责，不升级成第四种模式或通用权限
语言。若 richer ask-rule 类型没有 production 必要性，应在 caller 审计后删除，而不是借
M27 扩大配置面。

### 6. TUI 交互

- header/footer 的 permission chip 可点击；
- `/permissions` 打开同一个选择器；
- `↑/↓` 移动、`Enter` 选择、`Esc` 关闭；不发明隐藏的危险模式循环；
- 鼠标点击行与键盘选择产生同一个 action；
- 选择器使用 transcript 上方的轻量 inline sheet，不遮蔽整屏上下文；
- 每行显示名称和一句真实后果，当前项用 check 标记，危险项使用现有 warning role；
- 活跃 Run 的权限是冻结事实。运行中打开选择器只展示当前策略并说明“下次运行生效”，
  不修改正在执行的 Run；
- WorkSurface 显示 active Run 的 frozen policy；空闲 header 显示下一 Run 的选择；
- English 与 `zh-Hans` 必须同一 message catalog 完整覆盖。

### 7. 版本与恢复

本切片按实际 wire/state 变更提升 Run API、RuntimeEvent 和 State schema。旧 Run 不能只按
`auto_approve=false/true` 两值映射，因为其真实权限还取决于同一冻结环境中的
`trust_mode`、`allow_sandbox_elevation`、`sandbox`、Shell catalog 和 execpolicy identity。
只有完整旧 tuple 能无损映射为新 policy 时才允许一次性 SQLite rewrite；无法无损重写的
pending Start 或 active Run 按现有 fail-closed retirement 规则处理。新的 Ask preset 是
有意减少 workspace 内逐次审批的产品增量，不能伪装成旧 `auto_approve=false` 的兼容别名。

迁移完成后只保留新 reader/writer；禁止兼容 reader、双写、从 UI label 反推 policy 或在
resume 时重新读取当前配置。execution fingerprint 必须包含 frozen typed policy、
execpolicy snapshot identity 和 scoped-authority schema。

## 实施顺序

1. 先把 M26 canonical Run 可读性改动形成独立 clean checkpoint。
2. 写 protocol/state migration 与旧 bool 等价性测试，不接 UI。
3. 在 `crates/tools` 建立 authorization matrix，并让 Runtime 消费唯一 typed decision。
4. 把 `dse-execpolicy` 接入同一 production caller，删除 TUI/Tools 重复 snapshot/matcher。
5. 迁移 app、exec、app-server、Writer/recheck profile 与所有真实 caller。
6. 删除旧 config/flag 派生真相和失去必要性的 richer policy surface。
7. 最后实现三行 TUI selector、mouse hitbox、`/permissions`、双语文案和 WorkSurface 投影。
8. 通过 offline loopback、crash/reopen、PTY、全仓门禁后再更新 current facts 并独立提交。

每一步都必须保持编译与单一 production path，临时 adapter 最多跨本 M27，且在最终提交前
物理删除。

## 被拒绝的替代方案

- **只把 `Ask/AutoApprove` 菜单扩成三行**：界面承诺与底层 bool 不一致，是假功能。
- **把 `trust_mode`、sandbox、auto-approve、execpolicy 拼成 UI preset 后继续分别持久化**：
  组合会漂移，replay 不能说明当时真正授权。
- **由模型判断自己的调用是否安全**：存在自我授权和 prompt injection 闭环。
- **批准一次后记住 Shell 前缀或整个会话**：扩大用户一次性意图，且 crash/reopen 难以精确。
- **直接复制 Codex 配置结构或代码**：DSE 只吸收成熟交互问题，不引入第二 Runtime 或兼容层。
- **保留旧 bool 作为 fallback**：形成永久双路径和历史债。

## 后果

- 用户能在不中断普通编码的前提下选择可理解的安全/自主性平衡；
- permission UI、RunStore replay、Tool execution 和 config audit 对同一事实闭合；
- 现有 durable approval、deny、side-effect truth 和 recovery 继续复用；
- M27 是 Host/本地行为切片，不改变 DeepSeek prompt、模型、Thinking、Token 或 API surface，
  不需要付费模型请求；
- 若 scoped external/network authority 无法在现有 backend 被真实强制，对应能力必须缩小或
  保持 blocked，不能用文案或全局 full access 假装完成。
