# M10-G read-only fan-out 最终准入审计（2026-07-24）

## 结论

M10-G 的决定是：

```text
close_no_admissible_readonly_fanout_benefit_evidence
keep the consumed canonical read-only child mechanism
keep fixed root Pro/high, read-only child Flash/high and typed recheck Pro/max
do not add an automatic Host fan-out policy
remove fan-out from the active optimization queue
```

current production 已经能在一个 DeepSeek response 明确给出多个 `agent` calls 时启动同批
read-only child，并在一次 root integration request 前收齐 typed handoff。这个机制没有
第二 scheduler、Runtime、Store、工具目录或完成权。

但是，唯一直接比较 single root 与两个 read-only child 的 M7-G 正式矩阵没有产生一个
可用 arm；首个联网 control arm 的 observer identity defect 又使 billing 未落盘。
M9-C/M10-A/M10-B 的 current frozen evidence 只覆盖一个 read-only child，不是 fan-out
对照。M10-F 因而也没有找到 current multi-child quality/time loss 或可归因 treatment。

没有 verified success 非劣、false success 为零、accounting 完整且 wall time 稳定改善
至少 20% 的证据，所以不扩大、不默认调度、不读取 Key，也不再把 fan-out 留作 active
M10 候选。

## 真实调用图

consumer audit 区分了“已有机制”和“产品准入”：

```text
DeepSeek explicit agent tool calls
  -> AgentRuntime prepares and starts the whole same-batch child set
  -> ordinary read-only child: Flash/high
  -> typed failed-child recheck: Pro/max
  -> RunStore child lifecycle / request / usage / cost / terminal
  -> Host joins typed handoffs
  -> one Pro root integration request
```

`max_concurrent_children`、`max_depth` 和 TUI `[subagents]` 是这条 canonical lifecycle
的资源上限。它们不解析任务、不选择 child，也不会由 Host 自动生成并行计划。只有模型
显式返回多个 `agent` calls 才形成 fan-out。

现有 consumer 包括：

- `crates/runtime` 的 child launch/join、partial failure、cancel 与 recovery；
- `crates/app` 的 fixed actor route、DeepSeek composition 和 SQLite reopen；
- `crates/state` 的 pending ChildStarted SIGKILL/no-relaunch；
- app-server/TUI 的 canonical child projection；
- 单 Writer 的同一 Runtime 与 worktree orchestration。

因此没有可删除的 unconsumed production fan-out treatment branch。粗暴删除 concurrency、
child lifecycle 或 UI 投影会破坏已准入的单 child、explicit Writer 和恢复正确性。

## 证据矩阵

| evidence | current fact | 能否支持 fan-out 收益 |
| --- | --- | --- |
| M7-G formal | 9 pairs / 18 arms 预注册；`completed_arms=0`；首个联网 control billing unknown | 否 |
| M7-G2 | 11/11 observer fault windows；没有 production delta、Key 或 API | 只支持 evaluator durability |
| M9-C | 2 个 measurement-valid single read-only-child arms，均 verified | 只支持单 child correctness |
| M10-A | stopped campaign 中的 single read-only-child observations | 不是 fan-out 单变量 |
| M10-B | treatment single child 违反 frozen call args，candidate 已删除 | 不能作为 current fan-out loss |
| M10-F | 7 个 read-only trajectories 全部 completed；fan-out pairs=0；report 无 candidate | 否 |

M7-G summary 中旧 eager-join 诊断的 single/multi 都是 6/6，但它们不是同一 current
production delta 的正式因果矩阵，而且 multi 的 Token、时间和费用描述值反而更高。
本审计不把它们重标为正向或负向产品结论。

## correctness 复核

current production regression 继续证明：

- 两个 read-only child 都在任一个 child join 前启动；
- root 使用 Pro/high，普通 read-only child 使用 Flash/high；
- prior read-only child failure 只触发新的 Pro/max recheck，不在原 Run 偷换模型；
- 两份 typed handoff 均在一次 root integration request 前提交；
- root/child route、request、usage、cost、terminal 在 SQLite reopen 后逐字段一致；
- durable `ChildStarted` 后 SIGKILL 不重发请求、不重启 child；
- same-batch partial failure 与 cancel 会 settle 所有 child；
- read-only child 不能获得 Writer worktree 或 applied workspace side effect。

M7-G 和 M7-G2 Harness self-test、M10-F canonical report 在本切片重新执行通过。frozen raw
保持原 mode/hash/bytes；没有续跑、补 mate、拼接或重写历史。

## 保留与删除

保留：

- 一个 canonical `agent` tool；
- same-batch read-only child overlap；
- fixed actor route、typed handoff、accounting、reopen、cancel/recovery；
- M7-G/M7-G2 frozen manifests、summaries、0600 raw；
- 两个历史 runner。M9-C 的未满足 baseline cutover 明确仍以
  `scripts/eval-m7g-readonly-fanout.py` 为 reproducibility consumer；M7-G2 runner
  继续复算 fail-before-loss fault contract。

关闭：

- 自动 Host fan-out、默认 swarm、投票完成、multi-Writer；
- 为 fan-out 新增 prompt wrapper、模式、scheduler、ledger 或 model router；
- 没有 current production delta 的付费 successor；
- M10 active roadmap 中的 read-only fan-out admission 项。

本切片没有 production code、协议、schema、config 或用户面变更，也没有 candidate raw。
PRODUCT_PLAN 与 ADR-0008 不需要修改。

## 身份与安全边界

- 起始 commit：
  `8c576c2696be0b5dfd1f317bfb68a5a38837034e`；
- tree：
  `d7f850abcb85f2d80a8ce6d7afdaf10c1f095cc7`；
- Run API v12、RuntimeEvent v18、State v24、exec-stream v3；
- M7-G raw：
  `sha256:177bb20aa9b1d5d91b75a60ce3fde7c783c0e927567f9c2c79e2ad971fa546eb`；
- M7-G2 raw：
  `sha256:c733d7a4714f49477c1ce43a01963209c37179dbea509ce0e709c743f3c94009`；
- M10-F report：
  `sha256:765629c3a889bc44aabd9d6b34c42bb93ea32ed5aa2b909bf90a15b36203483e`。

credential read=false，network/API=0，new raw=false。

## 离线门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m10g-target
```

通过：

- M7-G Harness self-test 与 schedule/task-contract identity；
- M7-G2 11-window observer durability self-test；
- M10-F canonical trajectory report 重算；
- production two-child overlap、fixed route 与 exact SQLite reopen；
- process-level read-only `ChildStarted` SIGKILL/no-relaunch；
- Runtime exact recovery terminal、same-batch partial failure 与 cancel；
- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`；
- manifest JSON、frozen raw mode/hash 与 `git diff --check`。

## M10 闭环

M10-A 至 M10-E 的 product treatments 分别因 incomplete accounting、质量 veto、offline
viability、无独立 controller delta、无 measured environment loss 而未准入或删除。
M10-F 保留只读 loss analyzer，但没有产生新 candidate。M10-G 关闭最后一个 active
条件能力审计。

CodeWhale 继续以 fixed Pro 根能力、single canonical Runtime/Store、latest-revision
evidence、一个 explicit Writer worktree 和条件 read-only child 为 production 基线。
未来只有新的 canonical trajectory 先暴露重复、current、可冻结的多 child loss，并形成
真实 production 单变量，才能按新的独立 Goal 重新提出；不能把本次关闭当成待执行 backlog。
