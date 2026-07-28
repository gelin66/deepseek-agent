# ADR-0016：精益认知控制面与风险分级证明

- 状态：已接受
- 日期：2026-07-28
- 细化：ADR-0002、ADR-0011、ADR-0014
- 实施顺序：M44 形成 clean checkpoint 后、M45 前

## 决策摘要

DSE 的下一步不是增加第二个“规划器”、Goal Store、Memory Service、浏览器框架或更多测试层，
而是收敛已经存在的认知控制面：让 DeepSeek 在每一步只获得完成当前任务所需的最小、正确、
可追溯上下文，同时让 Host 只按真实风险要求证明。

```text
DeepSeek V4
  -> 认知控制面
       TaskContract / 当前目标
       有界 authority 与项目事实
       按需 Skills / actor-scoped tools
       context projection / compaction / continuation
       verifier feedback / actor policy
  -> 唯一 AgentRuntime
  -> tools / sandbox / Orchestrator
  -> EvidenceReceipt(latest workspace revision)
  -> 唯一 RunStore
```

“认知控制面”是现有 `crates/app`、`crates/context`、`crates/protocol`、`crates/runtime`、
`crates/tools` 和 `crates/orchestrator` 之间的一组边界，不是新 crate、进程、Runtime、Store 或
抽象层。它的工程目标是：

```text
verified productive outcome
-----------------------------------------------
human attention × tokens × wall time × complexity
```

极致工程指最小闭环复杂度下的高验证生产力，不指最多控制逻辑、最多 Agent 或最多测试。

## 真实问题

### 1. 开发 authority 已经重到妨碍认知

当前根 `AGENTS.md` 要求普通修改前完整阅读 Product Plan、全部 ADR、Roadmap、Evaluation 和
Current Architecture。这些权威正文合计 **17,248 行**：

| 文档 | 当前行数 |
|---|---:|
| `PRODUCT_PLAN.md` | 443 |
| `ROADMAP.md` | 6,768 |
| `EVALUATION.md` | 4,964 |
| `CURRENT_CODEWHALE.md` | 3,002 |
| ADR-0001～0015 | 2,071 |

这不再是“地图”，而是每次任务都重放项目历史。它同时增加启动 Token、遗漏当前重点的概率和
维护成本。历史证据是必要的，但不应全部成为每个 Agent 的默认工作记忆。

### 2. 稳定内核与高频策略仍缺少明确变更边界

DSE 已有很强的确定性内核，但模型、Prompt、工具描述、context budget、actor policy 和评测
经常与协议、存储、恢复、UI 一起被审计。结果是局部认知策略变化也容易触发全系统级工作，
甚至为了“证据完整”重复运行与候选收益无关的 campaign。

### 3. 证明机制有反客为主的风险

行为真相与 accounting 真相正交是正确的；但如果每个确定性工具、文档或 UI 投影都被要求做
多 cell 付费 A/B，评测本身就会成为产品。`billing evidence incomplete` 只表示不能精确声明
成本或效率，不等于已经由 contract、安全反例、真实 caller 和 replay 证明的行为不存在。

当前缺少一张统一的“变化类型 -> 最小充分证据”准入表，也缺少限制同一 revision 重复全量门
和低信号付费请求的机械预算。

### 4. 长任务有 durable replay，但“继续思考”仍可能携带过多旧上下文

DSE 已有唯一 RunStore、deterministic compaction、resume、terminal `continue` lineage 和
typed handoff；这些不能被另一套 Goal/Memory 真相替代。真正未充分证明的问题是：长任务在
安全边界进入 fresh continuation 时，能否只携带当前 TaskContract、已验证里程碑、未满足项、
workspace revision 和必要 artifact，而不是把整段历史重新塞给模型。

这个问题只能在出现可重复的长任务 loss 后改 production；不能先建设 Goal Store、私有
progress file、第二 checkpoint 系统或“自动大脑”。

### 5. Harness 收益和模型收益还没有被干净分离

DSE 需要回答两个不同问题：

1. 同一个 DeepSeek，在 DSE Harness 与最小 Harness 下，verified success、false success、
   人工介入、Token、时间和复杂度有什么差异；
2. 作为完整产品，DSE 与 Codex、Claude Code 在真实仓库任务上的差距在哪里。

第一个问题决定哪些 Harness 机制值得保留；第二个问题决定产品能力优先级。把二者混成一个
总榜，会把模型差异错归因给 Harness，或把缺少浏览器/UI 的产品差距错归因给 Runtime。

