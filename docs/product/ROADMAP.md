# DSE 开发路线图

> 文档类别：产品权威。仅定义实施顺序、迁移和删除点。

- 状态：执行中
- 当前阶段：M4 已关闭；M5-A canonical `TaskContract`/`EvidenceReceipt` 与 M5-B
  evidence-aware ContextBroker 均已完成正式 DeepSeek A/B。M5-B 结论为 `shrink`：
  保留硬上限安全压缩，删除手动和提前阈值压缩。M6-A 单 Writer isolated worktree
  垂直闭环已完成。M6-B1 v2 的 18 对 / 36 arms 正式 A/B 判定
  `reject_and_rework`；rework 已完成 verifier 卫生、显式 Writer admission、Host actor
  权限、named verifier、时序 EvidenceReceipt、精确 cleanup/recovery 和 Harness v3 冻结。
  rework candidate `3310aa73` 的正式 v3 在第 3 个 arm 出现一次无法证明计费的
  `deepseek_transport` 后按预注册规则立即停止，决策为 `hold_mechanism`、不具备产品指标
  资格。它不推翻 v2 的完整产品结论：Writer 继续 explicit-only，M6-B2 不准入；
  M5-C RepoGraph 因无缺失检索证据继续延后。M4 最终代码检查点为 `65fa88ba`；
  M5-B 收缩检查点为 `e2c870b0`；M6-A 代码与真实 canary 检查点为 `a982a9a8`。
  M6-B1 v2 candidate 为 `5d72ae94`，结果为 `reject_and_rework`；rework v3
  candidate 为 `3310aa73`，结果为 `hold_mechanism`。M7-A candidate `24c8a530`
  已完成 verifier contract 单 owner 与 typed completion recovery；正式 v1 因
  canonical JSON 评测口径失效判定 `hold`。M7-A2 已修复唯一 canonical JSON owner，建立
  公平 shared-fix control/treatment 并重新冻结；正式 suite 在第 7/40 arm 遇到一个真实
  incomplete response/accounting 后按预注册规则停止，仍为 `hold`、不具备产品指标资格。
  M7-A3 已在 `e98ca5ae` 建立不完整 stream 的 typed evidence、增量 usage accounting 与
  replay-safe retry；完整离线门禁通过。旧 control/treatment 无法同时承载字节等价修复并
  保持原 production delta，因此 formal 复评在读取 Key 前判定 `inadmissible`，M7-A 产品
  结论继续 `hold`。M7-B 已完成 canonical Strict 目录判定与 typed 工具失败恢复；六个默认
  可执行 actor 在 Strict 候选下仍全部原子回退 Standard，因此 live A/B 判定
  `inadmissible_no_surface_delta`，未读取 Key、未调用官方 API，用户 Strict 开关已删除。
  M7-C 已冻结 12 项 canonical 编辑任务并定位 Host correctness 瓶颈：起始 tools contract
  3/12，通过 `crates/tools` 单 owner 修复 stale/ambiguous/no-op/重复目标/rollback/mode 后
  12/12；真实 production loopback、Writer、SIGKILL/reopen 与全 workspace 门禁通过。
  当前没有 canonical FIM caller 或同 binary treatment surface，因此 FIM live A/B 同样在
  credential/API 前判定 `inadmissible_no_surface_delta`；Key 未读取，FIM production 接入
  继续 `hold`。CLI direct apply 与 TUI-local eval/edit 绕行已物理删除。M7-D 又审计并
  替换了单变体观测 WIP：v16->v14 降级、历史 executor monkeypatch、错位 recovery/revision、
  无 delta live/Key 路径已删除；`cf9b3fd6` 的 v16-native Harness 14/14 clean gates 通过。
  没有新的 production 模型失败样本或 treatment delta，编辑/FIM treatment 继续 `hold`。
  M7-E 随后把已有 reasoning/replay 成本定位为首要非编辑候选，并以同一 Standard Chat
  production binary 冻结 `high`/`off` paired A/B。v1-v3 因 evaluator 错判真实 verifier
  recovery 失效；v4 又证明随机 absolute workspace 会改变 canonical workspace revision
  和实际首请求，且外部停止时存在无法重建最终计费的 active arm。四次尝试全部排除出产品
  指标；final `458c3d7d` v5 固定 `live_api_admitted=false`，production 默认不变，决策为
  `hold`。任何复评都必须使用新的 successor manifest 从 position 1 重新准入。
  M7-F 随后按 DeepSeek 最新 cache-prefix unit 规则审计 exact production wire。
  `a1d68b05` 的三轮 loopback 证明：每轮 Host facts 是 ModelRequest 最后的 ephemeral user
  message，下一轮不会把它放回历史；即使只读后 facts 完全相同，相邻公共完整 messages 仍只
  到上一请求的 `len - 1`，而 edit 后 revision 会正确刷新。该 break 真实，但把旧 facts
  写回会让过期 revision/receipt 重新进入模型输入并需要新的 typed supersession/reducer
  状态；其它拆 system、前移/删除 facts、重排工具候选均无安全可归因收益。M7-F 因而
  `hold`，production 不变，Key 未读取、官方请求 0。
  M7-G 随后确认 canonical Runtime 已在同一 assistant response 中先启动同批全部 read-only
  child、再统一 join，production 不缺第二 scheduler。冻结的 9 对 / 18 arms 同 binary
  fan-out A/B 在首个 control arm 后因 Harness 把 caller-authored verifier plan 与
  Host-resolved canonical plan 做错误全等比较而停止；Key 已读取并发生网络请求，但
  `completed_arms=0`，accounting 未进入 raw，最终费用不可证明。按 `maximum_reruns=0`
  没有续跑。产品结论为 `hold / inadmissible_observer_identity_bug`，不具备指标资格；
  production 只保留 read-only child crash/reopen truth 修复，不新增或默认启用 fan-out。
  M7-G2 随后冻结 fail-before-loss observer contract：exact terminal snapshot、无 Key SQLite
  reopen snapshot 和 verifier observation 必须先于 identity/surface/accounting/产品指标派生
  写入 `0600` hash-chain raw。11 个 exception/SIGKILL 窗口全部离线通过，旧 M7-G raw 仍为
  unknown billing 且禁止续跑/拼样。由于 `062623e6` 后没有新的 fan-out production delta，
  observer-only revision 不能成为新 candidate；paid successor 在 Key/API 前判定
  `inadmissible_no_new_production_delta`，read-only child 继续 explicit-only。下一切片转向
  Agent 请求与 Token 预算反例。
  M7-H 随后从 canonical request/Token 浪费矩阵定位并修复 terminal `tools=[]` 请求错误使用
  ordinary catalog 做 hard-limit admission 的确定性 Host 缺陷；更广泛的模型请求/Token
  优化没有可归因 treatment，继续 `hold`。M7-I 又在 current RuntimeEvent v16/State v21 上
  冻结 13 项 near-limit 矩阵与 16 个 exact gates：root、read-only child、explicit Writer
  的 actual catalog、context estimate 与 DeepSeek `RequestPlan` 均可从 SQLite reopen 的
  `ModelRequestPrepared` 精确重建，mandatory facts 超限在 0 次模型请求前 typed fail
  closed。没有发现新的 production 反例或 material delta，因此关闭 M7 request/Token 调优，
  不读取 Key、不调用 API，准入 M8 DeepSeek-only 产品清理。
  M8-A 已从 clean `8b650356` 完成三个垂直切片：`e1a611ff` 让 CLI/auth/model/config
  只接受 DeepSeek，删除 `crates/agent` 与 generic Provider/OAuth/model registry；
  `63246e72` 将交互 TUI、Doctor、onboarding 和 Fleet argv 切到同一 DeepSeek 配置，
  删除 TUI 私有 provider catalog/route/billing/OAuth 路径；`00dcda0c` 让
  `crates/config` 只保留根级 `api_key`、`base_url`、`default_text_model` 模型真相，
  并物理删除 generic Provider、catalog、pricing、alias 和 route resolver。冻结的
  P01-P12 首启、无 Key、非法 Provider/模型、credential precedence、resume/reopen、
  PTY 与 exec/HTTP/stdio parity 矩阵全部离线通过；没有 material model treatment，
  因此 Key 未读取、官方 API 请求与外部网络访问为 0（仅使用本机 loopback）。M8-A 决策是
  `keep_deepseek_only_cutover / delete_generic_provider_paths`。
  M8-B 又从 clean `2ed3efe3` 建立唯一 CodeWhale 产品/交付 owner：正式 binary set 固定为
  `codewhale` 与 `codewhale-tui`，Rust 固定为 1.97.0，source package 绑定完整 revision、
  tree、Cargo.lock、target、inner/outer SHA-256，并以 immutable release + atomic symlink
  完成本地安装、升级、回滚和保留用户数据的卸载。真实 macOS release artifact 在禁网
  sandbox 通过，Linux 同一 lifecycle 在缓存容器 `--network none` 通过；`crates/release`、
  imported updater/CNB discovery、`codew`、旧产品 env/path reader、第二 metrics truth 和
  未接线部署资产已删除。代码候选 `307f6c09` 相对 baseline 净删除 1,702 行，决策为
  `keep_single_codewhale_identity_and_delivery_owner / shrink_imported_delivery_paths`。
  本切片没有模型 treatment，Key 未读取、官方 API 请求 0；一次候选后只读 metadata
  断言因漏设 toolchain override 触发 rustup 更新探测并立即中断，不能把整个 Agent session
  表述为从未尝试外网，但冻结的交付路径仍由 OS 网络隔离证明。
  M8-C 接着从 clean `e99bf6c7` 把 TUI 私有的 167-key catalog 收敛为 CLI/TUI 共享的唯一
  427-key fixed `zh-Hans` owner，并汉化保留的 Clap help、Doctor、Headless/recovery、
  `request_user_input` 和多 Agent Host chrome。命令/flag/enum、Doctor JSON、NDJSON、
  HTTP/SSE/stdio、模型/工具 ID、路径、代码和 raw provider/tool/stdout/stderr 保持稳定；
  `crates/protocol`、`runtime`、`state`、`app-server` 相对 baseline 零差异。L01-L14、
  foreign locale、80/120 列 CJK、两次 hermetic TUI、crash/reopen 和真实 `44b17940`
  安装包回归全部通过。决策为
  `keep_single_fixed_zh_hans_owner / shrink_duplicate_product_text_paths`；没有模型
  treatment，Key 未读取、官方 API 请求 0。
  M8-D 随后冻结同一 immutable `8371b6dd` binary pair、`deepseek-v4-flash`、
  current `https://api.deepseek.com/chat/completions`、五个 single/read-only/explicit
  Writer 任务和唯一 constitution prompt treatment。真实 caller 审计发现 app-server
  没有加载既有 context-owned override；`8371b6dd` 已让 TUI/exec/app-server 收敛并以
  外部进程 + State reopen 证明。v1-v4 分别暴露无 surface、multi accounting/task、
  Writer fixture 和 Writer scope 的 evaluator 旧投影，raw 均不可覆盖、不拼样。final v5
  已离线修正 current nested AgentTask 与 integrated `base..HEAD` scope，但首个 live arm
  的 usage/billing 不可证明，按预注册门禁立即 `aborted_unknown_billing`。正式 30 arms
  未完成，production bundled prompt 未切换，结论为
  `hold_prompt_candidate / keep_app_server_override_consistency`。旧
  `deepseek-chat`/`deepseek-reasoner` model alias 未使用；官方当前 ChatCompletions API
  路径没有被旧 alias 退役替代。
  M8-E 随后从 clean `433a871b` 冻结 PRODUCT_PLAN 的 16 项 V1 exit gap：
  V01/V02/V03/V04/V05/V07/V11/V14 共 8 项通过，V06/V08/V09/V10/V12/V13/V15/V16
  共 8 项阻塞，发布结论为 `not_releasable`。`a12bea45` 的 frozen manifest/result 曾把
  “用户选择 Anthropic Messages”写成 release premise；2026-07-24 用户明确否认该选择，
  PRODUCT_PLAN 与 ADR 也从未接受它。因此 frozen 证据保留作历史审计，但 Anthropic 不再
  计入 V09 或 release gap。当前官方 DeepSeek ChatCompletions production sender 与
  PRODUCT_PLAN 的 Standard/Strict 路线一致；V09 仍只因 FIM 无 canonical
  caller/parser/apply 而 blocked。locked/offline artifact 只含 `codewhale` 与
  `codewhale-tui`，安装/验证和完整 offline conformance 通过；它证明 baseline 可复现，
  不推翻这 8 项 blocker。M8-E 没有 production model delta，Key 未读取、官方 API
  请求 0。基于错误前提冻结的 M8-F 已标记
  `canceled_invalid_premise`，未提交 Messages production WIP 已精确删除，Key 未读取、
  官方 API 请求 0。
  M8-H 随后从 clean `dce858d0` 复核 M7-C 后没有发现新的 current-v16 production 编辑
  样本：Host deterministic matrix 仍为 12/12，canonical FIM caller/parser/apply/reopen
  全为 0。既有 `plan_fim` 生成 sender endpoint owner 明确拒绝的
  `/beta/completions`，而唯一 response parser 只接受 Chat `choices[].message`，因此它不是
  可归因 treatment，而是无消费者半分支。code candidate `7d9aa9a6` 物理删除 FIM
  planner/surface/accounting、永远为 0 的 exec/eval 字段、退役模型 alias 静默映射、future
  `deepseek-*` pass-through、TUI 重复目录 switch 与永远为 `None` 的 alias retirement
  UI/JSON。exec-stream 因公共 terminal 字段删除升到 v3；Run API v10、RuntimeEvent v16、
  State v21 不变，已有 Standard/Strict run 可原样重开。决策为
  `keep Standard/Strict / reject unreachable FIM production half-branch / retain frozen and
  eval-only protocol evidence`；V09 仍因 PRODUCT_PLAN 的 FIM 完成定义未满足而 blocked。
  没有 treatment delta，Key 未读取，官方 API 请求 0。
  M8-I 随后从 clean `15fea38e` 冻结旧 Auto classifier control，并以 code candidate
  `ef65bafa` 把 `model=auto` 改为 `crates/app` 唯一 owner 的 Host typed 保守路由：
  Auto root 与 explicit Writer 用 Pro，普通 read-only child 可用 Flash，typed
  recheck/rework 用 Pro；显式 model/reasoning 不变。额外 Flash classifier 请求、
  prompt/parser/provider DTO、关键词/500 字 heuristic、空 `recent_context` 与
  pre-RunCreated unknown-billing 分支已物理删除。由于旧 classifier control 和 Host
  candidate 不存在于同 revision/immutable binary，四 variant formal A/B 在 Key 前判定
  `inadmissible_no_single_binary_four_variant_surface`；产品默认保持 fixed Pro，显式 Auto
  机制保留但默认 admission 为 `hold`。Key 未读取、官方请求 0。
  M8-J 随后从 clean `bce36a53` 重验 M8-E 的 16 项 V1 matrix：M8-G 已使 V06
  `blocked -> pass`，M8-H/M8-I 不改变 V1 item，current input 为 9 pass / 7 blocked。
  code candidate `bcbc1616` 删除断开 canonical RunStore 的 `codewhale thread`、SQLite
  `threads` 表、`session_index.jsonl` 第二真相，以及无 production consumer 的
  Thread/App/Prompt/EventFrame 协议岛；State `v22 -> v23` 只做同事务物理删除并保留
  current run replay、pending Start、route audit 与 accounting。V12 因而关闭为 pass，
  current V1 为 10 pass / 6 blocked；剩余 V08/V09/V10/V13/V15/V16。该切片没有模型
  treatment，Key 未读取、官方请求 0。
  M8-K 从 clean `49a46581` 冻结 V08/V09/V10 successor audit。正式证据不支持继续按清单
  实现 multi-Writer、FIM 或 RepoGraph：M6-B1 拒绝扩大 Writer 且当前显式单 Writer
  lifecycle 完整；M8-H 已删除无 caller 的 FIM 半分支且没有新编辑生成失败；M5-C 一直因
  没有结构检索归因样本而延后。ADR-0005 因而把 V1 定义收敛为可验证能力结果：
  `keep explicit single Writer / keep Standard+Strict / keep bounded cross-file
  retrieval`，三种优化实现继续 evidence-gated hold。current matrix 为 13 pass /
  3 blocked，剩余 V13/V15/V16；没有 production 或 model treatment，Key 未读取、
  官方请求 0。
  M8-L 随后从 clean `4fef6a34` 合并审计 V13/V16。exact imported `352e86a6` 可以使用
  同一 official ChatCompletions/model、外部 task fixture 和 deterministic verifier，
  但 exec-stream v1 的 `retry_count` 永远为 `null`，没有 physical request count、
  failed/incomplete usage、root/child ledger、cost completeness/bucket 或 crash/reopen
  request truth；其 Engine 又有两层透明重发。因此跨 revision paid A/B 在 credential 前
  判定 `inadmissible_incomplete_baseline_accounting`。ADR-0006 接受 release benchmark
  successor：保留 M5-A 12/12 合格 official DeepSeek coding/false-success evidence，
  exact current candidate 再通过 production Git/verifier/reopen regression；五个共同
  用户 workflow 的显式动作数为 `5 -> 5`。V13/V16 因而关闭，current matrix 为
  15 pass / 1 blocked；只剩 V15，V1 仍不可发布。Key 未读取、official requests 0。
  M8-M 随后从 clean `d1d6ca5c` 冻结 current fixed-Pro、same-revision immutable-binary
  中文 prompt successor。19/19 Harness、6/6 journal SIGKILL、production activation、
  focused、workspace clippy/test 和 process crash/reopen 全部离线通过。正式 30-arm
  suite 的前 5 arm 均 verified、false success 为 0、31 个 usage response 和
  `$0.042682606` known-cost lower bound 闭合；第 6 arm 的首个
  `deepseek_transport` 没有 response headers/usage，canonical accounting 因而记录
  `billing_unknown=true`。Harness 按 `maximum_reruns=0` 在 6/30 立即停止，没有续跑、
  补 mate 或拼样。结论为
  `hold_prompt_candidate / keep_bundled_prompt / do_not_resume_or_splice`；candidate-only
  evaluator path 删除，production prompt 不变。V15 仍 blocked，current matrix 保持
  15 pass / 1 blocked，V1 仍不可发布。
  M8-N 最后审计 `PRODUCT_PLAN` 6.1 与旧 V15 的边界：候选接管必须 A/B 的规则继续
  保留，但从未接管的 M8-D/M8-M treatment 不应成为固定中文基线的永久发布依赖。
  ADR-0007 接受 immutable constitution identity、M5-A 12/12 合格 official DeepSeek
  coding/false-success evidence、同 constitution 的 Writer canary、M8-M 三个
  exact-current baseline diagnostics、current production conformance 和 whole-release
  rollback 组成的 V15 successor。M8-N 没有 material model-visible treatment，live
  准入为 `inadmissible_no_material_treatment`，Key 未读取、official requests 0。
  失去消费者的 M8-D candidate-only Rust test/current-tree fixture 已删除，通用
  app-server/exec/TUI override consistency 保留。V15 关闭后 current matrix 为
  16 pass / 0 blocked，决策为 `V1 可发布`（release-ready，不代表已经 push 或发布）。
  M9-B/M9-C 的 fixed-Pro regression acquisition 因真实 transport-before-usage 的
  unknown billing 保持 incomplete；corrected Harness 保留，不能续跑或拼样。M9-D 已按
  ADR-0008 退休并删除 Auto 产品语义，先于后续 billing acquisition 独立收敛；P0 以后
  只服务 fixed-Pro 效果实验。
  当前 Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3。CLI、TUI、本地 API 与
  根/只读子 Agent/Writer 子 Agent 已统一到
  `AgentApplication -> AgentRuntime -> RunStore`；hidden Workflow、ACP、direct review、
  旧 TUI SubAgent runtime、Classic shell、第二工具/状态/模型路由 owner 和无生产消费者的
  Goal/Memory 原型均已物理删除。focused、真实 PTY、进程级 crash/replay、严格 workspace
  Clippy 与完整 workspace tests 已通过。M1 的导入基线 A/B 与 M2 的完整官方 surface
  canary 仍是独立证据债务，不因 M4 关闭而自动完成
- 上次更新：2026-07-24

本文件是唯一执行路线。产品边界见 [PRODUCT_PLAN.md](PRODUCT_PLAN.md)，评测规则见
[EVALUATION.md](EVALUATION.md)。本文件可以根据开发证据调整顺序和实现细节，但不能
静默改变产品总纲中的固定架构决策。

## 1. 当前基线

- 导入基线：CodeWhale `352e86a611fdf3cd8bd27c36d24d482c06a71117`。
- 基线版本：workspace `0.8.68`。
- `codewhale exec`、app-server 与交互 TUI foreground 已共用
  `crates/app::AgentApplication`、唯一 `crates/runtime::AgentRuntime`、固定工具目录和
  SQLite `RunStore`；TUI 通过 canonical command/event projection 工作。
- `crates/core` 已删除；app-server 不再依赖 `core/tui`，也不启动 TUI 子进程。
- canonical `agent` 工具启动的根/子 Agent 使用同一实现；旧 `workflow-tool` 曾直接构造
  `DeepSeekClient + SubAgentRuntime + WorkflowTool`，现已连同 Workflow 私有状态和 UI
  物理删除。旧命令在读取配置、启动 TUI、打开 Store 或模型前 fail closed。
- M1-A 离线契约证据与生产工具目录测量已经完成。
- M1-B 官方 DeepSeek live canary 已通过 5/5，但仅属于协议兼容证据。
- M1-C 的共享真实 HTTP 请求硬预算已经接入当前候选；提交 `0a5b76a` 的 M4-A production
  `exec` acceptance 22/22，TUI crate 6,888 passed、3 ignored，完整 workspace 回归 0 失败。
- 已完成冻结的 pre-M3 候选与 M3 候选之间的真实单 Agent 编码 A/B；这证明 M3 整体切片
  在该任务上成功率不退化且 Token/成本下降，但不是导入基线 A/B，不能据此关闭 M1。
- M2-A 已让官方 DeepSeek `RequestPlan` 接入 production Client；planner 与物理请求账本已有
  独立 `crates/deepseek` owner，旧 TUI request-budget owner 已删除。M4-B 的受限真实
  Standard Chat + `read_file` canary 已通过，但未覆盖 M2 要求的全部官方 surface。
- 当前已有一组 DeepSeek 协议、Agent 可靠性和独立 verify 实验 WIP。
- WIP 保存提交：`2ccccdd4`。
- 本地归档分支：`archive/pre-product-plan-20260715`。
- 该 WIP 尚未通过新的产品评测门禁，不能视为稳定能力。

## 2. 执行原则

每个开发切片必须包含：

1. 问题和验收条件；
2. 契约或回归测试；
3. 一条可运行的垂直实现；
4. 调用方迁移；
5. 旧路径删除；
6. 基准结果与文档更新。

禁止只新增抽象或新实现而长期不接管生产入口。

“垂直迁移”必须在同一切片内让一个真实生产入口改用新内核，并删除该入口的旧循环、
旧事件翻译或旧状态写入。断代重构不保留旧兼容层、旧别名、双读双写、旧 fallback 或
并行生产路径。切片过大时按调用方或能力边界继续纵向拆分，但每个子切片仍须完整切换并
删除它替代的旧路径，不能用适配桥把新旧系统长期串在一起。

### 2.1 中文原生交付顺序

中文原生是产品要求，但不能成为翻译即将删除界面的返工。每个垂直切片按以下顺序执行：

1. 先确认该入口是否属于 DeepSeek 专用产品的保留链路；
2. 替代并删除无关 Provider 的选择、配置、帮助和错误分支；
3. 再用唯一 `zh-Hans` 消息目录汉化保留的人类界面，不建立 locale 状态或第二套 i18n；
4. 独立开发并评测中文原生模型提示词包，不把 UI 翻译混入 Runtime 语义。

人类界面固定 `zh-Hans`，不提供语言配置、检测、选择、切换、其他语言包或后处理翻译；
命令和 flags、工具名、Schema/API 字段、模型 ID、路径、代码、diff、stdout/stderr 和
原始日志保持稳定。最终门禁至少覆盖：

- 隔离 HOME 下的首次启动、`--help`、Setup/Doctor、认证失败、限流、超时、SSE、上下文和
  请求预算耗尽都提供中文摘要与可执行建议；
- `exec` 文本输出固定中文，NDJSON/机器事件字段与 golden fixture 不受界面文案影响；
- 单 Agent 和多 Agent 的成功、失败、取消、恢复链路均无非白名单英文，子 Agent 与恢复
  会话使用同一固定中文投影；
- 80/120 列终端下 CJK 宽度、截断和换行正确；
- 英文泄漏门禁只扫描保留的用户渲染路径，并维护技术字面量白名单，不做全仓 ASCII 扫描；
- 中文提示词包与当前版本做同任务 A/B，记录 verified success、工具错误率、轮次、Token、
  时延和成本；无能力回归且关键指标有净提升后才默认启用，版本必须可追溯和回滚。

2026-07-18 已完成固定语言基础设施候选：删除 Locale 状态与传播、环境语言检测、首次启动
语言步骤、7 个非简体中文语言包、运行时语言切换、`/translate` 以及对应的额外模型翻译
请求；保留唯一 `zh-Hans` 消息目录，并修复 canonical 首启、审批、命令与 CJK 终端宽度
验收。当前证据为 TUI 单元测试 4,989 通过、2 个预先忽略、0 失败，canonical Run 20/20、
canonical PTY 5/5、QA PTY 9/9、release runtime QA 5/5（1 个重型 fanout 用例预先忽略）、
TUI all-targets check、完整 workspace test 和 focused gate 通过。严格 workspace clippy 仍被
遗留 TUI 死代码/不可达模块告警阻断；告警数量随构建目标而异，不写死为产品指标。不得以
`allow` 压制，应继续物理删除无消费者路径。该切片只证明语言状态与翻译后处理已收敛，
不代表保留界面已经没有全部
英文，也不代表生产 Agent 系统提示已经完成中文重构；旧 Provider/Fleet/Workflow/TUI
文案应随 M4-C/M7 调用方迁移删除，保留界面再进入消息目录，生产提示词必须另做同任务 A/B。

首个中文原生生产提示词候选已经完成 24-run 正式 A/B，但未通过保留门槛：candidate
multi 从 baseline 的 6/6 降为 5/6 并真实耗尽请求预算；candidate single 另有一条因失败
重试缺 usage 而计量失效。候选 multi 的 Token 降低 12.27%，但请求、耗时和费用均上升，
不能用效率单项掩盖成功率回退。该版本保持 WIP，不默认启用；安全的 workspace path
移除继续保留。完整记录见
[中文原生生产提示词正式 A/B](../../eval/summaries/prompt-chinese-ab-2026-07-18.md)。
后续 v2/v3 的 3/cell canary 也均被拒绝：v2 没有减少请求，v3 虽让 single 和 multi root
收敛，却让 child 两次用满 4 轮并使 multi 降为 `1/3`。v2 WIP 已删除，v3 不合并；下一步
由 Runtime 保证 child 最终产物机会，不再叠加提示词限制。证据见
[中文生产提示词收敛 canary](../../eval/summaries/prompt-convergence-canaries-2026-07-18.md)。

## 3. 里程碑总览

| 里程碑 | 目标 | 状态 | 主要退出门槛 |
|---|---|---|---|
| M0 | 保护基线、整理仓库、建立唯一文档真相 | 已完成 | 工作区可追溯，产品方案落库，现有 WIP 被隔离说明 |
| M1 | 建立原始 DeepSeek 能力基准 | 已完成（M8-L 按 ADR-0006 接受 qualified real coding + exact-current release benchmark successor） | 真实编码证据、假成功、完整 accounting 与 current production retention 可复核；不伪造不可计量 imported A/B |
| M2 | 独立 DeepSeekBackend 与领域协议 | 进行中（当前候选全仓/exec/QA 回归通过，official live 待完成） | Production RequestPlan 通过真实路径/live 门禁，旧 DeepSeek 决策分支删除 |
| M3 | 最小 Headless AgentRuntime 垂直切片 | 已完成（仅 `exec`） | `exec` 单一生产 loop，离线/全仓/真实 DeepSeek 证据通过 |
| M4 | 统一工具、事件、RunStore 和产品入口 | 已完成 | CLI/TUI/API 同事件，所有生产模型循环统一 |
| M5 | ContextBroker、跨文件检索和 canonical 证据链 | 已完成（M5-A 完成；M5-B shrink；RepoGraph 按 ADR-0005 转为 post-V1 证据准入） | TaskContract/receipt 只由唯一 Runtime/RunStore 判定；现有跨文件检索可验证，结构索引不按名称堆功能 |
| M6 | 统一多 Agent 与 worktree 生命周期 | 核心机制完成（Writer explicit-only；M6-B2 不准入） | 唯一 Orchestrator、writer worktree 和并行净收益 |
| M7 | DeepSeek 专项调优与产品清理 | 已完成（M7-I 关闭 request/Token 调优；未准入的 FIM/thinking/cache/fan-out treatment 保持 hold） | 没有 material model treatment 时不消费 Key/API；可复现 correctness 与非结论入库 |
| M8 | V1 本地产品化 | 已完成（M8-N 按 ADR-0007 接受 fixed-Chinese baseline release evidence；16/16 pass，V1 可发布） | 自己的品牌、配置、CI、打包、固定中文界面和可归因 prompt/V1 gap 证据完整 |
| M17 | DSE 双语开源身份硬切换 | 进行中（M17-A 完成；M17-B next） | DSE 唯一身份、`en`/`zh-Hans` 完整产品面、单一 prompt 胜者与公开发布门禁闭环 |

## 4. M0：仓库基线与整理

### 工作

- 保存原始 CodeWhale 提交和现有 DeepSeek WIP。
- 建立 `PRODUCT_PLAN.md`、`ROADMAP.md`、`EVALUATION.md` 与核心 ADR。
- 重写根 README/AGENTS/CLAUDE/CONTRIBUTING，使其不再传播上游多 Provider 目标。
- 删除明确的版本 tracker、handoff 等工作状态残留。
- 给现有文档建立唯一索引和权威级别。
- 删除与 Rust Runtime 无编译依赖的网站、VS Code scaffold、npm 包装、云发布脚本、
  上游社区自动化、翻译和旧 dogfood/release 资料。
- 删除未被当前 Rust remote-setup 注册的 WeCom/Weixin 聊天桥。
- 盘点仍被 Runtime 引用的聊天桥、remote setup、通用 Provider 和旧 evidence。
- 不在本阶段破坏性移动 Agent 核心源码。

### 退出门槛

- 当前工作树没有来源不明的文件。
- 仓库入口只指向一套产品方案。
- WIP 和稳定能力明确区分。
- Markdown 链接和 `git diff --check` 通过。
- 清理没有影响 Cargo workspace metadata。

### 完成记录（2026-07-15）

- 原始导入、审计前 WIP 和归档分支均可追溯。
- 产品、架构、决策、参考与遗留文档已经分区，并建立唯一索引。
- 已移除与 Rust Agent 产品无编译依赖的上游网站、编辑器扩展、npm 包装、发布、社区与旧项目管理材料。
- `cargo metadata`、`cargo check`、Focused DeepSeek gate 和全 workspace tests 通过。
- Markdown 链接、Shell 语法、格式和 Git whitespace 检查通过。
- 本机 stable toolchain 未安装 Clippy component；CI 会安装并运行，不能把本地缺失记为代码通过。

## 5. M1：评测基线

### 已完成：M1-A 离线契约基线

- 2026-07-15 历史候选在当时同一 Harness/manifest 下通过 41/41 离线用例，导入提交通过
  12/12 个 `cross_revision` 用例。历史清单已原样归档，固定 Git blob
  `678c8e30d471e356cb93c47781c87b0c8624c26d`，不得用后续测试改名覆盖原口径。
- 历史 12 个跨提交用例曾覆盖当时的 Engine 与旧多 Agent 契约；它们是历史比较证据，
  不是当前 canonical Runtime、RunStore、Orchestrator 或 writer worktree 已完成的证明。
- 默认清单已迁移为 29 项当前 canonical 回归：11 项 Runtime、9 项 DeepSeek、2 项
  RunStore、3 项确定性工具、1 项 app-server 负向架构契约和 3 项真实 exec 用例。只有
  happy-path 与畸形参数两项 exec 仍可 `cross_revision`；当前“未公开工具必须 fail-closed”
  与导入版恢复语义不同，明确为 `candidate_only`，不冒充相对导入基线的能力提升。
- 当前 canonical 清单已在干净提交 `bb12bb5b395cbd3f0fc756c622ee408fe55240b6` 通过
  29/29，manifest blob 为 `93d51a22570720b9973a16e3246cae41970c3043`，结果 SHA-256
  为 `5079d435f3fe7cfc7ebc67cceb561162a1e772ab180461bc6129865afb7bdb56`。这只证明
  当前回归契约通过；两项 `cross_revision` 尚须在导入 worktree 复跑后才能形成新比较结果。
- writer worktree 没有 replacement，明确留在 M6；Strict `tool_choice`/嵌套 `anyOf`、FIM
  response parser、畸形 SSE、reasoning-only、工具业务失败恢复、child 失败 handoff 与
  失败测试结果仍是未进 runnable manifest 的证据债。
- 完整证据、哈希和解释边界见
  [M1-A 离线契约基线](../../eval/summaries/m1-offline-baseline-2026-07-15.md)。
- [`scripts/measure-tool-catalog.py`](../../scripts/measure-tool-catalog.py) 保存历史 Engine
  工具面测量；当前 production catalog 的 owner 已迁到 `crates/tools + AgentRuntime`，旧
  测量不能自动继承为当前结果。

M1-A 完成不等于 M1 完成：离线用例没有真实 DeepSeek Token、cache、成本和可验证任务
成功率，两套用例的总耗时也不可用于性能比较。

### 已完成：M1-B 官方 DeepSeek live canary

- 在干净提交 `366e8b5b37bedbbf3b1ebb326b14a70e885a57a2` 上，以 5 个受费用、Token、
  请求数和超时限制的真实请求通过 5/5。
- 覆盖 Standard Chat、Thinking tool call、`reasoning_content` exact replay、Beta Strict Chat
  和 Beta FIM；usage/cache 与 finish reason 均按各用例契约记录。
- 结果、哈希、usage 和费用口径见
  [M1-B DeepSeek live 协议 Canary](../../eval/summaries/m1-b-deepseek-live-2026-07-15.md)。
- 结果明确记录 `record_class=protocol_canary`、`product_metric_eligible=false` 和
  `verified_success=null`；它只证明当时官方 API 的 wire 契约兼容，不是编码任务成绩，
  也不证明当前生产 Client 已统一通过一个 RequestPlan 生成这些请求。

### 进行中：M1-C 固定真实编码任务基线

M1-C 先建立可执行的真实请求边界，再做 A/B：

1. **共享 API 请求硬预算**：在每次实际 HTTP `.send()` 前原子占用额度，覆盖根 Agent、
   transport retry、stream 恢复、compaction、验证/FIM 和子 Agent，不把 Engine step
   或模型 turn 冒充请求数。达到上限后必须返回类型化终态，并在最终元数据中记录 limit、
   已发送数和是否耗尽；预算封存后不得再产生后台请求。当前候选已经通过 production
   `exec` 验收 10/10、PTY QA 13/13，并在 `cargo test --workspace --locked` 中 0 失败。
2. **固定真实编码 A/B**：同任务、同仓库 revision、同模型、同工具面、同请求/Token/时间
   预算和同确定性验收器，对比导入提交和候选提交的 verified success、false-success、
   Token、时间、费用和 diff。该 A/B 尚未执行。

上述 10/10、13/13 和全仓 0 失败只证明当前候选的本地 production lifecycle、终态和
回归门禁，不证明官方 DeepSeek API 当前行为，也不证明 Agent 编码能力提升。当前候选的
credentialed official live canary 与真实编码 A/B 完成前，M1-C 仍不得标记为完成。

