# 子 Agent eager join 精确 A/B

> 日期：2026-07-18
>
> 状态：保留小型 Runtime 优化；不外推为普遍多 Agent 收益
>
> 范围：`f9dddd5d..528a72f2`、官方 DeepSeek、single/multi、6/cell

## 1. 结论

`528a72f2` 把同一轮 `agent` 工具启动的 child 在下一次 root 模型请求前完成 join，删除
父 Agent 等待 handoff 时没有新信息的一轮模型调用。相对相邻基线 `f9dddd5d` 的 24-run
精确 A/B 支持保留这个小型机制：

- 四个 cell 均为 `6/6` verified success，合计 `24/24` verified、0 false success、
  0 measurement invalid；
- 12 次 multi 运行均完整出现唯一 canonical `child_started`/`child_finished`，child 产物
  可证明，handoff 前 workspace 不变、root 无工具调用和写入，handoff 后 root 均完成真实
  mutation；
- candidate multi 的请求均值从 `8.667` 降至 `7.833`（`-9.62%`），Token 从
  `35,442.5` 降至 `32,510.0`（`-8.27%`），费用从 `USD 0.001302042` 降至
  `USD 0.001145924`（`-11.99%`）；
- candidate multi 的平均时间只从 `28,812.3 ms` 变为 `28,790.8 ms`（`-0.07%`），应视为
  持平，不能声称提速；
- 成对观察中，candidate multi 的请求为 4 对更少、2 对相同、0 对更多；Token 为 4 对更少、
  2 对更多，但时间和费用都只有 2 对更低、4 对更高。均值费用下降受少数大幅下降样本影响，
  不能外推为稳定的成本优势。

因此保留的是“有 child 时先取得 durable handoff，再让 root 继续”的直接机制和已测到的
请求削减，不是新的完成判定器、提示词限制或第二套调度器。本结果只覆盖一个固定 Python
任务和一个只读 Explorer；它不证明多 Agent 在更广任务集上提高成功率，也不关闭
writer-worktree Orchestrator、compaction A/B 或 M4 完整集成门禁。

## 2. 身份、预算与资格

| 项目 | 值 |
|---|---|
| evaluation id | `deepseek-exec-ab-5f1415652f654b1e884e00c809f66589` |
| baseline | `f9dddd5db252e9c19e8865abb67d491fa59c1079` |
| candidate | `528a72f2d59494a997150e691cdbc9fedade113c` |
| treatment diff | 2 files；`+345/-186`；生产代码 `+6/-64`，其余为 conformance tests |
| model | `deepseek-v4-flash`；reasoning effort `high` |
| task | `python-coalesce-ranges-v1` |
| schedule | 24/24；single/multi；6/cell；12 对严格平衡，baseline-first/candidate-first 各 6 |
| 每 run 预算 | 10 API 请求；360 秒；32 turns；`USD 0.025` 事后费用线 |
| suite 预算 | 最多 240 请求；`USD 0.60` 费用线 |
| harness SHA-256 | `3eda4a12c3a5efa0722a9331bb67bdedc208c240883a86a043261bf8b321eb42` |
| fixture SHA-256 | `c97ccfbcdb2d7f56ae97a751520e570249b2e8f5be7eb60685a7eeb0df4f6b48` |
| verifier SHA-256 | `4c12392f0b71533415feeb40a309bbdf134c85ac92e92882bd20df2b21c75886` |
| baseline binary pair SHA-256 | `d32e5e033b570faee0c7312629de09c0d785765901e00ed27ece28ab9bebed38` |
| candidate binary pair SHA-256 | `fc386ce431c691ab0d49e5bddb5bdf76153d84189deaa99bcc364aeac7acd649` |
| raw result | `eager-join-exact-ab-20260718T020753Z.jsonl`（本地忽略，权限 `0600`） |
| raw SHA-256 | `b89bada15af198125242ba0200f1f120ac0d4ed4128a9e5fe5ea29e4b226c8dd` |
| 总费用 | `USD 0.020018667` |
| 产品指标资格 | 四个 cell 与两个 lane comparison 均为 `product_metric_eligible=true` |

