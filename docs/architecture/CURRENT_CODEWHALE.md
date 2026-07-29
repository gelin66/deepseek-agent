# Current DSE Architecture

> 文档类别：迁移事实。只描述当前源码，不替代
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md)、
> [ROADMAP.md](../product/ROADMAP.md) 或 ADR。

- 快照日期：2026-07-29
- 导入基线：`352e86a611fdf3cd8bd27c36d24d482c06a71117`
- workspace version：`0.8.69`
- M4-B 被测代码：commit `a534a824670b60c807c5abf399ea8674d4beb527`，tree
  `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8`
- M4 最终代码检查点：`65fa88ba`
- M6-A 代码与真实 DeepSeek Writer canary 检查点：`a982a9a8`
- M6-B1 v2 正式 A/B candidate：`5d72ae94`
- M6-B1 rework v3 candidate：`3310aa73`
- M7-A production candidate：`24c8a530`
- M7-A2 canonical JSON correctness checkpoint：`570212ae`
- M7-A2 evaluator checkpoint：`1077fe93`
- M7-A3 incomplete stream correctness checkpoint：`e98ca5ae`
- M7-A3 Harness checkpoint：`db381889`
- M7-B production/code conformance checkpoint：`11230b8d`
- M7-B final production candidate：`e6b4b64d`
- M7-B final admission Harness checkpoint：`7e7eaadf`
- M7-B post-decision shrink / SQLite reopen proof：`4e3536f1`
- M7-C canonical tools correctness checkpoint：`7613073c`
- M7-C non-canonical edit cutover / final Harness checkpoint：`9cba8b53`
- M7-D v16-native observation projector checkpoint：`7ddf3bba`
- M7-D failure-truth / final Harness checkpoint：`cf9b3fd6`
- M7-E thinking admission production candidate：`b9b83cdf`
- M7-E last live evaluator checkpoint：`ee73e761`
- M7-E fail-closed hold / final Harness checkpoint：`458c3d7d`
- M7-F context-cache wire baseline checkpoint：`a1d68b05`
- M7-G read-only child recovery checkpoint：`13b94210`
- M7-G formal candidate：`062623e6`
- M7-G post-decision Harness hardening：`8763722c`
- M7-G2 fail-before-loss observer checkpoint：`f89dafc5`
- M7-H terminal catalog correctness checkpoint：`eb8763a1`
- M7-H final decision checkpoint：`e46ae822`
- M7-I current-revision baseline checkpoint：`f8d0b242`
- M7-I test-only strict lint checkpoint：`b1894069`
- M8-A CLI DeepSeek-only cutover：`e1a611ff`
- M8-A TUI Provider route deletion：`63246e72`
- M8-A config Provider model deletion：`00dcda0c`
- M8-B product identity cutover：`eddfd4bc`
- M8-B imported updater deletion：`ccc98245`
- M8-B CodeWhale state-path cutover：`d792113e`
- M8-B reproducible delivery candidate：`307f6c09`
- M8-C shared fixed zh-Hans owner：`062a747d`
- M8-C fixed zh-Hans production candidate：`44b17940`
- M8-D app-server prompt caller / immutable binary checkpoint：`8371b6dd`
- M8-D final evaluator checkpoint：`a51e145f`
- M8-D final live admission checkpoint：`26841208`
- M8-E frozen V1 gap contract / locked-offline artifact：`a12bea45`
- M8-E frozen-input resolver checkpoint：`f6063a7b`
- M8-G single TaskGraph production candidate：`64f6bc16`
- M8-H unreachable FIM half-branch deletion candidate：`7d9aa9a6`
- M8-I Host typed Auto route candidate：`ef65bafa`
- M8-J frozen V1 successor / V12 contract：`9e644add`
- M8-J legacy Thread truth deletion candidate：`bcbc1616`
- M8-K frozen V1 scope successor：`b44d7ff9`
- M8-K accepted scope candidate：`6a99cb79`
- M8-L frozen release benchmark successor：`14319b11`
- M8-L accepted release benchmark candidate：`d27553c4`
- M8-M prompt successor Harness：`d723d0b3`
- M8-M fixed-Pro live admission：`d60d5e52`
- M8-N V15 release-scope successor contract：`1a656bee`
- M8-N accepted release candidate：`498599dd`
- M9-A Host Auto immutable campaign candidate：`29c4980f`
- M9-A Host Auto live admission：`5032e4a4`
- M9-B fixed-Pro regression candidate：`983fa9ce`
- M9-B fixed-Pro live admission：`37c4cc96`
- M9-B read-only observer correction：`9342fb03`
- M9-C fixed-Pro successor contract：`d79b2a36`
- M9-C corrected Harness / immutable candidate：`7a91bbaa`
- M9-C fixed-Pro successor live admission：`84e20cd9`
- M14 typed observer conformance / M13 live path cutover：`7a9e2278`
- M17-A DSE product/Cargo identity cutover：`89f1bb9f`
- M17-B DSE config/state/protocol identity cutover：`2b6dd276d`
- M17-C DSE locked/offline delivery and CI cutover：`fd23400ca`
- M17-D DSE bilingual localization owner：`68f3aa739`
- M17-E DSE bilingual human projection cutover：`6464fe155`
- M17-F DSE bilingual prompt formal candidate / cutover：`73d02d05e` / `c1856fa4b`
- M17-G DSE identity-marker / nested-help / public repository：
  `389aac896` / `de6bc7004` / `85e241223`
- 当前阶段：M4 已关闭；M5-A canonical TaskContract/EvidenceReceipt 与 M5-B
  evidence-aware ContextBroker 均已完成正式 DeepSeek A/B。M5-B 已 shrink 为 hard-limit
  safety；M6-A 单 Writer isolated worktree 闭环已完成；M6-B1 v2 正式 A/B 判定
  `reject_and_rework`。rework 已完成，v3 因一次真实 transport attempt 的计费不可证明按
  预注册规则判定 `hold_mechanism`；Writer 保持 explicit-only，M6-B2 不准入。M7-A 已完成
  verifier contract 单 owner 与 typed completion recovery，正式 v1 因 Rust/Python
  canonical JSON 口径失效判定 `hold`。M7-A2 已修复唯一 canonical JSON owner 并完成公平
  shared-fix 复评；正式 suite 在 7/40 arms 后因一个真实 incomplete response/accounting
  停止，仍为 `hold`、不具备产品指标资格。M7-A3 已补齐 typed stream failure、增量 usage
  accounting 与 replay-safe retry；完整离线门禁通过，但旧 control/treatment 无法建立保持
  原 production delta 的公平 shared fix，因此没有读取 Key 或执行 live 复评，M7-A 继续
  `hold`。M7-B 已完成 Strict 整目录判定和 typed 工具失败恢复；六个默认可执行 actor 均无
  Strict treatment surface，正式 live A/B 在 credential/API 前判定不准入。M7-C 已完成
  canonical 编辑基线和 Host correctness 修复；tools contract 从 3/12 到 12/12，CLI direct
  apply 与 TUI-local eval/edit 绕行已删除。production 没有 canonical FIM caller，FIM live
  A/B 同样在 credential/API 前判定无 surface delta，Key 未读取。M7-D 已把单变体
  observation WIP 收敛为 v16-native offline projector，删除 v14 adapter、历史 executor
  monkeypatch 和无 delta live/Key 路径；final Harness 14/14 clean gates 通过，但没有新的
  production 模型样本或 editor treatment admission。M7-E 已在同一 Standard Chat production
  binary 上冻结 `reasoning_effort=high/off`，但四次 live 尝试分别暴露 evaluator recovery
  错判与 paired workspace identity 缺陷；v4 外部停止还留下 active-arm unknown billing。
  final v5 固定在 Key/API 前 fail closed，默认 thinking 行为不变，产品结论为 `hold`。
  M7-F 又证明每轮 ephemeral Host-facts tail 不进入下一请求历史，导致 provider
  request/output boundary 不能逐轮严格延伸；fresh revision/evidence、wire plan 与 SQLite
  reopen 均保持正确。没有候选能在不增加 stale-fact 语义的前提下形成安全 wire delta，
  因而 production 不变、Key 未读取、官方请求 0，决策为 `hold`。
  M7-G 证明同一 response 中的多个 read-only child 已由 canonical Runtime 真正重叠执行，
  不需要第二 scheduler；同时修复 durable `ChildStarted` 后 recovery terminal 被 Store
  拒绝的 crash/reopen correctness 缺陷。正式 9 对 / 18 arms A/B 在首个联网 control arm
  后因 Harness verifier-plan identity 错判停止，accounting 未写入 raw、费用不可证明；
  按 maximum_reruns=0 未续跑。结果为 `hold / inadmissible_observer_identity_bug`，不具备
  产品指标资格，explicit read-only child 行为与默认 admission 均不变。
  M7-G2 已把未来 evaluator 顺序冻结为 terminal snapshot、无 Key SQLite reopen snapshot、
  verifier snapshot、最后才派生 arm result；11 个 observer exception/raw/SIGKILL window
  全部离线通过。旧 raw 保持 immutable unknown billing。由于 M7-G candidate 后没有新的
  fan-out production delta，本阶段未读取 Key、未调用 API，paid successor 判定
  `inadmissible_no_new_production_delta`。M7-H 随后修复 terminal `tools=[]` hard-limit
  admission 错用 ordinary catalog 的确定性 Host 缺陷，但没有发现可准入的 broader
  request/Token model treatment。M7-I 在 current v16/v21 上把分散的 near-limit evidence
  冻结为 13 项矩阵和 16 个 exact gates；root、read-only child、explicit Writer 的 actual
  catalog、context estimate 与 DeepSeek RequestPlan 均能从 SQLite reopen 精确重建，
  mandatory facts 超限在 0 次模型请求前 fail closed。没有新的 production delta，Key 未读、
  API/网络请求为 0；M7 request/Token 调优关闭，M8 DeepSeek-only 产品清理已准入。
  M8-A 随后让 CLI、app-server 与交互 TUI 只接受同一根级 DeepSeek
  credential/endpoint/model 配置，并删除 `crates/agent`、generic Provider 参数、Provider
  OAuth、model catalog/pricing/alias、fallback 和 route resolver。首次启动、无 Key、
  credential precedence、非法 Provider/模型、Doctor/onboarding、PTY、resume/reopen 与
  exec/HTTP/stdio parity 全部离线通过。没有模型 treatment，Key 未读取、官方请求和网络
  访问为 0。
  M8-B 随后把正式 binary set 固定为 `codewhale`、`codewhale-tui`，让 config/state/
  settings/secrets 只使用 CodeWhale namespace，并建立唯一 locked/offline local delivery
  owner。`crates/release`、imported updater/CNB discovery、`codew`、旧产品 env/path
  compatibility、第二 metrics truth 和未接线部署资产已删除。candidate `307f6c09` 的真实
  macOS artifact 在 OS 禁网下完成安装/验证/Doctor/卸载，Linux 同一 lifecycle 在缓存
  container `--network none` 下通过；用户数据保持不变。没有模型 treatment，Key 未读取、
  官方 API 请求 0。一次候选后 metadata 断言的 rustup 更新探测被立即中断并记录，不能把
  delivery no-network evidence 扩张为整个 Agent session 的 no-network 结论。
  M8-C 随后把 TUI 私有 localization wrapper/catalog 收敛为 CLI/TUI 共享的唯一 fixed
  `zh-Hans` owner；CLI/TUI/app-server help、Doctor、Headless/recovery、
  `request_user_input` 与多 Agent Host chrome 使用同一 427-key catalog。machine JSON/
  NDJSON/API、命令/flag/enum、模型/工具 ID、路径、代码和 raw provider/tool/stdout/stderr
  保持原样。L01-L14、foreign locale、80/120 列 CJK、两次 TUI、crash/reopen 与真实
  `44b17940` 安装包回归通过；protocol/runtime/state/app-server 相对 baseline 零差异。
  没有模型 treatment，Key 未读取、官方 API 请求 0。
  M8-D 随后只评测一个 constitution prompt treatment。真实 process audit 发现 app-server
  未加载已有 context-owned override；`8371b6dd` 已让 TUI/exec/app-server caller 收敛，
  没有新增 prompt owner、模式或 Runtime。所有 successor 绑定同一
  `deepseek-v4-flash` immutable binary pair 和当前官方
  `https://api.deepseek.com/chat/completions`；旧 `deepseek-chat`/
  `deepseek-reasoner` model alias 没有使用。v1-v4 的 evaluator identity/projection 缺陷
  分别被新 schema 修正且旧 0600 raw 不覆盖；final v5 已离线覆盖 nested AgentTask、
  aggregate accounting、五个 fixture 与 integrated Writer `base..HEAD` scope，但首个
  live arm 的 billing unknown，按门禁立即停止。production bundled prompt 未改变，结论为
  `hold_prompt_candidate / keep_app_server_override_consistency`。
  M8-E 随后冻结 16 项 V1 exit matrix：8 pass、8 blocked，当前发布状态为
  `not_releasable`。M8-E frozen manifest/result 曾记录未经授权的 Anthropic Messages
  release premise；用户已明确否认，PRODUCT_PLAN/ADR 也从未接受。该 frozen 字段保留作
  历史审计，不再计入 release gap。current `crates/deepseek` Chat
  sender/parser/replay/accounting 是正确 production 路线。clean `a12bea45` 的
  locked/offline artifact 只含 `codewhale` 和 `codewhale-tui`，全量 offline
  conformance 和安装验证通过；Key 未读取、官方请求 0。M8-F Messages cutover 为
  `canceled_invalid_premise`，没有 production 代码接管。
  M8-G 随后物理删除无 canonical caller 的 Fleet/Lane 产品、协议、状态和 process shell，
  V06 关闭为 pass。M8-H 又复核 M7-C 以后没有新的 current-v16 production 编辑失败，
  deterministic Host matrix 仍为 12/12；旧 FIM planner 生成 sender 不拥有的
  `/beta/completions`，唯一完整 parser 又只读取 Chat `choices[].message`，因此它是无
  consumer 的 production 半分支，不是可运行 treatment。candidate `7d9aa9a6` 已删除
  FIM planner/error/surface/accounting、always-zero terminal 字段和 eval-only classifier，
  并让 config 成为 V4 model id 的唯一规范化 owner；冻结历史证据不改写。没有读取 Key，
  官方请求为 0。
  M8-I 又把 `model=auto` 从额外 Flash prompt classifier 收敛为
  `crates/app::ProductionModelRoutePolicy`；prompt/parser/heuristic、pre-RunCreated
  unknown-billing 分支和 whole-tree route inheritance 已删除。
  M8-J 随后重算 current V1 successor，并删除仍可见但断开 canonical RunStore 的
  `codewhale thread`、SQLite `threads` metadata 表、`session_index.jsonl` sidecar 和
  无 production consumer 的 Thread/App/Prompt/EventFrame protocol 岛。State v23 在同一
  迁移事务中删除旧表并保留 current run replay、pending Start、route audit 与 accounting；
  V12 关闭，current V1 matrix 为 10 pass / 6 blocked。没有模型 treatment，Key 未读取、
  官方请求 0。
  M8-K 随后按 frozen implementation admission contract 重验 V08/V09/V10。current
  production 没有 multi-Writer/FIM/RepoGraph owner 或同 revision treatment，也没有归因到
  缺少它们的新 failure sample；M6-B1、M7-C/M8-H 与 M5-B 的既有证据不支持为清单恢复
  已拒绝或已删除路径。ADR-0005 因而把 V1 验收收敛为一个显式 isolated Writer 的完整
  lifecycle、Standard Chat + Strict 整目录准入/无损回退，以及 canonical
  search/read/diff + ContextBroker + deterministic verifier 的 bounded cross-file 结果。
  current matrix 为 13 pass / 3 blocked；production source、协议版本和默认 fixed Pro
  均未改变，Key 未读取、官方请求 0。
  M8-L 随后证明 imported `352e86a6` 的 task/model/surface、external fixture/verifier 与
  binary identity 可对齐，但其 exec-stream v1 没有 physical request count、有效 retry
  count、failed/incomplete usage、root/child aggregate、cost completeness/bucket 或
  canonical reopen ledger；同一旧 Engine 又有两层透明 request reissue。paid cross-revision
  A/B 因而在 credential 前判定 `inadmissible_incomplete_baseline_accounting`。ADR-0006
  保留 M5-A 12/12 qualified official DeepSeek coding/false-success evidence，并要求
  exact current candidate 通过 production Git edit、verifier recovery、completion
  rejection、accounting/RequestPlan SQLite reopen 与 process CLI regression。五个共同
  workflow 的用户动作总数为 `5 -> 5`。V13/V16 关闭，current matrix 为
  15 pass / 1 blocked；V15 仍 blocked，V1 仍不可发布。production source/protocol/model
  surface 不变，Key 未读取、official requests 0。
  M8-M 随后以 current `d1d6ca5c` fixed-Pro immutable binary 建立全新 30-arm 中文
  prompt successor；旧 M8-D evaluator/test 被替代。离线 identity、journal SIGKILL、
  focused、workspace clippy/test 与 process reopen 全部通过。正式 suite 前 5 arm
  verified/false-success=0，第 6 arm 的首个 physical request 没有收到 response
  headers/usage，canonical ledger 记录 `billing_unknown=true`，按 frozen
  `maximum_reruns=0` 在 6/30 fail closed。production bundled prompt 不变，
  candidate-only evaluator path 删除；V15 继续 blocked，current matrix 仍为
  15 pass / 1 blocked，V1 不可发布。
  M8-N 随后审计 `PRODUCT_PLAN` 的 candidate admission 与 fixed-Chinese baseline
  release evidence 边界。current constitution 在 M5-A baseline/candidate、M6-A Writer
  canary、M8-M immutable binary 与 current source 中 byte-identical；M5-A 保留
  12/12 product-metric-eligible official DeepSeek coding/false-success arms，M8-M 保留
  三个 exact-current baseline diagnostics，current gates 和 delivery owner 另行证明
  root/read-only/Writer、verifier、accounting/reopen 与 whole-release rollback。
  ADR-0007 接受该 V15 successor，但不改写旧 candidate 质量结论，也不免除任何未来
  model-visible prompt treatment 的 A/B。M8-N 无 material model-visible delta，
  live 准入为 `inadmissible_no_material_treatment`，Key 未读取、official requests 0。
  失去消费者的 M8-D candidate-only test/fixture 已删除；通用 override consistency
  保留。current matrix 为 16 pass / 0 blocked，结论为 `V1 可发布`（release-ready，
  未 push、未发布）。
  M9-A 随后以同 immutable binary 比较 fixed Pro、fixed Flash diagnostic 与 Host Auto；
  27-arm suite 在第 18 arm 因 unknown billing fail closed，默认继续 fixed Pro。
  M9-B 又尝试建立六类 post-V1 fixed-Pro regression baseline；offline gates 全通过，
  但正式 v1 在第 2 个完整 arm 暴露 read-only/Writer observer 分类缺陷并在第 3 arm
  中止。observer 已修复并自测，旧 admission 因 Harness hash 改变自动失效；18-arm
  baseline 仍为 hold，不能续跑或拼接。M9-C 从新 position 1 重开并完成第一轮 6/6
  与第二轮两个 arm；第 9 个 scheduled arm 在 response/usage 前发生 transport failure，
  canonical ledger 记录 unknown billing 并停止。M9-C 同样不续跑、不拼接，18-arm
  baseline 仍为 hold，旧 M6/M7 runner 不删除。
  M9-D 接受 ADR-0008 并退休 Auto 产品方向：model/reasoning Auto 的输入、状态语义、
  Host 分支、显示与 current evaluator 投影已删除；中性 actual-route audit 和显式
  Pro/Flash baseline 能力保留。该范围删除不读取 Key、不调用 API，也不重开 M9-A。
- 当前协议：Run API v15、RuntimeEvent v22、State schema v28、exec-stream v6。产品默认
  固定 `deepseek-v4-pro` + `high`；Auto 产品方向已删除。

<a id="current-core-facts"></a>
## 1. 当前结论

Headless、本地 API 与交互 TUI foreground 已共用一条 Agent 执行链：

```text
codewhale exec --------\
app-server -------------+-> AgentApplication -> AgentRuntime
interactive TUI --------/          |                |
  TuiRunClient                     |                +-> DeepSeekModelPort
  CanonicalRunProjection           |                +-> ProductionToolExecutor
  run_presenter                    |
                                  +-> ProductionAgentOrchestrator
                                  |      +-> AgentRuntime（同一实现）
                                  |      +-> Git isolated worktree
                                  +----------------> SQLite RunStore
```

这三条入口不再拥有各自的模型循环、工具目录、终态判断或持久状态。交互 TUI 只提交
canonical Run command，并从 durable event 投影 root/child 状态。

最终 HEAD 调用图只有一个 `AgentRuntime` 定义和一个普通 Agent request
`ModelPort::stream` 调用点；hard-limit context compaction 完全本地确定，不调用模型。
canonical Agent Run 的 terminal 也只在该 Runtime 提交。`StateStore` 是唯一生产 SQLite
`RunStore` 实现；`InMemoryRunStore` 只用于测试。M6-A 的
`ProductionAgentOrchestrator` 只编排同一 Runtime 和 Git lifecycle，不拥有模型循环、
工具实现、Store 或 terminal。Fleet/Lane 产品、协议、状态和 process shell 已在 M8-G
物理删除；canonical Writer 的 worktree、Agent 模型循环、RunStore 与 terminal 没有另建
兼容路径。

这条统一主链已经具备强 coding/replay/evidence 基础，但 current capability surface 尚不是工程
完全体：没有 canonical Web search、完整 semantic interaction、public action scoped grant、managed
browser login/session、受控 upload/download 或 visual observation。ADR-0018 已把这些记录为当前
能力缺口，而不是永久安全边界；当前源码仍保持 fail closed，后续只能经同一主链和风险分级
权限补齐。

旧生产例外 `workflow -> workflow-tool -> WorkflowTool -> SubAgentRuntime ->
DeepSeekClient` 已物理删除；同时删除 Workflow/Workflow-JS crate、私有 JSON/JSONL
写入链、TUI 面板/事件/审批/触发和专属 SubAgent adapter。没有建立兼容桥或双写。旧命令
会在配置解析、TUI、Store 和模型初始化前 fail closed；显式
`--prompt "workflow ..."` 仍是合法自然语言输入。

因此当前生产可达的根/只读子 Agent/Writer 子 Agent 模型循环只剩 canonical
`AgentRuntime`。单 Writer worktree、验证、集成和清理已由唯一 Orchestrator 接管；
尚未实现的完整 DAG、多 Writer 并发和自动冲突处理也只能继续扩展该 owner，而不是保留或
恢复旧 Workflow Runtime 的理由。

旧 TUI `SubAgentRuntime`、`SubAgentManager`、`agents_*` 协调工具、私有 mailbox/
checkpoint/state、旧 child `ToolRegistry` 以及不可达 `/subagents` modal 也已物理删除。
Fleet 曾保留但从未接入的 `SharedSubAgentManager` 可选字段和 worker projection 同步删除；
Fleet 的真实执行仍是 `FleetExecutor -> codewhale exec`。只有 route、reasoning、prompt 和
全局 `FleetExecConfig` allow/deny 进入实际 argv；task-level role/tool scope 尚未执行，
所以仅为其声明 receipt 存在的 `WorkerRole`/`WorkerRuntimeProfile`/
`FleetWorkerRuntimeSpec` 已删除，`effective_permissions` 在 M6 enforced policy 接管前留空。

旧 TUI Goal/Hunt loop、私有 TaskContract/receipt/Goal completion store、Slop ledger 和
custom-command allowed-tools/pause 假状态也已物理删除。M5-A 没有恢复这些状态，而是在
`protocol/runtime/state` 中建立唯一 TaskContract/EvidenceReceipt/Host completion 链路；
历史测试只作为反例，仓库不存在旧类型 adapter、镜像 Store 或第二 completion loop。

M7-A 现在由 production composition 在 Run 创建、继续和恢复边界调用唯一
`ProductionToolExecutor` resolver，把调用方 verifier parameters 解析成实际执行的 frozen
plan；Runtime 的 Host verification 复用该 exact spec。旧的 caller/Host/recovery 三份 plan
推断已被替代。当前 RuntimeEvent v22 与 State schema v28 继续持久化 v16/v21 引入的
completion rejection typed `cause` 和 `required_transition`，恢复只能消费当前 generation
的 exact rejection 事实；
root、只读 child 和 Writer 没有因此分裂出新的 Runtime 或 completion owner。

WorkSurface 现在只投影 canonical child Agent，并保留 top/left/right 布局。旧键盘/鼠标
handler 从未接入生产事件循环，却生成不存在的 `/task` 与 `/jobs` 命令；该交互岛及其焦点、
选择、滚动、打开、停止和 hitbox 状态已物理删除。没有生产 TUI-local state writer、没有
RunStore 表或 RuntimeEvent 的 TUI-local Plan/Todo Store、`App.task_panel`、假工具及其
sidebar/footer/transcript reader 也已删除。工具 Activity 继续读取 `run_presenter`
产生的 canonical `GenericToolCell`，多 Agent 展示继续读取 canonical `child_agents`。M5 的
TaskContract/EvidenceReceipt 不通过恢复这些私有状态实现。
WorkSurface 不拥有 Runtime、Store、工具执行或 completion 判定。

旧 `ModePickerView` 与 `StatusPickerView` 没有生产构造或打开入口，只有模块内测试；两者及
其专属 View event 已删除。底层 `StatusItem` 与 footer 状态投影仍由现有真实调用方拥有；
没有执行语义的 `AppMode/default_mode` 启动标签链也已物理删除。

Headless `exec --auto` 现在选择 canonical `RunPermissionMode::Agent` 并启用工具；Host
判定 critical 的调用仍会产生 Ask，而非交互执行会 fail closed。普通交互 TUI 默认 Ask，
只有显式 process-local `--yolo` 选择 FullAccess。权限在 Run 创建时冻结并随
execution fingerprint/RunStore 重放；旧 `auto_approve`、`trust_mode`、sandbox/elevation
配置不再是 reader 或并列真相。

旧 `ConfigView` 同样没有生产构造、打开入口或 canonical 命令；其 2,000 余行编辑/筛选/
渲染岛和专属消息已删除。底层配置仍从文件和环境加载，Doctor 指向实际配置文件。
旧 `ThemePickerView` 及其 `settings_picker` 框架也只有自测构造；两者、无消费者的
`ConfigUpdated` 事件与专属消息已删除。主题解析、持久化加载、Ocean 渲染与主题对比测试
继续使用同一组 `ThemeId`/`UiTheme`。
旧 `FeedbackPickerView` 也没有生产入口；它及其私有 command-palette 事件、失效的
`Ctrl+K` 帮助项已删除。输入 `/` 打开的 canonical slash menu 不受影响。
旧 `FilePickerView` 和 `file_picker_relevance` 只有自测与一个无人调用的 opener；modal、
事件和失效的 `Ctrl+P` 帮助项已删除。生产 `@mention` 菜单只通过 canonical key handler
补全 composer 文本，候选使用 `Workspace::completions` 的确定性排序；接受候选不会提交、
读文件或构造隐藏上下文，下一次 Enter 才原样发送 `@path`。无人消费的内联 renderer、假
pending-context 预览与 `file_frecency` 已删除，历史磁盘文件不做破坏性清理。
旧 `prompt_suggestion` 模块没有生产调用者，却直接请求任意 `/chat/completions` 并绕过
`AgentRuntime`、`RunStore` 与统一 accounting；该潜在第二模型请求路径、永不写入的 ghost
text 状态和配置已删除，composer 继续显示确定性的中文空输入提示。
旧 TUI `schema_sanitize` 只有自身测试且不在 production request path；这份重复 sanitizer 已
删除。DeepSeek Strict Function Calling 的整目录兼容性判断、原子 fallback 与 Beta Chat
路由仍由 `crates/deepseek` 唯一拥有。
同目录的 `schema_canonicalize` 也只有自身测试，从未处理 MCP 或 production 请求中的工具
schema；该 207 行假缓存优化已删除。真正的请求前缀稳定性只按 canonical DeepSeek 投影与
实际 cache 命中证据评估，不保留未接线的重复变换器。
旧 TUI `ResourceTelemetry` 整模块通过 `#[allow(dead_code)]` 隐藏且没有生产调用方；其
budget pressure、格式化和估算 token throughput 只由自身测试调用，App 中对应的
`last_output_throughput` 也只有初始化与清空、没有 producer 或 renderer。该模块和写空字段
已删除。canonical Runtime/RunStore 的 usage/accounting、DeepSeek usage 账本，以及 presenter
真实消费的 token/cache/reasoning/cost 状态不变。
旧 TUI `ContextBudget` 也只有自身测试和一个零调用的 `route_context_budget` wrapper，文件
注释明确其 engine/TUI consumers 从未接线。该 505 行 foundation 与 wrapper 已删除；仍有
真实调用方的 route context window、canonical DeepSeek output limit、`crates/context`
projection 以及 Runtime/RunStore 的预算、恢复及 accounting 保持原 owner；后续 M5-B 已把
旧摘要 lifecycle 收缩为当前 hard-limit 本地投影事件。
`route_runtime` 现只保留生产 `resolve_route_candidate` 与 context override：交互 TUI 用它
建立 active route limits，Fleet 用它生成 `FleetResolvedRoute` receipt。没有生产调用方的
`ResolvedRuntimeRoute` 配置快照包装、`resolve_runtime_route`、私有 base-URL 猜测器及其
五个自测已删除；全仓零调用的 `route_output_limit_tokens` wrapper 同步删除。
`known_route_limits`、`route_context_window_tokens`、pricing/provider-lake 和 Provider/M7
范围未改变。
旧 `workspace_context` 的 refresh/collect 链没有生产 caller，App 的 cache/cell/timestamp
始终保持空值；footer 与 empty-state 却把这个空值解释为非 Git 仓库。该模块及三个 ghost
字段已删除，`StatusItem::GitBranch` 断代替换为只读取真实 `App.workspace` 的
`StatusItem::Workspace`，旧 `git_branch` 配置键不设 alias。canonical Run workspace、
workspace guard、`git_status`/`git_diff`、Fleet branch 与 writer worktree 均未改变。
`TerminalInputPump` 现在只拥有真实的终端输入线程、poll/read、限时接收和 Drop 生命周期。
旧 Engine 的 heartbeat/liveness、child pause/ack、detached restart、pending queue/drain、
watchdog/recovery snapshot 与 pause/resume terminal helper 均无生产调用方并已删除。终端
Key/Paste/Mouse/Resize/Focus 仍进入 onboarding/canonical loop；canonical Run 事件继续由独立
的 `run_events.try_recv` 进入 `CanonicalRunProjection` 和 presenter。

ADR-0020 当前只改变 repository delivery/release surface，不改变上面的生产主链。现有
`scripts/dse-delivery.sh` 仍是唯一离线 package/install/verify/upgrade/rollback/uninstall owner；新增
versioned POSIX installer 只解析固定 `gelin66/deepseek-agent` exact stable Release、验证顶层 manifest/
SHA 与 target archive，然后提取并校验同 revision 的 canonical delivery payload。release-time Python
只生成/验证 `dse.release.v1` JSON 和 SPDX，不进入 cold install 或 DSE runtime。

当前远端 release immutability=`enabled`；stable `v0.8.69` 已从 exact
`83c9d43151946c554b1e8ecab028e8eec54bc42f`、tree
`21e48a0a384b232a72fad778de45a16e2ebbdd19` 经 workflow run `30474005480` 发布。Release
`draft=false`、`prerelease=false`、`immutable=true`，四 native runner 均完成 build/fresh install/
version/doctor/verify/uninstall，installer/manifest/SHA256/SBOM 与四 archive 共 8 项资产通过 repo verifier、
GitHub artifact attestation 和 release-asset attestation。独立下载实际遭遇 curl 35/56 reset；versioned
installer 的有界 `--retry-all-errors` 恢复后 8/8 checksum/verifier/attestation 再次通过。

Sites 的 `https://dse.run/install.sh` 返回 `text/x-shellscript; charset=utf-8` 与 300 秒
must-revalidate cache，不托管 binary。隔离 macOS arm64 HOME 已通过主命令完成 `v0.8.68 -> v0.8.69`
upgrade、同版重跑、rollback、再升级、verify 与 uninstall；version/doctor 正确且 `DSE_HOME` marker
byte-identical。因此仓库 release 和 versioned installer 当前是已闭合事实。

