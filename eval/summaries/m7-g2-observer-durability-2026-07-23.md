# M7-G2 observer durability 与 successor 准入结论

## 1. 结论

- 起始 clean revision：
  `3382bbc4887984b31f39e31134c02fdd06615754`（M7-G 结论）。
- fail-before-loss evaluator checkpoint：
  `f89dafc56330c1c87d61a90f4941a0892688dae8`。
- 协议保持 Run API v10、RuntimeEvent v16、State v21、exec-stream v2；production
  `AgentApplication -> AgentRuntime -> RunStore`、`Orchestrator` 和 explicit read-only
  child 均未修改。
- 产品决策：**hold / live_successor_inadmissible_no_new_production_delta**。M7-G2
  已冻结并验证 observer durability contract，但该 contract 只修正离线评测的事实落盘顺序，
  不形成新的 fan-out production treatment。换 suite ID、output、binary 或 evaluator-only
  revision 仍是对已消费 M7-G treatment 的伪重跑。
- `/Users/gelin/Desktop/codewhale/key.txt` 未读取，官方 DeepSeek API 请求为 0；没有 release
  binary、paid arm 或产品指标。
- M7-G 的旧 `0600` raw 保持原 hash、原字节和 unknown billing；没有补写、续跑、补 mate
  或拼样。read-only child 继续 explicit-only，不默认启用自动 fan-out。

## 2. 真实问题与切片合同

M7-G v1 的 `execute_arm()` 在 terminal 后继续依次做 identity、accounting、surface、
lifecycle、external verifier 和产品指标派生，只有全部结束才把一个 arm 返回给外层
`emit()`。arm 又位于 `TemporaryDirectory` 中；任一派生异常都会先展开并删除唯一
SQLite State，再由外层写 abort。`8763722c` 只把 terminal/accounting/State 附加到一种
identity exception，未覆盖 verifier、surface、accounting 自身失败、raw 半写或 evaluator
进程被杀。

本切片冻结以下唯一顺序：

```text
plan
  -> exact terminal_snapshot
  -> no-credential sqlite_reopen_snapshot
  -> verifier_snapshot
  -> derived arm_result | abort
```

每条完整记录使用 `O_EXCL | O_APPEND | O_NOFOLLOW` 创建的 `0600` JSONL、单调 sequence、
前一记录 SHA-256 chain 和逐记录 file `fsync`；首次 reservation 同时 directory `fsync`。
既有 output 永不重开追加。最后一条半写记录只作为 detectable partial tail，不能参与派生、
续跑或恢复同 suite。

| 项目 | M7-G2 事实 |
|---|---|
| 单一 owner | `scripts/eval-m7g2-observer-durability.py` 只拥有离线 journal 顺序 |
| production owner | Runtime/Store/DeepSeek/tools 无改动 |
| 替代路径 | future successor 不得继续“完整 arm 派生后才 emit” |
| cutover 删除 | 本阶段不保留 paid runner、Key 参数、network caller 或 experiment-only production branch |
| 验收证据 | hash-chain/tamper/O_EXCL、自校验、11-window exception/SIGKILL matrix、production SQLite exact reopen |

## 3. fault-injection 结果

离线 Harness 以独立子进程覆盖 11 个窗口：

1. identity exception；
2. verifier exception；
3. surface exception；
4. accounting exception；
5. terminal record 写前 SIGKILL；
6. terminal record 半写 SIGKILL；
7. terminal 完整 `write` 后、`fsync` 前 SIGKILL；
8. terminal `fsync` 后 SIGKILL；
9. SQLite reopen snapshot `fsync` 后 SIGKILL；
10. verifier snapshot `fsync` 后 SIGKILL；
11. derived result 写前 SIGKILL。

结果为 11/11：

- 所有 fault 都没有提交 `arm_result`；
- identity/verifier/surface/accounting exception 均先保留 terminal 与 reopen facts，再写
  typed abort；
- 半写只留下一个可检测且不参与 hash chain 的 partial tail；
- 完整记录保持 `0600`、sequence/hash chain 可重建；
- 篡改 exact RunView 字段会被 record hash 拒绝；
- 已存在 output 被 `O_EXCL` 拒绝，不能换进程续写；
- fault child 全部 `key_accessed=false`、`network_accessed=false`。

production 定向门禁继续使用 canonical owner，而非 Python 模拟 Store：

- `codewhale-app` 的
  `m7g_production_loopback_read_only_children_overlap_and_reopen_exactly` 证明 4 个
  request、两个 child transport overlap、两份 typed handoff、root/child exact accounting，
  并在 `StateStore::open` 后逐字段 `assert_eq!` root 与两个 child replay；
- `codewhale-state` 的
  `read_only_child_started_sigkill_fails_closed_without_relaunch` 证明 process SIGKILL 后不重发；
- `codewhale-runtime` 的
  `exact_recovery_terminal_may_preserve_unsettled_read_only_child_truth` 证明只有 exact typed
  ambiguity 可以关闭 unfinished lifecycle。

该组合证明 production RunStore 的 request/usage/cost/terminal 可精确重开，以及 evaluator
contract 会在任何产品派生前按原 projection 持久化这些值。它不声称本阶段存在一个新的
paid arm；没有 production delta 时，live runner 被有意省略。

