# ADR-0018：工程完全体能力治理与能力簇交付

- 状态：已接受
- 日期：2026-07-28
- 部分取代：ADR-0015 的永久能力拒绝、逐 action repeated-loss 准入与 one-action/one-Goal
  后续路线
- 细化：ADR-0005、ADR-0011、ADR-0012、ADR-0014、ADR-0016
- owner：产品 authority；production 能力仍按现有 crate owner 落地

## 决策摘要

DSE 的产品目标是：

> 以 DeepSeek 为唯一模型后端、以极致工程能力为核心的完整生产 Agent。

“完整”表示用户能完成真实工程任务的端到端闭环，不表示无权限执行。安全是能力之上的
Host-owned 治理层：sandbox、最小授权、风险分级、确认、隔离、凭据托管、审计、恢复和
latest-revision evidence。不能继续把永久删除整个能力类别当作主要安全方案。

以下核心架构保持不变：Rust-native、local-first、DeepSeek-only、一个 `AgentRuntime`、一个
canonical `RuntimeEvent`、一个 `RunStore`、Host terminal authority、typed `ToolOutcome`、Writer
worktree 和 latest-revision evidence。能力扩张只能复用这些 owner，不能引入第二 Runtime/Store、
Provider marketplace、TypeScript sidecar 或 browser-specific Agent loop。

本 ADR 只改变长期产品边界、准入粒度和路线排序。它不改变当前 production 源码、DeepSeek
wire、Prompt、RuntimeEvent 或 State schema。

## 审计结论

### 1. 当前路线不能形成目标中的完整工程产品

W1～W3.1 已证明了安全 fetch、JS 语义观察、opaque ref、click/fill、fresh observation、teardown
与 replay。这些底层机制应保留。但当前 production 仍有确定性缺口：

- `browser_click`/`browser_fill` 只能操作 Host 注入的 exact-loopback ephemeral page；
- 没有 press、typed wait、scroll、select、textarea/contenteditable、back/tab 或多页面状态；
- 没有 public reversible action、外部副作用预览/确认与 scoped execution grant；
- 没有 managed isolated login/session、Cookie/storage 生命周期、受控 upload/download；
- 没有 canonical `web_search`、来源发现、交叉验证和 citation contract；
- 没有 selective visual observation；
- `RunPermissionMode` 已类型化，但 current root `Agent`/`FullAccess` composition 仍使用 broad
  sandbox posture，Ask 下网络又因缺少可强制 scoped grant 而 fail closed；
- 现有大量机制证据尚未组成覆盖代码、应用、Web、恢复和集成的持续 production dogfood 门。

因此，按本产品目标定义，当前 DSE 不能宣称可替代完整工程 Agent。继续逐 action 做
`admission audit -> implementation -> next admission` 只会扩大测试与文档，而不会及时闭合用户
任务。

### 2. 现有成果不是错误方向

以下 W1～W3.1 结果继续有效：

- public HTTP(S) fetch 的 SSRF、redirect、provenance 与 replay；
- Rust direct-CDP、pinned Chrome、isolated ephemeral profile、egress proxy 与 bounded AX/DOM；
- opaque capability ref、page epoch、stale/identity checks；
- action 后 fresh observation；
- side-effect started/outcome/recovery、committed reopen 不重执行；
- external-untrusted 内容与 Host completion boundary。

问题是把初始 slice exclusion 提升成最终产品边界，而不是这些机制本身过强。

## 三类边界

### A. 永久安全与架构不变量

以下约束长期保留，不能用“完全体”绕过：

1. 浏览器使用 DSE managed isolated profile；默认不读取用户个人 Chrome Profile、Cookie、
   extension 或历史。显式 attach 也必须是可撤销的 exact grant。
2. 网络 egress、origin、DNS/connect/redirect、下载目录与文件访问由 Host 强制；模型不能提供
   任意 proxy、header、证书绕过或开放端口。