### 后续待完成

1. **M1-D WIP 处置**：根据离线、live 协议和真实任务三层证据，对 DeepSeek 协议、
   Agent 可靠性和本地配置逐项给出保留、重做、缩小或删除结论。旧 TUI `verify` 模型
   critic 已确认无 canonical 生产消费者并在 M4 物理删除；确定性
   `crates/tools::run_verifiers` 保留并进入 M5 Host evidence 门禁。

模型 critic 不等同于测试证据；未经真实缺陷检出率和误报率评测，不得恢复或成为完成门禁。

### 退出门槛

- 离线契约与 live 协议基线已经可重复；真实任务 A/B 仍须可重复。
- 硬预算统计所有真实 HTTP 发送且并发不超限；耗尽、封存和终态元数据有离线回归与
  production `exec` 证据。
- 真实任务能测量 verified success、false-success、输入/输出/cache Token、时间和成本。
- 每项 WIP 有保留、重做、缩小或删除结论。
- `verify` 未经评测不得默认成为完成门禁。

## 6. M2：领域协议与 DeepSeekBackend

### M2-A：production DeepSeek request planner（代码接入完成，验收未完成）

已完成的代码事实：

- `crates/deepseek` 已成为 `ApiSurface::{StandardChat, StrictChat, Fim}`、`RequestPlan`、物理
  请求预算、usage ledger 与官方 V4 pricing 的唯一 owner；TUI 旧 client 只服务尚未删除的
  generic Provider/外围路径，不得重新拥有官方 DeepSeek surface 决策。
- 官方 DeepSeek Chat streaming/non-streaming Client 已消费同一 planner；Chat
  `RequestPlan` 一次性决定 surface、endpoint、wire model、streaming、reasoning replay、
  工具/strict 状态和 body。
- Strict 在整组 schema 兼容时走 Beta；任一不兼容时整组原子回退 Standard，并保留全部工具。
- FIM 保持独立 Beta Completions 规划语义、surface 与 accounting 类型；当前 canonical
  production caller 和完整 response parser 尚未落地，不能把 request-plan 测试冒充可用的
  事务性编辑链路。
- planner 与相关 Client 单元回归已经通过，未增加第二个 Runtime 或第二套 Client 主循环。

尚未完成的验收事实：

- Standard 与 Strict-candidate 原子 fallback 已有 canonical Runtime 到 production sender
  loopback；默认 actor 没有真实 Strict surface，FIM 仍缺 production caller，因此完整
  Standard/Strict/FIM live surface 矩阵尚不能成立；
- 多轮 exact reasoning/tool history replay、畸形/不完整 Chat response 与 replay-safe retry
  已有 production/crash 证据；FIM 完整 response parser 和事务性写入仍需独立垂直切片；
- 切换后的官方 DeepSeek live canary 尚未重跑。

因此 M2-A 当前只能标记为“代码接入完成、production-path/live gate pending”，不能标记
为验收完成。下一步先补齐上述门禁，再删除已被 planner 替代的旧决策分支。

### 同切片删除/替代

- 生产路径切换时删除它原有的 DeepSeek URL、Beta route、strict flag 保留/剥离和 FIM
  endpoint 决策分支；不能让新旧 planner 并存。
- 保留仍被未迁移调用方使用的通用 transport；不得为了目录纯度扩大删除范围。DeepSeek
  owner 的每次物理抽取必须同时迁移真实调用方并删除对应 TUI owner，不能复制实现。

### 后续工作

- 在 `protocol` 定义 Task、Run、Turn、Event、ToolOutcome、Evidence 和 TerminalState。
- 建立版本化 NDJSON 事件。
- 在生产迁移中逐步形成独立 `DeepSeekBackend` 职责；后续层不得再次猜 URL、删除工具或改变 surface。
- 统一 reasoning replay、SSE、finish reason、usage、cache、retry 和 limits。
- 将现有官方 live canary 固化为 Backend 变更后的受限回归，不把它当作编码 benchmark。

### 删除/替代

- 冻结旧通用 client，不再增加能力。
- 每迁移一个真实调用方，同一切片删除它对旧 URL、序列化、
  parser、retry 和 usage 分支的依赖。
- 新 Backend 接管全部调用后删除旧 DeepSeek 路由分支和通用 Provider 选择路径。

### 退出门槛

- `RequestPlan` 已由真实生产调用路径消费，不是无人调用的新抽象。
- 协议 fixture 覆盖正常流、畸形流、工具循环、retry 和不完整终止。
- Production planner 切换后的 Standard、Thinking exact replay、Strict 和 FIM canary 仍通过。
- Strict 不兼容时普通工具调用保持可用。
- usage 无重复计算。
- Backend 不依赖 TUI、工具实现或调度器。
- 没有新增第二个 Runtime、第二个生产 Client loop 或仅为未来准备的新 crate。

## 7. M3：最小 AgentRuntime

### 垂直链路

```text
Headless Task
  -> DeepSeek
  -> read/search
  -> patch/shell
  -> verify
  -> complete candidate
  -> RuntimeEvent
```

### 工作

- 以真实 `codewhale exec` 为第一个生产调用方，不先铺设无人使用的新框架。
- 从 TUI Engine 提取唯一 `AgentRuntime`、canonical transcript、request projection 和 turn loop。
- 建立 `ModelPort`、`ToolExecutor`、`RuntimeEventSink` 和 `RunStore` 小端口；首个切片使用
  `DeepSeekBackend`、内存 `RunStore`、fixture Backend 和最小真实工具集。上下文投影由
  canonical transcript 直接拥有，不为尚未实现的 ContextBroker 预建 `ContextPort`。
- 所有运行进度只发出 canonical `RuntimeEvent`；Headless 入口只发送命令和消费事件。
- 实现 steer、interrupt、cancel、usage 和明确终态。
- 使用内存 Store 与 fixture Backend 完成 conformance 测试。

### 删除/替代

- `codewhale exec` 切换到 `AgentRuntime` 的同一切片，删除它原有的 `spawn_engine`/TUI Engine
  生产入口，不保留第二套 Headless loop。
- 不新增第二个 runtime store、event enum 或 completion 判定器。

### 退出门槛

- Runtime 不依赖 TUI、HTTP 或具体数据库。
- 能在真实仓库对至少一个预先冻结的基础 DeepSeek 编码任务完成每 cell 3 次独立运行并由
  Host 确定性验收；广泛任务集不是这个最小架构切片的成功率声明。
- `codewhale exec` 只有一条生产 Agent loop，并能重放同一 `RuntimeEvent` 序列。
- 没有引入新的通用 Provider SDK。

### 完成记录（2026-07-16）

实现与删除：

- 新增独立、无 TUI/HTTP/数据库依赖的 `crates/runtime`；根与子 Agent 复用同一个
  `Arc<AgentRuntime>`，统一 canonical transcript、DeepSeek turn/tool loop、usage、
  steer/interrupt/cancel、预算和类型化终态。
- `codewhale exec` 的所有输出模式都只经过新 Runtime；旧 `spawn_engine`/TUI Engine
  入口、settle 翻译和第二套完成判定已从该调用方删除。静态检索未发现残留旧入口。
- 生产 DeepSeek adapter 继续使用已接线的官方 planner/SSE/accounting；Runtime 独占重试，
  Strict 整目录兼容时走 Beta，任一工具不兼容时保留全部工具并回退普通 Chat。
- 固定生产工具目录为 11 个代码工具，并按配置条件加入同 Runtime 的 `agent`；工具权限
  采用正向授权，输出格式和 deny-only 参数都不会隐式开放工具。

验证：

- `./scripts/dev-deepseek-agent.sh focused`、`cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets --locked -- -D warnings`、
  `cargo test --workspace --locked` 和 `git diff --check` 全部通过；TUI crate 回归
  6885 passed、0 failed、3 ignored，`exec` production acceptance 18/18。
- 独立 release 候选 revision 为
  `worktree-head-54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3-source-3274be47303193dd81b0ea0b8d26801a80ca1780abbaacd62c606cd3a3f2a54c`；
  `codewhale` SHA-256 为 `85a5dacdcfdfe72f6cdcfef78a712826e0ab70903e095e1a4a92451b605d646b`，
  binary-pair hash 为 `sha256:610cafea04551a215efaa14a4589e1422bc4ff816eebf1d69ef3c3fbdc0ea35f`。
- 真实 DeepSeek A/B 评测 ID 为 `deepseek-exec-ab-f8ac6974c20f403282adeaca57563f3c`；
  结果文件 SHA-256 为 `938d042d7ccaeed486896bade102012f924bb25fdc020cdae2c92f1827da8d11`。
  6/6 run 均由最新工作区修订上的冻结 Host verifier 验收通过，false-success 为 0，
  29/60 个请求、总费用 `$0.004391195`，未超过 `$0.15` suite 上限；cell、comparison 与
  summary 均为产品指标可用。脱敏逐 run 证据见
  [M3 Headless AgentRuntime 真实编码 A/B](../../eval/summaries/m3-runtime-single-2026-07-16.md)。

| single lane（每 cell 3 次） | pre-M3 baseline | M3 candidate | candidate - baseline |
|---|---:|---:|---:|
| verified success | 3/3 | 3/3 | 0 |
| false success | 0 | 0 | 0 |
| 平均总 Token | 40,133 | 25,753 | -35.83% |
| 平均 wall time | 12,267 ms | 11,572 ms | -5.67% |
| 平均 API 请求 | 4.67 | 5.00 | +7.14% |
| 平均费用 | `$0.000876154` | `$0.000587578` | -32.94% |

这是一个 bundled vertical-slice treatment：评测任务提示词相同，但生产工具目录 hash 不同，
所以 Token、时间和费用变化只能归因给整个 M3 候选，不能单独归因给 Runtime。候选三次的
冻结 Host verifier 均通过；`verification_runs` 精确命令形状计数为 1/3，另两次不能由该
遥测证明模型执行了指定字符串，因此该字段不冒充成功证据。

复杂度（相对冻结 pre-M3 source，排除忽略的本地运行状态）：

- Rust/Cargo 按文件路径的 `numstat`：非 test 路径 `+6,583/-3,595`，test 路径
  `+1,924/-194`；新文件中另有 812 行内嵌 `#[cfg(test)]`，将其移出生产口径后，
  生产新增上界为 5,771 行、测试新增下界为 2,736 行。该口径是 diff 规模，不冒充
  圈复杂度。
- 新增 34 个 protocol 与 11 个 runtime 公共 struct/enum；它们属于同一 canonical
  协议族。新增持久化真相为 0，只有一个内存 `RunStore` 实现。
- 新增第三方长期依赖为 0；只新增 `tui -> codewhale-runtime` 工作区依赖。
- `exec` 的生产 Agent loop 从一条旧路径替换为一条新路径，净新增并行运行路径为 0；
  最大模型可见目录是 11 个代码工具加条件启用的 `agent`，不是 12 套工具实现。

截至 M3 候选冻结时的边界与未完成项：

- 本证据只覆盖一个 Python 基础编码任务的 single lane，不外推到多 Agent、TUI、
  app-server、crash/resume 或任意仓库成功率。
- 内存 Store、当时的 tool-result 形状和 `crates/tui` 内 adapter 是 M4 的替换对象；其中
  SQLite 持久化、统一 `ToolOutcome`、`exec` 重放与 crash/resume 已由下面的 M4-A 行为
  门禁覆盖，但不回写或改造这份 M3 历史口径。
- TaskContract、最新修订 EvidenceReceipt 与 Host 完成验收的生产迁移属于 M5；模型自评
  仍不能被解释为确定性证据。
- 写入型子 Agent 的 worktree lane 与真实 multi A/B 属于 M6；在此之前不把并发 conformance
  夸大为多 Agent 产品完成。

## 8. M4：工具、状态和入口统一

### M4-A：Headless 状态真相（严格完成）

先只闭合 `exec` 的状态语义，不同时迁移 UI：

1. 在唯一 Runtime 定义统一 `ToolOutcome`，明确 invocation、operation、retry、evidence
   和 artifact 状态，并让现有 11 个 Headless 工具真实消费它；
2. 用 `crates/state` 实现唯一 SQLite append-only `RunStore`，保持与内存 Store 的同事件
   replay conformance；生产 `exec` 切换后删除其内存生产路径；
3. 实现同一 run 的 crash/reopen/resume、事件序号与 exactly-once terminal，进程被杀或
   重启后不得产生第二个完成判定；
4. 通过 fixture 故障注入、production `exec` acceptance、全仓回归和受限真实 DeepSeek
   resume 任务；记录成功率、假成功、恢复率、Token、时间、费用与复杂度；
5. 不迁移 TUI/app-server、不扩工具目录、不做多 Agent worktree、不新增第二个 Store 或
   临时兼容桥。M4-A 完成后再按 app-server、TUI 两个垂直切片迁移入口。

### M4-A 验收记录（2026-07-16）

- canonical event schema v3 和 State schema v6 已进入生产 `exec`；11 个固定工具直接消费
  canonical `ToolOutcome`，生产只写一个 SQLite `RunStore`，内存 Store 仅用于测试。
- transcript、usage/accounting、工具 outcome/artifact 和 terminal 均由 append-only event
  log 重放；sequence 单调、event id 幂等、lease epoch fencing，每个 run 只有一个持久终态。
- 模型在途、工具副作用、响应提交后 sink 未收到、终态提交后 sink 未收到和调用方重试等
  真实子进程窗口均通过预定义恢复契约；危险的外部在途状态 fail closed，不重复请求或副作用。
- 被测源码已冻结为 commit `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3`、tree
  `2f664616def40906c2ef62b5a3fec46a9a76c7a3`；它通过离线门禁、22 个 production `exec`
  acceptance、全仓 Clippy/test 和 locked/offline release build。
- 最终提交绑定的真实 DeepSeek fresh-run A/B 为 10/10 verified、false-success 0；但 candidate
  相对 baseline 的 Token、请求、时间和费用聚合分别为 `+16.38%`、`+13.64%`、
  `+8.22%`、`+10.32%`。
  归一化后的每请求成本近似不变，现有样本不足以把多 0.6 次请求归因给持久化，因此只记录为
  成功率不退化、效率证据负向且不确定，绝不声称性能提升。
- commit-bound v2 DeepSeek safe-point `SIGKILL -> reopen -> same run` canary 通过：6 次请求、
  30,022 Token、11,632 ms、`$0.000486651`，501 个事件且 501 个唯一 event id、1 个 terminal，
  prefix digest 不变，旧 lease owner 回收并由 epoch 2 新 owner 接管；冻结 Host verifier
  通过、false-success 0。无 Key terminal replay 新增事件 0，工作区、工具副作用和 accounting
  不变。该单次 canary `product_metric_eligible=false`。
- 完整脱敏证据、二进制/结果摘要、复杂度与残余风险见
  [M4-A Headless 状态真相证据汇总](../../eval/summaries/m4-a-headless-state-2026-07-16.md)。
- M4-A 到此严格关闭；后续不得借 M4-B 重开第二个 Runtime/Store 或给旧 app-server 加长期
  bridge。M4-B 只迁移并删除 app-server 自己的旧生产路径。

### M4-B：本地 API 单一 Runtime/RunStore 纵向切换（严格完成）

真实问题不是“缺一个 HTTP 接口”，而是当前本地 API 有三条互相冲突的执行/状态路径：

- app-server 的 `/prompt` 调用 `crates/core::Runtime::handle_prompt`，只产生伪造的
  response start/delta/end，不是生产 Agent loop；
- app-server 的 thread bridge 启动 sibling TUI 子进程；
- TUI runtime API 再通过 `spawn_engine + monitor_turn + RuntimeThreadStore` 维护私有事件和
  thread 状态。

因此 API 不能继承 M4-A 已验证的 DeepSeek、工具、恢复、账本和子 Agent 语义，并继续制造
第二套 completion 与持久事实。M4-B 的目的，是让 `exec` 与 app-server 共用一个真实的
application composition root；HTTP/SSE/stdio 只提交命令和投影 canonical Store event。

#### 范围与唯一 owner

1. `crates/app` 成为唯一 composition root 和产品命令 owner。只有在 `exec` 与 app-server
   两个真实调用者同时迁入时才创建，不先造空 crate；它组合唯一 `AgentRuntime`、
   `StateStore`、DeepSeek model port、固定 11 工具、预算和 active control。
2. 将当前 TUI 内 `DeepSeekModelPort`、`ProductionToolExecutor` 和最小 `RunRequest` 构建职责
   按依赖方向迁入 `app`，不复制实现；`runtime` 继续不知道 DeepSeek/HTTP/SQLite/TUI。
3. `exec` 改用同一 app service，presentation 与 signal handling 仍是薄客户端，行为和 M4-A
   事件/恢复契约不得变化。
4. app-server 只保留认证、CORS、body limit、HTTP/SSE/stdio framing。最小产品 surface 为
   `POST /v1/runs`、`GET /v1/runs/{id}`、
   `GET /v1/runs/{id}/events?after_sequence=N`，以及同一 run 下的
   `resume/steer/interrupt/cancel` 命令；stdio 使用相同 command DTO 与
   `StoredRuntimeEvent` envelope，不另造 schema。
5. app service 只允许不可持久化的 active-control registry/notification；所有可重放状态、
   usage、terminal 和 artifact 仍只写一个 SQLite `RunStore`。

本切片不迁移 TUI 交互 loop、TaskContract/EvidenceReceipt、RepoGraph/compaction、writer
worktree Orchestrator、Provider 全仓清理、提示词调优或全面汉化。也不把旧
request-user-input、approval、fork/undo/retry、task/fleet/session/automation/mobile API 用
compat bridge 包装成新能力；未进入 canonical command/event 的能力直接不对外宣称。

#### 完成记录（2026-07-17）

- `crates/app::AgentApplication` 已成为 `exec` 与 app-server 唯一 composition root；两者共用
  `AgentRuntime`、`DeepSeekModelPort`、固定 11 工具、SQLite `RunStore`、请求预算和账本。
- app-server 只保留 canonical start/get/events/resume/steer/interrupt/cancel 的
  HTTP/SSE/stdio framing；SSE id 等于 Store sequence，stdio 使用同一 DTO 和 Store event。
- `crates/core`、fake `/prompt`、raw model proxy、direct tool invoke、`RuntimeBridge`、TUI
  sibling process、私有 seq/thread 状态和旧 route alias 已物理删除；app-server dependency
  tree 不含 `core/tui`。`RuntimeThreadStore` 只剩交互 TUI 的真实消费者，删除点为 M4-C。
- 同一 fixture 的 exec/HTTP/stdio canonical parity、SSE replay、typed command outcomes、外部
  app-server `SIGKILL` 恢复、唯一 terminal、无 Key 重放和 M4-A crash matrix 均通过。
- 被测源码为 commit `a534a824670b60c807c5abf399ea8674d4beb527`、tree
  `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8`；focused、fmt、workspace Clippy/test、两次
  完整 TUI test、locked/offline release build 全部通过。
- 真实 `deepseek-v4-pro` canary 只允许一次 `read_file`：2 次 Standard Chat 请求、1 次工具
  调用、5,675 Token、3,908 ms、`$0.001378254`，89 个唯一事件且恰好一个 terminal；HTTP 与
  SSE 完全一致，终态可由 app-server 和顶层 launcher 无 Key 重放且追加事件为 0。
- M4-A checkpoint 到 M4-B 被测代码的 Git diff 为 299 files、`+33,892/-55,761`，净减少
  21,869 行。该范围包含 M4-B 的依赖纵切和删除，不能冒充单模块净复杂度。
- 完整脱敏证据、二进制摘要、费用、监督脚本诊断和残余风险见
  [M4-B 本地 API 证据汇总](../../eval/summaries/m4-b-local-api-2026-07-17.md)。单次 canary
  `product_metric_eligible=false`，不构成编码能力或效率提升声明。

#### 同切片删除/替代

- 删除 app-server 的 `RuntimeBridge`、TUI child-process、seq/thread maps 和事件翻译；
- 删除 `/prompt` fake loop、`/v1/chat/completions` raw Provider proxy、`/tool` direct invoke，
  以及 app-server 暴露的 legacy thread/job/MCP startup JSON/JSON-RPC aliases；不改 MCP
  transport 或 manager；
- 删除 CLI 到 sibling TUI app-server 的 delegation，以及重复的 `serve --http/--mobile`
  入口；MCP/ACP 等不同协议不在本切片顺手重构；
- 删除 app-server 对 `crates/core`、`crates/tui` 和 legacy runtime/session/task/fleet/lane
  状态文件的生产调用路径；`crates/core::Runtime::handle_prompt` 无剩余消费者时物理删除；
- TUI `runtime_api/monitor_turn/RuntimeThreadStore` 无剩余消费者时同步物理删除；若仍有 M4-C
  真实调用者，只允许保留源码到该切片的明确删除点，不能继续服务 app-server。

不保留旧 route alias、双读双写、事件转换兼容层或第二个 completion 判定。

#### 验收与量化证据

1. 同一冻结 fake model/tool fixture 下，`exec`、HTTP 和 stdio 的 normalized canonical
   event kind/payload/order、terminal、accounting 完全一致；transport frame 均可按
   `run_id + sequence + event_id` 对应 Store event，0 丢失、0 虚构。
2. SSE 从任意 sequence 重连只返回其后的事件直到 terminal；终态重放不读 Key、不调用
   model/tool、不追加事件。start/resume/steer/interrupt/cancel/not-found/already-running/
   terminal/recovery-required 都有 typed contract tests。
3. SQLite 保持恰好一个 terminal；外部进程 `SIGKILL` 后 app-server 重启可恢复同一 run，
   live owner 被拒绝、dead owner 被回收，M4-A 的 exec crash/resume 门禁持续通过。
4. `cargo tree -p codewhale-app-server` 不含 `core/tui`；app-server 不含 raw model loop；其生产
   调用图不存在 `handle_prompt|RuntimeBridge|spawn_engine|EngineEvent|monitor_turn|RuntimeThreadStore`。
5. app-server 启动和每个 run 新增 TUI child process 都为 0；新增 Agent loop、持久 Store、
   模型工具实现和 compat adapter 都为 0。记录新增/删除 production LOC 与依赖，目标是总
   路径下降，不以迁移代码量为进度。
6. release 外部进程 smoke 与一次费用受限的官方 DeepSeek API canary 通过；单次 canary
   标记 `product_metric_eligible=false`。若声称任务能力或效率提升，必须另做每 cell 至少
   3 次的真实 A/B，不能从 transport 迁移推断。
7. focused crate tests、fmt、workspace Clippy `-D warnings`、workspace tests、现有 M4-A
   exec acceptance 和 resume gates 全部通过，随后冻结可 checkout/rebuild 的提交。

#### M4-B 准备切片记录（已并入完成切片）

- 生产 system prompt、项目上下文、技能发现和既有 WorldState block 顺序已从 TUI 源码
  move 到无 TUI/core/tools/app 反向依赖的 `crates/context`；`exec` 与交互 TUI 均调用这一个
  builder，旧 TUI builder 和 runtime prompt converter 已删除。本切片只冻结现有正文、顺序
  和 Stable/Volatile 边界，没有调优或翻译提示词。
- 交互 TUI 在进入旧 `models::SystemPrompt` 请求路径前仍有一次纯表示转换；该 adapter 不构造
  或修改正文，并在 M4-C 交互 Runtime 切换到 canonical protocol type 时随旧请求模型一起删除。
- `ProductionPromptRequest` 已由 `exec` 与 app-server 通过真实 `crates/app` composition 复用。

### M4 收口记录

- M4 最终代码检查点为 `65fa88ba`。State schema v11 删除无生产消费者的
  `thread_goals`，v12 删除 retired thread/workflow 状态表和 `threads.current_leaf_id`；
  canonical `agent_runs`、`agent_run_events`、`agent_run_snapshots`、
  `agent_run_creations` 及必要 thread metadata 保留。
- 最终调用图只有一个生产 `AgentRuntime` 类型；普通请求与 compaction 的两个
  `ModelPort::stream` 调用点都位于该实现内。canonical Agent Run 的 terminal 只在同一
  Runtime 提交；SQLite `RunStore` 的生产实现只有 `StateStore`，另一个
  `InMemoryRunStore` 仅用于测试。Fleet ledger 是待 M6 收敛的编排状态，不是第二个 Agent
  模型循环、RunStore 或 terminal owner。
- M4 最终门禁：`./scripts/dev-deepseek-agent.sh focused`、canonical PTY 7/7、
  State 进程级 crash/replay 14/14（1 个 helper 按设计忽略）、
  `cargo clippy --workspace --all-targets --locked -- -D warnings` 和
  `cargo test --workspace --locked` 全部通过。

- M4-A 已完成 Headless CLI 的 canonical `ToolOutcome`、SQLite RunStore 和恢复闭环。
- M4-B 已完成 app-server 纵切，并删除该入口的旧 core/bridge/私有状态路径。
- M4-C 的交互控制基础切片已冻结：approval 与 request-user-input 共用一个 durable
  interaction 协议，steer 使用 `SteerQueued -> SteerApplied` 安全边界，interrupt/cancel 和
  command receipt 进入 canonical event/Store；HTTP 与 stdio 使用同一 schema。
- M4-C C2 先建立切换所需的 continuation lineage 和最小 context projection：Run API v3
  区分同 run `resume` 与新 root `continue`，RuntimeEvent v5 持久化 compaction 阶段，State
  schema v8 以 durable creation reservation 防止 start/continue/compact 重复创建。该候选已
  通过验收并冻结为提交 `4a3311ac`，但不代表 compaction 已产生产品收益。
- M4-C C3 已迁移交互 TUI foreground：只经 `TuiRunClient` 提交 command，由
  `CanonicalRunProjection`/presenter 投影 root 与 child durable event；旧 Engine、
  EventBroker、foreground state owner、`SessionManager`、child display cache 和旧 slash
  command system 已删除。
- M4-C 已删除隐藏 `workflow-tool -> WorkflowTool -> SubAgentRuntime -> DeepSeekClient`
  第二模型循环及其独立 Workflow 状态、UI、审批、触发和 JS authoring surface；
  `workflow`/`workflow-tool` 在任何配置、TUI、Store 或模型初始化前 fail closed。canonical
  `agent` 的根/子同 Runtime 能力保持不变；DAG/worktree 能力以后只能进入唯一
  Orchestrator，不恢复兼容桥。
- M4-C 已删除 `codewhale serve --acp` 与 1,210 行独立 session/stream/direct
  `DeepSeekClient` 实现。裸旧命令在 Config、TUI、RunStore 和模型前由 Clap 拒绝；只有显式
  `--prompt "serve --acp"` 才作为普通任务进入 canonical Agent。
- M4-C 已由 `83d9487f` 删除 direct `review -> DeepSeekClient::create_message`、模型内
  `ReviewTool`、私有 receipt 文档/状态和退役 review UI；代码审查只保留为 canonical
  reviewer Agent profile/任务能力，不再拥有独立模型循环或 completion 判定。
- M4-C 已删除无生产构造入口但仍参与编译的旧 TUI
  `SubAgentRuntime`/`SubAgentManager`、`agents_*` 协调工具、child `ToolRegistry`、
  私有 mailbox/checkpoint/state 及不可达 `/subagents` modal；Fleet 从未接入的
  `SharedSubAgentManager` 可选投影也已删除。Fleet 实际 worker 继续只通过 canonical
  `codewhale exec` 子进程执行。由于 task-level role/tool scope 尚未进入实际 argv，仅为
  声明 profile 生成权限 receipt 的 `WorkerRole`/`WorkerRuntimeProfile`/
  `FleetWorkerRuntimeSpec` 已删除，生产 receipt 的 `effective_permissions` 在 M6 enforced
  policy 接管前固定留空。route、reasoning、prompt 与全局 `FleetExecConfig` allow/deny
  继续按真实 exec 参数保留。
- M4-C 已物理删除无生产构造入口的旧 TUI `verify` 模型 critic、`FimEditTool`、
  `RunTestsTool`、`RunVerifiersTool` 及其私有 sender/parser。确定性 `run_tests`/
  `run_verifiers` 仍由 `crates/tools` 提供；`crates/deepseek` 继续拥有 Beta FIM request
  planner、surface 与 accounting 类型。该删除不声称 canonical FIM response parser 或
  事务性编辑链路已经完成，相关缺口仍按 M1/M2 证据债处理。
- M4-C 已删除退役 TUI model client/cache/mock/retry surface 及其旧请求、响应和 SSE DTO；
  `models.rs` 暂时只保留仍被 history、pricing、MCP/工具 schema 和配置状态真实消费的
  展示、计量与模型元数据，不再承担 DeepSeek transport 或请求规划职责。
- M4-C 已物理删除 2,607 行 TUI 私有生命周期 shell-hook owner、`App` 持有状态、
  `[hooks]` schema 与从未被 production 覆盖的 `ExecShellHost::collect_shell_env` 端口。
  切换后唯一真实行为损失是 `mode_change` shell 命令不再执行，交互启动也不再读取
  `.codewhale/hooks.toml`；`session_*`/`message_submit`/`tool_call_*`/`turn_end`/
  `subagent_*`/`shell_env` 等宣传过的事件原本就没有 canonical production 派发。
  canonical RuntimeEvent、RunStore、工具执行、MCP protocol notification、panic hook、Fleet webhook
  和桌面通知保持原 owner 与语义。
- M4-C 已继续删除零反向依赖的通用 `crates/hooks` workspace crate，以及
  `codewhale-config` 中没有 sink 注册方或读取方的 `[hook_sinks]` typed schema、
  get/set/unset/list 分支和自测承诺。删除前 Cargo 反向依赖图只包含该 crate 自身，配置字段
  也只被自身测试读取，因此没有生产行为损失；未知配置 extras 继续遵循既有通用行为，
  不为已删除能力建立专门兼容。MCP protocol notification、panic hook、Fleet alerts/webhook、
  桌面通知以及 canonical Runtime/tools/RunStore 均不依赖这两个旧 owner。
- M4-C 已删除从未接入当前交互循环的 TUI `FrameRateLimiter`/`FrameRequester`/`MotionPolicy`
  编译岛及其只写不读的 `constrained_frame_rate` 设置。现有 24ms 事件轮询、80ms 动画重绘、
  `low_motion` 和实际 spinner/ocean 渲染保持不变。
- M4-C 已删除没有写入、弹出或清空调用方的持久 composer stash，以及 Doctor 对历史
  `composer_stash.jsonl` 的假诊断、不可达 Ctrl+S/`/stash` 帮助和提示；后续调用图又确认
  进程内 recovery draft 只有清空时的 writer、没有任何 restore 入口，因此也已物理删除。
- M4-C 已删除唯一构造 helper 自身也无调用方的 TUI Setup Wizard、9 个无 canonical handler
  的 Setup 事件和 175 条专属文案。顶层 `codewhale setup`、Doctor setup 投影、
  `SetupState`/`UserConstitution` 及生产提示词加载语义保留；现有用户 sidecar 不自动删除。
- M4-C 已删除无任何生产按键入口的 Activity Detail/Turn Inspector、详情专属
  shell 路由、composer 外部编辑器、假 Ctrl+O/Alt+V/Alt+L 文案以及只读不写的推理折叠/
  详情高亮状态。显式开启推理时只显示 canonical 完整内容；canonical 审批的 `v` 参数查看
  和分页复制通过本地 View event 接入通用 `PagerView`，不创建 RuntimeEvent 或第二份状态。
- M4-C 已删除没有 canonical 命令、按键处理或 Run 事件的 TUI `/jobs` 作业中心、假
  Ctrl+B/Ctrl+X 控制、sidebar 点击动作和 TUI 私有 shell manager。固定 production catalog
  只公开同步有界 `exec_shell(command, timeout_ms?, cwd?)`；运行中的命令和输出由 canonical
  工具卡如实显示，取消、超时和进程树清理仍由 `ProductionToolExecutor` 的受管进程实现。
- M4-C 已删除只有自测、没有 production delta 写入方的旧 TUI `StreamingState` 与
  `streaming_thinking` collector，以及只初始化或 reset 的影子 reasoning 字段。canonical
  reasoning/assistant 增量继续由 `run_presenter` 直接从 `RuntimeEvent` 投影到 transcript；
  已完成 reasoning 只显示 canonical 可证明的“已完成”状态，不再依赖旧 collector 独有的
  duration 产生错误“空闲”状态；DeepSeek reasoning replay 与 Runtime transcript 不变。
- M4-C 已物理删除没有 production consumer 的 TUI Goal/Hunt loop、私有
  TaskContract/receipt/Goal completion store、Slop ledger、`ToolContext.goal_contract`、
  假 custom-command allowed-tools/pause 状态及其 Work/UI/config surface。交互 TUI 启动
  canonical Run 时 `ToolPolicy.allowed` 明确为 `None`。canonical Runtime terminal、RunStore、
  确定性 `run_verifiers` 和多 Agent/Fleet 均未改变。
- M4-C 已将 WorkSurface 收敛为 canonical child Agent 的只读投影：删除从未被生产事件循环
  调用的键盘/鼠标输入、焦点/选择/滚动、打开详情、停止确认、命中区和失效
  `/task`/`/jobs` 动作；随后删除没有生产 writer、RunStore 表或 RuntimeEvent 的 TUI-local
  Plan/Todo Store、`App.task_panel`/`TaskPanelEntry`、假工具及其 sidebar/footer/transcript
  reader。top/left/right 布局、状态排序与 canonical child 投影保留；Activity 继续读取
  canonical `GenericToolCell`。M5 的 TaskContract/EvidenceReceipt 必须由唯一 canonical
  owner 实现，不能恢复私有 Store。
- M4-C 已删除从未被 production presenter 构造的 TUI `ExecCell`/`ExecSource` 专用展示岛，
  同步删除只服务旧类型的 foreground shell chip、pending-CI 猜测、命令时长与 live-output
  分支。真实 `exec_shell` 仍从 canonical `ToolPrepared`/`ToolOutcomeCommitted` 投影为
  `GenericToolCell`，保留命令摘要、状态、失败输出、Activity、transcript、折叠保护和中断
  收敛，不增加兼容 adapter。
- M4-C 已继续删除无 canonical producer 的 `Exploring`/`PatchSummary`/`DiffPreview`/`Mcp`/
  `WebSearch` 专用 TUI 工具卡与不可达聚合 helper，并把失去变体意义的 `ToolCell` 直接折叠为
  `HistoryCell::Tool(GenericToolCell)`。固定 11 工具只保留一条状态、渲染、Activity、折叠和
  transcript 路径；`git_diff`/`git_status` 的 edit/read 语义已补齐。真实 MCP transport、
  discovery 和 CLI 不经过旧卡，未被删除。
- M4-C 已删除只服务退役 TUI 工具、没有 production executor 或 registry 消费者的旧
  `ToolSpec`/`ToolContext`/`RuntimeToolServices` abstraction island。TUI error taxonomy 直接
  消费 `codewhale_tools::ToolError`；固定 11 工具、`ProductionToolContext`、canonical
  sandbox/shell 与 Runtime `ToolExecutor` 保持唯一 owner，不新增兼容 adapter。
- M4-C 已删除零调用方的 TUI OpenAI/Anthropic message/tool DTO 与 MCP `to_api_tools`
  广告器。该广告器从未进入 canonical `AgentRuntime` 或 RunStore；MCP 配置、transport、
  OAuth、发现与 CLI 管理保留，后续若接入模型必须走唯一 `ToolExecutor`/`ToolOutcome` 契约。
- M4-C 已删除只有自测构造、没有生产打开入口的旧 Mode/Status picker modal、专属事件和
  状态行 picker 文案；`StatusItem` 配置与实际 footer 投影继续由原调用方保留，不恢复
  不可达的 `/mode` 或 `/statusline` 外壳。
- M4-C 已删除同样没有生产构造、canonical 命令或 Run 事件入口的旧 `ConfigView` 编译岛、
  专属测试和消息目录；真实 `Config`/`Settings` 启动加载继续保留。后续静态模式切片又删除
  失去迁移调用方的 `ApprovalPolicyControl`。Doctor 和当前参考文档改为真实
  `~/.codewhale/config.toml` 路径，不再宣传不存在的 `/config` 编辑器。