整个 public one-command Goal 尚有一个 Sites-owned transport gap：fresh HOME 下载 `SHA256SUMS` 时遭
curl 35 reset；当前薄 bootstrap 只有 `--retry 2 --retry-connrefused`，缺少
`--retry-all-errors`/`--retry-max-time`，并把 transport failure 误报为 asset missing。该次 versioned
installer 未被调用、没有 owned install path、预置 marker 未改变。现有 Sites 线程必须把 latest 与 asset
fetch 切到同一 18/22/28/35/56 typed bounded-retry 合同并通过一次 fresh main-command/version/doctor；
不需要改 Runtime/Store、重发 immutable Release 或重跑仓库 full。

原始切片 focused/full 与 patch targeted/focused 均通过；首次 patch full 只因 version 改变后的 prompt
provenance fixture 保留旧 `Cargo.toml` hash 而失败，更新 exact block/aggregate provenance 后的新 revision
full 一次 exit=`0`。PR #10 head CI run `30472034739` 与 default push CI run `30472976118` 三项均绿；
远端首次 PID marker red 属于先创建空文件再解析的 fixture race，修复只等待 typed PID 可解析，不改变
production recovery。没有 official DeepSeek request，production Runtime/Event/Store/catalog/Prompt/
DeepSeek wire delta=`0`。

## 2. 已统一的生产链

<a id="current-app"></a>
### Application service

`crates/app` 是 exec 与 app-server 的唯一 application composition owner：

- 组合官方 DeepSeek connection、credential 和 HTTP client；
- 组合 production prompt；
- 组合固定工具 executor 与本地执行策略；
- 打开同一种 SQLite `RunStore`；
- 绑定 physical request budget、model accounting 和 execution fingerprint；
- 解析 Host-only `ApplicationProbe` TaskContract，并在冷重开进入 Runtime 前按已持久化 lease
  回收 in-flight probe process tree；
- 以同一 root production composition 承载 search/fetch、isolated Writer、Host compile/app probe、
  seal/integrate/cleanup 与 terminal SQLite replay；Internal Alpha 的三个独立 task 已闭合该水平链；
- 首个 release candidate 又以同一 immutable binary 完成 TypeScript、Python、Rust recovery 与 explicit
  Writer 四个 official DeepSeek 工程任务及一个正确安全拒绝；所有 terminal cold reopen 只重放 committed
  facts，Writer root 不直接写入且 seal/integrate/cleanup 各 committed 一次；
- 维护轻量 process-local active control registry；
- 实现 start、continue、list_roots、get、events、resume、steer、interrupt、
  cancel、resolve_interaction；
- start/continue 通过 State schema v21 中保留的 durable creation reservation 先绑定
  `request_id + command digest` 与唯一 reserved run ID；
- control command 只有在对应 `SteerQueued`、`ControlRequested` 或 `InteractionResolved`
  已提交到 `RunStore` 后才返回 accepted sequence；重复 `request_id` 按持久回执幂等处理。

active registry 只保存当前进程可投递的 control handle，不是第二个 lifecycle 或持久事实。
run projection、event、lease 和 terminal 都从 `RunStore` 读取。

<a id="current-runtime"></a>
### Agent runtime

`crates/runtime` 是 UI、HTTP、DeepSeek transport 和 SQLite 无关的唯一根/子 Agent 内核。
根 Agent 和 child Agent 使用同一个 `AgentRuntime` 与 conformance semantics。Runtime 负责：

- canonical transcript；
- continuation lineage 与 model-visible context projection；
- model/tool 循环；
- root/child budget；
- control command；
- durable approval、request-user-input、两阶段 steer 与 typed interrupt/cancel；
- crash-safe event 提交；
- terminal candidate 与 Host 接受边界。

Runtime 自带的内存 Store 只用于测试，不进入 production composition。

当前 RuntimeEvent v22 继续保留 v6 将逻辑模型请求预算和物理 API 请求预算分开的语义：
Runtime 在进入
ModelPort 前拒绝第 N+1 个逻辑请求时持久化
`model_request_budget_exceeded`；只有 DeepSeek 物理 admission 实际拒绝请求并使
`exhausted_denied > 0` 时才持久化 `api_request_budget_exceeded`。`started == limit`
不等于物理耗尽，Runtime 不为证明耗尽而故意发送额外请求。旧泛化
`request_budget_exceeded` 不再接受。

M5-A 还把模型 `Stop` 从任务成功降为 `CompletionProposed`。每个 Agent run 在
`RunCreated` 冻结 `TaskContract`；非默认结构化 task 的 objective、constraints、non-goals
和 acceptance description 以唯一确定性中文格式进入 canonical transcript，因此
DeepSeek 能看到 Host 将执行的任务语义。Runtime 在接受候选前重新观测 workspace。显式
verifier acceptance 只接受匹配 generation、精确 parameters/plan、最新单调 workspace
generation 和已知 revision 的 Host-sealed `EvidenceReceipt`。任何 `MayWrite` 工具一旦
执行都会推进 workspace generation，即使内容 hash 恢复原值；旧 receipt 因而不能复活。默认
`TaskDefinition::host` 仍由 Host Runtime policy 接受候选，它不是确定性验证成功，也不能
自动计为评测的 `verified_success`。

M7-A2 后，`crates/protocol` 的 canonical JSON 不再依赖 `serde_json::Map` 的 feature 后端：
每层 object 显式按 UTF-8 key bytes 排序，array 保持原序；inline verification artifact 的
构造与 replay validation 共用同一 canonical-byte helper。跨 Rust/Python、
`serde_json/preserve_order` 开/关、M7-A v1 T1 artifact/receipt 和篡改反例共享固定向量。
没有新增第二种 digest、legacy adapter、兼容开关或 schema owner。

Runtime 还为每个 root/child 从共享逻辑预算预留一个可退还的最终请求许可。descendant 与
child 必须先 join，随后各自以 `tools=[]` 发出最后请求；没有最终容量时不得先提交假的
`ChildStarted`。hard-limit 本地 compaction 不调用模型，不能消耗最后许可；恢复则按该次
请求实际 advertised tool catalog 拒绝未授权工具。该最终请求许可机制已有离线
conformance/Store replay 和真实 A/B；A/B 保留可靠性机制，但其原始实现的 multi
Token/时间/费用均回退，不能作为产品效率提升。

`528a72f2` 又把同一 `agent` 工具 batch 启动的 pending child 在下一次 root 模型请求前
eager join，使 durable `ChildFinished` 先于 root continuation，删除没有 handoff 新信息的
父模型等待轮。相对相邻基线 `f9dddd5d` 的 24-run、6/cell 精确 A/B 为 24/24 verified；
candidate multi 请求均值下降 `9.62%`，Token/费用均值下降 `8.27%/11.99%`，平均时间只变化
`-0.07%`。12/12 multi lifecycle/handoff 完整，但样本只覆盖一个固定任务，时间和费用的
成对结果各只有 2/6 更低，因此不能外推为普遍多 Agent 提速或成本优势。

C2 把 continuation 与 recovery 分开：`resume` 继续同一个 run，`continue` 从一个终态
root 创建新的 root，并用 `continued_from_run_id` 记录 lineage；source 不被改写。完整
canonical transcript 仍 append-only，每次 request projection 由唯一 `crates/context`
ContextBroker 确定。只有预计输入超过 Host 根据官方 context/output capability 和 safety
headroom 派生的 hard input limit 时，Runtime 才在同一 root/child 本地压缩并提交
`ContextCompactionCommitted`。Store 从 transcript、真实 tool catalog、digest 和
before/after Token 重算，仍超限则 typed fail closed。该路径没有摘要模型请求、手动命令、
提前阈值、独立 root 或第二状态机。

<a id="current-orchestrator"></a>
### Agent orchestrator

`crates/orchestrator` 是唯一生产 Writer 编排与 Git workspace owner。当前只支持：

- 一个 root Integrator 与最多一个 isolated Writer；Writer 仍由同一
  `AgentRuntime` 和同一 `agent` 工具启动；
- clean Git repository、精确 base commit、冻结 allowed paths 和一个确定性 fast-forward
  integration 策略；
- 独立 worktree create、child execute、Host diff/changed-files/revision seal、
  worktree exact verifier、root branch CAS integrate、最新根 revision verifier 与 cleanup；
- 跨进程 OS lease、精确 Git `HEAD.lock`、幂等 integrate/cleanup 与 crash/reopen recovery；
- Linux bubblewrap / macOS seatbelt 下的 child workspace scope，以及
  `.git`、`.codewhale`、`.deepseek` 保护。

M6-B1 rework 将该机制收缩为显式 admission：默认 root 仍可启动 read-only child，但不会
获得 `isolated_write`；只有调用方显式启用一个 Writer 时才允许创建 worktree。Writer
treatment 的 root 由 Host actor policy 在工具执行前禁止 may-write，Writer child 只获得
冻结 allowed paths 内的必要写工具。权限来自 Runtime/Orchestrator 的 actor 身份，不依赖
中文提示词或模型自律。

TaskContract 现在冻结 named verifier ID、参数和执行计划；实际 workspace revision 由
Host verification 和 EvidenceReceipt 在执行时绑定。恢复类任务的 EvidenceReceipt 必须按
canonical event 顺序证明“修改前失败 → 有效修改 → 修改后 Host 通过”，只有最终绿不能
完成任务。seal/blocked 还会保存脱敏 reason、changed-files/scope 摘要；只有 artifact、
resource ownership/scope、Git cleanup metadata 或 exact cleanup 结果确实不确定时才
retained，确定无副作用时精确清理。

`AgentTask`、workspace assignment、Host-observed `AgentOutcome`、integration 和
post-integration verification 都是当前 RuntimeEvent v22 / State v28 的 canonical facts。
Orchestrator 不定义私有事件总线、JSON ledger、模型循环、DeepSeek transport、工具实现或
完成判定，也不拥有 M45-A ApplicationProbe 的 process/port/health/log 生命周期。它只为 probe
沿用调用方已经选定的 Writer worktree；一次性 probe 的实现、进程树 lease 与 cleanup 均在
`crates/tools`。Writer receipt 只是 child artifact；只有集成后绑定最新 root revision 的
EvidenceReceipt 可以满足 root TaskContract。

当前明确 fail closed：dirty/non-Git/unborn/不受支持 Git 仓库、base 漂移、branch CAS
变化、allowed path 越界、空 diff、缺失 artifact、verifier 失败和恢复歧义。M6-A 未实现
多 Writer、通用 DAG、脏工作区快照、自动冲突修复或远程 worker。

<a id="current-deepseek"></a>
### DeepSeek backend

`crates/deepseek` 是官方 DeepSeek Chat 请求事实 owner：

- Standard Chat 与 Beta Strict Chat surface 规划；
- 确定性 `RequestPlan`；
- Chat ordinary/non-streaming 与 SSE transport；
- reasoning/tool-call 历史回放；
- finish reason、typed error、retry 和 usage；
- 完整 SSE response 必须同时观察受支持 finish reason 与 `[DONE]`，DONE 后 data fail closed；
- content、reasoning、tool-call fragment、finish、usage 与 DONE 的最小 typed evidence；
- usage frame 到达即进入 accounting，异常 EOF/consumer drop 不丢失已知 usage；
- retryable 与 replay-safe 分离，partial output/tool-call 不允许自动重放；
- root/child physical request attribution；
- 官方模型 capability、output limit 和 pricing fixture。

固定 actor route policy 不在 backend。唯一 owner 是
`crates/app::ProductionFixedRoutePolicy`：未显式指定模型的 root 使用 Pro/high；
显式 Pro/Flash/reasoning 原样保留并由 child 精确继承；fixed-profile 普通 read-only
child 使用 Flash/high，explicit isolated Writer 使用 Pro/high，typed
recheck/rework/recovery 使用 Pro/max。每个 child selection 先冻结进 `AgentTask`，再由
同一 `AgentRuntime` 构造 exact `RunRequest`；不在一个 Run 内切换模型，也不存在 Auto
输入、classifier 或额外路由请求。

普通工具调用不因存在工具就误走 Beta；只有整组 schema strict-compatible 时使用
Beta Strict Chat。M7-B 冻结的六个默认可执行 actor 目录在 Strict 候选下均仍原子回退
Standard Chat；当前生产没有真实 Strict surface，也没有用户 Strict 开关。fallback 保留
完整工具数量、名称、顺序与 schema，不通过第二份 wire schema 或语义弱化进入 Beta。
官方 FIM 仍是独立 Beta Completions 协议，但 CodeWhale 当前不再声明 production FIM
surface。M8-H 证明旧 planner 没有 canonical caller，sender 不拥有它产生的 endpoint，
完整 parser 也不能读取 Completions 的 `choices[].text`；该半分支及其无消费者 accounting
已物理删除。未来只有新的 production 编辑失败样本同时证明瓶颈属于编辑生成，并且完整
Host-owned parser/apply/accounting/reopen 候选先成立，FIM 才能按新垂直切片重开。
Context cache 由官方 Chat 的稳定前缀自动触发，不存在手工 cache API。

M44 复核后，production DeepSeek surface 仍只有上述 Standard/Strict Chat。当前官方 Chat
reference 的 tool 类型只有 function；Web Search 只在独立 `/anthropic` compatibility 中出现。
采用它需要 Messages request/response/SSE、server-tool content blocks、thinking signature、
finish/continuation、source 与 usage replay 的第二 wire，现有 `ModelMessage/ModelOutput` 和 Chat
parser 不能无损表达。M44 没有增加 `ApiSurface`、endpoint、DTO、parser、RuntimeEvent 或 State
字段，当时结论是 `hold_wait_for_chat_surface`。ADR-0019 随后选择由现有 function tool 调用一个
Host-owned canonical search adapter；因此 DeepSeek wire 仍未增加第二 search/server-tool surface。

<a id="current-tools"></a>
### Tools

`crates/tools` 拥有 production 固定工具 catalog、schema、execution identity 和 handler。
当前 16 个 Host 工具为：

```text
apply_patch       browser_interact  browser_navigate  edit_file
exec_shell        file_search       git_diff           git_status
grep_files        list_dir          load_skill         read_file
run_tests         run_verifiers     web_fetch          web_search
```

M45-A 另有一个不计入这 16 个定义、不会发送给 DeepSeek 的 Host-only exact verifier
`application_probe`。`crates/app` 在 Start 时把 caller 的 program/argv、worktree 内 cwd、受限
env、health/assertion path、status/body 与各项 bounds 解析成 canonical `VerifierSpec`，并加入
Host 生成的一次性 128-bit lease；caller plan、URL、host 与 port 都不能覆盖 Host 事实。argv 必须
精确包含一次 `{dse_probe_lease}`，实际应用 response 必须回显对应
`dse-application-probe:<lease>`，所以释放临时 port 后抢占该端口的 foreign listener 不能产生
false pass。

执行只构造 `http://127.0.0.1:<host-port>` 的 bounded GET；不接受 shell string、redirect、proxy、
header、Cookie、auth、body、WebSocket 或 TLS 配置。startup/health/overall deadline、raw response、
stdout/stderr 与 teardown 都有硬边界，response/log 统一标记 `external_untrusted`。正常成功、
typed failure、cancel 和 timeout 都终止 owned process group/Job 并 reap；revision before/after
不一致时既有 artifact 变为 stale，不能 seal receipt。Agent/FullAccess root 可按现有 sandbox
执行；Ask 与普通 no-network actor 在 spawn 前拒绝。isolated Writer 只在这个 Host-only verifier path
派生 `host_loopback_only` sandbox：macOS 仅允许 `localhost:*` bind/inbound、没有 network-outbound；
Linux 保持 isolated network namespace。因 Host 与 Writer 不共享该 namespace，isolated Writer 的
Host-to-application 正向 loopback checkpoint 只在 macOS Seatbelt job 执行；Linux CI 保留普通 root
probe 与 bwrap namespace/egress 负向合同，不能把该正向路径写成跨平台成功。Internal Alpha 的三个正向
fixture 不使用共享进程锁；每项在 root receipt 前依次验证 child tool outcomes/receipt、Host seal、integration
和 root byte-exact final marker，再验证 root latest-revision receipt、cleanup 与 terminal。历史 draft response
携带当前 Host lease，故不是 foreign listener；旧 port-race/serialization 归因已删除。该例外不进入
model-visible shell/Web authority。

lease 已随 `HostVerificationPrepared` 的 frozen verifier 持久化，不增加 PID/port sidecar。
进程级 `SIGKILL` 后，`AgentApplication` reopen 只按 exact argv marker 扫描并回收 owned process
group；随后现有 Runtime 以 Host-verification ambiguity 形成一个 `RecoveryRequired` terminal，
不重新 spawn、HTTP 或请求 DeepSeek。已 committed terminal reopen 只重放 SQLite outcome/receipt。
`ToolOutcome`、inline artifact、`EvidenceReceipt`、RuntimeEvent 与 State schema 均未改变。

一次 official Flash canary 按 physical admission 1、runtime rerun 0、64 output tokens 执行，但没有
到达 `Completed`；首次 harness 又在断言前停止，未留下可引用的 request accounting。因此当前只
保留 deterministic production caller/rework/reopen 事实，不声明 official vertical usability、
精确费用或效率；没有为取得绿结果发出第二次请求。

M43 已加入 canonical `load_skill(name)`。production Start/Continue 由 `crates/context` 发现一次
不可变 Skill 快照，并把同一 `Arc<SkillRegistry>` 交给 prompt、root/child executor 与 execution
fingerprint。prompt 只在 actual actor catalog 含 `load_skill` 时列出精确名称/说明，不再暴露
文件路径或建议用 workspace `read_file` 猜测全局 Skill。tool schema 不接受 path、alias 或额外
字段；返回完整 body、source path、source bytes/SHA-256、returned bytes、
`truncated=false` 和 `trust=external_untrusted`。

发现 admission 只接受不超过 128 KiB 的完整普通 UTF-8 `SKILL.md`，canonical source 必须留在
本次显式 discovery root 内；不可读、无效、过大及同一
precedence cell 的归一化歧义均不向模型宣称可用。已接纳 body 在 run 内冻结，执行时不重读
文件；live resume 的重新发现若改变 snapshot hash，会由既有 production execution fingerprint
拒绝。SQLite terminal reopen 只重放 committed `ToolOutcome`。它没有增加 Skill session/store、
第二权限 owner、RuntimeEvent 或 State schema。

M41 已在 `crates/tools` 加入 canonical `web_fetch(url, max_chars?)`；ADR-0017 W1.1 将它从
HTTPS-only 一次迁移为 public HTTP(S)。HTTP 只允许规范化默认端口 80，HTTPS 保持既有端口行为；
每一跳都重新执行 URL、DNS/IP、connect pin 与 redirect 安全门。trajectory 一旦进入 HTTPS，
后续 HTTP target 在 DNS/connect 前以 `web_transport_downgrade` 拒绝。system proxy、自动
redirect、Cookie、认证、任意 header、证书绕过和自动解压均未开放。当前硬边界仍为 5 次
redirect、15 秒总 deadline、1 MiB raw response、2 MiB decompressed body、50,000 个返回字符和
32 个 canonical link；只接受 UTF-8/US-ASCII 的 HTML/XHTML/plain text，HTML 提取不执行 script。

成功结果携带 requested/final URL、status、media type、title、有界正文/链接、retrieved time、
source SHA-256、`source_sha256_scope=received_content_replay_identity`、读取/返回 bytes、
truncation、final transport、完整 trajectory/integrity、redirect count、upgrade 与
`trust=external_untrusted`。只要任一 hop 是 HTTP，trajectory/integrity 就保持
`plaintext_exposed/unprotected`，即使 final response 是 HTTPS；hash 不表示 publisher
真实性。Web-specific stable failure 及同一 transport provenance 放入现有
`ToolOutcome.metadata`，生命周期仍使用 canonical typed fields。没有改变 RuntimeEvent 或 State
schema，也没有 Web session/store/accounting ledger。committed outcome 经 SQLite reopen 只重放，
不重新 DNS/HTTP。root、coordinator 和 read-only child 依现有 read-only catalog 获得定义；Ask 对
exact URL 形成一次性 Host approval prompt，Agent/FullAccess root 可执行，isolated Writer 继续由
`actor_controlled_network_denied` 拒绝。

ADR-0019 已把一个 canonical `web_search(query, max_results?)` 加入 fixed catalog。它固定调用 Tavily
Basic/general HTTPS endpoint，Host 从既有 secret backend 或 `TAVILY_API_KEY` 解析 credential；模型不能
提供 provider/endpoint/header/Cookie/auth/proxy/browser 参数。query 最多 512 字符/2,048 bytes，结果最多
10 个，response 最大 1 MiB，connect/overall deadline 为 5/12 秒；endpoint DNS 必须全部为 public
unicast，client 使用 connect pin、no proxy、no redirect 与 identity encoding。结果返回 provider
request/usage identity、有界 rank/title/canonical public HTTP(S) URL/snippet/time/provenance/hash/bytes，
并明确 `evidence_role=discovery_only` 与 `trust=external_untrusted`。snippet 不能证明事实，生产工具说明
要求先用 `web_fetch` 或 browser 读取原来源并交叉核验。provider credits 只记录 usage unit，actual
charge 不可得时不声明精确成本。

Ask 对 exact query 形成一次性批准；Agent/FullAccess root 可执行，isolated Writer 仍由
`actor_controlled_network_denied` 拒绝。execution identity 包含 fixed search network identity；committed
SQLite reopen 只重放，started-without-outcome 进入既有 `RecoveryRequired`，不会重发可能计费的
search。M44 已删除的 `[search]`/`DSE_SEARCH_*`/Doctor provider selector 没有恢复，也没有 Search
Manager/Factory/Service、fallback chain、HTML search scraper、search session/store/ledger 或新 Runtime。

M46 首个 capability cluster 曾把 fixed catalog 从 W3.1 的 16 个收敛为 15：`browser_click` 与
`browser_fill` 已由唯一 `browser_interact` 取代；ADR-0019 加入 search 后，Web surface 现为
`web_search`、`web_fetch`、`browser_navigate`、`browser_interact`。browser owner 仍是 `crates/tools` 的
direct Tokio CDP，驱动
Host 预安装且 SHA-256 pinned 的 Chrome for Testing `151.0.7922.47`；同一 executor 只保留一个有界、
最多三页的 isolated session。exact-local 使用 TempDir/incognito；public 使用按 canonical workspace identity
派生、有独占锁和 bounded expiry 的 project profile。replacement navigate、terminal/drop 与 ambiguity 会
teardown process tree、loopback proxy 和 quarantine。managed profile 是 Chrome artifact，不是第二
snapshot tool、后台 daemon、Runtime、Store 或 session ledger。

public target 只允许默认端口 HTTP(S)，local target 只允许 Host 注入的 exact literal-loopback origin。
CDP Fetch interception 与 connect-pinned proxy 对每次 request/DNS/connect/redirect 重验，继续阻止
private/metadata、cross-origin Document、auth/referer、service worker、QUIC 与非代理 WebRTC。Cookie 只由
project profile/Chrome 在 exact origin 内管理，值不进入模型；download 仅在 exact granted URL 下进入
quarantine。额外 worker/popup 仍在启动暂停态关闭；只有 Host typed `tab_open` 可在同一 egress scope 内
增加受管 page。wire/decoded body、CDP frame、node、char、redirect、page/download count、bytes 与 deadline
都有硬上限；outcome 只保存 bounded `external_untrusted` semantic observation、replay hash、Chrome/CDP/
network identity 与 receipt，不保存 HTML、script、Cookie/storage value、screenshot 或 pixel。

`browser_navigate` 仍按既有 read-only actor catalog 可见：exact-local 自动执行，public Ask 显示 exact
target/read impact 后一次性批准，Agent/FullAccess 可执行；isolated Writer 不继承 local-origin grant，
Host-controlled network 由既有 actor policy 拒绝。现有 ToolOutcome、RuntimeEvent v18 和 RunStore 足以
无损表达 catalog、approval、started/outcome/recovery 与 receipt；committed SQLite outcome reopen 只
重放、不重新导航或 POST，protocol/state schema delta=0。Playwright 仍未进入 production dependency graph。

`browser_interact` 的 tagged schema 一次覆盖 `click`、`fill`、whitelisted `press`、typed `wait`、bounded
`scroll`、native `select`、`back`、`tab_open/switch/close` 与 Host-classified `submit`。textarea、普通
input 和 contenteditable 可获得 fill/press capability；password/file/login/secret/token/credential/OTP
不获得敏感能力。模型仍不能提供 selector、CSS/XPath、坐标、任意 key、JavaScript/eval、header、Cookie、
认证、proxy、Chrome flag 或文件路径。键盘事件只发送 Web `key/code`，不再注入 native/windows virtual
key code；select 使用确定性的 Host CDP DOM identity 操作，不执行模型脚本。

每个 capability ref 仍是随机 opaque token，绑定同一 run、browser、backend DOM identity、active page、
latest snapshot/page epoch 与 exact action set。每次动作前重取 DOM/AX/layout identity，拒绝 cross-run、
stale、missing、hidden、disabled、readonly、detached、ambiguous、semantic/capability drift；动作后必须返回
fresh observation 并旋转 refs/epoch。Host synthetic `submit` capability 不参与页面 semantic fingerprint，
但 exact target/form parameters/impact 的任何变化都会使 durable preview stale。live page/ref 从不写入
RunStore；cancel/timeout/transport ambiguity 仍 teardown，未知副作用不自动 replay。

`browser_navigate` 的可选 `focus` 现在在完整 eligible AXTree + DOMSnapshot 节点集上执行确定性的
task-cue、interaction 与 role priority，navigation/banner/footer 降权；只对无 capability 的相同 semantic
content 去重，然后才施加既有 node/char bounds。结果同时返回 focus recall、redundant ratio、
prompt-injection exposure、AX-only/DOM-without-AX、canvas/SVG blind spot 与 truncation-caused focus loss。
每次 action 后的 fresh observation 还返回同 page lineage、ref-independent 的 bounded semantic diff，
含 added/removed/changed/unchanged、stale ratio、最多 16 个 entries 和 bytes。该路径仍只使用一个
AX/DOM extractor，不执行模型脚本、第二 LLM pruning 或视觉 observation。

W3/W3.1 的 frozen manifest/summary/fixtures/history 保留；production cutover 已物理删除分立
`browser_click`/`browser_fill` catalog/schema/dispatch 与两个旧 integration test path，改为一个 cluster
test。没有 compatibility flag、one-action admission evaluator、Node/Playwright production sidecar 或第二
browser owner。

public routine interaction 只在当前 same-origin session/current epoch 消费 typed ref：Ask 显示 exact
origin/target/parameters/impact 并请求批准，Agent/FullAccess 自动执行；external side effect 只给明确的
same-origin reversible draft POST target 生成 `submit` ref，Ask/Agent 必须批准，FullAccess 才可直接执行。
`publish/delete/purchase/buy/send/message`、origin/scope escape 与非 exact POST 永不获得 grant。敏感/file
成功控件不进入 durable preview，只有实际提交值为空才可被 Host 从 canonical comparison 排除；非空值
立即 `browser_method_denied`。

批准只绑定一次 invocation、当前 workspace revision、exact target、canonical non-sensitive parameters
SHA-256 与 `reversible_draft_write` impact。执行前持久化既有 `ToolExecutionStarted`，成功 outcome 同时
保存 HTTP status、transport header receipt、页面 `data-receipt`、observed time 与 fresh observation；
request 已开始而 receipt 不闭合则为 Unsafe/Indeterminate，不伪造成功。root catalog 默认可见；
coordinator/read-only child 仍因既有 MayWrite actor catalog 不可见；isolated Writer 即使可见也由
`actor_controlled_network_denied` 拒绝。

