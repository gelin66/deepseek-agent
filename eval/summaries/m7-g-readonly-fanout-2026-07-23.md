# M7-G canonical read-only fan-out 审计结论

## 1. 结论

- 起始 clean revision：
  `ccb83e0bea2a34274a9d2d94100399195cd00d83`（M7-F 结论）。
- 正式候选：
  `062623e62328b1b01face1b64f73a48081eefa48`。
- 协议保持 Run API v10、RuntimeEvent v16、State v21、exec-stream v2。
- 产品决策：**hold / inadmissible_observer_identity_bug**。当前 production 已能在同一
  assistant response 中启动多个 read-only child 并在 Host join 前真实重叠执行；本切片没有
  增加 scheduler、模型循环、Runtime、Store、工具目录、产品模式或 Writer 并发。
- 正式 9 对 / 18 arms A/B 在首个 control arm 后停止。Key 已按授权读取且发生了官方网络
  请求，但 Harness 在 arm 结果落盘前错误比较 caller-authored verifier plan 与 Host-resolved
  canonical plan；`completed_arms=0`，usage、请求数和费用没有进入 raw，最终 billing
  **不可证明**。`maximum_reruns=0`，因此没有续跑、补 mate、换 output 或拼接结果。
- 该 formal 不具备产品指标资格，不能证明 fan-out 提高或降低 verified success、false
  success、wall time、Token、请求数、cache 或费用。显式 read-only child 机制保持现状，
  不默认启用自动 fan-out。

## 2. 审计事实与切片合同

| 项目 | 事实 |
|---|---|
| 真实问题 | 判断相互独立的只读调查能否通过 canonical child 重叠降低 wall time，而不把额外模型请求、上下文和 handoff 成本隐藏到别处 |
| 单一 owner | `AgentRuntime` 启动/汇合 child，`Orchestrator` 构造同一 Runtime，`RunStore` 持久化生命周期与 accounting |
| 当前调用图 | Runtime 先为同批全部 `agent` calls 写入 prepared/started 并启动 child，再统一 `join_children`；不是逐 child 串行执行 |
| treatment | 同一 binary/prompt/model/预算下，control 不 advertised `agent`；treatment advertised canonical `agent`，限定同回合恰好两个 read-only Explorer |
| cutover | 没有新 production treatment branch；保留既有 explicit child 机制、恢复正确性修复和离线证据 |

既有历史 eager-join 结果只有一个 child，不能证明 fan-out。其成功 arms 均为 `6/6`，但历史
single 平均约 `17,636` tokens / `12.99s` / `$0.000433`，multi 平均约 `32,510` tokens /
`28.79s` / `$0.001146`；这些值只用于定位“额外 child 开销可能吞掉并行收益”，不作为
M7-G 因果结论。

M7-G 冻结三类真实临时 Git 仓库任务：

1. 多规格 capability intersection；
2. 多组件 transitive reverse dependency impact；
3. 分层 policy merge 与 locked-key 反例。

每项都有 deterministic verifier、唯一允许修改文件和两个相互独立的只读分区。正式矩阵
固定同 revision、同 immutable binary、逐字相同任务文本、`deepseek-v4-flash`、
`reasoning_effort=high`、相同 output/request/turn/tool/wall budgets、每 cell 三次和
`maximum_reruns=0`。

## 3. production correctness 证据

只读审计发现一个与性能 treatment 独立的真实恢复缺陷：SIGKILL 发生在 durable
`ChildStarted` 之后时，Runtime 正确选择 typed `RecoveryRequired` terminal，但 RunStore
会把该 terminal 当成“未 settled child lifecycle”而拒绝提交，导致 crash truth 无法持久化。

`13b94210` 在 `crates/runtime` 唯一 owner 中只允许两个可证明的 fail-closed terminal：

- in-flight `agent` tool operation 与 durable `ChildStarted` 对应的 ToolExecution ambiguity；
- 指向确切 unfinished child ID 的 ChildRun ambiguity。

普通 failure 仍不能越过未闭合 child lifecycle。新增证据覆盖：

- 真实 app-server/DeepSeek loopback 中第二个 child 在第一个 child 未响应时已到达 transport，
  随后两份 typed handoff、accounting 和 SQLite reopen 逐字段一致；
- durable `ChildStarted` 后进程级 SIGKILL，重开后不重发模型请求、不重启 child，并持久化
  exact typed ambiguity；
- 同批一个 child 失败时保留 sibling handoff；
- cancel 会 settle 同批每个 pending child；
- root/read-only child/Writer 继续共用同一 Runtime/Store conformance。

该修复是 crash/reopen correctness，不是 fan-out 收益证据。

## 4. 正式身份与中止

正式候选 release identity：

- binary：`codewhale 0.8.68 (062623e62328)`；
- SHA-256：
  `432a6d6a18906826f16b9949ee7223bb1d365a2319cb0d5884b0a7280c76e911`；
