# M7-D RuntimeEvent v16 编辑失败观测闭环

## 1. 结论

- production 基线：`61597f59adb520e91bf5e6c32c94ffcf4e70aa91`
  （M7-C post-audit clean checkpoint）。
- v16-native projector checkpoint：`7ddf3bba60a0d6e0d33bd3752f18d3bc67927626`。
- failure-truth / final Harness checkpoint：
  `cf9b3fd6248ef07a4333e60f940c8e63ba7852f5`。
- 协议保持 Run API v10、RuntimeEvent v16、State v21、exec-stream v2。
- production Rust、工具目录、Runtime、RunStore、DeepSeek transport 和客户端均未修改。
- 产品决策：**keep v16-native offline observer；shrink/delete no-delta live
  Harness；hold all editor/FIM treatments**。
- 正式模型 arms 0、credential read false、official API requests 0、
  `product_metric_eligible=false`。

M7-D 的真实主要损失不是已经证明的 patch/FIM 能力不足，而是观测器本身会降级 schema、
错认恢复、隐藏 transaction ambiguity，并在 gate 失败时给出假 `pass`。本切片关闭了这些
评测正确性缺陷，但没有获得新的 production 模型失败样本，因此没有准入任何编辑机制。

## 2. 起始 WIP 审计

起始 WIP `5bb9b577` 只有一个 manifest 和一个单变体 Harness，没有 production Rust
变化。只读审计确认以下问题：

| 边界 | 起始行为 | 风险 | M7-D cutover |
|---|---|---|---|
| RuntimeEvent | 把 v16 envelope 的 `schema_version` 改成 14 后调用历史 validator | v16 的失败码和 lifecycle 要求未被验证 | 只接受 v16 |
| lifecycle identity | 按 `call_id` 配对 | call id 可跨 assistant turn 重用 | 只按 `operation_id` |
| Started | 不投影 `ToolExecutionStarted` | 无法区分 preflight、已开始和 crash window | Prepared/Started/Outcome 三段投影 |
| workspace revision | 读取 `ToolOutcome.workspace_revision` | 不是 canonical Runtime workspace state | 只读事件级 `workspace_state.revision` |
| recovery | 任意后续成功都算恢复 | 其他路径、其他工具、同批调用会误算 | 同 run、同工具、同结构化目标且经过新模型请求 |
| patch header target | 由 Harness 隐式猜测 | 会复制第二 patch parser | 无结构化 path 时标记 `unscorable` |
| transaction | 只识别 `side_effect_ambiguous` failure code | 漏掉其他 indeterminate outcome 和 Started 无 Outcome | 两类事实分别计数 |
| rerun/output | 任意 results JSON，可 replace | 改文件名即可绕过 `maximum_reruns=0` | manifest 唯一文件名、O_EXCL、不可覆盖 |
| credential | 无 treatment delta 仍提供 `live` 并读取 Key | 无法形成归因收益，异常前后计费事实不完整 | `live`、Key、cost acknowledgement 全删除 |
| executor | import 5,540 行历史 M7-A executor 并全局 monkeypatch | 历史 frozen Harness 与当前行为耦合 | 完全移除 import/monkeypatch |
| self-test | 依赖已删除 binary，只测一个 patch_parse synthetic | clean checkout 无法复核主要边界 | 15 项独立 regression |
| gate result | nested self-test `status=pass` 可覆盖 aggregate failure | CLI 会在退出 1 时打印 `pass` | aggregate `passed` 优先 |
| failure diagnostics | 失败 gate 只存 stdout/stderr hash | 无法知道失败 test | 非零 gate 在 0600 raw 中保存每流最多 64 KiB tail |

## 3. 冻结观测合同

唯一 owner 仍是 `eval`；Harness 不执行或模拟编辑器：

1. 输入是 AgentRuntime/RunStore 已拥有的 canonical `StoredRuntimeEvent` ledger。
2. 每个 edit lifecycle 必须有唯一 `ToolPrepared(operation_id)`；Started 和 Outcome
   只能绑定同一个 operation。
3. 成功 outcome 不得有 `failure_code`；所有不成功 outcome 必须有 v16 稳定 code。
4. preflight 失败允许没有 Started，但成功或声称 side effect 的 outcome 必须有 Started。
5. revision 只来自 `ToolOutcomeCommitted.workspace_state.revision`。
6. `edit_file` 目标取 parsed `path`；`apply_patch` 目标取 parsed `path` 或排序后的
   `changes[].path`。只有目标可解析时才评价 recovery。
7. recovery 必须同 run、同工具、同目标，且失败 Outcome 与后续成功 Prepared 之间至少有
   一个 `ModelRequestPrepared`。同一 model response batch 的后续成功不是模型恢复。
8. committed `side_effect=indeterminate` 与 MayWrite Started 无 Outcome 都计入
   transaction ambiguity；normal run 不产生 crash 频率结论。
9. failure bucket 必须跨至少 2 个任务累计至少 3 个 typed failure 才能准入
   patch-generation 或 edit-recovery 候选；无关的 unrecovered failure 不得替代该门槛。

`scripts/test-eval-m7d-edit-observation.py` 冻结 15 个反例，覆盖 schema、operation identity、
failure code、preflight、revision、同批/跨目标/跨工具恢复、未知 target、indeterminate
side effect、Started 无 Outcome、decision threshold、唯一 output、无 credential surface、
aggregate status 和有界失败诊断。

## 4. 离线证据

### 4.1 首个 suite：保留失败