真实 pinned-CfT task 在 7.21 秒内完成 local text editing/press/select/scroll/wait/back/tab/multi-page 和
credential-free mapped-public draft POST，verified task families=`2/2`、negative false allow=`0`，receipt=
`draft-receipt-001`。真实 `AgentApplication -> AgentRuntime` 中 public submit 先投影 exact approval，批准后
只执行一次；committed SQLite reopen 不重新导航/POST，click/fill `ToolExecutionStarted` 无 outcome 的
reopen 均为 `RecoveryRequired` 且 replay=`0`。RuntimeEvent、RunStore、State schema、DeepSeek wire/
model-visible Prompt delta=`0`；official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`。

#### ADR-0018 后的 current capability boundary

前三个 cluster 已闭合 semantic interaction、public reversible action、managed account workflow，以及
canonical source discovery + task-relevant semantic observation，并将
`crates/app` 的 root Agent 从 broad full-access workaround 改为 workspace-write + Host-controlled network。
current product 已能完成 deterministic unknown-source research vertical，但仍没有 visual observation，
并已用三个跨 code/app/Web/Writer/recovery 的 Internal Alpha task 证明 deterministic production integration；
三项现在无共享进程锁并发闭合。另一个 typed integration-conflict conformance 证明 seal 后若 root CAS
integration 失败，canonical revision/bytes 保持 base/draft、root receipt=`0`、terminal=`Blocked`，同时
cleanup=`Removed` 且没有 integration side effect；因此 root completion 只接受已集成 final marker。
首个 RC dogfood 冻结五个 cross-language arm，actual 为 positive=`4/4`、正确安全拒绝=`1/1`、false
success=`0`、SIGKILL/reopen resume=`1`，official accounting complete=`5/5`；它支持 bounded
`keep_release_candidate`，不支持 live-search、通用效率或产品级成功率声明。candidate CLI/TUI 已通过隔离
install/run/uninstall 和既有 delivery lifecycle；production Rust、Runtime/Store/Prompt/catalog delta=`0`。
首个 official DeepSeek dogfood 因 canary 的 512-token output cap fail closed；显式授权的 fresh 2,048-token
successor 越过该限制，但在第 6 个请求因人为 tool-call budget=`6` fail closed。两个 treatment 都没有重跑，
再次授权的 2,048-token / 16-tool successor 越过两个旧上限，但到 Host verifier failure 时已在第 8 个请求
耗尽 recovery budget并保持 Blocked。12-request successor 又两次启动 Writer，root marker 仍缺失；审计
随后发现旧 Alpha fixture 让不能读取 `server.py` 的 apply-only Writer 凭空提交预制文件。current task 已
改为真实 Writer `read_file -> apply_patch`，deterministic 3/3 保持闭合；修正后的 official treatment 已在
10 requests、0 retry 内到达 Host-accepted latest-revision `Completed`。terminal 后旧 observer 把两次合法
root `read_file` 误判为工具轨迹失败，现已离线改为核心顺序 + bounded reads 并有正反例。五个 treatment
都没有重跑；当前只证明一个 bounded deterministic Host-Web vertical，不宣称可替代完整工程 Agent。

长期不变量继续由代码与 authority 强制：public URL SSRF/egress、isolated profile、opaque ref、secret
Host 托管与脱敏、fresh observation、external-untrusted、exact authorization、started/outcome/
RecoveryRequired、committed reopen no-reexecution、bounds/teardown 和 single Runtime/Event/Store。

ADR-0018 的第二个 capability cluster 已闭合 Managed Browser Session。public browser 现在使用按 canonical
workspace identity 派生并加独占锁的 managed profile；Host 只把 opaque credential grant 暴露给模型，
secret 通过既有 secret owner 直接注入 exact same-origin login form，永不进入 ToolOutcome、RuntimeEvent、
SQLite 或日志。session cookie/storage 可查询有限计数并按项目清除；不同项目、origin、actor 不能共享。

workspace-granted upload 只接受 canonical workspace 内普通非 symlink 文件，并在授权与执行时绑定 size/
SHA-256；download 只进入 session quarantine，受数量、大小、deadline、redirect/origin、media sniff 与静态
扫描约束，永不自动打开/执行/覆盖。通过扫描的 artifact 只有在显式 `promote_download` 后以 no-overwrite
原子创建进入 workspace。登录、上传、下载、promotion、clear 均复用既有 started/outcome/recovery；
started-without-outcome reopen 为 `RecoveryRequired`，committed reopen 不访问网络或文件系统。

当前剩余 capability gap 是 official DeepSeek Alpha vertical 的成功验证、受确认的
destructive/financial/publish 动作与 selective visual。个人 Chrome、任意 selector/coordinate/JS、无界网络、
secret-to-model 和 unknown-side-effect replay 继续 fail closed。

M44 已删除没有 executor 的 TUI/config
search-provider 枚举、`[search]`/`DSE_SEARCH_*` reader 与 Doctor projection；遗留配置明确
fail closed。MCP 配置与插件发现仍没有进入模型统一工具面，不能算作 Agent 搜索或浏览器能力。

Runtime 在允许的 depth/budget 内追加内建 `agent` control tool；它启动的 child 仍是同一个
`AgentRuntime`，不是另一套 swarm loop。

交互 root run 还会追加 Runtime 内建 `request_user_input`；child run 强制非交互。该能力
不增加固定 Host 工具数量，approval 也是工具执行前置协议，不是模型可见的新工具。

所有 Host handler 返回 canonical `ToolOutcome`，明确区分 invocation、transport、operation、
retry、side effect、evidence、artifact 和 workspace revision。失败 outcome 必须携带稳定
`failure_code`；模型看到唯一中文摘要和恢复建议，英文 code/字段保持稳定，成功结果不改写。
Runtime 只按当次实际 advertised catalog 接受工具调用，crash/reopen 不重放已执行工具或
已提交 outcome。root、read-only child 和 isolated Writer 使用同一失败恢复 conformance。
M7-C 后，`edit_file` 的 prior-read freshness 绑定 exact-byte SHA-256，并在原子 replacement
前再次校验原字节；重叠 search 不再被误判唯一，替换保留文件 permissions。`apply_patch`
在任何写入前拒绝 duplicate/resolved duplicate target、rename、hunk count mismatch、no-op
与 ambiguous fuzzy placement。multi-file 普通失败会按记录回滚原字节并显式报告 rollback
不完整；它不声称提供跨文件 crash-atomic transaction。

顶层 CLI 的 direct `git apply` 与 TUI-local `codewhale eval` 简化编辑器已经物理删除；
入口只能通过 production composition 调用上述固定目录。旧 TUI child `ToolRegistry` 已
删除。TUI 下仍编译的其他宽工具实现不代表 canonical production catalog 会自动扩大，
其余无消费者模块按独立调用方切片继续清理。

<a id="current-state"></a>
### State

`crates/state::StateStore` 实现 production SQLite `RunStore`：

- 当前 State 物理 schema 为 v24；RunStore 继续复用同一 event/snapshot 表，不增加
  EvidenceReceipt 私表；
- 当前 canonical RuntimeEvent writer/reader 为 v18；
- append-only canonical event；
- reducer/snapshot/replay；
- continuation lineage 的快速 projection、workspace-scoped root 列表和原子 continuation
  创建；
- durable creation reservation：start/continue 的同 ID 同 payload 重试只对应
  一个 reserved run ID，不同 payload 复用 ID 被拒绝；
- 最近一次模型请求实际 advertised tool catalog 的持久化与 replay 重建；
- execution lease 与 epoch；
- pending model attempt、typed response/failure evidence、原子 retry decision 与 unknown billing；
- pending interaction、steer、terminal control 与携带规范化 payload 的 command receipt；
- frozen TaskContract、workspace state、completion candidate、Host verifier durable action
  与 EvidenceReceipt；
- AgentTask/workspace assignment、Host-observed AgentOutcome、Writer integration、
  post-integration verification 与 cleanup/recovery；
- terminal exactly-once；
- Terminal reducer 原子投影 `AgentOutcome.accounting`；root terminal accounting
  `sealed=true`，Writer child terminal 保持共享 ledger 的 `sealed=false`；
- v20 直接退役不兼容的 v19 materialized run/finalized creation，同时保留可安全恢复的
  pending Run API creation；不提供兼容 reader、alias 或双写；
- v21 直接退役全部 pre-v16 materialized run，因为历史失败无法无猜测补齐
  `failure_code`；只保留可从 canonical command 重建的 pending Start intent，不分类历史
  错误文本，不建立兼容 reader；
- v22 直接退役缺少 `ModelRouteAudit` 的 pre-v17 materialized run，因为 selected model
  不能诚实反推 caller requested mode/reasoning；保留 pending Start intent，因为当时的
  Host policy 在 `RunCreated` 前没有网络或计费副作用；
- v23 在同一 `IMMEDIATE` migration transaction 中删除 legacy `threads` metadata 表；
  fresh schema 不创建它；
- v24 退休包含 Auto route/reasoning 语义的全部 v23 materialized run，因为 omitted
  reasoning wire 不能无损映射为 high/max；只保留能直接按 RuntimeEvent v18 命令反序列化
  的 pending Start，并以中性 explicit/fixed-actor profile 保存 actual route audit；
- v25 在 DSE identity 硬切换后退休无法无损重写 active identity 的 exact transcript，
  同时保留 replay-safe pending Start；
- v26 删除无法无损映射到三档 `RunPermissionMode` 的旧 materialized Run 和 pending Start；
  旧 bool/trust/sandbox/elevation tuple 不被猜成新 preset，迁移后只有 v20 reader/writer；
- no-key terminal replay。

旧 `codewhale thread`、SQLite `threads` metadata 表与 `session_index.jsonl` 已删除；
没有 compatibility reader 或双写。旧 Workflow/SubAgent JSON/JSONL 写入链也已随隐藏
执行路径删除，没有迁为 `RunStore` 双写。

<a id="current-clients"></a>
## 3. 当前入口

### `codewhale exec`

- 真实执行进入 `AgentApplication`；
- text/NDJSON、receipt 和 exit code 投影仍在 `crates/tui`；
- start/continue/list_roots/resume/events/cancel 均读写 canonical Run API；
- `codewhale exec --continue <PROMPT>` 查询精确 workspace 下最新 root；只有最新 root 已终态
  时才以新 prompt 创建新的 root continuation。若最新 root 未终态，必须显式使用
  `codewhale exec --resume <RUN_ID>` 恢复同一个 run；
- crash/reopen/resume、terminal-first signal 和 no-key replay 已有外部进程门禁。

顶层 `codewhale` 仍委托现有 TUI binary 处理 exec 参数与输出，这是进程入口复用，不是
第二个 Runtime。composition、model、tool 和 Store 已不在该 adapter 中重复。

### `codewhale app-server`

- CLI 直接构造 production `AgentApplication`；
- 默认 HTTP/SSE 监听 `127.0.0.1:7878`；
- `--stdio` 提供 newline Run envelope；
- HTTP/SSE/stdio 只使用 canonical Run DTO 与 StoredRuntimeEvent；
- 当前 Run API v15 直接接收结构化 `TaskDefinition`，并投影 frozen TaskContract、
  completion decision、durable creation-intent list/recover；当前 RuntimeEvent
  writer/reader 为 v21；
- crate dependency tree 不含 `crates/core` 或 `crates/tui`；
- 不启动 sibling TUI process。

完整接口见 [RUNTIME_API.md](RUNTIME_API.md)。

### Interactive TUI

交互 foreground 已切到 `AgentApplication`：

- `TuiRunClient` 提交 start/resume/continue/steer/interrupt/cancel 和 interaction
  command；
- `CanonicalRunProjection` 与 presenter 只从 `RunStore` event 投影 root/child 进度、终态和
  durable outcome；
- 冷启动在无显式 resume、无初始输入时通过 canonical `ListRoots + Get` 打开 workspace
  Run Hub；`/runs` 到达同一 full-screen room，按 Store 的 `updated_at` 倒序显示 exact status、
  UTC 更新时间、TaskContract objective 与 continuation lineage；
- Run Hub 选择 active root 复用 `Resume`，选择 terminal root 从 sequence 1 重放 Store event
  并建立下一次 `Continue` source；`RecoveryRequired` 只读且不能 continuation，New run 只清除
  TUI 进程内选择。冷重开和同进程重选都重建 fresh projection，不持久化 TUI history；
- `CanonicalRunPresentation` 从同一 ordered event 派生 root 的思考/执行/等待/验证/返工/
  终态、已确认工作区变更、Host 验收进度、Agent 数、frozen permission 与恢复事实；
  宽终端默认显示右侧 task rail，窄终端响应式回退到顶部，输入框上方 phase strip 读取
  同一状态；不存在 TUI 私有 `runtime_turn_status`；
- 旧 foreground Engine、EventBroker、runtime-thread owner、`SessionManager`、child display
  cache 和 registry-driven slash command system 已删除；
- slash command 只剩统一的 `help/runs/cost/permissions/exit` canonical contract；
- 退役的 `crates/tui/src/compaction.rs`、`seam_manager.rs` 以及不再生效的 TUI
  `auto_compact` 开关/阈值状态均已删除；hard-limit compaction 位于
  `crates/context + crates/runtime`，不存在手动 `/compact` 或传输层 command；
- generic Provider/config/UI active path 已删除；保留的 DeepSeek provider 字段只参与
  canonical environment fingerprint / replay safety，不是用户模式或第二 backend。
- Work surface 只投影 canonical root/child/tool facts，不再投影已删除的 TUI 私有
  Plan/Todo、Goal/Hunt 或 custom-command pause 状态；普通 root edit 没有 exact
  filename receipt 时只显示 confirmed change，Writer 文件只在 root integration 后计数；
  canonical Run 的工具 allow-list 不从旧 UI 状态注入。

因此三个保留 foreground 入口与所有生产可达根/只读子/Writer Agent 模型循环已经统一；
M6-A 门禁确认 Writer lifecycle 也不引入第二条执行链。

<a id="current-owner-snapshot"></a>
## 4. Crate responsibility snapshot

| Crate | 当前生产职责 | 当前迁移债务 |
|---|---|---|
| `protocol` | canonical task、request、command、event、named verifier、显式稳定 JSON evidence、AgentTask/outcome、terminal | 后续 evidence 只按真实任务缺口扩展 |
| `runtime` | 唯一根/只读子/Writer Agent loop、Host actor capability、显式 Writer admission、时序 completion gate 与 reducer | M7 DeepSeek 可靠性/预算调优 |
| `orchestrator` | 单 Writer worktree、Host diff/verify/integrate、精确 cleanup/recovery | Writer 保持 explicit-only；不扩双 Writer |
| `deepseek` | 官方 DeepSeek Chat planner/transport/parser/accounting、完整 stream 证据与 usage 保全 | 只按真实协议失败扩展 |
| `context` | production prompt、evidence-aware projection 与 hard-limit compaction | RepoGraph 仅在缺失检索证据出现后启动 |
| `tools` | 固定 production tool catalog、无副作用 verifier 与执行 | 只按新的 production 编辑失败补证 |
| `state` | 唯一 SQLite RunStore、lease、replay；v23 物理删除 legacy thread truth | 只按 canonical Run 协议缺口迁移 |
| `app` | 唯一 production composition、Run command、显式 Writer policy 与 Orchestrator wiring | M7 策略调优 |
| `app-server` | HTTP/SSE/stdio projection | 无独立业务状态 |
| `cli` | 顶层命令与 production config 解析；run list/resume 只走 canonical Store | 只按真实产品入口扩展 |
| `tui` | exec/interactive canonical Run projection 与 workspace Run Hub | 不恢复 Provider/thread 私有状态 |
| `localization` | CLI/TUI 共享 fixed `zh-Hans` compile-time message owner | 只删除失去真实 caller 的 message id，不增加 locale |

`crates/core` 已删除。它原有的 fake `handle_prompt` 从未是 production Agent 能力；app-server
迁移后没有保留兼容 crate 或空壳。

## 5. 已删除的旧产品路径

M4-B 已物理删除：

- app-server fake core runtime；
- `/prompt`、raw chat proxy、direct tool invoke；
- RuntimeBridge、TUI child process、事件翻译和私有 seq/thread map；
- TUI runtime HTTP/mobile server 与 mobile page；
- `serve --http/--mobile`、`app-server --http/--mobile` alias；
- remote-setup bundle generator；
- Tencent Lighthouse deploy scripts/units；
- Feishu、Telegram 和 bridge-core Node 产品链；
- 对应的 QR、CORS 和孤儿 runtime-api config 依赖。

删除这些外围产品不会删除 `agent` 多智能体能力。它们是旧 chat/cloud 控制面，不是
`AgentRuntime × N + Orchestrator` 的目标多 Agent 架构。

M4-C foreground 切换后还已物理删除：

- `core/runtime_contract` 中从未编译、没有生产消费者的 10 个 speculative shadow 文件；
  `RuntimeEventKind` 现在只在 `crates/protocol` 定义。仍被 exec 输出、runtime 与 CLI
  presentation 消费的 typed `termination.rs` 保留并继续真实编译。
- TUI 无调用者的独立 `resume`/Agent-root picker wrapper、专属过滤 helper、projection public
  cursor getter，以及没有 producer/handler 的 Fleet model-draft ViewEvent/delivery cell；
  `attach_or_resume`、`latest_root`、canonical app Run API、内部投影游标与真实 Fleet 保留。
- 旧 foreground Engine、EventBroker 和 runtime-thread state owner；
- `SessionManager` 与旧 session/checkpoint helper；
- TUI child worker cache、mailbox reducer、fanout card 和第二展示真相；
- registry-driven slash command system（64 files，净删 25,516 行）；
- CodeWhale 自托管 MCP server 的两套实现与 `crates/mcp`；外部 MCP client 与 canonical
  app-server 保留；
- `codewhale serve --acp` 及其 1,210 行独立 session、stream 和 direct
  `DeepSeekClient` 路径；显式 `--prompt "serve --acp"` 仍只是普通 canonical Agent 输入。
- direct `review` completion、模型内 `ReviewTool`、私有 receipt 状态/文档和退役 review
  UI；canonical reviewer Agent profile 与普通自然语言代码审查任务保留。
- 退役 TUI model client、cache、mock、retry surface 及其旧请求/响应/SSE DTO；仍在
  `models.rs` 的类型只服务现有展示、计量、MCP/工具 schema 和模型元数据消费者，不再发送
  DeepSeek 请求。
- 没有当前事件循环消费者的 TUI frame limiter、frame requester、motion policy 及其
  `constrained_frame_rate` 假设置；实际事件轮询、动画 cadence 和 `low_motion` 路径保留。
- 无生产写入方的持久 composer stash、Doctor 假投影和不可达 Ctrl+S/`/stash` 产品声明；
  只有清空 writer、没有 restore 入口的进程内撤销草稿也已删除，`Ctrl-C`/`Esc` 直接清空输入。
- 唯一构造 helper 自身无调用方、且所有事件都没有 canonical handler 的 TUI Setup Wizard；
  CLI `setup`、Doctor setup 诊断、`SetupState`/`UserConstitution` 和提示词上下文消费继续保留。
- 无生产按键入口的 Activity Detail/Turn Inspector、shell details 路由、composer 外部编辑器、
  假快捷键提示和只读不写的推理折叠/详情高亮状态；canonical 审批参数仍可由 `v` 打开本地
  通用分页器并复制，审批状态继续只由 canonical interaction/RunStore 管理。
- 没有 canonical 命令、按键处理或 Run 事件的 `/jobs` 作业中心、Ctrl+B/Ctrl+X 假控制、
  sidebar 点击动作和 TUI 私有 shell manager；同步 `exec_shell` 的命令/输出直接投影到
  canonical 工具卡，生产 executor 的取消、超时和进程树清理保留。
- 只有自测而无 production delta 写入方的旧 TUI `StreamingState`/`streaming_thinking`
  collector 及其影子 reasoning buffer；canonical reasoning/assistant 流继续由
  `run_presenter` 从 durable `RuntimeEvent` 直接投影，完成态不再被旧 duration 字段误投影为
  “空闲”，DeepSeek reasoning replay 不变。
- 无生产消费者的 TUI Goal/Hunt loop、私有 TaskContract/receipt/Goal 工具、Slop ledger、
  verifier preview config、假 custom-command pause/allowed-tools 状态及其 UI/文档；
  canonical Runtime terminal、RunStore 和确定性 `crates/tools::run_verifiers` 保留。
- 没有生产构造或写入方的 TUI-local Plan/Todo Store 与 `update_plan`/`todo_*` 假工具，以及
  只读取永久空状态的 WorkSurface/sidebar/footer 和旧 transcript/checklist 特判；
  canonical root/child Run 投影和多 Agent 能力保留。
- 只有测试构造、没有 canonical producer、RunStore 表或 RuntimeEvent writer 的
  `App.task_panel`/`TaskPanelEntry` 及其 background shell reader；WorkSurface 现在只显示
  canonical child Agent，Activity 继续显示 canonical `GenericToolCell`。
- 没有 production 构造方的 TUI `ExecCell`/`ExecSource` 专用展示岛，以及只读取该旧类型的
  foreground shell chip、CI 状态猜测、命令时长和 live-output 分支；canonical
  `ToolPrepared`/`ToolOutcomeCommitted -> GenericToolCell` 投影仍完整承载 `exec_shell` 的
  命令、状态、输出、失败可见性、transcript 与 Activity 展示。
- 同样没有 canonical producer 的 `Exploring`/`PatchSummary`/`DiffPreview`/`Mcp`/
  `WebSearch` 五种 TUI 专用工具卡及其聚合、状态和 renderer；`ToolCell` 单变体兼容壳也已
  折叠为直接的 `HistoryCell::Tool(GenericToolCell)`。固定 16 工具现在只走这一展示模型，
  `git_diff`/`git_status` 也按真实名称获得 edit/read 语义；MCP transport 与 CLI 不依赖旧卡。
- 只服务已删除旧工具、没有生产 executor 或 registry 消费者的 TUI `ToolSpec`、
  `ToolContext`、`RuntimeToolServices` 与本地 `SandboxPolicy`；错误分类直接使用
  `codewhale_tools::ToolError`，固定目录、production context、sandbox 与 shell owner 不变。
- 无调用方的 TUI OpenAI/Anthropic message/tool DTO 与 MCP `to_api_tools` 广告器；它们未进入
  canonical `AgentRuntime`，删除不会改变 MCP 配置、transport、OAuth、发现或 CLI 管理。
  当前 model-visible 工具仍只由固定目录和 Runtime 条件内建工具产生。
- 顶层 `codewhale update` 与 CLI 自更新实现；仍有真实 TLS、MCP/OAuth、Fleet 和显式
  Doctor 消费者的 `crates/release` 保留，不属于本次删除。
- TUI 私有生命周期 shell-hook owner、`App.hooks`、`[hooks]` 配置和虚假参考文档已物理删除。
  切换后 `mode_change` shell hook 不再执行，受信任工作区中的 `.codewhale/hooks.toml`
  也不再读取；其余生命周期事件原本就没有 canonical production 派发。
  Runtime/tools/Run projection、MCP notification、panic hook、Fleet webhook 和桌面通知均未改变。
- 零反向依赖的通用 `crates/hooks` 及其 stdout/JSONL/webhook/Unix-socket fan-out 原型；删除前
  workspace Cargo 反向图只返回该 crate 自身。`codewhale-config` 的 `[hook_sinks]` typed
  schema 与 key-value API 分支同样没有 sink 注册方或生产读取方，现已连同只验证旧 TUI
  lifecycle `[hooks]` 透传的自测承诺删除。该切片没有生产行为损失，也不改变通用 unknown
  extras 处理；MCP protocol notification、panic hook、Fleet alerts/webhook、桌面通知和
  canonical Runtime/tools/RunStore 都由其他真实 owner 保留。
- 没有构造方、不会读取图片数据的旧 TUI `ToolCell::ViewImage` 文本卡；canonical
  `read_file` 仍通过 `crates/tools` 的 macOS Vision/Tesseract 后端提供本地图片 OCR。
- 只有自测构造、没有生产打开入口的实时对话 overlay、其专用缓存和失效快捷键；主
  transcript 仍由 `CanonicalRunProjection`/presenter 与 `TranscriptViewCache` 实时驱动。
- App 状态始终为 `None`、没有生产 toggle/key handler 的 File Tree pane 及其后台目录扫描；
  `file_mention` 仅保留 composer 路径补全，不与旧 pane 或隐藏文件注入混为一体。
- 只有自身测试消费者、从未被 canonical prompt composition 调用的旧 TUI `memory.rs`；生产
  system prompt 仍由 `crates/context` 唯一构造，项目 instructions/skills/WorldState 不变。
- 没有任何生产 renderer 或 key handler 读取的静态 `keybindings.rs` 目录；帮助与按键行为
  只以真实 UI/PTY 契约为准，不再维护一份自测通过但不可见的快捷键表。
- 零调用方的旧 TUI MCP manager formatter/pager adapter，以及没有生产 writer 的 manager
  snapshot DTO、App cache、restart hint 和伪连接健康投影。`McpPool`、transport、OAuth、
  配置加载与顶层 `codewhale mcp` 的连接/工具发现仍是真实生产能力；当前 canonical Agent
  固定工具目录并未加载 MCP pool，也不存在 TUI `/mcp` manager 或 model-visible MCP 工具。
  私有 `mcp` 模块中八个没有生产消费者的 public wrapper、对应 dead-code allow 与无人读取的
  shutdown report 也已删除；真实 reload/reconnect/transport shutdown 生命周期不依赖它们。
  随后又删除从未被 production caller 接线的 MCP tool/resource/prompt execution、动态 runtime
  server、prefixed-name dispatch 和 execution retry；连接现在只完成 initialize 与
  `tools/list` discovery。CLI 仍可连接和列出工具，但不会执行发现结果。当前门禁为 MCP
  定向 79/79（另有 1 个既有 ignored flaky listener）、真实 CLI 2/2、OAuth 6/6、HTTP auth
  2/2、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check、fmt 和 diff-check。
- MCP 配置修改现在只有 `crates/tui/src/mcp.rs` 一个 owner：`setup --mcp` 和
  `mcp init/add/remove/enable/disable` 都经同一套路径校验、完整 `McpServerConfig` 往返和
  原子写入，`main.rs` 原有的模板/load/save/init 重复实现已删除。修改命令只写 resolved
  global `mcp.json`，不会把 trusted workspace 或 plugin 合并结果反写；`list`、`login`、
  `logout`、`connect`、`tools` 和 `validate` 继续 workspace-aware。OAuth、network/TLS、
  stdio/Streamable HTTP/legacy SSE、连接和 tool discovery 保持；没有 caller 的 execution
  方法已由后续 M4 切片删除。配置中的 `execute_timeout` 仍可完整往返，但当前没有 runtime
  execution consumer。
- 零消费者的 `fast_hash` 类型别名与用户 regex LRU cache；真实正则消费者保留在各自 owner。
- 只有自身测试、App 只默认构造且从不读取的通用 Provider readiness snapshot；DeepSeek
  production transport 与 Doctor 明确探针继续分别承担请求和诊断职责。
- App 永远为 `None` 且没有生产 writer/handler 的旧 Decision Card overlay；结构化选择的
  唯一真实路径仍是 canonical `request_user_input` interaction。
- 零生产调用方的 TUI `arg_repair` 启发式修补器；正式路径由 DeepSeek transport 精确拼接
  SSE 参数，`ToolArguments` 同时保留 raw/parsed，Runtime 对畸形 JSON 产生 typed retry
  outcome 而不篡改原始调用。
- 只有自身测试的旧 `/models` 英文消息 formatter；TUI canonical 命令面没有模型列表或
  picker，运行模型仍由 DeepSeek 配置进入 canonical Run 请求。
- 从未被生产构造、没有事件 handler 的旧 `ElevationView`/widget/event 整岛；真实审批与
  结构化提问仍由 canonical interaction 承担。旧 Run 级
  `allow_sandbox_elevation` 已由 M27 删除；sandbox 拒绝不会借兼容 flag 扩权重试。
- 未注册且零执行调用方的旧 TUI `RequestUserInputTool`/parser 与 prompt shadow；保留的
  UserInput modal 直接消费 protocol request/response，提交仍落入 canonical RunStore。
- 旧 `composer_ui` 键盘处理器岛；唯一真实使用的 slash-menu 选择已迁回 canonical 事件
  owner `ui.rs`，其余 escape/history/word-motion/newline helpers 没有生产调用方。无 canonical
  输入入口的 recall、line/word-forward 编辑 helpers 与只写不可读的跨进程 history 文件线程
  也已物理删除；普通输入和模型 `exec_shell` 工具保持原真实链路。
- TUI 私有 approval cache、exact/grouping key、永远未设置的 timeout/tick，以及没有
  canonical Runtime 消费者的“批准并保存询问规则”事件载荷和界面。当前审批事件只携带
  `interaction_id + decision`，由 `TuiRunClient` 调用 canonical `resolve_interaction` 或
  `cancel`；工具、风险、参数与 durable replay 仍由原真实链路承担。
- 交互循环中曾零调用的 `ViewStack::handle_mouse` 现已成为唯一 modal 鼠标入口；活动 modal
  优先消费点击/滚轮并阻止背景穿透，产生的事件仍交给现有 canonical view-event handler。
  无 modal 的 transcript 滚动路径保持不变，没有新增私有 interaction owner。
- `scrolling.rs` 的 `TranscriptScroll::anchor_for` 只有自身测试；rapid mouse acceleration
  类型只被 `ViewportState` 默认构造且没有生产 reader。两组叶子现已删除，真实滚动仍由
  canonical mouse 的固定三行 delta、`pending_scroll_delta` 和
  `TranscriptScroll::scrolled_by`/`resolve_top` 驱动；Pager 保持独立滚动处理。
- `ColorCompatBackend` 的 forced/cached size override 字段和 setter 没有任何生产 writer，
  仅由同文件 3 个测试构造虚假尺寸路径，现已删除。`Backend::size()` 直接读取真实
  Crossterm backend；保留的颜色深度适配、palette/theme 更新与 OSC8 link 发送继续由原 owner 执行。
- transcript `selection.rs` 的 selection/autoscroll 状态没有任何生产事件 writer，只会默认构造，
  再被自动滚动 guard 与 renderer 读取；该模块、viewport 字段、resize clear、着色 helper 和专属自测
  现已删除。后续同样按无生产 writer 的调用图删除 composer 内部 selection 状态与着色分支；
  菜单/审批 palette 状态、Pager 系统复制、终端原生选择及 canonical scroll 均未改变。
- `CopyLineSeparator`、copy prefix width 和 soft-wrap separator 只服务已删除的 transcript selection/copy，
  但曾从 markdown 经 history 与 per-cell cache 一直写入 `TranscriptLineMeta`，没有任何生产 reader。
  该逐行字段、装饰 prefix 计算器、cache 影子数组和专属自证测试现已删除。仍有真实消费者的
  metadata helper 已重命名为 render 职责，仅传递 `Line`、OSC8 links、`is_code` 和 cell/line 映射；
  Pager/审批复制、终端原生选择、transcript scroll/cache 与渲染均未改变。
- `ui_text` 的 `history_cell_to_text`/`line_to_string`/`line_to_plain`/`append_spans_plain`/
  `slice_text` 只由自身测试或上述已退役 copy/export 假路径调用；删除后，
  `HistoryCell::transcript_lines`、`GenericToolCell::transcript_lines`、`osc8::strip_into` 与
  `TOOL_COMMAND_LINE_LIMIT` 也成为零消费者并已同步删除。当前不存在独立 transcript export owner；
  中文/CJK display-width 回归由真实 `text_display_width` owner 直接覆盖。仍被失败工具完整展示
  使用的 `RenderMode::Transcript`、`strip_ansi_into`、OSC8 生成/链接区域/发送、Pager/审批复制、
  普通 composer cursor/layout 和 transcript render/cache 均保持原生产链。
- composer Vim 设置曾只能在 App 启动时构造本地状态和顶栏标签；canonical key handler
  从未调用 `vim_mode.rs` 或任何 Vim helper，因而所谓 Normal/Insert/Visual 编辑没有生产交互入口。
  该模块、App 字段/helper、设置与别名、标签本地化和 widget 分支已删除。composer 仍由单一
  canonical 键盘路由驱动普通输入、cursor 与渲染，字符 `v` 不再可能被幽灵 modal 状态吞掉；Pager
  的 `j/k/g/G/y/q` 是独立真实交互，保持不变。
- sidebar 的旧 hover/click 元数据只有 renderer producer，没有事件 handler、tooltip 或
  popover consumer；`SidebarHoverState`/section/row/action、每帧全文克隆、tooltip shadow
  和从未构造的 `SidebarAgentCancel` 已物理删除。可见 Activity/Agents/Session 行继续直接
  渲染，canonical child/Fleet、modal 鼠标与 transcript 滚动保持原生产调用链。后续审计已
  证明 `last_sidebar_area` 也只有 renderer producer 并将其删除；当前保留的是一列可见分隔线，
  不是已经接通鼠标 reader 的 resize 能力。
- canonical TUI presenter 曾在 `GenericToolCell` 之外重复写入完整工具参数/输出到
  `ToolDetailRecord`，但该详情图没有任何生产 renderer、交互 handler 或其他语义 reader；
  只有历史前缀重键、active flush 搬运和自身 replay 测试维护这份影子状态。该结构、两个
  App map 及重复 producer 已删除；`tool_cells` 仍负责 prepared→outcome 的真实 history
  原位更新，`HistoryCell::Tool(GenericToolCell)` 仍承担 live/replay/transcript/Activity
  展示，canonical `RuntimeEvent`/`ToolOutcome`、`ActiveCell` 与 child/Fleet 均未改变。当前
  定向证据为 presenter 13/13、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check、
  fmt 与 diff-check。
- `HistoryCell::Error` 没有 production producer；删除前只有 4 个 renderer 自测直接构造它，
  其余引用均为 renderer 或 transcript cache 的穷尽匹配。该变体、专属标签/样式/纯文本换行
  helper 和直接构造测试现已删除。模型请求失败与 terminal failure 仍由 typed
  `RuntimeEvent` 经 canonical presenter 呈现；child/system 消息继续使用
  `HistoryCell::System`，失败工具继续使用 `HistoryCell::Tool(GenericToolCell)` 与
  `ToolStatus::Failed`，session diagnostics 继续使用 `error_taxonomy`。当前定向证据为
  history 72/72、transcript cache 1/1、presenter 13/13、canonical Run 19/19、PTY 6/6，
  并通过 TUI all-target check、fmt 与 diff-check；Widgets 全模块另有 2 个与该调用图无关的
  既有空状态文案断言失败，未作为本切片通过证据。
- `ToolStatus::Hydrated` 没有 canonical presenter 或其他 production producer；删除前只有
  theme/history/sidebar 的穷尽展示分支和一条主题自测断言。该变体及专属分支现已删除，
  `GenericToolCell` 只投影真实 lifecycle 的 `Running`/`Success`/`Failed`。名称相似但独立的
  canonical `RunReplay`、resume、SQLite `RunStore` 重建和 durable hydrate 语义位于
  protocol/runtime/state/app，均未改变。当前定向证据为 history 72/72、theme 5/5、sidebar
  45/45、presenter 13/13、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check。
- `McpOutputSummary`、`summarize_mcp_output` 与 `output_is_image` 只在
  `history/tool_output.rs` 内部互相调用，没有外部 consumer 或专属测试，现已整体删除。
  `summarize_tool_output`、`truncate_text`、canonical `GenericToolCell` 与其真实 presenter
  producer 均保留。MCP config/connect/tools/OAuth 以及 stdio/HTTP/SSE transport 位于独立
  生产路径，不依赖上述 TUI helper，也未改变。当前定向证据为 history 72/72、tool-output
  renderer 1/1、presenter 13/13、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check。
- `footer_ui::one_line_summary` 只有自身测试调用，现已连同该测试删除。真实 footer/sidebar/
  tool output 的摘要、截断及 `strip_ansi_into` consumers 均未改变；footer 9/9、canonical
  Run 19/19、PTY 6/6 与 TUI all-target check 通过。
- `ActiveCell` 没有 production producer；canonical presenter 已经把 `ToolPrepared` 直接写入
  `App.history` 和 `tool_cells`，再由 `ToolOutcomeCommitted` 更新同一 `GenericToolCell`。
  该模块、三个 App 字段、virtual transcript/cache、active footer/sidebar/phase 分支及其
  自测和本地化键现已删除，tool-run 检测只遍历 canonical history。terminal 只收束 streaming、
  loading 和 accounting，不会凭终态补写工具结果；没有 `ToolOutcome` 时保留真实 `Running`
  状态，不伪造 `Failed`。无真实等待来源时也不新增旧路径从未展示的 stall 文案。
  child/Fleet、canonical `RuntimeEvent`/`RunStore`、cancel/control 和工具输出展示均保持原 owner。
  当前证据为 presenter 14/14、history 72/72、sidebar 42/42、footer 10/10、phase 22/22、指定
  widget 5/5、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check、fmt 和 diff-check。
- 交互 TUI 现在在合并 user/workspace/project 配置后只接受官方 DeepSeek Provider，以及
  `deepseek-v4-pro`、`deepseek-v4-flash` 两种显式模型投影；失败发生在 raw terminal、
  RunStore 和 HTTP 之前。`run_tui` 会在终端初始化前再次核对 `TuiOptions.model` 与配置投影，
  `App::new` 不再允许持久 Settings 覆盖 provider/model。旧启动后 Provider 强制改写和 App
  私有 `provider_models` 状态已经删除。应用层也会在持久 creation reservation 前校验显式
  模型，因此 CLI、TUI 和本地 API 不会为非法模型留下 pending creation。交互入口读取
  provider-scoped/root 的原始显式模型并先行校验，不能再经通用默认解析把外国模型静默
  回落成 V4 Pro；确实没有配置时才采用官方默认。Run API 省略 model 时使用中性 fixed
  actor profile：root Pro/high、普通 read-only child Flash/high、Writer Pro/high、typed
  recovery/recheck/rework Pro/max。不存在 model/reasoning Auto 或额外 route request。
  onboarding 仍可写入官方 Key；Beta FIM transport 没有被实现或替代。
- exec stream-json 中零调用的旧 stdout 直写 helper 已删除。真实输出仍由 `ExecOutput` 队列
  统一写出，事件序列化继续由 `exec_stream_line`/`exec_stream_value` 拥有，terminal 仍通过
  `write_exec_stream_terminal` 等待 acknowledgement；canonical Runtime/RunStore、工具和子
  Agent 事件均未改变。exec stream、child receipt 和真实 terminal NDJSON 验收各 1/1，并
  通过 TUI all-target check、fmt 和 diff-check。
- `error_taxonomy` 中只有模块自测构造的 `ErrorEnvelope`、`ErrorSeverity`、全部 envelope
  constructor/Display/Error 实现及 `From<ToolError>` 已删除。生产会话诊断继续使用
  `ErrorCategory` 和 `classify_error_message`，调用链仍为
  `session-diagnostics -> classify_session_failure -> classify_error_message`。分类顺序继续
  保证精确 DeepSeek invalid/reasoning replay 错误先于泛化 tool 分类、API Key 认证先于授权、
  timeout 先于 network、rate-limit 先于 authentication、invalid-input 先于 tool。当前定向
  taxonomy 18/18、session diagnostics 7/7 通过，并通过 TUI check、fmt 和 diff-check。
- 最终调用图证明 Classic header/footer/sidebar 整帧链没有生产 renderer consumer。唯一仍
  使用的状态标记迁入 Underwater 后，Classic shell、resize/hover/width shadow、专属测试与
  211 条孤儿消息一起删除。Underwater 现在是唯一交互外壳；canonical child/Fleet、
  WorkSurface、modal、transcript、审批和工具卡仍从同一 Run 投影读取。
- `ui_text` 中没有 production caller 的 affix 截断入口及其只被内部调用的 helper、自证测试
  已删除。真实 modal title 继续使用 `semantic_truncate`，footer/sidebar/work-surface/thinking
  继续使用 `truncate_line_to_width`；`text_display_width` 对中文/CJK、组合字符、ZWJ、控制字符
  与窄宽度的契约保持不变。当前定向 ui-text 11/11 通过，并通过 TUI check、fmt 和 diff-check。
- sidebar Agent 行中只写不读的 `role`、只有两个自测调用的 tree sorter 和零调用的 running
  predicate 已删除。真实行仍由 canonical `child_agents` 直接生成并交给 `subagent_panel_rows`；
  `parent_run_id`/`spawn_depth`/`agent_tree_prefix`、终态投影、handoff、child/Fleet 展示均保留。
  当前定向 sidebar 40/40、Run projection 6/6 通过，并通过 TUI check、fmt 和 diff-check。
- 生产 `SidebarAgentRow` 现在只保存 canonical child 投影实际提供并消费的
  `parent_run_id`、`spawn_depth`、`name`、`status`。没有可达展示消费者的 id/progress，以及
  固定为空或零、没有交互 producer 的 model/objective/branch/steps/duration/expanded 已删除；
  不可触发的 expanded dossier
  和从未存在读取端的 `agent:<id>/full_transcript` handle 也已删除。handoff 没有丢失：
  `ChildFinished.handoff_content` 继续由 canonical presenter 投影进 transcript。当前证据为
  sidebar 33/33、presenter 14/14、Run projection 6/6、六子 Agent fanout 1/1、两项 exec 父子/
  孙级汇合各 1/1、Fleet worker 1/1，并通过 TUI check、fmt 和 diff-check。header 的
  `progress_only_count`/`fanout_*` 已由后续独立调用图切片处理。
- `SidebarSubagentSummary` 原有的 `progress_only_count`、`fanout_total`、`fanout_running` 没有
  生产 producer：唯一生产构造器始终通过 Default 写入 `0`/`None`，唯一非默认 fanout 值来自
  自身测试。三字段、header 的不可达覆盖分支及自证测试现已删除；header 只投影 canonical
  `child_agents` 的 total/running/role counts。真实 Runtime pending children、child lifecycle、
  release fanout、exec 汇合和 FleetExecutor 不依赖这组 sidebar 私有字段。当前证据为 sidebar
  32/32、presenter 14/14、Run projection 6/6、六子 Agent fanout 1/1、两项 exec 汇合各 1/1、
  Fleet worker 1/1，并通过 TUI all-target check、fmt 和 diff-check。
- spinner 中零调用的 `braille_spinner_frame_for_duration_ms` 薄包装已删除。生产 tool marker
  仍调用 `braille_spinner_frame(Instant)`，并复用 `braille_spinner_frame_for_elapsed_ms` 的 cadence、
  quick-event 门槛与 low-motion 语义；verification tick 不受影响。当前 spinner 3/3、TUI check、
  fmt 和 diff-check 通过。
- 没有生产构造者的 TUI `AutoReviewPolicy`、动态 allow/block 配置、私有审计事件和重复的
  shell/action 风险分类。production `crates/tools` 直接在 canonical
  `ToolApprovalPrompt::risk` 中给出 `Routine`/`Elevated`/`Critical`，TUI 只负责穷尽投影与
  展示，不再根据工具名或参数重算风险。
- 旧 Engine 的 post-edit LSP 集成。`crates/tui/src/lsp` 中的 `LspManager`、stdio transport、
  `publishDiagnostics` parser、diagnostic renderer 和 language registry 没有任何生产构造方，
  所有 manager 调用都仅属于自测；该四文件编译岛、TUI/`codewhale-config` 的重复
  `[lsp]` schema、侧栏虚假 `lsp: on/off` 状态和专属文档已同步删除。MCP transport 及
  `notifications/initialized` 不在该路径，继续保留。M5 若用 LSP 补强 RepoGraph，必须在
  canonical context/tools 路径重新实现并评测，不恢复 TUI Engine 兼容层。
- 旧 TUI side-git snapshot 岛及 `[snapshots]` 配置。它没有 production snapshot writer、
  restore/list UI、`/restore` handler 或 `revert_turn` 工具；唯一真实调用方是交互启动时对旧
  仓库执行保留期 prune。该 janitor 随整岛删除后，不再自动清理
  `~/.codewhale/snapshots`/`~/.deepseek/snapshots` 中的历史 side-git 数据，也不会在切换时
  自动删除用户文件。Runtime `RunSnapshot`/`RunReplay`、State `agent_run_snapshots`、canonical
  reducer/crash replay、工具 artifact 和 Fleet checkpoint 是不同 owner，均未改变。
- 旧 `setup --clean` JSON cleanup 产品面。`SessionManager` 删除后，
  `sessions/checkpoints/latest.json` 与 `offline_queue.json` 已没有 production writer/reader；
  `CleanPlan` 只在显式 CLI cleanup 及自身测试内闭环。删除后程序不再列出或移除既有历史
  文件，用户需要时只能手工处理。系统 skills 的生产自动安装仍保留；无调用的 uninstall/
  bundled-name helper 同步删除。Setup `--force`、SQLite RunStore/`agent_run_snapshots`、crash
  replay、进程内 busy-message queue、Fleet ledger/checkpoint 和 constitution checkpoint 均不经
  该路径。
- 没有生产命令、canonical Runtime 调用方或模型可见入口的 community skill installer。
  `crates/tui/tests/skill_cli.rs` 只是用 `#[path]` 重新编译该源文件，不是 CLI 验收；现已连同
  registry URL/安装大小 schema、伪配置说明和 `tar`/`flate2` 直接依赖物理删除。TUI
  和 `codewhale-config` 都不再将两个旧键建模为受支持 schema；config crate 只保持 TUI
  `[skills]` 表的通用 extra 往返，其中当前唯一有产品语义的键是 `scan_codewhale_only`。系统
  `install_system_skills`、版本 marker、local skill discovery 与 prompt
  注入继续工作；仅宣称不存在 `/skill install/update/trust/uninstall` 的 bundled
  `skill-installer` 不再进入新安装，既有用户目录不会被程序主动删除。
