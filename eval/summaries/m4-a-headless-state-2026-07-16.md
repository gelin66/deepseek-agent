# M4-A Headless 状态真相证据汇总

> 日期：2026-07-16
> 状态：实现与最终离线门禁已通过；可重建源码提交与正式 v2 canary 待完成
> 范围：仅 `codewhale exec`，不代表整个 M4 完成

## 1. 结论

M4-A 已经在真实生产 `exec` 路径实现并验证以下行为：

- 11 个固定生产工具统一返回 canonical `ToolOutcome`；
- `crates/state::StateStore` 是 `exec` 唯一生产 SQLite `RunStore`；
- canonical event log 可重放 transcript、usage/accounting、工具 outcome/artifact 与终态；
- 同一 run 具备单调 sequence、幂等 event id、lease epoch fencing 和唯一持久终态；
- 模型/工具外部操作的危险在途状态 fail closed，不自动重复请求或副作用；
- 真实 DeepSeek 编码 run 在安全持久点被 `SIGKILL` 后，可重开同一 run 完成；
- 已完成 run 可在没有 API Key 时重放，且不追加事件、不修改工作区或账本。

这还不是严格的最终完成签字。候选仍来自 dirty worktree，虽然已有 release 二进制行为
证据，但还没有一个能从 Git 直接 checkout/rebuild 的候选提交，也尚未用该提交自构建的
二进制运行正式 v2 recovery record。按照
[`EVALUATION.md`](../../docs/product/EVALUATION.md) 的代码提交记录要求，在冻结该提交之前，
本汇总不得把 M4-A 写成完全可复现的已完成里程碑，也不得启动 M4-B 来掩盖该缺口。

## 2. 范围与非目标

本切片只迁移 Headless `exec`：

```text
codewhale exec
  -> one AgentRuntime
  -> DeepSeekModelPort + fixed ProductionToolExecutor
  -> one SQLite RunStore
  -> canonical RuntimeEvent projection
```

本切片没有迁移 TUI/app-server，没有实现多 Agent worktree，没有扩大模型可见工具目录，
没有清理 Provider，也没有引入第二个 Runtime、Store、终态判定器或兼容桥。

## 3. 候选身份

| 项目 | 值 |
|---|---|
| Git HEAD | `54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3` |
| operator revision label | `worktree-head-54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3-source-d07423e03ff8ae5349e1323542a8df8ce4f6078c4f9fec0ac68a9017ee6b234f` |
| dispatcher SHA-256 | `6b21f04beeb6ab99c2fb09e3c79ae43f0be887cbd296106aa5b6c2763d70be92` |
| runtime SHA-256 | `20c45444167b573c1c44bc7f3afdb15676b41b2a34f0a4952d5edccfb557cec4` |
| binary-pair SHA-256 | `sha256:189852b559828b3fa0ef2de601e6e22b1e5a9752e0ad0bece66a3d4fb0b895e2` |

release 候选由离线、locked 的 `cargo build --release` 同时构建 `codewhale-cli/codewhale`
与 `codewhale-tui/codewhale-tui`。上述 revision label 是对 dirty source snapshot 的
operator attestation，不是 Git commit；二进制摘要证明被测行为身份，但不能单独证明源码
可 checkout/rebuild。

## 4. 实现与旧路径删除

- canonical event schema 为 v3；`ToolOutcome` 分离 invocation、transport、operation、
  side effect、retry、evidence、artifact 和 workspace revision。
- 固定生产目录为 `apply_patch`、`edit_file`、`exec_shell`、`file_search`、`git_diff`、
  `git_status`、`grep_files`、`list_dir`、`read_file`、`run_tests`、`run_verifiers`。
- `ToolEvidenceStatus::Produced` 只表示产物存在，不等于 M5 的 Host `Verified` receipt。
- State schema 为 v6；`agent_run_events` 是 append-only 语义真相，snapshot、lease 和快速
  projection 是可重建派生状态。
- 生产 `exec` 使用 `StateStore::open(None)`；`InMemoryRunStore` 只保留给 conformance/test。
- 旧领域 `ToolResult`、生产内存 Store、旧 `exec` Engine 入口、重复状态写入和旧 resume
  参数已从该调用方删除。`ContentBlock::ToolResult` 与 NDJSON `tool_result` 是稳定 wire
  词汇，不是旧领域兼容层。
- canonical terminal 先提交到 Store，再投影 stdout/NDJSON；“exactly once”只指每个 run
  恰好一个持久 terminal event，机器输出是可重放的至少一次投影。

## 5. 离线验证

当前待冻结源码在最后一次 Rust/评测 supervisor 变更后通过以下门禁：

