# M9-A Host Auto release admission（2026-07-24）

## 结论

M9-A 的产品决策是：

```text
keep Host typed policy
keep explicit deepseek-v4-pro / deepseek-v4-flash / reasoning
keep default fixed deepseek-v4-pro
hold Auto default admission
delete experiment-only runner
```

本轮没有把 Auto 包装成默认能力。正式 27-arm suite 在第 18 arm 遇到无 response
headers、无 response、无 usage 的 typed transport failure；canonical accounting
明确记录 `billing_unknown=true`、`complete=false`。按冻结的
`maximum_reruns=0` 与 stop-before-next-arm 规则，Harness 在派生该 arm 结果和启动第
19 arm 前停止。未补样、未完成 mate、未续跑或拼接历史结果。

## 冻结身份与真实问题

- 起始 release-ready checkpoint：`3770feba`；
- M9-A contract：`32dad0f6`；
- production caller correction：`ed5e9fe4`；
- immutable production/evaluator candidate：`29c4980ff4f6ffa682dd294f2503ca2691490d50`，
  tree `0496fbbd746d2f798d2a63e40b13547e89a63145`；
- live admission：`5032e4a4`；
- release binary：`codewhale 0.8.68 (29c4980ff4f6)`，
  SHA-256 `73834d742c291dedfe502ad9001d9c48896512deaad11ec97d7fe81411ebe2b8`；
- Run API v11、RuntimeEvent v17、State schema v23、exec-stream v3。

真实问题不是“是否让 Flash 多做工作”，而是 M8-I 的 Host typed Auto policy 尚未相对
fixed Pro 获得正式质量非劣与效率收益证据。审计确认真实同 binary delta 只发生在普通
read-only child：fixed Pro 的 root/child 都用 Pro；Host Auto 的 root 用 Pro、普通
read-only child 用 Flash。root-only 与 ordinary Writer 没有 model delta，因此只做
离线 conformance，不进入付费收益 cell。

审计还发现交互 TUI 在 `model=auto` 时会把显式 reasoning 强制重写为 `auto`。先写失败
测试后，M9-A 删除了该重写；CLI、TUI、HTTP/stdio 现在都把 model intent 与 reasoning
intent 独立投影。默认未配置 reasoning 仍为 Auto，Host policy 的普通路径仍解析为 high。

## 保留的单一 production 路径

- `crates/app::ProductionModelRoutePolicy` 仍是唯一产品路由 owner；
- Auto root 为 Pro，普通 read-only child 为 Flash，typed recheck 为 Pro；
- explicit Writer 仍 explicit-only，普通/rework 都选 Pro；
- child route 冻结在 `AgentTask`，并与 child `RunRequest` 精确绑定；
- root、read-only child、Writer 仍共用一个 `AgentRuntime`、一个 `RunStore` 和一个
  DeepSeek backend；
- official sender 仍为 OpenAI ChatCompletions `POST /chat/completions`；
- 没有恢复 `auto_route.rs`、额外 Flash classifier request、classifier prompt/parser、
  `recent_context`、关键词/500 字 heuristic、Provider、模式或 mid-run switch。

## 离线门禁

在读取 Key 前通过：

- TUI Auto 保留显式 reasoning 的 fail-first 回归；
- Host Auto typed policy matrix；
- Pro root → 两个 Flash read-only child → Pro root integration 的 production loopback；
- child route、request ledger、usage/cost 与 SQLite exact reopen；
- model-request in-flight 与 committed-terminal process SIGKILL/reopen；
- fail-before-loss Harness hash-chain、自检和四个 SIGKILL write window；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- immutable release build、credential-free 27-arm dry-run、`git diff --check`。

focused 首次运行中，一个已有 established-SSE timing test 观察到 retry-open 的通用
network error，而非预期 stream-stall；未改代码的 exact rerun、第二次完整 focused 与
后续 workspace full test 均通过。

## 正式设计

三个任务复用真实临时 Git repo 与 deterministic verifier：

