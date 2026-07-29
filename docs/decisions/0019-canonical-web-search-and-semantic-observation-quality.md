# ADR-0019：Canonical Web Search 与语义观察质量

- 状态：已接受
- 日期：2026-07-29
- 取代：ADR-0015/M44 的 `hold_wait_for_chat_surface` 与“production 尚未选择 search surface”结论
- 细化：ADR-0011、ADR-0012、ADR-0015、ADR-0016、ADR-0017、ADR-0018
- primary owner：`crates/tools`；`crates/app` 只负责 production caller/composition

## 决策摘要

DSE 采用一个 Host-owned、Rust-native 的 canonical `web_search`，并把它与既有 `web_fetch` 和
semantic browser 固定为一条研究路由：

```text
web_search -> source selection -> web_fetch
           -> browser_navigate/browser_interact only when rendering/action is necessary
           -> cross-check -> URL citation
```

搜索只负责发现。provider snippet、rank、score 或 HTTP success 都不能证明事实；形成结论前必须读取
原来源，并以至少两个独立来源交叉核验。外部搜索与网页内容始终是 `external_untrusted`。

首个 Host adapter 固定为 Tavily Search HTTPS endpoint 的 Basic/general surface。它是一个具体实现，
不是 Provider interface、fallback registry 或可配置 marketplace。未来若 DeepSeek 官方 Chat wire 提供
稳定的 server-side search，只能替换这个 Host adapter；不得改变 `AgentRuntime`、`RuntimeEvent`、
`RunStore` 或模型工具循环。

同一 capability cluster 将 semantic browser 的机械 first-eligible-N 截断替换为确定性的 task-cue、
role 与 interaction priority，并返回可测量的 observation quality 和 action 前后 structured semantic
diff。它不增加第二 LLM、模型生成脚本、视觉假接口或另一个 observation path。

## Problem、owner 与旧路删除

当前 root Agent 能读取用户给出的已知 URL，但面对未知来源的工程问题没有 canonical source discovery；
此前 TUI/config 中的多 search-provider 管理面没有 production executor，M44 已物理删除并 fail closed。
另一方面，browser observation 先取满足条件的前 N 个节点，导航/菜单噪音可能在 `max_nodes/max_chars`
边界前挤掉任务相关内容，且 action 后没有紧凑的变化摘要。

single primary owner 是 `crates/tools`：catalog、schema、fixed HTTPS client、结果 canonicalization、
authorization、observation pruning/diff 与 `ToolOutcome` 都在同一 owner。`crates/app` 只组合真实
`AgentApplication -> AgentRuntime` caller 与 reopen fixture。

cutover 不恢复 `[search]`、`DSE_SEARCH_*`、Doctor provider selector 或搜索 HTML workaround；也不保留
first-eligible-N active assertion。真实 caller 切换后只有一个 `web_search` definition/dispatch/network
identity，观察也只有一个 AXTree + DOMSnapshot extraction path。

## Canonical `web_search` 合同

模型输入只有：

```json
{
  "query": "bounded non-secret public query",
  "max_results": 5
}
```

- `query` 必填，无控制字符/首尾空白，最多 512 字符/2,048 bytes；
- `max_results` 可选，默认 5，范围 1～10；
- 不接受 provider、endpoint、answer、header、Cookie、credential、proxy、TLS、browser 或文件参数。

