# Current CodeWhale Architecture

> 文档类别：迁移事实。只描述当前源码，不替代
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md)、
> [ROADMAP.md](../product/ROADMAP.md) 或 ADR。

- 快照日期：2026-07-24
- 导入基线：`352e86a611fdf3cd8bd27c36d24d482c06a71117`
- workspace version：`0.8.68`
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
- 当前协议：Run API v11、RuntimeEvent v17、State schema v23、exec-stream v3。产品默认
  仍为固定 `deepseek-v4-pro`；Auto 未经正式质量/效率 A/B 不会成为默认。

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
推断已被替代。当前 RuntimeEvent v17 与 State schema v23 继续持久化 v16/v21 引入的
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

Headless `exec --auto` 现在只控制工具启用与自动批准，不再同时设置 `trust_mode`；因此 Fleet
worker 固定使用该参数也不会仅凭 `--auto` 获得任意工作区外路径访问。canonical API 的显式
`trust_mode`、当前显式 yolo 输入、workspace-scoped trusted roots 与持久 Run 恢复仍保持原
owner。surface parity 已验证真实 exec request 为 `auto_approve=true`、`trust_mode=false`。

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

## 2. 已统一的生产链

### Application service

`crates/app` 是 exec 与 app-server 的唯一 application composition owner：

- 组合官方 DeepSeek connection、credential 和 HTTP client；
- 组合 production prompt；
- 组合固定工具 executor 与本地执行策略；
- 打开同一种 SQLite `RunStore`；
- 绑定 physical request budget、model accounting 和 execution fingerprint；
- 维护轻量 process-local active control registry；
- 实现 start、continue、list_roots、get、events、resume、steer、interrupt、
  cancel、resolve_interaction；
- start/continue 通过 State schema v21 中保留的 durable creation reservation 先绑定
  `request_id + command digest` 与唯一 reserved run ID；
- control command 只有在对应 `SteerQueued`、`ControlRequested` 或 `InteractionResolved`
  已提交到 `RunStore` 后才返回 accepted sequence；重复 `request_id` 按持久回执幂等处理。

active registry 只保存当前进程可投递的 control handle，不是第二个 lifecycle 或持久事实。
run projection、event、lease 和 terminal 都从 `RunStore` 读取。

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

当前 RuntimeEvent v17 继续保留 v6 将逻辑模型请求预算和物理 API 请求预算分开的语义：
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
post-integration verification 都是当前 RuntimeEvent v17 / State v23 的 canonical facts。
Orchestrator 不定义私有事件总线、JSON ledger、模型循环、DeepSeek transport、工具实现或
完成判定。Writer receipt 只是 child artifact；只有集成后绑定最新 root revision 的
EvidenceReceipt 可以满足 root TaskContract。

当前明确 fail closed：dirty/non-Git/unborn/不受支持 Git 仓库、base 漂移、branch CAS
变化、allowed path 越界、空 diff、缺失 artifact、verifier 失败和恢复歧义。M6-A 未实现
多 Writer、通用 DAG、脏工作区快照、自动冲突修复或远程 worker。

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

产品 `model=auto` policy 不在 backend。唯一 owner 是
`crates/app::ProductionModelRoutePolicy`：显式模型/reasoning 原样保留；Auto root 与
explicit isolated Writer 使用 Pro，普通 read-only child 可使用 Flash，typed
recheck/rework 使用 Pro；Auto reasoning 通常为 high，只有 typed recovery facts 才提升
为 max，永不自行选择 off。每个 child selection 先冻结进 `AgentTask`，再由同一
`AgentRuntime` 构造 exact `RunRequest`；不在一个 Run 内切换模型。

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

### Tools

`crates/tools` 拥有 production 固定工具 catalog、schema、execution identity 和 handler。
当前 11 个 Host 工具为：

```text
apply_patch  edit_file    exec_shell   file_search
git_diff     git_status   grep_files   list_dir
read_file    run_tests    run_verifiers
```

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

### State

`crates/state::StateStore` 实现 production SQLite `RunStore`：

- 当前 State 物理 schema 为 v23；RunStore 继续复用同一 event/snapshot 表，不增加
  EvidenceReceipt 私表；
- 当前 canonical RuntimeEvent writer/reader 为 v17；
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
  不能诚实反推 caller requested mode/reasoning；保留 pending Start intent，因为 Host Auto
  policy 在 `RunCreated` 前没有网络或计费副作用；
- v23 在同一 `IMMEDIATE` migration transaction 中删除 legacy `threads` metadata 表；
  fresh schema 不创建它，并保留 current canonical run、pending Start、route audit 与
  accounting；
- no-key terminal replay。

