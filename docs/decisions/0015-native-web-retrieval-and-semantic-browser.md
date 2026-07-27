# ADR-0015：Rust 原生 Web 获取、搜索准入与语义浏览器 Harness

- 状态：已接受；W1 `web_fetch` 已准入，其他层仍按证据逐项准入
- 日期：2026-07-28
- 细化：ADR-0001、ADR-0002、ADR-0011、ADR-0012、ADR-0014
- 当前 production delta：0（W1 尚未合入）
- 当前执行关系：ROADMAP 已将 W1 作为下一条且唯一 in-progress production slice；搜索、
  ApplicationProbe、语义浏览器、浏览器 action 与视觉仍未随 W1 自动准入

## 文档权威与替代关系

本 ADR 是 DSE 关于网页获取、搜索发现、浏览器操作和未来视觉重入的唯一整合决策。此前在
开发任务中发送的 Firecrawl、Playwright、浏览器、DeepSeek Web Search、截图或视觉接口补充
均由本文取代，不再作为可独立排队的开发要求。

本文接受长期边界、候选机制和准入顺序，并直接准入 ROADMAP 的最小 W1：

```text
accepted architecture + admitted W1 web_fetch
  != admitted search/browser/vision platform
```

产品所有者已经明确把“读取已知公开 URL”列为下一阶段真实生产力需求。当前固定工具面又
确定性地缺少任何 canonical Web 读取工具；这一事实本身足以准入一个不改变模型协议、
Runtime 或 Store 的最小纵向能力，不再要求先消费一轮付费模型请求去重复证明“工具不存在”。

W1 仍必须在干净 checkpoint 后写明唯一 owner、acceptance、旧路和删除点。除 W1 外：

```text
search/browser/vision production code delta = 0
```

不得再通过聊天补充把本文拆成多个并行 backlog。需要调整本决策时，修改或 supersede 本
ADR，并由 ROADMAP 安排一个切片；不要新增平行“浏览器方案”“补充计划”或 handoff 文档。

## 真实问题

DSE 是本地编码 Agent。当前固定工具目录能读取本地仓库、编辑、执行 Shell 和验证，但没有
canonical `web_fetch`、`web_search` 或浏览器工具。由此可能出现五类彼此不同的任务缺口：

1. 用户已经给出 URL，但 Agent 不能取得公开文档正文；
2. 用户只给出问题，Agent 需要先发现候选来源；
3. 页面依赖 JavaScript、导航状态或会话，普通 HTTP 获取不足；
4. 本地 Web 应用必须实际启动、交互并验证 DOM 行为；
5. 任务只有依赖像素、图表、画布或截图才能理解。

这五类问题不能被一个“浏览器平台”或一个远程 Agent 服务混为同一能力。不同缺口对应不同
成本、权限、副作用、证据和模型输入；过早合并会引入云端依赖、第二执行循环、不可重放的
浏览状态、额外计费和视觉接口债。

当前源码还冻结了以下事实：

- production model backend 只有官方 DeepSeek ChatCompletions；
- fixed Host catalog 只有 11 个代码工具，没有 `web_search` 或 browser tool；
- `ToolOutcome -> RuntimeEvent -> RunStore` 已是唯一调用、结果、证据与恢复链；
- `RunPermissionMode`、Host authorization 与 OS sandbox 已是唯一权限链；
- `crates/orchestrator` 已有 evidence-gated `ApplicationProbe` 候选，但尚无重复 loss 准入；
- TUI 仍有只被配置/Doctor 使用的多 search-provider 枚举，它没有 canonical Agent executor，
  不是现有 Web 能力，也不能直接升级为 production 架构；
- 旧 `[vision_model]`、图片 attachment 和 `image_analyze` 因无 production consumer 已删除；
- 当前 `ModelMessage` 和 DeepSeek wire 是文本内容，`ToolArtifact` 也没有 durable image blob
  store。

因此本决策解决的是：一旦真实任务证明 DSE 需要外部 Web 能力，怎样以最小、Rust 原生、
Host 可强制、可验证且可删除的方式接入，而不是先造一个“以后都能用”的浏览器框架。

## 第一性原理

DSE 继续优化：

```text
verified task success / tokens / time / code complexity
```

Web 能力必须遵守以下推导顺序：

```text
任务需要外部事实或运行态证据
  -> 先识别最小缺失观察
  -> 使用最低成本、最低权限的获取机制
  -> Host 执行并产生 typed outcome
  -> DeepSeek 只消费有界、带来源的文本观察
  -> Host verifier 接受最新 workspace revision
```

