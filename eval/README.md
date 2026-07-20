# DeepSeek Agent 评测入口

本目录只保存能够复核产品能力的任务清单和小型汇总。原始日志与逐次结果分别写入
`eval/raw/` 和 `eval/results/`，默认不提交。

## 当前里程碑证据

M4-A 的 canonical `ToolOutcome`、SQLite RunStore、进程中断恢复、生产 `exec` 验收、
真实 DeepSeek 任务与复杂度证据见
[M4-A Headless 状态真相汇总](summaries/m4-a-headless-state-2026-07-16.md)。该汇总只保存
脱敏指标、二进制/事件摘要和证据边界，不复制 Key、模型正文、工具参数或原始逐次结果；
单次 live resume canary 明确不属于产品指标。长期恢复判定以
[EVALUATION.md](../docs/product/EVALUATION.md#53-持久化进程中断与恢复证据契约)为准。

M5-A 的 canonical TaskContract/EvidenceReceipt 不能由普通 Host-only exec A/B 验收。
专用评测器通过 app-server Run API v4/v5 给 baseline/candidate 提供相同 model-visible
任务，并让 candidate 真正执行冻结的 exact Host verifier：

```bash
python3 scripts/eval-m5-completion-gate.py --self-test
python3 scripts/eval-m5-completion-gate.py --dry-run
```

正式 12-run 结果、二进制/asset SHA、false-success 对照、费用和复杂度边界见
[M5-A canonical 完成门禁精确 A/B](summaries/m5-completion-gate-ab-2026-07-19.md)。
凭据化运行必须显式提供两个冻结二进制、`--key-file`、被忽略的 `--output` 和
`--acknowledge-cost`；Key 只进入 app-server 子进程环境。

M4-A resume supervisor 先复用生产编码 Harness 做离线自检；自检和 dry-run 都不读取 Key、
不联网：

```bash
python3 scripts/eval-deepseek-resume.py --self-test
python3 scripts/eval-deepseek-resume.py \
  --candidate /absolute/release/codewhale \
  --candidate-revision <frozen-revision> \
  --dry-run
```

显式确认费用后再提供 `--key-file` 与被忽略的 `--output eval/results/...json`。脚本只在
模型 I/O 前的可安全继续持久点执行外部 `SIGKILL`；危险的模型/工具 in-flight 窗口由离线
进程矩阵验证 fail-closed，不能用真实 Key 自动重试。正式结果自绑定 supervisor、依赖
Harness、候选 revision 和 binary-pair SHA，且始终标记 `product_metric_eligible=false`。

## M6-B1 同二进制 Writer 收益 A/B

[`eval-m6-writer-benefit.py`](../scripts/eval-m6-writer-benefit.py) 使用同一个冻结
`codewhale app-server --stdio` 二进制、相同 TaskDefinition、模型、工具 policy 输入与总预算，
比较 single（子 Agent 深度/并发为零）和 M6-A isolated Writer（深度/并发各一）。冻结计划、
三个 clean-Git fixture、任务、路径、verifier、顺序和决策门槛见
[`m6-b1-writer-benefit-ab-v1.json`](manifests/m6-b1-writer-benefit-ab-v1.json)。

先执行不会读取 Key 或联网的自测与计划检查：

```bash
python3 scripts/eval-m6-writer-benefit.py --self-test
python3 scripts/eval-m6-writer-benefit.py --dry-run
```

从冻结提交使用仓库外 Cargo target 构建一次 release 二进制后，再把同一个 binary/revision
提供给 dry-run。正式运行固定为 18 对、36 个 accepted arm；每个任务内奇数对
`single -> writer`、偶数对反向，且每个 arm 都从字节一致的全新 fixture 和独立 State
开始：

```bash
python3 scripts/eval-m6-writer-benefit.py \
  --dry-run \
  --candidate-binary /absolute/frozen/codewhale \
  --candidate-revision "$(git rev-parse HEAD)"

python3 scripts/eval-m6-writer-benefit.py \
  --acknowledge-cost \
  --key-file key.txt \
  --candidate-binary /absolute/frozen/codewhale \
  --candidate-revision "$(git rev-parse HEAD)" \
  --output eval/results/m6-b1-writer-benefit-ab.json
```

当前 child policy 只能从 parent policy 收窄，因此 Writer root 的实际目录仍包含写工具；
Harness 要求它恰好委派一次，并把任何 root 直接写入判为产品失败，不能伪称 Host 已物理
移除权限。正式 Harness 以 `--transport-max-retries 1` 启动同一个 app-server 二进制，
accepted-arm 还要求实际总 transport retry 不超过 1；所有 retry 都进入每 arm 10 次共享物理
请求硬预算。若配置身份或实际计量不符，该 arm 不具备正式资格，不能靠扩大预算或重跑模型
失败掩盖。效率中位数只使用 single 与 Writer 均通过冻结 verifier 的 pair，T1/T2/T3
分别至少需要 3 个这种双成功 pair；模型失败不会因更快、更省而被误算成效率收益。Harness
还会在每个 arm 前和最终落盘前复核仓库、fixture、manifest、Harness、helper 与 binary
身份，运行中变化会生成 typed abort。

Runtime 在最后一次 terminal model request 会有意移除全部工具；Harness 只允许最后一个
request 使用空目录，此前每个 request 仍必须精确匹配 treatment 目录。225 秒 Runtime wall
后先留 5 秒给原生 timeout 收敛；仍无终态时，Harness 才通过 canonical Run API 发送
cancel，并以 5 秒 stdio 轮询上限在总计 240 秒内收集 typed terminal 和完整 accounting，
而不是与原生 deadline 竞态，或把悬挂执行伪装成未知的成功/重采样。
同一个绝对 arm deadline 还约束每个 stdio frame（包括部分帧读写）、fixture/evidence Git
命令、外部 verifier 和 app-server 停止等待；临时目录删除后的总耗时也会复核，超限即失败。
TaskContract 中冻结的 verifier step timeout 必须与生产 `run_verifiers` 实际签入 observation
的 600000 ms 完全一致，否则即使命令通过，Host 也会因 spec 不同而拒绝 EvidenceReceipt；
Harness 自己的进程外复核仍使用独立的 30 秒上限。

每个 arm 冻结 0.02 USD 已知费用储备；新 arm 只有在“累计已知费用 + 未知账单储备 +
下一 arm 储备”不超过 0.50 USD 时才能启动，arm 后再次检查。该边界是已知费用下界加冻结
储备，不虚构 unknown billing 为精确零账单。

结果只保存 canonical 计量、事件种类、工具名/hash、receipt/hash 和 Git lifecycle 事实；
不保存模型正文、reasoning、工具参数/输出、stderr、临时路径或 Key。partial/final 结果均以
`0600` 原子写入被忽略的 `eval/results/`。

## M1 基线证据

2026-07-15 的同 Harness 结果、证据边界和后续决策见
[M1-A 离线契约基线汇总](summaries/m1-offline-baseline-2026-07-15.md)。

同日 5 次真实 DeepSeek 请求的协议结果、usage/费用和能力边界见
[M1-B DeepSeek live 协议 Canary](summaries/m1-b-deepseek-live-2026-07-15.md)。

2026-07-15 的 41 项历史清单已原样冻结为
[`archive/m1-offline-2026-07-15.tsv`](archive/m1-offline-2026-07-15.tsv)。它来自提交
`366e8b5b37bedbbf3b1ebb326b14a70e885a57a2`，Git blob 为
`678c8e30d471e356cb93c47781c87b0c8624c26d`；现有 41/41 candidate 与 12/12 imported
结果继续绑定这份历史口径，不能用后续重构后的测试冒充同一清单。

[`manifests/m1-offline.tsv`](manifests/m1-offline.tsv) 是当前 canonical 回归门禁，只登记
真实 owner 下仍存在的行为：

- `full-runtime-offline`：本地 DeepSeek mock 驱动真实
  `exec -> AgentApplication -> AgentRuntime`；
- `protocol-unit`：验证 `crates/deepseek` 的 Standard/Beta/Strict/FIM 规划、回放、usage 和
  类型化 SSE 结果；
- `runtime-contract`：验证 canonical Runtime、RunStore、确定性工具证据、多 Agent handoff
  与入口删除契约。

当前清单不再保留旧 Engine、TUI ToolRegistry、私有 mailbox、模型 critic 或声明性
worktree 测试。FIM 当前只登记 Beta route planner；完整 response parser、畸形 SSE、
reasoning-only、工具业务失败恢复、child 失败 handoff、失败测试结果和 writer worktree
仍是显式能力债，补齐 canonical owner 测试前不得放入可运行清单。

离线用例只证明确定性协议和运行时行为，不证明真实 DeepSeek 的编码智能；模型自评也不能
作为任务完成证据。

现有 `codewhale eval` / `crates/tui/src/eval.rs` 会直接调用一套重复实现的简化文件与
Shell 函数，绕过生产 Agent loop 和生产工具注册表，因此它只算 smoke，不纳入 M1
生产能力结论。

## 运行

当前提交的全部离线用例：

```bash
bash scripts/eval-m1.sh
```

只运行可与导入基线 `352e86a6` 比较的用例：

```bash
bash scripts/eval-m1.sh --scope cross-revision
```

复现 2026-07-15 历史口径时，必须把归档清单显式传给当时或可比较的干净 worktree：

```bash
bash scripts/eval-m1.sh \
  --repo /absolute/path/to/historical-worktree \
  --manifest eval/archive/m1-offline-2026-07-15.tsv
```

使用当前评测器测试另一个干净 worktree：

```bash
bash scripts/eval-m1.sh \
  --repo /absolute/path/to/worktree \
  --scope cross-revision \
  --output eval/results/m1-offline-imported.jsonl
```

脚本先按 package/target 缓存 Cargo test list，并要求每个精确测试名恰好匹配一项；清单过期
会在正式运行前失败。逐项执行后仍检查确实运行且通过了一个测试，避免“过滤器匹配零项但
Cargo 返回成功”的假绿。目标仓库必须是干净提交；清单、结果 Schema 或证据等级不合法时
也会直接失败。结果同时记录被测提交、评测器提交和 manifest blob，避免以后用不同清单
生成同名“基线”。整套运行完成后才会原子发布 JSONL 和正式日志目录；被中断的日志只会
留在带 `.incomplete.<pid>` 后缀的目录中。

## DeepSeek 协议 live canary

先检查固定计划，不读取 Key，也不联网：

```bash
python3 scripts/eval-deepseek-live.py --self-test
python3 scripts/eval-deepseek-live.py --dry-run
```

显式确认费用后运行最多 6 个请求，覆盖 Standard Chat、thinking 工具轮原样 replay、
Beta Strict 的 non-thinking 工具调用及第二轮无 `reasoning_content` 回放，以及 Beta FIM：

```bash
output="eval/results/m1-b-deepseek-live-$(git rev-parse --short=8 HEAD)-$(date -u +%Y%m%dT%H%M%SZ).jsonl"
python3 scripts/eval-deepseek-live.py \
  --acknowledge-cost \
  --key-file key.txt \
  > "$output"
```

`key.txt` 已被仓库忽略，仍应保持 `0600` 权限。脚本不输出模型正文、reasoning、工具参数、
完整请求/响应或 Key。每条记录带同一 `evaluation_id`、Harness 文件摘要；每次请求还带实际
发送的规范化 JSON body 摘要，既可关联证据，又不会保存请求正文。Strict non-thinking 第二轮
同时验证回放请求省略 reasoning 字段、响应 reasoning 为空。已归档的 M1-B 基线是旧版 5/5 结果，按原始 usage 估算费用为
`$0.0000850004`；当前 6 请求脚本的计划上界是 `$0.00471216`，并继续设置 `$0.01`
停止线，必须实际重跑后才能形成新的线上证据。这些记录只证明线上协议契约，不属于任务级
`verified_success`；完整口径见上方 M1-B 汇总。

## DeepSeek 生产 `exec` 编码评测

[`eval-deepseek-exec.py`](../scripts/eval-deepseek-exec.py) 使用真实
`codewhale exec --output-format stream-json` 做显式 baseline/candidate A/B。两个版本必须分别
提供二进制和彼此不同的 revision 标签；Harness 不 checkout、不切分支，也不把旧 Runtime 兼容层带进
候选代码。旧基线若不满足当前 stream/terminal receipt 契约，就如实记为 contract failure。

A/B 在 single 和 multi 两个 lane 内分别比较，不把两类任务混成一个成功率。启动前 Harness
会把 dispatcher 与相邻 `codewhale-tui` 复制为冻结二进制对，并校验 pair digest；同一对
二进制即使使用两个不同 revision 标签也没有 A/B 资格。子进程从环境 allowlist 启动，不能
继承 `DEEPSEEK_TUI_BIN` 等本地覆盖来替换实际被测 Runtime。multi lane 必须恰好启动一个
只读 Explorer，spawn 的 `agent_id` 必须与唯一 canonical `child_started`/`child_finished`
回执匹配；`child_finished` 是 durable handoff 边界，Runtime 在下一次 root 模型请求前完成
eager join。handoff 前工作区不得变化，之后必须由根 Agent 的文件 mutation 工具完成修改。
Shell 测试命令不能冒充修改。候选生产 stream 会在 `child_finished` 中输出脱敏
`artifact_present`；不提供该字段的旧 Runtime 或确实没有产物的 child，会以
`child_artifact_receipt_missing` 让对应 run 的
`verified_success=false`，不得把 completed 状态推断成有效 handoff。该失败样本仍保留在
success/false-success 分母中，不会因能力不足而从 A/B 中被筛掉。每个
`variant × lane` cell 默认独立运行 3 次；少于 3 次、缺 baseline、运行不完整或 usage/cost
不完整时，`product_metric_eligible` 必须为 `false`。计划和单次 run 永远不是产品指标，
只有完整 cell 与同 lane 的 A/B 聚合可以获得资格。

完整 2×2×3 矩阵使用确定性顺序平衡：single 在第 1 次从 baseline 开始，multi 同次从
candidate 开始；随后每个 repetition 交替，最终 6 对运行恰好 3 次 baseline-first、3 次
candidate-first。Plan 列出完整 schedule，每条 run 记录 `pair_order`、`pair_position` 和
`schedule_position`，summary 记录策略与平衡计数，避免缓存预热或时间漂移固定偏向某一版本。

`verified_success` 同时要求 Host 正常接受终态、确定性验收通过且本 lane 的协议/预算契约成立；
错误终态后工作区偶然通过验收，仍按失败样本计入，不能伪装成成功。

测量完整性与任务成功严格分离：合法的 typed `failed`、`timeout`、`budget_exhausted`、
`interrupted/canceled` 终态，以及可判定但未通过的 lane contract，都会作为
`verified_success=false` 进入 cell。只有 terminal/stream、请求账本、usage/cost 对账或必要的
复现实据缺失，才让 cell 失去比较资格。Host 若声明 `completed`，但 process、stream、Error
event、verifier、工作区、prompt、lane、价格、二进制或费用线任一不成立，则计为
`false_success=true`。

先运行完全离线的 Harness 自测；不会读取 Key 或联网：

```bash
python3 scripts/eval-deepseek-exec.py --self-test
```

基线与候选要在两个隔离目录中预先构建。每个公共 dispatcher 旁边都必须放对应版本、实际
承载 Agent loop 的 `codewhale-tui`；Harness 只执行传入的二进制：

```bash
/absolute/eval-bin/baseline/codewhale
/absolute/eval-bin/baseline/codewhale-tui
/absolute/eval-bin/candidate/codewhale
/absolute/eval-bin/candidate/codewhale-tui
```

先检查完整 2×2×3 计划。Dry-run 只检查显式目标与预算，不读取 Key、不联网：

```bash
python3 scripts/eval-deepseek-exec.py \
  --dry-run \
  --baseline-binary /absolute/eval-bin/baseline/codewhale \
  --baseline-revision 352e86a611fdf3cd8bd27c36d24d482c06a71117 \
  --candidate-binary /absolute/eval-bin/candidate/codewhale \
  --candidate-revision "$(git rev-parse HEAD)" \
  --runs-per-cell 3
```

每次 run 都使用同一上限：10 个 API 请求、360 秒 Runtime、32 turns 和 `$0.025`
事后费用验收线。默认 12 次运行，因此计划上限为 120 个请求、`$0.30`；费用线不是请求前
的实时硬停止线。费用聚合使用 Harness 按官方价格快照从 cache-hit/cache-miss/output Token
独立复算的美元成本；Runtime 自报成本单独保存并要求在容差内一致。显式确认后才能运行：

```bash
output="eval/results/m1-c-deepseek-exec-$(git rev-parse --short=8 HEAD)-$(date -u +%Y%m%dT%H%M%SZ).jsonl"
python3 scripts/eval-deepseek-exec.py \
  --acknowledge-cost \
  --key-file key.txt \
  --baseline-binary /absolute/eval-bin/baseline/codewhale \
  --baseline-revision 352e86a611fdf3cd8bd27c36d24d482c06a71117 \
  --candidate-binary /absolute/eval-bin/candidate/codewhale \
  --candidate-revision "$(git rev-parse HEAD)" \
  --runs-per-cell 3 \
  --output "$output"
```

脚本只把 Key 注入隔离后的子进程环境，不放入 argv 或结果；生产 NDJSON 原流只在内存中逐行消费，
不保存模型正文、reasoning、工具输入/输出、完整 stderr 或 Key。JSONL 依次包含逐 run
manifest、每个 cell 的 success/false-success/request/token/time/cost 聚合、single 与 multi
各自的 candidate-minus-baseline 差值，以及最终资格摘要。`--runs-per-cell 1` 或单边目标可
用于便宜诊断，但结果会明确保持 `product_metric_eligible=false`。
传入 `--output` 时，Harness 先写目标目录内的 `0600` 临时文件，完成 `flush`/`fsync` 后再
原子替换正式 JSONL；中断或内部异常不会发布半份长跑结果。未传时仍输出到 stdout。

确定性 verifier 在独立进程中运行；Harness 会绑定 verifier 输入工作区摘要，并在 verifier
退出后再次快照。文件内容、文件类型或 executable mode 任一变化都让证据失效。只有工作区根
`.git`、Python cache 和明确的本地 Runtime state 被忽略；嵌套 `.git`、符号链接及其他新增文件
都属于任务工作区证据。

## 结果边界

- `cross_revision`：测试在导入提交和当前提交都存在，可以做同口径比较；
- `candidate_only`：只说明当前候选实现没有退化，不能宣称相对导入基线有提升；
- 真实 API 任务必须另建显式启用、费用/Token/超时受限的 live suite；
- live suite 必须用预定义验收器判定 `verified_success`，不能用模型自评；
- 任何能力只有在成功率、假成功、Token、耗时和复杂度的净收益成立后才能进入稳定核心。