- size：`16,207,680` bytes；
- frozen Harness：
  `b08be16fb48ed3d5683e9c6277d13864f130280efd363ec6558836c713a8e636`；
- frozen manifest：
  `4ca1267c9da923aa9b84cde0d589300edf6c1084f83b7ec1819e7cd982aaa8ae`；
- schedule：
  `67e90598816cf5b7586e50d8cda1bdc456eb1291bd6b1c4155dfdcbf3fd10dbb`。

ignored `0600` raw：
`eval/results/m7-g-readonly-fanout-062623e6.jsonl`，3 records / 4,032 bytes，
SHA-256 `177bb20aa9b1d5d91b75a60ce3fde7c783c0e927567f9c2c79e2ad971fa546eb`。
它依次记录 plan、credential access 和 abort：

```text
key_accessed=true
network_accessed=true
completed_arms=0
error_code=run_identity_invalid
maximum_reruns=0
```

根因是 production 在 start 边界用 `crates/tools` resolver 替换 caller-authored verifier
plan：Python verifier 注入 `PYTHONDONTWRITEBYTECODE=1`，gate timeout 固定为 `600000ms`。
Harness 却拿提交前的空 env / `120000ms` plan 与 `RunCreated` 整体相等比较。该错误发生在
terminal、events、state 和 accounting 已读取之后，但旧 Harness 没有在 identity audit
失败时把 accounting 写入 abort record，因而无法证明首个 arm 的实际 usage/cost。

按预注册规则，unknown billing 立即停止 paid calls。`8763722c` 只做 post-decision 离线
hardening：

- Harness 任务合同改为与 Host resolver 相同的 env/timeout；
- identity failure 逐字段报告 model/effort/output/task hash；
- post-terminal identity failure 先保存 terminal、accounting 与 State schema；
- self-test 和 frozen hash 重建。

修正后的 Harness SHA-256 为
`9591bcb0f70178572845fc60461a30bc6b2f18265db94f3fd70bab3da334790e`，
task-contract hash 为
`f333a0cf4427efed3b6f97bc15c183e7e1f45cbf3af4a59ca831eb1677d2f1f2`。
它没有被用来重跑本 formal。

## 5. 官方协议复核

2026-07-23 重新核对 DeepSeek 官方一手资料：

- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)

当前官方 Chat 响应可以返回一个或多个 tool calls；文档没有提供需要 Host 猜测的
`parallel_tool_calls` 产品开关。并行来自模型同回合发出多个 canonical `agent` calls 和
Host 对无副作用 child 的并发执行。thinking 历史仍要求精确回放 `reasoning_content`。
易变价格按当日 fixture 固定：`deepseek-v4-flash` cache-hit input `$0.0028`、
cache-miss input `$0.14`、output `$0.28` / 1M tokens；旧 alias 当时公告将在
`2026-07-24 15:59 UTC` deprecated。

## 6. 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m7g-target
```

正式 Key 读取前已通过：

- owning runtime/app/state targeted tests；
- production transport overlap + SQLite reopen；
- read-only child SIGKILL/reopen；
- same-batch partial failure/cancel；
- Harness self-test、frozen hash 和 no-key/no-network dry-run；
- `cargo fmt --all -- --check`；
- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`。

中止后的 Harness hardening 又通过 self-test、frozen dry-run 与 `git diff --check`。没有
再次读取 Key或调用 API。

## 7. 产品取舍与下一切片

- **keep**：既有 explicit read-only child、真实同批 overlap、typed handoff/accounting、
  crash/reopen store 修复和冻结离线任务。
- **shrink**：不增加新的 scheduler 或模型可见同义工具；历史单-child eager-join 结果只作
  非因果诊断。
- **hold**：read-only fan-out 的产品收益、自动 admission、默认并发与任何 paid successor。
- **reject**：第二 Runtime/Store/model loop/tool catalog、通用 DAG、Writer 并发、产品模式、
  提示词 wrapper，以及把本次 abort 当作 control/treatment 指标。

下一切片若重开该问题，必须使用新的 successor suite ID、clean revision、output 和 immutable
binary，从 position 1 执行；第一道门不是再次付费，而是 fault-inject 每一个 post-terminal
observer failure，证明 terminal、exact RunStore accounting、raw identity 和费用先于任何
派生判定落盘。只有上一轮 unknown billing 不会重现、control/treatment surface 仍真实且
suite 不是同一 formal 的续跑时，才允许重新读取 Key。否则保持 explicit-only 并转向 Agent
请求/Token 预算这一更独立的瓶颈。

M7-G 不证明：

- 两个 child 比 single root 更快、更便宜或更可靠；
- 当前模型会稳定遵守“同回合恰好两个只读 child”；
- 一个完成但未落盘的 control arm 能代表九对矩阵；
- provider 文档中的多个 tool calls 自动意味着并行收益；
- correctness store 修复会改善任何效率指标。