由此得到六条原则：

1. **获取不等于搜索，搜索不等于浏览器，浏览器不等于视觉。** 每项单独准入。
2. **已知 URL 优先直接获取。** 不为静态文档启动 Chrome。
3. **浏览器是 Host 工具，不是第二 Agent。** DeepSeek 提议动作，Rust Harness 执行、限制、
   观察和记账。
4. **语义观察优先。** 当前 DeepSeek 是文本模型，主要输入是 DOM/Accessibility 文本和
   状态，不是每步截图或坐标。
5. **外部内容不获得指令权。** 页面、搜索结果、README 和脚本都是不可信 observation，
   不能改写用户目标、权限或系统契约。
6. **不能验证收益就删除。** 竞品存在、演示好看或工具调用更多都不是保留依据。

## 决策摘要

### 1. 当前不使用 Firecrawl 作为 production 依赖

Firecrawl Browser 提供远程云浏览器、托管 session/profile、Playwright/Node/Python/Bash
执行、独立 credits 和可选模型能力。它可以缩短原型时间，但会同时引入：

- 远程云执行和新的外部数据边界；
- DSE 之外的浏览器 session、profile 和生命周期 owner；
- 独立费用与 request-level accounting；
- Node/Python/Playwright 运行面；
- 更难精确重放和验证的第三方状态。

这些成本与 Rust-native、local-first、单 Runtime/Store 和当前 accounting 纪律冲突。
Firecrawl 最多可在独立研究中作为结果对照或 evaluator oracle；不得进入 production
Cargo/runtime、默认配置、用户凭据、RunStore 或工具目录。

同理，Browser Use、Stagehand、Playwright MCP 和其他远程 browser-agent 平台可以用于
研究或外部 verifier，不能成为 DSE production Runtime、sidecar 或第二 Agent loop。

### 2. production 浏览器候选固定为 Rust/Tokio + CDP

若浏览器切片得到准入，首选结构为：

```text
AgentRuntime tool call
  -> ProductionToolExecutor
  -> Rust Browser Harness
  -> pinned Chrome for Testing process
  -> Chrome DevTools Protocol
  -> bounded semantic observation
  -> ToolOutcome
  -> RuntimeEvent / RunStore
```

- 浏览器协议依赖 Chrome DevTools Protocol，不依赖 Node 或 Playwright runtime；
- async Rust CDP 客户端首先评测 `chromiumoxide 0.9.x`，但架构不暴露其类型；若其
  lifecycle、CDP coverage、维护或依赖成本不通过，删除 spike 后选择更小的 Rust adapter；
- 测试和可复现实验使用精确版本、平台、SHA-256 都冻结的 Chrome for Testing；
- production 不读取用户 Chrome profile、Cookie、extension 或已登录 session；
- 模型 Run 期间不得静默下载或升级浏览器。所需 binary 必须在 preflight 前已安装并通过
  identity/checksum 验证，否则返回 typed `browser_unavailable`；
- Chrome sandbox 保持开启，并继续受 DSE 当前 macOS Seatbelt / Linux bubblewrap 边界约束；
- browser process、临时 profile、download 目录和 network guard 全部为 Run-scoped，
  terminal、cancel、timeout 和 crash cleanup 必须闭合。

Playwright 可以继续作为 eval 中独立 DOM verifier，但不进入 production dependency graph。

### 3. Web 能力拆成三个独立 treatment

| 能力 | 解决的问题 | 最小候选 | 明确不承担 |
|---|---|---|---|
| 已知 URL 获取 | 静态公开文档/源码页面 | Rust `web_fetch` | 搜索、JS session、视觉 |
| 搜索发现 | 从问题找到候选来源 | 一个固定、可计费的 search surface | 页面交互、浏览器 session |
| 语义浏览器 | JS、导航、表单、本地 UI | Rust CDP Browser Harness | 图像理解、坐标操作、通用脚本平台 |

一次 ROADMAP 切片只能准入其中一个能力族。只有前一层无法解决同一重复 loss 时，才允许
进入更高成本层；不得一次提交 `fetch + search + browser + vision`。

### 4. 当前不预留视觉接口

DeepSeek 当前官方 V4 集成仍是 text-only；官方 Anthropic compatibility 也明确不支持
`type="image"` 输入。DSE 不为未存在的 wire contract 预先加入：