- M4-C 已删除只有自测构造的旧 `ThemePickerView` 及其专用 `settings_picker` 框架、
  `ConfigUpdated` 事件与主题 picker 消息；主题配置文件加载、`ThemeId`/`UiTheme`、Ocean
  渲染和主题对比测试继续保留，未把不可达 modal 当成真实主题能力。
- M4-C 已删除无生产入口的旧 `FeedbackPickerView`、其私有 command-palette 事件与失效的
  `Ctrl+K` 帮助项；真实 slash menu 仍由输入 `/` 打开并走 canonical 命令面。
- M4-C 已删除无人调用的旧 `FilePickerView`/`file_picker_relevance`、专属事件、消息与
  `Ctrl+P` 帮助项；`@mention` 只保留由 canonical key handler 驱动的 composer 路径补全，
  使用 Workspace 确定性排序，第一次 Enter/Tab 只接受候选，下一次 Enter 才原样提交
  `@path`。无人消费的内联正文/XML renderer、假 pending-context 预览与 `file_frecency`
  已物理删除，不再把未接入 canonical request 的行为宣传成附件能力。
- M4-C 已删除无生产调用者、却能绕过 canonical Runtime/Store/accounting 直连任意
  `/chat/completions` 的旧 prompt suggestion 模块，以及永不写入的 ghost-text 状态和配置；
  composer 保留确定性的中文空输入提示，不保留潜在第二模型请求路径。
- M4-C 已删除只剩自测的 TUI `schema_sanitize` 重复实现；DeepSeek Strict Function Calling
  的全目录兼容性、原子 ordinary-tool fallback 与官方 Beta Chat 路由继续由
  `crates/deepseek` 唯一负责，未删除或降级 DeepSeek Beta 能力。
- M4-C 已删除同样只有自测、从未接入 MCP 或 production request 的 TUI
  `schema_canonicalize`；上下文缓存依赖 canonical DeepSeek 请求投影的稳定前缀和真实命中
  证据，不把未接线的通用 schema 变换器算作能力。
- M4-C 已删除没有任何构造方、只显示路径文本且从未加载图片的旧 TUI
  `ToolCell::ViewImage`；真实图片读取仍由 canonical `read_file` 工具路由到本地 macOS
  Vision/Tesseract OCR，该能力及其回归测试保留。
- M4-C 已删除只有自测构造、没有按键或命令入口的旧实时对话覆盖层及其私有缓存，并同步
  移除失效的 `Ctrl+Shift+T` 声明。主 transcript、canonical Run 流式投影和
  `TranscriptViewCache` 继续作为唯一实时对话展示路径。
- M4-C 已删除始终为 `None`、没有生产打开或按键调用方的旧 File Tree pane、私有后台扫描
  和失效的 `Ctrl+Shift+E` 声明。真实 `@mention` 的确定性路径补全和 composer 菜单保留；
  canonical request 当前只保留原始 `@path`，文件访问仍由模型显式调用 `read_file`，不再
  保留第二套 TUI-local context owner。
- M4-C 已物理删除仅由自身测试调用、从未进入 production prompt 的旧 TUI `memory.rs`
  push/inject 实现。canonical `crates/context::production_system_prompt`、instructions、skills、
  WorldState、handoff 与 compaction 保留；历史 memory 配置/Doctor/侧栏假投影另片清理。
- M4-C 已删除没有任何 renderer 或 handler 消费的静态 `keybindings.rs` 目录；真实帮助与
  canonical slash command/PTY 契约继续由实际 UI 路径验证，不保留与运行时脱节的第二份
  快捷键真相。
- M4-C 已删除零调用方的旧 `tui/mcp_routing.rs` manager formatter/pager adapter；真实
  `McpPool`、stdio/HTTP transport、OAuth 与顶层 CLI 配置/连接/发现保持不变。canonical
  Agent 固定工具目录没有加载 MCP pool；不把 CLI discovery 或未接线的 TUI 展示器误当作
  model-visible MCP 能力。
- M4-C 已删除零消费者的 `fast_hash` 类型别名与用户 regex LRU cache 自测岛，并移除对应
  TUI 直接依赖；真实 eval/execpolicy/Fleet 正则调用继续使用各自明确实现。
- M4-C 已删除 904 行、只有自身测试且 App 只默认构造不读取的通用 Provider readiness
  快照岛。DeepSeek 正式请求、Doctor 单请求探针和 typed protocol outcome 不变；不保留一套
  从未被真实请求更新的“健康状态”假真相。
- M4-C 已删除 App 永远初始化为 `None`、没有任何生产写入或按键处理的旧 Decision Card
  overlay；结构化用户选择继续只走 canonical `request_user_input` interaction 与 RunStore。
- M4-C 已删除没有生产调用方的旧 TUI `arg_repair`。DeepSeek SSE 仍按增量精确重组参数，
  canonical `ToolArguments` 保留原始字节并严格解析；畸形 JSON 作为可重试的 typed tool
  outcome 返回模型，不再由未接线的启发式代码静默改写模型参数。
- M4-C 已删除只有自身测试的旧 `/models` 英文消息 formatter；canonical 命令面不暴露
  `/models` 或模型 picker，正式运行的模型选择仍由 DeepSeek 配置进入 `StartRunCommand`。
- M4-C 已删除只有自身测试、从未被生产构造或处理的旧 `ElevationView` 动态提权岛。
  canonical Approval/UserInput/Pager 交互保持不变；`allow_sandbox_elevation` 仍是 Run 启动时
  显式授权的宿主策略，不再与一套未接线的“拒绝后弹窗重试”实现混为一谈。
- M4-C 已删除没有生产构造者的 TUI `AutoReviewPolicy`、allow/block 配置、审计事件和第二套
  shell/action 风险解析。`crates/tools` 的 production executor 现在是
  `ToolApprovalPrompt::risk` 的唯一事实 owner，TUI 只把 canonical `Routine`/`Elevated`/
  `Critical` 一对一投影成展示 stakes；实际批准仍只通过 canonical interaction/RunStore。
- M4-C 已物理删除旧 Engine 遗留的 post-edit LSP 编译岛：`LspManager`、stdio
  transport、diagnostic renderer 和 language registry 的所有构造/诊断调用都仅存在于
  模块自测，canonical Runtime 从未消费。同步删除两套零消费者 `[lsp]` typed
  schema、默认显示 `lsp: on` 的虚假侧栏状态和专属配置/参考文档。当前不会启动
  language server 或注入合成模型消息；M5 的 RepoGraph LSP definition/reference 目标保留，
  只能在 canonical context/tools owner 下以纵向实现和 A/B 重新建立。
- M4-C 已物理删除 1,852 行旧 TUI side-git snapshot 岛、两套 `[snapshots]` schema、死
  `/restore`/`/undo` 文案和错误产品声明。当前没有任何 production `snapshot()`、
  `restore()`、列表或 UI/工具调用，唯一生产入口只是交互启动时为旧版本遗留仓库执行保留期
  prune；切换后的真实行为损失是程序不再自动清理磁盘上的历史 side-git 数据，且不会自动
  删除这些用户本地文件。canonical `RunSnapshot`/`RunReplay`、SQLite
  `agent_run_snapshots`、事件 reducer/crash replay、`ToolOutcome.artifacts` 和 Fleet
  checkpoint 均保持原 owner 与语义。
- M4-C 已删除只会枚举或移除旧
  `sessions/checkpoints/{latest.json,offline_queue.json}` 的 `setup --clean`、`CleanPlan` 和
  无调用的系统 skill uninstall/name classifier。原 `SessionManager` writer/reader 早已删除，
  当前没有生产路径生成或读取这两个 JSON；切换后的真实行为损失是程序不再代用户列出或删除
  已存在的历史文件，它们会留在磁盘直至用户手工处理。交互启动仍生产调用
  `install_system_skills`；Setup `--force`、canonical `state.db`/`agent_run_snapshots`、crash
  replay、进程内 busy-message queue、Fleet ledger/checkpoint 和 constitution checkpoint 均未改变。
- M4-C 已删除从未被启动入口调用的 TUI 后台版本检查闭环、专属 release-asset/semver
  helper、`[update]` typed schema、默认模板与假文档。全仓调用图中
  `spawn_startup_version_check` 只有定义，没有 spawn、join、toast 或 renderer 消费者，因此
  删除没有运行时行为损失；旧 `[update]` 表不再作为受支持配置。Doctor 的显式 release
  诊断与 `crates/release` 平台 TLS builder 及其 MCP/OAuth/Fleet alerts 消费者保持；
  系统 skills 自动安装也有独立生产调用方并保持不变。本切片没有把 updater 名称
  相似性误判为可整 crate 删除。
- M4-C 已物理删除没有生产命令或 Runtime 消费者的 community skill installer、直接把该
  源文件编入测试的伪 `skill_cli` 验收，以及只服务它的 registry URL/安装大小配置和
  `tar`/`flate2` 直接依赖。TUI 与 `codewhale-config` 都不再把这两个旧键建模为受支持
  schema；`codewhale-config` 只将 TUI 的表作为通用 extra 往返，当前 `[skills]` 唯一支持的语义是
  `scan_codewhale_only` 本地 discovery 范围。交互启动仍调用
  `install_system_skills`，其 marker/version、`scan_codewhale_only` 和 canonical prompt
  discovery 均保留。自动 bundle 中只宣称不存在的 `/skill install/update/trust/uninstall`
  的 `skill-installer` 已从源码 catalog 移除；程序不会主动删除用户磁盘上已有的历史目录。
- M4-C 已删除旧 `workspace-trust.json` 外部路径信任快照中零生产调用的
  `add/remove` writer、原子写 helper 和只验证这些死 writer 的测试。产品仍只读已有
  快照，并将当前 workspace 的 canonical 路径列表交给 `ProductionToolConfig`；
  `permits` 保留为读取边界回归，不增加第二权限判定。独立的 `[projects].trust_level`
  onboarding/MCP 信任读写保持不变；程序不会清理用户磁盘上的历史 trust 文件。
- M4-C 已删除未注册、零执行调用方的旧 TUI `RequestUserInputTool`/parser 和永远为 `None`
  的 prompt shadow。保留的 UserInput modal 直接使用 canonical protocol 类型，并继续通过
  `AgentRuntime` interaction 与 `RunStore` 提交或取消，不再经过第二套 TUI ToolSpec。
- M4-C 已把仍在使用的 slash-menu 上下选择收回 canonical `ui.rs`，并删除其余全部零调用
  的旧 `composer_ui` 键盘处理器；随后又删除同样没有 canonical 键盘入口的 composer
  history-search 状态、匹配器、renderer 和消息目录。下一独立切片又删除无键盘入口的
  input-history recall、只写不可读的磁盘 history 线程及其设置，并把 `Ctrl-C`/`Esc` 清空输入
  收缩为直接 clear；普通输入、paste、mention、slash、提交及 canonical Run 投影保持不变。
- M4-C 已删除只有 App 自测、没有 canonical key/mouse/paste producer 的 composer line/word
  forward 编辑 helpers 与两个 selection 叶子，并同步删除虚假的 `Ctrl-U`、word-motion 和
  `! command` 快捷键声明。真实 `Ctrl-W`、左右/Home/End、Backspace/Delete、paste 与模型
  `exec_shell` 工具不受影响；完整 selection 状态当时未混入该切片。
- M4-C 随后的独立调用图确认 composer selection 只有默认 `None` 和清理 writer，没有任何
  canonical key/mouse/paste producer；现已删除该 App 状态、编辑分支、字符索引重复布局和着色
  renderer。终端原生选择、菜单/审批选中、Pager 复制及普通 composer cursor/layout 保持不变；
  从未被读取的内部鼠标定位缓存同步删除。
- M4-C 已删除零生产消费者、仅由自身测试调用的 TUI `is_key_file`/`summarize_project`/
  `project_tree` 浅层 project-map helpers；当前生产上下文继续由 `crates/context`、显式文件
  工具和 canonical transcript 构造，M5 的 RepoGraph/ContextBroker 不通过保留旧 helper
  或兼容适配器实现。
- M4-C 已删除零生产调用方的 TUI `open_url` 及平台 browser-command 构造器；当前产品没有
  外链打开交互，不保留一套只有自身测试的系统命令能力。OAuth/MCP transport、终端文本复制
  和 DeepSeek HTTP 请求均不经过该 helper。
- M4-C 已删除零调用的 TUI `record_caught_panic`、`ensure_dir`、`pretty_json`、`url_encode`
  和 `estimate_message_chars`，并移除其中掩盖遗留的 `allow(dead_code)`；真实使用的受监督任务
  崩溃转储、原子/追加写入、路径展示和 `CountingWriter` 保留在现有调用方。
- M4-C 已删除从未进入固定 production catalog 的旧 TUI `js_execution`、Node resolver 与
  Doctor 假注册提示，并同步删除只服务该路径的 async/status dependency trait API。MCP 仍可
  按显式配置启动 Node server，canonical `run_verifiers` 仍覆盖 Node 项目；两者不依赖这套
  模型可见 JavaScript 执行器。
- M4-C 已删除从未接入 production catalog 的 TUI script-command plugin ToolSpec、复制其
  扫描逻辑的假 E2E、`[tools.plugin_dir]`/`[tools.overrides]` 配置和 `setup --tools`/Doctor
  脚手架；固定 canonical catalog 不再暗示可被本地脚本替换。真实 MCP、skills 与现有
  plugins 目录逻辑保持原调用边界，未被该删除重写。
- M4-C 已删除只有自身测试、从未被生产读取的 TUI `large_output_router`、`[workshop]`
  配置、`ToolContext` router/vars 字段和未接线的 V4-Flash synthesis 声明；canonical 工具
  artifact/evidence 存储不在该路径。旧 TUI spillover writer 另按真实调用图处理，不把两个
  不同 owner 混为一个切片。
- M4-C 已删除没有生产注册者或 producer、唯一 store 只由默认 `ToolContext` 空建的 TUI
  `handle_read`/`VarHandle` 原型及其 process-local store；canonical artifact、RuntimeEvent 与
  RunStore 不经过该路径。随该原型删除后失去最后调用者的 `CountingWriter` 也同步删除。
- M4-C 已删除零执行调用方的旧 TUI tool-output spillover writer/retriever、只由它消费的
  TUI 私有 artifact 文件和始终为 `None` 的展示字段，并移除 Doctor/启动 janitor/工具面文档
  中的假入口。正式 `ToolOutcome.artifacts`、确定性 verifier artifact、RuntimeEvent 与
  SQLite RunStore 继续构成唯一 canonical 证据和产物链。
- M4-C 已把 `key_shortcuts` 收缩为首启输入仍使用的 `is_text_input_key`，删除零调用的
  copy/paste/control-like/Ctrl-H 判定；正式文本粘贴继续由 terminal `Event::Paste` 处理，
  Pager 文本复制继续走 canonical local view event。
- M4-C 已删除零生产消费者且违反单 DeepSeek backend 边界的通用 `[vision_model]`/
  `image_analyze` 配置与 feature flag。DeepSeek Strict Function Calling 和 Beta FIM 不受
  影响；真实 `read_file` 本地 OCR 保留，且不会产生第二个远程模型请求面。
- M4-C 已删除首启状态机从不产生的 Provider 选择页、picker memory 与多 Provider 文案；
  真实首启直接进入 DeepSeek API Key/工作区信任/Tips，canonical `/provider` 仍被拒绝。
  通用 Provider 配置的最终 schema 删除仍留给 M7，不在本切片伪造兼容入口。
- M4-C 已删除 provider-lake 中零生产消费者的 configured-provider、picker model-list 和
  dashboard count 查询面。保留的 `catalog_offering_for_model` 仍服务现有 pricing，Codex
  route metadata 与 canonical Fleet 不经过已删除 API；通用 Provider catalog 的最终物理
  删除仍由 M7 完成。后续调用图又确认 live snapshot 只有模块自测 writer、生产始终为
  `None`，故同步删除其合并状态和自证测试；pricing 现在直接读取同一 bundled snapshot。
- M4-C 已删除零生产消费者的 `config::model_completion_names_for_provider` 硬编码模型列表、
  11 个专属列表测试、一个混合测试中的列表尾断言，以及只服务该列表的聚合/别名常量。
  默认模型、模型 alias/capability、Codex account roster、Fleet route receipt、bundled pricing
  和 DeepSeek `official_model_capabilities` 均不经过该旧 inventory API。
- M4-C 已继续删除旧 provider/model picker 的零生产消费者 adapter：display sorting、
  requested-model/route validator、wire-model wrapper、configured-provider 判定和 custom-kind
  convenience 方法及其自证测试。生产配置归一化、`resolve_route_candidate`、Fleet 路由回执、
  DeepSeek official model fail-closed 和 custom provider schema 保持原 owner。
- M4-C 已删除旧 provider setup UI 遗留的通用 bool/integer/provider-base-url/custom-provider
  TOML writers 及私有 normalization 闭包；这些函数只有自身测试调用。共享原子 TOML mutation、
  DeepSeek 首次配置、workspace trust、CLI login/logout 与 legacy approval migration 的生产写入
  路径继续保留并由原有验收覆盖。
- M4-C 已继续删除 provider-scoped API-key/model writers、targeted-key clear 和 Kimi
  credential-valid convenience predicate；它们同样只有自证测试或完全没有 caller。真实 CLI
  login/logout、DeepSeek 首启、Doctor key readiness 与 Kimi token refresh 不经过这些旧函数。
- M4-C 已删除从未接入请求执行的 TUI ProviderConfig `max_concurrency` 字段、alias、默认/夹紧
  逻辑及自证测试；canonical Runtime request budget、Fleet scheduler worker limits 和 subagent
  profile limits 保持各自生产 owner，不与该配置黑洞混淆。
- M4-C 已删除无调用方的 TUI retry-delay 与 search-provider convenience facade；DeepSeek
  transport retry projection 和 Doctor 使用的 typed search-provider resolution 保持原 owner。
- M4-C 已删除无调用方的 test-support prefix-diff 与 footer test-only parity helpers；回归测试
  改为直接验证生产 `render_footer_from -> FooterProps` 的 context-percent/session-cost 路由，
  不再由一套测试 helper 模拟真实 widget 布局。
- M4-C 已把仅供测试互调的公开 theme inventory/setting/mode-label helpers 收缩为 test-only
  shipped-theme 清单；真实 settings 主题启动、12 套 palette、终端适配与 Ocean 渲染未改动。
- M4-C 已删除从未接入 transport 的 TUI 私有 `http_headers` 字段、env merge、accessor 与
  自证测试；canonical config 仍残留的 generic header schema 留待 M7，生产 DeepSeek transport
  当前不消费任意 custom headers。
- M4-C 已删除 pricing/route-billing 中只有模块自测消费者的 route/child/compact-chip facade、
  `CostEstimate` convenience methods 和重复 catalog predicates；回归测试改为直接验证生产
  `for_route -> usage_chip -> format_usage_line`。官方 DeepSeek pricing、canonical accounting、
  `RunStore`、`/cost`、footer/sidebar、scorecard、Runtime/Fleet 预算与 child usage 聚合均保留。
- M4-C 已删除无任何 HTTP fetch、事件、Store 字段或 writer 的 DeepSeek account-balance 幽灵链：
  DTO、App `None` cell、可配置 footer item、余额布局、文案和六个自证测试均已物理删除。旧
  `status_items = ["balance"]` 由现有 tolerant unknown-item 规则自然忽略；canonical usage/cost、
  cache、scorecard、`/cost` 与 root/child accounting 保持原 owner。
- M4-C 已删除没有生产 writer 的 TUI `subagent_cost`、cost high-water 和本地 accrue 账本，
  sidebar 不再展示永久为零的假 `session + agents` 拆分。保留的 view scalar 已诚实命名为
  `total_cost_usd/cny`，唯一 writer 是 canonical Run presenter；`/cost`、footer、phase strip、
  sidebar 和 CNY fallback 直接读取 root+child 聚合总额。Header 中零-reader 的 cost 参数同步删除。
- M4-C 已删除只有自证测试调用、从未在生产启动或 onboarding 后触发的 Fleet-ready nudge，
  连同其 `feature_intro_shown` 持久化字段与专属文案一起物理删除；真实 Fleet、多 Agent、
  onboarding 和空状态不依赖该提示。
- M4-C 已删除只有 App/Approval 自测互相调用、没有 canonical key、slash command 或 Run event
  producer 的动态 mode/permission 循环状态机：`set_mode`、Tab/Shift-Tab cycle、Agent baseline、
  policy-lock UI mirror 及对应设置写入均已物理删除。后续调用图确认 `AppMode/default_mode`
  只剩启动标签、颜色与错误的 Plan“只读”提示，现已连同 legacy 设置迁移、Doctor 字段和渲染
  分支物理删除；旧 `default_mode` 被忽略且不能授予权限。真实显式 `--yolo` 输入仍直接
  投影 shell、自动批准和工作区外访问控制，不依赖模式标签；多 Agent/Fleet 不读取该旧壳。
  审批请求同时删除无人读取的英文 impact 副本，只保留根据 canonical tool/risk 输入生成的
  简体中文展示摘要；该摘要不是策略、证据或风险真相。
- M4-C 已把 TUI 审批投影从四个标签、三个重复行为收敛为真实 `Ask/AutoApprove` 两态，精确
  映射 canonical `RunProductControls.auto_approve=false/true`。`approval_policy` 只接受
  `on-request|auto`；旧 `untrusted/never/suggest/auto-review/full-access` 不再兼容。
  `Settings.permission_posture`、managed-lock 镜像和 saved-posture project baseline 已删除，
  配置、Doctor、project tightening、footer/header 只读取同一个 approval owner。trust、Shell、
  sandbox、durable approval/RunStore 与 Fleet 均保持独立真实语义。
- M4-C 已把 headless `exec --auto` 从工作区外路径信任中解耦：`--auto` 只启用工具与自动批准，
  不再把 `trust_mode` 置为真；Fleet worker 保留必需的 `exec --auto`，但不会因该 argv 自身
  获得 unrestricted external-path trust。明确的 canonical `trust_mode`、当前显式 yolo 输入、
  workspace-scoped trusted roots 以及已持久 Run 的恢复语义均保留，本切片不混入 managed
  requirements。真实 exec/HTTP/stdio surface parity 证明 `auto_approve=true`、
  `trust_mode=false`，Fleet argv 与 CLI help 定向回归同时通过。
- M4-C 已删除零调用方的 `.codewhale/constitution.json` RepoLaw 编译器、`globset` 依赖和
  从未进入 canonical Runtime/tool gate 的 `RepoLawRule/RepoLawAction`；constitution 的真实
  生产能力继续只作为简体中文 system-prompt guidance，由相关 `paths` 标注作用范围，不再
  谎称 Host 机械强制。TUI 同步删除没有任何 Runtime producer 的英文前缀识别、特殊审批皮肤、
  专属文案和自证测试，并删除只为该假路径保存却从不展示的原始英文 description 副本；普通
  durable approval、risk、intent、参数预览、execpolicy deny 和 canonical Run 投影均保留。
  Context 17/17、TUI approval 35/35、汉化目录 9/9 通过，context all-target 与 TUI production
  bin 严格 Clippy 均无 warning。
- M4-C 已物理删除 sibling `permissions.toml` 假策略闭环：TUI `Config` 和共享 `ConfigStore`
  虽会解析、合并、持久化并自测 typed rules，但没有 production consumer 把该 engine 注入
  canonical `ProductionToolConfig`，所以旧文件唯一真实效果是让无关启动因解析错误失败。
  现已删除 schema、loader、writer、路径 API、`ConfigReload` 假协议、示例/文档承诺及 config/TUI
  对 `codewhale-execpolicy` 的无效依赖，共净删约 1,000 行。另一条真实
  `~/.deepseek/execpolicy.toml -> production_snapshot -> ProductionToolConfig -> Shell host`
  deny-before-auto 链完整保留，并新增配置到 canonical snapshot 的桥接回归；config 348/348、
  protocol 71/71、TUI config 216/216、execpolicy 4/4、tools deny 回归、CLI lib 75/75 与
  dispatcher canonical 集成 5/5 通过，
  config/protocol all-target 与 TUI production bin 严格 Clippy 无 warning；focused 继续通过
  tools 299/299、DeepSeek 35/35、Runtime 53/53、App 38/1 ignored、app-server 23/23、exec
  terminal 24/24、canonical Run 19/19 与真实 PTY 7/7。
- M4-C 已删除 TUI 私有 managed config/requirements 假权限上限：它只在 `Config::load`
  检查显式写入的 approval/sandbox 字段，未进入 canonical `ProductionComposition`，因此
  `exec --auto`、HTTP/stdio、恢复/继续和 Fleet 均可绕过；默认值为空时也不受约束。该路径
  既不是自用 DeepSeek 产品范围，也不能作为 Host authority ceiling。现已删除 schema、
  `/etc/deepseek` 默认路径、环境变量、merge/load/check、项目 overlay 特判、自证测试和配置
  文档，不用一套表面策略冒充生产强制能力。真实 canonical approval、sandbox、trust、Shell
  execpolicy、RunStore 和多 Agent 权限输入保持原 owner；若未来确需不可绕过的宿主上限，必须
  经新 ADR 在唯一 application composition admission 处实现，而不能恢复 TUI 配置期检查。
- M4-C 已物理删除 400 余行从未接入启动、按键处理或设置写入的 `TuiPrefs`/`KeybindPrefs`
  与 `tui.toml` 读写原型。该文件的唯一生产调用只是解析 TOML 后显示“配置损坏”警告，实际
  theme、font size 和 keybinding 从未消费，因此不能算作可配置 UI 能力。同步删除专属自证
  测试、假警告和文档承诺；真实 `settings.toml`、主题选择、生产按键 handler、终端字体和
  canonical TUI Run 投影不变，已有用户 `tui.toml` 不主动删除也不再读取。
- M4-C 已删除 Fleet 中旧的“模型生成 Agent Profile 草稿”编译孤岛：草稿 DTO、
  不可信 JSON 抽取/清洗、TOML 渲染、文件名生成与专属自证测试在 setup UI 删除后已无
  production caller，只靠文件级 `allow(dead_code)` 留在构建中。同步删除三个只服务旧
  authoring 流程的 strict/identity 公开包装，保留 `load_workspace_agent_profiles_tolerant ->
  FleetRoster -> FleetManager -> canonical exec worker` 真实链、Profile 身份/权限校验和多 Agent
  能力；不恢复模型草稿 UI，也不新增兼容包装。
- M4-C 已把 Fleet executor 自测迁到真实生产入口，并删除无 Profile/Host 语义的
  `build_worker_exec_command`、`start_worker`、`poll_terminal`、`all_terminal` 便利包装及其
  prompt facade。生产与测试现共用 `build_worker_exec_command_with_profiles ->
  start_worker_on_host -> poll_terminal_with_status -> forget_worker`，真实进程和并发 worker 回归
  14/14、worker route/prompt 回归 7/7 通过；保留 Fleet Manager、Ledger、Local/SSH Host 和
  `codewhale exec -> AgentApplication -> AgentRuntime` 的 canonical 多 Agent 链。
- M4-C 已删除 TUI `App` 中旧 foreground Engine 退役后只初始化/清空、没有任何
  production producer 或 reader 的 `project_doc`、`dispatch_started_at`、
  `ignored_tool_calls` 和 `pending_tool_uses`。真实 project instruction 继续由 canonical
  prompt composition 拥有，tool prepare/outcome、turn lifecycle 和 terminal 继续只由
  `CanonicalRunProjection -> run_presenter -> App` 投影；不建 UI 镜像状态或兼容双写。
- M4-C 已删除 Fleet Roster 三个只有自测消费的 `built_ins_only`/`get`/
  `model_overrides` facade 与专属测试，并去掉文件级 `allow(dead_code)`。真实
  `FleetRoster::load -> members -> FleetManager -> profile-aware canonical exec worker` 合并优先级、
  容错加载、内置角色权限下限和多 Agent 调度链保留，不建立测试专用生产 API。
- M4-C 已按精确零引用证据删除 palette 中 6 个未消费的 RGB/语义常量及其
  `allow(dead_code)`；真实 `UiTheme`、命名主题解析、浅色/深色对比和
  `ColorCompatBackend` 完整保留，本切片不改变任何可见颜色。
- M4-C 已删除 `crates/context` 中无任何 workspace caller 的 monorepo merge、默认
  Compatible skill discovery/render 和旧 prompt composition facade，并把 flat prompt helper 收缩到
  `cfg(test)`。生产仍只走 `ProductionComposition -> production_system_prompt ->
  system_prompt_for_mode_with_context_skills_and_session -> explicit discovery mode`，项目指令、
  Skills、stable/volatile cache block、handoff 与 canonical DeepSeek prompt bytes 不变；本切片不实施
  M5 ContextBroker 或新 compaction runtime。
- M4-C 已将 TUI execpolicy 收缩为两条真实链：`execpolicy check` 继续通过
  Starlark parser 与 prefix rules 输出 JSON，`~/.deepseek/execpolicy.toml` 继续只解析为
  `ProductionExecPolicySnapshot` 并在 canonical Shell host 执行。现已删除无消费重导出、
  只有自证测试的 TUI TOML evaluator/matcher、heuristics fallback 和 `Evaluation`，并去掉
  整模块 `allow(dead_code/unused_imports)`；新增真实 Starlark load/match/JSON 回归，不复活
  sibling `permissions.toml` 或第二套 Agent 执行策略。
- M4-C 已删除不控制任何生产能力的 `shell_tool`/`web_search`/`apply_patch`/`mcp`
  假 feature flag、Doctor 假 MCP 开关和 lifecycle metadata，并删除文档中不存在的内建
  browsing/compatibility alias 承诺。`[features]` 现只保留有真实 caller 的 `subagents` 和
  `exec_policy`：前者控制 canonical `agent` 目录与 depth/concurrency，后者控制 Shell
  policy snapshot 加载；Shell、patch 与 MCP CLI 仍由各自真实 owner 控制，不通过假开关。
- M4-C 已物理删除整条无生产 reader 的 Notes 配置投影：`Config.notes_path`、环境与项目
  overlay、默认/旧路径解析和从主入口传入后立即丢弃的 `TuiOptions.notes_path`，并同步
  删除“model-visible note tool”的错误文档承诺。评测 Harness 独立创建和读取的
  `SeedWorkspace.notes_path` 保持不变；canonical 固定工具目录从未包含 `note`。
- M4-C 已删除仅由 MCP 自测调用、重复安装 rustls provider 的 `tls::reqwest_client`
  门面；自测与生产 transport 现都从同一个 platform HTTP client builder 构建客户端，
  async/blocking builder 和 MCP 网络行为保持不变。
- M4-C 已删除 Fleet alerts 中六个没有生产 caller 的事件 factory 及两条自证测试，并移除
  整模块 `allow(dead_code)`。生产 `fleet alert-dry-run` 继续直接构造事件并通过真实
  dispatcher、HTTPS adapter、脱敏和 inspection command 链；Ledger、scheduler、worker
  生命周期和 canonical 多 Agent 执行均未改变。
- M4-C 已删除没有任何生产 writer 的 MCP manager snapshot DTO、formatter、App 缓存、
  restart hint 与伪连接健康配色；footer/sidebar 只投影启动时真实加载的配置数量。保留的
  顶层 `codewhale mcp` CLI 继续承担配置、OAuth、stdio/Streamable HTTP/legacy SSE、连接
  和工具发现；当前 canonical Agent 固定工具目录没有加载 MCP pool，因此文档不再把 CLI
  discovery 误写成 model-visible 工具或 TUI `/mcp` manager。随后又删除了 TUI binary 私有
  `mcp` 模块内八个零生产消费者的“public API” wrapper、仅供测试读取的 shutdown report 与
  `#[allow(dead_code)]`；配置 reload、reconnect、transport shutdown 和 Drop 清理保留。
- M4-C 已把 `setup --mcp` 与 `mcp init/add/remove/enable/disable` 收口到现有 TUI
  `mcp` 模块的唯一配置 owner，物理删除 `main.rs` 重复的模板、读取、初始化和原子写入函数。
  `add_server_config` 直接接收完整 `McpServerConfig`，不再通过拆散参数丢失 headers、bearer、
  OAuth、scopes、resource、timeout、tool filter 或 transport 字段；写命令只读写 resolved
  global `mcp.json`，不会把 workspace/plugin merged inventory 反写。`list`、`login`、
  `logout`、`connect`、`tools` 和 `validate` 继续使用 workspace-aware 读取；OAuth、
  network/TLS、stdio/HTTP/SSE 和 MCP execution 方法均未在本切片改动。MCP config 21/21、
  OAuth 6/6、HTTP auth 2/2、隔离环境
  真实 CLI 2/2、canonical Run 19/19、PTY 6/6 和 TUI all-target check 均通过。
- M4-C 随后按真实调用图删除 MCP execution ghost：`McpConnection`/`McpPool` 中没有任何
  production caller 的 `tools/call`、resource read/list/template、prompt list/get、prefixed-name
  dispatch、动态 runtime server 和 stale-session execution retry 已物理删除，连接初始化只
  广告并发现 `tools/list`。顶层 `codewhale mcp list/connect/tools/login/logout/validate`、
  config merge/reload、workspace trust/plugin、OAuth、network/TLS/proxy/header、stdio、
  Streamable HTTP/legacy SSE、JSON-RPC framing 和 transport shutdown 保持原 owner。canonical
  Agent 仍没有加载 MCP pool，因此该删除没有移除可用的 Agent 工具执行能力；若未来要把 MCP
  工具纳入 Agent，必须经 canonical `crates/tools`/Runtime/RunStore/approval 新建纵向切片，
  不能恢复私有旁路。`execute_timeout` 仍作为完整配置往返字段保留，但当前没有 execution
  runtime consumer。当前门禁为 MCP 定向 79/79（另有 1 个既有 flaky TCP listener 测试
  ignored）、真实 CLI 2/2、OAuth 6/6、HTTP auth 2/2、canonical Run 19/19、PTY 6/6，
  并通过 TUI all-target check、fmt 和 diff-check。
- M4-C 已删除整模块以 `#[allow(dead_code)]` 隐藏、从未接入任何生产 caller 的 TUI
  `ResourceTelemetry`/budget pressure/估算吞吐 foundation，以及 App 中只会初始化和清空、
  从不写入或读取的 `last_output_throughput`。canonical Runtime/RunStore usage/accounting、
  presenter 的 token/cache/reasoning/cost 投影和 DeepSeek 官方 usage 账本均保留；本切片不以
  一个未接线的第二遥测模型冒充预算或性能能力。
- M4-C 已删除同样整模块以 `#[allow(dead_code)]` 隐藏、说明中明确“等待以后接线”的 TUI
  `ContextBudget` foundation，以及唯一引用它但自身零调用的 `route_context_budget`。
  `route_context_window_tokens`、canonical DeepSeek output limit、`crates/context` projection/
  compaction 和 Runtime/RunStore 预算恢复语义均保留；M5 不通过复活未接线的 TUI 预算模型
  建设第二套 ContextBroker。
- M4-C 已删除 `route_runtime` 中没有生产调用方的 `ResolvedRuntimeRoute ->
  resolve_runtime_route -> prepared_route_config` 配置快照包装链、其私有 base-URL 猜测器和
  五个只验证该死链的自测；同时删除全仓零调用的 `route_output_limit_tokens` wrapper。
  `resolve_route_candidate` 仍由交互 TUI 与 Fleet route receipt 直接消费，Codex route
  metadata、`known_route_limits`、`route_context_window_tokens`、pricing/provider-lake 与
  所有 Provider 配置保持原生产 owner；通用 Provider 最终清理仍属于 M7。
