# M4-A Headless 状态真相证据汇总

> 日期：2026-07-16
> 状态：严格完成
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

上述行为已绑定到可 checkout 的代码提交 `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3`：
该提交对应的最终源码通过离线门禁；正式 supervisor 又在 clean worktree 从该提交执行
locked/offline release build，并完成真实 DeepSeek v2 crash/reopen/resume canary。最终
fresh-run A/B 的 candidate cell 也绑定到同一提交和同一组 candidate release 二进制。
M4-A 因此严格完成；M4-B/M4-C 仍未开始，整个 M4 仍未完成。

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
| 被测代码提交 | `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3` |
| Git tree | `2f664616def40906c2ef62b5a3fec46a9a76c7a3` |
| 源码身份 | `git_verified_clean_source`；revision 与 HEAD 一致 |
| 构建命令 | `cargo build --release --locked --offline -p codewhale-cli -p codewhale-tui` |
| 工具链 | Cargo `1.97.0`；Rustc `1.97.0` |
| dispatcher SHA-256 | `ba7fa952f50d164cab45ae03dd68bc22c1b36c501a3f1a4eec3476d7667d30d6` |
| runtime SHA-256 | `a51f8c0301de2d97e7994aaff1eeaa42dbd05bfc49e8fc6b012c0a234d653329` |
| binary-pair SHA-256 | `sha256:f4a7065612180257882afab195c2f5674164bd3a37fbaa642fd319610aeda8ad` |

正式 v2 supervisor 自己执行构建并记录命令、退出码、Git commit/tree、clean 状态和
二进制摘要，而不是接受无法核验的 operator label。`0a5b76a` 是将此前 M2/M3/M4-A 与保留
WIP 冻结到一起的诚实 checkpoint，不冒充纯 M4-A 单切片 diff；本汇总之后的纯文档提交不
改变上述被测源码和二进制身份。

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

提交 `0a5b76a` 在最后一次 Rust/评测 supervisor 变更后通过以下门禁：

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
| locked/offline release build | passed |
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
| 模型请求已在途 | `model_request_in_flight_crash_is_not_reissued_after_reopen`：fail closed；不重发；未知账单不写成 0 |
| 工具副作用后、outcome 提交前 | `tool_side_effect_crash_is_not_executed_twice_after_reopen`：fail closed；副作用计数不增加 |
| 模型响应已提交、sink 未收到 | `committed_model_response_resumes_without_duplicate_request_usage_or_assistant`：从持久响应继续；请求、usage、assistant entry 不重复 |
| terminal 已提交、sink 未收到 | `committed_terminal_is_returned_after_reopen_without_second_terminal`：重放同一 terminal；terminal count 保持 1 |
| commit 成功、调用方未收到返回 | `caller_retry_after_reopen_returns_the_committed_event_by_id`：同 event id 重试返回原事件，不追加第二份 |

`execution_epoch_fences_stale_leases_and_reclaims_dead_pid` 证明失效 owner 可回收且旧 epoch 被
fence；`concurrent_store_instances_allow_only_one_writer` 证明存活 owner 存在时第二个
Store 实例不能同时推进同一 run。

## 6. 真实 DeepSeek fresh-run A/B