- `VisionProvider`、`supports_images` 或 multimodal capability enum；
- image content block、screenshot message、base64/data URL projection；
- browser screenshot tool、`click_at(x,y)` 或坐标映射；
- image blob table、image artifact store 或 screenshot retention policy；
- “以后可用”的空 trait、adapter、feature flag 或 compatibility field。

当官方 DeepSeek 在 DSE 使用的生产 surface 上同时支持 image input、tool calling、thinking
history、streaming、usage/accounting 和 replay 后，再基于真实请求/响应协议新建 ADR 并
重构。届时比较 AX-only 与 AX+selective screenshot；当前代码不为该未来保留结构。

## 能力选择矩阵

新 loss 先按下表路由，不得默认选择浏览器：

| 已复现的任务缺口 | 应评测的最小候选 | 不应先做 |
|---|---|---|
| 用户给出 URL，HTML/文本可直接取得 | `web_fetch` | Chrome、截图、搜索 API |
| 不知道来源 URL，需要发现文档 | 独立 `web_search` treatment | 抓取搜索结果 HTML、远程 browser agent |
| 页面正文依赖 JS | read-only semantic browser | unrestricted JS、视觉模型 |
| 需要点击/填写/按键才能观察结果 | ref-based browser actions | 坐标点击、用户 Chrome profile |
| 本地 Web 应用行为无法由 HTTP/test 验证 | ApplicationProbe + semantic browser | 通用云浏览器平台 |
| 只有 canvas、图表或像素差能判断 | 当前 hold，等待真实 multimodal contract | 空视觉抽象、screenshot-every-step |

当多个缺口同时存在，默认优先级为：

```text
direct HTTP fetch
  -> structured search discovery
  -> read-only semantic browser
  -> ref-based interaction
  -> only then reconsider visual observation
```

优先级不是强制把所有层都实现；W1 已由明确产品需求直接准入，更高成本层仍须独立满足
repeated-loss admission。

## `web_fetch` 候选合同

`web_fetch` 已由明确产品需求和 current catalog 缺口直接准入。owner 为 `crates/tools`，
使用现有 Rust HTTP/TLS 栈，不增加 Provider、browser process、第二 Runtime 或第二 Store。

最小输入：

```text
url
optional bounded selector or text limit
```

模型不得提供任意 header、Cookie、认证、代理、证书、文件路径或 executable script。

最小输出：

```text
requested_url
final_url
status
media_type
title
bounded extracted text
bounded canonical links
retrieved_at
source_sha256
bytes_read / bytes_returned / truncated
trust = external_untrusted
```

Host 必须执行：

- 只接受 `https`；只有 TaskContract-scoped ApplicationProbe 才可接受精确 loopback `http`；
- 每次 DNS、connect 和 redirect 都拒绝未授权 loopback/private/link-local/multicast、云 metadata
  和非 HTTP(S) scheme，防止 SSRF、DNS rebinding 和 redirect escape；
- 限制 redirect、response bytes、decompressed bytes、content type 和 deadline；
- HTML 只做确定性正文/标题/链接提取，原始脚本不执行；
- binary、archive、PDF/image 不在本切片自动转入其他能力；
- 返回来源与截断事实，不能把提取失败包装成空成功。

若普通 HTTP 获取已解决任务，不再启动 browser treatment。

## 搜索发现候选合同

搜索是独立的外部检索能力，不是 `web_fetch` 的一个 provider mode。当前不选定 production
search provider，也不把现存 TUI/Doctor 的 Bing、DuckDuckGo、Tavily、Bocha、Metaso、
SearXNG、Baidu、Volcengine、Sofya 枚举提升为 Agent 架构；它当前没有 canonical executor，
是 caller audit 对象。

W1 交付后，只有真实任务仍因未知 URL 无法完成时，搜索才进入一个最多两天的协议决策；
该决策必须先比较：

1. 官方 DeepSeek 是否已在 DSE 当前 ChatCompletions surface 上提供可用、可计量、可重放的
   Web Search；
2. 若只有 DeepSeek Anthropic compatibility 支持 Web Search，是否值得通过新 ADR 改变
   “单 Chat transport”边界；本 ADR 不授权该变化；
3. 若使用外部搜索 API，哪一个固定 adapter 在来源质量、稳定性、隐私、区域可用性、
   request identity、费用和删除成本上通过评测。

production 只允许一个接受的 search surface，不提供 provider marketplace、fallback chain
或自动 router。不得通过抓取搜索引擎 HTML 作为默认产品方案；其 DOM、反爬、条款和结果形状
不稳定，不能提供成熟的长期合同。

