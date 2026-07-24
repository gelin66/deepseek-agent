# M8-I Host typed Auto 路由结论

- 日期：2026-07-24
- baseline：`15fea38e28aa60a9ae51e8e85b38c05ff019781a`
- manifest commit：`97881512ff1de321068fdb1a347026c3f23a194b`
- code candidate：`ef65bafa`
- manifest：`eval/manifests/m8-i-host-auto-route-v1.json`
- 决策：`keep_explicit_models_and_host_policy /
  delete_prompt_classifier_and_unknown_billing_preroute_debt /
  hold_auto_default_admission`
- Key：未读取
- 官方 DeepSeek API 请求：0

## 1. 真实问题

旧 `model=auto` 在 canonical Runtime 创建前额外发送一次 non-streaming
`deepseek-v4-flash` Chat 请求：

- output budget 128；
- Thinking Off；
- 4 秒 timeout；
- production `recent_context=""`；
- classifier 同时猜 model、thinking 和无产品意义的 provider；
- 失败后依赖关键词集合和 500 字阈值；
- root 与全部 child 继承整棵任务树的同一个选择。

该请求占用 root/child 共用的 physical request ledger，并在 `RunCreated` 前制造可能已经
计费却没有 canonical run 的 crash 窗口。既有测试只证明 parser、fallback、预算和恢复
机制，没有证明相对 fixed Pro 的 verified success 非劣。真实问题因此是错误的产品 owner
与多余网络副作用，不是 classifier prompt 还不够好。

## 2. 切片合同

| 项目 | 结论 |
|---|---|
| 单一 owner | `crates/app::ProductionModelRoutePolicy` |
| backend 边界 | `crates/deepseek` 只保留官方 model capability、Chat planner、transport/parser/accounting |
| root | Auto 固定 Pro；普通 high，typed recovery max |
| read-only child | 普通 Flash/high；typed failed-handoff recheck Pro/max |
| explicit Writer | 继续 explicit-only；Auto selection Pro/high，typed rework Pro/max |
| 显式选择 | model/reasoning 原样保留，不经过 Auto |
| root Flash | 当前无 typed bounded-low-risk/no-tools product fact，不准入 |
| mid-run switch | 不实现；每个 RunRequest immutable |
| cutover | 真实 caller 迁移后物理删除 classifier、heuristic 和 unknown-billing preroute |

原则是：**Pro 负责，Flash 调查，Host 验证**。

## 3. 实现与删除

### Host policy 与 canonical replay

Run API v11、RuntimeEvent v17、State schema v22 增加强制
`ModelRouteAudit`：

- requested model mode；
- requested reasoning；
- Host policy version；
- stable reason code。

selected model/reasoning 继续使用 `RunRequest` 的既有 canonical 字段，actor/workspace
authority 继续使用 `RunRequest`/`AgentTask`，没有第二份派生状态真相。每个 child route
先冻结进 `AgentTask`，再由同一个 `AgentRuntime` 构造 exact child `RunRequest`。execution
fingerprint 绑定 route policy identity；SQLite reopen 校验并原样恢复，不重新选模。

State v22 无法从 pre-v17 的 selected model 诚实推断 caller requested mode/reasoning，
因此直接退役旧 materialized run。pending Start command 保留，因为 Host policy 在
`RunCreated` 前不再发模型请求，可安全恢复同一 reserved Run。

### 物理删除

- `crates/deepseek/src/auto_route.rs`：530 行；
- Flash classifier request、prompt、parser、provider DTO；
- 关键词/500 字 heuristic 与空 `recent_context`；
- `DeepSeekAutoRouteFailed` Run API reason 和 exec route startup 分类；
- `CreationIntent::is_unknown_billing`、pending creation unknown-billing API/TUI projection；
- 旧 unknown-billing PTY guard 和失去生产者的中文文案；
- 测试中“classifier + root 两次请求”的旧账本假设。

保留的是 terminal/model-attempt 的真实 `billing_unknown` accounting；它仍用于在途模型
请求 crash，不能与已删除的 pre-route creation 债混淆。

code candidate 为 28 files、`+1177/-1117`，净增加 60 行。新增主要来自 route audit、
State v22 migration、root/read-only/Writer/reopen/HTTP/exec 反例；生产上删除一个网络
请求 owner、完整 prompt/parser/heuristic 模块和一条 recovery 分支。由于正式质量/效率
A/B 未准入，不把结构收敛或净行数包装成产品收益。

## 4. 离线证据

production loopback 观察到四个真实 Chat request：

1. Pro root 启动两个只读调查；
2. Flash read-only child A；
3. Flash read-only child B；
4. Pro root 汇聚并经 Host completion gate 完成。

同一测试从 SQLite 重开，证明 root/child model、reasoning、route audit、AgentTask binding
和 request accounting 原样重建。另一个 exec production test 证明：

| 指标 | 旧 classifier | Host policy |
|---|---:|---:|
| first root turn 前 physical requests | 1 | 0 |
| `RunCreated` accounting baseline started | 1 | 0 |
| root execution requests | 1 | 1 |
| Auto root model | classifier output | Pro |
| creation 前 unknown-billing 窗口 | 有 | 无 |

