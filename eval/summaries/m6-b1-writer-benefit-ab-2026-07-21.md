# M6-B1 single / isolated Writer 正式 DeepSeek A/B

> 日期：2026-07-21
>
> 记录类别：`formal_same_binary_treatment_ab`
>
> 产品指标资格：`true`
>
> 决策：`reject_and_rework`
>
> M6-B2：不准入

## 1. 结论

本轮完成预注册的 18 对、36 个正式 arms，0 无效尝试、0 unknown billing、0
transport/runtime retry。结果不是 Harness 中止或协议误判，可以正式计入产品决策。

Writer 将 verified success 从 `2/18` 提高到 `4/18`，但同时出现：

- 7 次 Writer false-success，违反必须为 0 的硬门槛；
- 1 次 Writer treatment 根调用禁止的 may-write 工具（`run_verifiers`，未实际编辑）；
- 7 次 `recovery_required` 与保留 worktree/branch 状态；
- 总 Token `+35.495%`、费用 `+52.452%`、时间 `+39.765%`；
- 只有 1 对 single 与 Writer 同时成功，无法形成有效效率样本；
- T1 control 的 Token、费用、时间开销均超过预注册上限；
- T3 Writer `0/6` verified，6/6 false-success，并用满每 arm 10 请求。

因此不能选择 `keep_default`、`shrink_on_demand` 或 `hold_mechanism`。唯一符合预注册规则的
决策是 `reject_and_rework`：保留已证明可工作的单 Writer 隔离机制，但不得默认调度，
不得开发双 Writer、DAG 或第二 scheduler；先修 verifier 卫生、actor 权限和
TaskContract evidence，再重新冻结 M6-B1。

## 2. 冻结身份

| 轴 | 身份 |
|---|---|
| candidate revision | `5d72ae94b794b022a916151e1741c5b52de60edb` |
| version | `codewhale 0.8.68 (5d72ae94b794)` |
| binary SHA-256 | `95a8768a4b41d121bc7c6b954e8310c2dd21d8ef940d7daeed95d73f1a039aeb` |
| result schema | `codewhale.eval.m6-writer-benefit.v2` |
| plan schema | `codewhale.eval.m6-writer-benefit-plan.v2` |
| manifest SHA-256 | `3c0f0893c2e906ddcf7408af95ad9880e960cbebe80193ab56116bd94fc85b10` |
| manifest content SHA-256 | `414fd3ab38376d99c8ad181f5bb32c0d39fb8bc5febd4994a30718b6a67ee9be` |
| Harness SHA-256 | `3d595730fd3c2e0452d488b1de466f417b0001683747e75c0ad2523f277654b9` |
| schedule SHA-256 | `f2c6e0c0eed0da65921abb45379423d7c2064a4570ec3391c0ad806348aa97ed` |
| model / surface | `deepseek-v4-flash` / Standard Chat |
| raw result SHA-256 | `cd9c81bf07e3e45a1f4fd9f631544a36d0c8ac4b0e08b75701b30e6eef1de8b5` |

原始结果为本地 Git 忽略文件
`eval/results/m6-b1-writer-benefit-ab.json`，权限 `0600`。它不保存 prompt、模型文本、
reasoning、工具参数/输出、stderr、临时路径或 Key。任务、fixture tree、task definition、
verifier 文件和 verifier spec 的冻结 SHA-256 均与 v2 manifest 一致。

## 3. Cell 结果

`requests` 是 root+child physical started；`tokens` 是 input+output；以下开销是每 arm
中位数。

| Cell | verified | false-success | requests | tokens | cost USD | time s |
|---|---:|---:|---:|---:|---:|---:|
| T1 single | 0/6 | 6 | 6 | 33,422.5 | 0.002293354 | 30.593 |
| T1 Writer | 2/6 | 1 | 9.5 | 43,475.5 | 0.002752280 | 39.786 |
| T2 single | 2/6 | 3 | 7 | 47,571.5 | 0.002989997 | 39.669 |
| T2 Writer | 2/6 | 0 | 9 | 53,242 | 0.003627764 | 41.458 |
| T3 single | 0/6 | 6 | 6 | 28,767 | 0.001486663 | 21.980 |
| T3 Writer | 0/6 | 6 | 10 | 55,583.5 | 0.003919356 | 44.965 |