候选工具名固定为 `web_search`，最小输入为 query 与有界结果数；时间/站点过滤只有存在真实
任务和确定性 schema 后再加。最小输出必须给出每个结果的 title、canonical URL、snippet、
provider/source identity、rank 和 retrieval time；snippet 是发现线索，不是最终事实，重要
结论必须再经 `web_fetch` 或 browser 读取原来源。

若外部 search 或 DeepSeek Web Search 产生独立费用、额外模型请求或 Token，必须进入同一
canonical accounting observation。任何 unknown billing、usage incomplete 或无法按请求
归因的费用继续按 ADR-0011 fail closed；不得把套餐余额、credits 或“免费额度”记作零成本。

## 语义浏览器工具面

只有重复 loss 明确要求 JS/runtime/session 或本地 UI 行为时，才在实际 actor catalog 增加
最小工具。初始候选不提供通用 `browser_eval`，而使用明确、可审计的小 schema：

```text
browser_navigate(url)
browser_snapshot(scope_ref?, max_nodes?)
browser_click(snapshot_id, element_ref)
browser_fill(snapshot_id, element_ref, text)
browser_press(snapshot_id, key)
browser_wait(condition, timeout_ms)
```

`browser_close` 不需要成为模型工具；Host 在 terminal/cancel/timeout/cleanup 时拥有无条件
teardown。back、tab、download、upload、dialog、Cookie、storage、auth、PDF 和 screenshot
均不进入初始目录；只有独立重复 loss 才能扩展。

若多工具 schema 的 catalog Token 或 DeepSeek 选择错误形成实际 loss，可以在后续单变量
treatment 比较一个 action-discriminated tool；不能在没有轨迹前先造通用 `browser(op, ...)`
平台。

### Snapshot 与 element ref

浏览器 observation 以 Accessibility tree 为主，按需结合 DOM 的确定性字段。每个有界节点
最多包含：

```text
element_ref
role
accessible_name
text/value
enabled/disabled
checked/selected/expanded
link target origin/path where safe
```

- ref 是 Host 生成的 opaque ID，绑定 `browser_session + page_epoch + snapshot_id + node`；
- ref 只对生成它的最新 snapshot 有效；任何 action、navigation、document replacement 或
  超出阈值的 DOM mutation 都使旧 ref 失效；
- stale、missing、hidden、disabled、detached 或 ambiguous ref 一律 typed fail closed；
- 模型只选择 ref，Host 解析真实 DOM node、可见性、可交互性和必要 geometry；
- 模型永远不接收可直接操作的 x/y 坐标；
- snapshot 按 node/byte/depth 限制，支持 scope 缩小和确定性截断，不把整页 HTML、CSS、
  script、network log 或 accessibility tree 无界塞入 prompt；
- action 完成后 Host 必须观察新的 page epoch，并在同一 ToolOutcome 返回新的 snapshot ID、
  URL/title 和有界状态摘要，形成 `observe -> act -> observe`，不能只返回“clicked=true”。

### Wait 合同

`browser_wait` 只接受 typed condition，例如：

```text
document_ready
url_matches
element_present
element_absent
text_present
network_quiet_with_bound
```

不接受任意 JavaScript predicate。所有 wait 有硬 deadline；timeout 返回可行动的最后 URL、
snapshot identity 和条件事实，不能包装成空成功。

## Browser Harness 生命周期

Browser Harness 是 `crates/tools` 内一个窄 adapter，不是 daemon、Service、Manager crate 或
第二 Runtime。生命周期固定为：

```text
capability preflight
  -> create ephemeral profile/download dir
  -> start pinned Chrome process group
  -> attach CDP
  -> establish egress/origin policy
  -> navigate / observe / act
  -> commit ToolOutcome
  -> terminal/cancel/timeout cleanup
```

边界如下：

- 一个 Run 默认最多一个 browser session；并行 child 不共享 session、ref、Cookie 或 page；
- read-only child 若未来获得浏览器，只能拥有独立 read-only session；Writer 不因 actor 身份
  自动获得浏览器；
- session state 不成为第二 persistent truth。RunStore 只保存 canonical tool lifecycle 和
  有界 ToolOutcome；
- process crash 后不宣称恢复 live page、JS heap、Cookie 或 in-flight request；
- Prepared 未 Started 可按现有工具规则重新判定；Started 未 Outcome 一律 side-effect
  ambiguous / recovery required，不自动重复 click、fill、press 或 navigation；