- 旧 `workspace-trust.json` 外部路径快照的 `add/remove` 及原子写入先因零调用方删除；
  M27 三档权限 cutover 又删除了最后一个 `WorkspaceTrust::load_for` reader、tools trusted
  path 字段和 fingerprint truth。Ask 不会从历史文件获得隐藏外部路径授权，Agent/FullAccess
  也不需要第四种 trust 来源。onboarding/MCP 仍真实读写的 `[projects].trust_level` 是不同
  owner，继续只控制项目配置与 MCP 信任；程序不会主动删除既有历史文件。
- TUI startup version checker 及 `[update]` schema。`spawn_startup_version_check` 在全仓只有
  定义，既没有启动调用，也没有 task join、toast 或 renderer 消费者；其余 release JSON、
  asset completeness 和自制 semver helper 只由这条死链及自身测试引用。删除因此没有运行时
  行为损失，旧 `[update]` 表不再由 typed config 消费。Doctor 仍显式执行 release 诊断；
  `crates/release` 的平台 HTTP/TLS builder 继续服务 MCP/OAuth/Fleet alerts 与配置网络调用，
  均未随幽灵启动检查删除。
- pre-session Launch menu 的 `LaunchState`、action/handler、renderer/hitbox、`launch_screen`
  设置和专属本地化。生产启动曾构造该状态并同步执行 `git rev-parse`，随后在第一帧前无条件
  将其隐藏；没有 launch action 进入事件循环、Run command、Lane 或 Fleet。删除因此只移除
  不可达 UI 和无效启动子进程，零生产行为损失。首启 onboarding、canonical TUI Run、CLI
  resume/continue、underwater shell/ocean 与 Fleet/Lane/worktree 均不经过该旧外壳。
- TUI desktop notifications 模块及其 `[notifications]`/`tui.notification_condition` schema、
  专属本地化和 Windows Audio/Debug/UI features。OSC/BEL/macOS 通知、声音、terminal
  title/taskbar 及配置入口没有模块外生产调用，故删除为零生产行为损失；唯一真实消费者是
  footer 对 `humanize_duration` 的调用，该 helper 和边界测试现由 footer 直接拥有。MCP
  `notifications/initialized`/`notifications/progress`、Fleet alerts/webhooks、canonical
  active/run status 和 panic hook 均为独立生产路径，未被删除。
- 系统剪贴板 read/image 与伪 composer attachment 状态。生产只有 Pager copy 经
  `ClipboardHandler::write_text` 写系统剪贴板；图片读取/PNG 落盘、App attachment
  插入/选择/移除、手写 `[Attached ...]` parser 和对应 UI/文档均没有生产入口，删除为零
  生产行为损失。arboard 现关闭默认 image feature，TUI direct `image` dependency 已删除；
  terminal `Event::Paste`/onboarding、文本 writer、普通 `@mention`、canonical `read_file`
  OCR 和现役 composer 文本编辑均保持原 owner。旧 rapid-key paste-burst handler 从未进入
  canonical Key 事件分支，App 只会轮询无法由生产输入激活的默认状态；该两模块、状态、
  设置/别名和自证测试现已物理删除。无 bracketed marker 的字节按普通按键直接进入
  composer，不再虚构可区分快速键入与裸 paste；真实 bracketed mode 启停/恢复、API-key
  paste、CRLF/裸 CR 归一化、超大文本处理与 Pager copy 均不经过已删除 fallback。
- `key_shortcuts` 中零调用的 copy/paste/control-like/Ctrl-H predicates；保留首启输入所需
  的 `is_text_input_key`，真实 paste/copy 仍由 terminal event 与 Pager local event 承担。
- 零生产消费者的通用 `[vision_model]`/`image_analyze` 配置与 feature；正式模型面仍只有
  DeepSeek，图像文本提取继续由 `crates/tools` 的本地 `read_file` OCR backend 承担。
- 首启状态机不可达的 Provider 选择页与 picker memory；当前真实首启只包含 Welcome、
  DeepSeek API Key、工作区信任和 Tips，不再编译一页无法到达的多 Provider UI。
- 零生产调用方的 provider-lake picker/dashboard 查询面，以及只有测试 writer、生产始终为
  `None` 的 live snapshot/merge 状态；保留的 bundled catalog lookup 仍服务现有 pricing 与
  尚待 M7 收敛的 route metadata，canonical Fleet 和 DeepSeek transport 不读取已删除的
  configured-provider/model-list API。
- `config::model_completion_names_for_provider` 的硬编码 Provider 模型列表只有专属自测，
  provider picker 与 inventory 消费者均已删除；该函数、列表测试和仅服务该列表的聚合常量
  现已物理删除。真实默认模型、alias/capability 常量、Codex account roster、Fleet route、
  bundled pricing 与 DeepSeek 官方模型校验保持各自 owner。
- 旧 provider/model picker 留下的 display sorting、requested-model/route validator、wire-model
  wrapper、configured-provider 判定和 custom-kind convenience 方法没有生产调用方，仅由配置
  自测互相证明，现已物理删除。真实配置加载归一化、`route_runtime::resolve_route_candidate`、
  Fleet route receipt、DeepSeek official model fail-closed 与 custom provider schema 均保留。
- 旧 provider setup UI 留下的通用 bool/integer/provider-base-url/custom-provider TOML writers
  只有自身测试调用，现已连同私有 normalization 闭包物理删除。生产配置修改仍统一经过
  `mutate_config_document` 和原子 `0o600` 写入；DeepSeek 首次配置、workspace trust、CLI
  login/logout 与 legacy approval migration 的真实窄入口均保留。
- 同一旧 setup 链的 provider-scoped API-key/model writers、targeted-key clear 和 Kimi
  credential-valid convenience predicate 也没有生产消费者，现已物理删除。`codewhale login`
  /`logout`、TUI DeepSeek 首次配置、Doctor `has_api_key_for` 与 Kimi token refresh 链保持不变。
- TUI ProviderConfig 曾接受 `max_concurrency` 及三个 alias，却从未把它接到任何请求 semaphore；
  该字段、默认/夹紧逻辑和自证测试现已删除。canonical Runtime request budget、Fleet scheduler
  的 worker limits 与 subagent profile limits 是独立的真实执行链，均未改动。
- TUI pricing/route-billing 中只供模块自测调用的通用 route/child/compact-chip 包装层和
  `CostEstimate` convenience methods 已删除。官方 DeepSeek 双币 pricing、canonical
  `ModelAccounting`、`RunStore` 回放、`/cost`、footer/sidebar、scorecard、确定性预算和 Fleet
  回执均继续走原生产 owner；本切片没有删除或伪造子 Agent 的真实 Token/成本聚合。
- TUI 曾声明 DeepSeek 账户余额 DTO、后台刷新 cell、可配置 footer item 和余额布局，但全仓
  没有 `/user/balance` 请求或任何 writer；该链运行期永远为 `None`。现已连同自证测试和文案
  整体删除，旧 `status_items = ["balance"]` 会由既有未知项规则忽略。真实 usage/cost、
  cache、scorecard、`/cost` 和 root/child accounting 不经过该幽灵链。
- TUI 的 `subagent_cost` 与 cost high-water 字段没有生产 writer，sidebar 因此长期显示虚假的
  `session + agents` 拆账。该第二账本和零-reader header cost 参数现已删除；Run presenter 只把
  canonical `ModelAccounting` 的 root+child 聚合总额投影为 `total_cost_usd/cny`，`/cost`、
  footer、phase strip 和 sidebar 都直接读取该唯一总额。按 Agent 拆账只有 protocol 增加 actor
  cost 维度后才可实现，不能由 TUI 推算。
- 首次启动后的 Fleet-ready nudge 只有测试调用、没有生产触发点；其 App 方法、持久化
  `feature_intro_shown` 标记、中文文案和自证测试现已删除。真实 onboarding、空状态与 Fleet
  命令/多 Agent 调度不经过该幽灵提示。
- App 的动态模式与 permission posture 状态机没有 canonical key、slash command 或 Run event
  producer；`set_mode`、Tab/Shift-Tab cycle、Agent baseline、policy-lock UI mirror 和对应设置
  写入只在自测内闭环，现已物理删除。后续调用图又证明 `AppMode/default_mode` 只剩启动标签、
  颜色和错误的 Plan“只读”提示，因此连同 legacy YOLO 设置迁移、Doctor 字段和渲染分支一起
  删除；旧 `default_mode` 现在被忽略且不能授予权限。真实显式 `--yolo` 输入只在当前进程
  为后续新 Run 选择 FullAccess 并启用 shell，不经过模式标签。`ApprovalRequest` 不再重复保存无人
  读取的英文 impact 列表；保留的简体中文 `impacts()` 只是 TUI 展示摘要，canonical risk
  仍只来自 `crates/tools`。
- M27 建立了唯一 `RunPermissionMode::{Ask, Agent, FullAccess}`。TUI 只显示“请求批准 /
  替我审批 / 完全访问权限”三行；默认 Ask，session 选择只影响后续新 Run，active Run 继续
  投影冻结值。`dse exec --auto` 映射 Agent，显式 `--yolo` 映射 FullAccess；没有 Custom、
  `[permissions]`、config editor、隐藏第四模式或自由组合字段。
- `crates/tools` 在副作用前生成唯一 typed `ToolAuthorizationDecision`，绑定 exact tool、
  arguments digest、workspace revision、matched rule 与 risk；Runtime 只持久化
  decision、排序 durable approval/start/outcome 并在 reopen 时复用。Ask 的普通工作区
  edit/test prompt 为 0；当前 backend 无法证明一次性网络/外部路径授权，因此 canonical
  path-bearing tool、显式 Shell cwd 与可识别 network 调用在 Ask 下 fail closed。任意
  子进程内部 I/O 只声明 OS sandbox 基线，不从 Shell 字符串伪推导。Agent 的
  Host-critical 调用仍 Ask，FullAccess 无动态 prompt，但 explicit execpolicy deny 与
  hard invariant 始终优先。
- RuntimeEvent v22 的 `ToolAuthorizationDecision` 还必须绑定 typed
  `ToolExecutionGrant`。普通模型调用只有 `Ordinary`；Runtime 只会从 exact
  acceptance-ID handle 和 frozen TaskContract 派生 contract-verifier grant，tools 会用
  canonical `VerifierSpec` digest 重新验证。该 grant 只让 exact verifier executable
  program 通过 generic external-path 分类；external cwd、普通外部路径、network、write
  root、explicit deny 和现有 OS sandbox 均未放宽。State v27 退休缺少该 grant 的旧
  materialized Run，只保留能按当前 Start command 无损恢复的 pending intent。M31 的
  两项 fixed-Pro/high Ask continuity task 均取得 latest-revision failed→write→pass
  receipt、false success=0、exact SIGKILL/SQLite reopen 和完整 accounting，因此该
  grant 已保留；临时 M31 Harness consumer 已物理删除。
- `dse-execpolicy` 已收缩为 production 与 `execpolicy check` 共用的 TOML allow/deny
  matcher。TUI 私有 snapshot/parser、tools duplicate matcher、richer ask/session/network
  amendment 类型和旧 bool/trust/sandbox/elevation reader 已物理删除。Run API v14、
  RuntimeEvent v21、State schema v27；v26 不猜旧 tuple，无法无损映射的旧 materialized
  Run 与 pending Start 一次性 fail-closed retirement，之后只保留新 reader/writer。
- 可自由组合 policy/network/writable roots、且不产生 canonical Run/authorization/RunStore
  事实的 `dse-tui sandbox run` 直接执行旁路及其专属 parser/双语文案已删除；底层 sandbox
  backend 继续仅由 canonical tools 与 isolated Writer 消费。
- `[features].exec_policy` 曾只让 TUI 忽略规则、不能同步 app-server，形成 caller 分叉；
  M27 已删除该开关，所有 production surface 都读取同一可选 `execpolicy.toml`。
- 旧 TUI `RetryPolicy::delay_for_attempt` 和 `Config::search_provider` facade 没有 caller，现已
  删除；M44 又删除了同样没有 executor 的 typed search-provider resolution 与 Doctor 投影。
  生产 DeepSeek retry projection 不受影响。
- test-support 的未使用 prefix-diff helpers 与 footer 的四个 test-only parity helpers 没有
  真实测试 caller，现已删除；新增断言直接经过 `render_footer_from -> FooterProps` 保护
  canonical context-percent 与 session-cost 路由，生产 `FooterWidget`/phase strip 未改动。
- 主题模块的公开 selectable inventory、setting facade 和 mode-label helper 只有测试调用，现已
  收缩为 `#[cfg(test)]` shipped-theme 清单；生产 `settings.toml -> ThemeId -> UiTheme ->
  ColorCompatBackend`、12 套 palette 与 Ocean 渲染链保持不变。
- TUI 私有 Config 的 root/provider `http_headers`、env merge 和 accessor 从未接入模型 transport，
  现已连同自证测试删除，避免继续暴露配置黑洞。canonical `crates/config` 仍残留 generic
  header schema，留待 M7 收口；生产 DeepSeek transport 当前不消费任意 custom headers。
- 只由自身测试调用的 TUI `is_key_file`/`summarize_project`/`project_tree` 浅层 project-map
  helpers；生产上下文仍由 `crates/context`、显式文件工具与 canonical transcript 负责，
  没有为当时尚未开始的 M5 RepoGraph/ContextBroker 保留兼容层；当前 ContextBroker 已由
  后续 canonical vertical slice 实现。
- 零生产调用方的 TUI `open_url` 及其平台 browser-command 构造器；当前没有外链打开交互，
  OAuth/MCP transport、终端复制和 DeepSeek HTTP 请求不依赖该 helper。
- 零调用的 TUI `record_caught_panic`、`ensure_dir`、`pretty_json`、`url_encode` 与
  `estimate_message_chars`；真实使用的 supervised panic dump、写入、路径显示和计数 writer
  继续由各自生产调用方保留。
- 从未进入固定 production catalog 的旧 TUI `js_execution`、Node resolver 和 Doctor 假
  注册提示；显式配置的 Node MCP server 与 canonical `run_verifiers` 的 Node 项目检查不经
  过该旧模型工具，继续保留。
- 从未接入 production catalog 的 TUI script-command plugin ToolSpec、复制扫描器的假 E2E、
  `[tools.plugin_dir]`/`[tools.overrides]` 与 `setup --tools`/Doctor 脚手架；canonical 固定工具
  目录不再被配置宣称可替换，真实 MCP、skills 和现有 plugins 目录逻辑不变。
- 只有自身测试、没有生产读取方的 TUI `large_output_router`、`[workshop]` 配置、
  `ToolContext` router/vars 字段与未接线的 V4-Flash synthesis 声明；canonical artifact/
  evidence owner 不在该路径，旧 spillover writer 仍待独立调用图切片。
- 没有生产注册者或 producer、唯一 store 只由默认 `ToolContext` 空建的 TUI
  `handle_read`/`VarHandle` 原型与 process-local store；canonical artifact、RuntimeEvent、
  RunStore 不经该路径，最后仅被它使用的计数 writer 也已删除。
- 零执行调用方的旧 TUI tool-output spillover/retrieval 链、只服务该链的私有 artifact 文件、
  Doctor/启动 janitor 投影和始终为 `None` 的 history 字段；canonical `ToolOutcome.artifacts`、
  verifier artifact、RuntimeEvent 和 RunStore 不依赖这套旧文件缓存，继续保留。

## 6. 当前验证事实

M4-B 提交中的离线验收已经证明：

- exec、HTTP/SSE、stdio 对同一个 read-only tool fixture 产生相同的 12 个 normalized
  canonical event、terminal、usage 和 accounting；
- HTTP SSE 的每个 `id` 等于 Store sequence，`data` 是完整 Store event；stdio 也
  0 丢失、0 虚构；
- 外部 app-server 进程 `SIGKILL` 后恢复同一 run，前缀不变、epoch 提升、event id 唯一、
  terminal 恰好一个；
- live owner 并发 resume 被拒绝；terminal 可由第三个无 Key 进程原样重放；
- app-server 不依赖 core/tui，生产调用图没有旧 bridge 符号。

完整 workspace 门禁、冻结 release build 和官方 DeepSeek canary 均已通过。精确结果、
二进制摘要、费用和非结论见
[M4-B 本地 API 证据汇总](../../eval/summaries/m4-b-local-api-2026-07-17.md)；不能从一次
链路 canary 推断编码能力或效率提升。

M4-C C1 实现提交为 `1d127b78`。conformance/Store replay 已证明 interaction ordering、
幂等响应和同 run 恢复状态机；外部监督进程 `SIGKILL` 测试已覆盖 `InteractionRequested`、
`InteractionResolved`/before-tool-start、`SteerQueued`/before-applied、
`ControlRequested`/tool-in-flight 与 `SteerApplied`/next-model-not-prepared 窗口。focused、
all-target check、全仓 clippy 和 workspace tests 均通过。该冻结本身不构成编码能力或
效率提升证据。

M4-C C2 已冻结为提交 `4a3311ac`：Run API v3、RuntimeEvent writer
v5/read v4-v5；后续提交 `35fc3cc4` 的 durable run creation delivery 把当前 Run API
提升到 v4、State schema 提升到 v9。continuation、root list、manual/automatic context
projection 已进入 exec/app-server 的 canonical 链路。focused、workspace Clippy
`-D warnings`、串行完整 workspace tests、内存/SQLite parity，以及由外部监督进程
`SIGKILL` 的 compaction prepared/in-flight/committed 恢复矩阵均通过。费用受限的官方
DeepSeek production sender canary 以 6/6 请求覆盖 Standard、Thinking/tool-history replay、
Beta Strict 与 FIM，完整 usage、无 transport retry，费用为 `USD 0.0000969904`；该 canary
不包含 compaction on/off 收益对照，且 `product_metric_eligible=false`。

此后交互 TUI foreground 与 child projection 已完成 canonical 切换，最终 State schema 为
v12、RuntimeEvent 为 v6、Run API 为 v4。M4 最终验证结果：

- focused 门禁退出码 0；
- canonical PTY 7/7，exec terminal 24/24，Run surface parity 1/1；
- Runtime conformance 53/53，State 进程级 crash/replay 14/14（1 个 helper 按设计忽略）；
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 退出码 0；
- `cargo test --workspace --locked` 退出码 0；
- app-server 依赖树不含 `core`/`tui`，TUI 依赖树不含 Lane。

当前证据证明三个 foreground 入口已统一，也证明协议、lineage、持久恢复、accounting 与
官方 surface 兼容；hidden workflow、ACP 和 direct review 模型路径已物理删除。最终请求
许可和 eager join 均已有真实 multi A/B：前者的原始实现保留可靠性但 multi 效率未通过，
后者在一个固定任务中降低请求且不回归 handoff。完整 eager-join 身份、四 cell 与 pair
边界见
[子 Agent eager join 精确 A/B](../../eval/summaries/eager-join-exact-ab-2026-07-18.md)。
M5-B 已补齐 compaction on/off 产品收益证据：candidate on/off 均 `6/6` verified，
Token 下降但费用 6/6 对上升、时间方向各半，所以只保留 hard-limit safety 并删除主动
产品面。完整身份与限制见
[M5-B ContextBroker 正式 A/B](../../eval/summaries/m5-context-broker-ab-2026-07-20.md)。
M6-A 又在 `a982a9a8` 完成离线 lifecycle/crash/parity 门禁和一次真实 DeepSeek Writer
机制 canary：7/7 请求完成、0 retry、fast-forward integrate、最新根 revision verifier
通过，临时 worktree/branch 清理。它只证明生产机制可用，
`product_metric_eligible=false`；完整身份与限制见
[M6-A isolated Writer 机制 canary](../../eval/summaries/m6-a-isolated-writer-canary-2026-07-21.md)。
M6-B1 随后在候选 `5d72ae94` 完成 18 对 / 36 arms 正式同二进制 A/B：计量和
canonical terminal 全部闭合，但 single/Writer 仅 `2/18` / `4/18` verified，Writer
仍有 7 false-success、1 次 root may-write 调用和 7 次 retained recovery；Token 与费用
分别增加 35.5% / 52.5%。正式决策为 `reject_and_rework`，要求 Writer 只通过显式
opt-in admission 使用，也不准入双 Writer；当时默认 `RunLimits` / tool policy 尚未
完成默认关闭 cutover。完整结果见
[M6-B1 Writer 收益 A/B](../../eval/summaries/m6-b1-writer-benefit-ab-2026-07-21.md)。
其 rework 在 `3310aa73` 完成 verifier 卫生、explicit-only admission、Host actor
capability、named verifier、时序 EvidenceReceipt、精确 cleanup/recovery 和 Harness v3
冻结；离线门禁全部通过。正式 v3 在第 3 个 arm 遇到一次 retryable
`deepseek_transport`：10 个 physical attempts 中 9 个有 provider response/usage，1 个计费
不可证明。canonical Terminal、RunView 与 accounting 完全一致，Harness 按预注册规则立即
停止且未重采样；结果为 `hold_mechanism`、`product_metric_eligible=false`。这不推翻 v2
的产品结论，Writer 继续 explicit-only，M6-B2 仍不准入。完整身份和证据边界见
[M6-B1 rework Writer 收益 A/B v3](../../eval/summaries/m6-b1-writer-benefit-ab-v3-2026-07-22.md)。

M7-A candidate `24c8a530` 的离线门禁和 exact release identity 已冻结。正式 DeepSeek v1
在首个 T1 pair 后停止：baseline blocked，candidate Completed 且有唯一 Host receipt，两边
外部 verifier 都通过、accounting 都闭合。停止原因不是 candidate 行为错误，而是 Harness
按 sorted JSON 重算 artifact SHA，production binary 却因 `serde_json/preserve_order` 和
未真正排序的 `canonical_json` 按插入序计算。该 mismatch 同时出现在 baseline/candidate，
因此 raw `false_success/reject` 不能成为产品结论；2/40 arms 也不足以保留 candidate。
当前状态为 `hold`，原 suite 不续跑，完整证据见
[M7-A DeepSeek Agent 收敛正式 A/B v1](../../eval/summaries/m7-a-agent-convergence-ab-2026-07-22.md)。

M7-A2 已将相同 canonical JSON correctness patch 应用到 control `18de2ad2` 和 treatment
`c6a74304`，并以独立 exact binaries、共同向量和 v3 Harness 从 position 1 重新冻结。
正式 DeepSeek suite 的前 6 arms 计量完整：T1–T3 treatment 3/3 Completed+verified，control
0/3 blocked，全部 0 false-success。第 7 arm T4 treatment 修改范围和外部 verifier 通过，
但 canonical terminal 为 Failed；4 个 physical response/attempts 只有 3 个 usage response，
`incomplete_responses=1`、`usage_complete=false`、`complete=false`。Harness 按冻结规则
停止，aggregate 为 `hold`、`product_metric_eligible=false`，已知 USD `0.019383566` 只作
下界。raw 没有保存足以归因网络、provider、输出上限或其他具体原因的 typed failure；当前
只能确认 production response/usage 生命周期没有闭合。

因此 M7-A2 证明 canonical JSON 修复消除了 v1 evaluator 假阳性，并为 T1–T3 提供强方向性
机制证据；它没有证明 5-task 总体收益、正式效率或 T5 read-only child。T4 control 与 T5
均未执行，原 raw 不续跑、不补 mate、不拼样。下一 production 切片必须先收敛 incomplete
response 的可诊断、accounting、actionable-output 安全恢复和脱敏 typed failure evidence，
再以新 candidate/suite/output 从 position 1 refreeze；不据此开放多 Writer。完整证据见
[M7-A2 DeepSeek Agent 收敛正式 A/B](../../eval/summaries/m7-a2-agent-convergence-ab-2026-07-22.md)。