两个版本的生产 system prompt stable-prefix SHA、工具目录 SHA、任务 prompt、模型和预算
分别一致；变更只位于 Runtime child join 时序及其 conformance tests。Harness 记录
`harness_git_dirty=true`，因为主工作树在隔离构建和运行期间有与 treatment 无关的并行
开发；两个目标来自独立 worktree，24/24 run 均通过 frozen binary pair 与 frozen fixture
校验。该事实不取消当前 Harness 资格，但必须披露。

Key 只从被忽略的 key file 注入 child environment，未进入 argv、结果或本摘要。24/24 run
均具备完整 usage/cost、actor accounting、stream、budget、TaskContract、production prompt
和 verifier 证据，0 transport retry、0 cost ceiling violation。

## 3. 四个 cell

| Lane | 版本 | verified | 请求 mean / median | Token mean / median | 时间 mean / median | 费用 mean / median |
|---|---|---:|---:|---:|---:|---:|
| single | baseline | 6/6 | 4.667 / 4.5 | 17,939.83 / 17,129.0 | 14,751 / 11,994 ms | 0.000455267 / 0.000429618 |
| single | candidate | 6/6 | 4.500 / 4.0 | 17,636.33 / 15,362.5 | 12,986 / 11,344 ms | 0.000433211 / 0.000390816 |
| multi | baseline | 6/6 | 8.667 / 8.5 | 35,442.50 / 34,370.0 | 28,812 / 26,500 ms | 0.001302042 / 0.001155375 |
| multi | candidate | 6/6 | 7.833 / 8.0 | 32,510.00 / 33,509.5 | 28,791 / 29,556.5 ms | 0.001145924 / 0.001159990 |

聚合相对变化：

| Lane | success | 请求 | Token | 时间 | 费用 |
|---|---:|---:|---:|---:|---:|
| single | 0 pp | -3.57% | -1.69% | -11.96% | -4.84% |
| multi | 0 pp | -9.62% | -8.27% | -0.07% | -11.99% |

single lane 不启动 child，candidate 中的 eager join 分支不会生效；它的差异只能作为同一
测试运行中的方差观察，不能归因给 treatment。multi 的 root 请求从
`7/6/6/6/7/6` 变为 `6/5/6/6/6/5`，child 请求从 `2/3/2/2/3/2` 变为
`2/3/2/2/2/2`。这与删除父 Agent 空等轮的方向一致，但样本不足以把 child 的一次减少也
归因为实现。

## 4. Multi lifecycle 与 handoff

12/12 multi run 同时满足：

- 恰好一次成功的只读 Explorer spawn；
- 恰好一个匹配的 `child_started` 和 `child_finished`；
- child lifecycle 顺序、root/child depth 和 spawn/receipt 身份一致；
- `child_finished` 携带可证明的 completed artifact；
- handoff 前 root 工具调用为 0、写入为 0，workspace snapshot 保持不变；
- handoff 后至少一次 root mutation，确定性 verifier 通过。

这证明 live 任务中的 canonical eager handoff 没有靠 root 预先工作、child 直接写入或
completed 状态推断来制造成功。它仍不替代离线 conformance 对嵌套 child、多工具轮、恢复和
最终请求许可边界的证明。

## 5. 决策

1. 保留 `528a72f2` 的 eager join 实现；它在不增加生产抽象的情况下净删 58 行生产代码。
2. 保留原有每 Agent 最终请求许可、`tools=[]` final、无容量零假 lifecycle 和 advertised
   catalog 恢复不变量；本切片只删除 root 等待 handoff 的无信息请求。
3. 不把单任务的 Token/费用均值下降宣传为普遍多 Agent 优势，尤其不声称 wall time 改善。
4. 后续扩展任务集时继续分层记录 root/child 请求、pair 分布和 lifecycle/handoff；若请求
   削减不复现或成功率回退，再重新评估，不靠提高预算或堆提示词掩盖。
5. M4 仍需完整 workspace 门禁和剩余旧编译岛清理；M6 的 writer worktree/Integrator 能力
   不由本次只读 Explorer 任务提前判定完成。