- committed outcome 在 SQLite reopen 后只重放已存结果，不重新执行浏览器动作；
- 若需要继续，模型必须创建新的 session、重新 navigate、snapshot 并取得新 ref；
- Chrome process tree、profile、download 和 local proxy 在正常/取消/超时/崩溃恢复检查中
  必须可识别并清理，不留后台 browser truth。

## 网络、权限与安全边界

Browser 只有在 Host 能实际强制下列边界时才可准入；仅弹窗或在 Prompt 中提醒不算 enforcement。

### Egress 与 origin

- public Web session 使用 Host-owned egress guard；Chrome 不允许绕开 guard 直连；
- 顶层导航、redirect、iframe、WebSocket 和 subresource 都执行 scheme/host/DNS/IP 检查；
- 默认拒绝 loopback、RFC1918/ULA、link-local、multicast、Unix/file/data/javascript/chrome、
  云 metadata 和未授权自定义协议；
- DNS 解析结果与 connect target 都校验，redirect 或 rebinding 不能越过；
- public session 不拥有本地 ApplicationProbe origin；local probe session 只允许 Host
  租约中的精确 loopback address/port，默认禁止外网；
- 页面内 cross-origin top-level navigation 若超出已授权 origin，必须在 request 发出前
  阻断并要求新的显式 navigate/authorization；
- 不接受模型提供的 proxy、certificate bypass、header、Cookie 或 Chrome flag。

如果 CDP、OS sandbox 与最小本地 egress guard 仍不能可靠强制这些边界，public browser
candidate 必须 `reject_and_delete`，不能以“Chrome 自身大致安全”完成。

### Permission

- 不增加第四 permission mode；继续使用 Ask、Agent、FullAccess 和 explicit deny/hard
  invariant；
- `crates/tools` 对 exact browser invocation 生成唯一 `ToolAuthorizationDecision`，绑定
  tool、arguments digest、workspace revision、target origin、risk 与 matched rule；
- Ask 只有在 origin/network scope 可被 egress guard 真正强制时才允许 durable approval；
  否则保持现有 typed fail closed；
- Agent 可执行 Host 分类为普通且满足 egress policy 的读取动作；external side effect、
  credential、upload、download open、purchase、publish、message send 和 destructive
  action 仍必须 ask 或 hard deny；
- FullAccess 不绕过 SSRF、profile isolation、scheme deny、download isolation、secret
  redaction、browser sandbox 或 TaskContract scope；
- child/Writer 不能从 root browser session 继承更高 authority。

### 初始副作用范围

第一版 public semantic browser 只承诺读取和 GET-style navigation：

- POST/PUT/PATCH/DELETE、form submit、file chooser/upload、下载自动打开、权限弹窗、
  clipboard、camera、microphone、notification、geolocation 和支付均阻断；
- `fill` 只修改 ephemeral page state，不自动 submit；
- `click` 若将触发被禁止 request/action，egress/interaction guard 必须在副作用前停止；
- local ApplicationProbe 可以在 disposable test workspace 内执行 TaskContract 明确要求的
  UI mutation，但仍不得访问真实外部账户、用户 profile 或未授权网络。

未来若真实任务需要登录、持久 session、上传或外部写操作，必须新建独立 ADR；不能通过
逐步放宽初始工具描述偷渡。

### Prompt injection 与秘密

- 所有网页文本、ARIA label、搜索 snippet、title、console/network message 都标记为
  `external_untrusted`；
- 页面中“忽略此前指令”“运行命令”“上传文件”等文字只是数据；
- 任何页面不能扩大工具 catalog、permission、allowed origin、TaskContract 或 completion；
- URL 中的 userinfo、Cookie、authorization header、localStorage/sessionStorage、表单
  secret、console token 和 query secret 在日志、ToolOutcome、artifact 与 UI 中脱敏；
- 初始 browser 不读取 DSE credential、环境变量、用户浏览器凭据或系统 keychain。

## 本地 ApplicationProbe 集成

本地 Web 应用是编码 Agent 最直接的浏览器用例，但只有 M36 定义的
`application_visibility`/`verification_visibility` loss 跨独立任务重复才准入。

唯一分工：

| owner | 职责 |
|---|---|
| `crates/orchestrator` | worktree-local app start、process group、port lease、health、log、teardown |
| `crates/tools` | HTTP 获取或 semantic browser session/action |
| `crates/protocol` | 最小 tool schema/outcome/evidence 字段，不定义 Browser Runtime |
| `crates/runtime` | 现有 tool loop、authorization、completion/replay |
| `crates/state` | 现有 ToolPrepared/Started/Outcome 和 artifact replay，不建 browser 表 |
| `crates/context` | bounded semantic observation projection，不建 page memory |
| `crates/app` | capability composition 与 fixed actor catalog |