M7-A3 已完成该 production correctness 切片。DeepSeek stream 只有在受支持 finish 与
`[DONE]` 都闭合后才提交；usage 在 frame 到达时立即记账；content、reasoning 或任意
tool-call fragment 都形成 typed response evidence 并禁止自动重放。RuntimeEvent v15、State
v20 与 RunStore 重开保持 response evidence、retry decision 和 accounting 一致。Harness
同时区分 HTTP response、usage response、accounting usage 与 committed usage，并在任何
output/Key/API 前拒绝不可采信的 live 复评。

旧 control 的 v13/v18 与 treatment 的 v14/v19 使完整 v15/v20 correctness patch 无法同时
保持 byte-equivalent shared fix 和原 M7-A production delta；因此本轮没有新 formal manifest、
binary 或 raw，也没有读取 Key。M7-A 产品结论继续 `hold`。完整实现、离线门禁与公平性边界见
[M7-A3 DeepSeek 不完整流式响应诊断与安全恢复](../../eval/summaries/m7-a3-incomplete-stream-recovery-2026-07-22.md)。

M7-B 继续使用同一 production composition，没有增加 Runtime、Store、Provider、模型可见
工具或第二目录。`crates/deepseek` 的 compatibility planner 对当次实际 advertised catalog
给出首个 typed blocker；完整目录兼容时才能选择 Beta Strict Chat，任何不兼容都原子回退
Standard Chat。malformed 或不完整 tool-call response 不再被当作可执行调用；reasoning 与
tool-call history 精确绑定 actor turn 回放。

当次完整 `ModelRequestPrepared` 是实际 advertised catalog 的唯一持久输入，State 同时保留
catalog hash；execution fingerprint 绑定 `strict_tools` policy。surface 与 fallback reason
不作为第二份派生状态重复写入，而由唯一 DeepSeek planner 在重开后确定性重建。production
composition 的 SQLite reopen 测试已证明 exact request 与完整 `RequestPlan`（含 surface、
reason、endpoint、body）前后一致，且 strict policy 改变会产生不同 fingerprint。

当前六个默认可执行 actor 的 frozen catalog 均回退 Standard：root headless、interactive、
coordinator 与 read-only child 的首个 blocker 是 `agent` required 合同；depth-limit child
是 `file_search` required；isolated Writer 是 `apply_patch.oneOf`。这些都是当前工具语义，
不能通过删除约束、空值/sentinel、schema 转换或工具裁剪无损消除。terminal no-tools 目录
没有 Strict function call treatment。该结论只覆盖冻结的默认 actor 目录，不外推任意自定义
`ToolPolicy` 子集。

RuntimeEvent v16 / State v21 把每个失败 `ToolOutcome.failure_code` 变成强制 canonical
事实，并保留 invocation、transport、operation、side effect、retry、evidence、artifact、
workspace revision。TUI、JSON summary 与 exec-stream v2 只投影这些事实；旧本地 Strict
开关及其配置/UI 链已物理删除。actor conformance、重复失败上限和真实进程 SIGKILL/reopen
门禁均通过。

最终 admission 结果为 `inadmissible_no_surface_delta`：0 formal arms、0 official API
requests、credential read false、`product_metric_eligible=false`。因此保留 planner、诊断、
无损 fallback 和 typed recovery；Strict 不成为当前默认产品路径，不执行 live Standard/
Strict A/B，也不建立 schema transformer。冻结结论后的 `4e3536f1` 只删除两个可推导的
Strict decision 布尔值与一个零调用包装函数，并补充 SQLite reopen 重建证明；wire 行为、
冻结 candidate、manifest 和 raw 未改。完整证据见
[M7-B Strict 工具调用准入与失败恢复](../../eval/summaries/m7-b-strict-tool-admission-2026-07-22.md)。

M7-C 在不改变 Run API v10、RuntimeEvent v16、State v21 或 exec-stream v2 的前提下，
冻结并关闭了 canonical 编辑 Host 反例。`crates/tools` 仍是唯一 schema/handler owner；
`edit_file` 使用 read-time byte digest，并在原子 replacement 前再次校验原字节；这不是把
content predicate 与 rename 合并的线性化 CAS。`apply_patch` 先完成全量 target/hunk/
result preflight 再写入，单文件 publish 使用原子 replace 并保留 permissions，multi-file
普通错误按精确记录回滚。所有拒绝仍进入唯一 typed `ToolOutcome`，而不是新的编辑状态机。

冻结 tools contract 在起始 production 上 3/12、候选 12/12。clean `9cba8b53` 的 8 项
Harness gates 覆盖 production loopback、latest revision verifier recovery、SQLite reopen、
read-only child、isolated Writer 和三段工具/app-server SIGKILL；official API requests 0，
credential read false。focused、fmt、workspace clippy/test 全部通过。

FIM 调用图仍止于 `crates/deepseek` 的 request planner/accounting；没有 production
Completions transport/parser、Host-owned edit apply 或 RunStore lifecycle，因而没有同
binary treatment surface。M7-C 判定 `inadmissible_no_surface_delta / hold`，未读取 Key、
未执行 live A/B。CLI direct apply、TUI-local eval/edit 与其旧 acceptance 已在 cutover
物理删除。完整事实与官方协议复核见
[M7-C canonical 编辑能力基线与 FIM 准入结论](../../eval/summaries/m7-c-edit-baseline-2026-07-23.md)。

结论后复核 checkpoint `1cb65b82` 继续收紧同一 owner：`changes` 不得携带 patch-only
`path/fuzz/create_if_missing`，`path` 不得覆盖 `/dev/null` create/delete header，delete-to-null
必须移除完整内容，create-from-null 不得覆盖已有文件，checked create 用 no-clobber publish。
当前模型可见描述明确逐文件原子发布与跨文件 crash 非事务；root headless、root interactive
和 isolated Writer 的当前 catalog hash 已随描述重冻，历史 manifest 不重写。

freshness 的当前边界是 exact-byte check 后 atomic rename；外部不守约 writer 仍可能竞争
check/rename，删除同理存在 check/remove 窗口。Started 的 `MayWrite` 即使 outcome 未应用、
revision 未变，也会推进 workspace generation；这与 direct-tools no-op fixture 不推进任何
Runtime generation 的历史测试边界不同。

M7-D 没有增加 production owner。`scripts/eval-m7d-edit-observation.py` 只接受
`StoredRuntimeEvent.schema_version=16`，按 `operation_id` 投影 edit Prepared/Started/Outcome，
并从 `ToolOutcomeCommitted.workspace_state.revision` 取 Runtime revision。recovery 只有在同
run、同工具、同结构化 target 且经过新的 `ModelRequestPrepared` 时成立；patch header target
无法由 structured arguments 确认时保持 unscorable。committed indeterminate side effect 与
Started 无 Outcome 分开记录，normal run 不估计 crash frequency。

该 evaluator 不导入历史 M7-A executor，不调用工具或编辑器，也没有 transport、Key、计费、
release binary 或 result replace 路径。首个 clean suite 的 workspace-test exit 101 被保留；
诊断复跑通过。final `cf9b3fd6` v2 suite 修正 aggregate status 和有界失败 tail 后 14/14 gates
通过。当前事实只支持 keep offline observer、删除 no-delta live Harness 和 hold editor/FIM
treatment；不支持任何模型能力或效率收益。

M7-E 没有增加 production owner 或默认分支。`crates/deepseek` 继续唯一拥有 Standard Chat
的 reasoning planner、wire parser 和 usage/accounting；`AgentRuntime` 与 `RunStore` 继续
拥有 `StartRunCommand.reasoning_effort`、exact `ModelRequest`、`RequestPlan`、
RuntimeEvent、receipt 和 terminal。`high` 与 `off` 使用同一模型、工具目录、预算、streaming、
completion/verifier 与 binary；区别只来自现有 reasoning treatment 及其合法诱发的
`reasoning_content` replay。

production composition test 已证明 high/off 的 exact RequestPlan 可在 SQLite reopen 后
重建，并证明同一 stable workspace 中首请求只需归一化唯一 Host-owned
`task_generation` 行即可匹配。评测同时确认 workspace revision 会绑定 canonical
workspace/repository absolute path；不同随机 workspace 是实际 prompt 差异，不是可以从
wire identity 中删除的噪音。

final v5 Harness 让每个 pair 复用 suite-owned fixed workspace slot，同时隔离 State、
RunStore、run ID 与 binary copy，并在 pair 闭合时立即核对 messages、actor、tools、surface、
预算、revision、binary、fixture 和 schedule。它不拥有第二套 planner、Runtime、Store 或
accounting。由于 v4 被外部停止时 active arm 可能已有无法重建最终 billing 的 in-flight
request，v5 的 `live_api_admitted=false` 在 preflight、output reservation、Key read 和 API
之前 fail closed。该历史 M7-E 切片当时没有改变默认路由；M9-D 后续已删除
model/reasoning Auto，当前 fixed actor route 由 ADR-0008 约束。

M7-F 没有新增 production owner。`SystemPrompt` 继续保存 stable constitution 与 ordered
volatile world-state blocks；`runtime_system_instructions` 将它们用固定 separator 拼成一个
DeepSeek system message，`cache_control` 不进入 wire。ContextBroker 随后投影 canonical
transcript，并把 task generation、workspace generation/revision、receipt、completion
rejection 与 verifier failure 作为最后一个 Host user message。该 tail 没有 transcript
index，因此下一请求不会在 assistant/tool output 前重放它。

production loopback 冻结的三轮 messages 为 `2/4/6`，相邻公共完整 messages 为 `1/3`；
只读后 Host facts 相同也仍是 break，写入后 revision 则正确刷新。raw HTTP body 等于从
`ModelRequestPrepared` 重建的 `RequestPlan`，SQLite reopen 后事件与 snapshot 精确。
当前工具目录顺序已稳定；root/read-only child/Writer 的 system/catalog 差异均来自角色与
权限。

把旧 Host facts 写回下一轮可形成真实 wire delta，但会把 superseded revision/receipt
重新加入模型输入，还需要新的 typed supersession、compaction、child fork、reducer 与
crash/reopen 合同。本阶段没有该状态，也没有多 system message、fact deletion/reorder 或
tool duplication 路径。完整事实见
[M7-F canonical context-cache 前缀审计结论](../../eval/summaries/m7-f-context-cache-prefix-2026-07-23.md)。

M7-G 同样没有新增 production owner。`AgentRuntime` 对同一 response 的多个 `agent` calls
先逐一持久化 prepared/started 并启动 child，再统一 `join_children`；production
loopback 已在第一个 child 保持未响应时观察到第二个 child 到达 DeepSeek transport。child
仍由 `ProductionAgentOrchestrator` 构造同一 Runtime，handoff、usage、cost 和 terminal
仍进入同一 RunStore。

`crates/runtime::store` 现在允许两类精确的 `RecoveryRequired` terminal 关闭 unfinished
read-only child lifecycle：指向 durable `ChildStarted` 对应 in-flight `agent` operation 的
ToolExecution ambiguity，或指向 exact unfinished child ID 的 ChildRun ambiguity。普通
failure 仍拒绝。进程级 SIGKILL 证明 SQLite reopen 后 typed ambiguity 逐字段保留且不重发
模型请求、不重启 child；同批 partial failure/cancel 也会 settle 每个 child 并保留已完成
handoff。

正式 fan-out Harness 只通过 Run API v10/app-server stdio 投影 production facts，不实现
工具或 Agent loop。candidate `062623e6` 的 frozen binary/fixture/schedule/dry-run 通过后
才读取 Key；首个 control arm 联网后，旧 Harness 把 caller-authored verifier plan 与 Host
resolver 的 canonical env/timeout 错误比较，以 `run_identity_invalid` 停止。旧 abort 没有
保存已读取 accounting，最终 billing 未知；`completed_arms=0`、`maximum_reruns=0`，没有
产品指标和续跑。post-decision `8763722c` 只修正离线 observer 与 failure evidence，不改变
production 或重跑 formal。完整事实见
[M7-G canonical read-only fan-out 审计结论](../../eval/summaries/m7-g-readonly-fanout-2026-07-23.md)。

M7-G2 仍没有新增 production owner。离线 journal 现在冻结
`terminal_snapshot -> sqlite_reopen_snapshot -> verifier_snapshot -> arm_result|abort`
顺序；每条记录是 `0600`、`O_EXCL|O_APPEND|O_NOFOLLOW`、sequence/previous-hash chain、
逐记录 file `fsync`，首次 reservation 另做 directory `fsync`。11 个独立子进程覆盖
identity/verifier/surface/accounting exception、terminal 写前/半写/写后未 fsync，以及
三个 snapshot checkpoint 后和 result 前 SIGKILL；所有 fault 都未先提交 derived result。
半写 tail 只能审计，既有 output 不能重开续写。

production exactness 继续由现有 Rust owner 验证：fan-out loopback 在 SQLite reopen 后逐字段
匹配 root/child replay、usage、request、cost 和 terminal；read-only child SIGKILL 不重发，
typed recovery 只接受 exact unfinished lifecycle ambiguity。旧 M7-G raw 仍是 unknown
billing 且不可续跑。`062623e6` 后没有新的 fan-out production delta，因此 M7-G2 没有
credential read、API request 或 paid binary，决策为
`live_successor_inadmissible_no_new_production_delta`。完整事实见
[M7-G2 observer durability 与 successor 准入结论](../../eval/summaries/m7-g2-observer-durability-2026-07-23.md)。

M7-H 没有新增 production owner、事件或持久状态。`AgentRuntime` 仍为 root、
read-only child 和 explicit Writer 各保留一次可归还的 terminal model permit；只有
`ModelRequestPrepared` 落盘后它才成为 logical request，DeepSeek inference lease 开始后才
成为 physical attempt。离线矩阵中所有可观测分组的 logical/physical actor request 数一致，
没有发现第二模型循环或隐藏重试。

当前 `run_until_terminal` 先根据实际 permit 选择 ordinary catalog 或 reserved terminal
`tools=[]`，然后用该 exact catalog 构造 `effective_context`、判断 hard limit 并在必要时提交
唯一 `ContextCompactionCommitted`。旧实现曾在选择 terminal permit 之前用 full ordinary
catalog 做这一步，导致 near-limit no-tools 请求可能在 0 次请求前被错误拒绝。
`eb8763a1` 删除了该 pre-model-turn 分支与 `ContextCompactionControl`，没有改变
`ModelRequest`、RuntimeEvent v16、State v21、Run API v10 或 exec-stream v2。

失败契约同时覆盖 synthetic huge catalog 与 fixed `ProductionToolExecutor` catalog。
baseline/candidate 独立离线 binary 在同一 1-request budget 下分别 fail/pass；prepared
terminal request 的 catalog 为空、没有虚假 compaction，SQLite replay 仍由既有
`ModelRequestPrepared` 和 compaction reducer 合同重建。historical raw 中没有 compaction/
hard-budget 样本，旧 schema 也没有 per-request terminal catalog，因此当前只能保留该
deterministic correctness 修复，不能宣称广泛 request/Token 效率收益。

M7-H 没有读取 credential、调用官方 API 或改变默认 reasoning。DeepSeek thinking tool-call
历史要求完整回传对应 `reasoning_content`；现有 reasoning replay 不能因绝对 Token 较大就
删除。产品决策为 `keep_fix / hold_broader_optimization /
live_inadmissible_no_model_treatment`。完整矩阵与证据边界见
[M7-H canonical request/Token 浪费矩阵与 terminal catalog 结论](../../eval/summaries/m7-h-request-token-waste-2026-07-23.md)。

M7-I 同样没有新增 production owner、事件、schema、预算或模型策略。current 请求事实链仍是：

```text
AgentRuntime permit + actor catalog
  -> ContextBroker effective_context / estimate / optional compaction
  -> RuntimeEvent v16 ModelRequestPrepared
  -> State v21 reducer / SQLite reopen
  -> DeepSeek RequestPlan
```

`ModelRequestPrepared` 已包含完整 canonical `ModelRequest`；`ContextCompactionCommitted`
包含 source digest、before/after estimate、projection、usage 与 matching tools。StateStore
只重放这些既有事件，DeepSeek planner 只从 reopened `ModelRequest` 派生 endpoint/body/
response mode。M7-I Harness 不保存第二份 terminal permit、estimated-token 或 planner
decision。

新增 app regression 在真实临时 Git workspace 中让 root、read-only child 和 explicit
Writer 分别准备请求，写入 StateStore 后关闭并 reopen，再逐 actor 比较 request、catalog、
effective estimate 和 DeepSeek RequestPlan。角色权限保持来自同一 production catalog：
root/Writer 可见其 admitted 写能力，read-only child 不可见写工具。新增 Runtime regression
证明 mandatory facts 自身超过 hard limit 时直接提交 typed terminal；不会先压缩、准备请求
或调用 ModelPort。

其余 ordinary/terminal compaction、logical/physical budget、partial failure、cancel、
SIGKILL/reopen 与 production Git/verifier loopback 由既有 canonical tests 共同覆盖。
13 项 manifest 对 16 个 exact filters 的 clean formal run 全部通过；本切片只补 coverage，
没有 material production delta、credential read、API request 或 network access。决策为
`close_request_token_optimization_and_admit_m8_cleanup`。完整事实见
[M7-I near-limit context/request budget 结论](../../eval/summaries/m7-i-near-limit-context-2026-07-23.md)。

M8-A 没有改变上述 canonical execution 或 protocol owner。当前三条入口的 model setup 为：

```text
CLI / interactive TUI / app-server
  -> crates/config root api_key + base_url + default_text_model
  -> crates/deepseek capability / planner / transport / accounting
  -> AgentApplication -> AgentRuntime -> RunStore
```

credential precedence 是 explicit CLI、root config、CodeWhale keyring、`DEEPSEEK_API_KEY`；
Provider 不再是可选配置。远程 endpoint 只接受官方 DeepSeek，显式 loopback 只用于离线
production-path fixture；app-server 始终要求官方 HTTPS。named profile、project config 和
Fleet profile 都不能设置 Provider 或替换 credential/endpoint/model owner。

CLI 的 `ProviderArg`、generic login/logout/model registry 与整个 `crates/agent` 已删除。
TUI 的 `ApiProvider`、provider-specific OAuth、model catalog/provider lake、route runtime/
billing/scorecard 和第二份 capability/pricing route 已删除。`crates/config` 的
`ProviderKind`、Providers tables、catalog、pricing、models.dev、model reference、fallback
与整棵 generic route resolver 已删除。保留的 `crates/tui::pricing` 只展示 canonical
DeepSeek usage/cost，不选择模型或路由。M44 已删除没有 canonical executor 的 `[search]`
retrieval adapter 配置及 Doctor projection；MCP OAuth 只认证 MCP transport，不认证模型后端。

旧 `provider = "deepseek"`、`[providers.deepseek]` 与 camel-case model keys 也 fail closed；
没有兼容 reader、dual write 或 Provider 模式。当前 `config.example.toml` 同时通过
`crates/config::ConfigToml` 和交互 TUI loader。M8-A 从 `8b650356` 到 `00dcda0c` 净删除
39,023 行，Run API/RuntimeEvent/State/exec-stream 未变。完整事实见
[M8-A DeepSeek-only production 配置与入口结论](../../eval/summaries/m8-a-deepseek-only-entry-2026-07-23.md)。

M8-B 没有改变上述 model setup 或 canonical execution。当前 product/delivery identity 为：

```text
clean CodeWhale source + Cargo.lock + Rust 1.97.0
  -> scripts/codewhale-delivery.sh package --locked --offline
  -> codewhale.delivery.v1 manifest + inner/outer SHA-256
  -> immutable prefix/lib/codewhale/releases/<identity>
  -> atomic current/previous activation
  -> prefix/bin/{codewhale,codewhale-tui}
```

workspace repository metadata 指向 owner origin
`https://github.com/gelin66/deepseek-agent`；upstream 只保留 read-only source history，
不是 package/release source。embedded build metadata 只接受 `CODEWHALE_BUILD_*`，TUI
sibling override 只接受 `CODEWHALE_TUI_BIN`，DeepSeek transport User-Agent 为
`CodeWhale/<version>`。真正的 provider protocol env（如 `DEEPSEEK_API_KEY`）保持不变。

产品配置、状态、settings 与 secrets 只使用 `CODEWHALE_HOME`、
`CODEWHALE_CONFIG_PATH` 和 `.codewhale`；`.deepseek` 不被读取、迁移、回写或卸载。
OS keychain service 为 `codewhale`。CLI `metrics` 第二状态真相、旧 `codew` binary、
重复 example、旧开发脚本、TUI updater/version check、`crates/release` 和 CNB/imported
GitHub discovery 已物理删除。Doctor 是确定性的本地诊断，不查询 release metadata。

delivery artifact 精确绑定 product/version/target/full source revision/tree、Cargo.lock
SHA、rustc identity、source mode 和两项 binary set；archive 与内部文件都验证 SHA-256。
install/upgrade 不覆盖 immutable release，current/previous 通过平台原子 symlink 切换；
rollback 不重建，uninstall 只删除程序和 delivery metadata。macOS real release lifecycle
和 Linux fixture lifecycle 都在网络被禁止时通过。代码 candidate 相对 `2ed3efe3` 净删除
1,702 行，Run API v10、RuntimeEvent v16、State v21、exec-stream v2 未变。完整事实见
[M8-B 产品身份与本地交付结论](../../eval/summaries/m8-b-product-delivery-2026-07-23.md)。

M8-C 没有改变 model setup、canonical execution 或 delivery owner。产品文本链现在是：

```text
crates/localization/locales/zh-Hans.json
  -> crates/localization::{MessageId, tr, tr_args}
  -> crates/cli + crates/tui
  -> codewhale + codewhale-tui
```

这是固定语言 compile-time owner，不读取 `LANG/LC`，没有 locale 配置、语言切换、第二
catalog 或 translation model call。真实 Clap parser、Doctor、Headless errors、
`request_user_input`、approval、canonical status 与多 Agent chrome 使用同一 message
truth；模型提供的问题/选项、provider/tool 输出、stdout/stderr、路径、代码和 diff 原样
投影。Doctor JSON、exec stream-json、HTTP/SSE/stdio 字段/enum 和 stored event JSON 不做
本地化。

旧 `crates/tui/src/localization.rs` 和 `crates/tui/locales/zh-Hans.json` 已物理删除。
shared catalog 由 baseline 167 keys 扩展为 427 keys；静态 gate 要求 catalog/MessageId
exact parity、恰好一个 catalog/rust-i18n owner，并拒绝 locale detection/switching 回流。
80/120 列的 Clap help 与 `request_user_input` 以 Unicode display width 验证；真实中文 PTY
继续覆盖 composer/cursor/hit target。candidate `44b17940` 的 installed 两项 binary 在
`LANG=C/LC_ALL=C` 下通过 help、Doctor text/JSON、missing Key、retired command 和
uninstall，manifest 精确绑定 tree `f4444098`。

M8-C 相对 `e99bf6c7` 为 31 files、`+2,741/-963`，净增加 1,778 行；正增量来自一个共享
427-key interface contract 与真实 caller/回归测试，不作为 Agent 能力指标。
`crates/protocol`、`crates/runtime`、`crates/state`、`crates/app-server` 相对 baseline
零差异，Run API v10、RuntimeEvent v16、State v21、exec-stream v2 未变。完整事实见
[M8-C 固定 zh-Hans 产品界面结论](../../eval/summaries/m8-c-fixed-zh-hans-interface-2026-07-23.md)。

M8-D 没有改变 bundled prompt、model routing、Runtime、RunStore、tool catalog、权限、
请求预算或协议。保留的 production 行为变化只有：

```text
TUI / exec / app-server
  -> codewhale_context::prompts::load_prompt_overrides_from_config_home
  -> ProductionComposition
  -> AgentApplication -> AgentRuntime -> RunStore
```

候选 constitution 只存在于 `eval/fixtures/m8-d-prompt/v1`，通过既有 override surface 注入
immutable binary；它不是默认配置、用户模式或第二 prompt truth。外部 app-server
process test 在禁网下提交 Start，随后从 State reopen 的 `RunCreated` request 验证 prompt。
两臂其余 system blocks、cache controls、model、tool/permission/budget/accounting identity
保持相同。

官方 2026-07-24 文档仍定义 base URL `https://api.deepseek.com` 和 Standard Chat
`POST /chat/completions`，当前模型为 `deepseek-v4-flash`/`deepseek-v4-pro`。当天退役的是
`deepseek-chat` 与 `deepseek-reasoner` 旧模型别名，不是 ChatCompletions interface。
FIM 仍是独立 Beta `/beta/completions`，M8-D 没有 FIM caller 或请求。

final v5 首 arm 在一个无 usage 的失败 attempt 后由 canonical accounting 记录
`billing_unknown=true`、`complete=false`、`surface_usage=[]`，Harness 立即停止。
因此 current architecture 只包含 caller consistency fix，不包含新的默认 prompt 能力。
完整身份、raw 和非结论见
[M8-D 中文原生 Agent prompt A/B 结论](../../eval/summaries/m8-d-native-zh-prompt-ab-2026-07-24.md)。

M8-E 没有改变 production architecture。它只增加 frozen audit contract、offline verifier
和结论文档。当前 DeepSeek wire 事实是：

```text
AgentApplication -> AgentRuntime -> DeepSeekModelPort
  -> RequestPlan(Standard Chat / lossless Strict fallback)
  -> POST /chat/completions
  -> Chat response/SSE parser
  -> canonical usage/retry/accounting
  -> RunStore
```

官方 Anthropic Messages base URL `https://api.deepseek.com/anthropic` 不出现在
production planner 或 transport，这是有意的产品边界，不是缺口。CodeWhale 不兼容
Claude Code/Anthropic 生态，也不需要 Messages request DTO、content-block/SSE parser
或第二 transport。唯一 `DeepSeekModelPort` 继续使用官方 DeepSeek ChatCompletions；
`deepseek-v4-pro`/`deepseek-v4-flash`、Standard/Strict planner、reasoning/tool replay、
usage/retry/accounting 与 RunStore 链均保留。

M8-E 当时冻结的其他 architecture blockers 是以下历史快照；M8-G/H/J 的 successor
结论会在后文逐项 supersede，不反向改写该 frozen evidence：

- `ProductionAgentOrchestrator` 与 Fleet/Lane 的用户可见命令、协议和状态概念并存；
- explicit single Writer 有完整 worktree/verify/integrate/cleanup，但 multi-Writer 未准入；
- FIM 只有 `crates/deepseek` planner/accounting 基础，没有 canonical response/apply caller；
- RepoGraph 不存在；
- 没有相对 imported `352e86a6` 的真实 coding 与 workflow-step 完成证据；
- M8-D bundled prompt 未切换。

clean `a12bea45` artifact 绑定 tree `966afe2f`、Cargo.lock `ff53b498…f0ef2` 和
Rust 1.97.0，只包含 `codewhale`、`codewhale-tui`；两者在真实临时 prefix 安装验证后
报告相同 revision。focused、workspace clippy/test、CLI/TUI/API parity、
root/read-only/Writer、SIGKILL/reopen 与 delivery gates 通过。这证明上图现有架构可复现，
不消除上述 blocker。完整矩阵见
[M8-E V1 退出证据总审计](../../eval/summaries/m8-e-v1-exit-audit-2026-07-24.md)。

M8-G 已 supersede 上述 M8-E 的 Fleet/Lane 快照。当前唯一 TaskGraph 产品事实为：

```text
AgentApplication
  -> AgentRuntime(root/read-only child/explicit Writer)
  -> RuntimeEvent v16
  -> RunStore(State v21)

explicit Writer Git side effects only
  -> ProductionAgentOrchestrator
  -> worktree/diff/verify/integrate/root verify/cleanup
```

`AgentTask`、`AgentOutcome`、`parent_run_id`、shared budget 和 terminal lifecycle 是
canonical graph facts；没有新的 TaskGraph DTO、crate、scheduler 或 Store。
Fleet protocol/config/ledger/lease/scheduler/SSH/alerts/worker/UI、bundled skill，以及
Lane registry/tmux/inline/process shell 均已物理删除。CLI 不再提供 `fleet`/`lane`，
TUI 不再提供 `/fleet`；旧 spellings 在 config、TUI、Store 或 model 前 fail closed。

setup-state schema 为 v2，已删除 `OperateFleet` 和 receipt flag；旧 v1 Fleet-bearing
record 不经兼容 reader 重开。Doctor JSON 的 `task_graph` 只报告
`AgentRuntime`、`RunStore`、`ProductionAgentOrchestrator` 与 root/read-only/explicit
Writer actor，明确 `remote_fleet=false`、`multi_writer=false`。

code candidate `64f6bc16` 相对 `650df581` 净删除 15,110 行，visible CLI command
`20 -> 18`、setup step `7 -> 6`。Run API v10、RuntimeEvent v16、State v21 和
exec-stream v2 未改变；focused、workspace Clippy/test、CLI/TUI/API parity、
root/read-only/Writer、SQLite reopen 与 SIGKILL recovery 通过。V06 当前为 pass；FIM、
RepoGraph、multi-Writer、imported-baseline coding/workflow-step A/B 和 prompt
unknown-billing successor 仍未完成。完整事实见
[M8-G 单一 TaskGraph 产品概念收敛](../../eval/summaries/m8-g-taskgraph-convergence-2026-07-24.md)。

M8-H 已 supersede 上述 M8-E/M7-C 的“保留 FIM planner/accounting 基础”快照。当前
production 只有官方 DeepSeek Standard Chat 与 lossless Beta Strict fallback：

```text
canonical model ids
  -> deepseek-v4-pro | deepseek-v4-flash
  -> DeepSeekModelPort
  -> https://api.deepseek.com/chat/completions
  -> Chat parser/replay/accounting
```

旧 `plan_fim` 产生的 `/beta/completions` URL 不属于 sender endpoint owner，唯一完整
response parser 只接受 Chat `choices[].message`，且 Runtime/RunStore 没有 FIM
revision-bound Host apply/reopen consumer。candidate `7d9aa9a6` 因而删除 FIM
planner/error/surface/accounting、公共 terminal 的 always-zero `fim_response_count`、
eval-only `fim_edit`/`write_file` classifier 和重复模型 alias 兼容层。exec-stream
`v2 -> v3` 只反映公共 terminal 字段删除；RuntimeEvent v16、State v21 不变。

这项收敛的结论是 **keep Standard/Strict / reject unreachable FIM production
half-branch / hold FIM re-entry**，不是证明 FIM 质量较差。M7-C deterministic matrix
仍为 12/12，本阶段没有新 current-v16 production 编辑失败、没有同 binary treatment，
所以 Key 未读取、官方 API 请求 0。完整事实见
[M8-H FIM 产品范围与历史债收敛](../../eval/summaries/m8-h-fim-scope-debt-2026-07-24.md)。

M8-H 的 imported-baseline fallback 也只完成离线准入。exact `352e86a6` binary 能使用
相同 `/chat/completions` 与 `deepseek-v4-pro`，但旧 terminal schema 没有 request count、
cost completeness/bucket 或 canonical root/child reopen ledger，retry count 也未知。
因此 live coding/workflow-step A/B 为
`inadmissible_incomplete_baseline_accounting`；旧 revision 不修改，Key 未读取。

M8-I 当时把 pre-run Flash classifier 收敛为 Host typed policy，并删除了额外
non-streaming Flash 请求、classifier prompt/parser、provider DTO、关键词/500 字
heuristic、空 `recent_context`、`DeepSeekAutoRouteFailed` 和 creation unknown-billing
projection。其 frozen manifest/summary 继续记录当时事实，但 Auto 产品结论已由
ADR-0008/M9-D supersede。

M9-D 当前 production route 为：

```text
Start/Continue typed facts
  -> AgentApplication::ProductionFixedRoutePolicy
  -> RunRequest(actual model, actual reasoning, neutral ModelRouteAudit)
  -> AgentRuntime
       -> explicit child(inherit exact model/reasoning)
       -> read-only AgentTask(Flash/High)
       -> failed read-only recheck AgentTask(Pro/Max)
       -> explicit Writer AgentTask(Pro/High or typed rework Max)
  -> DeepSeekModelPort -> ChatCompletions
  -> RunStore(State v24)
```

未显式指定模型的 root 固定 Pro/high；显式 `deepseek-v4-pro`、
`deepseek-v4-flash` 和明确 reasoning 由 child 精确继承。fixed-profile read-only child
的 Flash handoff 必须返回 Pro root 汇聚并由 Host evidence gate 验收；失败后新建
Pro/max recheck，不在原 Run 中切模型。Writer 使用 Pro/high，typed recovery/rework 使用
Pro/max。RuntimeEvent v18/State v24 不包含 Auto caller intent，只保存 actual
model/reasoning、actor、中性 route profile/policy/reason。SQLite 重开只验证该选择，不再
路由。

