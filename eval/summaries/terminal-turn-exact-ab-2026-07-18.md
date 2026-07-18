# 每 Agent 最终请求机制精确 A/B

> 日期：2026-07-18
>
> 状态：机制正确性保留；产品收益未通过，进入重做/缩小
>
> 范围：`0ae9cb7f..8ab0e145`、官方 DeepSeek、single/multi、3/cell

## 1. 结论

`8ab0e145` 的最终请求机制解决了离线状态机中的真实可靠性边界，但本次真实编码 A/B
没有证明 multi lane 获得净产品收益：

- baseline/candidate 的 single 与 multi 均为 `3/3` verified success；
- 12 次运行共 `12/12` verified、0 false success、0 measurement invalid；
- candidate single 的平均请求、Token、时间和费用分别下降 `21.05%`、`26.21%`、
  `20.70%` 和 `21.92%`；
- candidate multi 的平均请求、Token、时间和费用分别上升 `3.70%`、`9.20%`、
  `15.16%` 和 `17.78%`；
- 生产 system prompt stable prefix、工具目录、评测任务、模型、预算和 verifier 均未改变。

因此不能宣称该切片提升了多 Agent 能力或效率。当前决策不是回退已证明的终局可靠性
不变量，也不是继续叠加提示词限制，而是把实现归类为“重做/缩小”：后续必须减少
不必要的 child/root 最终轮成本，并以同一任务重新 A/B；在此之前不得扩大收益结论。

## 2. 身份与边界

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-3a74ea43105642eaa3e70a5f1e902cf8` |
| baseline | `0ae9cb7f68d8172fb18fdb658fbd853a7ac1b791` |
| candidate | `8ab0e14598202bfd82b1a9be71de93e91d03eca9` |
| model | `deepseek-v4-flash` |
| schedule | 12/12；single/multi；3/cell；严格成对平衡 |
| treatment diff | 8 files；`+1,520/-238`，包含生产代码与测试 |
| harness SHA-256 | `3eda4a12c3a5efa0722a9331bb67bdedc208c240883a86a043261bf8b321eb42` |
| fixture SHA-256 | `c97ccfbcdb2d7f56ae97a751520e570249b2e8f5be7eb60685a7eeb0df4f6b48` |
| verifier SHA-256 | `4c12392f0b71533415feeb40a309bbdf134c85ac92e92882bd20df2b21c75886` |
| raw result | `terminal-permit-exact-ab-20260718T010143Z.jsonl`（本地忽略） |
| raw SHA-256 | `771c0ae5cb13a66db2aa2ce026c75781dc2316d02c922db62ac835464bcf83e0` |
| 总费用 | `USD 0.010924458` |

Harness 在运行时记录 `harness_git_dirty=true`，原因是主工作树同时存在与评测文件无关的
Workflow/Provider 纯删除改动。评测脚本与 `eval/fixtures` 在启动前单独验证为无修改；
baseline/candidate 二进制、fixture 和 verifier 均由 Harness 再次冻结并校验 SHA。结果按
现有评测契约仍为 `product_metric_eligible=true`，但此事实必须随证据披露，不能省略。

## 3. 分层结果

| Lane | 版本 | verified | 请求均值 | Token 均值 | 时间均值 | 费用均值 |
|---|---|---:|---:|---:|---:|---:|
| single | baseline | 3/3 | 6.333 | 26,421 | 14,238 ms | USD 0.000579281 |
| single | candidate | 3/3 | 5.000 | 19,495 | 11,291 ms | USD 0.000452280 |
| multi | baseline | 3/3 | 9.000 | 37,938 | 25,518 ms | USD 0.001198398 |
| multi | candidate | 3/3 | 9.333 | 41,428 | 29,387 ms | USD 0.001411527 |

相对变化：

| Lane | success | 请求 | Token | 时间 | 费用 |
|---|---:|---:|---:|---:|---:|
| single | 0 pp | -21.05% | -26.21% | -20.70% | -21.92% |
| multi | 0 pp | +3.70% | +9.20% | +15.16% | +17.78% |

multi candidate 的三次 child 请求数为 `2/3/2`，baseline 为 `2/2/2`。多出的一次 child
请求对应本次 multi 均值回退的主要离散信号；样本只覆盖一个固定任务，不能把它解释成
普遍退化，也不能在没有新实现变化时用更多同质重复调用把负信号冲淡。

## 4. 协议与计量证据

- 两个 lane 的 production system prompt stable prefix SHA 完全相同；
- 两个版本的工具目录 SHA 完全相同；
- 12 次运行均满足 actor accounting、请求预算、终态、任务和 lane contract；
- multi 运行均只启动一个只读 Explorer，child lifecycle、handoff 和写入时序证据完整；
- 12 次运行 usage/cost 完整，0 transport retry，0 cost ceiling violation；
- Key 只进入 child environment，未进入 argv 或结果文件；
- 结果文件权限为 `0600`，位于 Git 忽略目录。

## 5. 决策

1. 保留 `tools=[]` 终局请求、先 join descendants/children、无容量零假 lifecycle、
   advertised catalog 恢复和 compaction 不偷取最终许可等已由离线反例证明的不变量。
2. 不把本次 single lane 的效率改善外推为多 Agent 收益。
3. 不提高总请求预算，不恢复 v3 提示词，也不增加新的提示词限制。
4. 下一实现必须先定位何时已有可接受 artifact、何时仍需额外终局请求，在不建立第二完成
   判定器的前提下缩小额外轮次；若无法简化且 multi A/B 仍回退，应重新评估实现形态。
5. writer worktree 与 durable orchestration 仍按 M6 由真实调用方拉出，不用本次结果提前
   创建 TaskGroup、event v7 或空 `orchestrator` crate。
