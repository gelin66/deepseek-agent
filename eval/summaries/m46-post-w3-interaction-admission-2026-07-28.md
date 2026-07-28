# M46 post-W3 browser interaction admission audit

- 日期：2026-07-28
- clean baseline：`64b83a21b7859e38ca7ce43a4a035c6337702d36`
- 决定：`admit_next_goal_ref_based_browser_fill_w3_1_contract_only`
- production Rust / Cargo dependency / browser action tool / session delta：`0 / 0 / 0 / 0`
- DeepSeek Key / official request / actual cost：`not read / 0 / $0`
- 完整 formal evaluator result / reruns：`1 / 0`
- pre-result harness implementation stops：`2`，均在 oracle 前停止，不计为成功样本或正式结果
- product metric：`product_metric_eligible=false`

## 问题、owner、旧路与删除点

W3 已让 root Agent 对 Host-owned exact-loopback page 使用 opaque ref 执行一个 click 并取得 fresh
observation，但 current refs 只覆盖 click-safe button/switch，catalog 也只有 `browser_click`。真实本地
应用的文本录入是否构成下一同族缺口，不能由功能清单、单个 demo，或把 fill/press/wait 混合计数
决定。

本 audit 的 owner 是现有 Evaluation authority。control 直接运行 current
`ProductionToolExecutor` 的 `web_fetch + browser_navigate + browser_click`，permission mode 固定为 root
`Agent`；外部 oracle 仅作为 credential-free exact-loopback evaluator，不进入 production Cargo、
Runtime、Store 或 browser lifecycle。它替代“click 已存在，所以顺手补完整表单 API”的旧推断。

若准入，下一 Goal 的删除点已经冻结：真实 `browser_fill` caller cutover 后删除本 audit 的 Node/
Playwright oracle 和 test-only missing-tool control，不保留 selector 或第二执行入口。本 audit 本身不删除
既有 W3 fixture、manifest、result、summary 或 Git history，也不改写 frozen evidence。

## 预注册合同

正式完整矩阵前冻结的
[`m46-post-w3-interaction-admission-v1.json`](../manifests/m46-post-w3-interaction-admission-v1.json)
包含：

- 恰好两个 task id、不同 independence key 和同一 `fill` action family；
- 每个任务一个 exact role/name target、一个有界 fill value 和一个 post-action role/name/state；
- target name、fill value、post-action name/state 不以 literal 出现在 raw HTTP；
- baseline、fixture、server、evaluator、oracle、test-only caller、Node/Playwright 与 pinned CfT identity；
- same-loss threshold `2`，mixed family 不可拼接，false-success 必须为 `0`；
- Host teardown 与真实 AgentApplication committed click SQLite reopen 必须保持通过；
- official requests=`0`、credential read=`false`、external network=`false`、maximum reruns=`0`；
- 即使准入，本 audit 也只冻结一个后续 fill-family focused Goal，production implementation=false。

evaluator self-test 为 `11/11`：credential read、重复 independence、mixed family、raw target/value、
false-success、missing target、changed control surface、单任务 oracle、teardown failure 和 replay failure
均不能产生 false admission。

## 两个真实任务与结果

两份 fixture 都由真实 loopback HTTP application 启动，textbox/searchbox 及 post-fill status 由 JavaScript
创建。production `web_fetch` 不执行 script；production `browser_navigate` 使用 pinned CfT，能看见精确
text-entry target，但 fill-target refs=`0`，`browser_fill` 不在 15-tool root catalog，真实 Runtime
preflight 返回 `unknown_tool + side_effect=not_applied`。current control 因而不能产生 post-fill state，
也没有错误声称完成。

eval-only oracle 使用 Playwright `1.61.0`、Node `v24.18.0` 与 CfT `151.0.7922.47`。每任务只按 exact
role/name fill 一次，只允许 Host-assigned literal-loopback origin 和 GET/HEAD；输出不超过 2 nodes /
4,096 bytes，截图、坐标、Cookie/storage、认证、上传下载和 public action 均为 0，context/browser 均关闭。

| task | current W3 initial target | oracle one fill result | control verified | false success |
|---|---|---|---:|---:|
| `m46_post_w3_release_channel_fill` | `textbox / Release channel` | `status / Release channel set to canary / data-channel=canary` | 0 | 0 |
| `m46_post_w3_test_filter_fill` | `searchbox / Test filter` | `status / Test filter applied: network / data-filter=network` | 0 | 0 |