### 6. 安全边界必须先于无人值守能力扩张

Writer worktree 已隔离写入，但 root 的文件系统与网络权限仍需在进入浏览器动作、登录态或更长
无人值守任务前形成 OS 可强制的最小授权。Prompt 服从和工具 allowlist 都不能替代 sandbox、
网络域/目标约束与凭据隔离。

## 第一性原理

### 1. 模型负责判断，Host 负责事实和边界

DeepSeek 负责提出计划、选择工具、解释观察与生成改动。Host 负责权限、工具 schema、状态、
副作用、重试、workspace revision、verifier 和终态。能确定执行的规则不得继续堆进 Prompt。

### 2. 默认上下文必须是地图，不是仓库历史

默认输入只回答五件事：当前目标、不可违反的边界、唯一 owner、当前事实、如何按需查证。
历史、完整 ADR、旧里程碑和大输出通过索引或 artifact 按需读取。渐进披露不是“少给信息”，
而是“需要时能精确找到唯一权威”。

### 3. 控制强度与不可逆风险成比例

验证成本随副作用、恢复难度和声明强度上升，而不是随文件数量或开发焦虑上升。确定性工具与
model-visible 策略不使用同一种准入门。

### 4. 稳定内核冻结，高频策略可替换

| 稳定内核：只有真实 correctness/security loss 才改 | 高频策略：允许小步替换和回滚 |
|---|---|
| `TaskContract`、terminal authority | model ID、reasoning effort |
| canonical `RuntimeEvent`、唯一 `RunStore` | Prompt 与工具 description |
| permission、sandbox、tool outcome | context selection / compaction policy |
| workspace revision、receipt、verifier | actor profile 与 fan-out policy |
| crash/replay、continuation lineage | browser observation 与检索策略 |
| Writer worktree、review/verify/integrate | benchmark task 与默认预算 |

高频策略必须有稳定 identity、fixture、held-out task 和 whole-candidate rollback；不为每个策略
创建新 Manager/Factory/Service trait。

### 5. 删除是重构完成的一部分

替换旧 authority 路径、重复 context、重复 evaluator consumer 或无收益策略时，同一切片删除
旧路。长期 adapter、双写、第二 truth 和“以后可能有用”的 disabled 实现不准保留。

## 决策

### A. 把开发 authority 改成有界启动地图

根 `AGENTS.md` 继续保存不可违反的产品边界和 owner map，但默认读取合同改为：

1. 完整读取 Product Plan；
2. 读取 Roadmap 的“当前/下一切片”窗口；
3. 按变更 owner 读取相关 accepted ADR；
4. 读取 Current Architecture 的对应 owner 当前事实；
5. 只有要修改评测合同或判断 keep/delete 时，读取 Evaluation 的稳定规则与当前条目；
6. 冲突或影响固定架构时，再沿索引扩大读取范围。

全部 ADR、里程碑历史和评测结果仍是权威、仍可查、不得伪造或重写；只是从 unconditional
bootstrap 改为 owner-scoped retrieval。必须提供机械索引，使 Agent 不依赖猜测定位相关内容。

第一阶段不大规模删历史正文。先切默认读取路径并证明没有漏掉固定边界；历史正文的压缩必须
另有链接完整性和 frozen evidence 审计，不能趁机“清理”证据。

### B. 建立按变化类型分级的最小充分证据

| 变化 | 最小充分证据 | 默认不需要 |
|---|---|---|
| 文档、索引、纯 projection | 链接/fixture、唯一权威、调用方或用户工作流检查 | Key、付费 A/B、全 workspace soak |
| 确定性工具/UI/config | contract、安全反例、真实 caller、reopen/replay、一次 focused gate | 多 cell Prompt campaign |
| protocol/state/recovery/security | conformance、迁移、fault injection、reopen、focused + pre-integration full gate | 无关模型样本 |
| model-visible Prompt/context/route | same-DeepSeek、same-task/budget、held-out A/B、false success=0 | 仅凭代码审美准入 |
| 产品级能力/效率声明 | 私有真实任务、完整 accounting、人工介入/Token/时间/成功率 | 用单次 canary 外推 |

附加规则：

- 同一 workspace revision 的 full gate 最多在 pre-integration 运行一次；局部开发使用 owning
  crate focused gate，除非失败影响面要求扩大。