| 门禁 | 结果 |
|---|---:|
| protocol 单元测试 | 48 + 13 passed |
| Runtime conformance | 28 passed |
| State parity/lifecycle | 13 + 6 passed |
| 真实子进程 crash 矩阵 | 5 passed；1 个 helper ignored |
| SQLite RunStore | 11 passed |
| production `exec` acceptance | 22 passed |
| TUI crate 回归 | 6,888 passed；3 ignored |
| `cargo fmt --all -- --check` | passed |
| workspace all-target Clippy `-D warnings` | passed |
| `cargo test --workspace --locked --offline` | passed |
| Python resume supervisor self-test | passed |
| `git diff --check` | passed |

完整 workspace 必须在允许本机 loopback、进程树、PTY 和 macOS 系统能力的正常执行环境中
运行。受限沙箱曾让 92 个测试以 `Operation not permitted` 同时失败；相同源码在正常环境
通过，因此该次不计为产品失败。旧 TUI 的 `permission_postures_persist_across_restart`
曾在一次早期全仓运行中偶发失败，随后定向重复 30/30、完整 TUI `6,888/6,888` 和最终
workspace 全部通过。本切片没有因此改造未迁移 TUI；其全局 settings-path 测试隔离风险
保留为 M4-C 迁移前的旧路径风险。

`exec_terminal_acceptance` 的 raw SSE fixture 曾在全仓负载下因未读取完整 POST body 就关闭
socket 而触发 macOS TCP reset。fixture 已改为先消费完整 request body，并使用合法 SSE
keepalive；两个相关 stall 用例重复 12/12 后，完整 production acceptance 为 22/22。

进程故障矩阵覆盖：

| 中断窗口 | 恢复契约 |
|---|---|
| 模型请求已在途 | fail closed；不重发；未知账单不写成 0 |
| 工具副作用后、outcome 提交前 | fail closed；副作用计数不增加 |
| 模型响应已提交、sink 未收到 | 从持久响应继续；请求、usage、assistant entry 不重复 |
| terminal 已提交、sink 未收到 | 重放同一 terminal；terminal count 保持 1 |
| commit 成功、调用方未收到返回 | 同 event id 重试返回原事件，不追加第二份 |

## 6. 真实 DeepSeek fresh-run A/B