Host request 固定为 `POST https://api.tavily.com/search`，Bearer credential 只从既有 secret backend 或
`TAVILY_API_KEY` 取得，不进入 schema、Prompt、ToolOutcome、日志或 SQLite。request plan 固定
`search_depth=basic`、`topic=general`、`auto_parameters=false`、`include_answer/raw_content/images=false`
和 `include_usage=true`。官方协议依据为
[Tavily Search API](https://docs.tavily.com/documentation/api-reference/endpoint/search)；Basic request 的
当前 credit 语义依据为
[Tavily Credits & Pricing](https://docs.tavily.com/documentation/api-credits)。

Host 强制 public-only DNS、connect pin、HTTPS-only endpoint、no proxy、no redirect、identity encoding、
5 秒 connect、12 秒总 deadline 与 1 MiB response body。401/403、429、其他 status、DNS/connect、
Content-Type、JSON、request identity、deadline 与 size 均返回稳定 typed failure；错误 body 不进入模型。

成功结果至少返回：canonical query、fixed adapter identity、provider request id/response time/usage credits、
ranked canonical public HTTP(S) URL、source host、title、bounded snippet、optional published date/score、
results read/returned/dropped/truncated、retrieved time、received-response SHA-256/bytes、billing truth、
`evidence_role=discovery_only`、citation requirement 与 `trust=external_untrusted`。结果 URL 的真实 DNS、
redirect 与 transport 安全仍由后续 `web_fetch`/browser 每一跳重新裁决；search result 不授予网络权限。

credits 是 provider 返回的 usage unit，不等于实际 charge。缺少账单事实时必须记录
`provider_usage_units_only_actual_charge_unavailable`，不得声明精确费用或效率。

## Semantic Observation Quality 合同

`browser_navigate` 增加可选、有界、非 secret 的 `focus` task cue。Host 对完整 eligible AX/DOM 节点集：

1. 统计 focus match、prompt-injection signal、AX-only、DOM-interactive-without-AX 与 canvas/SVG blind spot；
2. 按 focus match、interaction capability、semantic role 确定性排序，navigation/banner/footer 降权；
3. 只对无 capability 的相同 semantic content 安全去重；
4. 再施加既有 `max_nodes/max_chars` 边界并生成 opaque refs；
5. action 后基于同 page epoch lineage 的 backend DOM identity 形成 ref-independent bounded diff。

每次 observation 返回 strategy、task-relevant recall bps、redundant ratio、prompt-injection exposure、
AX/DOM/visual blind spot 与 truncation-caused focus loss；action 后另返回 added/removed/changed/unchanged、
stale ratio、最多 16 个有界 diff entries 和 bytes。metrics 描述观察质量，不把关键词 match、DOM/AX 或
diff 存在冒充任务成功。网页文本仍是 external-untrusted；模型不能生成 selector、JS 或 pruning 程序。

## Authorization、replay 与 recovery

- root/coordinator/read-only child 依既有 read-only actor catalog 获得 `web_search`；isolated Writer 的
  Host-controlled network 继续由 `actor_controlled_network_denied` 拒绝；
- Ask 显示 exact query 与“发送到固定 search endpoint”的影响后一次性批准；Agent/FullAccess 在永久
  egress/secret 不变量内允许；
- execution identity 包含 fixed search network identity，resume mismatch fail closed；
- committed `ToolOutcome` 经 SQLite reopen 只重放，不再次 search/fetch/browser；
- `ToolExecutionStarted` 无 outcome 的 search 进入既有 `RecoveryRequired`，不自动重复可能计费的请求；
- 现有 `ToolOutcome`、RuntimeEvent 和 RunStore 可以无损表达结果、failure、usage unit、started/outcome 与
  replay，因此 protocol/state schema delta 为 0。

## 验收与决策边界

keep 需要 deterministic catalog/schema/authorization/bounds/failure/provenance tests、真实 production
search -> 两个独立 source -> fetch -> citation caller、SQLite reopen/recovery no-reexecution、真实 pinned
Chrome task-cue recall/diff vertical、focused 与最终 revision 一次 full gate。

official DeepSeek requests 默认 0；这个确定性工具簇不需要付费 A/B。若 Host 有完整 search credential，
最多一次 Basic、零 rerun 的 search canary；缺少 credential 时明确跳过。没有 complete accounting 不影响
闭合 behavior proof，但阻止成本/Token/效率声明和下一次付费请求。

本决策不引入 SearchManager/Factory/Service、第二 Provider/Runtime/Store、search session/ledger、
browser Agent、Node/Playwright/Firecrawl sidecar、搜索 HTML 抓取、screenshot/vision、个人 Chrome 或
任意 header/auth/proxy。DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 与
managed browser session contract 保持不变。

## 后果

root Agent 首次能在同一 production/runtime/persistence 主链中发现未知公开来源、读取原文、交叉核验并
给出可重放引用；JS 页面仍只在 fetch 不足时进入 semantic browser。观察边界从“页面前 N 个节点”变为
可测量的任务相关确定性 treatment，但不会用另一模型或视觉假接口隐藏缺失。下一步应进入跨
code/app/Web/Writer/recovery 的内部 Alpha integration checkpoint，而不是再建立一个 search provider 或
one-action/one-Goal 微切片。