clean `7ddf3bba` 的首个一次性 suite：

- suite：`m7-d-edit-observation-v1-61597f59`
- repository before/after：
  `7ddf3bba60a0d6e0d33bd3752f18d3bc67927626` /
  tree `d9fbd3908c462d0fbb7ae2239a4d9aa1dfc927fb`，dirty false。
- 13/14 gates 通过；`workspace-test` exit 101，wall time 236,002 ms。
- result SHA-256：
  `4e0324e62afca27ebf44bf5fc47f986faeb149a520ab28eacf740f7e1a1cc18e`。
- 文件 mode 0600；credential false；API requests 0。

该 suite 正确返回 exit 1，但 CLI 错误打印 `status=pass`。由于当时只保存失败输出 hash，
无法从记录判断具体失败 test。随后不改源码直接执行同一个
`cargo test --workspace --locked` 完整通过，因此只能记录为“首个 suite 的 workspace gate
失败未能复现”，不能猜测根因或把它静默改写为通过。

### 4.2 v2 suite：clean pass

`cf9b3fd6` 修复 status 和失败诊断后冻结新 suite；没有覆盖或续跑首个 suite：

- suite：`m7-d-edit-observation-v1-61597f59-v2`
- manifest SHA-256：
  `04c3b514bc2afb75705d4a491e99652e4ebd946df52ad020d94811aa9bea3dd6`
- Harness SHA-256：
  `dd2d05e6caee045509022ee923add8387b6a084f2353e530f8bd967f2d8a6128`
- regression SHA-256：
  `513f6f4708a02edd87a96d73068dfe198c5c63574574da6b00bd673eb4d9a423`
- repository before/after：
  `cf9b3fd6248ef07a4333e60f940c8e63ba7852f5` /
  tree `f48fe5e5e4a2a8aec391fdeaba2c7463e0e1f5f1`，dirty false。
- 14/14 gates 通过，总 wall time 390,759 ms。
- result SHA-256：
  `72c251575d8da43e97310b2a9b094ea4b1198c1e31729c6892f91d84e1b12a5e`。
- 文件 mode 0600；credential false；API requests 0。

门禁包括：

- 15 项 projector/Harness regression；
- 18 个 M7-C tools contract assertions 和 production app loopback；
- root/read-only unsafe-replay conformance；
- explicit Writer 25 项完整 lifecycle suite；
- ToolPrepared、ToolExecutionStarted、ToolOutcomeCommitted 三个 process SIGKILL/reopen；
- app-server SIGKILL/reopen；
- `./scripts/dev-deepseek-agent.sh focused`；
- fmt、workspace clippy `-D warnings`、workspace test、`git diff --check`。

## 5. 复杂度与删除

与 `5bb9b577` WIP 相比：

- active Harness 从 766 行到 803 行，净增加 37 行；
- manifest 从 159 行到 268 行，增加 109 行明确合同；
- 新增 327 行 regression；
- `crates/*` production 增量为 0。

增加部分全部用于 v16 lifecycle、失败可诊断性和 regression。以下高风险路径已物理删除：

- `v16_as_v14`、历史 executor import 与三处全局 monkeypatch；
- stale release binary identity 与已删除 target 依赖；
- `live`、schedule、Key read、cost acknowledgement、单变体 arm execution；
- 任意 results 文件名、replace 写入和从旧 outcome revision 推断 workspace。

因此代码总量增加 473 行 eval-only contract/test，但没有增加产品工具、Runtime、Store、
surface、用户模式或模型循环。

## 6. 产品取舍

- **keep**：v16-native projector、operation-id lifecycle、保守 recovery、transaction
  ambiguity、一次性 0600 offline record。
- **shrink/delete**：单变体 live Harness、credential/cost 路径、历史 executor adapter。
- **hold**：FIM、另一种编辑策略、多文件 durable transaction 实现。
- **reject as evidence**：把首个失败改写为通过、把诊断复跑拼入原 suite、把 normal run
  当 crash 频率、把 offline correctness 当模型收益。

本 Goal 定位并关闭的主要损失是 **evaluation truth**。它没有证明当前 production 的主要
产品损失仍在编辑生成，也没有新的频率证据支持 FIM 或 transaction state。故下一开发切片
不得以“需要样本”为理由执行无 treatment 的付费单变体；应先提出一个由现有真实反例支持、
能在同一 binary 明确改变行为的最小 production candidate，再冻结 paired A/B。若没有这种
delta，编辑路线保持关闭，转向已有证据更强的非编辑损失。

## 7. 官方协议边界

M7-D 删除了 transient model、价格和 API 执行字段，不新增 DeepSeek wire claim，也没有
调用官方 API。FIM、Completions、Thinking/Tool Calls 的一手资料已在 M7-C 于
2026-07-23 复核：

- [FIM Completion](https://api-docs.deepseek.com/guides/fim_completion/)
- [Completions API](https://api-docs.deepseek.com/api/create-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

这些易变事实不由 M7-D manifest 复制；后续若产生真实 surface delta，必须重新复核并冻结
当时 fixture。

## 8. 非结论

M7-D 不证明：

- 当前 DeepSeek 首次编辑成功率、恢复率、verifier success、Token、wall time 或费用改善；
- FIM 优于、等于或劣于 `apply_patch`/`edit_file`；
- normal run 中存在或不存在多文件 crash；
- 首个 workspace-test 失败的具体根因；
- `maximum_reruns=0` 的 offline suite 可以替代每 variant × task 至少 3 次的正式模型 A/B。
