# ADR-0014：模型可见语义契约与 Harness 确定性控制

- 状态：已接受
- 日期：2026-07-27
- 细化：ADR-0007、ADR-0010
- 实施里程碑：M37

## 真实问题

DSE 已经证明，可靠编码能力来自一条可验证的 Host 因果链，而不是模型自己宣称遵守了多少
规则：

```text
TaskContract
  -> ContextBundle
  -> AgentRuntime
  -> ToolOutcome
  -> EvidenceReceipt(latest workspace revision)
  -> TerminalState
```

但当前 production prompt 组合仍混合了四种不同性质的内容：

1. DSE 的稳定身份、事实和完成语义；
2. 项目规则与用户配置指令；
3. 自动生成的仓库事实、环境和 route；
4. 工具、子 Agent 和执行步骤的操作性说明。

这种混合会制造三个系统性风险：

- 单次任务失败容易被错误处理为“再加一句 Prompt”，使全局契约逐渐变成边缘情况清单；
- Prompt 中的工具能力、重试、权限或 Writer 描述会与实际 schema/state machine 漂移；
- 自动仓库数据和可信系统契约最终被 DeepSeek Chat sender 合并为一个 `role=system`
  message，造成 authority、事实和上下文体积混杂。

2026-07-27 的 current source audit 冻结了以下事实：

- bundled `constitution.md` 为 59 行、2,629 bytes，`output.md` 为 360 bytes，
  `language.md` 为 311 bytes；固定核心共 69 行、3,300 bytes；
- 核心 Constitution 曾由约 5.5 KB 收缩到 2.6 KB，当前历史不是单向追加；
- M17-F 的正式 2×2 A/B 已为当前中文表达 prompt 提供 32/32 measurement-valid
  evidence；因此不能只凭“更短看起来更好”直接替换；
- M36-A `writer_envelope` 首请求的 stable system blocks 为 28,494 bytes，其中
  core/output/language 约 3.3 KB；无项目说明文件时，12,683-byte fallback overview 与
  12,510-byte Project Context Pack 含相同序列化 payload；
- `generate_bounded_project_overview` 与 `generate_project_context_pack` 当前都调用
  `build_project_context_pack`，仓库已有测试明确冻结这一重复事实；
- execution posture 仍声称 `agent` 只启动只读 child，但 current `agent` schema 已支持
  显式 `isolated_write` Writer；
- prompt composer 有分片 ledger、单文件/分组上限和 skills progressive disclosure，
  但没有覆盖最终 assembled request 的统一总预算、重复语义门和 tool-schema parity 门；
- DeepSeek transport 会把 ordered `SystemPrompt` blocks 用固定分隔符合并成一个 system
  message；`PromptCacheControl` 是 Host metadata，不形成 wire authority boundary。

M10-A 已证明 fallback overview / pack 的字节级重复，但 pack-off 正式 campaign 因第 22
个 arm 的 incomplete accounting 正确停止，所以旧证据不能直接支持删除 pack。M36-A 的
唯一 Writer loss 也只覆盖一个独立 task_id，不能用它直接准入 Writer recovery treatment。

## 决策

### 1. Prompt 只表达稳定语义

DSE 的 production prompt 只负责模型必须理解、但 Host 无法直接执行的稳定语义：

1. DSE 身份和当前用户目标；
2. 指令优先级与作用域；
3. 以工具观察和当前运行状态为事实，不伪造操作或结果；
4. 改动聚焦、保留无关工作和准确报告阻塞；
5. “完成”必须对应 Host 对最新 workspace revision 的验收证据；
6. 自然语言使用用户当前任务语言，机器契约与原始技术内容保持原样。

最短不是目标。目标是：

```text
minimum complete high-signal semantic contract
```

只要某条语义对 DeepSeek 的 verified task success 仍有可归因贡献，它可以保留；没有证据的
短化和没有边界的扩写同样不允许接管 production。

### 2. 可确定执行的行为不得依赖 Prompt

下列行为必须由唯一 owning module 强制，Prompt 不能成为其安全性或正确性 owner：