推荐闭环：

```text
workspace revision
  -> worktree-local app start
  -> exact process/port/health receipt
  -> HTTP check
  -> only if needed semantic browser observe/action
  -> deterministic DOM/URL/API assertion
  -> logs + verifier receipt bound to latest revision
  -> guaranteed teardown
```

HTTP/status/API assertion能验证时不得强制启动 Chrome。DOM assertion 应由 Host verifier
读取稳定 role/name/text/state，不让模型自评“页面看起来正常”。无视觉模型时不把截图当
completion evidence。

## Canonical evidence、replay 与 accounting

Browser/search/fetch 不增加 `BrowserEventStore`、session database、trajectory memory 或
第二 accounting ledger。所有事实继续经过：

```text
ToolPrepared
  -> ToolAuthorizationDecision
  -> ToolExecutionStarted
  -> ToolOutcome
  -> optional bounded ToolArtifact / EvidenceReceipt
  -> TerminalState
```

### ToolOutcome 最低事实

按能力包含：

```text
invocation identity
requested and final canonical URL/origin
operation status and stable failure_code
redirect / page epoch / snapshot identity
bytes/nodes returned and truncation
source/provenance/trust
side_effect and retry disposition
workspace revision where applicable
artifact sha256/media_type/length when existing bounded artifact is used
```

不保存整页 HTML、无限 DOM、Cookie jar、Chrome profile、视频、截图或 network archive。
如果 bounded inline artifact 不够表达 verified evidence，该 capability 必须先证明 durable
artifact 需求，再扩展现有 artifact owner；不能为浏览器预建 blob platform。

### Completion evidence

模型的“已经打开/点击/看到了”仍是 advisory。Host 只接受与 TaskContract 和最新 workspace
revision 匹配的证据，例如：

- final URL/title 和稳定 DOM/AX assertion；
- HTTP status、response schema 或 local API assertion；
- app process/port/health/log receipt；
- explicit verifier result；
- browser action 后新 snapshot 中的预注册状态变化。

console/network log 只有 TaskContract 明确要求时才采集有界摘要。工具成功、页面存在或一张
截图都不自动等于 `verified_success`。

### Accounting

- local Chrome/CDP 没有第三方 browser credits；记录本地 process time、actions、pages、
  network requests、bytes 和 wall time作为 Harness 成本；
- DeepSeek 模型请求继续进入现有 physical request/usage/cache/cost ledger；
- 外部 search/fetch 若收费，必须有请求级 usage/price/seal 事实，并与行为 truth 正交；
- DeepSeek 官方 Web Search 文档说明会产生额外模型 Token 请求时，这些请求不得隐藏在
 普通 Chat usage 之外；
- pre-header、partial usage 或第三方 credits 无法精确归因时按 ADR-0011 停止正式 campaign；
  不猜零、不用余额差补账、不通过 Firecrawl 包月 credits 掩盖成本。

## 实施准入与节奏

以下是 ROADMAP 的执行协议。W1 已排队；W2–W5 仍是候选。任何时刻只允许一个
in-progress production slice。

### Gate W0：冻结任务与安全合同，最多一个工作日

W0 不运行新的大规模 loss acquisition，也不读取 Key。它只冻结 W1 的确定性合同：

- 一个已知官方 HTTPS 文档 URL；
- redirect、private/loopback/link-local/metadata、非 HTTPS 与 DNS 重绑定反例；
- body/decompressed bytes、content type、deadline 与 truncation 边界；
- HTML title/text/link 的确定性提取 fixture；
- ToolOutcome、authorization、RunStore reopen 与 actor catalog identity。

完成这些合同后直接进入 W1。禁止把 W0 扩成新的 6/8/20-task 付费 campaign；确定性工具缺失
不需要用模型随机性再次证明。

后续真实 dogfood loss 仍按稳定 taxonomy 归因：

```text
known_url_retrieval
search_discovery
javascript_rendering
browser_interaction
application_visibility
visual_only
permission_or_egress
provider_transport_or_accounting
task_or_eval_defect
model_capability_ceiling
```

重复损失门只用于从 fetch 升级到更高成本的 search/browser/action/vision，不再阻止已准入的
W1。

### Slice W1：最小 `web_fetch`

只在 `known_url_retrieval` 重复时执行：