3. 模型只消费 opaque semantic ref/typed observation；不开放 unrestricted eval/JavaScript、
   任意 DOM identity、坐标脚本或任意 Chrome flags。
4. secret/credential 由 Host/系统安全存储托管，只在获批目标使用；不进入 Prompt、网页
   observation、日志、artifact 或模型工具参数。
5. 外部内容始终是 `external_untrusted`，不能改写 TaskContract、权限、catalog 或完成标准。
6. 副作用使用 prepared/authorized/started/outcome/recovery 语义；started 后结果未知时不自动
   重复，必须恢复、核验或请求人工处置。
7. action 后必须 fresh observation；页面返回、工具成功或截图存在都不等于 verified success。
8. 完成证据绑定 latest workspace revision、TaskContract 与 Host verifier/receipt。
9. 进程、页面、profile、session、download 和临时 secret grant 均有 deadline、cancel、resource
   bounds、teardown 与 reopen 语义。
10. destructive、financial、publish、message-send、security-sensitive 和不可逆操作必须强确认、
    最小授权并返回可审计 receipt。
11. 只有一个 AgentRuntime、RuntimeEvent、RunStore、permission truth 和 accounting truth；不创建
    browser/session-specific 第二事实系统。

### B. 当前实现缺口，不是最终拒绝

以下事实可以描述 current production，但不得再写成永久产品边界：

- browser action 仅 loopback；
- 只有 click/fill；
- 没有 press/wait/scroll/select/textarea/contenteditable/back/tab/multi-page；
- 没有 public action 或 form submit；
- 没有 managed login、Cookie/storage/session；
- 没有受控 upload/download；
- 没有 canonical search/research；
- 没有 screenshot/visual observation；
- 不能处理登录后的外部工程系统。

这些能力只有在执行层没有可强制边界时才暂时 fail closed。正确的后续工作是补齐 enforcement，
不是把缺口重新命名为安全结论。

### C. 继续拒绝的实现机制

- unrestricted model eval/JavaScript、无边界 shell/network；
- secret 进入模型上下文或外部不可信页面；
- 默认直接读取用户 Chrome Profile；
- 崩溃后自动重放未知外部副作用；
- 无确认购买、发布、发送、删除或安全敏感变更；
- 第二 Runtime、第二 RunStore、browser Agent loop、Provider marketplace；
- Firecrawl/Playwright/Node production sidecar；
- 空 `VisionProvider`、BrowserManager/Factory/Service 或 speculative compatibility layer；
- 用模型自评、HTTP 200、工具成功或 screenshot 冒充任务完成。

## 风险分级执行语义

产品 capability 与执行授权分离：

| 风险层 | 默认行为 | 必须记录 |
|---|---|---|
| Read | 在 egress/path/scope guard 内自动执行 | target、provenance、bounds、outcome |
| Reversible local action | sandbox/worktree 内自动执行 | revision、diff/action、fresh evidence、rollback/cleanup |
| External side effect | 执行前展示 exact target、参数和影响；按 Run/项目策略确认 | approval/grant identity、started/outcome、remote receipt |
| Destructive/financial/publish/security-sensitive | 强确认、最小一次性授权；无法约束则拒绝 | human decision、execution fingerprint、result/recovery receipt |

用户可以给当前 Run 或项目授予 scope 明确、可撤销的权限。grant 不能来自模型参数，不能扩大
到其他 origin/path/action，也不能在 crash/reopen 后无证据续用。`FullAccess` 仍不绕过永久
安全不变量。

计费是行为旁边的状态事实，不是 DSE 的核心能力或阻止工程闭环的替代品。usage 不完整继续
阻止成本/效率声明和下一次付费请求，但不抹掉已经闭合的 deterministic behavior evidence。

## 从 micro-slice 切换到 capability cluster

### 准入规则

明显属于完整工程 Agent 基线的能力，可以由产品目标、capability map 和真实端到端任务直接
准入，不要求先人为制造两次“缺失工具”失败。repeated-loss gate 继续用于：