- M4-C 已删除从未被生产事件循环刷新、运行期永远为 `None` 的 TUI workspace/git cache、
  后台 cell、TTL 和整套 `workspace_context` 模块。旧 footer/empty-state 因此会把真实 Git
  仓库谎报为 `(no git)`/“无 git”；现在 `StatusItem::GitBranch` 断代改为只接受
  `workspace` 的 `StatusItem::Workspace`，footer/sidebar/empty-state 直接投影 canonical
  `App.workspace` 并显示“工作区”。旧 `git_branch` 配置键不保留 alias；真实
  `git_status`/`git_diff`、Run workspace guard、Fleet branch/worktree 字段不变。
- M4-C 已把 `TerminalInputPump` 收缩为唯一真实链路：后台限时 poll/read、`recv_timeout` 和
  Drop 清理。删除旧 Engine 留下且零调用的非阻塞/pending-drain helper、heartbeat/liveness、
  child-terminal pause/ack、detached restart、dispatch/turn/tool watchdog、recovery snapshot、
  pause/resume terminal 及只由自身测试消费的 focus helper。onboarding 和 canonical loop 的
  Key/Paste/Mouse/Resize/Focus 事件不变；`run_events.try_recv -> CanonicalRunProjection ->
  presenter` 是另一条保留链，未被同名旧 input helper 误删。
- M4-C 已删除不可达的 pre-session Launch menu。`App` 虽会按 `launch_screen` 设置构造
  `LaunchState` 并无条件执行一次 `git rev-parse`，但 canonical foreground 在首帧和输入线程
  启动前立即把它强制隐藏；launch key/mouse action、worktree/resume/changelog/quit 分派和
  session count 从未有生产消费者。该状态、设置、本地化、renderer、自测岛和无效 Git probe
  现已物理删除，没有生产行为损失。onboarding、canonical `TuiRunClient`/Run 投影、CLI
  resume/continue、underwater shell/ocean 以及真实 Fleet/Lane/worktree 能力保持原 owner。
- M4-C 已删除零生产调用方的 TUI desktop notifications 岛：OSC/BEL/macOS 发送、Windows
  声音、terminal title/taskbar、配置决策和专属本地化全部只由模块内自测消费，从未接入
  canonical turn/child 终态，因此删除没有生产行为损失。唯一模块外消费者
  `humanize_duration` 已迁入 footer owner 并保留秒/分/时/日/周边界测试；只服务该幽灵岛的
  `[notifications]`、`tui.notification_condition` 和 Windows Audio/Debug/UI features 同步
  删除。MCP JSON-RPC notifications、Fleet alerts/webhooks、canonical Run 状态及 panic hook
  是不同 owner，均保持不变。
- M4-C 已删除系统剪贴板读取/图片落盘和伪 composer attachment 岛。生产只调用
  `ClipboardHandler::write_text`；`read`、image PNG、`PastedImage`/`ClipboardContent`、
  App paste/attachment/selection 方法和手写 `[Attached ...]` parser 只在死方法、自测与伪
  renderer 状态内闭环，因此没有生产行为损失。arboard 继续以纯文本写入模式服务 Pager
  copy，OSC52/wl-copy/pbcopy/PowerShell fallback 保留；terminal `Event::Paste`、onboarding、
  普通 `@mention` 和 `read_file` OCR 仍是原生产 owner。不存在的 `/attach` 与 Ctrl-V 图片
  能力声明已删除，TUI direct `image` dependency 也随唯一消费者移除。随后独立调用图切片
  确认 rapid-key paste-burst handler 从未被 canonical Key 事件调用，每帧只轮询永不激活的
  默认状态；现已删除该状态机、App 空轮询、设置/别名和自证测试。无 bracketed marker 的
  原始字节与快速键入不可区分，因此不再宣称 trailing Enter 可被启发式拦截；QA 改为证明
  快速普通按键不丢失且仍可编辑。`Event::Paste`、API-key onboarding、bracketed terminal
  mode、CRLF/裸 CR 归一化、超大粘贴路径与 Pager copy 保持原真实 owner。
- M4-C 已把审批事件收缩为真实的 `interaction_id + decision`，继续经 canonical
  `resolve_interaction`/`cancel` 写入并重放 RunStore。删除从未有 Runtime 消费者的 TUI
  approval cache/grouping key、永远未设置的 timeout/tick 链，以及虚假的“批准并保存询问
  规则”动作、预览和空 `tools` 模块；工具名、风险、参数、`y/n/Esc/v`、Pager 复制和 durable
  interaction 语义均保留。模态框鼠标接线属于独立行为切片，不与本次真相清理混合。
- M4-C 随后的独立行为切片已把 canonical `Event::Mouse` 先路由到活动 modal：Approval
  左击产生同一 canonical decision，滚轮只移动 modal 选择，Pager 沿用自己的滚动处理，且
  同一事件不再穿透到底层 transcript/sidebar/composer；没有 modal 时原 transcript 三行
  滚动行为不变。该接线不增加第二事件状态机。
- M4-C 已删除滚动 owner 中两组未接线叶子：`TranscriptScroll::anchor_for` 只有自身测试，
  rapid mouse acceleration 状态也只被 `ViewportState` 默认构造、从未读取。canonical mouse
  继续直接提交固定三行 delta；`pending_scroll_delta`、`resolve_top`/`scrolled_by`、键盘滚动
  和 Pager 自有 mouse/Vim 键均保持原生产路径。
- M4-C 已删除 `ColorCompatBackend` 中没有任何生产 writer、只由 3 个自测激活的
  forced/cached size override 字段与 setter。`Backend::size()` 现在直接委托真实 Crossterm backend；
  颜色深度适配、palette/theme 动态更新与 OSC8 link 输出保持原生产路径。
- M4-C 已删除没有 Key/Mouse/Run event producer 的 transcript selection/autoscroll 模块、
  默认空状态、自动滚动 guard、resize clear 和 renderer 着色岛，并移除只直接调用私有着色函数的自测。
  普通 composer cursor/layout、菜单/审批选中态、系统文本复制、canonical transcript/scroll 与 Pager 保持原 owner。
- M4-C 已删除随上述 transcript selection/copy 生产路径一同失去 reader 的 copy metadata writer-only 管线：
  `CopyLineSeparator`、soft-wrap separator、装饰 prefix width 曾跨 markdown/history/cache/
  `TranscriptLineMeta` 逐行计算与传递，却没有生产消费者。字段、计算器、cache 数组与专属自测已物理删除；
  真实 render metadata helper 已按职责重命名，继续传递 `Line`、links、`is_code` 和 cell/line 映射。
  OSC8、Pager/审批系统复制、普通 composer cursor/layout、scroll/cache 渲染均保持原 owner。
- M4-C 已继续删除只由自身测试或上述退役 copy/export 假路径调用的 `ui_text`
  `history_cell_to_text`/`line_to_string`/`line_to_plain`/`append_spans_plain`/`slice_text`。
  沿调用图成为零消费者的 `HistoryCell::transcript_lines`、`GenericToolCell::transcript_lines`、
  `osc8::strip_into` 与专属常量/自测同步物理删除；没有把不存在的 transcript export 或 clipboard
  consumer 写成产品能力。中文/CJK 宽度回归已迁到真实 `text_display_width` owner；
  `RenderMode::Transcript` 的失败工具 uncapped 路径、ANSI 清理、OSC8 生成/链接区域/发送以及
  Pager/审批复制、普通 composer cursor/layout、render/cache 均保持原生产路径。
- M4-C 已删除从未被 canonical key handler 调用的 composer Vim 孤岛：
  `vim_mode.rs` 的 Normal-mode handler 没有任何生产调用方，设置值只能构造 App 状态并显示顶栏标签。
  模块、App helper/字段、设置/别名/列表、标签本地化和 widget 分支已物理删除；普通 composer 输入/cursor/渲染、
  canonical 键盘路由和 Pager 自有 `j/k/g/G/y/q` 保持原 owner，可打印字符 `v` 仍为 composer 正常输入。
- M4-C 已删除 sidebar 每帧构造但从未被事件处理器、popover 或 renderer 读取的
  `SidebarHoverState`/section/row/action 元数据、全文副本和 tooltip shadow，以及从未被
  构造的 `SidebarAgentCancel` 事件。Activity/Agents/Session 的可见行继续由原 renderer
  直接生成；canonical child/Fleet 投影、modal 鼠标和 transcript 滚动均保留。后续调用图已
  证明 `last_sidebar_area` 同样只有 renderer producer 并将其删除；当前只保留可见分隔线，
  resize 状态是否接通或删除需独立切片决定。该切片不把 producer-only 点击描述误当成真实
  多 Agent 控制能力。
- 该删除切片的 focused gate 已通过：Runtime conformance 53/53、DeepSeek 35/35、
  app 37 passed/1 ignored、app-server 23/23、exec production loopback 24/24、
  canonical TUI Run 20/20、PTY 5/5；State `run_store`、CLI canonical runs 与 TUI unit
  门禁也通过。exec 多 Agent fixture 同步锁定已接受的 eager join：单层 3 次请求、嵌套
  5 次请求，不再要求已经删除的无信息 wait 轮。
- focused 的 TUI 子集已迁移到 canonical command、Run client/projection/presenter、本地
  approval、Fleet 和 DeepSeek Doctor；旧 memory、旧 schema sanitizer、旧 model client、
  stream decoder 和 legacy route 测试不再冒充当前核心门禁。过滤器仍先按 `--list` 校验，
  任一零匹配继续 fail closed。
- M4-C 已删除 canonical TUI presenter 持续双写但没有生产语义消费者的
  `ToolDetailRecord`、`tool_details_by_cell` 和永远没有写入方的 `active_tool_details`，同步
  移除历史前缀重键、active flush 搬运与 replay 自测对这份影子详情账本的依赖。工具展示与
  重放继续直接比较 `GenericToolCell` 的名称、状态、参数摘要、输出、输出摘要和 diff 标记；
  `tool_cells` 的 prepared→outcome 原位更新、canonical `RuntimeEvent`/`ToolOutcome`、
  `ActiveCell` 以及 child/Fleet 能力均保持原 owner。定向 presenter 13/13、canonical Run
  19/19、PTY 6/6、TUI all-target check、fmt 和 diff-check 均通过。
- M4-C 已删除没有任何 production producer、只由 4 个 renderer 自测直接构造的
  `HistoryCell::Error`，以及只服务该变体的标签、样式、纯文本换行 helper 和穷尽匹配分支。
  模型请求失败和 terminal failure 继续由 typed `RuntimeEvent` 经 canonical presenter 投影；
  child/system 消息仍使用 `HistoryCell::System`，失败工具仍由
  `HistoryCell::Tool(GenericToolCell)` 与 `ToolStatus::Failed` 完整展示，session diagnostics
  仍保留 `error_taxonomy`。定向 history 72/72、transcript cache 1/1、presenter 13/13、
  canonical Run 19/19、PTY 6/6、TUI all-target check、fmt 和 diff-check 均通过；Widgets
  全模块另有 2 个与本切片无调用关系的既有空状态文案断言失败，未越界修改。
- M4-C 已删除从未被 canonical presenter 或其他 production producer 构造的
  `ToolStatus::Hydrated`，以及 theme/history/sidebar 的穷尽展示分支和唯一直接断言。
  当前工具展示状态只保留真实 lifecycle 的 `Running`/`Success`/`Failed`；这次删除不涉及
  protocol/runtime/state/app 的 `RunReplay`、resume、SQLite `RunStore` 重建或 durable
  hydrate 语义。定向 history 72/72、theme 5/5、sidebar 45/45、presenter 13/13、canonical
  Run 19/19、PTY 6/6 和 TUI all-target check 均通过。
- M4-C 已删除 `tool_output` 内零外部消费者、也没有专属测试的
  `McpOutputSummary`/`summarize_mcp_output`/`output_is_image` 内部闭环。canonical presenter
  继续通过 `summarize_tool_output` 更新 `GenericToolCell`，共享 `truncate_text` 和真实工具
  输出渲染保持不变；MCP config/connect/tools/OAuth 及 stdio/HTTP/SSE transport 不经过该
  TUI helper，均未改变。定向 history 72/72、tool-output renderer 1/1、presenter 13/13、
  canonical Run 19/19、PTY 6/6 和 TUI all-target check 均通过。
- M4-C 已删除只由自身测试调用的 `footer_ui::one_line_summary`。真实 footer/sidebar/tool
  output 摘要、截断和 `strip_ansi_into` 路径均保留；footer 9/9、canonical Run 19/19、
  PTY 6/6 与 TUI all-target check 通过。
- M4-C 已删除没有任何 production producer、只在 TUI 自测中直接构造的 `ActiveCell`。
  canonical presenter 原本已经把 `ToolPrepared` 直接写入 `App.history`/`tool_cells`，再由
  `ToolOutcomeCommitted` 原位更新同一 `GenericToolCell`；因此 active module、三个 App 字段、
  virtual transcript/cache、footer/sidebar/phase 的 active 分支及其自测和本地化键现已物理删除，
  tool-run 检测只读取 canonical history。terminal 不会凭终态补写工具结果，不能把缺少
  `ToolOutcome` 的真实 `Running` 工具推断为 `Failed`；无真实等待来源时也不激活旧路径从未
  展示的 stall 文案，两条红线均有回归测试。
  child/Fleet、canonical `RuntimeEvent`/`RunStore`、cancel/control 与工具输出展示均未改变。
  定向证据为 presenter 14/14、history 72/72、sidebar 42/42、footer 10/10、phase 22/22、
  指定 widget 5/5、canonical Run 19/19、PTY 6/6，并通过 TUI all-target check、fmt 和
  diff-check。
- M4-C 当时已把交互 TUI 的 provider/model 收敛为单一 DeepSeek 入口真相：user、workspace、
  project 配置合并后，非官方 DeepSeek Provider 或非当时准入的 `auto`/`deepseek-v4-pro`/
  `deepseek-v4-flash` 模型会在 raw terminal、RunStore 和 HTTP 之前以简体中文失败；TUI
  入口还会二次校验 `TuiOptions` 没有偏离同一配置投影。旧的启动后强制改写 Provider、
  `Settings.default_provider`/`provider_models`/`default_model` 路由覆盖和 App 私有
  `provider_models` 状态已删除。`AgentApplication` 同时在创建 reservation 之前校验所有入口
  的显式模型，非法模型不会留下 pending creation。当时的 `auto` 由 production DeepSeek planner
  决定官方模型；onboarding 只持久化/安装官方 DeepSeek Key，并幂等写回同一个 DeepSeek
  Provider，不改变已校验模型路由。最终集成门又证明通用 `Config::default_model` 会把显式
  外国模型静默回落到默认 V4 Pro；交互入口现先读取 provider-scoped/root 的原始显式值，
  只有确实未配置时才使用默认模型，并在任何回落前完成官方模型校验。过期的 Z.ai PTY
  fixture 同步改为官方 DeepSeek dispatcher 配置。通用 Settings/Config schema 的物理清理仍属于 M7，
  本切片不声称 FIM transport 已完成；该历史 Auto surface 已由 M9-D/ADR-0008 直接
  supersede 并删除。定向与 focused 证据为 App 38/38（另 1 个外部进程
  helper 忽略）、Runtime conformance 53/53、DeepSeek 35/35、工具 299/299、exec 24/24、
  canonical Run 19/19、canonical PTY 7/7；集成修复后的 TUI bin 为 1,537/1,537（另 1 个
  忽略）、通用 PTY 为 9/9，并通过 app/TUI all-target check、fmt 和 diff-check。
- M4-C 已删除零调用的 exec stream-json 旧 stdout 直写 helper；生产内容、工具结果、子 Agent
  lifecycle 与 terminal receipt 继续统一经 `ExecOutput` 队列、`exec_stream_line` 和
  `write_exec_stream_terminal` 输出，保留背压、terminal acknowledgement 与有界关闭语义。
  exec stream、child receipt 和真实 terminal NDJSON 验收各 1/1，并通过 TUI all-target check、
  fmt 和 diff-check。
- M4-C 已删除只由模块自测构造、没有生产边界消费者的 `ErrorEnvelope`、`ErrorSeverity`、
  全部 envelope constructor/Display/Error 实现与 `From<ToolError>` 转换。会话诊断仍通过
  `session-diagnostics -> classify_session_failure -> classify_error_message` 使用保留的
  `ErrorCategory` 与分类器；精确 DeepSeek invalid/reasoning replay 错误仍优先归为
  `InvalidInput`，API Key 认证仍先于普通授权，timeout 仍先于 network，rate-limit 仍先于
  authentication，invalid-input 仍先于 tool。定向 taxonomy 18/18、session diagnostics 7/7
  通过，并通过 TUI check、fmt 和 diff-check。
- M4-C 最终调用图证明 Classic header/footer/sidebar 整帧链没有生产 renderer consumer；
  唯一仍需的状态标记已迁入 Underwater shell 后，Classic shell 与其 hover/resize/宽度设置、
  专属测试和本地化消息一并删除。Underwater 是当前唯一交互外壳，canonical child/Fleet、
  WorkSurface、modal、transcript、审批、工具卡和 PTY 输入链保持原 owner。该切片删除
  6,384 行并把简体中文消息目录从 381 项收缩到 170 项，没有保留第二套 UI 兼容路径。
- M4-C 已删除零 production caller 的 `semantic_truncate_with_affixes`、只被它和一条自测调用的
  `semantic_truncate_between_affixes`，以及该自证测试。真实 modal title 仍使用
  `semantic_truncate`；footer/sidebar/work-surface/thinking 仍使用 `truncate_line_to_width`，
  中文/CJK、组合字符、ZWJ、控制字符与窄终端宽度契约均保留。定向 ui-text 11/11 通过，并通过
  TUI check、fmt 和 diff-check。
- M4-C 已删除 `SidebarAgentRow` 中只写不读的 `role`、仅由两个自测调用而生产从未执行的
  `sort_sidebar_agent_rows_as_tree`，以及零调用的 running-status helper。canonical
  `ChildStarted`/`ChildFinished -> child_agents -> sidebar_agent_rows -> subagent_panel_rows`
  仍是唯一真实 sidebar 子 Agent 链；`parent_run_id`、`spawn_depth`、`agent_tree_prefix`、终态矩阵、
  handoff 与 Fleet 投影均保留。删除的是假覆盖，不是多 Agent 能力。定向 sidebar 40/40、Run
  projection 6/6 通过，并通过 TUI check、fmt 和 diff-check。
- M4-C 已将生产 `SidebarAgentRow` 收缩为调用图中真实存在的 `parent_run_id`、`spawn_depth`、
  `name`、`status` 四项，并删除没有可达展示消费者的 id/progress、固定为空或零且没有交互
  producer 的 model/objective/branch/steps/duration/expanded、不可展开的 dossier 分支和不存在的
  `agent:<id>/full_transcript` handle。canonical `ChildFinished.handoff_content` 仍由 presenter
  投影到 transcript，子 Agent 事件顺序、层级、七态终态矩阵、中文宽度、Fleet worker 与父子/
  孙 Agent terminal 汇合均保留。定向 sidebar 33/33、presenter 14/14、Run projection 6/6、
  六子 Agent fanout 1/1、两项 exec 多层汇合各 1/1、Fleet worker 1/1 通过，并通过 TUI check、
  fmt 和 diff-check。
- M4-C 已进一步删除 `SidebarSubagentSummary` 中生产永远为 `0`/`None` 的
  `progress_only_count`、`fanout_total`、`fanout_running`，以及唯一通过手填 `Some(6)` 自证的
  navigator 测试。Agents header 现在只读取 canonical `child_agents` 生成的 total/running 和
  depth role 统计。真实 Runtime pending children、`ChildStarted`/`ChildFinished`、六子 Agent
  fanout、exec 父子/孙级汇合与 Fleet worker 均未进入该假 summary 路径。定向 sidebar 32/32、
  presenter 14/14、Run projection 6/6、六子 Agent fanout 1/1、两项 exec 汇合各 1/1、Fleet
  worker 1/1 通过，并通过 TUI all-target check、fmt 和 diff-check。
- M4-C 已删除零调用的 `braille_spinner_frame_for_duration_ms` 薄包装。生产 running-tool 标记
  继续由 `braille_spinner_frame(Instant)` 计算真实 elapsed，底层共享 cadence、400ms quick-event
  门槛、low-motion 静止帧和 verify tick 均保留。spinner 3/3 通过，并通过 TUI check、fmt 和
  diff-check。
- M4-C 已删除没有 production command/consumer 的通用 `Settings::set`、`apply_preset`、
  `display`、`available_settings`、calm preset 和它们的专用解析/本地化/自证测试。运行时仍直接
  加载并归一化 `settings.toml`；M7 暂留的 Provider model 持久化改走私有 DeepSeek 默认模型
  校验，不再依赖伪 `/set` 入口。无效 `/settings set`、`/sidebar auto --save` 文档已改成直接
  编辑配置文件，并纠正当前默认值和环境覆盖事实。settings 26/26、catalog sync 1/1、App
  67/67 通过，并通过 TUI strict Clippy、fmt 和 diff-check。
- M4-C 已删除 `crates/tools` 中只由 `parity_tools` 自证的第二套 `ToolDescriptor`/
  `ToolRegistry`/`ToolCallRuntime`/handler 调度抽象，以及 protocol 中只被该岛占用的
  `ToolKind`/`ToolPayload`/`ToolOutput`/`LocalShellParams` DTO。生产固定目录、canonical
  `ToolDefinition`/`ToolInvocation`/`ToolOutcome`、`ProductionToolExecutor`、DeepSeek 工具
  调用和确定性 `run_verifiers` 均保持不变。tools 296/296、tools doctest 2/2（1 ignored）、
  protocol 71/71 通过，并通过两个 crate 的 all-target strict Clippy、fmt 和 diff-check。
- M4-C 已删除零 production consumer 的 `protocol::runtime` 外部 Runtime/Tool Bridge：
  `RuntimeEventEnvelope`、capability advertisement、dynamic external tool 和 turn environment
  DTO 及其自证 parity。它们既不是 canonical `RuntimeEvent`，也从未接入 app-server Run API；
  删除后唯一对外事件契约仍为持久化 `StoredRuntimeEvent`/Run API projection。protocol
  57/57 通过，并通过 all-target strict Clippy、fmt 和 diff-check。
- M4-C 已删除只由自身测试构造、面向 mobile/chat bridge 和未来共享链接的整个
  `protocol::workroom` 岛。它没有 Store、app-server route、TUI 或 Runtime consumer，且违反
  本地 DeepSeek coding agent 的固定产品边界；canonical Run/child/Fleet 协议均未使用该概念。
  protocol 48/48 通过，并通过 all-target strict Clippy、fmt 和 diff-check。
- M4-C 已删除旧 Engine 留下且生产永远为 `false` 的 `turn_error_posted`、`is_purging`，
  并移除失败 phase、footer working/label 和 empty-state 中对应恒假分支。失败状态继续只读
  canonical `runtime_turn_status`；真实 compaction、loading 与 child/Fleet activity 保持不变。
  underwater 7/7、footer 12/12、widgets 70/70、App 67/67 通过，并通过 TUI strict Clippy、
  fmt 和 diff-check。
- M4-C 已删除旧 submit path 留下且生产永远为 `None` 的 `last_send_at`，连同 send flash
  timer、renderer tint helper 和唯一手工测试。canonical submit、transcript history、collapse
  index mapping 与低动态设置不依赖该假时间戳。widgets 69/69、App 67/67 通过，并通过 TUI
  strict Clippy、fmt 和 diff-check。
- M4-C 已删除旧 Engine completion path 留下且生产永远为 `None` 的
  `ocean_receipt_settle_start`，连同 receipt cascade renderer/helper 和唯一手工测试；由此失去
  消费者的 `TranscriptLineMeta::cell_line` 薄 helper 也同步删除。真实工具结果、canonical
  transcript metadata、collapse mapping 和 completion 状态投影均保持不变。widgets 68/68、
  App 67/67 通过，并通过 TUI strict Clippy、fmt 和 diff-check。
- M4-C 已删除旧 Engine completion path 留下且只有测试能写成 `Some` 的
  `ocean_completion_started_at`，连同 finishing phase、completion breath/brightness 分支和
  专用测试。canonical `runtime_turn_status=completed` 现在直接投影为“✓ 完成”；正常 Ocean
  phase animation、低动态、工具结果和 terminal 状态均保持不变。ocean 10/10、underwater
  6/6、widgets 68/68、catalog sync 1/1、App 67/67 通过，并通过 TUI strict Clippy、fmt 和
  diff-check。
- M4 退出时，三个入口使用同一 `AgentRuntime`、`RuntimeEvent` 和 `RunStore`，并统一
  steer、resume、request-user-input、现有 compaction 与 completion 的 canonical
  command/event 投影。C2 只建立最小、可恢复的 projection；按任务相关性和 evidence 新鲜度
  选择上下文、确定性保留 TaskContract/diff/evidence 及验证净收益仍属于 M5。真实工具只有在
  production consumer 同步迁移时才物理收敛到 `crates/tools`，不做空目录式模块搬家。

#### M4-C C1：durable interaction/control 契约（已完成）

- 真实问题：canonical Runtime 缺少持久 approval、request-user-input、运行中 steer 安全边界
  和可恢复 control receipt，直接切换 TUI 会产生能力退化。
- 验收条件：审批前零副作用；交互响应和控制命令按 payload 幂等；steer FIFO 且只在原子
  model/tool/child 边界应用；崩溃重放不重复请求、副作用或 terminal。
- 单一 owner：协议类型属于 `crates/protocol`，状态机属于 `crates/runtime`，持久事实只进入
  `RunStore`，应用命令只由 `crates/app::AgentApplication` 接收。
- 替换旧语义：删除 v3 `Steered`、内存投递即 accepted 和 in-flight steer 直接进入
  `RecoveryRequired` 的语义，不引入兼容 alias。
- 测试与证据：protocol/runtime/app/Store conformance；外部监督进程 `SIGKILL` 覆盖
  `InteractionRequested`、`InteractionResolved`/before-tool-start、
  `SteerQueued`/before-applied、`ControlRequested`/tool-in-flight 和
  `SteerApplied`/next-model-not-prepared；focused、all-target check、全仓 clippy 和
  workspace tests 通过。
- 切换删除点：本切片删除旧协议语义；其后 C3 caller cutover 已物理删除交互前台的
  `EngineEvent` control 回写、乐观 transcript 双写和 runtime-thread owner。隐藏 workflow
  第二循环不属于该前台实现，现已由后续纯删除切片移除。

#### M4-C C2：continuation 与最小 context projection（已完成）

- 真实问题：旧 `exec --continue` 把 continuation 与同 run recovery 混为一谈，canonical
  Runtime 也没有可持久恢复的 model-visible context projection；直接切换交互 TUI 会丢失长
  会话延续与 compaction 行为。
- 验收条件：`resume` 只推进同一 run；`continue` 只从非 `RecoveryRequired` 的终态 root
  创建独立新 root，source 不变且 lineage 明确；完整 transcript append-only，compaction 只
  改变请求 projection；摘要请求计入预算/accounting，prepared 可安全恢复，in-flight 不确定
  时 fail closed；creation crash/concurrency 不产生第二个 run。
- 单一 owner：command/lineage/event 属于 `crates/protocol`，projection 构建属于
  `crates/context`，状态机属于 `crates/runtime`，State schema v8 的 lineage、root list 和
  durable creation reservation 属于 `crates/state`，入口只经
  `crates/app::AgentApplication`。
- 替换旧语义：`codewhale exec --continue <PROMPT>` 通过 canonical `list_roots` 找到精确
  workspace 最新 root，并创建新 root；若最新 root 未终态则要求
  `codewhale exec --resume <RUN_ID>`。旧 latest-resumable lookup 和 continue-as-resume
  语义不保留。
- 测试与证据：protocol/runtime/app/Store/HTTP/stdio/exec 的 lineage、projection、
  accounting、schema 与 reservation 契约通过；内存/SQLite root-list parity 与外部监督进程
  `SIGKILL` 的 compaction prepared/in-flight/committed 窗口通过；focused、workspace
  Clippy `-D warnings` 和串行完整 workspace tests 通过。官方 DeepSeek production sender
  canary 以 6/6 请求覆盖 Standard、Thinking/tool-history replay、Beta Strict 与 FIM，
  完整 usage、无 transport retry，费用 `USD 0.0000969904`；该 canary
  `product_metric_eligible=false`，且不替代 compaction on/off 真实 A/B，因此不得声称
  Token、成本或 verified task success 改善。
- 切换删除点：C2 切换 exec/app-server 的旧 continuation lookup；其后 C3 已删除交互
  foreground engine/session/runtime-thread 调用链，`afeca3c4` 已物理删除退役的
  `crates/tui/src/compaction.rs`/`seam_manager.rs`，`1ff73a00` 又删除了不再影响 canonical
  Runtime 的 TUI `auto_compact` 假设置和阈值状态。M5-B 已以正式 A/B 决定 shrink：
  保留 evidence-aware ContextBroker 与 hard-limit safety，删除主动产品面，不堆叠第二套
  摘要器。

#### M4-C C3：交互前台与 child 投影切换（已完成）

- 真实问题：交互 TUI 已能调用 application service，但仍保存旧 child UI/cache 和
  registry-driven slash command 语义，造成第二展示状态与大量无消费者实现。
- 单一 owner：command 只属于 canonical Run API；执行只属于 `AgentRuntime`；TUI 只通过
  `TuiRunClient`、`CanonicalRunProjection` 和 presenter 投影 `RunStore` durable facts。
- 实现与删除：`5bffa951` 切换 foreground，`f470e5c3` 删除旧 foreground loop，
  `ec6aaa5e` 删除旧 state owners，`2ab6f3f8` 删除 `SessionManager`；`ef31f295` 与
  `2c60e22f` 让 child UI 只消费 canonical outcome；`0ae9cb7f` 删除旧 slash command system，
  共 64 files、`+7/-25,523`。
- 证据：canonical Run 20/20、canonical PTY 5/5、run presenter 13/13、canonical commands
  5/5；child blocked/recovery/terminal outcome 与中文宽字符投影有定向回归。
- 非结论：该切片只完成交互前台切换；后续纯删除切片已移除隐藏 workflow、ACP 和 direct
  `review` 模型路径。M4 仍需完整集成门禁和剩余旧编译岛清理，不能因三个已知入口统一就
  提前关闭。

#### 每 Agent 最终请求许可（机制完成，产品收益未通过）

- 真实问题：共享逻辑请求预算可被 child 的工具轮或自动 compaction 全部消耗，使 child
  没有产物轮、父 Agent 也没有集成轮；继续叠加提示词限制在 v3 canary 中使 multi 降至
  `1/3`。
- 机制：`8ab0e145` 为每个 root/child 预留一个可退还逻辑请求许可；descendant/child 先
  join，再发 `tools=[]` 的最终请求。无最终容量时不得先写 `ChildStarted`；恢复按该请求
  实际 advertised catalog 拒绝未授权工具；硬限制以内的自动 compaction 不得消耗最后许可。
- 持久协议：RuntimeEvent v6 / State schema v10 引入最近模型请求实际 advertised tool
  catalog 的持久化与重建；当前 RuntimeEvent v16 / State v21 仍保留该事实。
- 离线证据：Runtime conformance 51/51、State `run_store` 18/18，并覆盖嵌套 join、tool-free
  final、prepared/replay、无容量零 lifecycle 和 compaction 不偷取许可。
- 真实 A/B：相对 `0ae9cb7f` 的 exact-pair、single/multi、3/cell 官方 DeepSeek A/B 为
  12/12 verified、0 false success、0 measurement invalid；candidate single 的平均 Token/
  时间/费用下降 `26.21%/20.70%/21.92%`，但 candidate multi 在成功率不变时上升
  `9.20%/15.16%/17.78%`，请求均值也上升 `3.70%`。
- 决策：保留离线反例已经证明的终局可靠性不变量，但产品收益判定为“重做/缩小”，不得
  宣称多 Agent 效率提升。下一实现先减少不必要的 child/root 最终轮，再以同任务复测；
  不恢复 v3 提示词、不提高总请求预算。证据见
  [每 Agent 最终请求机制精确 A/B](../../eval/summaries/terminal-turn-exact-ab-2026-07-18.md)。

#### 子 Agent eager join（小型机制保留）

- 真实问题：同一轮 `agent` 工具启动 child 后，旧 Runtime 先让 root 发出一次没有 handoff
  新信息的模型请求，随后才等待 child；这浪费请求，也让父 Agent 在缺少调查结果时继续。
- 实现与删除：`528a72f2` 在 `agent` 工具 batch 后立即 join pending child，使 durable
  `ChildFinished` 先于下一次 root `ModelRequestPrepared`；没有新增状态、工具或抽象，生产
  代码 `+6/-64`。
- 离线证据：Runtime conformance 53/53，覆盖 child 多步 search/read、首轮直接完成、
  同 batch 两个 child、嵌套 join、恢复和原有最终请求许可不变量。
- 真实 A/B：相对相邻基线 `f9dddd5d` 的 official DeepSeek、single/multi、6/cell 为
  24/24 verified、0 false success、0 measurement invalid；12/12 multi lifecycle/handoff
  完整。candidate multi 请求均值下降 `9.62%`，Token 下降 `8.27%`、费用下降 `11.99%`，
  平均时间仅下降 `0.07%`。
- Pair 边界：multi 请求 4 对下降、2 对相同；Token 4 对下降、2 对上升；时间与费用均只有
  2 对下降、4 对上升。保留的是本任务已证明的请求削减和正确 handoff，不宣称普遍提速、
  成本优势或成功率提升。证据见
  [子 Agent eager join 精确 A/B](../../eval/summaries/eager-join-exact-ab-2026-07-18.md)。

#### RuntimeEvent v6：请求预算终态 taxonomy 纠偏（已完成）

- 真实问题：旧 `RequestBudgetExceeded` 同时表示 Runtime 逻辑请求 gate 和 DeepSeek 物理
  admission rejection，导致未发送第 N+1 个物理请求时仍投影
  `llm_api_request_budget_exhausted`，与 `exhausted_denied = 0` 冲突。
- 验收条件：逻辑 gate 与物理拒绝使用不同 failure kind/error code；达到
  `started == limit` 不算物理耗尽；RunStore 原样重放；评测 Harness 交叉校验终态和账本。
- 单一 owner：failure kind 属于 `crates/protocol`，判定属于 `crates/runtime`，物理拒绝事实
  仍只属于 `crates/deepseek` accounting，exec 只做薄投影。
- 替换和删除：RuntimeEvent writer/reader 断代切换到 v6，删除泛化
  `request_budget_exceeded` 及 retry stop reason，不保留 alias。
- 测试证据：protocol serde、root Runtime conformance、exec projection、SQLite raw
  persistence/reopen replay 和 Harness self-test 覆盖两类终态及“达到上限但没有拒绝”的
  反例。

### 删除/替代

- Headless 已删除 `exec` 的旧 TUI Engine/spawn 路径；`crates/core` 和 fake
  `Runtime::handle_prompt` 已随 app-server 切换物理删除。
- app-server 的 TUI 子进程 bridge、`RuntimeBridge`、`monitor_turn` 与私有事件/状态翻译已删除；
  交互 TUI 的旧 foreground runtime-thread owner 也已删除。
- TUI 已只保留交互命令和 canonical `RuntimeEvent` 投影。
- TUI `core/runtime_contract` 中从未编译、没有生产消费者的 10 个 speculative shadow 文件已
  物理删除；仓库只保留 `crates/protocol` 的 canonical `RuntimeEventKind`。仍被 exec 输出、
  runtime 与 CLI presentation 消费的 typed `termination.rs` 继续通过真实 path import 编译。
- TUI 已删除无调用者的独立 `resume`/Agent-root picker wrapper、只供其使用的过滤 helper，
  `CanonicalRunProjection` 的零消费者 public cursor getter，以及没有 producer/handler 的
  Fleet model-draft ViewEvent 和 delivery cell。`attach_or_resume`、`latest_root`、canonical
  app 的 list/resume API、内部投影游标与真实 Fleet 执行链均保留。
