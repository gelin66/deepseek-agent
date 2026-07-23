# M7-H canonical request/Token 浪费矩阵与 terminal catalog 结论

日期：2026-07-23

起始 revision：`48a0b44df2b8b0ba0f292af943a78d020484133e`

失败契约：`1943df4c0d9af834efe07ed13b735e344b5d7978`

production 修复：`eb8763a12fc3616577221a061bf5365680149b06`

production catalog 回归：`b1a01ce9`

冻结 observer：`15b5983e`，binary identity follow-up `b805a668`

## 1. 结论

决策为：

```text
keep deterministic Runtime correctness fix
hold broader request/token optimization
live inadmissible_no_model_treatment
```

当前证据定位到一个真实、单 owner、可删除旧路径的 Host 缺陷：reserved terminal
`tools=[]` 请求会在 catalog 选择前按 full ordinary catalog 做 hard-limit/compaction 决策。
这可以把实际可发送的 terminal 请求在 0 次模型请求前错误拒绝为
`ContextLimitExceeded`。

修复只调整 `crates/runtime` 的决策顺序：先确定本次实际 advertised catalog，再以同一
catalog 做 context estimate、compaction 和最终 `ModelRequest`。旧的 pre-model-turn
full-catalog 分支与一次性 `ContextCompactionControl` 已删除，production Rust 为
`+15/-56`。没有新增 Runtime、Store、事件、工具、提示词、模式、预算或依赖。

这不是新的模型策略或 provider surface。付费 API 调用无法比 deterministic Host contract
提供更多归因信息，因此没有读取 `/Users/gelin/Desktop/codewhale/key.txt`、没有调用官方
API、没有构建 paid release binary，也不宣称 Token、时间、费用或一般任务成功率改善。

## 2. 起始事实与审计范围

起始分支为 `deepseek-agent`，HEAD `48a0b44d`，tree
`c0b7b0f6f50b65ec8f7797971781b922e722b3ab`，工作树 clean。当前协议保持 Run API v10、
RuntimeEvent v16、State v21、exec-stream v2；root、read-only child 与 explicit Writer
继续共用：

```text
AgentApplication -> AgentRuntime -> RunStore
```

只读调用图覆盖：

- terminal request permit 的 reservation、归还与 `ModelRequestPrepared` 消费点；
- DeepSeek inference lease、physical attempts、usage/cache/retry/cost；
- root/child reasoning 与 tool-call reasoning replay；
- same-batch child join、handoff 后 root integration；
- ContextBroker compaction、hard-input limit 与 terminal failure；
- root/read-only/Writer、partial failure、cancel、SQLite reopen 与进程 crash；
- exec/app-server/TUI production caller，确认没有第二模型循环或私有 FIM/edit 路径。

permit reservation 本身不是逻辑请求。只有 `ModelRequestPrepared` durable commit 才成为
logical request；DeepSeek request lease 开始后才成为 physical attempt。提前 Stop 会归还
未使用 permit，不会强制制造一次请求。

## 3. 冻结 request/Token waste matrix

`scripts/eval-m7h-request-token-waste.py` 对四份既有 ignored evidence 做 SHA-256 校验后只读
投影；不打开 credential、不联网、不复制 Runtime、accounting 或 context estimator。
manifest 为 `eval/manifests/m7-h-request-token-waste-v1.json`，`maximum_reruns=0`。

| dataset | runs | root / child requests | reasoning | replay | compaction | hard exhausted | handoff observed |
|---|---:|---:|---:|---:|---:|---:|---:|
| terminal permit v1 | 12 | 76 / 13 | 11,683 unattributed | 22,814 unattributed | 0 | 0 | 6 |
| eager join v1 | 24 | 127 / 27 | 21,349 unattributed | 27,629 unattributed | 0 | 0 | 12 |
| M6-B1 Writer v2 | 36 | 167 / 118 | 45,797 root + 13,703 child | 93,376 root + 42,763 child | 0 | 0 | 10 |
| M7-A2 partial | 7 records / 6 valid | 50 / 0 | 16,638 root | 40,310 root | 0 | 0 | 0 |

每个可观测分组的 logical `ModelRequestPrepared` 与 physical started request 完全相等，
cross-check mismatch 为 0。四份历史 suite 互相重叠，不能相加成总体频率。M7-A2 是 partial、
不具产品指标资格。

矩阵的主要边界：

- 没有观察到 compaction 或 hard-budget exhaustion，只能报告样本为 0，不能推断全局为 0；
- Writer v2 可按 actor 精确归因 usage；旧 exec aggregate usage 不能安全拆分；
- 所有 79 个旧 record 都缺少逐请求 terminal catalog identity，统一标为
  `terminal_catalog_unobservable`；
- DeepSeek thinking tool-call replay 是官方协议要求，不能仅因 replay Token 较大就删除；
- 历史 terminal-permit A/B 已证明 terminal 可靠性会增加请求/Token/费用，仍按可靠性机制
  保留；历史 eager-join 已删除一次无信息 root integration 请求，当前没有第二个同类 owner。

最终 ignored output 为 `0600`、12,546 bytes，SHA-256：

```text
cdad0a5e91594d30c548ede92d0b7eee74f8bbec4a861f59dac90d9d2feea1d6
```

observer self-test 覆盖 O_EXCL、0600、current/legacy usage 字段与重复输出 fail closed。

## 4. 真实反例、验收和 cutover

实现切片按仓库要求冻结为：

1. 真实问题：terminal no-tools 请求错误使用 ordinary catalog 的 context size。
2. 验收：当 no-tools context 低于 hard limit、ordinary catalog context 高于 hard limit 时，
   只准备一个 `tools=[]` 请求、无 compaction、terminal 完成。