1. 冻结 URL/task/verifier/security fixture；
2. 在 `crates/tools` 先写失败合同；
3. 接入一个 `web_fetch` tool 与现有 authorization/outcome；
4. 迁移真实 caller；
5. 不把 TUI/config 中未进入模型 catalog 的 search-provider 管理面冒充 W1 caller，也不在
   本切片顺手重构它；
6. 通过 SSRF、redirect、truncation、crash/reopen、catalog parity 和 production loopback；
7. 用一个费用受限的真实 DeepSeek known-URL canary 验证模型可选择并消费该工具；
8. keep 或完整删除。

W1 不引入 Chrome、search provider、browser types、视觉 placeholder 或第二 DeepSeek wire。

### Slice W2：read-only semantic browser

只有 `javascript_rendering` 或 `application_visibility` 在 W1 后仍重复时执行：

1. 用 eval-only spike 比较 CDP adapter 生命周期与依赖，spike 不进入 production；
2. 冻结 Chrome for Testing identity、process/profile cleanup 与 egress guard；
3. 只实现 navigate、snapshot 和 Host teardown；
4. observation 只含 AX/DOM bounded text；
5. 覆盖 stale document、redirect、prompt injection、process crash 与 exact ToolOutcome replay；
6. 先用于 read-only local/public任务，不增加 action、search 或 screenshot。

### Slice W3：ref-based browser actions

只有 `browser_interaction` 在 W2 后仍重复时执行：

1. 每次只增加一个 action family；
2. click/fill/press/wait 全部使用最新 snapshot ref；
3. action 后强制 fresh observation；
4. public POST/upload/download/auth 等副作用继续阻断；
5. local disposable ApplicationProbe 按 TaskContract 开放最小交互；
6. stale ref、hidden/disabled、cross-origin、crash-after-start 负向测试必须 100% 拒绝。

### Slice W4：ApplicationProbe 收敛

只有 local service/API/UI task 形成重复 `application_visibility` loss 时执行。它复用 W1/W2/
W3 已被保留的最小能力，不创建第二 browser owner；若 W2/W3 从未准入，ApplicationProbe
只能使用 process/health/log/HTTP。最终必须绑定 latest workspace revision、deterministic
assertion 和 teardown receipt。

### Slice W5：搜索发现

只有 `search_discovery` 独立重复时执行，与 browser slice 分开：

1. 重新复核官方 DeepSeek 当前 search surface、usage 和 pricing；
2. 冻结一个且仅一个候选 surface；
3. 删除/替代现存无 canonical executor 的多-provider config/Doctor 假能力；
4. search result 只负责发现，原来源再 fetch/browser；
5. 完成 source quality、prompt injection、billing、reopen 和 same-model held-out A/B；
6. 无净收益或 accounting 不闭合则删除全部 candidate。

W5 的编号不表示必须晚于所有 browser action；ROADMAP 应根据重复 loss 只选择当时最小的
一个 slice。编号用于保持合同边界，不是并行工作队列。

## 评测与保留门

每个 production candidate 必须冻结 task、workspace、DeepSeek model/effort、Prompt、actor、
tool authority、request/Token/deadline、verifier、browser/search identity 和 accounting。
只有改变模型可见策略、默认行为或需要比较多个可行 treatment 时才要求 control/treatment
A/B。确定性 W1 交付使用 contract/security tests、真实 caller、production loopback 和一个
费用受限 canary；不得为证明工具存在而执行多 cell 付费 A/B。至少报告：

```text
verified_success
false_success
correct_safety_rejection
first_authoritative_source_time
source precision / unsupported-claim count
browser actions / stale-ref failures
snapshot nodes / bytes / truncation
network requests / downloaded bytes
model requests / input / output / cache tokens
wall time / API cost / local browser time
crash-reopen exactness
production lines / dependencies / new concepts
```

决策顺序固定：

```text
1. identity / task / observer / source provenance valid
2. false_success == 0
3. SSRF / origin / permission / side-effect safety non-regression
4. correct safety rejection non-regression
5. verified task success improves on the affected task family
6. latest-revision evidence and crash/reopen exactness
7. source quality and unsupported claims
8. model/browser/search requests, Token, bytes, wall time and cost
9. production code, dependencies and concept complexity
```

最低保留条件：

- W1 的 known-URL task、production caller、SSRF/redirect/truncation 反例和 reopen 全部通过；
  若是搜索/浏览器/模型可见策略 treatment，affected family 仍须先有重复 loss，并按
  EVALUATION 的正式 A/B 规则执行；