- offline fixture 能回答的问题不读取 Key。
- official canary 每个候选默认最多一次、maximum reruns=0；只有预注册的随机产品 A/B 才使用
  多样本。
- accounting 不完整时，停止下一付费请求并禁止成本/效率声明；不推翻已经闭合的确定性行为。
- 测试必须绑定真实 failure、架构不变量或 caller；只证明实现细节的重复测试应删除或合并。

### C. 保留一个认知状态真相

任务目标继续由 `TaskContract` 冻结，进展由 canonical RuntimeEvent、receipt、artifact、terminal
state 和 continuation lineage 推导。开发此 ADR 时使用 Codex 任务自身的 Goal 跟踪执行，但
**不得**把 Goal/Hunt、`thread_goals`、progress Markdown 或新 Store 重新引入 DSE production。

若未来 fresh continuation 有重复 loss，只允许从现有事实派生一个有界
`VerifiedMilestoneProjection`：

```text
objective / acceptance IDs
latest verified workspace revision
committed receipts and artifact handles
unsatisfied conditions and typed blockers
next bounded action
source run / sequence provenance
```

它是 `RunStore` 的 derived projection，不可独立写入、不可获得 terminal authority。没有至少
两个独立长任务的同类 loss，不实施该 production 变化。

### D. Harness 采用“弱点 -> 最小候选 -> 私有回归 -> 删除”的自我改进回路

每个 Harness 候选必须声明：

```text
真实 failure
  -> owner_code:loss_code
  -> 最小 owning-module treatment
  -> same-DeepSeek held-out comparison
  -> keep / shrink / reject_and_delete
```

不从单次 anecdote 直接增加全局 Prompt、角色、工具或测试；不通过让 evaluator 更容易来制造
收益；公开 benchmark 用于方向，私有新鲜任务用于准入。

### E. 双层 benchmark，不再混淆 Harness 与完整产品

1. **Harness isolation**：DSE current 对比一个最小 DeepSeek loop；固定模型、effort、任务、
   预算和环境，只改变 Harness。
2. **Product reality**：DSE 对比当前 Codex/Claude Code；允许各自原生最佳实践，衡量用户最后
   能否更快得到可验证结果。

共同指标为 verified task success、false success、人工介入次数/分钟、wall time、请求/Token、
费用完整性和 candidate code/concept delta。没有完整 accounting 的 cell 只用于 correctness，
不进入效率排名。

### F. 能力扩张遵守安全前置门

M45 ApplicationProbe 仍先于语义浏览器；M46 仍只在 HTTP 不足的真实页面 loss 后启动。
浏览器动作、登录态、视觉、多模态和 Firecrawl 不属于本 ADR 第一实现切片。进入这些能力前，
必须为真实 owning actor 建立可执行的文件系统/网络/进程最小权限、凭据隔离和 teardown；不做
视觉 placeholder，也不为未来模型预留空接口。

## 首个实现 Goal

现有 M44 开发任务完成定向门、形成 clean commit 后，继续使用同一任务并切换为以下 Goal：

> 在不改变 DeepSeek wire、model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 和产品
> capability 的前提下，把 DSE 开发 authority 改为有界地图与 owner-scoped 按需读取，并把
> 验证准入改为风险分级；删除旧 unconditional full-read 和重复 gate 路径，以确定性证据证明
> 开发启动负担下降且固定架构边界零遗漏。

### 固定范围

- owner：repository guidance + `docs/product` / `docs/architecture` / `docs/decisions`；若需要机械
  检查，只能落在已有 scripts/test owner；
- 修改根 authority 入口、当前窗口索引、owner -> ADR/current facts/eval contract 路由；
- 写出单一 risk-tier gate contract，并让现有开发脚本/文档调用它，不复制另一套 gate；
- 更新 Roadmap 的唯一执行顺序和 Evaluation 的当前合同；不创建平行 roadmap、handoff 或
  tracker；
- 使用现有任务 Goal，不启动 M45/M46，不并行开新任务。

### 明确非目标

- 不改生产模型行为、Prompt、context compaction、actor topology；
- 不新增 Goal Store、Memory、Planner、Manager/Factory/Service trait；
- 不拆大 crate，不做 UI、品牌、Provider、浏览器或视觉工作；
- 不读取 DeepSeek Key，不做付费 campaign；
- 不删除 frozen raw、manifest、summary 或 Git 历史。

### 验收