1. capability intersection；
2. dependency impact；
3. layered policy resolution。

三个 variant 是 fixed Pro（唯一质量基线）、fixed Flash（诊断）与 Host Auto（候选）。
每个任务/variant 原定 3 次，共 27 arms；同一 binary、TaskContract、fixture、agent 工具
目录、权限、budget、reasoning=high、verifier、retry policy 与
`maximum_reruns=0`。每个任务恰好启动两个同 batch 的 read-only child。raw 使用
`O_EXCL|O_APPEND|O_NOFOLLOW`、0600、逐 record fsync 和 previous-record SHA-256 chain；
terminal、canonical store、无凭据 SQLite reopen、external verifier 必须先落盘，最后才
允许 route/accounting/product metric 派生。

## 正式停止证据

ignored raw：

- `eval/results/m9-a-host-auto-release-admission-29c4980ff4f6-v1.jsonl`；
- mode `0600`；
- 41,339,450 bytes；
- 110 个完整 hash-chained records；
- raw SHA-256
  `0528f77ab6494209dd877a01a538acb6c8c187ed9c87a51680737c3eee7b748a`；
- final record hash
  `14dfdc02634da3629b534f20ef9fca97f6e01a56566e287cf5e6dce9eebea857`。

停止前产生 17 个完整 arm results：fixed Pro 6、Host Auto 6、fixed Flash 5。17/17
verified success，false success 0，route/lifecycle/reopen/accounting 17/17 有效。第 18
arm 是第二轮 t1 的 fixed Flash 诊断臂。17 个完整 arms 的 201 次已结算请求合计
1,099,288 input tokens、64,052 output tokens、known cost `$0.213945915`；第 18 arm
另有 1 次不可证明计费的 attempt，因此全 suite 不能把它计为 202 次 known-cost 请求：

```text
terminal = failed(deepseek_transport)
response_headers_received = false
content / finish_reason / usage = false
root started/completed/in_flight = 1/1/0
surface responses = 0
billing_unknown = true
accounting complete = false
retry decision = stop(retry_limit_reached)
```

它的 terminal、canonical store、无凭据 reopen 与失败 verifier 都已 durable；因 billing
不完整，没有生成 arm result。

已完成的 6 个 Auto/Pro pair 只能作为停止前描述，不能成为 product aggregate：

- 两边 6/6 均 verified，false success 0；
- Auto cost 4 胜 2 负；
- 描述性 Auto/Pro 总费用 ratio `0.8698`，只下降约 13.0%；
- 描述性 Auto/Pro wall ratio `0.8922`，只下降约 10.8%。

即使忽略 formal matrix 不完整的问题，这两个描述性 aggregate 也未达到冻结的约 20%
效率门。不得据此宣称正式非劣、正式失败、默认 admission 或稳定收益。

## 删除与产品决定

M9-A 保留 TUI caller correctness fix、冻结 contract/live-admission、本 summary 和 ignored
0600 raw。失去消费者的 1,551 行 M9-A live runner 在记录 exact Git/hash identity 后物理
删除；它不进入长期 benchmark framework。没有 production experiment toggle、第二 route
owner、第二 Runtime/Store 或模型可见同义工具需要回滚。

产品默认继续固定 `deepseek-v4-pro`。显式 Pro/Flash/reasoning 保留；Host Auto 机制继续
作为显式 hold surface，普通 read-only child 可用 Flash，但不附带质量或节省声明。
下一次 Auto admission 必须是新的 position-1 successor，不得续跑、补 mate 或拼接本 raw。

## 官方复核

复核日期为 2026-07-24：

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)；
- [current models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)；
- [Thinking mode](https://api-docs.deepseek.com/guides/thinking_mode/)。

官方定位支持 Pro 负责 Agentic Coding、Flash 适合简单 Agent，但不能替代 CodeWhale
same-task fixed-Pro quality gate。当前 production 继续使用最新官方 V4 model IDs 与
ChatCompletions endpoint；没有恢复退役 alias、Anthropic Messages 或 FIM surface。