- 昂贵或可选优化；
- 新 Runtime/Store/wire、额外 Provider 或架构分叉；
- 多个可行 treatment 的默认选择；
- Prompt/context/route 等 model-visible 策略；
- 高度不确定、维护成本很高或可能删除的候选。

每个 capability cluster 必须闭合一个完整用户任务族，可以由多个 reviewable internal commit
组成，但使用一个 cluster Goal、一个 frozen task matrix 和一个 final integration decision。不能再
为每个明显必要的 key/button 建立独立 Goal、独立 evaluator 和独立路线段。

测试数量不构成成果。每个 cluster 必须报告：以前不能完成的任务、现在能完成的任务、
verified success、false success、人工确认、wall time、Token/request（有模型请求时）、返工次数、
crash/recovery、side-effect receipt、生产复杂度和删除量。

## Capability map 与顺序

| 能力簇 | Current | 目标闭环 | 主 owner | 路线状态 |
|---|---|---|---|---|
| 完整代码工程 | canonical read/edit/shell/git/verify、ApplicationProbe、root/child/单 Writer、resume | 多语言跨文件理解→编辑→build/test/static check→service/log/debug→diff/review/integrate→latest-revision completion | existing owner chain；composition=`crates/app` | 持续 dogfood，不另建 Runtime |
| Web 研究 | public HTTP(S) `web_fetch` | 一个 canonical `web_search`→来源选择→fetch/browser→交叉验证→引用 | `crates/tools` | search surface 另行决策，但不再因“无两次缺失测试”无限 hold |
| 语义交互与 public action | loopback navigate/click/fill | press/wait/scroll/select/text editing/back/tab/multi-page + public reversible action + risk receipt | `crates/tools` | **首个 production cluster** |
| Managed browser session | ephemeral isolated profile | project-scoped login/session、Host credential、Cookie/storage lifecycle、受控 upload/download | `crates/tools` | 在首簇 enforcement 后 |
| Visual re-entry | 无 image wire/observation | DOM/AX 不足时 selective screenshot/visual evidence | `crates/deepseek` wire + `crates/tools` observation | 等官方 DeepSeek multimodal contract，不预建空接口 |
| Integration/dogfood/release | crate/fixture gates 与历史 campaigns | 跨代码、应用、Web、恢复和 Writer 的真实任务集，定期 merge/release checkpoint | `crates/app` | 每个 cluster 的水平准入门，优先清理积压 |

## 首个后续 production capability cluster

名称：**Semantic Interaction + Public Action Governance**。

### Problem、owner 与 old path

- problem：当前 Agent 只能在 exact-loopback 页面 click/fill，不能完成常见 JS 应用交互或在用户
  授权下操作 public engineering system；Ask 网络全拒绝与 Agent broad sandbox 之间也没有 scoped
  external grant。
- single primary owner：`crates/tools`。`crates/app` 只组合真实 TaskContract/caller；protocol/state/
  runtime 只在现有 approval/outcome 无法无损表达 exact grant/receipt 时做最小版本变更。
- old path：ADR-0015 的 one-action/one-Goal admission loop、action exact-loopback blanket deny、
  Ask-network-always-deny 与 broad-access-only 二选一。迁移后删除 active 文案/fixture/caller 中的
  这些旧断言；冻结历史 evidence 不改写。

### Frozen real task families

1. **完整 local SPA 交互**：启动 worktree app，滚动到目标，填写 textarea/contenteditable，选择
   option，发送允许的 key，等待 typed DOM/AX condition，验证 state/API/log，再 teardown。
2. **public reversible engineering workflow**：在用户授权的 disposable public test origin 上导航、
   选择、填写并保存可撤销 draft；执行前展示 exact origin/action/parameters/impact，提交后返回
   remote receipt，并能撤销或由 fixture 清理。
3. **external side-effect negative**：未经确认的 submit/message/publish/delete/purchase、scope escape、
   credential leak、download open 与 upload outside grant 必须 100% 阻止。