1. 普通 owner-scoped 修改的 mandatory bootstrap 上限不超过当前 17,248 行基线的 25%，并有
   fixture/脚本列出实际读取集合；跨固定架构的变更仍能逐级展开全部相关 authority。
2. Product Plan 中固定边界、ADR-0001～0016 accepted decisions、owner map、当前 milestone、
   focused/full gate 和 deletion rule 均能从根入口精确到达，链接检查 100% 通过。
3. fresh Agent 仅凭启动地图能回答：当前目标、唯一 owner、禁止项、focused gate、何时跑 full
   gate、替换后删除什么；预注册问题零遗漏。
4. 同一 revision 不再被默认要求重复 full gate；确定性变化不会触发无关 paid A/B；高风险
   protocol/state/security 和 model-visible 变化的原有严格门不降低。
5. `git diff --check`、authority fixture、相关 focused gate 通过；production Rust delta=0，
   DeepSeek official requests=0。
6. 旧 unconditional full-read 文字和重复验证说明在调用方切换后物理删除；没有第二权威入口。

### 停止与回滚

- 若有一个固定架构边界无法从新入口确定定位，拒绝切换并恢复旧入口；
- 若风险分级使既有 correctness/security regression 逃逸，回滚分级而不是补 Prompt；
- 若实现需要新 Runtime/Store/Provider、生产 Goal 或广泛 Rust 重构，立即缩小范围；
- 若 3～5 个工作日不能形成可用的启动地图和机械门，只保留已证明的最小切片，其余删除。

## 后续顺序

首个 Goal 完成并形成 clean checkpoint 后再重新审计：

1. 是否存在重复的 long-horizon continuation loss；没有则不做 milestone projection；
2. 以 same-DeepSeek 最小 Harness 建立 capability/complexity 基线；
3. 按既定 Roadmap 进入 M45 ApplicationProbe；
4. 只有真实 JS 页面证明 HTTP 不足时进入 M46 只读语义浏览器；
5. root 网络/文件系统 containment 在任何 unattended browser action 前完成。

这些不是并行开发授权。Roadmap 是唯一执行顺序，任何后续项都必须等当前 Goal clean
checkpoint 后再启动。

## 研究依据

- OpenAI 的 Harness Engineering 强调仓库应提供短地图、结构化文档与机械架构约束，而不是
  让 Agent 默认吞入一部“千页手册”：
  <https://openai.com/index/harness-engineering/>
- Anthropic 的 managed agents 经验表明模型和 Harness 假设会快速过时，应稳定 session、
  sandbox 与 Harness 接口，把策略保持可替换：
  <https://www.anthropic.com/engineering/managed-agents>
- Anthropic 的长任务研究把 fresh context、结构化 handoff 与独立 evaluator 作为 compaction
  之外的关键机制，但这些必须建立在 durable state 上：
  <https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>
  <https://www.anthropic.com/engineering/harness-design-long-running-apps>
- Context Engineering 的核心是选择最小高信号上下文，而不是最大化上下文：
  <https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents>
- Self-Harness 使用 weakness mining、最小候选和 held-out regression，适合作为 DSE 的可删
  自改进回路：<https://arxiv.org/abs/2606.09498>
- `mini-swe-agent` 展示了随着模型能力提升，极简 loop 可以替代许多脆弱专用接口，提醒 DSE
  必须用收益证明每一层 Harness 复杂度：<https://github.com/SWE-agent/mini-swe-agent>
- benchmark 需要隔离基础设施噪声与评测污染：
  <https://www.anthropic.com/engineering/infrastructure-noise>
  <https://openai.com/index/separating-signal-from-noise-coding-evaluations/>
- 无人值守能力必须以 OS 级 sandbox 和 containment 为前提：
  <https://www.anthropic.com/engineering/claude-code-sandboxing>
  <https://openai.com/index/running-codex-safely/>

## 后果

正面结果是 Agent 更快到达当前问题，Harness 候选更容易独立替换，测试与付费证据回到“防止
真实失败”的角色，并保持 DSE 已有的单 Runtime、单 Store、latest-revision completion 与
Writer worktree 优势。

代价是项目必须维护精确 authority 索引和风险分类；开发者不能再用“把所有文档都读一遍”或
“把所有测试都再跑一遍”代替影响分析。任何生产认知策略优化也必须继续接受 same-DeepSeek
held-out 证据，不能把“精益”误解为降低 correctness、安全或可恢复性。