- 隐藏 workflow 的 Workflow/SubAgent JSON/JSONL 写入链、专属 adapter 和 UI 已随第二
  Runtime 物理删除；未把旧状态迁成 canonical 双写。
- ACP 独立 session/stream/direct completion 已删除；不保留 editor 协议兼容桥。

### 整个 M4 的退出门槛

- CLI/API/交互 TUI foreground 对同一 canonical command/event contract 工作。
- 内存 Store 与 SQLite Store 重放一致。（M4-A 行为门禁已通过）
- crash/resume 和 exactly-once completion 通过。（`exec` 与 app-server 已通过）
- exec/app-server/交互 foreground 不存在第二个生产 loop、可写 Agent Store 或入口私有
  completion 语义；hidden workflow/ACP/direct `review` 不再构成生产例外。最终 workspace
  门禁、调用图复核和旧编译岛删除已通过，M4 关闭。

## 9. M5：RepoGraph、ContextBroker 与 canonical 证据链

### M5-A：canonical TaskContract 与 EvidenceReceipt（已完成）

- 真实问题：当前模型可以提出完成，Host 只有 terminal 机制，却没有绑定任务 generation、
  最新 workspace revision 和确定性验收结果的产品级完成契约，因此仍可能“回答完成但没有
  证据”。
- 验收条件：每个 root generation 冻结 objective、constraints、non-goals 与 acceptance；
  写操作使旧 evidence 失效；只有参数和 workspace revision 精确匹配的确定性 verifier
  receipt 能满足对应 acceptance；Host 单点接受 terminal，模型自评只能是 advisory。
- 单一 owner：wire contract 在 `crates/protocol`，状态机和完成接受在
  `crates/runtime`，持久事实与重放在 `crates/state`；TUI、exec 和 app-server 只投影。
- 替代和删除：不恢复已删 TUI Goal/Hunt/receipt prototype，不建立 adapter、镜像 Store 或
  第二 completion loop。新链切换后删除任何仍重复推断“成功”的 presentation helper。
- 测试与评测：先写 root/child 共用 conformance、SQLite crash/replay、revision invalidation、
  false-success 反例和三入口 parity；再在冻结编码任务上比较 verified success、false-success、
  Token、请求数、时间、费用与复杂度。
- 本切片不先做 RepoGraph、LSP 或多 Agent DAG。先让“什么叫完成”只有一个可恢复真相，
  后续 ContextBroker、RepoGraph 和 Orchestrator 才能复用同一证据闭环。

#### 当前完成事实

- Run API v5 用结构化 `TaskDefinition` 替代自由 `input`；`RunCreated` 冻结唯一
  `TaskContract`，结构化 constraints/non-goals/acceptance 进入唯一确定性中文 transcript，
  root/child 共用同一 Runtime completion gate。
- RuntimeEvent v7 新增 workspace observation、completion proposal/rejection 和
  Host verifier prepared/started/committed 事实；模型 `Stop` 不再直接制造 `Completed`。
- `run_tests.args` 已断代改为 `Vec<String>`；`run_verifiers` receipt 只接受冻结的 exact
  resolved plan。工具只生产 typed observation，只有 Runtime 能签 EvidenceReceipt。
- 任意 `MayWrite` 执行都会推进单调 workspace generation；same-hash 写入、缺失实际
  artifact、伪造 Completed、错 generation/revision/parameters 均 fail closed。
- SQLite 没有增加 receipt 私表；现有 canonical event/snapshot/reducer 是唯一持久真相。
  Prepared/InFlight/Committed 三个 Host verifier 窗口均通过真实子进程 `SIGKILL` 后重开。
- focused、严格 workspace Clippy、完整 workspace tests、真实 PTY/exec 和
  exec/HTTP/stdio 逐事件 parity 已通过。
- 正式 DeepSeek 12-run 显式 verifier A/B 为
  `2eabda53cb8943b1875a20d2cf7a70f8`：编码 baseline/candidate 均 `3/3` verified；
  强制伪完成 baseline `3/3` false-success，candidate `3/3` correct-rejection、0
  false-success；四个 cell 计量有效且成对首请求投影相同。编码 candidate 相对 baseline
  请求持平、Token `+1.08%`、时间 `+9.22%`、费用 `+3.13%`，保留理由只来自完成正确性
  改善而非效率收益。完整结果见
  [M5-A canonical 完成门禁精确 A/B](../../eval/summaries/m5-completion-gate-ab-2026-07-19.md)。

### M5-B：evidence-aware ContextBroker 与 compaction（已完成并 shrink）

- 单一 `crates/context` owner 已接管 root/child 的每次 request projection；canonical
  transcript 仍 append-only，Store 用 exact source index、tool catalog、digest 和
  before/after Token 重算每次 committed projection。
- 确定性 pinned facts 包括 TaskContract、当前 workspace generation/revision、最新有效
  receipt、未解决 verifier failure、当前任务 mutation group 和已 join child handoff；
  reasoning/tool group 保持原子，不调用摘要模型。
- 正式官方 DeepSeek 评测为 12 对 / 24 treatment arms、2 个长任务、3/cell。candidate
  compaction on/off 均 `6/6` verified、0 false-success；on 的 Token 6/6 对下降，平均
  `-8.743%`，但费用 6/6 对上升，平均 `+7.995%`，请求完全相同，时间方向各半。
- 旧 baseline 模型摘要 on 为 `0/6`、off 为 `4/6`，因此旧模型摘要路径保持物理删除。
  candidate 的 ContextBroker 可靠性保留，但主动压缩没有通过产品收益门槛。
- `e2c870b0` 落实 shrink：只在下一请求预计超过基于官方 context/output capability
  派生的 Host hard input limit 时在同一
  `AgentRuntime` 本地压缩；删除 `/compact`、HTTP/stdio Compact、独立 compaction root、
  90% 提前阈值、特殊 purpose/terminal/creation 状态。Run API v6、RuntimeEvent v9、
  State v13，净删 `1,074` 行。
- 完整身份、binary/result SHA、cell、pair、未知计费下界与归因限制见
  [M5-B ContextBroker 正式 A/B](../../eval/summaries/m5-context-broker-ab-2026-07-20.md)。

### M5-C：增量 RepoGraph（post-V1 证据触发）

- M5-B 没有把失败定位为“缺少结构检索”，ADR-0005 因而确认 RepoGraph 不作为 V1
  实现门槛。
- 首个纵向切片优先复用 ripgrep、git diff 和包清单；tree-sitter/LSP/embedding 必须各自
  证明比现有 project map 提高 verified success 或减少 Token，不能一次性全部引入。
- RepoGraph 只向同一 ContextBroker 提供候选事实，不拥有模型循环、任务状态或完成判定。

### 删除/替代

- 历史 TUI/Goal 本地判定、receipt 记账和重复状态已在 M4 物理删除；M5 不恢复镜像同步、
  adapter 或旧语义 fallback。
- 最终只保留一套 `TaskContract`、`EvidenceReceipt`、`TerminalState` 和 Host 接受流程。

### 退出门槛

- RepoGraph 重新准入仍需先由真实任务定位结构检索缺口，再证明相比 current canonical
  search/read/context 提高成功率
  或减少 Token。
- ContextBroker 已按 A/B 完成 shrink：保留硬限制可靠性，删除未产生净收益的主动压缩；
  不宣称 compaction 降低成本、缩短时间或提高成功率。
- 写操作会使旧证据失效。
- false-success 显著下降。
- 根/子 Agent 与 Headless/TUI/API 对同一 TaskContract 共享同一验收判定和 RunStore 真相。
- 不存在第二套 Goal 状态机、receipt store、completion 判定器或兼容桥。

## 10. M6：统一多 Agent

多 Agent 是必须保留的产品能力。本阶段不是删除智能体，而是删除多套重复内核和产品外壳。

### M6-A：单 Writer isolated worktree 闭环（已完成）

- `crates/orchestrator::ProductionAgentOrchestrator` 是唯一生产编排 owner；它复用同一个
  `AgentRuntime`、固定工具目录、canonical `RuntimeEvent` 与 SQLite `RunStore`，不拥有
  第二模型循环、工具实现、终态或私有 ledger。
- 唯一模型可见 `agent` 工具现在可以由 Host 接纳为一个 `IsolatedWrite` AgentTask。
  当前严格限制为一个 root Integrator、最多一个 Writer、clean Git workspace、精确 base
  commit 与冻结 allowed paths；Writer 角色本身不自动获得写权限。
- Writer 从精确 base 创建独立 worktree，在相同 `AgentRuntime` 内执行；Host 封存真实
  changed files、binary-safe diff、revision、检查和 usage，执行 worktree exact verifier，
  再以唯一 fast-forward 策略集成。根分支使用跨进程 lease、精确 `HEAD.lock` 和
  compare-and-swap，base 或分支变化时 typed conflict，不覆盖用户修改。
- 集成后根 workspace generation/revision 推进，并在最新根 revision 重新执行 verifier；
  Writer receipt 不能直接满足 root TaskContract。成功、失败、取消与恢复均走 canonical
  lifecycle 和幂等 cleanup。
- Writer 的 Linux bubblewrap / macOS seatbelt 工具执行只允许其 worktree，显式保护
  `.git`、`.codewhale` 和 `.deepseek`；只读 child 行为和后续只读委派保持不变。
- Run API v7、RuntimeEvent v10 与 State v15 持久化 `AgentTask`、workspace assignment、
  Host-observed `AgentOutcome`、integration、post-integration verification 和 cleanup/recovery。
  exec、TUI、HTTP/SSE/stdio 只投影这些 canonical facts。
- Lane 的第二份 worktree create/remove 与重复字段/CLI 参数已物理删除；Lane/Fleet 仍有
  真实消费者的 lifecycle/执行面没有在本切片无证据删除，也没有成为 Writer 的第二 owner。
- 真实 DeepSeek 生产 canary 在 `a982a9a8` 通过完整
  `root -> Writer -> edit -> verify -> integrate -> root verify -> complete -> cleanup`
  链路：7/7 Standard Chat 请求完成、0 retry、根仓干净、worktree/临时 branch 均删除，
  费用 USD `0.001594964`。它是 `mechanism_canary`，
  `product_metric_eligible=false`，不证明多 Agent 更快或更省。
- DeepSeek canary 暴露并修复了一个真实协议缺口：assistant tool call 与对应 tool result
  之间若出现 child handoff，request projection 现在会暂存 handoff，先精确回放 tool
  result，再恢复 handoff；缺失 tool result 在发 HTTP 前 typed fail closed。Strict
  Function Calling 的 Beta 路由与整目录原子 fallback 未被误改。

M6-A 明确没有实现完整 DAG、多 Writer 并发、脏工作区快照、自动冲突修复或远程 worker。
这些都不能从单次 canary 推断为值得开发。

### M6-B：先评测，后决定是否扩到双 Writer

- 先冻结同任务、同模型、同工具、同请求/Token/费用上限的 current single-agent 与
  M6-A writer-agent 对照；至少包含可分工写任务和不适合委派的 control 任务，每 cell
  至少 3 次。
- 同时测量 verified success、false-success、wall time、API requests、Token/cache、
  费用、writer admission/rejection、冲突、integration、cleanup/recovery 与新增复杂度。
- 若 M6-A 只在少数任务有收益，收缩为显式/确定性按需委派；若没有净收益，保留可靠的
  isolated-write 机制但不默认调度，且不开发多 Writer。
- 只有预注册门槛通过，才进入独立的 M6-B2：最多两个 allowed paths 不重叠的 Writer、
  有界并发 2、同一个 Orchestrator/Runtime/Store，范围重叠或集成歧义一律 fail closed。
  不开发通用 DAG、自由聊天 swarm、新工具族或另一套 scheduler。

M6-B1 已在候选 `5d72ae94` 上完成正式同二进制 A/B：

- `deepseek-v4-flash` Standard Chat，3 个任务 × 2 treatments × 6 次，共 18 对 /
  36 arms；0 invalid attempt、0 unknown billing、0 transport/runtime retry；
- single 为 `2/18` verified、15 false-success；Writer 为 `4/18` verified、
  7 false-success；
- Writer 总 Token `+35.5%`、费用 `+52.5%`、时间 `+39.8%`；仅 1 对双方成功；
- 出现 1 次 Writer root 调用 treatment 禁止的 may-write 工具和 7 次
  `recovery_required` / retained Git 状态；
- T3 single/Writer 12/12 最终 verifier 虽绿，却都没有形成冻结的
  “失败 -> 修改 -> 通过”时序 evidence；
- 正式决策为 `reject_and_rework`，`hard_gate_met=false`，M6-B2 不准入。完整身份、
  cell/pair、费用和归因见
  [M6-B1 Writer 收益 A/B](../../eval/summaries/m6-b1-writer-benefit-ab-2026-07-21.md)。

M6-B1 rework 已在候选 `3310aa73` 完成，不是新架构层：

1. `crates/tools` 已消除 verifier 生成 `__pycache__` 等 workspace 副作用；
2. `app` / Runtime 已完成 Writer 默认关闭、显式 opt-in 的 admission cutover；
3. Runtime/Orchestrator 已由 Host actor policy 强制 Writer root 只读，不依赖提示词；
4. TaskContract 已绑定 named frozen verifier，EvidenceReceipt 已表达“失败 → 有效修改 →
   Host 通过”的最小有序事实；
5. seal/blocked 恢复已产生可诊断 reason；只在 artifact、resource ownership/scope、Git
   cleanup metadata 或 exact cleanup 结果确实不确定时 retained；
6. Harness 已冻结完整 tool definition hash，并保存 seal 未提交时的脱敏 scope 摘要；
7. 离线 focused、workspace Clippy/tests 和 27 个 Harness self-tests 通过后，以 v3
   manifest 重新冻结同三任务；完整工具 definition、8 个唯一源码 owner、candidate、
   Harness、manifest 和 schedule 身份均在任何 live API 前冻结。

rework v3 正式同二进制 A/B 在 candidate `3310aa73ef3bae45ce9296e65964c9b7531c22f0`
上启动后，按预注册规则得到 `hold_mechanism`：

- 完成 1 个 T1 pair 的 2 个 measurement-valid arms；第 3 个 T2 single arm 出现一次
  `deepseek_transport`，10 次 physical attempt 中 9 次有 response/usage、1 次计费不可证明；
- canonical Terminal 唯一且位于末尾，Terminal、RunView、accounting 完全一致；缺口不是
  Harness 丢事件，而是一次已开始、无 provider response/usage 的 transport attempt；
- Harness 立即停止，未启动 T2 mate、未重采样；共 29 次 physical starts，已知费用仅为
  USD `0.012583452` / CNY `0.089881800` 下界；
- `product_metric_eligible=false`、`hard_mechanism_abort=false`、0 false-success、0 Writer
  safety finding，不能用 3 个 arms 声称 Writer 收益或回归；
- v3 不推翻 v2 的 `reject_and_rework` 产品证据，也不开放 M6-B2。Writer 保持显式按需
  admission；不得续跑 `3310aa73`、补 T2 mate、复用 T1 pair 或与未来结果拼样。

完整身份、计量归因和防重采样边界见
[M6-B1 rework Writer 收益 A/B v3](../../eval/summaries/m6-b1-writer-benefit-ab-v3-2026-07-22.md)。
新的 v4 只有在独立、实质性的产品代码变化及离线证据后才成立，且必须在 live API 前重新
冻结并从 schedule position 1 全新运行；no-op、注释、版本或 Harness-only 换号不构成
新 candidate。当前不开发双 Writer，下一产品阶段优先进入 M7 的 DeepSeek 单 Agent、
只读多 Agent、工具/上下文/失败恢复专项调优。

### 工作

- canonical `agent` 工具启动的根 Agent 与子 Agent 已运行相同 `AgentRuntime`、发出相同
  `RuntimeEvent` 并写入同一 `RunStore` 契约；后续差异只允许来自 TaskContract、预算、
  权限和 workspace。
- 建立唯一 `Orchestrator`，只负责 TaskGraph、预算、并发、mailbox、follow-up、wait、
  interrupt 和结果汇聚，不拥有第二套模型/工具循环。
- 建立 `AgentTask/AgentOutcome`。
- 建立 worktree create、diff、review、verify、merge、conflict、cleanup。
- 模型侧只保留一个 `agent` 工具入口；生命周期与调度语义是其参数/事件，不扩张为多组工具。
- 将现有 SubAgent、Workflow DAG、Fleet ledger/lease 和 Lane worktree 中经过测试的能力逐项
  迁入 `Orchestrator`，每迁完一个生产调用方就删除对应旧入口和重复状态写入。

### 删除/替代

- hidden `workflow-tool` 的 `SubAgentRuntime` 第二模型循环已提前纯删除；canonical
  `AgentRuntime` child 能力由 conformance 回归保护。
- 能力迁入并有回归测试后，删除 Workflow/Fleet/Lane 重复用户概念、scheduler 和状态真相。
- `workflow-js` 已随旧第二 Runtime 删除；不得以恢复 authoring surface 的方式重建它。

### 退出门槛

- 根/子 Agent通过同一 conformance suite。
- 写 Agent 不共享 cwd。
- AgentOutcome 包含证据、文件、检查、未解决项和 usage。
- worktree 的 create、diff、review、verify、merge/conflict 和 cleanup 生命周期可恢复。
- 固定任务 A/B 证明适合并行的任务相对单 Agent 在 verified success、时间、Token、成本和
  冲突率综合后有可测净收益；无收益时自动退回单 Agent。

## 11. M7：专项调优与外围清理

### M7-A：单 Agent 可验证完成与拒绝恢复

生产候选 `24c8a530` 已把 verifier spec 收敛为 `crates/tools` resolver 的单一真相，并把
completion rejection 的原因和所需恢复动作持久化为 typed RuntimeEvent；调用方手写 plan、
Host 执行 plan 与恢复时重建 plan 的重复 owner 已删除。root、read-only child 和显式单
Writer 仍使用同一个 Runtime/Store/Event owner，没有新增模型循环、Provider、scheduler、
prompt treatment 或多 Writer。

实现分为两个可审查提交：`13b5c3eb` 由 `crates/tools` 唯一解析并冻结真实 verifier
contract，`24c8a530` 由 `protocol/runtime/state` 唯一表达、持久化和恢复 typed rejection。
从起始 checkpoint 到 candidate 的 `crates/` diff 为 19 files、`+2,045/-428`；其中独立
tests 为 5 files、`+633/-41`，其余源码路径上界为 `+1,412/-387`（仍包含源码内联测试）。
没有新增 Cargo 依赖、模型可见工具、Runtime、Store 或兼容开关；该复杂度只有正式复评
通过后才能证明值得长期保留，当前 `hold` 不把代码增加本身算作进步。

离线 focused、workspace test/clippy、精确 release binary 和 34 项冻结 Harness 自测均通过。
正式 v1 预注册为 20 对 / 40 arms，但在首个 T1 pair 后因 Harness 把 candidate 计算为
`false_success` 而停止。事后只读复核证明该值是假阳性：生产二进制的 `serde_json` 启用
`preserve_order`，Rust `canonical_json` 没有真正排序，Python Harness 却按字典序重算
artifact SHA。candidate 实际拥有唯一 Host receipt，外部 verifier、scope、权限、lineage、
ledger、accounting 均通过；baseline 和 candidate 都出现同一 artifact mismatch。

因此 v1 的产品结论是 **hold**，不是 keep，也不是安全 reject：只完成 2/40 arms，不能证明
总体收益；原 suite 不覆盖、不续跑、不拼样。下一独立切片先修复唯一 canonical JSON owner，
加入跨 key-order 与 Rust/Python 固定向量，再用新的 candidate、suite ID、output path 从
position 1 完整 refreeze。完整身份、费用、已执行事实和非结论见
[M7-A DeepSeek Agent 收敛正式 A/B v1](../../eval/summaries/m7-a-agent-convergence-ab-2026-07-22.md)。

M7-A2 已完成上述 correctness 切片。`crates/protocol` 现在逐层显式排序 object key，artifact
构造与 replay 验证共用同一 canonical-byte helper；跨 Rust/Python、`preserve_order`
开/关、M7-A v1 T1 和篡改反例由共同向量冻结。相同修复被字节等价地应用到 control
`18de2ad2`（parent `3351213b`）与 treatment `c6a74304`（parent `24c8a530`）；M7-A
production delta 没有被 canonical correctness 混入 treatment。

新的 20 对 / 40 arms 正式 suite 从 position 1 执行，在第 7 arm 按 accounting gate 停止：

- 前 6 arms 计量完整、0 false-success；T1–T3 treatment 为 3/3 verified，control 为
  0/3 blocked，提供强方向性机制证据但不构成总体收益结论；
- 第 7 arm T4 treatment 的外部 verifier 和修改范围通过，但 canonical terminal 为 Failed；
  4 个 physical responses/attempts 只有 3 个 usage response，`incomplete_responses=1`、
  `usage_complete=false`、`complete=false`；
- aggregate 为 `hold`、`product_metric_eligible=false`，已知费用 USD `0.019383566` 只作
  下界；T4 control 与 T5 均未执行，不能宣称 read-only multi Agent 或 5-task 总体收益。

这次停止不是 Harness canonical JSON 假阳性，也不是 safety reject。raw 没有保存足以归因
网络、provider、输出上限或其他具体原因的 typed failure，因此不得猜测根因；它只证明
production response/usage 生命周期未闭合。原 A2 raw 不续跑、不补 mate、不覆盖、不拼样。
下一独立切片先定义并修复 incomplete response 的可诊断、accounting 与安全恢复边界，补齐
脱敏 typed failure evidence 和离线 stream/crash/replay 反例；只有实质 production 变化后
才能用新 candidate、suite ID/output 从 position 1 全量 refreeze。当前不开放多 Writer，
也不以 Harness-only 改动换号重跑。完整证据见
[M7-A2 DeepSeek Agent 收敛正式 A/B](../../eval/summaries/m7-a2-agent-convergence-ab-2026-07-22.md)。

M7-A3 已完成独立 correctness 切片。`crates/deepseek` 现在要求受支持的 finish 与 `[DONE]`
共同闭合 response，拒绝 DONE 后 data，并在 usage frame 到达时立即记录 accounting；partial
content、reasoning 或任意 tool-call fragment 都会形成最小 typed evidence 并禁止自动重放。
Runtime 只在 response 可证明 replay-safe 且错误可重试时原子准备 retry；RuntimeEvent v15
和 State v20 直接持久化该证据，root/read-only child 使用同一 conformance，crash 后仍不
重发 in-flight request。旧 v19 materialized run 直接退役，不建立兼容 reader 或双写。

Harness 已区分 response count 与 usage response count，以 `accounting.usage` 保存异常 EOF 前
已经观察到的 usage，并只输出 failure/attempt/message 的脱敏 typed 投影或哈希。对 M7-A2
T4 的修正投影移除了派生 surface mismatch，但 incomplete、usage incomplete 和费用下界仍
独立成立；原 raw、manifest 和 `hold` 结论没有改变。Harness 39/39、focused、fmt、workspace
clippy/test 全部通过。

正式 M7-A 复评没有执行：M7-A3 的 12-path patch 在旧 treatment 上可直接应用，但旧 control
有 8 个不同 blob、三方模拟出现 8 个 changed-in-both 文件/20 个冲突块；更重要的是 control
v13/v18、treatment v14/v19 与新 v15/v20 的直接切换不能同时满足相同 patch、相同 numstat/
patch-id 和原 production delta 不变。Harness 已在任何 output/Key/API 前明确阻断复评，未
创建新 formal manifest 或 raw。M7-A 继续 `hold`；若未来重评，必须从共同 corrected base
定义新的 treatment delta，不得冒充旧 delta。完整证据见
[M7-A3 DeepSeek 不完整流式响应诊断与安全恢复](../../eval/summaries/m7-a3-incomplete-stream-recovery-2026-07-22.md)。

### M7-B：Strict 工具调用准入与失败恢复闭环

M7-B 从 `7ae5b268` 开始，保留 `crates/tools` 的唯一 production schema owner，并让
`crates/deepseek` 只对每次真实 advertised catalog 做整组确定性 compatibility 判定。
兼容时仍能规划官方 Beta Strict Chat；任一工具不兼容时整组原子回退 Standard Chat，工具
数量、名称、顺序、schema 和语义均不改变。畸形或不完整 tool-call fragment 现在 fail
closed；reasoning 与 tool-call 历史按 actor turn 精确回放。

RuntimeEvent v16 / State schema v21 为所有失败的 `ToolOutcome` 强制持久化稳定
`failure_code`，并继续分别表达 invocation、transport、operation、side effect、retry、
evidence、artifact 与 workspace revision。模型只收到一份确定性的中文失败摘要与恢复建议，
稳定 code/字段保持英文。root、read-only child 和显式单 Writer 共用同一 conformance；
进程级 SIGKILL 证明 `ToolPrepared` 与 `ToolOutcomeCommitted` 两侧恢复不会重复执行工具。
TUI 与 exec-stream v2 只投影 canonical 事实，不拥有第二套失败分类。

当次实际 advertised catalog 只随完整 `ModelRequestPrepared` 持久化一次，State 另存 catalog
hash，execution fingerprint 绑定 `strict_tools` policy；surface 与 fallback reason 由唯一
DeepSeek planner 确定性派生，不建立第二份状态。production composition 的 SQLite reopen
测试已经证明 exact request 逐字段一致、完整 `RequestPlan` 重建一致，并证明 strict policy
变化会改变 fingerprint，恢复不能静默换策略。

最终冻结的六个默认可执行 actor catalog 均没有真实 Strict surface：root/coordinator/
read-only child 首先受 `agent` 或 `file_search` 的 required 语义阻断，isolated Writer 首先受
`apply_patch.oneOf` 阻断。删除这些约束、引入 nullable/sentinel、建立第二套 wire schema
或丢弃工具都会改变生产合同，因此不被接受。terminal no-tools 目录没有函数调用，也不构成
Strict treatment。

准入 Harness 在 release build、Key 和官方 API 前得到
`inadmissible_no_surface_delta`：formal A/B 为 0 arms，`product_metric_eligible=false`，
官方 API 请求 0，credential read 为 false。结论是保留官方 Beta Strict planner、确定性
compatibility diagnostics 与无损 Standard fallback；Strict 不成为默认产品路径，不提供
用户开关，不建立 schema transformer。准入冻结后的 `4e3536f1` 只删除可推导的 Strict
decision 布尔值和零调用 wrapper，并增加 SQLite reopen 重建证明，没有修改 frozen
candidate、manifest、raw 或 wire 行为。完整身份和非结论见
[M7-B Strict 工具调用准入与失败恢复](../../eval/summaries/m7-b-strict-tool-admission-2026-07-22.md)。

### M7-C：canonical 编辑基线与 FIM 准入

M7-C 从 `afb9b0ab` 开始，以 `c4972130` 冻结 12 项任务、完整失败矩阵与
`maximum_reruns=0` Harness。只读调用图和历史样本把优先问题定位为 Host correctness，而
不是已经可归因的模型 patch 生成瓶颈：原 `edit_file` 的 len+mtime freshness 可漏掉
same-length/same-mtime 改写与重叠搜索；`apply_patch` 会接受 duplicate target、rename、
hunk count mismatch、no-op，并在重复块上取第一个 fuzzy 候选；原子替换还会丢 mode，
multi-file 普通失败会吞 rollback error。

`7613073c` 在 `crates/tools` 唯一 owner 内完成 exact-byte digest、publish 前原字节校验与原子
replacement、重叠唯一性、
permissions 保留、全量 preflight、ambiguous fuzzy fail-closed 与可观察 rollback；root、
read-only child、Writer 继续使用同一个 Runtime/Store/ToolOutcome。起始 12 项 tools contract
为 3/12，候选为 12/12；这是确定性 correctness 证据，不冒充模型成功率 A/B。clean
`9cba8b53` 的 Harness 8/8 gates、production loopback、Writer integrate/cleanup、三段工具
crash window、app-server SIGKILL/replay、focused、fmt、workspace clippy/test 全部通过。

cutover 已物理删除 CLI `codewhale apply` 的直接 `git apply`、TUI-local `codewhale eval`
简化编辑器及剩余 acceptance/说明。固定 11 个 production 工具不增加同义入口；当前 actor
catalog hash 随收紧后的 schema 重冻，历史 M7-B manifest 不改写。

FIM 复核确认它是独立 Beta `/completions` surface；当前仓库只有 request planner/accounting
基础，没有 production transport/parser、revision-bound Host apply lifecycle 或 canonical
caller，因此不存在同 binary treatment delta。正式 live A/B 在 Key/API 前判定
`inadmissible_no_surface_delta`：0 arms、0 requests、credential read false。产品决策是
**keep canonical tool fixes，hold FIM**；不为制造实验恢复 `FimEditTool` 或增加第二模型循环。
完整证据见
[M7-C canonical 编辑能力基线与 FIM 准入结论](../../eval/summaries/m7-c-edit-baseline-2026-07-23.md)。

下一编辑切片先收集新的 production failure 样本。只有 patch 生成/恢复确为主要损失且能
冻结完整 Host-owned treatment 时，才建立 FIM parser/accounting/apply/replay 垂直路径；
若剩余瓶颈是多文件 crash 歧义，则优先补 operation-specific durable transaction facts。

2026-07-23 的结论后只读复核发现四个未进入原 12 项 manifest 的确定性 Host 反例。
`1cb65b82` 在同一 `crates/tools` owner 内拒绝 `changes` 与 patch-only controls 混用、禁止
`path` 覆盖 `/dev/null` create/delete 语义、要求 delete-to-null 真正清空目标，并禁止
create-from-null 覆盖已有文件；checked create 改为 no-clobber publish。模型可见描述同步
明确“逐文件原子 publish、跨文件 crash 非事务”，当前 writable actor catalog identity
重冻而历史 M7-B/M7-C manifest 不改写。

复核同时把旧文档中的“publish CAS”收窄为准确事实：已有文件只在原子 replacement 前做
exact-byte precondition，外部不守约 writer 仍可竞争 check/rename；delete 也存在 check/remove
窗口。canonical Runtime 中 Started 的 `MayWrite` 即使拒绝且 revision 不变也会推进 generation，
所以 frozen E10 的 generation 文案只描述 direct-tools fixture，不是 production Runtime 合同。
这些勘误不产生 FIM treatment；Key 仍未读取，live 仍为
`inadmissible_no_surface_delta`。

### M7-D：RuntimeEvent v16 编辑失败观测闭环

M7-D 从 M7-C post-audit `61597f59` 与 WIP `5bb9b577` 开始。WIP 会把
RuntimeEvent v16 的 schema number 改为 v14 后交给历史 M7-A evaluator，按 `call_id` 配对
lifecycle，从旧 `ToolOutcome.workspace_revision` 取 revision，并把任意后续成功当恢复。
它还允许任意 result 文件名和 replace 写入，并在没有 control/treatment delta 时提供读取
Key 的单变体 `live`。

`7ddf3bba` 把 Harness 切换为独立 v16-native projector：Prepared/Started/Outcome 只按
`operation_id` 绑定；所有失败必须有 v16 `failure_code`；Runtime revision 只取事件级
`workspace_state.revision`；恢复必须同 run、同工具、同结构化目标并经过新的模型请求。
无法从 structured arguments 得到目标的 patch header 保持 `unscorable`，不复制第二 patch
parser；indeterminate side effect 与 Started 无 Outcome 分开计入 transaction ambiguity。
历史 executor import、全局 monkeypatch、stale binary identity、`live`/Key/cost/schedule 和
replace output 已物理删除。

首个 clean `7ddf3bba` suite 的 13/14 gates 通过，workspace test exit 101；它正确 exit 1，
但 CLI 因 nested self-test status 错印 `pass`，且 hash-only 记录无法定位失败 test。原
0600 raw 保留，随后同命令诊断运行完整通过。`cf9b3fd6` 冻结新 v2 suite，使 aggregate
`passed` 成为唯一 CLI status，并只在失败时向 0600 ignored record 写入每流最多 64 KiB
诊断 tail。v2 在同一 clean revision/tree 上 14/14 通过，覆盖 15 项 projector regression、
root/read-only、explicit Writer、三段工具 crash、app-server SIGKILL、focused、fmt、
workspace clippy/test 与 diff check；credential read false、official API requests 0。

决策为 **keep v16-native offline observer；shrink/delete no-delta live Harness；hold
editor/FIM treatments**。本切片关闭的是 evaluation truth 损失，production Rust 增量为 0，
不提供新的模型编辑失败频率或收益。下一候选必须先有真实反例支持并形成同 binary
control/treatment delta；否则关闭编辑路线，转向证据更强的非编辑损失。完整身份、首个失败
记录和非结论见
[M7-D RuntimeEvent v16 编辑失败观测闭环](../../eval/summaries/m7-d-edit-observation-2026-07-23.md)。

### M7-E：默认 Thinking 准入

M7-E 从 M7-D 结论 `7b8b4851` 开始，只读比较 canonical RunStore 与既有正式结果。M7-A2
成功候选的 17 个请求包含 3,042 reasoning tokens 与 7,263 replay tokens；较早完整
eager-join suite 的 154 个请求包含 21,349 reasoning tokens 与 27,629 replay tokens。
`high`/`off` 又是当前 `crates/deepseek` 已拥有的 Standard Chat production 字段，因此它是
唯一能在不增加工具、Runtime、Store、模型循环或产品模式的情况下形成同 binary 非编辑
treatment 的候选。candidate `b9b83cdf` 只补 protocol/production contract 与冻结 Harness，
没有改变该切片当时的默认 `Auto` 行为；该历史行为已由 M9-D/ADR-0008 supersede 并删除。

正式目标为 5 tasks × 2 variants × 3 runs，15 pair / 30 arms，
`maximum_reruns=0`。v1 把成功 verifier 的保守 side-effect 投影误判为歧义；v2/v3 又把任务
明确要求且已经“failed verifier → effective edit → passed verifier”闭环的恢复错判为
unresolved failure。v4 修正这些 evaluator 缺陷后，在两个完整 pair 上发现实际首请求身份
仍不同。只读调用图定位到 canonical workspace revision 会绑定 workspace/repository absolute
path，而 v4 每个 arm 使用不同随机 workspace，故这不是可归一化噪音，而是真实 prompt
差异。

v4 在第二个同类 mismatch 后由外部 SIGINT 精确停止；已完成 5 arms、27 requests、5/5
verified、0 false success、已知费用 6,346,379 nanousd，但 active
`t3/reasoning_off/run_1` 可能已有 in-flight request，临时 State/RunStore 已清理，最终 billing
无法重建。v1-v4 合计 18 个已完成 arms、94 requests、已知费用 24,736,160 nanousd，都因
evaluator/fairness 失效而排除出产品指标，费用只是下界。

final `458c3d7d` v5 让同一 pair 复用 suite-owned fixed workspace slot、每臂从同一 fixture
重建且保持 State/RunStore/run ID 隔离；只允许归一化恰好一条 Host-owned
`task_generation`，并在 pair 闭合时立即比较真实 messages、actor、tools、surface、预算、
revision、binary 与 fixture。exact high/off RequestPlan 的 SQLite reopen、root/read-only/
Writer conformance、focused、fmt、workspace clippy/test、production loopback 与 process
crash/reopen 均通过。

由于 v4 留下 active-arm unknown billing，v5 在 preflight、output reservation、Key read 和
API 前固定 fail closed；没有 v5 raw 或官方请求。产品决策为 **hold**：保留官方 high/off
协议与公平 Harness，不改变 production 默认，不把 v1-v4 partial 结果拼成收益结论。未来
只有新 successor manifest 能显式重新准入，必须从 position 1 执行全新 30 arms。下一切片
先离线审计 exact ModelRequest 的 stable-prefix/cache break position，不立即重做付费
thinking A/B。完整身份、费用边界和非结论见
[M7-E 默认 Thinking 准入结论](../../eval/summaries/m7-e-thinking-admission-2026-07-23.md)。

### M7-F：canonical context-cache 前缀审计