旧 `codewhale thread`、SQLite `threads` metadata 表与 `session_index.jsonl` 已删除；
没有 compatibility reader 或双写。旧 Workflow/SubAgent JSON/JSONL 写入链也已随隐藏
执行路径删除，没有迁为 `RunStore` 双写。

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
- 当前 Run API v11 直接接收结构化 `TaskDefinition`，并投影 frozen TaskContract、
  completion decision、durable creation-intent list/recover；当前 RuntimeEvent
  writer/reader 为 v17；
- crate dependency tree 不含 `crates/core` 或 `crates/tui`；
- 不启动 sibling TUI process。

完整接口见 [RUNTIME_API.md](RUNTIME_API.md)。

### Interactive TUI

交互 foreground 已切到 `AgentApplication`：

- `TuiRunClient` 提交 start/resume/continue/steer/interrupt/cancel 和 interaction
  command；
- `CanonicalRunProjection` 与 presenter 只从 `RunStore` event 投影 root/child 进度、终态和
  durable outcome；
- 旧 foreground Engine、EventBroker、runtime-thread owner、`SessionManager`、child display
  cache 和 registry-driven slash command system 已删除；
- slash command 只剩统一的 `help/cost/exit` canonical contract；
- 退役的 `crates/tui/src/compaction.rs`、`seam_manager.rs` 以及不再生效的 TUI
  `auto_compact` 开关/阈值状态均已删除；hard-limit compaction 位于
  `crates/context + crates/runtime`，不存在手动 `/compact` 或传输层 command；
- generic Provider/config/UI active path 已删除；保留的 DeepSeek provider 字段只参与
  canonical environment fingerprint / replay safety，不是用户模式或第二 backend。
- Work surface 只投影 canonical child/tool facts，不再投影已删除的 TUI 私有 Plan/Todo、
  Goal/Hunt 或 custom-command pause 状态；canonical Run 的工具 allow-list 不从旧 UI
  状态注入。

因此三个保留 foreground 入口与所有生产可达根/只读子/Writer Agent 模型循环已经统一；
M6-A 门禁确认 Writer lifecycle 也不引入第二条执行链。

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
| `tui` | exec/interactive canonical Run projection | 不恢复 Provider/thread 私有状态 |
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
  折叠为直接的 `HistoryCell::Tool(GenericToolCell)`。固定 11 工具现在只走这一展示模型，
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
  结构化提问仍由 canonical interaction 承担，Run 级 `allow_sandbox_elevation` 策略保留。
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
  `auto`、`deepseek-v4-pro`、`deepseek-v4-flash` 三种模型投影；失败发生在 raw terminal、
  RunStore 和 HTTP 之前。`run_tui` 会在终端初始化前再次核对 `TuiOptions.model` 与配置投影，
  `App::new` 不再允许持久 Settings 覆盖 provider/model。旧启动后 Provider 强制改写和 App
  私有 `provider_models` 状态已经删除。应用层也会在持久 creation reservation 前校验显式
  模型，因此 CLI、TUI 和本地 API 不会为非法模型留下 pending creation。交互入口读取
  provider-scoped/root 的原始显式模型并先行校验，不能再经通用默认解析把外国模型静默
  回落成 V4 Pro；确实没有配置时才采用官方默认。`auto` 仍走官方
  DeepSeek production planner，onboarding 仍可写入官方 Key；通用 Provider 配置 schema 尚待
  M7 删除，Beta FIM transport 也没有被本切片实现或替代。当前 focused、App 38/38（另 1 个
  外部进程 helper 忽略）、canonical Run 19/19、canonical PTY 7/7、TUI bin 1,537/1,537
  （另 1 个忽略）、通用 PTY 9/9、app/TUI all-target check、fmt 和 diff-check 均通过。
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
- 旧 `workspace-trust.json` 外部路径快照的 `add/remove` 及原子写入只有自身测试调用，
  现已物理删除。保留的 production 路径只用 `WorkspaceTrust::load_for` 读取已有文件，
  再把 canonical paths 交给 `ProductionToolConfig`；`permits` 只保留为读取契约测试。这与
  onboarding/MCP 仍在真实读写的 `[projects].trust_level` 是两条独立路径，后者及
  approval、Runtime 权限、project trust 均未改变；既有用户 trust 文件不会被自动清理。
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
  删除；旧 `default_mode` 现在被忽略且不能授予权限。真实显式 `--yolo` 输入仍直接投影为
  shell、自动批准和工作区外访问控制，不经过模式标签。`ApprovalRequest` 不再重复保存无人
  读取的英文 impact 列表；保留的简体中文 `impacts()` 只是 TUI 展示摘要，canonical risk
  仍只来自 `crates/tools`。
