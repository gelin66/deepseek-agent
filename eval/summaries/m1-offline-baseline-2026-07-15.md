# M1-A 离线契约基线（2026-07-15）

> 状态：M1-A 已完成；M1 总里程碑仍在进行中。

这份汇总只回答一个问题：在同一评测器和同一清单下，导入提交与当前提交各自保留了
哪些可重复的离线契约。它不把测试通过解释为 Agent 编码能力提升。

## 1. 可复核结果

| 本地结果（默认不提交） | 被测提交 | Harness 提交 | Manifest blob | JSONL SHA-256 | 范围 | 结果 | 记录耗时 |
|---|---|---|---|---|---|---|---|
| `eval/results/m1-offline-current-366e8b5b.jsonl` | `366e8b5b37bedbbf3b1ebb326b14a70e885a57a2` | `366e8b5b37bedbbf3b1ebb326b14a70e885a57a2` | `678c8e30d471e356cb93c47781c87b0c8624c26d` | `c43de6064e23927848a7270a139d0de97e04ed0c5c77018996596331f6141034` | `all` | 41/41 | 88 s |
| `eval/results/m1-offline-imported-352e86a6-h366e8b5b.jsonl` | `352e86a611fdf3cd8bd27c36d24d482c06a71117` | `366e8b5b37bedbbf3b1ebb326b14a70e885a57a2` | `678c8e30d471e356cb93c47781c87b0c8624c26d` | `73fc2b5a86b531dc3f3ec46967f7e74f1d313a52869e80acda714b71c36fec6f` | `cross-revision` | 12/12 | 323 s |

清单为 [m1-offline.tsv](../manifests/m1-offline.tsv)，评测入口为
[`scripts/eval-m1.sh`](../../scripts/eval-m1.sh)。导入结果只运行 12 个两边都存在的
`cross_revision` 用例；它不是与当前 41 项对称的完整套件。当前结果中的另外 29 项均为
`candidate_only`，只能说明候选提交的对应契约通过，不能用于声称相对导入提交有提升。

## 2. 证据层级

| 证据层级 | 当前数量 | 可比较数量 | 能证明什么 | 不能证明什么 |
|---|---:|---:|---|---|
| `full-runtime-offline` | 6 | 6 | 注入模型或本地 mock 驱动的生产 Engine/工具链契约保持工作 | 真实 DeepSeek 智能、线上协议质量、真实任务成功率 |
| `runtime-contract` | 17 | 6 | 终态、恢复、上下文、多 Agent 预算/worktree 等确定性契约 | 模型是否能自主完成真实编码任务 |
| `protocol-unit` | 13 | 0 | DeepSeek 路由、Strict/FIM、SSE、reasoning replay、usage 的局部契约 | 官方 API 端到端兼容与线上稳定性 |
| `critic-plumbing` | 4 | 0 | `verify` 模型评审工具的接线与结果归一化 | critic 能发现真实缺陷或可充当完成证据 |
| `config-contract` | 1 | 0 | 本地 DeepSeek 配置约定 | Agent 能力或可用性已经提升 |

因此，`41/41 current` 与 `12/12 imported` 的严谨结论是：已登记的离线契约在各自范围
内全部通过，且 12 个可比较契约没有发生回归。两边都是 pass；`pass/pass` 没有能力增量，
更不能替代真实任务 A/B。

88 秒和 323 秒也不可用于性能比较。整套耗时包含 Cargo 构建/缓存、测试进程启动和主机
调度，且两边执行的用例数不同；它不是受控、重复的 Agent 任务延迟。

## 3. 生产工具目录测量

在干净提交 `366e8b5b` 上运行 [`scripts/measure-tool-catalog.py`](../../scripts/measure-tool-catalog.py)，
由真实生产 Engine turn 捕获同一请求阶段的完整目录与模型可见目录：

| 目录 | 工具数 | 序列化 JSON 字节 | 粗略 Token 估算 |
|---|---:|---:|---:|
| 完整生产目录 | 92 | 78,483 | 19,621 |
| 当前模型可见目录 | 26 | 29,465 | 7,367 |

模型可见目录的序列化体积比完整目录低 62.46%。这里的 Token 只是
`ceil(serialized_json_bytes / 4)` 的确定性估算，`server_usage_measured=false`；它证明按需暴露
已进入真实 Engine 请求，不证明服务端实际节省同等 Token，也不证明任务能力提升。目录内容
还会受编译能力和运行环境影响，后续对比必须固定相同环境与 revision。

## 4. 当前没有的证据

M1-A 离线基线本身没有调用真实 DeepSeek API，因此没有可归因的：

- 输入、输出、reasoning、cache hit/miss Token 与 API 成本；
- 真实仓库任务的 `verified_task_success`、false-success 和失败分类；
- 同任务、同验收器、同预算下的导入提交与候选提交 A/B；
- 可比较的单任务墙钟时间或多 Agent 相对单 Agent 的净收益。

Standard Chat、Thinking tool replay、`/beta` Strict Chat 与 `/beta` FIM 的线上协议证据已由
[M1-B DeepSeek live canary](m1-b-deepseek-live-2026-07-15.md) 单独记录。该证据同样不代表
真实编码能力。

## 5. 决策

1. 将这对结果冻结为 **M1-A 离线契约基线**；M1-A 完成，但 M1 不完成。
2. 保留 12 个跨提交契约，包括现有多 Agent 契约，作为后续重构的防回归底线。
3. 29 个 `candidate_only` 用例覆盖的 WIP 仍是候选能力，不能因测试通过自动进入稳定核心。
4. `verify` 继续作为独立实验；在真实缺陷检出率和误报率得到证据前，不成为完成门禁。
5. DeepSeek Strict schema 不兼容时，只允许 Strict 准备降级；普通工具调用能力必须保留。

## 6. 后续切片

### M1-B：有上限的 DeepSeek live canary（已完成）

- 官方线上 5 个请求全部通过，费用、请求数与超时均有硬上限；
- 已验证 Standard、Thinking tool call + exact replay、Beta Strict 与 Beta FIM；
- 结果为协议 canary，`verified_success=null`，不进入产品能力指标。

### M1-C：固定真实编码任务基线

- 选择小而有代表性的仓库任务，冻结输入仓库 revision、任务说明、预算和确定性验收器；
- 用同一执行器分别运行导入提交与当前候选，记录 `verified_task_success`、false-success、
  输入/输出/cache Token、墙钟时间、费用、工具失败和最终 diff；
- 多次运行或明确标记单次结果为探索性，只有净收益稳定后才接受能力升级。

### M1-D：现有 WIP 处置

根据 M1-B/M1-C 证据，对 DeepSeek 协议、Agent 可靠性、`verify` 和本地配置四个切片逐项
给出保留、重做、缩小或删除结论。之后才进入 M2 的协议与 Backend 重构。