config、CLI、TUI、API、帮助和 current evaluator 不再接受或显示 model/reasoning Auto。
不存在 classifier、额外路由请求、关键词分支、运行中 dynamic router 或兼容 reader。
State v24 直接退休无法无损映射 exact omitted-reasoning wire 的 v23 materialized run，
只保留 v18-safe pending Start。

M8-J 当前 state / CLI surface 为：

```text
codewhale runs | resume | exec --resume/--continue
  -> AgentApplication
  -> canonical RunStore(State v24)

legacy codewhale thread <8 subcommands>
  X no dispatch / no metadata Store / fail closed before model
```

旧 `codewhale thread` 读写独立 `threads` metadata 表，并把 resume/fork 委托给已退休
TUI thread 语义；`StateStore` 还维护 `session_index.jsonl` sidecar。protocol 根部的
Thread/App/Prompt/EventFrame DTO 只有自身 parity test，没有 production caller。candidate
`bcbc1616` 先保留并验证 canonical `runs`/`resume` caller，再删除以上旧入口、CRUD、
sidecar、DTO、文案和专属测试。

State `v22 -> v23` 的 migration 在同一 `IMMEDIATE` transaction 中
`DROP TABLE IF EXISTS threads`，fresh v23 不创建旧表。v22 debt fixture 证明升级后
current canonical run、pending Start、route audit 与 accounting 原样保留；注入 drop
failure 时 user_version 和旧对象一起回滚。`RunEnvironment.provider="deepseek"` 仍用于
environment fingerprint / replay safety，execpolicy network types 也继续服务真实 caller；
它们不是 generic Provider 产品模式。

该切片把 V12 从 blocked 关闭为 pass，current V1 matrix 为 10 pass / 6 blocked，剩余
V08/V09/V10/V13/V15/V16。它只提供状态单一真相、删除量和恢复正确性证据，没有模型
treatment，不能推导 verified success、Token、时间或费用提升；Key 未读取、官方请求 0。
完整事实见
[M8-J V1 successor 与 V12 历史债删除](../../eval/summaries/m8-j-v1-successor-v12-debt-2026-07-24.md)。

M8-K 没有添加 production 功能。frozen manifest `b44d7ff9` 要求候选必须同时具备
current attributable failure、canonical caller、deterministic verifier、同 revision
immutable treatment、完整 accounting、单一 owner 和 cutover 删除。V08/V09/V10 的
current production audit 都不满足这组条件：

| Item | 当前能力 | 可归因新失败 / treatment | V1 scope 决策 |
|---|---|---|---|
| V08 | 每 root 一个 explicit isolated Writer，完整 Host lifecycle | 0 / 无 | keep single Writer；multi-Writer evidence-gated |
| V09 | official Standard Chat；Strict whole-catalog admission/lossless fallback | 0 / 无 FIM caller | keep Chat；FIM evidence-gated |
| V10 | canonical search/read/diff、ContextBroker、deterministic verifier | 0 / 无 RepoGraph caller | keep bounded cross-file outcome；RepoGraph evidence-gated |

ADR-0005 只删除了 PRODUCT_PLAN 中没有证据支撑的 implementation-name literals，没有
宣布 multi-Writer、FIM 或 RepoGraph 已实现或质量较差，也没有改变 ADR-0003、唯一
DeepSeek ChatCompletions backend、AgentRuntime、RunStore 或工具目录。V08/V09/V10
按现有可验证能力关闭后 current V1 matrix 为 13 pass / 3 blocked，剩余 V13/V15/V16，
V1 仍不可发布。完整事实见
[M8-K V1 产品范围 successor](../../eval/summaries/m8-k-v1-scope-successor-2026-07-24.md)。

M8-L 没有修改 production source。imported/current audit 证明同一 task、official
Chat/model、external verifier 与 wall-time 可以冻结，但旧 terminal 的
`retry_count=null`、物理请求/failed usage/cost/reopen 缺失使 paid A/B 不能生成完整
measurement。Harness 不补猜旧账，结果为
`inadmissible_incomplete_baseline_accounting`，credential/API 均未使用。

ADR-0006 接受如下 release evidence chain：

```text
M5-A qualified official DeepSeek evidence
  + exact current AgentApplication/Runtime/Store production regression
  + imported/current common user-action comparison
  = V13/V16 release benchmark successor
```

M5-A 12/12 arms 只覆盖一个 coding task 和一个 false-claim 反例：coding 两侧均 3/3
verified，candidate false-success 0；false-claim candidate 3/3 correct rejection，
accounting 完整。current candidate 另行证明 Git edit、verifier fail→recovery、latest
receipt、false completion rejection、root/read-only/Writer RequestPlan/accounting reopen
与 resume no-request。login/interactive/headless/inspect/resume 的显式用户动作是
`5 -> 5`。两层证据不能推导 current 相对 imported 的 success/Token/time/cost delta，也
不能用祖先结果免除未来 model-visible treatment A/B。

V13/V16 关闭后 current matrix 为 15 pass / 1 blocked；只剩 V15 billing-provable
Simplified Chinese Agent prompt comparison，V1 仍不可发布。完整事实见
[M8-L release benchmark successor](../../eval/summaries/m8-l-release-benchmark-successor-2026-07-24.md)。

M8-M 没有修改 production source 或 bundled constitution。它只用已有显式 prompt
override，把同一 immutable fixed-Pro binary 的 current prompt 与唯一 fact-gap candidate
送入 canonical production chain：

```text
isolated CODEWHALE_HOME / constitution variant
  -> ProductionComposition
  -> AgentApplication -> AgentRuntime
  -> DeepSeekModelPort -> POST /chat/completions
  -> canonical usage/retry/accounting
  -> RunStore -> no-credential SQLite reopen
  -> external deterministic verifier
```

preflight 证明两臂除 stable constitution bytes 外的 actor、authority、TaskContract、
catalog、reasoning、budget、cache controls、remaining prompt blocks 和 binary identity
相同。正式 suite 在第 6 arm 的第 1 次 request 遇到无 response headers/usage 的 typed
transport failure；RunStore sealed/reopen 后仍保留 `started=1`、
`surface_responses=0`、`billing_unknown=true`、`complete=false`。Harness 没有把它重试、
估价或当作 0 cost，而是以 `aborted_unknown_billing` 停止。

因此 current architecture 仍只有 `crates/context` 一个 production prompt owner；没有
candidate selector、模式、fallback、第二 model loop、Provider 或 transport。M8-M
candidate-only Python runner/test 已删除，frozen manifest、summary 和 ignored `0600`
raw 只作为停止证据；该 candidate 仍为 hold。完整事实见
[M8-M billing-provable 中文 Agent prompt successor](../../eval/summaries/m8-m-billing-provable-zh-prompt-successor-2026-07-24.md)。

M8-N 没有再造 prompt candidate。它冻结 current constitution SHA-256
`39f2eeb30519e143eed2d4c627fcb97d323c6b95ad9060816a93b72ea994d409`，证明该
constitution 与 M5-A 12/12 合格 official DeepSeek arms、M6-A Writer canary、M8-M
immutable binary 及 current source byte-identical，并以 exact-current gates 和现有
delivery owner 补齐 production retention、版本和 whole-release rollback：

```text
fixed-Chinese immutable prompt identity
  + qualified official DeepSeek coding/false-success evidence
  + exact-current root/read-only/Writer/verifier/reopen conformance
  + immutable release identity and rollback
  = V15 release-scope successor
```

ADR-0007 只关闭 fixed baseline 的 release evidence，不把 rejected/hold treatment
重写为收益，也不允许未来候选绕过 current same-revision 同任务 A/B。M8-N 没有
model-visible treatment，live 准入为 `inadmissible_no_material_treatment`，credential
和 official API 均未使用。M8-D candidate-only app test/current-tree fixture 已删除；
app-server/exec/TUI 共用 override loader 和进程级一致性测试保留。current V1 matrix 为
**16 pass / 0 blocked**，发布状态为 **V1 可发布**（只表示 release-ready）。完整事实见
[M8-N V15 release-scope successor](../../eval/summaries/m8-n-v15-release-scope-successor-2026-07-24.md)。

M9-A 没有增加 production route。它在当时确认三个调用面由同一 Host policy 解析，
显式 Pro/Flash 由 child 精确继承，typed read-only/Writer route 使用已冻结的 model。
`AgentTask`、child `RunRequest`、route audit、request ledger 与 accounting 继续由现有
protocol/runtime/state 精确持久化和重开。

正式 Auto admission 使用 candidate `29c4980f` 的唯一 official ChatCompletions backend。
第 18 arm 在无 headers/usage 的 transport failure 后由 canonical ledger 标为
`billing_unknown=true` 并停止；17 个完整 arms 不能构成 27-arm product metric。该
历史 campaign 当时结论为 hold，现已由 ADR-0008 的范围退休 supersede。实验专属 runner
已删除；没有 classifier、
Provider、production variant、第二 Runtime/Store 或第二 transport 留在当前源码。完整
事实见
[M9-A Host Auto release admission](../../eval/summaries/m9-a-host-auto-release-admission-2026-07-24.md)。

M9-B 没有增加 production surface。它以 candidate `983fa9ce` 的同一
`codewhale app-server --stdio`、Run API v11、RuntimeEvent v17、State v23、一个
AgentApplication/AgentRuntime/RunStore 与 official DeepSeek Standard Chat backend
执行六类 fixed-Pro regression task。root、read-only child 与 explicit Writer 的
model 都是 `deepseek-v4-pro`、reasoning `high`；route audit、shared request ledger、
usage/cost、Host receipt 与 SQLite reopen 继续来自 canonical Store。

正式 v1 在第 2 个完整 arm 暴露 evaluator-only observer defect：read-only child 的
`agent_result_collected` 是 root/read-only/Writer 共用事件，却被 Harness 当成
Writer-only lifecycle。Runtime 与 Store facts 本身正确；修正后对同一 durable snapshot
重放得到 child completed、零 child writes、handoff 后 root mutation 与 lane valid。
campaign 按 observer-failure gate 在第 3 arm 中止，没有 rerun/splice。当时生产默认
fixed `deepseek-v4-pro`；没有 classifier、Provider、FIM、Anthropic、
第二 Runtime/Store 或第二工具目录。原 admission 绑定 pre-fix Harness hash，corrected
runner 会 fail closed，不会复用旧 campaign。完整事实见
[M9-B fixed-Pro coding regression baseline](../../eval/summaries/m9-b-fixed-pro-regression-baseline-2026-07-24.md)。

M9-C 继续使用同一个 corrected current Harness，没有复制第二个 evaluator 或修改
production。successor 从新的 manifest/admission/raw position 1 开始，只按 hash 解析
M9-B 的 task/tool contract 并使用新的 acceptance ID。candidate `7a91bbaa` 的 binary
仍是唯一 `codewhale app-server --stdio`、Run API v11、RuntimeEvent v17、State v23、
AgentApplication/AgentRuntime/RunStore 与 official DeepSeek Standard Chat production
composition；root、read-only child 与 Writer 全部显式 fixed Pro/high。

正式 campaign 第一轮六类 task 各完成一次，第二轮再完成 read-only 与 recovery；8 个
arm result 的 route/lane/reopen/accounting 均有效、false success 0。第二轮 Writer 在
child 创建前的第一个 root request 遇到无 response/usage 的
`deepseek_transport`。Store 没有伪造 Token 或费用，而是保存
`billing_unknown=true`、`complete=false`；Harness 在 terminal/Store/reopen/verifier
落盘后停止。没有重跑或拼接，也没有完整 18-arm baseline。默认仍 fixed Pro；旧 M6/M7
runner 因 cutover gate 未满足而保留。完整事实见
[M9-C fixed-Pro regression successor](../../eval/summaries/m9-c-fixed-pro-regression-successor-2026-07-24.md)。

M9-D 没有模型 treatment。它接受 fixed-Pro effect-first 的长期产品范围，删除
model/reasoning Auto 的用户面、protocol state、app policy 分支、TUI 投影和 current
Harness 旧字段；保留实际 model/reasoning/actor 与
`ModelRouteProfile::{Explicit, FixedActor}`。Run API v12、RuntimeEvent v18、State v24
在一个 direct cutover 中工作；pre-v18 materialized run 因 exact wire 语义不可无损映射
而退休，v18-safe pending Start 保留。默认 root/Writer 为 Pro/high，fixed-profile
read-only child 为 Flash/high，typed recovery/recheck/rework 为 Pro/max。M8-I/M9-A
frozen evidence 未改写；Key 未读取，official API 请求 0。完整事实见
[M9-D Auto retirement](../../eval/summaries/m9-d-auto-retirement-2026-07-24.md)。

M9-E 没有 production 或 Harness 代码变更。官方 Chat completion `id`/usage 只存在于
已经收到的 response；`/user/balance` 是账户聚合，Usage export 只公开月度按 Key amount
分解，官方没有 request-level reconciliation、settlement bound 或 pre-header failure
计费语义。P0 因而在 credential 前关闭为
`infeasible_exact_pre-header_request_reconciliation_under_current_official_contract`：
Key 未读取、official API 请求 0，现有 `billing_unknown -> formal campaign stop`
保持。完整事实见
[M9-E billing evidence boundary](../../eval/summaries/m9-e-billing-evidence-boundary-2026-07-24.md)。

M10 前置调用图复核确认，当前唯一控制闭环已经具备 TaskContract、ContextBroker、
AgentRuntime、canonical ToolOutcome、EvidenceReceipt/verifier artifact、RunStore 与
corrected Harness；当时预注册的五个独立候选缺口为：

- prompt 默认继续追加 project context pack；无说明文件时 fallback overview 与 pack 共用
  `build_project_context_pack`，`.codewhale/.claude rules` 仍 eager 注入；
- ContextBroker 没有 task-aware、预算化 ranked working-set regions；
- acceptance satisfaction 只在 completion gate 构造，运行中没有派生 progress view；
- app 只有部分 typed recovery/recheck 选择，没有完整 failure-directed Host controller；
- RunEnvironment/ToolArtifact 没有单独的 deterministic project environment profile
  或 TaskContract-scoped service/UI runtime artifacts；M10-E 已证明 current fixed-Pro
  轨迹没有可由它们修复的 measured loss，因此这不是已准入缺口。

skills 已经只把当前 actor 可通过 `load_skill` 读取的 exact name/description 放入 prompt，
正文由 Host 从 run-scoped immutable snapshot 按需返回；single Writer
worktree、latest-revision Stop Gate、RunStore exact replay、atomic tools 和 stable-prefix/
accounting 均已有，不能在 M10 重复建设。A→E 只有通过各自证据门才允许增强上述现有
owner；M10-A 至 M10-E 的实际结果以下列 current facts 为准。

M10-A 当前事实：

- `crates/context` 可在显式测量调用中从 canonical prompt composition 派生 fragment
  ledger；正常 production 调用仍只返回完全相同的 `SystemPrompt`，不持久化 ledger；
- no-instructions offline fixture 已冻结 fallback overview 与默认 pack 的相同 JSON
  payload；这是真实重复候选，但尚无 live task 收益结论；
- frozen candidate `14c922de` 曾让同一 binary 通过隔离 config home 显式产生
  pack-on/pack-off，并冻结 36-arm schedule、prompt marker、accounting/reopen/verifier/
  tool repetition；
- 正式 campaign 在 21 个完整 arm 后，于第 22 个 response headers/reasoning 已到达、
  finish/usage 未到达的窗口停止；canonical terminal/Store/reopen 均保存
  `deepseek_transport`、`retry_safe=false`、`usage_incomplete=true`、零 retry；
- pack-off 没有 product admission。production 固定 pack-on，`context.project_pack`
  config/CLI/TUI/API、内部 prompt bool 与 M10-A-only Harness 分支已删除；旧 key
  fail closed，不存在永久双轨；
- derived prompt ledger 与 fallback overview/pack duplicate characterization 保留，它们
  不改变正常 model-visible prompt bytes，也不持久化第二份 truth；
- root recovery frozen label 的 observer mismatch 已修正为认 canonical committed
  Host temporal receipt，而不是要求 model 重复 final verifier。frozen raw 不改写。

完整事实见
[M10-A scoped context pack](../../eval/summaries/m10-a-scoped-context-pack-2026-07-24.md)。

M10-B 当前事实：

- 已删除的 `crates/context::working_set` 候选曾在同一 immutable binary 内提供一个
  有界确定性 selector；offline localization 达到 Recall@5 1.00、median first relevant
  rank 1、负任务 0 regions，但这只是机制证据；
- formal fixed-Pro campaign 的前 5 个 arms measurement-valid：4 个 verified success，
  1 个 treatment read-only arm 通过外部文件 verifier 和 latest-revision Host receipt，
  但遗漏冻结的 `wall_time_secs=180`，actor/lane contract 无效并形成 frozen
  false-success label。绝对质量门因此失败；
- 第 6 个 treatment safety arm 在 response/usage 前发生 typed transport failure；
  canonical accounting/reopen 保存 `billing_unknown=true` 并停止，未重跑。矩阵不完整，
  不能比较 variant 的搜索、Token、时间或费用；
- 正式决定是 `reject_quality_veto_and_delete`，acquisition 事实为
  `stop_incomplete_accounting`。selector、safe-read helper、task-aware caller、
  volatile prompt fragment、`context.working_set` config/CLI/TUI/API、fixture/benchmark
  与 Harness treatment branch 均已物理删除；
- production 现在回到固定 project pack、无 Working-Set selector/map 的单路径。frozen
  manifests、ignored 0600 raw、Git 历史与 summary 只保留审计，不是 current consumer。

完整事实见
[M10-B budgeted working-set](../../eval/summaries/m10-b-budgeted-working-set-2026-07-24.md)。

M10-C 当前事实：

- production 没有 Acceptance Progress 类型、renderer、prompt marker、配置/CLI/TUI/API
  入口、plan store、Goal/Hunt 或第二完成权威；
- frozen WIP candidate 曾证明可以从 canonical TaskContract、receipt/rejection、
  verifier observation 与 workspace revision request-locally 派生 progress，并在
  Runtime/Store/compaction/reopen/root/read-only/Writer 上保持一致；
- offline viability 显示 request-visible pending `235 -> 356`、verifier rejection
  `538 -> 547`，均增加 ContextBroker estimated tokens；唯一 `492 -> 386` 的下降属于
  Host 已终止、无下一模型请求的 satisfied state；
- current fixed-Pro 已有两个 accounting-complete、verified、false success 0 的
  deterministic root recovery 样本。候选在无观察到的 failure headroom 下净增加
  1,501 行，因此在 credential 前被否决；
- cutover 后整个 `crates/` tree 与 M10-C 起点 `11528a99` 字节级一致。Runtime 与
  Store 继续从同一个原有 `ContextInput` 构造 canonical Host facts；TaskContract、
  EvidenceReceipt、latest-revision completion gate、fixed actor routes 和 exact replay
  均未变化；
- frozen offline manifest、Git 历史与 summary 只解释删除决定。Key 未读取、API 请求
  为 0，没有 live product comparison。

完整事实见
[M10-C Acceptance Progress](../../eval/summaries/m10-c-acceptance-progress-2026-07-24.md)。

M10-D 当前事实：

- production 没有独立 failure-directed app controller、第二 loop、分类请求、自动
  rollback 或 recovery store；
- 工具失败的唯一模型反馈是 `ToolOutcome.model_content()`；14 个 stable code 结合
  operation/side-effect/retry 给 root/read-only/Writer 同一中文修正动作；
- Runtime 已拥有 verifier `failure -> effective mutation -> pass`、相同 revision 拒绝
  重验、replay-safe no-output model retry、atomic retry decision、budget terminal 和
  crash/reopen exactly-once；
- app 的 fixed route policy 只把真实 completion rejection/Host verifier failure/prior
  child failure 映射为 Pro/max recheck；没有 mid-run router 或 difficulty classifier；
- current frozen trajectories 中 8 个 verifier failure 已恢复；唯一其他
  workspace-precondition arm 的产品失败是 child arguments contract，不是恢复缺口；
- `context_missing`、`reasoning_insufficient`、泛 `environment_failure` 仍不是
  canonical causal facts。M10-D 因而拒绝新增通用 controller，并把 environment
  differentiation 留给 M10-E；
- Key/API/raw 为 0，Run API v12、RuntimeEvent v18、State v24、exec-stream v3 未变化。

完整事实见
[M10-D Failure-Directed Recovery](../../eval/summaries/m10-d-failure-directed-recovery-2026-07-24.md)。

M10-E 当前事实：

- production 没有 ProjectEnvironmentProfile、第二 environment store、service manager、
  global browser mode 或 console/network/screenshot artifact tool；
- RunEnvironment 与 ProductionExecutionFingerprint 已绑定 canonical workspace、
  DeepSeek provider、fixed route、tool catalog、retry、tool execution identity 与
  sandbox controls；resume mismatch 在任何 HTTP 前失败；
- TaskContract verifier 在 RunCreated 前由 production resolver 转为 exact plan，resume
  要求同一解析结果；run_verifiers 的 mutable auto ecosystem plan 只供 advisory tool
  使用，不能签发 Host acceptance；
- verifier 在执行前后捕获 workspace revision，只对稳定 revision 产生 hash-checked
  inline ToolArtifact；Writer 用 fresh worktree tools，集成后 root 重新签收证据；
- current M9-C/M10-A/M10-B 的 37 个 canonical snapshots、280 个 ToolOutcome 中没有
  exec_shell/run_tests/environment/setup/service/UI failure；37 个 acceptance 都是单步
  exact `/usr/bin/python3` verifier；
- TaskAcceptance 当前只有 Host/Verifier，没有 service/UI runtime typed trigger。
  M10-E 因而拒绝新增 profile/runtime-artifact 分支；Key/API/raw 为 0，Run API v12、
  RuntimeEvent v18、State v24、exec-stream v3 未变化。

完整事实见
[M10-E Environment / Runtime Artifacts](../../eval/summaries/m10-e-environment-runtime-artifacts-2026-07-24.md)。

M10-F 当前事实：

- `scripts/eval-m9b-fixed-pro-regression.py --trajectory-report` 是唯一 current read-only
  loss analyzer；没有新 runner、production dependency、Store、event、prompt mutation
  或完成权；
- analyzer 验证 M9-C/M10-A/M10-B frozen 0600 journal 的 schema、sequence、hash chain、
  file hash/size 与完整 tail，从 canonical Store snapshots/Runtime events 派生聚合；
- 输出只有 strata/count/hash/status，不含 raw prompt、tool arguments/content、
  credential 或 evaluation id；连续两次输出 byte-identical；
- 37 trajectories 中 9 个 typed failure 全部以 Host evidence 恢复；两个 frozen
  false-success 被区分为已纠正 observer contradiction 与已删除 treatment lane failure；
  3 个缺失 label 是既有 accounting stops；
- 重复 observation 按 actor 分离，并在 applied mutation/revision change 后重置；只有
  prior Tool message 仍在当前 ModelRequest 才算 visible duplicate。current controls 的
  visible duplicate read/tool 均为 0；
- 当前 report 为 `insufficient_current_loss_evidence`，没有新的 production candidate。
  analyzer 保留用于后续 loss identification；Key/API/network/new raw 为 0。

完整事实见
[M10-F Trajectory Loss Analyzer](../../eval/summaries/m10-f-trajectory-loss-analyzer-2026-07-24.md)。

M10-G 当前事实：

- Host 没有自动 fan-out policy；多个 read-only child 只来自同一 DeepSeek response
  显式返回的多个 canonical `agent` calls；
- `AgentRuntime` 先启动同批 child 再 join，ordinary read-only child 固定 Flash/high，
  typed failed-child recheck 固定 Pro/max，root 固定 Pro/high；
- root/child route、request、usage、cost、terminal 在 SQLite reopen 后 exact；durable
  `ChildStarted` SIGKILL 不重发、不重启，partial failure/cancel settle 全部 child；
- M7-G formal 没有可用 arm且 billing unknown；M7-G2 只有 observer durability；
  M9-C/M10-A/M10-B/M10-F 的 7 个 current read-only trajectories 都是 single-child，
  没有 fan-out comparable pair；
- concurrency/depth limits、child lifecycle、TUI/app-server projection、M7-G/M7-G2
  reproducibility runners 都有真实 consumer；production 不存在待删除的独立 fan-out
  treatment branch；
- active fan-out admission 已按
  `close_no_admissible_readonly_fanout_benefit_evidence` 关闭。production/schema/config
  delta、Key、API、network、new raw 均为 0。

完整事实见
[M10-G read-only fan-out 最终准入审计](../../eval/summaries/m10-g-readonly-fanout-final-audit-2026-07-24.md)。

M11 当前只增加 eval acquisition 输入，不改变 production：

- `scripts/eval-m9b-fixed-pro-regression.py` 仍是唯一 corrected fixed-Pro Harness；
  默认 M9-C campaign 保持 frozen v11/v17/v23 contract，`--campaign m11` 选择 current
  v12/v18/v24 的独立 multi-language manifest；
- M11 任务覆盖 Rust、TypeScript、Python、跨文件、deterministic verifier recovery、
  CLI/service、一个 read-only child、一个显式 isolated Writer 和无工具安全假完成；
  每个 fixture 都在独立临时 Git 仓库 materialize，先证明 verifier fail 且不污染 tree；
- M11 每个 root/child RunRequest 都显式冻结 `deepseek-v4-pro/high`。这不会改变产品
  fixed actor profile：普通产品 read-only child 仍可固定 Flash/high；M11 只是用 Pro
  消除 acquisition 中的模型变量，不恢复 Auto；
- Harness 在 label 前依次保存 terminal、canonical Store、credential-free SQLite
  reopen 与 external verifier snapshot，并用 ignored 0600、exclusive、fsynced、
  hash-chained journal 覆盖 SIGKILL 窗口；unknown billing 或 incomplete accounting
  仍立即停止下一 arm，`maximum_reruns=0`；
- 正式 acquisition 完成 4 个 measurement-valid arms 后，在第 5 arm 因
  `usage_incomplete=true`、accounting complete=false、`billing_unknown=false` 停止；
  raw 保存 5 个 terminal/Store/reopen/verifier snapshots、4 个 arm results 和一个 abort，
  没有重跑或补样；
- 4 个完整结果是 2 verified、1 correct safety rejection、1 non-terminal loss、
  false success=0。唯一 loss 为 `rust_cli`：workspace external verifier 通过，但
  terminal blocked 且无 Host receipt；
- 同一 read-only analyzer 把 `root_recovery` 的无 arm-result snapshot 分类为
  measurement interruption，并证明唯一 product loss 只出现在一个 independent task。
  当前 production candidate=none。

M11 没有改变 production crate/config/protocol/schema。后续仍要求同一 stable current
loss 至少跨两个 independent tasks，才可另立一个单-owner、单变量、可删除旧路的 vertical
slice。M10-A–G 的已拒绝/关闭 treatment 不能因本次单 task loss 而恢复。完整事实见
[M11 loss baseline](../../eval/summaries/m11-loss-baseline-2026-07-25.md)。

M12 纠正了 M11 的唯一粗粒度 loss projection，但仍没有改变 production：

- M11 `rust_cli` 的 canonical Host verifier 运行在缺少 rustup default toolchain 的隔离
  `HOME`，external verifier 则继承另一环境；该轨迹现在稳定分类为
  `evaluation_environment_mismatch`，不是 Host terminal convergence loss；
- M12 每个 arm 让 Host 与 external verifier 共享同一隔离 `HOME`、显式 `.rustup`
  identity 与 Rust 1.97.0；这是 Harness acquisition contract，不是新的 production
  environment owner；
- `rust_endpoint` / `typescript_cache` 各 3 次，6/6 verified、false success=0；
  latest-revision Host receipt、external verifier、route/lane、usage/cache/cost 与
  credential-free SQLite reopen 全部闭合；
- combined M11+M12 analyzer 从 11 canonical Store trajectories 派生 8 verified、
  1 correct rejection、1 environment mismatch、1 measurement interruption、0 false
  success 与空 current product loss 集合；
- `close_hypothesis_no_repeated_product_loss` 不产生 completion controller、
  request-budget path、prompt、retry、protocol、State schema 或 Runtime/Store delta。

M12 之后，current production 仍是唯一 AgentApplication、AgentRuntime、RunStore、
DeepSeek ChatCompletions sender 和 fixed actor route。完整事实见
[M12 terminal convergence reproduction](../../eval/summaries/m12-terminal-convergence-2026-07-25.md)。

M13 只增加 eval acquisition 与关闭证据，没有改变 production：

- `scripts/eval-m9b-fixed-pro-regression.py --campaign m13` 选择 6 个 current long-task
  fixtures；默认 M9-C、M11、M12 campaign 保持 frozen；
- strata 覆盖两项 cross-file root debugging、一项 `verifier_failed` recovery、两项
  typed edit conflict recovery 与一个 explicit isolated Writer；每项 3 次，
  `maximum_reruns=0`；
- `required_failure` 观察器只从 canonical ToolPrepared/ToolOutcome/Host receipt 派生；
  edit precondition failure 要求 non-applied，verifier failure 保留执行后
  `indeterminate/unsafe`，且指定 failure 先于 applied mutation 与 final Host pass；
  不持久化第二 recovery truth；
- Host 与 external verifier 共享每 arm 隔离 `HOME`、`.rustup` identity 和 Rust
  1.97.0；fixture materialization 在 Git init 前证明 verifier fail；
- 每个 root/Writer RunRequest 显式为 `deepseek-v4-pro/high`，仍使用唯一
  AgentApplication、AgentRuntime、RunStore、ChatCompletions sender 与工具目录；
- 三次 fresh position-1 acquisition 都在下一 arm 前停止：Writer scope 输入顺序、
  malformed stale-patch hunk count、verifier failure disposition 依次暴露
  evaluator-contract mismatch；三份 raw 不续跑、不补 mate、不拼接；
- v3 的 production terminal、external verifier、latest-revision Host receipt、changed
  scope 与 accounting 全部闭合；`run_verifiers` 失败的 canonical
  `side_effect=indeterminate/retry=unsafe` 是已执行外部命令后的保守 truth，不能被
  observer 改写为 `not_applied/after_correction`；
- M13 决定为 `close_m13_inadmissible_observer_contract_instability`。production
  crate/config/protocol/State schema delta=0；没有 stable loss 跨两个 task 的有效证据，
  不新增 recovery owner 或 treatment。

完整冻结契约见
[M13 长任务恢复损失基线](../../eval/summaries/m13-long-task-loss-baseline-2026-07-25.md)。

M14 只改变 eval observer 与已关闭 campaign 的消费者边界，不改变 production：

- `scripts/eval-m9b-fixed-pro-regression.py --observer-conformance` 是唯一新增入口；它只
  读取提交的 12-case corpus，不接收 Key、binary、admission、raw 或 output path；
- observer label 只来自 canonical event kind、`ToolOutcome` 六个稳定 axis、
  `AgentWorkspaceAssignment` 与 exact reopened facts；没有第二 Runtime、Store、event、
  tool outcome 或 recovery truth；
- protocol test 通过 `ToolOutcome::validate()` 校验所有 corpus outcome；tools tests
  冻结 parse-valid stale patch 的 `workspace_precondition` 与 verifier failure 的
  `indeterminate/unsafe`；runtime/state/process tests 冻结 canonical Writer scope、
  SQLite reopen 和 SIGKILL exactly-once；
- `--campaign m13`、错误 generic disposition classifier、M13-only loader/profile/
  self-test 与不可达 trajectory branch 已删除。默认 M9-C 及 M11/M12 campaign 保留；
- M13 frozen manifest/fixture/summary/raw 保持不可变且不是输入。12/12 offline cases
  通过，report byte-identical；production schema/config/model sender/accounting 均未变。

M14 决定为 `keep_offline_observer_conformance / retire_m13_live_acquisition`。实现
checkpoint 为 `7a9e2278`；完整证据见
[M14 observer conformance](../../eval/summaries/m14-observer-conformance-2026-07-25.md)。

M15 当前只增加 eval acquisition 输入，不改变 production：

- `scripts/eval-m9b-fixed-pro-regression.py --campaign m15` 选择 8 个新的 current
  fixed-Pro tasks；M9-C/M11/M12 与 M14 observer consumer 保持；