- TUI 审批状态现在只有 `Ask/AutoApprove`，分别精确投影 canonical
  `auto_approve=false/true`；footer/header 显示“需要审批/自动批准”。持久配置唯一 owner 是
  `Config.approval_policy`，只接受 `on-request|auto`。旧 `Settings.permission_posture`、兼容别名、
  managed-lock UI 镜像和 saved-posture project baseline 已删除；这两个审批状态不改变
  `trust_mode`、Shell catalog、sandbox、execpolicy deny 或 durable RunStore 语义。
- 旧 TUI `RetryPolicy::delay_for_attempt` 和 `Config::search_provider` facade 没有 caller，现已
  删除；生产 DeepSeek retry projection 与 Doctor 的 typed search-provider resolution 保留。
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
之前 fail closed。当前默认 `Auto` / thinking-enabled 没有改变。

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
DeepSeek usage/cost，不选择模型或路由；`[search].provider` 只选择 retrieval adapter。
MCP OAuth 只认证 MCP transport，不认证模型后端。

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

M8-I 当前 production route 为：

```text
Start/Continue typed facts
  -> AgentApplication::ProductionModelRoutePolicy
  -> RunRequest(model, reasoning, ModelRouteAudit)
  -> AgentRuntime
       -> read-only AgentTask(Flash/High)
       -> failed read-only recheck AgentTask(Pro/Max)
       -> explicit Writer AgentTask(Pro/High or typed rework Max)
  -> DeepSeekModelPort -> ChatCompletions
  -> RunStore(State v23)
```

Auto root 始终 Pro；产品没有 typed bounded-low-risk/no-tools 输入，因此当前不授予 root
Flash。read-only child 的 Flash 结果必须返回 Pro root 汇聚，并由既有 Host evidence gate
验收；failed handoff 只允许启动新的 Pro child 重查，不在原 Run 中偷偷切模型。显式
`deepseek-v4-pro`、`deepseek-v4-flash` 和显式 reasoning 完全绕过 Auto policy。

旧 `crates/deepseek/src/auto_route.rs`、额外 non-streaming Flash 请求、128-token
classifier prompt/parser、provider DTO、关键词/500 字 heuristic、空 `recent_context`、
`DeepSeekAutoRouteFailed` 和 creation unknown-billing projection 已物理删除。Auto Start
的 `RunCreated.accounting_baseline` 现在是 0 次请求，首个且唯一物理请求属于 root。
RuntimeEvent v17 与当前 State v23 强制 route audit 与 child binding；该 audit 由 v22
引入，v23 只删除 legacy Thread truth，SQLite 重开不重新路由。

formal A/B 没有准入：frozen old classifier control 在 `15fea38e`，candidate 在
`ef65bafa`，而删除旧路径后不存在同时承载 fixed Pro/Flash、旧 classifier 与 Host policy
的同 revision/immutable binary。为跑满矩阵恢复 production classifier toggle 会重新制造
已删除的模式和第二 route owner。因此结果为
`inadmissible_no_single_binary_four_variant_surface`；Key 未读取、官方请求 0。产品默认保持
固定 Pro，显式 Auto 机制保留但默认 admission 为 hold。

M8-J 当前 state / CLI surface 为：

```text
codewhale runs | resume | exec --resume/--continue
  -> AgentApplication
  -> canonical RunStore(State v23)

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
raw 只作为停止证据。V15 仍 blocked。完整事实见
[M8-M billing-provable 中文 Agent prompt successor](../../eval/summaries/m8-m-billing-provable-zh-prompt-successor-2026-07-24.md)。

## 7. 明确非结论

当前源码不证明：

- hard-limit compaction 已证明节省成本、缩短时间或提高任务成功率；正式 A/B 只支持其
  可靠性保留，不支持这些效率结论；
- 中文 Agent prompt A/B 已获得收益；M8-C 只完成 fixed `zh-Hans` Host 产品界面和
  machine/raw 非翻译边界，不改变模型可见 system prompt；Linux source release build
  仍只由 CI matrix 拥有而非本机观察；
- M8-D candidate 已通过完整、计费可证明的正式 A/B；final v5 在首 arm 因 unknown billing
  停止，v1-v4 的不完整 evaluator attempts 不得拼接为产品指标，production prompt 未切换；
- M8-M successor 已关闭 V15 或证明 candidate 更好/更差；formal 只完成 5 个
  measurement-valid arms，第 6 arm 因无 response/usage 的 unknown billing 停止，旧/新
  samples 均不得补 mate、续跑或拼接；
- M8-E 的 offline conformance 和双 binary artifact 已使 V1 可发布；其 frozen matrix
  保持 8 blocked，M8-G/J/K successor 已把 current matrix 收敛为 3 blocked，但仍不可发布；
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
  当前只有 request-count、replay 和安全边界的 mechanism evidence，没有 live product
  metric，Auto 不是默认；
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