M7-F 从 M7-E 结论 `8285a883` 开始，先按 2026-07-23 DeepSeek 官方 Context Caching 文档
重新冻结 cache 语义。当前 provider 只命中完整匹配的已持久化 cache-prefix unit；unit 可在
用户输入末尾、模型输出末尾、重复公共前缀和长序列固定 token 间隔形成，且整体是
best-effort。`PromptCacheControl` 不是官方字段。

调用图证明 `crates/context` 的 stable constitution / volatile world-state blocks 在
`crates/deepseek` 被拼成单个 system message，随后是 canonical transcript，最后追加
task generation、workspace revision、receipt/rejection/verifier 状态。`a1d68b05` 增加
test-only production loopback：三轮请求 messages 数为 `2/4/6`，相邻公共完整 messages 为
`1/3`，恰好等于上一请求去掉 Host-facts tail。首轮只读后 facts 字节相同但仍被省略；第二轮
edit 后 latest revision 正确变化。raw loopback body 等于 persisted ModelRequest 重建的
RequestPlan，SQLite reopen 前后 event/snapshot 完全一致。

四份 M7-E raw 的 18 个 closed-accounting arms 仅作非因果诊断：94 requests、395,812 input、
281,344 cache-hit、114,468 cache-miss，aggregate hit ratio `71.0802%`。它们没有逐请求 usage
或 M7-F candidate，且 M7-E pairing 已失效，不能成为收益指标。

产品决策为 **hold**。多 system message 缺少官方语义保证；facts 前移/删除会破坏 prefix 或
freshness/evidence；tool catalog 已稳定。唯一真实潜在 delta 是按时间顺序重放旧 Host facts，
但这会把过期 revision/receipt 加入后续输入，并要求 typed supersession、compaction、
reducer 与 crash/reopen 合同。本切片不为缓存弱化 truth，不读 Key、不调用官方 API。完整
身份、候选取舍与非结论见
[M7-F canonical context-cache 前缀审计结论](../../eval/summaries/m7-f-context-cache-prefix-2026-07-23.md)。

### M7-G：canonical read-only fan-out 准入

M7-G 从 M7-F 结论 `ccb83e0b` 开始。只读调用图证明现有 `AgentRuntime` 已经把同一模型
response 中的多个 `agent` calls 全部 prepared/started 并启动后才进入 `join_children`；
`Orchestrator` 继续只构造同一个 Runtime，RunStore 继续保存唯一 child lifecycle、
handoff 和 accounting。没有第二 scheduler、模型循环、工具目录、RunStore、通用 DAG 或
Writer 并发可删除或接管。

三类临时 Git 仓库任务分别冻结多规格 capability intersection、transitive dependency
impact 和分层 policy resolution。每项都有 deterministic verifier、唯一允许修改文件和
两个相互独立的只读分区。manifest 固定同 revision/binary、逐字相同 prompt、
`deepseek-v4-flash`、`reasoning_effort=high`、相同 request/output/turn/tool/wall budgets、
每 cell 三次、9 对 / 18 arms 与 `maximum_reruns=0`。control 不 advertised `agent`；
treatment 只通过现有 canonical `agent` 目录显式要求同回合两个 read-only Explorer。

离线 production loopback 已证明两个 child 的 transport 真实重叠、typed handoff/accounting
闭合且 SQLite reopen 精确。进程级 SIGKILL 同时暴露一个独立 correctness bug：durable
`ChildStarted` 后 Runtime 选择的 typed `RecoveryRequired` terminal 会被 RunStore 当作未
settled lifecycle 拒绝。`13b94210` 只允许指向确切 in-flight agent/unfinished child 的
recovery ambiguity 落盘，普通 failure 仍 fail closed；重开不重发模型请求、不重启 child。
同批 partial failure 保留 sibling handoff，cancel settle 全部 pending child。

正式候选 `062623e6` 与 release SHA-256
`432a6d6a18906826f16b9949ee7223bb1d365a2319cb0d5884b0a7280c76e911`
通过 focused、fmt、workspace clippy/test、production loopback、process crash/reopen、
Harness self-test 和 no-key/no-network dry-run 后才读取 Key。首个 control arm 已发生网络
请求，但旧 Harness 用提交前 verifier plan（空 env、120000ms）比较 `RunCreated` 中由
`crates/tools` resolver 冻结的 plan（`PYTHONDONTWRITEBYTECODE=1`、600000ms），以
`run_identity_invalid` 停止。raw 记录 `completed_arms=0`、`key_accessed=true`、
`network_accessed=true`，但没有保存已读取的 accounting；最终 usage/cost 不可证明。

按预注册规则立即停止且不重跑。`8763722c` 只离线修复 canonical plan 预期，并让未来任何
post-terminal identity abort 先保存 terminal/accounting/State；它不换号续跑本 formal。
产品决策为 **hold / inadmissible_observer_identity_bug**：保留 explicit child 机制、
overlap/reopen regression 与 store correctness 修复；不宣布 fan-out 收益，不新增自动
admission 或默认并发。完整身份、raw hash、官方资料和非结论见
[M7-G canonical read-only fan-out 审计结论](../../eval/summaries/m7-g-readonly-fanout-2026-07-23.md)。

任何 successor 必须使用新的 suite ID、clean revision、output 和 immutable binary，从
position 1 开始；先 fault-inject 所有 post-terminal observer failure 并证明 accounting/
费用先落盘。无法排除再次 unknown billing 时不读 Key，转向独立的 Agent 请求/Token 预算
瓶颈。

### M7-G2：observer durability 与 successor 身份门禁

M7-G2 从 clean `3382bbc4` 开始，只修复离线 evaluator contract，不改变 production。
审计确认 M7-G 的 `execute_arm()` 只有在 terminal 后完成 identity、accounting、surface、
verifier 和产品派生才返回外层 `emit()`；任一异常会先删除 arm 临时目录中的唯一 SQLite。
`8763722c` 只保存一种 identity abort 的部分 facts，不覆盖其它 observer 或 evaluator kill。

`f89dafc5` 冻结新的 no-key/no-network gate：`terminal_snapshot ->
sqlite_reopen_snapshot -> verifier_snapshot -> arm_result|abort`。raw 用 `0600`、
`O_EXCL|O_APPEND|O_NOFOLLOW`、逐记录 sequence/previous-hash、file `fsync` 和首次 directory
`fsync`；半写 tail 只能审计，不能续写或派生。同一 Harness 以独立进程覆盖 identity、
verifier、surface、accounting exception，terminal 写前/半写/写后未 fsync，以及
terminal/reopen/verifier fsync 后和 result 前 SIGKILL，共 11/11 通过。production owner 的
fan-out overlap + exact SQLite reopen、read-only child SIGKILL/no-relaunch 和 typed recovery
targeted tests 同时通过。

旧 M7-G raw hash、3 records、`completed_arms=0` 和 unknown billing 保持不变，禁止补写、
补 mate 或拼样。新的 offline raw 为 14 records、`0600`、Key/network false，决策是
`hold_no_new_production_delta`。Git 证据证明 `062623e6` 后只有 evaluator changes，没有
新的 Runtime/Store/DeepSeek/tools/catalog/prompt/budget/fan-out production behavior；因此
换 suite ID、output、binary 或 evaluator-only revision 仍是旧 treatment 的伪重跑。
M7-G2 没有读取 Key、没有调用 API、没有构建 paid release binary。

产品决策为 **hold / live_successor_inadmissible_no_new_production_delta**：保留 explicit
read-only child、既有 production recovery regression 和 fail-before-loss offline contract；
不默认启用 fan-out，不宣布产品收益。下一切片先离线审计 canonical Agent request 与 Token
预算，特别是 child/root terminal request、reasoning replay、handoff 后 integration 和
hard-budget exhaustion；只有真实反例支持 material production delta 后，才能从 position 1
建立新的 paid suite。完整身份、raw hash、官方复核和非结论见
[M7-G2 observer durability 与 successor 准入结论](../../eval/summaries/m7-g2-observer-durability-2026-07-23.md)。

### M7-H：canonical request/Token 浪费矩阵与 terminal catalog 修复

M7-H 从 clean `48a0b44d` 开始，不预设多请求、reasoning replay、compaction 或预算上调能
提高产品指标。冻结 observer 只投影已有 canonical evidence：terminal-permit、eager-join、
M6-B1 Writer 和 M7-A2 partial。12 个分组中的 root/child logical
`ModelRequestPrepared` 与 physical started request 差异为 0；现有样本没有观察到 compaction
或 hard-budget exhaustion。Writer 数据可精确归因 root/child reasoning 与 replay，旧 exec
raw 只能保留为 unattributed；旧 schema 也没有逐请求 terminal catalog identity，不能从历史
结果反推当前 near-limit 行为。

只读调用图发现一个独立的确定性 Host 反例：旧 `AgentRuntime` 在判断下一次请求是否使用
reserved terminal `tools=[]` 之前，先用完整 ordinary catalog 估算 hard limit 并尝试
compaction。因而一个实际 terminal no-tools 请求本可装入上下文时，仍可能在 0 次模型请求前
错误落为 `ContextLimitExceeded`。失败契约先在 `1943df4c` 冻结；`eb8763a1` 把 catalog
选择移到唯一 hard-limit/compaction 决策之前，并删除旧的 pre-model-turn full-catalog
分支与一次性 control enum，production Rust 为 `+15/-56`。真实
`ProductionToolExecutor` catalog 回归位于 `b1a01ce9`。

离线 A/B 使用同一 deterministic model、同一 1-request 总预算、`maximum_reruns=0` 和两个
独立构建目录：baseline binary
`11a9f7c54ff364cc19be1b2b53d2c1f3fe997869f31865799bcc10d2f27db82b`
稳定失败，candidate binary
`6412b8d76e05f982a36b47c7ada70ce414c87d06b1ae31d9921da70cbd85660a`
稳定通过并只准备 1 个空 catalog 请求。首次共享 Cargo target 的候选运行复用了基线 binary，
已明确作废且不进入证据。最终 ignored `0600` observer result 为 12,546 bytes，SHA-256
`cdad0a5e91594d30c548ede92d0b7eee74f8bbec4a861f59dac90d9d2feea1d6`。

产品决策为 **keep deterministic Runtime correctness fix / hold broader request-token
optimization / live inadmissible_no_model_treatment**。该变化修复 Host admission，不是新的
模型策略、工具 surface 或效率 treatment；付费请求无法增加归因力，因此没有读取 Key、
没有调用官方 API，也不声明 Token、时间或费用收益。Run API v10、RuntimeEvent v16、
State v21、exec-stream v2、默认 reasoning、预算、root/read-only/Writer admission 均不变。
完整矩阵、门禁、官方协议复核与非结论见
[M7-H canonical request/Token 浪费矩阵与 terminal catalog 结论](../../eval/summaries/m7-h-request-token-waste-2026-07-23.md)。

下一切片只补当前 v16/v21 production trace 对 per-request context estimate、actual catalog、
compaction 与 hard-budget boundary 的可复现 near-limit coverage；不得新增可由
`ModelRequestPrepared`/RunStore 重建的第二状态真相。只有该覆盖出现新的真实 production
反例时才改机制；否则关闭 request/Token 调优并转入 M8 准入清理。

### M7-I：near-limit context/request budget 可复现闭环

M7-I 从 clean `6e9e9b7b`、tree `d360072e4d2af26612061d6e47d620682a585853`
开始。只读调用图确认唯一事实链未分叉：Runtime 选择 ordinary/terminal permit 与实际
catalog，ContextBroker 用同一 catalog 估算并按需压缩，RuntimeEvent v16 持久化
`ContextCompactionCommitted` 和完整 `ModelRequestPrepared`，State v21 reducer/SQLite
reopen 重建请求，DeepSeek planner 再从该 `ModelRequest` 确定性生成 `RequestPlan`。

`f8d0b242` 只增加 current-revision coverage、冻结 manifest 和只读 Harness，没有修改
production Rust、协议或 schema。新增 production-composition 回归在真实临时 Git workspace
中覆盖 root、read-only child、explicit Writer，逐 actor 证明 actual catalog、effective
context estimate 和 DeepSeek `RequestPlan` 在 SQLite reopen 前后精确一致；另一个 Runtime
回归证明 mandatory facts 超限返回 typed `ContextLimitExceeded`，且
`ModelRequestPrepared`、compaction 和 ModelPort 调用均为 0。`b1894069` 只收敛测试代码的
strict Clippy 表达式，不改变语义。

冻结的 13 项矩阵覆盖 ordinary/terminal、普通与 terminal compaction、mandatory facts
over limit、逻辑/物理请求预算、partial failure、cancel、compaction SIGKILL/reopen、
read-only child、Writer 和真实 Git/verifier loopback。Harness 在 clean `f8d0b242` 上以
`maximum_reruns=0` 完成 16/16 exact gates；ignored `0600` result 为 3,616 bytes，
SHA-256 `aac12ec7c16b8ce28d5acf0cd2b5753297a838b12d68fbd46382ab3edfd5dc2c`。
focused、fmt、workspace Clippy/test 和 Harness self-test/formal 均通过。

产品决策为 **close request/Token optimization / admit M8 cleanup**。本切片没有 material
production delta，没有模型、提示词、reasoning、catalog、预算、context policy 或 Provider
surface treatment，因此 `product_metric_eligible=false`；Key 未读取，官方 API 请求和网络
访问均为 0。M7-I 只证明 current v16/v21 near-limit correctness 可复现，不声明 Token、
时间、费用或一般任务成功率收益。完整身份、矩阵、门禁与非结论见
[M7-I near-limit context/request budget 结论](../../eval/summaries/m7-i-near-limit-context-2026-07-23.md)。

下一切片进入 M8，先审计 DeepSeek-only credential/config/model/Doctor/onboarding/help 的真实
production caller 与 generic Provider 遗留；每个 caller 切换到唯一 DeepSeek owner 后物理
删除旧路径，不把品牌改名、发布或新模型选择系统混入同一切片。

### M8-A：DeepSeek-only production 配置与入口切换

M8-A 从 clean `8b650356` 冻结 P01-P12 离线矩阵，并按真实 caller 分三个提交完成切换：

1. `e1a611ff`：CLI/auth/model/config 只解析 DeepSeek；credential precedence 固定为
   CLI -> config -> keyring -> env；删除 `crates/agent`、generic Provider 参数、跨 Provider
   OAuth/keyring/model registry。
2. `63246e72`：交互 TUI、Doctor、onboarding、exec projection 与 Fleet worker argv 使用同一
   DeepSeek 配置；删除 TUI provider catalog、provider OAuth、route/billing/scorecard 和
   私有模型路由。
3. `00dcda0c`：`crates/config` 成为唯一根级 credential/endpoint/model owner；project config
   和 named profile 不得改变模型权威；删除 generic Provider、catalog、pricing、alias、
   fallback 和 route resolver，且不保留 compatibility reader 或 dual write。

模型后端和 canonical 执行链没有变化；Run API v10、RuntimeEvent v16、State v21、
exec-stream v2 保持不变。起始到代码 cutover 共 71 files、`+3,689/-42,712`，净删除
39,023 行。首次启动、无 Key、外来 Provider/模型、endpoint/TLS、credential precedence、
resume/reopen、root/read-only/Writer、真实 PTY 与 exec/HTTP/stdio parity 均通过。示例配置
由 `crates/config` 与交互 TUI 同时解析验证。

本切片没有改变请求、模型、reasoning、prompt、catalog 或预算，付费请求不能增加归因力；
按冻结 gate 未读取 Key、未调用官方 API、未访问外部网络（仅使用本机 loopback）。产品决策为
**保留 DeepSeek-only 切换，删除 generic Provider 路径**。完整矩阵、门禁和非结论见
[M8-A DeepSeek-only production 配置与入口结论](../../eval/summaries/m8-a-deepseek-only-entry-2026-07-23.md)。

### M8-B：CodeWhale 产品身份与可复现本地交付

M8-B 从 clean `2ed3efe3` 冻结 D01-D12，并按真实 caller 完成四个 cutover：

1. `eddfd4bc`：workspace metadata、embedded build identity、User-Agent 与 binary discovery
   只使用 CodeWhale；正式 binary set 固定为 `codewhale`、`codewhale-tui`，删除 `codew`
   与 DeepSeek-branded 产品 override。
2. `ccc98245`：把仍被使用的 TLS helper 收回 TUI owner，Doctor 不再请求 imported release
   metadata，物理删除 `crates/release`、CNB/GitHub updater discovery 和专用依赖。
3. `d792113e`：config/state/settings/secrets 只读写 `CODEWHALE_HOME`、
   `CODEWHALE_CONFIG_PATH` 与 `.codewhale`；删除 `.deepseek` migration/fallback、第二
   metrics state truth 和产品 env compatibility reader。
4. `4aff11f6`：`scripts/codewhale-delivery.sh` 成为唯一 package/install/verify/rollback/
   uninstall owner；CI 的 macOS/Linux matrix 运行同一 lifecycle。`28f8a34c`、
   `e4232142`、`307f6c09` 随后让离线 source build 只复用已安装且版本精确匹配的 Rust
   1.97.0，不自动联网安装。

candidate `307f6c09` 的 macOS source artifact 为 17,660,358 bytes，manifest 绑定完整
revision/tree、Cargo.lock SHA、target、Rust 和准确两项 binary set；相同输入的 fixture
archive SHA 可复现，archive/binary tamper、wrong target 均在 activation 前拒绝。真实
macOS package/install/verify/Doctor/uninstall 在 `sandbox-exec` 禁网下通过；Linux 同一
lifecycle 在缓存 `golang:1.26-bookworm`、`--network none` 下通过。uninstall 只删除程序与
delivery metadata，用户 sentinel 保持 byte-identical。

Run API v10、RuntimeEvent v16、State v21、exec-stream v2 和
`AgentApplication -> AgentRuntime -> RunStore` 均未改变。相对 baseline 共 59 files、
`+1,574/-3,276`，净删除 1,702 行；Key 未读取、官方 API 请求 0。最终只读 source gate
有一次漏设 `RUSTUP_TOOLCHAIN=stable`，rustup 更新探测被立即中断；它不属于 delivery
owner，结果记录明确披露，不能扩张为整个 session 的 no-network 结论。产品决策为
**保留唯一 CodeWhale 身份与交付 owner，收缩/删除 imported delivery 路径**。完整结果见
[M8-B 产品身份与本地交付结论](../../eval/summaries/m8-b-product-delivery-2026-07-23.md)。

随后 M8-C 单独建立 fixed `zh-Hans` 产品界面：冻结 CLI/TUI/Headless/Doctor/错误恢复/
多 Agent 状态的真实 user-facing caller、英文泄漏、CJK 宽度和 machine-protocol 稳定矩阵；
只翻译 Host 生成且保留的文本，不改变 raw provider/tool output，也不混入中文 Agent prompt
A/B、MCP、RepoGraph、多 Writer 或模型选择。该切片现已按下节完成。

### M8-C：固定 zh-Hans 产品界面

M8-C 从 clean `e99bf6c7` 冻结 L01-L14，并以 `crates/localization` 建立 CLI/TUI 共享的唯一
compile-time message owner。原 TUI 私有 `localization.rs` 与 167-key catalog 已删除；
candidate `44b17940` 的共享 catalog 为 427 keys。真实 CLI/TUI/app-server help、Doctor、
Headless/recovery、`request_user_input` 与多 Agent Host chrome 固定为中文，不读取
`LANG/LC`，也没有语言选择、第二 catalog 或 translation model call。

machine/raw 边界保持不变：命令、flags、enum、Doctor JSON、exec stream-json、
HTTP/SSE/stdio、模型/工具 ID、路径、代码、diff 和 raw provider/parser/tool/stdout/stderr
不被后处理翻译。`crates/protocol`、`crates/runtime`、`crates/state`、`crates/app-server`
相对 baseline 的 diff 为 0，Run API v10、RuntimeEvent v16、State v21、exec-stream v2 与
`AgentApplication -> AgentRuntime -> RunStore` 未改变。

L01-L14 全部通过；foreign locale 的真实安装包、80/120 列 help 与
`request_user_input`、中文 PTY、root/read-only/Writer、process crash/SQLite reopen、
workspace Clippy/test 和两次 hermetic TUI 均通过。真实 locked/offline artifact 绑定
candidate `44b17940`、tree `f4444098`，两项 installed binary 的 version 身份一致。
相对 baseline 共 31 files、`+2,741/-963`，净增加 1,778 行；该正增量只作为一个共享
427-key interface contract 与测试的成本记录，不作为能力指标。

产品决策为 **保留唯一 fixed zh-Hans owner，收缩/删除重复产品文案路径**。本切片没有
模型 treatment，Key 未读取、官方 API 请求 0，也不声明 verified coding success、Token、
时间或费用改善。完整结果见
[M8-C 固定 zh-Hans 产品界面结论](../../eval/summaries/m8-c-fixed-zh-hans-interface-2026-07-23.md)。

### M8-D：中文原生 Agent prompt 同任务 A/B

M8-D 以 M8-C 后的 `491c069c` 为基线，只把 constitution 的固定五步 checklist 替换为
两句事实缺口循环。candidate 从未替换 bundled prompt；真实 treatment 通过既有
config-home override 注入。审计发现 `codewhale app-server` 没有像 exec/TUI 一样加载
该 context-owned override，`8371b6dd` 完成唯一 caller 修复并冻结两项 release binary：
`codewhale` SHA `d7d6afbe…98f823b`、`codewhale-tui` SHA
`f0239151…5661c4f`。

正式 contract 固定 `deepseek-v4-flash`、当前官方
`https://api.deepseek.com/chat/completions`、相同 tool/permission/budget/cache suffix、
五个任务、3 runs/cell、30 arms 和 `maximum_reruns=0`。`deepseek-chat` 与
`deepseek-reasoner` 旧模型别名没有进入请求；2026-07-24 退役的是旧 alias，不是
`/chat/completions` surface。FIM 独立 Beta surface 未调用。

v1-v4 的 2/5/6/7 completed arms 依次暴露 app-server 无 delta、旧 multi projection、
错误 Writer fixture Git identity 和错误 Writer lifecycle/scope projection；每个 successor
都使用新 schema/Harness/output，从 position 1 开始，旧 raw 保持 0600 且不续跑、不拼样。
v5 已离线证明 current nested AgentTask、aggregate accounting、五个 fixture identity 与
clean integrated Writer `base..HEAD` scope，但第一个 baseline arm 后
`billing_unknown=true`、`complete=false`、`surface_usage=[]`。Harness 立即停止，
没有第 2 个 arm。

因此没有完整、计费可证明的正式矩阵。production bundled prompt 不接管，产品决策为
**hold prompt candidate / keep app-server override consistency**。不存在候选 production
prompt branch 可删除；保留 ignored fixture/Harness/manifests/raw 作为身份与停止证据。
v1-v4 可证明费用下界合计 `$0.052454002`，v5 费用未知，不能补算总费用。完整事实见
[M8-D 中文原生 Agent prompt A/B 结论](../../eval/summaries/m8-d-native-zh-prompt-ab-2026-07-24.md)。

### M8-E：V1 退出证据总审计

M8-E 以 clean `433a871b` 为 frozen production input，逐项审计 PRODUCT_PLAN 的 16 项
V1 完成定义。`eval/manifests/m8-e-v1-exit-audit-v1.json` 绑定该 revision、tree、
Cargo.lock、toolchain 和四份权威文档 blob；`scripts/test-eval-m8e-v1-exit.py` 从 frozen
revision 读取权威输入，后续文档更新不能反向改变原审计。

结果为 8 pass / 8 blocked：

- pass：唯一 DeepSeek backend、AgentRuntime、RuntimeEvent、RunStore，root/child 同一
  conformance，CLI/TUI/API 薄客户端，latest-revision EvidenceReceipt，fixed zh-Hans；
- blocked：单一 TaskGraph、多 Writer V1 完成面、完整 Standard/Strict/FIM routing、
  RepoGraph、重复产品/状态彻底删除、imported-baseline coding A/B、中文 prompt A/B、
  workflow-step 不增证明。

2026-07-24 官方复核确认 `deepseek-chat`/`deepseek-reasoner` 是退役的 legacy model
alias，`deepseek-v4-pro`/`deepseek-v4-flash` 保留；官方同时支持 OpenAI 格式和 base URL
为 `https://api.deepseek.com/anthropic` 的 Anthropic Messages。后者只是官方兼容接口，
不是 CodeWhale 产品需求。PRODUCT_PLAN 固定的 production 路线仍是 DeepSeek Standard
Chat、lossless Strict fallback 与独立 FIM；当前 Chat request/parser/replay/accounting
应保留。

clean `a12bea45` 的 locked/offline source artifact 绑定 tree `966afe2f`、Cargo.lock
`ff53b498…f0ef2`、Rust 1.97.0，只包含 `codewhale` 和 `codewhale-tui`。归档 SHA 为
`6104e450…a86ffc`，真实临时 prefix install/verify 与两项 version identity 通过。
focused、fmt、workspace clippy/test、exec/HTTP/stdio、canonical PTY、root/read-only/
Writer、SIGKILL/reopen 和 delivery self-test 全部通过。这只证明当前 baseline 可复现且
内部一致，不满足缺失的 8 项 V1 条件。

产品决策为
**not releasable / keep canonical chain and delivery / hold prompt、FIM、fan-out 与 broader
request/cache 调优**。M8-D v5 不续跑，Key 未读取，官方 API 请求 0。完整结论见
[M8-E V1 退出证据总审计](../../eval/summaries/m8-e-v1-exit-audit-2026-07-24.md)。

M8-F 的 Anthropic Messages cutover 因 `invalid_premise` 取消。冻结它的
`066e15cb` 保留在 Git 历史中，但 manifest、专属测试和全部未提交 production WIP 已由
纠正提交删除；没有读取 Key 或调用官方 API。下一切片回到真实 V1 blocker：先把
`ProductionAgentOrchestrator` 与 Fleet/Lane 收敛为一个 TaskGraph 产品概念，并在
cutover 后删除重复协议、配置、状态和 UI 词汇。随后再处理 FIM 产品范围/证据、
imported-baseline coding/workflow-step A/B 与 billing-provable 中文 prompt successor。

### M8-G：单一 TaskGraph 产品概念收敛

M8-G 以 clean `650df581` 为 baseline。只读调用图确认 canonical TaskGraph 已由
`AgentTask` / `AgentOutcome`、`AgentRuntime` child lifecycle、`RunStore` 持久真相和
`ProductionAgentOrchestrator` 的 explicit Writer Git side effects 共同实现；不存在
需要新 crate、DTO、Store 或 scheduler 才能填补的执行缺口。

code candidate `64f6bc16` 物理删除 Fleet protocol/config/ledger/lease/scheduler/SSH/
alerts/worker/UI、bundled skill，以及完整 Lane registry/tmux/inline/process shell。
`codewhale fleet`、`codewhale lane` 和 `/fleet` 不再是产品面；旧调用在配置或模型启动前
fail closed。setup-state 从 7 步收敛为 6 步并升到 schema v2，旧 Fleet-bearing record
不兼容读取；Doctor 只投影 canonical `task_graph` owner/actors。CLI 可见命令从 20 降到
18。

相对 baseline 为 48 files、`+150/-15,260`，净删除 15,110 行。Run API v10、
RuntimeEvent v16、State v21、exec-stream v2 和唯一
`AgentApplication -> AgentRuntime -> RunStore` 未改变。focused、workspace strict
Clippy/test、root/read-only/explicit Writer、exec/HTTP/stdio parity、SQLite reopen、
process SIGKILL recovery、fmt 与 diff gate 全部通过。Key 未读取，官方 API 请求 0。

决策为 **keep canonical TaskGraph / delete Fleet and Lane / close V06**。V12 已收窄但仍
需独立审计 generic provider vocabulary；V16 只证明本切片步骤下降，不能替代相对
imported `352e86a6` 的同任务 workflow-step A/B。下一切片冻结 Standard/Strict/FIM 的
真实 V1 产品范围：只有编辑基线证明主要瓶颈仍在生成且 FIM 有可归因 surface delta 时
才建立最小 Host-owned 候选，否则保持 hold/reject 并转入 imported-baseline A/B。
完整结论见
[M8-G 单一 TaskGraph 产品概念收敛](../../eval/summaries/m8-g-taskgraph-convergence-2026-07-24.md)。

### M8-H：FIM 产品范围与无消费者历史债收敛

M8-H 以 clean `dce858d0` 为 baseline，先冻结
`eval/manifests/m8-h-fim-scope-debt-v1.json`。M7-C 已关闭 12/12 deterministic Host
编辑反例；M7-D 之后没有新的 current RuntimeEvent v16 production 编辑失败样本，也没有
同 binary FIM treatment。静态 caller graph 进一步证明：

- `plan_fim` 生成 `https://api.deepseek.com/beta/completions`；
- 唯一 `DeepSeekEndpoint::owns_url` 只允许 Standard/Beta Strict Chat URL；
- 唯一 non-streaming parser 读取 `choices[0].message`，不是 FIM `choices[0].text`；
- Runtime/RunStore 没有 fresh-read/revision/prefix/suffix-bound Host apply lifecycle；
- production caller、response parser、apply 和 reopen 数量均为 0。

因此 live FIM A/B 在 credential/API 前仍为
`inadmissible_no_surface_delta`。code candidate `7d9aa9a6` 删除 `plan_fim`、
`FimPlanError`、FIM surface/accounting、`fim_response_count`、eval-only
`fim_edit`/`write_file` classifier 债；同时让 config 成为 V4 模型目录唯一 owner，
`deepseek-chat`/`deepseek-reasoner` 与 speculative `deepseek-*` 不再静默改写或延迟失败。
TUI 删除重复 model switch、永远为空的 alias retirement DTO/Doctor 分支和对应文案。
冻结 M7-C/M8-E 历史证据以及有独立协议价值的 direct live Harness 保持不改写，不成为
production caller。

删除公共 terminal 的 always-zero FIM 字段后 exec-stream `v2 -> v3`，不保留兼容 reader。
RuntimeEvent v16/State v21 不变，因为 production sender 从未能产生 FIM `SurfaceUsage`，
已有 Standard/Strict serialized event 形状不变。相对 baseline 为 26 files、
`+185/-214`，净删除 29 行（新增主要为冻结 manifest）。

决策为 **keep Standard Chat and lossless Strict fallback / reject and delete unreachable
production FIM half-branch / hold FIM re-entry**。这不证明 FIM 质量较差；只有新
production 失败证据把 patch generation/recovery 定位为主要损失，并且完整
Host-owned parser/apply/accounting/reopen treatment 先成立，才可重新准入。V09 仍是
`blocked_product_scope_decision`，不能把“准确地不支持”冒充 PRODUCT_PLAN 的完成。
fallback 随后对 exact imported `352e86a6` 构建 locked/offline immutable binary，并在
本机回环证明它可使用相同 `/chat/completions` 与 `deepseek-v4-pro`；但旧 exec terminal
没有 request count、cost completeness/bucket 或可重开的 root/child started ledger，
retry count 也未知。按完整 accounting 门禁，live coding/workflow-step A/B 在 credential
前为 `inadmissible_incomplete_baseline_accounting`；不修改旧 baseline、不读 Key、
官方请求仍为 0。

完整结论见
[M8-H FIM 产品范围与历史债收敛](../../eval/summaries/m8-h-fim-scope-debt-2026-07-24.md)。

### M8-I：Host typed 保守 Auto 路由

M8-I 以 clean `15fea38e` 为 baseline，先提交
`eval/manifests/m8-i-host-auto-route-v1.json` 冻结旧 classifier 的 request/body/timeout、
fallback heuristic、whole-tree inheritance 和四 variant A/B 门禁。真实问题不是需要更强
prompt classifier，而是旧路径会在 `RunCreated` 前额外发送一次
`deepseek-v4-flash` non-streaming Chat 请求，却没有相对 fixed Pro 的 verified-success
非劣证据。

code candidate `ef65bafa` 建立唯一 `crates/app::ProductionModelRoutePolicy`：

- 显式 `deepseek-v4-pro`/`deepseek-v4-flash` 与显式 reasoning 原样保留；
- Auto root 固定 Pro；普通 reasoning 为 high，只有 typed recovery 才为 max；
- 普通 read-only child 为 Flash/high，failed child 的新 recheck 为 Pro/max；
- explicit isolated Writer 仍 explicit-only，Auto selection 为 Pro/high，typed rework 为
  Pro/max；
- 当前没有 typed bounded-low-risk/no-tools 产品事实，因此不授予 root Flash；
- 每个 `RunRequest`/`AgentTask` selection immutable，不做 mid-run switch。

RuntimeEvent v17/State v22 强制 `ModelRouteAudit`，而 selected model/reasoning、actor 和
workspace authority 继续使用既有 canonical 字段，不增加第二份状态真相。pre-v17
materialized run 无法诚实恢复 caller intent，直接退役；pending Start 因创建前已无模型
副作用而保留并可 exact recovery。Run API 升到 v11，只删除失去生产者的
`DeepSeekAutoRouteFailed` 与 creation unknown-billing projection。

cutover 物理删除 530 行 `crates/deepseek/src/auto_route.rs`，以及 classifier
prompt/parser/provider DTO、关键词/500 字 fallback、空 `recent_context`、startup route
分类和旧 PTY guard。production loopback 证明 Auto root 的首个且唯一物理请求为 Pro，
两个普通 read-only child 为 Flash，回到 Pro root 汇聚；SQLite 重开保持 exact route。
focused、workspace strict Clippy/test、exec 25/25、app-server 23/23、PTY 6/6、State
SIGKILL/reopen、M7-C/DeepSeek Harness 与 M8-E 8/8 contract 全部通过。

formal A/B 未读取 Key：variant C 的 frozen old classifier 只存在于 `15fea38e`，variant D
只存在于 `ef65bafa`；删除旧 production branch 后没有同 revision/immutable binary
同时承载 A/B/C/D 的真实 surface。为评测重新加入 classifier toggle 会违反本切片的单
owner/cutover 删除边界。因此准入结论为
`inadmissible_no_single_binary_four_variant_surface`，不产生 verified success、Token、费用
或 wall-time 产品指标。

当时决策为 **keep explicit models and Host policy / delete prompt classifier / hold Auto
default admission**。该未来准入条款已被 M9-D/ADR-0008 supersede：Auto 产品方向现已
退休且不会重开。完整历史结论见
[M8-I Host typed Auto 路由](../../eval/summaries/m8-i-host-auto-route-2026-07-24.md)。

### M8-J：V1 successor 审计与 V12 历史债删除

M8-J 以 clean `bce36a53`、Run API v11、RuntimeEvent v17、State v22、exec-stream v3
重新计算 M8-E matrix。冻结输入保持 8/16 不改写；M8-G 已关闭 V06，M8-H 只收敛 FIM
半分支而没有满足 V09，M8-I 不对应 V1 item。因此候选前 current truth 是 9 pass /
7 blocked，V12 是最小且仍有真实 production consumer 的 blocker。

只读 caller graph 证明旧链是第二产品/状态真相：

- `codewhale thread` 暴露 list/read/resume/fork/archive/unarchive/set-name/clear-name，
  但读写的是独立 `threads` metadata 表；resume/fork 委托已退休的 TUI thread 语义；
- `StateStore` 除 canonical run/event/snapshot/creation 外还维护 `threads` 与
  `session_index.jsonl`；
- protocol crate 根部的 Thread/App/Prompt/EventFrame DTO 只有自身 parity test，
  production consumer 为 0；
- `RunEnvironment.provider="deepseek"` 是 canonical replay-safety fact，execpolicy 的
  network policy types 也有真实 caller，不属于 generic Provider 产品债。

manifest commit `9e644add` 先冻结该边界。code candidate `bcbc1616` 迁移真实入口到既有
`codewhale runs`、`codewhale resume` 与 `exec --resume/--continue`，随后物理删除旧
CLI dispatch、thread CRUD/session index、无消费者协议/本地化/测试。State schema
`v22 -> v23` 在同一 `IMMEDIATE` transaction 中 `DROP TABLE IF EXISTS threads`；fresh
v23 不创建旧表。定向反例证明：