- 任务覆盖 scoped rules、同名 symbol localization、多文件 acceptance、verifier
  recovery、真实 JSONL subprocess、一个 read-only child、一个 explicit Writer 和
  no-tool safety counterexample；每项 3 次，`maximum_reruns=0`；
- 每个 root/child RunRequest 显式为 `deepseek-v4-pro/high`，继续使用唯一
  AgentApplication、AgentRuntime、RunStore、ChatCompletions sender 与工具目录；
- Host 与 external verifier 共享 per-arm isolated `HOME`、`.rustup` identity 与
  Rust 1.97.0；8 个初始 fixture verifier 全部 fail 且 tree byte-stable，7 个正向
  verifier 在仓库外参考修复副本通过，安全反例继续失败；
- M14 observer 仍为 12/12，M9-C/M11/M12/M15 self-test、journal hash-chain 与四个
  SIGKILL window 通过；
- candidate `8f887fe2` 与 admission `0c589f2b` 已从 position 1 执行正式 acquisition；
  16/24 arms 后由 `false_success_observed` 硬停，没有继续或补跑；
- 16 arms 全部 accounting-complete、billing-known、SQLite reopen exact；frozen facts
  为 11 verified、2 correct safety rejection、1 false-success label；
- canonical audit 证明 false-success label 来自 evaluator exact changed-file 假设：
  external verifier、latest Host receipt、route 与 lane 已通过，两文件等价实现未修改
  参考方案预设的第三个 helper；raw 不回写，read-only report 记为
  `evaluation_scope_mismatch`；
- 其余只有一个 root `deterministic_verifier_failed` 与一个不同 owner 的
  `writer_delegation_failed`，没有跨 task stable loss；决定为
  `inadmissible_evaluator_scope_mismatch / insufficient_repeated_current_loss`，
  production 保持不变。

M15 没有 production crate/config/protocol/schema/model sender delta。Run API v12、
RuntimeEvent v18、State v24 与 exec-stream v3 不变；M13 frozen raw 不读取、不续跑、
不拼接。完整证据见
[M15 current product-loss acquisition](../../eval/summaries/m15-current-product-loss-acquisition-2026-07-25.md)。

M16 只改变 corrected Harness 的 completion observer，不改变 production：

- `allowed_paths` 是唯一 changed-file 安全边界；实际 `changed_files`、Writer seal 与
  integration 是 canonical observation，reference changed-file set 只保留为
  `exact / implementation_subset / additional_within_scope / alternate_within_scope`
  诊断；
- current `verified_success` 要求非空且 scope-valid 的修改、冻结 external verifier、
  valid route/lane、completed terminal，以及与 terminal decision 和
  `workspace_state_after` 一致的 latest-revision `EvidenceReceipt`；任何一项缺失都不能
  被参考实现文件集合补偿；
- Writer seal 与 integrated root diff 必须互相一致并位于 allowed scope，但不再要求
  等于某个参考 patch 的 exact file set；
- `--acceptance-conformance` 的 18-case 离线 corpus 覆盖 M15 两文件等价实现、
  implementation subset、alternate/additional in-scope 修改、越界、安全反例、
  verifier/receipt/revision、route/lane、Writer 与 exact reopen；report 连续两次
  byte-identical；
- M15 frozen manifest/raw/labels 保持不可变；通用 legacy projection 仍得到冻结
  11 verified / 2 correct rejection / 1 false-success label，以及 corrected
  12 / 2 / 0 product projection；该一例继续归因
  `evaluation_scope_mismatch`；
- M15 formal entry 在 Key 或 network 前稳定拒绝为 `m15_campaign_closed`，不能续跑剩余
  schedule；下一次 product-loss acquisition 必须使用全新 identity 和 position 1。

M16 不改变 Run API v12、RuntimeEvent v18、State v24、exec-stream v3、DeepSeek
ChatCompletions sender、fixed actor route、Runtime、Store、工具目录或 production
verifier。完整证据见
[M16 acceptance-equivalence observer](../../eval/summaries/m16-acceptance-equivalence-observer-2026-07-25.md)。

M17-A 已把当前活动产品、binary 和 Cargo/import 身份硬切为 DSE：

- workspace package set 精确为 16 个 `dse-*` crate，binary target 只有 `dse`、
  `dse-tui`；
- CLI/TUI help/version、official DeepSeek User-Agent 和 production constitution 使用
  DSE；constitution 仅改变名称，执行/权限/验证/工具/多 Agent 条款不变；
- root、read-only child、explicit Writer 继续共用同一
  `AgentApplication -> AgentRuntime -> RunStore`；fixed route、DeepSeek Chat sender、
  accounting 和 completion owner 不变；
- 当前仍是 Run API v12、RuntimeEvent v18、State v24、exec-stream v3；
- `.codewhale`、`CODEWHALE_*`、active protocol/media type 与 delivery script 仍是
  M17-B/C 的待删除旧路径，不能把 M17-A 误报为整个 identity/release cutover 完成；
- DSA 从未形成 production commit；`83d05775` 只保留为被 DSE 决策 supersede 的 Git
  历史，frozen `DeepSeek Agent` 评测标题和 evidence 未改写。

完整证据见
[M17-A DSE product identity](../../eval/summaries/m17-a-dse-product-identity-2026-07-25.md)。

M17-B 已把当前活动 config/state/protocol identity 硬切为 DSE：

- home/config/env 只认 `~/.dse`、`DSE_HOME`、`DSE_CONFIG_PATH` 与活动 `DSE_*`；
  `.dse` 是 project metadata、worktree、skills、logs、tool artifacts 和 local state 的
  唯一新写入 namespace；
- Secret file owner 与 keychain service 使用 DSE，prompt wrapper/constitution tag、
  `application/vnd.dse.verification+json`、`<dse:runtime_event>` 和
  `dse.exec-stream` 是唯一活动协议身份；
- 当前版本为 Run API v12、RuntimeEvent v19、State v25、exec-stream v4。v25 原子保留
  replay-safe pending Start，retire 不能在不改写 transcript 的 v18 materialized runs；
  没有 compatibility reader、dual write、第二 Store 或第二协议真相；
- 当前开发机 exact-copy config、settings、setup state、permissions、onboarded marker
  和 file Secret 到 `~/.dse`，逐项 `cmp` 成功且权限保持；`~/.codewhale` 原目录仍是
  完整只读备份，旧 sessions/logs/tool outputs 没有进入 DSE state；
- frozen `DeepSeek Agent` 评测标题、历史 `codewhale.eval.*`/canonical JSON fixture、
  manifest、hash、summary/raw 与真实仓库路径仍保持原事实。它们是明确 allowlist，不是
  当前可调用产品面；
- M17-C 已把 delivery/dev/CI 当前 caller 切到 DSE；旧活动文件名、artifact/install
  path 和 M8-L no-consumer release reader 已删除。

完整证据见
[M17-B DSE config/state/protocol identity](../../eval/summaries/m17-b-dse-config-protocol-identity-2026-07-25.md)。

M17-C 当前交付事实：

- 唯一 package/install/verify/rollback/uninstall owner 是
  `scripts/dse-delivery.sh`；schema 是 `dse.delivery.v1`，binary set 是
  `dse,dse-tui`，immutable install root 是 `lib/dse`；
- `scripts/dev-dse.sh`、`scripts/test-dse-delivery.sh`、TUI hermetic runner、
  current developer docs 和 `.github/workflows/ci.yml` 已迁移到 DSE；
- old CodeWhale delivery/dev/test 文件名与 install root/link 不再有 current caller；
  无消费者 M8-L release evaluator/test 已删除，frozen manifest/summary/result 仍保持
  历史事实并可在旧 revision 获取其 runner；
- current `fd23400ca` 在 macOS arm64 以 Rust 1.97.0、Cargo.lock 和 offline cache
  建成 source package，安装后两项 binary 均报告同一 revision，verify/uninstall 通过；
- Linux arm64 在已缓存 Bookworm image、network denied、source read-only 条件下通过
  deterministic package/upgrade/rollback/uninstall fixture；
- GitHub Actions 已配置 Linux/macOS 同一 owner。`8c57c4dba` 已由用户单独授权推送到
  private origin，但对应 run 在任何 step 前因账户付款或 spending limit 被 GitHub
  拒绝；这不是代码门禁结果。当前没有 tag、release 或 public visibility 变化。

完整证据见
[M17-C DSE delivery and CI](../../eval/summaries/m17-c-dse-delivery-ci-2026-07-25.md)。

M17-E/M17-G 当前 localization / human projection 事实：

- `crates/localization` 是唯一 product-language owner，活动语言精确为 `en` 和
  `zh-Hans`；M17-E checkpoint 为 768/768 key，M17-G 嵌套 auth/model help 纠错后 current
  catalog 为 776/776 exact keys，named placeholders 仍相同；
- CLI/TUI process language 在启动时冻结，优先级为显式 `--language`、persisted
  `[ui].language`、旧本地 DSE 迁移、fresh environment；没有 per-Run/per-Agent locale、
  language classifier、翻译模型或额外 DeepSeek request；
- fresh noninteractive process 使用 English；fresh interactive TUI 提供一次双语选择并
  通过 canonical `dse_config::ConfigStore` 持久化；旧 `.onboarded` 安装在没有语言配置时
  保持 `zh-Hans`；
- CLI/TUI retained help、config、setup、Doctor、MCP/OAuth、approval、error、recovery、
  root/read-only child/Writer 和 work-surface 人类文本均消费同一 catalog；app-server
  保持 canonical machine HTTP/SSE/stdio，只有 CLI wrapper 的 human error 被本地化；
- language 只影响 human projection。raw tool/provider/stdout/stderr、stable code、
  JSON/NDJSON/HTTP/SSE、Run API v12、RuntimeEvent v19、State v25、exec-stream v4、
  official DeepSeek Chat sender、fixed actor route、request/accounting 与 RunStore facts
  均未改变；首次双语 picker 和模型用 prompt/template 是明确稳定 allowlist；
- 双语真实 approval、English 80-column、CJK layout、root/read-only/Writer/recovery、
  process crash/reopen 和 workspace 全量门禁已通过；
- M17-F 已用 fixed-Pro/high 完成 block-1 2x2 正式评测：32/32 measurement-valid，
  26 个正向 verified、4 个正确安全拒绝、false success 0。English prompt 在
  `rust_scoped_rules:en` 出现 treatment-only loss，因此未通过非劣门；block 2 未执行；
- production 只保留中文表达的 system prompt，并按用户当前任务语言回答。M17-F 被测
  normalized SHA-256 为 `a9179903...`；M17-G 把 active context marker 从 `cw:ctx`
  修为 `dse:ctx` 后，current full assembled fixture SHA-256 为
  `d7746692db36eea33da0305553499b708a4d8b2b7d9688633aba1673142d49c9`，constitution 与
  winner 不变。不存在 prompt selector、双 production branch、语言分类请求或翻译模型；
  English candidate、临时翻译 scaffolding、eval assets 与 M17-F-only runner 已删除；
- `README.md` 是英文 canonical public entry，`README.zh-CN.md` 是完整中文入口；current
  reference、贡献/安全/行为/来源、CODEOWNERS 与 issue/PR templates 由
  `scripts/check-public-repository.py` 检查。M7-A/M7-A2 `DeepSeek Agent` 标题与 frozen
  evidence 是历史 allowlist，未改写为当前产品名；
- Sandbox 当前真实 enforcing 路径是 macOS Seatbelt 与 Linux bubblewrap；isolated Writer
  无 enforcing backend 时 fail closed。Landlock/seccomp 未接入 spawned child，Windows
  不声明 local OS sandbox。

完整证据见
[M17-E DSE bilingual human projection](../../eval/summaries/m17-e-dse-bilingual-human-projection-2026-07-25.md)。
M17-F 的正式身份、指标、质量否决与删除证据见
[M17-F DSE bilingual prompt 2x2](../../eval/summaries/m17-f-bilingual-prompt-ab-2026-07-25.md)。
M17-G 的公开仓库、身份 allowlist、秘密/许可/来源与门禁证据见
[M17-G DSE bilingual public repository](../../eval/summaries/m17-g-dse-public-repository-2026-07-25.md)。

M17-H local candidate `a8c4bafab` 又删除了最后一个 tracked
`.codewhale/constitution.json` 死路径、whale theme aliases/内部 palette identity 与
鲸鱼空状态视觉，并由 public checker 对 active crates 建立精确 old-identity allowlist。
它没有新增 repo-local `.dse/constitution.json`，所以 M17-F 的 production prompt owner
和 winner 不变。当前 ignored `eval/raw` 269 个文件均为 `0600`；LICENSE 与导入基线
byte-identical。

同 revision macOS locked/offline source artifact 与 Linux arm64 无网络 fixture lifecycle
通过。private origin 已与 `8c57c4dba` 对齐；GitHub Actions run `30164259559` 因账户
付款或 spending limit 在 runner step 前被拒绝，private branch protection/rulesets 也因
套餐限制不可用。用户随后明确当前只做本地、不再操作 GitHub，因此 M17 以本地 DSE V1
完成收口；remote CI、rename、protection、visibility、tag 与 release 均为未执行且延期的
独立外部工作，没有被冒充为通过。活动 workspace 宿主目录保持原路径，不影响 DSE 产品、
协议、安装目录或发布物身份。
完整事实与下一授权边界见
[M17-H DSE release-readiness](../../eval/summaries/m17-h-dse-release-readiness-2026-07-25.md)。

M18 又以 installed artifacts 完成纯本地首日生命周期：fresh `DSE_HOME` 下的
English/`zh-Hans` 首次 TUI、`dse exec`、same-Run resume、exact-source upgrade、
rollback 与 data-preserving uninstall 全部通过。installed TUI acceptance 现在优先使用
Cargo 提供的 runtime `CARGO_BIN_EXE_dse-tui`，production binary、Runtime、Store、
protocol 与 model path 未改变。

同一 current fixed-Pro/high binary 的 M18 reliability acquisition 留下 15 条完整
canonical Store trajectory：11 个正向 verified、3 个正确安全拒绝、false success 0，
以及一个由 Host 正确 blocked 的 TypeScript verifier failure。第 16 arm 在 terminal
snapshot 前达到 frozen `run_deadline`，因此只计为 measurement interruption；没有 rerun、
补 mate 或 physical request accounting 推断。stable loss 只覆盖一个独立 task family，
结论为 `insufficient_repeated_current_loss`，没有 production candidate 或 caller 迁移。
当前 fixed actor routes、唯一 AgentRuntime/RunStore、canonical tools 和 official
DeepSeek ChatCompletions 保持不变。GitHub 与远端 CI 不属于 M18 gate。

M19 在 corrected fixed-Pro Harness 和新冻结 fixture/manifest 上建立并执行了一个本地
position-1 acquisition，不改变 production crate：

- 新 campaign 使用 Run API v12、RuntimeEvent v19、State v25、exec-stream v4 和
  official DeepSeek OpenAI-format `/chat/completions`；
- root 与 explicit Writer 都显式 `deepseek-v4-pro/high`；没有 Auto、classifier、
  dynamic route 或额外模型请求；
- outer watchdog 不再先销毁 active per-arm State。它先停止 credential-bearing
  app-server，再无 credential 重开同一 SQLite Store，投影 terminal presence、physical
  started/completed/in-flight、usage/sealed/billing_unknown；
- nonterminal deadline snapshot 固定为 measurement/product-loss ineligible，in-flight
  不推断 billed/unbilled，`maximum_reruns=0`；
- 新任务是两个独立 TypeScript verifier recovery、一个 Rust cross-file debug、一个显式
  isolated Writer 四文件迁移与一个安全反例；每项三次，使用一个 immutable binary；
- production Runtime typed deadline、RunStore、ToolOutcome、sender/parser/accounting、
  prompt、工具目录和 actor profiles 当前均无 delta。只有同一 stable loss 跨两个独立
  task ID 重复并完成 owner audit，才允许后续最小 vertical fix。

M19 是全新 position-1 acquisition；M18 manifest/raw/15 条完整轨迹和 deadline interruption
保持冻结，不续跑、不补 mate、不拼接。GitHub 与远端 CI 不属于 M19 gate。

M19 candidate `93e3ee34d` 的 offline gates、immutable binary 与 no-Key dry-run 全绿。
正式首 arm 的一个 physical request 在 response headers/finish/usage/content/reasoning 前
提交 typed `deepseek_transport` failed terminal；root accounting 为
`started=1 / completed=1 / in_flight=0 / sealed=true / billing_unknown=true /
complete=false`，无 retry。Harness 在 0 个完整 arm 后 `accounting_incomplete` abort；
后 14 个 arm 未启动。

read-only report 只得到 1 个 canonical Store trajectory、0 arm result、1 measurement
interruption、0 product loss 和 0 repeated independent loss。M19 因此是
`stop_incomplete_accounting / insufficient_repeated_current_loss`，没有 production
candidate。watchdog 的 credential-free reopen 路径保留在唯一 corrected Harness；
Runtime、Store、sender、accounting、prompt、tools 与 fixed actor profiles 均未改变。

M20 把本地 Doctor 从 production accounting 外的 one-token Chat inference 改为一个
bounded non-inference account probe：

- 唯一实现为 `crates/deepseek::DeepSeekConnectionConfig::probe_account`；
- 使用 canonical endpoint config/client/TLS/Bearer/User-Agent/header timeout；
- 只发送一次 official `GET /user/balance`，无 retry、无 model request budget 或 usage
  ledger；
- 只接受 `is_available: bool` 与 `balance_infos: array` 的 response shape，不返回或保存
  balance；
- CLI/TUI 的 en/zh-Hans projection 明确其只证明 official host/credential
  reachability，不证明 Chat、billing 或 Agent success；
- 当前 production coding sender 仍唯一指向 official
  `https://api.deepseek.com/chat/completions`，使用 V4 model IDs。

immutable `dse-tui` 的三个正式本地 account probe 全部 reachable，0 model request、
0 known API cost。它 collectively 证明当时 DNS/TCP/TLS/HTTP/auth 可达，但不增加
health/billing owner，也不解决 request-level/pre-header billing truth。

该 viability 随后只授权一个全新 M20B fixed-Pro acquisition。M20B candidate
`7797ec9d7aaa` 和 admission `00a10a196` 使用新 Python/TypeScript/safety tasks、
fixed Pro/high、同一 canonical Runtime/Store/tools 与 `maximum_reruns=0`。首个 Python
arm 在 8 个 usage-complete response 后 verified；第二个 TypeScript arm 已通过 external
verifier，但第六个 response 发生 typed `deepseek_transport`，六个 response 只有五个
usage。Store 与 credential-free reopen 一致保存
`started/completed=6/6 / in_flight=0 / billing_unknown=false /
usage_complete=false / sealed=true`。

Harness 在 1 个完整 arm 后停止，后 7 arm 未启动。read-only projection 得到 2 个
trajectory、1 verified arm、1 measurement interruption、false success 0、0 product
loss 与 0 repeated independent loss。当前事实因此是
`stop_incomplete_accounting / insufficient_repeated_current_loss`，不是完整 baseline，
也没有 production recovery、retry、prompt、tool、route、Runtime 或 Store candidate。
完整证据见
[M20 transport viability and fixed-Pro successor](../../eval/summaries/m20-transport-viability-and-fixed-pro-successor-2026-07-26.md)。

M21 对 M20B 的 partial-response interruption 做了纯本地 owner audit。冻结事件证明第六个
response 在 headers 和一个 reasoning delta 后 56 ms 即失败，不是 Harness 120 s 或
production 900 s idle timeout。真实 loopback HTTP fixture 通过 canonical reqwest byte
stream 和 DeepSeek SSE transport，在一个合法 reasoning frame 后截断 declared body，
逐项复现：

- typed `deepseek_transport`、transport category、retryable；
- actionable reasoning 令 replay unsafe，因此没有 Runtime/transport retry；
- finish、`[DONE]`、usage 与 committed model response 均不存在；
- response count 增加而 usage response 不增加，
  `incomplete_responses=1 / billing_unknown=false / usage_incomplete=true`；
- Runtime 不发出 `CompletionProposed`，external verifier pass 不能代替 Host completion；
- State schema v25 在 SQLite reopen 后精确保留 failure/evidence/retry/accounting。

该 replay 没有暴露本地 transport/parser/accounting/retry/completion 缺陷，production
代码保持不变。当前可证边界是 response body 在 partial reasoning 后中断；未收到的 finish、
`[DONE]`、usage 和余下内容不可重建，也不能安全盲重放。M21 没有读取 Key、调用官方 API、
访问外部网络、创建第二 sender/Runtime/Store 或启动付费 successor。完整证据见
[M21 partial-response owner audit](../../eval/summaries/m21-partial-response-owner-audit-2026-07-26.md)。

M22 保留了一个纯本地、无 schema 变化的 DeepSeek streaming cutover。当前
`crates/deepseek/src/transport.rs` 在每次 reqwest body chunk 已经到达后，解析其中所有完整
SSE frame，并只把相邻同类 reasoning/content delta 收敛成一个 `ModelStreamEvent`。
evidence、tool fragment、finish、usage、`[DONE]`、provider error 与 parse error 都会先
flush 已收到的文本，再推进现有 typed boundary。没有 timer、跨 chunk 等待、配置或第二
stream path。

`AgentRuntime` 仍是唯一 RuntimeEvent v19 emitter，按它实际收到的 converged
`ModelStreamEvent` 分配 content/reasoning index；`StateStore` 仍 append-only，delta 仍是
projection-neutral event，只推进 durable sequence 而不重写 snapshot JSON。Run API v12、
State v25、exec-stream v4、canonical transcript、EvidenceReceipt 和 client reducer 均未
改变。旧数据库不迁移，既有逐帧事件照常精确 replay；新 run 只产生更少、payload 更大的
同 schema delta event，因此没有 compatibility reader 或双写。

冻结 M20B scale profile 的 deterministic A/B 从 2,952/5,231 个 delta event 收敛到
8/9 个；SQLite+WAL 分别下降 94.33%/96.05%，Run API event retrieval 加 canonical JSON
下降 97.97%/98.62%。真实 production loopback 又通过
`AgentApplication -> DeepSeekModelPort -> AgentRuntime -> RunStore -> Run API` 证明
reasoning/content 拼接、terminal、无 Key reopen 与 exact event projection。M21
partial-response fail-closed、accounting、SIGKILL/reopen、root/read-only/Writer 和
CLI/TUI/app-server conformance 保持通过。决策为
`keep_minimal_streaming_delta_convergence`；没有 Key、官方 API、外部网络、GitHub、push
或 release。完整证据见
[M22 canonical streaming-delta convergence](../../eval/summaries/m22-streaming-delta-convergence-2026-07-26.md)。

M23-A 只改变现有 corrected Harness/analyzer 的派生规则，不改变 production：

- ADR-0011 将 `behavior_status` 与 `accounting_status` 冻结为两个正交轴；
- behavior 只由冻结 task identity、workspace outcome、external verifier、production
  terminal、latest-revision Host receipt、route/lane 与 observer 决定；
- accounting 只由 canonical physical request/usage/pricing/seal ledger 决定，非
  complete 状态继续停止下一付费 request，且不能进入 cost/Token/full-utility aggregate；
- 10-case offline corpus 覆盖 complete、partial、pre-header、deadline、observer、
  receipt、unpriced 与 false-success 窗口，10/10 通过且 report byte-identical；
- analyzer 连接 canonical Store、credential-free SQLite reopen 与 verifier snapshot；
  旧 `arm_result=None -> measurement_incomplete` product-loss 分支已删除；
- M11/M12/M15/M18/M19/M20B frozen journal 可按新规则重算，但 frozen raw、manifest、
  summary 和历史 admission decision 不改写；
- production 仍是 Run API v12、RuntimeEvent v19、State v25、exec-stream v4、唯一
  AgentRuntime/RunStore、fixed actor route、canonical tools 与 official DeepSeek
  ChatCompletions。Key/API/network 为 0。

M23-A 结论为 `keep_orthogonal_behavior_accounting_truth`。它没有建立 Hardness baseline、
没有授权 high/max A/B，也没有形成 ApplicationProbe、symbol localization、
VerifiedMilestone 或 Tool ACI candidate。完整证据见
[M23-A behavior/accounting truth](../../eval/summaries/m23-a-behavior-accounting-truth-2026-07-26.md)。

M23-B1 没有改变 production。现有 corrected Harness 增加一个 credential-free `m23b`
campaign，冻结 20 个独立任务、3 个平衡 round 和 60 个 future fixed-Pro/high control
arm。136-file monorepo fixture 可为每个 arm 物化 fresh Git repository；17 个正向
reference solution 全部从初始 verifier failure 变为 pass，3 个安全反例继续失败。
Go service 与 Chrome/Playwright DOM verifier、本机 toolchain、reference changed scope、
continuity/runtime assertion、资源预算和四个 journal crash window 均由同一 Harness
离线自证，连续两次 freeze/self-test byte-identical。Key、API、network、model request
与 production delta 均为 0。

M23-B1 结论为 `keep_offline_hardness_task_set_control_not_acquired`。当前没有
fixed-Pro Hardness trajectory、pass@1/pass^3、false-success aggregate 或重复
owner/cause loss matrix，因此不授权 high/max、ApplicationProbe、symbol localization、
VerifiedMilestone 或 Tool ACI。完整证据见
[M23-B1 Hardness task set](../../eval/summaries/m23-b1-hardness-task-set-2026-07-26.md)。

M23-B2/B3 仍只改变唯一 corrected Harness，不改变 production。B2 从 canonical
RuntimeEvent、workspace mutation、Host receipt 与 external verifier 派生 Hardness
metrics，并把真正 resume 限定为 durable approval checkpoint、不同 process、byte-exact
event prefix、reopen 时 physical request 不增长和新进程继续。B3 用现有
app-server external-process test child 实现这条 lifecycle；3 个冻结长任务才获得
interactive caller，其余 arm 不变。process self-test 证明 SIGKILL/reopen/resume 后
请求与文件副作用不重复，正式 M23B arm 现在投影同一 metric/truth contract。

M23-B4 从 clean `e68d215c` binary 和 `8090adce` admission 启动 formal
fixed-Pro/high control。前 5 个 arm 为 verified success、false success 0 且 accounting
complete；第 6 个 read-only task 形成 canonical failed terminal、Store exact reopen 与
verifier snapshot，但 response usage incomplete。runner 在下一物理 request 前停止，
没有 rerun 或 mate。ADR-0011 analyzer 保留 6 个 behavior observation 和 5 个完整
accounting observation；唯一
`deepseek_transport:deepseek_transport` loss 只覆盖一个独立 task。

M23 因而以 `stop_incomplete_accounting` +
`insufficient_repeated_current_loss` 停止。当前没有完整 Hardness baseline、high/max
对照或四个 production candidate；production 继续是唯一 AgentRuntime/RuntimeEvent/
RunStore、fixed actor route、canonical tools、Host latest-revision completion 与 official
DeepSeek ChatCompletions。完整证据见
[M23-B4 Hardness control](../../eval/summaries/m23-b4-hardness-control-2026-07-26.md)。

M24 没有改变 production architecture。它从 clean detached
`92d8b84b3a79167b8d912365af8d1e808a94c2ed` 重建当前本地 V1 artifact，并重新验证：

```text
clean DSE source + Cargo.lock + Rust 1.97.0
  -> scripts/dse-delivery.sh package --locked --offline
  -> dse.delivery.v1 exact manifest / inner + outer SHA-256
  -> isolated install / verify / upgrade / rollback / uninstall
  -> existing CLI / TUI / app-server / RunStore conformance
```

artifact 只含 manifest、LICENSE、SHA256SUMS、`dse` 和 `dse-tui`；两项 binary 都报告
`0.8.68 (92d8b84b3a79)`。当前 exact artifact、双语 human surface、fixed actor route、
M22 same-chunk convergence、M21 partial-response fail-closed、pending Start、SQLite
reopen、SIGKILL/replay、accounting 和 root/read-only/Writer surface parity 全部通过。
public repository、tracked-secret、269 个 ignored `0600` raw、LICENSE/provenance 和
retired-identity allowlist 也保持闭合。

决定为 `keep_local_v1_release_candidate_no_blocker`，production delta=0。M24 没有读取
Key、调用 official API、访问 GitHub、push 或 release，也没有增加第二 installer、
Runtime、Store、Provider、protocol 或兼容路径。完整事实见
[M24 local release candidate](../../eval/summaries/m24-local-release-candidate-2026-07-26.md)。

M25 没有改变 production architecture。根 `AGENTS.md` 不再保存当前实现/里程碑账本，
而是 134 行 / 5,488 bytes 的稳定 authority + owner directory；当前版本、cutover、
拒绝结论和 frozen evidence 继续只由 PRODUCT_PLAN/ADRs、ROADMAP、EVALUATION、
CURRENT_CODEWHALE 与 `eval/` 各自拥有。

现有 `scripts/check-public-repository.py` 现在机械限制根 guide 的 line/byte budget，验证
五个 authority link、stable architecture/work/protocol/replay/Git/validation rules，并
拒绝 milestone/commit ledger。三个内建负向 fixture 覆盖 oversized、
missing-authority 与 mutable-history false green。没有第二 checker、parallel docs、
dependency layer、Rust file split 或 compatibility reader。

12 个 stable rule anchor 的 median first line 从 309.5 降到 76，guide bytes 下降
74.24%；focused、strict Clippy、workspace test 和 clean-checkout locked/offline release
regression 通过。Run API v12、RuntimeEvent v19、State v25、exec-stream v4、唯一
AgentRuntime/RunStore、fixed actor route、official DeepSeek ChatCompletions 和 canonical
tools 均保持不变。完整事实见
[M25 agent-legibility cutover](../../eval/summaries/m25-agent-legibility-2026-07-26.md)。

<a id="current-tui"></a>
## 7. 当前 TUI 表面系统（M28 complete）

M28 已在 final cutover closure `07214e2ef`（首次 PTY cutover `1e420d9d7`，
parity closure `068c8e3a9`，旧术语清理 `60df6f8f7`）完成
`keep_native_surface_and_delete_legacy`：

- `shell.rs` 是唯一 terminal-native 主外壳；transcript-first 主工作区在宽屏使用
  canonical Run right rail，在中屏使用同事实 top strip，在窄/矮屏使用 single column；
- production topology 只有 main work surface、bottom sheet、full-screen room 和 inline
  approval interruption。onboarding、user input、permission、approval、pager/help/cost/
  diff/evidence 都已迁移；generic centered modal 不存在；
- palette 只保留一个 semantic token owner 与 terminal color-depth adaptation；
  terminal background/foreground 是 base，没有 selectable theme、background 或第二
  reader；
- idle event-driven 且真实 PTY 初帧后 5 秒 byte-still；鱼、气泡、渐变、flee animation、
  80ms ambient cadence 和 decorative completion 均已删除；
- persistent display variants、layout/decorative settings、旧 normalizer、Doctor 退役
  command hints、硬编码 settings warning 和孤儿 session/command fixtures 已删除；
- tool detail 使用一个默认 compact rule 与 process-local row expansion，不写入第二状态；
- `CanonicalRunPresentation` 仍是 task/activity/change/verification/Agent/permission/
  recovery/terminal 的唯一 presentation truth；TUI 没有 plan、progress、evidence 或
  completion owner。

English/`zh-Hans` × 五个冻结尺寸的真实 PTY resize、keyboard/mouse/paste、
onboarding/slash/mention/approval/permission、first-run、live/terminal、pending
creation recovery 和 credential-free SQLite reopen 通过。live 与 reopen 的尺寸、非空
glyph/坐标、前后景、modifier 与 cursor 逐项相等；只规范化等价的 terminal-default
空白编码。M28 未修改
RuntimeEvent、Run API、State、DeepSeek、tools、prompt、model 或 permission 语义，也没有
General Settings、ThemePicker、ConfigView、legacy toggle 或 compatibility reader。设计与
后续约束见 [ADR-0013](../decisions/0013-native-tui-surface-system.md)、
[DESIGN.md](../../DESIGN.md) 和 [ROADMAP M28](../product/ROADMAP.md)。

M29 没有改变 production architecture。它从 M28 clean checkpoint `548afbe3e` 出发，把
长 root task、verifier recovery、approval deny/approve、read-only child、explicit
Writer、process/Store reopen 和 terminal interaction 8 条 workflow 重新绑定到现有唯一
production composition 与 deterministic acceptance。

