# 中文生产提示词收敛 canary 证据

> 日期：2026-07-18
>
> 状态：v2、v3 均拒绝；停止继续叠加提示词限制
>
> 范围：同一生产编码任务、`deepseek-v4-flash`、single/multi、3/cell

## 1. 结论

两次小规模 canary 都没有达到扩大为 6/cell 正式 A/B 的门槛。

- v2 在固定五阶段清单后增加停止、父子去重和 child 结果轮提醒，但请求没有下降，总 Token
  几乎不变，并有两次失败请求缺少计费数据。该 WIP 已从主线删除。
- v3 从 `b088fd13` 出发，只把固定五阶段清单替换为事实缺口循环。它让 single 稳定为
  4 次请求，也让 multi root 稳定为 6 次请求，证明删除固定清单的方向有真实信号。
- v3 的只读 child 却有两次用满 4 次请求；一次没有形成 Host 可接受的最终 handoff，另一次
  在修改前即失败。candidate multi 因此只有 `1/3` verified success，而 baseline 为 `3/3`。
- 成功率是硬门槛，不能用 single 的效率提升抵消 multi 的可靠性回退。v3 不合并，也不继续
  用 v4 文案限制补救。下一步应由 `AgentRuntime` 保证 child 为最终产物保留可执行机会，再
  以独立 treatment 评测。

当前继续保留 `b088fd13` 的简体中文产品契约和 workspace path 隐私修复，但不把它们声明为
已经取得的 Agent 能力提升。

## 2. v2：叠加停止规则

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-0da1fa59ba684ae0a0d0799827ef8bb3` |
| baseline | `b088fd138c6715933e74526a2c65aecd58211789` |
| candidate | `aaec2108eac77cba07efa3d65fabd5c64e1f1321` |
| schedule | 12/12；3/cell；严格成对平衡 |
| raw result | `prompt-v2-canary-b088fd13-vs-aaec2108-r3-20260718.jsonl` |
| raw SHA-256 | `49e15817b30ed960d8f406e161b2d4bc2eecf8df2d9c927e1690659498a0ac16` |

| Lane | 版本 | verifier | 产品计量 | 请求 | Token |
|---|---|---:|---:|---:|---:|
| single | baseline | 3/3 | 3/3 | `4/6/5`，mean 5.000 | 62,060 |
| single | candidate | 3/3 | 2/3 | `5/6/6`，mean 5.667 | 66,994 |
| multi | baseline | 3/3 | 3/3 | `9/9/10`，mean 9.333 | 121,782 |
| multi | candidate | 3/3 | 2/3 | `10/9/10`，mean 9.667 | 116,911 |

两个 candidate cell 各有一次 Runtime 模型请求重试缺少对应 usage，均为
`billing_unknown_attempts=1`，所以产品计量不完整。即使只把它们作为诊断样本，v2 仍比
同轮 baseline 多 3 次请求；single Token 增加 `7.95%`，multi Token 减少 `4.00%`，合计
仅增加 63 Token，没有可区分于波动的净收益。

## 3. v3：删除固定五阶段清单

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-8873558e361844fea061533f33c0ca5f` |
| baseline | `b088fd138c6715933e74526a2c65aecd58211789` |
| candidate | `ccedb35311adfa9307c34196461a57e30d4261f2` |
| schedule | 12/12；3/cell；严格成对平衡 |
| evidence | 完整；0 measurement invalid；0 false success |
| raw result | `prompt-v3-canary-b088fd13-vs-ccedb353-r3-20260718.jsonl` |
| raw SHA-256 | `11920544abf631fa71a20178c403e74eedf301da8052ca83226129bf547837bd` |
| 总费用 | `USD 0.010508428`；低于 `USD 0.30` suite ceiling |

| Lane | 版本 | verified | 请求 | actor 请求 | Token | 平均时间 | 费用 |
|---|---|---:|---:|---:|---:|---:|---:|
| single | baseline | 3/3 | `6/4/5`，mean 5.000 | root `6/4/5` | 61,969 | 12,953 ms | 0.0016201304 |
| single | candidate | 3/3 | `4/4/4`，mean 4.000 | root `4/4/4` | 44,675 | 9,684 ms | 0.0016356424 |
| multi | baseline | 3/3 | `8/8/9`，mean 8.333 | root `6/6/7`；child `2/2/2` | 102,642 | 22,018 ms | 0.0034048056 |
| multi | candidate | 1/3 | `10/8/10`，mean 9.333 | root `6/6/6`；child `4/2/4` | 107,192 | 21,877 ms | 0.0038478496 |

v3 single 的请求减少 `20.00%`、Token 减少 `27.91%`、平均时间减少 `25.24%`，但费用
没有下降。multi root 的平均请求从 6.333 降为 6.000，但 child 从 2.000 增为 3.333，
导致总请求增加 `12.00%`、Token 增加 `4.43%`、费用增加 `13.01%`，并出现两次预算终止。

`candidate:multi:1` 的 workspace verifier 通过，但 child 4 次请求后没有留下完整 handoff，
Host 正确拒绝完成。`candidate:multi:3` 在修改前用满预算且发生两次工具失败，workspace
verifier 也失败。这不是计量噪声。

## 4. 取证边界

- 两轮均由相同 Harness commit、fixture、verifier、任务提示、模型、工具目录和预算执行。
- 所有 24 次 run 的 production system prompt 取证完整；State schema 为 v9，
  RuntimeEvent schema 为 v5。
- 结果文件、冻结二进制、临时 workspace 和 key file 均保持本地忽略，没有提交。
- `status=completed` 和 `product_metric_eligible=true` 只表示 v3 证据完整可比较，不表示候选
  通过产品门槛；保留结论仍由 verified success 和预设效率门槛决定。

## 5. 决策

1. 删除未通过的 v2 WIP，不合并隔离的 v3。
2. 不继续增加“不得重复”“必须停止”等提示词限制。
3. 保留 v3 暴露出的设计事实：root 的固定清单可减少，但 child 最终产物保障必须进入
   Runtime 机制。
4. 后续机制切片必须先用 conformance 证明 child 工具轮不会吞掉最终 handoff 机会，再做
   同任务、同预算 A/B；不得通过增加总请求预算掩盖问题。