总计：

- single：2/18 verified、15 false-success、113 请求、683,567 tokens、
  USD `0.041742440`、538.468 秒；
- Writer：4/18 verified、7 false-success、172 请求、926,199 tokens、
  USD `0.063637391`、752.590 秒；
- suite：285 请求、1,489,265 input、120,501 output、997,504 cache-hit、
  491,761 cache-miss、59,500 reasoning、136,139 reasoning-replay tokens；
- suite 费用 USD `0.105379831` / CNY `0.752713080`，耗时 1,310.078 秒。

Writer 总量相对 single：

| 范围 | Requests | Tokens | Cost | Time |
|---|---:|---:|---:|---:|
| 全部 | +52.2% | +35.5% | +52.5% | +39.8% |
| T1 | +58.3% | +25.8% | +34.1% | +32.5% |
| T2 | +27.9% | +6.4% | +13.6% | +10.8% |
| T3 | +76.5% | +100.9% | +165.2% | +98.2% |
| T2+T3 | +49.4% | +39.9% | +61.0% | +43.4% |

## 4. 逐 pair

`V` 为 verified success，`FS` 为 false-success，`RR` 为
`recovery_required`，`B` 为 blocked。最后一列依次为 Writer 相对 single 的
requests / tokens / cost / time。

| Pair | Single | Writer | Writer − single |
|---|---|---|---|
| T1-1 | FS | V | +66.7% / +2.8% / -11.4% / -6.0% |
| T2-1 | FS | RR | +12.5% / -14.1% / -6.2% / -16.4% |
| T3-1 | FS | FS | +100.0% / +157.4% / +223.6% / +127.8% |
| T2-2 | B | V | +0.0% / -24.6% / -24.9% / -26.2% |
| T3-2 | FS | FS | +66.7% / +78.2% / +116.7% / +40.8% |
| T1-2 | FS | RR | +80.0% / +20.9% / -15.8% / +10.8% |
| T3-3 | FS | FS | +150.0% / +197.2% / +243.1% / +160.2% |
| T1-3 | FS | FS | +66.7% / +65.1% / +118.4% / +65.3% |
| T2-3 | FS | RR | +50.0% / +41.7% / +64.2% / +74.7% |
| T1-4 | FS | V | +25.0% / -14.5% / +5.1% / +1.0% |
| T2-4 | FS | RR | +12.5% / -8.0% / +14.9% / +29.9% |
| T3-4 | FS | FS | +66.7% / +72.2% / +115.2% / +101.3% |
| T2-5 | V | RR | +60.0% / +82.2% / +77.4% / +83.7% |
| T3-5 | FS | FS | +66.7% / +95.3% / +183.6% / +133.6% |
| T1-5 | FS | RR | +50.0% / +38.4% / +64.4% / +69.4% |
| T3-6 | FS | FS | +42.9% / +57.5% / +137.4% / +59.9% |
| T1-6 | FS | RR | +80.0% / +87.5% / +107.0% / +106.2% |
| T2-6 | V | V | +66.7% / +31.7% / +29.2% / -6.2% |

成功方向为 Writer 独赢 3 对、single 独赢 1 对、双成功 1 对、双失败 13 对。
唯一双成功的 T2-6 中，Writer 时间只改善 6.2%，未达到 15%；Token 和费用分别回退
31.7% 与 29.2%，均超过 20%。T1/T2/T3 双成功样本分别为 0/1/0，远低于每任务至少 3
对的效率证据门槛。

## 5. 计量、协议和生命周期审计

36/36 arms 均满足：

- 同 candidate revision、binary、model、Standard Chat surface 和冻结 schedule；
- root 恰好一个、位于末尾且 sequence 一致的 canonical Terminal；
- Terminal accounting、RunView accounting 完全一致，root `sealed=true`；
- Writer child 存在时，child Terminal/RunView/sequence 完全一致；
- root+child ModelResponseCommitted usage、response count、physical request 和 surface
  cost 与 terminal ledger 闭合；
- started=completed、in-flight=0、records-after-seal=0、sealed-denied=0；
- 0 unknown billing、0 unpriced、0 transport retry、0 runtime retry；
- actual advertised tool-name catalog 与 treatment 一致，非空 definition hash 在全部请求中
  稳定；空目录只允许在 terminal request；
