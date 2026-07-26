# M29 local release-candidate workflow acceptance

- 日期：2026-07-27
- 基线：M28 clean checkpoint `548afbe3e`
- 冻结合同：`cf34589fb`
- 合同 source tree：`c22c87c2bbfaaf0d825b79af5e8cd91ea0c3baf8`
- 结论：`keep_current_workflow_no_reproducible_blocker`
- production delta：0
- Key / official API / external network / GitHub / push / release：
  `0 / 0 / 0 / 0 / 0 / 0`

## 问题与证据边界

M24、M26、M27、M28 分别证明 delivery、canonical Run presentation、permission 和
terminal surface 的局部合同，但不能自动推出一条真实用户工作流完整可用。M29 因此没有
新增 Harness，而是把 8 个冻结 workflow 映射到现有唯一 production composition 和各层
canonical owner，使用 loopback ChatCompletions、真实临时 Git repository、deterministic
verifier、SQLite reopen 和真实 PTY 重新执行。

production fix 只有在同一 owner/cause 跨两个独立 execution 重复，或确定性 fault
injection 击穿 false-success、latest-revision、permission、Writer isolation 或
exactly-once 不变量时才准入。矩阵没有产生这样的 defect，因此没有修改 production、
增加 probe、复制 evaluator 或保留临时 runner。

## 冻结矩阵结果

| workflow | canonical evidence | result |
|---|---|---|
| long root task | production loopback 多轮 tool/verifier recovery、唯一 Host terminal | pass |
| verifier recovery | failed receipt -> rejection -> effective write -> latest-revision receipt | pass |
| approval deny | durable interaction + English/zh-Hans PTY denial；start/side effect 0 | pass |
| approval approve/reopen | interaction requested/resolved SIGKILL windows；start/outcome/side effect once | pass |
| read-only child | fixed Flash/high audit、read-only catalog、handoff-before-root-request | pass |
| explicit Writer | one Runtime、真实 Git worktree、seal/verify/integrate/cleanup once | pass |
| Store/process reopen | event prefix、RequestPlan、accounting、receipt、terminal exact；no resend | pass |
| terminal workflow | typing/paste/resize/mouse/approval/resume、CLI/HTTP/stdio parity | pass |

矩阵的关键 deterministic suites：

- `dse-runtime` conformance：85/85；
- Writer orchestration：25/25；
- `dse-app` production/application：57 passed，1 个 external helper ignored；
- `dse-state` process SIGKILL：38 passed，1 个 helper ignored；
- `dse-orchestrator` 真实 Git/worktree：44/44；
- app-server external process：3 passed，1 个 helper ignored；
- canonical TUI PTY：7/7；
- canonical TUI Run：18/18；
- bilingual/mouse/paste/resize QA PTY：15/15；
- release runtime QA：5 passed，1 个预注册 heavy storm ignored；
- CLI/HTTP/stdio surface parity：2/2。

focused 和完整 workspace 又从全新 test temp identity 重跑上述核心路径，因此没有把单次
成功或选择性 rerun 当作结论。没有失败 execution、mate、rerun 或隐藏结果。

## 权限、路由与完成不变量

- root 保持 `deepseek-v4-pro/high`；
- 普通 read-only child 保持 `deepseek-v4-flash/high`；
- explicit Writer 保持 Pro/high；
- typed recheck/recovery 保持 Pro/max；
- `Ask/Agent/FullAccess` 与 explicit deny/hard invariant 没有变化；
- child 不能替代 root completion；
- Writer 只写 isolated worktree；
- failed/stale/unapproved invocation 不产生副作用；
- Host 只接受 latest workspace revision 的 deterministic receipt；
- reopen 后没有第二 request、第二 side effect 或第二 terminal。

`false_success=0` 只属于本次 deterministic loopback/fault-injection matrix，不外推为
official DeepSeek coding quality。

## Exact-source local delivery

当前 clean contract commit 以 Rust 1.97.0、`--locked --offline`、
`CARGO_INCREMENTAL=0` 和 `/private/tmp/dse-m29-target` 构建：

```text
dse-0.8.68-aarch64-apple-darwin-cf34589fb50b.tar.gz
SHA-256 3470f51c2f37fdcb8469d19f75b091149fe7222fc86923a82d3e517d06984eaf
bytes   15881759
```

artifact 安装并验证为 `0.8.68 (cf34589fb50b)`；`dse`、`dse-tui`、English/`zh-Hans`
help smoke 均通过。uninstall 删除程序并保留隔离 `DSE_HOME`。既有 delivery
tamper/upgrade/rollback/data-preservation self-test 同样通过。

## 全部门禁

本地通过：

```text
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -I -B scripts/check-public-repository.py
./scripts/test-dse-delivery.sh
exact-source package / install / verify / bilingual smoke / uninstall
git diff --check
```

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、`CARGO_NET_OFFLINE=true` 和独立
`/private/tmp/dse-m29-target`。

## 结论、删除与非结论

结论为 `keep_current_workflow_no_reproducible_blocker`。M29 只新增冻结合同、权威结果和
本 summary；production code、RuntimeEvent、Run API、State schema、model、prompt、
tools、permission、route、TUI semantics 和 delivery owner delta 均为 0。

没有 M29-only production treatment、probe、fixture、runner 或 old path 需要保留。临时
Cargo target、artifact、install prefix、DSE_HOME、PTY/SQLite/Git fixture 都在 Goal 完成前
精确删除。

本结论不证明 official DeepSeek verified success、Token、cache、费用、模型 wall-time、
真实 macOS terminal host 差异或远端 CI/公开发布。下一项能力开发仍需要新的、重复且可归因
的 current production loss；不得从本次 regression 通过制造新功能。