- exact v22 debt 升级后 current canonical run 原样 replay，旧表消失；
- 注入 drop failure 时 user_version 与旧对象一起回滚；
- 9 种旧 `thread` spellings 在 config、TUI、RunStore、credential/model 前 fail closed；
- root/read-only/Writer、HTTP/stdio/exec、SQLite reopen 与 SIGKILL recovery 不退化。

code candidate 为 11 files、`+232/-1,797`，净删除 1,565 行；连同冻结 manifest 相对
baseline 为 12 files、`+398/-1,797`。决策为
**keep canonical RunStore / shrink and delete legacy Thread truth / close V12**。
current matrix 为 10 pass / 6 blocked，剩余 V08 multi-Writer、V09 FIM scope、V10
RepoGraph、V13 imported-baseline coding A/B、V15 billing-provable 中文 prompt A/B、
V16 imported-baseline workflow-step A/B。没有模型 treatment 或可付费产品指标，Key
未读取、官方请求 0。

完整结论见
[M8-J V1 successor 与 V12 历史债删除](../../eval/summaries/m8-j-v1-successor-v12-debt-2026-07-24.md)。

### M8-K：V08/V09/V10 产品范围 successor

M8-K 以 clean `49a46581`、Run API v11、RuntimeEvent v17、State v23、exec-stream v3 和
10 pass / 6 blocked 的 current matrix 开始。manifest commit `b44d7ff9` 在任何 authority
变化前冻结 V08/V09/V10 的实现准入合同：

| Item | current production | 可归因新失败 | 同 binary treatment | 决策 |
|---|---|---:|---:|---|
| V08 multi-Writer | 每 root 一个 explicit Writer，完整 Host lifecycle | 0 | 无 | keep single / reject V1 multi-Writer / hold re-entry |
| V09 FIM | official Standard/Strict Chat；FIM 半分支已删除 | 0 | 无 | keep Chat / reject V1 FIM / hold re-entry |
| V10 RepoGraph | canonical search/read/diff + ContextBroker + verifier | 0 | 无 | keep bounded cross-file outcome / reject named V1 graph / hold re-entry |

三项都不满足“current typed failure、真实 caller、deterministic verifier、同 revision
immutable treatment、完整 accounting”这组实施准入条件，因此没有选择 production
功能切片，也没有读取 Key 或请求 API。为补清单恢复旧 FIM、增加第二 Writer scheduler，
或创建无 caller RepoGraph 会直接违反已有正式证据。

ADR-0005 记录产品范围 successor：

- V08 由现有显式单 Writer 的完整 lifecycle 验收；multi-Writer 不是 V1 门槛；
- V09 由 Standard Chat 和 Strict 整目录准入/无损回退验收；FIM 不是 V1 门槛；
- V10 由 canonical 搜索/读取、bounded ContextBroker 和 deterministic verifier 的跨文件
  结果验收；RepoGraph 实现名不是 V1 门槛。

这不是宣布三种优化已经完成或无价值。任何一项重开都必须出现新的可归因 production
失败，并从 frozen task position 1 建立完整垂直 treatment。按新的能力定义，V08/V09/V10
关闭为 pass，current V1 matrix 为 13 pass / 3 blocked；剩余 V13 imported-baseline
coding comparison、V15 billing-provable 中文 prompt comparison 和 V16
imported-baseline workflow-step comparison，V1 仍不可发布。

manifest：

- `eval/manifests/m8-k-v1-scope-successor-v1.json`。

accepted scope candidate 为 `6a99cb79ee1a63c57215456b5f0f040659b9426d`，tree
`60e80a06c8d0a0f4b097a6af6d22a5df39205d25`。完整结论见
[M8-K V1 产品范围 successor](../../eval/summaries/m8-k-v1-scope-successor-2026-07-24.md)。

### M8-L：V13/V16 release benchmark successor

M8-L 以 clean `4fef6a34`、Run API v11、RuntimeEvent v17、State v23、exec-stream v3 和
13 pass / 3 blocked matrix 开始。manifest/Harness commit `14319b11` 在 authority
变化前冻结 imported/current 可比性、qualified real-model evidence、current release
regression 与 user-action contract。

只读 audit 独立确认 exact imported `352e86a6` 的 task/model/surface、binary、外部
fixture/verifier 与 wall-time boundary 可以对齐；不能对齐的是它没有记录的物理事实：

- terminal `retry_count` 在三个 production 构造点都是 `None`；
- 没有 `api_request_count`、started/in-flight、failed/incomplete usage、root/child
  aggregate 或 cost completeness/bucket；
- 旧 Engine 内至少两层可以重新发送相同 stream request，最终 `TurnComplete` 只携带
  observed aggregate usage；
- 旧 app-server 没有 canonical `TaskContract`/`EvidenceReceipt`，SQLite reopen 不能重建
  每个 root/child attempt。

因此 paid imported/current A/B 为
`inadmissible_incomplete_baseline_accounting`。它在 Key/output/API 前停止；不修改旧
revision，不用外部价格估算补账。

ADR-0006 把 V13 收敛为一条可复核 evidence chain，而不是伪造旧臂 superiority：

1. M5-A 的 12/12 official DeepSeek arms 作为窄范围 real coding/false-success evidence：
   coding 两侧均 3/3 verified，candidate false-success 0；forced false claim 从 baseline
   3/3 false-success 变为 candidate 3/3 correct rejection，全部 accounting 完整；
2. exact current candidate 通过
   `AgentApplication -> AgentRuntime -> RunStore` 的临时 Git edit、verifier
   failure/recovery、latest receipt、completion rejection、root/read-only/Writer
   RequestPlan/accounting 与 SQLite reopen regression；
3. 未来任何 model-visible treatment 仍须自己的 current same-binary A/B，不能借祖先结果
   跳过准入。

V16 把步骤单位冻结为一次显式 command submission 或 TUI start。login、interactive
start、headless coding、run inspection 与 resume 五个共同 workflow 在 imported/current
均各需一次动作，总数 `5 -> 5`；Runtime event、model request 与内部 verifier step 不算
用户动作，已删除 Fleet/Lane/Thread/session truth 不恢复为 compatibility step。

决策为 **reject inadmissible imported paid A/B / keep release benchmark successor /
close V13 and V16**。current V1 matrix 为 15 pass / 1 blocked；V15 billing-provable
Simplified Chinese Agent prompt comparison 仍 blocked，V1 仍不可发布。production
model/transport/Runtime/Store/protocol/tool catalog 无变化，Key 未读取、official API
requests 0。accepted benchmark candidate 为 `d27553c4c8145a5c0bd3bb0edcf6b6befffa394a`，
tree `f8e7a15694b7f8a62ddd98044fc0838ad1dbeec6`；private formal result SHA-256 为
`fdf2865659fe9bbe61ebf4951e5e0e02adb5cb718c0737ce762911047746fd86`。完整结论见
[M8-L release benchmark successor](../../eval/summaries/m8-l-release-benchmark-successor-2026-07-24.md)。

### M8-M：billing-provable 中文 Agent prompt successor

M8-M 以 clean `d1d6ca5c`、Run API v11、RuntimeEvent v17、State v23、exec-stream v3
和 15 pass / 1 blocked matrix 开始。它不续跑 M8-D v1-v5，而是冻结同一 current
revision、immutable `codewhale`/`codewhale-tui` binary pair、fixed
`deepseek-v4-pro`、5 个任务、30 个全新 arms 和 `maximum_reruns=0`。candidate 仍只把
constitution 五步 checklist 替换为两行 fact-gap loop，不增加 selector、mode、Provider、
Runtime、Store 或 prompt owner。

Harness commit `d723d0b3` 物理替代旧 M8-D Python evaluator/test，并建立 fail-before-loss
`0600` journal：suite plan 在 credential 前 fsync；每 arm 的 exact terminal、无 Key
SQLite reopen 和 verifier snapshot 在 observation 前依次 fsync；日志使用 exclusive
claim、append/no-follow、单调 sequence 和 SHA-256 chain。19/19 self-tests、6/6 SIGKILL
fault windows、immutable production activation、focused、fmt、workspace strict
Clippy/test、exec/HTTP/stdio/TUI parity 和 process crash/reopen 全部通过。live admission
单独冻结在 `d60d5e52`。

正式 suite 的前 5 个 arm 全部 measurement-valid/verified，false success 为 0；31 个
physical responses、144,976 input tokens、13,637 output tokens 和
`$0.042682606` known-cost lower bound 闭合。第 6 个 t5 candidate arm 的首个 Pro
request 产生 typed `deepseek_transport`，response headers/content/finish/usage 均未
观察到。RunStore exact reopen 后 accounting 为：

```text
started=1
surface_responses=0
usage_responses=0
billing_unknown=true
complete=false
```

Harness 按冻结门禁立即写入 `aborted_unknown_billing`，没有执行第 7 arm、重试、补 mate
或续跑。raw 有 33 个完整 hash-chained records、0 partial tail，SHA-256 为
`641d7d80129b4673bd93eb4d4a0af57f278b326357af71c04188b8eae74acec7`。完成的 5 arms
不是 product-metric eligible aggregate，两个完整 pair 不能支持 keep/reject 质量结论。

决策为 **hold prompt candidate / keep bundled prompt / do not resume or splice**。
candidate-only M8-M runner/test 在记录冻结 identity 后删除；frozen manifest、summary 与
ignored `0600` raw 保留。V15 仍 blocked，current matrix 保持 15 pass / 1 blocked，
V1 仍不可发布。

完整结论见
[M8-M billing-provable 中文 Agent prompt successor](../../eval/summaries/m8-m-billing-provable-zh-prompt-successor-2026-07-24.md)。

### M8-N：V15 fixed-Chinese baseline release-scope successor

M8-N 从 clean `21200ccf` 与 15 pass / 1 blocked matrix 开始。审计确认
`PRODUCT_PLAN` 6.1 正确要求任何改变 prompt 语义的未来候选在接管前完成 current
same-revision 同任务 A/B；旧 V15 却把这条 candidate admission rule 扩张成了固定中文
baseline 必须持续发明新 treatment 的发布条件。M8-D/M8-M 两个候选从未接管，且都因
unknown billing 正确 fail closed；它们继续 hold，不续跑、不补 mate、不拼样。

冻结的 V15 successor 连接以下不可变事实：

- bundled constitution 的 SHA-256 为
  `39f2eeb30519e143eed2d4c627fcb97d323c6b95ad9060816a93b72ea994d409`，在
  M5-A baseline/candidate、M6-A Writer canary、M8-M binary 与 current source 中
  byte-identical；
- M5-A 12/12 official DeepSeek arms 全部 product-metric eligible，覆盖 coding 与
  forced-false-claim，40 次请求、144,903 tokens、`$0.004110153` accounting 完整；
- M8-M 的 t1/t3/t5 current baseline observations 均 measurement-valid、verified、
  false-success=0；它们是 exact-current diagnostic，不是 candidate benefit claim；
- current root/read-only child/Writer、prompt provenance、verifier rejection/recovery、
  accounting/RequestPlan SQLite reopen 和 locked/offline release lifecycle 负责
  exact-source retention 与 whole-release rollback。

ADR-0007 因而关闭 V15，但不把旧 full-Chinese treatment 重写为能力提升，也不免除任何
未来语义候选的 A/B。M8-N 没有新的 model-visible delta，live API admission 为
`inadmissible_no_material_treatment`；credential read=false、official API requests=0、
external network=false。cutover 只删除失去消费者的 M8-D candidate-only app test 和
current-tree fixture，保留真实 caller 仍使用的 override loader consistency、冻结 Git
history/manifests/summaries/`0600` raw、唯一 DeepSeek ChatCompletions/Runtime/Store。

V15 关闭后 current matrix 为 **16 pass / 0 blocked**，V1 结论为 **V1 可发布**。该状态
只表示 release-ready；本切片不 push、不发布、不改远端。完整结论见
[M8-N V15 release-scope successor](../../eval/summaries/m8-n-v15-release-scope-successor-2026-07-24.md)。

### M9-A：Host Auto post-V1 release admission

M9-A 从 release-ready `3770feba` 冻结 fixed Pro、fixed Flash 与 Host Auto 的同 binary
正式比较。审计确认 `crates/app::ProductionModelRoutePolicy` 仍是唯一 owner：Auto root
与 explicit Writer 用 Pro，普通 read-only child 用 Flash，typed recheck/rework 用 Pro；
旧 LLM classifier、prompt/parser、关键词/500 字 heuristic、Provider 与 mid-run switch
均不存在。只有普通 read-only child 任务形成真实 paid delta；root-only/Writer 只做离线
conformance。

切片先以失败测试修复 TUI 在 `model=auto` 时丢弃显式 reasoning 的 caller parity 缺陷，
再冻结 3 task × 3 variant × 3 repeat、maximum_reruns=0、同 TaskContract/tools/budget/
verifier 的 27-arm suite。candidate `29c4980f` 的 focused、fmt、workspace strict
Clippy/test、root/read-only/Writer route、SIGKILL/reopen、fail-before-loss journal 与
immutable binary dry-run 全部通过。

正式 suite 在第 18 arm 遇到没有 response headers/usage 的 typed transport failure。
RunStore 精确记录 `billing_unknown=true`、`complete=false`；Harness 在下一 arm 前停止，
没有重跑、补 mate 或拼接。停止前 17 个完整 arms 全部 verified、false success 0、
route/lifecycle/reopen/accounting 有效，但 formal matrix 不完整，不能生成产品 aggregate。
6 个已完成 Auto/Pro pair 的描述性费用 ratio 为 `0.8698`、wall ratio 为 `0.8922`，也未
达到约 20% 门。

当时决策是 **keep Host typed policy / hold Auto default / keep fixed Pro default**。显式
Pro/Flash/reasoning 保留；M9-A-only 1,551 行 runner 删除，manifest/summary 与 ignored
0600 raw 保留。M9-D/ADR-0008 后续删除 Auto，因此该 successor 条件不再是当前产品路线；
若只复核历史 campaign 也不能续跑本次 raw。完整事实见
[M9-A Host Auto release admission](../../eval/summaries/m9-a-host-auto-release-admission-2026-07-24.md)。

### M9-B：post-V1 fixed-Pro coding regression baseline

M9-B 从 M9-A 的 `hold Auto / keep fixed Pro default` 决策开始，冻结一个不含 product
treatment 的 post-V1 regression label collector。六个独立临时 Git repo 覆盖 root
单文件、root 多文件迁移、verifier failure recovery、一个 read-only child、一个
explicit isolated Writer 和 tools-disabled false-completion；每个 task 原定 3 次，共
18 arms。所有请求固定 official `deepseek-v4-pro`、reasoning `high`、streaming
ChatCompletions `POST /chat/completions`、同一 immutable candidate `983fa9ce`、同
TaskContract/tools/budget/verifier、零 transport/runtime retry 和
`maximum_reruns=0`。

Key 前，六个 fixture tree/base、18-arm schedule、hash-chain/SIGKILL Harness、
focused、fmt、workspace strict Clippy/test、root/read-only/Writer/recovery/reopen/
CLI/TUI/API production loopback、release binary 与 credential-free dry-run 全部通过。
正式 v1 的 `root_single` 完整 verified；`readonly_investigation` 的 terminal、Store、
无凭据 reopen、external verifier、route、Host receipt 与 handoff 后 root mutation 也
全部有效，但 Harness 错把 read-only 共用的 `agent_result_collected` 当成 Writer-only
event，产生错误的 `lane_valid=false`。这是预注册的 observer failure，不是模型失败。

第 3 arm 运行中立即中止；没有继续、重跑、补样、续跑或拼接。raw 为 15 个完整
hash-chained records、无 partial tail、0600，SHA-256
`67df6a7056edefbf680113536e9e85b42df46bdd86ce08541865d8962f798fcc`。
两个已结算 arm 合计 12 requests、known cost `$0.021395127`；第 3 arm 没有
terminal/accounting snapshot，可能存在未证明计费的 attempt，因此不估价或加入合计。

observer fix `9342fb03` 把 shared read-only lifecycle 与 Writer-only
workspace/seal/integration/cleanup 事件明确分开并加入 Harness self-test。原 live
admission 绑定 pre-fix Harness hash，修复后自动 fail closed。决策为
**hold incomplete baseline / keep corrected Harness / keep fixed Pro default / do not
resume or splice v1**。

原 contract 规定只有新 runner 实际证明全部六类 lane 后，才删除被替代的 current-tree
M6/M7 one-off runner。v1 未满足 cutover，相关脚本暂时保留；fresh successor 必须用新
manifest/admission/raw 从 position 1 开始。完整事实见
[M9-B fixed-Pro coding regression baseline](../../eval/summaries/m9-b-fixed-pro-regression-baseline-2026-07-24.md)。

### M9-C：position-1 fixed-Pro regression successor

M9-C 没有续跑 M9-B。新的 successor manifest 只按 SHA-256 继承 M9-B 的 task、
tool-policy 与 official-review contract，替换六个 acceptance ID，并从新的 candidate、
admission、immutable binary、schedule position 1 与独占 raw 开始。旧 M9-B admission
在 credential 前被 corrected single Harness 拒绝。candidate `7a91bbaa` 的 fixture、
Harness hash-chain/SIGKILL、focused、fmt、workspace strict Clippy/test、production
root/read-only/Writer/recovery/reopen/CLI/TUI/API loopback、release build、dry-run 与
formal preflight 全部通过。

正式 campaign 第一轮六类 task 全部完成；第二轮又完成 read-only 与 recovery，共 8 个
measurement-valid arm result、49 个已结算请求、known cost `$0.075802984`，全部达到
预期 outcome 且 false success 0。第 9 个 scheduled arm 是第二轮 explicit Writer；
它在 child 创建前的首个 Pro 请求发生无 response headers/usage 的 typed
`deepseek_transport`。RunStore 精确记录 started/completed `1/1`、usage responses 0、
`billing_unknown=true`、`complete=false`。Harness 写完 terminal、Store、无凭据
SQLite reopen 与 verifier 后以 `accounting_incomplete` 停止，没有第 10 arm、重跑、
补 mate、续跑或拼接。

决策是 **hold incomplete M9-C successor / keep fixed Pro default / keep corrected
Harness / retain old M6/M7 runners**。18-arm cutover gate 未满足，因此不生成 baseline
aggregate，不删除 `eval-m6-writer-benefit.py`、`eval-m6-writer-canary.py`、
`eval-m7-agent-convergence.py` 或 `eval-m7g-readonly-fanout.py`。manifest、admission、
summary 与 ignored 0600 raw 保留。

M9-A 与 M9-C 都因 response 前 transport failure 导致 unknown billing。M9-D 的范围删除
先独立完成；此后才审计不选择性重采样、仍 fail-before-loss 的 billing-provable
campaign acquisition contract。不得机械再开 successor或弱化 accounting。完整事实见
[M9-C fixed-Pro regression successor](../../eval/summaries/m9-c-fixed-pro-regression-successor-2026-07-24.md)。

### M9-D：Auto retirement / fixed actor route cutover

用户明确接受 fixed-Pro effect-first 作为长期方向，并要求彻底删除 Auto，而不是继续
`hold` 或保留未来 admission。ADR-0008 因而冻结唯一 production route policy：

- 未显式指定模型的 root 为 Pro/high；
- 显式 Pro/Flash/reasoning 原样保留并由 child 精确继承；
- fixed-profile 普通 read-only child 为 Flash/high，显式 isolated Writer 为 Pro/high；
- typed recovery/recheck/rework 为 Pro/max；
- 不存在启动前 classifier、关键词 heuristic、额外模型路由请求或运行中动态切换。

切片物理删除 model/reasoning Auto 的 config、CLI/TUI/API 输入、protocol 枚举/字段、
app policy 分支、显示与 current fixed-Pro Harness 旧投影。中性 `ModelRouteProfile`
继续记录 actual route，不保存第二份 caller Auto intent。Run API v12、RuntimeEvent v18
与 State v24 直接切换；v23 materialized runs 可能包含无法无损映射的 omitted
reasoning wire 语义，因此整体退休，只保留能按 v18 命令直接反序列化的 pending Start。
没有 compatibility reader 或 dual write。

这是用户接受的范围删除，不是模型 treatment：不读取 Key、不调用官方 API、不重开
M9-A，也不需要付费证明 Auto 无收益。M8-I/M9-A 的 frozen manifest/summary/raw 保持
历史字节不变；当前权威结论改为 `retire_and_delete_auto / keep fixed actor route audit`。
完整事实见
[M9-D Auto retirement](../../eval/summaries/m9-d-auto-retirement-2026-07-24.md)。

### M9-E：fixed-Pro billing evidence boundary

M9-E 给 P0 设定一次官方文档 + 当前 production/Harness caller graph 的有界审计；只有
官方契约能提供 request-level identity、账单 reconciliation 与有界结算语义时，才允许
读取隔离 Key 做最小 canary。复核确认：

- successful Chat response/stream 有 completion `id` 和 usage，但 pre-header failure
  两者都不存在；
- `/user/balance` 是账户级聚合余额，没有 request identity、更新时限或强一致性承诺；
- Usage export 只公开月度 CSV 和按 Key amount 分解，没有公开 request-level schema、
  completion ID 或结算水位；
- `user_id` 只用于内容安全、cache 与调度隔离，不是 billing identity；
- keep-alive 与十分钟排队关闭语义不能回答客户端 pre-header failure 是否已执行/计费。

因此当前官方公开契约不能把 M9-A/M9-C 的物理 attempt 精确追溯为 billed/unbilled。
独占 Key、余额差和月度 aggregate 只能作为诊断，不能满足 formal per-attempt truth。
M9-E 在 credential 前结束，Key 未读取、official API 请求 0，也不增加 sender/header/
accounting/Harness 代码。现有 `billing_unknown -> stop` 保留；未来 fixed-Pro 实验只对
physical accounting 完整的 arms 形成 Token/费用结论。任何准入规则放宽必须另立 ADR，
不得在候选评测中临时修改。完整证据见
[M9-E billing evidence boundary](../../eval/summaries/m9-e-billing-evidence-boundary-2026-07-24.md)。

### M10：fixed-Pro 原生能力优化闭环

M10 不按 Codex/Claude/Kiro/Cursor/Devin 等产品复制功能，而把有效机制收敛到当前唯一
控制闭环：

```text
TaskContract -> 目标/参考输入
ContextBroker -> 状态估计与高信噪比观测选择
AgentRuntime -> 唯一执行控制器
canonical tools + ToolOutcome -> 执行器与传感器
EvidenceReceipt + verifier artifact -> revision-bound 反馈
crates/app typed recovery policy -> 误差控制器
RunStore -> 精确轨迹与恢复状态
corrected eval Harness -> 离线系统辨识与候选准入
```

调用图复核确认已有机制不重建：单 Writer worktree、skills metadata-only/JIT 正文读取、
TaskContract/EvidenceReceipt/latest-revision completion gate、RunStore exact replay、
read-before-edit/atomic tools/typed failure、DeepSeek stable prefix/accounting/reasoning replay
全部保留。M5-B proactive compaction shrink、M7-F cache treatment hold、M7-G read-only
fan-out 未准入结论不改写。

执行顺序固定为 A→B→C→D→E→trajectory analyzer→最后才讨论 read-only fan-out。
每个产品候选都用 fixed Pro、单变量、真实 caller、external verifier 和 accounting-complete
arms；质量门失败或无归因净收益时完整删除 treatment。

#### M10-A：Scoped Context Map

- 真实问题：`ProductionPromptConfig.project_context_pack_enabled=true` 默认在
  `AGENTS.md`/项目指令之后再追加 broad pack；没有说明文件时 ephemeral overview 与 pack
  又调用同一个 `build_project_context_pack`，形成字节级重复。`.codewhale/rules` 和
  `.claude/rules` 的全部 Markdown 当前可 eager 合并至 500 KB。
- 验收：先冻结 prompt fragment ledger；pack-off 单变量必须 verified success 非劣、
  false success 0、关键任务无 treatment-only failure，并使 median cache-miss input
  实质下降（预注册门为至少 10%）且重复 read/search/request 不增加。随后 scoped rules
  只在目标 path/working set 命中时读取正文，compaction/reopen identity 一致。
- owner：`crates/context`；`crates/app` 只保留唯一 production 接线。
- 替代旧路：默认 project context pack、fallback duplicate、eager all-rules assembly；
  不替代 root authority、短 fallback overview 或 skills progressive disclosure。
- 证据：offline byte/token/source/scope/digest ledger 与 fixture；通过后才做同 binary
  fixed-Pro A/B。
- cutover：pack-off 通过则删除配置开关、默认追加和失去消费者的 pack renderer/tests；
  scoped rules 通过则删除 eager rules block。任一阶段失败只删除该 treatment。

第一阶段正式结果：

- `92c8c0db` 在 canonical prompt composer 内建立只读 fragment ledger，记录
  source/scope/sha256/bytes/estimated tokens/stability；正常 production prompt 路径不做
  ledger hash/token 工作，既有模型可见 block fixture 哈希完全不变；
- offline fixture 已证明无说明文件时 overview 与 pack 内含同一序列化 payload，pack-on
  的 README 哨兵出现两次、pack-off 只出现一次；
- `d048146a` 让 app-server 与 exec/TUI 一样读取现有 typed
  `context.project_pack`，从而同一个 immutable binary 可显式构造 control/treatment；
  这不是新模式或 evaluation-only flag；
- `eval/manifests/m10-a-scoped-context-pack-v1.json` 与单独 live admission 冻结
  6 tasks × 2 variants × 3 runs = 36 arms；正式 campaign 从 position 1 开始，
  `maximum_reruns=0`；
- 21 个 arm 形成完整 terminal/Store/reopen/verifier/result；第 22 个
  `writer_migration / pack_off` 首个 root response 收到 headers 和一段 reasoning 后，
  在 finish/`[DONE]`/usage 前发生 typed `deepseek_transport`。accounting 保存
  `usage_incomplete=true`、`complete=false`，Harness 按契约停止；
- frozen raw 的一个 control false-success label 经事件取证证明是 observer
  false positive：canonical Host 已提交 `failed_write_pass` latest-revision receipt，
  external verifier 也通过；旧 lane audit 错误地额外要求 model 自己执行 final pass。
  raw 保持不变，current Harness 改为认 canonical Host final pass 并有正/负 self-test；
- 决策为 `stop_incomplete_accounting / do not admit pack_off`。production 固定
  pack-on；`context.project_pack` 用户/config/app/TUI/CLI 与 M10-A-only Harness branch
  已物理删除，旧 key fail closed。prompt ledger 与 offline duplicate characterization
  保留；
- nested scoped-rules treatment 不启动，因为其 pack-off 前置门未满足。下一独立切片
  进入 M10-B，不续跑、补 mate 或拼接 M10-A。

完整身份、partial observations、typed stop 与删除边界见
[M10-A scoped context pack](../../eval/summaries/m10-a-scoped-context-pack-2026-07-24.md)。

#### M10-B：Budgeted Working-Set Selector

- 真实问题：当前 ContextBroker 只做 transcript/Host facts 的 deterministic projection
  与 hard-limit compaction；没有 task-aware ranked repository regions。
- 验收：在固定行数/Token 预算内输出 `path/range/reason/evidence/digest/expand_hint`；
  localization benchmark 记录 Recall@K、first relevant rank、预算覆盖与无关上下文，
  end-task A/B 还必须降低有效修改前调用数且 verified success 非劣。
- owner：`crates/context` 的现有 ContextBroker。
- 替代旧路：启动 broad pack 和 root 无目标反复 discovery；不替代 `rg/read`。
- 证据：先 deterministic fixture/离线 benchmark，再 fixed-Pro end-task A/B。
- cutover：无最终任务净收益即删除 selector；不建 RepoGraph crate、embedding、向量库或
  LLM reranker。

正式决定为 `reject_quality_veto_and_delete`，同时保留
`stop_incomplete_accounting` 的 acquisition 事实：

- candidate `d79ebf43` 与 live admission `2ec2e8a1` 冻结同一 binary、fixed Pro/high、
  fixed pack-on、6 tasks × 2 variants × 3、`maximum_reruns=0` 与 36-arm 交错 schedule；
- 前 5 个 arms measurement-valid，其中 4 个 verified success；唯一
  `readonly_investigation / working_set_on` 虽通过 external verifier、只改目标文件并有
  latest-revision Host receipt，但 child 调用缺失冻结的 `wall_time_secs=180`，因此
  actor/lane contract 无效，raw 形成 1 个 frozen false-success label。绝对
  `false_success=0` admission gate 已不可满足；
- 第 6 个 `safety_false_completion / working_set_on` 在 response/usage 前发生 typed
  `deepseek_transport`；accounting 保存 `billing_unknown=true`、complete=false、
  started/completed/in_flight=`1/1/0` 并停止。Harness 没有重跑、补 mate、续跑或拼接；
- 矩阵不完整且 task/variant 不平衡，所以不能比较 discovery、Token、时间或费用，也不能
  把部分 localization 指标变成产品收益；但已发生的 measurement-valid false-success
  是独立质量 veto，候选不能准入；
- cutover 已删除 selector、task-aware production caller、volatile map、临时
  `context.working_set` config/CLI/TUI/API 接线、fixture/benchmark 与 M10-B-only Harness
  分支。生产恢复 fixed pack-on、无 Working-Set map 的原路径，不保留兼容 reader 或双轨；
- frozen formal manifest、live admission、localization manifest、ignored 0600 raw、
  Git 历史与 decision summary 保留审计；它们没有 current production consumer。

完整证据见
[M10-B budgeted working-set](../../eval/summaries/m10-b-budgeted-working-set-2026-07-24.md)。
下一独立切片为 M10-C Acceptance Progress；不得用 M10-B 的不完整 raw 为另一候选补样。

#### M10-C：Acceptance Progress Projection

正式决定为 `reject_offline_viability_and_delete`：

- WIP candidate `9fd1ff8d` 证明 Runtime 可只从 TaskContract、receipt/rejection、
  Host verifier observation、workspace revision 与 temporal evidence 派生
  `satisfied/pending/invalidated/evidence_needed`，并使 live Runtime、Store reopen、
  compaction、root/read-only/Writer 与 production loopback 使用同一 request-local
  projection；没有新增 RuntimeEvent、State schema、store、plan 或完成 owner；
- credential 前的同 fixture ContextBroker token gate 显示所有 model-request-visible
  状态均不改善：pending `235 -> 356`，verifier rejection `538 -> 547`；唯一
  satisfied `492 -> 386` 的下降发生在 Host 已 sealed terminal receipt、不会再有下一
  model request 的状态；
- 候选为 20 files、`+1,587/-86`、净增加 1,501 行；current fixed-Pro frozen baseline
  已有两个 verified、false success 0、accounting-complete 的
  `run_verifiers -> edit_file -> run_verifiers` root recovery arms，没有观察到候选要
  修复的 acceptance-progress loss；
- 候选因此在 offline viability gate 被否决。Key 未读取、官方 API 请求为 0、没有
  live A/B 或产品指标；
- cutover 物理删除 projection、prompt marker、config/CLI/TUI/app 接线和 treatment-only
  tests。删除后整个 `crates/` tree 与 M10-C 起点 `11528a99` 字节级一致；production
  继续使用唯一 legacy Host-facts、TaskContract/EvidenceReceipt/latest-revision gate，
  不保留 compatibility reader、dual path 或第二真相。

冻结 manifest 与完整决定见
[M10-C Acceptance Progress](../../eval/summaries/m10-c-acceptance-progress-2026-07-24.md)。
下一独立切片为 M10-D；不得用 M10-C 未发生的 live treatment 推断 success/cost。

#### M10-D：Failure-Directed Recovery Controller

正式决定为 `reject_no_safe_independent_controller_delta`：

- current failure graph 已按 owner 闭合：tools 产生 failure/side-effect/retry truth，
  `ToolOutcome.model_content()` 为三个 actor 给出同一 typed 恢复反馈，Runtime 强制
  verifier failure→有效修改→pass 并只原子重放 replay-safe no-output model failure，
  app 只从真实 rejection/prior child failure 固定选择 Pro/max；
- M9-C/M10-A/M10-B frozen raw 共 9 个带 typed tool failure 的 arm：8 个
  `verifier_failed` 均由 current path 实际恢复；1 个 `workspace_precondition` arm 已通过
  文件 verifier/Host receipt，质量否决来自冻结 child 调用参数不匹配，不是缺少恢复
  action；
- `context_missing`、`reasoning_insufficient` 与泛 `environment_failure` 不是 current
  canonical causal facts；wrong-file semantic 也不能从通用 path rejection 推导。新增
  app map 只能重复现有 owner 或用关键词/模型自评猜测，并会引入第二 controller loop/
  自动副作用；
- deterministic fault injection 通过 schema correction、read-only transport recovery、
  verifier transition、safe retry、retry crash/reopen、重复失败有界、context/budget
  fail-closed、tools failure matrix 与 fixed actor route；
- 没有 production candidate、Key、API、raw 或 live A/B。保留现有 owner-specific
  recovery，不新增/删除 production 路径。

完整矩阵见
[M10-D Failure-Directed Recovery](../../eval/summaries/m10-d-failure-directed-recovery-2026-07-24.md)。
下一独立切片为 M10-E；只有 deterministic environment facts 先建立，环境失败才可能拥有
安全的窄 Host action。

#### M10-E：Reproducible Environment + Runtime Artifacts

正式决定为 `reject_no_measured_environment_or_runtime_artifact_loss`：

- current owner 已覆盖可复现执行核心：RunEnvironment/execution fingerprint 绑定
  workspace、fixed route、tool catalog、retry、authority 与 sandbox；app 在 RunCreated
  前把 TaskContract verifier 解析为 exact plan，resume 要求同一解析结果；
  run_verifiers 捕获执行前后 revision 并只在稳定时产生 hash-checked ToolArtifact/
  EvidenceReceipt；Writer 使用 fresh worktree tools 并在 root 集成后重验；
- M9-C/M10-A/M10-B 的 37 个 canonical Store snapshots 共含 280 个 ToolOutcome：
  `exec_shell=0`、`run_tests=0`、environment/setup/service/UI failure=0；全部 37 个
  acceptance 都是单步 exact `/usr/bin/python3` verifier；
- current TaskAcceptance 只有 Host/Verifier，没有 service/UI runtime 的 typed trigger。
  新增 ProjectEnvironmentProfile 会复制 run_verifiers 的 Rust/Node/Python/Go 确定性
  resolver；全局 browser/service collector 则必须从任务文本、manifest 或泛
  `operation_failed` 猜测，形成第二环境真相；
- deterministic conformance 通过 exact resolver/execution 环境一致、成功/失败 verifier
  不污染 workspace、caller plan 覆盖、latest-revision receipt、HTTP 前 fingerprint
  rejection 与 Writer fresh tools/root reverify；
- 没有 production candidate、Key、API、raw 或 live A/B；不新增 profile/schema/store/
  config/browser/service path，也不删除 TUI Doctor 或 tool-specific dependency diagnostics。

完整矩阵见
[M10-E Environment / Runtime Artifacts](../../eval/summaries/m10-e-environment-runtime-artifacts-2026-07-24.md)。
下一独立切片为 M10-F；未来只有 trajectory 指向一个真实 service/UI false-success
stratum，且能冻结 typed runtime contract，才允许窄切片重开。

#### M10-F：Trajectory Learning（只读 eval）

正式决定为 `keep_read_only_analyzer`，产品结论为
`insufficient_current_loss_evidence_for_a_new_product_candidate`：

- corrected M9-C Harness 增加唯一 `--trajectory-report` 只读 mode；它验证三份 0600
  frozen journal 的 schema/sequence/hash-chain/file identity，并从 canonical Store
  snapshots 派生 strata/context/tool/failure/evidence/recovery，不输出 raw 或 id；
- 37 trajectories / 34 labels / 3 accounting stops 被独立重算；9 个 typed-failure
  trajectories 全部以 canonical Host evidence 恢复，1 个 M10-A frozen false-success
  是已纠正 observer contradiction，1 个 M10-B false-success 是已删除 treatment 的
  child-call contract failure；