- 0 measurement-invalid、0 Harness 安全 finding、0 invalid attempt。

Catalog evidence 的边界是：single root/Writer child 的非空 definition hash 始终为
`ae0eeaf5…44c7a`，Writer root 始终为 `ef9a4c86…aff4`，但 v2 manifest 只预注册了工具
名字/顺序，没有预注册完整 JSON Schema hash；因此本结果证明同二进制运行内稳定，不能单独
证明 schema 相对计划从未漂移。raw 也按隐私设计只保存 event 摘要，事后不能从该 JSON
重放完整 canonical event stream。

成功的 11 次 Writer integration 全部是唯一 fast-forward commit，0 integration failure；
4/4 verified Writer 都有绑定最新根 revision 的 EvidenceReceipt。正式 suite 结束后没有
M6-B arm 临时目录、`codewhale/writer/*` branch 或进程残留。仓库里其他历史/并行开发
worktree 不属于本轮，未被清理或改动。

## 6. 真实失败归因

### 6.1 verifier 会污染任务工作区

T1 single 的 6 次失败和 T2 single 的 3 次 false-success / 1 次 blocked 均出现
`__pycache__/*.cpython-314.pyc`，使 changed-files/path-scope 失败。其中 T1 的 6 次与
T2 的 3 次 false-success 最终 exact verifier 仍是绿，Host receipt 也接受完成；T2-2
则是 blocked、exact verifier 失败且没有 receipt。这说明当前 `run_verifiers` 的默认/
辅助检查可能在被测 workspace 生成产物，而完成门禁没有把任务范围和工作区洁净度作为
canonical evidence。

Writer 的隔离 worktree 能阻止这些产物直接进入根仓，但没有解决 verifier 卫生本身。
6 次 child 已完成且 seal prepared 的 Writer 在 seal 前停止，另 1 次 child blocked；
7 次均进入 typed `recovery_required` 并保留 worktree/branch。保留是避免丢失不确定
artifact 的 fail-closed 行为，不是后台进程失控，但它仍违反本轮“不留临时 Git 状态”的
产品硬门槛。该模式与 single 的 verifier 生成物强相关，但 raw 只从
`AgentSealCommitted` 提取 Writer changed files；这 7 次没有 seal commit，所以不能从
正式结果证明它们具体产生了哪个越界文件。下一 Harness 必须在 seal rejection 前保存脱敏
的 changed-files/scope 摘要。

### 6.2 T3 的最终绿不等于真实恢复

T3 的 single/Writer 12/12 最终 exact verifier 都通过，但冻结的 evidence audit 全部为：

```text
call_count=2
exact_frozen_parameters=false
verdicts=[null, passed]
failed_then_passed=false
```

即模型修改前确实调用了 verifier，修改后也得到通过，但第一次调用没有形成精确、可验证的
失败事实。生产 TaskContract 只看最终 receipt，仍接受 `Completed`，所以 12 次全部成为
false-success。问题不是 DeepSeek Beta/Standard Chat 误路由，也不是 transport；它是
模型被要求重新拼装复杂 verifier 参数，以及 Host completion 没有表达时序 evidence。

### 6.3 root 权限仍依赖提示词

Writer treatment 为了让 child 获得写工具，root request 仍广告同一父 catalog，并在提示词
中要求根不直接调用 may-write 工具。T1-3 的 root 调用了 `run_verifiers`；它未直接编辑
文件，但按冻结 treatment 的 may-write 分类属于 actor-policy violation。Actor 权限
必须由 Host admission 强制，不能只靠中文提示词。

### 6.4 多 Agent 不是主要 admission 或 integration 问题

18/18 Writer assignment 均合法且成功创建隔离 worktree；11/11 已进入 integration 的
Writer 全部成功 fast-forward 并清理。失败集中在 verifier、seal 前 evidence、root
capability 和 completion acceptance，而不是需要增加 Writer 数量、DAG 或新 scheduler。

## 7. 预注册门槛