4. **recovery**：每个 action 后 fresh observation；started-without-outcome 不重放；committed reopen
   不访问网络；cancel/timeout/Chrome crash 清理 profile/process/grant。

### Acceptance

- 一个 cluster matrix 覆盖 press、typed wait、scroll、select、textarea/contenteditable、back/tab 与
  多页面 state；不要求每个动作另建 admission Goal。
- local task family 与 disposable public reversible task 都通过真实
  `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RunStore` caller。
- Read 与 reversible local action 在强制 sandbox 内无需逐动作打断；external side effect 使用 exact
  preview/approval/grant/started/outcome receipt；high-risk 操作始终强确认。
- SSRF/origin/secret/scope/downgrade/stale-ref/unknown-side-effect false allow=`0`；false success=`0`。
- SQLite reopen、started ambiguity、latest-revision evidence、resource teardown 和 actor catalog parity
  全部闭合。
- 以真实 dogfood 报告 task success、人工介入、时间、Token/request、返工、恢复和复杂度；accounting
  不完整时只禁止成本/效率结论，不抹掉闭合行为结果。
- replacement caller 切换后删除 per-action eval-only gate、loopback-only action assertion 和 broad
  access workaround；不保留 compatibility flag。

### Integration order

1. 冻结 cluster task/safety/permission/recovery matrix，并审计现有 approval/scoped-authority 表达力；
2. 在同一 Rust CDP/ref/epoch lifecycle 补齐完整 semantic interaction；
3. 接入 public reversible action 与 exact risk preview/grant/receipt，先 deterministic service，再最多
   一个 credential-free transport canary；
4. 迁移 root/child/Writer catalog 与真实 caller/reopen，删除旧 deny/micro-admission 路径；
5. 运行 affected-task dogfood、focused 与按实际风险的一次 pre-integration full gate；形成独立
   reviewable commits，最终只保留一条 production path。

若 3～5 天不能形成至少一个完整纵向 task family，则缩小 cluster 的任务数量或 action 组合，但
不能退回 one-button/one-Goal 循环，也不能通过放宽永久安全不变量制造成功。

## Authority supersession

- ADR-0015 的 W1～W3.1 实施事实、Rust/CDP/ref/replay 安全机制继续有效；其“只有 repeated loss
  才能讨论每个 browser action”、one-action-family/Goal 和把登录/session/upload/download/public
  action/visual/search 写成长期开除项的规则由本 ADR 取代。
- ADR-0005 的多 Writer、FIM、RepoGraph 证据门保持有效；它约束可选实现名称，不阻止完整工程
  baseline capability。
- ADR-0012 的 typed permission、Host authorization、一次性授权和 hard invariants 保持有效；后续
  capability cluster 必须把它扩展到真正可强制的 scoped external grant，而不是旁路。
- ADR-0014 的 Prompt/context/route held-out A/B 保持有效；本 ADR 不把确定性能力缺口改成 Prompt
  treatment。
- ADR-0016 的 risk-tier gate、bounded authority 与 Harness treatment discipline 保持有效；
  repeated-loss 只不再作为明显基线能力的通用入场券。
- ADR-0017 的 public HTTP fetch、transport provenance 与 SSRF contract 保持有效。

## 本次 authority Goal 验收

- production Rust/Cargo、DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 和
  product catalog delta=`0`；
- official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`；
- 只更新 Product Plan、唯一 Roadmap/Evaluation/current architecture、accepted decision index 和
  stable repository guide；不创建平行 roadmap/handoff/tracker；
- Risk 0 authority/link/diff gate 通过并形成独立 reviewable commit。

## 后果

DSE 继续以强 Host 边界获得可靠性，但不再把初始能力缺失包装成最终安全。Roadmap 从
browser action 微切片切到完整能力簇；真实任务成功、集成与可恢复 external receipt 成为成果，
测试数量和费用显示退回支持性证据。未来能力可以更完整，但仍只能通过现有 Rust Runtime、
permission、ToolOutcome、RunStore 和 evidence 主链交付。
