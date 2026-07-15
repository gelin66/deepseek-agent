# M1-A 离线契约基线（2026-07-15）

> 状态：M1-A 已完成；M1 总里程碑仍在进行中。

这份汇总只回答一个问题：在同一评测器和同一清单下，导入提交与当前提交各自保留了
哪些可重复的离线契约。它不把测试通过解释为 Agent 编码能力提升。

## 1. 可复核结果

| 本地结果（默认不提交） | 被测提交 | Harness 提交 | Manifest blob | JSONL SHA-256 | 范围 | 结果 | 记录耗时 |
|---|---|---|---|---|---|---|---|
| `eval/results/m1-offline-current-880da451.jsonl` | `880da451ff8617f16a72ec5e5dcebdf608ba7113` | `880da451ff8617f16a72ec5e5dcebdf608ba7113` | `678c8e30d471e356cb93c47781c87b0c8624c26d` | `d1b05f8e0f23e52e36fb45a6e64b7e48bfb9b1013539765104567cfe58bdd3cd` | `all` | 41/41 | 72 s |
| `eval/results/m1-offline-imported-352e86a6-h880da451.jsonl` | `352e86a611fdf3cd8bd27c36d24d482c06a71117` | `880da451ff8617f16a72ec5e5dcebdf608ba7113` | `678c8e30d471e356cb93c47781c87b0c8624c26d` | `70f8b414f58ce1b302b3c9bcb5a04e26472fee062a49b48822c9f84b9e41152f` | `cross-revision` | 12/12 | 57 s |

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

72 秒和 57 秒也不可用于性能比较。整套耗时包含 Cargo 构建/缓存、测试进程启动和主机
调度，且两边执行的用例数不同；它不是受控、重复的 Agent 任务延迟。

## 3. 当前没有的证据

本轮没有调用真实 DeepSeek API，因此没有可归因的：

- Standard Chat、`/beta` Strict Chat 与 `/beta` FIM 线上请求证据；
- 输入、输出、reasoning、cache hit/miss Token 与 API 成本；
- 真实仓库任务的 `verified_task_success`、false-success 和失败分类；
- 同任务、同验收器、同预算下的导入提交与候选提交 A/B；
- 可比较的单任务墙钟时间或多 Agent 相对单 Agent 的净收益。

## 4. 决策

1. 将这对结果冻结为 **M1-A 离线契约基线**；M1-A 完成，但 M1 不完成。
2. 保留 12 个跨提交契约，包括现有多 Agent 契约，作为后续重构的防回归底线。
3. 29 个 `candidate_only` 用例覆盖的 WIP 仍是候选能力，不能因测试通过自动进入稳定核心。
4. `verify` 继续作为独立实验；在真实缺陷检出率和误报率得到证据前，不成为完成门禁。
5. DeepSeek Strict schema 不兼容时，只允许 Strict 准备降级；普通工具调用能力必须保留。

## 5. 下一切片

### M1-B：有上限的 DeepSeek live canary

- 分开验证 `ApiSurface::StandardChat`、`ApiSurface::StrictChat` 和 `ApiSurface::Fim`；
- 覆盖普通工具调用、全 strict 函数、strict 不兼容回退、reasoning/tool replay、SSE 终止、
  usage/cache 字段和错误分类；
- 预先固定请求次数、Token、费用和超时上限，原始响应脱敏留证；
- 输出协议成功率和每种 surface 的 Token/费用，不宣称真实编码能力。

### M1-C：固定真实编码任务基线

- 选择小而有代表性的仓库任务，冻结输入仓库 revision、任务说明、预算和确定性验收器；
- 用同一执行器分别运行导入提交与当前候选，记录 `verified_task_success`、false-success、
  输入/输出/cache Token、墙钟时间、费用、工具失败和最终 diff；
- 多次运行或明确标记单次结果为探索性，只有净收益稳定后才接受能力升级。

### M1-D：现有 WIP 处置

根据 M1-B/M1-C 证据，对 DeepSeek 协议、Agent 可靠性、`verify` 和本地配置四个切片逐项
给出保留、重做、缩小或删除结论。之后才进入 M2 的协议与 Backend 重构。