全部 workflow 通过；focused、strict workspace Clippy、workspace test、双语 PTY、
真实 Git Writer、process SIGKILL、surface parity、delivery self-test 和当前 source 的
locked/offline package/install/verify/uninstall 也通过。没有重复且可归因的 defect，
所以结论为 `keep_current_workflow_no_reproducible_blocker`，production delta=0。没有新增
Harness、probe、Runtime、Store、protocol、renderer、permission/model route 或兼容路径。
完整事实见
[M29 local workflow acceptance](../../eval/summaries/m29-local-release-workflow-2026-07-27.md)。

M30 从 M29 clean checkpoint `1b7f92a97` 继承 current Run API v13 / RuntimeEvent v20 /
State v26 / exec-stream v4 identity，并按 hash 引用 M23 的 20-task fixture、task
contracts、tool policies 与 reference patches；不继承 M23 的旧 binary、raw、admission
或停止位置。当前 fixed actor route、三档 permission、唯一
AgentApplication/AgentRuntime/RunStore、canonical tools 与 latest-revision Host
completion 均未改变。

用户在 offline self-test、real process SIGKILL/reopen、strict Clippy/test 与 immutable
release identity 闭合后，明确授权冻结 `$10` 上限的 current acquisition。admission
`2761f1b8d` 绑定 candidate `f74804a08`；formal campaign 完整闭合 12 个 arm，在第 13
个 Writer arm 保存 terminal/Store/credential-free reopen/verifier facts 后因
`billing_unknown=true` 按合同停止。没有重跑、补 mate 或执行后七个任务。

当前只读 canonical loss matrix 有 13 条 Store trajectory、12 条 full-utility
observation：9 verified success、1 correct safety rejection、2 verified product
failure、false success 0；第 13 条是 billing unknown 与 route invalid observation。
两个 accounting-complete 的 independent long-horizon task 重复
`host_completion:verified_workspace_without_terminal_receipt`，因此只准入一个
`crates/runtime` named-verifier ACI audit。两条 canonical 时间线的 contract-bound
verifier 调用都在执行前被 binding 拒绝，最终 Host exact verifier 通过却缺少
failed-write-pass lineage；Stop Gate 的拒绝仍是正确行为。production delta 仍为 0，
不得弱化 receipt、伪造 fail-before、补跑 acquisition 或建立第二 verifier path。完整
事实见
[M30 current dogfood loss acquisition](../../eval/summaries/m30-dogfood-loss-acquisition-2026-07-27.md)。

M30 随后的 description-only named-verifier ACI treatment 已正式
`reject_and_delete`。candidate `d0d509b6e` 没有改变 schema、resolver、RuntimeEvent、
State 或 Store，只明确 `verifier_id` 应为 TaskContract acceptance ID。两个原 loss
task 都完成 fixed-Pro/high、Ask continuity SIGKILL/reopen、external verifier 和完整
accounting，false success 仍为 0；但 verified success 为 0/2。18/18 次模型调用已使用
正确 acceptance ID，随后 18/18 次在 operation 启动前被 current Host permission
policy typed fail-closed，因为执行后端不能证明一次性 external-path authority。两项
workspace 最终均通过 external verifier，但仍无 canonical Host receipt，Stop Gate 正确
保持 blocked。

ACI 文字、对应测试与临时 `m30t` Harness consumer 已物理删除，当前 production
named-verifier description/schema/resolver、三档 permission policy、唯一 Runtime/Store
与 failed-write-pass gate 均恢复 treatment 前事实。frozen manifest/admission、ignored
0600 raw、Git 历史和
[M30 treatment summary](../../eval/summaries/m30-named-verifier-aci-treatment-2026-07-27.md)
仅用于审计；它们没有 production consumer。本结果只暴露一个未来需独立冻结的
permission/verifier integration owner，不授权在 M30 内叠加第二修复。

M31 已以 `keep_contract_verifier_grant` 收口。current production 使用 typed
`ToolExecutionGrant::TaskContractVerifier`：Runtime 只从 exact acceptance-ID handle
和 frozen `VerifierSpec` 派生 grant，tools 重新验证 canonical digest、invocation 和
workspace revision 后，才允许 exact verifier `commands[].program` 使用 Host 冻结的
read/execute authority。普通 external path/cwd、network、explicit deny、hard invariant、
read-only child 与 Writer sandbox 不变。current identity 因 durable shape 变更为 Run API
v14 / RuntimeEvent v21 / State v27 / exec-stream v4；两项原 loss task 均得到
latest-revision failed-write-pass receipt、exact reopen、false success 0 和完整 accounting。
临时 M31 Harness consumer 已删除，frozen contract/admission/raw/summary 保留。

M32 已以 `stop_incomplete_accounting` 收口。它从 M31 clean checkpoint `bfa8ec84f`
冻结新的 20-task position-1 fixed-Pro/high control-only contract；offline fixture、
reference、continuity SIGKILL/reopen 与全仓门禁闭合后，用户在 `$10` suite ceiling 下
明确授权。formal run 闭合 6 个 accounting-complete arm（5 verified success、1 verified
product failure、false success 0），第 7 个 Writer arm 在 response headers 前得到
typed `deepseek_timeout`，随后以 `usage_complete=true, billing_unknown=true` 停止。
没有重跑或执行后十三项。

只读正交 report 保留 7 个 canonical trajectory、6 个 full-utility observation 和 1 个
billing-unknown/route-invalid observation。唯一闭合 loss 只来自一个 task，结论为
`insufficient_repeated_current_loss`；不准入 treatment，也不把 partial result 外推为
完整 Hardness baseline、`no_repeated_current_loss` 或 M31 broad regression-safe。
temporary M32 Harness consumer 已删除并恢复到 M32 前 blob；frozen
contract/admission/analysis、ignored 0600 raw 与
[M32 summary](../../eval/summaries/m32-hardness-regression-2026-07-27.md) 保留。current
production 仍是 M31 exact TaskContract verifier grant、固定 actor route、唯一
AgentRuntime/RunStore 与 canonical tools，没有 M32 production delta。

M33 已把模型请求重试收敛到唯一 `AgentRuntime`。DeepSeek transport 每次调用只发送
一个物理请求，并只返回 typed timeout/network/HTTP/stream evidence、usage/accounting
与可选 `Retry-After`；它不再拥有 retry loop、sleep 或 retry 配置。Runtime 仅在
retryable、replay-safe、无 actionable output、次数与物理预算都允许时，原子提交 failed
attempt 与下一 attempt 的 `decision_unix_ms/backoff_ms/not_before_unix_ms/max_retries`。
默认仍是初次请求加最多两次重试，固定 1s/2s；实际 HTTP `Retry-After`（delay-seconds 或
HTTP-date）只能延长等待。SQLite reopen 会等待剩余 not-before 后只发送一次；已 in-flight
的 attempt 仍 fail closed 为 `RecoveryRequired`。

这次 durable shape 硬切换将协议更新为 Run API v15、RuntimeEvent v22、State v28 和
exec-stream v5。v28 只保留可安全重建的 pending Start，旧 materialized Run 不通过
compatibility reader 猜测新 retry schedule。`[retry]`、`--transport-max-retries`、
`TransportRetryPolicy`、`with_retries_disabled`、production retry fingerprint 与
transport retry accounting 已删除；唯一 corrected Harness 也不再传递失效 CLI flag。
TUI/CLI/app-server 只投影 stored event；中英文状态给出 failure、retry ordinal/total、
等待秒数或停止原因。app-server sequence reconnect、same-Run resume 与上游模型请求
retry 是三个不同概念；DeepSeek 没有 event id/partial continuation 合同，因此任何已观察
content/reasoning/tool/usage/finish 的中断都不会被包装成流式断点续传。

credential-free loopback/fake-clock、root/read-only/Writer、process SIGKILL/reopen、
surface parity 与 focused 门禁闭合后，使用同一 dirty-source identity 的 release
`dse`/`dse-tui` 做了一个授权 official DeepSeek Standard Chat canary。第一次 delivery
preflight 因缺少 sibling `dse-tui` 在网络前停止（官方请求 0）；补齐同源 companion 后
一个 Pro/high 请求完成，physical started/completed/in-flight 为 `1/1/0`，
runtime retry 0，usage/cost complete，billing unknown 0。该 canary 只证明当前普通成功
路径与 accounting，没有用真实服务故障替代确定性的 retry safety matrix。

M34 没有改变上述 retry policy、backoff、protocol event 或 State truth。真实
production binary 经 loopback 注入 response-header timeout、两次 connection reset、
429 + `Retry-After`、连续 503、401 与 partial SSE close 后，证明重复缺口只在客户端
投影：plain exec 不显示 attempt/wait，stream-json 不显示逐次失败，TUI terminal 又会
覆盖停止原因。当前 CLI 将 transient progress 写入 stderr、模型输出保持 stdout；TUI
使用同一双语 category/ordinal/wait，在窄终端保留 typed warning，并把最终停止原因写入
history。exec-stream v6 新增 bounded `model_request_failed` 投影，只携带 stored
failure、retry decision 与紧凑 accounting，不复制完整 system prompt、transcript 或
tool catalog。app-server 继续原样投影 canonical stored event；sequence reconnect、
same-Run resume 与 upstream retry 仍是三个不同机制。

M35 用 official DeepSeek current `deepseek-v4-pro/high` 对这条 production 链执行了
24 个 fresh Git/HOME/State/RunStore Runs：plain、read、grep→read、read→edit 各 6 次，
24/24 verified，false success 0；physical started/completed/in-flight=`54/54/0`，
model failure/Runtime retry=`0/0`，usage/cost complete 24/24，billing unknown 0，
credential-free SQLite reopen 24/24 exact。TTFR median/p95 为 2,020/2,380ms，wall
median/p95 为 4,796/7,428ms，已知费用 `$0.014071409`。工具任务的多 physical request
是正常 tool loop，不是 retry。

没有 live failure 跨 profile/round 重复，所以 current production retry policy、1s/2s
backoff、exec-stream v6 和客户端 projection 均不改变。M35 temporary Harness consumer
已删除并恢复到 M35 前 exact blob；frozen contract/admission、ignored `0600` journals
与 [M35 summary](../../eval/summaries/m35-official-reliability-soak-2026-07-27.md)
保留。第一次 observer argv 错误在网络前停止并单独留痕，修正后 formal 从新 identity/
position 1 开始，没有续写或拼接。

M36-A official acquisition 已从 immutable `03b40abe6dcc` binary、20 个 fresh acceptance
identity 和 position 1 完成。20/20 canonical Store/reopen/verifier observations 闭合：
16 个正向 verified success、3 个正确安全拒绝、1 个 Writer verified product failure，
false success=0；accounting 20/20 complete，known cost `$0.278444080`。三个 long-horizon
SIGKILL/reopen、service/API/UI、两个 read-only child 和第二个 Writer 均通过。

唯一 canonical loss 为
`writer_envelope -> orchestrator:writer_integration`，没有跨两个独立 task_id 重复，所以
决定为 `keep_current_harness_no_repeated_loss`，production crate delta=0。frozen raw
final summary 的 incomplete 值来自 eval-only aggregate 把 positive lane validity 错当
measurement completeness；绑定双 Harness hash 的 credential-free analysis 对同一
132-record `0600` journal 两次产生 byte-identical 20/20 truth report，没有修改 raw 或
重调 API。temporary M36 Harness consumer 已删除并恢复 pre-M36 exact blob；只保留 frozen
contract/admission/analysis/raw、summary 与 Git 历史。production-compiled crate 行为无
delta；最终 gate 只把一个 `#[cfg(test)]` 五回合 loopback terminal timeout 从 5 秒调为
bounded 15 秒，避免并行 focused 在正常完成前失败。

M37-A 没有改变任何 model-visible bytes 或 production execution。`crates/context`
原有显式 measurement ledger 已原位扩展为 canonical projection audit：每个 ordered
fragment 记录 source、owner、authority、trust、stability、hash、bytes、estimated
tokens、payload/duplicate relation、model visibility 和可选 tool-schema claim；ledger
同时派生最终 DeepSeek 单 system-message identity、stable-prefix boundary 与 bundled core
no-growth gate。普通 `production_system_prompt` 仍只返回相同 `SystemPrompt`，ledger
不进入 RunStore、RuntimeEvent 或模型上下文。

current core 仍为 3,300 bytes。离线 fixture 冻结 no-AGENTS、rules/skills、
small/medium/large、显式 override 和 source ordering；actual catalog audit 证明 root 与
read-only child 的 read-only claim 匹配，Writer coordinator 因 schema 同时公开
`isolated_write` 而 mismatch，explicit Writer 没有 `agent`。no-AGENTS fallback overview
与 project pack 被稳定标记为 same-payload wrapper。root/read-only/Writer 的 prompt
provenance、完整 ModelRequest/RequestPlan 和 SQLite reopen 精确一致。

实现 checkpoint 为 `5fb2bc971`。它没有修改 Constitution/output/language、execution
posture、project pack、DeepSeek model/request、tool catalog、permission、Runtime、
RunStore、TUI 或 Writer state machine；Key 未读取，official API 请求为 0。M37-B/C/D/E
仍需独立单变量准入，M36-A 的单个 Writer loss 仍归 M36-A2。

M37-B 随后只为 formal A/B 临时增加同 binary guarded selector：control 保持 current
execution posture，treatment 只删除 60 UTF-8 bytes 的 stale read-only-only `agent`
claim。immutable candidate `78d8ddb2c` 的 official Pro/high campaign 在 12 个完整 arm 后
因第 13 个 control arm 的 pre-header `deepseek_transport` 留下
`billing_unknown_attempts=1` 而 fail closed。该 arm 的 durable 一秒 Runtime retry、最终
completion、SQLite reopen、verifier 和已收到 usage 均闭合，但无法证明失败物理 attempt
是否计费，所以不能形成 30-arm product aggregate。

current production 已恢复 M37-A checkpoint 的原 posture；M37-B environment selector、
alternate composer branch、Harness campaign/aggregate 和专属测试没有 production
consumer，均已物理删除。Run API 15、RuntimeEvent 22、State schema 28、exec-stream 6、
DeepSeek sender、tool catalog、permission、actor route、Runtime 和 RunStore 无变化。frozen
contract/admission、ignored `0600` raw hash 与
[M37-B summary](../../eval/summaries/m37-b-posture-schema-ab-2026-07-27.md)只记录历史事实，
不提供 continuation 或第二 prompt truth。

M37-C 随后在 candidate `241941733` 上临时增加 exact-payload-digest guarded
single-projection 分支，并冻结 36-arm fresh Pro/high A/B。全部离线门禁通过；formal
position 1 已到达合法 `request_user_input` 连续性 checkpoint，但专属 Harness consumer
沿用旧的非 M30 approval-only 判定而写入
`abort(hardness_user_input_not_admitted, completed_arms=0)`。由于没有任何 complete arm、闭合
accounting 或 summary，且合同禁止 rerun，candidate 按 incomplete-evidence 门删除。

current production 因此仍无 M37-C selector/alternate projection，fallback overview 与默认
Project Context Pack 继续双重投影同 payload。M37-C Harness campaign/aggregate/专属测试也无
consumer 并已删除；只保留 frozen fixture/contract/admission、ignored `0600` raw hash 与
[M37-C summary](../../eval/summaries/m37-c-context-dedup-ab-2026-07-27.md)。Run API 15、
RuntimeEvent 22、State schema 28、exec-stream 6、DeepSeek sender、Runtime/RunStore、工具、
权限和 Writer behavior 均未改变。M37-D/E 没有由该 incomplete result 准入。

M38 随后只修复 corrected Harness 的 observer truth。pending interaction 现在从 canonical
`UserInteractionPrompt` kind/payload 派生，并验证 typed response compatibility；campaign
名称不再决定 approval 或 `request_user_input`。root 的两类 interactive request 可被正确
投影，read-only child 与 explicit Writer 继续按非交互 profile fail closed。

当 observer 在 durable RunStore checkpoint 后拒绝 trajectory 时，Harness 先追加
hash-chained `observer_abort_snapshot`，保存 event-prefix identity、terminal boundary、
physical request、runtime retry、known usage/cost 和 accounting status，再写 abort；raw model
content/reasoning/tool arguments/credential 不进入 snapshot。14-case fixture、四个 SIGKILL
窗口、exact reopen、partial-tail/tamper rejection 和 M9-C/M30 byte-identical report 已离线
闭合。true pre-header provider billing ambiguity 仍是 `billing_unknown`，没有被本地 projection
伪造为已闭合。

M38 没有修改 production crate、Prompt、DeepSeek request、RuntimeEvent、RunStore、tools、
permission 或 Writer behavior；Run API 15、RuntimeEvent 22、State v28、exec-stream v6 保持
不变。M37-C frozen evidence 没有读取、修改、续跑或拼接；Key 未读取，official API 请求为 0。

M36-A2 已从 immutable `a0847c5c1c04` binary 完成 3 个 fresh explicit Writer control-only
arms。Python retry-accounting 与 TypeScript fixed-route task 完成 isolated Writer seal、root
integration、latest-revision receipt、external verifier 和 cleanup；Rust bounded-header task
因 agent call/cardinality 未形成 sealed diff，Host 正确 blocked 且没有 false completion。
三臂 behavior/accounting 均闭合，false success=0；唯一
`orchestrator:writer_integration` 只覆盖一个 fresh task_id，低于两个独立 task 的 frozen
门槛。结果为 `keep_current_harness_no_repeated_loss`，production crate/Runtime/RunStore/
Prompt/tools/permission/Writer behavior delta=0，也没有准入 treatment 或 A/B。

M36-A2 temporary Harness consumer 已删除并恢复到 pre-campaign exact blob `d3916654f`；只
保留 frozen manifest/admission/fixture/reference、ignored `0600` raw 与 summary。历史 M36
raw 没有作为输入。ADR-0015 仍是 implementation-not-admitted，当前没有 browser/search/
vision production 路径。

M39-A 随后从 immutable `4f93060fe2ee` binary 完成六个 fresh current pack-on
control-only tasks。四个正向任务 verified success，explicit Writer 因 child 未验证、无
seal/integration/latest receipt 而被 Host 正确 blocked，一个 no-tool 安全反例正确拒绝；
`false_success=0`，behavior/accounting 6/6 complete，known cost `$0.066798629`。

唯一 canonical loss 为
`writer_record_migration -> orchestrator:writer_integration`，只覆盖一个 fresh task_id；
没有 `context:localization`，所以没有 production treatment。frozen raw 的错误
`complete=false` 只来自 eval aggregate 把已闭合 Writer lane failure 误作 measurement
incomplete；credential-free report 复用同一 M39 owner projection，对 exact raw 两次生成
byte-identical 6/6 truth，没有修改 raw 或重调 API。

M39 campaign selector、loader、live caller、aggregate/self-test 与 trajectory consumer 已
删除，唯一 corrected Harness 恢复 exact blob `d3916654f`。production Prompt、Project
Context Pack、DeepSeek request、RuntimeEvent、RunStore、tools、permission、actor route、
Writer behavior、CLI/TUI/app-server 均无 delta。frozen contract/live admission/analysis、
fixture/reference、ignored `0600` raw、summary 与 Git 历史保留。ADR-0015 仍为
implementation-not-admitted，当前没有 browser/search/vision production 路径。

M40-A 从 immutable `e2ed02c3a805` binary 启动新的八任务 engineering-loss schedule，但按
冻结停止门只运行 position-1 `rust_feature_matrix`。该 workspace 的 deterministic verifier
通过；第七个 physical DeepSeek request 在 headers 和 reasoning 后、usage/finish/`[DONE]`
前断流。Runtime 记录 `deepseek_transport`、`retryable=true`、`actionable_output=true`、
`retry_safe=false` 并停止，没有盲发第八次；Run terminal failed、latest receipt 缺失、
`false_success=0`。

canonical Store/reopen 一致，accounting 记录 7 started/completed、6 usage responses、1
incomplete response、runtime retry 0。已返回 usage 的局部 known cost 为 `$0.010289171`，但
最后请求没有 provider usage，因而 `usage_incomplete=true`、accounting complete 0/1、full
utility 0/1。Harness 在第二 arm 前以 `accounting_incomplete` 停止；只读报告得到
`reject_incomplete_acquisition`，production delta=0，没有 candidate、treatment 或 A/B。

M40 temporary selector/loader/live caller/aggregate/self-test/trajectory consumer 已删除，唯一
Harness 恢复 exact blob `d3916654f`。frozen fixture/reference、contract/admission/analysis、
ignored `0600` raw、summary 与 Git 历史保留。ADR-0015 仍是
implementation-not-admitted，当前没有 browser/search/vision production 路径。

## 8. 明确非结论

当前源码不证明：

- M37-A/B/C 已证明删除 execution posture 句子或 project context pack 会提升 verified
  success、Token、cache、时间或费用；它只冻结两项 current debt 和零行为变化 audit。
  M37-B/C 的 incomplete formal attempts 也没有证明收益，不能从 partial/raw 进入
  production treatment；

- M36-A 的 16/17 positive pass@1 可外推为所有仓库、语言或公开 benchmark，或一个
  `writer_envelope` loss 已证明 Writer owner 需要 treatment；M36-A2 fresh set 中同一
  `orchestrator:writer_integration` 只出现一次，且合同禁止把 M36-A 历史单例拼入 fresh
  threshold，因此 M36-B/C 未准入；

- M35 已测得 official timeout/429/5xx/partial failure incidence 或 live recovery rate；
  24 个独立 Run 没有触发模型故障，只证明当次成功路径、latency、accounting 与 reopen
  闭合，retry safety 仍由 M33/M34 deterministic fault matrix 证明；

- M29 的 credential-free deterministic workflow pass 等于 official DeepSeek coding
  quality、Token、cache、费用、模型 wall-time、远端 CI 或公开发布；它只证明当前本地
  production composition 没有复现合同内的 workflow blocker；

- M25 的 guide bytes/规则行位下降等于真实 DeepSeek Token、cache、wall-time、费用或
  verified-success 提升，或任一超大 Rust 文件/依赖边需要重构；

- M24 的本地 locked/offline release gate 等于远端 CI、公开发布、真实 DeepSeek 质量、
  M23 Hardness baseline 或新的 verified-success/成本结论；它只把 current exact source
  重新绑定到现有本地 V1 release contract；

- M22 的 loopback 同 chunk 收敛率等于所有真实 DeepSeek/代理/OS 网络分片，或该本地
  event/SQLite/客户端效率收益已经提高真实编码任务 verified success；它只保留不等待
  下一 chunk 的 deterministic transport cutover；
- M21 已识别是 DeepSeek provider、代理、OS 或其他网络组件关闭了 M20B response body；
  它只排除当前 deterministic local owner mismatch 和 idle timeout；
- M20 的 3/3 account reachability 等于 Chat inference、Agent completion 或单 request
  billing 可证；M20B 只有一个完整 arm，不能形成 9-arm quality/cost baseline；
- M18 已建立完整 18-arm current reliability aggregate，或一个 TypeScript verifier loss
  已跨独立 task 重复；15 条完整轨迹之外的 Writer deadline arm 只是测量中断，不能被
  补算为 product outcome、verified failure 或计费事实；
- M19 已建立 fixed-Pro coding baseline、证明 TypeScript/Writer product loss、费用或改进；
  首请求的 billing_unknown measurement interruption 不能形成这些结论；
- hard-limit compaction 已证明节省成本、缩短时间或提高任务成功率；正式 A/B 只支持其
  可靠性保留，不支持这些效率结论；
- 中文 Agent prompt A/B 已获得收益；M8-C 只完成 fixed `zh-Hans` Host 产品界面和
  machine/raw 非翻译边界，不改变模型可见 system prompt；Linux source release build
  仍只由 CI matrix 拥有而非本机观察；
- M8-D candidate 已通过完整、计费可证明的正式 A/B；final v5 在首 arm 因 unknown billing
  停止，v1-v4 的不完整 evaluator attempts 不得拼接为产品指标，production prompt 未切换；
- M8-M successor 已证明 candidate 更好/更差或等价；formal 只完成 5 个
  measurement-valid arms，第 6 arm 因无 response/usage 的 unknown billing 停止，旧/新
  samples 均不得补 mate、续跑或拼接；
- M8-E 的 offline conformance 和双 binary artifact 单独已经使 V1 可发布；其 frozen
  matrix 保持 8 blocked，只有后续 M8-G/J/K/L/N successor 才逐项关闭 current blockers；
- 旧模型 alias 退役等于 ChatCompletions surface 退役；官方文档与当前 production
  contract 都证明这是两个不同层次；
- 当前中文 Agent prompt 已获得能力提升；首个正式 A/B 及后续 v2/v3 收敛 canary 均未通过，
  v3 的 multi child 两次用满 4 轮并把成功率降为 `1/3`，见
  [正式 A/B](../../eval/summaries/prompt-chinese-ab-2026-07-18.md) 和
  [收敛 canary](../../eval/summaries/prompt-convergence-canaries-2026-07-18.md)；
- RepoGraph、多 Writer 并发、通用 DAG 或自动冲突修复已完成；M8-K 只删除没有证据
  支撑的 V1 implementation literals，并未作这些实现的质量结论；
- M5-A 只在一个固定 Python 编码任务和一个伪完成反例上证明 false-success 下降，尚未证明
  所有真实项目的假成功归零或获得通用 Token/时间收益；
- eager join 已在广泛任务上提高 multi verified success、降低 Token/费用或缩短时间；
- M6-A 单 Writer 相对当前 single-agent 已证明净收益；M6-B1 只观察到 verified
  `2/18 -> 4/18`，但 false-success、安全和开销硬门槛失败；
- transport 迁移本身提升了真实编码成功率；
- M7-A verifier 单 owner 与 typed recovery 已证明提高完整正式任务集的 verified success 或
  效率；A2 虽修复 v1 canonical JSON 口径并观察到 T1–T3 `0/3 -> 3/3`，但正式 suite 因
  incomplete response 只执行 7/40 arms，T4 mate、T5 和 read-only child 均无本次 live 证据；
- M7-B 已证明 Strict 提高或降低 verified success、参数正确率、Token、时间或费用；当前
  production actor 没有真实 Strict treatment surface，因此本阶段没有执行产品 A/B；
- M7-C 已证明真实 DeepSeek 的编辑成功率、恢复率、Token、时间或费用改善，或 FIM 相对
  patch/edit 更好或更差；3/12 -> 12/12 只覆盖冻结的 deterministic Host 反例；
- M8-H 已证明 FIM 质量较差或永不应实现；它只证明旧 production 半分支无 caller、sender、
  parser/apply/reopen 闭环，当前没有可归因 treatment；
- M8-I 已证明 read-only Flash child 相对 fixed Pro 质量非劣或成本/时间稳定改善约 20%；
  M9-A 虽产生 17 个完整成功 arms，但第 18 arm 因 unknown billing 停止，27-arm matrix
  不完整；6 个描述性 Auto/Pro pairs 的 cost/wall 改善也只有约 13.0%/10.8%，Auto 不是
  默认；M9-D 的 Auto 删除是用户接受的范围决策，不把该不完整历史改写为收益结论；
- M9-B 已建立可用于 release regression 的完整 fixed-Pro 18-arm baseline；v1 只产生
  2 个完整 arm results，第 2 个又因 read-only/Writer observer 分类错误而
  measurement-ineligible，第 3 arm 没有 terminal/accounting snapshot。修正重放只能
  证明 observer defect，不能补算、续跑或拼接为 baseline；
- M9-C 已建立可用于 release regression 的完整 fixed-Pro 18-arm baseline；successor
  只有 8 个 measurement-valid arm results，第 9 个 scheduled arm 因 response 前
  transport failure 形成 `billing_unknown=true` 并按规则停止。第一轮 6/6、八个完整
  observations 和 false success 0 都不能替代 18-arm aggregate，也不能准入 Auto 或
  触发旧 runner 删除；
- M7-E 已证明 reasoning-off 提高或保持完整任务集的 verified success，或稳定降低 Token、
  请求、wall time 和费用；v1-v4 的 18 个已完成 arms 因 evaluator/fairness 失效而不可作为
  产品指标，v4 active arm 的最终 billing 也未知；
- M7-F 已证明重放旧 Host facts 能提高 cache hit、降低费用或保持 verified success；
  M7-E raw 的 71.08% aggregate hit ratio 没有逐请求 break 或同 binary treatment 身份；
- M7-G 已证明两个 read-only child 比 single root 更快、更便宜或更可靠；正式矩阵没有
  产生可用 arm，首个联网 arm 的 billing 也未落盘；
- M7-G2 observer durability 已产生新的 fan-out product candidate 或允许补算/重跑 M7-G；
  它只证明未来 evaluator 必须先保存 Store truth，paid successor 仍不准入；
- M7-H 已证明当前 Agent 在一般编码任务上减少模型请求、reasoning/replay Token、时间或
  费用；它只修复一个 terminal no-tools hard-limit admission 反例，历史矩阵也没有
  compaction/hard-budget 样本；
- M7-I 已证明 near-limit 路径在一般任务中的出现频率或效率收益；它只证明 current v16/v21
  的 actual catalog、context estimate、compaction/request ordering、预算失败与 reopen
  correctness 可复现，没有 production treatment 或产品指标；
- 单次 live canary 可以成为产品指标。

这些能力只能按 ROADMAP 的后续切片实现，并按 EVALUATION 的同任务、同预算、重复 A/B
决定保留或删除。

<a id="current-development-authority"></a>
## 9. Development authority 与验证入口

M44 clean checkpoint 之后，repository guidance 由 ADR-0016 约束。根 `AGENTS.md` 只要求完整
读取 Product Plan、Roadmap 当前窗口和一个 owner route 指定的 ADR/current-fact section；
Evaluation 仅在修改评测合同或作 keep/delete 判断时读取。`docs/README.md` 是机械路由地图，
不是第二份 Roadmap 或架构真相。

`scripts/check-public-repository.py --authority-only` 冻结 M44 的 17,636 行 unconditional
bootstrap 基线、六个预注册问题、十一条 owner route、accepted ADR 可达性和实际读取集合。
`scripts/dev-dse.sh` 是唯一 executable gate owner；根 guide 只保存 Risk 0～4 分类，不重复
full-gate 命令。该切片不修改 production Rust、DeepSeek wire/Prompt、RuntimeEvent 或 Store。

首个 checkpoint 实测 repository-guidance route 为 1,024 行、worst-case tools route 为 1,755
行；加入下述排序复核后当前值为 1,044 / 1,758 行，仍低于 4,409 行 hard ceiling，固定边界
22/22 可达。旧 unconditional full-read 和根 guide 的重复 command list 已删除。Risk 0 gate 与
首个 checkpoint 唯一一次 pre-integration full gate 均通过。

ADR-0016 排序后的 continuation 复核没有发现可准入的 current loss：M12 current loss set 为空，
M13 是 inadmissible evaluator-contract instability，M36-A 三个独立 long-horizon task exact
SIGKILL/reopen `3/3`。observed/required 为 `0/2`，所以 current production 不存在
`VerifiedMilestoneProjection`、milestone Store 字段、第二 progress truth 或对应 caller。

仓库现有 `scripts/eval-adr0016-harness-isolation.py` 只是 credential-free offline evaluator，
不是 production Agent runtime。它冻结 same `deepseek-v4-pro/high`/Standard Chat 的结构比较：
minimal loop 的四个 comparison concepts 对 DSE current 的十个；DSE 增加的六个概念是
TaskContract、canonical RuntimeEvent、RunStore、typed authorization、latest-revision Host
receipt/completion 与 Writer worktree lifecycle。七项 capability contract 中 DSE current 为
`7/7`，minimal definition 为 `1/7`；前者由六条 exact Rust owner tests 和 M36-A long-horizon
`3/3` 支撑。该 inventory 不是 live minimal implementation 或质量/成本排名。

该 checkpoint 的决定为 `keep_current_harness_reject_verified_milestone_no_repeated_loss`，当时
production Rust、DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 均无
delta 且 official requests=0。此后 M45-A 与 M46 W2/W3/W3.1 已完成；不能继续把旧 checkpoint 的
“未启动”当成 current fact。

ADR-0018 现已进入 accepted decision index。它保留 bounded authority/risk-tier gate，同时把后续
product baseline 从 per-action repeated-loss audit 切到 capability cluster；Risk 0 本切片没有
production Rust、DeepSeek wire/Prompt、Runtime/Event/Store、catalog 或 UI delta，也没有读取 Key 或
发 official request。同一 authority checker 的 actual result 为 repository-guidance=`1,360` 行、
worst-case tools=`2,639/4,409` 行、fixed boundary=`24/24`，链接与 authority contract 通过。
