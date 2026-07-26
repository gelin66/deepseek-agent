# M27 canonical permission policy

- 日期：2026-07-26
- 结论：`shrink_to_enforceable_permission_subset`
- 范围：纯本地 Host/runtime 权限闭环；无 Key、official API、GitHub、push 或 release

## 真实问题

旧 production 同时存在 `auto_approve`、`trust_mode`、sandbox/elevation 字符串、TUI
私有 execpolicy snapshot/parser、tools safety matcher 和 richer diagnostic policy engine。
它们不能表达同一个可重放权限事实，也让普通 workspace work 被无意义审批打断。

## 验收与唯一 owner

- protocol：仅 `RunPermissionMode::{Ask, Agent, FullAccess}`；
- tools：对 exact invocation 作最终 allow/ask/deny；
- runtime：decision → durable interaction → start → outcome 的唯一顺序；
- state：v26 exact replay；不能无损映射的旧 tuple fail-closed retirement；
- app：Run 创建时绑定 mode 与 Host sandbox profile；
- TUI：只选择/投影三个 preset，active Run 不被 next-run 选择改写；
- execpolicy：只解析、匹配现有 TOML allow/deny。

不存在 Custom、`[permissions]`、自由组合、模型自批、第二 Runtime/Store、approval cache
或 parallel policy snapshot。

## 机制结果

| case | Ask | Agent | FullAccess |
|---|---|---|---|
| workspace read/edit/test/verifier | allow | allow | allow |
| canonical path argument / explicit shell `cwd` | fail closed | allow | allow |
| recognized network invocation | fail closed | allow unless critical | allow |
| Host critical | Ask（若同时需要不可 scoped network 则 fail closed） | Ask | allow |
| explicit deny / hard invariant | deny | deny | deny |

附加不变量：

- explicit allow 不能降级 Agent 的 Host-critical Ask；
- approval 绑定 exact arguments SHA 与 workspace state；
- approval 后 revision 变化产生 `authorization_stale`，不进入
  `ToolExecutionStarted`，tool call 与 side effect 都为 0；
- non-interactive Ask fail closed；
- persisted decision 在 reopen 后复用，不读取当前 config 重新匹配；
- Writer 强制 isolated worktree、无 external/network，不能继承父级扩权。

当前 backend 无法强制一次性 external/network authority，所以没有把 Ask 弹窗或全局
`danger-full-access` 冒充 scoped grant。typed path gate 覆盖 canonical path-bearing
tools、`apply_patch` touched files、`run_verifiers` program/cwd 和 `exec_shell.cwd`；
任意子进程内部自行打开的文件仍只受 OS sandbox 基线约束，DSE 不宣称能从 Shell 字符串
完整推导该 I/O。这一单元按可强制边界收缩。

## Cutover 删除

物理删除：

- canonical wire/state 中的 `auto_approve`、`trust_mode`、
  `allow_sandbox_elevation` 与用户 `sandbox_mode`；
- `approval_policy`、持久 `yolo`、permission config/env reader 与旧 CLI flags；
- TUI 私有 `ProductionExecPolicySnapshot`、Starlark parser/rules/decision 岛；
- tools 重复 prefix matcher；
- `dse-execpolicy` 中无 production 必要性的 ask/session/network amendment、layer、
  compatibility 与 `bash_arity`；
- Starlark、multimap 及失去消费者的依赖；
- 无写入 owner 的 `workspace-trust.json` 外部路径 reader 与 tools trusted-path
  fingerprint 字段；
- 绕开 Run API/RunStore、可自由组合 policy/network/writable roots 的
  `dse-tui sandbox run` 直接执行旁路及其专属 parser/双语文案；
- 只让 TUI 忽略规则、却不能改变 app-server 的 `[features].exec_policy` 分叉开关。

保留：

- config/protocol 对旧 key/`custom` 的显式拒绝 guards；
- State v26 retirement 的旧 tuple negative fixtures；
- frozen 历史 manifest/summary/raw；
- `[projects].trust_level` 的项目配置与 MCP onboarding owner；
- `execpolicy.toml` 的单一 TOML allow/deny职责。

## 验证

定向机制集覆盖 protocol/config/app、tools matrix、runtime 85-case conformance、
Writer 25-case conformance、State migration/process crash、CLI/TUI/app-server surface、
English/`zh-Hans` keyboard/mouse PTY 和 production loopback。最终集成门为：

```text
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -I -B scripts/check-public-repository.py
git diff --check
```

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、`CARGO_NET_OFFLINE=true` 与独立
`/private/tmp/dse-m26-m27-target`。本结论不证明 DeepSeek verified success、Token、
cache、费用或 wall-time 改善。