| 门槛 | 结果 |
|---|---|
| 18 pairs / 36 arms / 6 per cell | 通过 |
| accounting 完整、unknown=false | 通过 |
| Writer false-success=0 | **失败：7** |
| 无 root may-write 调用、范围/集成/临时 Git 异常 | **失败：1 root may-write 调用、7 retained recovery** |
| 每任务 Writer success 不低于 single | 通过：T1 0→2、T2 2→2、T3 0→0 |
| 成功 Writer 最新根 EvidenceReceipt | 通过：4/4 |
| 可靠性收益且 Token/费用开销 ≤25% | **失败：+35.5% / +52.5%** |
| T2/T3 效率收益 | **失败：样本不足且唯一双成功不达标** |
| T1 control | **失败：Token +25.8%、费用 +34.1%、时间 +32.5%** |

## 8. 代码复杂度

从 Goal 起始检查点 `39f5902c` 到正式 candidate `5d72ae94`：

- `crates/*/src` 增加 243 行、删除 16 行，生产净增 227 行；
- Rust tests 增加 595 行、删除 29 行，测试净增 566 行；
- 其余主要增量位于本地 eval Harness、冻结 manifest 和三个 fixture，不进入生产 Runtime。

生产增量只修复真实运行暴露的 transport retry 配置、child wall deadline、terminal
convergence、Writer recovery billing 和 State v15 terminal accounting projection；没有
新增 crate、Provider、模型路径、工具、Runtime、Store、scheduler 或兼容桥。State v15 是
一次 canonical event 到物化 snapshot 的确定性修复，不保留长期双 schema 分支。

本轮没有为了改善 A/B 结果增加提示词层、模式或 Agent 数量。正式失败后的 verifier、
capability 和 evidence 改造留给独立 rework slice，不混入已经冻结的 candidate。

## 9. 诊断尝试与费用

正式 v2 样本没有复用任何旧 arm。正式前的 v1/旧 revision 运行只用于暴露并修复：
child deadline、terminal convergence、fixture Git identity、cancel conflict accounting、
Writer recovery、State terminal accounting projection 和 Harness sealed provenance。

| 诊断结果 | 可见费用下界 USD | 说明 |
|---|---:|---|
| `aborted-e72e4c60` | 0.006070456 | 另有 unknown exposure |
| `aborted-42a36f06` | 0.007226481 | 旧 evidence 边界 |
| `aborted-6e91db68` | 0.001560541 | 另有 unknown exposure |
| `aborted-be4e9e39` | 0.004408768 | cancel-conflict 计量 |
| `diagnostic-join` | 0.005060698 | terminal convergence |
| `diagnostic-terminal-error-e16d7dae.partial` | 0.001599718 | `sealed=false`，只作下界 |
| `aborted-b6434bd3-unsealed` | 0.033284294 | 9 arms 全部 `sealed=false` |

诊断可见费用下界为 USD `0.059210956`；加正式 suite 后，本 Goal 可见费用下界为
USD `0.164590787`，另有两次旧诊断的未知暴露。旧 raw 均保持 `0600`、Git ignored，
不进入正式 18 对。

## 10. 下一切片

下一阶段不是 M6-B2，而是 **M6-B1 rework**，按 owner 拆成最小垂直切片：

1. `crates/tools`：verifier 必须无工作区副作用；Python cache 指向仓库外或强制禁写，
   并以 clean-workspace 回归证明。
2. `app/runtime`：完成 Writer 默认关闭、显式 opt-in 的 admission cutover。当前默认
   `RunLimits` / tool policy 仍会暴露 `agent` 路径，尚未交付这项正式决策。
3. `runtime/orchestrator`：root 与 Writer child 使用同一 catalog owner，但 Host 按 actor
   强制 capability；Writer treatment 的 root 写调用在执行前 typed deny。
4. `protocol/runtime`：TaskContract 绑定 named frozen verifier，模型只引用 verifier ID，
   不重新拼装完整参数；EvidenceReceipt 可表达“修改前失败、修改后通过”的最小有序事实。
5. `orchestrator`：seal rejection 和 child blocked 产生可诊断 reason；确定无副作用时清理，
   只有 artifact 状态确实不确定时保留恢复，不新增第二恢复状态机。
6. `eval`：冻结完整 tool definition hash，并在 seal 未提交时保存脱敏 scope 摘要。
7. 完成离线回归后重新冻结同三任务 M6-B1；仍使用一个 Writer、一个 Orchestrator、
   一个 Runtime、一个 Store。第 2 项 cutover 完成前不得宣称“不默认 admission”；
   未重新通过正式 A/B 前不做双 Writer。