- 166 次 read_file 不能直接解释为重复损失。按同 actor、同 mutation epoch、且旧 Tool
  message 仍在下一 ModelRequest 的严格 identity，current controls 的 visible duplicate
  read/tool 都为 0；唯一 visible duplicate 是 treatment 的 1 次 run_verifiers；
- analyzer 因能替代 M10-D/E one-off jq、复算已知结论并拒绝 naive repetition 假信号而
  保留；它不进入 production、不自改 prompt、不用 LLM judge/Key/network；
- 当前没有满足重复、单 owner、离线 fixture、单变量与旧路删除条件的下一 production
  candidate。不得为保持开发节奏恢复 M10-A–E treatment。

完整证据见
[M10-F Trajectory Loss Analyzer](../../eval/summaries/m10-f-trajectory-loss-analyzer-2026-07-24.md)。

#### M10-G：read-only fan-out 最终准入审计

正式决定为 `close_no_admissible_readonly_fanout_benefit_evidence`：

- current canonical mechanism 仍只由一个 `agent` tool、同一 `AgentRuntime`、RunStore
  lifecycle/accounting 和 fixed actor route 组成；Host 没有自动 fan-out policy。多个
  child 只来自同一 DeepSeek response 的显式多个 `agent` calls；
- M7-G 唯一 single-root / two-read-only-child 正式矩阵为 0 个可用 arm，首个联网
  control 的 billing unknown；M7-G2 只证明 11 个 observer fault window，不产生新的
  production delta；
- M9-C/M10-A/M10-B 与 M10-F 共提供 7 个 completed current read-only trajectories，
  但都只覆盖单 child，没有 fan-out control/treatment pair，不能证明质量非劣或 wall
  time 至少下降 20%；
- existing overlap、typed handoff、fixed Flash/high child、Pro/max recheck、SQLite
  reopen、SIGKILL/no-relaunch、partial failure/cancel 与 TUI projection 继续有真实
  consumer，不能作为“无用 fan-out treatment”删除；
- historical M7-G runner 仍由 M9-C 未满足的 baseline cutover 明确保留，M7-G2 runner
  复算 fail-before-loss contract；frozen manifest/summary/raw 保持 immutable；
- active admission 候选关闭。没有 production code/schema/config/raw、Key、network 或
  API；不新增 scheduler、swarm、multi-Writer、投票完成或第二 Runtime/Store。

完整结论见
[M10-G read-only fan-out 最终准入审计](../../eval/summaries/m10-g-readonly-fanout-final-audit-2026-07-24.md)。
M10 fixed-Pro 原生优化闭环至此关闭；后续只能由新的 canonical trajectory 先证明一个
重复、current、可冻结的 production loss，再作为新 Goal 提出，不能把 M10-A–G 继续
保留为机械 backlog。

#### M11：真实多语言 fixed-Pro loss baseline

M11 当前是 acquisition contract，不是 product treatment。它复用
`scripts/eval-m9b-fixed-pro-regression.py` 唯一 corrected Harness，并以独立
`--campaign m11` 选择冻结的 Rust、TypeScript、Python、跨文件、deterministic
recovery、CLI/service、read-only child、显式单 Writer 与安全假完成任务；默认 M9-C
campaign 的 manifest、协议版本和复算行为保持不变。没有第二 evaluator、Runtime、
Store、tool catalog、Provider 或模型路由器。

冻结 contract 为：

- 8 个真实临时 Git fixture × 3 次，从新 schedule position 1 开始，共 24 arms；
- 同一 clean revision、immutable `codewhale` binary、显式
  `deepseek-v4-pro/high`、官方 OpenAI-format `POST /chat/completions`、同一工具目录、
  TaskContract、预算与 external verifier，`maximum_reruns=0`；
- Rust verifier 使用仓库外临时 Cargo target，Node 通过 25.6 原生 TypeScript type
  stripping，Python 使用 3.9 stdlib；每个 fixture 在 Git init 前必须 verifier fail，
  且 verifier 不能改变 fixture tree；
- 每个 arm 必须先持久化 terminal、canonical Store、无 credential SQLite reopen 与
  verifier snapshot，再派生 verified/false-success/lane/accounting label；journal 为
  ignored 0600、exclusive、fsynced、hash-chained；
- `billing_unknown`、usage/accounting 不完整、identity/observer/safety 歧义或成本门
  任一触发即在下一 arm 前停止，不重跑、不补 mate、不续跑、不拼接历史 raw；
- 只有 24/24 measurement-valid、7 个正向 cell 全部 3/3 verified、安全 cell 3/3
  正确拒绝且 false success=0，才形成 current loss baseline；这仍不是收益 A/B；
- 只有同一个 stable current loss 在至少两个独立任务重复，且存在单一 owner、
  deterministic fixture、单变量 treatment 和可删除旧路，才允许另立 production
  vertical slice。单个失败或工具调用计数不能授权实现。

正式 acquisition 已以 `stop_incomplete_accounting` 结束，trajectory 决定为
`insufficient_repeated_current_loss`：

- candidate `f256c497`、admission `ef91861c` 与 immutable release binary 冻结同一
  24-arm contract；focused、fmt、workspace strict clippy/test、root/read-only/Writer、
  RunStore reopen、SIGKILL 与 CLI/TUI/API gates 全部通过；
- 前 4 arms measurement-valid：`readonly_investigation` 与 `typescript_service`
  verified，安全反例正确拒绝，false success=0；`rust_cli` 的双文件 external verifier
  已通过，但 10 次请求后 terminal blocked 且无 Host receipt；M11 当时将它粗粒度投影为
  `verified_workspace_without_terminal_receipt`；
- 第 5 个 `root_recovery` 已落下 terminal、canonical Store、credential-free reopen 与
  verifier snapshot，但 `usage_incomplete=true`、accounting complete=false、
  `billing_unknown=false`。Harness 在下一 arm 前停止，没有重跑、补 mate、续跑或拼接；
- ignored 0600 raw 有 32 个完整 hash-chained records、partial tail=0；5 个 canonical
  trajectories / 4 个 arm results / 1 个 accounting abort 被同一 analyzer 独立复算；
- M12 后续证明 `rust_cli` 是 Host/external verifier Rust 工具链环境不一致，不是
  production loss；即使按 M11 当时投影也只属于一个 task。因此不立 production
  candidate，不恢复 M10-A–G treatment，不新增 completion controller、prompt、tool、
  Runtime/Store 状态或 retry path。

2026-07-25 官方复核仍确认唯一 production sender 使用
`https://api.deepseek.com/chat/completions` 与 `deepseek-v4-pro/high`；2026-07-24
退役的是 legacy model alias，不是 ChatCompletions surface。完整身份、描述性 prefix、
官方来源、门禁与非结论见
[M11 loss baseline](../../eval/summaries/m11-loss-baseline-2026-07-25.md)。

#### M12：Host terminal convergence 跨任务复现

M12 只纠正 M11 环境身份并复现假设，不建立 product treatment：

- candidate `5716713f`、admission `0c055929` 冻结两个独立任务
  `rust_endpoint` / `typescript_cache`，各 3 次，`maximum_reruns=0`；
- Host 与 external verifier 每个 arm 共享同一隔离 `HOME` 和显式 `.rustup` identity，
  固定 Rust 1.97.0；M11 app-server 隔离 `HOME` 缺少 rustup default 的稳定错误签名因此
  被分类为 `evaluation_environment_mismatch`；
- 6/6 verified、false success=0、32 physical requests、212,278 input tokens、
  32,904 output tokens、119,552 cache-hit、92,726 cache-miss、known cost
  USD 0.069395666；每个 arm 都有 latest-revision Host receipt、external verifier、
  accounting complete 与 exact SQLite reopen；
- combined M11+M12 analyzer 复算 11 trajectories / 10 labels / 1 historical accounting
  interruption，current product loss 集合为空，连续两次 report byte-identical；
- product 决定为 `close_hypothesis_no_repeated_product_loss`。production
  crate/config/protocol/schema delta=0；不新增终止 controller、预算、prompt、retry 或
  environment treatment。

完整身份、环境纠正、官方协议复核、门禁和非结论见
[M12 terminal convergence reproduction](../../eval/summaries/m12-terminal-convergence-2026-07-25.md)。
后续候选必须来自新的 accounting-complete canonical trajectory，并先满足同一 stable
current loss 至少跨两个独立 task 重复；不得继续围绕 M11 粗粒度 label 补样。

#### M13：长任务恢复损失基线

M13 已以 `close_m13_inadmissible_observer_contract_instability` 关闭，不写 recovery
treatment：

- 继续复用 `scripts/eval-m9b-fixed-pro-regression.py` 唯一 corrected Harness；
  `--campaign m13` 选择 6 个独立任务，默认 M9-C/M11/M12 contract 不变；
- 任务覆盖两项跨文件调试、一项 deterministic verifier fail-before/write/pass、两项
  typed 编辑冲突恢复和一个显式 isolated Writer；每项 3 次，共 18 arms；
- 所有 root/Writer request 显式冻结 `deepseek-v4-pro/high`，共享同一
  AgentApplication、AgentRuntime、RunStore、Standard Chat sender、工具目录和
  TaskContract/EvidenceReceipt owner；
- Host 与 external verifier 沿用 M12 的相同隔离 `HOME` 与显式 Rust 1.97.0 identity；
  每个 fixture 必须 fail-before、Git base/hash 可重建且 verifier 不污染 tree；
- `ambiguous_edit`、stale-context `workspace_precondition` 和 `verifier_failed` 是冻结任务
  协议，用来观察 current typed feedback 后的恢复；前两项必须 non-applied，
  `verifier_failed` 保留执行外部命令后的 canonical `indeterminate/unsafe`，三者均先于
  首次 effective mutation 与 final Host receipt；
- unknown billing、incomplete accounting、false success、identity/observer 歧义或成本门
  均在下一 arm 前停止，`maximum_reruns=0`，没有补 mate、续跑或拼接；
- 只有同一 stable current product loss 至少跨两个独立 task 重复，且能指定单一 owner、
  单变量 treatment、deterministic fixture 与 old-path deletion，才准许后续 vertical
  slice；否则关闭假设，不新增 controller。

2026-07-25 官方复核确认 production 继续使用
`https://api.deepseek.com/chat/completions`、`deepseek-v4-pro/high`。7 月 24 日退役的是
legacy model alias，不是 ChatCompletions。三次 position-1 acquisition 分别在 3、6、5
个 arm results 后揭示 Writer scope order、malformed stale hunk、verifier disposition
三项 evaluator-contract mismatch；每次均在下一 arm 前停止。三份 raw 不续跑、不拼接，
共 126 physical requests、known cost USD 0.274936878，均不作为 product metric。
最后一次 production terminal、external verifier、latest-revision Host receipt 和
accounting 实际闭合；false-success label 只来自 observer 把已执行外部命令的 verifier
failure 错误要求为 `not_applied/after_correction`，而 canonical outcome 正确为
`indeterminate/unsafe`。

因此不再付费迭代 M13 observer，不生成产品 trajectory candidate；stable loss 未跨两个
独立任务成立。当前 production/schema/config delta=0；完整证据见
[M13 长任务恢复损失基线](../../eval/summaries/m13-long-task-loss-baseline-2026-07-25.md)。

#### M14：typed observer conformance 与 M13 live path 退役

M14 以实现 checkpoint `7a9e2278` 收敛 evaluator contract，不重开 M13：

- 唯一 corrected Harness 新增纯离线 `--observer-conformance`；12 个冻结 case 覆盖
  semantic-set canonicalization、`patch_parse` 与 `workspace_precondition` 边界、
  verifier `indeterminate/unsafe`、Writer worktree assignment 及 exact reopen；
- 所有 label 只投影 canonical event kind、`ToolOutcome` 六个稳定 axis、
  `AgentWorkspaceAssignment` 与 byte-equivalent reopened facts，不持久化第二状态真相；
- 12/12 case 通过，含 6 positive / 6 negative；连续两次 report byte-identical，
  result SHA-256 为
  `09b840b8af0ee136d291a7ebf2203ebb05467fd24bc5adfa670dbacb093dd450`；
- `--campaign m13`、M13 successor loader、错误的 generic failure disposition 断言、
  不可达 trajectory branch 与 M13-only acquisition/self-test 路径已物理删除；
  M9-C/M11/M12 现有消费者保持通过；
- M13 manifest、fixture、summary 与 ignored raw 仍是不可变历史证据，不读取、不续跑、
  不拼接。Key、API、network、新 raw 和 production behavior delta 均为 0；
- focused、fmt、workspace strict clippy/test、root/read-only/Writer、ToolOutcome exact
  replay、SQLite reopen、process SIGKILL 与 CLI/TUI/API parity 全部通过。Run API v12、
  RuntimeEvent v18、State v24、exec-stream v3 不变。

决定为 `keep_offline_observer_conformance / retire_m13_live_acquisition`。这只恢复未来
采集的观察可信性，不产生成功率、恢复率或成本结论。新的 product-effect acquisition
必须另立冻结 manifest，从新 identity/position 1 开始；observer、accounting 或 evidence
任一歧义仍在下一 arm 前 fail closed。

完整事实见
[M14 observer conformance](../../eval/summaries/m14-observer-conformance-2026-07-25.md)。

#### M15：fixed-Pro current product-loss acquisition

M15 是新的 position-1 current loss 采集，不是 product treatment。它继续复用
`scripts/eval-m9b-fixed-pro-regression.py` 唯一 corrected Harness；`--campaign m15`
只选择新的 manifest、fixture、schedule、journal schema 与后续只读 analysis input。
M9-C/M11/M12 与 M14 observer 入口保持原合同，M13 raw 不读取、不续跑、不拼接。

冻结输入为 8 个独立临时 Git task × 3 次，共 24 arms：

- Rust scoped rules/wire contract；
- TypeScript stack-trace localization 与同名 decoy；
- Python 多文件单向配置迁移；
- Rust verifier fail-before / split-stream recovery；
- Python JSONL 真实子进程协议；
- 一个 read-only child 的 service graph handoff；
- 一个显式 isolated Writer 的 envelope migration；
- 一个 no-tool authorization 假完成反例。

每个 root/child RunRequest 显式冻结 `deepseek-v4-pro/high`；所有 arm 使用同一 immutable
binary、official DeepSeek OpenAI-format `POST /chat/completions`、同一 TaskContract、
tool catalog、预算与 external verifier，`maximum_reruns=0`。Host 和 external verifier
共享同一 per-arm isolated `HOME` 与 Rust 1.97.0 rustup identity。每个原始 fixture
verifier 必须确定性失败且前后 tree byte-stable；7 个正向 verifier 还须在仓库外参考修复
副本中独立通过，安全反例继续失败。

证据顺序固定为 terminal → canonical Store → credential-free SQLite reopen →
external verifier → label。journal 必须 ignored、0600、exclusive、fsynced、
hash-chained。observer、latest-revision evidence、unknown billing、incomplete usage、
identity、安全或成本歧义都在下一 arm 前停止；不重跑、不补 mate、不 resume、不重采样。

只有同一 stable current product loss 跨至少两个独立 task ID 重复，才允许审计一个现有
owner 与单变量可删除 treatment；粗粒度 failure label 只授权 owner audit，不自动授权实现。

正式 candidate `8f887fe2`、admission `0c589f2b` 从 position 1 执行到 16/24 后按
`false_success_observed` 停止，没有继续或补跑。16 arms 全部 accounting-complete、
billing-known、State v24 reopen 一致；123 requests、793,780 input、78,978 output、
known USD 0.219194238。frozen label 为 11 verified / 2 correct safety rejection / 1
false-success。

read-only canonical audit 证明该 false-success 是 evaluator scope mismatch：
`typescript_stacktrace` external verifier、Host receipt、route 与 lane 全部通过，模型用
两文件等价实现满足语义，但 evaluator 把参考实现的三文件 exact changed set 当成完成条件。
raw 保持不可变；corrected report 把它记为非产品损失。其余只有单 task
`deterministic_verifier_failed` 与另一个单 task `writer_delegation_failed`，没有跨 task
同 owner 重复。M15 决定为
`inadmissible_evaluator_scope_mismatch / insufficient_repeated_current_loss /
keep_production_unchanged`；A–E treatment 均不准入，不恢复 M10 分支，也不重开 M15。
完整证据见
[M15 current product-loss acquisition](../../eval/summaries/m15-current-product-loss-acquisition-2026-07-25.md)。

### 调优

- release benchmark 持续验证 qualified real coding evidence、current exact-production
  regression 与 common workflow action contract；不得重开缺失 legacy accounting 的
  imported paid A/B；
- Auto 已按 ADR-0008 退休并删除，不再做 successor、A/B、默认准入或未来调优；
- 只有满足 M8-H re-entry gate 才重开 `apply_patch/search-replace/FIM` A/B；
- thinking、上下文预算和压缩策略；
- stable prefix/cache；
- 并行只读工具（M7-G product metric 不准入，explicit-only）；
- Agent 数量和预算；
- 新的中文 Agent prompt 组合属于 post-V1 优化；只有出现 material model-visible
  treatment 时，才以 current same-revision immutable control/treatment 做同任务 A/B，
  候选按版本评测并通过 whole-release rollback；
- 首个合并候选 `b088fd13` 及后续 v2/v3 收敛 canary 均因 multi 可靠性或计量门槛被拒绝；
  v3 已证明 fixed checklist 影响 root 收敛，也证明 child 最终结果轮不能靠提示词保证；
  后续先改 Runtime 机制，不恢复已经删除的模式、人格、Provider 或兼容提示层；
- 只有基准证明需要时才加入 embedding。

### 剩余清理

- 遗留 historical evidence 和最终不再需要的导入资料；
- 无接线 stub、旧语义适配层和新旧双路径；
- fixed `zh-Hans` 已收敛为一个共享 owner；后续只删除失去真实 caller 的 message id，
  不恢复 TUI 私有 catalog 或 locale 状态。

Telegram、Feishu、bridge-core、remote-setup 调用面和 Tencent Lighthouse 部署链已在 M4-B
因 app-server 旧控制面删除而同步物理删除，不再列为 M7 待办。

顶层 `codewhale update`、CLI/TUI 自更新和版本检查、`crates/release`、CNB/imported
GitHub release discovery 及其专用依赖已删除；本地升级和回滚只由
`scripts/codewhale-delivery.sh` 消费显式 artifact，不访问 release metadata。

清理必须先通过依赖盘点；Cargo 核心能力不得因外围删除而退化。Provider 专用的人类界面
随对应旧路径一起删除，不投入翻译；每个切片只汉化已经确认保留的 DeepSeek 配置、Agent
运行和多 Agent 链路。

## 12. M8：V1 产品化

- 正式产品名、二进制名、配置目录和 User-Agent；
- 自己的 origin/upstream 远程策略；
- DeepSeek-only 配置向导；
- 固定 `zh-Hans` 的 CLI/TUI/Headless 文本界面与中文帮助、Doctor、错误恢复和多 Agent 状态；
- 保持 NDJSON/API 字段、命令参数、工具名、模型 ID、路径、代码和原始输出稳定；
- 固定中文 Agent prompt 具有 immutable 源码身份、合格 live evidence、exact-current
  retention 和 whole-release rollback；未来语义候选仍须同任务 A/B 后才能接管；
- 本地开发、安装、卸载和数据迁移；
- 精确 Rust toolchain；
- 自己的 CI、版本、changelog 和发布流程；
- 架构依赖门禁和长期 benchmark。

M8 退出前必须通过第 2.1 节的中文端到端、机器协议稳定性、CJK 终端布局、英文泄漏和
提示词 A/B 门禁；只增加翻译字符串但保留英文主流程，不计为完成。

## 13. M17：DSE 双语开源身份硬切换

### 13.1 Goal

把 M8-N 的未发布 CodeWhale/fixed-Chinese release-ready checkpoint 收敛为可公开发布的
DSE V1：

```text
DSE-only current product identity
  + English-first public repository
  + complete en / zh-Hans human interface
  + user-language Agent responses
  + one evidence-selected production prompt
  + protected, audited, reproducible public release
```

M17 是用户接受的新产品需求，不是继续无边界优化 Agent。它不得恢复 Auto、Provider、
Anthropic Messages、FIM、multi-Writer、第二 Runtime/Store、翻译模型或旧 locale 生态。
当前中文 release-ready revision 是整个 M17 的 rollback baseline，M17 未完成前不得公开、
推送 release 或把部分双语界面宣称为正式完成。

实施由 [ADR-0009](../decisions/0009-dse-product-identity.md) 与
[ADR-0010](../decisions/0010-bilingual-product-and-prompt-admission.md) 约束。

### 13.2 固定产品身份

| 范围 | 唯一目标 |
|---|---|
| 产品 | `DSE` / `DeepSeek Engineer` |
| 主命令 | `dse` |
| TUI | `dse-tui` |
| Cargo/import | `dse-*` / `dse_*` |
| 用户目录 | `~/.dse` |
| 产品环境变量 | `DSE_*` |
| release artifact | `dse-{version}-{target}-...` |
| active protocol/eval | `dse.*` |
| vendor media type | `application/vnd.dse.*` |
| UI locale | `en`, `zh-Hans` |
| production prompt | A/B 后只保留一个 |

`DEEPSEEK_API_KEY` 等官方 DeepSeek 命名保持不变。CodeWhale 只允许存在于 MIT 来源说明、
导入基线、Git 历史和不可改写 frozen evidence allowlist。新活动代码、协议、运行和
release 不得继续生成旧身份。

### 13.3 执行前保护

1. 记录 clean/dirty、branch、HEAD、remote 和当前 release identity；
2. 保留当前中文 prompt SHA、Run API v12、RuntimeEvent v18、State v24 与 exec-stream v3
   rollback 事实；
3. 盘点所有 `CodeWhale/codewhale/codew/CODEWHALE_/.codewhale` 生产消费者并按 owner
   分类，不能用 broad replace 代替调用链审计；
4. 保留所有用户和其他 Agent 的未提交修改。M17 计划落库时已观察到
   `scripts/eval-m9b-fixed-pro-regression.py` 有现存修改，不得 reset、restore、覆盖或
   顺手纳入 branding commit；
5. branding、localization、prompt experiment、public docs/release 分开提交；不得在一个
   diff 同时改变身份、Runtime 语义和模型能力。

### 13.4 垂直切片

#### M17-A：DSE 产品身份契约

- **真实问题**：当前活动身份仍由 CodeWhale binary/package/path/protocol/delivery/prompt
  共同拥有，只改展示名会留下双身份；
- **唯一 owner**：`crates/cli` 的产品入口与 workspace manifest，其他模块只迁移消费方；
- **实现**：产品显示为 DSE；binary 为 `dse`、`dse-tui`；Cargo package/import 使用
  `dse-*`/`dse_*`；help/version/User-Agent 与 model-visible identity 使用 DSE；
- **旧路径**：`codewhale`、`codewhale-tui`、`codew` 和 active `codewhale-*`；
- **验收**：focused build、all-target Cargo metadata/tree、CLI/TUI version/help、
  root/read-only/Writer prompt provenance 与 current conformance；
- **cutover 删除**：旧 binary target、alias、active package/import、旧产品 title 和
  prompt identity；不改 frozen evidence。

Prompt 的 `CodeWhale -> DSE` 只改身份，不重写执行、验证、工具或多 Agent 条款。通过
conformance 后冻结为 M17-F 两个语言 variant 的共同品牌基线。

M17-A 已在 `89f1bb9f` 完成：16 个活动 package/import 全部切为 `dse-*`/`dse_*`，
binary target 精确为 `dse`、`dse-tui`，CLI/TUI help/version、official DeepSeek
User-Agent 和 production constitution 使用 DSE。Run API v12、RuntimeEvent v18、
State v24 与 exec-stream v3 未改变；`.codewhale`、`CODEWHALE_*` 和 active machine
namespace 明确保留给 M17-B，不在本切片制造隐式协议迁移。完整证据见
[M17-A DSE product identity](../../eval/summaries/m17-a-dse-product-identity-2026-07-25.md)。

#### M17-B：DSE config/state/protocol identity

- **真实问题**：旧 home/env/schema/media type 会让新 DSE 继续依赖上游产品身份；
- **唯一 owner**：config/state/protocol 各自现有 canonical owner，不增加 migration
  manager 或第二 Store；
- **实现**：`~/.dse`、`DSE_HOME`/`DSE_CONFIG_PATH`、active `dse.*` schema 和
  `application/vnd.dse.*`；受影响协议显式升版；
- **旧路径**：`.codewhale`、`CODEWHALE_*`、active `codewhale.*` namespace；
- **验收**：isolated HOME、config/Secret、Start/Run/reopen/resume、canonical JSON/
  NDJSON/HTTP/SSE、schema migration 和 hash/replay；
- **cutover 删除**：旧 path/env reader、schema 双写与 compatibility branch。

由于旧身份尚未公开发布，一次性迁移只跨本切片：复制并校验本地 config、Secret 与可保留
状态，保留原目录备份；DSE release candidate 前删除迁移器和旧 reader，不向公共 V1
发布永久兼容层。不能无损迁移的旧 materialized state 必须先形成只读备份和明确 disposition，
不得伪造 hash-chain 或静默丢失。

#### M17-C：DSE delivery 与 CI

- **真实问题**：M8-B release owner 仍绑定 `codewhale` binary、路径和 artifact；
- **唯一 owner**：现有 delivery script；
- **实现**：`scripts/dse-delivery.sh`、`scripts/dev-dse.sh`、DSE artifact/checksum/
  manifest、`lib/dse`、CI artifact `dse-*`；
- **旧路径**：旧 delivery/dev scripts、`lib/codewhale`、旧 program links 和 artifact；
- **验收**：Rust 1.97.0 locked/offline package/install/verify/upgrade/rollback/uninstall，
  source/Cargo.lock/toolchain/inner+outer checksum identity，macOS real lifecycle 与 Linux
  fixture lifecycle；
- **cutover 删除**：旧 install link、artifact allowlist、CI command、temporary migration
  fixture 和旧 release reader。

最终 shipped binary set 严格为 `dse`、`dse-tui`；release package 中出现第三个可执行文件
或旧名称即失败。

#### M17-D：双语 localization owner

- **真实问题**：sole `zh-Hans` 与 hard-coded locale 阻止英文用户完整使用公开产品；
- **唯一 owner**：`crates/localization`；
- **实现**：最小 `ProductLanguage { English, SimplifiedChinese }`，exact-key/
  placeholder-compatible `en.json` 与 `zh-Hans.json`，解析顺序固定为：

  ```text
  --language
    -> ui.language
    -> first-run bilingual choice
    -> noninteractive new environment defaults to en
  ```

- **旧路径**：hard-coded `locale = "zh-Hans"`、sole-catalog assertions、中文泄漏白名单；
- **验收**：catalog key/placeholder parity、unknown locale rejection、旧本地 DSE 迁移保持
  `zh-Hans`、restart/resume 稳定、无额外模型请求；
- **cutover 删除**：旧 sole-catalog test、散落 locale 分支和无消费者 message id。

只支持 `en` 与 `zh-Hans`。不读取语言后调用模型，不恢复历史语言包，不增加语言插件、
在线翻译、`/translate` 或 per-Run/per-Agent locale。

#### M17-E：CLI/TUI/app-server 双语投影

- **真实问题**：catalog 存在不等于真实入口可用，硬编码中文、CJK-only layout 或
  localized machine output 都会形成假双语；
- **唯一 owner**：各 thin client 的 canonical human projection；业务事实仍来自 app/
  Runtime/Store；
- **实现**：CLI/TUI/onboarding/Doctor/help/approval/cancel/error/recovery/context/
  multi-Agent/Writer/release 人类文本消费同一解析 locale；
- **旧路径**：客户端内嵌中文、私有 fallback、机器字段本地化；
- **验收**：两种语言 golden/PTY、English narrow layout、CJK 80/120-column width/wrap/
  hit target、root/read-only/Writer/resume/recovery、app-server/exec/TUI 一致性；
- **cutover 删除**：client-local catalog、重复 formatter 和 locale-specific业务分支。

locale 切换前后的 command/flag/tool/model/path/code/diff/stdout/stderr、stable error code、
canonical JSON/NDJSON/HTTP/SSE、route/model/reasoning/catalog/budget/request count 与
RunStore facts必须相同。

#### M17-F：DSE 中英文 prompt 2×2 A/B

- **真实问题**：旧 English-long vs Chinese-rewrite 实验混合了语言、内容、结构和长度，
  不能决定国际开源 DSE 应保留哪一种内部 prompt；
- **唯一 owner**：`crates/context` production prompt 与现有 corrected Harness；
- **baseline**：逐条使用 DSE 身份的 current Chinese prompt；
- **candidate**：条款、顺序、强度、权限、完成/验证和多 Agent 规则等价的 English prompt；
- **共同固定**：tool catalog/schema、Runtime、Store、model/reasoning、预算、fixture 初态、
  external verifier、非语言 prompt blocks、schedule 与 accounting；
- **cutover 删除**：失败 variant、eval-only selector/assets、temporary config/test surface
  和双 production branch。

两个 variant 都要求：除非用户显式指定，Agent 使用用户当前任务语言回答；代码、命令、
协议和技术标识保持原样。该规则不增加 Host classifier 或额外模型请求。

正式 block 1：

```text
8 independent task families
  x 2 task languages (English, Simplified Chinese)
  x 2 system prompt languages (English, Simplified Chinese)
  = 32 runs
```

任务覆盖单文件、跨文件、搜索定位、调试、安全拒绝、verifier recovery、read-only child
和 explicit Writer。每个任务的中英文 TaskContract 语义等价；external verifier 验证
行为和约束，不要求参考实现的 exact changed-file set。

只有 block 1 全部 measurement-valid、没有质量否决且预注册分析仍需更多置信度时，才执行
完整 block 2；总量最多 64 runs。不得选择性 rerun、补 mate、拼接旧 prompt raw 或修改
任务/预算。credential 前冻结并通过 fixture、prompt/binary/catalog hash、schedule、
journal/reopen、observer、费用 ceiling 和 no-Key preflight。

硬门顺序：

```text
verified success
  > false success
  > correct safety rejection
  > request-budget exhaustion
  > requests
  > tokens
  > wall time
  > cost
```

任一额外 false success、质量回退、unknown billing、incomplete usage、identity drift、
observer/evaluator ambiguity 或费用硬门都在下一 arm 前停止。成本改善不能补偿任务成功、
安全或 false-success 回退。

最终只允许四种结论：

1. English 在两个任务语言层都不回归且相同或更好：保留单一 English prompt；
2. Chinese 在两个任务语言层都相同或更好：保留单一 Chinese prompt；
3. 两者只在同语言层占优：不增加 Auto；最多再预注册一个 single compact bilingual
   candidate；
4. 无效或证据不足：保留 current Chinese DSE prompt，不声明语言优劣。

winner 通过 exact-current gates 后才接管；whole-release rollback 是唯一 prompt rollback
owner，不增加 prompt store、selector、mode 或 compatibility branch。

#### M17-G：英文优先的公开仓库

- **真实问题**：当前 README 陈旧且中文单入口，不能准确表达 DSE current architecture、
  安装方式和贡献边界；
- **唯一 owner**：根公共文档与 GitHub governance；
- **实现**：

  ```text
  README.md             English canonical entry
  README.zh-CN.md       complete Chinese entry
  CONTRIBUTING.md       contribution/review contract
  SECURITY.md           vulnerability reporting
  CODE_OF_CONDUCT.md    community behavior
  CODEOWNERS            owner-reviewed integration
  issue/PR templates    English-first with Chinese link
  ```

- **旧路径**：过期 Run API/Event/State、已删除 Auto、旧 M8/FIM next-step 和 CodeWhale
  current-product claims；
- **验收**：中英文安装/配置/命令一致，链接与代码块可执行，license/provenance、DeepSeek-only、
  fixed routing、single Runtime/Store、explicit Writer 和 release identity准确；
- **cutover 删除**：stale README claims、平行 roadmap/handoff/version tracker。

Roadmap、Evaluation 和架构事实仍只有一套权威文档；不复制整套双语 Roadmap。历史中文
评测无需翻译，公共入口提供准确英文导航。

#### M17-H：公开发布

1. 全 workspace fmt、clippy `-D warnings`、test、focused、crash/reopen 和 release
   lifecycle 通过；
2. active-source identity allowlist 审计证明旧 CodeWhale 名称只剩 provenance/frozen
   history；
3. secret、ignored raw、生成文件、license、上游归属和 remote 审计通过；
4. 先推送 DSE candidate 到私有远端并等待全部 CI；
5. 配置 default branch protection、required checks、CODEOWNERS 与 PR review；
6. 确认 GitHub slug 后改名；优先 `dse`，不能使用时必须由用户选择唯一备用名；
7. 用户显式确认后再把仓库设为 public、创建 tag/release；
8. release 后从 fresh environment 复验英文/中文 install、first run、coding、resume、
   rollback 和 uninstall。

推送、远端改名、public visibility 和 release 是外部变更，不能由“执行 M17”隐含授权；
每项必须在本地与私有远端门禁通过后按用户明确授权执行。

### 13.5 M17 退出门槛

- 当前活动产品身份只使用 DSE；
- binary set 严格为 `dse`、`dse-tui`；
- package/import、path/env、protocol/eval/media type、delivery/CI 与 model identity 已切换；
- `en`/`zh-Hans` catalog exact parity，所有保留人类入口真实双语；
- 一个进程一个 locale，不存在模型语言分类、翻译请求或 per-Agent locale；
- 中文任务默认中文回答、英文任务默认英文回答，显式语言要求被遵守；
- prompt 2×2 campaign 得到有效结论或按 fail-closed 规则保持中文基线；
- production 只剩一个 prompt，失败候选和 selector 已删除；
- locked/offline DSE release lifecycle 与全量门禁通过；
- English README、中文入口、治理和来源说明准确；
- private remote CI、branch protection 和最终 public release 各自有确认事实；
- 没有临时 adapter、旧 binary alias、双写或无删除点历史债。

本地 workspace 目录从 `.../codewhale` 改为 `.../dse` 是最后的宿主操作；必须等所有共享
目录任务完成并确认没有运行中的 shell/worktree 后执行，不能在 M17-A 的源码 rename 中
移动当前工作区。

## 14. 当前源码迁移表

| 当前实现 | 目标归属 | 替代后删除 |
|---|---|---|
| `client.rs`、`client/chat.rs` | `deepseek` | 通用 Provider/DeepSeek 混合 client |
| 退役 `tui/compaction`、`seam_manager`（已删除） | `context` | canonical hard-limit 实现位于 `context + runtime` |
| `project_context`、`working_set` 遗留半区 | `context` | 浅层 project map 和重复投影 |
| `tui/src/tools/*` | `tools` | TUI 工具业务逻辑 |
| legacy thread tables、Fleet ledger（已删除） | `state` | canonical `RunStore` 是唯一持久真相 |
| Fleet、Lane（M8-G 已删除） | `runtime + orchestrator` | 只保留 canonical child lifecycle 与 explicit Writer Git side effects |
| 交互 foreground/child projection（M4-C C3 已迁移） | `app + runtime + tui` | 旧 Engine、runtime-thread、SessionManager、child cache 已删除 |
| `app-server` canonical projection（M4-B 已迁移） | `app + app-server` | TUI 子进程桥已删除 |
| `crates/core` 脚手架（M4-B 已删除） | `app + runtime` | fake `handle_prompt` 已删除 |

## 15. 调整机制

里程碑结束时只允许三种结论：

- **保留**：真实评测有净收益，复杂度合理；
- **重做/缩小**：方向有价值，实现或默认策略不合理；
- **删除/推迟**：没有收益、明显负优化或不属于产品范围。

改变固定架构边界必须新增 ADR，说明问题、证据、替代方案、迁移和删除影响。
不能用临时开发困难作为恢复多 Provider、多 Runtime 或多状态真相的理由。
