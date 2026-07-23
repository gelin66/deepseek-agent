# DeepSeek Agent 评测规范

> 文档类别：产品权威。仅定义能力的验证与保留门槛。

- 状态：V1 评测契约
- 上次更新：2026-07-23

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