正式的 5 次/cell single-lane A/B：

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-5c9a4160fbf14a15afd4b45a84887103` |
| result SHA-256 | `a6a2cd0880cc992aca315b25d30b3c2fdccb25807db538b60a2254a5581f0e22` |
| verified success | 10/10 |
| false success | 0 |
| 总请求 | 49 |
| 总 Token | 252,111 |
| 总 wall time | 126,951 ms |
| 总费用 | `$0.005061179` |

每次运行使用相同任务、模型、评测提示词、工具目录与预算；prompt SHA-256 为
`sha256:461ec5858d944511f0362a3ebdabcf8ef842bc3ae954db4927829072f69d8194`，工具目录
SHA-256 为 `sha256:7f6556e7031e3847f5fcded23ede4f609d3e09fdc523ae5250d26d7fa0393c92`。

| single lane（每 cell 5 次） | M3 baseline | M4-A candidate | candidate - baseline |
|---|---:|---:|---:|
| verified success | 5/5 | 5/5 | 0 |
| false success | 0 | 0 | 0 |
| 平均 Token | 23,394.4 | 27,027.8 | +15.53% |
| 平均 API 请求 | 4.6 | 5.2 | +13.04% |
| 平均 wall time | 12,040.4 ms | 13,349.8 ms | +10.88% |
| 平均费用 | `$0.000474935` | `$0.000537301` | +13.13% |

按请求归一化后，candidate 的 Token/request 为 +2.201%，ms/request 为 -1.918%，
cost/request 为 +0.078%。聚合退化主要来自 candidate 平均多 0.6 次请求，但现有样本不能
证明这是持久化造成还是模型随机性。结论必须保持为：成功率和假成功没有退化，fresh-run
聚合效率证据负向且归因不确定；不能声称 M4-A 提升了 Token、时间或费用。

## 7. 真实 DeepSeek crash/reopen/resume

当前已完成的正式 v1 canary（历史证据，不用于最终签字）：

| 项目 | 值 |
|---|---|
| result schema | `codewhale.eval.m4a-resume-canary.v1` |
| result SHA-256 | `sha256:b4e343af5bcb75d38961773a1567d5dde7de8ccbbc8b8a479fc3ad011414f214` |
| crash phase | `durable_pre_model_io` |
| kill | external `SIGKILL`，exit `-9` |
| crash prefix | 1 event：`run_created` |
| same-run resume | passed |
| Host verifier | passed |
| false success | 0 |
| API 请求 | 4/10 |
| Token | 19,180 |
| wall time | 8,667 ms |
| 费用 | `$0.000372999` |
| final event/terminal | 529 events；1 terminal；sequence contiguous；event id unique |
| no-key terminal replay | passed；0 new events；workspace/accounting unchanged |
| product metric eligible | `false` |

监督进程只在 Store 已持久化 `run_created`（也允许已追加 `model_request_prepared`）、模型
请求尚未进入 in-flight 且 lease owner 正确时停止并杀死进程。恢复使用同一 `run_id`；完成后只修改
`ranges.py`，冻结 verifier 的 public/hidden cases、文件集合、文件模式和 immutable files
全部通过。随后移除 Key 再次执行 `--resume`，进程退出 0，事件 count/sequence/digest、工作区
摘要与稳定 terminal/accounting 全部不变。

第一次诊断 run 的 same-run resume 本身成功，但临时 supervisor 错把本次投影的
`duration_ms` 纳入持久终态相等比较，因此错误判失败。修正为比较稳定 terminal 与完整
accounting、自测通过后，临时 supervisor 的独立 validation run 通过。随后将 supervisor
落为 [`scripts/eval-deepseek-resume.py`](../../scripts/eval-deepseek-resume.py)，增加自身 SHA、
依赖 Harness SHA、候选 revision 与 binary-pair 自绑定，再执行了上表的最终正式 canary。
前两次不计为产品样本，但实际消耗仍披露：第一次为 4 请求、19,060 Token、8,593 ms、
`$0.000335138`；第二次为 6 请求、30,439 Token、12,597 ms、`$0.000528466`。三次操作
总计 14 请求、68,679 Token、29,857 ms、`$0.001236603`。

v2 supervisor 的 dirty-source preflight 另行通过：5 请求、25,684 Token、11,169 ms、
`$0.000523746`，result SHA-256 为
`c763610353f1fbc075659be1bf7e3470dffc6a1e79735ebb96383734c1ea4ecf`。它已覆盖 sequence
前缀摘要、唯一 event id、唯一 terminal、request/usage delta、工具副作用摘要、dead-owner
lease reclaim 与 no-key replay，但仍是 preflight，不能代替最终 Git-bound 正式 canary。
最终 v2 supervisor 还会从干净 Git 提交执行 locked/offline release build，并记录 commit、
tree、旧/新 lease owner、before/after/delta 副作用与二进制 pair 摘要。

当前待提交 supervisor SHA-256 为
`b9510949687e176f91643040a1f421defdd99092914dcfad8cfe843cd678597f`，其依赖的 exec
Harness SHA-256 为 `ef97a5e87b7ee76bfb76fc4321785317979de4526f660833c735c6c7a1cdbc97`；
正式 v2 会把两者写入结果。原始结果位于被忽略的 `eval/results/`，权限为 `0600`；仓库只记录
脱敏汇总与结果摘要，不保存 Key、模型正文、reasoning、工具参数、event JSON 或 stderr。

## 8. 复杂度

当前 M4-A 归属 footprint，而不是伪造的 M3→M4 精确 source diff：

| 指标 | 当前 footprint |
|---|---:|
| 生产承载 Rust 文件物理行（含内嵌测试） | 9,656 |
| 独立 integration/fault Rust 测试物理行 | 4,400 |
| M4-A 归属 Rust 文件物理行合计 | 14,056 |
| public domain struct/enum/trait | 79 |
| 固定生产工具 | 11 |
| 生产 `exec` Agent loop | 1 |
| 生产 `exec` 可写 RunStore 真相 | 1 |

依赖变化包括一个新的 workspace `runtime` crate；`state` 增加已有 workspace/平台能力所需的
`async-trait`、`runtime`、`libc`、`tokio` 与 `windows-sys` 依赖，`tui` 增加 `runtime/state`
工作区依赖。没有因为本切片新增 Provider SDK、云服务、市场或第二个数据库。

另有 832 行 Python live-resume supervisor，属于可删除/可替换的评测基础设施，不进入生产
Rust footprint。它复用现有 exec Harness、fixture 和 verifier，不实现第二套 Agent 行为。

由于没有冻结的 M3 source tree 可用于逐文件归因，这些数字只表示当前所有权 footprint，
不能写成净新增 LOC、圈复杂度改善或 M3→M4 精确差值。

## 9. 边界、残余风险与下一步

- 候选仍缺可 checkout/rebuild 的 Git commit，以及从该提交自构建二进制运行的正式 v2
  canary；两者都是 M4-A 严格签字前的硬缺口。
- fresh-run A/B 只覆盖一个 Python single-lane 任务，且效率结果负向/不确定；不能外推到
  广泛仓库成功率，也不能作为性能提升声明。
- live safe-point continuation 与 no-key replay 均为 1/1 canary，只证明链路可用，不是恢复率
  的统计估计；危险窗口由离线真实子进程矩阵覆盖。
- lease 存活判断目前使用 PID；极端 PID reuse 可能把失效 lease 暂时误判为
  `AlreadyRunning`。该风险不会产生双 writer，但可能阻止恢复，后续应评测是否需要更强
  的 process identity。
- TUI/app-server 仍有旧 loop 和私有状态；M4 总体没有完成。
- M5 的 TaskContract/EvidenceReceipt、M6 的 writer worktree 和 multi A/B 均不属于本切片。
- 源码提交冻结并重验身份后，下一垂直切片才是 M4-B app-server 单一 Runtime 迁移。
