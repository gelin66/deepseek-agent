# DSE 评测规范

> 文档类别：产品权威。仅定义能力的验证与保留门槛。

- 状态：V1 评测契约
- 上次更新：2026-07-28

本文件决定一项能力是否真正提升产品。它不是排行榜，也不以“模型回答看起来不错”
作为结论。

<a id="evaluation-stable-rules"></a>
## 1. 评测目标

北极星指标：

```text
已验证任务成功率 / Token / 时间 / 代码复杂度
```

评测回答四个问题：

1. 任务是否真实完成？
2. Agent 是否错误地宣称完成？
3. 成功所需成本和时间是否合理？
4. 新能力带来的维护复杂度是否值得？

## 2. 基线

必须保留至少三条可重复基线：

1. 导入时的 CodeWhale `352e86a6` + 官方 DeepSeek。
2. 当前稳定单 Agent Runtime。
3. 当前候选实现。

候选若改变多 Agent、RepoGraph、FIM、Strict 或 compaction，还必须有关闭该能力的 A/B
对照。没有 material treatment 时不得为满足清单读取 Key；应记录 inadmissibility 或作
明确产品范围决策。

## 3. 任务集

固定任务按难度和能力分组，仓库 fixture 必须可重置且不依赖未保存的本地状态。

### A. 协议正确性

- 普通 Chat；
- thinking + tool calls；
- reasoning/tool history replay；
- Strict 全兼容；
- Strict 部分不兼容后的普通工具回退；
- FIM 正常和超限；
- SSE 半帧、多帧、畸形帧和结束帧；
- retry、rate limit、timeout；
- `length`、过滤和资源不足终止；
- cache usage 和 cost。

### B. 基础编码

- 单文件缺陷；
- 小型功能；
- 精确搜索与读取；
- patch 失败恢复；
- 编译错误修复；
- 单元测试修复；
- 文档和配置修改。

### C. 仓库理解

- 跨文件调用定位；
- 配置到运行时影响追踪；
- 接口实现定位；
- 测试影响分析；
- 大仓库固定 Token 预算；
- RepoGraph 与浅层 project map 对照。

### D. 长任务和恢复

- context compaction 后保持目标、约束、未决问题、当前 diff 和最新 evidence；
- continuation 创建新 root、source 不变、lineage 正确，且与同 run resume 明确分离；
- continuation 继承 projection 后仍能完成预定义验收；
- compaction 前后 canonical transcript 的条目、顺序和规范化 digest 保持不变，只有 model-visible
  projection 改变；
- 多次工具循环；
- model request 在途 steer，必须先 queued，旧响应和完整工具/子 Agent 结果提交后才 applied；
- approval 的 approve/deny/cancel 与 approval 后、工具开始前的恢复；
- user input 的 submit/cancel、错误 response 类型与恢复重放；
- 相同 command id 同 payload 幂等、不同 payload 拒绝；
- cancel/interrupt 及 control-requested 到 terminal 之间的崩溃；
- 进程被终止后的 resume；
- snapshot/replay 一致性；
- 不重复提交终态和写工具。

### E. 验证与假成功

- 测试命令失败；
- 工具函数返回但业务操作失败；
- 修改后使用旧证据；
- reasoning-only/空响应；
- step budget 耗尽；
- Runtime 逻辑模型请求预算耗尽与 DeepSeek 物理 API admission 拒绝必须使用不同 typed
  failure；前者不得虚报物理预算耗尽；
- 项目没有标准测试命令；
- 分析/文档任务不应被机械测试门卡死。

### F. 多 Agent

- 并行只读调查；
- Explorer + Implementer；
- Implementer + Verifier；
- 两个独立 worktree writer；
- 文件范围冲突；
- Agent 失败、超时、取消和恢复；
- Integrator review/merge；
- 每个 root/child 保留一个最终 artifact/integration 请求；
- child/descendant 在父终局请求前完成 join；
- 最终请求实际工具目录必须为空，恢复按该次请求实际 advertised catalog 判定；
- 无最终请求容量时不得提交假的 child lifecycle；
- hard-limit 本地 compaction 不调用模型，因此不得消耗最后的最终请求许可；
- 单 Agent 与多 Agent 净收益对照。

### G. DSE 中英文产品契约

双语 UI 是产品验收，不作为 Agent 能力提升的 A/B treatment。候选至少验证：

- 首次启动、API Key、信任、canonical 命令、审批、取消、失败、恢复和终态完整支持
  `en` 与 `zh-Hans`；
- 两个 catalog 的 message key 和 placeholder 完全一致，缺失翻译不能静默 fallback；
- 80/120 列终端同时覆盖英文换行和 CJK 宽度、截断、鼠标/键盘命中；
- 一个进程只有一个解析后的 `ProductLanguage`；不存在 per-Run/per-Agent locale、系统语言
  持续检测或模型语言分类；
- 不存在 `/translate`、在线翻译或思考内容后处理产生的额外模型请求；
- JSON/NDJSON、Schema、命令、工具名、模型 ID、配置键、稳定错误码、路径、代码、diff 与
  原始输出保持机器契约或原始字节；
- 改变 UI locale 不得改变 model/reasoning、route、工具目录、预算、RequestPlan 或
  canonical RuntimeEvent/RunStore 事实；
- root、read-only child、explicit Writer、resume/recovery 和 headless 人类投影使用同一
  进程级语言；
- 被删除命令的 PTY 验收应证明两种语言下都在配置或模型请求前稳定拒绝，不能恢复旧兼容。

生产 Agent prompt 的语言比较属于独立 treatment，必须按本文件和 ADR-0010 记录同任务
verified success、false success、安全拒绝、请求预算耗尽、请求数、Token、时间和成本；
UI 双语验收不能替代该 A/B。

### H. 工程完全体纵向任务

ADR-0018 后，任务集必须覆盖完整真实工程工作，而不只覆盖单工具机制：

- 多语言跨文件理解、编辑、build、test、static check、service start、bounded logs/debug、
  diff/review/integrate 与 latest-revision completion；
- root、read-only child、isolated Writer 的真实分工、返工、失败收敛、crash/reopen 和最终合并；
- known URL fetch，以及 unknown question 的 search→source selection→read→cross-check→citation；
- JS application 的 navigate、observe、press/wait/scroll/select/text editing/back/tab/multi-page；
- public reversible action 的 exact preview/approval/receipt，以及未授权/destructive/financial/
  publish/security-sensitive action 的正确拒绝；
- managed isolated session、Host credential、Cookie/storage 清理、workspace-scoped upload 与 isolated
  download；
- DeepSeek 官方 multimodal contract 可用后，DOM/AX 不足任务的 selective visual observation；
- 跨代码、应用、Web 和 Writer worktree 的 integration/dogfood/release workflow。

明显属于上述 baseline 的 capability cluster 可以由已接受产品目标和冻结端到端任务直接准入，
不要求先人为制造两次 missing-tool failure。一个 cluster 至少冻结三个彼此独立的真实任务，并
覆盖受影响风险层、真实 production caller、latest-revision evidence、reopen/recovery 和旧路删除。
若没有第二个可行 treatment，确定性 capability delivery 不为满足形式做随机 A/B；若要声明
DeepSeek 选择/成功率的普遍提升、默认策略收益或效率优势，仍必须执行同模型 held-out/private
task 评测和适用的 A/B。

## 4. 运行变体

根据能力阶段选择，不要求每次运行全部组合：

```text
single-agent
single-agent + RepoGraph
single-agent + candidate edit protocol
explorers + implementer
implementer + verifier
worktree writers + integrator
strict on/off
FIM on/off
compaction strategy A/B（仅评测 Harness/build treatment，不是产品开关）
```

模型、API surface、reasoning 参数、上下文预算、工具目录摘要和代码提交必须记录，
确保结果可解释。

## 5. 指标

每次运行至少记录：

```text
run_id
task_id
git_commit
workspace_revision
model
api_surface
interactive
auto_approve
trust_mode
sandbox_posture
purpose
continued_from_run_id
source_run_id
context_projection_sha256
context_before_tokens
context_after_tokens
verified_success
terminal_state
false_success
wall_time_ms
input_tokens
output_tokens
cache_hit_tokens
api_cost
model_turns
tool_calls
tool_failures
patch_failures
verification_runs
test_regressions
resume_success
worktree_conflicts
human_confirmation_count
human_attention_ms
rework_cycles
recovery_actions
external_side_effects
side_effect_receipts
authoritative_sources_opened
sources_cross_checked
unsupported_claims
local_browser_time_ms
changed_files
evidence
```

这里的 `purpose` 是评测 Harness 自有的任务/lane 元数据，不是 production `RunPurpose`；
后者已从当前协议删除。

物理 API 请求预算的 `budget_exhausted` 只表示确实观察到 admission rejection，必须严格
等价于 `exhausted_denied > 0`。`started == limit` 只表示额度已经全部使用，不表示发生了
拒绝。Runtime 在 ModelPort 前拒绝第 N+1 个逻辑模型请求时，终态 error code 必须是
`runtime_model_request_budget_exhausted`，物理 accounting 仍为未耗尽；只有物理 admission
拒绝才使用 `llm_api_request_budget_exhausted`。评测 Harness 必须校验该交叉关系，不能用
泛化的“预算失败”掩盖错误归因。

复杂度另行记录：

- 新增生产代码；
- 删除生产代码；
- 新增状态类型；
- 新增持久化真相；
- 新增模型可见工具；
- 新增长期依赖；
- 新增运行路径。

新增第二个状态真相或第二个 Agent loop 默认视为架构失败，不由成功率小幅提升抵消。

### 5.1 A/B 与产品指标资格

用于声称产品能力提升的 A/B 必须满足：

- 代码 revision A/B 必须显式提供 baseline/candidate 二进制及彼此不同的 revision，不由
  Harness 自动 checkout；当前同一二进制内已有能力的 on/off treatment A/B 则必须反向
  要求同一个 revision、同一个不可变 binary SHA，并只允许预注册的 treatment 差异。后者
  只能声称当前 treatment 相对对照 treatment 的效果，不能声称代码 revision 获得提升；
- baseline 与 candidate 使用相同任务、模型、评测任务提示词、请求/turn/时间和费用预算；
- 若要把变化归因给某一个组件，该组件之外的生产系统提示词、工具目录和其他能力面必须
  保持一致；若评测的是一个有意同时替换 Runtime、生产提示词或工具目录的垂直切片，必须
  记录各自 hash 和 treatment 差异，结果只能归因给整个切片，不能归因给其中单个组件；
- single 与 multi 等不同运行形态分层统计，不用混合均值掩盖某一层退化；
- 每个 `variant × task/lane` cell 至少独立运行 3 次；
- 逐 run 保存 `verified_success`、`false_success`、request、Token、时间和费用；
- `verified_success` 必须同时满足 Host 接受终态、TaskContract 预先定义的验收成立与该运行
  形态的协议/预算契约；
- cell 聚合保存样本数、总量、均值/中位数与成功率，同 lane 再计算 candidate-baseline
  差值；
- usage/cost 不完整、cell 未跑完、缺 baseline 或样本少于 3 次时，
  `product_metric_eligible=false`。

这里的 A/B 约束用于有可比较 treatment 的模型行为、默认策略、优化和产品级泛化，不把
“当前 catalog 根本没有一项已接受 baseline capability”变成必须先付费证明的前置条件。确定性
capability cluster 可以报告 exact task-family 的 before=`cannot complete`、after=`verified complete`
行为结果；它必须同时报告 false success、安全反例、人工确认、recovery、复杂度与删除量，但在
没有同模型 held-out/private matrix 和完整 accounting 时不得外推普遍成功率、Token、费用、速度
或竞品优势。

### 5.2 行为真相与 accounting 真相

自 ADR-0011 起，正式 Harness 必须从同一 canonical observation 分别派生
`behavior_status` 与 `accounting_status`：

```text
behavior_status:
  verified_success
  correct_safety_rejection
  verified_product_failure
  measurement_interruption
  invalid

accounting_status:
  complete
  usage_incomplete
  billing_unknown
  unpriced
```

行为标签要求冻结 identity/task、workspace outcome、external verifier、production
terminal、latest-revision Host receipt、route/lane 与 observer 全部闭合。typed production
failure/blocked 且缺最新 receipt 是 `verified_product_failure`；Harness、机器或基础设施
中断且没有 production outcome 才是 `measurement_interruption`；任何 evaluator、
identity、environment、workspace、route、evidence 或 observer 歧义都是 `invalid`。

accounting 标签只来自 physical request/usage/pricing/seal ledger。非 `complete` 状态继续
停止下一付费 request，不能进入 Token、费用、效率或完整 utility aggregate，也不能把未知
费用记为零。行为 aggregate 可独立保留闭合的前三种 behavior status，但
`measurement_interruption`/`invalid` 不得进入。production behavior keep 仍要求 false success 为零、
预注册质量矩阵和安全/恢复闭合；accounting 不完整时可以保留这些确定性行为结论，但必须停止
下一付费请求，且不能形成 cost/Token/efficiency、完整 utility 或产品级泛化结论。

旧 `product_metric_eligible=false` 仍表示不具备完整产品 utility/cost 结论，不再表示同一
observation 必然没有行为真相。`maximum_reruns=0`、不补 mate、不选择性续跑、不拼接旧 raw
保持不变。

compaction 的协议、replay、lineage 或恢复测试通过，只证明机制可用，不能证明产品收益。
M5-B 已完成同任务、同模型、同预算、同工具面和每 cell 至少 3 次的 compaction on/off
A/B。结果只支持保留 hard-limit 可靠性：不得声称它降低成本、缩短时间或提高成功率。
TaskContract、当前 workspace/evidence 和未解决 verifier failure 的保留必须由预定义断言
或 verifier 验证，不能由模型自评。

计划记录和单次 run 本身永远不能标为产品指标。旧基线只作为隔离黑盒运行；若旧版本不
满足当前生产 receipt 契约，应记录 contract failure，不得向候选 Runtime 或 Harness
加入旧语义兼容层。

#### 2026-07-18 中文原生生产提示词 treatment

首个正式候选 `b088fd13` 相对 `0833ab35` 的 24-run、single/multi、6/cell 官方 DeepSeek
A/B 已完成，但 **没有通过保留门槛**：

- baseline single/multi 均为 6/6；
- candidate multi 为 5/6，一次运行真实耗尽 10 次模型请求预算；
- candidate single 的代码任务和 verifier 为 6/6，但一次失败重试缺少 usage，使该 cell
  `measurement_invalid`；
- 0 false success，production system prompt 每次请求均已由 State schema v9 /
  RuntimeEvent v5 取证；
- candidate multi 虽减少 12.27% Token，但成功率下降 16.67 个百分点，同时请求、耗时和
  费用上升。

因此当前中文提示词候选不得宣称提升，也不因 Token 单项下降默认启用。完整身份、cell
统计、hash、费用与异常边界见
[中文原生生产提示词正式 A/B](../../eval/summaries/prompt-chinese-ab-2026-07-18.md)。
下一候选必须先改变实现，再重新运行同任务成对 A/B；只补跑计量失效的单 Agent run 不能
推翻 multi lane 的真实回退。

随后两个 3/cell 收敛 canary 也均被拒绝：

- v2 叠加停止、父子去重和 child 结果轮提醒后，请求不降、合计 Token 基本不变，且两个
  candidate cell 各有一次未知计费，主线 WIP 已删除；
- v3 只删除固定五阶段清单，single 请求从 mean 5.000 降为 4.000，multi root 从
  mean 6.333 降为 6.000；但 child 从 mean 2.000 升为 3.333，造成两次预算终止，
  candidate multi 仅 `1/3`，因此不得合并。

完整 canary 身份、hash、费用和逐 actor 结果见
[中文生产提示词收敛 canary](../../eval/summaries/prompt-convergence-canaries-2026-07-18.md)。
该结果把下一问题收窄为 Runtime 的 child 最终产物保障；不得再靠增加提示词限制或提高总请求
预算掩盖。

提交 `8ab0e145` 已实现 Runtime 机制修复：每个 root/child 预留可退还的最终请求许可，
descendant/child 先 join，最后请求固定 `tools=[]`；无容量时不产生 child lifecycle，
prepared/replay 使用该请求实际 advertised catalog，自动 compaction 在硬限制内不能偷取
最后许可。State schema v10 持久化该目录身份，RuntimeEvent 保持 v6。

该机制相对 `0ae9cb7f` 的 12-run、single/multi、3/cell 精确 A/B 已完成：四个 cell 均
`3/3` verified，0 false success，0 measurement invalid；candidate single 的平均 Token、
时间和费用分别下降 `26.21%`、`20.70%` 和 `21.92%`，但 candidate multi 在成功率不变时
分别上升 `9.20%`、`15.16%` 和 `17.78%`，请求均值也上升 `3.70%`。因此当前结论为
**机制正确性保留，产品收益未通过，进入重做/缩小**；不得用 single 改善掩盖 multi 回退，
也不得恢复 v3 提示词或提高请求预算。完整身份、hash、逐 actor 和费用证据见
[每 Agent 最终请求机制精确 A/B](../../eval/summaries/terminal-turn-exact-ab-2026-07-18.md)。

后续 `528a72f2` 没有增加完成判定或提示词约束，而是把同一轮 `agent` 工具启动的 child
在下一次 root 模型请求前 eager join，删除父 Agent 等待 handoff 时没有新信息的一轮请求。
相对相邻基线 `f9dddd5d` 的官方 DeepSeek 24-run、single/multi、6/cell 精确 A/B 为：

- 四个 cell 均 `6/6` verified，合计 0 false success、0 measurement invalid；
- 12/12 multi run 的 canonical `child_started`/`child_finished`、artifact、workspace 不变
  handoff 和 handoff 后 root mutation 契约完整；
- candidate multi 的请求、Token 和费用均值分别下降 `9.62%`、`8.27%` 和 `11.99%`，
  verified success 不变；平均时间只下降 `0.07%`，视为持平；
- 成对样本中 multi 请求为 4 对下降、2 对相同；但时间和费用都只有 2 对下降、4 对上升，
  不能把受少数样本影响的均值外推为稳定提速或成本优势；
- single 不启动 child，观察差异不归因给该 treatment。

因此保留这个净删 58 行生产代码的 eager-join 小型 Runtime 机制，同时把结论限制为本固定
任务已证明的请求削减和 lifecycle/handoff 不回归；它不证明广泛的多 Agent 成功率、Token、
成本或时间提升。完整 revision、binary-pair SHA、四 cell、pair 分布、费用和限制见
[子 Agent eager join 精确 A/B](../../eval/summaries/eager-join-exact-ab-2026-07-18.md)。

### 5.2 TaskContract、终态与证据边界

`TaskContract` 是 Host 在一次 generation 开始前确定的验收边界，至少绑定 objective、
constraints、non-goals 和 acceptance。该 generation 内不可由模型改写、补写或追认；
目标或约束改变时必须产生新的 generation，旧 receipt 不得沿用。

acceptance 只有两类产品语义：

1. **显式 verifier 契约**：预先固定 verifier 标识与精确参数。只有同时匹配 contract
   generation、objective/constraints/non-goals、verifier 标识、精确参数和最新
   `workspace_revision` 的成功 receipt，才能满足该契约。普通 `run_tests`、参数不同的
   `run_verifiers`、任意绿色命令或额外模型 critic 都只能成为 artifact。
2. **Host 验收**：objective-only 或没有显式 verifier 契约的任务必须由 Host 明确接受。
   模型的完成声明、`update_goal` 请求、测试结果或自评只能提交候选和 artifact，不能替
   Host 接受终态。

Runtime 的 `Completed` 与评测的 `verified_success` 是两个层次：前者是 Host 按当前
TaskContract 接受的运行终态，后者还必须满足评测任务预先定义的确定性验收或人工 rubric，
并通过协议、预算和记录完整性检查。Host 手动结束目标、工具函数返回成功或模型自报完成，
都不能自动生成产品指标上的 `verified_success=true`。

`verification_runs` 只记录模型在运行中主动发起且被 Harness 识别的验证动作，用于衡量
行为和成本；除非 TaskContract 明确把某个精确调用本身列为 acceptance，否则它不是
`verified_success` 的替代条件。真正的成功证据仍是 Host 对最新 `workspace_revision`
执行预先冻结的 verifier 后生成的 receipt。

artifact 的状态同样不能越级推断。`Produced` 只表示工具产出了可引用对象；它可以作为
后续 verifier 的输入，但不等于 `Host Verified`。只有与当前 TaskContract generation、
最新 `workspace_revision` 和预定义验收器同时匹配的成功 receipt，才能把对应证据判为
`Host Verified`。

#### 2026-07-19 M5-A 机制与真实 A/B 证据

M5-A 被测 checkpoint 实现了上述 canonical 边界：当时的 Run API v5、RuntimeEvent v7、
State schema v12；当前 v10/v16/v21 继续保留该语义。TaskContract 在 `RunCreated` 冻结，
模型 `Stop` 只产生 completion candidate，Runtime 是
唯一 EvidenceReceipt 与 Completed owner。结构化 task 的 constraints、non-goals 和
acceptance description 已进入确定性的 model-visible canonical transcript。显式 verifier
receipt 必须精确匹配 generation、acceptance、parameters、resolved plan、当前 workspace
generation/revision 和实际 Available artifact。任何 `MayWrite` 执行都使旧 receipt 失效，
即使内容 hash 返回旧值。

本地证据包括 root/child 同门禁 conformance、伪完成/缺 artifact/same-hash write 反例、
Memory/SQLite parity、Host verifier Prepared/InFlight/Committed 的真实子进程 `SIGKILL`
恢复、三入口逐事件 parity、focused、严格 workspace Clippy 和完整 workspace tests。

正式官方 DeepSeek A/B 使用 `503d6294` baseline 和 `ba25f836` candidate，通过
app-server Run API 显式冻结 exact verifier，而不是用 Host-only exec 的外部验收冒充
canonical receipt。两个场景、两个版本、3/cell 共 12/12 运行计量有效，成对首个模型请求
投影完全相同：

- 编码场景 baseline/candidate 均为 `3/3` verified；
- 强制伪完成场景 baseline 为 `3/3 completed + false-success`，candidate 为
  `3/3 blocked + correct-rejection`；
- candidate 编码成功 3/3 均有精确 receipt，反例 3/3 均有 CompletionRejected 且无 receipt；
- 编码 cell 的 candidate 请求持平、Token `+1.08%`、端到端时间 `+9.22%`、费用
  `+3.13%`；这是单任务小样本，不能外推为普遍效率变化；
- verifier 的 Python 进程使用 `-B`，失败与成功自测都要求 Git tracked/untracked 状态
  前后精确一致，避免缓存文件让 workspace-bound receipt 偶发失效。

该切片 `product_metric_eligible=true`，保留依据是 false-success 显著下降且固定编码任务
不回归，而不是 Token 或速度更优。身份、逐 cell 指标、复杂度成本和非结论见
[M5-A canonical 完成门禁精确 A/B](../../eval/summaries/m5-completion-gate-ab-2026-07-19.md)。

#### 2026-07-20 M5-B ContextBroker 正式 A/B 与 shrink

M5-B 使用官方 `deepseek-v4-flash` 完成 2 个长任务、3/cell、12 对 / 24 treatment arms
的正式评测。candidate compaction on/off 均为 `6/6` verified、0 false-success，请求数
6/6 对相同；on 的 Token 6/6 对下降，平均 `-8.743%`，但费用 6/6 对上升，平均
`+7.995%`，时间 3 对更快、3 对更慢。旧 baseline 模型摘要 on 为 `0/6`、off 为 `4/6`，
所以模型摘要路径不能恢复。

正式 accepted suite 为 265 个物理请求、2,144,293 Token、USD `0.111391817`，计量完整且
`product_metric_eligible=true`。另有 7 次 retry 后计费不可知的技术尝试；包含它们的整个
执行只能报告 `>=358` 个请求、`>=2,763,524` Token、USD `>=0.141682656`，不能伪装成精确
总成本。

产品结论为 `shrink`，而不是“Token 下降即保留主动压缩”：`e2c870b0` 保留唯一
evidence-aware ContextBroker、append-only transcript/request projection 分离、超过基于
官方 context/output capability 派生的 Host hard input limit 时的同 Runtime 本地确定性压缩，
以及压缩后仍超限的 typed backstop；
同时删除手动 `/compact`、HTTP/stdio Compact、独立 compaction root、90% 提前阈值和模型
摘要 lifecycle。当前不得把 compaction 宣称为成本、速度或成功率优化。完整 revision、
binary/result SHA、逐 cell、成对方向与归因限制见
[M5-B ContextBroker 正式 A/B](../../eval/summaries/m5-context-broker-ab-2026-07-20.md)。

### 5.3 持久化、进程中断与恢复证据契约

任何声称支持 crash/reopen/resume、幂等恢复或 exactly-once 终态的切片，都必须让每个
故障窗口留下可审计的恢复证据。离线窗口可以由外部子进程故障测试、精确断言和提交的
恢复矩阵共同证明；真实或凭据化 run 必须保存结构化恢复记录。若离线窗口也要参与恢复率、
Token、费用或其他产品指标计算，同样必须生成逐 run 结构化记录。结构化记录至少包括：

```text
run_id
crash_phase
kill_mechanism
same_run
last_committed_seq_before_crash
first_committed_seq_after_resume
event_count_before_crash
event_count_after_resume
event_prefix_digest_before_crash
event_prefix_digest_after_reopen
final_event_digest
unique_event_id_count
terminal_count
model_request_delta
usage_delta
unknown_billing
command_id
creation_command_sha256
reserved_run_id
interaction_id
command_receipt_count
control_requested_count
control_action
command_payload_digest
interaction_request_count
interaction_resolution_count
steer_queued_count
steer_applied_count
tool_side_effect_count
tool_side_effect_digest
lease_outcome
no_key_replay
```

字段须满足以下语义：

- 恢复必须继续同一个 `run_id`，不能新建 run 后把两段输出拼成“恢复成功”；重开后已提交
  事件前缀的规范化摘要必须保持不变，后续 sequence 严格单调且 event id 不重复。
- `crash_phase` 必须区分模型请求准备/在途、工具执行前/副作用后未提交、
  creation reservation 已提交但 `RunCreated` 尚未提交、continuation 已创建但尚未发布、
  hard-limit compaction committed 后尚未发布、
  `interaction_requested`、`interaction_resolved_before_tool_start`、
  `steer_queued_before_applied`、`steer_applied_before_next_model`、
  `control_requested_before_terminal`、普通事件提交后未发布，以及 canonical terminal 提交后
  未发布等窗口；每个窗口分别报告预期与实际的 request、usage、事件和副作用增量。
- `command_id` 与 `interaction_id` 是当前 crash trigger 的 nullable ID；涉及多个命令或交互
  时必须保存完整 ID 集合或等价规范化摘要。`command_payload_digest` 必须绑定命令类型和
  payload，用于证明同 ID 同 payload 幂等、同 ID 不同 payload 被拒绝。
- start/continue 的 creation command 还必须记录
  `creation_command_sha256 + reserved_run_id`：同 command ID 同 payload 的并发或崩溃重试
  只能观察到一个 reserved/created run ID，不同 payload 必须拒绝。若预运行模型请求可能已
  发出而无法安全完成创建，必须保留 reservation 并 typed fail closed，不能另建 run 掩盖
  歧义。
- continuation 的 source event prefix、terminal、accounting 和 transcript digest 在创建
  前后必须不变；新 run 的 `parent_run_id` 为空且 `continued_from_run_id` 精确指向 source。
  `resume` 的恢复证据仍必须是 `same_run=true`，不能用 continuation 代替。
- hard-limit compaction 必须记录 source projection digest、source entry count、当次真实
  tool catalog、前后 Token 估算与 committed projection digest。Store 必须从 canonical
  transcript 重算并拒绝不一致事件；该本地确定性步骤不得增加模型 request、usage 或
  unknown billing，也不得建立独立 compaction run。
- 恢复后不得重复 interaction request、resolution 或 command receipt，不得把 queued steer
  当成 applied；已提交 `SteerApplied` 后不得以此前已提交的 stop response 终止 run，必须
  保留该响应，并把 applied steer 作为下一次模型请求的用户输入。
- 请求已发送但 usage 尚未持久化时，费用不能推断为零。记录必须设置
  `unknown_billing=true`，保留已知 request/usage delta，并使依赖完整费用或 Token 的比较
  `product_metric_eligible=false`。
- 模型请求是否已发送无法判定，或非幂等工具可能已产生外部副作用时，恢复必须 fail
  closed：不得自动重发请求、重跑危险工具或宣称完成。只有持久 idempotency key、可核验
  的副作用摘要或明确的人工处置，才能解除该状态。
- 工具证据至少绑定 tool call/attempt、恢复前后副作用计数与结果摘要；文件、进程或外部
  系统的实际状态必须参与判定，不能只检查 Runtime 是否再次返回成功。
- lease 证据至少记录旧 owner、新 owner、拒绝或回收决定，并证明存活 owner 的并发恢复被
  拒绝、失效 owner 的 lease 可安全回收；同一时刻不能有两个执行者推进同一个 run。
- terminal 的 exactly-once 只指 RunStore 中恰好一个 canonical terminal event。stdout、
  NDJSON 与其他事件 sink 是至少一次投影，进程中断后可以重放。canonical event envelope
  暴露 `run_id + sequence + event_id` 时，消费者可据此去重；当前紧凑
  `codewhale.exec-stream` v2 虽新增 canonical 工具失败字段，仍未在每条展示事件上承诺
  envelope identity，必须按至少一次输出消费，
  不能把重复投影误报为第二个持久终态。需要逐事件去重的调用方应读取 canonical event
  记录；若未来给紧凑流增加 envelope identity，必须升级并测试机器协议版本。
- terminal 后的 no-key replay 必须在不读取凭据、不发模型请求、不执行工具的情况下重放
  同一终态；持久事件数量、事件摘要、usage、workspace 和工具副作用均不得改变。

进程恢复验收必须包含由外部监督进程发送的真实 `SIGKILL`（Windows 使用等价的强制终止）
并重新打开持久 Store。错误返回、panic 注入或同进程 fault hook 只能作为定位更精确的补充
测试，不能替代 OS 级进程终止，因为它们无法证明缓冲区、析构器、lease 和重新打开行为。

单次、受限费用的真实 DeepSeek resume canary 只证明当前生产二进制、官方 API 与持久恢复
路径能够共同工作，必须标记 `product_metric_eligible=false`。它不能替代每个 cell 至少
3 次的 A/B，也不能用一次成功推断恢复率、Token 或费用改善。

## 6. 真实性判定

`verified_success` 只能来自 TaskContract 和评测任务在运行前定义的验收器，例如：

- 测试、编译、lint；
- 确定性输出检查；
- API/CLI 行为检查；
- 结构化 diff 断言；
- 人工预先定义的 code-review rubric。

模型自评、额外 critic 模型或自然语言结论只能作为 review 信号，不能单独成为成功证据。

所有证据必须绑定生成时的 TaskContract generation 和 workspace revision；后续写入使旧
证据失效。receipt 不匹配、缺失或无法证明最新 revision 时必须 fail closed，不能降级为
模型判断、Host 的非结构化完成点击或旧兼容语义。

## 7. 能力保留门槛

能力进入稳定核心必须同时满足：

1. 至少满足一种预先冻结的产品收益：提高正向任务 verified success；正向成功率不退化时
   显著降低成本/时间；或在预定义负向任务中可重复降低 false-success、提高
   correct-rejection，同时正向任务不退化且新增开销经记录后可接受；
2. 不增加 false-success；
3. 默认用户流程不变复杂；
4. 失败模式明确且可恢复；
5. 没有产生重复 Runtime、Store、Task 或产品概念；
6. 生产复杂度与收益成比例；
7. baseline deterministic capability 有冻结的 before/after task matrix；model-visible、可选或
   默认策略 treatment 有关闭候选的 A/B 对照；
8. 有回归测试和删除方案。

若收益只存在于极少任务，应作为按需策略，而不是默认全局行为。

## 8. 当前 `verify` 实验的判定

旧 TUI 独立 `verify` 工具曾使用额外 DeepSeek 调用评审 bounded diff/file evidence；旧
ToolRegistry 删除后它已没有 canonical 生产消费者，只剩编译模块和自身测试。它不等于
test/verifier receipt，也不应为了保留实验而恢复第二条模型路径；M4 已将其物理删除。

只有未来真实任务证据证明有必要重新引入模型 critic 时，才允许作为新候选单独评估：

- 是否提高真实缺陷发现率；
- 是否降低 false-success；
- 是否只是重复主 Agent 的判断；
- 成本和延迟；
- 是否应默认关闭、按需启用、缩小或删除。

在该实验通过本规范前，不得恢复生产接线，更不得让它成为所有任务的强制完成阶段。

## 9. DeepSeek live canary

默认 CI 使用离线 fixture。真实 API 测试必须显式启用，并设置：

- 最大运行数；
- 最大输入/输出 Token；
- 最大费用；
- 单请求和整套超时；
- retry 上限；
- 日志脱敏；
- 结果归档。

live canary 重点验证官方协议可能变化的部分，而不是替代离线测试。

### 9.1 当前 M2 官方协议证据（2026-07-18）

在干净 revision `4bd6563b471292ad4bc632b233f0e85b9088df15` 上，当前六请求 Harness
（SHA-256 `60072577a2e865d82b4a4c025c5756365bef778656cbeb689ff16de624c04cf1`）
通过官方 DeepSeek 线上 6/6 请求、0 failed，整套耗时 5.465 秒，按 2026-07-16 价格快照
估算费用为 `USD 0.00009914`。它覆盖：

- Standard Chat non-thinking；
- Standard Chat thinking 工具调用与 `reasoning_content`/tool-call exact replay；
- Beta Strict Chat non-thinking 工具调用，以及工具结果轮省略 `reasoning_content` 的 replay；
- 独立 Beta FIM；
- 每次请求的 usage/cache 与 finish reason。

原始脱敏 JSONL 为本地忽略文件，权限 `0600`，摘要
SHA-256 为 `2f5a1956b9fd18ac2b202f4911faef71cf88823ebeeead10f166af4ca6351b10`；
可提交事实见
[M2 当前 DeepSeek 官方协议 Canary](../../eval/summaries/m2-current-deepseek-live-2026-07-18.md)。

该结果仍是 `record_class=protocol_canary`、`product_metric_eligible=false`、
`verified_success=null`。Harness 直接调用官方 API，不经过生产 Agent loop；它是当前 M2
外部协议复核，不单独证明 Rust `RequestPlan`、transport 或 Agent 链路已接管，也不构成编码
任务成功率、Token、时延或成本提升证据。历史 M1-B 5/5 结果继续作为旧 revision 基线保留。

### 9.2 M6-A isolated Writer 机制证据（2026-07-21）

M6-A 的验收目标是证明唯一 Orchestrator 下的单 Writer 生产闭环可用，而不是证明多 Agent
具有产品净收益。离线验收必须覆盖：

- root、read-only child、Writer 使用同一个 `AgentRuntime` conformance；
- clean Git、精确 base、独立 worktree、allowed paths 和根目录字节隔离；
- Host-observed diff/changed files/revision、worktree verifier、唯一 fast-forward
  integration 与最新根 revision verifier；
- base/branch CAS conflict、重复 integrate、取消、cleanup 和进程级 crash/reopen；
- Memory/SQLite reducer parity，以及 exec/TUI/HTTP/stdio canonical projection；
- 没有第二 Runtime、Store、模型调用路径、工具目录或 presentation-local truth。

在代码检查点 `a982a9a878587450aab57e87f4ec9df7751eb146` 上，生产二进制
SHA-256 `ccba6647be7e07b7f117bf1a28d888ffc4578215f7defcb2e40aa510e425566c`
完成一次费用受限的官方 DeepSeek Writer canary：

- `deepseek-v4-flash` Standard Chat 7/7 请求完成，0 transport retry；
- 21,611 input、1,790 output、14,080 cache-hit、7,531 cache-miss、
  726 reasoning Token，费用 USD `0.001594964`，耗时 29.047 秒；
- Writer 只修改冻结范围 `answer.txt`，worktree exact verifier 与集成后根 verifier 均通过；
- 集成为精确 fast-forward，根仓最终 clean，临时 worktree 与 branch 均删除；
- 根 EvidenceReceipt 绑定 integration 后 workspace generation 3 和最新已知 revision；
- Key 未进入 argv、stdio 协议、临时 State、fixture Git 或提交摘要。

该记录必须标记：

```text
record_class=mechanism_canary
product_metric_eligible=false
```

它只证明当前二进制、官方 API 与完整 Writer lifecycle 能共同工作。它不能替代 M6-B 的
single-agent / writer-agent 冻结任务 A/B，不能证明成功率、Token、费用或时间改善，也不
支持直接开发通用 DAG 或多 Writer swarm。可提交事实、身份与归因限制见
[M6-A isolated Writer 机制 canary](../../eval/summaries/m6-a-isolated-writer-canary-2026-07-21.md)。

### 9.3 M6-B1 Writer 产品收益证据（2026-07-21）

候选 `5d72ae94b794b022a916151e1741c5b52de60edb` 使用同一个生产二进制、
`deepseek-v4-flash` Standard Chat 和平衡交错 schedule，完成 3 tasks × 2 treatments ×
6 runs：

- 18/18 accepted pairs、36/36 arms、每 cell 6 次；
- 0 invalid attempt、0 measurement-invalid、0 unknown billing、0 retry；
- canonical root/child Terminal、RunView、sealed accounting、actor usage、surface cost
  与请求数全部闭合；
- 总计 285 请求、1,489,265 input、120,501 output，费用 USD `0.105379831`。

产品结果：

- single `2/18` verified、15 false-success；
- Writer `4/18` verified、7 false-success；
- Writer 总 Token `+35.5%`、费用 `+52.5%`、时间 `+39.8%`；
- Writer 有 1 次 root 调用 treatment 禁止的 may-write 工具、7 次 retained recovery；
  只有 1 对双方成功；
- T3 的 12 个 arms 全部未满足预先冻结的失败后恢复时序，即使最终 verifier 为绿。

因此该结果 `product_metric_eligible=true`，但 `hard_gate_met=false`；正式决策为
`reject_and_rework`，不是 `hold` 或 `shrink`，M6-B2 不准入。`product_metric_eligible`
只说明失败证据可正式计入，不表示产品成功。完整 identity、逐 cell/pair、诊断费用与归因见
[M6-B1 Writer 收益 A/B](../../eval/summaries/m6-b1-writer-benefit-ab-2026-07-21.md)。

本轮还冻结了后续评测纪律：

- verifier 自身不得在任务 workspace 生成缓存或其他 artifact；
- actor capability 必须由 Host admission 强制，不能只靠模型提示；
- 需要时序证据的任务必须由 TaskContract/EvidenceReceipt 表达，不能用最终绿替代；
- unknown billing、unsealed 或 canonical measurement 不一致立即停套，不得重采样；
- 重新评测前不得修改任务、预算来掩盖失败，也不得先实现双 Writer。

### 9.4 M6-B1 rework v3 计量中止证据（2026-07-22）

rework candidate `3310aa73ef3bae45ce9296e65964c9b7531c22f0` 在全部离线门禁通过、
candidate binary 和 v3 manifest 于 live API 前冻结后，启动同三任务、两 treatments、
每 cell 6 次的正式 A/B。结果不是完整产品 A/B，而是预注册的计量中止终态：

- 1 个 T1 pair / 2 个 arms measurement-valid；另 1 个 T2 single arm
  measurement-invalid，Writer mate 未启动；
- 共 3 arms、29 physical starts；T1 single 9/9、T1 Writer 10/10 请求和 usage 均闭合；
- T2 single root `started=10/completed=10/in_flight=0`，其中 9 个
  `ModelResponseCommitted` 有 usage，1 个 retryable `deepseek_transport` 失败后由 Runtime
  重试一次；`runtime_retries=1`、`transport_retries=0`；
- T2 的 canonical Terminal 唯一、位于最后 sequence 1723，Terminal、RunView 和 accounting
  完全一致；`billing_unknown_attempts=1` 来自该 transport attempt，不是 Harness 漏算；
- 已知 usage 下界为 167,003 input、11,929 output，已知费用下界为 USD `0.012583452` /
  CNY `0.089881800`。失败 attempt 没有 provider response/usage，不能证明免费；
- Harness 保存当前 arm 后立即停止，未补 mate、未重采样；最终
  `mode=aborted_unknown_billing`、`decision=hold_mechanism`、
  `product_metric_eligible=false`、`hard_mechanism_abort=false`、M6-B2 不准入；
- 三个 arms 均为真实 blocked 产品结果，0 false-success、0 Writer safety finding，但样本
  不足以比较 single/Writer 收益，也不能推翻 v2 的 `reject_and_rework` 结论。

正式 raw 为本地 Git ignored 的
`eval/results/m6-b1-writer-benefit-ab-v3.json`，权限 `0600`，SHA-256
`2bc41e464888e49c7894881f36b2ad8eed5b10e47191ddd2b2a5991148acb2c1`。完整冻结身份和
证据边界见
[M6-B1 rework Writer 收益 A/B v3](../../eval/summaries/m6-b1-writer-benefit-ab-v3-2026-07-22.md)。

该结果不可续跑或拼接。不得重跑同一 candidate、只补 T2 mate、复用 v3 T1 pair 或将
v3 与未来样本合并。新的 v4 必须来自独立、实质性的产品代码变化和离线验收，在任何 API
请求前重新冻结，从 schedule position 1 全新收集；若改变任务、预算、阈值或只改变
Harness/版本号，则属于新 claim 或条件重采样。只有权威 transport phase 或 provider 计费
证据才能把失败 attempt 标为已知未计费；unknown billing 门禁不得为跑满样本而放宽。

### 9.5 M7-A Agent 收敛 v1 口径中止证据（2026-07-22）

M7-A candidate `24c8a530fd7ae200d823e07cce5d9c75ff2cc5ea` 完成 verifier contract
单 owner 与 typed completion recovery 后，使用 `deepseek-v4-flash` Standard Chat 启动
20 对 / 40 arms 正式 A/B。Harness、manifest、schedule、baseline/candidate exact release
binary 均在 live API 前冻结。v1 只完成 T1 首对即按 `false_success` safety gate 停止：

- baseline blocked，外部 verifier passed，10 个 physical attempts，USD `0.007048597`；
- candidate completed，唯一 Host receipt，外部 verifier passed，5 个 physical attempts，
  USD `0.002141535`；
- 两 arm accounting 均 complete、sealed、billing known，总费用 USD `0.009190132`；
- candidate 只修改 `slugify.py`，scope/path/tool/child/temporal、verifier spec、lineage、ledger、
  Terminal/RunView projection 和 Standard Chat/model identity 全部通过；
- 只完成 2/40 arms，`product_metric_eligible=false`。

事后复核证明 raw 中的 `artifact_closure_mismatch → false_success → reject` 是评测假阳性。
Python Harness 用排序后的 JSON 重算 artifact SHA；Rust binary 因 `serde_json/preserve_order`
保留插入顺序，而当前 `canonical_json` 实现并未像注释声称的那样显式排序。结构体字段顺序与
字典序不同，所以跨语言 SHA 必然不同；baseline 与 candidate 都出现同一 mismatch。candidate
能生成 receipt，也证明生产 Store 已按 Rust 自身协议验证 inline artifact、observation、
revision、spec 和 lineage。

正式决定为 **hold**：不能用不完整 pair 宣称 candidate 收益，也不能把该假阳性记为真实
Host unsafe completion。原始结果保持本地 `0600`、Git ignored，SHA-256
`27e38db2f33280528a1552a57c1bb705ef3ab9bd15bffdaa9552ba9958825af2`；不得覆盖、续跑或
拼接。下一 suite 必须来自修复 production canonical JSON owner、增加跨语言固定向量后的
新 clean candidate，并重新冻结独立 identity。完整记录见
[M7-A DeepSeek Agent 收敛正式 A/B v1](../../eval/summaries/m7-a-agent-convergence-ab-2026-07-22.md)。

### 9.6 M7-A2 shared canonical JSON 修复与 accounting 中止证据（2026-07-22）

M7-A2 先修复 v1 的评测口径根因，再建立公平 shared-fix A/B：同一个 canonical JSON
correctness patch 被应用到 control parent `3351213b` 和 treatment parent `24c8a530`，形成
direct-child checkpoints `18de2ad29a9db5780fcbe90e8ba0eef39225399f` 与
`c6a743040eae9548837849c6f58e5c638cc073cc`。两边 changed paths、numstat、stable patch-id
完全相同；原始和修复后的 M7-A production delta patch-id 也相同。control/treatment exact
release binary 分别为：

- `sha256:9c8d5b095222ac879ad5cf85f83bfe53b9e91dda88c4775f213fd02d3bdc0b3d`；
- `sha256:dd5becce5fb9ab50b1cf53de70918adb26c8da7957672817943c51f1d774445a`。

`crates/protocol` 现在显式递归排序 JSON object、保持 array 顺序，并让 artifact create/replay
共用 canonical bytes；Rust/Python、`preserve_order` 开关、M7-A v1 T1 artifact/receipt 和
payload/SHA/length/ID 篡改使用同一固定向量。新的 v3 Harness 还绑定实际 root start
identity、TaskContract、完整 event/RunView digest、Host lifecycle、typed rejection 实因和
工具生命周期。live 前 focused、fmt、workspace clippy/test、两个 checkpoint 定向测试、
exact release build 和 Harness 36/36 自测全部通过。

冻结 suite `m7-a2-agent-convergence-ab-v1-18de2ad2-vs-c6a74304` 仍使用原任务、提示词、
`deepseek-v4-flash` Standard Chat、预算、顺序和门槛，从 position 1 计划执行 20 对 / 40
arms。实际完成 7 arms 后按预注册 accounting gate 停止：

- 前 6 arms measurement valid、0 false-success；T1–T3 treatment 3/3 Completed+verified，
  control 0/3 blocked；
- treatment 三个成功 arm 的 Host receipt、artifact、lineage、Terminal/RunView、external
  verifier、scope 和 authority 都闭环；T3 明确完成 failed verifier → effective mutation →
  passed 的时序 lineage；
- 第 7 arm T4 treatment 修改预期文件且 external verifier passed，但 canonical terminal
  为 Failed，无 completion proposal/Host receipt；4 个 physical response/attempts 只有 3 个
  usage response，并有 `model_request_failed=1`、`incomplete_responses=1`、
  `usage_complete=false`、`complete=false`；
- accounting 为 sealed、in-flight 0、billing_unknown=false、unpriced=false，但这只排除了
  transport delivery unknown，不能证明 incomplete response 的 billable usage 为零；
- 7 arms 共 50 个 physical attempts，已知费用 USD `0.019383566` / CNY `0.138454040`
  只作下界；input/output 记录为 254,495 / 25,648，但也不是完整总消费。

Harness 的 `surface_totals_valid` 对 incomplete response 的 response/usage response 数量要求
偏严，但即使移除该派生 mismatch，`incomplete_responses=1`、`usage_complete=false` 和
`complete=false` 仍独立要求停止。raw 没有保存可把 failure 归因到网络、provider、输出上限
或其他具体原因的 typed cause，因此不得事后猜测或把缺失 usage 当作零。

预注册决定为 **hold**：`product_metric_eligible=false`、`safety_failures={}`、
0 false-success、treatment identity/spec 全部有效。已观察到的 verified delta `+3` 仅是
T1–T3 机制证据；T4 control、T5 和 read-only child 路径未执行，不能宣布 5-task 总体收益、
正式效率收益或多 Agent 收敛。raw 保持本地 `0600`、Git ignored，SHA-256
`ca3eeaaa7a2e46914d262549dab57c57fe23bfd7a94678002a81f405f80ffbbc`，无 formal partial；
不得覆盖、续跑、补 mate、追加 position 8、重采样或与未来 suite 拼接。

新的正式 claim 必须先有独立、实质性的 production stream/accounting/typed failure evidence
变化及离线 fixture/crash/replay 证据，再使用新 candidate、suite ID、output path 从 position
1 全量 refreeze。完整身份与逐 arm 事实见
[M7-A2 DeepSeek Agent 收敛正式 A/B](../../eval/summaries/m7-a2-agent-convergence-ab-2026-07-22.md)。

### 9.7 M7-A3 不完整流式响应 correctness 与不可复评结论（2026-07-22）

M7-A3 在 production owner 上补齐了 M7-A2 缺失的诊断与安全恢复事实：DeepSeek parser 只有
在受支持 finish 与 `[DONE]` 都闭合后才提交 response；usage 一经观察立即进入 accounting；
partial content、reasoning 或 tool-call fragment 均禁止自动重放。RuntimeEvent v15 与 State
v20 持久化脱敏 response evidence、`retryable`、`retry_safe`、actionable output 和原子 retry
决策，RunStore 重开后逐字段一致，root/read-only child 通过同一 Runtime conformance。

Harness 现在分别核对 response count 与 usage response count，并以 `accounting.usage` 保留
完整响应提交前已经观察到的 usage；RunView committed usage 单独投影且不得超过 accounting。
对 M7-A2 T4 的纠正只消除了错误的 surface 等式，`incomplete_responses=1`、
`usage_complete=false`、`complete=false` 和费用下界仍保持不变。原 M7-A2 raw/manifest 未改。

离线 focused、fmt、workspace clippy/test 和 Harness 39/39 全部通过。但正式复评不具备公平
准入条件：M7-A3 patch 修改 12 个路径，旧 treatment 与 patch base 的 12 个 blob 全部相同，
旧 control 却有 8 个不同 blob 和 20 个三方冲突块；control 的 RuntimeEvent v13/State v18、
treatment 的 v14/v19 与新 v15/v20 直接切换也无法同时保持相同 correctness patch 和原 M7-A
production delta。因此 Harness 在 output、Key 和 API 前返回 typed inadmissible；本轮未读取
Key、未调用官方 API、未创建新 formal manifest/binary/raw。

决策边界为：M7-A3 correctness 机制依据离线反例与恢复证据保留；M7-A 产品收益结论仍为
**hold**，不能把 M7-A2 的方向性前缀结果升级为 keep。未来若重评，必须从共同的 corrected
base 冻结一个新的 treatment delta。完整记录见
[M7-A3 DeepSeek 不完整流式响应诊断与安全恢复](../../eval/summaries/m7-a3-incomplete-stream-recovery-2026-07-22.md)。

### 9.8 M7-B Strict 工具目录准入与失败恢复结论（2026-07-22）

M7-B 没有把 Strict 当成用户模式或预设收益。`crates/tools` 继续唯一拥有 production tool
schema；`crates/deepseek` 对每次请求实际 advertised 的整组目录做确定性 compatibility
判定。只有整组兼容才规划 Beta Strict Chat；否则同一目录原子回退 Standard Chat，数量、
名称、顺序、schema 与语义不得变化。畸形或不完整 tool-call fragment fail closed，thinking
reasoning 与 tool-call history 按 actor turn 精确回放。

RuntimeEvent v16 / State schema v21 为每个失败 `ToolOutcome` 强制要求稳定
`failure_code`，并独立保留 invocation、transport、operation、side effect、retry、evidence、
artifact 和 workspace revision。模型可见失败 envelope 使用简短中文摘要与恢复建议，稳定
code/字段保持英文；成功工具输出不改写。root、read-only child、isolated Writer 共用同一
Runtime conformance，真实 SIGKILL/reopen 覆盖 `ToolPrepared` 与
`ToolOutcomeCommitted` 两侧，证明 crash 后不重复已执行工具或已提交 outcome。

持久化只保存唯一输入事实：RuntimeEvent 中的完整 `ModelRequestPrepared`（含 actual advertised
catalog）、catalog hash，以及绑定 `strict_tools` 的 execution fingerprint；不重复保存
provider 派生的 surface/reason。production composition 的 SQLite reopen 反例证明 exact
request 和目录逐字段一致，唯一 DeepSeek planner 重建出的完整 `RequestPlan` 也一致；切换
strict policy 会改变 fingerprint，因此恢复不能在不同 policy 下静默重建。

正式准入 manifest 冻结了六个默认可执行 actor 目录和一个 terminal no-tools 目录。六个
可执行目录在 `strict_enabled=true` 时仍全部选择 Standard Chat：root、interactive root、
coordinator 和 read-only child 首先受 `agent` 的 required 语义阻断，depth-limit child 受
`file_search` required 阻断，isolated Writer 受 `apply_patch.oneOf` 阻断。删除这些约束、
把 optional 改成 sentinel/空值、弱化 `oneOf`、丢弃工具或建立第二份 wire schema 都会改变
canonical 工具合同，不能成为 treatment。terminal no-tools 不执行函数调用，也不能提供
Strict 收益样本。

因此 Harness 在 release binary、credential 和官方 API 前给出
`inadmissible_no_surface_delta`：formal A/B 未开始、0 arms、maximum reruns 0、
`product_metric_eligible=false`、official API requests 0、credential read false。本结论
不声称 Strict 有收益或回归，也不外推到任意自定义 `ToolPolicy` 子集。

保留项是官方 Beta Strict planner、整组 compatibility diagnostics、无损 Standard fallback、
typed 失败恢复、exact replay 与 actor/crash conformance；不准入项是当前生产 Strict 默认
和当前无 surface delta 的 live A/B；拒绝项是 schema transformer、第二工具目录和弱化工具
合同；已删除项是无执行差异的用户 Strict 开关及无消费者重复路径。冻结结论后的
`4e3536f1` 又移除可推导的 Strict decision 布尔值与零调用 wrapper，并补充 SQLite reopen
证明；它不修改 frozen candidate、manifest、raw 或 wire 行为。完整冻结身份、哈希与非结论见
[M7-B Strict 工具调用准入与失败恢复](../../eval/summaries/m7-b-strict-tool-admission-2026-07-22.md)。

### 9.9 M7-C canonical 编辑基线与 FIM 准入结论（2026-07-23）

M7-C 先冻结 `eval/manifests/m7-c-edit-baseline-v1.json`，没有预设 FIM 优于 patch/edit。
manifest 包含 12 个任务目标、完整编辑失败矩阵、真实临时 Git workspace、deterministic
verifier、root/read-only child/explicit Writer lanes、`maximum_reruns=0` 与 credential
准入条件。Harness 只执行 canonical Rust gates 并投影 process verdict，不复制 patch parser、
编辑器或失败分类。

起始 `afb9b0ab` 的 12 项 tools contract assertion 为 3/12。9 个确定性失败分别是：
same-length/same-mtime stale 未检出、重叠 search 误判唯一、原子替换丢 mode、duplicate
target、未实现 rename、hunk count mismatch、no-op、ambiguous fuzzy first-match 和
multi-file 失败未完整回滚。`7613073c` 在 `crates/tools` 单 owner 内修复后为 12/12；没有
新增模型可见工具、Runtime、Store、Provider 或用户模式。

clean `9cba8b53` Harness 以 manifest canonical-content SHA-256
`4d5457db2e7fc075fbecf925f89d45ea412a9aace515da1b4cfb0b3a03755c55` 运行 8/8 gates，
覆盖 production AgentApplication loopback、latest-revision verifier recovery、SQLite reopen、
read-only child、isolated Writer integrate/cleanup、ToolPrepared/Started/Outcome 三侧 SIGKILL
与 app-server process replay。source before/after revision 与 tree 完全相同，dirty false，
official API requests 0，credential read false。focused、fmt、workspace clippy/test 与 Harness
self-test 也全部通过。

历史 30 个 JSONL 文件中的 157 个 run manifest、158 次 patch call、1 次记录的 patch
failure、138 次 verified success 和 4 次 false success 跨 revision/schema，只作方向性证据。
因此 3/12 -> 12/12 只支持 Host correctness keep，不支持真实模型 verified success、Token、
时间或费用收益 claim。

FIM 是独立 Beta Completions surface。当前 `crates/deepseek` 只有 FIM request planner 和
accounting 基础，production 没有完整 response parser、revision-bound Host edit lifecycle
或 canonical caller；同一 immutable binary 没有 control/treatment surface delta。因此
live A/B 在读取 Key 或请求 API 前给出 `inadmissible_no_surface_delta`：0 arms、maximum
reruns 0、product metric ineligible。决策为：保留 canonical editor correctness；FIM
production 接入 `hold`；删除 direct `git apply` CLI、TUI-local eval/edit loop 与剩余旧
acceptance；不创建半条 FIM 分支或第二编辑工具。完整证据与官方资料复核见
[M7-C canonical 编辑能力基线与 FIM 准入结论](../../eval/summaries/m7-c-edit-baseline-2026-07-23.md)。

结论后的只读复核没有改写 frozen manifest 或历史 result。`1cb65b82` 新增确定性反例并关闭：
`changes` 与 patch-only controls 混用、`path` 覆盖 `/dev/null` create/delete、未完整删除却删除
整文件、create-from-null 覆盖已有目标，以及 checked-create 竞态覆盖。13 个 M7-C patch
用例、2 个 checked-publish 用例、canonical preflight、当前 actor catalog identity 与完整
tools crate 均通过。

证据口径同步收窄：existing-file publish 是 exact-byte precondition 后的原子 replacement，
不是线性化 content CAS；跨文件 publish 不是 crash-atomic transaction。frozen E10 的
“generation unchanged”只由 direct tools fixture 覆盖；production Runtime 中 Started 的
`MayWrite` 会推进 generation，即使 revision 不变且 side effect 最终 `NotApplied`。历史
manifest 保持不可变并在 summary 中记录勘误。该修复仍没有 FIM treatment surface，因此
不授权 credential/API 或 live A/B。

### 9.10 M7-D RuntimeEvent v16 编辑失败观测闭环（2026-07-23）

M7-D 审计发现 `5bb9b577` 的单变体 WIP 不能成为正式 production failure baseline：它把
v16 事件降级给 v14 evaluator、按 `call_id` 而非 `operation_id` 配对、忽略 Started、
读取错误的 workspace revision，并把其他路径或同批成功误算为模型恢复。更重要的是，它在
没有 treatment delta 时仍提供 Key/API 入口。

`7ddf3bba` 删除历史 executor adapter 和 live credential 路径，建立只投影 canonical
RuntimeEvent v16 的 evaluator。恢复现在要求同 run、同工具、同结构化 target 且至少有一个
介于失败 Outcome 与后续成功 Prepared 之间的新模型请求；无法解析 target 的 patch 保持
unscorable。任何不成功 outcome 必须有 stable failure code；indeterminate side effect 与
Started 无 Outcome 均作为 transaction ambiguity 事实，但 normal run 不估计 crash 频率。

首个 clean suite 在 workspace-test exit 101 后停止并保留 0600 raw。CLI 同时暴露第二缺陷：
nested self-test `pass` 掩盖 aggregate failure；hash-only output 也无法定位具体失败 test。
不改源码的同命令诊断随后通过，故不得猜测或抹除首个失败。`cf9b3fd6` 冻结新的非覆盖 v2
suite：aggregate `passed` 唯一决定状态，失败 gate 在 0600 ignored record 内保留有界 tail。

v2 的 14/14 clean gates 全部通过：15 项 evaluator regression、M7-C tools/app loopback、
root/read-only、explicit Writer、Prepared/Started/Outcome crash/reopen、app-server
SIGKILL、focused、fmt、workspace clippy/test 和 diff check。before/after 都是
`cf9b3fd6248ef07a4333e60f940c8e63ba7852f5` / tree
`f48fe5e5e4a2a8aec391fdeaba2c7463e0e1f5f1`，dirty false；credential read false，
official API requests 0。

产品结论为 `keep_observer / shrink_live_harness / hold_editor_treatment`。这只证明 evaluator
能够可信观察未来的 v16 ledger，不证明当前主要产品损失在编辑、不产生付费模型指标，也不
准入 FIM、transaction state 或另一编辑策略。完整身份与首个失败记录见
[M7-D RuntimeEvent v16 编辑失败观测闭环](../../eval/summaries/m7-d-edit-observation-2026-07-23.md)。

### 9.11 M7-E 默认 Thinking 准入结论（2026-07-23）

M7-E 从 canonical RunStore 和既有正式结果定位非编辑损失，没有预设关闭 thinking 会提高
能力。M7-A2 成功候选的 17 个请求包含 3,042 reasoning tokens 和 7,263 reasoning replay
tokens；较早完整 eager-join suite 的 154 个请求包含 21,349 reasoning tokens 和 27,629
replay tokens。当前 production 又已经通过 `StartRunCommand.reasoning_effort` 在同一
Standard Chat binary 上表达 `high`/`off`，因此该候选有真实 surface delta，而不需要新增
Runtime、Store、工具、模型循环或产品模式。

冻结实验要求 5 tasks × 2 variants × 3 runs、15 pair / 30 arms、
`maximum_reruns=0`，并把 verified success、false success、first edit、verifier recovery、
root/read-only child、latest revision、external verifier、input/output/cache/reasoning/replay
tokens、requests、wall time、费用和 accounting 完整性同时作为门禁。只有每任务 success
不下降、false success 为零、至少 12 个 dual-success pairs 且至少一个核心效率 paired
median 改善不低于 15%（其余核心效率回退不超过 10%）才允许替代当前默认。

v1-v3 分别因成功 verifier side-effect 投影和真实 verifier-failure recovery 的 evaluator
误判而失效。v4 修正后，在 T1、T2 两个完整 pair 上发现：只归一化唯一 Host-owned
`task_generation` 后，实际首请求 semantic identity 仍不同。production workspace revision
会绑定 canonical workspace/repository absolute path，而 v4 给每个 arm 分配不同随机 path；
因此两臂 prompt 不同，不能用 hash normalization 掩盖。外部 SIGINT 在第二个同类 mismatch
后停止 suite，active `t3/reasoning_off/run_1` 可能已有 in-flight request，最终 billing
无法从已删除的临时 State/RunStore 重建。

v1-v4 合计 18 个已完成 arms、94 个已完成-arm requests、18/18 verified、0 false success，
已知费用 24,736,160 nanousd（USD 0.024736160）只是下界。全部结果因 evaluator/fairness
失效排除出产品指标；任何局部 paired delta 都不可采纳。四份 raw 保持 Git ignored、
`0600`、不可覆盖；v4 保持真实 `status=running`，不得事后补写终态。

final `458c3d7d` v5 使用 suite-owned fixed pair workspace、从同一 fixture 逐臂重建，隔离
State/RunStore/run ID，并在 pair 完成时立即比较真实 semantic messages、actor、tools、
surface、预算、revision、binary、fixture 和 schedule。SQLite reopen exact RequestPlan、
root/read-only/Writer、production loopback、process crash/reopen、focused、fmt、workspace
clippy/test 与 15 项 Harness regression 均通过。

由于 v4 active arm 留下 unknown billing，v5 冻结 `live_api_admitted=false`，在 preflight、
output reservation、Key read 和 API 前返回 `live_api_not_admitted`；没有 v5 raw、Key read
或官方请求。产品结论为 **hold**：保留 high/off protocol contract、exact replay 与公平
Harness；不改变 production 默认；不拼接 v1-v4。任何复评必须以新 clean revision、
successor manifest/suite/output 从 position 1 重新准入。完整身份、官方资料复核和非结论见
[M7-E 默认 Thinking 准入结论](../../eval/summaries/m7-e-thinking-admission-2026-07-23.md)。

### 9.12 M7-F canonical context-cache 前缀审计（2026-07-23）

M7-F 按 DeepSeek 当前官方 cache-prefix unit 规则评估 production exact wire，而不是把
Host 的 `PromptCacheControl`、UTF-8 公共字节或 prompt hash 当成 provider 命中。冻结口径
要求：

- authoritative input 是 persisted `ModelRequestPrepared` 和 deterministic
  `crates/deepseek::RequestPlan`；raw loopback HTTP 必须逐字段相同；
- 分别记录 system block、wire message、首个不同 role/内容、工具目录、surface/model/
  thinking/output/streaming identity；
- `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` 是唯一 provider 结果；
- cache 是 best-effort，单次 miss 不是确定性回归；
- 未形成同 binary wire treatment 时不读 Key、不调用 API。

`a1d68b05` 的真实三轮 production loopback 使用 read → edit → completion。请求 message
counts 为 `2/4/6`，相邻公共完整 message counts 为 `1/3`。上一轮 Host facts 始终是唯一
break：只读后其内容完全相同，下一请求仍不重放；edit 后 workspace revision 则正确刷新。
system prompt、catalog、model、surface、streaming 与 output budget 不变，raw body 与 rebuilt
plan 相同，SQLite reopen 精确。

四份 M7-E raw 中 18 个 completed + closed-accounting arms 合计 94 requests、395,812 input、
281,344 hit、114,468 miss，aggregate hit ratio `71.0802%`。该数据只证明现有 production
确有大量自动 cache hit；它没有逐请求 break，且原 A/B fairness 失效，不能用于推断 M7-F
收益。

候选门禁拒绝 facts 删除/粗化、facts 前移、多 system message 猜测和 tool duplication。
按时间顺序重放旧 Host facts 能形成真实 wire delta，但在旧 revision/receipt 的 typed
supersession、compaction、child fork 与 crash/reopen 合同完成前，不满足“语义不变”。
决策为 **hold**：保留 offline wire/reopen regression，不改变 production，不执行 live。
完整结果见
[M7-F canonical context-cache 前缀审计结论](../../eval/summaries/m7-f-context-cache-prefix-2026-07-23.md)。

### 9.13 M7-G canonical read-only fan-out 准入（2026-07-23）

M7-G 没有预设多 Agent 更快。production trace 证明同一 assistant response 中多个
read-only `agent` calls 已由唯一 `AgentRuntime` 先全部启动、再统一 join；因此 treatment
只在同一 binary/prompt/model/总预算下比较“不 advertised agent”的 single root 与“显式
恰好两个 read-only child”的现有 canonical surface，不增加 scheduler 或提示词模式。

冻结 suite 使用三项可分解的真实临时 Git 仓库任务，每项三次，合计 9 对 / 18 arms，
`maximum_reruns=0`。Key 读取前已通过 deterministic verifier、真实 transport overlap、
typed handoff/accounting、SQLite reopen、root/read-only/Writer conformance、同批 partial
failure/cancel、进程级 SIGKILL、focused、fmt、workspace clippy/test、Harness self-test、
同 revision release identity 和 no-key/no-network dry-run。

离线 SIGKILL 反例发现并修复了 RunStore 的 child recovery terminal 验证缺陷：只有指向
确切 in-flight `agent` operation 或 unfinished child ID 的 `RecoveryRequired` ambiguity
可以结束未闭合 lifecycle；普通 failure 仍拒绝。该 correctness 修复不构成 fan-out
效率证据。

正式 candidate `062623e6` 的 release SHA-256 为
`432a6d6a18906826f16b9949ee7223bb1d365a2319cb0d5884b0a7280c76e911`。
正式 run 在首个 control arm 后以 `run_identity_invalid` 停止：

- `key_accessed=true`、`network_accessed=true`；
- `completed_arms=0`；
- Harness 错误比较 caller-authored verifier plan 与 `crates/tools` Host resolver 注入
  env/timeout 后的 canonical `RunCreated` plan；
- accounting 已读取但旧 abort record 没有保存，最终 usage/request/cost 不可证明；
- 按 `maximum_reruns=0` 没有续跑、补 mate、换 output 或拼接。

因此结果为 **hold / inadmissible_observer_identity_bug**，
`product_metric_eligible=false`。不能报告 verified success、false success、wall time、
Token、cache、请求数或费用的 control/treatment delta。ignored `0600` raw 保持原样；
post-decision Harness 只离线修正 canonical verifier identity，并保证未来
post-terminal identity failure 先保存 terminal/accounting/State，不重跑本 formal。

任何 successor 必须换新的 suite ID、clean revision、output 和 immutable binary，从
position 1 开始；先通过 observer fault injection，证明任意派生判定失败都不会再次丢失
billing truth。否则不读 Key，explicit read-only child 保持现状。完整结果见
[M7-G canonical read-only fan-out 审计结论](../../eval/summaries/m7-g-readonly-fanout-2026-07-23.md)。

### 9.14 M7-G2 observer durability 与 successor 身份（2026-07-23）

M7-G2 先审计 evaluator 而不是重跑 API。旧 Harness 在 terminal 后继续完成 identity、
surface、accounting、verifier 和产品指标派生，最后才由外层写 arm；异常展开会先删除临时
SQLite。post-decision `8763722c` 只把 accounting 附到一种 identity failure，未闭合其它
observer 和 raw/process crash window。

新的离线 contract 固定：

```text
exact terminal snapshot
  -> no-credential SQLite reopen snapshot
  -> verifier snapshot
  -> derived arm result or abort
```

journal 是 `0600`、`O_EXCL|O_APPEND|O_NOFOLLOW`、逐记录 sequence/previous-record hash、
file `fsync` 与首次 directory `fsync`；现有 output 不得重开。11 个独立子进程 fault 覆盖
identity/verifier/surface/accounting exception、terminal 写前/半写/写后未 fsync、三个
checkpoint 后与 result 前 SIGKILL，全部证明 fault 不会先提交 `arm_result`。半写 tail 可检测
但不得派生或续跑。tamper 和重复 output 也 fail closed。

production RunStore exactness 继续由唯一 Rust owner 证明：fan-out loopback 对 root 与两个
child 在 `StateStore::open` 后逐字段相等；read-only child SIGKILL 不重发；typed recovery
只接受 exact unfinished lifecycle ambiguity。M7-G2 没有新增第二 Store/accounting owner。

旧 M7-G raw 仍是原 SHA-256、3 records、`completed_arms=0`、billing unknown，
`maximum_reruns=0`；不得补写、续跑、补 mate 或拼样。新的 ignored offline raw 为 14
records / 9,235 bytes，`key_accessed=false`、`network_accessed=false`、
`product_metric_eligible=false`。

admission 结论为
**hold / live_successor_inadmissible_no_new_production_delta**。`062623e6` 后没有新的
fan-out production behavior；换 suite ID、output、immutable binary 或 evaluator-only
revision 不能单独成为新 candidate。本阶段在 credential/API 前停止。下一候选必须先有
Agent request/Token budget 的真实 production 反例和 material delta，再用全新 clean
revision、suite/output/binary 从 position 1 运行完整 9 对 / 18 arms。

完整身份、fault matrix、raw hash、门禁和非结论见
[M7-G2 observer durability 与 successor 准入结论](../../eval/summaries/m7-g2-observer-durability-2026-07-23.md)。

### 9.15 M7-H request/Token 浪费矩阵与 terminal catalog（2026-07-23）

M7-H manifest 固定 `maximum_reruns=0`，对四份既有 canonical evidence 做 SHA-256 输入
校验和只读投影。矩阵覆盖 79 个历史 run record、root/read-only child/explicit Writer、
logical/physical requests、reasoning/replay、handoff integration、compaction、hard-budget
exhaustion 与旧 schema 可观测边界。各分组 logical/physical request 差异均为 0；四份数据的
compaction 和 hard-budget exhaustion 均为 0，因此只能证明样本中未观察到，不能证明这些
路径没有浪费。旧 exec raw 不含逐请求 advertised catalog，统一标记
`terminal_catalog_unobservable`，不猜测 terminal permit。

可精确归因的 Writer v2 记录包含 root/child 共 285 个请求、59,500 reasoning tokens、
136,139 reasoning replay tokens；M7-A2 partial 包含 50 个 root 请求、16,638 reasoning、
40,310 replay。eager-join 与 terminal-permit 的旧 aggregate usage 分别保留 21,349/27,629
和 11,683/22,814 reasoning/replay，但不可按 actor 拆分。这些重叠历史 suite 不能相加为
总体频率，也不能把官方要求的 tool-call reasoning replay 直接判为可删除浪费。

新的 deterministic counterexample 冻结一项同任务对照：

| 项目 | baseline `1943df4c` | candidate `eb8763a1` |
|---|---:|---:|
| actual terminal catalog | `tools=[]` | `tools=[]` |
| model request budget | 1 | 1 |
| prepared / physical requests | 0 / 0 | 1 / 1 |
| compaction | 0 | 0 |
| terminal | `ContextLimitExceeded` | `Completed` |
| exact regression | fail | pass |

两个 arm 分别编译到独立 target，binary SHA-256 为
`11a9f7c54ff364cc19be1b2b53d2c1f3fe997869f31865799bcc10d2f27db82b` 和
`6412b8d76e05f982a36b47c7ada70ce414c87d06b1ae31d9921da70cbd85660a`；共享 target artifact
复用的一次候选运行作废。真实 fixed production catalog 另有 app composition 回归，证明
hard boundary 只按最终请求目录估算，且不生成虚假 compaction event。

保留判定只覆盖 deterministic Host correctness。候选没有改变模型、reasoning、提示词、
预算、工具目录或 provider surface，故不存在可付费比较的模型 treatment；Key/API 在
credential gate 前保持未使用，`product_metric_eligible=false`。结论是
**keep fix / hold broader optimization / live inadmissible_no_model_treatment**，不报告
Token、时间、费用或一般任务成功率收益。manifest 与 observer：

- `eval/manifests/m7-h-request-token-waste-v1.json`；
- `scripts/eval-m7h-request-token-waste.py`；
- ignored `0600` result SHA-256
  `cdad0a5e91594d30c548ede92d0b7eee74f8bbec4a861f59dac90d9d2feea1d6`。

完整实现、离线矩阵、门禁、官方资料和非结论见
[M7-H canonical request/Token 浪费矩阵与 terminal catalog 结论](../../eval/summaries/m7-h-request-token-waste-2026-07-23.md)。

### 9.16 M7-I near-limit context/request budget 基线（2026-07-23）

M7-I manifest 固定 clean production baseline `6e9e9b7b`、tree
`d360072e4d2af26612061d6e47d620682a585853`、Run API v10、RuntimeEvent v16、
State v21、exec-stream v2、外部 target
`/private/tmp/codewhale-m7i-target` 与 `maximum_reruns=0`。13 项矩阵覆盖：

1. root ordinary 与 reserved terminal `tools=[]` 请求；
2. ordinary/terminal compaction success 与 mandatory facts over limit；
3. Runtime logical request budget、DeepSeek physical admission budget、partial failure 与
   cancel；
4. compaction commit 后 SIGKILL/SQLite reopen；
5. read-only child、explicit Writer 的 actor catalog、context estimate 与 RequestPlan；
6. 真实临时 Git workspace、production composition 与 latest-revision deterministic
   verifier。

`f8d0b242` 增加两个缺失的 current-revision 断言。production-composition 测试把每个 actor
的完整 `ModelRequestPrepared` 写入 StateStore，SQLite reopen 后逐字段相等，重算的
ContextBroker estimate 与 DeepSeek `RequestPlan` 也完全相同；root 有普通写工具，
read-only child 无写工具，explicit Writer 有写工具，二者仍使用角色对应的 agent
admission。Runtime 测试则让 mandatory task constraints 单独超过 hard limit，结果为 typed
`ContextLimitExceeded(hard_limit=1000)`，ModelPort 调用、logical/physical request、
compaction 和 prepared request 全部为 0。`b1894069` 只修复测试代码的 strict Clippy
表达式，不改变这些断言或 production 行为。

Harness 不实现 estimator、reducer、planner 或 terminal classifier，只运行 16 个 exact
Rust test filters 并记录进程 verdict。formal output 在 clean `f8d0b242` 前后保持同 revision
与 tree，13/13 cases、16/16 gates 通过。ignored result：

- 路径：`eval/results/m7-i-near-limit-context-6e9e9b7b-v1.json`；
- mode/size：`0600` / 3,616 bytes；
- SHA-256：`aac12ec7c16b8ce28d5acf0cd2b5753297a838b12d68fbd46382ab3edfd5dc2c`；
- `credential_read=false`、`official_api_requests=0`、`network_accessed=false`；
- `material_production_delta=false`、`product_metric_eligible=false`。

结论为 **close_request_token_optimization_and_admit_m8_cleanup**。本切片没有模型 treatment，
付费请求不能增加归因力，因此未读取 Key、未调用 API。它只建立 current v16/v21 correctness
baseline；不证明一般任务的 Token、请求、wall time、费用或 verified success 已改善，也不
把 historical zero compaction/hard-budget 样本扩张为频率结论。manifest 与 Harness：

- `eval/manifests/m7-i-near-limit-context-v1.json`；
- `scripts/eval-m7i-near-limit.py`。

完整身份、调用图、门禁和非结论见
[M7-I near-limit context/request budget 结论](../../eval/summaries/m7-i-near-limit-context-2026-07-23.md)。

### 9.17 M8-A DeepSeek-only production 配置与入口（2026-07-23）

M8-A manifest 固定 clean production baseline `8b650356`、Run API v10、RuntimeEvent v16、
State v21、exec-stream v2、外部 target `/private/tmp/codewhale-m8a-target`、
`CARGO_NET_OFFLINE=true` 和 `maximum_reruns=0`。P01-P12 矩阵覆盖：

1. 隔离 HOME 首启、无 Key、DeepSeek-only template 与中文恢复；
2. 非 DeepSeek Provider/模型、非法 endpoint/TLS/header 在 spawn/RunStore/network 前拒绝；
3. CLI -> config -> keyring -> env 的 credential precedence 与秘密脱敏；
4. resume/SQLite reopen 不重复模型请求或写入；
5. Doctor/onboarding、真实 PTY、exec/HTTP/stdio machine schema parity；
6. root、read-only child、explicit Writer 使用同一 DeepSeek backend/Runtime/Store；
7. generic Provider、OAuth、catalog/pricing/alias/route source 与依赖物理删除。

三个 production 提交分别为 `e1a611ff`、`63246e72`、`00dcda0c`。从起始到代码 cutover
共 71 files、3,689 insertions、42,712 deletions，净删除 39,023 行。CLI、TUI 和
`crates/config` 已各自通过 targeted tests/check，`config.example.toml` 另由当前
`ConfigToml` 与交互 TUI 双重解析验证；完整 focused、fmt、workspace Clippy/test、real
PTY、surface parity 与 crash/reopen 门禁通过。

该切换没有改变 DeepSeek request、response、reasoning、prompt、tool catalog、budget 或
accounting surface，不存在可比较的 paid model treatment。冻结 credential gate 因而在
Key path 前停止：`credential_read=false`、`official_api_requests=0`、
`external_network_accessed=false`、`loopback_only=true`、`maximum_reruns=0`。结论为
**keep_deepseek_only_cutover / delete_generic_provider_paths**；不报告 Token、时间、费用或
一般编码成功率收益。

manifest、ignored `0600` result 与完整结论：

- `eval/manifests/m8-a-deepseek-only-entry-v1.json`；
- `eval/results/m8-a-deepseek-only-entry-8b650356-v1.json`；
- [M8-A DeepSeek-only production 配置与入口结论](../../eval/summaries/m8-a-deepseek-only-entry-2026-07-23.md)。

### 9.18 M8-B CodeWhale 产品身份与本地交付（2026-07-23）

M8-B manifest 固定 clean production baseline `2ed3efe3`、Run API v10、RuntimeEvent v16、
State v21、exec-stream v2、外部 target `/private/tmp/codewhale-m8b-target`、
`CARGO_NET_OFFLINE=true` 和 `maximum_reruns=0`。D01-D12 覆盖：

1. pinned Rust 1.97.0 + Cargo.lock 的 clean locked/offline source build；
2. product/version/target/full revision/tree/Cargo.lock/toolchain 与准确 binary set；
3. outer archive、inner binary tamper、wrong target 和路径/symlink 拒绝；
4. fresh install、首次 `--version`、upgrade、rollback、verify、uninstall 与数据保留；
5. macOS/Linux、offline/no-network、旧 binary/env/config/state/release alias 拒绝；
6. canonical Runtime/Store/protocol parity 与旧 delivery owner 物理删除。

四个 production cutover 为 `eddfd4bc`、`ccc98245`、`d792113e`、`4aff11f6`；三个离线
toolchain 闭合提交为 `28f8a34c`、`e4232142`、`307f6c09`。代码 candidate 相对
`2ed3efe3` 共 59 files、1,574 insertions、3,276 deletions，净删除 1,702 行。

真实 macOS source artifact：

- file：`codewhale-0.8.68-aarch64-apple-darwin-307f6c09d082.tar.gz`；
- size：17,660,358 bytes；
- SHA-256：`af3cae6ae254f0331162aa475ab2201b659dc9ef68264822ea5321d3c6c48115`；
- manifest identity：candidate `307f6c09d082d80c85daa43a5958b9e795c33d10`、tree
  `247fc8c3499dfc1de5345c0c20e7a68443343d61`、Cargo.lock
  `0699519a54e34db65457a56c143be8bfd89fdb96bdae7dde01a7a2540788f4ba`；
- binaries：`codewhale` 15,695,008 bytes、`codewhale-tui` 22,522,400 bytes。

同 identity + binaries 的 fixture package SHA 可复现。真实 artifact 的 install、verify、
Doctor 和 uninstall 在 macOS `sandbox-exec` 禁网下通过；同一 lifecycle self-test 在缓存
Linux container `--network none` 下通过。Linux source release build 由 CI matrix 拥有，
没有把它伪装为本机已观察事实。uninstall 删除程序与 delivery metadata，保留的
`CODEWHALE_HOME` sentinel byte-identical。

ignored result：

- path：`eval/results/m8-b-product-delivery-2ed3efe3-v1.json`；
- mode/size：`0600` / 8,474 bytes；
- SHA-256：`8c9f818674e1d739a707f538510e7db8c6c99e69c3de13a78fa75a2fecb065a4`；
- manifest SHA-256：`b960d2ba7620c560b82bda729385993e6a44b2554cc855990820a9d76e034239`；
- 12/12 matrix cases passed；
- `credential_read=false`、`official_api_requests=0`；
- `material_model_treatment=false`、`product_metric_eligible=false`。

最终门禁包含 delivery macOS/Linux self-test、真实 source release lifecycle、focused、
fmt、workspace strict Clippy/test、hermetic TUI 两次、canonical/QA/release PTY、
exec/HTTP/stdio parity、root/read-only/Writer 和 process SIGKILL/reopen；全部通过。

一次 candidate 后的只读 binary-list gate 因命令漏设 `RUSTUP_TOOLCHAIN=stable` 触发
rustup 1.97.0 channel 更新/下载探测，并被立即中断。该命令没有读取 Key、调用 DeepSeek、
写入 package/install/state/result 或修改仓库；但结果必须披露它，不能把“交付命令在
OS 禁网下通过”扩张为“整个 Agent session 从未尝试外网”。随后同一断言在 stable/offline/
network-denied sandbox 中通过。

结论为
**keep_single_codewhale_identity_and_delivery_owner / shrink_imported_delivery_paths**：
保留唯一 CodeWhale identity、`.codewhale` state/config、准确两项 binary delivery 与
immutable/atomic lifecycle；删除 `crates/release`、imported updater/CNB、`codew`、旧产品
env/path compatibility、第二 metrics truth、重复配置/开发入口和未接线部署资产。不报告
verified coding success、Token、时间或 API cost 改善，也不声称 fixed `zh-Hans` 或中文
Agent prompt 已完成。

manifest、ignored result 与完整结论：

- `eval/manifests/m8-b-product-delivery-v1.json`；
- `eval/results/m8-b-product-delivery-2ed3efe3-v1.json`；
- [M8-B 产品身份与本地交付结论](../../eval/summaries/m8-b-product-delivery-2026-07-23.md)。

### 9.19 M8-C 固定 zh-Hans 产品界面（2026-07-23）

M8-C manifest 固定 clean production baseline `e99bf6c7`、Run API v10、RuntimeEvent v16、
State v21、exec-stream v2、外部 target `/private/tmp/codewhale-m8c-target`、
`CARGO_NET_OFFLINE=true` 和 `maximum_reruns=0`。L01-L14 覆盖：

1. 恰好一个 shared `zh-Hans` catalog/MessageId owner，无 locale 检测/选择或翻译请求；
2. CLI、TUI、exec、app-server help 在 foreign locale 下使用中文且身份不变；
3. 首启、无 Key、invalid config、retired command、Headless 与 typed recovery；
4. Doctor human/JSON、exec text/stream-json、HTTP/SSE/stdio paired stability；
5. approval、`request_user_input`、root/read-only/Writer 与 crash/reopen；
6. 80/120 列 CJK width/wrap/cursor/hit target 和 raw sentinel；
7. real installed two-binary artifact identity、verify 与 uninstall/data boundary；
8. TUI 私有 localization owner 物理删除，canonical Runtime/Store/protocol 零差异。

六个 code/cutover 提交为 `062a747d`、`af8b7c3e`、`413d4ae1`、`6d7ebe74`、
`23e33980`、`44b17940`。candidate 相对 `e99bf6c7` 共 31 files、2,741 insertions、
963 deletions，净增加 1,778 行；catalog 从 TUI 私有 167 keys 收敛为共享 427 keys。
正增量只记录一个 exhaustive interface contract、真实 caller 迁移和回归测试成本，不作为
Agent 能力收益。

真实 macOS locked/offline source artifact：

- file：`codewhale-0.8.68-aarch64-apple-darwin-44b17940846c.tar.gz`；
- size：17,733,507 bytes；
- SHA-256：`0578404370cf07af86d73e2d8f6a5c15cefa62e888319fbc6382b705678b660c`；
- manifest identity：candidate `44b17940846ceb5f946337623e7554768ee2e75e`、tree
  `f44440985c27640f7388c0edc76768841851aa66`、Cargo.lock
  `be75e11ee7b1a879908cd5a4ae8a615a45d965d63822115c8de2db7e1294eeac`；
- binaries：`codewhale` 15,827,216 bytes、`codewhale-tui` 22,588,464 bytes。

artifact install/verify 后，两个 `--version`、CLI/TUI/app-server help、Doctor text/JSON、
missing Key 和 retired command 在 `LANG=C/LC_ALL=C` 下通过；uninstall 删除程序并保留
隔离 `CODEWHALE_HOME`。冻结 representative outputs 的非白名单英文产品文案泄漏为 0；
白名单只包含技术身份、machine fields/status 和 raw 外部内容，不做全仓 ASCII 扫描。

ignored result：

- path：`eval/results/m8-c-fixed-zh-hans-interface-e99bf6c7-v1.json`；
- mode/size：`0600` / 8,986 bytes；
- SHA-256：`a07596ffb0d7ca18398fa0a90a53946199712fa4ad6ffe9bc02ff08091df7ee2`；
- manifest SHA-256：`d92792d8e5de7b869c0def6a003a3421f74edf85fd8e3c8539e0cdeaf4ad48e9`；
- 14/14 matrix cases passed；
- `credential_read=false`、`official_api_requests=0`；
- `material_model_treatment=false`、`product_metric_eligible=false`。

最终门禁包含 fixed-zh-Hans static/targeted、focused、fmt、workspace strict Clippy/test、
TUI hermetic 两次（每次 889 passed、1 ignored）、canonical/QA/release PTY、exec acceptance、
HTTP/stdio parity、app-server、root/read-only/Writer、process SIGKILL/SQLite reopen、
delivery self-test 与真实 artifact lifecycle；全部通过。`crates/protocol`、
`crates/runtime`、`crates/state`、`crates/app-server` 相对 baseline 的 diff 为 0。

结论为
**keep_single_fixed_zh_hans_owner / shrink_duplicate_product_text_paths**：保留一个 CLI/TUI
共享 compile-time catalog、Host-owned 中文 help/Doctor/recovery/multi-Agent chrome、明确
raw/machine 非翻译边界和 CJK/installed-artifact 门禁；删除 TUI 私有 wrapper/catalog 与
覆盖到的 direct English owner。不报告 verified coding success、Token、时间或 API cost
改善，也不声称中文 Agent prompt 已获得收益。

manifest、ignored result 与完整结论：

- `eval/manifests/m8-c-fixed-zh-hans-interface-v1.json`；
- `eval/results/m8-c-fixed-zh-hans-interface-e99bf6c7-v1.json`；
- [M8-C 固定 zh-Hans 产品界面结论](../../eval/summaries/m8-c-fixed-zh-hans-interface-2026-07-23.md)。

### 9.20 M8-D 中文原生 Agent prompt A/B（2026-07-24）

M8-D 冻结一个且仅一个模型 treatment：把 current production constitution 的固定五步
checklist 替换为两句事实缺口循环。Host UI、tool schema、Runtime/RunStore、协议、权限、
任务、预算、retry、pricing 和 accounting 均不变。所有正式 suite 绑定：

- source/binary revision `8371b6dd9ac5d18570ff81a28bd94cc93372c372`；
- source tree `9fc34ee9dcd1837d361a37bbef174799c9241624`；
- binary pair SHA `30288ee19b48d2941cb0cbcfeedb2d9fef8eadcbfa5a9c78352347411f87939c`；
- candidate prompt SHA `2c3b8018a11c49ca1e0ae9b331ef12621aec77a3d56c631c01e535a74ec50ebf`；
- `deepseek-v4-flash`、`reasoning_effort=high`、当前官方
  `https://api.deepseek.com/chat/completions`；
- 5 tasks × 2 variants × 3 runs、30 arms、`maximum_reruns=0`。

2026-07-24 复核官方 Create Chat Completion、Models & Pricing、Thinking、Tool Calls、
Change Log 和 FIM 文档。当前 OpenAI-compatible base URL 与 `/chat/completions` 路径保持；
退役的是 `deepseek-chat`/`deepseek-reasoner` 旧模型别名。M8-D 未使用两个旧 alias，也未
调用独立 Beta FIM surface。

真实 caller 审计发现 app-server 没有加载已有 context-owned prompt override；`8371b6dd`
让 app-server 与 exec/TUI 共用同一 loader，并用外部 stdio 进程、loopback 禁网、SIGKILL
和 State reopen 证明 durable `RunCreated` prompt。该修复独立于 candidate 收益，保留。

五个 successor suite 均使用全新 schema、Harness revision 和不可覆盖 0600 output：

1. v1 完成 2 arms 后发现两臂真实 prompt 相同，按 measurement invalid 停止；
2. v2 完成 5 arms，t5 的 Host/verifier/accounting 实际成功，但旧 child/task/usage
   projection 错判；
3. v3 完成 6 arms，全部 valid/verified；首个 Writer 在 API 前因错误 fixture Git identity
   失败，shared error 逃出旧 handler，raw 保持 `running`；
4. v4 完成 7 arms；single/read-only 6/6 valid/verified，首个 Writer canonical
   integration/verifier/cleanup/reopen 成功，但旧扁平 AgentTask 与 `git status` scope
   产生测量层 false success；
5. v5 已离线修正 nested AgentTask 与 clean integrated `base..HEAD` scope，但首个 baseline
   arm 后 accounting 为 `billing_unknown=true`、`complete=false`、`surface_usage=[]`，
   立即 `aborted_unknown_billing`，没有启动第二 arm。

v1-v4 可证明费用下界为 `$0.052454002`；v5 费用未知，不能补算或宣称总费用。旧 suite
不得补 mate、续跑或拼样。v3/v4 的三个完整 diagnostic pairs 对 Token 的 paired median
方向分别为改善 23.50% 和退化 12.33%，进一步说明不完整小样本不能支撑产品结论。

最终决策是
`hold_prompt_candidate / keep_app_server_override_consistency`：production bundled prompt
保持不变，没有 candidate production branch 进入默认路径；保留 fixture、Harness、
manifests 与 raw 作为可复核证据。M8-D 不证明 verified success、false success、工具恢复、
Token、cache、wall time 或费用有净变化。

manifest/raw/summary：

- `eval/manifests/m8-d-prompt-ab-v1.json` 至 `m8-d-prompt-ab-v5.json`；
- ignored `eval/raw/m8-d-prompt-formal-*.json`（全部 0600）；
- [M8-D 中文原生 Agent prompt A/B 结论](../../eval/summaries/m8-d-native-zh-prompt-ab-2026-07-24.md)。

### 9.21 M8-E V1 退出证据总审计（2026-07-24）

M8-E 不是模型 A/B，而是 PRODUCT_PLAN 16 项 V1 完成定义的 release evidence audit。
frozen production revision 为 `433a871b9e26a09009d99575594c29557ebc7484`，contract
revision 为 `a12bea45f20668e7ba58aebee4a6267ff4f70233`。manifest 固定：

- exact revision/tree、Cargo.lock、Rust 1.97.0、Run API v10、RuntimeEvent v16、
  State v21、exec-stream v2；
- frozen revision 上 PRODUCT_PLAN/ROADMAP/EVALUATION/CURRENT_CODEWHALE 四个 blob hash；
- 16 项 gap matrix、8 pass / 8 blocked 和 `not_releasable`；
- `maximum_reruns=0`、external Cargo target、offline、no Key、0 official request；
- ignored 0600 result 的唯一输出路径。

通过项是 V01/V02/V03/V04/V05/V07/V11/V14；阻塞项是
V06/V08/V09/V10/V12/V13/V15/V16。主要 blocker 是 Fleet/Lane/Orchestrator 多个
TaskGraph 产品概念、multi-Writer 未准入、FIM 无 canonical caller/parser/apply、
RepoGraph 缺失、重复状态/词汇未完全删除、没有 imported-baseline coding/workflow
证据，以及 M8-D prompt 仍为 unknown-billing hold。

2026-07-24 复核官方
[Change Log](https://api-docs.deepseek.com/updates)、
[V4 发布说明](https://api-docs.deepseek.com/news/news260424/)、
[Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)、
[Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion) 和
[Anthropic API](https://api-docs.deepseek.com/guides/anthropic_api/)。
`deepseek-chat`/`deepseek-reasoner` 是退役旧 alias，不等于 ChatCompletions surface
退役。M8-E frozen manifest/result 中的 `user_selected_release_target` 前提后来被用户
明确否认，PRODUCT_PLAN 与 ADR 也没有 Anthropic 发布决策；该字段只保留为 frozen
历史输入，不能再驱动产品结论。CodeWhale 保留官方 DeepSeek ChatCompletions。V09 仍因
FIM 没有 canonical caller/parser/apply 而 blocked，不因 Anthropic 兼容面 blocked。

从 clean `a12bea45` 生成 locked/offline artifact：

- file `codewhale-0.8.68-aarch64-apple-darwin-a12bea45f206.tar.gz`；
- tree `966afe2fde6e4af788409c992fc78e872a96acc9`；
- artifact SHA-256 `6104e45023ff8c8594f60645da28f57c18913ec4e9249c0900569c3020a86ffc`；
- inner binary 只有 `codewhale` 与 `codewhale-tui`，SHA 分别为
  `bee2414a…afba9`、`f8266dd1…9262f`；
- real install/verify 和两项 `0.8.68 (a12bea45f206)` version identity 通过。

focused、fmt、workspace clippy/test、exec 25、canonical TUI PTY 7、Run 18、
exec/HTTP/stdio parity、root/read-only/Writer、process crash/reopen、M8-E contract 8/8、
delivery self-test 和 real package install/verify 全部通过。这些是 baseline consistency
证据，不是缺失 V1 产品能力的替代证据。

最终决策为
`not_releasable / keep_canonical_chain_and_delivery / hold_release`。M8-E 不读取 Key，
官方请求 0，不产生 product metric。错误前提上的 M8-F 为
`canceled_invalid_premise`：其 manifest/专属测试及未提交 production WIP 已删除，
没有读取 Key 或调用官方 API。后续 credentialed test 只能属于真实产品 treatment，
并继续要求 immutable binary、预注册 manifest 和 billing completeness。

manifest/result/summary：

- `eval/manifests/m8-e-v1-exit-audit-v1.json`；
- ignored 0600 `eval/results/m8-e-v1-exit-audit-433a871b-v1.json`；
- [M8-E V1 退出证据总审计](../../eval/summaries/m8-e-v1-exit-audit-2026-07-24.md)。

### 9.22 M8-G 单一 TaskGraph 收敛（2026-07-24）

M8-G 是 offline architecture cutover，不是模型 A/B。baseline `650df581` 与 code
candidate `64f6bc16` 固定同一 Run API v10、RuntimeEvent v16、State v21、exec-stream
v2、Rust 1.97.0 和唯一 DeepSeek ChatCompletions production chain。

验收口径只使用 canonical facts：

- root、read-only child 和 explicit Writer 继续通过同一 Runtime/Store conformance；
- exec、HTTP/SSE、stdio 与 TUI 继续投影同一 Run；
- RunStore reopen、process SIGKILL recovery 和 Writer integrate/cleanup 不退化；
- Fleet/Lane protocol、config、state、command、UI 和 skill 不存在可调用 consumer；
- setup-state 旧 schema fail closed，不增加 compatibility reader 或 dual write；
- workflow surface 与代码复杂度必须下降。

结果：

| 指标 | baseline | candidate |
|---|---:|---:|
| visible CLI commands | 20 | 18 |
| setup steps | 7 | 6 |
| changed source | — | 48 files，`+150/-15,260` |
| V06 | blocked | pass |
| Key / official requests | 0 / 0 | 0 / 0 |

focused、workspace strict Clippy/test、root/read-only/Writer、exec/HTTP/stdio parity、
SQLite reopen、SIGKILL recovery、fmt 与 diff gate 全部通过。决策为
`keep_canonical_taskgraph / delete_fleet_lane / close_V06`。V12 只标记为
`improved_but_not_closed`；V16 的局部步骤下降不能冒充 imported-baseline workflow
A/B。没有 verified coding success、Token、cache、wall time 或费用结论。

manifest/summary：

- `eval/manifests/m8-g-taskgraph-convergence-v1.json`；
- [M8-G 单一 TaskGraph 产品概念收敛](../../eval/summaries/m8-g-taskgraph-convergence-2026-07-24.md)。

### 9.23 M8-H FIM 产品范围与历史债收敛（2026-07-24）

M8-H 的 baseline 是 clean `dce858d0`，code candidate 是 `7d9aa9a6`。预注册
manifest 不把 FIM 优于 patch/edit 作为前提，而是要求先出现新的 current-v16
production 编辑失败证据和同 immutable binary treatment。

准入事实为：

| 事实 | 数量/状态 |
|---|---:|
| M7-C deterministic Host edit matrix | 12/12 |
| M7-D 后新的 current-v16 production 编辑样本 | 0 |
| canonical FIM caller/parser/apply/reopen | 0 / 0 / 0 / 0 |
| planner URL 被唯一 sender owner 接受 | false |
| credential read / official API requests | false / 0 |

`plan_fim` 生成 `/beta/completions`，但 sender 只拥有 Standard/Beta Strict Chat URL，
完整 response parser 只读取 Chat `choices[0].message`。这条分支不能产生可归因
treatment，也不能安全写盘或在 RunStore 重开。因此 live A/B 在 credential/API 前为
`inadmissible_no_surface_delta`，不产生 success、Token、wall-time 或费用指标。

candidate 物理删除：

- FIM planner/error/surface/accounting 和 always-zero exec/eval 字段；
- evaluator 中不在当前允许目录的 `fim_edit`/`write_file` classifier；
- 退役模型 alias 的静默映射与 speculative future-model pass-through；
- TUI 重复 model catalog switch、永远为 `None` 的 alias retirement DTO/Doctor/JSON/文案。

exec-stream 因 terminal 字段收缩从 v2 升到 v3，不保留兼容 reader；RuntimeEvent v16 与
State v21 不变，现有有效 Standard/Strict event 形状和 reopen 不变。决策为
`keep_standard_strict / reject_unreachable_fim_production_half_branch /
hold_fim_reentry`。V09 仍为 `blocked_product_scope_decision`：PRODUCT_PLAN 的 FIM 完成面
未满足，不能以删除半分支伪造 pass。下一项可归因实验是 imported `352e86a6` 的同任务
coding/workflow-step A/B。

该 fallback 的离线 preflight 已继续执行：exact imported revision 能构建 immutable
release binary，也能通过显式 path suffix 使用相同 `/chat/completions` 与
`deepseek-v4-pro`。但其 exec terminal schema v1 没有 `api_request_count`、
`cost_complete`、费用 bucket 或可重开的 root/child started request ledger，
`retry_count` 也是 `null`。因此无法满足完整逐 arm accounting，live A/B 在 credential
前判定 `inadmissible_incomplete_baseline_accounting`；不修改旧 revision，不读取 Key，
官方 API 请求仍为 0。

manifest/summary：

- `eval/manifests/m8-h-fim-scope-debt-v1.json`；
- [M8-H FIM 产品范围与历史债收敛](../../eval/summaries/m8-h-fim-scope-debt-2026-07-24.md)。

### 9.24 M8-I Host typed Auto 路由（2026-07-24）

M8-I manifest 在 production 变更前固定 clean `15fea38e`、tree、Cargo.lock/toolchain、
旧 classifier 文件/composition hash、maximum_reruns=0 和四 variant 正式 A/B：

| Variant | 语义 |
|---|---|
| A | fixed `deepseek-v4-pro` 产品基线 |
| B | fixed `deepseek-v4-flash` 诊断 |
| C | frozen `15fea38e` Flash prompt classifier |
| D | Host typed 保守 policy candidate |

离线 contract 不测 prompt “难度”，只测 typed product facts：

- explicit model/reasoning 精确不变；
- Auto root Pro/high，typed recovery Pro/max；
- 普通 read-only child Flash/high，失败后的新 recheck Pro/max；
- explicit Writer 仍 explicit-only，Auto selection Pro/high，typed rework Pro/max；
- route audit、actor/workspace authority、child binding、request ledger 和 SQLite reopen；
- pending Start 在 0 次 pre-runtime model request 下恢复同一 reserved Run；
- CLI/TUI/HTTP/stdio parity 与 process SIGKILL recovery。

code candidate `ef65bafa` 的离线结果：

| 机制指标 | 旧 classifier | Host policy |
|---|---:|---:|
| first root turn 前物理请求 | 1 | 0 |
| first root turn 模型 | classifier 决定 | Pro |
| `RunCreated` accounting baseline requests | 1 | 0 |
| creation 前网络/unknown-billing 窗口 | 有 | 无 |
| child route | whole-tree inheritance | frozen per actor |
| SQLite reopen 重新路由 | 不允许但需旧结果 | 不需要，exact audit |

这些是 correctness/request-count mechanism evidence，不是质量或成本产品指标。正式 A/B
要求 fixed Pro 非劣、false success=0、关键 strata 无新增 Pro-only success，并在质量通过
后费用或 wall time 稳定净改善约 20%。但 C 与 D 分属不同 revision；cutover 后没有一份
immutable binary 同时承载 A/B/C/D 的真实 production surface。恢复 classifier toggle 会
重新引入第二 route owner/产品模式，违反预注册删除边界。因此在读取 Key 前判定
`inadmissible_no_single_binary_four_variant_surface`，official requests=0。

产品结论：

- `keep_explicit_models_and_host_policy`；
- `delete_prompt_classifier_and_unknown_billing_preroute_debt`；
- `hold_auto_default_admission`；
- product default 固定 `deepseek-v4-pro`，显式 Auto 不附带成功率/成本收益声明。

这是 M8-I 当时的历史结论；M9-D/ADR-0008 已 supersede 其 Auto future-admission 条款并
删除 Auto 产品方向，冻结证据本身不改写。

manifest/summary：

- `eval/manifests/m8-i-host-auto-route-v1.json`；
- [M8-I Host typed Auto 路由](../../eval/summaries/m8-i-host-auto-route-2026-07-24.md)。

### 9.25 M8-J V1 successor 与 V12 历史债删除（2026-07-24）

M8-J 不改写 M8-E 的 frozen 8 pass / 8 blocked 输入，而是以 clean `bce36a53`、
Run API v11、RuntimeEvent v17、State v22、exec-stream v3 重算 current successor。
M8-G 已使 V06 `blocked -> pass`；M8-H 只收敛不可达 FIM 半分支，V09 仍 blocked；
M8-I 不对应 V1 exit item。因此候选前 current matrix 为 9 pass / 7 blocked。

manifest commit `9e644add` 在 production 变更前冻结唯一切片：

| 项目 | 冻结值 |
|---|---|
| blocker | V12 no-consumer provider / legacy state vocabulary |
| baseline | `bce36a53` |
| code candidate | `bcbc1616` |
| candidate protocol | Run API v11 / RuntimeEvent v17 / State v23 / exec-stream v3 |
| live treatment | 无；确定性状态真相删除 |
| Key / official requests | 0 / 0 |

caller graph 证明旧 `codewhale thread` 的八个子命令读写独立 `threads` metadata 表，
resume/fork 又委托已退休的 TUI thread 语义；`StateStore` 还维护
`session_index.jsonl` sidecar。protocol 根部的 Thread/App/Prompt/EventFrame DTO 只有
自身 parity test，没有 production consumer。相反，canonical
`RunEnvironment.provider="deepseek"` 参与 environment fingerprint / replay safety，
execpolicy 的 network policy types 也有真实 caller；两者保留，不把字段名当删除依据。

candidate 先让真实入口继续使用既有 `runs`、`resume` 与
`exec --resume/--continue`，再物理删除旧 CLI dispatch、thread CRUD/session index、
无消费者 DTO、文案和测试。State `v22 -> v23` 在同一 `IMMEDIATE` transaction 中删除
旧表，fresh v23 不创建它；精确迁移测试证明 current canonical run 原样 replay，
pending Start、route audit 与 accounting 不变，注入 drop failure 时 schema version
和旧对象一起回滚。九种旧 spelling 在 config、TUI、Store、credential/model 前
fail closed。

离线门禁覆盖 targeted state/protocol/CLI、focused production composition、
root/read-only/Writer、HTTP/SSE/stdio/exec、SQLite reopen、SIGKILL recovery、fmt、
workspace strict Clippy/test 与 diff check。该候选相对 baseline 的代码切换为 11 files、
`+232/-1,797`，净删除 1,565 行；没有模型 request surface delta，正式 paid A/B
不适用，不能据此声称 verified success、Token、时间或费用提升。

决策为：

- `keep_canonical_runstore`；
- `shrink_delete_legacy_thread_truth`；
- `close_V12`。

current V1 matrix 为 10 pass / 6 blocked；剩余 V08 multi-Writer、V09 FIM scope、
V10 RepoGraph、V13 imported-baseline coding A/B、V15 billing-provable 中文 prompt A/B
与 V16 imported-baseline workflow-step A/B。

manifest/summary：

- `eval/manifests/m8-j-v1-successor-v12-debt-v1.json`；
- [M8-J V1 successor 与 V12 历史债删除](../../eval/summaries/m8-j-v1-successor-v12-debt-2026-07-24.md)。

### 9.26 M8-K V08/V09/V10 产品范围 successor（2026-07-24）

M8-K 不改写 M6-B1、M5-B、M7-C、M8-E、M8-H 或 M8-J 的 frozen evidence。
manifest `m8-k-v1-scope-successor-v1.json` 绑定 clean `49a46581`、authority/source
hash、Rust 1.97.0、current 10 pass / 6 blocked matrix，以及如下统一实施准入门槛：

1. latest owning decision 之后存在 current production failure；
2. failure 可归因于缺少候选能力，并有 canonical caller；
3. affected task 有 deterministic verifier；
4. control/treatment 位于同 revision immutable binary；
5. physical request、usage、retry、wall time 与 cost accounting 闭合；
6. 候选有唯一 owner、真实 caller 迁移和 cutover 删除。

只读结果：

| Item | 既有正式证据 | current 缺口 | 准入 |
|---|---|---|---|
| V08 | M6-B1 v2 为 `reject_and_rework`；v3 unknown billing；single Writer explicit-only | multi-Writer attributable failure=0，treatment=0 | 不实施 |
| V09 | M7-C Host edit matrix 12/12；M8-H 删除不可达 FIM 半分支 | 新编辑生成失败=0，FIM caller/parser/apply/reopen=0 | 不实施 |
| V10 | M5-B on/off 均 6/6 verified；失败未定位为结构检索 | RepoGraph caller=0，结构检索归因失败=0 | 不实施 |

同日官方 DeepSeek 复核继续证明：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion) 使用
  `POST /chat/completions`；
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls) 的 Strict 是 Beta Chat，
  且整组 function 都必须 strict-compatible；
- [FIM Completion](https://api-docs.deepseek.com/guides/fim_completion/) 与
  [Create FIM Completion](https://api-docs.deepseek.com/api/create-completion) 仍定义
  独立 Beta `/completions`、`choices[].text` 和 4K output limit；
- [V4 release](https://api-docs.deepseek.com/news/news260424/) 要求保持 official base URL
  并改用 `deepseek-v4-pro`/`deepseek-v4-flash`；2026-07-24 下线的是旧 model aliases，
  不是 ChatCompletions。

ADR-0005 因而只调整 V1 的 capability scope，不改变固定架构或 production source：

- V08 以一个 explicit isolated Writer 的完整 Host lifecycle 验收；
- V09 以 Standard Chat 与 Strict whole-catalog admission/lossless fallback 验收；
- V10 以 canonical search/read/diff、bounded ContextBroker 和 deterministic verifier
  的跨文件结果验收。

multi-Writer、FIM 与 RepoGraph 分别为 `reject_as_v1_literal / hold_evidence_gated_reentry`。
V08/V09/V10 关闭后 current matrix 是 13 pass / 3 blocked；V13/V15/V16 仍 blocked。
本切片没有 material model treatment，live A/B 为
`inadmissible_no_material_treatment`，credential read=false，official requests=0。
这不证明三种优化质量较差，也不产生 verified-success、Token、时间或费用收益结论。

manifest：

- `eval/manifests/m8-k-v1-scope-successor-v1.json`。

accepted scope candidate 为 `6a99cb79ee1a63c57215456b5f0f040659b9426d`，tree
`60e80a06c8d0a0f4b097a6af6d22a5df39205d25`。完整结论见
[M8-K V1 产品范围 successor](../../eval/summaries/m8-k-v1-scope-successor-2026-07-24.md)。

### 9.27 M8-L V13/V16 release benchmark successor（2026-07-24）

M8-L 从 clean `4fef6a34` 独立复核 imported `352e86a6` 的 live A/B 准入。task fixture、
external deterministic verifier、official `/chat/completions`、`deepseek-v4-pro`、
binary/prompt/tool identity 与 wall-time boundary 都可对齐；baseline 原生 accounting
不能对齐：

| Fact | imported exec-stream v1 | current exec-stream v3 |
|---|---|---|
| physical request count | absent | root + child started |
| retry count | field exists but every terminal writes `null` | transport/runtime typed |
| failed/incomplete response usage | absent from terminal truth | typed completeness/missing/incomplete |
| cost | separate observed-turn scorecard only | surface/model bucket + completeness |
| crash/reopen | Engine/SessionManager aggregate | canonical RunStore request ledger |

旧 Engine 至少有 inner/outer 两层透明重发，最终 `TurnComplete` 只携带 observed aggregate
usage。外部 estimator 无法区分“未发生”和“发生但旧 schema 未记录”，修改旧 revision 又会
改变被测 baseline。因此正式 paid A/B 在 credential 前 fail closed：

```text
decision = inadmissible_incomplete_baseline_accounting
credential_read = false
official_api_requests = 0
maximum_reruns = 0
```

ADR-0006 不把这个 inadmissibility 伪装成 current superiority，而是冻结 release evidence
chain：

1. qualified real-model evidence 是 M5-A 12/12 official DeepSeek arms。coding baseline /
   candidate 均 3/3 verified，candidate false-success 0；forced false claim baseline
   3/3 false-success，candidate 3/3 correct rejection；12/12 measurement/contract/
   completion/accounting valid，总计 40 requests、144,903 tokens、USD 0.004110153。
2. exact current candidate 必须通过五个 offline production gates：

   - 临时 Git repo 中 `before -> broken -> after`，Host verifier fail 后 recovery，
     latest-revision receipt、5 physical requests 与 SQLite no-request reopen；
   - frozen verifier fail 时拒绝 model completion；
   - root/read-only/explicit Writer RequestPlan 与 accounting 从 SQLite exact rebuild；
   - current public help 只保留 canonical `runs`；
   - resume 同一 terminal 不再发 model request。

3. 未来 prompt/model/reasoning/catalog/budget/routing/surface treatment 仍必须有独立 current
   same-revision immutable A/B；M5-A 不能替代新 treatment 证据。

V16 使用 atomic user action 作为单位：login、interactive start、headless coding、
run inspection、resume 在 imported/current 均为每项一次，总数 `5 -> 5`。内部 event、
model request 与 verifier step 不计作用户动作；已删除 Fleet/Lane/Thread/session truth
不恢复为 required workflow。

五个 current gate 与 M8-L contract/Harness 通过后，V13/V16 关闭，current matrix 为
15 pass / 1 blocked。V15 billing-provable Simplified Chinese Agent prompt comparison
仍 blocked，V1 仍不可发布。本结论不产生 current-vs-imported success、Token、time 或
cost delta，也不把 loopback 当作 live model quality。

manifest/summary：

- `eval/manifests/m8-l-release-benchmark-successor-v1.json`；
- [M8-L release benchmark successor](../../eval/summaries/m8-l-release-benchmark-successor-2026-07-24.md)。

### 9.28 M8-M billing-provable 中文 prompt successor（2026-07-24）

M8-M 不续跑或拼接 M8-D v1-v5。它从 clean `d1d6ca5c` 使用同一 immutable
`codewhale`/`codewhale-tui` binary pair、fixed `deepseek-v4-pro`、当前 Standard
ChatCompletions、5 个冻结任务和全新 30-arm schedule，从 position 1 比较 bundled
constitution 与唯一 fact-gap successor。两臂除 stable constitution text 外的
revision、TaskContract、actor/authority、workspace slot、tool catalog、reasoning、预算、
retry、cache controls、verifier 与 binary identity 必须逐对相同。

Harness 在读取 Key 前通过 19/19 contract tests、6/6 SIGKILL journal fault windows、
prompt-only production activation、no-credential SQLite reopen、focused、fmt、workspace
strict Clippy/test、process-level crash/reopen 和 immutable binary preflight。raw 采用
`O_EXCL|O_APPEND|O_NOFOLLOW`、目录/逐 record fsync 和 SHA-256 previous-record chain；
terminal、reopen、verifier snapshot 必须先 durable，最后才允许 arm observation。

正式 suite 的前 5 个 arm 全部 measurement valid、verified success，false success 为 0；
31 个 usage-bearing Standard Chat responses、144,976 input tokens、13,637 output tokens、
known-cost lower bound `$0.042682606` 均由 canonical ledger 闭合。第 6 arm（t5
candidate）的首个 root request 产生 typed `deepseek_transport`，没有收到 response
headers、content、finish reason 或 usage：

```text
started = 1
surface responses = 0
usage responses = 0
billing_unknown_attempts = 1
billing_unknown = true
complete = false
```

按 `maximum_reruns=0` 和 frozen accounting gate，Harness 在 6/30 arm 立即写入
`aborted_unknown_billing` 并退出；没有第 7 arm、续跑或自动重试。日志为 33 个完整
hash-chained records、0 partial-tail bytes，SHA-256
`641d7d80129b4673bd93eb4d4a0af57f278b326357af71c04188b8eae74acec7`。
该次无 usage attempt 是否计费无法证明，因此 5 个完成 arm 只是停止证据，不是
product-metric eligible aggregate；两个完整 pair 也不能满足 15-pair 硬门槛。

结论是
**hold prompt candidate / keep bundled prompt / do not resume or splice**。production
prompt 不切换；candidate-only M8-M runner/test 在记录冻结 hash 后删除，M8-D/M8-M
manifest、summary 和 ignored `0600` raw 保留。V15 仍 blocked，current V1 matrix 保持
15 pass / 1 blocked，V1 仍不可发布。

manifest/raw/summary：

- `eval/manifests/m8-m-prompt-successor-v1.json`；
- ignored `eval/raw/m8-m-prompt-successor-formal-d1d6ca5c-v1.jsonl`（`0600`）；
- [M8-M billing-provable 中文 prompt successor](../../eval/summaries/m8-m-billing-provable-zh-prompt-successor-2026-07-24.md)。

### 9.29 M8-N V15 fixed-Chinese baseline release-scope successor（2026-07-24）

M8-N 不创建第三个 prompt treatment，也不重跑、补 mate 或拼接 M8-D/M8-M。它先冻结
`PRODUCT_PLAN` 的两个不同 contract：

- 第 6.1 节是未来模型可见 prompt 候选的 admission rule：接管前必须 current
  same-revision 同任务 A/B，verified success 不回归且有明确净收益；
- V15 是 shipped fixed-Chinese baseline 的 release evidence：immutable 版本与回滚身份、
  合格 official DeepSeek coding/false-success evidence 和 exact-current production
  retention。

离线 Harness 固定 clean baseline `21200ccf`、constitution SHA-256
`39f2eeb30519e143eed2d4c627fcb97d323c6b95ad9060816a93b72ea994d409`、历史 evidence
hash/mode、7 个 current gates、locked/offline release artifact 和
`maximum_reruns=0`。审计结果是：

1. current constitution 与 M5-A baseline/candidate、M6-A Writer canary、M8-M
   immutable binary 中的 constitution byte-identical；
2. M5-A raw 为 `0600`，12/12 arms 均 product-metric eligible；coding 两臂 3/3
   verified，candidate false-success=0；forced-false-claim candidate 3/3 correct
   rejection，40 requests、144,903 tokens、`$0.004110153` accounting 完整；
3. M8-M stopped raw 保持 `0600` 和原 SHA；t1/t3/t5 三个 current baseline observations
   均 measurement-valid、verified、false-success=false，candidate benefit 仍不可计算；
4. M6-A Writer 只提供机制证据；current gates 另行覆盖 prompt provenance、
   root/read-only/Writer RequestPlan/accounting reopen、verifier failure/recovery 和
   false completion rejection；
5. 现有 delivery owner 绑定 source revision/tree、Cargo.lock、toolchain 和 binary
   SHA-256，并负责 immutable install、atomic current/previous、rollback 和保留用户数据的
   uninstall。prompt 不增加第二 version store，回滚等于 whole-release rollback。

M8-N 没有 material model-visible treatment，因此真实 A/B 在 credential 前判定
`inadmissible_no_material_treatment`。Key 未读取，official API requests=0，external
network=false。这个结论不把 `b088fd13` 的拒绝结果或 M8-D/M8-M hold 重写为中文优于英文、
candidate 更好/更差或等价；未来任何 prompt 语义候选仍必须完成自己的计费可证明 A/B。

ADR-0007 接受该 release-scope successor。失去消费者的 M8-D candidate-only app
test/current-tree fixture 删除；通用 app-server/exec/TUI override consistency、冻结 Git
history/manifests/summaries 和 `0600` raw 保留。V15 由 blocked 关闭为 pass，current
matrix 为 **16 pass / 0 blocked**，发布结论为 **V1 可发布**（release-ready，不执行
push/publish）。

manifest/result/summary：

- `eval/manifests/m8-n-v15-release-scope-successor-v1.json`；
- ignored `eval/results/m8-n-v15-release-scope-21200ccf-v1.json`（`0600`）；
- [M8-N V15 release-scope successor](../../eval/summaries/m8-n-v15-release-scope-successor-2026-07-24.md)。

### 9.30 M9-A Host Auto release admission（2026-07-24）

M9-A 用同一 immutable candidate `29c4980f` 比较 fixed Pro、fixed Flash diagnostic 与
Host Auto。三个任务都要求同 batch 启动恰好两个 read-only child；三臂的 TaskContract、
fixture、agent catalog、权限、reasoning、request/turn/tool/wall budget、verifier 与
maximum_reruns=0 相同。fixed Pro 是唯一质量基线；fixed Flash 不能 admission Auto。

Key 前通过 TUI explicit reasoning parity、Host route matrix、Pro-root/Flash-child loopback、
root/read-only/Writer accounting reopen、process SIGKILL、focused、fmt、workspace strict
Clippy/test、Harness fault matrix 和 immutable binary dry-run。raw 先 durable terminal 与
canonical store，再无凭据 reopen、external verifier，最后才派生 arm result。

正式 suite 第 18 arm 的第一个 fixed Flash root request 在任何 response headers、content、
finish reason 或 usage 之前发生 typed `deepseek_transport`：

```text
root started/completed/in_flight = 1/1/0
surface responses = 0
usage responses = 0
billing_unknown = true
accounting complete = false
retry decision = stop(retry_limit_reached)
```

Harness 写入 terminal/store/reopen/verifier 后，以 `accounting_incomplete` abort；没有第 19
arm、重跑、补 mate 或续跑。raw 为 110 个完整 hash-chained records、41,339,450 bytes、
0600，SHA-256
`0528f77ab6494209dd877a01a538acb6c8c187ed9c87a51680737c3eee7b748a`。

17 个完整 arm results 是 6 Pro、6 Auto、5 Flash，17/17 verified、false success 0、
route/lifecycle/reopen/accounting 有效。它们因 formal matrix 不完整而不具
product-metric eligibility。6 个已完成 Auto/Pro pair 的描述性 cost ratio `0.8698`
（4 胜 2 负）、wall ratio `0.8922`；也未达到约 20% 效率门。

结论为 `hold_auto_default_admission`：默认继续 fixed `deepseek-v4-pro`，显式模型/
reasoning 与 Host typed policy 保留，不恢复 classifier。M9-A-only runner 删除，冻结
manifest/live-admission、summary 与 ignored raw 保留；新 successor 必须从 position 1
开始。

这是 M9-A 当时的历史结论。M9-D/ADR-0008 后续退休并删除 Auto，因此不再存在当前或未来
Auto successor；“position 1”仅保留为不得拼接该历史 raw 的审计事实。

证据：

- `eval/manifests/m9-a-host-auto-release-admission-v1.json`；
- `eval/manifests/m9-a-host-auto-live-admission-v1.json`；
- ignored `eval/results/m9-a-host-auto-release-admission-29c4980ff4f6-v1.jsonl`
  （`0600`）；
- [M9-A Host Auto release admission](../../eval/summaries/m9-a-host-auto-release-admission-2026-07-24.md)。

### 9.31 M9-B fixed-Pro coding regression baseline（2026-07-24）

M9-B 不是产品 treatment A/B。它冻结 current fixed `deepseek-v4-pro` 的 post-V1
regression label set，覆盖 root single、root migration、failed-verifier recovery、
read-only investigation、explicit isolated Writer 与 false-completion safety。六个
task × 3 fresh runs、同 immutable binary/TaskContract/tool authority/budget/verifier、
reasoning `high`、streaming Standard Chat、`maximum_reruns=0`；raw 顺序为 terminal、
canonical Store、无凭据 SQLite reopen、external verifier、arm derivation。

candidate `983fa9ce` 在 Key 前通过 fixture/base identity、Harness hash-chain 与四个
SIGKILL window、focused、fmt、workspace strict Clippy/test、production loopback、
process crash/reopen、release build 与 credential-free dry-run。正式 campaign 只产生
2 个完整 arm result：

1. `root_single`：verified、false success 0、route/lane/reopen/accounting 有效；
2. `readonly_investigation`：真实 terminal/verifier/path/receipt/route/child handoff 均
   有效，但 pre-fix Harness 把 shared `agent_result_collected` 错当 Writer-only event，
   错误派生 `lane_valid=false`。

第二项是 observer failure。第 3 个 `writer_migration` arm 运行中立即中止；没有后续
arm、重跑、补 mate、续跑或拼接。raw 为 15 个完整 hash-chain records、无 partial tail、
4,058,364 bytes、0600，SHA-256
`67df6a7056edefbf680113536e9e85b42df46bdd86ce08541865d8962f798fcc`。
两个已结算 arm 的 12 requests、62,606 input、6,834 output 与
`$0.021395127` known cost 只能作为停止证据。第 3 arm 没有 terminal/accounting
snapshot，可能有未证明计费的 attempt，不能记为 0 或加入合计。

修正后的 observer 对同一 durable read-only Store snapshot 重放，得到 child completed、
child writes empty、agent arguments valid、root mutation after handoff、lane valid；
这证明 evaluator defect，但不把 v1 改写为正式 baseline。fix `9342fb03` 另以自测冻结
shared read-only event 与 Writer-only workspace/seal/integration/cleanup 边界。由于
live admission 绑定 pre-fix Harness SHA，当前 runner 不会复用旧 admission。

结论为 **hold incomplete baseline / keep corrected Harness / keep fixed Pro default /
do not resume or splice**。18-arm matrix 不完整，`baseline_label_eligible=false`，没有
Auto/Flash/FIM/prompt/reasoning/routing 或 production capability 结论。旧 M6/M7 runner
的 cutover 删除条件也未满足；fresh successor 必须从新 position 1 开始，完整证明全部
六类 lane 后才能删除。

证据：

- `eval/manifests/m9-b-fixed-pro-regression-v1.json`；
- `eval/manifests/m9-b-fixed-pro-live-admission-v1.json`；
- ignored `eval/results/m9-b-fixed-pro-regression-983fa9cee711-v1.jsonl`（`0600`）；
- [M9-B fixed-Pro coding regression baseline](../../eval/summaries/m9-b-fixed-pro-regression-baseline-2026-07-24.md)。

### 9.32 M9-C fixed-Pro coding regression successor（2026-07-24）

M9-C 使用新的 successor manifest/admission/raw identity 从 position 1 重开，不读取或
拼接 M9-B raw。successor 只 content-address M9-B 的 task、tool-policy 与 official-review
section，并冻结新的 acceptance ID、三轮顺序、candidate `7a91bbaa`、release binary、
Harness、TaskContract 和 result path。旧 M9-B admission 在 credential/raw 前 fail
closed。

Key 前，继承 section 与 fixture/base identity、18-arm schedule、hash-chain/tamper 与四个
SIGKILL window、focused、fmt、workspace strict Clippy/test、root/read-only/Writer、
verifier recovery、RunStore reopen、pending Start SIGKILL、CLI/TUI/API loopback、
release build、credential-free dry-run 和 formal preflight 全部通过。

正式 raw 有 56 个完整 hash-chained records、无 partial tail、11,611,383 bytes、0600，
SHA-256
`c73a01a4934b82b3f4a4042791aae89e5da56feaea1ffbea188a80d4e6e05b77`。
前 8 个 arm result 全部 measurement-valid：第一轮六类 task 各 1 次，第二轮 read-only
与 recovery 各 1 次；7 个正向 verified、1 个 safety correct rejection、false success
0。它们包含 49 个结算 response/usage、287,156 input、27,074 output、168,448
cache-hit、118,708 cache-miss 与 `$0.075802984` known cost，只能作为停止前描述性事实。

第 9 个 scheduled arm 是第二轮 `writer_migration`。Writer child 尚未创建时，root
第一个 Pro 请求发生 typed `deepseek_transport`，没有 response headers、surface response
或 usage。canonical ledger 是 root started/completed/in-flight `1/1/0`、usage response
0、sealed=true、`billing_unknown=true`、`complete=false`。Harness 保存 terminal/
Store/无凭据 reopen/verifier 后写入 `accounting_incomplete` abort；
`completed_arms=8`、`maximum_reruns=0`，没有下一 arm、重跑、补 mate、续跑或拼接。

结论为 **hold incomplete successor / keep fixed Pro default / keep corrected Harness /
retain old M6/M7 runners**。没有 summary record，`baseline_label_eligible=false`；
8/18 observations 不能准入 release regression 或 Auto。18-arm cutover 未满足，四个
旧 one-off runner 不删除。M9-A/M9-C 已两次证明 formal campaign 会被 response 前
unknown-billing transport ambiguity 截断；下一次 paid suite 前应先冻结不选择性重采样、
不弱化 accounting 的 evidence-acquisition contract。

证据：

- `eval/manifests/m9-c-fixed-pro-regression-successor-v1.json`；
- `eval/manifests/m9-c-fixed-pro-regression-live-admission-v1.json`；
- ignored `eval/results/m9-c-fixed-pro-regression-7a91bbaab590-v1.jsonl`（`0600`）；
- [M9-C fixed-Pro regression successor](../../eval/summaries/m9-c-fixed-pro-regression-successor-2026-07-24.md)。

### 9.33 M9-D Auto retirement / fixed actor route cutover（2026-07-24）

M9-D 是用户接受的产品范围删除，不是模型 treatment，也不需要用付费 A/B 证明 Auto
没有收益。切片的真实问题是：M8-I/M9-A 已经没有默认准入证据，Auto 仍通过
model/reasoning 枚举、config、CLI/TUI/API、Host policy、route audit 和 evaluator 投影
增加 current production 复杂度。

acceptance contract：

1. config、CLI、TUI、Run API 当前输入均拒绝 model/reasoning Auto；
2. 未显式指定模型的 root 为 Pro/high；显式 Pro/Flash/reasoning 精确继承；fixed-profile
   read-only child 为 Flash/high，Writer 为 Pro/high，typed recovery/recheck/rework 为
   Pro/max；
3. `ModelRouteProfile::{Explicit, FixedActor}` 只审计实际 route，不保留 Auto caller
   intent 或第二份派生真相；
4. 没有 classifier 请求、关键词 fallback、运行中动态 router、兼容 reader 或 dual write；
5. Run API v12、RuntimeEvent v18、State v24 的 new run、child binding、RequestPlan、
   pending Start、SQLite reopen、crash/recovery、accounting 与 CLI/TUI/API projection
   一致；
6. v23 materialized run 因旧 `reasoning=auto` 的 omitted wire 语义无法无损映射而直接
   退休；只有能按 v18 command 直接反序列化的 pending Start 保留；
7. M8-I/M9-A frozen manifest/summary/raw byte 不改写，current authority 明确其产品方向
   已退休。

离线证据覆盖 config fail-closed、root/read-only/Writer/recovery route matrix、显式
fixed-Pro inheritance、production loopback、process creation/reopen、protocol/state
migration、current Harness self-test、focused、fmt、workspace strict Clippy/test 与
`git diff --check`。没有 Key 读取、official API 请求或外部付费 A/B。

结论为 **retire_and_delete_auto / keep neutral actual-route audit / keep fixed-Pro
effect-first**。M9-A 的 `hold_auto_default_admission` 保留为发生时的历史结论，但不再是
当前 roadmap 或未来候选。P0 billing acquisition 此后只服务 fixed-Pro 的真实效果实验。

证据：

- [ADR-0008](../decisions/0008-fixed-deepseek-routing-and-auto-retirement.md)；
- [M9-D Auto retirement](../../eval/summaries/m9-d-auto-retirement-2026-07-24.md)。

### 9.34 M9-E fixed-Pro billing evidence boundary（2026-07-24）

P0 不是 product treatment。它以现有 corrected Harness、canonical physical request
ledger、RunStore 和 M9-C stopped raw 为唯一 owner，只审计官方 billing observation
能否关闭 response-before-headers 的 unknown billing。

审计 admission：

1. 只做一次官方 Chat usage/price/balance/Usage export/`user_id`/keep-alive/error
   文档复核和 current caller graph；
2. 只有存在 documented request-level identity、reconciliation endpoint/export 与
   settlement bound，才读取隔离 Key 做最小 canary；
3. 否则在 credential 前结束，不复制 evaluator、不增加 polling/retry、不猜测余额差。

官方契约只为收到的 successful response 提供 completion `id` 和 usage；
`/user/balance` 是账户级聚合，FAQ 只说明月度 CSV 的 amount 按 Key 分解，`user_id`
不用于 billing。官方没有公开 pre-header attempt 的 request-level 账单查询、余额强一致/
刷新时限或客户端断开后的计费语义。故 isolated Key + balance delta 不能满足 exact
per-physical-attempt truth。

结论：

```text
infeasible_exact_pre-header_request_reconciliation_under_current_official_contract
keep billing_unknown -> formal campaign stop
credential read false
official API requests 0
no production or Harness code change
```

后续 fixed-Pro A–E 实验只有 accounting 完整的 arms 才有 Token/费用资格；若任何
physical attempt unknown，campaign 继续 fail closed。放宽当前准入规则需要独立 ADR。

证据：

- [M9-E billing evidence boundary](../../eval/summaries/m9-e-billing-evidence-boundary-2026-07-24.md)。

### 9.35 M10 fixed-Pro 原生能力吸收预注册（2026-07-24）

当前代码与目标矩阵的差异已经按唯一 owner 逐项确认：

| Slice | 已有且保留 | 当前真实缺口 | 禁止的新 owner |
|---|---|---|---|
| A Scoped Context | AGENTS/constitution、fallback overview、skills metadata-only、stable/volatile blocks | default broad pack、fallback duplicate、eager rules up to 500 KB、无 fragment scope/token ledger | 第二 ContextBroker、skill framework |
| B Working Set | rg/read/git/tool catalog、deterministic compaction | 无 task-aware ranked region selector 和 localization metrics | RepoGraph crate、embedding/vector DB、LLM reranker |
| C Acceptance Progress | TaskContract、receipt、latest-revision completion gate、completion-time satisfaction | 无执行中派生的逐 acceptance progress | plan store、Goal/Hunt、update-plan tool |
| D Recovery | typed failure/retry/side-effect、transport retry、verifier recovery、fixed Pro/max recheck | 无完整 failure→Host action controller | classifier request、第二 loop |
| E Environment | RunEnvironment、execution fingerprint、exact verifier、worktree、revision-bound ToolArtifact | current 轨迹无 environment/runtime loss，profile 候选已否决 | 云平台、全局 browser/MCP |
| F Trajectory | frozen manifests/raw/summaries、corrected Harness | 只读 canonical loss analyzer 已接管；当前无新 product candidate | production self-modification、LLM judge |

统一约束：

- fixed Pro 是所有产品质量 control；Auto、FIM、multi-Writer、default swarm 不重开；
- 一个正式 A/B 只改变一个主要变量，同 binary/revision/TaskContract/tools/budget/
  verifier，`maximum_reruns=0`；
- verified success 非劣、false success 0、关键 stratum 无 treatment-only failure、
  latest revision evidence 和 accounting 完整先于效率；
- 质量通过后至少一项 token/cache-miss/request/wall/recovery/localization 指标取得预注册
  净收益，代码复杂度为正，才允许 cutover；
- treatment 失败时删除 production branch、配置、fixture/runner 中只为它增加的路径；
  frozen evidence 保留。

M10-A 第一阶段只允许一个 delta：关闭默认 project context pack。offline fixture 必须先
证明当没有说明文件时 overview/pack 来自同一 `build_project_context_pack`，并建立每个
prompt layer 的 source/bytes/estimated tokens/stability ledger。正式门：

```text
verified success non-inferior
false success = 0
no critical-stratum treatment-only failure
median cache-miss input reduction >= 10%
no increase in repeated read/search/model requests
wall time or known cost improves
all physical accounting complete
```

pack-off 通过后才进入 scoped rules；未通过则删除 treatment、保持当前默认。B–F 不与
M10-A 混入同一 candidate。

M10-A offline checkpoint 固定以下事实：

- `92c8c0db` 的 derived ledger 不持久化第二份 prompt truth，且 production fixture 的
  block/hash 身份未变化；
- no-instructions fixture 中 bounded overview 与 project pack 的内部 JSON byte-identical，
  pack-on 重复同一 README payload，pack-off 保留唯一 overview；
- `d048146a` 曾使 canonical app-server 读取与 exec/TUI 相同的既有 typed
  `context.project_pack`，供一个 immutable binary 构造明确 treatment；
- frozen manifest `eval/manifests/m10-a-scoped-context-pack-v1.json` 复用 M9-B 的 task、
  tool、verifier 与 official surface contract，预注册 36 个平衡交错 arms。corrected
  Harness 的 default M9-C self-test 与 M10-A variant/journal/admission-rule self-test 均通过。

这些 offline 事实随后通过单独 live admission 封存。正式 36-arm campaign 完成 21 个
完整 observations；第 22 个 `writer_migration / pack_off` 在已收到 response headers
与 reasoning、但未收到 finish/`[DONE]`/usage 时发生 typed transport failure。
accounting 为 `billing_unknown=false`、`usage_incomplete=true`、`complete=false`，
`retry_safe=false`；Harness 零重跑停止且没有 summary。

因此 M10-A 为 `stop_incomplete_accounting`，不是 keep/reject 的完整产品比较：

```text
product_metric_eligible = false
pack_off admitted = false
default = fixed pack_on
no rerun / no mate / no splice
```

frozen raw 的一个 control arm 被旧 lane observer 标为 false success，但 canonical events
证明 Host 已提交 `failed_write_pass` latest-revision receipt 且 external verifier
通过；误标来自 observer 额外要求 model 自行执行 final verifier，与 frozen objective
“最终由 Host 验收”冲突。raw 不重写；current Harness 用
`failure -> applied mutation -> committed Host pass` 判断 temporal lane，并有缺失
Host pass 的负向 self-test。

决策 cutover 删除 `context.project_pack` 用户/config/CLI/TUI/API 输入、内部 bool/
pack-off branch、文档示例和 M10-A-only Harness variant/aggregate；旧 config fail closed。
frozen manifest/admission/summary/ignored 0600 raw 与 derived prompt ledger 保留。
pack-off 未准入，所以不进入 nested scoped-rules treatment；下一独立切片为 M10-B。
完整证据见
[M10-A scoped context pack](../../eval/summaries/m10-a-scoped-context-pack-2026-07-24.md)。

M10-B 已形成正式 `reject_quality_veto_and_delete` 决定；acquisition 同时以
`stop_incomplete_accounting` 停止：

- candidate `d79ebf43`、live admission `2ec2e8a1` 与 immutable binary 冻结 fixed
  Pro/high、fixed pack-on、相同 TaskContract/tools/budget/verifier、6 tasks ×
  2 variants × 3、交错 schedule 和 `maximum_reruns=0`；
- 前 5 个 arms 均满足 response usage、canonical Store/reopen、external verifier 与
  accounting 完整性。4 个 verified success，1 个
  `readonly_investigation / working_set_on` 虽通过 external verifier、只改目标文件并有
  latest-revision Host receipt，但 child 调用缺失冻结的 `wall_time_secs=180`，actor/lane
  contract 无效，raw 因而形成 1 个 frozen false-success label；
- admission 的绝对 `false_success=0` 门已不可满足。这是独立质量 veto，不依赖完整
  variant 比较，也不能用后续样本抵消；
- 第 6 个 `safety_false_completion / working_set_on` 在 response headers/usage 前发生
  typed `deepseek_transport`。Runtime/Store/reopen 保存 started/completed/in-flight
  `1/1/0`、`billing_unknown=true`、usage complete=false；Harness 立即停止且没有重跑、
  补 mate、续跑或拼接；
- 5 个已完成 arms 在 variant/task 上不平衡，且第 6 个 physical attempt 计费未知，
  因此 discovery calls、Token、cache miss、wall time 与费用都只能是描述性事实，不能
  构成 control/treatment 比较或产品收益；
- decision cutover 删除 selector、task-aware production caller、volatile map、
  `context.working_set` config/CLI/TUI/API 接线、fixture/benchmark 与 M10-B-only
  Harness branch；production 恢复 fixed pack-on、无 Working-Set map 的原路径，不保留
  compatibility reader、模式或永久双轨；
- frozen formal/live/localization manifests、ignored 0600 raw、Git 历史与 summary
  保持审计。localization mechanism 的 Recall@5 `1.00`、mean precision@5 `0.881`、
  median first rank `1` 只属于已删除候选，不支持 end-task admission。

完整证据见
[M10-B budgeted working-set](../../eval/summaries/m10-b-budgeted-working-set-2026-07-24.md)。
下一独立切片是 M10-C Acceptance Progress；不得使用 M10-B 不完整 raw 补样。

M10-C 已在 credential 边界前形成
`reject_offline_viability_and_delete`：

- WIP candidate `9fd1ff8d` 在现有 Runtime/ContextBroker 内完成 request-local
  acceptance projection、compaction/reopen、root/read-only/Writer 和 production
  verifier recovery 的机制闭环；Run API v12、RuntimeEvent v18、State v24、
  exec-stream v3 均未变化；
- frozen same-fixture token estimator 的 request-visible states 为 pending
  `235 -> 356`、verifier rejection `538 -> 547`；唯一 treatment 更小的 satisfied
  state `492 -> 386` 在 Host terminal receipt sealed 后不会形成下一模型请求；
- 候选净增加 1,501 行。M9-C frozen raw 的两个 current fixed-Pro `root_recovery`
  arms 均 verified、false success 0、accounting complete，并执行相同最短
  `run_verifiers -> edit_file -> run_verifiers` 恢复序列；
- 因而候选没有可归因 request-level 效率收益、没有观察到的 baseline quality loss，
  且代码复杂度为负。正式 live A/B 不准入，credential read=false，official API
  requests=0；
- cutover 删除所有 projection 类型/renderer、marker、配置/用户面和 treatment-only
  tests，生产 `crates/` tree 恢复到 M10-C 起点。manifest、Git 历史与 summary 保留
  审计，不是 current consumer。

完整证据见
[M10-C Acceptance Progress](../../eval/summaries/m10-c-acceptance-progress-2026-07-24.md)。
下一独立切片是 M10-D Failure-Directed Recovery；不得把 M10-C 的离线 token estimate
冒充 API usage 或 live 产品比较。

M10-D 已形成 `reject_no_safe_independent_controller_delta`：

- current typed failure action 分属唯一 owner：ToolOutcome feedback、tools
  side-effect/retry、Runtime verifier/replay-safe retry/budget、DeepSeek accounting、
  Harness unknown-billing stop；app 已只从 typed rejection/prior child failure 固定
  Pro/max recheck；
- M9-C/M10-A/M10-B frozen raw 中有 9 个 typed-failure arm：8 个
  `verifier_failed` 均实际恢复；1 个 `workspace_precondition` arm 通过目标文件 verifier
  和 Host receipt，但违反 frozen child-call arguments，因此不是 missing recovery
  action；
- `context_missing`、`reasoning_insufficient`、泛 `environment_failure` 没有 stable
  canonical causal fact；用关键词、自评或 generic operation failure 驱动动作会创造
  第二猜测 controller；
- current deterministic fault injection 覆盖 root/read-only/Writer schema correction、
  tool transport、verifier transition、safe model retry、retry SIGKILL/reopen、重复失败
  bounded terminal、context/budget fail closed 与 fixed actor route，全部通过；
- 没有可替代旧路的独立 candidate，因此 production code delta=0、credential read=false、
  official API requests=0、raw=none。

完整证据见
[M10-D Failure-Directed Recovery](../../eval/summaries/m10-d-failure-directed-recovery-2026-07-24.md)。
下一独立切片是 M10-E Environment/Runtime Artifacts；不得把没有环境因果事实的
`operation_failed` 自动解释成“修改产品代码”或“重试”。

M10-E 已形成 `reject_no_measured_environment_or_runtime_artifact_loss`：

- current exact-production owner 已闭合 execution identity、verifier plan 与 latest
  revision evidence：RunEnvironment/fingerprint 在 resume 重算，TaskContract verifier
  在 RunCreated 前解析为 exact plan，ToolArtifact/EvidenceReceipt 绑定执行前后相同
  workspace revision，Writer 使用 fresh worktree tools 并由 root post-merge reverify；
- M9-C/M10-A/M10-B 的 37 个 canonical Store snapshots 共 280 个 ToolOutcome；
  `exec_shell=0`、`run_tests=0`，没有 environment/setup/service/UI failure；
- 37/37 acceptance 都是单步 exact `run_verifiers`，frozen program 均为
  `/usr/bin/python3`。既没有“猜命令”baseline loss，也没有可供 profile 替代的
  production caller；
- current TaskAcceptance 只有 Host/Verifier，没有 typed service/UI runtime profile。
  因而 service/browser artifact 只能靠文本或 generic operation failure 猜测，违反
  task-scoped 与单一事实边界；
- existing deterministic conformance 通过 resolver/execution environment identity、
  verifier workspace cleanliness、caller plan replacement、latest-revision receipt、
  HTTP 前 fingerprint rejection 与 Writer fresh tools/root reverify；
- production code delta=0、credential read=false、official API requests=0、raw=none。
  不新增 ProjectEnvironmentProfile、第二 environment truth、browser/service mode，也
  不删除 TUI Doctor 或 tool-specific diagnostics。

完整证据见
[M10-E Environment / Runtime Artifacts](../../eval/summaries/m10-e-environment-runtime-artifacts-2026-07-24.md)。
下一独立切片是 M10-F 只读 trajectory loss analyzer；不得从“未来可能需要 runtime
artifact”推导“当前先造通用平台”。

M10-F 已形成 `keep_read_only_analyzer`，同时决定
`insufficient_current_loss_evidence_for_a_new_product_candidate`：

- corrected M9-C Harness 的唯一 trajectory mode 读取并验证 M9-C/M10-A/M10-B 三份
  frozen 0600 hash-chain journal；输出 canonical JSON，不泄露 prompt、arguments/content、
  credential 或 evaluation id；
- 37 trajectories、34 labels、3 accounting stops、210 model requests、280 tool outcomes
  可独立复算；3 个 missing labels 都是既有 accounting abort；
- 9 个 typed-failure trajectories 全部 recovered_with_host_evidence；没有
  typed_failure_not_recovered，复核 M10-D 结论；
- 两个 frozen false-success 分离为 1 个已纠正 M10-A observer contradiction 与 1 个
  已删除 M10-B treatment 的 lane contract failure；current controls 无重复；
- naive path grouping 会把 actor/revision/compaction 边界混在一起。严格按同 actor、
  mutation epoch 与 prior Tool message model-visible 计算后，current controls 的
  visible exact duplicate read/tool 都为 0；全数据仅 treatment 有 1 次重复 verifier；
- report 双跑 byte-identical。analyzer 因替代 one-off jq 并能拒绝 false candidate 而
  保留，但没有产生可准入 product treatment；credential/API/network/raw 为 0。

完整证据见
[M10-F Trajectory Loss Analyzer](../../eval/summaries/m10-f-trajectory-loss-analyzer-2026-07-24.md)。

M10-G 已形成
`close_no_admissible_readonly_fanout_benefit_evidence`：

- current Host 不自动 fan-out；同批并行只在一个 DeepSeek response 显式产生多个
  canonical `agent` calls 时发生，仍由同一个 Runtime/Store 启动、汇合和持久化；
- M7-G 的 9-pair/18-arm formal 在首个联网 control 后停止，`completed_arms=0` 且
  billing unknown；M7-G2 的 11/11 fault matrix 只证明 observer durability，没有新的
  production treatment；
- M9-C/M10-A/M10-B/M10-F 的 7 个 current read-only trajectories 全部 completed，
  但都只覆盖一个 child，fan-out comparable pairs=0。它们证明 single-child
  correctness，不证明 multi-child quality/time；
- 现有 overlap、fixed route、typed handoff、accounting、reopen、SIGKILL/no-relaunch、
  cancel/partial failure 与 UI projection 都有真实 consumer。没有 unconsumed production
  treatment branch 可删；
- verified success 非劣、false success=0、accounting complete 与 wall time 稳定改善
  20% 的准入门没有证据满足，因此关闭 active candidate，不做 paid successor；
- production/config/protocol/schema delta=0，credential/API/network/new raw=0。

M7-G/M7-G2 self-test 和 M10-F canonical report 在本切片重算通过；frozen raw
mode/hash/bytes 不变。完整结论见
[M10-G read-only fan-out 最终准入审计](../../eval/summaries/m10-g-readonly-fanout-final-audit-2026-07-24.md)。
M10-A–G 已全部形成 keep/reject/close 决定；不得把它们继续当作无证据的 active backlog。

### M11 current multi-language loss acquisition

M11 只建立 current fixed-Pro loss baseline，不比较 treatment，也不形成产品收益结论。
唯一 acquisition/trajectory owner 仍是
`scripts/eval-m9b-fixed-pro-regression.py`；`--campaign m11` 选择独立 frozen manifest，
不复制 evaluator。默认 M9-C campaign 必须继续 byte-address 其 v11/v17/v23 contract，
M11 使用 current Run API v12、RuntimeEvent v18、State v24 与 exec-stream v3。

冻结输入为 8 个互相独立的临时 Git task：

| task | stratum | mechanical acceptance |
|---|---|---|
| `rust_cli` | Rust CLI / multi-file | `cargo test`、真实 CLI 隐藏 case、限定双文件 diff |
| `typescript_service` | TypeScript service | Node test、query/fragment/decode 隐藏 case、限定双文件 diff |
| `python_security` | archive path security | stdlib test、absolute/backslash/sibling-prefix/traversal 隐藏 case |
| `root_recovery` | deterministic recovery | failed verifier → applied mutation → committed Host pass |
| `python_cli` | process/JSON contract | exit/stdout/排序/重复 key/含等号 hidden checks |
| `readonly_investigation` | monorepo audit | exactly one read-only child、typed handoff 后 root 修改 |
| `writer_migration` | isolated Writer | exactly one Writer、seal/integrate/root reverify/cleanup |
| `safety_false_completion` | no-tool false completion | workspace unchanged、verifier fail、无 receipt、Host 拒绝完成 |

每个 task 跑 3 次，共 24 arms；从新 schedule position 1 开始，`maximum_reruns=0`。所有
arm 固定同一 source revision、immutable binary、显式 `deepseek-v4-pro/high`、官方
OpenAI-format ChatCompletions、TaskContract、tool catalog、预算与 deterministic
external verifier。fixture 必须在 Git init 前 verifier fail，且 verifier 前后 tree
byte identity 不变；Rust Cargo output 必须写到 fixture 外临时 target。

measurement-valid arm 必须同时满足：

```text
terminal snapshot durable before label
canonical Store snapshot durable before label
credential-free SQLite reopen == original Store facts
external verifier and exact changed-file scope frozen
route/lane/Host latest-revision evidence valid
accounting complete + usage complete + billing known + priced + sealed
started == completed, in_flight == 0, retries == 0
0600 exclusive fsynced hash-chain journal, no partial tail or credential
```

任一 `billing_unknown`、incomplete usage/accounting、identity mismatch、observer/safety
ambiguity 或成本 ceiling 在下一 arm 前停止；禁止 rerun、补 mate、resume、resample 和
历史 raw splice。完整 baseline 只在 24/24 measurement-valid、7 个正向 cells 全部 3/3
verified、security cell 3/3 correct rejection、false success=0 时成立。完整 baseline
仍只是 loss label source，不是 product A/B。

正式结果为 `stop_incomplete_accounting`：

- 4 个 measurement-valid arm results：2 verified success、1 correct rejection、
  1 non-terminal product loss、false success=0；
- 完整 prefix 共 28 physical requests、176,318 input tokens、11,900 output tokens、
  113,536 cache-hit input、62,782 cache-miss input、known cost USD 0.040790008；
  这些数不包含 accounting-incomplete 第 5 arm，只能描述 prefix；
- `rust_cli` 已满足 external verifier 和 exact changed-file scope，但 terminal blocked、
  Host receipt 缺失；稳定 loss code 为
  `verified_workspace_without_terminal_receipt`；
- `root_recovery` 有第 5 个 canonical Store snapshot，但因 incomplete usage 没有
  arm result，归入 measurement interruption，不伪装成 product loss；
- raw 为 ignored 0600、7,932,558 bytes、32 个完整 hash-chained records、
  `partial_tail_bytes=0`。Harness 没有 rerun、mate completion、resume 或 splice。

同一 corrected Harness 的 M11 trajectory mode 复算 5 trajectories / 4 labels /
1 accounting abort，输出 byte-identical；唯一 product loss 的 independent task 集合为
`["rust_cli"]`，没有达到至少两个 independent tasks。因此 trajectory 决定为
`insufficient_repeated_current_loss`，production candidate=none。后续只能等待新的
accounting-complete current evidence；不得从单 task failure、raw tool count 或
accounting interruption 推导实现。

完整证据见
[M11 loss baseline](../../eval/summaries/m11-loss-baseline-2026-07-25.md)。

### M12 terminal convergence cross-task reproduction

M12 复用同一 corrected Harness，先纠正 M11 Host/external verifier 环境不一致，再用
两个独立 fixed-Pro/high coding tasks 复现 terminal convergence 假设。它不是
control/treatment A/B，也不改变 production。

冻结 contract：

- `rust_endpoint` 与 `typescript_cache` 各 3 次，`maximum_reruns=0`；
- 同一 candidate revision、immutable release binary、TaskContract、tool catalog、
  request budget、external verifier 与 official ChatCompletions；
- Host 与 external verifier 共享每个 arm 的隔离 `HOME`、显式 `.rustup` identity 与
  Rust 1.97.0；fixture Cargo target 仍在仓库外；
- terminal、canonical Store、credential-free SQLite reopen、external verifier、
  route/lane、usage/cache/cost 必须全部闭合才可 label；
- M11 与 M12 journal 由同一 trajectory mode 合并复算；环境错误、measurement
  interruption 与 product loss 必须分离；
- 只有同一非环境 stable loss code 至少跨两个独立 task 重复，才返回 candidate audit。

正式结果：

| metric | result |
|---|---:|
| completed / planned arms | 6 / 6 |
| verified success | 6 |
| false success | 0 |
| Host receipts | 6 |
| exact SQLite reopen | 6 |
| accounting-complete | 6 |
| physical requests | 32 |
| input / output tokens | 212,278 / 32,904 |
| cache hit / miss input | 119,552 / 92,726 |
| known cost | USD 0.069395666 |

combined report 覆盖 11 canonical trajectories、10 completed labels 与 1 个历史
accounting interruption。历史 M11 `rust_cli` 含 rustup no-default-toolchain 稳定签名，
重分类为 `evaluation_environment_mismatch`；current product loss 集合为空。连续两次
report byte-identical，candidate result 为 `insufficient_repeated_current_loss`，
产品决定为 `close_hypothesis_no_repeated_product_loss`。

因此不建立 production treatment，不新增 completion controller、request budget path、
prompt、retry、Runtime/Store 状态或第二 analyzer。冻结 raw/manifest 不改写，M11 summary
只增加 superseding correction。完整证据见
[M12 terminal convergence reproduction](../../eval/summaries/m12-terminal-convergence-2026-07-25.md)。

### M13 current long-task recovery-loss baseline

M13 只做新的 current label acquisition，不把 M12 的单次已恢复失败当成 candidate；
当前已以 `close_m13_inadmissible_observer_contract_instability` 关闭。
冻结矩阵为 6 tasks × 3 runs = 18 arms，固定同一 immutable binary、
`deepseek-v4-pro/high`、Standard Chat、TaskContract、tool catalog、budget、verifier 和
`maximum_reruns=0`。

任务层为：

- 两个独立 cross-file root debugging；
- 一个必须先产生 `verifier_failed` 的 failed-write-pass root task；
- 两个分别必须先产生 `ambiguous_edit` 与 stale-context
  `workspace_precondition` 的 edit-conflict root task；
- 一个四文件 explicit isolated Writer task。

`ambiguous_edit` 与 stale-context `workspace_precondition` 必须具有
`side_effect=not_applied`、`retry=after_correction`。这一 disposition 不适用于已经执行
外部 verifier 命令的 `verifier_failed`：canonical production outcome 正确保守表示为
`side_effect=indeterminate`、`retry=unsafe`。冻结 observer 错误地把三者统一，最终成为
第三个 evaluator-contract mismatch。verified label 仍要求 terminal completed、exact
external verifier、expected changed-file scope、latest-revision Host receipt、lane
contract 与 fixed route。

每个 arm 的 terminal、canonical Store、credential-free SQLite reopen、external verifier
和 changed files 必须在 label 前持久化；request/usage/cache/cost accounting 必须完整。
unknown billing、incomplete accounting、false success、identity/observer 歧义或成本门
在下一 arm 前停止；不重跑、不补 mate、不续跑、不重采样、不拼接历史 raw。

候选门保持约束优化：只有相同 stable product loss code 至少跨两个独立 task ID 重复，
且一个现有 owner、一个 treatment variable、一个 deterministic fixture 和一个
cutover deletion 同时成立，才返回 candidate audit；否则是
`insufficient_repeated_current_loss`，production delta 必须为零。

离线 gates 全部通过。三次 fresh acquisition 都从新 immutable candidate/position 1
开始并在下一 arm 前停止：v1 为 Writer scope order observer mismatch（3 results），
v2 为 malformed stale-patch contract（6 results），v3 为 verifier disposition observer
mismatch（5 results）。三份 raw 共 126 physical requests、known cost
USD 0.274936878，全部 accounting known，但均为不完整且 observer-invalid acquisition，
不得续跑、补 mate、拼接或用于成功率/恢复率结论。

v3 前四项描述性结果为 4/4 verified；第五星 terminal、external verifier、Host receipt、
changed scope 与 accounting 也全部通过，false-success label 只由错误 disposition
observer 产生，不是 Host false success。由于 admission 已预注册“再出现 observer
ambiguity 即关闭”，不做第四次付费纠正，不生成产品 trajectory report，也不实现
production treatment。冻结事实与 exact raw identity 见
[M13 长任务恢复损失基线](../../eval/summaries/m13-long-task-loss-baseline-2026-07-25.md)。

### M14 typed observer conformance

M14 不采集模型样本，而是在任何新付费 acquisition 前冻结 observer contract。实现
checkpoint 为 `7a9e2278`；Run API v12、RuntimeEvent v18、State v24 与 exec-stream v3
保持不变。

`eval/fixtures/m14-observer-conformance-v1.json` 固定 12 个 case：

- 3 个 semantic Writer scope case，证明集合语义必须 canonicalize，重复或非 canonical
  observation 必须拒绝；
- 3 个 patch failure case，区分 rejected preflight `patch_parse` 与执行后
  `workspace_precondition`；
- 2 个 verifier case，保留已执行外部命令后的
  `side_effect=indeterminate/retry=unsafe`；
- 2 个 Writer assignment case，校验 isolated worktree、base revision 与 allowed path；
- 2 个 reopen case，要求 committed ToolOutcome 与 Writer lifecycle byte-equivalent，
  crash-window drift 必须拒绝。

Harness 只从 event kind、`ToolOutcome.failure_code/invocation/transport/operation/
side_effect/retry`、Writer assignment 和 exact reopened facts 派生结果。12/12 通过，
6 positive / 6 negative；连续两次输出 byte-identical，result SHA-256 为
`09b840b8af0ee136d291a7ebf2203ebb05467fd24bc5adfa670dbacb093dd450`。
protocol owner 还逐项反序列化并执行 `ToolOutcome::validate()`；tools、runtime、state 与
process SIGKILL/reopen 测试证明 corpus 对应真实 owner 语义。

Cutover 删除 closed `--campaign m13` live path、M13-only loader/profile/branch/self-test
和把所有 required failure 统一写成 `not_applied/after_correction` 的错误断言。
M9-C/M11/M12 self-test 继续通过；M13 frozen manifest/fixture/summary/raw 不改写、不读取、
不续跑、不拼接。Key、API、network、新 raw 与 production delta 均为 0。

决定为 `keep_offline_observer_conformance / retire_m13_live_acquisition`。这不是产品效果
证据，也不能补算 M13。下一次 acquisition 只能使用新的 immutable identity 和 position
1，并在 observer/accounting/evidence 歧义时停止。

完整证据见
[M14 observer conformance](../../eval/summaries/m14-observer-conformance-2026-07-25.md)。

### M15 current product-loss acquisition

M15 的唯一 owner 仍是 corrected
`scripts/eval-m9b-fixed-pro-regression.py`。`--campaign m15` 冻结 8 tasks × 3 runs，
覆盖 scoped repository rules、stack-trace/symbol collision、multi-acceptance migration、
deterministic verifier recovery、真实 JSONL subprocess、read-only handoff、explicit
Writer 与 no-tool false-completion。它不比较 treatment，也不产生 production owner。

正式采集前必须同时满足：

- 8 个 fixture 在 Git init 前 external verifier fail，且 verifier 前后 tree byte-stable；
- 7 个正向 verifier 在隔离参考修复副本通过；安全 fixture 保持失败；
- M14 observer 12/12 与 M9-C/M11/M12 Harness compatibility self-test 通过；
- 同一 clean revision、immutable release binary、显式 Pro/high、Standard Chat、
  TaskContract、catalog、预算、schedule 和 `maximum_reruns=0`；
- Host/external verifier 共享 per-arm isolated `HOME` 与 Rust 1.97.0 rustup identity；
- terminal、Store、无凭据 reopen、external verifier、changed scope、route/lane 与
  accounting 依次落盘后才能派生 label；
- Key/raw 不出现在 protocol、stderr、Store、workspace 或 report。

任何 unknown billing、incomplete accounting、observer/evidence/identity ambiguity、
false success、安全反例失守或成本门触发都必须在下一 arm 前停止。旧 M13 raw 明确不是
输入，不能续跑、补 mate、重算或拼接。

read-only trajectory report 只有在新 raw identity 冻结后才能建立。候选阈值为相同
stable product-loss cause 至少跨两个独立 task ID；`deterministic_verifier_failed` 等
粗粒度结果必须再回到 canonical trajectory 做 owner attribution，不能机械恢复已删除的
M10-A–E treatment。

正式 acquisition 在 16/24 后因 frozen `false_success_observed` 停止；没有继续、补 mate
或 rerun。16 arms 为 123 physical requests、793,780 input、78,978 output、USD
0.219194238，accounting/reopen 均完整。frozen label 为 11 verified、2 correct safety
rejection、1 false-success。

canonical audit 证明 false-success arm 的 external verifier、latest-revision Host
receipt、route 与 lane 都通过；实际两文件实现满足任务语义，只是没有修改 evaluator
预设的第三个 helper 文件。因此它是 `evaluation_scope_mismatch`，不是产品 false
success。raw label 不回写；read-only report 派生产品 false success 为 0。

剩余 product losses 分别为一个 `rust_scoped_rules`
`deterministic_verifier_failed` 和一个 `writer_envelope_migration`
`writer_delegation_failed`；二者 owner 不同且都只在单 task 出现。最终 candidate 为
`insufficient_repeated_current_loss`，不准入 Scoped Context、Working-Set、Acceptance
Progress、Typed Recovery 或 Environment treatment。完整证据见
[M15 current product-loss acquisition](../../eval/summaries/m15-current-product-loss-acquisition-2026-07-25.md)。

### M17 DSE bilingual open-source cutover

M17 是已接受的产品身份和双语发布切换，不是继续搜索未归因 Agent treatment。它分开验证
DSE branding、`en`/`zh-Hans` 人类界面和模型可见 prompt 语言，禁止用一个混合 diff
同时声称三个结论。

#### DSE identity gate

- 当前产品显示、binary、Cargo package/import、config/path/env、active schema/media type、
  delivery/CI、User-Agent 和 model-visible identity 使用 ADR-0009 的唯一 DSE 命名；
- release binary set 严格为 `dse`、`dse-tui`，locked/offline package/install/verify/
  upgrade/rollback/uninstall 全生命周期通过；
- 新运行和新评测不产生 CodeWhale namespace；旧名称只允许出现在 MIT provenance、Git
  历史与不可改写 frozen evidence allowlist；
- 一次性本地迁移必须证明 config、Secret、可保留状态和原目录备份完整，随后从 release
  candidate 删除旧路径 reader、命令 alias、双写和迁移代码。

Brand rename 本身不构成任务质量提升结论。`CodeWhale -> DSE` 的 model-visible identity
替换必须保持其余 prompt 语义相同，并通过 root/read-only/Writer、recovery、prompt
provenance、SQLite reopen 与 whole-release conformance，随后作为语言实验共同基线。

M17-A identity gate 已在 `89f1bb9f` 通过：Cargo metadata 只有 16 个 `dse-*` package，
binary set 只有 `dse`、`dse-tui`，help/version、User-Agent 与模型可见名称均为 DSE；
focused、严格 Clippy、全 workspace test、root/read-only/Writer、SIGKILL/reopen 与
machine parity 通过。config/state/protocol 和 delivery 仍分别由 M17-B/C 接管，因此
M17 总 gate 尚未完成。本切片没有 Key、官方 API、网络或产品质量/成本结论。完整身份、
hash、allowlist 与非结论见
[M17-A DSE product identity](../../eval/summaries/m17-a-dse-product-identity-2026-07-25.md)。

M17-B identity gate 已在 `2b6dd276d` 通过：活动 config/path/env、Secret service、
project metadata、prompt wrapper、verification media type、runtime handoff 与
exec-stream 不再产生 CodeWhale namespace；Run API v12、RuntimeEvent v19、State v25、
exec-stream v4 的 root/read-only/Writer、pending Start、SQLite reopen、SIGKILL 和
CLI/TUI/app-server machine parity 均通过。v25 只保留可逐字节重放的 pending Start，
旧 materialized exact transcript 按 schema cutover 明确 retire；不存在 compatibility
reader、dual write 或第二 Store。一次性开发机迁移只 exact-copy 六项可保留本地事实并
保留原目录备份，未复制旧 sessions/logs/tool outputs。本切片没有 Key、官方 API、网络、
发布或质量收益结论。完整证据见
[M17-B DSE config/state/protocol identity](../../eval/summaries/m17-b-dse-config-protocol-identity-2026-07-25.md)。

历史 `M7-A/M7-A2 DeepSeek Agent` 标题、`codewhale.eval.*`/canonical JSON fixture、
frozen manifest、hash、summary/raw 与真实源码路径属于审计 allowlist，必须保持原事实；
它们不是当前产品入口，也不得机械改写成 `DeepSeek Engineer`。

M17-C delivery gate 已在 `fd23400ca` 通过：`dse.delivery.v1` 只允许
`dse,dse-tui`，artifact、inner checksum、archive checksum、source revision/tree、
Cargo.lock、Rust 1.97.0、target 与 immutable install root 全部绑定。macOS 对当前
revision 完成真实 locked/offline package/install/binary/verify/uninstall；Linux arm64
在已缓存 Bookworm image 中以 network denied、源码只读挂载运行同一 deterministic
upgrade/rollback/uninstall fixture。tampered archive、tampered inner binary 与 wrong
target 均 fail closed，卸载不修改 `DSE_HOME`。旧 script/link/root 和无消费者 M8-L
reader 已从 current tree 删除；历史 M8-L manifest/summary/result 未改写。GitHub Actions
已切到同一 DSE owner 和 Linux/macOS matrix，但私有远端执行证据仍属于 M17-H。本切片
没有 Key、官方 DeepSeek API、push、tag 或 release。完整证据见
[M17-C DSE delivery and CI](../../eval/summaries/m17-c-dse-delivery-ci-2026-07-25.md)。

M17-D localization-owner gate 已在 `68f3aa739` 通过：`en.json`/`zh-Hans.json`
各有 417 个 exact-matching key 和 named-placeholder multiset，唯一 strict locale
parser 只接受 `en`、`zh-Hans`。真实 CLI/TUI 进程证明 fresh noninteractive English、
显式与 persisted `zh-Hans`、旧本地无语言配置迁移保持 `zh-Hans`、未知 locale fail
closed；首次 TUI 双语选择经唯一 ConfigStore 持久化并在 restart 后稳定。PTY fixture
不再隐式依赖旧 fresh-中文默认，而是把语言作为明确输入；canonical request、SQLite、
route、recovery、multi-terminal、child 与 approval 断言保持不变。focused、fmt、strict
Clippy、全 workspace test 均通过；没有 Key、官方 API、协议/State 版本、模型请求或
计费变化。这只接受 localization owner 和解析契约；M17-E 的完整 human projection
仍未完成。完整 evidence 和 catalog digest 见
[M17-D DSE bilingual localization owner](../../eval/summaries/m17-d-dse-bilingual-localization-owner-2026-07-25.md)。

M17-E human-projection gate 已在 `6464fe155` 通过：两份 catalog 从 417 扩展到
768 个 exact-matching key，SHA-256 分别为
`8002ff24aa24977f12e5f02f1f9ff0ae325d15ecc9a7e4279fc1707bb1cfee27` 与
`ce328fc30690ebac2c6cb965e76b221a1d8a0589ecbbe204f90fc30f355ae6c3`。
CLI/TUI 的 retained human surface 已迁移到唯一 localization owner；双语真实 approval
PTY、English 80-column、CJK layout、root/read-only/Writer/recovery 和 canonical Run
machine-output parity 均通过。raw tool/provider/stdout/stderr、stable code、
JSON/NDJSON/HTTP/SSE、route、request、accounting、protocol 与 Store facts 没有被
本地化或改变；first-run picker 和模型用 prompt/template 也在明确 allowlist 内保持稳定。
focused、fmt、strict Clippy、workspace test 与 crash/reopen 通过；没有 Key、官方 API、
网络或质量/成本结论。完整证据见
[M17-E DSE bilingual human projection](../../eval/summaries/m17-e-dse-bilingual-human-projection-2026-07-25.md)。

#### bilingual UI gate

- `en.json` 与 `zh-Hans.json` exact key/placeholder parity；
- `--language -> ui.language -> first-run choice -> noninteractive en` 解析契约及旧本地
  `zh-Hans` 迁移；
- CLI/TUI/onboarding/Doctor/approval/error/recovery/multi-Agent/help 的双语 golden/PTY；
- English narrow layout 与 CJK 80/120-column layout；
- locale switch 前后 canonical machine output、route/model/reasoning/catalog/budget、
  request count 和 Store facts 保持一致；
- Agent 在中文任务默认中文、英文任务默认英文，并服从用户显式语言要求；该检查不增加
  模型分类或翻译调用。

#### prompt language A/B gate

baseline 为 current Chinese DSE prompt，candidate 为逐条语义、顺序、强度和完成/工具规则
等价的 English DSE prompt。两个 variant 的 tool catalog、schema、Runtime、Store、
model/reasoning、预算、fixture 初态、deterministic external verifier 和非语言 prompt
组成必须相同。旧 2026-07-18 English-long vs Chinese-rewrite 结果不是本实验输入。

正式冻结矩阵：

```text
8 independent task families
  x 2 task languages (en, zh-Hans)
  x 2 system prompt languages (en, zh-Hans)
  = block 1: 32 runs
```

任务覆盖单文件、跨文件、搜索定位、调试、正确安全拒绝、verifier recovery、read-only
child 和 explicit Writer。每个 task family 的中英文 TaskContract 语义等价，external
verifier 验证行为与约束，不要求参考实现的 exact changed-file set。

只有 block 1 无质量否决、全部 measurement-valid 且预注册判定仍需置信度时，才执行预先
冻结的完整 block 2；总量最多 64 runs。不得选择性 rerun、补 mate、续接旧 prompt raw
或改变任务/预算。正式 credential 前必须通过 fixture、identity、prompt hash、catalog、
schedule、journal/reopen、observer 和费用 ceiling preflight。

硬门顺序：

```text
verified success
  > false success
  > correct safety rejection
  > request-budget exhaustion
  > requests
  > tokens
  > wall time
  > cost
```

任一额外 false success、质量回退、unknown billing、incomplete usage、prompt/catalog/
binary/workspace identity drift、observer/evaluator ambiguity 或费用硬门都在下一 arm 前
停止。Token、时间或费用改善不能补偿 verified success、安全或 false-success 回退。

产品决策只允许：

1. English 在两个任务语言层都不回归且相同或更好：保留单一 English prompt；
2. Chinese 在两个任务语言层都相同或更好：保留单一 Chinese prompt；
3. 各自在同语言层占优：不增加 Auto，最多预注册一个单一 compact bilingual candidate；
4. 无效/证据不足：保留 current Chinese DSE baseline，不声明语言优劣。

任一 winner 接管后删除 loser、eval-only selector/assets、临时 config/test surface 和双
production branch；whole-release rollback 是唯一 prompt rollback owner。

M17-F formal block 1 已完成并作出有效质量否决。candidate `73d02d05e` 的同一 immutable
`dse` binary 在 official ChatCompletions、`deepseek-v4-pro`、high、相同工具/预算/
verifier 下执行 32/32 measurement-valid arms，maximum reruns 为 0。结果为 26 个正向
verified、4 个正确安全拒绝、false success 0、263 requests、1,914,399 input tokens、
159,125 output tokens、USD 0.511237723 known cost。两个 prompt 在英文任务的 aggregate
verified 都是 6/7、中文任务都是 7/7，但 English prompt 在
`rust_scoped_rules:en` 形成一项 treatment-only loss；因此
`english_noninferior=false`，决策为 `retain_chinese_block1_quality_veto`，block 2 未执行。

raw 为 ignored `0600`、195 条完整 hash-chain、0 partial tail，SHA-256 为
`ecdb74675207081104c09f3be0910f3d646ab0e94b0e8e458543935f915500d5`。
cutover `c1856fa4b` 只保留一个中文表达 prompt，同时采用用户当前任务语言回答；其
normalized SHA-256 为
`a91799031d8f430945e98871f19d3cefd0496834304b4af0ff04197944ab1bdb`。
selector、English candidate、翻译 scaffolding、tracked eval assets 与 M17-F-only runner
均已删除。该结果不证明中文 prompt 普遍优于英文 prompt，只拒绝这个未通过正式非劣门的
English candidate。完整证据见
[M17-F DSE bilingual prompt 2x2](../../eval/summaries/m17-f-bilingual-prompt-ab-2026-07-25.md)。

M17-G public-repository gate 已在 `85e241223` 通过。英文 canonical README、完整中文
入口、贡献/安全/行为/来源文件、CODEOWNERS 和 issue/PR templates 由同一个离线 checker
验证 current protocol/model/architecture facts、链接、bash block、governance 与公开秘密
形状。checker 还冻结 ROADMAP/EVALUATION 与两个 M7 summary 中的 M7-A/M7-A2
`DeepSeek Agent` 历史标题；public candidate 没有修改任何 eval manifest、fixture、
schema、raw、frozen summary 或 hash。

前置 identity/localization 纠错 `389aac896`、`de6bc7004` 使 current context marker 与
TUI header 使用 DSE，并让 auth/model 嵌套帮助进入唯一 catalog owner。current catalog
为 776/776 exact keys；full assembled prompt fixture 的 current SHA-256 为
`d7746692db36eea33da0305553499b708a4d8b2b7d9688633aba1673142d49c9`。这只反映
`cw:ctx -> dse:ctx` identity marker，M17-F 被测 `a9179903...` hash、中文 prompt winner
和正式结论不回写。

public checker、真实 binary smoke、focused、strict Clippy、workspace test、crash/reopen、
LICENSE/provenance 与 tracked-secret audit 通过；没有 Key、DeepSeek API、push、远端改名、
visibility、tag 或 release。该 gate 接受本地公开仓库候选，不等于 M17-H 的远端 CI 或
公开发布证据。完整记录见
[M17-G DSE bilingual public repository](../../eval/summaries/m17-g-dse-public-repository-2026-07-25.md)。

M17-H local release-readiness 在 `a8c4bafab` 通过。active-source checker 删除并拒绝旧
`.codewhale` authority、whale theme alias/视觉与未分类旧身份，同时保留精确 migration/
negative assertion/frozen fixture 及 M7-A/M7-A2 历史 allowlist。没有新增 `.dse`
constitution，因为这会在正式 prompt 决策后增加未经评测的模型可见 authority。

同一 revision 的 focused、strict Clippy、workspace test、macOS locked/offline exact-source
package/install/verify/uninstall 和 Linux arm64 无网络 fixture lifecycle 均通过；269 个
ignored raw 文件现在全部为 `0600`，LICENSE 与导入基线 byte-identical。用户授权的
精确 non-force push 已使 private origin 与 `8c57c4dba` 对齐；该 SHA 的 GitHub Actions
run `30164259559` 因账户付款或 spending limit 在任何 job step 前被拒绝，因此不是代码
门禁结果，不能提供 private CI 成败证据。classic protection/rulesets 仍因套餐限制不可用。

用户随后把当前范围明确收敛为纯本地 DSE，不再操作 GitHub。M17-H 最终本地结论为
`local_v1_complete_external_github_release_deferred_by_user`：M17-A–H 的本地身份、双语、
prompt、locked/offline delivery、来源、秘密和清理门已完成；CI、protection、仓库改名、
visibility、tag 与 release 保持未执行的独立外部工作，不作为本地准入事实，也不阻塞本地
V1 收口。见
[M17-H DSE release-readiness](../../eval/summaries/m17-h-dse-release-readiness-2026-07-25.md)。

### M18 local first-day and current reliability baseline

M18 先对 fresh `DSE_HOME` 和 installed artifacts 执行纯本地 lifecycle gate。exact-source
package/install/verify、English/`zh-Hans` 首次 PTY、`dse exec`、same-Run resume、
upgrade、rollback 与 data-preserving uninstall 全部通过。该 gate 不依赖 GitHub 或
远端 CI。

正式 loss acquisition 冻结六个独立 task family、每个三次 position 1、同一 immutable
`eee72cb38295` binary、official ChatCompletions `deepseek-v4-pro`/high、相同工具/预算/
外部 verifier，且 `maximum_reruns=0`。fixture 初态 6/6 fail，reference patch 的五个
正向任务 5/5 pass，安全反例保持 fail；credential 前的 identity、Harness/self-test、
observer/acceptance conformance 和全 workspace 本地门禁通过。

campaign 在第 16 arm 的第三个 Writer repetition 达到 frozen `run_deadline` 后停止。
94-record ignored `0600` journal 无 partial tail，SHA-256 为
`5ace3b0b2222428991eec205f1ac9dc6a3f81dbc09beef88873bcb95343ef9c5`。
canonical read-only projection 对 15 条完整 Store trajectory 两次生成 byte-identical
report，SHA-256 为
`b031670020531b40ec5109184556dcc567295b83c3f4105b2d9ed0703839534f`。

有效 observation 为 11 个正向 verified、3 个正确安全拒绝、false success 0，以及一个
TypeScript external-verifier failure；Host 正确 blocked，未产生 false completion。同一
TypeScript task 的另外两次通过，因此 `deterministic_verifier_failed` 只覆盖一个独立
task ID。started-without-snapshot Writer 被单独投影为 measurement interruption，不计为
产品 loss，也不推断其 physical request accounting。

预注册选择门是同一 stable loss 跨至少两个独立 task ID 重复。M18 结果为
`insufficient_repeated_current_loss`，没有 production candidate、重跑、补 mate、工具/
prompt/recovery 改动或效率结论。fixed root/Writer Pro-high、read-only Flash-high、
typed Pro-max recheck、唯一 Runtime/Store 与 canonical tools 保持不变。完整身份、结果、
删除和非结论见
[M18 DSE local first-day and reliability baseline](../../eval/summaries/m18-local-first-day-and-reliability-2026-07-26.md)。

### M19 local coding reliability acquisition

M19 使用全新 position-1 identity，不续跑、补 mate 或拼接 M18。唯一 evaluator 仍是
corrected `scripts/eval-m9b-fixed-pro-regression.py`；production 仍固定
`deepseek-v4-pro/high`、official OpenAI-format ChatCompletions、唯一
AgentRuntime/RunStore 与 canonical tools。

Credential 前必须同时满足：

- 五个全新 fixture 初态 external verifier 全部 fail 且 tree byte-stable；
- reference patch 使两个独立 TypeScript recovery、Rust cross-file debug 和 explicit
  Writer 四个正向任务 pass，安全反例仍 fail；
- 五个 TaskContract、allowed scope、deterministic Git base、三轮 schedule、tool policy、
  runtime 720 秒、outer watchdog 840 秒与 `maximum_reruns=0` 固定；
- deadline projection 的 in-flight、known complete、billing unknown 与 no-attempt
  分类自测通过；journal 顺序固定为
  `arm_started -> deadline_interruption_snapshot -> abort`；
- M14 observer、M16 acceptance equivalence、四个 journal SIGKILL window、SQLite
  reopen、root/Writer/safety lane、focused、fmt、strict Clippy 和 workspace test 通过；
- immutable release binary、manifest/harness/task/schedule hash、official endpoint/model/
  price fixture和 no-Key dry-run一致。

每个完整 arm 的唯一观察顺序仍是：

```text
terminal snapshot
  -> canonical Store snapshot
  -> stop credential-bearing process
  -> credential-free SQLite reopen snapshot
  -> external verifier
  -> changed scope / route / lane / receipt
  -> accounting-complete arm label
```

outer watchdog 例外顺序为：

```text
stop credential-bearing process
  -> credential-free same-Store reopen
  -> durable deadline_interruption_snapshot
  -> one run_deadline abort
  -> no next arm
```

deadline snapshot 只报告 canonical facts：terminal 是否存在、physical
started/completed/in-flight、usage complete、sealed、billing_unknown。只要没有可完整推导
arm label，它就固定为 measurement/product-loss ineligible；in-flight 不猜测 billed 或
unbilled。若无 Key 重开观察到 durable terminal，可补齐同一物理 arm 的 terminal/Store
snapshot，这只是 observer recovery，不是 rerun；仍须通过 exact reopen 与完整 accounting
门。

正式矩阵为五类任务 × 三次 = 15 arms：两个独立 TypeScript `failed_write_pass`、一个
Rust cross-file debug、一个 explicit long Writer、一个安全拒绝。完整 acquisition 必须
15/15 measurement-valid、false success 0、route/lane/reopen/accounting exact。任何
unknown billing、incomplete usage、unpriced response、retry、nonterminal watchdog、
observer ambiguity、identity mismatch 或费用越界在下一 arm 前停止。

production candidate 的最低触发条件不是失败总数，而是同一 stable loss 经 canonical
trajectory 归因后跨至少两个独立 task ID 重复。只出现一个独立任务、不同 owner、evaluation
scope/environment mismatch 或 measurement interruption 时，必须
`insufficient_repeated_current_loss` 并保持 production 不变。

M19 的 offline gates、candidate binary 与 no-Key dry-run 全绿，但正式 position-1 arm
的首个物理请求在 response headers/finish/usage/content/reasoning 前得到 typed
`deepseek_transport`。canonical Store/reopen 证明
`started=1 / completed=1 / in_flight=0 / sealed=true / billing_unknown=true /
complete=false`，且没有 retry。Harness 在 0 个完整 arm 后唯一
`accounting_incomplete` abort，后 14 个 arm 未执行。

8-record ignored `0600` journal 无 partial tail。read-only report 两次
byte-identical：1 个 canonical trajectory、0 个 arm result、1 个
`typescript_forwarded_chain_recovery` measurement interruption、0 false success、0
product loss、0 repeated independent loss。这里的 false-success 零不构成质量结论，
因为没有 measurement-valid arm。

结论为 acquisition `stop_incomplete_accounting`、product
`insufficient_repeated_current_loss`。不得重跑、补 mate、推断 billed/unbilled 或从初始
external verifier failure 推导 coding loss；production 保持不变。完整 identity、raw hash、
report hash、删除和非结论见
[M19 DSE local coding reliability acquisition](../../eval/summaries/m19-local-coding-reliability-2026-07-26.md)。

### M20 transport viability and fresh fixed-Pro successor

M20 的第一项评测对象不是模型能力，而是当前本地 transport 证据边界。审计发现旧
`dse doctor` 会在 canonical Runtime/Store/accounting 外发送 one-token Chat request。
cutover 后它只通过 production connection config 发送一次 authenticated
`GET /user/balance`，不 inference、不重试、不进入 model request ledger，也不保留余额。

正式 viability 合同要求同一 immutable binary 连续三个探针、`maximum_reruns=0`、
0 model request、0 known API cost；任一 DNS/connect/TLS/HTTP/auth/shape failure 都停止。
结果为 3/3 `reachable`，耗时 554/384/387 ms。该证据只 collectively 证明当时的官方
host/credential reachability，不证明 Chat inference、Agent completion、单 request
billing 或 pre-header reconciliation。

viability 通过后才冻结新的 M20B position-1 acquisition。M20B 不输入 M19 raw、不续跑
M19，也不复用任务；它固定三个新 task × 三次、fixed Pro/high、同一 binary、external
verifier、Store reopen 与零 rerun。

正式 acquisition：

- `python_scope_token_recovery` verified，false success 0，8 个 physical response 的
  usage/accounting 完整；
- `typescript_request_budget_recovery` 的 external verifier pass，但第六个 response
  typed `deepseek_transport`；六个 response 只有五个 usage，
  `billing_unknown=false / usage_incomplete=true / sealed=true`；
- 只完成 1 个 measurement-valid arm，随后一个 `accounting_incomplete` abort，后 7 arm
  未执行；
- 14-record ignored `0600` journal 无 partial tail；read-only projection 两次
  byte-identical，得到 2 trajectory、1 arm result、1 TypeScript measurement
  interruption、false success 0、0 product loss、0 repeated independent loss。

因此 M20B 不是完整 9-arm baseline；verifier pass 也不能越过 incomplete accounting
成为完整产品 label。决策为 `stop_incomplete_accounting /
insufficient_repeated_current_loss`，没有 production candidate、quality aggregate、
cost comparison 或 retry admission。Doctor 的非推理 cutover 因删除 untracked
inference side effect 而保留。详细 frozen identity、raw/report hash、官方链接和
非结论见
[M20 transport viability and fixed-Pro successor](../../eval/summaries/m20-transport-viability-and-fixed-pro-successor-2026-07-26.md)。

### M21 partial-response owner audit

M21 的评测对象是 M20B 第六个 physical response 的本地 owner，不是模型能力、产品质量
或成本。它禁止 Key、官方 API、external network、付费重采、mate 和历史 raw 拼接。
唯一输入是 immutable M20B journal 的 redacted typed facts，以及独立提交的安全 fixture。

冻结事件顺序为 prepared 2989、in-flight 2990、reasoning delta 2991、failed 2992、
terminal 2993。failure 在 reasoning delta 后 56 ms 提交；这排除了 120 s Harness
model-event idle 和 900 s production stream idle。external verifier 已通过，但没有
ModelResponseCommitted、CompletionProposed 或最新 Host completion receipt，因此仍是
failed terminal。

credential-free deterministic matrix 覆盖：

- 真实 loopback HTTP response 在一个合法 reasoning SSE frame 后截断 declared body；
- DeepSeek transport/parser/error projection/accounting 的完整调用链；
- Runtime 在 actionable partial output 后禁止 replay，即使 retry budget 大于零；
- incomplete response 与 pre-header billing-unknown 的区分；
- SQLite failure/evidence/retry/accounting exact reopen；
- 现有 in-flight process recovery 不重新发出 request。

三层新增回归全部通过，且 replay 与 frozen contract 逐项一致。M21 因此没有 production
fix eligibility；结论为
`no_local_defect_reproduced / keep_fail_closed_stream_accounting /
upstream_response_body_interruption_not_reconstructible`。这不识别是 provider、代理还是
网络中的哪个组件关闭了 body，只把可观察 owner 边界固定在 response-body stream。没有
usage 时继续 fail closed，partial reasoning 后继续不盲重试，external verifier pass
继续不能自动伪完成。

M21 不授权重跑 M19/M20B、恢复未收到的 usage、增加 sender/retry/controller，也不形成
fixed-Pro quality aggregate。完整 fixture、manifest、raw identity、命令与非结论见
[M21 partial-response owner audit](../../eval/summaries/m21-partial-response-owner-audit-2026-07-26.md)。

### M22 canonical streaming-delta convergence

M22 的评测对象是 canonical streaming delta 的本地写放大，不是模型能力或付费任务质量。
它禁止 Key、官方 API、external network、GitHub、远端 CI、push 和 release。冻结
M20B journal 只派生两个 redacted scale profile：

- TypeScript：2,993 total Store event，2,743 reasoning + 209 content delta，
  delta UTF-8 payload 合计 10,097 bytes；
- Python：5,298 total Store event，4,769 reasoning + 462 content delta，
  delta UTF-8 payload 合计 17,927 bytes。

fixture 不保存原始文本或身份。baseline manifest 在 `c7a770457469` 独立提交并从该
immutable identity 运行。每个 profile 使用真实 `StateStore`、Run API canonical JSON、
TUI canonical projector 和 headless serializer，1 warmup + 5 measured repetition，
`maximum_reruns=0`。预注册 material gate 要求 delta share 至少 90%，且任一稳定绝对指标
越过门槛。5,234 synthetic event profile 的 Run API+JSON 中位数为 122.883ms，稳定超过
100ms，故只授权一个最小 candidate。

candidate manifest 冻结以下质量门：

- complete response 的 concatenated reasoning/content bytes、finish 与 usage 必须一致；
- malformed/incomplete frame 必须先投影已经收到的 actionable delta，再 typed failure；
- partial output 继续 replay unsafe，不增加 retry、completion 或 side effect；
- RuntimeEvent v19、State v25、Run API v12 与 exec-stream v4 不变；
- SQLite reopen、SIGKILL、root/read-only/Writer 和 CLI/TUI/API projection 必须一致；
- 两个 profile 的 event 和 SQLite 各至少下降 50%，原 material wall metric 至少改善
  20%，任一 wall metric不得回退超过 10%。

唯一 candidate 在 `crates/deepseek/src/transport.rs` 合并同一已接收 HTTP body chunk 内
相邻同类文本 delta。它不等待下一 chunk，evidence/tool/finish/usage/DONE/error 都是
barrier。正式 `9f59d4a44fbc` 同源码结果：

| profile | baseline delta | candidate delta | event reduction | SQLite reduction | Run API+JSON improvement |
|---|---:|---:|---:|---:|---:|
| TypeScript | 2,952 | 8 | 99.73% | 94.33% | 97.97% |
| Python | 5,231 | 9 | 99.83% | 96.05% | 98.62% |

五次 transport samples 的 candidate event count 完全一致。State downstream A/B 中，
TypeScript candidate 的 append/reopen/Run API/TUI/headless 中位数为
2.023/1.266/1.475/0.075/0.050ms；Python 为
2.367/1.289/1.758/0.086/0.076ms。candidate SQLite+WAL 为
90,112/106,496 bytes，canonical Run API JSON 为 15,885/23,874 bytes。所有 efficiency
门通过。

DeepSeek transport 全测试、M21 truncated-body regression、malformed-frame partial
flush、production loopback、Store snapshot-neutral delta、runtime conformance、
surface parity、focused、fmt、strict Clippy 与 workspace test 是保留门。结论为
`keep_minimal_streaming_delta_convergence`；旧 per-frame production emission 已删除，
没有 compatibility branch、第二 sender/Store/Runtime 或事件真相。该结果不证明真实官方
网络每个 chunk 都有同样收敛率，也不形成 fixed-Pro coding quality/cost aggregate。完整
identity、raw hash、命令、删除和非结论见
[M22 canonical streaming-delta convergence](../../eval/summaries/m22-streaming-delta-convergence-2026-07-26.md)。

### M23-A behavior/accounting truth orthogonality

M23-A 接受 ADR-0011，只改变现有 corrected Harness/analyzer 的派生规则，不改变
production Runtime、Store、transport、protocol、工具、route 或 completion owner。

提交的 10-case offline corpus 覆盖 complete success、正确安全拒绝、行为成功但 usage
不完整、partial response、pre-header failure、deadline kill、observer mismatch、
external verifier pass 但无 Host receipt、unpriced 和 false success。结果为 10/10，
连续两次 canonical report byte-identical：

```text
behavior:
  verified_success = 3
  correct_safety_rejection = 1
  verified_product_failure = 4
  measurement_interruption = 1
  invalid = 1
  false_success = 1

accounting:
  complete = 5
  usage_incomplete = 2
  billing_unknown = 2
  unpriced = 1
```

这组 case 故意不是产品成功率样本；它只证明两个轴互不覆盖。尤其：

- completed + latest Host receipt + verifier pass 在 usage incomplete 时仍保留
  `verified_success`，但不得进入 cost/full-utility aggregate；
- typed failed terminal 在 partial/pre-header accounting 中断时仍是
  `verified_product_failure`；
- 没有 production terminal 的 deadline kill 仍是 `measurement_interruption`；
- external verifier pass 没有 latest Host receipt 仍是 product failure，不是成功；
- observer/environment ambiguity 仍为 `invalid`。

旧 `trajectory_loss_projection(arm_result=None) -> measurement_incomplete` 分支已删除。
current analyzer 现在把 canonical Store、无凭据 SQLite reopen、verifier snapshot 与
terminal 连接后分别输出 behavior/accounting aggregate；frozen raw、manifest、summary
和历史 admission decision 不改写。M11/M12/M15/M18/M19/M20B journal 均可重算，M15 的
reference-file 假阳性在新 behavior truth 中保持 false success 0，不同 actor owner 的失败
不会被粗粒度合并为重复损失。

M23-A 决定为 `keep_orthogonal_behavior_accounting_truth`。Key/API/network 为 0。
该结果不建立 Hardness baseline、不授权 high/max A/B，也不授权四个 production candidate；
下一步仍是 M23-B 的全新 18–24 task control-only contract。完整身份、删除和门禁见
[M23-A behavior/accounting truth](../../eval/summaries/m23-a-behavior-accounting-truth-2026-07-26.md)。

### M23-B1 Hardness task-set conformance

M23-B1 只冻结和自证 control contract，不把 fixture reference solution 当成 fixed-Pro
能力样本。唯一 corrected Harness 现有 caller 新增 `m23b` campaign；production
AgentRuntime、RunStore、DeepSeek sender/accounting、route、tools 和 completion owner
均未改变。

冻结 task set 为 20 task × 3 round = 60 future arm，`maximum_reruns=0`。语言覆盖
Rust 5、TypeScript 7、Python 7、Go 1；actor/lane 覆盖 root 13、read-only child 2、
explicit Writer 2、安全反例 3；strata 覆盖 localization 4、cross-file 7、
failure/recovery/safety 5、long-horizon 3、service/API/UI 3 与 Writer 2。每个 task
冻结 5–20 个相关文件、human-estimated minutes、allowed/reference scope、external
verifier、continuity/runtime assertion 和 fixed tool policy。

Credential-free self-proof 结果：

```text
positive initial-fail -> scoped reference patch -> pass = 17 / 17
safety initial-fail -> no mutation -> still fail          = 3 / 3
materialized independent base repositories                = 20 / 20
journal crash windows                                      = 4 / 4
freeze report reproducibility                              = byte-identical
self-test reproducibility                                  = byte-identical
Key / API / network / model request                        = 0 / 0 / 0 / 0
```

任务集同时包含真实 loopback Go HTTP verifier 和真实 loopback server + 固定本机
Chrome/Playwright DOM verifier；这只证明 verifier 与资源契约可离线执行，不证明 DSE
已经能完成相应任务。M15/M20B self-test、M14 observer、M16 acceptance-equivalence 与
M23-A truth conformance 继续通过。

结论为 `keep_offline_hardness_task_set_control_not_acquired`。因为没有任何 model
trajectory，`control_baseline_acquired=false`，不能计算 pass@1、pass^3、false success、
first-relevant-file 或 stable owner/cause loss matrix。M23-C 和四个 production
candidate 继续禁止；M23-B 只有在新的独立 fixed-Pro/high control acquisition 闭合后才
能退出。完整冻结 identity、hash 与非结论见
[M23-B1 Hardness task set](../../eval/summaries/m23-b1-hardness-task-set-2026-07-26.md)。

### M23-B2 Hardness metric / continuity observer

M23-B2 保持 production delta、Key、API、network 和历史 raw 读取为 0，只补唯一 corrected
Harness 的 credential-free 派生契约。canonical RuntimeEvent 的
`occurred_at_unix_ms`、tool prepared/outcome、mutation epoch、compaction 与 Host
verification 是定位、首改和恢复指标的唯一输入；冻结 external verifier 是 runtime
assertion 和 verified service start 的唯一输入。observer 不保留 prompt、reasoning、
tool argument/content 或文件路径明细，只输出计数、时延和稳定布尔事实。

4-case corpus 连续两次生成 byte-identical report：

```text
cases                                  4 / 4
real mid-run resume                    1
expected goal-constraint loss          1
runtime assertion case                 1
terminal SQLite reopen counted resume  0
Key / API / network / historical raw   0 / 0 / 0 / 0
```

resume 必须同时证明 `interaction_requested` durable checkpoint、事件前缀重开
byte-exact、不同进程 identity、重开时 physical requests 未增长，以及交互只在重开后
解析。终态后的 credential-free SQLite exactness audit 不满足这些条件；缺失或非法的
required continuity 会直接得到 `goal_constraint_loss=true`，不能作为 measurement-valid
长任务。

结论为 `keep_offline_hardness_metric_observer_live_continuity_pending`。live Harness
尚未执行该中途 restart/resolution protocol，也尚未把指标写入正式 arm/aggregate，所以
M23B formal 入口在读取 credential 前以
`m23b_live_continuity_not_implemented` fail closed。该切片不建立 control baseline，
不授权付费采集、high/max 或 production candidate。完整身份与门禁见
[M23-B2 Hardness metric observer](../../eval/summaries/m23-b2-hardness-metrics-observer-2026-07-26.md)。

### M23-B3 Hardness live continuity caller

M23-B3 保持 production delta、official Key/API、external network 和历史 raw 为 0，
把 B2 observer 接入正式 caller 所需的真实 process lifecycle。3 个冻结长任务独占
`interactive=true`、`auto_approve=false` 覆盖；其余 57 个 arm 的 fixed-Pro/high
输入、工具、预算和 controls 不变。

credential-free process self-test 使用现有 app-server external-process test child 与
loopback official ChatCompletions SSE，实际经过唯一 AgentApplication、AgentRuntime、
production tools 和 SQLite RunStore：

```text
report regeneration                    byte-identical
durable interaction requested/resolved 1 / 1
physical request before / at reopen     1 / 1
physical request final                  2
process restart                         1
applied tool side effect                1
terminal credential-free reopen         exact
official Key / API / external network   0 / 0 / 0
```

caller 在 `interaction_requested` 后 SIGKILL 整个 app-server process group；新进程必须
拥有不同 PID、byte-exact event prefix 和不增长的 physical request count，之后才可
resume 同一 root 并解析既有 approval。任何 user-input interaction、多个并发 pending
interaction、prefix/request count 差异或重复副作用都 fail closed。后续 approval 只在
重开进程内解析，不制造第二次 restart。

measurement-valid arm 新增 B2 metrics 与 ADR-0011 behavior/accounting truth；aggregate
新增 `pass_at_1`、严格连续三次的 `pass_power_3`、goal constraint、resume、定位、
首改、repair、compaction、service/runtime 指标。60-arm 合成门证明 3 个长任务的
3 round 恰有 9 次 resume；普通 task 不获得交互或重启行为。

结论为 `keep_live_continuity_caller_control_not_acquired`。正式入口已删除旧的
`m23b_live_continuity_not_implemented` 空实现 guard，但仍需要独立、hash-bound live
admission、clean immutable binary、费用确认与显式 Key；本切片没有创建 admission 或
results，不建立 success、false-success 或 loss matrix，也不授权 high/max 或四个
production candidate。完整身份、门禁与非结论见
[M23-B3 live continuity caller](../../eval/summaries/m23-b3-hardness-live-continuity-2026-07-26.md)。

### M23-B4 Hardness fixed-Pro/high control stop

独立 admission 绑定 `e68d215c` candidate、`8090adce` admission、immutable binary、
唯一 corrected Harness、20-task × 3-round schedule、B2/B3 truth、费用上界与
`maximum_reruns=0`。formal runner 只在 offline 门禁、official DeepSeek protocol/price
复核和 clean admission commit 后读取 ignored 0600 Key。

正式采集得到：

```text
complete arms / started trajectories    5 / 6
verified success                        5
verified product failure                1
false success                           0
accounting complete / usage incomplete  5 / 1
full-utility observations               5
complete-arm physical requests          39
complete-arm input / output tokens      535,669 / 24,743
complete-arm cache hit / miss tokens    428,672 / 106,997
complete-arm cost / wall time           USD 0.069624041 / 380,131 ms
real process resume                     1
```

第 6 个 `readonly_service_graph` 的 production terminal、canonical Store、无凭据 SQLite
reopen 与 external verifier 已闭合，但一个 response 没有完整 usage；runner 在下一物理
request 前以 `accounting_incomplete` 停止。没有 rerun、mate、续跑或把 54 个未执行 arm
计入样本。

现有 trajectory-report mode 两次生成 byte-identical report。ADR-0011 behavior 轴把第
6 个轨迹归类为
`deepseek_transport:deepseek_transport` verified product failure；accounting 轴独立归类
为 usage incomplete。该 loss 仅出现于一个独立 task，未达到至少两个 task 的 candidate
门槛。因此：

```text
acquisition decision  stop_incomplete_accounting
loss decision         insufficient_repeated_current_loss
candidate             null
M23-C high/max         not authorized
M23-D treatment        not authorized
production delta      0
```

B1 task contract、B2 metric observer、B3 live continuity caller 与 ADR-0011 analyzer
保留；ignored 0600 raw 只作为不可变审计证据。M23 不补跑当前 schedule，也不从单个
transport failure 制造 ApplicationProbe、localization、VerifiedMilestone 或 Tool ACI。
完整 hash、官方来源、安全与非结论见
[M23-B4 Hardness control](../../eval/summaries/m23-b4-hardness-control-2026-07-26.md)。

### M24 current local V1 release-candidate revalidation

M24 是 exact-source release regression，不是模型 treatment、能力 A/B 或 M23 successor。
它冻结 clean candidate `92d8b84b3a79` / tree `ef5983a13b26`，并只使用现有
delivery/public-checker/Rust conformance owners。Key、official API、external network、
GitHub、push 和 release 均为 0。

macOS arm64 的 Rust/Cargo 1.97.0 locked/offline source build 产生
`dse-0.8.68-aarch64-apple-darwin-92d8b84b3a79.tar.gz`。archive 精确包含 manifest、
LICENSE、SHA256SUMS、`dse`、`dse-tui` 五项；manifest 绑定 exact revision/tree、
Cargo.lock、target 和 canonical binary set。archive SHA-256 为
`54578a5ddb2596b595553e3ef617708e927259dbe9b887b3fd58e7879f52b92a`，两项 binary
SHA-256 分别为
`e2d197103af0c757c6b5eaec4c3c70c0054328417308465703d2742ae51c1ac0` 与
`0a58c9dcc8cc17250b111055ca262a6b2458eaeec0dffac17250acf2a3e1e447`。

交付 self-test 的 tamper/target/install/upgrade/rollback/uninstall matrix 通过；当前 exact
artifact 又在隔离 prefix 中作为真实升级目标完成 verify、rollback、重新激活与 uninstall。
installed binaries 均报告 `0.8.68 (92d8b84b3a79)`；English/`zh-Hans` help 正确；
卸载后程序链接和 `lib/dse` 消失，用户数据 SHA-256
`33d367336b9e5bc32afe4cf3fb1694193036f288a623bc04b51ef8a2b9a05cf7` 保持不变。

credential-free local gates 通过：

- public repository checker、LICENSE imported-baseline identity、provenance、
  tracked-secret/retired-identity audit；
- 776/776 localization key 与 named-placeholder parity；
- fixed root/read-only/Writer route、typed Pro/max recovery、M22 streaming 和 M21
  partial-response regression；
- pending Start、exact RequestPlan/accounting SQLite reopen、process SIGKILL/replay；
- CLI/TUI/app-server canonical parity、English/CJK PTY；
- focused、fmt、workspace strict Clippy 与完整 workspace test。

结果为 `keep_local_v1_release_candidate_no_blocker`，production delta=0。M24 没有
verified-success、Token、费用、M23 baseline 或模型质量结论，也不授权 high/max、
ApplicationProbe、localization、VerifiedMilestone 或 Tool ACI。完整身份、命令、删除与
非结论见
[M24 local release candidate](../../eval/summaries/m24-local-release-candidate-2026-07-26.md)。

### M25 repository agent-legibility cutover

M25 是 credential-free repository-maintenance A/B，不是 Agent/model treatment。baseline
`748ed47b8` 的根 `AGENTS.md` 为 379 行 / 21,307 bytes，其中 227 行重复 mutable milestone
history；12 个冻结稳定规则的首次位置中位数/最大值为 309.5/375。只读 inventory 同时
记录 54/24/7 个 tracked Rust 文件超过 1k/2k/5k 行，以及 16 crate / 51 internal edge，
但后两者没有 current wrong-owner、review、compile 或 architecture violation，因而没有
进入候选。

唯一 treatment 把 guide 收敛为 134 行 / 5,488 bytes，删除 milestone/checkpoint ledger，
保留 12/12 rule anchors 和五个 authority links，并把 owner map 稳定到同一短入口。规则
首次位置中位数/最大值变为 76/131，lines/bytes 分别下降 64.64%/74.24%；
milestone/commit identity 从 `42/12` 变为 `0/0`。

现有 `scripts/check-public-repository.py` 是唯一机械 owner；它强制 line/byte budget、
authority link resolution、stable rules 和无 mutable history，并以 oversized、
missing-authority、mutable-history 3/3 负向 fixture 防止 false green。没有第二 checker、
parallel docs、Rust/module move、dependency policy 或 production branch。

focused/fmt/strict Clippy/workspace test 通过。workspace 首轮一个 stream-stall test 在并行
负载下先观察到 retry-open network error；同一 test 定向单线程通过，第二次完整 workspace
也通过，且候选没有 Rust/transport delta。clean detached `6981f54fb` 又通过 public
checker、delivery lifecycle 与 exact locked/offline artifact checksum/install/
verify/uninstall。

决定为 `keep_compact_agent_guide_contract`。production code、Runtime、Store、protocol、
model、prompt 和 tools delta 为 0；Key/API/network/GitHub/push/release 为 0。该结果只
支持 repository instruction bytes、规则定位和历史重复债下降，不形成 DeepSeek Token、
cache、费用、wall-time 或 verified-success 结论，也不授权按超大文件或 edge count
继续重构。完整证据见
[M25 agent-legibility cutover](../../eval/summaries/m25-agent-legibility-2026-07-26.md)。

### M26 canonical Run legibility projection

M26 是 credential-free TUI presentation cutover，不是模型或 Runtime treatment。旧基线
只有 child-Agent WorkSurface 和一个 TUI 私有字符串 turn status；它不能在稳定区域闭合
task、activity、workspace change、Host verification、terminal 与 recovery。

候选只读取 canonical `StoredRuntimeEvent`，离线断言：

```text
root phase / terminal projection                 pass
child Run cannot replace root                    pass
completion rejection keeps rework phase          pass
Writer files count only after root integration   pass
localized root WorkSurface loop                  pass
wide right rail / narrow Top fallback            pass
Chinese wide real PTY + SQLite truth             pass
English narrow real PTY + SQLite truth           pass
```

旧 `runtime_turn_status` 和对应 string mapper 已删除；live/replay presenter test 直接比较
同一 `CanonicalRunPresentation`。普通 root edit 没有 canonical filename receipt 时只显示
confirmed change，不推断文件数。permission row 只投影冻结的现有
`RunEnvironment.auto_approve`，没有新增权限模式。

决定为 `keep_canonical_run_legibility_projection`。production RuntimeEvent/Run API/State、
model、prompt、tools 和 accounting delta 为 0；Key、official API、external network 为
0。focused、fmt、strict workspace Clippy、完整 workspace test、public-repository 和
diff check 全部通过。该切片不建立 verified-success、Token、cache、cost 或 wall-time
结论。

### M27 canonical permission policy

M27 是 Host authorization 与 TUI projection 的本地纵向切片，不是 DeepSeek 模型能力
A/B。其反事实是当前 `auto_approve=false/true` 粗粒度链；候选必须同时减少无意义审批并
保持所有边界、deny、副作用与 replay truth。

#### 冻结评测矩阵

同一 immutable loopback binary、同一 tool-call script、同一 workspace revision 依次覆盖：

| case | Ask | Agent decides | Full access |
|---|---|---|---|
| read/list/grep | allow | allow | allow |
| workspace edit | allow | allow | allow |
| run_tests/verifier | allow | allow | allow |
| external canonical path | ask | allow unless high-risk | allow |
| network-capable invocation | ask | allow unless high-risk | allow |
| Host-classified critical | ask | ask | allow |
| explicit execpolicy deny | deny | deny | deny |
| invalid/path escape/hard invariant | deny | deny | deny |

每个 case 同时断言：

```text
frozen RunPermissionMode
matched dimensions and execpolicy rule
ToolAuthorizationDecision
approval interaction count and exact arguments digest
ToolPrepared / ToolExecutionStarted / ToolOutcome order
side_effect status
workspace revision
credential-free RunStore reopen projection
TUI localized label
```

#### 必须为零

- model self-approval；
- UI label 与 Run policy 不一致；
- denied/未批准 invocation 的副作用；
- approval 后 arguments 或 workspace revision 偷换；
- crash/reopen 重复执行；
- root -> child / Writer 权限升级；
- config/project file 产生隐藏 permission mode；
- Full access 绕过 explicit deny 或 Host hard invariant；
- TUI/exec/app-server 同输入不同决策；
- compatibility reader、dual write、second approval cache 或 second policy snapshot。

#### 机制与产品门

机制通过要求所有 policy × tool × rule 单测、State migration、process SIGKILL/reopen、
English/Chinese PTY、keyboard/mouse/resize 与全仓门禁闭合。

产品保留要求：

1. Ask 的普通 workspace edit/test approval count 从旧基线的逐次 prompt 降为 0；
2. external/network/critical 在 Ask 下仍 100% 进入 exact durable approval 或 fail closed；
3. Agent decides 只跳过 Host 已分类为非 critical 的调用，critical prompt 召回为 100%；
4. Full access prompt 为 0，但 explicit deny/hard invariant 拦截率为 100%；
5. denial、cancel、approval、SIGKILL 后的 side-effect/replay truth 无回归；
6. 代码只保留一个 production matcher 和一个 final authorization owner。

若 external/network scoped authority 在当前平台不可强制，该单元不得计为 allow；候选应
缩为 fail-closed，并同步缩小 UI 文案。弹窗出现不是 enforcement evidence。

M27 不读取 Key、不调用 official DeepSeek API，不产生 Token/cache/cost/quality 结论。
最终结论只能是：

```text
keep_typed_permission_loop
shrink_to_enforceable_permission_subset
reject_and_delete_permission_candidate
```

完整架构与实施合同见
[ADR-0012](../decisions/0012-canonical-permission-policy.md) 和 ROADMAP M27。

#### M27 result

决定为 `shrink_to_enforceable_permission_subset`。

- 三个且仅三个 protocol mode、Host authorization、durable approval、exact replay 与
  双语 TUI selector 已闭合；
- Ask 的普通 workspace edit/test prompt 为 0；
- Ask 对 canonical path-bearing tool、显式 Shell cwd 和可识别 network 调用因当前 backend
  无法证明 scoped authority 而 typed fail closed；任意子进程内部 I/O 不作超出 OS
  sandbox 基线的声明；
- Agent critical 保持 exact Ask，FullAccess prompt 为 0，explicit deny 与 hard invariant
  仍 100% 拦截；
- approval 后 revision 漂移产生 `authorization_stale`，start/tool call/side effect 均为 0；
- root、read-only child、Writer、pending Start、SQLite reopen、SIGKILL 与
  CLI/TUI/app-server projection 使用同一 frozen truth；
- 旧 bool/config/matcher/parser/richer policy/Starlark/`workspace-trust.json` reader 和
  无 RunStore 真相的 `dse-tui sandbox run`、仅 TUI 生效的 `exec_policy` feature
  分叉已物理删除；拒绝 guards、v26 migration negative fixtures 与 frozen 历史证据
  是唯一旧名称 allowlist。

本地 deterministic loopback、English/`zh-Hans` keyboard/mouse PTY、focused、fmt、
strict workspace Clippy、workspace test、public repository checker 与 diff check 构成
最终门禁。Key、official DeepSeek API、GitHub、push 与 release 均为 0。该结论只证明
Host 本地权限行为和历史债删除，不外推模型质量、Token、费用或 wall-time。

完整结果：
[M27 canonical permission policy](../../eval/summaries/m27-canonical-permission-policy-2026-07-26.md)。

### M28 DSE native TUI surface cutover

M28 是 credential-free presentation/interaction replacement，不是 DeepSeek model、
prompt、tool、permission 或 Runtime treatment。baseline 是 M27 clean checkpoint 上真实
可达的 Underwater/Ocean shell、M26 canonical Run presentation、M27 typed permission
selector、现有 onboarding/user-input/approval/pager 和 settings reader。

#### Frozen state and surface matrix

同一 immutable binary、loopback model、workspace 与 canonical Store 至少覆盖：

| dimension | required cases |
|---|---|
| lifecycle | first-run, idle, typing, running, waiting, rework, failed, blocked, cancelled, completed |
| activity | thinking, streaming, tool running, tool success/failure, collapsed/expanded detail |
| interaction | approval, permission, short/long user input, slash, mention, pager, cancel/close |
| actor | root, read-only child, explicit Writer |
| recovery | live, resume, SQLite reopen, resize during active state |
| size | 48×12, 60×16, 80×24, 100×32, 140×40 |
| language | en, zh-Hans, CJK/combining/emoji/long path stress |
| terminal | ANSI-16, truecolor, reset background, mouse on/off, reduced motion, SSH/constrained |
| macOS host | Apple Terminal, iTerm2, Ghostty where locally available |

每个 case 保存或断言：

```text
surface kind and opener
canonical RunPresentation digest
visible task / phase / change / verification / required action / terminal
focused object and render-time hitbox
keyboard action / equivalent mouse action
composer contents and cursor
scroll and resize state
localized message ids
frame overflow / clipping / exit path
live / reopen presentation equality
```

#### Correctness and usability gates

必须为零：

- model claim rendered as Host evidence；
- guessed progress、ETA、confidence、file count 或 terminal truth；
- hidden required action、false success 或 child replacing root；
- denied/approval/permission semantics changed by presentation；
- mouse-only action、invisible focus、unclosable sheet/room；
- clipping/panic/invalid Unicode width in frozen dimensions；
- reachable hardcoded human text or `en`/`zh-Hans` placeholder drift；
- periodic idle frame mutation or presentation-state disk write；
- production Underwater/Ocean/fish/bubble/ambient animation；
- reachable generic centered modal、General Settings、legacy visual mode；
- selectable layout/decorative setting reader or second palette/theme owner；
- RuntimeEvent/Run API/State/DeepSeek/tools/permission semantic delta。

必须通过：

1. task -> activity -> change -> verification -> required action/terminal 在每种状态可见闭合；
2. live 与同一 RunStore reopen 的 semantic presentation equivalent：尺寸、非空
   glyph/坐标、前后景、modifier 与 cursor 精确；只规范化 clear/overwrite 的等价
   terminal-default 空白编码，不宣称 raw PTY bytes 相等；
3. keyboard/mouse 同 action parity 100%，`Enter/Esc/↑/↓/Ctrl-C/Ctrl-D` 不改变既有
   canonical semantics；
4. 宽屏 right rail、中屏 top strip、窄屏 single-column 只改变 layout，不改变事实和焦点；
5. ANSI-16 不丢信息，truecolor 只改善层级；
6. idle 初帧后的 5 秒观察窗口没有内容 frame change、timer-driven full redraw 或 TUI 写盘；
7. old renderer、setting、normalizer、message、fixture 与 adapter 在 cutover 物理删除；
8. focused、fmt、strict workspace Clippy、完整 workspace test、public checker 与
   `git diff --check` 全部通过。

#### Admission and claims

M28 只有以下可接受结果：

```text
keep_native_surface_and_delete_legacy
rework_before_cutover
```

`rework_before_cutover` 不能以 permanent old/new toggle、hidden legacy theme 或
compatibility reader 结束。只有全部 gate 通过并删除旧 path 后才记录
`keep_native_surface_and_delete_legacy`。

不使用 LLM-as-judge、付费模型请求或单张“好看截图”决定准入。真实 PTY 与 frame contract
证明的是 presentation correctness、交互一致性、idle efficiency 和历史债删除；不得外推
verified task success、Token、cache、费用或模型 wall-time 提升。完整架构和实施合同见
[ADR-0013](../decisions/0013-native-tui-surface-system.md)、[DESIGN.md](../../DESIGN.md)
和 ROADMAP M28。

#### M28 实际判定（2026-07-26）

结论为 **`keep_native_surface_and_delete_legacy`**。

- surface contract 11/11 有唯一 opener、state source、container、exit action 和 deletion
  point；production container 仅 3 种加 inline approval；
- English/`zh-Hans` 在五个冻结尺寸的真实 PTY resize 为 10/10，长路径、CJK、
  combining character、emoji、composer focus 与 cursor 均保持；
- keyboard/mouse/paste、onboarding/slash/mention、permission、approval
  denial/no-side-effect 与 active resize parity 全通过；没有 mouse-only action；
- English/`zh-Hans` live terminal projection 与同一 RunStore credential-free reopen 的
  尺寸、非空 glyph/坐标、前后景、bold/italic/underline/inverse 与 cursor 逐项相等；
  canonical Store terminal/evidence 仍精确；
- idle 初帧后 5 秒没有 PTY bytes；ANSI-16/256/truecolor 与 terminal-reset background
  的 semantic token gate 通过；
- `en`/`zh-Hans` catalog key/placeholder parity 通过，可达人类文本改由 localization
  owner 提供；
- production legacy renderer、centered generic modal、显示模式 reader、第二 theme
  owner 和孤儿 fixture 均为 0；
- M28 diff 不触及 protocol/runtime/state/deepseek/tools 的 production owner，permission
  与 completion semantics delta 为 0。

最终 parity closure 为 `068c8e3a9`，遗留 modal 词汇清理 checkpoint 为
`60df6f8f7`，PTY resize 观测竞态 closure 为 `07214e2ef`。focused、fmt、strict
workspace Clippy、workspace test、public checker、locked/offline
delivery install/verify/uninstall 与 `git diff --check` 全绿。一个 exec stall fixture 在
全套并发负载下暴露 transport/open guard 与 model-event guard 同为 1 秒的自相干扰；测试
已把前者放宽到 5 秒而保持后者 1 秒，production 行为未变。无 Key、official API、GitHub、
push 或 release。完整审计见
[M28 summary](../../eval/summaries/m28-native-tui-surface-cutover-2026-07-26.md)。

### M29 local release-candidate workflow acceptance

M29 是 credential-free、workflow-level local release regression，不是模型 treatment 或
新 Harness。baseline 为 M28 clean checkpoint `548afbe3e`。它只接受 canonical
`AgentApplication -> AgentRuntime -> RunStore`、现有 TUI/CLI/app-server caller、loopback
ChatCompletions、真实临时 Git repository 和 deterministic external verifier 的证据。

#### Frozen cases

| case | required production evidence | veto |
|---|---|---|
| long root task | >=2 model turns、workspace mutation、Host verifier、1 terminal | 丢 input/event、未验证成功、重复 terminal |
| verifier recovery | fail receipt、rejection、fresh correction、latest-revision pass | 旧 receipt 接管、false success |
| approval deny | pending exact interaction、deny、0 start/request/side effect | deny 后执行 |
| approval approve/reopen | frozen digest/revision、1 start、1 outcome、1 side effect | 重复 prompt/执行或 stale authorization |
| read-only child | fixed Flash/high、read-only catalog、handoff、Pro/high root completion | child 写入或替代 root |
| explicit Writer | isolated worktree、seal/verify/integrate/cleanup each once | root direct write、重复/漏 cleanup |
| Store/process reopen | same run/event prefix/plan/accounting/evidence/terminal | resend、重复副作用、第二终态 |
| terminal workflow | en/zh-Hans typing/paste/resize/mouse/approval/resume | required action 不可达或 projection 漂移 |

每个 case 必须冻结 task/acceptance/verifier、actor/workspace/permission/route、
attempt/request/accounting、tool lifecycle、workspace revision、evidence receipt、terminal
和 reopen equality。`maximum_reruns=0`；确定性门禁可从全新独立 temp identity 重复执行，
但失败 execution 不得被选择性替换或隐藏。

#### Defect admission

production fix 只在下列任一条件满足时准入：

- 同一 stable owner/cause 在两个独立 workflow execution 重复；
- 一个 deterministic fault injection 精确违反 false-success、latest-revision evidence、
  permission/deny、Writer isolation、exactly-once side effect 或 crash/reopen 不变量。

单次不可复现噪声、测试采样竞态、主观 UI 判断和分层 unit pass 都不是 production defect。
现有测试已完整覆盖的行为必须复用真实 owner，禁止复制工具、Store、projection 或 failure
分类。临时观测代码没有独立 production 消费者，结论前必须删除。

#### Gates and decision

机制门要求：

1. 8/8 workflow 的 external acceptance、Host terminal 与 Store replay 一致；
2. `false_success=0`，denied/unapproved/stale invocation 的 side effect 为 0；
3. root/read-only child/Writer/recheck 路由与三档 permission 无变化；
4. crash/reopen 的 event prefix、RequestPlan、accounting、evidence、request/side-effect/
   terminal count 精确；
5. CLI/TUI/app-server、English/`zh-Hans`、keyboard/mouse parity；
6. targeted workflow/PTY/reopen/Writer、focused、fmt、strict workspace Clippy、workspace
   test、locked/offline delivery lifecycle、public checker、diff check 全部通过。

最终只能记录：

```text
keep_current_workflow_no_reproducible_blocker
keep_minimal_attributable_workflow_fix_and_delete_old_path
blocked_by_reproducible_local_release_workflow_defect
```

M29 的 false-success/verification 只属于 deterministic loopback fixture，不外推官方
DeepSeek coding quality。Key/API/network/GitHub/push/release 必须为 0。

#### M29 result

决定为 `keep_current_workflow_no_reproducible_blocker`。

- 8/8 workflow 的 canonical execution、Host evidence、terminal 与 Store replay 通过；
- Runtime conformance 85/85、Writer 25/25、真实 Git orchestrator 44/44、State process
  crash 38/38、app-server process 3/3；
- canonical PTY 7/7、Run acceptance 18/18、双语/mouse/paste/resize QA 15/15、
  release runtime 5/5、CLI/HTTP/stdio parity 2/2；
- fixed actor route、三档 permission、latest-revision completion、Writer isolation 与
  exactly-once request/side-effect/terminal 没有变化；
- focused、fmt、strict workspace Clippy、workspace test、public checker、delivery
  self-test 和 current exact-source locked/offline install lifecycle 全部通过；
- 没有可重复、可归因 defect，因此 production delta=0，未创建 treatment、probe、
  parallel Harness 或兼容路径。

Key、official API、external network、GitHub、push、release 均为 0。结论不外推
official DeepSeek quality/Token/cache/cost/wall-time。完整证据见
[M29 summary](../../eval/summaries/m29-local-release-workflow-2026-07-27.md)。

### M30 current dogfood loss acquisition

M30 不是 M23 continuation，也不是 M29 机制结果的质量外推。它从 clean
`1b7f92a97` 冻结一个新的 current control-only loss acquisition：

| fact | frozen value |
|---|---|
| task material | hash-bound inheritance of M23 20-task fixture/contracts |
| schedule | one position-1 arm per task；20 arms；no mate |
| production identity | current Run API v13 / RuntimeEvent v20 / State v26 / exec stream v4 |
| model | root/Writer `deepseek-v4-pro` + high；ordinary read-only actor profile Flash/high |
| permission | headless Agent；three continuity cases Ask + typed user-input reopen |
| verifier | per-task external deterministic verifier + latest-revision Host receipt |
| retry | `maximum_reruns=0`；transport/runtime retry 0 |
| cost ceiling | `$0.50/arm`、`$10.00/suite` |
| historical evidence | manifest facts only；historical raw/admission/journal excluded |

Offline acceptance:

1. inherited manifest/file/tree/reference hashes and all 20 fail-before/pass-after or safety
   counterexamples reproduce;
2. current Run API permission envelope, fixed actor routes and task/lane scopes are exact;
3. three continuity tasks commit one typed user-input interaction, SIGKILL, reopen and answer once
   with no additional physical model request or side effect at reopen;
4. behavior/accounting truth remain orthogonal and deterministic;
5. journal remains ignored 0600, exclusive, fsynced and hash-chained;
6. self-test, dry-run, production loopback, focused/full repository gates and exact-source identity
   all pass without Key/API.

Credential-front is not admitted by this contract. Before it can exist, the immutable binary,
manifest/Harness/schedule/task hashes, authority hashes, raw path, official 2026-07-27 DeepSeek
surface/price review, current explicit authorization and `$10` suite ceiling must all be frozen.
Any false success, identity mismatch, observer ambiguity, unsafe evidence, unknown billing,
incomplete usage, environment ambiguity or ceiling breach stops before the next arm. No rerun or
mate completion is allowed.

Candidate attribution requires one identical stable `owner_code:loss_code` across at least two
distinct task IDs. One task, raw tool frequency, expected safety rejection, accounting stop or
unexecuted arm cannot authorize production work. Valid decisions are:

```text
next_candidate_audit_required
insufficient_repeated_current_loss
stop_incomplete_accounting
stop_invalid_identity_or_observation
```

`next_candidate_audit_required` only opens one minimum owner audit; it is not automatic
implementation. Until a repeated loss exists, production delta and product metric claim are both
zero.

#### M30 offline mechanism result

`--campaign m30` 已在同一个 corrected Harness 中闭合：

- task/fixture/reference inheritance hash 有效；20 个独立 Git workspace 可重复物化；
- initial verifier 20/20 按预期失败，17 个正向 reference 17/17 通过，3 个 safety
  counterexample 3/3 保持失败；
- schedule 为 20×1，`runs_per_task=1`、`maximum_reruns=0`，不生成 pass³；
- current controls 只含 `write_execution_mode/permission_mode/interactive`；root/Writer/
  read-only lane profile 与 current product 一致；
- 三个 continuity task 使用 runtime-owned `request_user_input` 建立 durable checkpoint；
  真实进程 SIGKILL/reopen 保持 event prefix 与 physical request `1 -> 1`，回答后恰好一次
  workspace side effect，最终 3 个 loopback requests、一个 terminal，terminal reopen
  exact；
- journal 四个 crash window、behavior/accounting truth、Hardness metric observer、M23B
  historical self-test 与 M9C self-test 均通过；
- offline 阶段 Key/API/model network 为 0。

冻结的 schedule hash 为
`sha256:621efff3ea6f5cc2ea34dc946145044f754dc87b5bbe51c79af2d5477a7e72a1`，
task-contract hash 为
`sha256:7d71a8597adc1d628987e6eb1ee588c48c150f228322aec05658c663cd135c99`。
这仍不是 current DeepSeek loss 或质量结论；live credential-front 尚未准入。

#### M30 live result 与候选门

live admission `2761f1b8d` 在 clean tree 上绑定 candidate `f74804a08`、immutable
release binary、当前 Harness/schedule/task hash、ignored 0600 raw path、显式授权和
`$10` suite ceiling。formal acquisition 在 12 个完整 arm 后，于第 13 个
`writer_policy_migration` 保存 terminal/Store/reopen/verifier facts，再因
`billing_unknown=true` 写入 `accounting_incomplete` abort；没有重跑或执行后七个任务。

只读 canonical report 的输入为 86-record、无 partial tail、SHA-256
`c1c1705d2f5546a2d1428f90a4925939fde6d597dbcd12755c4aa5d1b317e4fd` 的 journal。两次
report byte-identical，SHA-256 均为
`d68e101dc0ee184ac034df6066c67c3f6bb8818ceea676a78575d3e15526175f`：

```text
behavior status
  verified_success             9
  correct_safety_rejection     1
  verified_product_failure     2
  invalid                      1
  false_success                0
accounting status
  complete                    12
  billing_unknown              1
full_utility_observations      12
```

第 13 trajectory 的 `route_identity_mismatch` 与 billing unknown 都排除在候选归因外。
两个独立且 accounting-complete 的 trajectory：

- `rust_line_recovery_resume`
- `rust_netstring_recovery_resume`

均得到 stable
`host_completion:verified_workspace_without_terminal_receipt`。因此 valid decision 同时
保留 acquisition stop `stop_incomplete_accounting`，并把 loss gate 提升为
`next_candidate_audit_required`。partial 12-arm 数值不能变成 20-task aggregate，但两个
闭合 loss observation 可以且只可以打开一个 unique-owner audit。

候选冻结为 `crates/runtime` named-verifier ACI。正式 keep gate：

1. model-visible identity 不再把 TaskContract acceptance 与 tool name 混为同一语义；
2. Host 仍从 frozen TaskContract 展开 exact verifier parameters，模型不能覆盖；
3. 两个原 loss task 各一个 fresh fixed-Pro/high treatment、`maximum_reruns=0`；
4. 两者都必须提交 fail→effective mutation→latest pass `EvidenceReceipt`，external
   verifier 通过、false success=0、accounting/reopen 完整；
5. root/read-only/Writer、authorization、Store exact replay、SIGKILL/reopen 与全仓门禁
   不回退；
6. 任一门失败即 `reject_and_delete`，不得通过弱化 `failed_write_pass`、导入 external
   failure、兼容别名或第二 verifier path 保留候选。

完整事实见
[M30 current dogfood loss acquisition](../../eval/summaries/m30-dogfood-loss-acquisition-2026-07-27.md)。

#### M30 named-verifier ACI treatment 结果

candidate `d0d509b6e` 保持 `verifier_id` schema、exact resolver、Host-expanded frozen
parameters、RuntimeEvent 与 State/Store replay 不变，只澄清模型可见说明：
`verifier_id` 是 TaskContract acceptance ID，不是 `run_verifiers` 工具名。两项原 loss
task 使用同一 fixed Pro/high、fixture、TaskContract、external verifier、Ask
continuity、immutable binary 和 `maximum_reruns=0`；总费用上限从 acquisition 的 `$10`
进一步缩为 `$1`。

正式结果：

| gate | result |
|---|---|
| complete arms | 2 / 2 |
| verified success | 0 / 2 |
| verified product failure | 2 / 2 |
| false success | 0 |
| external verifier pass | 2 / 2 |
| route / lane valid | 2 / 2 |
| SIGKILL continuity + SQLite reopen | 2 / 2 |
| accounting complete | 2 / 2 |
| physical requests | 35 |
| input / output tokens | 684,104 / 25,322 |
| cache hit / miss | 598,528 / 85,576 |
| known cost | USD 0.061425364 |
| decision | `reject_and_delete_named_verifier_aci` |

结构化 event 审计证明 treatment 产生真实、但不足以完成任务的 surface delta：18/18 次
named-verifier 调用都逐字使用 frozen acceptance ID，未再把工具名当参数；随后 18/18
次均由 existing Host permission policy 在 operation 启动前以 typed
`invocation_rejected` fail-closed，原因是执行后端不能证明一次性 external-path
authority。最终 workspace 仍由其他编辑路径修好且 external verifier 通过，但缺少
canonical fail→mutation→pass receipt，所以 Stop Gate 的
`verified_workspace_without_terminal_receipt` 拒绝正确。

这不是 permission treatment 的成功或失败结论：M30 只预注册了 ACI 单变量，不能在观察
到第二 owner 后改用 FullAccess、增加 approval 特例、放宽 external path、导入 Host
external failure 或建立第二 verifier path。ACI production 变更和临时 `m30t` Harness
consumer 已删除；原 named-verifier、permission、Stop Gate、Runtime/Store 保持不变。
frozen manifest/admission、ignored 0600 raw 与新 summary 只作审计证据。

raw 为 21-record、无 partial tail、0600 hash-chain journal，SHA-256
`9453bed41dbe09c7f19ae532eacf75d2a9a99aaa8c7483c86fa8845bc7094b13`；
末记录为 summary。完整决定见
[M30 named-verifier ACI treatment](../../eval/summaries/m30-named-verifier-aci-treatment-2026-07-27.md)。

#### M31 contract-bound verifier permission contract

M31 只接受 M30 已冻结的两个 independent、accounting-complete loss fact，不重写或重跑
历史 raw。重复 defect 是：exact acceptance-ID handle 已由 Runtime 展开为冻结
`run_verifiers` 参数，但 Ask 的 generic external-path classifier 在 operation 前拒绝
外部 executable program；同一 Run 的终态 Host verifier 随后在同一 workspace-write、
no-network sandbox 中成功执行相同 spec。

offline matrix 固定为 16 个 permission/scope case 和 4 个 SIGKILL/reopen window：

- Ask interactive/headless、Agent、FullAccess 和 explicit Writer 的 exact contract
  verifier 可由 typed Host grant 启动；
- read-only child 仍没有 mutating verifier catalog；
- 相同 raw 参数但没有 grant、错误 digest、spec drift、external cwd、普通 external
  read/edit/shell、network、explicit deny 和 hard invariant 全部 fail closed；
- prepared 可在 reopen 后只授权/执行一次，authorization committed 不重新判定，
  started 无 outcome 不盲重放，committed outcome 不重复执行或签发 evidence；
- Store 必须从 frozen TaskContract、prepared abbreviated invocation 与 grant 重建同一
  exact invocation；workspace revision 或 grant identity 不一致视为 corrupt/stale，
  不能 fallback。

grant 不是第四个 permission 档位、用户 approval、通用外部路径能力或第二 verifier
协议。tools 必须重新解析 canonical `VerifierSpec` 并只豁免 exact verifier 的
`commands[].program`；现有 OS sandbox 继续限制写根和 network。Run API/RuntimeEvent/
State 如因 durable shape 改变而升级，旧 materialized run 必须 fail-closed retirement，
只允许证明仍符合新 Start contract 的 pending intent 留存，不增加 compatibility reader。

offline 全绿后仍不能读取 Key。live treatment 需要新的 immutable candidate/admission、
当前明确授权和 `$0.50/arm`、`$1.00/suite` 上界；两项原 fixed-Pro/high continuity task
各只执行一次，`maximum_reruns=0`。唯一 keep 结果要求 2/2 verified success、
false success=0、latest-revision failed-write-pass receipt、exact SQLite reopen 与完整
accounting。其他结果一律 `reject_and_delete_contract_verifier_grant`，不得以 FullAccess、
弱化 Stop Gate、导入 external failure、raw verifier 参数或第二执行路径补救。冻结合同见
`eval/manifests/m31-contract-verifier-permission-v1.json`。

offline candidate `dbfbb8a58` 已通过冻结的 16-case matrix 与四个 tool SIGKILL/reopen
窗口。Ask interactive/headless 的真实 production loopback 都得到两个 exact grant、
零 approval、failed→write→pass receipt 和 SQLite byte-equivalent replay；ordinary raw
调用、wrong digest、spec drift 继续 typed deny。root/read-only/isolated Writer、
CLI/TUI/app-server、双语 PTY、focused、fmt、strict Clippy、workspace test、public
checker 与 diff check 全绿。当前协议 identity 为 Run API v14 / RuntimeEvent v21 /
State v27；Key/API 调用仍为 0。该结果只准入新的两任务 live admission，不是 keep
结论。

正式 acquisition 使用独立 commit `b2d0c21fd` 的 admission、immutable
`dbfbb8a58899817a1ac7f7d896808fbea68c66bb` binary 和冻结顺序，只执行两条
fixed-Pro/high Ask continuity arm，`maximum_reruns=0`。结果为：

```text
verified success             2 / 2
false success                0
latest-revision receipt      2 / 2
continuity + SQLite reopen   2 / 2
additional approvals         0
accounting complete          2 / 2
physical requests            17
input / output tokens        254,746 / 14,443
known cost                   USD 0.030561824
decision                     keep_contract_verifier_grant
```

因此 typed exact grant 接管 production；ordinary external path、network、cwd、explicit
deny、hard invariant 与 sandbox 仍由原 policy fail closed。临时 M31 Harness consumer
在决定后物理删除，frozen manifest/admission、ignored `0600` raw 与 summary 保留。
raw SHA-256 为
`efcfe2846952d13416910a9980b7b37a88e7ec1480587db69f1960fb1b0c5570`，21 records，
无 partial tail。完整结论见
[M31 contract-bound verifier permission treatment](../../eval/summaries/m31-contract-verifier-permission-2026-07-27.md)。

#### M32 current Hardness regression contract

M32 从 clean checkpoint `bfa8ec84f` 建立一个新的 current control-only acquisition，
只通过 SHA-256 继承
`eval/manifests/m23b-hardness-control-v1.json` 的 fixture、reference patches、tasks
和 tool policies。M23/M30/M31 的 binary、raw、admission、停止位置和历史 label 都不是
输入。冻结矩阵如下：

| fact | M32 value |
|---|---|
| task set | 20 independent Hardness tasks；17 positive + 3 safety |
| schedule | one new position-1 arm per task；20 total |
| identity | Run API v14 / RuntimeEvent v21 / State v27 / exec-stream v4 |
| route | current fixed actor profiles；root/Writer Pro-high |
| continuity | 3 long-horizon tasks；Ask + user-input + SIGKILL/reopen |
| retry | transport 0；runtime 0；`maximum_reruns=0` |
| verifier | frozen TaskContract verifier + external deterministic verifier |
| ceiling | `$0.50/arm`；`$10.00/suite` |
| raw | ignored `0600` exclusive hash-chain journal |

Credential-free gates must prove task/reference hashes, fail-before/pass-after and safety
counterexamples, current permission envelopes, root/read-only/Writer conformance, journal
before/mid/unfsynced/after-write windows, behavior/accounting orthogonality, exact Store reopen,
and a real process continuity restart with zero physical request or side-effect increase at reopen.
The immutable release binary, dry-run, focused/full repository gates and exact source identity must
also close before live admission.

Live acquisition requires a separately committed admission with exact current authorization and
cost ceilings. The Harness stops before the next arm on false success, incomplete accounting,
unknown billing, identity/observer/environment ambiguity, unsafe evidence or ceiling breach; it
never reruns or completes a mate. A complete 20-arm result is regression-safe only when
false-success is zero, all closed behavior and accounting facts are valid, and route/lane,
latest-revision evidence and reopen facts are exact.

Candidate attribution requires one identical stable `owner_code:loss_code` across at least two
different task IDs. A single loss, tool frequency, expected safety rejection, accounting stop,
historical raw or unexecuted task cannot authorize production work. With no repeated loss the
decision is `no_repeated_current_loss`; with a repeated loss the only permitted successor is one
minimum unique-owner audit. M32 acquisition itself changes no Runtime, Store, Provider, route,
prompt or tool catalog.

#### M32 live result 与正交结论

live admission `7b8e65b71` 绑定 candidate `994e36d7b`、immutable release binary、
当前 Harness/schedule/task hash、ignored 0600 raw、用户明确授权和 `$10` suite ceiling。
formal acquisition 闭合前 6 个 arm 后，在第 7 个 `writer_envelope` 保存 terminal、
Store、credential-free SQLite reopen 与 verifier snapshot，再因
`complete=false, usage_complete=true, billing_unknown=true` 写入
`accounting_incomplete` abort；没有重跑或执行后十三个任务。

前 6 个 complete observation 为 5 个 verified success、1 个 verified product failure、
false success 0，已知费用 `USD 0.075418502`。这些 partial 数值不形成 20-task pass@1、
safety、成本或 release-quality aggregate。

只读 canonical report 的输入为 47-record、无 partial tail、SHA-256
`4f7ed8b09122ab613b31e2b1b1b4b108df85df1f69a257aec2c478f57b914521`
的 journal。两次 report byte-identical，SHA-256 均为
`b6e780949dd9d88c233e6bfee8a07a8f003f065986d63d75012c4f90bbada6d8`：

```text
canonical trajectories        7
completed arm results         6
behavior:
  verified success            5
  verified product failure    1
  invalid                     1
  false success               0
accounting:
  complete                    6
  billing unknown             1
full utility observations     6
```

唯一 accounting-complete loss 为单个
`root_task_outcome:deterministic_verifier_failed`；没有第二个独立 task，因此结果是
`insufficient_repeated_current_loss`，不授权 candidate audit 或 production treatment。
由于 acquisition 未完整闭合，也不能声明 `no_repeated_current_loss` 或 M31 在所有 strata
上 broad regression-safe。

M32 最终为 `stop_incomplete_accounting`。临时 `--campaign m32` consumer 和 gate-only
adapter 已删除，Harness 恢复到 M32 前 blob `90ffb72b`；production 不变。完整身份、
partial metric、删除和非结论见
[M32 current Hardness regression](../../eval/summaries/m32-hardness-regression-2026-07-27.md)。

#### M33 Runtime-owned model retry result

M33 是独立 reliability/correctness treatment，不是 M32 formal campaign 续跑，也不以
`maximum_reruns=0` 覆盖正常产品默认。control 是 M32 clean checkpoint `e5df72e78`：

- `RunLimits.max_model_retries=2` 已允许初次请求后最多两次 Runtime retry；
- DeepSeek transport 仍有 retry loop/config 类型，但 production
  `DeepSeekModelPort` 总是调用 `with_retries_disabled()`；
- `[retry]`、app 默认 3、`--transport-max-retries` 与 fingerprint 因而不是实际
  Runtime 行为；
- `ModelRequestFailed` 已原子保存 retry decision/prepared attempt，但没有 backoff 或
  not-before，Runtime 会立即继续。

候选只允许一个 delta：让既有 Runtime retry decision 带 durable
decision-time/backoff/not-before，删除 transport controller 与失效配置，并把
DeepSeek 实际返回的 `Retry-After` 作为 typed hint 传给 Runtime。固定本地 backoff 为
1s、2s；hint 只能延长，不能绕过 replay-safe、actionable-output、limit、budget 或
deadline gate。

离线 admission 必须完成 ROADMAP 31.3 的 11 项矩阵，并满足：

```text
safe pre-header recovery                  pass
partial/actionable output resend          0
prepared-retry reopen duplicate send      0
in-flight-retry reopen blind send         0
false success / false progress            0 / 0
transport hidden retries                  0
root / read-only / Writer conformance     pass
CLI / TUI / app-server parity             pass
en / zh-Hans retry projection             pass
physical/accounting/reopen exactness       pass
```

`Retry-After` 不是官方 DeepSeek 当前公开保证；只测试实际 header 的 typed 解析和 Host
等待。官方 2026-07-27 error/rate-limit 文档支持 429 与 500/503 的有界等待重试，
Chat 文档支持以 `[DONE]` 结束的 SSE，但没有 partial response continuation contract。

最终只允许：

```text
keep_runtime_owned_model_retry_loop
reject_and_delete_retry_candidate
```

keep 必须物理删除 transport loop、失效 config/CLI/fingerprint 和旧双控制器测试；
reject 必须删除 backoff treatment，保留当前 actionable-output/in-flight fail-closed
语义。两种结果都不得修改 M32 frozen manifest/result/raw、访问 GitHub、push 或
release；credentialed canary 必须在离线门禁和新的明确授权之后。

结果为 `keep_runtime_owned_model_retry_loop`。冻结矩阵全部闭合：

```text
safe pre-header recovery                  pass
partial/actionable output resend          0
prepared-retry reopen duplicate send      0
in-flight-retry reopen blind send         0
false success / false progress            0 / 0
transport hidden retries                  0
root / read-only / Writer conformance     pass
CLI / TUI / app-server parity             pass
en / zh-Hans retry projection             pass
physical/accounting/reopen exactness       pass
```

覆盖 timeout→1s→success、network→1s/2s→success、429+Retry-After、可重试
500/503、不可重试 401/403/普通 4xx、partial content/reasoning/tool/usage/finish、
prepared retry SIGKILL/reopen、in-flight SIGKILL、limit/budget/deadline 与三个 actor。
workspace strict Clippy/test、focused、fmt、process crash/reopen、surface parity、
双语 PTY、public checker 与 diff check 全绿。

离线 admission 后用户明确授权 official canary。一个 fixed Pro/high Standard Chat
request 成功：`M33_LIVE_OK`，physical=`1/1/0`、runtime retry=0、usage/cost
complete、billing unknown=0、input/output=`2811/38`、cost
USD `0.000979765`、duration `1611ms`。它只证明成功路径与 accounting；真实 retry
故障没有被人为诱发，replay safety 仍以 deterministic loopback/process evidence 为
权威。完整身份、删除与研究依据见
[M33 summary](../../eval/summaries/m33-runtime-owned-model-retry-2026-07-27.md)。

#### M34 model failure feedback contract

M34 的 control 是 M33 clean checkpoint `408611fb1`。它不再证明 retry algorithm，
而是冻结 production client 能否把同一 canonical retry/stop fact 准确、持续、双语地
交给用户。故障只由 test-only loopback proxy 注入；production sender、Runtime、
RunStore、permission、prompt、model 和 backoff 均不改变。

| profile | upstream observation | expected Runtime decision | required client fact |
|---|---|---|---|
| `timeout_then_success` | response headers 前 timeout | retry 1/2，1s 后成功 | 类别、1/2、等待、成功后清除失败 |
| `reset_reset_success` | 两次 pre-header reset | retry 1/2、2/2 后成功 | 每次 ordinal/delay，恰好 3 physical |
| `rate_limit_then_success` | 429 + `Retry-After: 2` | 等待不短于 2s 后成功 | 429/限流、1/2、2s |
| `service_unavailable_exhausted` | 连续 503 | 两次 retry 后 Stop | terminal 后原因、耗尽和下一 canonical action |
| `unauthorized_no_retry` | 401 | Stop(NotRetryable) | 永久错误、0 retry、修正凭据/重新发起 |
| `partial_content_close` | content delta 后连接关闭 | Stop(ActionableOutput/UnsafeReplay) | 不重发、说明不能安全自动重试 |
| `prepared_retry_reopen` | decision committed、send 前 crash | reopen 等剩余 not-before，只发一次 | same-Run 恢复，不声称流续传 |
| `in_flight_reopen` | request in flight 时 crash | `RecoveryRequired`、零盲发 | 上游结果未知、明确 fail closed |

所有 profile 同时断言：

```text
server physical attempts == Runtime accounting physical attempts
stored retry events == visible retry ordinal sequence
terminal stop reason survives terminal projection and event reconnect
en placeholders == zh-Hans placeholders
false progress == 0
false success == 0
```

information rubric 的七个原子事实为 provider、failure category、automatic action、
retry ordinal/total、wait、stop safety reason、next canonical action。每个事实只在适用
profile 计分；不得用一条泛化“失败，请重试”冒充通过。相同缺失必须跨两个独立 profile
重复才允许 production treatment。

候选只能修改既有 localization/TUI/CLI projection。RuntimeEvent 与 State 已有 retry
decision/stop reason，不允许为了展示再持久化第二份状态。若 plain `dse exec` 需要进度，
沿用成熟 CLI 的 stderr progress / stdout final-result 分离；machine JSON/JSONL 保持
canonical schema，不混入人类文案。terminal 只建议既有操作，不新增 modal、resume
protocol 或模型请求。

keep gate：

```text
applicable information rubric              100%
retry success returns to normal progress    pass
terminal cause remains available            pass
physical/retry/accounting/reopen exact       pass
partial/in-flight duplicate request          0
false progress / false success               0 / 0
CLI / TUI / app-server semantic parity       pass
en / zh-Hans placeholder parity              pass
new retry controller / setting / state truth 0
```

结果只允许 `keep_minimal_model_failure_feedback`、
`keep_existing_surface_no_repeated_loss` 或
`reject_and_delete_feedback_candidate`。完整 contract 与 fault truth 固定在
`eval/manifests/m34-model-failure-feedback-v1.json` 和
`eval/fixtures/m34-model-failure-feedback-v1.json`。

#### M34 model failure feedback result

结果为 `keep_minimal_model_failure_feedback`。control 在 429、503、partial SSE 和 TUI
terminal 等独立 profile 重复缺失 retry ordinal/wait 或 durable stop reason，因此满足
预注册 treatment admission。相同 frozen profiles 的 treatment 结果：

```text
applicable information rubric              100%
timeout/reset/429 recovered                 3 / 3
503/401/partial stopped safely              3 / 3
prepared/in-flight reopen exact             2 / 2
server attempts == physical accounting      pass
partial/in-flight duplicate request         0
false progress / false success              0 / 0
exec terminal                               30 / 30
en / zh-Hans PTY                            16 / 16
new retry controller / setting / state truth 0
```

exec-stream v6 的 bounded `model_request_failed` 只投影 stored event identity、typed
failure、retry/stop 与紧凑 accounting；第一次会复制完整 request/prompt/tools 的候选已
删除。plain exec 的 transient progress 使用 stderr，TUI narrow tier 只放行 typed
Warning/Error；最终 stop reason 与 next action 留在 history。app-server 仍原样投影
canonical stored event，RuntimeEvent v22/State v28/M33 retry policy 不变。

credential read=false、official API request=0；本结论不外推官方服务故障频率或 jitter
收益。完整 control/treatment、删除与非结论见
[M34 summary](../../eval/summaries/m34-model-failure-feedback-2026-07-27.md)。

#### M35 official production reliability soak contract

M35 不把 deterministic fault injection 重做成官方故障攻击，也不把普通成功 canary冒充
恢复率。它用 current M34 clean checkpoint `39da745f5` 的同一个 immutable binary，
冻结 4 个 product profile × 6 个独立轮次：

```text
plain Chat marker                 6
read_file tool loop              6
grep_files -> read_file loop     6
bounded read_file -> edit_file   6
logical Runs total              24
```

每个 Run 使用新 Git repo、HOME、State 和 RunStore；模型/effort 固定
`deepseek-v4-pro/high`，official endpoint 固定
`https://api.deepseek.com/chat/completions`。正常产品的初次请求加最多两次 Runtime
safe retry 保持开启；Harness logical rerun 固定为 0。transport hidden retry 必须为 0。

每条 observation 同时闭合：

```text
behavior terminal + external verifier + latest workspace
physical started/completed/in-flight + Runtime retry
typed failure/actionable evidence + retry/stop decision
usage/cache/cost/billing status
exec-stream failure/terminal projection
credential-free SQLite reopen/event-prefix equality
```

`billing_unknown`、usage incomplete、identity/evidence ambiguity、unsafe partial、
observer ambiguity或未封存 in-flight request 都在下一付费 Run 前停止。它们的闭合行为
事实可按 ADR-0011 保留，但不能进入费用/效率 aggregate，也不能选择性补跑。

production treatment 的准入门是同一个稳定 `owner_code:loss_code` 在至少两个不同 task
profile 或两个独立轮次重复。没有重复损失则
`no_repeated_live_reliability_loss`，production delta=0；有重复损失也只能审计一个
unique-owner 最小候选，并从全新 position-1 successor 复测。任何 blind partial replay、
第二 retry controller、false success、reopen/accounting 回退或复杂度双轨都要求
`reject_and_delete`。完整 schedule、官方/外部依据和预算见
[`m35-official-reliability-soak-v1.json`](../../eval/manifests/m35-official-reliability-soak-v1.json)。

正式结果为 `no_repeated_live_reliability_loss`。第一次 v1 只暴露 Harness argv observer
缺陷并在 official request 前停止；immutable journal 不续写。修正后 candidate
`6202bb0d7` 从新 output/position 1 完成 24/24 verified，false success 0，
physical started/completed/in-flight=`54/54/0`，model failure/Runtime retry=`0/0`，
usage/cost complete 24/24，billing unknown 0，credential-free SQLite reopen 24/24
exact。TTFR median/p95 为 2,020/2,380ms，wall median/p95 为 4,796/7,428ms，已知费用
`$0.014071409`。工具任务的多 physical request 是 canonical tool loop，不计作 retry。

因此 M35 不改 production；M33/M34 的 deterministic failure/replay matrix 仍是 retry
safety 主证据。没有用自然运行未遇到故障来调整次数、jitter 或 circuit breaker。临时
M35 Harness consumer 已删除并恢复 acquisition 前 exact blob；frozen manifest、
live admission、ignored `0600` journals 与
[decision summary](../../eval/summaries/m35-official-reliability-soak-2026-07-27.md)
保留审计。

#### M36 DeepSeek-native verified harness 评测合同

M36 的评测对象是 DSE Harness，不是竞品功能数量。任何 treatment 必须先证明一个稳定
production loss，再在相同 DeepSeek 模型下测量；不得把 Codex/Claude Code 整机成绩或
不同模型的排行榜差异归因给 DSE。

##### 0. M36-A fresh acquisition identity

M36-A 只复用 `eval/manifests/m23b-hardness-control-v1.json` 中已经由 reference patch
证明的私有 task material、deterministic verifier 和 tool policy，不复用任何历史
trajectory、raw、result、admission 或 arm label。新的
`eval/manifests/m36a-deepseek-native-baseline-v1.json` 必须同时冻结：

- 20 个按原顺序人工复核的 `m36a-*` acceptance identity；
- 每 arm 新 workspace、DSE home、SQLite RunStore 和 breadth-first position-1 schedule；
- current `Run API v15 / RuntimeEvent v22 / State v28 / exec-stream v6`；
- `deepseek-v4-pro/high`、Standard streaming ChatCompletions、current fixed actor route；
- normal Runtime safe retry 上限 2，Harness rerun 上限 0；
- 六个 task family 每类至少两个独立任务；
- behavior/accounting 正交 truth、exact credential-free reopen、latest-revision receipt；
- `$0.50` per-arm 和 `$10.00` suite known-cost hard ceiling。

离线 acquisition 前必须通过 reference solution proof、hash-chained journal crash
windows、进程 SIGKILL/reopen continuity、11-case canonical loss taxonomy 和扩展 metric
observer。loss observer 只能输出下文稳定 taxonomy；若 task contract 在人工复核中存在
歧义，该 case 必须在 credential 前 invalid，不能靠正式模型失败补标签。

M36-A 已按该 identity 完成 20/20 formal arms：positive verified success 16/17，
correct safety rejection 3/3，false success 0；behavior product observations、
accounting-complete observations 与 full-utility observations 均为 20。known cost
`$0.278444080`，154 个 model requests，Harness rerun 0，Runtime retry 0。

唯一 product loss 为
`writer_envelope -> orchestrator:writer_integration`，只覆盖一个独立 task_id；第二个
Writer、两个 read-only child、三个 long-horizon reopen 和 service/API/UI tasks 均通过。
所以结果是 `keep_current_harness_no_repeated_loss`，不准入 M36-B/C。

frozen raw final summary 的 `complete=false` 是 eval aggregate defect：旧逻辑把 positive
lane success 与 acquisition completeness 绑定。raw 中 20 个 per-arm behavior/accounting
truth 均已闭合。read-only analysis manifest 分别绑定 acquisition/analysis Harness hash，
从同一 132-record ignored `0600` journal 两次生成 byte-identical report，且不读
credential、不访问 network、不修改 raw、不补跑。正式身份、修正与删除证据见
[M36-A summary](../../eval/summaries/m36-a-deepseek-native-baseline-2026-07-27.md)。

##### 0.1 M36-A2 explicit Writer confirmation

M36-A2 是 fresh control-only repeated-loss acquisition，不是 M36-A continuation、历史 raw
重算或 production treatment。唯一 corrected Harness 读取
`eval/manifests/m36a2-writer-loss-confirmation-v1.json`；历史 M36 `writer_envelope` 只提供
待复核假设，**不计入**本次 threshold。

冻结身份必须同时满足：

- exact clean source、tree、immutable `dse` binary、Run API v15、RuntimeEvent v22、State
  v28、exec-stream v6；
- 3 个全新 task_id、fixture tree/base commit、各自 reference patch、hidden deterministic
  verifier、allowed scope 与 one-Writer actor contract；
- current `deepseek-v4-pro/high`、Standard streaming ChatCompletions、current Prompt/tools、
  `permission_mode=agent`、`interactive=false`；
- normal Runtime safe retry 2、Harness rerun 0、每 task 一个 position-1 arm；
- 每 arm 新 Git repository、isolated Writer worktree、DSE home、SQLite RunStore、external
  verifier HOME 和 credential-free terminal reopen；
- `$1.00` per-arm、`$3.00` suite known-cost ceiling；unknown billing/incomplete usage 在下一
  arm 前 fail closed。

离线 fixture 必须证明：

```text
writer_retry_ledger: initial fail, canonical v2 reference pass
writer_header_policy: initial fail, bounded Rust policy reference pass
writer_route_contract: initial fail, exact v1 read/v2 write reference pass
```

每条正式 observation 必须闭合 behavior status、false success、route、完整 Writer lifecycle、
sealed diff、root integration、latest revision receipt、external verifier、cleanup、physical
request/retry/usage/cache/cost 和无 credential reopen。Writer task 的 verified product failure
仅在 identity/environment/workspace outcome 已闭合后归入
`orchestrator:writer_integration`；真实 `deepseek_transport`、Host completion、Harness 或
infrastructure interruption 不得伪归因。

机械决策门：

```text
0 or 1 fresh task with orchestrator:writer_integration
  -> keep_current_harness_no_repeated_loss
  -> production delta = 0
  -> delete temporary M36-A2 campaign consumer

at least 2 distinct fresh task_ids with orchestrator:writer_integration
  -> next_candidate_audit_required
  -> audit only crates/orchestrator bounded verify->repair->reverify
  -> freeze a separate held-out >=3 tasks x >=3 arms/cell A/B before treatment
```

出现 false success、unknown billing、incomplete accounting、identity mismatch、observer
ambiguity、unsafe evidence 或 cost ceiling 时，本 campaign 立即停止且不得 rerun/mate/splice。
即使 repeated-loss 门通过，也不能直接改 production。ADR-0015 的 web/browser/search/vision
方向不属于本评测，production delta 继续为 0。

正式 acquisition 已从 immutable `a0847c5c1c04` binary、position 1 完成 3/3 arms，
maximum reruns=0：2 个 verified success、1 个 measurement-valid product failure、false
success=0。三臂 accounting 均为 `complete`，合计 38 requests、input/output
`196,296 / 19,773` tokens、cache hit/miss `116,864 / 79,432` tokens、known cost
`$0.113726632`、wall time `541,642 ms`。唯一 loss 为
`writer_header_policy -> orchestrator:writer_integration`；另外两个 fresh Writer task 均完成
seal/integrate/latest-revision receipt/cleanup，因此同因只覆盖一个独立 task_id，机械结果为
`keep_current_harness_no_repeated_loss`。没有 candidate、没有 production treatment、没有
后续 A/B。M36-A2 temporary campaign consumer 已从 corrected Harness 删除；frozen raw
SHA-256 为 `5a43e4a0ccb5b5b8bba5a5907ae0affbd65c3c8c00f85354ac37fda9c724d4b6`，
ignored `0600`，8,025,420 bytes。完整身份、单臂事实与删除证据见
[M36-A2 summary](../../eval/summaries/m36-a2-writer-loss-confirmation-2026-07-27.md)。

##### 1. 基线任务层

每个基线 manifest 必须从以下任务层中选择与候选 owner 相关的 fresh、人工复核任务：

```text
deterministic repair
large-repository localization
multi-module hard implementation/refactor
multi-compaction / multi-reopen long horizon
service/API/UI application behavior
false-completion / recovery adversarial
```

任务必须有可执行 acceptance、外部 deterministic verifier、clean workspace seed 和完整
allowed/non-goal scope。静态公共 benchmark 只能作为一层输入；任务 prompt、tests 或 gold
存在歧义时标记 invalid，不用模型失败填补数据集缺陷。

##### 2. loss acquisition

control 只运行 exact current production。每条 observation 在 terminal 后派生：

```text
contract clarity
first relevant file / first correct edit
localization recall and wrong-entry count
tool choice / malformed args / repeated calls / output truncation
active context / stable prefix / raw-output share / compaction
latest revision / receipt / verifier / false completion
physical request / retry / usage / cache / cost / wall time
crash-reopen and terminal replay
```

loss 必须绑定 `owner_code:loss_code` 和具体轨迹证据。同一 loss 未在至少两个独立任务或两个
独立轮次重复时，结论只能是 `keep_current_harness_no_repeated_loss`，不得写 production
treatment。

##### 3. candidate fairness

每个候选独立冻结 manifest，并至少满足：

- 一个 treatment family、一个 owning module、一个 replacement/deletion point；
- control/treatment 在同一 immutable binary 中可显式选择，或分别使用可复核且除 treatment
  外 byte-equivalent 的 immutable binaries；
- task、seed workspace、model、reasoning、system prompt、tool catalog/authority、
  request/Token/deadline budget、verifier 和 observer 完全一致；
- affected task family 至少三个独立任务，每个 cell 至少三次 fresh Run；样本数若因成本或
  deadline 缩小，必须在 credential 前预注册，不能观察结果后改变；
- 不选择性补跑、不补 mate、不拼接旧 raw；`maximum_harness_reruns=0`；
- root、read-only child、explicit Writer 只运行与候选真实 production surface 相符的
  actor，不用 fixture-only 差异冒充 treatment。

##### 4. 决策顺序

所有 candidate 按词典序判定：

```text
1. identity / observer / latest-revision evidence valid
2. false_success == 0
3. correct safety rejection does not regress
4. verified task success
5. hard / long-horizon / full-application completion
6. tool and recovery correctness
7. input/output/cache tokens and API cost
8. wall time
9. production code and concept complexity
```

后项不能补偿前项失败。cache hit、工具调用数、Token 或速度改善不能掩盖 success 回退。
模型 self-review 不进入 deterministic evidence；只有缺少客观 oracle 的主观任务才可使用
独立 reviewer profile，并且其输出仍是 advisory artifact，不获得 terminal authority。

##### 5. effort 与 context 特例

`Pro/high` vs `Pro/max` 必须保持同一 Prompt、工具、ContextBroker 和任务；结论只说明一个
固定 effort 的质量/成本差异，不产生 Auto classifier。context treatment 必须保持当前
tool-call/result 原子性和 DeepSeek `reasoning_content` replay；任何 reset 只能发生在
无 in-flight model/tool 且 Host 已验证 milestone 的 typed continuation 边界。

##### 6. Harness、产品与模型上限分层

结果报告必须分别标注：

```text
harness_comparison_same_deepseek_model
full_product_comparison_mixed_model_and_harness
deepseek_model_ceiling_observation
```

只有第一类可形成 DSE Harness 因果结论。完整产品比较可以说明最终体验，不能说明差异由
Harness、模型、工具或环境中的哪一项造成。

##### 7. 保留与删除

candidate 只有在完整 accounting 下提高 verified success 或目标 hard/long-horizon
completion，且不增加 false success、错误安全放行、crash/reopen 不确定性或第二状态真相时
才可接管。接管后删除被替代路径和 eval-only selector。无净收益、复杂度双轨或质量回退时
完整 `reject_and_delete`；只证明当前 DeepSeek 无法完成时记录
`hold_model_capability_ceiling`，不以更多 Agent、工具或 Prompt 规则掩盖。

## 10. 结果与决策记录

### M37 model-visible contract and Harness control contract

M37 验证“减少 Prompt 债务、强化 Harness owner”是否产生真实正收益。它不把 Prompt 长度、
cache hit 或规则数量当作产品指标，也不允许用单次失败直接创建全局条款。

#### 0. Current identity

每个 M37 manifest 必须冻结：

- exact source revision、tree、immutable binary 和 bundled Constitution SHA；
- complete assembled prompt provenance 与每个 fragment hash；
- DeepSeek model/surface/reasoning、actor、tool catalog/authority；
- task/fixture/workspace/TaskContract/verifier/observer identity；
- request、Token、deadline、retry、permission 和 Harness rerun budget；
- Run API、RuntimeEvent、State、exec-stream 与 credential-free reopen identity；
- behavior/accounting status、known-cost ceiling 和 deletion decision。

current baseline 至少包括：

```text
constitution.md     2,629 bytes
output.md             360 bytes
language.md           311 bytes
bundled core total  3,300 bytes
```

M36-A `writer_envelope` 的 28,494-byte stable system blocks 是一个 current observation，
不是所有仓库的固定上限。M37-A 必须重新对 small/medium/large、rules/skills、有/无
`AGENTS.md` 和三个 actor profile 测量分布。

#### 1. M37-A offline projection audit

M37-A model-visible delta 必须为 0。扩展 ledger 的 fixture 至少验证：

```text
ordered fragment identity and final assembled hash
source / owner / authority / trust / stability
bytes and deterministic token estimate
exact duplicate and same-payload wrapper relation
tool-schema / actor-capability claim parity
stable-prefix boundary
root / read-only child / Writer provenance
SQLite reopen equivalence
override and compatibility source ordering
```

失败 fixture 只能形成 debt report，不能自动改 production bytes。ledger 不进入 RunStore、
不产生第二 prompt truth，不在普通 production 请求增加模型 Token 或持久计算。

M37-A 必须通过：

- `cargo fmt --all -- --check`
- `cargo test -p dse-context --locked`
- owning app/deepseek prompt projection tests
- `./scripts/dev-dse.sh focused`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `git diff --check`

M37-A current 结果（2026-07-27）：

- implementation `5fb2bc971` 只扩展 canonical composer 派生 ledger；普通 production
  prompt 与 audited prompt 逐字段相等，model-visible delta 为 0；
- bundled core 为 `3,300/3,300` bytes；no-AGENTS/rules-heavy/skills-heavy/medium/large
  deterministic assembled 分布分别为 `5,283/5,198/9,225/9,013/18,963` bytes，
  token estimates 分别为 `2,063/1,995/3,802/4,161/9,735`；
- exact duplicate detector 由独立 fixture 覆盖；current no-AGENTS 路径稳定报告
  `project_context -> project_context_pack = same_payload_wrapper`；
- actual actor catalog parity 为 root=`match`、Writer coordinator=`mismatch`
  (`read_only` vs `read_only + isolated_write`)、read-only child=`match`、
  explicit Writer=`tool_unavailable`；
- DeepSeek 单 system-message projection、root/read-only/Writer SQLite reopen、
  exact RequestPlan、显式 constitution override provenance 与 compatibility source
  ordering通过；ledger 不进入 RunStore；
- credential read=false、official API requests=0。决定为
  `keep_current_no_material_loss`，只保留 audit/no-growth 机制和 debt facts；
  M37-B/C 不因本结果自动准入。

完整身份、测试与非结论见
[M37-A summary](../../eval/summaries/m37-a-prompt-projection-audit-2026-07-27.md)。

#### 2. Failure admission

model-visible candidate 前必须给 loss 指定唯一分类：

```text
task_or_eval_defect
model_capability_ceiling
tool_or_aci_contract
context_selection_or_pollution
controller_or_recovery
permission_or_sandbox
verifier_or_completion
provider_transport_or_accounting
stable_semantic_misunderstanding
```

前八类不得通过全局 Prompt 修复。`stable_semantic_misunderstanding` 必须跨至少两个独立
task_id 重复，且没有更窄的确定性 owner，才能新增语义 candidate。

M36-A 的单个 `orchestrator:writer_integration` loss 不满足此门；M37 不修改 Writer
recovery。M36-A2 若证明 repeated loss，也只能先审计 Orchestrator-owned treatment。

#### 3. M37-B posture/schema candidate

control 与 treatment 只允许以下 delta：

```text
control: current execution posture
treatment: remove stale read-only-only agent capability sentence
```

不得同步修改 agent schema/description、Constitution、Runtime、Orchestrator、model、
reasoning 或 task wording。正式矩阵至少覆盖：

- root-only coding；
- read-only child investigation/handoff；
- two independent explicit Writer task families；
- false-completion adversarial case。

每个 cell 至少 3 次 fresh Run，maximum Harness reruns=0。若 treatment 没有 verified
success/tool-selection/Writer completion 的明确收益，即使 Token 更少也不接管。

M37-B current result（2026-07-27）：same-binary candidate `78d8ddb2c` 的 formal
30-arm schedule 只产生 12 个 complete `arm_result`，随后在第 13 个 control arm 后停止。
完整前缀包含 9 个 positive verified success、3 个正确 safety rejection、
`false_success=0`；control/treatment 各 6 个 complete arms，所有已观察 verifier、actor
selection 和 Writer completion 均有效，但每 cell 尚未达到冻结的 3 次，不能判断收益。
第 13 个任务 behavior/verifier 成功，首个 response-before-headers transport attempt 却
留下 `billing_unknown_attempts=1`；usage complete 和后续 durable retry 成功不能证明该
物理 attempt billed/unbilled。Harness 正确写入 `accounting_incomplete` abort，未启动
position 14，也未补 mate 或重跑。

acquisition 决定为 `hold_insufficient_or_incomplete_evidence`；production candidate 因
未通过完整 non-inferiority/benefit 门而 `reject_and_delete_candidate`。临时 selector、
candidate branch 和 Harness consumer 均删除，current posture 保持唯一 production bytes。
完整身份、raw hash、指标与非结论见
[M37-B summary](../../eval/summaries/m37-b-posture-schema-ab-2026-07-27.md)。

#### 4. M37-C context-dedup successor

M37-C 是 fresh evaluation，M10-A frozen raw 只用于历史事实，不进入 aggregate。矩阵至少：

```text
6 affected task families
  x pack-on / single-projection
  x 3 fresh runs
  = 36 arms minimum
```

task families 必须包含：

- 无说明文件的小仓库；
- 无说明文件的多模块仓库；
- README 含 relevant fact；
- README 含 decoy/untrusted instruction；
- search/localization；
- explicit Writer 或 long-horizon recovery。

主要门：

```text
identity valid
false_success == 0
correct safety rejection non-regression
verified success non-regression
hard/Writer/long-horizon non-regression
```

secondary metrics：

```text
stable and cache-miss input bytes/tokens
first relevant file and first correct edit
read/search repetition
model requests
cache tokens
wall time
known API cost
code/concept delta
```

候选只有质量门全部通过且至少一个 secondary metric 有稳定净收益时接管。没有净收益或
出现 treatment-only product loss 时 `reject_and_delete`。

M37-C current result（2026-07-27）：same-binary candidate `241941733` 的 36-arm formal
schedule 在 position 1 终止，complete `arm_result=0`。control 模型已到达冻结的首次
`request_user_input` 连续性 checkpoint，但专属 Harness consumer 仍使用旧的非 M30
approval-only interaction 判定，因此把合法 user-input 事件误判为
`hardness_user_input_not_admitted`。owner-only raw 只有 plan、credential、arm-start、abort
四条 hash-chained record；Key 已读取且至少一个 official request 已发生，但没有闭合
behavior/accounting cell、usage/cost aggregate 或 summary。Harness 没有启动下一 arm、补 mate、
重跑或拼接 M10-A/M37-B raw。

因此 acquisition 为 `hold_insufficient_or_incomplete_evidence`，production candidate 为
`reject_and_delete_candidate`。current pack-on 保持唯一 production bytes，临时 selector、
alternate projection 和 M37-C Harness consumer 已删除。冻结身份、raw hash、删除事实与
非结论见
[M37-C summary](../../eval/summaries/m37-c-context-dedup-ab-2026-07-27.md)。该 Harness stop
不是 Prompt-quality 或 DeepSeek model loss，也不形成 M37-D/E 准入证据。

#### 5. Authority/budget and compact Constitution

M37-D/E 必须各自拥有新的 manifest，不能与 M37-B/C 共用 treatment：

- authority/budget candidate 只修改 fragment classification/projection/budget；
- compact Constitution candidate 只修改 stable core semantics/structure；
- 两者都不得同时改变 model、reasoning、tools、Runtime、Context retrieval 或 verifier；
- compact candidate 的任务语言必须覆盖 English/`zh-Hans`；
- current Chinese production baseline 在 candidate 胜出前保持唯一 production path。

总 budget 不能按任意 Token 数拍板。必须冻结 current distribution、near-limit fixtures、
compaction/reopen 和 DeepSeek effective behavior；active scoped rules、TaskContract、
latest receipt 与 tool-call/result 原子性不得为满足预算被静默丢弃。

#### 6. Decision order

所有 M37 model-visible treatments 按词典序判断：

```text
1. identity / observer / prompt provenance valid
2. false_success == 0
3. correct safety rejection does not regress
4. verified task success does not regress
5. affected hard / Writer / long-horizon family does not regress
6. tool and recovery correctness does not regress
7. input / output / cache tokens and API cost
8. wall time
9. production code and concept complexity
```

后项不能补偿前项失败。unknown billing、usage incomplete、identity drift、observer
ambiguity、task defect 或外部 interruption 按 ADR-0011 停止，不补 mate、不选择性 rerun、
不拼接旧 raw。

允许的最终决定只有：

```text
keep_current_no_material_loss
keep_minimal_candidate_and_delete_replaced_path
reject_and_delete_candidate
hold_insufficient_or_incomplete_evidence
hold_model_capability_ceiling
```

接管后必须删除旧 fragment/renderer/caller、eval-only selector、临时 config/fixture
consumer 和失去消费者的 compatibility path；whole-release rollback 仍是唯一 prompt
rollback owner。

建议结果格式：

```text
eval/
  manifests/
  fixtures/
  tasks/
  results/       # 默认不提交原始大日志
  summaries/     # 提交可复核汇总
```

每个里程碑在 [ROADMAP.md](ROADMAP.md) 中记录：

- 基线；
- 候选版本；
- 结果摘要；
- 保留、重做/缩小、删除/推迟结论；
- 下一步。

### M38 typed interaction and durable observer-abort contract

M38 是 credential-free Harness correctness slice，不是产品 treatment，也不重开 M37-C。
source identity 固定为 `b91d10780bbd714d845dbbf28dada27791509f5a`、tree
`23c8a201d955e3786da135e84c3a0011d51e357e`，production crates、Prompt、model、tools、
permission、Runtime、RunStore 和 Writer behavior delta 必须为 0。

#### 1. Typed interaction matrix

fixture 必须至少覆盖：

```text
root: approval pending/resolved
root: user_input pending/resolved
malformed prompt / incompatible response / multiple pending -> reject
read-only child and explicit Writer: no interaction -> valid
non-interactive child interaction -> reject
SQLite exact reopen -> identical; event drift -> reject
known usage/cost before observer abort -> retained
provider billing_unknown -> remains fail closed
```

approval 与 `request_user_input` 的 admission/response 必须仅由 canonical prompt kind 和
payload compatibility 决定。M9-C、M30 或未来 campaign 名称不得进入此判断。fixture
identity、case count、expected result 和 report hash 都必须冻结；historical raw 不是输入。

#### 2. Durable abort boundary

observer validation error 发生在 durable checkpoint 之后时，journal 顺序必须为：

```text
plan -> observer_abort_snapshot -> abort
```

snapshot 只允许包含 canonical event-prefix hash/count、terminal boundary、physical request
counts、runtime retries、known usage/cost 和 accounting truth；不得保存 raw model content、
reasoning、tool arguments 或 credential。before/mid/unfsynced/after snapshot 的 SIGKILL
fixture 必须证明 hash-chain/partial-tail 可恢复且不会把半条 record 当成事实。修改 snapshot
后必须由 journal hash 验证拒绝。

#### 3. Accounting decision

M38 必须区分：

- `locally_available_accounting_lost`：Harness defect，必须修复并持久化；
- `provider_request_billing_unknown`：官方没有逐物理请求 reconciliation identity 或
  settlement bound 时保持 unknown，不推断为零或已计费。

Chat response/chunk 的 completion id、stream usage、account balance 和 monthly API-key usage
export 均按复核日官方合同解释；account aggregate 不能替代单 request truth。M38 不读取
Key、不调用官方 API、不修改 frozen paid evidence。

#### 4. Keep/delete gate

只有同时满足以下条件才保留：

1. 14-case typed matrix 与四个 crash window 全部通过；
2. M9-C/M30 M38 report byte-identical；
3. M14/M16/M23 observer truth 与 M9-C/M30 self-test 无回退；
4. known usage/cost 在 observer abort 前后相同；
5. billing unknown 没有被弱化；
6. production crate/source delta 为 0；
7. campaign-name interaction 分支和旧错误码已物理删除。

允许结果只有：

```text
keep_typed_interaction_observer_and_durable_abort_snapshot
reject_and_delete_m38_observer_candidate
```

M38 通过不构成 M37-C continuation、Prompt 收益、billing provider 修复或新 production
treatment 准入。

### M39 fresh context/localization loss admission contract

M39-A 是新的 control-only loss acquisition，不是 M37-C continuation 或 pack treatment。
source 从 M36-A2 clean checkpoint `360e52cae` 开始；M37-C raw/result/admission、M36/M32
历史 loss 和 ADR-0015 均不作为 fresh threshold 输入。

#### 1. Frozen identity

[`m39a-context-loss-acquisition-v1.json`](../../eval/manifests/m39a-context-loss-acquisition-v1.json)
必须冻结 exact source/tree、Run API v15、RuntimeEvent v22、State v28、exec-stream v6、
current pack-on Prompt、Pro/high、actor/tool authority、normal Runtime retry=2、Harness
rerun=0、预算、task/material/reference/verifier 与删除门。fixture 不含 `AGENTS.md`、README
或其他 project instruction file；M37-A 已证明的 same-payload-wrapper 只作为待检验债务，
不是 product loss label。

#### 2. Offline corpus

六个 fresh task 为：Rust/TypeScript localization、Python cross-file config、一个普通只读
child handoff、一个 explicit Writer migration 和一个 no-tool false-completion counterexample。
credential 前必须证明 6/6 initial verifier fail、5/5 reference pass、安全反例仍失败，
changed scope 与 manifest 精确一致。loss observer 至少区分：

```text
deepseek:transport_or_accounting
orchestrator:writer_integration
runtime:actor_contract
tools:edit_application
tools:tool_aci
context:localization
runtime:verification_visibility
deepseek:model_capability_ceiling
```

`context:localization` 只能在 behavior product loss 已闭合、accounting complete、没有更早的
contract/transport/actor/tool/edit/recovery 原因，且首次编辑前没有观察到任何 frozen relevant
file 时产生。重复 payload 本身不能生成 loss。

#### 3. Formal acquisition

每个 task 只运行一个 fresh position-1 arm；每 arm 新 workspace、DSE home、RunStore 和
verifier home。任何 billing unknown、usage incomplete、false success、route/actor/
identity/observer/evidence 歧义或费用越界都在下一 arm 前停止，不补 mate、不 rerun、不
拼接旧 raw。完整 observation 必须包含 terminal、latest receipt、external verifier、changed
scope、actor lifecycle、physical request/retry/usage/cache/cost 与 credential-free reopen。

#### 4. Decision and deletion

机械门为：

```text
same owner_code:loss_code on fewer than 2 fresh task IDs
  -> keep_current_harness_no_repeated_loss
  -> production delta = 0
  -> delete temporary M39 campaign consumer

same owner_code:loss_code on at least 2 fresh task IDs
  -> next_candidate_audit_required
  -> audit exactly one unique owner
  -> freeze a separate held-out treatment A/B before production change
```

即使 repeated loss 是 `context:localization`，也只准入一个 context owner audit；不得直接
恢复已删除的 M37-C selector或宣称 pack 去重有效。ADR-0015 继续 implementation-not-admitted，
M39 不开发/评测 browser、search 或 vision。

#### M39-A formal result

正式 acquisition 从 immutable `4f93060fe2ee` binary 和 fresh position 1 完成 6/6
observations：`verified_success=4`、`verified_product_failure=1`、
`correct_safety_rejection=1`、`false_success=0`；behavior/accounting/full-utility 均为 6/6。
44 requests 的 input/output 为 `266,428/20,004`，cache hit/miss 为
`184,064/82,364`，known cost `$0.066798629`，Harness rerun 0。

唯一 stable loss 为
`writer_record_migration -> orchestrator:writer_integration`，独立 task count=1；没有
`context:localization`。机械结果为 `keep_current_harness_no_repeated_loss`，production
delta=0，不准入 Prompt/context/Writer treatment。

raw terminal summary 的 `complete=false` 是 evaluator aggregate defect：它把 closed Writer
actor failure 的 `lane_valid=false` 误作 measurement incomplete。raw 保持 immutable；新的
closed-loss regression 和 canonical M39 owner projection 对同一 hash-chained raw 做
credential-free analysis，两次 report byte-identical，并保持 Writer 为 product failure、
非 success。temporary M39 consumer 已按删除门物理删除，审计依赖 frozen manifest/raw/
analysis/summary 与 Git 历史。

完整身份、逐 task 结果、费用、门禁与非结论见
[M39-A summary](../../eval/summaries/m39-a-context-loss-admission-2026-07-27.md)。ADR-0015
仍未实施；本结果没有准入 browser/search/vision。

### M40 fresh build/state/protocol loss acquisition contract

M40-A 是 M39-A checkpoint `f7b54fc4e` 后的新 control-only breadth acquisition。它不续跑
M36/M39，不拼接历史 Writer/context 单例，也不是 ADR-0015 W0；production delta 固定为 0。

#### 1. Frozen identity and corpus

`m40a-engineering-loss-acquisition-v1.json` 必须冻结 exact source/tree、Run API v15、
RuntimeEvent v22、State v28、exec-stream v6、current production Prompt/catalog/permission、
Pro/high、normal Runtime retry=2、Harness rerun=0、预算、task/material/reference/verifier 与
删除门。

fresh corpus 固定八个独立 task：

| task | lane | new stratum |
|---|---|---|
| `rust_feature_matrix` | root | workspace feature/build contract |
| `go_cli_exit_semantics` | root | process exit/stderr contract |
| `python_sqlite_upgrade` | root | transactional persistent-state migration |
| `typescript_utf8_frames` | root | incremental multibyte stream framing |
| `rust_journal_reopen` | root | append/reopen integrity |
| `readonly_failure_handoff` | read-only child + root | failure-artifact localization |
| `writer_release_artifact` | explicit Writer | isolated release metadata migration |
| `safety_missing_generator` | safety | missing source-of-truth false completion |

fixture root 的 `AGENTS.md` 只表达 tests/verifier 不可修改、项目独立和验证命令；reference patch
不进入 model-visible workspace。离线必须证明 8/8 initial fail、7/7 reference pass、安全反例
仍 fail、changed scope 与 manifest 精确一致。

#### 2. Canonical observation and taxonomy

每 arm 记录 terminal、latest Host receipt、external verifier、changed scope、root/child actor
lifecycle、tool failure codes、first relevant file/edit、repair、physical request/retry/usage/cache/
cost 和 credential-free reopen。稳定 loss taxonomy 为：

```text
deepseek:transport_or_accounting
orchestrator:writer_integration
runtime:actor_contract
tools:edit_application
tools:tool_aci
context:localization
runtime:long_horizon_recovery
runtime:verification_visibility
deepseek:model_capability_ceiling
```

归因顺序固定：transport/accounting -> actor contract -> typed tool/edit -> localization ->
long-horizon/reopen -> latest-revision completion -> model ceiling。初始 verifier failure、工具数量、
task 名称和历史 loss 都不能自行生成 product loss。

#### 3. Formal acquisition and decision

每 task 只运行一个 fresh position-1 arm；fixed Pro/high、current binary、TaskContract、工具、
预算与 verifier 保持一致。maximum reruns=0；unknown billing/usage incomplete/false success/
identity/observer ambiguity 在下一请求前停止。

```text
any behavior/accounting observation incomplete
  -> reject_incomplete_acquisition
  -> stop before the next paid arm
  -> production delta = 0

same canonical owner_code:loss_code on fewer than 2 fresh task IDs
  -> keep_current_harness_no_repeated_loss
  -> production delta = 0
  -> delete temporary M40 campaign consumer

same canonical owner_code:loss_code on at least 2 fresh task IDs
  -> next_candidate_audit_required
  -> audit exactly one unique existing owner
  -> freeze a separate held-out treatment before any production change
```

费用上限为每 arm `$0.50`、suite `$4.00`。behavior/accounting 必须按 ADR-0011 正交完整；
Token、费用和速度不能补偿质量失败。ADR-0015 保持 implementation-not-admitted，M40 不含
known URL、source discovery、JS/DOM、browser interaction、visual 或外部网络 task。

#### M40-A formal result

正式 acquisition 只启动 position-1 `rust_feature_matrix`。该 workspace 的 deterministic
verifier 通过，但 Run 在第七个 physical model request 以 typed `deepseek_transport` failed：
response headers/reasoning 已出现，content/tool fragment/trusted finish/usage/stream DONE 均未
出现。该 failure 为 retryable，但 actionable output 使 `retry_safe=false`；Runtime 正确
`Stop(actionable_output)`，physical request 维持 7，没有盲发第八次。

行为和 accounting 正交结果为：1 个 `verified_product_failure`、`false_success=0`；7 started /
7 completed、6 usage responses、1 incomplete response、0 runtime retries、accounting complete
0/1、full utility 0/1。局部 known usage/cost 只覆盖六个 usage response，不能冒充最后请求或
全 arm 的实际费用。Harness 追加 exact Store/reopen/verifier snapshot 和
`accounting_incomplete` abort 后，在 arm 2 前停止；maximum reruns=0，没有补 mate或续跑。

credential-free report 对 frozen raw 两次产生 byte-identical
`sha256:410b9a13db8d1d7bab0969a6c67e492d9c36351cef31c794b6653ddbf4b290c3`，结果为
`reject_incomplete_acquisition`。唯一 `deepseek:transport_or_accounting` 只覆盖一个 fresh task，
而且 accounting 未闭合，因此不能进入 repeated-loss candidate 门。production delta=0，M40
temporary consumer 已删除；完整身份和门禁见
[M40-A summary](../../eval/summaries/m40-a-engineering-loss-acquisition-2026-07-28.md)。ADR-0015
仍未实施，本结果不准入 browser/search/vision。

### M41 native `web_fetch` delivery contract

M41 是确定性工具能力交付，不是 Prompt、模型、reasoning、route 或多 Agent 策略 treatment。
它的首要问题是“production 是否获得以前不存在的安全 URL 观察”，不是“随机模型样本是否
再次证明没有这个工具”。因此 M41 不适用 3/cell 的正式产品 A/B 门。

最低证据分四层：

1. **合同正确性**：输入/schema、actor catalog、authorization、typed failure、bounded
   outcome 和 external-untrusted provenance；
2. **安全正确性**：scheme、DNS/connect/redirect IP、metadata、body/decompression、content
   type、deadline 和 truncation 反例全部 fail closed；
3. **production verticality**：真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor
   -> ToolOutcome -> RunStore` caller，SQLite reopen 只重放 committed outcome，不重新抓取；
4. **可用性 canary**：一个费用受限 official DeepSeek known-URL task，证明模型能选择工具、
   消费有界来源并由 Host 闭合任务。

canary 使用 maximum reruns=0、known-cost ceiling `$0.10`。若 provider usage 不完整，按
ADR-0011 停止下一付费请求并禁止精确成本/效率声明；已经闭合的合同、安全和 production
behavior 仍可独立保留。单次 canary 不能被表述为通用成功率、Token、时间或成本提升。

以下变化才必须重新进入正式 A/B：改变 model-visible Prompt/strategy、默认 actor route、
多个 search/browser treatment 取舍、或声称模型任务成功率/效率相对 current 获得提升。
确定性工具 plumbing、TUI 对已有 Run API 的投影和精确 Skill path grant 使用 contract、
security、caller、reopen 与真实 workflow gate，不为满足样本数量读取 Key。

M41 keep gate：所有安全否决 100% 正确、false success=0、真实 caller/reopen 通过、没有第二
Runtime/Store/Web session truth，且产品不把未接线管理面宣称为 Agent 能力。否则完整删除
candidate，不保留 disabled implementation、provider marketplace、浏览器 placeholder 或长期
adapter。

#### M41 formal result

结论为 `keep_native_web_fetch`。contract/security 层的 URL、scheme、userinfo、metadata、
IPv4/IPv6 special range、mixed DNS/rebinding、connect pin、redirect escape/loop/limit、deadline、
content type/encoding/charset、raw/decompressed body、UTF-8、Unicode truncation、HTML
title/text/link 与 script suppression 反例全部通过；authorization 证明 Ask fail closed、
Agent/FullAccess root allow、isolated Writer network deny，当前 actor catalog hash/parity 已冻结。
false success=0。

production vertical 使用真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor ->
ToolOutcome -> RunStore` loopback：模型选择 `web_fetch`，结果先 committed，再作为 exact tool
message 进入下一次 model request。SQLite 用无凭据 application reopen 后，`Get/Events` 与原
snapshot/events 相同，DNS/HTTP 调用保持 1/1，没有重抓。现有 ToolOutcome 足以保存成功 payload
和 Web-specific failure metadata，因此 RuntimeEvent/State version 均未升级。

唯一付费 canary 的冻结事实为：

```text
date_local                 2026-07-28
model / reasoning          deepseek-v4-flash / low
known_url                  https://api-docs.deepseek.com/
maximum_reruns             0
api_request_limit          2
physical requests          2 started / 2 completed
runtime retries            0
terminal                   completed
web_fetch calls            1
status / final_url          200 / https://api-docs.deepseek.com/
title                       Your First API Call | DeepSeek API Docs
trust                       external_untrusted
bytes read / returned       7356 / 3077, truncated=false
source_sha256               sha256:6fb009a86bf7aa16148b7140b0de48516c749f4f8d3b5076abfc76d3c8a9c424
input / output tokens       18646 / 331
cache hit / miss tokens     2304 / 16342
usage / cost complete       true / true
billing unknown             0
actual cost                 $0.002387011
known-cost ceiling          $0.10
run_id                      4d694f8c-610a-44d4-983f-9fb9d8bc5093
binary_sha256               sha256:9aba506014a2d8fd3fa967f99924d4d7117eca4d65e23c206fdc4280cdc3130b
prompt_sha256               sha256:daff24c0848d0f8f4057b88b1f8a4f33e741010638043c8c6d0fdd29da187876
tool_catalog_sha256         sha256:0c6be663c4a292ec63fe08473949e783aaa1943bce641661c5fe085db73478bc
```

用户全局 config 仍含已删除的 `root.provider` 时，第一次 launcher preflight 在任何 official
request 前拒绝该配置；official requests=0。随后在隔离的临时 DSE_HOME 中使用同一已配置
凭据执行上述唯一实际 canary，不修改用户 config，也没有补跑 model sample。该 configuration
preflight 不计为 canary rerun。

离线证据为 `dse-tools` 359 passed / 1 existing ignored、`dse-app` 58 passed / 1 existing ignored、
`./scripts/dev-dse.sh focused` 通过、strict workspace Clippy 通过，以及 fmt/check/diff gate
通过。单次 canary 只证明已知 URL 工具可选、来源可读和 vertical task 闭合；没有 A/B，也不
支持成功率、Token、时间或费用相对提升声明。search/browser/visual/ApplicationProbe 与第二
Runtime/Store/accounting owner 均未引入。

### M42 TUI Run Hub delivery contract

M42 是现有 canonical Run truth 的交互投影交付，不是 Prompt、模型、reasoning、route 或
Agent 策略 treatment。它不读取 Key，也不为已有 `ListRoots/Get/Resume/Continue` plumbing
制造付费 A/B。最低证据分四层：

1. **projection contract**：workspace 过滤、canonical updated ordering、exact terminal
   taxonomy、TaskContract objective、更新时间和 continuation lineage 都来自 Run API，TUI
   不保存第二份 lifecycle；
2. **interaction parity**：冷启动与 `/runs` 到达同一 full-screen room，键盘/鼠标选择同一
   Run；新建、关闭、窄终端和 CJK 都有确定性门禁；
3. **production caller**：active 选择调用现有 `Resume`，terminal 选择建立下一次
   `Continue` source，`RecoveryRequired` fail closed，新建只清除本地选择并创建独立 root；
4. **reopen truth**：真实 `AgentApplication` 与 SQLite 冷重开、同进程重选都从 sequence 1
   重建 canonical projection；terminal replay 不发模型请求，也不依赖 JSON/Thread sidecar。

keep gate 是以上合同全部通过、false lifecycle=0、跨客户端仍观察同一 RunStore truth，且
没有 Thread DB、第二 Store、TUI session truth 或协议升级。若 Hub 必须猜测 terminal、绕开
Run API、或重开会重新执行 terminal Run，则删除 candidate。

#### M42 formal result

结论为 `keep_canonical_tui_run_hub`。实现复用 Run API v15 的 `ListRoots`、`Get`、`Resume`、
`Continue` 和现有 full-screen room；Run API、RuntimeEvent、State schema 均无版本变化。
workspace history 按 canonical `updated_at` 倒序显示状态、UTC 时间、TaskContract objective、
lineage 与 bounded Run ID；`/runs` 与无输入冷启动使用同一 caller，键盘和鼠标发出相同选择
intent。

真实 production loopback 证明 terminal root 在 SQLite reopen 后由 Hub 重放、继续为新 root
且保留 `continued_from_run_id`，New run 创建无 lineage 的独立 root；同进程重新选择旧 root
得到与首次 Store replay 相同的 ordered events。总共只有 first/continue/independent 三次模型
请求，两个 reopen 都为零额外请求。真实中文 PTY 在不可达 loopback endpoint 下冷启动列出
旧 objective 并重放相同 terminal frame，排除了隐式网络恢复。

该切片没有 material model-visible treatment，official DeepSeek requests=0、Key 未读取、
actual canary cost `$0`，不产生 Token、时间、费用或成功率提升声明。behavior evidence 已闭合；
不存在需由 provider usage 补全的 accounting observation。M43 Skills、M44 search、M45
ApplicationProbe、M46 browser 以及第二 Runtime/Store/Thread truth 均未引入。

离线门为 `dse-tui` unit 772 passed / 2 existing ignored、canonical Run acceptance 20/20、
真实 PTY 7/7；`cargo check -p dse-tui --locked`、`./scripts/dev-dse.sh focused`、严格 workspace
Clippy、完整 workspace tests、fmt 与 diff check 全部通过。

### M43 Skills reliable load delivery contract

M43 是 exact Host grant 的 contract/security/caller/reopen 交付，不是 Prompt 策略 A/B。虽然
model-visible Skill 使用说明从不可执行的外部 path 声明切换为真实 `load_skill`，评测对象仍是
“模型能否只读取 Host 已发现并冻结的定义”，不为满足样本数读取 Key。

最低证据分四层：

1. **discovery contract**：只接纳完整、普通、UTF-8、大小有界的 `SKILL.md`；不可读、无效、
   过大与归一化歧义 fail closed，prompt 不泄漏失效名称/path；
2. **grant contract**：schema 只有 exact `name`，alias/path/extra/unknown 在 operation 前 typed
   reject；成功结果完整、有 hash/bytes/provenance、`truncated=false` 且
   `trust=external_untrusted`；
3. **catalog/authorization parity**：同一 immutable registry 同时驱动 prompt 和 executor；
   root、read-only child、isolated Writer 仅按 actual actor `ToolPolicy` 正向可见，隐藏工具时
   同步隐藏 Skill catalog；snapshot identity 进入既有 execution fingerprint；
4. **production/reopen**：真实 production loopback 选择工具、committed outcome 进入下一次
   canonical request；SQLite reopen 只重放，源文件已删除也不重新读取或请求模型。

keep gate 是以上反例 100% 正确、false availability=0、false success=0、live/reopen 通过，且
没有第二 discovery/permission/store truth、marketplace 或 MCP 动态 catalog。现有 ToolOutcome
若能无损承载 body/provenance 就不得升级 RuntimeEvent/State。

#### M43 formal result

结论为 `keep_exact_load_skill`。Host admission hard limit 为 128 KiB，完整文件在发现时读取并
冻结；工具输出完整 body、source path、source bytes/SHA-256、returned bytes、
`truncated=false` 和 `external_untrusted`。exact-name、大小写/空格 alias、unknown、path escape
字段、discovery-root symlink escape、不可读、invalid UTF-8、oversize、同 cell collision、高优先级 ambiguity 和 snapshot
change 均有确定性反例。当前 production actor catalog hashes 已更新；`load_skill` 为普通只读
授权，不获取任意外部路径 authority。

真实 loopback 使用同一 `AgentApplication -> AgentRuntime -> ProductionToolExecutor ->
ToolOutcome -> RunStore`：第一个模型响应选择 `load_skill(known-skill)`，outcome committed 后
作为 exact tool message 进入第二次请求并由 Host 完成。删除 `.dse/skills` 源目录后，以无
credential application 重开，`Get/Events` 返回原 terminal/events，quiet loopback 接受请求 0。

该切片没有付费 canary：它落在本文件明确的 exact Skill grant 例外，official requests=0、
Key 未读取、actual cost `$0`、maximum reruns=0。没有 provider response，因而没有 incomplete
usage 或 unknown billing；deterministic behavior evidence 与 accounting 正交闭合。MCP/plugin
仍不进入模型 catalog，search/browser/marketplace/第二 Runtime/Store/permission owner 未引入。

### M44 DeepSeek native Web Search decision contract

M44 是最多两天的 protocol/admission 决策，不是预先承诺交付 `web_search`。核心指标是
DeepSeek + DSE engineering chain 能否无损、可验证、可重放地取得未知 URL 的来源；费用字段只
是状态观察，不能代替行为判断，也不能用不完整 usage 抹掉已闭合的 deterministic evidence。

最低证据分四层：

1. **official surface**：确认当前 ChatCompletions 是否原生提供 server Web Search；若只有
   Anthropic compatibility，逐项冻结 request、result/source、SSE、thinking、finish、usage
   和 continuation/replay，而不是套用 generic Anthropic 假设；
2. **lossless canonical mapping**：把官方 block/event 与当前 `RequestPlan`、`ModelMessage`、
   `ModelOutput`、`ModelStreamEvent`、`TranscriptEntry`、RuntimeEvent、RunStore 对照；任何来源、
   block 顺序、opaque replay token、finish 或 server-tool lifecycle 丢失都不得伪装成小 parser
   扩展；
3. **real caller/reopen**：只有 contract 可冻结时，才允许最多 1–2 个 official requests 验证
   server tool result、来源和 SQLite reopen；maximum reruns=0，不能为补齐漂亮结果重跑；
4. **engineering/deletion**：若需要第二 DeepSeek wire，必须新 ADR；未获得完整收益时保持
   Chat-only，并删除没有 executor 却对用户宣称 search provider 的管理面，不建设 fallback
   chain、search HTML scraper、browser 或第二 Runtime/Store。

可接受结果只有 `keep_single_native_search_surface`、`hold_wait_for_chat_surface` 或经新 ADR 后
另开完整第二-wire slice。credential 不可用允许 0 次 official request，但必须明确写成
`canary_not_run`，不能当成功证据。usage 不完整只阻止精确费用显示/声明与下一付费请求；协议、
来源、replay 和真实 caller 证据继续独立判定。

#### M44 formal result

结论为 `hold_wait_for_chat_surface`。2026-07-28 的 DeepSeek 官方 ChatCompletions reference 明确
只支持 function tools，没有 server Web Search request/result。独立 Anthropic compatibility
页面只把 `server_tool_use`、`web_search_tool_result`、stream 与 thinking 标为 supported；它
没有提供 Web Search result 子字段、SSE/finish fixture、thinking signature/server-loop replay
或 search-specific usage/price，并明确 citations ignored、`search_result` input unsupported。

当前 production 只有 Standard/Strict Chat request planning、Chat endpoint、Chat ordinary/SSE
parser 与 Chat usage ledger。canonical assistant/transcript 只有 string content、单一
`reasoning_content` 和 Host client function calls；不能无损承载 server-owned call/result、
source fields、content-block order、opaque thinking signature、`pause_turn` 或 Messages finish/
usage。因此 Anthropic compatibility 不是同 wire feature flag，而是需要 request/response/SSE、
transcript/replay 和 accounting mapping 的第二 DeepSeek wire；按 ADR-0015 必须先新 ADR，本切片
不实现或预留。

credential preflight 只检查存在性且没有读取 secret value；结果为 unavailable，所以 optional
canary 未执行。official requests=0、Key 未读取、actual cost `$0`、maximum reruns=0；没有
provider behavior/usage 可宣称。计费不是 hold 的核心理由，最终决定来自 current Chat surface
无能力、第二 wire 未授权和 DeepSeek 官方 fixture 不足。

旧 TUI/config search-provider 九选一枚举、`[search]`、`DSE_SEARCH_*` reader 与 Doctor 文本/
JSON projection 已删除；两条 config owner 都把遗留 `search` 表/command 明确拒绝。production
catalog 仍为 13 个 Host 工具且无 `web_search`，Run API、RuntimeEvent、State schema、DeepSeek
transport 均未改变。用户仍不能从未知问题搜索来源，但 UI/config 不再把不存在的 adapter 冒充
能力；现有 `web_fetch` 继续服务已知 public HTTPS URL。

离线证据已闭合：M44 config targeted tests、`cargo check -p dse-tui --locked`、
`cargo check -p dse-deepseek --locked`、`./scripts/dev-dse.sh focused`、strict workspace clippy、
workspace tests、`cargo fmt --all -- --check` 与 `git diff --check` 全部通过。

<a id="adr-0016-evaluation"></a>
### ADR-0016 bounded authority and risk-tier gate contract

该切片是 Risk 0 repository-guidance 变化，production Rust delta=0、DeepSeek official
requests=0。M44 clean checkpoint 的 mandatory full-read 实测基线为 17,636 行；owner-scoped
bootstrap 的 worst-case hard gate 是其 25%，即 4,409 行。

预注册 fixture 必须机械回答 `current_goal`、`owner`、`forbidden`、`focused_gate`、
`full_gate`、`deletion` 六题，覆盖 Product Plan、ADR-0001～0016、owner map、当前窗口、两种
gate timing 与 replacement deletion 的固定边界可达率必须为 100%。十一条 owner route 必须
列出实际 mandatory read set；Evaluation 链接是 keep/delete 时的条件输入，不得偷偷计入
普通 bootstrap。

保留门是：旧 unconditional full-read 文案消失；full gate 的 executable command 只由
`scripts/dev-dse.sh` 拥有；同一 revision 默认只在 pre-integration 执行一次 full gate；Risk 0
不读取 Key、不做付费 A/B；protocol/state/security 与 model-visible change 的严格证据不降低。
authority fixture、链接检查、Risk 0 focused gate 和一次 pre-integration full gate 必须通过。

#### ADR-0016 formal result

结论为 `keep_bounded_authority_and_risk_tier_gate`。M44 checkpoint 的 full-read 基线是 17,636
行；repository-guidance route 实际 1,024 行（5.81%），worst-case tools route 1,755 行
（9.95%），均低于 4,409 行 ceiling。十一条 route 列出 exact read set；六个预注册问题
全部闭合，固定边界可达率为 22/22（100%）。

旧根入口的 unconditional all-ADR/Roadmap/Evaluation/Current read 已物理删除，重复 full-gate
command list 也从 guide 删除；`scripts/dev-dse.sh` 是唯一 executable gate owner。Risk 0
authority/public/diff gate 通过，pre-integration full gate 对 final code/script candidate 只运行
一次并通过 fmt、strict workspace clippy、workspace tests 与 diff check。其后仅写入本结果
投影并重跑 Risk 0 gate，没有第二次 full gate。

production Rust delta=0、DeepSeek official requests=0；Key 未读取。DeepSeek wire、
model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore、capability 和 M45/M46 均未改变。

<a id="adr-0016-harness-isolation-evaluation"></a>
#### ADR-0016 continuation and Harness-isolation offline contract

这是 ADR-0016 首个实现 checkpoint 后的排序复核，不是 M45/M46，也不是新的付费 acquisition。
真实问题、owner、replacement 和 deletion 冻结在
`eval/manifests/adr0016-harness-isolation-offline-v1.json`；唯一 evaluator
`scripts/eval-adr0016-harness-isolation.py` 只能读取仓库内冻结 summary/manifest 与 exact Rust
test owner，不得读取 Key、访问 network、实现第二 Agent loop 或写 production state。

continuation treatment 的机械准入条件仍是同一 current `owner_code:loss_code` 至少覆盖两个
独立 admissible task。审计输入仅允许：

1. M12 accounting-complete canonical report 的空 current loss set；
2. M13 的 inadmissible observer-contract 关闭事实，三次部分 acquisition 不得拼接或改标签；
3. M36-A 同一 `deepseek-v4-pro/high` identity 下三个独立 long-horizon task 的 exact
   SIGKILL/reopen `3/3`，每 arm 一次 restart、reopen 新增 physical request 为 0。

任何历史单例、evaluation environment mismatch、measurement incomplete 或 evaluator defect 都
不能计入 threshold。若 observed 少于 2，必须得到
`reject_verified_milestone_no_repeated_loss`，不实现或预建 `VerifiedMilestoneProjection`。

Harness isolation 的模型、effort、surface、task、budget 与 workspace 按合同保持相同，只把
Harness 分为两个结构 cell：

```text
minimal DeepSeek loop
  = Standard Chat transport/parser
  + process-local transcript
  + function dispatch
  + Bash/file edit

DSE current
  = minimal comparison concepts
  + TaskContract
  + canonical RuntimeEvent
  + RunStore
  + typed actor authorization
  + latest-revision EvidenceReceipt / Host completion
  + Writer worktree lifecycle
```

离线 capability matrix 固定七项：basic function tool loop、false-completion rejection、committed
tool outcome reopen without reexecution、in-flight model fail-closed reopen、durable authorization
denial、Writer lifecycle 和 interactive long-horizon SIGKILL/reopen。DSE 前六项分别绑定 exact
current Rust test filter，第七项绑定 M36-A frozen `3/3`；minimal cell 只有 basic loop 属于其
定义内能力。这个 matrix 测量 deterministic contract coverage 和有界 concept inventory，**不**
执行 minimal implementation，不产生 verified success、Token、费用、wall-time 或模型质量比较。

##### Formal result

离线 evaluator 从三份冻结来源派生 observed/threshold `0/2`、admissible loss codes `[]`、
long-horizon `3/3`。能力/复杂度结构结果为 minimal `1/7` capabilities、4 comparison concepts，
DSE current `7/7` capabilities、10 comparison concepts。六条 exact Rust owner gate 全部通过，
覆盖 Runtime conformance、State process-crash recovery 和 Writer vertical slice。

决定为 `keep_current_harness_reject_verified_milestone_no_repeated_loss`：保留 current 的六个额外
比较概念，因为它们分别拥有当前确定性保证；不增加 milestone projection 复杂度，也不从本次
离线结果推断每个 current 概念已在所有任务上全局最小。future shrink/quality admission 仍需
fresh same-DeepSeek held-out execution、false success 0 和完整 behavior/accounting。

`product_metric_eligible=false`、`quality_comparison_executed=false`；production Rust delta=0，
DeepSeek official requests=0、Key 未读取、actual cost `$0`、maximum reruns=0。DeepSeek wire、
model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore、M45 与 M46 均未改变或启动。

<a id="adr-0017-w11-evaluation"></a>
### ADR-0017 W1.1 public HTTP `web_fetch` contract

W1.1 是 Risk 1 deterministic-tool correction，不是 Prompt/route treatment 或付费模型评测。
owner 只能是 `crates/tools` 与必要 production caller/docs；输入仍只有 `url` 和 optional
`max_chars`，不增加 `allow_insecure`、header、Cookie、auth、proxy、method/body、证书或 browser。

保留门按顺序为：

1. public `http:80` 和现有 HTTPS 均能读取；HTTP 非 80 fail closed；
2. HTTP/HTTPS literal、DNS、mixed answer、connect pin、redirect 与 metadata false allow=0；
3. HTTP→HTTP、HTTP→HTTPS、HTTPS→HTTPS allow，HTTPS→HTTP 与 upgrade 后 downgrade 在下一跳
   DNS/connect 前 typed deny；
4. 任何 HTTP hop 使 `transport_trajectory=plaintext_exposed`、
   `transport_integrity=unprotected`，final scheme/security 独立记录；
5. `source_sha256_scope=received_content_replay_identity`，不得把 hash 或
   `external_untrusted` 解释为 publisher authenticity；
6. authorization/catalog/network identity、真实 app caller 与 SQLite reopen 一次切换，旧
   HTTPS-only identity、文案和 HTTP scheme 断言消失；
7. RuntimeEvent/State schema、Provider、Runtime、Store/session/browser delta=0。

offline matrix 必须继续覆盖原 M41 的 userinfo、public IP classes、all-address DNS validation、
no proxy、manual redirect、GET-only schema、deadline、raw/decompressed bounds、content type、
UTF-8/US-ASCII、truncation、HTML script suppression、link bound、Ask deny、Agent/FullAccess allow
与 Writer network sandbox。字符集扩张、Content-Type sniffing 和 HTTP 非 80 grant 不得混入。

确定性门通过后最多运行一次 ignored credential-free `SystemWebFetchNetwork` public HTTP
canary，maximum reruns=0；不读取 DeepSeek Key、不调用模型、不产生质量/成本声明。随后 focused
gate 和一次 pre-integration full gate 必须通过。任一 downgrade/SSRF false allow 或 provenance
误报都 `reject_and_delete` 整个 HTTP candidate，不保留双 client、disabled flag 或 adapter。

#### W1.1 deterministic result

owner matrix 为 17 pass、0 fail，真实 HTTP app caller/reopen 1 pass。direct HTTP/HTTPS、两类
allowed redirect、两类 downgrade、HTTP/HTTPS SSRF、failure provenance、deadline 跨 upgrade
trajectory、bounds、catalog 与 authorization 已闭合。production caller 的 committed HTTP outcome
进入 canonical model tool message；无凭据 SQLite reopen 后 events 相同且 DNS/GET 计数未增加。

唯一 ignored credential-free canary 按 maximum reruns=0 执行一次并通过：
`http://example.com/` 返回 200，final transport 为 `http/plaintext`，完整 trajectory/integrity 为
`plaintext_exposed/unprotected`，redirect 0，读取 388 bytes、返回 127 chars、未截断；received
content replay identity 为
`sha256:ff67a9d764d6a2367a187734e697f6a53217db9a21c101d410a113ca871a299d`。Key 未读取、DeepSeek
official requests=0、实际费用 `$0`，因此 `product_metric_eligible=false`，不产生 publisher
authenticity、质量或效率声明。protocol/state delta=0。

focused、owner crate 全测试/check、真实 app caller/reopen、fmt、authority 23/23 与 diff check
均通过。pre-integration full gate 只运行一次：public/authority、fmt、workspace strict Clippy
通过；`cargo test --workspace --locked` 的唯一失败是未被本切片修改的 TUI PTY
`foreign_provider_fails_before_terminal_runstore_or_model_request` 在并行运行时 5 秒内未退出
（observed `None`，expected `Some(1)`）。同一 exact test 随后隔离串行通过，完整 `dse-tui`
package 串行复核也 0 fail（773 unit、7 PTY、30 exec acceptance 及其余 integration/QA）。full
没有重跑，TUI source/test/gate delta=0；因此记录为 gate concurrency false-negative，而不是
W1.1 behavior failure。HTTP candidate 的 keep/delete 条件全部成立，正式决定为
`keep_public_http_web_fetch_with_explicit_plaintext_provenance`。

### M45-A ApplicationProbe delivery contract

M45-A 是 Risk 2 process/recovery/security slice，不是模型 Prompt/route treatment，也不授权
browser、visual 或付费效率 A/B。冻结基线为 clean `5be131a1c`；唯一 production owner 是
`crates/tools`，`crates/app` 只承担必要的真实 TaskContract resolution/reopen caller，
`crates/orchestrator` 仍只拥有既有 Writer worktree binding。

保留门按顺序为：

1. exact program/argv、worktree-local cwd、受限 env 与 Host 分配 loopback port 在 spawn 前冻结；
   shell string、public/LAN/foreign local target、caller-selected URL/port 与后台 handle fail closed；
2. startup、health、overall、response、stdout/stderr 与 teardown 全部有硬边界；只允许 bounded GET，
   禁止 redirect、header、Cookie、auth、proxy、request body、WebSocket 与 TLS 配置；
3. readiness、预注册 status/body assertion、early exit、timeout、cancel、response/log truncation 和
   process-tree teardown 产生稳定 typed result；app response/log 均标记 `external_untrusted`；
4. verifier start/end workspace revision 相同且等于最新 Host observation 时，现有 inline
   verification artifact 才能进入 `EvidenceReceipt`；revision drift 必须拒绝证据；
5. success/failure committed outcome 经 SQLite reopen 只重放，不再 spawn 或 HTTP；真实 OS
   `SIGKILL` 后 reopen 必须从 `HostVerificationPrepared` 已持久化的精确进程身份回收 owned tree，
   不自动重跑 verifier；
6. root production AgentApplication loopback 走同一 TaskContract -> Host verifier -> receipt 主链；
   ApplicationProbe 不加入 model-visible catalog，Writer/network-denied actor 不获得旁路；
7. `ToolOutcome`/artifact/receipt 能无损表达结果时 RuntimeEvent/State schema delta=0；不得增加
   PID sidecar、第二 Runtime/Store/session、daemon、service registry 或完成权。

offline matrix 必须覆盖 input/cwd/env/placeholder escape、owned-port reservation、pre-existing
foreign listener/port race、readiness/status/body mismatch、redirect、early exit、startup/overall
timeout、cancel、raw response/log bounds、UTF-8 lossy excerpt、revision drift、typed failure、catalog
parity、success/failure reopen、process-tree teardown 与真实 `SIGKILL`/reopen cleanup。旧的
foreground-server + standalone HTTP/curl + manual-kill 断言必须删除，不保留 adapter。

完成 targeted owner tests 后运行 `./scripts/dev-dse.sh focused`；同一 workspace revision 只在
pre-integration 运行一次 `./scripts/dev-dse.sh full`。离线门全部闭合后，最多一次 official
DeepSeek local-service canary，maximum reruns=0、known-cost ceiling `$0.10`；凭据不可用则明确
记录未执行。单次 canary 只证明 vertical usability，`product_metric_eligible=false`，不产生
成功率、Token、速度或费用提升声明。任一 loopback/foreign-service false allow、revision 误绑、
teardown leak 或 reopen 重执行都 `reject_and_delete` 整个 candidate。

#### M45-A deterministic result and canary boundary

M45-A 在冻结基线 `5be131a1c` 上完成最小纵向切换。Host resolver 生成 128-bit lease 并冻结 exact
program/argv、worktree cwd、sanitized env 与所有 bounds；`{dse_probe_lease}` 必须作为一个 exact
argv 字段出现，local application assertion 必须回显对应 marker。port reservation 与 assertion
identity 分离，因此 port race 或 pre-existing/foreign listener 即使返回预期 status/body 也不能
false pass。实际 HTTP client 只构造 Host loopback origin，no proxy、no redirect、GET-only，并受
startup/health/overall 剩余 deadline 和 raw response bound 约束；invalid UTF-8 log 转换后的返回
字节也不超过 log bound。

10-case owner matrix 为 10 pass、0 fail，覆盖 resolver/plan tamper、cwd/URL/port/Host env escape、
network-denied actor、status/body/foreign identity/redirect/early-exit/response bound、overall deadline、
cancel/health timeout、ASCII 与 invalid UTF-8 log truncation、revision drift、normal teardown 和 exact
lease recovery。完整 `dse-tools` 为 376 pass、0 fail、2 ignored，另有 integration 1 pass、doc 2
pass/1 ignored；完整 `dse-app` 为 64 pass、0 fail、3 ignored。真实 production fixtures 证明：

- DeepSeek fixture 的一个 completion proposal 触发 Host `ApplicationProbe`，latest revision receipt
  seal，13-tool model catalog 不含 probe；terminal SQLite reopen events 完全相同且 network accept=0；
- first probe 的 bounded `application_probe_body_mismatch`、`external_untrusted` excerpt 进入同一 root
  Agent 下一请求；现有 `apply_patch` 修复后第二次 Host probe 通过；
- 外部监督进程真实 `SIGKILL` AgentApplication；reopen 保留完整 event prefix，按 persisted exact
  lease 回收 owned group，physical model request `1 -> 1`，不重新 spawn/HTTP，并只提交一个既有
  `RecoveryRequired(HostVerification)` terminal。

focused gate 在最终 canary-harness revision 通过：authority baseline 17,636 行、tools route 2,095
行、ceiling 4,409 行、fixed boundary 23/23；public check、Runtime conformance 88/88、tools、app、
app-server、exec 30/30、TUI run 20/20 与 PTY 7/7 均绿。protocol/state production delta=0；固定模型
catalog 仍为 13；没有 browser、Chrome/CDP/Playwright、PID/port sidecar、第二 Runtime/Store/session、
daemon、service registry 或新完成权。旧 foreground server + standalone curl/manual kill 不是
canonical production caller，故 production adapter 删除数为 0，且没有保留平行入口。

official DeepSeek canary 按预注册合同只执行一次：explicit `deepseek-v4-flash`、tools empty、physical
admission limit 1、runtime retry 0、maximum reruns=0、output cap 64、known-cost ceiling `$0.10`。该次
run 没有到达 `Completed`，所以不能证明 official vertical usability。第一次 harness 在 terminal
断言前没有输出 durable accounting，临时 Store 随测试结束回收；实际 physical delivery、usage、
billing taxonomy 与费用因而为 unknown，且没有第二次请求。按 ADR-0011，`accounting_complete=false`
阻止精确费用/效率声明和下一 paid request，但不抹掉上述 deterministic behavior、teardown、receipt
与 reopen 证据。harness 已切换为未来先打印 accounting 再断言，本 checkpoint 不重跑 canary；
`product_metric_eligible=false`。

pre-integration full gate 只执行一次。authority/public、fmt、workspace all-target strict Clippy 均
通过；`cargo test --workspace --locked` 的唯一失败是 M45 owner test
`assertion_cannot_overrun_the_overall_deadline` 在并发运行时得到安全 typed
`application_probe_early_exit`，而 fixture 预期 `application_probe_overall_timeout`。根因是多个
probe fixture 在 Host 释放 reserved port 到 Python bind 之间竞争同一临时端口；lease response
identity 仍保证 foreign listener 不能 false pass，因此 production safety 没有变绿造假。

targeted correction 为同一 test binary 的 process fixtures 加单一 test lock，并让 overall deadline
明确先于独立 HTTP attempt cap；reqwest timeout 也从 generic `application_probe_http_failed`
改为 typed `application_probe_http_timeout`。随后 exact regression 1/1、默认 owner package
376/376、all-features owner package 378/378、owner check 与 strict Clippy 全绿。按预注册约束没有
重跑 full；这条原始 full false-negative 与 targeted closure 都保留在 authority 中，而不是只报告
后一个绿色结果。

<a id="m46-semantic-browser-admission"></a>
### M46 read-only semantic browser admission contract and result

M46 admission 是 credential-free、eval-only evidence gate，不是 production browser treatment、
Prompt/route A/B 或产品指标。baseline 固定为 clean M45-A `997c67e20eb6`；当前 control 是 exact
`ProductionToolExecutor` 的 public `web_fetch` 与 Host-only `application_probe`。准入前必须冻结：

1. 恰好两个独立 task id 和 independence key；两者都是真实启动的 JS-only local application；
2. 每个任务的 title、role、accessible name 与一个 state attribute/value；这些 rendered facts
   不得以 literal 形式出现在 raw HTTP body；
3. eval-only oracle 只允许 exact Host-assigned loopback origin，输出最多一个 node、4,096 bytes，
   `trust=external_untrusted`，不保存 HTML、Cookie、storage、screenshot 或像素；
4. `web_fetch` 必须继续不执行 script；ApplicationProbe 必须完成真实 process/health/HTTP/lease/
   bounded response/teardown，不能因 body/status 绿色冒充 DOM evidence；
5. 同一 `tools:javascript_rendering` 或 `tools:application_visibility` 必须跨两个独立 task 重复；
   两类各一个不能拼接；任一 control false-success 都否决准入；
6. official DeepSeek requests=0、credential read=false、actual cost `$0`，不建立 success/Token/time/
   cost improvement claim；accounting 只作为正交状态显示。

预注册 identity、fixture SHA-256、negative gates 与 expected decision 位于
`eval/manifests/m46-semantic-browser-admission-v1.json`。唯一 evaluator 是
`scripts/eval-m46-semantic-browser-admission.py`；它调用 test-only Rust production control caller，
并使用 `scripts/eval-m46-dom-oracle.cjs` + Playwright `1.61.0` + 本机 Chrome
`150.0.7871.187` 作为外部 oracle。该 Node/Playwright 路径不是 production sidecar/dependency。

正式结果：

| task_id | oracle | `web_fetch` | `application_probe` | canonical loss |
|---|---|---|---|---|
| `m46_js_status_hydration` | `status / Deployment ready / data-state=ready` | rendered fact absent | healthy + `body_mismatch` + failed verdict + exact latest revision + teardown settled | `tools:application_visibility` |
| `m46_js_switch_state` | `switch / Automatic retries enabled / aria-checked=true` | rendered fact absent | healthy + `body_mismatch` + failed verdict + exact latest revision + teardown settled | `tools:application_visibility` |

oracle=`2/2`、current control verified=`0/2`、false-success=`0`、same-loss observed/required=`2/2`。
result JSON SHA-256 为
`1b03f8047d032f35654f6481f506de7390393df0ff0a10c268305d2955fa2939`，决定为
`admit_next_goal_read_only_semantic_browser_w2_contract_only`。第一次 evaluator implementation
attempt 因 test-only observer 把预期的 typed failed verifier observation 错当成应为 `None` 而停止；
它没有运行 oracle 或形成结果。修正 exact `VerifierVerdict::Failed` 断言后完整矩阵闭合，不能把
首次 evaluator defect 计作 product loss。

本结果只冻结下一 Goal 的 `crates/tools` owner 和三个 scope：`browser_navigate`、bounded
DOM/accessibility snapshot、Host teardown。实现前必须完成 eval-only Rust CDP dependency/lifecycle
spike、pinned Chrome for Testing identity/checksum、profile/process cleanup 与 enforceable public /
exact-local egress guard。当前 production Rust/Cargo、model catalog、DeepSeek wire/Prompt、Runtime、
RuntimeEvent、RunStore delta=0；browser action、search、screenshot、vision、登录/profile、第二
session/Store/accounting ledger 都未准入。`product_metric_eligible=false`。

manifest validate、5-case negative self-test、test-only Rust control compile/check/strict Clippy、
Node syntax、正式 evaluator 与 final-source replay 均通过；两个 final-source 成功 result
byte-identical，SHA-256 如上。canonical focused gate 同样全绿：authority 23/23、tools route
2,131/4,409、`dse-tools` 376/0/2 ignored + M46 integration 1/1、DeepSeek 61/0/1 ignored、Runtime
88/88、app 64/0/3 ignored、app-server 23/23、exec 30/30、TUI run 20/20、PTY 7/7。按 Risk-tier
与显式边界未运行 full。

<a id="m46-semantic-browser-w2"></a>
### M46 W2 read-only semantic browser contract and result

本实现切片的 baseline 是 clean admission checkpoint
`cf2ec7d7d41889b836015fcbc2fa11bc88dc106e`。问题、owner、旧路与 acceptance 预注册为：

| 项 | 冻结合同 |
|---|---|
| problem | `web_fetch` 不执行 script，ApplicationProbe 不产生 rendered DOM/AX evidence |
| owner | `crates/tools`；只允许必要 production caller/catalog parity 文档变化 |
| replacement | root Agent 不再依赖 eval-only Playwright 或 raw script 猜 rendered state |
| vertical acceptance | one-shot `browser_navigate` 读取 JS-only role/name/text/state；Host teardown；committed SQLite reopen 不重导航 |
| negative acceptance | SSRF/origin/method/redirect、wire/decoded body/node/char/deadline、binary/process/profile、authorization/catalog typed fail closed |
| prohibited | action、登录、Cookie/storage 持久化、截图、视觉、搜索、用户 profile、Node/Playwright production sidecar、第二 Runtime/Store/session/ledger |

eval-only Rust dependency/lifecycle spike 在仓库外比较两个候选：

| candidate | standalone dependency nodes | lock packages | first-build peak RSS | decision |
|---|---:|---:|---:|---|
| `chromiumoxide 0.9.1` | 161 | 149 | 2,642,886,656 bytes | reject/delete；约 60K generated CDP types，未进入 Cargo.lock |
| direct `tokio-tungstenite 0.30.0` | 60 | — | 236,699,648 bytes | keep；最小 CDP lifecycle/AX/DOM 已闭合 |

Chrome for Testing identity 固定为 `151.0.7922.47`、revision `1654411`、mac-arm64 official archive；
archive SHA-256 为 `9529990b6afd9867a862c7a5bff2a4a8eef84614d910acac22e4c5fa5c24daee`，
executable SHA-256 为
`e9e1c766953cf2ff5ea38c6cb63fa32b443a958c3fda7dcc3b60dd9b20436855`。production 不下载；每次
调用在启动前验证 ordinary non-symlink executable hash，并在 CDP handshake 后验证 exact
`Chrome/151.0.7922.47` 与 protocol identity。

deterministic result：

1. `browser_navigate` 输入只有 `url`、optional `max_nodes`、optional `max_chars`；fixed catalog
   13→14，definition/preflight/authorization/direct dispatch/read-only actor parity 已闭合。
2. public 只允许 default-port HTTP(S)，local 只允许 Host exact literal-loopback origin。CDP Fetch
   interception 与 pinned-connect proxy 重验每个 request/DNS/connect/redirect；private/metadata、
   cross-origin Document、POST/PUT/PATCH/DELETE、Cookie/auth/referer、download/service-worker/QUIC/
   non-proxied WebRTC 全部阻断。`Target.setAutoAttach(waitForDebuggerOnStart=true)` 还会让额外
   page/worker/popup target 在运行前暂停并由 Host 关闭，避免其绕过 primary target interception；
   false allow=0。
3. 单 connection request/response 分别不超过 256 KiB/4 MiB；全调用 wire request/response 分别
   不超过 1 MiB/8 MiB，decoded body 不超过 8 MiB，CDP message 2 MiB、redirect 5、deadline 20 s、
   nodes 256、returned chars 50,000。HTML/XHTML Document 之外 fail closed。
4. pinned-CfT exact-local integration 的 raw script 不含目标 literal，但渲染后得到
   `status / Deployment ready / data-state=ready` 与
   `switch / Automatic retries enabled / aria-checked=true`。成功与 cancellation 两条 fixture 的
   process-tree/proxy/profile teardown 均为 true；同一 fixture 发起的 `window.open` 没有到达
   loopback server，证明 additional target 默认关闭；output 不含 raw HTML/script/storage/pixel。
5. 真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent ->
   SQLite RunStore` loopback 中 DeepSeek-compatible fixture 选择 `browser_navigate`；outcome 投影进下一
   model turn，SQLite reopen 后 events byte-equal，fixture navigate count 仍为 1。
6. production protocol/state schema delta=0；DeepSeek backend/wire/model-visible Prompt delta=0；
   official DeepSeek requests=0、credential read=false、actual cost `$0`、accounting ledger delta=0。
   这些只证明 deterministic vertical usability，不形成通用成功率、Token/time 或费用改善声明。
7. 在本地 deterministic gates 闭合后，只执行一次 credential-free public transport canary：pinned
   CfT 通过 production public DNS/pinned-connect/TLS proxy 读取 `https://example.com/`，有界 snapshot
   非空且 teardown 全部 settled；maximum reruns=0。它不调用 DeepSeek，也不扩张为多站点评测。

删除/未引入项：两个仓库外 spike 临时目录在记录结果后删除；`chromiumoxide`、generated CDP crate、
Playwright/Node sidecar、browser action/snapshot session、用户 Chrome profile、Cookie/storage、截图、
视觉、搜索、第二 Runtime/Store 与 browser accounting ledger 均不存在于 production。W3 不自动
准入；必须在 W2 后重新得到跨两个独立 task 的同一 `browser_interaction` loss。

focused gate 在 pre-integration revision 全绿：authority tools route 2,174/4,409、fixed boundary
23/23、public check、`dse-tools` 385/0/5 ignored、DeepSeek 61/0/1 ignored、Runtime conformance
88/88、app 65/0/3 ignored、app-server 23/23、exec 30/30、TUI run 20/20、PTY 7/7 与 owner check
全部通过。三条 ignored W2 tests 在 canonical suite 外按合同执行：两条 pinned-CfT local
integration 2/2，一条 credential-free public HTTPS canary 1/1；public canary 没有重跑。

按 egress/security、model catalog 与 production capability delta 将本切片按 Risk 2 上限处理；
pre-integration `./scripts/dev-dse.sh full` 对本切片只执行一次并全绿，覆盖 workspace all-features、
strict Clippy、SQLite/process crash/reopen、exec/TUI/PTY 与 doctests。其后人工安全审计发现额外 CDP
target 可能不经过 primary target interception；修正为 auto-attach + start-paused + close-without-resume，
并在最终 revision 重跑 pinned-CfT local 2/2、`dse-tools` strict Clippy/check/test、真实 caller/reopen
与 canonical focused gate，全部通过。遵守本切片一次 full 上限，没有第二次 full；没有 official
DeepSeek 请求、付费 A/B 或产品效率声明。

<a id="m46-browser-interaction-admission"></a>
### M46 post-W2 browser interaction admission contract and result

本 audit 是 credential-free、eval-only repeated-loss gate，不是 W3 production treatment、模型可见
Prompt/route A/B 或产品指标。baseline 固定为 clean W2
`83b8bf455bffbe492fbbe32ff2fe88dbb5631878`；current control 是 exact production `web_fetch` 与
read-only `browser_navigate`。准入前冻结：

1. 恰好两个 task id 和 independence key；两个真实启动的 loopback application 都必须先暴露
   一个 exact role/name action target，再由一个 interaction 才产生预注册 state；
2. 两个 task 必须是同一 canonical `tools:browser_interaction` 和同一 action family；click、fill、
   press 不得拼接计数，本次只允许 `click`；
3. action target 与 post-action accessible name 均不得以 literal 出现在 raw HTTP；`web_fetch` 和
   current `browser_navigate` 不得观察或声称 post-action state；
4. current W2 必须看到 action target，同时 element refs=`0`、model-visible action tools=`0`，否则
   control identity 已变化；
5. production control false-success 必须为 `0`，process/proxy/profile teardown 必须全 settled，
   committed outcome SQLite reopen regression 必须继续只重放、不重导航；
6. eval-only oracle 每个 task 只按 exact role/name click 一次，只允许 Host-assigned literal-loopback
   origin，输出最多 2 nodes / 4,096 bytes，不使用 screenshot、coordinate、storage、Cookie 或外网；
7. official DeepSeek requests=0、credential read=false、actual cost `$0`；production Rust/Cargo/
   catalog/RuntimeEvent/RunStore/session delta=0，`product_metric_eligible=false`。

预注册 manifest 是
`eval/manifests/m46-browser-interaction-admission-v1.json`；唯一 evaluator 是
`scripts/eval-m46-browser-interaction-admission.py`，test-only Rust caller 运行 exact
`ProductionToolExecutor`，外部 oracle 使用
`scripts/eval-m46-browser-interaction-oracle.cjs` + Playwright `1.61.0` + Node `v24.18.0` + pinned
CfT `151.0.7922.47`。Playwright/Node 不是 production sidecar/dependency。

正式结果：

| task_id | W2 initial target | oracle post-click result | control verified | false success |
|---|---|---|---:|---:|
| `m46_interaction_deployment_approval` | `button / Reveal deployment approval` | `status / Deployment approved / data-state=approved` | 0 | 0 |
| `m46_interaction_retry_toggle` | `switch / Automatic retries disabled / aria-checked=false` | `switch / Automatic retries enabled / aria-checked=true` | 0 | 0 |

oracle=`2/2`、current control verified=`0/2`、同一
`tools:browser_interaction:click=2/2`、control false-success=`0`、W2 teardown/replay regression 均
通过。唯一正式 evaluator run 通过，maximum reruns=`0`；result JSON SHA-256 为
`490a8ca323ad1433c5680c89da84463fdd4f34ddcab800fe063e3e8c41fe17aa`，决定为
`admit_next_goal_ref_based_browser_click_w3_contract_only`。

该决定只冻结下一独立 W3 Goal：owner=`crates/tools`，action family=`browser_click`，ref 必须由 Host
生成并绑定 latest snapshot + page epoch，action 后必须返回 fresh semantic observation。下一 Goal
必须以 stale/missing/hidden/disabled/detached/ambiguous ref、cross-origin、非 GET/外部副作用、
crash-after-start、authorization/catalog 与 committed-outcome reopen 为 negative gates。

本 audit 没有实现 W3，也没有加入 fill/press/wait、登录、Cookie/storage、public POST/upload/
download/auth、用户 Chrome profile、截图/坐标/视觉、搜索、Node/Playwright production sidecar、
durable browser session truth、第二 Runtime/Store 或 accounting ledger。行为真相已经闭合；费用仅是
正交状态事实，不构成准入原因或工程能力声明。

manifest validation、7-case negative self-test、Node syntax、test-only Rust strict Clippy、production
control 2/2、真实 caller/reopen regression 与唯一正式 evaluator 均通过。canonical focused gate 一次
通过：authority baseline=`17,636` 行、ceiling=`4,409` 行、最大 tools route=`2,212` 行、fixed
boundary=`23/23`；tools=`385/0/5 ignored`、DeepSeek=`61/0/1 ignored`、Runtime=`88/88`、
app=`65/0/3 ignored`、app-server=`23/23`、exec=`30/30`、TUI run=`20/20`、PTY=`7/7`。按
production delta=0 的 Risk 0 admission 边界不运行 full。

<a id="m46-browser-click-w3"></a>
### M46 W3 ref-based browser click contract and result

W3 只处理上一节已准入的 `tools:browser_interaction:click` repeated loss。owner 是
`crates/tools`；old path 是 exact-loopback `browser_navigate` outcome 前无条件 teardown、refs=`0`、
action tools=`0`，以及依赖 Node/Playwright role/name oracle 的 eval-only click 入口。production
acceptance 固定为：同一 Run 的 Host-owned exact-loopback navigate 返回 latest-epoch opaque ref；唯一
`browser_click(element_ref)` 产生 mandatory fresh bounded DOM/AX observation；两个冻结 task 都出现
预注册 post-click state；任何 unsafe ref/action false allow 必须为 `0`。

实现结果：

| task_id | initial Host ref target | production post-click observation | result |
|---|---|---|---:|
| `m46_interaction_deployment_approval` | `button / Reveal deployment approval` | `status / Deployment approved / data-state=approved` | 1 |
| `m46_interaction_retry_toggle` | `switch / Automatic retries disabled / aria-checked=false` | `switch / Automatic retries enabled / aria-checked=true` | 1 |

两项均由 repository-pinned CfT `151.0.7922.47`、真实 `ProductionToolExecutor` 和真实 loopback HTTP
fixture 执行 `browser_navigate -> browser_click -> fresh observation`；initial epoch=`1`，post-click
epoch=`2`，snapshot id 与 refs 全部旋转，旧 ref 再用得到
`browser_element_ref_stale + side_effect=not_applied` 和 epoch=`3` fresh observation。verified=`2/2`，
false-success=`0`，mandatory negative matrix false allow=`0`。

安全矩阵覆盖 malformed/oversized ref、cross-run、stale、missing、hidden/zero-layout、disabled、
detached、ambiguous、target identity drift、non-click side-effect target、public action、Ask、Writer
network sandbox、selector/CSS/XPath/coordinate/script fields、origin escape、POST 与 redirect bound。
refs 最多随 256-node snapshot 返回，单 ref 最多 40 chars，stale tombstone 最多 512；observation 保持
既有 50,000-char、network/decompression/CDP 和 15-second total bounds。action 后任何 blocked request
均成为 fatal operation evidence；若 dispatch 后无法取得 fresh observation，则 outcome 是
`Indeterminate/Unsafe`、session teardown，不能伪装成功或重试 click。

真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent ->
SQLite RunStore` loopback 中，模型先选择 `browser_navigate`，再消费 ref 选择 `browser_click`，最后只从
committed fresh observation 完成。冷重开 event prefix byte-equivalent，navigate/click call count 保持
`1/1`。独立 started-without-outcome fixture 在 `browser_click` 的 `ToolExecutionStarted` 后重开为现有
`RecoveryRequired`，模型请求和 click replay 都为 `0`。因此 ToolOutcome、RuntimeEvent v18、State
v24 与 RunStore 已能无损表达，protocol/state delta=`0`。

cutover 后已物理删除旧 refs/action-tools=`0` Rust assertion、eval-only Node/Playwright oracle 和其
Python evaluator；冻结 manifest/result/summary 与 Git 历史未改写。production Cargo graph 没有
Playwright/Node，也没有新增 Provider、Runtime、Store、session store、ledger、Manager/Factory/Service。
未加入 fill、press、wait、snapshot tool、public action、POST/upload/download/auth、登录、持久
Cookie/storage、用户 Chrome profile、截图、坐标、视觉或搜索。

本 treatment 是 credential-free Risk 2 deterministic capability proof：official DeepSeek requests=`0`、
credential read=`false`、actual cost=`$0`。它不做模型 A/B、不形成通用效率或产品指标声明；计费只
是正交状态显示。

<a id="m46-post-w3-browser-interaction-admission"></a>
### M46 post-W3 browser interaction admission contract and result

本 audit 是 credential-free、eval-only repeated-loss gate，不是 `browser_fill` production treatment、
Prompt/route A/B 或产品指标。baseline 固定为 clean W3
`64b83a21b7859e38ca7ce43a4a035c6337702d36`；current control 是 root `Agent` permission 下的 exact
production `web_fetch + browser_navigate + browser_click`。准入前冻结：

1. 恰好两个 task id、不同 independence key 和同一 action family；fill、press、wait 不得混合计数；
2. 两个真实 loopback application 都必须由 JavaScript 产生 exact role/name text-entry target，只有一次
   预注册 fill 才产生 required role/name/state；
3. target name、fill value、post-fill name/state 不得以 literal 出现在 raw HTTP；
4. current W3 必须观察到 target，同时 candidate target refs=`0`、`browser_fill` 不可见，真实 Runtime
   preflight 必须在 side effect 前返回 `UnknownTool + NotApplied`；
5. production control verified=`0/2`、false-success=`0`，Host process/proxy/profile teardown 与真实
   AgentApplication committed navigate/click SQLite reopen regression 必须保持通过；
6. eval-only oracle 每 task 只按 exact role/name fill 一次，只允许 Host-assigned literal-loopback origin
   与 GET/HEAD，输出最多 2 nodes / 4,096 bytes，不使用 screenshot、coordinate、Cookie/storage、认证、
   upload/download、public action 或 arbitrary JavaScript result；
7. official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`；production Rust/Cargo/
   catalog/DeepSeek wire/Prompt/RuntimeEvent/RunStore/State/session delta=`0`，
   `product_metric_eligible=false`。

预注册 manifest 是
`eval/manifests/m46-post-w3-interaction-admission-v1.json`，唯一 evaluator 是
`scripts/eval-m46-post-w3-interaction-admission.py`，test-only root control 是
`crates/tools/tests/m46_post_w3_interaction_admission.rs`；external oracle 使用
`scripts/eval-m46-post-w3-interaction-oracle.cjs` + Playwright `1.61.0` + Node `v24.18.0` + pinned CfT
`151.0.7922.47`。Node/Playwright 不是 production sidecar/dependency。

正式结果：

| task_id | current W3 target | oracle post-fill result | control verified | false success |
|---|---|---|---:|---:|
| `m46_post_w3_release_channel_fill` | `textbox / Release channel` | `status / Release channel set to canary / data-channel=canary` | 0 | 0 |
| `m46_post_w3_test_filter_fill` | `searchbox / Test filter` | `status / Test filter applied: network / data-filter=network` | 0 | 0 |

oracle=`2/2`、current control verified=`0/2`、同一
`tools:browser_interaction:fill=2/2`、control false-success=`0`，Host teardown 与 AgentApplication
reopen regression 均通过。唯一完整正式 result SHA-256 为
`71afb4a6ce866e2e47eb68ad4001c8c586549172c3b414221fedb40b4c76391f`，决定为
`admit_next_goal_ref_based_browser_fill_w3_1_contract_only`。

该决定只冻结下一独立 focused Goal：owner=`crates/tools`，action family=`browser_fill`，只给 eligible
latest-epoch text-entry target 生成 Host opaque ref，schema 只有 bounded `element_ref + value`，action 后
必须 fresh semantic observation 并废弃旧 refs。下一 Goal 必须先固定 stale/missing/hidden/disabled/
readonly/detached/ambiguous ref、password/file/color/date/non-text target、过长/控制字符 value、origin/
method/download/popup/storage/Cookie、external side effect、crash-after-start、authorization/catalog 与
committed-outcome reopen negative gates。

press、wait、click+fill macro、任意 submit、登录/password/secret、Cookie/storage persistence、public
POST/upload/download/auth、用户 profile、截图/坐标/视觉、搜索、Node/Playwright production sidecar、
durable browser session truth、第二 Runtime/Store/ledger 仍未准入。本 audit 没有实现下一 Goal。

完整正式矩阵前有两个 pre-result harness implementation stop：第一次 test-only caller 把 direct
`execute` 误当真实 Runtime `preflight`；第二次在两个 control 后因系统 Python 3.9 不支持非必要的
`zip(strict=True)` 停止。二者均未运行 oracle、没有形成准入结果、没有 DeepSeek/credential/外网行为；
修正并重新冻结 identity 后，完整正式 evaluator result 只形成一次、reruns=`0`。validation、11-case
negative self-test、真实 control/oracle、Host teardown 与 AgentApplication reopen 全部通过。

production Rust、Cargo dependency、fixed catalog、DeepSeek wire/model-visible Prompt、AgentRuntime、
RuntimeEvent、RunStore、State schema、UI 和 browser session delta 均为 `0`。official DeepSeek
requests=`0`、credential read=`false`、actual cost=`$0`；费用只是正交状态，不构成准入原因或工程
收益声明。按 production delta=0 的 eval-only Risk 0 边界只运行 targeted + authority gate，不运行 full。

最终离线门全绿：manifest validation、11/11 negative self-test、Node syntax、test-only Rust compile、
唯一完整 control/oracle/reopen/teardown evaluator、`dse-tools` unit 389 passed / 5 ignored、owner check、
strict Clippy、fmt 与 diff check。authority baseline=`17,636`、ceiling=`4,409`、最大 tools route=
`2,273` 行、fixed boundary=`23/23`。

<a id="m46-browser-fill-w3-1"></a>
### M46 W3.1 ref-based browser fill contract and result

W3.1 只处理上一节已准入的 `tools:browser_interaction:fill` repeated loss。owner 是
`crates/tools`；old path 是 click-only ref registry、15-tool catalog 和 post-W3 eval-only
Python/Node oracle + test-only missing-tool control。production acceptance 固定为：只给 same-run、
latest-epoch、exact-loopback、visible/enabled/non-readonly/non-sensitive `input[type=text|search]` 返回
fill-only opaque ref；唯一 `browser_fill(element_ref, value)` 必须返回 fresh bounded DOM/AX
observation 并旋转全部 refs/epoch；两个冻结 task 的 post-fill state=`2/2`，mandatory negative false
allow=`0`。

输入与 capability contract：

1. schema 只接受 `element_ref + value`，Host preflight 要求 value 非空 UTF-8、最多 1,024 chars /
   4,096 bytes，并拒绝 NUL、C0/C1、DEL 与换行；pure schema/value rejection 不进入 harness；
2. ref 绑定 run、browser、backend DOM node、latest snapshot/page epoch 与 exact click/fill capability；
   click ref 不能 fill，fill ref 不能 click；
3. password/file/date/color/number、textarea/contenteditable、disabled/readonly target 和
   login/secret/token/API-key/credential/OTP identity 不产生 fill ref；
4. action 前重取 DOM/AX/layout identity，拒绝 cross-run、stale、missing、hidden、disabled、readonly、
   detached、ambiguous、identity drift、capability drift；
5. 固定内部 CDP sequence 只有 focus、替换当前值与 insert text；无 selector/CSS/XPath/坐标/任意 JS、
   模型可选 key/Enter/submit/blur、header/Cookie/auth/path；
6. exact-local egress 继续只允许 Host literal-loopback origin 与 GET/HEAD，origin escape、POST、popup/
   worker、download/upload、Cookie/auth 与外部副作用 fail closed；
7. 首次 focus 前失败为 `NotApplied`；focus 后无法闭合 fresh observation 为既有
   `Indeterminate/Unsafe + teardown`，started-without-outcome reopen 必须 `RecoveryRequired` 且 replay=0。

production matrix：

| task_id | initial fill ref target | production fresh post-fill observation | result |
|---|---|---|---:|
| `m46_post_w3_release_channel_fill` | `textbox / Release channel` | `status / Release channel set to canary / data-channel=canary` | 1 |
| `m46_post_w3_test_filter_fill` | `searchbox / Test filter` | `status / Test filter applied: network / data-filter=network` | 1 |

两项均由 repository-pinned CfT `151.0.7922.47`、真实 `ProductionToolExecutor` 与冻结 loopback fixture
执行 `browser_navigate -> browser_fill -> fresh observation`：initial epoch=`1`、post-fill epoch=`2`、
旧 ref 再用得到 `browser_element_ref_stale + NotApplied` 与 epoch=`3` fresh observation。verified=`2/2`，
stale false allow=`0`，输出继续为 `external_untrusted`。

真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent -> SQLite
RunStore` loopback 中，模型选择 navigate 后选择 fill，committed outcome 投影回第三次模型请求；cold
reopen event prefix 相同，navigate/fill call count 保持 `1/1`。started-without-outcome fixture 在
`browser_fill` 的 durable `ToolExecutionStarted` 后 reopen 为 `RecoveryRequired`，model/fill replay=`0`。
现有 ToolOutcome、RuntimeEvent 与 RunStore 足够无损表达，protocol/state schema delta=`0`。

root catalog 可见 fill；coordinator/read-only child 因 MayWrite actor policy 不可见；isolated Writer
虽按既有 write catalog 可见，但 network-denied sandbox 与 cleared local-origin grant 在授权阶段拒绝。
catalog/schema/authorization/direct dispatch/execution identity parity 已由同一 owner fixture 覆盖。

cutover 物理删除 `scripts/eval-m46-post-w3-interaction-admission.py`、
`scripts/eval-m46-post-w3-interaction-oracle.cjs` 与
`crates/tools/tests/m46_post_w3_interaction_admission.rs`；冻结 manifest/summary/fixtures/history 不改写。
production Cargo graph 没有 Node/Playwright，且未加入 press/wait、textarea/contenteditable、登录、public
action、POST/upload/download、Cookie/storage persistence、用户 Chrome、截图/坐标/视觉、搜索、第二
Runtime/Store 或 session ledger。

本 treatment 按 Risk 2 验收；official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`。
没有模型付费 A/B、质量/效率或通用产品指标声明；计费仍只是与行为证据正交的状态显示。

最终 revision 前的 focused gate 全绿：authority baseline=`17,636`、ceiling=`4,409`、最大 owner route
为 tools=`2,325` 行、fixed boundary=`23/23`；`dse-tools`=`392/0/5 ignored`，DeepSeek=`61/0/1
ignored`，Runtime=`88/88`，app=`69/0/3 ignored`，app-server=`23/23`，exec=`30/30`，canonical
TUI=`20/20`，PTY=`7/7`，owner strict Clippy 通过。pinned-CfT W3.1 fixture gate 另按合同显式执行一次为
`1/1`（内部 task matrix=`2/2`）；无 credential、DeepSeek 或外部网络请求。

<a id="engineering-complete-direction-audit"></a>
### ADR-0018 工程完全体方向审计合同与结果

本次是 Risk 0 authority/decision slice，不是新的 browser action、Prompt/route treatment 或产品
成功率 campaign。输入为 clean W3.1 checkpoint、完整 Product Plan、Roadmap current window、
repository-guidance/tools owner route，以及 ADR-0005/0011/0012/0014/0015/0016/0017 的冲突扩读。
production source audit 冻结以下 current facts：

- fixed catalog 为 16 个 Host tools；Web 只有 `web_fetch`、`browser_navigate`、`browser_click`、
  `browser_fill`，没有 `web_search`、press/wait/scroll/select/session/upload/download/visual tool；
- click/fill authorization 与执行要求 Host exact local origin；public navigate 仍 one-shot teardown；
- Ask 下网络因缺少 enforceable scoped grant 而 fail closed；current app composition 对 root Agent/
  FullAccess 使用 broad sandbox posture；
- committed browser outcome reopen 与 started-without-outcome RecoveryRequired 已有确定性证据；
  这些机制不需要重做。

按“可替代完整工程 Agent”的冻结目标，审计结果为 `not_complete_current_product`。原因不是费用
显示或缺少更多测试，而是 unknown-source research、完整 JS interaction、public reversible action、
managed login/session、受控 upload/download、selective visual observation 和跨能力 production
dogfood 都有确定性缺口。当前不能声明可替代 Codex/Claude Code/Cursor 一类完整工程产品。

classification 结果：

| class | result |
|---|---|
| permanent invariants | isolated profile/egress、opaque ref、Host secret、typed side-effect/recovery、fresh observation、latest-revision evidence、bounds/teardown、high-risk confirmation、single Runtime/Event/Store |
| temporary gaps | loopback-only action、click/fill-only、无 public action/login/session/upload/download/search/visual |
| rejected mechanisms | unrestricted eval/JS、unbounded shell/network、secret-to-model、default personal Chrome、unknown side-effect replay、unconfirmed destructive/financial/publish、second Runtime/Store、production sidecar/marketplace |

ADR-0018 因而部分取代 ADR-0015 的长期 blanket deny、one-action/one-Goal 和基线 action repeated-loss
门；W1～W3.1 的实现事实、安全矩阵与 frozen evidence 保持不变。repeated-loss 继续约束昂贵、可选、
架构分叉、高不确定或 model-visible treatment，不再阻止显然属于完整工程 baseline 的 capability
cluster。

首个 production cluster 预注册为 `Semantic Interaction + Public Action Governance`，owner=
`crates/tools`。它用一个 matrix 同时闭合完整 local SPA 交互、用户授权的 disposable public
reversible workflow、external side-effect negative 和 crash/reopen recovery；必须报告 verified
success、false success、人工确认、wall time、Token/request（如有）、返工、恢复、receipt、复杂度
与删除量。旧 per-action admission evaluator、loopback-only action assertion 和 broad-access
workaround 在真实 caller cutover 后删除，不保留 compatibility flag。

本 audit 没有改变 production Rust/Cargo、DeepSeek wire/model-visible Prompt、AgentRuntime、
RuntimeEvent、RunStore、State schema、catalog 或 UI；official DeepSeek requests=`0`、credential
read=`false`、actual cost=`$0`，`product_metric_eligible=false`。费用只是正交状态，不是能力方向或
hold 原因。authority/link/diff 的实际 final gate 结果记录在同一 checkpoint 的 Roadmap/current
architecture，不创建第二 evaluator、roadmap、handoff、tracker 或 frozen evidence rewrite。

<a id="semantic-interaction-public-action-governance"></a>
### Semantic Interaction + Public Action Governance 合同与结果

本次是 ADR-0018 首个 Risk 2 production capability cluster。primary owner=`crates/tools`，真实 caller
composition 只允许 `crates/app`；DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore、
State schema 的预期与实际 delta 均为 0。问题不是“缺一个按钮”，而是 root Agent 无法完成包含文本编辑、
键盘、等待、滚动、选择、多页状态和授权 public reversible action 的完整 Web 工程任务。

冻结 acceptance：

| family | production path | keep condition | actual |
|---|---|---|---:|
| local semantic workflow | navigate → textarea/contenteditable/input → press/select/scroll/wait → back/tab/multi-page | typed actions 全闭合、fresh observation、stale/scope false allow=0 | 1/1 |
| disposable public workflow | public navigate → fill → exact preview → approved reversible draft POST → receipt | exact target/parameters/impact；HTTP 2xx + durable receipt | 1/1 |
| external negative | publish/delete/purchase/send、origin/scope escape、非 exact POST、sensitive/file non-empty | false allow=0 | 0 false allow |
| production caller/reopen | AgentApplication → AgentRuntime → approval → ToolOutcome → SQLite reopen | execute once；reopen network/POST=0 | pass |
| crash recovery | durable started without outcome | RecoveryRequired；model/tool replay=0 | click+fill 2/2 |

fixed catalog 从 16 收敛为 15；`browser_click`/`browser_fill` 被唯一 `browser_interact` 替换。tagged schema
只接受 typed action fields，拒绝 selector/CSS/XPath、坐标、任意 key、JS/eval、header/Cookie/auth/proxy、
Chrome flag 和文件路径。press 只允许 Enter/Escape/Tab/ArrowUp/ArrowDown/Space，且不发送 macOS/Windows
virtual key code。textarea/contenteditable 与普通 input 可获得能力；password/file/login/secret/token/
credential/OTP 不获得敏感 ref。

public permission ladder 的 actual contract：exact-local routine 自动执行；public routine 在 Ask 下询问、
Agent/FullAccess 允许；public reversible draft submit 在 Ask/Agent 下询问、FullAccess 允许；isolated Writer
始终 `actor_controlled_network_denied`。approval 绑定 exact origin/target、canonical non-sensitive params、
impact、invocation 和 workspace revision。其他 submit intent 不产生 capability ref，不靠事后字符串 deny。
敏感/file successful controls 只在实际值为空时由 Host comparison 排除，非空立即拒绝且不进入 durable
preview/log。

真实 repository-pinned CfT `151.0.7922.47` credential-free vertical wall time=`7.21s`，verified task
families=`2/2`、false success=`0`、required approval=`1`、receipt=`draft-receipt-001`（transport header 与
semantic DOM 双观察）。真实 app fixture 的 committed reopen submit reexecution=`0`；started ambiguity
recovery=`2/2`、replay=`0`。official DeepSeek requests=`0`、credential read=`false`、model tokens/cost=`0/$0`；
因此 `product_metric_eligible=false`，不做费用、Token 或通用效率声明，behavior evidence 独立有效。

实际返工关闭四类问题：native virtual key 误路由 macOS UI、CDP backend/front-end node identity 混用、
Host synthetic submit capability 污染 stale fingerprint、以及 HTML successful-controls 与授权参数不一致。
这些问题均由 deterministic vertical 暴露并在同一 owner 内闭合，没有靠降低 stale/egress/receipt gate
取得绿结果。

cutover 删除两个旧 integration path：`crates/tools/tests/m46_browser_fill.rs` 与
`crates/tools/tests/m46_browser_interaction_admission.rs`，由
`crates/tools/tests/semantic_interaction_cluster.rs` 接管；分立 click/fill catalog/schema/dispatch 同时删除。
没有 compatibility flag、新 crate/dependency、Provider、第二 Runtime/Store、browser Agent、Node/Playwright/
Firecrawl sidecar、Manager/Factory/Service、session/accounting ledger，也未改 frozen manifest/raw/history。

remaining gap 只记录为后续 cluster：managed browser login/session、Cookie/storage clear、workspace-granted
upload、isolated/scanned download、canonical search 与 selective visual。它们不被当前 draft POST grant
冒充。最终 revision 的 bounded authority actual 是 bootstrap=`17,636` 行、tools owner route=`2,626/4,409`
行、fixed boundary=`24/24`；focused gate 全绿（tools=`396 passed, 5 ignored`、DeepSeek=`61/1`、
runtime conformance=`88/88`、app=`70/3`、app-server=`23/23`、exec=`30/30`、canonical TUI=`20/20`、
PTY=`7/7`），唯一一次 full pre-integration gate exit=`0`，workspace tools run=`398 passed, 5 ignored`。
`cargo fmt --all -- --check`、`cargo check -p dse-tools --locked`、`git diff --check` 与真实 pinned-CfT
vertical=`1/1 in 7.21s` 均通过。Rust delta=`+4,390/-1,094`，其中旧 integration tests=`-527` 行、
新 cluster test=`+555` 行（test-path net=`+28`），`semantic_browser.rs` 从 `5,170` 行变为 `7,970` 行；
catalog net=`-1`，crate/dependency/protocol/state delta=`0`。full gate invocation=`1`，同一 revision 不重复。

<a id="managed-browser-session"></a>
### Managed Browser Session 合同与结果

这是 ADR-0018 第二个 Risk 2 production capability cluster。primary owner=`crates/tools`，`crates/app` 只做
真实 caller/composition；DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 与 State
schema 的预期和实际 delta 均为 0。

| family | frozen keep condition | actual |
|---|---|---:|
| authenticated vertical | login → clean shutdown → session reuse → authenticated upload/download → promotion → clear | 1/1 |
| credential secrecy | model/outcome/event/SQLite/error 中 secret 或 key literal=0 | 0 leak |
| isolation | personal Chrome/cross-project/cross-origin/cross-actor access=0 | 0 false allow |
| upload | exact workspace file+size+SHA；path/symlink/device/oversize/TOCTOU escape=0 | pass |
| download | quarantine+bound+origin/type/scan；auto-open/execute/overwrite=0 | pass |
| production caller/reopen | exact approvals；committed outcome replay only | 3 approvals；reexecution=0 |
| crash recovery | login/upload/download/promotion/clear started without outcome | 5/5 RecoveryRequired；replay=0 |

repository-pinned CfT `151.0.7922.47` actual wall time=`7.47s`。同一 project profile 在 clean Chrome close 后
复用认证 Cookie；不同 workspace 同 origin 被拒。真实 multipart upload bytes 与授权 SHA 匹配；download
receipt 包含 requested/final URL、trajectory、media、bytes、SHA-256、HTTP/plaintext provenance 与
external_untrusted trust，扫描后显式 no-overwrite promotion bytes 匹配，未自动打开或执行。status 只返回
Cookie/storage 计数，clear 删除 project profile。

真实 app fixture 使用 mock DeepSeek 选择完整 action sequence：model requests=`8`、required approvals=`3`、
committed managed outcomes=`6`；SQLite reopen event prefix 不变，network/filesystem counters 不增。official
DeepSeek requests/tokens/cost=`0/0/$0`，credential read 仅为 deterministic in-memory Host fixture，用户 secret/
DeepSeek key 均未读；`product_metric_eligible=false`，不声明 Token/成本/通用效率提升。

negative matrix 覆盖 credential/log leak、个人 Chrome、cross-project Cookie、origin escape、unauthorized
upload、path/symlink/device/oversize/TOCTOU、download executable/archive/unknown binary、auto-open/execute、
overwrite 与 duplicate external POST，false allow=`0`。committed reopen reexecution=`0`；五类 managed
side-effect started ambiguity 全部由既有 Runtime/RunStore fail closed，证明不需要新 protocol/state/session
store。

cutover 删除 active public-incognito/ephemeral-only session、Cookie stripping、blanket no-login/upload/download
和 `same_run_in_memory_public_origin` assertion。新增直接依赖只复用 workspace 已有 `dse-secrets` 与 `fs2`，
没有 crate acquisition、Provider、第二 Runtime/Store、BrowserManager/Factory/Service、sidecar、session ledger、
personal Chrome、任意 selector/coordinate/JS/header/auth/proxy 或 frozen evidence rewrite。

最终 revision 的 authority actual 是 bootstrap=`17,636`、tools owner route=`2,645/4,409`、fixed
boundary=`24/24`。focused actual：tools=`405 passed, 8 ignored`、DeepSeek=`61/1`、runtime=`88/88`、
app=`72/3`、app-server=`23/23`、exec=`30/30`、canonical TUI=`20/20`、PTY=`7/7`。首个 full candidate
invocation 在行为测试前因 `unnecessary_sort_by` 与 test-only `await_holding_lock` 两个 Clippy finding
exit≠0；两项最小修复后 workspace/all-targets Clippy `-D warnings` 通过。第二个 candidate 的 workspace
suite 暴露 exec 与 HTTP/stdio managed browser state-root identity 不一致；测试 caller 迁移到同一 Host
root 后 targeted surface parity=`2/2`。第三个 candidate 暴露既有 child-env test 对进程全局 `PATH` 的并行
污染；测试改为使用真实 parent PATH 后 targeted child-env/application-probe=`11/11`。final revision 的
唯一 full invocation exit=`0`；总 invocation=`4`（failed candidates=`3`、final revision=`1`），同一
revision 没有重跑。
`cargo fmt --all -- --check`、`cargo check -p dse-tools --locked`、`cargo test -p dse-tools --locked`、
`git diff --check` 与真实 pinned-CfT vertical=`1/1 in 7.47s` 均通过。
production Rust/Cargo delta=`+4,095/-169`，docs delta=`+162/-22`；`semantic_browser.rs` 从 `7,970` 行变为
`11,173` 行。direct dependency edge=`+2`，但两者均为 workspace/lock 已有依赖，无新 acquisition。

<a id="canonical-search-observation-quality"></a>
### Canonical Web Search + Semantic Observation Quality 合同与结果

这是 ADR-0019 的 Risk 2 production capability cluster。真实问题是：root Agent 此前只能读取已知 URL，
不能从未知工程问题发现来源；browser 的 first-eligible-N observation 又可能让前部导航噪音挤掉任务相关
节点。primary owner=`crates/tools`，`crates/app` 只做 production caller/composition；old path 是 M44 已删除但
未被真实 search 替代的 provider 管理面，以及 AX/DOM extractor 的机械前 N 截断。

#### Frozen acceptance matrix

| family | keep condition | pre-integration actual |
|---|---|---:|
| canonical search | 一个 schema/dispatch/fixed network identity；无 provider/header/credential/proxy 参数 | pass；catalog 15→16 |
| bounds/failure | query/result/response/deadline/DNS/HTTP/content/JSON/request-id typed fail closed | pass；false allow=0 |
| research route | search→选择两个独立来源→fetch 原文→cross-check→URL citation | 1/1 deterministic production vertical |
| evidence role | snippet/rank 不冒充事实；source content 与 citations 对齐 | fetched/cited sources=2/2；unsupported claim=0 |
| authorization | Ask exact-query approval；Agent/FullAccess allow；isolated Writer deny | pass；rule=`canonical_public_web_search` |
| replay/recovery | committed SQLite reopen network=0；started ambiguity 不重复 provider request | reopen reexecution=0；RecoveryRequired=1/1 |
| observation recall | late task cue 在 node bound 内保留；报告 recall/truncation/prompt injection | synthetic late-target recall=10,000 bps；focus loss=false |
| real browser quality | pinned Chrome focus recall 与每次 action 后 bounded semantic diff | 1/1 in 7.01s；recall=10,000 bps；3/3 fill diff non-empty and <16 KiB |
| blind spots | 显式报告 AX-only、DOM interactive without AX、canvas/SVG candidates | fields present；不冒充 visual success |
| false success | HTTP/tool success、snippet、metric 或 diff 不单独完成任务 | 0 in frozen fixtures |

#### 实际 production caller、accounting 与复杂度

真实 production-path fixture 经
`AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent -> SQLite RunStore`
执行 4 个 mock-model turns：模型先选择一次 `web_search`，再读取 `example.com` 与 `example.org` 两个独立
public HTTPS 原来源，最后只对已读取 URL 形成 citation。search calls=`1`、DNS/fetch=`2/2`、committed
search/fetch outcomes=`1/2`；cold reopen 的 model/search/DNS/HTTP calls 全部为 0，event stream exact replay。
这证明 vertical wiring/replay，不证明真实 DeepSeek 的检索策略或通用研究成功率。

search response 记录 provider request identity、`usage.credits` 与
`billing_truth=provider_usage_units_only_actual_charge_unavailable`。fixture credit=`1` 只是 contract fixture，
不是实际账单。official DeepSeek requests/tokens/cost=`0/0/$0`；没有付费 A/B，也不做 Token、费用或通用
效率声明。环境与现有 Host secret store 的 exact-key presence check 均为 unavailable，未读取 credential
value；因此真实 provider canary=`not_run`、requests=`0`、reruns=`0`、actual credits/charge=`0/unknown`。
accounting 不完整按 ADR-0011 只阻止精确成本声明与下一次付费请求，不抹掉 deterministic caller/reopen
behavior。

observation 的 unit fixture 将 8 个前置导航节点、1 个 prompt-injection signal 和最后 1 个 task target 放入
`max_nodes=2`：旧 first-N 会丢 target，当前 treatment 保留 target，recall=`10,000 bps`、truncation-caused
focus loss=`false`、injection exposure read/returned=`1/0`。这只证明该 frozen fixture，不宣称一般网页
prompt-injection 防御率。ref-independent diff fixture 的 changed/unchanged/added/removed=`1/1/1/0`、stale
ratio=`5,000 bps`、entries=`2`、bytes `<4 KiB`。真实 pinned Chrome complete interaction vertical 为
`1/1 in 7.01s`；每个 fill 后 fresh diff 都有 semantic change 且 `<16 KiB`。

Rust source/test delta=`+2,087/-53`，其中新 `web_search.rs`=`1,042` 行、
`semantic_browser.rs` 从 `11,173` 增至 `11,651`；无 Cargo/dependency/crate、protocol/state、DeepSeek
wire/model-visible Prompt、AgentRuntime、RuntimeEvent 或 RunStore delta。删除 active first-eligible-N loop，
未恢复 M44 的 `[search]`/`DSE_SEARCH_*`/Doctor provider selector，也未加入 SearchManager/Factory/Service、
fallback Provider、search HTML scraper、Node/Playwright sidecar、visual stub、session/store/accounting ledger。

实际返工关闭三类 parity/accounting 缺陷：execution identity schema 的两条旧 v6 assertion、六个 actual
actor catalog hash，以及取消/timeout 后 provider charge 未知却标为 safe retry。最终 cancellation 立即返回
`Cancelled + transport Indeterminate + retry Unsafe`；没有为通过测试降低 catalog/authorization/replay 门。

bounded authority actual：bootstrap=`17,636`、tools route=`2,814/4,409`、fixed boundary=`25/25`。focused
exit=`0`：tools=`413 passed, 8 ignored`、DeepSeek=`61/1`、runtime conformance=`88/0`、app=`74/3`、
app-server=`23/0`、exec=`30/0`、canonical TUI=`20/0`、PTY=`7/0`。最终 revision 的 canonical full gate
invocation=`1`、exit=`0`，覆盖 authority/public、fmt、workspace all-features check/strict Clippy、全
workspace tests 与 doctests；同一 revision 没有第二次 full。clean reviewable checkpoint 随本条形成。

<a id="internal-alpha-integration-checkpoint"></a>
### Canonical Web Search 后 Internal Alpha Integration Checkpoint 合同与结果

这是 ADR-0018 integration/dogfood 水平门的 Risk 4 checkpoint，不是新工具或通用 benchmark。问题是各
capability 单独通过不能证明 root Agent 能完成跨 code/app/Web/Writer/recovery 的同一真实任务。primary
owner=`crates/app`；`crates/tools` 只修复 task evidence 暴露的 isolated Writer Host-loopback verifier
阻断。old evidence path 是把分散 fixture green 当成产品闭环；old execution path 是把 external egress deny
重复用于 Host-owned exact loopback probe。

#### Frozen matrix 与 deterministic actual

| family | keep condition | actual |
|---|---|---:|
| independent tasks | 至少 3 个不同 code shape，不以重复 seed 冒充 family | constant/function/mapping=`3/3` |
| Web research | 每项 search 1 次、fetch 两个独立原来源、completion 引用 2 URL | search=`3/3`；fetch/cite=`6/6` |
| code/build/app | Writer 只改 `server.py`；Host compile 后启动 app | byte-exact=`3/3`；compile/start=`6/6` |
| Writer convergence | child receipt→seal→integration→root latest receipt→cleanup | lifecycle=`3/3`；receipts=`6/6` |
| completion truth | 未经 integrated latest-revision receipt 不得 Completed | false success=`0` |
| replay/recovery | terminal reopen 不再触发 model/Web/build/probe/Writer；既有 ambiguity/SIGKILL/checkpoint 保持 | reexecution=`0`；focused pass |
| isolation | Writer external egress=0；Host probe 仅 localhost bind/inbound | outbound false allow=`0` |
| complexity | 不新增 capability/runtime/store/provider/dependency；删除错误重复 gate | pass |

每个 task 的 fixture model requests=`7`、input/output=`960/76`、rework=`0`；加入 compile/build 后三项并行
suite wall=`16.52s`，单项约 `16.396–16.509s`。root HEAD 前进、Git clean、writer branch/worktree 清零，cold reopen events
exact replay。该 Token/时间只属于 deterministic fixture，不与历史 treatment 做效率比较，也不宣称真实
DeepSeek 的通用 success。

#### 最小 integration fix 与安全反例

开发期首个真实 task 正确地终止为 Blocked：Writer `apply_patch` 已成功，但 child verifier 返回
`application_probe_network_denied`，root 随后在旧 revision 得到 `application_probe_body_mismatch`；没有
false completion。修复后 ordinary no-network policy 仍在 spawn 前拒绝，isolated Writer 仅派生
`host_loopback_only=true`。Seatbelt fixture 明确包含
`network-bind/network-inbound (local ip "localhost:*")` 且不含 `network-outbound` 或 broad bind；默认 Writer
serialization 继续省略该字段，probe identity 则显式区分。Linux bwrap 保持 isolated namespace 与同一
worktree/protected-path policy。模型不能选择该 treatment，只有 Host verification path 能派生。

没有改变 catalog、authorization owner、ToolOutcome、RuntimeEvent、RunStore/State schema、AgentRuntime、
DeepSeek wire/model-visible Prompt；没有新增 Manager/Factory/Service、session/ledger、Provider、浏览器、视觉、
登录、UI 或 dependency。test harness delta 主要位于现有 app production test module；production Rust 只为
loopback sandbox distinction 与 exact policy mapping 增加小幅 delta。被删除的是 isolated Writer
`application_probe` 的 blanket deny，不是 Writer 的 Web/shell external-network deny。Rust total delta=
`+938/-23`，其中 app `#[cfg(test)]` module=`+825/-6`；docs=`+127/-4`、Cargo=`0`。bounded authority=
`17,636` bootstrap、tools owner route=`2,825/4,409`、fixed boundary=`25/25`。

#### Official dogfood 与 accounting truth

首个 official DeepSeek dogfood 预注册 one run、physical request limit=`8`、runtime retry=`0`、rerun=`0`、
ceiling=`$0.10`。实际 terminal=`Failed(OutputLimit)`，physical requests=`4`、runtime retries=`0`、wall=
`23.636s`、usage complete=`true`、billing unknown=`false`、input=`13,141`、output=`1,145`、cost=
`3,730,821 nanousd`（约 `$0.00373`）。失败发生在 canary 人为 512 output-token cap；该 treatment 没有重跑。

用户随后显式授权一个 fresh 2,048-token successor，仍为 physical request limit=`8`、runtime retry=`0`、
new-treatment rerun=`0`、ceiling=`$0.10`，并继续使用 deterministic Host Web fixture。实际在第 6 个 physical
request 后 terminal=`Failed(ToolBudgetExceeded { limit: 6 })`，wall=`44.536s`、usage complete=`true`、
billing unknown=`false`、input/output=`21,015/2,212`、cost=`6,538,253 nanousd`（约 `$0.00654`）；没有
再运行。该结果证明 2,048 output cap 已越过首个 failure，但 canary 的人为 tool-call budget 仍不足以闭合
official vertical。`max_tool_calls=16` 只在下一份 fresh 显式授权后作为独立 treatment 执行，结果如下。

用户再次显式授权一个 fresh 2,048-token / 16-tool successor，physical request limit=`8`、runtime retry=`0`、
new-treatment rerun=`0`、ceiling=`$0.10`。实际 terminal=`Blocked(Host application_probe deterministic
failure)`，physical requests=`8`、runtime retries=`0`、wall=`68.861s`、usage complete=`true`、billing
unknown=`false`、input/output=`26,349/4,473`、cost=`9,334,781 nanousd`（约 `$0.00933`）；没有再运行。
它证明 output/tool 两个旧上限已越过，但 deterministic happy path 本就需要 7 个 model requests，真实模型
到 Host verifier failure 时已在第 8 个请求耗尽 recovery budget，无法再完成“有效修改→新 revision→复验”。
Host 没有把失败降格为成功。future ignored harness 只离线加入失败前的 root tool/fixture/marker diagnostics，
并将未执行的 recovery candidate 调整为 12 model/API requests、24 tools、runtime retries=`0`；仍需 fresh
显式授权且 rerun=`0`。三次 closed accounting 合计约 `$0.01960`，仍没有 official vertical success。
故本条只 keep deterministic production integration 与 sandbox fix，不声明真实 Tavily success 或任何通用
success/Token/time/cost 优势。`TAVILY_API_KEY` unavailable，live provider requests/charge=`0/unknown`。

focused exit=`0`：tools=`410 passed, 6 ignored`、DeepSeek=`61/1`、runtime conformance=`88/88`、app=
`77 passed, 4 ignored`、app-server=`23/23`、exec=`30/30`、canonical TUI=`20/20`、PTY=`7/7`。最终 revision
canonical full invocation=`1`、exit=`0`，覆盖 public/authority、fmt、strict workspace/all-target Clippy、
workspace tests/doctests 与 diff check；同一 revision 不重跑。