3. 单一 owner：`crates/runtime`；app 只提供真实 production composition 回归。
4. 替代路径：model turn 前独立的 full-catalog compaction decision。
5. 证据：失败先行 regression、production catalog boundary、Runtime/Store crash/replay
   conformance、独立 binary offline A/B。
6. cutover 删除：旧 pre-model-turn branch 与 `ContextCompactionControl`。

`1943df4c` 的失败契约先证明旧行为：terminal 为 `ContextLimitExceeded`，logical/physical
requests 均为 0。`eb8763a1` 后，同一 task、同一 deterministic `OneShotModel`、同一
1-request 总预算、`maximum_reruns=0` 得到 `Completed`，logical/physical requests 均为 1，
advertised tools 为 0，compactions 为 0。

独立构建 binary：

| arm | revision / tree | binary SHA-256 | result |
|---|---|---|---|
| baseline | `1943df4c` / `b9247f0b` | `11a9f7c54ff364cc19be1b2b53d2c1f3fe997869f31865799bcc10d2f27db82b` | expected fail |
| candidate | `eb8763a1` / `389aaa8a` | `6412b8d76e05f982a36b47c7ada70ce414c87d06b1ae31d9921da70cbd85660a` | pass |

首次让两个 worktree 共用 Cargo target 时，candidate 复用了 baseline test binary 并失败；
该运行身份无效，已作废。随后两个 arm 分别编译到独立 target，manifest 明确记录
`discarded_shared_target_reuse=true`，防止把 stale artifact 当成候选结果。

`b1a01ce9` 又以真实 `ProductionToolExecutor` catalog 计算 hard boundary，证明唯一请求
advertise 空 catalog、没有 `ContextCompactionCommitted`，不是 synthetic huge catalog 才能
通过的特例。

## 5. 门禁

所有 Cargo 构建均使用 `CARGO_INCREMENTAL=0`，并位于
`/private/tmp/codewhale-m7h-target` 或其独立 A/B 子目录。

通过：

- `cargo fmt --all -- --check`；
- runtime lib 40/40、conformance 81/81、Writer orchestration 25/25；
- app production catalog regression 与 app 全套 53 passed / 1 helper ignored；
- State process crash compaction reopen；
- `./scripts/dev-deepseek-agent.sh focused`；
- workspace clippy `--all-targets --locked -- -D warnings`；
- workspace tests 与 doc-tests；
- app-server process SIGKILL/reopen、exec in-flight SIGKILL/no resend、TUI/CLI projections；
- production-path loopback；
- general Harness self-test 25/25；
- M7-H observer self-test 与 frozen output byte rebuild；
- `git diff --check`。

第一次 workspace test 运行中，`codewhale-tools` 的 custom verifier 临时仓库有一次 macOS
`sandbox-exec` path-filter 类型错误；同一 exact test 立即通过，随后默认并发的完整 workspace
test 重新运行并全部通过。该故障没有触及 Runtime 候选代码，未据此修改无关 sandbox 路径。

## 6. 官方 DeepSeek 复核

复核日期：2026-07-23。只使用官方一手资料：

- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing)：当前官方
  `deepseek-v4-pro` / `deepseek-v4-flash`、1M context、384K maximum output、cache hit/miss
  与 output 价格；repository pricing fixture 与 capability fixture 已覆盖，不在本切片改写。
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)：thinking tool call
  对应的 `reasoning_content` 必须在后续请求完整回传；这解释 replay Token 是协议成本，
  不是可静默删除的冗余。
- [Function Calling](https://api-docs.deepseek.com/guides/function_calling)：普通 tool calls
  属于 Chat；Strict 是 Beta Chat，要求 beta endpoint 与完整 strict-compatible catalog。
- [FIM Completion guide](https://api-docs.deepseek.com/guides/fim_completion/) 与
  [Create FIM Completion](https://api-docs.deepseek.com/api/create-completion)：FIM 是独立
  Beta `/completions`，使用 prefix/suffix，guide 上限 4K；当前 production 仍没有 canonical
  FIM edit caller，本切片不恢复已删除的 `FimEditTool`。

易变的模型、limit 与价格继续由 `crates/deepseek` fixture 固定；本次复核没有发现需要与
terminal catalog 修复混合提交的协议变化。

## 7. 产品决策与非结论

保留：

- actual request catalog 先于 hard-limit/compaction 的唯一 Runtime 决策；
- exact terminal request、compaction、accounting 与 SQLite replay 合同；
- 冻结 request/Token observer 和独立 binary identity 规则。

Hold：

- reasoning on/off、budget、compaction 策略或 child/root request 数的进一步默认调优；
- paid request/Token A/B；当前没有模型 treatment。

不保留或不新增：

- 第二 context estimator、第二 Runtime/Store/accounting；
- 可由 `ModelRequestPrepared`/RunStore 重建的持久字段；
- 预算上调、自动 fan-out、Writer 并发、提示词包装、模式系统或 FIM edit caller；
- 为制造 live delta 而改变工具 catalog。

本切片不证明：

- 一般编码任务的 verified success、false success、Token、时间或费用改善；
- reasoning replay 可以安全删除；
- compaction 或 hard-budget exhaustion 的 production 频率为 0；
- root、read-only child 或 Writer 的默认预算应改变；
- FIM 相对 patch/edit 有收益；
- 一次 deterministic Host regression 是付费产品指标。

下一切片应只补 current v16/v21 trace 的 near-limit coverage：从 canonical
`ModelRequestPrepared`/RunStore 重建 actual catalog、context estimate、compaction 与
hard-budget boundary，覆盖 root/read-only/explicit Writer、cancel、partial failure 与
crash/reopen。不得新增第二状态真相。若没有新的 production 反例，应关闭 request/Token
调优并转入 M8 准入清理。
