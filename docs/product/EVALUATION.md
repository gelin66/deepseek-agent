# DeepSeek Agent 评测规范

> 文档类别：产品权威。仅定义能力的验证与保留门槛。

- 状态：V1 评测契约
- 上次更新：2026-07-18

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
- 自动 compaction 在未达到硬上下文限制时不得消耗最后的最终请求许可；
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
compaction strategy A/B
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
context_compaction_trigger
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

- 显式提供 baseline/candidate 二进制及彼此不同的 revision，不由 Harness 自动 checkout；
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
在 M5 完成同任务、同模型、同预算、同工具面和每 cell 至少 3 次的 compaction on/off A/B
前，不得声称它减少 Token/成本、缩短时间或提升任务成功率；摘要是否保留 TaskContract、
当前 diff 和最新 evidence 也必须由预定义断言或 verifier 验证，不能由摘要模型自评。

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
  `ContextCompactionPrepared`、`ContextCompactionInFlight`、compaction committed 后尚未发布、
  `interaction_requested`、`interaction_resolved_before_tool_start`、
  `steer_queued_before_applied`、`steer_applied_before_next_model`、
  `control_requested_before_terminal`、普通事件提交后未发布，以及 canonical terminal 提交后
  未发布等窗口；每个窗口分别报告预期与实际的 request、usage、事件和副作用增量。
- `command_id` 与 `interaction_id` 是当前 crash trigger 的 nullable ID；涉及多个命令或交互
  时必须保存完整 ID 集合或等价规范化摘要。`command_payload_digest` 必须绑定命令类型和
  payload，用于证明同 ID 同 payload 幂等、同 ID 不同 payload 被拒绝。
- start/continue/compact 的 creation command 还必须记录
  `creation_command_sha256 + reserved_run_id`：同 command ID 同 payload 的并发或崩溃重试
  只能观察到一个 reserved/created run ID，不同 payload 必须拒绝。若预运行模型请求可能已
  发出而无法安全完成创建，必须保留 reservation 并 typed fail closed，不能另建 run 掩盖
  歧义。
- continuation 的 source event prefix、terminal、accounting 和 transcript digest 在创建
  前后必须不变；新 run 的 `parent_run_id` 为空且 `continued_from_run_id` 精确指向 source。
  `resume` 的恢复证据仍必须是 `same_run=true`，不能用 continuation 代替。
- compaction 必须记录 trigger、source projection digest、source entry count、前后 Token
  估算、摘要请求增量与 committed projection digest。prepared 尚未 in-flight 可按同一
  compaction/attempt 恢复；in-flight 后 request/usage 不确定时必须
  `unknown_billing=true` 并进入 `RecoveryRequired`，不能盲目重发。
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

1. 在目标任务组中提升 verified success，或在成功率不退化时显著降低成本/时间；
2. 不增加 false-success；
3. 默认用户流程不变复杂；
4. 失败模式明确且可恢复；
5. 没有产生重复 Runtime、Store、Task 或产品概念；
6. 生产复杂度与收益成比例；
7. 有关闭能力的 A/B 对照；
8. 有回归测试和删除方案。

若收益只存在于极少任务，应作为按需策略，而不是默认全局行为。

## 8. 当前 `verify` 实验的判定

当前 WIP 中的独立 `verify` 工具使用额外 DeepSeek 调用评审 bounded diff/file evidence。
它不等于 test/verifier receipt，必须单独评估：

- 是否提高真实缺陷发现率；
- 是否降低 false-success；
- 是否只是重复主 Agent 的判断；
- 成本和延迟；
- 是否应默认关闭、按需启用、缩小或删除。

在该实验通过本规范前，不得让它成为所有任务的强制完成阶段。

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