聚合结果：oracle verified=`2/2`、current control verified=`0/2`、同一
`tools:browser_interaction:fill=2/2`、control false-success=`0`。既有真实
`AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RuntimeEvent -> SQLite
RunStore` regression 保持 committed navigate/click reopen 只重放、调用计数 `1/1`；独立 pinned-CfT
Host lifecycle regression 证明 retained exact-loopback process/proxy/profile 均由 Host teardown。

唯一完整正式 result SHA-256：

```text
sha256:71afb4a6ce866e2e47eb68ad4001c8c586549172c3b414221fedb40b4c76391f
```

## Harness stops

完整正式结果前有两次 evaluator implementation stop，均未运行 oracle，也没有形成准入结果：

1. test-only caller 对未公开 `browser_fill` 直接调用 `execute` 并错误要求 invocation=`Rejected`；真实
   Runtime 会先调用 `preflight`。caller 改为真实 preflight 后，Host 的 `UnknownTool + NotApplied`
   语义保持不变。
2. 两个 production control 已完成后，系统 Python `3.9.6` 不支持 evaluator 中非必要的
   `zip(strict=True)`；脚本删除该语法依赖后才进入 replay/teardown/oracle。

两次 stop 都没有 DeepSeek request、credential read、外网请求或 oracle action；不能计作失败/成功 task
sample，也没有用来补挑结果。修正后的 identity 重新冻结后，完整正式矩阵只运行一次、reruns=`0`。

## 决定与下一合同

重复损失门通过，只准入下一独立 Goal 的一个 action family：

```text
crates/tools
  -> eligible latest-epoch text-entry opaque ref
  -> browser_fill(element_ref, value) only
  -> bounded non-secret value
  -> mandatory fresh post-fill semantic observation
  -> old refs invalidated + Host-owned exact-loopback teardown
  -> existing ToolOutcome / RuntimeEvent / RunStore recovery truth
```

下一 Goal 必须拒绝 stale/missing/hidden/disabled/readonly/detached/ambiguous ref，password/file/color/date
等非准入 input，过长值与控制字符，origin escape、非 GET、download/popup/storage/Cookie 和外部副作用；
operation-start 后 ambiguity 必须走既有 RecoveryRequired 且不得自动 fill，committed outcome reopen 只重放。

press、wait、click+fill macro、任意 form submit、登录/密码/secret、Cookie/storage 持久化、public POST/
upload/download/auth、用户 profile、截图/坐标/视觉、搜索、Node/Playwright production sidecar、第二
Runtime/Store/session ledger 仍未准入。

## 身份

```text
manifest sha256  6b266960f0a05f76d4dc38d5269511b7c9129232e661644e9e13dfa411f8de4f
evaluator sha256 9403cf00e70875f79646123c387f54011eb167566d70308b9fa7acc189b4043a
oracle sha256    79ae6637ceb1fa265b9c31bc14ee97966fc195cc7ec34c640a41055a30554caa
control sha256   4f3659e68d42a76c74c373d7feef5e2bc7ffb1f6fbbe2155a169447299ffb838
server sha256    f11185ab32b5e46a739a9cab692aa7870d1613fdf421dec60ba3fbc4a3c7fbd7
task A sha256    f7304df919af8af063d3a3c780b3b1928b94303bc40a28e89019331dc41eee6c
task B sha256    cd25deb2ae79742a95fc682f1e81659d266b14e682aad27c1993d4b39e0eb0b4
pinned CfT sha   e9e1c766953cf2ff5ea38c6cb63fa32b443a958c3fda7dcc3b60dd9b20436855
```

production Rust、Cargo graph、tool catalog、DeepSeek wire/model-visible Prompt、AgentRuntime、
RuntimeEvent、RunStore、State schema 和 browser session truth delta 均为 `0`。official DeepSeek
requests=`0`、credential read=`false`、actual cost=`$0`；费用只是正交状态，不是准入理由，也不形成
成功率、Token、时间或费用提升声明。

最终离线门通过：manifest validation、11/11 negative self-test、Node syntax、test-only Rust compile、
唯一完整 control/oracle/reopen/teardown evaluator、`dse-tools` unit 389 passed / 5 ignored、owner check、
strict Clippy、fmt 与 diff check 全绿。authority baseline=`17,636`、ceiling=`4,409`、最大 tools route=
`2,273` 行、fixed boundary=`23/23`。按 production delta=0 的 Risk 0 边界不运行 focused/full。