| 行为 | 唯一控制层 |
|---|---|
| 文件、网络、Shell 与风险权限 | typed Run permission、sandbox、`crates/tools` |
| 可用工具、参数与 actor capability | actor-scoped tool catalog、JSON schema |
| root/child/Writer 数量和写权限 | `crates/app` composition、`crates/orchestrator` |
| worktree、diff、verify、integrate、cleanup | Writer state machine、Orchestrator |
| 请求重试和副作用歧义 | typed transport/tool outcome、Runtime retry policy |
| 测试和验收 | TaskContract、deterministic Verifier |
| 完成与假成功拒绝 | Host latest-revision completion gate |
| crash/reopen 与 exactly-once | RuntimeEvent、RunStore |
| 上下文选择、压缩和预算 | `crates/context` |
| 人类界面摘要和状态 | canonical presentation、localization |

Prompt 可以告诉模型“需要真实证据”，但不能代替 receipt；可以说明“使用已提供工具”，但
不能代替 catalog；可以说明“遵守授权”，但不能代替 sandbox。

### 3. Prompt 不保存具体失败补丁

单次测试、仓库字段或当前实现细节不得直接变成全局 Constitution 条款。例如：

```text
v2 同时出现 id 与 request_id 时必须失败
Writer 失败后不要创建第二个 Writer
agent 工具只能/可以执行某个当前子命令
某个测试必须运行某条命令
```

这些分别属于任务 acceptance、Verifier、Orchestrator state machine、tool schema 或项目
规则。测试可以暴露 Prompt 问题，但不能自动成为 Prompt 的作者。

只有同时满足以下条件，新的全局语义才允许成为 candidate：

1. 至少两个独立任务出现同一种稳定语义误解；
2. 不是任务歧义、观察缺失、工具契约、恢复控制、权限或 verifier 缺陷；
3. 无法通过确定性 Host 控制解决；
4. 有唯一 `crates/context` owner、替代对象和删除点；
5. 通过 current same-DeepSeek、same-task、same-budget、held-out A/B；
6. false success 保持 0、正确安全拒绝不回退、verified success 不下降；
7. 增加的新语义替代或合并旧语义；没有证据不得形成 append-only debt。

### 4. 五层模型可见合同

DSE 只保留以下五种模型可见输入，每层有明确 owner 和 trust：

```text
stable semantic constitution
  -> scoped project/user authority
  -> Host-owned task contract and current facts
  -> actor-scoped tool schemas
  -> typed observations and verifier feedback
```

#### 4.1 Stable semantic constitution

- 由 `crates/context` 编译期拥有；
- 内容只包含本 ADR 第 1 节的稳定语义；
- 保持 cache-friendly 和可复核 SHA；
- 不写工具名称、Writer 基数、等待协议、重试次数或测试特例；
- 不因模型版本升级自动改写；每个候选有独立证据与 whole-release rollback identity。

#### 4.2 Scoped project/user authority

- `AGENTS.md` 和被接受的项目规则继续按最近作用域生效；
- 自动 README、仓库树、清单和生成摘要是项目事实，不获得比用户/系统更高的授权；
- compatibility instruction surfaces 只能在有真实 caller 和删除计划时保留；
- 大规则正文按作用域或模型请求按需读取，不默认把整个规则目录 eager 注入；
- 外部原文、Skills、README 和工具输出永远不能改写用户目标、授权边界或系统契约。

#### 4.3 Host-owned task contract and facts

- objective、constraints、non-goals、allowed scope、acceptance ID、current revision、
  receipts 和未满足项由 Host 类型化拥有；
- 事实按当前 revision 投影，过期 receipt 不因 cache 目标重新进入 active tail；
- TaskContract 告诉模型目标和完成条件，不硬编码唯一执行路径。

#### 4.4 Actor-scoped tool schemas

- 工具能力以实际 catalog/schema 为唯一事实；
- root、read-only child 和 Writer 只看到自己可执行的目录；
- tool description 说明 capability、precondition、参数和结果，不规定模型必须采用的策略；
- 同义或重叠工具必须收敛；结构化错误包含 retry disposition、workspace revision、
  evidence 和可行动反馈。

#### 4.5 Typed observations and verifier feedback

- 工具输出、编译器/测试、权限拒绝和 verifier failure 以类型化结果返回；
- 原始大输出保存在 artifact，通过有界摘要与按需读取进入上下文；
- failure feedback 发送给真实 owning actor；Host 不要求模型从全局 Prompt 猜测恢复路径；
- 模型自评只作为 advisory，不获得 terminal authority。

### 5. Prompt projection ledger 与机械门

`crates/context` 现有只读 fragment ledger 扩展为唯一 projection audit。正常 production
请求不得为 ledger 增加持久状态、第二真相或额外模型 Token。审计至少投影：