最终提交绑定的 5 次/cell single-lane A/B：

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-e5235bac28e5473ca75252cef22a104c` |
| result SHA-256 | `5692236a23f0e6223164fc6a1cdd29b727ed57e8438699c3306e3c3247fcf71b` |
| candidate revision | `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3`；Harness 记录 `git_dirty=false` |
| candidate binary pair | `sha256:f4a7065612180257882afab195c2f5674164bd3a37fbaa642fd319610aeda8ad` |
| verified success | 10/10 |
| false success | 0 |
| 总请求 | 47 |
| 总 Token | 235,669 |
| 总 wall time | 97,045 ms |
| 总费用 | `$0.004559766` |

每次运行使用相同任务、模型、评测提示词、工具目录与预算；prompt SHA-256 为
`sha256:461ec5858d944511f0362a3ebdabcf8ef842bc3ae954db4927829072f69d8194`，工具目录
SHA-256 为 `sha256:7f6556e7031e3847f5fcded23ede4f609d3e09fdc523ae5250d26d7fa0393c92`。

| single lane（每 cell 5 次） | M3 baseline | M4-A candidate | candidate - baseline |
|---|---:|---:|---:|
| verified success | 5/5 | 5/5 | 0 |
| false success | 0 | 0 | 0 |
| 平均 Token | 21,782.4 | 25,351.4 | +16.38% |
| 平均 API 请求 | 4.4 | 5.0 | +13.64% |
| 平均 wall time | 9,321.6 ms | 10,087.4 ms | +8.22% |
| 平均费用 | `$0.000433606` | `$0.000478348` | +10.32% |

按请求归一化后，candidate 的 Token/request 为 +2.419%，ms/request 为 -4.771%，
cost/request 为 -2.920%。聚合退化主要来自 candidate 平均多 0.6 次请求，但现有样本不能
证明这是持久化造成还是模型随机性。结论必须保持为：成功率和假成功没有退化，fresh-run
聚合效率证据负向且归因不确定；不能声称 M4-A 提升了 Token、时间或费用。

此前 dirty candidate 的 10/10 A/B 因 binary pair 与最终构建不一致，仅保留为历史诊断，
不再用于最终候选结论。

## 7. 真实 DeepSeek crash/reopen/resume

正式 v2 canary：

| 项目 | 值 |
|---|---|
| result schema | `codewhale.eval.m4a-resume-canary.v2` |
| result SHA-256 | `210a9771f4f78df77f8459ab2216381eea1ff2a3378af5b70bccee5c3f6720ee` |
| tested source | commit `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3`；tree `2f664616def40906c2ef62b5a3fec46a9a76c7a3`；clean |
| tested binary pair | `sha256:f4a7065612180257882afab195c2f5674164bd3a37fbaa642fd319610aeda8ad` |
| crash phase | `durable_pre_model_io` |
| kill | external `SIGKILL`，exit `-9` |
| crash prefix | 1 event：`run_created` |
| same-run resume | passed |
| Host verifier | passed |
| false success | 0 |
| API 请求 | 6/10；0 retry；usage response 6 |
| Token | 30,022（input 29,222；output 800；cache hit 27,904；miss 1,318；reasoning 290；replay 705） |
| wall time | 11,632 ms（持久 terminal duration 11,588 ms） |
| 费用 | `$0.000486651` / CNY `0.00347608`；usage 完整；unknown billing false |
| final event/terminal | 501 events；501 unique event id；sequence 1..501；1 terminal |
| 工具副作用 | before 0；after 1；delta 1；前后摘要均记录 |
| lease | epoch 1 -> 2；旧 PID 已死亡；新 owner 已观测；最终 owner 已释放 |
| no-key terminal replay | passed；0 new events；workspace/accounting unchanged |
| product metric eligible | `false` |

监督进程只在 Store 已持久化 `run_created`、模型请求尚未进入 in-flight 且 lease owner
正确时发送 `SIGKILL`。恢复继续同一 `run_id`，首个新 sequence 为 2；崩溃前后前缀摘要均为
`sha256:da2dcdc031ed642cdeadedb522b51ff871b29aadcdfd51c64c804e6411ac1951`，
最终事件摘要为
`sha256:87465340785d7be5898212d63798f142d5cd12783e6679418685a1e015607ffe`。
完成后只修改 `ranges.py`，冻结 verifier 的六类检查全部通过。随后移除 Key 再次执行
`--resume`，进程退出 0，事件 count/sequence/digest、工作区、工具副作用和稳定
terminal/accounting 全部不变。

所有真实恢复运行的实际消耗均披露，不把诊断失败隐藏为零成本：

| 运行 | 请求 | Token | wall time | 费用 USD |
|---|---:|---:|---:|---:|
| v1 首次诊断 | 4 | 19,060 | 8,593 ms | `$0.000335138` |
| v1 validation 诊断 | 6 | 30,439 | 12,597 ms | `$0.000528466` |
| v1 正式历史 canary | 4 | 19,180 | 8,667 ms | `$0.000372999` |
| v2 dirty-source preflight | 5 | 25,684 | 11,169 ms | `$0.000523746` |
| v2 commit-bound 正式 canary | 6 | 30,022 | 11,632 ms | `$0.000486651` |
| 合计 | 25 | 124,385 | 52,658 ms | `$0.002247000` |

supervisor SHA-256 为
`b9510949687e176f91643040a1f421defdd99092914dcfad8cfe843cd678597f`，其依赖的 exec
Harness SHA-256 为 `ef97a5e87b7ee76bfb76fc4321785317979de4526f660833c735c6c7a1cdbc97`。
原始结果位于被忽略的 `eval/results/`，权限为 `0600`；仓库只记录
脱敏汇总与结果摘要，不保存 Key、模型正文、reasoning、工具参数、event JSON 或 stderr。

计入历史 dirty A/B、最终 commit-bound A/B 和上表五次恢复运行，M4-A 实际消费合计为
121 次请求、612,165 Token、276,654 ms 被测进程 wall time、`$0.011867945`。该数值只用于
成本披露，不把不同证据等级的运行合并成成功率样本。

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

冻结 checkpoint 的 Git diff 为 `+34,234/-5,323`，但它同时包含 M2、M3、M4-A 和保留 WIP，
不能冒充 M4-A 的净代码增量。

## 9. 边界、残余风险与下一步

- fresh-run A/B 只覆盖一个 Python single-lane 任务，且最终候选效率结果负向/不确定；不能外推到
  广泛仓库成功率，也不能作为性能提升声明。
- live safe-point continuation 与 no-key replay 均为 1/1 canary，只证明链路可用，不是恢复率
  的统计估计；危险窗口由离线真实子进程矩阵覆盖。
- lease 存活判断目前使用 PID；极端 PID reuse 可能把失效 lease 暂时误判为
  `AlreadyRunning`。该风险不会产生双 writer，但可能阻止恢复，后续应评测是否需要更强
  的 process identity。
- TUI/app-server 仍有旧 loop 和私有状态；M4 总体没有完成。
- M5 的 TaskContract/EvidenceReceipt、M6 的 writer worktree 和 multi A/B 均不属于本切片。
- 下一垂直切片是 M4-B app-server 单一 Runtime/RunStore 迁移；它必须删除 app-server 的
  TUI 子进程 bridge、私有 thread store 和 completion 翻译，不把 M4-A 再实现一遍。
