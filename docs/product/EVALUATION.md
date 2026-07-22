# DeepSeek Agent 评测规范

> 文档类别：产品权威。仅定义能力的验证与保留门槛。

- 状态：V1 评测契约
- 上次更新：2026-07-22

本文件决定一项能力是否真正提升产品。它不是排行榜，也不以“模型回答看起来不错”
作为结论。

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

涉及多 Agent、RepoGraph、FIM、Strict 或 compaction 的改动还必须有关闭该能力的
A/B 对照。

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

### G. 固定简体中文产品契约

固定中文是产品验收，不作为 Agent 能力提升的 A/B treatment。候选至少验证：

- 首次启动、API Key、信任、canonical 命令、审批、取消、失败、恢复和终态使用简体中文；
- 80/120 列终端的 CJK 宽度、截断、换行和鼠标/键盘命中保持正确；
- 只有一个 `zh-Hans` 消息目录，不存在 Locale 状态、环境检测、语言选择或运行时切换；
- 不存在 `/translate` 或思考内容后处理翻译产生的额外模型请求；
- JSON/NDJSON、Schema、命令、工具名、模型 ID、配置键、路径、代码、diff 与原始输出保持
  机器契约或原始字节；
- 被删除命令的 PTY 验收应证明中文拒绝且没有模型请求，不能恢复旧兼容来满足旧测试。

生产 Agent 提示词的中文重构属于独立 treatment，仍需按本文件记录同任务成功率、Token、
时间和成本；UI 中文验收不能替代该 A/B。

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
State schema v12；当前 v9/v14/v19 继续保留该语义。TaskContract 在 `RunCreated` 冻结，
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
  `codewhale.exec-stream` v1 并未在每条展示事件上承诺这些字段，必须按至少一次输出消费，
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
7. 有关闭能力的 A/B 对照；
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

## 10. 结果与决策记录

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
