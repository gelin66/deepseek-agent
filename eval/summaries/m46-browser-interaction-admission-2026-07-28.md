# M46 post-W2 browser interaction admission audit

- 日期：2026-07-28
- clean baseline：`83b8bf455bffbe492fbbe32ff2fe88dbb5631878`
- 决定：`admit_next_goal_ref_based_browser_click_w3_contract_only`
- production Rust / Cargo dependency / browser action tool / session delta：`0 / 0 / 0 / 0`
- DeepSeek Key / official request / actual cost：`not read / 0 / $0`
- formal evaluator runs / reruns：`1 / 0`
- product metric：`product_metric_eligible=false`

## 问题、owner 与旧路

W2 已能一次性读取 JavaScript 渲染后的 bounded DOM/accessibility state，但它只返回 observation，
不返回 `element_ref`，也没有 click/fill/press/wait action。单个交互演示或把不同 action family
拼成一个计数，不能准入 W3。

本 audit 的 owner 是现有 Evaluation authority；production control 直接运行 current
`crates/tools::ProductionToolExecutor` 的 `web_fetch` 与 `browser_navigate`，没有复制其实现。
它替代凭 feature list、竞品存在或单个 demo 直接加入 browser actions 的旧推断。Playwright
只作为 exact-loopback external oracle，不是 production dependency、sidecar、session 或 Agent loop。

## 预注册合同

正式矩阵前冻结的
[`m46-browser-interaction-admission-v1.json`](../manifests/m46-browser-interaction-admission-v1.json)
包含：

- 恰好两个 task id 与不同 independence key；
- 同一 canonical `tools:browser_interaction` 和同一 `click` action family；
- 每个任务唯一的 exact role/name target 与 post-action role/name/state；
- fixture、server、evaluator、oracle、test-only control 与 pinned CfT SHA-256；
- same-loss threshold `2`，mixed action family 不可拼接，false-success 必须为 `0`；
- W2 process/proxy/profile teardown 与 committed SQLite outcome reopen 必须保持通过；
- official requests `0`、credential read `false`、external network `false`、maximum reruns `0`；
- 即使准入，本 audit 也只能冻结下一独立 click-family W3 Goal，production implementation=false。

evaluator self-test 为 `7/7`：credential read、重复 independence key、raw HTTP 含 post-action
literal、mixed action family、control false-success、W2 teardown regression 与 W2 replay regression
七种 false allow 全部被拒绝。

## 任务与实际观察

两份 fixture 都是真实启动的 loopback HTTP application。action target 与 post-action accessible
name 均以字符值生成，不以 literal 出现在 raw response。production `web_fetch` 只取得静态文本；
production `browser_navigate` 使用 pinned CfT `151.0.7922.47`，能看到精确 target，但没有 ref 或
action surface，因此不能产生 post-action state。两次调用的 process tree、egress proxy 与 profile
teardown 全部 settled。

eval-only oracle 使用 Playwright `1.61.0`、Node `v24.18.0` 和同一 pinned CfT；每个 task 只按
exact role/name click 一次，只允许 Host-assigned literal-loopback origin，输出不超过 2 nodes / 4,096
bytes，截图与坐标为 0，context/browser 均关闭。

| task | initial W2 observation | oracle one click result | HTTP / W2 verified | false success |
|---|---|---|---:|---:|
| `m46_interaction_deployment_approval` | `button / Reveal deployment approval` | `status / Deployment approved / data-state=approved` | `0 / 0` | 0 |
| `m46_interaction_retry_toggle` | `switch / Automatic retries disabled / aria-checked=false` | `switch / Automatic retries enabled / aria-checked=true` | `0 / 0` | 0 |

聚合：oracle `2/2`、current control verified `0/2`、同一
`tools:browser_interaction:click=2/2`、control false-success `0`；现有真实
`AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent ->
SQLite RunStore` regression 继续证明 committed outcome reopen 只重放且 navigate count 不增加。

唯一正式 result SHA-256：

```text
sha256:490a8ca323ad1433c5680c89da84463fdd4f34ddcab800fe063e3e8c41fe17aa
```

## 决定与下一合同

重复损失门通过，但只准入下一独立 W3 focused Goal 的一个 action family：

```text
crates/tools
  -> Host-generated opaque element_ref bound to latest snapshot + page epoch
  -> browser_click only
  -> mandatory fresh post-action semantic observation
  -> Host-owned ephemeral lifecycle + teardown
  -> existing ToolOutcome / RuntimeEvent / RunStore truth
```

下一 Goal 必须以 stale/missing/hidden/disabled/detached/ambiguous ref、cross-origin、非 GET 或外部
副作用、crash-after-start、authorization/catalog 与 reopen 为 negative gates。fill、press、wait、
登录、Cookie/storage、public POST/upload/download/auth、用户 Chrome profile、截图/坐标/视觉、搜索、
Node/Playwright production sidecar、durable browser session truth、第二 Runtime/Store/ledger 仍未准入。

本 audit 没有改变 production Rust、Cargo dependency、tool catalog、DeepSeek wire/Prompt、
RuntimeEvent、RunStore、State schema 或 UI；用户现在仍只有 read-only `browser_navigate`。

## 身份与验证

```text
manifest sha256  4a581a9277f3e66a486ad9a546c459552c35ab4e1988272d9a7037bbc7b6a0df
evaluator sha256 ad7072918db533046963f15ee03756042bb478172d7cd9ba35d07383d1fbec44
oracle sha256    48a9fc7293c7d3cf79cffbdaddd4d16b26b12fe640b6273f5bd1e36cbbc1077d
control sha256   6cd674260674d98b66531459d75ea4af8ee5510162139a695073aebed7d330b1
server sha256    f11185ab32b5e46a739a9cab692aa7870d1613fdf421dec60ba3fbc4a3c7fbd7
task A sha256    ba713a6d0f0a8b48fa598c730dd29dc4b819840d8a88f7f5fda5e521fab0ecc9
task B sha256    c7e76aa2b9b2f9339b5fb22ee2752e6f129329a408013fc1ac89d95e5d8eb98f
pinned CfT sha   e9e1c766953cf2ff5ea38c6cb63fa32b443a958c3fda7dcc3b60dd9b20436855
```

已完成：manifest validation、7-case negative self-test、Node syntax、test-only Rust strict Clippy、
production control 2/2、replay regression、唯一正式 evaluator 1/1。canonical focused gate 一次
通过：bounded authority baseline=`17,636` 行、ceiling=`4,409` 行、最大 tools owner route=`2,212`
行、固定边界可达率=`23/23`；tools=`385/0/5 ignored`、DeepSeek=`61/0/1 ignored`、Runtime
conformance=`88/88`、app=`65/0/3 ignored`、app-server=`23/23`、exec=`30/30`、TUI run=`20/20`、
PTY=`7/7`。本切片是 production delta=0 的 eval-only admission，不运行 full。