## 4. 身份与 raw

冻结身份：

- Harness SHA-256：
  `e0df9086636b1c811dac5c9f1973135f9ff1cc2391dfc079e6ee59ad93922ab0`；
- manifest SHA-256：
  `faa9e7d2c8fc1120dfd814379d884ad0badb45d3ac4385f42454f09c017f1b79`；
- fault matrix SHA-256：
  `42245c0b3fe4e3d8f86a05d8ddab90e44403eceede8e9bea0e712f1eb1f402b5`；
- sample exact-store projection SHA-256：
  `0279798e999315cac6466b143fac1ce0f0218b8c6513b9066b1987951e4d9a91`。

新的 ignored offline raw：
`eval/results/m7-g2-observer-durability-3382bbc4-v1.jsonl`，`0600`，
14 records / 9,235 bytes，SHA-256
`c733d7a4714f49477c1ce43a01963209c37179dbea509ce0e709c743f3c94009`。
最后一条明确记录：

```text
decision=hold_no_new_production_delta
successor_live_api_admitted=false
product_metric_eligible=false
key_accessed=false
network_accessed=false
maximum_reruns=0
```

旧 M7-G raw 仍为
`eval/results/m7-g-readonly-fanout-062623e6.jsonl`，`0600`，3 records / 4,032 bytes，
SHA-256 `177bb20aa9b1d5d91b75a60ce3fde7c783c0e927567f9c2c79e2ad971fa546eb`。
其 abort 仍只有 `completed_arms=0`、`run_identity_invalid`、Key/network true；
没有 accounting，故最终 billing 继续为 unknown。

## 5. live successor 为什么不准入

满足 observer durability 只是必要条件，不是新的产品 treatment。Git 调用图和历史证明：

- fan-out 的最后 material production candidate 仍是 `062623e6`；
- `8763722c` 只改 Harness verifier identity 和 abort details；
- `3382bbc4 -> f89dafc5` 只增加离线 manifest/journal，不改变 Runtime、Store、DeepSeek、
  tools、catalog、prompt、budget 或 read-only child admission；
- 因而新 binary 在 production 行为上仍执行与已消费 M7-G 相同的 control/treatment。

按 `EVALUATION.md`，Harness-only 修复、重命名、换 output 或重新编译不能单独产生新 candidate。
本阶段因此在 Key read、binary freeze 和 API 之前 fail closed。未来只有出现由真实
request/Token counterexample 支持的 material production delta，才能用全新 clean revision、
suite ID、output、immutable binary，从 position 1 运行完整 9 对 / 18 arms；
`maximum_reruns=0`，且不得引用旧 arm。

## 6. 官方协议复核

2026-07-23 重新核对 DeepSeek 官方一手资料：

- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)

当前 Chat reference 仍允许 `tool_choice=auto|required` 产生一个或多个 tool calls；没有需要
CodeWhale 新增的第二 fan-out API 或产品模式。streaming usage 仍通过 `[DONE]` 前的独立
usage chunk 闭合。thinking tool-call 历史仍要求后续请求完整回放 `reasoning_content`。
当前 `deepseek-v4-flash` 价格 fixture 仍是 cache-hit input `$0.0028`、cache-miss input
`$0.14`、output `$0.28` / 1M tokens。M7-G2 没有使用这些价格发起请求。

## 7. 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m7g2-target
```

已通过：

- Harness frozen-hash self-test、tamper/O_EXCL contract 与 11/11 fault matrix；
- ignored offline raw 生成、完整 chain reopen、mode/hash 核对；
- production fan-out overlap + exact SQLite reopen targeted test；
- read-only child process SIGKILL/no-relaunch targeted test；
- Runtime exact recovery terminal targeted test；
- `cargo fmt --all -- --check`；
- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- `git diff --check`。

## 8. 产品取舍与下一切片

- **keep**：explicit-only canonical read-only child、M7-G overlap/recovery regression、M7-G2
  fail-before-loss offline journal。
- **shrink**：无 paid successor runner、无第二 accounting/Store owner；observer sample 只验证
  journal 语义，production exactness 继续由 Rust owner 测试。
- **hold**：fan-out 产品收益、自动 fan-out admission、默认 child concurrency。
- **reject**：把 evaluator-only revision 当新 production candidate、续跑/补 mate/拼接旧 raw、
  第二 Runtime/Store/model loop/tool catalog、Writer 并发、通用 DAG 或 prompt wrapper。

下一独立切片转向 canonical Agent 请求与 Token 预算：先从 RunStore 事件离线量化 child/root
terminal request、reasoning replay、handoff 后 root integration 和 hard-budget exhaustion，
找到一个可复现的浪费反例后才改唯一 Runtime/DeepSeek owner；没有反例就不开发、不读 Key。

M7-G2 不证明：

- 两个 read-only child 比 single root 更快、更省 Token 或更便宜；
- observer durability 会提高 verified success；
- 可以补算 M7-G 首个 control arm 的费用；
- 新 suite ID 足以重新获得 paid evaluation 资格；
- Agent 请求或 Token 浪费的具体 production 根因已经确定。