- false success 为 0；
- private-IP/metadata/redirect escape、stale ref、未授权跨 origin、blocked external
  side effect 的正确拒绝率为 100%；
- root/read-only/Writer、permission、tool lifecycle、SIGKILL/reopen、CLI/TUI/app-server
  canonical projection不回退；
- behavior/accounting 按 ADR-0011 正交闭合；
- treatment 接管后删除旧/假入口、eval-only selector、临时 spike、失去消费者的 config/
  dependency 和兼容 branch。

可接受结果只有：

```text
keep_current_no_external_web_capability
keep_minimal_fetch_and_delete_replaced_path
keep_minimal_search_and_delete_replaced_path
keep_minimal_semantic_browser_and_delete_replaced_path
reject_and_delete_candidate
hold_insufficient_or_incomplete_evidence
hold_model_capability_ceiling
```

## 明确拒绝的路线

- Firecrawl production、远程 browser sandbox、cloud profile 或 browser credits；
- Node/TypeScript/Python/Playwright sidecar；
- Browser Use、Stagehand、Playwright MCP 或其他产品的 Agent loop；
- 第二 Runtime、browser-specific Agent、browser event store、session database；
- 把搜索、fetch、browser、MCP、IDE 和远程执行合成一个外部工具平台；
- provider marketplace、fallback chain、自动 search/browser router；
- 抓取搜索引擎 HTML 作为稳定默认搜索合同；
- 用户 Chrome profile、Cookie、extension、登录态或系统 keychain 注入；
- unrestricted JavaScript/eval、任意 Chrome flags、任意 header/proxy；
- coordinate click、screenshot-every-step、视觉 placeholder 或 image blob store；
- 自动 POST、购买、发布、发送消息、上传、下载打开或持久登录；
- 用页面文字修改系统规则，或把搜索 snippet 直接当成最终来源；
- 用模型自评、截图存在、工具调用成功或页面返回 200 冒充任务成功；
- 为未来能力创建空 BrowserManager/WebProvider/VisionProvider/Factory/Service；
- 在当前开发切片未完成时插入 browser/search 实现，或把聊天补充当作新 milestone。

## 后果

- DSE 若未来获得浏览器，将是现有 Harness 的一个 Host 工具能力，而不是新的产品/runtime；
- 静态获取、来源发现、JS 浏览和本地应用验证可以按真实 loss 独立保留或删除；
- Rust CDP + pinned Chrome for Testing 是 browser treatment 的首选技术路线，Firecrawl 和
  Playwright MCP 不进入 production；
- semantic AX/DOM observation 适配当前 text-only DeepSeek，不为未发布的视觉能力制造债；
- permission、egress、provenance、ToolOutcome、RunStore、latest-revision verifier 和
  accounting 继续是唯一真相；
- 当前多 search-provider 配置不被视为已有能力；未来切片必须审计并删除或替代其无 caller
  路径；
- 视觉能力只有在官方 DeepSeek 真实 multimodal wire 可用后，才通过新 ADR 做完整纵向
  重构；
- W1 已由 ROADMAP 准入，但在实现合入前 current tool catalog 仍保持 11 个 Host 工具；
  W2–W5 在新的 ROADMAP 准入前不会改变 crate、Cargo dependency、tool catalog、protocol、
  State schema、delivery artifact 或用户界面。

## 研究依据（2026-07-27 复核）

- [DeepSeek：V4 Coding Agent integrations（text-only）](https://api-docs.deepseek.com/quick_start/agent_integrations/github_copilot/)
- [DeepSeek：Anthropic API compatibility](https://api-docs.deepseek.com/guides/anthropic_api)
- [DeepSeek：Claude Code integration 与 Web Search](https://api-docs.deepseek.com/quick_start/agent_integrations/claude_code)
- [DeepSeek：Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek：Thinking Mode 与 tool-call replay](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Microsoft：Playwright MCP](https://github.com/microsoft/playwright-mcp)
- [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
- [Chrome for Testing](https://github.com/GoogleChromeLabs/chrome-for-testing)
- [chromiumoxide 0.9.1](https://docs.rs/crate/chromiumoxide/0.9.1)
- [OpenAI：Computer use custom harness](https://developers.openai.com/api/docs/guides/tools-computer-use)
- [OpenAI：Harness engineering](https://openai.com/index/harness-engineering/)
- [Anthropic：Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Firecrawl Browser](https://docs.firecrawl.dev/features/browser)
