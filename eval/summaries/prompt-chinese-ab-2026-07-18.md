# 中文原生生产提示词正式 A/B 证据

> 日期：2026-07-18
>
> 状态：候选拒绝；不是能力提升证据
>
> 范围：生产 system prompt 垂直切片；评测任务提示、模型、工具目录与预算保持一致

## 1. 结论

提交 `b088fd138c6715933e74526a2c65aecd58211789` 的中文原生生产提示词候选不能替代
`0833ab355529ac0b9f0f576ab4bbdd5b1ea5bbb4` 基线。

- 基线 single 与 multi 均为 `6/6` verified success。
- 候选 multi 为 `5/6`；失败运行耗尽 10 次真实模型请求预算，因此是可计量的能力回退。
- 候选 single 的代码和确定性 verifier 实际为 `6/6`，但其中一次 transport retry 没有
  usage，产生 `billing_unknown_attempts=1`，所以该 cell 的成本证据不完整，不能进入产品指标。
- 全部运行 `false_success=0`；生产 system prompt、State schema、RuntimeEvent、root/child
  关系和 request ledger 的取证完整。
- 候选 multi 平均 Token 减少 `12.27%`，但成功率下降 `16.67` 个百分点、请求增加
  `14.58%`、耗时增加 `4.59%`、费用增加 `7.48%`。单项 Token 下降不能抵消成功率回退。

本次不通过不是“中文提示词无价值”的结论，只说明当前合并 treatment 不合格。下一候选必须
针对重复读取、父 Agent 重复调查和完成前额外模型轮次做更小改动，再重新冻结二进制评测。
`b088fd13` 中移除模型可见绝对 workspace 路径的隐私与 stable-prefix 修复不依赖本次能力
结论，继续保留。

## 2. 身份与完整性

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-b42a958dd21d42d29b66da431afc6cf8` |
| harness commit | `5ef02ee7f5f3919e3579dd392194a94be8126136`；dirty=false |
| baseline revision | `0833ab355529ac0b9f0f576ab4bbdd5b1ea5bbb4` |
| candidate revision | `b088fd138c6715933e74526a2c65aecd58211789` |
| model | `deepseek-v4-flash`；reasoning effort `high` |
| schedule | 24/24 runs；6/cell；single + multi；严格成对平衡 |
| tool catalog | 两个变体均为 `sha256:fb862bb833696fe473beda8b14b585a55aa079948b3d411669a488f5587af0d9` |
| raw result | `prompt-ab-formal-0833ab35-vs-b088fd13-r6-20260718-v2.jsonl` |
| raw result SHA-256 | `3174d6f3d267cc81264630b0628df75700abd7f07c7de662cd25591dc430f062` |
| 总费用 | `USD 0.020659066`；低于 `USD 0.60` suite ceiling |

结果文件由 Harness 原子落盘并保持本地忽略，不提交运行日志、临时 workspace 或凭据。Key
只从被忽略的 key file 注入子进程环境，未进入 argv、结果或本摘要。

## 3. Cell 结果

| Cell | 资格 | 成功 | 请求 mean / median | Token mean / median | 时间 mean / median | 费用 mean / median |
|---|---|---:|---:|---:|---:|---:|
| baseline single | eligible | 6/6 | 4.167 / 4 | 20,727.83 / 19,544.5 | 10,878.17 / 10,562 ms | 0.000444504 / 0.000428299 |
| candidate single | ineligible | 5/6 指标；任务 6/6 | 5.167 / 4.5 | 20,324.17 / 16,148.5 | 12,705.33 / 11,920 ms | 0.000581812 / 0.000537485 |
| baseline multi | eligible | 6/6 | 8 / 8 | 42,125.67 / 42,036.5 | 25,248.67 / 24,978 ms | 0.001164866 / 0.001158727 |
| candidate multi | eligible | 5/6 | 9.167 / 9 | 36,956.67 / 36,825.5 | 26,408.67 / 25,732.5 ms | 0.001251996 / 0.001294182 |

候选 single 的原始 delta 因 cell ineligible 不能作为产品结论。仅作诊断时，它相对基线为：
请求 `+24.00%`、Token `-1.95%`、耗时 `+16.80%`、费用 `+30.89%`。

## 4. 提示词与协议取证

- 24/24 run 的 production system prompt evidence 完整，错误码为 0。
- State schema 为 v9，RuntimeEvent writer 为 v5。
- baseline stable prefix 在 12 次运行中固定为
  `sha256:8d1d157b06a55e7d85349ec96dfd67150afe2578fd2a74dbfa5257bd192c2aef`，
  10,080 bytes。
- candidate stable prefix 在 12 次运行中固定为
  `sha256:a48a3cac7a236f4364b9ace0384fffb4d24d3ccbe1edd0ad9a955528e4ec7fdc`，
  5,067 bytes。
- single 的固定评测任务 prompt hash 在两个变体均为
  `sha256:461ec5858d944511f0362a3ebdabcf8ef842bc3ae954db4927829072f69d8194`；
  multi 均为
  `sha256:3074d37303674f7e43270b620a45ff5184fe96a852ca7a0206c23094972035b8`。
- multi 的 root/child 均共享所属变体的稳定前缀；没有把子 Agent 的额外角色块错误计入
  stable prefix。

因此本次比较确实测到了生产提示词切片，而不是 temp workspace、任务提示或工具目录漂移。

## 5. 两条异常

`candidate:single:5` 首次请求失败后按同一 prepared request 重试成功，进程退出 0、终态
completed、代码任务与 verifier 均通过。5 次物理请求只有 4 条 usage，导致费用无法闭合，
`measurement_invalid=true`。这是计量不完整，不是代码任务失败。

`candidate:multi:4` 为真实失败：root 6 次、child 4 次请求后触发
`llm_api_request_budget_exhausted`，进程退出 1、终态 failed。最终 workspace verifier
虽然通过，但 child 没有形成被父运行接受的完整结果回执，TaskContract shape 与完成声明
均未通过。Host 正确拒绝把“文件看起来已改好”误算为 verified success。

## 6. 决策

按 [评测规范](../../docs/product/EVALUATION.md)：

1. 两个 lane 都必须有完整可比较证据；
2. 候选 verified success 不得低于基线；
3. 关键效率改善不能以另一 lane 的明显退化换取。

当前候选同时违反前两项，结论为 **拒绝**。不为补齐单 Agent 的一次未知计费而重复花费，
因为 eligible 的 multi lane 已经足以否决该候选。下一轮必须先改变实现，再运行新的有界
成对 A/B；不得把本轮 Token 降幅写成已获得的产品能力。