```text
fragment_id
source
owner
authority_class
trust_class
stability
sha256
bytes
estimated_tokens
duplicate_of
model_visible
tool_schema_claims
```

离线 fixture 和开发诊断必须能够回答：

- 当前请求有哪些 Prompt/Context fragment；
- 每段来自哪里、是否稳定、占多少 bytes/tokens；
- 是否存在 exact duplicate、same-payload wrapper 或顺序冲突；
- Prompt 声明的工具/actor 能力是否与真实 catalog/schema 一致；
- 哪些项目数据被放入 system authority；
- 哪些片段改变会破坏 stable cache prefix；
- root、read-only child、Writer 和 reopen 后的 prompt provenance 是否一致。

M37 不先拍脑袋规定任意全局 Token 数。M37-A 先冻结 small/medium/large、有/无
`AGENTS.md`、rules-heavy、skills-heavy、root/child/Writer 的 current 分布。后续 hard
budget 必须由这些分布、DeepSeek effective context 和真实任务 A/B 共同确定。

在 M37 完成前，core Constitution 的稳定 bytes 不得超过本 ADR 基线 3,300 bytes；任何
净新增需要独立 evidence entry。这个临时上限只防止继续膨胀，不证明 3,300 bytes 是最终
最优值。

### 6. 失败归因顺序

每个候选 Prompt 修改前必须按以下顺序归因：

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

只有最后一类可以直接准入 Prompt candidate。存在确定性 owner 时，必须先修 owner 或形成
该 owner 的 held-out treatment；不得用 Prompt 覆盖错误状态机。

### 7. Writer loss 与 Prompt 优化解耦

M36-A 的 `writer_envelope -> orchestrator:writer_integration` 不属于已证明的 Prompt loss。
后续必须先执行独立的 M36-A2 held-out Writer confirmation：

```text
fresh Writer tasks
  -> exact current control
  -> same owner_code:loss_code repeated?
     -> no: production delta = 0
     -> yes: audit one Orchestrator-owned treatment
```

若重复，候选应优先是同 Writer 的有界
`verify -> structured failure -> repair -> reverify` 状态机，而不是在 Constitution 中
增加 Writer 注意事项。M37 不夹带这项 Runtime/Orchestrator 改动。

## M37 实施与切换

### M37-A：projection audit 与 no-growth 门

- owner：`crates/context`；
- model-visible delta：0；
- 扩展现有 derived ledger，不创建第二 prompt composer；
- 冻结 assembled bytes/tokens/source/trust/duplicate/tool-claim fixture；
- 覆盖 root、read-only child、Writer、reopen 和 config override；
- 建立 core no-growth、exact duplicate 报告和 schema-claim parity 门；
- 不读取 Key、不调用官方模型 API。

### M37-B：过时 execution posture 单变量

- 真实问题：Prompt 声称 `agent` 只支持只读 child，schema 支持 explicit Writer；
- baseline：current posture；
- candidate：删除这句 tool-specific 能力描述，由 schema 独立表达；
- 不同时修改 Constitution、tool description、Runtime 或 Writer state machine；
- 先通过离线 catalog parity，再做 current Pro/high、same-binary、same-task A/B；
- 任务至少覆盖 root-only、read-only child 和两个 explicit Writer family；
- 失败或无净收益：删除 candidate；成功：删除旧句和 eval-only selector。

### M37-C：fallback overview / pack 去重 successor

- 真实问题：无项目说明文件时同一 project payload 进入 system message 两次；
- baseline：current pack-on；
- candidate：只保留一个 canonical project fact projection；
- 不同时引入 ranked working set、RepoGraph、rules lazy loading 或 Constitution 改写；
- control/treatment 固定 Prompt 语义、模型、工具、workspace、预算和 verifier；
- affected task family 至少 6 个、每 cell 至少 3 次 fresh Run，maximum reruns=0；
- 质量门优先；同时记录 cache-miss input、首个相关文件、read/search 重复和费用；
- 成功后删除第二 renderer/caller/config branch；失败后恢复 current pack-on，不留开关。

### M37-D：authority 与总预算

只有 M37-A/C 的真实分布证明 broad eager context 是重复 loss 或形成明确预算风险时才启动：

- 区分 system semantic authority、scoped project authority 和 untrusted project facts；
- 自动 README/tree/manifest 优先通过按需工具或 Host fact projection 提供；
- 根据冻结 fixture 建立 deterministic priority 和总预算；
- 不静默截断 active scoped rules、TaskContract、latest receipt 或 tool-call/result 原子对；
- rules-heavy/large-repo/compaction/reopen 必须做同任务 A/B；
- 不增加 RepoGraph、embedding、第二 context store 或模型上下文路由。