无 Key app-server 测试先留下 durable Auto Start reservation，观察 0 个模型请求；补齐
fixture credential 后通过 HTTP 恢复 exact reserved Run。process crash/reopen 同时覆盖
pending creation、model in-flight、ToolPrepared/outcome、read-only child、Writer、
verifier 与 terminal exactly-once。

门禁通过：

- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`；
- tools 347 pass / 1 ignored；
- DeepSeek 51/51；
- Runtime conformance 82/82；
- App 57 pass / 1 ignored；
- app-server 23/23；
- exec process 25/25；
- canonical TUI Run 18/18；
- canonical PTY 6/6；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- State process crash 38 pass / 1 ignored；
- M7-C Harness self-test；
- DeepSeek live Harness self-test；
- M8-E frozen contract 8/8；
- `git diff --check`。

所有 Cargo 命令使用
`CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8i-target`。

## 5. 正式 A/B 准入

manifest 冻结：

- A：fixed Pro；
- B：fixed Flash；
- C：`15fea38e` old classifier；
- D：Host typed policy；
- 每 cell 至少 3 次，balanced interleaved，`maximum_reruns=0`；
- fixed TaskContract、tool catalog、budget、verifier 和完整逐 arm accounting；
- fixed Pro 非劣、false success=0、关键 strata 无新增 Pro-only success；
- 质量通过后费用或 wall time 稳定净改善约 20%。

本轮没有读取 Key。原因不是没有 treatment，而是四 variant 无法满足同
revision/immutable binary identity：

- C 只存在于 frozen baseline `15fea38e`；
- D 只存在于 code candidate `ef65bafa`；
- cutover 后同一 binary 不再包含 old classifier；
- 为了 live A/B 恢复 classifier toggle 会重新引入第二 route owner、产品模式和已经删除的
  pre-route 网络/恢复路径。

准入结论为
`inadmissible_no_single_binary_four_variant_surface`。因此没有 verified success、false
success、Token、cache、费用或 wall-time 产品指标，也不能宣称 Flash child 已证明省钱。

## 6. 产品决策

保留：

- 显式 `deepseek-v4-pro` / `deepseek-v4-flash`；
- 显式 reasoning；
- app-owned typed Host policy；
- Auto root Pro、read-only Flash investigation、Pro recheck/Writer；
- exact route audit、replay、request ledger 与 terminal accounting。

删除：

- prompt-only Flash classifier；
- classifier prompt/parser/heuristic/provider DTO；
- pre-RunCreated classifier request 与 creation unknown-billing recovery；
- whole-tree child model inheritance；
- 失去生产者的 API/TUI/测试债。

暂缓：

- Auto 成为产品默认；
- root Flash；
- 同一 Run 内 cascade/switch；
- learned router。

产品默认继续是 `deepseek-v4-pro`。未来只有新 successor 在同 immutable identity 下通过
完整质量门和约 20% 效率门，Auto 才能成为默认。若未来 RunStore 积累足够真实配对标签，
可以另行实验本地可拒答 estimator；不得恢复“再调用一次 LLM 当 router”。

## 7. 官方事实复核

复核日期：2026-07-24。

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)：Pro 强调 Agentic
  Coding，Flash 面向简单 Agent；该定位只用于设计保守先验，不替代 CodeWhale A/B。
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)：当前 Pro
  cache-miss input/output 单价约为 Flash 的 3.1 倍；费用差异只有在质量门通过后才可用于
  admission。
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)：
  当前 production endpoint 是 `https://api.deepseek.com/chat/completions`，官方 V4
  model ids 为
  `deepseek-v4-pro` / `deepseek-v4-flash`。
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)：普通 effort 默认
  high，复杂 Agent request 可提升；CodeWhale 不让 Auto 自行选择 off。
- [Agent routing with harness state](https://arxiv.org/abs/2607.11399)、
  [large-scale routing benchmark](https://arxiv.org/abs/2601.07206) 与
  [quality-estimation routing](https://arxiv.org/abs/2410.10347) 支持使用 execution
  state/quality estimate 和 abstention，而不是只按首条 prompt 关键词猜测。

## 8. 非结论与下一切片

本切片不证明：

- Flash read-only child 相对 Pro verified success 非劣；
- Auto 降低总费用或 wall time；
- typed recovery 提升 max 的收益；
- mid-run cascade、local learned router 或 root Flash 应当实现；
- fixed Pro 默认应被替换。

下一切片应先基于 clean M8-I checkpoint 重跑 V1 exit successor audit，确认 M8-G/H/I
关闭或收窄了哪些旧 blocker，再只选择一个仍有真实 consumer 的 blocker。优先检查 V12
剩余 no-consumer provider/legacy state vocabulary；不把 imported-baseline accounting
缺失、FIM hold、中文 prompt unknown billing 或 Auto quality debt混成一个实现切片。