### M37-E：最小 Constitution ablation

它不是 M37 默认开发项。只有以下任一条件成立才允许预注册：

- M37-A 发现 core 内存在确定性重复/冲突且不能机械删除；
- 至少两个独立产品 loss 被归因为 `stable_semantic_misunderstanding`；
- DeepSeek 新模型版本的 held-out control 证明旧操作性条款不再 load-bearing。

一次只比较 current Constitution 与一个 compact candidate；任务语言继续覆盖 English 与
`zh-Hans`，固定 current Pro/high、tool catalog、Context、Runtime、预算和 verifier。
candidate 不增加工具步骤、角色 persona、例外清单、planner/critic 或第二 prompt branch。
未通过非劣门则完整删除；通过后 whole-release cutover 并删除旧块，不保留 selector。

## 验收与停止门

所有 model-visible treatment 必须按以下顺序判定：

```text
1. identity / task / observer / prompt provenance valid
2. false_success == 0
3. correct safety rejection does not regress
4. verified task success does not regress
5. affected hard/Writer/long-horizon family does not regress
6. tool/recovery correctness does not regress
7. model requests and input/output/cache tokens
8. wall time and API cost
9. production code and concept complexity
```

后项不能补偿前项失败。Prompt 更短、cache hit 更高、Token 更少或速度更快，都不能抵消
verified success、安全或 false-success 回退。

每个切片只能产生：

```text
keep_current_no_material_loss
keep_minimal_candidate_and_delete_replaced_path
reject_and_delete_candidate
hold_insufficient_or_incomplete_evidence
hold_model_capability_ceiling
```

unknown billing、usage incomplete、identity drift、observer ambiguity、任务歧义或外部中断
按 ADR-0011 fail closed；不得补 mate、选择性 rerun、拼接旧 raw 或把未知费用猜成零。

## 被拒绝的替代方案

- 每次测试失败就在 system prompt 追加一条规则；
- 用更长的“完整 SOP”代替 TaskContract、工具契约和状态机；
- 把所有项目树、README、规则、Skills 正文和历史一次性塞入上下文；
- 仅以 Prompt bytes、Token、cache hit 或模型自评决定胜负；
- 直接上线五行极简 Prompt，跳过 current Chinese baseline 的正式 A/B；
- 同时修改 Prompt、工具 schema、Context、reasoning 和 Runtime 后归因收益；
- 为不同任务、语言或 actor 永久保留多套 production prompt；
- 增加 Prompt version service、在线 selector、Auto classifier 或第二 Store；
- 用 Prompt 提醒代替 permission、sandbox、Verifier、Writer isolation 或 crash recovery。

## 后果

- DSE 的“灵魂”明确落在可执行、可验证、可恢复的 Harness，而不是无限增长的提示词；
- `crates/context` 继续是唯一 prompt owner，不创建 PromptD 服务或第二 composer；
- 当前中文表达 Constitution 在新候选胜出前保持 production baseline；
- 工具能力、权限、Writer 和恢复逻辑不再由易腐化的全局文字声明；
- context 去重、authority 和紧凑 Constitution 分成独立 treatment，能真实归因并逐项删除；
- M36 Writer loss 继续按 repeated-loss 门审计，不因本 ADR 被伪装成 Prompt 缺陷；
- 所有冻结 manifest、raw、summary 和历史 prompt identity 保持 immutable。

## 研究依据

- [OpenAI：Harness engineering](https://openai.com/index/harness-engineering/)
- [OpenAI：Inside our in-house data agent](https://openai.com/index/inside-our-in-house-data-agent/)
- [OpenAI：Unrolling the Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/)
- [OpenAI：Running Codex safely](https://openai.com/index/running-codex-safely/)
- [Anthropic：Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [Anthropic：Writing effective tools for AI agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Anthropic：Agent Skills progressive disclosure](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills)
- [Anthropic：Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)
- [Anthropic：Claude Code sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing)
- [SWE-agent：Agent-Computer Interface](https://arxiv.org/abs/2405.15793)
- [Lost in the Middle](https://arxiv.org/abs/2307.03172)
- [Coding Agents are Effective Long-Context Processors](https://arxiv.org/abs/2603.20432)
- [Inside the Scaffold：coding-agent harness taxonomy](https://arxiv.org/abs/2604.03515)
