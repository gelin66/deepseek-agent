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
- 上次更新：2026-07-28

<a id="current-execution-window"></a>
## 0. 当前执行窗口

- M44 DeepSeek 原生 Web Search 决策已形成 clean checkpoint `801391577`，结论为
  `hold_wait_for_chat_surface`。
- ADR-0016 首个实现 Goal 已完成：有界 development authority、owner-scoped retrieval 与
  risk-tier gate 已切换；production Rust delta=0、official requests=0。
- ADR-0016 排序后的 continuation/Harness 离线复核已完成：可准入的重复 continuation loss
  为 0，`VerifiedMilestoneProjection` 不实施；same-DeepSeek structural baseline 已冻结，
  production Rust delta=0、official requests=0。
- ADR-0017 W1.1 已完成并 keep：canonical `web_fetch` 已从 HTTPS-only 一次迁移为 public
  HTTP(S) + monotonic transport provenance，clean checkpoint 为 `5be131a1c`。
- M45-A ApplicationProbe 已在 clean checkpoint `997c67e20eb6` 完成：一次性、worktree-local 的
  process start -> loopback health/HTTP assertion -> bounded logs -> latest-revision receipt ->
  teardown/reopen 已接入 canonical Host verifier 主链。
- M46 admission audit 与只读 W2 production 已完成：两个独立 JS-only local task 的同一
  `tools:application_visibility` loss 已由一个 Rust-native、one-shot、Host-owned
  `browser_navigate + bounded DOM/AX snapshot + teardown` 闭合；action/search/视觉仍未启动。
- M46 W3 已完成并 keep：`browser_navigate` 为 Host-owned exact-loopback ephemeral page 返回
  same-run/latest-epoch opaque refs，唯一新增 `browser_click(element_ref)`；两个冻结 fixture 的
  post-click state=`2/2`、负向 false allow=`0`，committed click reopen 不重放。production
  protocol/state schema delta=0、official DeepSeek requests=0。
- M46 post-W3 interaction admission 已完成：两个独立 exact-loopback text-entry task 产生同一
  `tools:browser_interaction:fill=2/2`，production control=`0/2`、false-success=`0`，Host teardown
  与 AgentApplication reopen regression 均通过；只准入后续单一 `browser_fill` focused Goal，
  该历史 checkpoint 的 production delta=0，press/wait/public action/视觉/搜索当时未启动；其
  one-action 后续规则现由 ADR-0018 取代。
- M46 W3.1 已完成并 keep：同一个 Host-owned direct-CDP lifecycle 现只为 eligible、非敏感、可编辑的
  `input[type=text|search]` 返回 fill-only opaque ref，并新增唯一
  `browser_fill(element_ref, value)`；两个冻结 fixture 的 fresh post-fill state=`2/2`、stale reuse
  与 mandatory negative matrix false allow=`0`。真实 AgentApplication committed fill reopen 不重放，
  started-without-outcome 进入既有 `RecoveryRequired`；protocol/state delta=0、official requests=0。
- ADR-0018 工程完全体方向审计与首个 production cluster 已完成：W1～W3.1 安全机制继续保留，
  `browser_click`/`browser_fill` 已由一个 `browser_interact` 替换；同一 direct-CDP owner 现闭合完整
  semantic interaction、最多三页状态、public reversible draft POST、exact approval/receipt 和
  caller/reopen/recovery。catalog 从 16 收敛为 15；protocol/state/DeepSeek Prompt delta=0，official
  DeepSeek requests=0。search/observation quality/visual 仍是后续 capability gap。
- ADR-0018 第二个 production cluster 已完成：同一 Rust direct-CDP owner 现在提供 project-isolated managed
  profile、Host-owned credential login、bounded Cookie/storage lifecycle、workspace-authorized upload、
  isolated/scanned download 与 explicit no-overwrite promotion；真实 pinned Chrome 和 production app/reopen/
  recovery 均闭合，protocol/state/Prompt delta=0、official DeepSeek requests=0。
- ADR-0019 Canonical Web Search + Semantic Observation Quality 已完成并 keep：production cutover 已把
  fixed catalog 从 15 增至 16，root Agent 可经唯一 Host-owned `web_search` 发现来源，再用 `web_fetch`
  读取两个独立原来源并给出 URL citation；semantic browser 以 task-cue/role/interaction priority 取代机械
  first-N，返回 quality metrics 与 action diff。真实 caller/SQLite reopen/recovery 和 pinned Chrome vertical
  已闭合；focused 与唯一一次 full gate 均 exit=`0`，形成 clean reviewable checkpoint。
- Canonical Web Search 后的 Internal Alpha Integration Checkpoint 已完成 deterministic keep：三个独立
  Python application shape 都经同一 root `web_search -> 2x web_fetch -> isolated Writer -> compile/start ->
  Writer receipt -> seal/integrate -> root latest-revision receipt -> cleanup -> SQLite reopen` 生产链闭合，
  verified=`3/3`、false success=`0`、reopen reexecution=`0`。切片同时删除 isolated Writer 对 Host-owned
  loopback verifier 的错误 blanket deny，只开放 sandbox 内 `localhost:*` bind/inbound，external outbound
  仍为 0。首个 official DeepSeek dogfood 以 4 requests、0 retry、13,141/1,145 tokens、约 `$0.00373`
  到达 `Failed(OutputLimit)`；显式授权的 fresh 2,048-token successor 以 6 requests、0 retry、
  21,015/2,212 tokens、约 `$0.00654` 到达 `Failed(ToolBudgetExceeded { limit: 6 })`。两个 treatment 都没有
  重跑。再次授权的 2,048-token / 16-tool successor 以 8 requests、0 retry、26,349/4,473 tokens、约
  `$0.00933` 到达 `Blocked(Host application_probe deterministic failure)`；它越过两个旧上限，但耗尽
  recovery request budget。12-request successor 仍 Blocked；新增诊断证明 root 已两次启动 Writer，但
  root marker 仍缺失。审计定位原 deterministic fixture 让只有 `apply_patch`、不能读取 `server.py` 的
  Writer 凭空提交预制文件。当前 fixture/task 已切到 Writer `read_file -> apply_patch` 的真实链并保持
  3/3 verified。修正后的 official treatment 以 10 requests、0 retry、46,281/2,981 tokens、约 `$0.01262`
  到达 Host-accepted latest-revision `Completed`；测试进程随后只因 brittle exact-tool-list observer 把两次
  合法 root `read_file` 误判而 exit 101。observer 已离线改为核心顺序 + bounded reads 并由正反例闭合，
  treatment 未重跑。该结果只证明 bounded vertical usability，不声明通用生产力提升或 live Tavily success。
- M40-A 继续保持 `reject_incomplete_acquisition`，不能续跑或补样；没有并行启动后续项。

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
| M17 | DSE 双语开源身份硬切换 | 本地 V1 已完成（M17-A–H）；GitHub 发布显式延期 | DSE 唯一身份、`en`/`zh-Hans` 完整产品面、单一 prompt 胜者与本地 locked/offline 发布门禁闭环 |

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
  `exec_policy`：前者控制 canonical `agent` 目录与 depth/concurrency，后者在当时控制
  Shell policy snapshot 加载；Shell、patch 与 MCP CLI 仍由各自真实 owner 控制。M27
  cutover 后 `exec_policy` 已成为所有 production caller 必须一致消费的 canonical matcher，
  因此该 feature toggle 也随平行 snapshot 一起删除；`[features]` 当前只剩 `subagents`。
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

M17-B 已在 `2b6dd276d` 完成硬切换：活动路径/env 只认 `~/.dse`、`DSE_HOME`、
`DSE_CONFIG_PATH` 和 `DSE_*`，prompt wrapper、Secret service、verification media type、
runtime handoff 与 exec stream 只发出 DSE identity。RuntimeEvent 升到 v19、State 升到
v25、exec-stream 升到 v4；Run API 保持 v12，因为 command envelope 没有变化。State v25
在同一事务中保留 replay-safe pending Start，删除无法在不改写 exact transcript 的旧
materialized v18 runs，不增加 compatibility reader 或 dual write。当前开发机只把六项
可无损本地事实 exact-copy 到 `~/.dse` 并逐项 `cmp`：config、settings、setup state、
permissions、onboarded marker 和 file Secret；原 `~/.codewhale` 完整保留为备份，历史
sessions、tool outputs 与日志未冒充 canonical DSE state。完整证据见
[M17-B DSE config/state/protocol identity](../../eval/summaries/m17-b-dse-config-protocol-identity-2026-07-25.md)。

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

M17-C 已在 `fd23400ca` 完成：唯一 owner 为 `scripts/dse-delivery.sh`，manifest schema
为 `dse.delivery.v1`，artifact/binary/install root 精确为 `dse-*`、`dse`/`dse-tui`
和 `lib/dse`；`scripts/dev-dse.sh`、`scripts/test-dse-delivery.sh`、TUI hermetic
runner、README/CONTRIBUTING/AGENTS 当前调用方和 GitHub Actions matrix 已全部迁移。
旧 delivery/dev/test 文件名、旧 install link/root 生产能力与无消费者 M8-L release
reader 已物理删除；frozen M8-L manifest/summary/result 仍作为历史事实保留并可从其被测
revision 复算。macOS 同 revision locked/offline source package 的真实 install/verify/
uninstall 通过，Linux arm64 以已缓存 Bookworm image、只读源码挂载和
`--network none --pull never` 通过同一 upgrade/rollback fixture。私有远端 CI 的实际
运行仍由 M17-H 发布门执行，不能由 workflow YAML 代替。完整证据见
[M17-C DSE delivery and CI](../../eval/summaries/m17-c-dse-delivery-ci-2026-07-25.md)。

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

M17-D 已在 `68f3aa739` 完成：`crates/localization` 现在是唯一语言 owner，两个 catalog
各有 417 个完全相同的 key 和 named-placeholder multiset；CLI/TUI 只接受精确 `en`、
`zh-Hans`，并按 explicit、persisted、旧本地迁移、first-run/fresh 环境的固定规则冻结
process language。fresh noninteractive 为 English，旧 `.onboarded` DSE 且没有语言配置
的安装保持 `zh-Hans`；fresh interactive TUI 的双语选择通过 canonical ConfigStore 写入
`[ui].language`。真实进程、restart、PTY、focused、strict Clippy、workspace test、
SIGKILL/reopen 和固定 actor route 门全部通过；没有 Key、官方 API、额外模型请求、协议
或 Store 变化。M17-E 仍必须迁移剩余 hard-coded human projection，不能把本切片误报为
全产品双语完成。完整证据见
[M17-D DSE bilingual localization owner](../../eval/summaries/m17-d-dse-bilingual-localization-owner-2026-07-25.md)。

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

M17-E 已在 `6464fe155` 完成：`en.json` 与 `zh-Hans.json` 各有 768 个 exact-matching
key 和 named-placeholder multiset；CLI/TUI 的 help、config、setup、Doctor、MCP/OAuth、
approval、error、recovery、root/read-only child/Writer 与 work-surface 人类投影全部迁移
到同一 catalog owner。英文 80-column multiline、中文 CJK 80/120-column、双语真实
approval PTY 和 canonical Run parity 通过；切换 locale 不改变 JSON/NDJSON/HTTP/SSE、
route、request、accounting、协议或 RunStore。raw stdout/stderr/tool/provider facts、
stable code、首次语言选择器、模型用 prompt/template 与历史 frozen evidence 保持原样。
focused、fmt、strict Clippy、全 workspace test 和 crash/reopen 均通过；没有 Key、官方
API、网络或发布动作。完整证据见
[M17-E DSE bilingual human projection](../../eval/summaries/m17-e-dse-bilingual-human-projection-2026-07-25.md)。

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

M17-F 已在 formal candidate `73d02d05e` 和 cutover `c1856fa4b` 完成。冻结的 block 1
执行 32/32 measurement-valid arms：26 个正向 verified、4 个正确安全拒绝、
false success 0，263 个 physical requests、1,914,399 input tokens、159,125 output
tokens、USD 0.511237723 known cost 与完整 accounting。English/Chinese prompt 在英文
任务分别为 6/7、6/7，在中文任务均为 7/7；但 English prompt 在
`rust_scoped_rules:en` 出现一个 treatment-only verified-success loss，因此
`english_noninferior=false`，Harness 按预注册门禁写入
`retain_chinese_block1_quality_veto`，没有运行 block 2、补 mate 或 rerun。

production 只保留 normalized SHA-256
`a91799031d8f430945e98871f19d3cefd0496834304b4af0ff04197944ab1bdb`
的中文表达 prompt，并把回答语言固定为用户当前任务语言（显式要求优先）。临时 selector、
English candidate、翻译 scaffolding、六个 eval assets 与 M17-F-only runner 已物理删除；
frozen contract/admission、ignored `0600` raw、summary 与 Git 历史保留为审计证据。完整
结论见
[M17-F DSE bilingual prompt 2x2](../../eval/summaries/m17-f-bilingual-prompt-ab-2026-07-25.md)。

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

M17-G 已在 public candidate `85e241223` 完成。`README.md` 现在是英文 canonical
entry，`README.zh-CN.md` 是完整中文入口；贡献、安全、行为准则、来源声明、CODEOWNERS
与 issue/PR templates 已建立。public-repository checker 在本机 focused 与 GitHub Actions
共用，检查 current facts、链接、bash block、governance、秘密形状，以及 M7-A/M7-A2
`DeepSeek Agent` 历史标题不被当前品牌机械改写。无消费者且陈旧的 KEYBINDINGS/PROVIDERS
参考页已删除，Sandbox 参考也只声明真实 enforcing backend。

公开审计前置纠错分别为 `389aac896`（`cw:ctx -> dse:ctx`、DSE header、删除 whale
状态选项）与 `de6bc7004`（嵌套 auth/model help 进入唯一 localization owner）。current
locale catalog 各有 776 个 key；完整 assembled prompt fixture 因 identity-only marker
变化为 SHA-256
`d7746692db36eea33da0305553499b708a4d8b2b7d9688633aba1673142d49c9`。
M17-F 被测 hash `a9179903...`、中文 prompt winner 与 frozen evidence 保持原样，不能
把 current marker 修正回写为正式 A/B 输入。focused、strict Clippy、workspace test、
真实 binary smoke、秘密/许可/来源与历史 identity allowlist 均通过；没有 Key、API、
push、tag、release 或 visibility 变化。完整证据见
[M17-G DSE bilingual public repository](../../eval/summaries/m17-g-dse-public-repository-2026-07-25.md)。

#### M17-H：本地发布就绪与外部发布边界

1. 全 workspace fmt、clippy `-D warnings`、test、focused、crash/reopen 和 release
   lifecycle 通过；
2. active-source identity allowlist 审计证明旧 CodeWhale 名称只剩 provenance/frozen
   history；
3. secret、ignored raw、生成文件、license、上游归属和 remote 审计通过；
4. 外部发布被授权时，先推送 DSE candidate 到私有远端并等待全部 CI；
5. 配置 default branch protection、required checks、CODEOWNERS 与 PR review；
6. 确认 GitHub slug 后改名；优先 `dse`，不能使用时必须由用户选择唯一备用名；
7. 用户显式确认后再把仓库设为 public、创建 tag/release；
8. release 后从 fresh environment 复验英文/中文 install、first run、coding、resume、
   rollback 和 uninstall。

推送、远端改名、public visibility 和 release 是外部变更，不能由“执行 M17”隐含授权；
每项必须在本地与私有远端门禁通过后按用户明确授权执行。

M17-H 本地 release-readiness 已在 `a8c4bafab` 闭合。活动 TUI palette、主题输入和空状态
视觉已从 whale 收敛为 DSE；无消费者 `.codewhale/constitution.json` 被删除，但没有迁移
为会改变 M17-F prompt 的新 `.dse` authority block。public checker 现在只允许 crates
中的旧输入拒绝、State 迁移和 frozen fixture 事实，并继续冻结 M7-A/M7-A2
`DeepSeek Agent` 历史标题。

同 revision focused、strict Clippy、workspace test、macOS Rust 1.97.0 locked/offline
source package/install/verify/uninstall、Linux arm64 `--pull never --network none` 完整
fixture lifecycle、秘密/许可/来源与 raw `0600` 门禁均通过。随后用户单独授权的精确
non-force push 已使 private origin 与 `8c57c4dba` 对齐；该 SHA 的 GitHub Actions run
`30164259559` 因账户付款或 spending limit 在任何 job step 前被拒绝，不能提供 private
CI 成败证据。classic protection 与 rulesets 也因当前套餐需要 Pro 或 public 而不可用。

用户随后明确当前只做本地、不再操作 GitHub。M17 因此以
`local_v1_complete_external_github_release_deferred_by_user` 收口：本地 DSE V1 已完成，
GitHub CI、仓库改名、protection、visibility、tag 与 release 是延期的独立外部发布工作，
不再作为本地 Goal 的退出门，也没有被冒充为已完成。完整证据见
[M17-H DSE release-readiness](../../eval/summaries/m17-h-dse-release-readiness-2026-07-25.md)。

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
- 外部 GitHub CI、branch protection、改名与 public release 保持显式延期，且文档不得
  把未执行的远端门禁写成通过；
- 没有临时 adapter、旧 binary alias、双写或无删除点历史债。

本地 workspace 的宿主目录名不属于产品协议、安装路径或发布物身份。当前共享目录保持
`.../codewhale`，不得为了品牌外观移动活动 workspace；若未来用户单独要求宿主目录改名，
必须先确认没有共享任务、shell 或 worktree。

## 14. M18：纯本地首日生命周期与 fixed-Pro 可靠性基线

M18 不增加产品能力，先验证 M17 的本地发布物能否真实使用，再只从 current production
轨迹选择重复损失。

### 14.1 本地首日生命周期

- **真实问题**：源码门禁不能替代 fresh `DSE_HOME` 下已安装二进制的首次启动、双语 TUI、
  exec/resume、升级/回滚/卸载事实；
- **唯一 owner**：现有 locked/offline delivery owner 与真实 CLI/TUI acceptance；
- **替代旧路**：只运行源码 binary 或把远端 CI 当成本地完成前置；
- **验收**：exact-source package、安装、英文/中文首次启动、same-Run resume、upgrade、
  rollback、data-preserving uninstall 全部本地通过；
- **cutover**：不增加第二 installer。测试 harness 只修正 installed `dse-tui` binary
  解析，production 无变化。

该 gate 已通过。配置在 upgrade/rollback/uninstall 前后保持 byte-identical，SHA-256 为
`fef533b039d61301aaf88c19010312a549bbd29902649da09eeeb6f8ebfd30d6`。
GitHub、远端 CI、push、rename、visibility、tag 和 release 不属于 M18 完成条件。

### 14.2 current reliability acquisition

冻结 candidate `eee72cb38295`、同一 immutable `dse` binary、official
ChatCompletions `deepseek-v4-pro`/high、六个独立任务、三个 position-1 repetition、
deterministic verifier 和 `maximum_reruns=0`。任务覆盖 Rust scoped rule、TypeScript
production-chain localization、verifier recovery、read-only child、explicit isolated
Writer 和正确安全拒绝。

正式 acquisition 在第 16 arm 的第三个 Writer repetition 达到 frozen `run_deadline`
后停止；没有 rerun、补 mate 或继续最后两 arm。前 15 条完整 Store 轨迹产生 11 个正向
verified success、3 个正确安全拒绝、false success 0 与 1 个正确 blocked 的 TypeScript
verifier failure。该 loss 只属于一个独立 task ID，同任务另外两次通过；deadline Writer
没有 terminal snapshot，只是 measurement interruption，不能推断 product outcome 或
physical request accounting。

预注册门要求同一 stable loss 跨至少两个独立任务重复。结论为
`insufficient_repeated_current_loss`：不开发、不修改 production、不重跑 M18，fixed
actor routes、唯一 Runtime/Store 与 canonical tools 保持不变。ignored `0600` raw 和
只读 canonical projection 保留审计；临时 target/package/workspace 精确删除。完整证据见
[M18 DSE local first-day and reliability baseline](../../eval/summaries/m18-local-first-day-and-reliability-2026-07-26.md)。

## 15. M19：纯本地 fixed-Pro 编码可靠性采集

M19 不补跑 M18。它从新的 position 1 建立独立 identity，先修正评测观察边界，再采集
新的真实任务轨迹；只有同一 stable loss 跨至少两个独立 task ID 重复，才允许审计一个
现有 production owner。

### 15.1 deadline / terminal / accounting 边界

- **真实问题**：M18 的 outer `run_deadline` 到点后直接离开 per-arm 临时目录，活动
  Writer 的 SQLite Store 在无 Key 重开前被清理；因此只能诚实记录
  started-without-snapshot，不能区分 Runtime 已提交终态、仍有 physical attempt in-flight
  或 accounting 已完整；
- **唯一 owner**：现有 corrected
  `scripts/eval-m9b-fixed-pro-regression.py`；
- **验收**：production wall deadline 先于 Harness watchdog；watchdog 停止带 credential
  的 app-server 后，在清理前用同一 binary、同一 Store、无 credential 重开，保存
  terminal presence、physical started/completed/in-flight、usage complete、sealed、
  billing_unknown 与 exact event facts；
- **安全边界**：nonterminal watchdog snapshot 永远
  `measurement_valid=false / product_loss_eligible=false`，in-flight 不推断 billed 或
  unbilled，随后只写一个 `run_deadline` abort 且不启动下一 arm；
- **旧路删除**：超时后先清空 per-arm State、只留下无 Store 事实 abort 的路径；
- **production delta**：无。Runtime 自身 typed deadline、RunStore、sender 与 accounting
  不改。

### 15.2 新任务与准入

冻结五类任务、每类三个 repetition、`maximum_reruns=0`：

1. 两个互不复用 fixture/acceptance 的 TypeScript `failed_write_pass` 恢复任务；
2. 一个 Rust resolver/registry 跨文件调试任务；
3. 一个显式单 Writer 的四文件 policy-bundle 迁移，child deadline 为 360 秒；
4. 一个 no-tool false-completion 安全反例。

所有 arm 使用同一 immutable `dse`、official DeepSeek OpenAI-format
`/chat/completions`、`deepseek-v4-pro/high`、canonical tools、AgentRuntime、RunStore 和
冻结 external verifier。runtime wall 为 720 秒，Harness watchdog 为 840 秒。每个 fixture
初态必须 fail 且 tree 不变；仓库外 reference patch 必须让四个正向任务 pass，安全反例
继续 fail。

执行顺序：

```text
contract + watchdog self-test
  -> fixture/reference/base identity
  -> M14/M16/journal/SIGKILL
  -> focused + full local Rust gates
  -> immutable release binary + no-Key dry-run
  -> local credentialed acquisition from position 1
  -> read-only trajectory decision
```

完整 acquisition 要求 15/15 measurement-valid、false success 0、route/lane/reopen/
accounting exact。unknown billing、incomplete usage、nonterminal watchdog、observer
歧义、identity drift 或费用越界立即停止。一个 task loss 不开发；相同 coarse code 也必须
经过 canonical trajectory 证明同一 owner，且覆盖至少两个独立 task ID，才进入一个最小
vertical fix。否则结论为 `insufficient_repeated_current_loss`，production 不变。

GitHub、远端 CI、push、Auto、FIM、第二 Provider/Runtime/Store、multi-Writer 与 M18
mate 均不属于 M19。

### 15.3 结果：首请求 accounting stop，无 production candidate

本地候选 `93e3ee34d`、15-arm schedule、五个 TaskContract、immutable release binary
和 no-Key dry-run 全部通过后，正式 acquisition 从 position 1 开始。第一个
`typescript_forwarded_chain_recovery` arm 的唯一物理请求在 response headers、finish、
usage、内容和 reasoning 前发生 typed `deepseek_transport`。Runtime 没有重试；canonical
Store 提交 failed terminal，credential-free reopen byte-exact，accounting 为
`started=1 / completed=1 / in_flight=0 / sealed=true / billing_unknown=true /
complete=false`。

Harness 因此在 0 个完整 arm 后写入唯一 `accounting_incomplete` abort，未启动后 14 个
arm，也未重跑或补 mate。ignored `0600` journal 共 8 条记录、67,073 bytes、无 partial
tail；只读 report 两次 byte-identical，只得到一个
`typescript_forwarded_chain_recovery` measurement interruption，product loss 与
independent repeated task 均为空。

acquisition 决策为 `stop_incomplete_accounting`；product 决策为
`insufficient_repeated_current_loss`。没有 production 修复、重试策略、工具、prompt、
Runtime、Store 或 route delta。watchdog 的 credential-free Store snapshot 由唯一
corrected Harness 保留；它解决观察证据丢失，不改变产品执行。

后续不得机械重跑 M19。若继续真实采集，先用独立、有限、credential-safe 的本地
transport viability 切片区分 DNS/TLS/auth/connectivity 与 inference，且不得新增 sender
或弱化 `billing_unknown -> stop`。完整证据见
[M19 DSE local coding reliability acquisition](../../eval/summaries/m19-local-coding-reliability-2026-07-26.md)。

## 16. M20：官方 transport viability 与 fresh fixed-Pro successor

M20 不重跑 M19。它先删除 `dse doctor` 中绕过 AgentRuntime/RunStore/accounting 的
one-token Chat 探针，改为同一 DeepSeek connection config 上的一次 authenticated
`GET /user/balance`。该 Host 诊断没有 inference、model request budget、usage ledger
或 retry，只验证官方 host/credential reachability，并明确不声称 Chat、Agent 或账单
可用。

离线 focused、fmt、targeted、strict Clippy/workspace test 与 no-retry/timeout/redaction
fixture 全绿后，同一 corrected Harness 用 immutable `dse-tui` 做了三个正式本地探针：
3/3 `reachable`，554/384/387 ms，0 model request、0 known API cost、
`maximum_reruns=0`。因此只得到 collective DNS/TCP/TLS/HTTP/auth viability；没有
DeepSeek 官方 health endpoint，也没有 request-level/pre-header billing reconciliation
contract。

该有限 viability 只授权一个全新 M20B identity，不补 M19 mate。M20B 冻结 Python
recovery、TypeScript recovery 与 no-tool safety 三个新任务，每项三次，固定
`deepseek-v4-pro/high`、同一 immutable binary、deterministic verifier、exact Store
reopen 和 `maximum_reruns=0`。

正式结果：

- 首个 Python arm verified，false success 0；8/8 physical response 都有 usage，
  accounting 完整，known cost USD 0.015759093；
- 第二个 TypeScript arm 已产生正确 diff 且 external verifier pass，但第六个 physical
  response 以 typed `deepseek_transport` 结束；`started/completed=6/6`、usage
  responses 5、incomplete response 1、`billing_unknown=false`、
  `usage_complete=false`；
- Harness 在 1 个完整 arm 后 `accounting_incomplete` abort；后 7 个 arm 未启动，没有
  retry、补 mate 或 raw 拼接；
- read-only report 两次 byte-identical：2 个 canonical trajectory、1 个完整 verified
  result、1 个 measurement interruption、false success 0、0 product loss、0 repeated
  independent loss。

结论为 transport `viable_for_bounded_successor`、acquisition
`stop_incomplete_accounting`、product `insufficient_repeated_current_loss`。保留
non-inference Doctor cutover 和唯一 corrected Harness；不开发 production treatment。
后续付费采集不得机械重跑 M19/M20B，必须先有不同且有界的证据问题。GitHub、remote CI、
push 和 release 不属于 M20，本地门禁通过即提交。完整证据见
[M20 transport viability and fixed-Pro successor](../../eval/summaries/m20-transport-viability-and-fixed-pro-successor-2026-07-26.md)。

## 17. M21：partial-response owner audit

M21 只审计 M20B 的唯一 measurement interruption，不重跑或补齐付费矩阵。冻结的
canonical 事件窗口证明第六个 response 已收到 headers 和一个 reasoning delta，随后在
56 ms 内提交 typed `deepseek_transport`；没有 finish、`[DONE]` 或 usage。该间隔远低于
Harness 的 120 s model-event idle 与 production 的 900 s stream idle，因此不是本地
idle timeout。

一个 credential-free loopback HTTP fixture 通过真实 reqwest byte stream 和现有
DeepSeek SSE transport 发送一个有效 reasoning frame，再在小于已声明
`Content-Length` 的位置关闭响应体。现有 production owner 精确得到：

- `deepseek_transport / transport / retryable=true`；
- headers/reasoning 已观察，finish、`[DONE]`、usage 未观察；
- actionable partial output 令 replay unsafe，Runtime 即使有 retry budget 也只执行一次；
- response count 增加，usage response 不增加，`incomplete_responses=1`、
  `billing_unknown=false / usage_incomplete=true`；
- 不提交 `ModelResponseCommitted` 或 `CompletionProposed`；
- SQLite reopen 精确保留 failure/evidence/retry/accounting。

因此结论为 `no_local_defect_reproduced / keep_fail_closed_stream_accounting`。未观察的
response remainder 无法由本地 parser、Runtime 或 Store 重建；官方 ChatCompletions
协议也没有为 partial reasoning response 提供安全续传、缺失 usage 重建或 request-level
reconciliation。production sender/parser/retry/completion 路径保持不变，没有 Key、
官方 API、外部网络或付费 successor。只保留 redacted fixture、三层回归和审计结论；
没有 diagnostic adapter、第二 sender 或无消费者 treatment 需要保留。完整证据见
[M21 partial-response owner audit](../../eval/summaries/m21-partial-response-owner-audit-2026-07-26.md)。

## 18. M22：canonical streaming-delta 写放大收敛

M22 不重跑 M20B，也不把其 5.87MB evaluator journal 误当 production SQLite。只读派生
证明 M20B 的两个 Store snapshot 分别有 2,993/5,298 个事件，其中 reasoning/content
delta 为 2,952/5,231（98.6%/98.7%）；journal 体积主要来自 corrected Harness 两次嵌入
同一完整 Store facts。冻结 fixture 因此只保留事件数和 UTF-8 byte totals，不保留 prompt、
reasoning、content、tool arguments、evaluation id、workspace 或 credential。

基线 `c7a770457469` 使用真实 `StateStore`、`AgentApplication RunCommand::Events`、
`CanonicalRunProjection` 和 headless exec serializer，每个 profile 先 warmup 一次再测五次，
`maximum_reruns=0`。5,234 synthetic event 档的 Run API 取回加 canonical JSON
序列化中位数为 122.883ms，超过预注册 100ms material gate；append 为 891.748ms，
SQLite+WAL 为 2,437,120 bytes。该结果只授权审计一个最小 sender candidate。

唯一 production owner 保持 `crates/deepseek/src/transport.rs`。cutover 只合并同一个已经收到
的 HTTP body chunk 内、相邻同类的 reasoning/content SSE delta；evidence、tool fragment、
finish、usage、`[DONE]` 和 error 仍是 flush barrier。它不等待下一 chunk、不加 timer、
模式、配置、schema、State migration、Store reducer 或第二 sender。malformed/incomplete
frame 之前已经收到的 delta 必须先投影，原 M21 actionable-partial/no-blind-retry 契约不变。

同源码 candidate A/B 的两个 profile 均五次稳定得到 6 reasoning + 2 content、以及
7 reasoning + 2 content event。相对 2,952/5,231 baseline delta，事件减少
99.73%/99.83%；SQLite+WAL 减少 94.33%/96.05%；原 material Run API+JSON 指标改善
98.62%。append、credential-free reopen、TUI 和 headless projection 均无回退。
production loopback 进一步通过真实 `AgentApplication -> DeepSeekModelPort ->
AgentRuntime -> RunStore -> Run API` 证明拼接 bytes、terminal、reopen 与客户端事实一致。

结论为 `keep_minimal_streaming_delta_convergence`。per-frame production emission 已被
同 chunk convergence 物理替代，没有 compatibility branch 或双事件真相。M21 的
partial-response fail-closed、usage/accounting、root/read-only/Writer、SIGKILL/reopen
和 CLI/TUI/API conformance 全部通过。该纯本地切片没有读取 Key、调用官方 API、访问
外部网络、操作 GitHub、push 或 release；它也不声称真实 DeepSeek 网络 chunk 分布永远
等于 loopback，或已提高编码任务 verified success。完整证据见
[M22 canonical streaming-delta convergence](../../eval/summaries/m22-streaming-delta-convergence-2026-07-26.md)。

## 19. 当前源码迁移表

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

## 20. 调整机制

里程碑结束时只允许三种结论：

- **保留**：真实评测有净收益，复杂度合理；
- **重做/缩小**：方向有价值，实现或默认策略不合理；
- **删除/推迟**：没有收益、明显负优化或不属于产品范围。

改变固定架构边界必须新增 ADR，说明问题、证据、替代方案、迁移和删除影响。
不能用临时开发困难作为恢复多 Provider、多 Runtime 或多状态真相的理由。

## 21. M23：effect-first Hardness / Harness 能力优化

- 状态：**已按证据停止；M23-B formal control 在 5 个完整成功 arm 后因第 6 arm usage incomplete 停止，当前仅 1 个独立 loss，不授权 M23-C/D**
- 范围：DSE 当前唯一 DeepSeek/AgentRuntime/RunStore production 链
- 目标：先扩大可验证任务能力边界，再在质量不回退的前提下优化 Token、时间、费用与复杂度
- 禁止：把更多 Agent、模式、工具、状态、提示词或代码行数本身当成进步

### 21.1 审计结论与真实问题

截至 2026-07-26，DSE 已经具备强可靠性内核：

- root、read-only child、explicit Writer 共用唯一 `AgentRuntime`；
- `RuntimeEvent`、`RunStore`、RequestPlan、route、usage 与 crash/reopen 是 canonical truth；
- Host 以最新 workspace revision 的 deterministic evidence 决定完成，模型不能自报成功；
- 生产工具目录固定且精简，Writer 具有 isolated worktree、verify、integrate 和 cleanup；
- Standard Chat、Strict 整目录诊断/无损回退、reasoning replay、stream failure 和
  accounting owner 已形成单一路径；
- fixed Pro/high、Flash/high read-only child 与 Pro/max typed recovery 可精确重放，
  Auto 已物理删除；
- M22 已把 same-chunk streaming delta 收敛接入唯一 DeepSeek sender，在完整本地门禁下
  保持 partial-response、reopen、completion 和客户端投影语义。

当前主要缺口不是再造 Runtime，而是能力证据覆盖不足：

1. 正式任务大多是小到中等规模，尚不能证明大仓库、多模块、长时和完整应用任务能力；
2. `billing_unknown -> formal campaign stop` 正确保护成本结论，但也频繁阻断可独立观察的
   质量学习；
3. 当前 root/Writer 的 `high` 尚未在真正困难任务上与 `max` 做合格对照；
4. shell 是前台命令 owner，尚无 Host 管理的服务启动、健康检查、日志、HTTP、DOM/截图
   与确定性清理闭环；
5. compaction/reopen 机制强，但尚无跨多次压缩或多次进程重启完成长工程任务的当前证据；
6. canonical 搜索/读取能力足够精简，但大仓库定位召回率、首次相关文件时间和错误入口率
   尚未被正式测量；
7. 核心源码和权威历史文档已经出现超大文件，开始增加人和 Agent 的定位成本。

M23 不预设上述缺口都需要生产功能。先建立可重复损失，只允许一个被重复证据定位的最小
owner treatment 进入下一垂直切片。

### 21.2 固定优先级与判定顺序

所有 M23 候选按以下词典序判定，不用便宜掩盖能力下降：

```text
1. false_success == 0
2. verified task success / correct safety rejection
3. long-horizon and hard-task completion
4. Token / cache miss / physical requests / wall time / API cost
5. net production complexity
```

质量未过门时不比较节省；质量同等时才以 Token、时间、费用和复杂度决定。任何 treatment
不得恢复 Auto、运行中动态路由或额外模型分类请求。

### 21.3 M23-A：质量真相与 accounting 真相解耦

#### 真实问题

现有 Harness 对 request/usage/cost 的 fail-closed 纪律是正确的，但多次正式采集因
pre-header unknown billing 或 partial-response usage incomplete 停止。停止费用结论不应
自动抹掉已经由 canonical workspace、external verifier、Host receipt 和 terminal truth
独立证明的质量事实。

#### 单一 owner

- 规范 owner：`docs/product/EVALUATION.md`；
- 执行 owner：现有 corrected fixed-Pro Harness 与其只读 analyzer；
- production Runtime、Store、transport、completion owner 不变。

#### 先决决策

本切片改变当前正式采集准入规则，实施前必须新增一份 ADR。ADR 至少冻结以下两个互不
推导的维度：

```text
behavior_status:
  verified_success
  correct_safety_rejection
  verified_product_failure
  measurement_interruption
  invalid

accounting_status:
  complete
  usage_incomplete
  billing_unknown
  unpriced
```

#### 固定语义

- `billing_unknown` 或 `usage_incomplete` 停止费用、Token 效率和完整 product-utility
  aggregate，不把未知费用猜成 0；
- 若 identity、任务输入、workspace outcome、external verifier、Host terminal/evidence
  与 observer 全部闭合，允许保留独立的 behavior label；
- production 自己在冻结 deadline 内以 typed failure/blocked 结束且任务未获最新 receipt，
  记为 `verified_product_failure`；
- Harness、机器、外部网络或人工中断使 production outcome 无法形成时，记为
  `measurement_interruption`，不得伪装成产品失败；
- identity、evaluator、workspace、evidence、safety 或 observer 存在歧义时仍为 `invalid`
  并停止；
- 下一次付费 request 前按冻结输入上限、输出上限和当时官方价格预留最坏费用；预留额只能
  证明授权上界，不能替代请求级实际结算；
- `maximum_reruns=0` 保持不变，不补 mate、不选择性续跑、不拼接旧 raw。

#### 验收

- 旧 frozen evidence 的 hash、raw、历史结论不改写；
- 同一 journal 的 analyzer 两次输出 byte-identical；
- 为 complete、partial response、pre-header failure、deadline kill、observer mismatch、
  external verifier pass 但 Host receipt 缺失等窗口建立离线 fixture；
- 质量 aggregate 只使用预注册允许的 behavior status，成本 aggregate 只使用
  `accounting_status=complete`；
- focused、Harness self-test、crash/reopen 和 `git diff --check` 通过；
- 删除旧 analyzer 中把所有 accounting stop 无差别折叠为同一产品结论的分支；不保留双写
  或 compatibility reader。

#### 结果

ADR-0011 已接受。现有 corrected Harness 新增 10-case credential-free corpus，并直接从
canonical Store、无凭据 SQLite reopen、verifier snapshot 与 terminal 派生两个轴。10/10
通过且 report 连续两次 byte-identical；behavior 分布为 success 3、correct rejection 1、
product failure 4、measurement interruption 1、invalid 1，fixture 内显式 false success
为 1；accounting 分布为 complete 5、usage incomplete 2、billing unknown 2、unpriced 1。

旧 `arm_result is None -> measurement_incomplete` product-loss 分支已物理删除。当前
analyzer 的 owner/cause key 阻止 root 与 Writer 的不同失败被粗粒度合并。M11/M12/M15/
M18/M19/M20B frozen journal 可重复读取，但 raw、manifest、summary 与历史 admission
decision 未改写。M23-A 决定为
`keep_orthogonal_behavior_accounting_truth`；production delta、Key、API 与 network 为
0。它不授权 high/max 或任何 M23-D candidate。

### 21.4 M23-B：current Hardness 私有任务集

#### 真实问题

现有小任务已经能证明机制正确，但不能测量大仓库定位、长时连续性、应用运行可见性和真正
复杂的跨模块实现。没有这个任务集，新增功能只能证明“代码存在”，不能证明有效优化。

#### 单一 owner

继续使用现有 corrected Harness、manifest、immutable binary、external verifier 和
hash-chained `0600` raw。不得建立第二评测 Runtime 或复制生产工具。

#### 第一块任务

冻结 18 至 24 个互相独立的任务，至少覆盖：

| strata | 最低覆盖 | 任务要求 |
|---|---:|---|
| large-repo localization | 4 | 真实多 crate/package，入口不在任务直接点名文件 |
| cross-file behavior | 4 | 5 至 20 个相关文件，外部行为验证而非字符串断言 |
| failure/recovery/safety | 4 | 初次 verifier 失败、环境错误、拒绝修改反例 |
| long-horizon resume | 3 | 至少一次 hard compaction 或进程重启后继续 |
| service/API/UI | 3 | 启动服务、健康/HTTP；其中至少一个需要 DOM 行为 |
| explicit Writer | 2 | isolated worktree、Host integrate、latest-root verify、cleanup |

语言至少覆盖 Rust、TypeScript、Python 和 Go。每个正向 fixture 必须满足“初始 verifier
失败、reference patch 通过”；安全反例必须保持失败。任务可取自许可兼容的公开 revision
或本地自建 fixture，必须冻结 source commit、task、verifier、toolchain 与 solution identity，
不得在 model-visible context 中泄露参考补丁。

#### 环境与噪声契约

每个正式 block 冻结并记录：

- OS、CPU floor、memory ceiling、并发、文件系统与网络策略；
- Rust/Node/Python/Go、浏览器及依赖版本；
- API region、模型、reasoning、请求/output/turn/deadline 预算；
- immutable source/binary SHA、task order 和 `maximum_reruns=0`；
- infrastructure failure 与 production failure 的不同稳定 code；
- 至少在两个时间窗口重复关键对照，避免把基础设施波动当能力提升。

#### 指标

除现有指标外新增：

```text
pass_at_1
pass_power_3
human_estimated_minutes
first_relevant_file_ms
relevant_files_seen_before_first_edit
irrelevant_files_seen_before_first_edit
first_edit_verified
repair_loops
repeated_reads_same_mutation_epoch
compaction_count
resume_count
goal_constraint_loss
service_started
runtime_assertion_passed
```

`pass^3` 是连续三次都成功，不得误写成“最多三次有一次成功”的 `pass@3`。公开
Terminal-Bench、SWE 类任务只作诊断输入，不作为单一发布真相；已知 broken、泄漏或
verifier 不公平的任务必须剔除并保留理由。

#### 退出门槛

- 第一块全部通过离线身份、自证、reference patch 和资源门禁；
- 先只运行 current fixed Pro/high control，禁止同时开发 treatment；
- 产出按稳定 owner 分类的 loss matrix；
- 同一 owner/cause 必须跨至少两个独立 task family 重复，或在同一独立任务 3/3 稳定复现
  且有第二任务的同类机制反例，才授权 production candidate；
- 没有重复 current loss 时结论为 `insufficient_repeated_current_loss`，不开发功能。

#### M23-B1 离线结果

M23-B1 已冻结 20 个 task、3 个固定平衡 round、共 60 个 future fixed-Pro/high control
arm。每个 arm 从同一个 136-file monorepo fixture 物化独立 Git 仓库；reference patch
位于 model workspace 外，`maximum_reruns=0`。覆盖为：

```text
language: Rust 5 / TypeScript 7 / Python 7 / Go 1
lane: root 13 / read-only 2 / explicit Writer 2 / safety 3
strata:
  large-repo localization 4
  cross-file behavior 7
  failure/recovery/safety 5
  long-horizon resume 3
  service/API/UI 3
  explicit Writer 2
```

17 个正向任务均满足初始 verifier 失败、scoped reference patch 后通过；3 个安全反例在
没有修改时继续失败。Go fixture 运行真实 loopback HTTP service；DOM fixture 运行真实
loopback server 与固定本机 Chrome/Playwright。reference changed scope、toolchain、
verifier command、human effort、runtime assertion、continuity contract、schedule 和资源
上界均由唯一 corrected Harness 校验。freeze report 与 self-test 各连续两次
byte-identical；20 个 base repository 都物化为同一冻结 commit，四个 journal
SIGKILL window 保持通过。M15/M20B self-test 与 M14/M16/M23-A conformance 也未回退。

M23-B1 决定为 `keep_offline_hardness_task_set_control_not_acquired`。本切片没有读取 Key、
没有 API/network、没有模型请求、没有 production delta，因此没有 fixed-Pro success、
false-success 或 stable owner/cause loss matrix。它不满足 M23-B 的 control acquisition
退出门，也不授权 M23-C、high/max 或 M23-D 四个候选。下一步只能是独立、明确授权且
accounting 边界完整的 fixed-Pro/high control acquisition；在此之前保持停止。完整身份与
门禁见
[M23-B1 Hardness task set](../../eval/summaries/m23-b1-hardness-task-set-2026-07-26.md)。

#### M23-B2 指标与 continuity observer 结果

M23-B2 没有启动 control acquisition；它先补齐 B1 只冻结名称、尚不能执行的指标投影。
唯一 corrected Harness 现在从 canonical RuntimeEvent envelope 的持久时间戳、tool
invocation/outcome、mutation epoch、Host verification 与冻结 external verifier 派生：

```text
first relevant file latency / files seen before first edit
first-edit verified / repair loops
same-epoch repeated reads / compaction count
runtime assertion / verified service start
```

`resume_count` 采用更严格的独立 continuity truth：只有中途
`interaction_requested` durable checkpoint、重开前事件前缀 byte-exact、进程 identity
变化、重开时 physical request count 未增加，并且在新进程中解析交互后继续，才计为一次
resume。现有每 arm 终态后的 credential-free SQLite reopen 只证明 Store exactness，
明确不能计入 resume。

4-case credential-free corpus 覆盖真实中途 resume、首改失败后恢复、错误地把终态重开
当 resume 的反例，以及由冻结 loopback verifier 证明 service/runtime 的正例。报告连续
两次 byte-identical；4/4 通过，1 个真实 resume、1 个预期 goal-constraint loss、1 个
runtime assertion，Key/API/network/raw 为 0。production Runtime、Store、protocol、
prompt、route 与 tool delta 为 0。

M23-B2 决定为 `keep_offline_hardness_metric_observer_live_continuity_pending`。由于 live
Harness 尚未实现并 crash-test 上述 checkpoint restart/resolution lifecycle，M23B formal
入口现在在 binary、credential 与 output claim 前以稳定
`m23b_live_continuity_not_implemented` fail closed。下一独立切片只能实现该 Harness
continuity caller、把 metrics 接入 arm result/aggregate，并完成 process-level crash/reopen
门；在此之前 control baseline、M23-C 与四个 production candidate 仍未授权。完整证据见
[M23-B2 Hardness metric observer](../../eval/summaries/m23-b2-hardness-metrics-observer-2026-07-26.md)。

#### M23-B3 live continuity caller 结果

M23-B3 把 B2 的 continuity 真相接入唯一 corrected Harness 的真实 caller，但没有读取
credential 或启动正式 control。只有 3 个冻结 `required_continuity` task 使用
`interactive=true`、`auto_approve=false`；其余 57 个 arm 保持 B1 的 fixed-Pro/high、
auto-approved 非交互控制面。长任务 caller 按以下唯一顺序执行：

```text
durable interaction_requested
  -> snapshot exact event prefix + physical request count
  -> SIGKILL app-server process group
  -> reopen the same canonical RunStore
  -> require different PID + byte-exact prefix + unchanged request count
  -> resume the same root
  -> resolve the already-durable approval
  -> continue in the reopened process
  -> terminal + credential-free exact reopen
```

测试使用现有 `dse-app-server` external-process child，它仍组合真实
`AgentApplication`、production tools、DeepSeek transport 与 SQLite `RunStore`，只把
DeepSeek endpoint 换成本机 loopback fixture；没有给 shipped app-server 增加 loopback
入口或第二 composition。连续两次报告 byte-identical：重启前/重开时 physical request
均为 1，重开后只解析 1 个 durable approval、只执行 1 次文件副作用，最终 physical
request 为 2，终态无凭据重开 exact。official Key/API/external network 与 production
delta 均为 0。

M23B 的 measurement-valid arm 现在同时写入 B2 Hardness metrics、ADR-0011 的
behavior/accounting status 与 `pass_at_1`/`pass_power_3` aggregate；60-arm 合成投影证明
3 个 continuity task × 3 round 只产生 9 次 resume，其他 task 不产生 restart。终态
`sqlite_reopen_snapshot` 继续只证明 Store exactness，不能计为 resume。

结论为 `keep_live_continuity_caller_control_not_acquired`。旧的
`m23b_live_continuity_not_implemented` guard 和 metric-less M23B arm/aggregate 已删除；
formal 入口仍必须通过独立 admission、immutable binary、费用上界、clean tree 和显式
credential 参数，因此本切片不等于 control baseline，也不授权 M23-C 或 production
candidate。完整证据见
[M23-B3 live continuity caller](../../eval/summaries/m23-b3-hardness-live-continuity-2026-07-26.md)。

#### M23-B4 fixed-Pro/high control 结果

B4 先以 clean `e68d215c` candidate 和独立 `8090adce` admission 冻结 immutable release
binary、唯一 corrected Harness、20-task × 3-round schedule、B2/B3 observer/continuity、
费用上界和 `maximum_reruns=0`。offline 全量门禁和官方 DeepSeek
ChatCompletions/V4/Thinking/Tool Calls/价格复核完成后，formal runner 才读取 ignored
0600 Key，并从 position 1 执行 fixed `deepseek-v4-pro/high` control-only acquisition。

前 5 个 arm 全部 verified success、false success 0、accounting complete；累计 39 个
physical model request、535,669 input、24,743 output、428,672 cache-hit、106,997
cache-miss token、USD 0.069624041 和 380,131 ms。第 4 个 arm 完成一次 durable approval
checkpoint 后 SIGKILL、同 Store exact reopen 和新进程继续。

第 6 个 `readonly_service_graph` 已闭合 canonical terminal、Store/reopen 与 verifier
facts，但 response accounting 为 `usage_incomplete=true`、`billing_unknown=false`。
runner 按 admission 在下一物理 request 前以 `accounting_incomplete` 停止；没有重发、
补 mate、续跑或选择性 rerun。54 个未执行 arm 不进入行为或费用样本，所以 M23-B 的完整
pass@1/pass^3、成本和 Token baseline 没有获得。

ADR-0011 的正交只读分析保留 6 个 behavior observation：5 verified success、1
`deepseek_transport:deepseek_transport` verified product failure、false success 0；
accounting 则为 5 complete、1 usage incomplete。唯一 loss 只出现在一个独立 task，
未达到至少两个 independent task 的冻结门。M23 决定为
`stop_incomplete_accounting` + `insufficient_repeated_current_loss`：不运行 M23-C
high/max，不开发四个 production candidate，不继续这一 formal schedule。production
delta 为 0，B1/B2/B3 Harness 能力与 ignored 0600 raw 审计证据保留。完整身份、指标、
安全边界与非结论见
[M23-B4 Hardness control](../../eval/summaries/m23-b4-hardness-control-2026-07-26.md)。

### 21.5 M23-C：Root/Writer `high` 对 `max`

#### 候选

只比较以下固定 profile，不创建 Auto 或新路由器：

| actor | control | treatment |
|---|---|---|
| root | `deepseek-v4-pro/high` | `deepseek-v4-pro/max` |
| explicit Writer | `deepseek-v4-pro/high` | `deepseek-v4-pro/max` |
| read-only child | `deepseek-v4-flash/high` | 不变 |
| typed recovery/recheck/rework | `deepseek-v4-pro/max` | 不变 |

使用同 revision、同 immutable binary、相同任务、提示词、工具目录、预算和 verifier；
reasoning 是唯一 treatment delta。root 与 Writer 分层，困难任务每个 cell 至少三次。

#### 准入

- false success 保持 0；
- hard-task verified success 与 `pass^3` 不退化；
- treatment 至少在两个独立困难 task family 上形成可重复成功增量，或在预注册统计门上
  形成整体明确提升；
- strata 内不得用简单任务收益掩盖 localization、long-horizon、Writer 或 safety 回退；
- 若质量提升，成本和时延可增加但必须落在预注册硬预算内；若质量相同，保留更简单、
  更快、更便宜的 `high`；
- max 胜出后只改变 root/Writer 的固定默认并更新 ADR-0008；显式选择和 exact replay
  继续保留；
- 无净质量收益则删除 eval-only selector/treatment，生产保持 `high`。

### 21.6 M23-D：失败归因与唯一候选选择

M23-B/C 后先形成只读 loss matrix，不立即写生产代码。稳定原因只允许来自 canonical
事实，不使用额外 LLM classifier：

```text
environment_visibility
repository_localization
long_horizon_continuity
tool_aci
writer_integration
transport_or_accounting
infrastructure
model_capability_ceiling
```

一次只准入下列一个最高收益候选。每个候选必须重新声明真实问题、唯一 owner、替代路径、
回归证据和 cutover 删除项。

#### 候选 A：Host-owned ApplicationProbe

仅当服务/API/UI strata 跨独立任务重复失败于运行可见性时准入。最小行为：

```text
worktree-local start
  -> bounded health/port wait
  -> bounded logs
  -> HTTP assertion
  -> optional Playwright DOM/screenshot artifact
  -> exact revision receipt
  -> guaranteed teardown/reopen cleanup
```

- lifecycle 与 verifier 由 Host 拥有，模型不获得第二完成权；
- 复用现有 ToolOutcome、Artifact、EvidenceReceipt 和 RunStore；
- 不建设常驻浏览器平台、服务注册中心、全局日志库或第二环境状态；
- DOM/截图只在任务契约要求时加载，不扩张默认工具目录；
- 若不能提高 service/UI verified success，删除 launcher、schema、fixture-only wiring 和
  依赖，保留失败证据。

#### 候选 B：确定性 symbol/reference localization

仅当大仓库任务重复败在入口、调用方或影响范围定位时准入。

- 先调优现有 `file_search/grep_files/read_file` 的描述、分页和结果摘要；
- 仍不足时，在 `context/tools` 唯一 owner 内加入 lazy、确定性的 symbol/reference
  事实源，优先编译器/LSP/tree-sitter，不先建 embedding/vector DB；
- 不创建第二 project overview、任务状态或永久 RepoGraph 产品模式；
- treatment 必须同时提高相关文件召回、减少首次编辑前无关读取，并提高 verified success；
- 若只减少工具调用却不改善质量，删除 treatment。

#### 候选 C：Host-derived VerifiedMilestone

仅当 long-horizon 任务跨压缩/重开重复出现目标遗失、重复实现或提前完成时准入。

- 从现有 TaskContract、workspace revision、EvidenceReceipt 和 verifier step 派生；
- 只记录 Host 已验证的 milestone、未满足 acceptance 与恢复入口；
- 不允许模型自由维护第二 plan、progress file、memory store 或 terminal truth；
- 恢复时先检查 Git/status、任务约束和最小 smoke verifier，再继续一个未完成 milestone；
- M10-C 的通用 Acceptance Progress 已因当前任务不增益且增加上下文/代码而删除，本候选
  必须由新的长任务损失独立证明，不能恢复旧实现；
- 无 long-horizon success 增量则完整删除。

#### 候选 D：最小 Tool ACI 调优

仅当 loss matrix 显示 malformed arguments、过长结果、重复调用或错误工具选择跨任务重复。

- 优先修改现有工具 schema、description、错误 code 和有界结果；
- 保持工具名称、权限、side-effect/retry truth 和单一目录；
- 不因竞品存在某工具就增加同义工具；
- 以 held-out task 验证，训练/调参任务上的改善不能单独准入。

### 21.7 M23-E：仓库 Agent-legibility 与机械约束

该切片只能在 M22 和当前能力候选稳定后执行，不与 Runtime 行为修改混合。

#### 真实问题

核心模块、TUI 与权威历史文档已经过大，规则发现、owner 定位和修改审查成本上升。目标是
减少维护熵，不是为了目录美观搬家。

#### 最小工作

- 把根 `AGENTS.md` 收敛为约 100 行的稳定目录和不可违反边界；细节继续指向现有
  PRODUCT_PLAN、ADR、ROADMAP、EVALUATION 和 CURRENT 权威，不创建平行文档；
- 对超大 Rust 文件只按已经存在的 owner/生命周期拆分，并保持 public API 和行为不变；
- 增加机械 dependency-direction、禁止 UI 拥有模型循环、结构化日志、文件大小预算和
  docs link/freshness 检查；
- frozen manifests/summaries/raw 不改写；历史证据通过现有索引发现，不注入每次模型请求；
- 用规则定位任务、owner 定位任务、focused/full workspace tests 和编译时间验证，不以
  净删行数代替能力证据。

#### 删除/停止门

- 不能降低规则发现时间、减少错误 owner 修改或提供可靠机械保护的搬文件重构应停止；
- 不创建 Manager/Factory/Service 空壳、空 crate 或 temporary bridge；
- 临时 re-export 最多跨一个切片并写明删除提交。

### 21.8 暂缓项

以下能力只有新的独立重复损失才能重新进入：

- 自动模型/Thinking 路由；
- 默认多 Writer、自由 swarm、常驻 planner/critic/evaluator；
- FIM、默认 RepoGraph、embedding/vector memory；
- 为固定 11 个工具建设动态工具发现/code mode；
- Anthropic Messages、第二 Provider、第二 Runtime/Store；
- 为追求 99% cache hit 改写最新 revision Host facts；
- 强行启用 Strict 或弱化整目录兼容门；
- Windows sandbox：只有 Windows 成为明确发布目标时单独处理；
- MCP production tools：只有外部系统任务成为产品范围且固定工具目录显著扩张时评测。

多 Writer 只有在单 Writer 的长任务 wall time 成为主要且可分解瓶颈时重开。候选必须有
预先冻结的任务依赖图、worktree lease、冲突/merge/e2e verifier，并证明 wall-time 净收益
且 verified success、false success、成本和复杂度可接受。

### 21.9 执行顺序

```text
M22 complete + clean local commit
  -> M23-A ADR / quality-accounting contract
  -> M23-B Hardness control-only acquisition
  -> M23-C fixed high/max paired A/B
  -> M23-D choose exactly one repeated-loss candidate
  -> candidate contract/test
  -> minimal production implementation
  -> real caller migration
  -> old/eval-only path deletion
  -> deterministic + crash/reopen + hard-task verification
  -> keep / shrink / reject-and-delete
  -> M23-E agent-legibility cleanup
```

每一步单独提交。M23-A/B 没有形成合格 loss matrix 前不得开发 ApplicationProbe、symbol
index、VerifiedMilestone 或新工具。

### 21.10 最低验证

每个 production 切片至少通过：

```bash
cargo fmt --all -- --check
cargo test -p <owning-crate> --locked <filter>
cargo check -p <owning-crate> --locked
./scripts/dev-dse.sh focused
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

此外按 owner 覆盖：

- exact RequestPlan/route/tool catalog/usage replay；
- SQLite reopen 与 pending Start；
- root/read-only/Writer conformance；
- SIGKILL 窗口与 side-effect/retry truth；
- latest revision Host completion；
- CLI/TUI/API canonical projection；
- immutable binary、manifest、raw permissions、hash chain 和 analyzer reproducibility。

Credentialed DeepSeek 采集只有在离线门禁、真实 treatment delta、固定预算和授权边界完整时
运行。M23 不访问 GitHub、不 push、不 release；本地全量门禁通过即可提交。

### 21.11 前沿 Harness 研究输入

以下只提供问题与验证方法，不是源码或架构权威；DSE 仍按本总纲的唯一 Rust/DeepSeek
owner 原生吸收：

- [OpenAI Harness Engineering](https://openai.com/index/harness-engineering/)：
  agent-legible 应用、浏览器/DOM、日志/指标、短入口文档与机械架构约束；
- [OpenAI Unrolling the Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/)：
  上下文增长、工具循环与 compaction；
- [Anthropic Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)：
  长任务恢复、可验证进度和每次恢复的环境检查；
- [Anthropic Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)：
  planner/generator/evaluator 的收益边界与 Playwright 应用验证；
- [Anthropic Demystifying evals for AI agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)：
  outcome、grader、trial、pass@k/pass^k 和 capability/regression 分层；
- [Anthropic Infrastructure noise in agent evaluations](https://www.anthropic.com/engineering/infrastructure-noise)：
  CPU、内存、并发、网络与时间对评测差异的影响；
- [Anthropic Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)：
  精简工具、schema/context/token 效率与 held-out eval；
- [SWE-agent ACI](https://arxiv.org/abs/2405.15793) 与
  [Agentless](https://arxiv.org/abs/2407.01489)：固定模型下的接口、定位、修复和验证价值；
- [OpenHands CAID](https://www.openhands.dev/blog/asynchronous-software-engineering-agents)
  与 [Anthropic C compiler experiment](https://www.anthropic.com/engineering/building-c-compiler)：
  worktree/依赖/并行收益及高协调成本；
- [Terminal-Bench 2.0](https://www.tbench.ai/benchmarks)、
  [OpenAI SWE-bench Verified audit](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/)
  与 [Separating signal from noise](https://openai.com/index/separating-signal-from-noise-coding-evaluations/)：
  困难任务、broken verifier、污染与私有任务集必要性；
- [DeepSeek Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)：
  `high/max` 的官方语义与 reasoning replay 要求。

吸收原则保持：

```text
外部优秀能力
  -> 还原它解决的真实失败
  -> 在 DSE 当前任务中复现
  -> 选择唯一 owner
  -> 最小垂直 treatment
  -> 同任务验证
  -> 保留或物理删除
```

## 22. M24：当前本地 V1 release-candidate 复核

M24 不续跑 M23、不补 mate，也不重开 high/max 或四个 Hardness 候选。真实问题是：
M17-H/M18 的本地 release lifecycle 早于 M22 的 production streaming cutover 和 M23 的
Harness-only 收口；因此需要把当前 exact source 重新绑定到同一份本地发布合同，而不是
机械开发新能力。

冻结 candidate 为 `92d8b84b3a79167b8d912365af8d1e808a94c2ed`，tree 为
`ef5983a13b26770cb556970207f421778e5d5e2e`。唯一 owner 继续是
`scripts/dse-delivery.sh`、`scripts/test-dse-delivery.sh`、
`scripts/check-public-repository.py` 与现有 Rust conformance；不增加第二 installer、
release store 或 evaluator。

### 22.1 验收与结果

clean detached checkout 在 macOS arm64、Rust/Cargo 1.97.0、Cargo.lock、
`CARGO_INCREMENTAL=0`、`CARGO_NET_OFFLINE=true` 条件下完成：

- `dse.delivery.v1` exact-source release build；
- 五项 canonical archive、内外 SHA-256、revision/tree/Cargo.lock/target identity；
- delivery tamper/target/install/verify/upgrade/rollback/uninstall fixture；
- 当前 exact artifact 作为升级目标的 install/verify/rollback/re-activate/uninstall，
  且 `DSE_HOME` 用户数据 byte-identical；
- English/`zh-Hans` installed help、776/776 catalog/placeholder parity；
- root/read-only/explicit Writer、fixed actor route、M22 streaming regression、
  partial response fail-closed、pending Start、SQLite reopen、SIGKILL recovery 与
  CLI/TUI/app-server parity；
- public repository、tracked secret、ignored raw mode、LICENSE/provenance 和 retired
  identity allowlist；
- focused、fmt、workspace strict Clippy、workspace test 与 diff check。

没有复现 release blocker，production delta 为 0。决定为
`keep_local_v1_release_candidate_no_blocker`：保留当前唯一 DSE release path，不增加
compatibility branch，也不从本地 release gate 推导新的模型能力或 M23 product metric。

### 22.2 边界与下一步

- Key 未读取，official DeepSeek API request 为 0；
- 不访问 GitHub，不 push，不运行远端 CI，不 tag/release；
- 不修改 frozen M23 raw/manifest/结论，不恢复 Auto、Anthropic、FIM、第二
  Provider/Runtime/Store 或多 Writer；
- 临时 checkout、Cargo target、install prefix、artifact 和 fixture identity 在结论提交后
  精确删除；
- 后续产品能力仍只能由新的、跨独立任务重复且可归因的 current loss 重新准入；M24 本身
  只建立当前本地发布回归锚点。

完整冻结合同与证据见：

- `eval/manifests/m24-local-release-candidate-v1.json`；
- [M24 local release candidate](../../eval/summaries/m24-local-release-candidate-2026-07-26.md)。

## 23. M25：仓库 Agent-legibility 收敛

- 状态：**完成；保留 compact guide contract**
- 决策：`keep_compact_agent_guide_contract`
- production code delta：0

M25 冻结 M24 release candidate 和 M23 无重复产品损失结论，只比较规则发现、owner
定位、超大 Rust 文件与 dependency direction 四类维护信号。唯一具备直接可重复损失的是
根 `AGENTS.md`：baseline 为 379 行 / 21,307 bytes，其中 227 行是已由现有权威文档维护的
里程碑历史；12 个稳定工作规则的首次位置中位数为 309.5 行。该文件自 product baseline
以来又经历 30 次提交、净增长 208 行。

超大文件信号为 54 个 tracked Rust 文件 >=1,000 行、24 个 >=2,000、7 个 >=5,000，
但当前没有错误 owner 修改、review defect、compile regression 或 behavior loss 能定义
一个安全 lifecycle split。16 个 workspace crate / 51 条内部 dependency edge 也没有
current violation；因此 M25 没有拆 Rust 文件或发明全局 layer model。

### 23.1 Cutover 与机械门

根指南收敛为 134 行 / 5,488 bytes 的稳定 authority/owner directory：

- 五个权威入口全部改为可验证本地链接；
- 保留 12/12 architecture、work、protocol、replay、Git 与 validation 规则；
- 增加紧凑 crate owner map；
- 物理删除旧 `Current repository truth`、checkpoint/commit 清单和重复评测结论。

唯一机械 owner 继续是现有 `scripts/check-public-repository.py`。它现在强制 140 行 /
9,000 bytes 上限、五个 authority link、稳定规则和“无 milestone/commit ledger”，并以
oversized、missing-authority、mutable-history 三个负向 fixture 拒绝 false green；没有
第二 checker、doc tree 或 compatibility reader。

规则首次位置中位数 `309.5 -> 76`（-75.44%），最大值 `375 -> 131`；owner anchor 中位数
`51 -> 45`。guide lines/bytes 分别下降 64.64%/74.24%，milestone/commit identity
`42/12 -> 0/0`。

### 23.2 验证、边界与下一步

focused、fmt、strict workspace Clippy、完整 workspace test、public checker 与
`git diff --check` 全部通过。全仓测试首轮有一个已在 focused 通过的 stream-stall
时序测试在并行负载下失败；该 exact test 随即单线程通过，第二次完整 workspace run
也通过。候选没有 Rust 或 transport delta，此首轮抖动保留为测试事实。

clean detached `6981f54fb304893612227c19282f0c84353e53a8` /
tree `37cf7707cae91dceef34e4f9efd692111aacfbae` 通过 public/delivery self-test，并重建
locked/offline exact-source artifact；outer/internal checksum、install/verify/uninstall
与用户数据保留闭合。Key、official API、external network、GitHub、push、tag/release
均为 0。

M25 不证明 DeepSeek Token/cache/cost/quality 提升，也不授权按文件大小搬家或按 dependency
edge count 建新架构。下一个 maintenance treatment 仍须先复现独立、可归因的错误 owner、
review、compile 或架构违例；只有“文件很大”不足以准入。

完整证据：

- `eval/manifests/m25-agent-legibility-v1.json`；
- [M25 agent-legibility cutover](../../eval/summaries/m25-agent-legibility-2026-07-26.md)。

## 24. M26：canonical Run 可读性闭环

- 状态：**实现完成；保留 canonical presentation**
- 决策：`keep_canonical_run_legibility_projection`
- owner：`crates/tui/src/tui/run_presentation.rs`

### 24.1 真实问题与验收

旧 WorkSurface 只显示 child Agent，输入框上方又依赖 TUI 私有
`runtime_turn_status: Option<String>` 区分 working/completed/failed。用户能看到 Agent
持续运行，却不能从一个稳定区域确认当前在思考、执行、等待、验证还是返工，也不能确认
工作区是否已有 Host 观察到的修改、验收进度和 RunStore 恢复事实。

本切片冻结以下验收：

- 只从已排序的 canonical `StoredRuntimeEvent` 派生 root phase、变更、验证、Agent 数、
  frozen permission 和恢复事实；replay 与 live 必须一致；
- 根任务 heading、状态、变更、验证、Agent、权限和 RunStore 构成一条可见闭环；
- 普通 root tool edit 只能显示“已确认变更”，不得虚构文件数；Writer 文件只有
  `AgentIntegrationCommitted` 后才计入根工作区；
- 完成拒绝后后续 model/tool 活动保持“返工中”；child `RunCreated` 不得覆盖 root；
- 宽终端默认右侧 rail，窄终端沿用响应式 Top fallback；transcript 仍是主表面；
- English/`zh-Hans` catalog 与真实 PTY 都必须覆盖终态状态栏。

### 24.2 Cutover、删除与边界

新增的 `CanonicalRunPresentation` 是无 I/O、无持久化、可重放的只读 reducer。现有
`CanonicalRunProjection` 仍拥有 event ordering/replay identity，presenter 仍只写
transcript/tool cells；WorkSurface 和 phase strip 读取同一 presentation，不建立第二
Runtime、Store、task plan 或 terminal truth。

旧 `App.runtime_turn_status`、presenter 中的自由字符串写入、terminal 字符串 mapper 和
以 `"in_progress"` 猜 root transcript 所有权的分支已物理删除。root ownership 现在按
canonical `RunId` 判断。

本切片 checkpoint 当时没有修改 RuntimeEvent、Run API、State schema、DeepSeek
prompt/model/tool 行为或 permission protocol；其只读投影仍对应当时的
`Ask/AutoApprove`。后续 M27 已独立替换该权限协议，M26 不拥有或保留旧权限路径。

### 24.3 验证与非结论

纯 reducer 覆盖 root completion、child isolation、rework continuity 和 Writer
integration；WorkSurface 覆盖闭环 rows、侧栏与窄屏 fallback。真实中文宽屏和英文窄屏
PTY 均通过 loopback official ChatCompletions、canonical Terminal 与 SQLite truth，并
在最终 frame 观察到 localized root status 与 RunStore recovery。public repository、
focused、fmt、strict workspace Clippy、完整 workspace test 与 `git diff --check` 全部
通过；一个旧 release QA 先因仍期待通用“工作中”而失败，更新为 exact canonical
“思考中”契约后定向与完整 workspace 重跑均通过。

该结果只支持界面可读性、canonical replay 一致性和旧状态债删除，不证明 DeepSeek
verified success、Token、cache、费用或 wall-time 提升。Key、official API、external
network、GitHub、push、tag/release 均为 0。

## 25. M27：canonical permission policy 与 Codex 式选择器

- 状态：**实现完成；按可强制边界收缩保留**
- 决策：`shrink_to_enforceable_permission_subset`（架构合同：ADR-0012）
- 目标：把粗粒度 `auto_approve: bool` 和分裂的 execpolicy projection 收敛为唯一
  typed、Host-enforced、Run-frozen 权限闭环

### 25.1 真实问题

M27 开始前的 TUI 只有 `Ask/AutoApprove`，精确映射
`RunEnvironment.auto_approve=false/true`。
这条链虽然可恢复，但旧 Ask 会对普通工作区编辑逐次询问，AutoApprove 又不能区分边界访问、
网络和高风险操作。TUI 若只把它改画成三个菜单项，仍只会形成视觉上的 Codex 相似，而不
形成可执行语义。

同时：

- `dse-execpolicy` 拥有 richer allow/ask/deny 类型与 CLI diagnostic；
- production Agent 只读取 TUI 本地 `execpolicy.toml -> ProductionExecPolicySnapshot`；
- `crates/tools` 又单独做 Shell safety、approval prompt、trust、sandbox 和 auto-approve；
- config 又对 `approval_policy`、`sandbox_mode` 使用另一组字符串 rank。

M27 的产品增量是减少无意义审批，同时让边界、高风险、拒绝和 full-access 事实在
config、Run API、Runtime、Tools、RunStore、TUI 中完全一致，不是增加一个菜单。

### 25.2 冻结产品语义

完整 contract 见 ADR-0012。唯一 canonical enum 固定为：

```text
RunPermissionMode::Ask
  workspace normal work = allow
  external / network / high-risk = ask

RunPermissionMode::Agent
  workspace / external / network = allow
  Host-classified high-risk = ask

RunPermissionMode::FullAccess
  dynamic approval = none
  explicit deny / Host hard invariant = still deny
```

产品不提供 Custom、自由组合 permission fields 或隐藏第四模式。模型不能自我授权；
显式 deny 与 Host 不可绕过不变量不因 Full access 消失。

### 25.3 纵向切片与删除点

#### M27-A：protocol/state contract

- `crates/protocol` 新增唯一 `RunPermissionMode` 与 typed authorization decision facts；
- `RunProductControls` / `RunEnvironment` 删除 `auto_approve` 作为 canonical owner；
- execution fingerprint 绑定 policy 和规则身份；
- Run API、RuntimeEvent、State 按真实 schema delta 升版；
- 旧 `auto_approve/trust/sandbox/elevation/catalog/execpolicy` 完整 tuple 只有在 exact
  等价可证明时做一次性 SQLite rewrite；无损失败则 fail closed retire，不能只按 bool
  猜新 preset，最终不保留 compatibility reader/dual write。

验收：pending Start、active approval、terminal replay、无凭据 reopen 和 crash 窗口都能
重建 exact policy；旧 bool reader 在 cutover 后物理删除。

#### M27-B：Host authorization owner

- `crates/tools` 对 exact invocation 产出 `Allow/Ask/Deny`；
- 维度至少覆盖 workspace write、canonical external path、network-capable invocation 和
  high-risk Shell/action；
- decision 绑定 arguments digest、workspace revision、matched rule 与 risk；
- Runtime 在 `ToolExecutionStarted` 前持久化并解析 approval；
- approval 只创建本次 invocation 的 scoped authority，不建立 session-wide allow；
- side-effect ambiguous 不自动重试，不用全局 danger-full-access 冒充一次性批准。

验收：完整 tool × policy × rule matrix；deny 总是胜出；Agent preset 的常规编辑/测试无
prompt、critical prompt；Full access 无动态 prompt 但 hard deny 仍阻止。

#### M27-C：execpolicy 收敛与减法

- production 和 `dse execpolicy check` 使用同一个 `dse-execpolicy` matcher；
- `crates/tools` 只合并 rule decision、Host risk 与 Run policy，不复制 prefix/path matcher；
- TUI 本地 `ProductionExecPolicySnapshot` 和 tools duplicate matcher 在 caller 迁移后删除；
- 已有 `execpolicy.toml` 只保留当前显式 allow/deny 职责，不新增 ask-rule、自定义 mode 或
  通用权限语言；
- richer policy 类型若没有 production caller，在同一 cutover 删除。

验收：同一 fixture 在 CLI check、TUI Run、exec 和 app-server 得到相同 matched rule 与
allow/ask/deny；不存在第二规则 snapshot 真相。

#### M27-D：所有真实 caller 与旧配置删除

- 不新增 `[permissions]` 或持久 Custom；TUI 默认 Ask，session selector 只影响后续新 Run；
- `dse exec --auto` 映射 Agent；非交互 critical ask 必须 fail closed，不得暗中变成
  FullAccess；`--yolo` / TUI Full access 才组成明确 full-access mode；
- root、read-only child、explicit Writer、recheck/rework 继承或收紧 exact policy，不能
  因 actor profile 扩权；
- app-server/API 不接受 UI label，只接受 typed policy；
- 旧 `approval_policy`、bool/path、`trust_mode` 和无 owner sandbox 产品字符串给出明确
  删除提示后物理删除，不保留 alias 或 migration mode。

验收：CLI/TUI/API parity、Writer worktree confinement、child non-escalation、project
config 不能产生 permission mode 的负向测试。

#### M27-E：Codex 式 TUI 选择器

- 点击 permission chip 或输入 `/permissions` 打开同一个 inline selector；
- 中文/英文只显示 Request approval、Agent decides、Full access；
- `↑/↓`、Enter、Esc 与 mouse row click 复用一个 action；
- 不加入静默 Shift-Tab 危险循环或新的模式状态机；
- active Run 只显示 frozen policy；运行中选择只影响 next Run，并明确提示；
- WorkSurface 显示 active policy，header 在 idle 显示 next-run policy；
- Full access 使用现有 warning color role，文本明确 OS permission/explicit deny 边界；
- 不提供 Custom、config editor 或高级规则入口。

验收：80-column English、CJK wide/narrow、macOS Terminal/iTerm2、keyboard-only、mouse、
resize、active/replay/terminal PTY。

### 25.4 正向收益门与停止条件

必须用同一 loopback model script 形成 deterministic 任务：

1. read/list/grep；
2. workspace edit；
3. run_tests/verifier；
4. external path；
5. network-capable command；
6. critical command；
7. explicit deny；
8. denial 后无副作用；
9. approval 后 crash/reopen；
10. root/child/Writer 不扩权。

最低保留条件：

- Ask：普通 workspace edit/test prompt 为 0，external/network/critical 精确 prompt；
- Agent：普通 edit/test prompt 为 0，Host-classified critical 精确 prompt；
- Full access：动态 prompt 为 0，explicit/hard deny 仍为 deny；
- false allow、重复副作用、replay policy drift、child escalation 均为 0；
- denied invocation 的 side effect 必须为 `NotApplied`；
- live 与 reopen 后 decision、matched rule、policy hash byte-equivalent；
- 没有第二 Runtime、Store、approval cache、config owner 或 UI-only permission truth。

若 scoped external/network authority 无法被当前 backend 真实强制，停止对应能力并缩小
文案/预设；不得以“已弹窗”或 global full access 作为通过。M27 是纯 Host/runtime 本地切片，
Key 和 official DeepSeek API request 均应为 0。

### 25.5 门禁与提交边界

依次通过：

- protocol/state migration、tools/execpolicy/runtime/app targeted tests；
- production loopback + exact crash/reopen；
- CLI/TUI/app-server parity；
- Chinese/English real PTY、mouse hitbox 与 narrow resize；
- `./scripts/dev-dse.sh focused`；
- `cargo fmt --all -- --check`；
- strict workspace Clippy；
- full workspace test；
- public repository checker 与 `git diff --check`。

M26 先形成独立 checkpoint；M27 不得把现有 M26 UI 改动、权限协议、release 或品牌混成
一个提交。M27 最终可按 A/B-C/D-E 形成少量可审查本地提交；不 push、不 release。

### 25.6 结果、删除与边界

Run API v13、RuntimeEvent v20、State schema v26 现只接受
`RunPermissionMode::{Ask, Agent, FullAccess}`。每次 executable tool 在 start 前提交唯一
`ToolAuthorizationDecision`，绑定 exact tool、arguments SHA、workspace state、risk、
matched rule 与 disposition；Ask approval 的 arguments 或 revision 在启动前变化时以
`authorization_stale` fail closed。Runtime 只排序 durable interaction/start/outcome，
最终分类仍由 `crates/tools` 拥有。

确定性矩阵结果：

```text
Ask workspace edit/test prompts             0
Ask typed path/network authority            fail closed
Agent ordinary external/network             allow
Agent Host-critical                         exact Ask
FullAccess dynamic prompts                   0
explicit deny / hard invariant bypasses      0
denied/stale side effects                    0
reopen decision drift / duplicate execution  0
child or Writer permission escalation        0
```

当前 macOS/Linux composition 没有能证明一次性外部路径或网络 grant 范围的 backend，
所以 Ask 对 canonical path-bearing tool、显式 Shell cwd 与可识别 network 调用不伪装成
可批准后执行，而是 typed deny。任意子进程内部 I/O 仍只有 OS sandbox 基线，DSE 不宣称
能从 Shell 字符串完整推导；这是 `shrink_to_enforceable_permission_subset`，不是完整
scoped-authority keep。

Cutover 已物理删除旧 bool/trust/sandbox/elevation reader、持久 permission config、
TUI 私有 execpolicy parser/snapshot、tools duplicate matcher、richer ask/session/network
policy engine、`bash_arity`、Starlark/multimap 依赖，以及无写入 owner 的
`workspace-trust.json` 外部路径 reader。绕开 canonical Run/RunStore、可自由组合
policy/network/writable roots 的 `dse-tui sandbox run` 直接执行旁路及其专属文案也已删除。
会使 TUI 忽略规则而 app-server 继续执行的 `[features].exec_policy` 分叉开关也已删除。
`execpolicy.toml` 只剩 production 与
`dse execpolicy check` 共用的 TOML allow/deny matcher；Host critical 不能被 allow rule
降级。`[projects].trust_level` 继续只拥有项目配置/MCP onboarding 信任。

`Custom`、`[permissions]`、隐藏第四模式、自由组合字段和 compatibility reader 均不存在。
旧字段字符串只保留在显式拒绝测试、State v26 一次性 retirement fixture 和 frozen 历史
证据中，不是 production reader。完整证据见
[M27 canonical permission policy](../../eval/summaries/m27-canonical-permission-policy-2026-07-26.md)。

## 26. M28：DSE 原生 TUI 全表面切换

- 状态：**完成；`keep_native_surface_and_delete_legacy`**
- cutover checkpoint：`07214e2ef`（首次真实 PTY cutover `1e420d9d7`；parity closure
  `068c8e3a9`；旧术语清理 `60df6f8f7`；最终 PTY 观测闭合 `07214e2ef`）
- 架构决策：[ADR-0013](../decisions/0013-native-tui-surface-system.md)
- 设计合同：[DESIGN.md](../../DESIGN.md)
- 目标：把所有可达 TUI surface 切到同一 terminal-native 表面系统，并物理删除
  Underwater/Ocean、通用居中 modal、显示设置矩阵和并行 palette 路径
- 范围：纯 presentation/interaction；不改变 DeepSeek、prompt、model、Thinking、tools、
  canonical permission、RuntimeEvent 语义、RunStore 或 completion owner

### 26.1 真实问题与冻结基线

M26 只解决 canonical Run 可读性，M27 只解决权限语义和选择器。M28 开始前仍存在：

- 唯一主 shell 名为 Underwater，并默认渲染 Ocean gradient、3 条鱼、气泡和 80ms
  ambient animation cadence；
- onboarding 使用居中 `Borders::ALL` card，`UserInputView` 使用约 `82% × 68%`
  居中 modal，而 approval、permission、pager 和主 shell 使用其他 chrome；
- `settings.toml` 仍把 ocean treatment、fancy animation、WorkSurface placement、theme、
  background、composer density/border、transcript spacing 和 status ornament 暴露为并列
  视觉真相；
- reachable renderer 混用 resolved `app.ui_theme` 与 direct global palette；
- Doctor 中仍有 `/constitution`、`/setup`、`/model`、`/setup tools`、
  `/setup persistence` 等退役提示，settings parse warning 仍有硬编码英文；
- 旧 feature/测试文件仍描述已删除的 session/command surface。

这些是 current baseline，不得把 M26/M27 的通过误报为“全界面已重构”。

### 26.2 固定表面与信息闭环

M28 只保留：

1. main work surface；
2. bottom sheet；
3. full-screen room；
4. transcript/composer 之间的 inline approval interruption。

主循环固定为：

```text
用户任务
  -> 当前 activity
  -> Host-observed change
  -> Host verification
  -> required action / terminal outcome
```

transcript 保持主表面。`CanonicalRunPresentation` 继续是 task、phase、change、
verification、Agent、permission、recovery 和 terminal 的唯一 presentation owner。
UI 不新增 plan、ETA、confidence、文件数、进度百分比或 completion truth。

### 26.3 垂直切片

#### M28-A：表面合同与可达矩阵

- 枚举从首次启动到 resume/reopen 的全部 production surface 和 opener；
- 冻结 English/`zh-Hans`、键盘/鼠标、宽高、颜色深度和 live/replay frame contract；
- 在现有 TUI test owner 内增加 surface reachability 与 legacy-ban 断言，不建立第二
  screenshot framework、第二 event store 或 LLM visual judge；
- 先更新 `crates/tui/AGENTS.md` 为 ADR-0013 的当前实施规则，但明确 M28 完成前旧 shell
  仍是 baseline，不得把目标写成已实现事实。

验收：每个可达 view 都有 opener、state source、target surface、退出 action 和旧路径
删除点；未知 surface 不进入重构。

#### M28-B：唯一 token 与 shell owner

- 建立一个 resolved DSE presentation token/theme owner，所有 reachable renderer 显式
  消费它；
- 以终端 background/foreground 为基础，固定 accent/success/warning/error/muted/focus
  语义，ANSI-16 与 truecolor 信息等价；
- 把 `underwater.rs` 的真实 header/footer/phase/focus 职责迁入中性 shell owner；
- 主 caller 接管后删除 `ocean.rs`、fish/bubble/ambient renderer、flee animation、
  `UI_UNDERWATER_ANIMATION_MS` 和 idle timer redraw；
- 不创建 legacy/new theme 双读、compatibility renderer 或 feature flag。

验收：idle 初帧后 5 秒无内容 frame change、无 presentation state/disk write；运行活动仍
由 canonical event 驱动并保持 streaming delta 原速。

#### M28-C：主工作表面与响应式布局

- 宽屏使用 transcript 主列 + 右侧 canonical Run rail；
- 中屏把同一摘要投影为 transcript 上方短 strip；
- 窄/矮屏使用单列，按“重复标题 -> caption -> border -> 空白 -> 次要 metric”的顺序减法；
- composer、slash menu、`@mention`、phase、terminal outcome 和 required action 保持稳定；
- 删除用户可选 left/right/top placement、composer density/border、transcript spacing 与
  status ornament；产品只保留一套默认布局；
- root/child/Writer、tool collapsed/expanded、rework 和 completion 均使用同一语法。

验收：resize 前后输入、焦点、scroll、active Run 和 canonical presentation 不漂移；不得
因窄屏先隐藏任务、当前状态、所需操作、验证或终态。

#### M28-D：所有次级表面迁移

- onboarding 变为 quiet full-screen room，保留 language/API key/project trust/Tips 的真实
  顺序、mask/paste/validation 和 back/cancel；
- `UserInputView` 变为 adaptive bottom sheet，短选择不遮蔽 transcript，长问题先内部滚动，
  超出 sheet 预算才进入同一 full-screen room；
- permission 继续使用 M27 三行 bottom sheet，不改变任何 policy 语义；
- approval 保持 inline interruption，并统一 token、focus、action rail 与 mouse hitbox；
- pager/help/log/diff/evidence 共用一个 full-screen room primitive；
- slash 与 mention menu 保持 composer-attached，不改为 command palette。

验收：不存在可达 `centered_rect` 或 generic centered modal；所有 close/cancel/confirm
恢复原 focus/scroll，键盘与 mouse 产生同一 action。

#### M28-E：设置、文案与残留减法

在 caller 审计后物理删除没有必要性的：

- `ocean_treatment`、`fancy_animations`、`work_surface_placement`；
- selectable appearance theme、`background_color`、`composer_density`、
  `composer_border`、`transcript_spacing`、`status_indicator`；
- persistent `calm_mode`、`tool_collapse_mode`、`show_tool_details` display variants；tool
  detail 改为同一默认折叠规则与 process-local row expansion，不保留第二布局模式；
- 对应 defaults、normalizer、environment branch、message、docs 和 self-proving tests；
- reachable direct palette reads、old surface helper、Underwater/Ocean 命名和临时 adapter；
- Doctor 的 retired command hints、硬编码用户英文、orphan session/command feature；
- 只证明已删除视觉模式的 Cargo/test feature 或 fixture。

必须保留真实兼容或无障碍职责：bracketed paste、synchronized output、Unicode width、
ANSI depth、terminal capability、SSH/low-motion safety和 input/tool compatibility。
`show_thinking`、`reasoning_effort`、cost currency、mention behavior 和 PDF/symlink 等
非纯外观设置只按各自真实行为 owner 审计，M28 不得借“视觉统一”改变模型可见 prompt、
reasoning route、工具行为或 workspace traversal。
若某字段同时承担真实非视觉职责，先拆到唯一 owner 再删除视觉别名，不能误删功能。

不新增 General/Appearance/Theme/Layout/Advanced settings page；没有兼容 reader、alias、
隐藏 legacy theme 或 Custom。

#### M28-F：真实 PTY cutover 与旧路径删除

- 对冻结矩阵执行 buffer/PTY、process reopen 和 installed-binary 回归；
- 确认所有 production opener 已进入新 surface；
- 物理删除最后一个 old renderer、old message、old setting、old snapshot/fixture；
- 更新 `CURRENT_CODEWHALE.md`、public repository checker 和配置参考，只记录实际结果；
- M28 形成独立 reviewable commits，不混入 Runtime、model、prompt、permission 或 release
  能力改动。

### 26.4 冻结验收矩阵

状态至少覆盖：

```text
first-run language / API key / trust / tips
idle / typing / slash / mention
thinking / model streaming / tool running
tool success / tool failure / collapsed / expanded
approval routine / elevated / critical / deny
permission Ask / Agent / FullAccess
user input short / long / cancel / invalid response
read-only child / Writer / rework
failed / blocked / cancelled / completed
pager help / cost / diff / evidence
live / resume / SQLite reopen / terminal resize
```

终端矩阵至少覆盖：

- `48×12`、`60×16`、`80×24`、`100×32`、`140×40`；
- English 与 `zh-Hans`，长路径、CJK、combining character、emoji；
- ANSI-16、truecolor、terminal reset background；
- macOS Terminal、iTerm2、Ghostty；无法自动化的宿主差异记录人工观察，不伪造通过；
- keyboard-only、mouse on/off、scroll、resize、focus restore、bracketed paste；
- ordinary、`NO_ANIMATIONS`、SSH/受限终端；
- live event 与同一 Store reopen 后的 exact presentation parity。

### 26.5 正向门、否决项与删除标准

必须同时满足：

- 每个 frame 的 task/state/change/verification/permission/terminal 与 canonical facts 一致；
- required user action 在所有尺寸和语言中可见，false progress/false success 为 0；
- 全部可达 surface 使用同一 token、focus、hitbox、action rail 和 container grammar；
- keyboard/mouse action parity 为 100%，无 mouse-only/hidden shortcut behavior；
- 所有冻结尺寸无 panic、越界、主要内容遮挡或无法退出；
- idle frame 0 周期视觉变化、0 周期 presentation 写盘；
- `en`/`zh-Hans` key/placeholder parity 100%，reachable hardcoded human text 为 0；
- production Underwater/Ocean/fish/bubble/centered generic modal/General Settings/旧视觉
  setting reader 和第二 palette owner 为 0；
- Runtime、State、protocol、DeepSeek、tools 和 permission semantic delta 为 0；
- 代码结果优先净删除旧 renderer/setting/test；若新增 owner 后旧 path 未删除，不得完成。

任一 canonical truth、审批/权限、input、resume、CJK、退出或 terminal restoration 回归均
否决 cutover。失败时在同一新 surface 上返工；不得以“暂时保留旧模式”完成 M28。

### 26.6 验证与边界

最低顺序：

```text
targeted renderer / focus / hitbox / localization tests
  -> surface reachability + legacy-ban
  -> English/zh-Hans buffer matrix
  -> real PTY keyboard/mouse/resize
  -> canonical live/reopen parity
  -> ./scripts/dev-dse.sh focused
  -> cargo fmt --all -- --check
  -> cargo clippy --workspace --all-targets --locked -- -D warnings
  -> cargo test --workspace --locked
  -> public repository checker
  -> git diff --check
```

M28 不需要 Key、official DeepSeek API 或付费 A/B；loopback model 和 canonical Store 已足够
验证 UI correctness。视觉主观印象不能替代上述门禁，PTY/snapshot 也不能被外推成模型质量、
Token、cache、成本或 verified-success 提升。

### 26.7 实际结果

M28-A–F 已按顺序完成真实 caller cutover。production 只剩 main work surface、bottom
sheet、full-screen room 与 inline approval interruption；宽屏 rail、中屏 strip 和窄屏
single-column 都读取同一个 `CanonicalRunPresentation`。onboarding、user input、
permission、approval、pager/help/cost/diff/evidence 已迁入固定容器，键盘和 mouse 共享
render-time action/hitbox。

已物理删除：

- `underwater.rs`、`ocean.rs`、鱼/气泡/渐变/80ms idle animation；
- `status_indicator.rs`、generic centered/modal/shadow helper；
- selectable theme/background pipeline、direct theme remap、显示模式与布局/装饰 setting
  reader；
- 退役 Doctor command hints、硬编码人类英文、孤儿 session/command Gherkin 与旧视觉
  evidence 文档。

冻结的 English/`zh-Hans` × `48×12`、`60×16`、`80×24`、`100×32`、`140×40`
真实 PTY resize 矩阵通过；同一 terminal Run 在 English/`zh-Hans` credential-free
SQLite reopen 后，尺寸、非空 glyph/坐标、前后景、bold/italic/underline/inverse 和
cursor 逐 cell 相等；clear/overwrite 的 terminal-default 空白编码按合同规范化。真实
onboarding/slash/mention/approval/permission mouse row、运行中 resize、ANSI depth/reset
background、Unicode width、idle 5 秒 byte-still 和 installed delivery lifecycle 均由
deterministic gate 覆盖。macOS Terminal、iTerm2、Ghostty 的宿主渲染差异没有在无对应
宿主的自动化环境中伪造人工结论。

M28 范围内没有修改 protocol、Runtime、State、DeepSeek、tools、prompt、model 或
permission 语义；没有 Key、official API、GitHub、push 或 release。完整结果见
[M28 summary](../../eval/summaries/m28-native-tui-surface-cutover-2026-07-26.md)。

## 27. M29：本地 release-candidate 真实工作流验收

- 状态：**完成；`keep_current_workflow_no_reproducible_blocker`**
- 基线：M28 clean checkpoint `548afbe3e`
- 目标：用唯一 credential-free production composition 闭合真实用户工作流，而不是把
  M24、M26、M27、M28 的分层机制通过拼接成可用性结论
- 范围：验收与缺陷修复；不改变 DeepSeek model/prompt/tool catalog、三档 permission、
  fixed actor route、RuntimeEvent/Run API/State schema 或 Host completion 语义

### 27.1 真实问题、owner 与反事实

M24 证明 exact-source 本地交付，M26 证明 canonical Run 可读，M27 证明权限闭环，M28
证明 terminal-native presentation/interaction 正确。当前仍缺少一个以用户工作流为单位的
可复现矩阵，逐项证明：

- 长任务不会因多轮 tool/model/verification 生命周期丢失终态或输入；
- verifier 失败后能在同一 Run、最新 workspace revision 上返工并完成；
- approval 的 deny/approve/reopen 都保持 exact invocation 与零重复副作用；
- read-only child 的证据由 root 汇聚，child 不能替代 root completion；
- 显式 Writer 只在 isolated worktree 写入，Host seal/verify/integrate/cleanup 恰好一次；
- terminal resume 与 SQLite reopen 恢复同一事实、焦点和唯一终态。

唯一 execution owner 仍是 `AgentApplication -> AgentRuntime -> RunStore`；底层行为分别由
现有 tools、state、orchestrator、localization 和 TUI owner 提供。M29 的 acceptance owner
是现有 Rust integration/PTY test surface，不新增 evaluator、第二 Store、第二 event
protocol、第二 renderer 或 parallel workflow controller。

旧路径不是 production feature，而是“从互不相连的机制测试推断整条工作流可用”的证据
缺口。M29 以明确 case -> production caller -> frozen acceptance -> Store evidence 映射
替代该推断；若现有用例已经覆盖，只引用并重跑真实 owner，不复制实现。

### 27.2 冻结工作流矩阵

全部 case 使用同一 source revision、locked/offline binary、loopback ChatCompletions、
临时真实 Git repository、canonical tools、fixed actor route、外部 deterministic verifier、
`maximum_reruns=0` 和独立 SQLite Store：

| workflow | 必须观察的真实闭环 |
|---|---|
| long root task | 至少两次 model turn、一次 workspace mutation、一次 Host verifier、唯一 terminal |
| verifier recovery | fail receipt -> completion rejection -> fresh correction -> latest-revision pass |
| approval deny | exact pending interaction -> deny -> tool start/side effect 为 0 -> 非成功终态 |
| approval approve/reopen | exact digest/revision 冻结；reopen 后一次 start/outcome，不重复 interaction/side effect |
| read-only child | Flash/high child、只读 catalog、typed handoff、Pro/high root 汇聚与唯一 completion |
| explicit Writer | Pro/high Writer worktree、root 不被直接写、seal/verify/integrate/cleanup 各一次 |
| RunStore reopen | 同一 run id/event prefix/request plan/accounting/evidence/terminal；无第二 request 或 terminal |
| terminal interaction | English/`zh-Hans` 的 typing/paste/resize/mouse/approval/resume，required action 始终可达 |

每个 workflow 冻结并检查：

```text
task acceptance and external verifier identity
run / attempt / actor / workspace assignment
permission and actual fixed route audit
model request count and accounting completeness
ToolPrepared / ToolExecutionStarted / ToolOutcome order
workspace revision before/after and side-effect count
child/Writer handoff and Host evidence receipt
terminal state/count and SQLite reopen equality
visible required action and terminal projection
```

### 27.3 缺陷准入与删除规则

M29 只允许修复：

1. 同一 stable owner/cause 在两个独立 workflow execution 中重复；或
2. 一个 deterministic fault-injection case 精确违反 safety、latest-revision evidence、
   exactly-once side effect、permission、reopen 或 false-success 不变量。

超时、并发调度或 PTY 采样失败必须先证明是 production defect；测试自身竞态只在现有 test
owner 内收敛，不得借机改变 production semantics。主观不顺、单次不可复现噪声、模型质量
猜测和无 treatment delta 的重构不进入代码。

若复现真实缺陷，顺序固定为 failing contract/test -> 最小唯一-owner修复 -> 真实 caller
迁移 -> 被替代路径物理删除 -> 全矩阵回归。若没有缺陷，production delta 为 0。任何临时
trace、probe、fixture repository、SQLite、PTY capture、Cargo target 和 installed artifact
都在结论前精确删除；失去消费者的 M29-only runner/test helper 也必须删除。

### 27.4 判定与门禁

硬门：

- workflow matrix 全部通过，`false_success=0`；
- latest-revision verifier、approval/deny 和 Writer isolation 无例外；
- crash/reopen 保持 event prefix、exact RequestPlan/accounting、零重复 request/side effect、
  一个且仅一个 terminal；
- root Pro/high、read-only child Flash/high、Writer Pro/high、typed recheck Pro/max 不变；
- Ask/Agent/FullAccess 与 explicit deny/hard invariant 不变；
- CLI/TUI/app-server、English/`zh-Hans` 和 keyboard/mouse 投影一致；
- focused、targeted PTY/reopen/Writer、fmt、strict workspace Clippy、workspace test、
  locked/offline delivery lifecycle、public repository checker 与 `git diff --check` 全通过。

可接受结论只有：

```text
keep_current_workflow_no_reproducible_blocker
keep_minimal_attributable_workflow_fix_and_delete_old_path
blocked_by_reproducible_local_release_workflow_defect
```

本切片不读取 Key、不调用 official DeepSeek API、不访问 GitHub、不 push、不 release，也
不形成 DeepSeek verified-success、Token、cache、费用或模型 wall-time 改善结论。

### 27.5 实际结果

8/8 冻结 workflow 全部通过。production loopback 闭合多轮 tool/verifier recovery；
Runtime 85/85、Writer orchestration 25/25、真实 Git orchestrator 44/44、State SIGKILL
38/38、app-server external process 3/3、canonical TUI PTY 7/7、Run acceptance 18/18、
双语 QA PTY 15/15、surface parity 2/2。预注册 external helper/heavy storm 仍保持各自
ignored 角色，没有 unexpected skip 或零匹配 filter。

focused、fmt、strict workspace Clippy、workspace test、public checker、delivery
self-test 全绿。clean contract commit `cf34589fb` 又构建 locked/offline exact-source
artifact，install/verify、双 binary version、English/`zh-Hans` help smoke 与 uninstall
通过。

没有 defect 满足“跨独立 execution 同因重复”或 deterministic safety invariant
counterexample 的准入条件；因此 production delta=0，没有新增 treatment/probe/runner，
也没有 old production path 可删。Key、official API、external network、GitHub、push、
release 均为 0。完整结果见
[M29 summary](../../eval/summaries/m29-local-release-workflow-2026-07-27.md)。

## 28. M30：current production dogfood loss acquisition

- 状态：**live acquisition 已按 accounting 合同停止；named-verifier ACI treatment 已正式 reject-and-delete，production 恢复零 delta**
- 基线：M29 clean checkpoint `1b7f92a97`
- owner：唯一 corrected Harness
  `scripts/eval-m9b-fixed-pro-regression.py`
- production delta：重复、可归因 current loss 出现前为 0

### 28.1 真实问题与边界

M29 已证明当前本地 production composition 的 8 条确定性 workflow 闭合，但不能外推
official DeepSeek coding quality。M23 的 20-task Hardness campaign 属于旧 revision、旧
Run API/permission contract，且在第六条轨迹 usage incomplete 后已正式停止；不能续跑、
补 mate 或把 5 个成功和 1 个独立 loss 当作 current baseline。

M30 不预设新功能。它复用 M23 已冻结的 fixture、reference solution、task scope 和
deterministic verifier，只建立新的 current identity、Run API v13 / RuntimeEvent v20 /
State v26 permission/reopen 合同和一轮 breadth-first schedule。任务事实由 M23 manifest
按 hash 继承，不复制第二任务集；历史 raw、live admission、旧 authority hash 和旧
campaign journal 均不继承。

### 28.2 冻结 acquisition

20 个独立任务各执行一次，顺序固定为 M23 position 1；每个 arm 使用 fresh Git
repository、独立 SQLite Store/verifier HOME、同一 immutable current binary、
`maximum_reruns=0`、canonical tools、external verifier 和 Host latest-revision receipt。
root 与显式 Writer 固定 Pro/high，普通 read-only child 按现有 actor profile 固定
Flash/high；这不是 Auto。普通 headless run 使用 `permission_mode=agent`；需要 durable
restart 的三个任务使用 Ask/interactive 与 canonical `request_user_input`，在 user-input
interaction commit 后 SIGKILL，reopen 后才 answer，且 reopen 本身不得增加 physical
model request。普通 workspace edit/test 在 Ask 下仍保持 prompt=0。

这是一组 current loss observations，不是 pass³、A/B 或 release quality score。每条轨迹
分别派生 behavior truth 与 accounting truth；任一 false success、identity/observer/
environment ambiguity、unknown billing、usage incomplete、unsafe evidence 或 cost ceiling
在下一 arm 前停止，不重跑。

### 28.3 候选门与删除

只有同一 `stable owner_code:loss_code` 跨至少两个不同 task id 重复，且对应 behavior、
latest revision、route/lane、reopen 和 accounting 均闭合，才允许审计一个最小
unique-owner candidate。达到阈值也不自动授权实现；必须重新写明 problem、acceptance、
owner、old path、tests 与 deletion。

若没有重复 loss，M30 以 `insufficient_repeated_current_loss` 收口。若 acquisition 因
accounting/identity 停止，则只记录精确停止事实，不从部分样本形成 aggregate。任何后续
候选未通过质量、replay 或 accounting 门，完整删除 treatment 和失去消费者的接线。

合同为
`eval/manifests/m30-dogfood-loss-acquisition-v1.json`。离线 manifest/Harness/self-test、
current production loopback、crash/reopen、route/permission parity 与全仓门禁闭合前不
读取 Key、不调用 official API；credential-front 还必须取得本 Goal 的明确授权和冻结
$0.50/arm、$10/suite 上界。全程不访问 GitHub、不 push、不 release。

### 28.4 Offline Harness checkpoint

唯一 corrected Harness 已增加 `--campaign m30`，通过 hash 读取 M23 的四段任务物料，
不复制 task corpus、verifier 或 failure classifier。M30 使用 current Run API v13
controls；20 个 start envelope 都只含
`write_execution_mode/permission_mode/interactive`，没有旧
`auto_approve/trust/sandbox/elevation` reader。三个 continuity task 的 root catalog
显式加入已有 runtime builtin `request_user_input`；这不是新工具 owner。

credential-free self-test 实际物化 20 个 fresh Git repository，证明 20/20 初始状态、
17/17 reference solution 和 3/3 safety counterexample，并覆盖 journal before/mid/
unfsynced/after-write fault window。current app-server/process-test loopback 又证明：

```text
user-input checkpoint physical requests = 1
SIGKILL + SQLite reopen requests        = 1
answer + one apply_patch + completion   = 3
interaction requested/resolved          = 1 / 1
workspace side effects                   = 1
terminal reopen exact                    = true
```

M23B 与 M9C self-test、M23 truth/hardness observer regression 均继续通过。该 checkpoint
只准入 current offline acquisition mechanism；没有 live admission、Key、official model
request 或 loss matrix。

### 28.5 Live acquisition 与正交结论

用户在冻结 `$0.50/arm`、`$10/suite`、`maximum_reruns=0` 后明确授权。独立 admission
commit `2761f1b8d` 绑定 candidate `f74804a08`、release binary、Harness、schedule、task、
authority、raw path 与 official DeepSeek 2026-07-27 review；credential 内容只在 clean
formal preflight 通过后由唯一 Harness 读取。

campaign 完整闭合 12 个 arm；第 13 个 `writer_policy_migration` 已保存 canonical
terminal、Store、SQLite reopen 与 verifier snapshot，但 accounting 为
`complete=false, usage_complete=true, billing_unknown=true`。Harness 写入
`accounting_incomplete` abort 并在第 14 arm 前停止；没有重跑、补 mate 或继续后七个任务。

只读 trajectory-report 两次 byte-identical，确认：

```text
canonical trajectories       13
completed arm results        12
behavior:
  verified success            9
  correct safety rejection    1
  verified product failure    2
  invalid observation         1
  false success               0
accounting:
  complete                   12
  billing unknown             1
full utility observations    12
```

第 13 arm 的 billing/route invalid fact 不授权 production work，也不进入成本或质量
aggregate。两个 accounting-complete 的独立 long-horizon task：

```text
rust_line_recovery_resume
rust_netstring_recovery_resume
```

均在最新 workspace 上通过 external verifier，却缺少 Host terminal receipt；stable
owner/cause 均为
`host_completion:verified_workspace_without_terminal_receipt`。因此 acquisition 结论为
`stop_incomplete_accounting`，loss 结论为 `next_candidate_audit_required`。这不是完整
20-task baseline、release quality 或自动 treatment admission。完整身份与指标见
[M30 summary](../../eval/summaries/m30-dogfood-loss-acquisition-2026-07-27.md)。

### 28.6 已执行候选：named-verifier ACI

1. **真实问题**：两个 loss task 的 canonical 时间线共出现九次 contract-bound verifier
   调用，全部在执行前被 named-verifier binding 拒绝；最终 Host exact verifier 通过，
   但没有 fail→mutation→pass lineage，所以 Stop Gate 正确返回
   `EvidenceLineageUnavailable`。
2. **验收**：只消除 model-visible TaskContract acceptance identity 与 tool identity
   歧义；保持 Host 展开 exact parameters、latest-revision receipt、false success=0、
   replay/crash/accounting 完整。两个独立同任务 fixed-Pro/high、零重跑 treatment 都必须
   形成完整 failed-write-pass receipt。
3. **唯一 owner**：`crates/runtime` 已有 named-verifier tool specialization/binding。
4. **替代旧路径**：删除把 acceptance identity 与 tool identity 同称为 verifier ID 的
   模糊 ACI；不增加模型可见同义工具、不接受 raw verifier 参数覆盖。
5. **证据**：先写 schema、binding、authorization、Store replay、
   failed-write-pass/continuity 失败测试；再走同一 production composition 的两任务
   vertical treatment。当前 acquisition raw 只作 control fact，不重跑。
6. **cutover/deletion**：若 treatment 通过，删除模糊描述与失去消费者的测试；若任一
   behavior、receipt、false-success、replay 或 accounting 门失败，完整删除 treatment，
   保留当前 Stop Gate 与 summary。

### 28.7 Treatment 结果与删除

description-only candidate `d0d509b6e` 只把 model-visible `verifier_id` 说明收敛为
TaskContract acceptance ID，并明确它不是 `run_verifiers` 工具名；schema enum、exact
resolver、Host parameter expansion、RuntimeEvent、State 和 Store replay 全部未改。
targeted red/green、focused、strict Clippy、workspace test、root/read-only/Writer、
app-server/exec SIGKILL、surface parity 与双语 PTY 均通过。独立 treatment manifest 与
admission 冻结两个原 loss task、同 fixed Pro/high、同 TaskContract/fixture/verifier、
Ask continuity、`maximum_reruns=0`、`$0.50/arm` 和 `$1.00/suite`。

正式两臂都完成 accounting、external verifier、canonical Store、SQLite reopen 和
continuity，但都没有 Host terminal receipt：

```text
verified success             0 / 2
verified product failure     2 / 2
false success                0
behavior owner/cause         host_completion:
                             verified_workspace_without_terminal_receipt
accounting complete          2 / 2
physical requests            35
known cost                   USD 0.061425364
decision                     reject_and_delete_named_verifier_aci
```

本次结构化时间线把 acquisition 审计继续向前推进了一层：18/18 个 treatment
`run_verifiers` 调用都只带 frozen acceptance ID，说明 ACI 歧义已被消除；但 18/18 又在
operation 启动前被 current Host permission policy fail-closed，因为执行后端不能证明
一次性 external-path authority。不能把这个第二 owner/cause 静默塞进同一变量
treatment，也不能通过 FullAccess、兼容别名、raw 参数、第二 verifier path 或弱化
`failed_write_pass` 取得成功。

因此按预注册 gate：

- ACI description 与其测试已从 production 物理删除；
- `m30t` 临时 Harness campaign 已删除，唯一 corrected Harness 恢复 acquisition 前
  SHA-256；
- frozen treatment manifest、live admission、ignored 0600 hash-chain raw、Git 历史和
  decision summary 保留审计；
- production named-verifier schema/resolver、permission policy、Stop Gate、Runtime 和
  Store 保持原样。

M30 最终以 `reject_and_delete_named_verifier_aci` 收口，不继续开发第二候选。后续只有
新的独立 Goal 先把“contract-bound Host verifier 在 fixed permission profile 下如何
获得可证明的最小执行 authority”冻结为 typed owner/cause，并证明不扩大 ordinary tool
authority，才允许审计 permission/verifier integration；本次 raw 不授权重跑或叠加修复。
完整结果见
[M30 named-verifier ACI treatment](../../eval/summaries/m30-named-verifier-aci-treatment-2026-07-27.md)。

## 29. M31：contract-bound Host verifier 最小执行授权

- 状态：**complete；`keep_contract_verifier_grant`**
- 基线：M30 clean checkpoint `401545971`
- owner：`crates/runtime` 派生并重放 exact grant，`crates/tools` 作最终授权与 sandbox
  判定，`crates/protocol`/`crates/state` 只承载唯一 typed durable fact
- production delta：candidate `dbfbb8a58` 的 typed exact grant 已由两任务 live gate
  保留；临时 M31 Harness consumer 已删除

### 29.1 可重复 production defect

M30 的两个独立 Ask/interactive continuity task 中，18/18 次
`run_verifiers` 调用都已使用正确的 TaskContract acceptance ID，并由 Runtime 展开为
同一份冻结参数；它们仍在 `ToolExecutionStarted` 前被
`ask_external_path_fail_closed` 拒绝。随后终态 Host verification 在同一 Ask Run、
同一 workspace/no-network sandbox 中启动并成功执行相同的
`/usr/bin/python3` verifier。失败因此不是模型 ACI、verifier 本身或 Stop Gate defect，
而是通用 external-path 分类把 Host 冻结的 executable program 当成模型选择的外部路径。

M31 不把 Ask 改成 FullAccess，也不开放 ordinary external path。唯一候选是
Runtime 从 exact acceptance-ID handle 与 frozen `VerifierSpec` 派生一个不可由模型请求
的 typed execution grant；tools 重新解析 canonical spec 并只允许该 exact
`run_verifiers` executable program 使用 Host 已冻结的 read/execute authority。external
cwd、普通 read/edit/shell path、network、explicit deny、hard invariant、read-only child
目录和 isolated Writer sandbox 必须保持原边界。

### 29.2 冻结顺序与删除门

合同与 16-case permission matrix、4 个 crash/reopen window 冻结在
`eval/manifests/m31-contract-verifier-permission-v1.json` 和
`eval/fixtures/m31-contract-verifier-permission-v1.json`。实现顺序为：

1. protocol digest/grant contract 与 forged/spec-drift 负向测试；
2. Runtime exact derivation、ToolPrepared 持久事实与 Store replay；
3. tools 最终授权，只删除 exact verifier program 的 generic denial；
4. root/read-only/Writer、Ask interactive/headless、sandbox、SQLite 与 SIGKILL 门禁；
5. 只有 offline 全绿后，才建立新的 live admission，冻结相同两项 fixed-Pro/high
   treatment、`maximum_reruns=0`、授权与费用上界。

两项原任务必须都形成 latest-revision failed→write→pass receipt，false success=0、
accounting/reopen 完整，候选才可保留。任一 ordinary authority 扩张、scope/replay/
evidence/accounting 失败或任一 task 未 verified success，typed grant 与所有候选接线
完整删除。当前合同阶段不读 Key、不请求 official API、不访问 GitHub、不 push、不
release。

### 29.3 Offline candidate checkpoint

candidate `dbfbb8a58` 把完整 `VerifierSpec` canonical digest、typed
`ToolExecutionGrant::TaskContractVerifier`、Runtime exact derivation、tools final
authorization 与 State v27 retirement/reopen 接入唯一 production 链。generic external
path 只在 canonical spec digest 完全一致时忽略 verifier `commands[].program`；普通 raw
参数、错误 digest、spec drift、external cwd、普通外部路径、network 与现有 sandbox
边界不变。Run API / RuntimeEvent / State 当前 identity 为 v14 / v21 / v27。

16-case matrix、Ask interactive/headless production loopback、failed→write→pass temporal
receipt、SQLite exact reopen、prepared/authorized/in-flight/outcome-committed 四个 SIGKILL
窗口、root/read-only/Writer、CLI/TUI/app-server、双语 PTY、focused、fmt、strict
workspace Clippy/test、public checker 与 diff check 全绿。该 checkpoint 未读取 Key、未
请求 official API；后续 live acquisition 只使用 immutable `dbfbb8a58` binary 与独立
M31 admission，并按专属授权各执行两个原任务一次。

### 29.4 Live keep 与 cutover

专属 admission commit `b2d0c21fd` 冻结两项 fixed-Pro/high、Ask/interactive continuity
treatment，`maximum_reruns=0`、`$0.50/arm`、`$1.00/suite`。正式 Harness 正常退出 0：

- 两项均为 verified success，false success=0；
- 两项都形成 verifier fail→workspace mutation→latest-revision pass receipt；
- 两次 interaction commit 后 SIGKILL 均从 exact event prefix 恢复，终态 SQLite reopen
  与 Store snapshot 一致，额外 approval 为 0；
- accounting 2/2 complete，17 requests、254,746 input tokens、14,443 output tokens、
  known cost `$0.030561824`；
- raw 是 21-record、无 partial tail、`0600` hash-chain journal，SHA-256
  `efcfe2846952d13416910a9980b7b37a88e7ec1480587db69f1960fb1b0c5570`。

结论为 `keep_contract_verifier_grant`。保留 exact grant、digest、Runtime derivation、
tools validation 与 State v27 retirement；删除临时 `--campaign m31` loader、aggregate、
preflight、continuity/live runner 分支，corrected Harness 恢复 pre-M31 blob
`90ffb72bbd830ec7e1e66c685768bea37b37e0c3`。frozen contract/treatment/admission、
ignored `0600` raw 与 summary 保留作审计证据。完整结论见
[M31 contract-bound verifier permission treatment](../../eval/summaries/m31-contract-verifier-permission-2026-07-27.md)。

## 30. M32：current Hardness 全量回归与剩余损失门

- 状态：**complete；stop_incomplete_accounting / no treatment admitted**
- 基线：M31 clean checkpoint `bfa8ec84f`
- owner：唯一 corrected `scripts/eval-m9b-fixed-pro-regression.py` Harness
- production delta：control acquisition 阶段为 0

### 30.1 真实问题与验收

M31 只在两个原 loss task 上证明 typed exact TaskContract verifier grant 修复有效；
它没有证明该 production delta 在完整 current Hardness 分层上无回归，也没有定位修复后
仍然重复的 current loss。M30 的旧 revision、停止位置、raw 和 12-arm partial result
不能续跑、补 mate 或冒充 M32 baseline。

M32 通过 hash 继承 M23 冻结的 20 个 task/fixture/reference/tool-policy material，
但冻结全新的 Run API v14 / RuntimeEvent v21 / State v27 / exec-stream v4 source
identity。正式 schedule 为每个 task 一个新的 position-1 arm，共 20 arms；
fixed Pro/high、`maximum_reruns=0`、transport/runtime retry 0。普通 arm 使用 current
Agent permission；三个 long-horizon arm 使用 Ask + typed `request_user_input`，
在 interaction durable 后 SIGKILL，并从同一 RunStore exact reopen、answer、continue。

完整验收为：

1. 20/20 initial verifier 按合同失败，17/17 reference solution 通过，3/3 safety
   counterexample 保持失败；
2. root、ordinary read-only child、explicit Writer、safety 和三种 continuity actor
   profile 与当前 production composition 一致；
3. behavior truth 与 accounting truth 正交；latest-revision receipt、external verifier、
   route/lane、SQLite reopen 和 crash prefix 全部闭合；
4. false success 为 0，任何 unknown billing、usage incomplete、identity/observer/
   environment ambiguity、unsafe evidence 或 ceiling breach 都在下一 arm 前停止；
5. 只有同一 `owner_code:loss_code` 跨至少两个不同 task id 重复，才允许审计一个最小
   unique-owner treatment；否则结论为 `no_repeated_current_loss` 并停止功能开发。

### 30.2 顺序、授权与删除

先提交 contract/manifest，再让唯一 Harness 增加一个可删除的 M32 consumer；完成
fixture/reference、journal crash windows、deterministic loopback、真实 process
SIGKILL/reopen、dry-run、source identity 和全仓离线门禁。credential-front 必须另行冻结
immutable binary、Harness/schedule/task/authority hash、ignored `0600` raw 路径、当前
明确授权、`$0.50/arm` 与 `$10.00/suite` 上界后才可读取 Key 或请求 official API。

若 20 arms 完整闭合且没有重复 current loss，保留 manifest/result/summary 身份，删除
M32 临时 live consumer，不开发 production treatment。若出现重复 loss，只允许按
problem/acceptance/owner/old-path/tests/deletion 新开一个单变量 candidate；失败即物理
删除 candidate 与临时接线。历史 M23/M30/M31 evidence 不修改。全程不访问 GitHub、
不 push、不 release。

### 30.3 正式结果与收口

用户在 immutable candidate、`$0.50/arm`、`$10/suite` 和 `maximum_reruns=0`
冻结后明确授权。formal campaign 闭合 6 个 accounting-complete arm：5 个 verified
success、1 个 `root_task_outcome:deterministic_verifier_failed`、false success 0。
第 7 个 `writer_envelope` 的 root 首请求在 response headers 前得到 typed
`deepseek_timeout`；terminal、Store、credential-free reopen 与 verifier facts 均保存，
但 accounting 为
`complete=false, usage_complete=true, billing_unknown=true, sealed=true`。

Harness 按合同在第 8 arm 前写入 `accounting_incomplete` abort；没有重发、续跑后十三项
或拼接历史 raw。只读 report 两次 byte-identical，确认 7 个 canonical trajectory、
6 个 complete accounting observation、1 个 billing-unknown/route-invalid observation
和 6 个 full-utility observation。唯一闭合 loss 只出现在
`rust_router_localization`，未达到两个独立 task 的阈值，因此 loss gate 为
`insufficient_repeated_current_loss`，不准入 treatment。

这不是完整 20-task regression，也不能宣称 `no_repeated_current_loss` 或 M31 broad
regression-safe。M32 以 `stop_incomplete_accounting` 收口；fixed actor route、M31 exact
grant、唯一 Runtime/Store、canonical tools 与 production prompt 均不改变。临时 M32
Harness consumer 已物理删除并恢复到 M32 前 blob `90ffb72b`，frozen
contract/admission/analysis、ignored 0600 raw 与 summary 保留。完整事实见
[M32 current Hardness regression](../../eval/summaries/m32-hardness-regression-2026-07-27.md)。

## 31. M33：模型请求超时恢复与唯一重试闭环

- 状态：**completed；keep_runtime_owned_model_retry_loop**
- 基线：M32 clean checkpoint `e5df72e78`
- 唯一决策 owner：`crates/runtime::AgentRuntime`
- typed failure/accounting owner：`crates/deepseek`
- durable truth owner：现有 canonical `RuntimeEvent` / `RunStore`
- production surface：CLI、TUI、app-server 只投影同一 stored retry fact

### 31.1 真实问题与可测验收

M32 的 `transport/runtime retry=0` 与 `maximum_reruns=0` 是 position-1 正式采集合同，
不是正常产品策略。正常 `RunLimits` 已默认 `max_model_retries=2`，但 current production
同时留下两套互相漂移的控制面：

1. `DeepSeekModelPort` 构造时总是关闭 transport retry，真正的安全重试由 Runtime
   `plan_model_failure` 决定并与 failed attempt 原子提交；
2. app/TUI/config/CLI 仍暴露 transport 默认 3 次、`[retry]`、
   `--transport-max-retries`、1s/2x/60s 与 execution fingerprint 字段；
3. Runtime 的 canonical retry 目前在 durable decision 后立即再次发送，没有可重开
   `not-before`，用户表面只显示 opaque attempt id；
4. DeepSeek 已解析 HTTP `Retry-After`，但没有把 hint 带到 Runtime。

M33 只建立一个有界、可观察、可恢复的 Host 闭环：

```text
DeepSeek typed failure / response evidence / usage / optional Retry-After
  -> AgentRuntime retryable + replay-safe + no-actionable-output + limit/budget
  -> failed attempt + prepared retry + backoff/not-before atomic RuntimeEvent
  -> RunStore exact replay
  -> wait remaining time
  -> exactly one next physical attempt
```

默认仍是初次请求加最多两次 Runtime retry。第一次/第二次固定退避 1s/2s；
`Retry-After` 只在 HTTP response 实际携带时作为不短于本地退避的 typed hint，等待受既有
Run deadline 约束。没有设置页、无限重连、transport 内循环、语言/模型分类或第二
controller。

### 31.2 安全边界

- 只有 response headers 前的 timeout/network，或无 content/reasoning/tool/usage/finish
  actionable evidence 的 429、可重试 5xx，才可进入有界自动重试；
- 401/403、普通 4xx、协议错误、failure cause 改变、预算/次数耗尽必须停止；
- 任意 content、reasoning、tool-call fragment、usage、finish、`[DONE]` 或 incomplete
  stream 的 replay-unsafe 证据都禁止盲目重发；
- retry decision 已提交但尚未发送时，SIGKILL/reopen 必须按 persisted not-before 等待并
  只发送一次；retry 已 in-flight 时仍返回 canonical `RecoveryRequired`，不猜测上游是否
  执行；
- app-server sequence reconnect 只重放本地 stored events；same-Run resume 只恢复
  canonical run。二者都不是 DeepSeek partial SSE continuation。

官方 DeepSeek 2026-07-27 文档确认 429 是并发/限速错误，500/503 建议短暂等待后重试，
streaming 以 `[DONE]` 结束并可能发送 keep-alive；官方没有承诺 `Retry-After` 或 partial
stream continuation。实现只能消费实际 response header，不能扩大官方保证：

- https://api-docs.deepseek.com/quick_start/rate_limit/
- https://api-docs.deepseek.com/quick_start/error_codes/
- https://api-docs.deepseek.com/api/create-chat-completion

### 31.3 冻结离线矩阵

优先 loopback 与 fake clock，不使用真实 sleep：

1. pre-header timeout → 1s backoff → success：一个 logical request、两个 physical
   attempts、一个 canonical retry；
2. network reset → 1s/2s → success：初次 + 两次，request/accounting 精确；
3. 429 + Retry-After → success：Runtime 等待 typed hint，transport 不重发；
4. 500/503 有界重试；401/403/普通 4xx不重试；
5. partial content/reasoning/tool fragment 或 usage 后 stall/incomplete：
   physical attempt=1，Stop(ActionableOutput/UnsafeReplay)；
6. retry decision committed、send 前 SIGKILL：SQLite reopen 后等待剩余时间并发送一次；
7. retry in-flight SIGKILL：`RecoveryRequired(ModelRequest)`，零盲发；
8. retry limit/request budget/deadline 耗尽：明确 terminal，false progress/success=0；
9. root、普通 read-only child、explicit Writer 共用同一 conformance；
10. CLI/TUI/app-server 与 en/zh-Hans 投影同一 retry ordinal/total/backoff/stop reason；
11. physical started/completed/in-flight、runtime retry、usage completeness、
    billing unknown 与 reopen 前后一致。

### 31.4 实施、旧路删除与 keep gate

顺序固定为：

1. protocol/Runtime/Store 失败测试，先暴露立即重试和缺失 not-before；
2. DeepSeek 传递 typed `Retry-After`，Runtime 生成唯一 backoff decision；
3. State migration、prepared/in-flight crash/reopen 与 fake-clock loopback；
4. 真实 app/CLI/TUI/app-server caller 和双语 projection；
5. 删除 `TransportRetryPolicy` 自动循环、`with_retries_disabled`、
   `[retry]` reader/example/reference、`--transport-max-retries`、production retry
   fingerprint 与只验证旧双层路径的测试；
6. targeted、focused、fmt、strict Clippy、workspace test、process SIGKILL/reopen、
   surface parity、public checker 和 diff check。

只有安全 timeout/network/429/5xx 能自动恢复、partial output 不重发、crash/reopen
不重复、surface/accounting 一致且 active code 只剩一个 controller 时，结论才是
`keep_runtime_owned_model_retry_loop`。任一 exactly-once、evidence、billing 或 replay
门失败，删除 M33 candidate 并保留当前 fail-closed 语义。M33 不修改或续跑 M32 frozen
evidence、不访问 GitHub、不 push、不 release；credentialed canary 只有在用户重新明确
授权后才可运行。

### 31.5 结果、删除与非结论

M33 结论为 `keep_runtime_owned_model_retry_loop`。Run API v15、RuntimeEvent v22、
State v28 与 exec-stream v5 已硬切换；DeepSeek transport 每次调用只发送一个物理
request，Runtime 原子持久化 retry decision、attempt、1s/2s backoff 与 not-before，
实际 `Retry-After` 的 delay-seconds/HTTP-date 只能延长等待。fake clock、loopback 与
process SIGKILL/reopen 证明 prepared retry 重开后只发送一次，in-flight retry 重开仍
fail closed；root、普通只读 child、Writer、CLI、TUI、app-server、en/zh-Hans 与
accounting 使用同一 stored fact。任何 content/reasoning/tool/usage/finish evidence
仍禁止盲重发，false success/progress 为 0。

`TransportRetryPolicy`、transport sleep/loop、`with_retries_disabled`、`[retry]`、
`--transport-max-retries`、production retry fingerprint、transport retry accounting
及 active Harness 对失效 flag 的依赖已物理删除。历史 evaluator 与 frozen evidence 中
的旧字段仅作为当时事实保留，不是 current consumer。

离线门禁闭合后，用户明确授权使用测试 Key。一个同源 release
`dse`/`dse-tui`、official DeepSeek Standard Chat、Pro/high canary 成功返回
`M33_LIVE_OK`：physical started/completed/in-flight=`1/1/0`、runtime retry=0、
usage/cost complete、billing unknown=0、2811 input、38 output、2849 total、
USD 0.000979765、1611ms。第一次 delivery preflight 因缺少 sibling `dse-tui`
在网络前停止，官方请求数为 0；补齐同源 companion 后才执行上述唯一请求。

真实 canary 只证明普通成功路径和 accounting，没有人为制造远端故障，也不替代
deterministic retry safety matrix。DSE 没有 DeepSeek partial SSE continuation 协议；
app-server sequence reconnect、same-Run resume 与 upstream retry 仍是三种不同机制。
完整结论见
[M33 Runtime-owned model retry](../../eval/summaries/m33-runtime-owned-model-retry-2026-07-27.md)。

## 32. M34：模型故障反馈与恢复体验闭环

- 状态：**completed；keep_minimal_model_failure_feedback**
- 基线：M33 clean checkpoint `408611fb1`
- 唯一重试决策 owner：既有 `crates/runtime::AgentRuntime`
- 人类投影 owner：`crates/localization` + `crates/tui` / `crates/cli`
- 故障注入 owner：test-only controlled loopback proxy；不进入 production

### 32.1 真实问题与可测验收

M33 已证明安全 transient failure 可以由唯一 Runtime 闭环自动恢复，也证明 partial
output 与 in-flight crash 不得盲重发。它还没有用真实客户端回答：用户是否能持续看见
失败种类、正在执行第几次重试、还要等待多久、为什么停止，以及停止后应使用哪一个既有
canonical 操作。

M34 不改变 retry policy、次数、backoff 或 replay gate。它先用受控代理向同一个
production DeepSeek sender 注入 response-before-headers timeout、connection reset、
429 + `Retry-After`、503、partial SSE close 和 process crash，再由真实 CLI/TUI/
app-server 读取 canonical stored events。体验验收使用确定性 information rubric，而
不是 LLM judge：

1. transient failure 明确显示 DeepSeek、稳定失败类别、自动动作、当前重试序号/总数和
   等待时间；
2. retry 成功后回到正常进度，不留下 false failure 或重复 terminal；
3. limit、永久 4xx、partial output、unsafe replay 和 crash recovery 的最终原因在
   terminal 后仍可取得，并指向既有的 new task / same-Run resume / fail-closed
   `RecoveryRequired` 之一；
4. en / zh-Hans 使用同一 `MessageId` 和相同 placeholder 集，窄终端不遮蔽关键事实；
5. app-server event sequence reconnect 只重放 Store，same-Run resume 只恢复 Run；
   二者都不得描述为 DeepSeek partial stream continuation；
6. physical attempt、Runtime retry、usage/accounting、billing unknown、event prefix 与
   SQLite reopen 前后一致，false progress / false success 为 0。

### 32.2 外部依据与产品取舍

2026-07-27 复核的官方/一手资料边界：

- DeepSeek 把 429、500、503 视为可短暂等待后重试的 transient failure；400/401/422
  是修正请求或凭据的永久错误；Chat streaming 以 `[DONE]` 结束但没有 partial stream
  continuation contract：
  <https://api-docs.deepseek.com/quick_start/error_codes/>、
  <https://api-docs.deepseek.com/quick_start/rate_limit/>、
  <https://api-docs.deepseek.com/api/create-chat-completion>；
- AWS / Google 要求有界 exponential backoff、单一 retry layer、幂等/replay safety
  gate，并观察 error、attempt、delay 和 final outcome；不能因 timeout 就假定上游未执行：
  <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/>、
  <https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/>、
  <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/retry-strategy>；
- Codex 把执行进度输出到 stderr，并把 saved-session resume 与 transport failure 分开；
  Claude Code 的 `--resume` / `--continue` 同样是会话恢复，不是 token stream 续传：
  <https://developers.openai.com/codex/codex-manual.md>、
  <https://docs.anthropic.com/en/docs/claude-code/cli-usage>。

DSE 只吸收这些机制，不复制 Provider、第二 controller、设置页或产品词汇。当前单本地
Runtime 没有测得并发惊群，因此保持 M33 可重放的确定性 1s/2s，不凭云端通用建议加入
随机 jitter。

### 32.3 顺序、删除与 keep gate

先冻结
`eval/manifests/m34-model-failure-feedback-v1.json` 与对应 fault fixture；随后：

1. test-only proxy + 真实 binary 基线，保存每个 scenario 的 server attempts、
   RuntimeEvent、SQLite/accounting 与 en/zh-Hans surface observation；
2. 只把跨至少两个独立 fault profile 重复的 information loss 定义为 production defect；
3. 最小修改既有人类投影，不新增 protocol/state truth；transient status 保持紧凑，
   terminal stop fact 必须可持续取得；
4. 重跑相同 frozen profiles，并完成 process SIGKILL/reopen、surface parity、focused、
   fmt、strict Clippy、workspace test、public checker 与 diff check；
5. treatment 接管后删除被替代的 silent/ephemeral projection 及失去消费者的测试 helper；
   若没有重复损失或 treatment 不通过，删除 treatment，只保留可复现 baseline 与结论。

只允许 `keep_minimal_model_failure_feedback`、
`keep_existing_surface_no_repeated_loss` 或
`reject_and_delete_feedback_candidate`。M34 不需要故意攻击官方 DeepSeek 服务；如运行
普通 official canary，只能验证成功路径，不能替代 controlled fault evidence。全程不
访问 GitHub、不 push、不 release。

### 32.4 结果

M34 结论为 `keep_minimal_model_failure_feedback`。真实 production binary 的 frozen
loopback profiles 覆盖 response-header timeout、两次 reset、429 + `Retry-After`、
连续 503、401 与 partial SSE close；M33 的 fake-clock、prepared-retry SIGKILL/reopen、
in-flight fail-closed 和三 actor conformance 同时复验。所有 server attempt 与 canonical
physical/runtime-retry accounting 一致，partial/in-flight duplicate request=0，
false progress/success=0/0。

control 的重复缺口是客户端信息丢失：plain exec 静默、stream-json 缺逐次 failure、
TUI terminal 覆盖 stop reason。cutover 后，plain exec 用 stderr 输出紧凑的
provider/category/ordinal/wait，stdout 保持模型内容；TUI en/zh-Hans 在 12×48 仍显示
typed retry warning，并把 stop reason/next action 留在 history；exec-stream v6 用 bounded
`model_request_failed` 事件投影 stored failure、retry/stop 与紧凑 accounting，不复制
完整 prompt/transcript/tool catalog。RuntimeEvent v22、State v28、M33 retry policy 与
app-server exact stored-event replay 均未改变。

第一次 full stored-event exec projection 和会截断 ordinal/wait 的长 status 候选已删除；
旧 silent/ephemeral branches 已被真实 caller 替代。30/30 exec terminal、16/16 双语 PTY、
focused、fmt、strict Clippy、workspace test、public checker 与 diff check 通过。没有读取
Key、没有 official API request、没有 GitHub/push/release。完整证据见
[M34 summary](../../eval/summaries/m34-model-failure-feedback-2026-07-27.md)。

## 33. M35：官方 DeepSeek production reliability soak

- 状态：**complete；`keep_current_retry_no_new_treatment`**
- 基线：M34 clean checkpoint `39da745f5`
- production retry owner：既有 `crates/runtime::AgentRuntime`
- typed transport/accounting owner：既有 `crates/deepseek`
- acquisition owner：唯一 corrected `scripts/eval-m9b-fixed-pro-regression.py` Harness

### 33.1 真实问题与外部边界

M33 已用 fake clock、loopback 和进程故障证明唯一 Runtime retry 的安全性，M34 已用
受控代理证明同一 stored failure fact 能被 CLI/TUI/app-server 准确投影；但官方路径只有
一个普通成功 canary。当前仍缺真实独立 Run 上的延迟分布、自然 timeout/network/
429/5xx/partial incidence、Runtime retry 恢复率和用户状态完整性。没有这些样本，不应
凭云端通用文章继续调整次数、backoff、jitter 或 UI。

2026-07-27 复核官方事实：唯一 production endpoint 仍是
`https://api.deepseek.com/chat/completions`，模型为 `deepseek-v4-pro`，stream 以
`[DONE]` 结束且可能包含 `: keep-alive`；429/500/503 可短暂等待后重试，400/401/402/422
需修正请求或账户；官方没有承诺 `Retry-After`、request-level billing reconciliation 或
partial stream continuation。AWS/Google 的成熟原则只作为机制约束：单一 retry layer、
bounded backoff/deadline、retryability 与 replay safety 分离、attempt/final outcome
可观察。Codex/Claude Code 的 saved-session resume 也不能解释为上游 token stream 续传。

### 33.2 冻结 acquisition

冻结 manifest：
[`m35-official-reliability-soak-v1.json`](../../eval/manifests/m35-official-reliability-soak-v1.json)。
同一个 immutable `dse`/`dse-tui`、Pro/high、official Standard Chat、prompt/tool policy、
预算和 observer 下交错 6 轮：

1. tool-less marker Chat；
2. `read_file` 单工具读取；
3. `grep_files -> read_file` 定位；
4. `read_file -> edit_file` 受限 workspace 修改。

共 24 个全新独立 Run，每个使用新 Git repo、HOME、State 和 RunStore。正常产品
`max_model_retries=2` 保持开启；`maximum_harness_reruns=0`，不得补跑 logical Run。
逐 Run 保存 physical attempts、Runtime retry、typed failure/backoff/stop、terminal、
外部 verifier、usage/cache/cost、raw mode 与 credential-free SQLite reopen。任何
`billing_unknown`、usage incomplete、identity/evidence/observer ambiguity、unsafe
partial 或 in-flight 未闭合，都在下一付费 Run 前停止。

### 33.3 treatment 与删除门

只有同一 `owner_code:loss_code` 跨至少两个不同 task profile 或两个独立轮次重复，才准入
一个最小 production treatment；否则结论是 `no_repeated_live_reliability_loss`，不改
production。自然 soak 不故意攻击、并发压测或制造官方 429/5xx。

若准入 treatment，必须从新的 immutable identity 和 position 1 successor 复测；不得续跑、
覆盖或拼接 M35 v1 raw。候选必须保持 false success=0、partial/in-flight 零盲发、
exact reopen/accounting 与唯一 Runtime retry owner，否则完整删除。正式决策后删除
临时 M35 Harness consumer；manifest、ignored `0600` raw 和 summary 保留审计。

### 33.4 正式结果与删除

第一次 formal v1 因临时 Harness 把全局 `--model` 放在 `exec` 后而在网络前停止：
completed Run=0、official request=0；3-record `0600` journal 保持 immutable。修正
`6202bb0d7` 增加真实 production argv self-test 与 typed `m35_exec_no_stream` 后，一个
独立 canary 以 1 physical、0 retry、usage/cost complete 成功；正式 acquisition 再从
新 binary、新 output 和 position 1 开始，没有补跑或拼接 v1。

candidate `6202bb0d7` 的 24 个独立 official Pro/high Runs 全部 verified：

```text
verified success             24/24
false success                 0
physical started/completed   54/54
physical in-flight            0
model failure events          0
Runtime retries               0
usage/cost complete          24/24
billing unknown               0
credential-free reopen       24/24 exact
TTFR median/p95          2,020/2,380 ms
wall median/p95          4,796/7,428 ms
input/output tokens     106,121/4,113
known cost             $0.014071409
```

工具任务的 2–3 个 physical attempts 是正常 tool loop，不是 retry；canonical
`runtime_retry_count` 保持 0。没有同一 live `owner_code:loss_code` 跨 profile/round
重复，因此决定为 `no_repeated_live_reliability_loss`，production delta=0。没有凭通用
云端建议增加 jitter、次数、circuit breaker 或第二控制器。临时 M35 Harness consumer
已物理删除并恢复 M35 前 exact blob `5f3f613c`；保留 frozen contract、live admission、
两个 ignored `0600` journal、summary 与 Git 历史。完整证据见
[M35 summary](../../eval/summaries/m35-official-reliability-soak-2026-07-27.md)。

## 34. M36：DeepSeek-native verified harness 优化计划

- 状态：**M36-A fresh official acquisition 已完成；决定
  `keep_current_harness_no_repeated_loss`，production delta=0**
- 起始基线：M35 clean checkpoint `e84abb1ed`
- 产品边界：继续执行 PRODUCT_PLAN、ADR-0001/0002/0003/0005/0008/0011；不改变
  DeepSeek-only、单 `AgentRuntime`、单 `RuntimeEvent`、单 `RunStore`、单 Writer 默认和
  Host latest-revision completion
- 北极星：`verified task success / tokens / time / code complexity`

### 34.1 结论与方法

M36 不把 Codex、Claude Code、Gemini CLI、Aider、SWE-agent、OpenHands、Pi/Oh My Pi 或
其他 Agent 的功能清单当作 DSE backlog。外部系统只用于回答三个问题：

1. 它解决的是哪一种可复现任务损失；
2. 有效机制能否进入 DSE 已有因果链中的唯一 owner；
3. 在相同 DeepSeek 模型、任务、预算和 verifier 下，是否产生 held-out 正收益。

DSE 的唯一 Harness 闭环保持：

```text
TaskContract
  -> ContextBundle
  -> DeepSeek-native AgentRuntime
  -> ToolOutcome
  -> EvidenceReceipt(latest workspace revision)
  -> TerminalState / typed recovery
```

`RunStore` 是以上事实的唯一持久真相。任何计划、里程碑、多 Agent、TUI 摘要和评测结果都
只能由这条链投影或编排；不得建立模型自由维护的第二 plan、memory、progress、completion
或 retry controller。

前沿研究提供两个直接约束：

- DeepSeek V4 官方 Coding Agent 评测使用的是以 Bash 和文件编辑为核心的 minimal harness，
  不是多 Provider、多 planner 或大工具市场；DSE 应优先提高少量工具和 Host 反馈质量；
- Claw-SWE-Bench 在固定模型下观察到 Harness 差异与模型差异都可显著改变 Pass@1，说明
  Harness 值得深度优化，但必须把模型选择与 Harness 贡献分开测量，不能用竞品整机成绩
  代替 DSE 工程归因。

### 34.2 DeepSeek 原生定义

“原生”指准确利用官方 DeepSeek 行为，不指复制任一兼容客户端的产品结构：

1. production 继续只走官方 DeepSeek ChatCompletions Standard/Strict surface；公开 wire
   使用 OpenAI-style Chat 格式不等于增加 OpenAI Provider；
2. tool-call 回合的 assistant `reasoning_content`、`content`、`tool_calls` 与 tool result
   按官方语义精确、完整回放；不得把工具结果伪装成 user message；
3. `RequestPlan` 在请求前确定 surface、model、reasoning、tool catalog、strict、
   streaming 和 replay；sender 不二次猜测；
4. 不接 Anthropic Messages，不增加第二 transport、第二 Provider 或通用兼容抽象；
5. DeepSeek 报告中的内部 DSML/XML 不是公开 API 接入要求，不在 DSE 另造 wire protocol；
6. 1M context 是容量上限，不是默认填充目标；active context 继续由 ContextBroker 按任务和
   Token 预算渐进加载，不能用大窗口掩盖上下文污染；
7. cache 只优化安全稳定前缀；最新 revision、receipt 和未满足 acceptance 不能为了命中率
   被前移、删除或写回为 stale history；
8. fixed root/Writer 继续 Pro/high，read-only child 继续 Flash/high，已有 typed
   recovery/recheck/rework 继续 Pro/max。M36 不恢复 Auto、分类请求、关键词 heuristic 或
   运行中动态切换。

若困难任务上的 current Pro/high 与 Pro/max 对照产生完整正向证据，只能形成“是否修改一个
固定产品默认值”的后续决策；不得据此恢复 per-task Auto。改变 ADR-0008 的固定默认值需要
独立 ADR、current same-binary A/B 和完整回滚身份。

### 34.3 顶尖 Agent 能力的原生映射

| 外部经验 | 要吸收的问题本质 | DSE 唯一落点 | 明确不复制 |
|---|---|---|---|
| Codex | 清晰 goal/context/constraints/done、确定性 test/diff、sandbox、worktree、只读并行调查 | `TaskContract`、Verifier、typed permission、Writer worktree、read-only child | OpenAI Provider、产品 surface、插件/云平台 |
| Claude Code | just-in-time context、渐进探索、长任务跨会话连续性 | `ContextBundle`、artifact、Host-derived verified milestone、typed continuation | 常驻 planner/generator/evaluator、模型自由 memory |
| SWE-agent | 模型友好的 ACI、有界读取、简洁搜索、编辑后即时错误反馈 | 现有 `read_file/file_search/grep_files/edit_file/exec_shell` schema 与 outcome | 同义工具、专用命令语言、第二工具目录 |
| Agentless | localization -> repair -> validation 的简单可解释链 | TaskContract/Context/ToolOutcome/Evidence 主链 | 为展示自主性增加自由 workflow |
| Aider | Token 预算内的符号/引用导航 | 重复定位损失后的 lazy compiler/LSP/tree-sitter facts | 默认 RepoGraph、每请求全仓 map、vector memory |
| Gemini CLI | actor-scoped tool catalog、read-only planning、denied tool 不进入模型目录 | fixed actor catalog、Host permission、read-only child | 通用 policy DSL、Auto model routing |
| OpenHands | 单一 mutable truth、可恢复 replay、Agent/Application 边界 | RunStore、RuntimeEvent、app/CLI/TUI/API 分层 | 多 Provider SDK、云平台、配置图拼装 |
| Pi/Oh My Pi | minimal loop、steer/follow-up、stale-safe edit 与紧凑结果候选 | 现有 Runtime command、canonical edit、bounded outcome | 32+ 工具、Provider 抽象、默认 browser/debugger/memory |

表中机制只是候选来源，不代表 production admission。某能力若不能映射到一个现有 owner、
不能替代旧路径或没有可复现损失，就不开发。

### 34.4 执行切片

#### M36-A：能力基线与 loss matrix

先冻结 fresh、人工复核、可确定性验收的任务矩阵，不先写 production treatment：

```text
small/medium deterministic repair
large-repository localization and impact analysis
multi-module hard implementation/refactor
multi-compaction / multi-reopen long-horizon task
service/API/UI application behavior
false-completion and recovery adversarial cases
```

每条轨迹至少记录：

- 首个相关文件时间、首次正确编辑时间、错误入口和相关文件 recall；
- tool selection、malformed arguments、重复调用、空输出歧义、结果截断和 stale edit；
- 每轮 active context、稳定前缀、raw tool-output 占比、compaction/reopen 次数；
- verifier plan、latest revision receipt、false completion/rework；
- physical request、retry、usage/cache/cost、wall time、terminal 和 credential-free reopen。

loss 只能归入稳定 taxonomy：

```text
contract_ambiguity
localization
context_pollution_or_loss
tool_aci
edit_application
verification_visibility
long_horizon_recovery
writer_integration
transport_or_accounting
model_capability_ceiling
```

同一 `owner_code:loss_code` 未跨至少两个独立任务重复，不进入 production。

当前 M36-A 垂直切片使用唯一 corrected
`scripts/eval-m9b-fixed-pro-regression.py`，不复制 evaluator。它只继承 M23 私有
136-file monorepo 的 task material、reference patches、deterministic verifier 和
tool policy；20 个任务全部获得新的 `m36a-*` acceptance identity、新 workspace、
新 RunStore 和新 position-1 schedule，历史 raw/result/admission 均不是输入。人工复核
已逐项核对 objective、constraints、non-goals、gold scope 与隐藏 verifier；
reference proof 保持 17 个正向全通过、3 个 false-completion 反例全失败。

离线 contract 新增六个 task family、10-code canonical loss taxonomy、每请求 context/
stable-prefix/raw-tool-output ledger，以及定位、首次正确编辑、Tool ACI、stale edit、
completion rework 和 latest receipt 指标。正常 production `max_model_retries=2` 保持
生效，Harness `maximum_reruns=0`；这是 current product 能力基线，不把 M32 的正式
position-1 retry=0 外推成产品默认。此阶段 production delta=0；只有正式 acquisition
产生同一 canonical `owner_code:loss_code` 跨至少两个独立 task_id 重复，才允许审计
M36-B/C 的一个候选。

正式 acquisition 已从 immutable `03b40abe6dcc` binary、position 1 完成 20/20 arms：
16 个正向 verified success、3 个正确安全拒绝、1 个 Writer verified product failure，
false success=0；behavior/accounting/full-utility observations 均为 20，known cost
`$0.278444080`，maximum reruns=0。三个 long-horizon SIGKILL/reopen、service/API/UI、
两个 read-only child 和第二个 explicit Writer 均通过。

唯一 canonical loss 为
`orchestrator:writer_integration`，只出现在 `writer_envelope` 一个独立 task；未达到两个
task_id 的门槛。frozen raw final summary 曾因把 positive lane validity 错当 acquisition
completeness 而报告 incomplete；三个独立 eval-only correction commits 只读重算同一
132-record `0600` journal，20/20 truth 闭合且两次 report byte-identical，没有修改 raw、
补跑或重调 API。结果为 `keep_current_harness_no_repeated_loss`，M36-B/C 不准入。

temporary M36 Harness consumer 已物理删除并恢复 M36 前 exact blob `5f3f613c`；
production-compiled crates 始终无 delta。最终 focused 暴露的唯一 crate source 变化是
`#[cfg(test)]` loopback helper 的 5 秒 timeout 小于五回合正常执行时间；`8657a5b5c`
将 bounded test timeout 调为 15 秒，targeted/focused/workspace test 随后通过，不改变
production deadline 或行为。frozen manifest/admission/analysis/raw 与
[M36-A summary](../../eval/summaries/m36-a-deepseek-native-baseline-2026-07-27.md)
保留审计。

#### M36-A2：fresh explicit Writer repeated-loss confirmation

- 状态：**已完成，决定 `keep_current_harness_no_repeated_loss`；production delta=0，
  temporary campaign consumer 已删除**
- 真实问题：M36-A 只有 `writer_envelope -> orchestrator:writer_integration` 一个独立
  task_id 失败，尚不足以说明 Orchestrator 存在可泛化 production 缺陷；继续凭单例开发
  verify/repair 状态机会造成无证据复杂度。
- 唯一 acquisition owner：现有 corrected
  `scripts/eval-m9b-fixed-pro-regression.py`；只有 fresh loss 达门后，production 候选 owner
  才允许是 `crates/orchestrator`。
- production old path：本阶段不替代任何 production path，crate delta 必须为 0；历史
  M36 raw/result 不是输入，也不计入 repeated-loss threshold。

冻结 3 个全新、人工复核、初始必失败且 reference 必通过的 explicit Writer task：

```text
writer_retry_ledger      Python physical-attempt/accounting migration
writer_header_policy     Rust bounded canonical header policy
writer_route_contract    TypeScript fixed-route codec migration
```

每 task 只执行一个 fresh `deepseek-v4-pro/high` position-1 arm，使用新 Git repository、
isolated Writer worktree、RunStore、DSE home、hidden deterministic verifier 和 latest-root
receipt；normal Runtime safe retry 保持 2，Harness `maximum_reruns=0`。behavior 与 accounting
继续按 ADR-0011 正交，unknown billing 在下一 arm 前停止，不补 mate、不重跑。

只有本次 3-task fresh set 中至少两个不同 task_id 都产生精确
`orchestrator:writer_integration`，才允许审计一个 Orchestrator-owned bounded
verify -> repair -> reverify treatment。即使达到门槛，也必须另冻 held-out 同任务 A/B 后才可
改 production；本 acquisition 不自动准入实现。若 0/1 个任务出现该 loss，决定为
`keep_current_harness_no_repeated_loss`，删除 M36-A2 Harness selector/consumer，只保留 frozen
manifest、raw、summary 与可复核身份。

最低离线证据为 3/3 reference proof、fixture/base-commit identity、current 三字段 permission
controls、Writer lifecycle/cleanup observer、hash-chained journal 四个 SIGKILL 窗口、exact
SQLite reopen、synthetic 0/1 与 2-task threshold、focused/full workspace gate。ADR-0015
仍为 implementation-not-admitted；M36-A2 不开发 browser/search/vision。

正式 acquisition 使用 immutable `a0847c5c1c04` binary，从 position 1 按冻结顺序完成
3/3 arms，maximum reruns=0。`writer_retry_ledger` 与 `writer_route_contract` verified
success；`writer_header_policy` 是 measurement-valid
`orchestrator:writer_integration` product failure。false success=0，三臂 behavior/accounting
均闭合，38 个 physical model requests，input/output `196,296 / 19,773` tokens，known cost
`$0.113726632`，wall time `541,642 ms`。失败只覆盖一个 fresh task_id，未达到两个独立
task 的准入门；不实现、也不评测 verify -> repair -> reverify treatment。唯一 corrected
Harness 的 M36-A2 selector/loader/observer/aggregate/self-test/CLI consumer 已物理删除并恢复
到 M36-A2 前 exact blob `d3916654f`；frozen manifest、admission、fixture/reference、ignored
`0600` raw 与
[M36-A2 summary](../../eval/summaries/m36-a2-writer-loss-confirmation-2026-07-27.md)
保留审计。M36-B/C 继续不准入。

#### M36-B：DeepSeek effort 与 context control-only

只做两个可归因控制实验，不同时改变 Prompt、工具或 Runtime：

1. 在冻结困难任务集上比较 current `deepseek-v4-pro/high` 与
   `deepseek-v4-pro/max`，验证 Max 是否提高 verified success/降低 false completion，
   并完整记录 output/reasoning Token、请求、wall time 和费用；
2. 先观察实际 active context 与失败位置；只有损失重复指向 under-context 或
   context pollution 时，才比较 current ContextBroker 与一个最小 working-set treatment。

不做多档 Auto、不让 Flash 分类任务、不以 1M 最大窗口作为 treatment。任何 in-place
compaction 都必须保持当前 tool-call/result 原子性和 DeepSeek reasoning replay；若证据要求
context reset，只能在无 in-flight model/tool、Host 已验证 milestone 的 typed continuation
边界建立新请求历史，不能静默截断正在进行的工具推理链。

#### M36-C：一次只准入一个最高收益 Harness treatment

按 M36-A 的重复损失，最多选择下列一个候选：

1. **最小 Tool ACI**：先改现有 schema、description、typed error、分页或 bounded summary；
   可测试 lint-on-edit、search file-first 再按需展开 snippet、明确 empty-success 和 stale
   target feedback，但一次实验只改变一个行为族；
2. **确定性 localization**：先改进现有 file/grep/read；仍失败时才在 `context/tools`
   owner 内 lazy 加载 symbol/definition/reference fact，不建设永久 RepoGraph；
3. **Host-derived VerifiedMilestone**：只从 TaskContract、revision、EvidenceReceipt 与
   verifier 派生已验证进度、未满足 acceptance 和恢复入口，不允许模型维护第二进度真相；
4. **Host-owned ApplicationProbe**：只在 service/API/UI 任务重复败于运行可见性时，建立
   bounded start -> health/port -> logs -> HTTP -> optional DOM/screenshot -> receipt ->
   guaranteed teardown/reopen cleanup；
5. **content-anchored edit**：只有 canonical edit 的真实模型轨迹重复出现 stale/ambiguous
   target，且现有 12/12 Host correctness 仍不能解决时才评测；不能因竞品宣传 hashline
   就替换已正确的 edit owner。

candidate 必须使用现有 crate owner、事件、ToolOutcome、artifact、receipt 和 Store。
不创建 Manager/Factory/Service 空壳，不增加第二模型循环或第二完成权。

#### M36-D：长程与多 Agent 的受控扩展

只有单 Agent 基线证明 context pollution 或可并行 wall-time 是主要瓶颈时才执行：

- read-heavy exploration、测试和日志分析可交给 bounded read-only child，返回结构化摘要，
  根 Agent 保留 TaskContract、决策与最终收敛；
- single Writer 继续默认；它使用 isolated worktree、diff、verify、integrate、root
  latest-revision verify 和 cleanup；
- 多 Writer 只有在依赖图可冻结、子任务真正独立、单 Writer wall time 跨任务重复成为主要
  瓶颈时重开，并必须证明净时间收益且不增加 conflict、false success、Token 或复杂度；
- 不建立自由聊天 swarm、无限递归、长期 reviewer/critic 群或第二 scheduler。

#### M36-E：外部系统能力保持范围外

MCP、浏览器、web search、IDE/GUI、远程执行和云平台不属于 M36 核心 Harness。只有产品范围
出现外部系统任务，且固定工具面无法完成时才另立里程碑。届时外部内容必须带 provenance/
trust/capability 边界进入 ToolOutcome，不得让网页、README 或 MCP 输出直接获得指令权。

### 34.5 三层比较，避免混淆模型与 Harness

正式结果必须分开报告：

```text
Harness comparison:
  same DeepSeek V4 model/effort
  DSE vs minimal/generic compatible harness

Product comparison:
  DSE + DeepSeek V4
  vs Codex / Claude Code / other complete products

Model-ceiling comparison:
  official-style minimal DeepSeek harness
  vs DSE treatment under the same DeepSeek model
```

只有第一层可以直接归因 DSE Harness。第二层反映用户最终体验，但同时混合模型、Harness、
工具和产品环境；第三层说明工程增益与 DeepSeek 当前模型上限之间的距离。

### 34.6 准入、删除与停止门

每个 M36 production treatment 都必须：

1. 先冻结 real problem、唯一 owner、control、treatment、held-out tasks、预算和 verifier；
2. control/treatment 使用相同 DeepSeek model、reasoning、Prompt、workspace、tool authority、
   deadline、request/Token budget 和 external verifier；
3. false success 保持 0，correct safety rejection 不回退；
4. verified success 或目标 hard/long-horizon task completion 有明确净提升；
5. 不用 Token、cache、费用或速度改善掩盖质量下降；
6. behavior/accounting 按 ADR-0011 正交记录，unknown billing 不猜零、不拼样、不补 mate；
7. candidate 通过 root/read-only/Writer、crash/reopen、focused 和 full workspace gates；
8. replacement cutover 后物理删除旧路径、eval-only selector、临时 fixture consumer 和
   无消费者依赖；
9. held-out 无净收益、出现 false success、增加第二 truth 或复杂度不可接受时
   `reject_and_delete`。

可接受结果只有：

```text
keep_current_harness_no_repeated_loss
keep_minimal_treatment_and_delete_replaced_path
reject_and_delete_candidate
hold_model_capability_ceiling
```

### 34.7 明确拒绝的开发路线

M36 不准入：

- “把每个顶尖 Agent 最强功能各抄一个”的竞品 backlog；
- Anthropic Messages、OpenAI Provider、第二 DeepSeek compatibility Provider；
- Auto model/Thinking classifier、关键词路由或运行中升级；
- 默认 planner/critic/evaluator 三 Agent 流水线；
- 默认多 Writer、swarm、Agent 社交协议和重复 team tools；
- 默认 RepoGraph、embedding/vector memory 或每请求全仓 map；
- 为固定小工具集建设动态 tool search/code mode；
- 为 99% cache hit 重排或弱化 latest-revision Host facts；
- 浏览器/MCP/云平台先于真实外部任务；
- 用 benchmark 排名、工具调用数、代码行数或架构图复杂度冒充产品提升。

### 34.8 研究依据

- [DeepSeek V4 Technical Report](https://huggingface.co/deepseek-ai/DeepSeek-V4-Pro/blob/89d501aed998d33fa4f4702102ec1bb2331e10f6/DeepSeek_V4.pdf)
- [DeepSeek Thinking Mode and tool-call replay](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Claw-SWE-Bench](https://arxiv.org/abs/2606.12344)
- [SWE-agent Agent-Computer Interface](https://swe-agent.com/0.7/background/aci/)
- [Agentless](https://arxiv.org/abs/2407.01489)
- [Aider repository map](https://aider.chat/docs/repomap.html)
- [Anthropic context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [Anthropic long-running harness design](https://www.anthropic.com/engineering/harness-design-long-running-apps)
- [Codex best practices](https://learn.chatgpt.com/guides/best-practices)
- [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents)
- [Codex sandbox](https://learn.chatgpt.com/docs/sandboxing)
- [OpenHands V1 design principles](https://docs.openhands.dev/sdk/arch/design)
- [OpenAI coding evaluation audit](https://openai.com/index/separating-signal-from-noise-coding-evaluations/)

## 35. M37：模型可见契约减法与 Harness 控制边界

- 状态：**M37-A 离线 projection audit 已完成；M37-B/C 正式 acquisition 均因
  incomplete evidence 停止且候选已删除；M37-D/E 未自动准入**
- 起始基线：M36-A clean checkpoint `400ec0813`
- 长期决策：[ADR-0014](../decisions/0014-model-visible-contract-and-harness-control.md)
- owner：`crates/context`；工具、权限、Runtime、Orchestrator、Verifier 继续由现有模块拥有
- 北极星：`verified task success / tokens / time / code complexity`

### 35.1 目标与明确非目标

M37 解决的不是“把 Prompt 改得更短”，而是阻止测试失败演变成 append-only Prompt debt，
并把所有可确定执行的行为放回 Harness owner。M37 固定：

```text
Prompt = stable semantic contract
Harness = enforceable policy + tools + state + evidence + recovery
```

当前 bundled core 69 行、3,300 bytes，不存在单向膨胀事实；M17-F 又为 current Chinese
prompt 提供了正式 32-arm evidence。因此 M37 不直接替换 Constitution，不用五行候选覆盖
current baseline，也不把一次 Writer 失败写成全局规则。

M37 不修改 DeepSeek model/Thinking、tool catalog、permission、RuntimeEvent、RunStore、
Writer state machine、TUI、Provider 或 external surface。M36-A2 Writer-loss confirmation
是独立 Goal，不能混入 M37。

### 35.2 已冻结的 current debt

1. M36-A Writer 首请求的 stable system blocks 为 28,494 bytes，core/output/language 约
   3.3 KB；
2. 无项目说明文件时 fallback overview 与 Project Context Pack 使用同一 payload；
3. execution posture 声称 `agent` 只支持只读 child，而 current schema 支持
   `isolated_write`；
4. prompt fragments 最终合并成单个 DeepSeek system message，项目事实与系统契约的
   authority 需要显式审计；
5. 现有 fragment ledger 没有统一 total budget、duplicate relation 和 tool-schema claim
   parity；
6. M10-A pack-off 因 accounting incomplete 停止，不能从旧 raw 推导删除收益。

### 35.3 垂直切片

#### M37-A：projection audit 与 no-growth 门

- 扩展现有只读 fragment ledger，不创建第二 composer/Store；
- 每段记录 source/owner/authority/trust/stability/hash/bytes/token estimate/duplicate；
- 离线检查 Prompt 中 tool/actor capability claim 与真实 catalog/schema；
- fixture 覆盖有/无 `AGENTS.md`、rules-heavy、skills-heavy、small/large repo，
  root/read-only child/Writer 和 reopen；
- 冻结 current assembled 分布、exact duplicate 和 system-authority 数据；
- core stable bytes 暂不得高于 3,300；model-visible delta=0；
- focused、context/deepseek/app targeted tests、workspace gates 通过后形成 clean checkpoint。

M37-A 不读取 Key、不调用官方 API。它只建立观测与机械门，不因发现 debt 自动修改
production prompt。

M37-A 已在 `5fb2bc971` 完成：

- `crates/context` 直接扩展原有 derived ledger，新增 ordered fragment identity、
  owner/authority/trust、payload relation、final assembled/stable-prefix identity、
  3,300-byte bundled core gate 和 actor-scoped tool claim parity；没有第二 composer、
  Store、RuntimeEvent 或 production caller；
- fixture 覆盖有/无 `AGENTS.md`、compatibility source ordering、rules-heavy、
  skills-heavy、small/medium/large、显式 constitution override、root、Writer
  coordinator、read-only child、explicit Writer 与 SQLite reopen；
- 普通 `production_system_prompt` 与 audited prompt 逐字段相等；DeepSeek 仍把相同 blocks
  合并成唯一 system message，root/read-only/Writer reopen 后 ledger、prompt 和
  RequestPlan 精确一致；
- 当前机械观测为：无项目说明时 `project_context -> project_context_pack` 是
  `same_payload_wrapper`；普通 root/read-only child 的 `agent` claim 与实际 read-only
  schema 一致，Writer coordinator 的实际 schema 还含 `isolated_write`，因此同一
  execution-posture claim 为 mismatch；explicit Writer 没有 `agent` tool；
- current deterministic fixture 的 assembled bytes/token estimates 为 no-AGENTS
  `5,283/2,063`、rules-heavy `5,198/1,995`、skills-heavy `9,225/3,802`、
  medium `9,013/4,161`、large `18,963/9,735`。这些是离线分布，不是任意仓库 hard limit；
- Constitution/output/language 仍为 `2,629 + 360 + 311 = 3,300` bytes。M37-A 没有修改
  production prompt、project pack、tool catalog、permission、Runtime、RunStore、Writer
  或 DeepSeek 请求，也没有读取 Key 或调用官方 API。

结论为 `keep_current_no_material_loss`：保留机械 audit/no-growth gate 和两项 current debt
事实；不从离线发现自动进入 M37-B/C，更不把 M36 单个 Writer loss改写成 Prompt treatment。
完整证据见
[M37-A prompt projection audit](../../eval/summaries/m37-a-prompt-projection-audit-2026-07-27.md)。

#### M37-B：execution posture/schema parity

只有 M37-A 证明 stale claim 的真实 source/caller 后才启动：

- baseline 保持 current posture；
- candidate 只删除“`agent` 只支持只读 child”的 tool-specific 句子；
- tool schema、Constitution、Runtime、Orchestrator 和 actor route 不变；
- root-only、read-only child、explicit Writer 使用 same-binary Pro/high A/B；
- 无质量回退且工具选择/Writer completion 有明确净收益才 cutover；
- 否则删除 candidate，不加补充解释或 compatibility branch。

M37-B 已在同一 immutable binary `78d8ddb2c` 上启动 30-arm Pro/high 配对 A/B，正式
raw 在 12 个完整 arm 后按合同停止。12/12 complete arms 为 9 个 positive verified
success、3 个正确 safety rejection、`false_success=0`；已观察的 control/treatment 各
6 个完整 arm，均没有 tool selection、Writer completion 或 verifier 回退，也没有形成
可归因差异。第 13 个 control arm 最终完成并通过 external verifier，但首个物理请求在
response headers 前发生 typed `deepseek_transport`；一次 durable 1 秒 Runtime retry
成功后，最终仍有 `billing_unknown_attempts=1`。Harness 写入
`abort(accounting_incomplete, completed_arms=12)`，没有启动 position 14、补 mate 或重跑。

因此 formal 决定为 `hold_insufficient_or_incomplete_evidence`，不能形成 30-arm
non-inferiority/benefit aggregate；production 候选按准入门执行
`reject_and_delete_candidate`。原 execution posture 保持唯一 production 路径，临时
selector、alternate prompt branch、M37-B Harness consumer 与专属测试均已物理删除；只
保留 frozen contract/admission、ignored `0600` raw hash 和
[M37-B summary](../../eval/summaries/m37-b-posture-schema-ab-2026-07-27.md)。M37-C 不由
这些 partial observations 自动准入。

#### M37-C：fallback overview/pack 去重 successor

它是 M10-A 的 fresh successor，不续跑或拼接旧 21 个完整 arms：

- baseline=current pack-on，candidate=同一 project fact payload 只投影一次；
- 不同时加入 ranked working set、RepoGraph、lazy rules 或新预算算法；
- 至少 6 affected task families × 2 variants × 3 fresh runs；
- false success=0、安全和 verified success 不回退是硬门；
- secondary metrics 为 cache-miss input、首相关文件、read/search 重复、requests、wall/cost；
- winner cutover 后删除第二 renderer/caller 与 eval-only selector；失败则恢复 pack-on。

M37-C 已冻结 6 families × 2 variants × 3 fresh runs 的 same-binary Pro/high 合同，并在
candidate `241941733` 上完成全部离线门禁。正式 position 1 的 control 已读取授权测试 Key
并到达冻结的 `request_user_input` 连续性 checkpoint，但 M37-C Harness consumer 错把
非 M30 的合法 user-input interaction 限定为 approval，写入
`abort(hardness_user_input_not_admitted, completed_arms=0)`。raw 只有 plan、credential、
arm-start、abort 四条 hash-chained record，没有 `arm_result`、闭合 accounting 或 summary；
没有启动 position 2、mate 或 rerun。

按 `maximum_reruns=0` 与 incomplete-evidence 删除门，formal 决定为
`hold_insufficient_or_incomplete_evidence`，production treatment 为
`reject_and_delete_candidate`。current pack-on 继续作为唯一 production prompt；临时 selector、
alternate projection、M37-C Harness campaign/aggregate 与专属测试均已物理删除。只保留 frozen
fixture/contract/admission、ignored `0600` raw hash 与
[M37-C summary](../../eval/summaries/m37-c-context-dedup-ab-2026-07-27.md)。本结果不证明
去重有益/有害/等价，也不准入 M37-D/E。

#### M37-D：authority 与总预算

只有 M37-A/C 的分布或 repeated loss 证明必要性才进入。自动 README/tree/manifest 不与
stable Constitution 共享未区分 authority；总预算从真实 fixture 和 A/B 导出，不拍脑袋
设定。不得截断 active scoped rules、TaskContract、latest receipt 或 tool-call/result
原子对，不增加第二 context store、RepoGraph 或 embedding。

#### M37-E：最小 Constitution ablation

默认不执行。只有 core 冲突或至少两个独立 `stable_semantic_misunderstanding` loss 才允许
一个 compact candidate。English/`zh-Hans` 任务、Pro/high、工具、Context、Runtime、
预算和 verifier 全部固定；质量非劣后才比较 Token/时间/成本。失败候选完整删除，production
永远只保留一个 Prompt。

### 35.4 准入、删除和停止门

每个 model-visible candidate 必须满足
[M37 evaluation contract](EVALUATION.md#m37-model-visible-contract-and-harness-control-contract)：

1. 同一 owner 一次只改一个变量；
2. prompt provenance、binary、task、workspace、model、effort、catalog、budget、verifier
   和 observer 可复核；
3. false success 为 0、安全不回退、verified success 不回退；
4. 成本、cache、Token 和速度不能补偿质量损失；
5. behavior/accounting 按 ADR-0011 正交闭合；
6. 不补 mate、不选择性 rerun、不拼旧 raw；
7. cutover 删除旧块、临时 selector/fixture consumer 和失去消费者的配置；
8. 无净收益即 `reject_and_delete`，证据不完整即 hold。

单次任务失败先归因 task/eval、model、ACI、context、controller、permission、verifier、
transport，只有稳定语义误解才允许改 Prompt。测试发现问题，但不自动生成系统规则。

### 35.5 研究输入

- [ADR-0014 研究依据](../decisions/0014-model-visible-contract-and-harness-control.md#研究依据)
- [M10-A scoped context pack](../../eval/summaries/m10-a-scoped-context-pack-2026-07-24.md)
- [M17-F bilingual prompt A/B](../../eval/summaries/m17-f-bilingual-prompt-ab-2026-07-25.md)
- [M36-A DeepSeek-native baseline](../../eval/summaries/m36-a-deepseek-native-baseline-2026-07-27.md)

## 36. M38：typed interaction observer 与 durable abort accounting

- 状态：**已完成；保留 Harness 修复并删除 campaign-name interaction 语义**
- 起始基线：M37-C clean checkpoint `b91d10780`
- owner：既有 corrected Harness `scripts/eval-m9b-fixed-pro-regression.py`
- production delta：0；Run API v15、RuntimeEvent v22、State v28、exec-stream v6 不变

### 36.1 真实问题与边界

M37-C position 1 到达 canonical `request_user_input` checkpoint 后，Harness 仍用 campaign
名称猜测 pending interaction 是 approval 还是 user input。它在 terminal/accounting
snapshot 前中止，导致 RunStore 已持久化的 usage 与 known cost 没有进入 journal。这是
observer owner 的本地事实丢失，不是 Prompt、Runtime、Provider 或产品能力失败。

M38 只修复两个局部 owner 问题：

1. pending interaction 从 canonical `UserInteractionPrompt` 的 typed kind/payload 派生；
2. observer 在 durable RunStore checkpoint 后失败时，先写 hash-chained safe abort
   snapshot，再写最终 abort。

真正的 response-before-headers provider attempt 若没有 response id、usage 或逐请求账单
对账合同，仍保持 `billing_unknown`。M38 不用 account-level balance 或 monthly API-key
export 猜测单个物理请求是否计费。

### 36.2 验收与 cutover

冻结的 14-case corpus 覆盖 root approval/user input、resolved/malformed/mismatched/multiple
interaction、read-only child、Writer、exact reopen/event drift、known usage/cost 和 provider
billing unknown。observer-abort journal 另外覆盖 before/mid/unfsynced/after snapshot 四个
SIGKILL 窗口、partial-tail 拒绝和 hash-chain tamper 拒绝。

M9-C 与 M30 运行同一 M38 conformance 得到 byte-identical report，证明 campaign 名称不再
决定 interaction kind。M9/M14/M16/M23/M30 既有 Harness conformance/self-test 保持通过。
正式 cutover 删除：

- `CAMPAIGN` 驱动的 approval/user-input admission；
- `CAMPAIGN` 驱动的 approved/answered response construction；
- `hardness_user_input_not_admitted` 历史 observer code；
- durable Store facts 已存在时只写 abort、不保存 accounting boundary 的路径。

M37-C frozen manifest/raw/summary 不修改、不续跑、不补 mate；M38 不读取 Key、不访问网络。
结论为 `keep_typed_interaction_observer_and_durable_abort_snapshot`。完整证据见
[M38 summary](../../eval/summaries/m38-typed-interaction-observer-2026-07-27.md)。

## 37. M39：fresh context/localization loss admission audit

- 状态：**M39-A 已完成；无 repeated loss，production delta=0，临时 consumer 已删除**
- 起始基线：M36-A2 clean checkpoint `360e52cae`
- acquisition owner：唯一 corrected
  `scripts/eval-m9b-fixed-pro-regression.py`
- production treatment：未准入；`crates/context` 保持 current pack-on 路径
- 共享边界：ADR-0015 仍为 implementation-not-admitted；本里程碑不开发或评测
  browser/search/vision

### 37.1 真实问题与验收

M36-A2 已再次证明 Writer 同因只覆盖一个 fresh task，不能继续为该单例堆 recovery。
M32 的 Rust localization 单例又被更晚 M36-A 的三项 localization 3/3 反证为未重复。
当前仍有一项确定性但未证明 material 的 context debt：无显式项目说明文件时，fallback
overview 与默认 Project Context Pack 会把同一 payload 投影两次；M37-C 正式 A/B 因旧
interaction observer 在 position 1 停止，且候选按合同已删除，不能续跑或拼样。

M39-A 只用 exact current pack-on production 做 control-only acquisition，回答：在新的
无项目说明文件仓库中，是否至少两个独立任务都形成可复核的 `context:localization` loss。
验收冻结为：

1. 6 个全新 task/material，5 个正向任务覆盖 root、read-only child、explicit Writer，另有
   1 个 no-tool 假完成反例；
2. 每项初始 verifier 失败，5 项 reference solution 通过，安全反例继续失败；
3. fixed `deepseek-v4-pro/high`、current Prompt/tools/permission/Runtime/Store，正常 Runtime
   safe retry=2，Harness rerun=0；
4. 每 arm 新 Git repository、DSE home、RunStore、verifier home，terminal 后 exact
   credential-free SQLite reopen；
5. behavior/accounting 正交闭合，false success=0；unknown billing、usage incomplete、
   identity/observer/evidence 歧义在下一付费 arm 前停止；
6. 同一 canonical `owner_code:loss_code` 未跨至少两个 fresh task_id 重复时，决定必须为
   `keep_current_harness_no_repeated_loss`，production delta 保持 0。

### 37.2 单一 owner、旧路与删除门

本阶段没有 production old path 被替代，也不恢复 M37-C alternate projection。临时
Harness 只增加 M39 manifest loader、fresh fixture/reference proof、context-aware loss
projection、aggregate/self-test 与 live caller；所有事实继续来自 canonical
RunStore/verifier/accounting observation。

只有 fresh set 中 `context:localization` 至少覆盖两个 task_id，下一独立切片才可审计一个
`crates/context` treatment；它仍需新的 same-task held-out A/B，不能由 acquisition 自动接管。
若没有 repeated loss，删除 M39 temporary campaign consumer，只保留 frozen manifest、
ignored `0600` raw、summary 与 Git 历史。即使出现其他 repeated owner，也一次最多准入一个
唯一 owner audit；不得同时修改 Prompt、工具、Runtime 或 actor route。

### 37.3 费用、门禁与明确非目标

正式 schedule 为 6 个 position-1 arms，per-arm known-cost ceiling `$0.50`、suite ceiling
`$3.00`。读取既有授权测试 Key 前，必须完成 fixture/reference proof、loss threshold、
journal crash windows、M38 typed interaction、truth/acceptance observer、process
SIGKILL/reopen、immutable binary dry-run、focused、fmt、strict workspace Clippy/test、
public checker 与 `git diff --check`。

M39-A 不证明 duplicate pack 有益或有害，不执行 M37-C successor，不修改 Prompt bytes、
ContextBroker、RuntimeEvent、State、tool catalog、permission、Writer behavior 或 UI；不恢复
Auto、FIM、RepoGraph、planner/critic、多 Writer或第二 Provider/Runtime/Store。

### 37.4 正式结果与删除

immutable `4f93060fe2ee` binary 从新 schedule position 1 完成 6/6 arms：四个正向任务
verified success、一个 explicit Writer verified product failure、一个安全反例正确拒绝，
`false_success=0`。behavior/accounting 6/6 闭合；44 个 model requests、input/output
`266,428/20,004`、cache hit/miss `184,064/82,364`、known cost `$0.066798629`。

唯一 loss 是 `writer_record_migration -> orchestrator:writer_integration`，只覆盖一个 fresh
task_id；没有 `context:localization`，因此决定为
`keep_current_harness_no_repeated_loss`。fallback overview/Project Context Pack 重复事实仍是
debt，但没有达到 treatment 准入门。

frozen raw 的末尾 summary 曾因 eval-only aggregate 把 Writer lane failure 同时当成
measurement incompleteness 而写出 `complete=false`。绑定 acquisition/analysis Harness hash
的 credential-free report 从同一 immutable raw、canonical Store/reopen/verifier/accounting
事实两次生成 byte-identical 6/6 truth；raw 未修改，API 未重调。闭合失败现在计入 loss、
永不计为 success。

M39 temporary campaign selector、loader、live caller、aggregate/self-test 与 trajectory
consumer 已物理删除，Harness 恢复 M39 前 exact blob `d3916654f`。保留 frozen
contract/live admission/analysis、fixture/reference、ignored `0600` raw、
[M39-A summary](../../eval/summaries/m39-a-context-loss-admission-2026-07-27.md)与 Git 历史。
ADR-0015 继续 implementation-not-admitted；没有 browser/search/vision implementation。

## 38. M40：fresh build/state/protocol loss acquisition

- 状态：**M40-A 已完成；`reject_incomplete_acquisition`，production delta=0**
- 起始基线：M39-A checkpoint `f7b54fc4e`
- acquisition owner：唯一 corrected
  `scripts/eval-m9b-fixed-pro-regression.py`
- production owner：未选择；只有本轮 fresh repeated-loss 门通过后才允许审计一个唯一 owner
- 外部能力边界：ADR-0015 继续 implementation-not-admitted；不开发或评测 browser、search、
  vision、Web fetch 或 ApplicationProbe

### 38.1 真实问题与新覆盖

M36-A/M39-A 已覆盖常规 deterministic repair、large-repo localization、service/API/UI、
long-horizon reopen、read-only graph、explicit Writer migration 与 false completion，但两轮都
没有同一 fresh `owner_code:loss_code` 跨两个独立任务重复。继续把历史 Writer/context 单例
拼接为候选会违反 ADR-0011 和 M36 门槛。

M40-A 只扩展 exact current control 到尚未独立覆盖的 instruction-bearing 工程 strata：

```text
Rust workspace feature/build matrix
Go CLI exit-code and stderr contract
Python SQLite transactional schema migration
TypeScript incremental UTF-8 stream framing
Rust append-journal reopen integrity
read-only child failure-artifact investigation and root handoff
explicit Writer release-artifact metadata migration
missing generator source false-completion counterexample
```

八个 task 都使用新 material、新 acceptance identity、新 Git workspace、DSE home、RunStore、
verifier home 和 position-1 schedule；M36/M39/M32 的 raw、result、admission 和 loss 均不是
输入。fixture root 含最小 `AGENTS.md`，只冻结真实 production/test scope 和各项目验证命令，
不嵌入 gold patch 或唯一修复步骤。

### 38.2 验收、归因和删除

1. 8/8 初始 verifier 失败，7/7 positive reference solution 通过，安全反例继续失败；
2. fixed `deepseek-v4-pro/high`、current Prompt/tools/permission/Runtime/Store，normal Runtime
   safe retry=2，Harness rerun=0；
3. root、read-only child、explicit Writer 与 safety lane 使用各自 current actor catalog；
4. behavior/accounting 正交闭合、false success=0、terminal 后 credential-free exact SQLite
   reopen；
5. loss 只从 canonical terminal/receipt/verifier/actor/tool/accounting facts 保守归因，不从
   task 名、历史结论或工具调用数量猜测；
6. 同一 exact `owner_code:loss_code` 至少覆盖两个本轮 fresh task_id，才返回
   `next_candidate_audit_required`；即使通过也只准入一个独立 held-out audit，不自动实现；
7. accounting 全闭合但未达 repeated-loss 门时决定为
   `keep_current_harness_no_repeated_loss`；任一 arm usage/accounting 不完整则立即
   `reject_incomplete_acquisition`。两种结果都保持 production delta=0，并删除 M40
   temporary selector/loader/live caller/aggregate/self-test/trajectory consumer。

本阶段不替代 production old path，也不加入 treatment。frozen fixture、reference、contract、
admission、ignored `0600` raw、analysis/summary 与 Git 历史是审计证据；临时 Harness consumer
在决定后物理删除。

### 38.3 费用与停止门

正式 schedule 为 8 个 fresh position-1 arms，per-arm known-cost ceiling `$0.50`、suite
ceiling `$4.00`。credential 前完成 reference proof、loss threshold、journal crash windows、
M38 interaction、truth/acceptance observer、process SIGKILL/reopen、immutable binary dry-run、
focused、fmt、strict workspace Clippy/test、public checker 与 diff check。

任何 billing unknown、usage incomplete、false success、identity/route/actor/observer/evidence
歧义或费用越界都在下一付费 arm 前停止；不补 mate、不重跑、不拼接历史 raw。M40 不修改
Prompt、ContextBroker、RuntimeEvent、State、tool catalog、permission、Writer behavior、UI、
DeepSeek surface 或 fixed actor route。

### 38.4 正式结果与删除

immutable `e2ed02c3a805` binary 从 position 1 启动 `rust_feature_matrix`。workspace 修改通过
deterministic verifier，但第七个 physical DeepSeek request 在收到 response headers 和
actionable reasoning 后、收到 content/finish/usage/`[DONE]` 前断流。Runtime 将其分类为
retryable 但 `retry_safe=false`，以 `actionable_output` 停止，没有发出第八个请求；Host terminal
为 failed、latest receipt 缺失、`false_success=0`。

canonical accounting 为 7 started / 7 completed、6 usage responses、1 incomplete response、
runtime retry 0；已知六次 usage 的局部费用为 `$0.010289171`，但最后一次真实 usage/费用无法
从 provider response 还原，所以 accounting complete 为 0/1、full-utility observation 为 0。
Harness 按冻结门在 arm 2 前停止，没有补 mate、选择性重跑或拼接历史证据。只读报告得到唯一
`deepseek:transport_or_accounting` 单例，但 incomplete accounting 先于 repeated-loss 门，最终
决定为 `reject_incomplete_acquisition`，没有 production owner、treatment 或 A/B。

M40 temporary campaign consumer 已物理删除，唯一 Harness 恢复 exact blob `d3916654f`。
fixture/reference、contract/admission/analysis、ignored `0600` raw、
[M40-A summary](../../eval/summaries/m40-a-engineering-loss-acquisition-2026-07-28.md)与 Git 历史
保留。ADR-0015 继续 implementation-not-admitted；没有 browser/search/vision implementation。

## 39. M41：原生 `web_fetch` 生产能力

- 状态：**已完成并保留（2026-07-28）**
- Goal：以前 DSE 无法以 canonical 工具读取用户给出的公开 URL；完成后，root Agent 能在
  现有权限、ToolOutcome、RunStore 和完成链中安全读取有界网页文本与来源事实。
- owner：`crates/tools`
- 起始事实：current catalog 只有 11 个本地代码工具；没有 `web_fetch`、search、browser、
  MCP model executor 或第二 Web owner。

### 39.1 固定范围

W1 只实现一个已知 URL 获取工具：

```text
url
optional max_chars
```

结果至少包含 requested/final URL、status、media type、title、有界正文、有界 canonical links、
retrieved time、source SHA-256、读取/返回 bytes、truncation 和
`trust=external_untrusted`。模型不能提供任意 header、Cookie、认证、代理、证书、文件路径或
脚本。

Host 必须在每次解析、连接和 redirect 上拒绝 private/loopback/link-local/multicast、metadata、
非 HTTPS scheme 与 redirect escape；限制 redirect、deadline、response/decompressed bytes 和
content type。HTML 只做确定性文本/标题/链接提取，不执行脚本。第一版不读取 PDF、图片、
archive，不启动 Chrome，也不增加 search provider。

### 39.2 纵向实现与旧路

1. 先在 `crates/tools` 写 URL、egress、redirect、body、extract 和 outcome 的失败合同；
2. 把 `web_fetch` 加入现有 production catalog/schema/authorization/executor；
3. 复用现有 `ToolOutcome -> RuntimeEvent -> RunStore`，不增加 Web Store、session 或第二
   accounting ledger；
4. root 默认可见；read-only child/Writer 是否可见必须由现有 actor catalog 正向授权决定，
   不从 Prompt 或角色名称推断；
5. TUI/config 中未进入模型 catalog 的 search-provider 管理面不算 W1 caller，本切片不顺手
   重构；只在用户文案中禁止把它描述为已具备的 Agent 搜索能力；
6. 若实现发现必须改变 RuntimeEvent/State schema，先证明现有 ToolOutcome 无法无损表达，
   不得为 Web 预建 Manager/Factory/Service 或兼容层。

### 39.3 验收、费用与停止门

- deterministic：HTTPS、redirect、SSRF/DNS rebinding、metadata、content type、解压上限、
  truncation、HTML 提取、typed failure、authorization、catalog parity 和 SQLite reopen；
- production loopback：同一 AgentApplication/AgentRuntime 路径中模型选择 `web_fetch`，结果
  经 Store 重放且不重复请求网络；
- real canary：一个 current official DeepSeek known-URL task，maximum reruns=0、费用上限
  `$0.10`；只证明工具可选、来源可读和任务闭合，不宣称通用成功率/成本提升；
- gates：`./scripts/dev-dse.sh focused`、`cargo fmt --all -- --check`、
  `cargo test -p dse-tools --locked`、`cargo check -p dse-tools --locked`、`git diff --check`；
- accounting incomplete 只阻止精确成本声明和后续付费请求，不抹掉已闭合的 deterministic
  behavior；该规则作为本切片的 evaluator policy 小改，不另立 M41-accounting 里程碑。

M41 不执行多 cell 付费 A/B。只有安全边界无法强制、结果不能进入 canonical outcome/replay，
或真实 caller 无法使用时，才 `reject_and_delete`；不得因文档、测试或评测工作量本身延期
交付。

### 39.4 正式结果与删除

M41 在 `crates/tools` 交付第 12 个固定 Host 工具 `web_fetch(url, max_chars?)`，复用 workspace
已有的 Reqwest/Rustls、`ProductionToolExecutor`、authorization、`ToolOutcome`、RuntimeEvent
和 SQLite RunStore。Host 固定执行 public HTTPS、userinfo/metadata deny、逐跳 DNS 全地址校验、
connect pin、manual redirect、deadline、raw/decompressed body、content type/encoding/charset 与
UTF-8 安全门；HTML 只产生有界 title/text/canonical links。Ask 因缺少可强制的 scoped network
approval 而 deny，Agent/FullAccess root allow，isolated Writer 继续由 network-denied sandbox
deny。现有 `ToolOutcome.content + metadata` 足够表达成功与 Web-specific typed failure，所以
RuntimeEvent/State schema 未变。

deterministic suite 覆盖 URL、public/special IP、mixed DNS/rebinding、connect pin、redirect
escape/loop/limit、metadata、deadline、content type、raw/decompression/UTF-8、Unicode truncation、
script suppression、link bound、authorization 与 catalog parity。真实 production loopback 中
DeepSeek fixture 选择 `web_fetch`，committed outcome 进入下一次 canonical model request；同一
SQLite 以无凭据 `AgentApplication` reopen 后 `Get/Events` byte-for-byte 重放，DNS/HTTP 计数
保持不变。

唯一 official canary 使用 `deepseek-v4-flash`/low 和
`https://api-docs.deepseek.com/`，maximum reruns=0、2-request hard limit、只允许 `web_fetch`。
模型调用工具一次并完成任务；页面为 200、title `Your First API Call | DeepSeek API Docs`、
`trust=external_untrusted`，读取 7,356 bytes、返回 3,077 bytes、未截断。canonical accounting
为 2 started / 2 completed、runtime retry 0、usage/cost complete、18,646 input / 331 output
tokens，费用 `$0.002387011`，低于 `$0.10` ceiling；没有 billing unknown 或 incomplete usage。
该单次 canary 只证明 vertical usability，不宣称通用效率或成功率提升。

`./scripts/dev-dse.sh focused`、严格 workspace Clippy、`dse-tools` 全测试/check、production app
targeted/full tests、fmt 与 diff check 均通过。M41 没有引入 search、Chrome/CDP、ApplicationProbe、
视觉、第二 Provider/Runtime/Store、Web session、独立 accounting ledger、Manager/Factory/Service
或 compatibility path，也没有改造 TUI/config 的 search-provider 管理面；因此 keep。

### 39.5 ADR-0017 W1.1：public HTTP 与 transport provenance

- 状态：**完成并 keep**
- owner：`crates/tools` + 必要 `crates/app` caller/docs
- 用户变化：以前 public `http://` 固定 `web_scheme_denied`；完成后 Agent 能读取默认端口 80
  的公开 HTTP 文本，并明确知道任何 HTTP hop 都是 `plaintext_exposed/unprotected`。

W1.1 一次迁移 URL preflight、Reqwest HTTP(S) client、scheme/port、manual redirect、canonical
link extraction、authorization reason、catalog description、network fingerprint 与真实
application reopen fixture。HTTP 只允许规范化端口 80；HTTPS→HTTP 和 HTTP→HTTPS→HTTP 在
目标 DNS/connect 前以 `web_transport_downgrade` 拒绝。HTTP→HTTPS 可以升级，但最终 TLS 不会
抹去之前的明文暴露。所有 userinfo、DNS 全地址 public-unicast、connect pin、metadata、proxy、
Cookie、auth/header、GET-only、deadline、body/decompression、content type/charset 与 HTML
script suppression 边界保持。

成功与失败 outcome 区分 final scheme/security 和完整 trajectory/integrity，并记录 redirect
count/upgrade；`source_sha256_scope=received_content_replay_identity` 明确 hash 不代表 publisher
真实性。现有 JSON `ToolOutcome` 足以无损表达，预期 RuntimeEvent/State delta=0。W1.2 charset、
W1.3 sniffing 与 HTTP 非 80 exact grant 只保留为后续 loss candidate，不进入本切片。

deterministic owner tests 已覆盖 direct HTTP/HTTPS、默认/非默认 HTTP port、HTTP→HTTP、
HTTP→HTTPS、HTTPS→HTTPS、两类 downgrade、HTTP/HTTPS literal/DNS/mixed/redirect SSRF、connect
pin、failure provenance、deadline 后完整 trajectory、content/bounds/link 与 actor authorization。
真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RunStore`
fixture 已迁移为 HTTP，SQLite reopen 后 DNS/GET 仍为 1/1，没有再次访问网络。旧 HTTPS-only
client、network identity、catalog 文案、HTTP scheme 反例和单一 authorization 假设已删除。

唯一 credential-free public HTTP transport canary（maximum reruns=0）读取
`http://example.com/`：status 200、final scheme/security `http/plaintext`、trajectory/integrity
`plaintext_exposed/unprotected`、redirect 0、读取 388 bytes、返回 127 个正文字符、未截断，
`source_sha256=sha256:ff67a9d764d6a2367a187734e697f6a53217db9a21c101d410a113ca871a299d`。
该 canary 未读取 DeepSeek Key、模型/官方请求 0、费用 `$0`，只证明真实 public HTTP transport
可用，不构成质量、真实性或效率声明。

focused、`cargo fmt --all -- --check`、`cargo test -p dse-tools --locked`（366 pass / 0 fail / 2
ignored）、`cargo check -p dse-tools --locked`、真实 app caller/reopen、authority 23/23 和 diff
check 均通过。pre-integration full gate 严格只运行一次：public/authority、fmt 与 workspace strict
Clippy 通过；workspace tests 仅在既有 TUI PTY `foreign_provider...` 的并行 5 秒退出等待出现一次
`exit=None`。该 exact test 随后隔离通过，完整 `dse-tui` package 以 focused 的串行合同复核为
0 fail（773 unit、7 PTY、30 exec acceptance 及其余 integration/QA）；没有重跑 full，也没有把
TUI test/gate 修改混入 W1.1。该 false-negative 与 `crates/tools`/必要 app caller 无调用或 diff
交集，所有 workspace 测试语料最终均有 green evidence。

## 40. M42–M46：生产力后续顺序

这些里程碑保存 M42～M46 的完成顺序；M46 W2/W3/W3.1 已形成 production checkpoint。
ADR-0018 之后不再从这里机械派生一个 action 一个 Goal；active future order 由 40.13 的
capability cluster 接管。

1. **M42 TUI Run Hub（已完成）**：复用 `list_roots/resume/continue`，实现 workspace/project
   运行列表、状态、更新时间、新建、恢复和继续；不创建 Thread DB 或第二 Store。
2. **M43 Skills 可靠加载（已完成）**：提供 Host-owned exact `load_skill(name)` 或精确 discovered-path
   grant；不能读取的全局 Skill 不再向模型宣称可用。MCP/plugin 未进入模型 catalog 前保持
   管理面或隐藏，不建设 marketplace。
3. **M44 DeepSeek 原生 Web Search 决策（已完成，hold）**：最多两天、最多一到两个官方请求，验证 Anthropic
   compatibility 的 server tool result、stream、thinking、usage、finish、来源和 replay；
   若需要第二 DeepSeek wire，必须新 ADR。无完整收益则等待 Chat surface，不建 provider
   fallback chain。
4. **M45 ApplicationProbe（M45-A 已完成）**：已实现 worktree-local process、port/health、logs、
   HTTP assertion、latest-revision receipt 和 teardown/reopen；HTTP 已足够，本切片未启动 Chrome。
5. **M46 语义浏览器 W2/W3/W3.1（已完成）**：Rust/Tokio direct CDP + pinned Chrome for
   Testing 已交付 bounded navigate、opaque ref click/fill、fresh observation、Host teardown 与
   reopen；current action 仍只限 exact-loopback，完整交互和 public risk governance 转入 40.13。

### 40.1 M45-A ApplicationProbe pre-registration 与 formal result

真实问题不是缺少第二个通用 shell，而是 Agent 修改 local HTTP service 后，现有 Host verifier
只能等待前台命令退出，不能在同一 canonical receipt 中证明 process start、loopback readiness、
HTTP assertion、有界日志与 teardown。M45-A 的唯一 production owner 是 `crates/tools`；
`crates/app` 只解析/冻结真实 TaskContract caller，`crates/orchestrator` 继续只拥有 Writer
worktree，不新增进程服务或私有生命周期。

切片必须复用 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome/Artifact
-> EvidenceReceipt -> RuntimeEvent -> RunStore`。ApplicationProbe 是 Host-owned exact verifier，
不加入模型可见 catalog；启动只接受 exact program/argv、worktree 内 cwd 与有界环境，HTTP 只访问
Host 分配的 loopback port。成功或失败都要记录 status/body digest/excerpt、stdout/stderr bytes 与
truncation、exit/timeout/cancel、revision before/after 和 teardown/reap 事实，所有应用输出均为
`external_untrusted`。

被替换的旧路是用 foreground `run_verifiers`/shell 等待永久 server、再用独立 curl 或人工 kill
拼接证据；cutover 后不保留 background probe、PID sidecar、第二 Store/session 或兼容入口。
M45-A 是 Risk 2：先完成 contract/safety/caller/reopen 与真实 OS `SIGKILL` fault matrix，再跑
focused，并只在 pre-integration 对同一 revision 跑一次 full gate。HTTP 已足够时 M46 保持关闭。

实现结果保留 `crates/tools` 单一 owner：新增的 `application_probe` 只由 Host verifier 调用，
不计入或暴露到 13-tool DeepSeek catalog。Start resolver 用 Host 生成的 128-bit lease 替换 caller
plan；exact argv marker、sanitized env、worktree cwd、Host 分配 loopback port 与 no-proxy/no-redirect
GET 在 spawn 前冻结。应用 assertion 除预注册 status/body 外必须回显 exact lease identity，因此
释放临时 listener 后的 port race/foreign listener 不能 false pass。Ask/isolated Writer 的既有
network-denied sandbox 在 spawn 前拒绝；Agent/FullAccess root 走同一 canonical verifier 权限链。

正常、失败、cancel 与 timeout 都执行 owned process-tree teardown/reap。真实外部 `SIGKILL` fixture
证明 macOS/Linux reopen 从 `HostVerificationPrepared` 的 exact lease 回收 tree，保留原 event prefix，
只提交一个既有 `RecoveryRequired(HostVerification)` terminal，physical model request 保持 `1 -> 1`，
不重新 spawn/HTTP。terminal SQLite reopen 也保持 events byte-for-byte 相同且 quiet loopback accept
count 为 0。成功 receipt 只在 verifier before/after 等于最新 workspace revision 时 seal；revision
drift 为 stale。失败 body/status/log 的 bounded `external_untrusted` facts 进入同一 root Agent rework
turn，并由第二次 Host probe 闭合，不新增完成权。

deterministic M45 module matrix 为 10 pass、0 fail；完整 `dse-tools` 为 376 pass、0 fail、2 ignored，
另有 1 个 integration 与 2 个 doc pass；完整 `dse-app` 为 64 pass、0 fail、3 ignored（两个
process helper 加一个 one-shot official canary）。
旧 foreground-server + standalone curl/manual-kill 从未是 canonical production 实现，因此可删除
production adapter 数为 0；本切片也未引入 background handle、PID/port sidecar、第二 Runtime、
Store、session、daemon、service registry、browser/Chrome/CDP/Playwright 或新 protocol/state 字段。

最终 revision 的 focused gate 全绿：bounded authority 基线 17,636 行、tools owner route 2,095 行、
ceiling 4,409 行，固定边界可达率 23/23；public、Runtime 88-case conformance、tools/app/app-server、
exec、TUI/PTY 与 owner check 均通过。official DeepSeek Flash canary 只执行一次，冻结 physical
request admission 1、runtime rerun 0、output cap 64，实际没有到达 `Completed`，且第一次 harness
在 terminal 断言前未输出 durable accounting；按 maximum_reruns=0 没有再次请求。实际 usage、
费用和失败 taxonomy 因而为 unknown，不能声称 vertical usability、费用或效率提升，也不能以
该 canary 抹掉 deterministic behavior/reopen 证据。harness 已改为未来先输出 accounting 再断言，
但本切片不重跑。

pre-integration full gate 严格只运行一次。public/authority（最终 full 时 tools route 2,101 行、
fixed boundary 23/23）、fmt 与 workspace strict Clippy 通过；workspace tests 的唯一失败是本切片
`assertion_cannot_overrun_the_overall_deadline` 在并发 test binary 中，Python fixture 的释放后端口
被另一 probe 抢占，安全地产生 `application_probe_early_exit` 而不是测试预期的
`application_probe_overall_timeout`。这是 false deny/test isolation 问题，不是 timeout 越界或
foreign-service false allow。修复把进程型 probe fixtures 在同一 binary 内串行化，并将 reqwest
attempt timeout 从通用 `http_failed` 提升为 typed `application_probe_http_timeout`，同时保证测试
overall deadline 先于独立 attempt cap。exact regression、默认 `dse-tools` 376/0/2 ignored、
all-features owner package 378/0/2 ignored、owner check/strict Clippy 均随后通过；full 没有重跑。

### 40.2 M42 formal result

M42 已在现有 interactive TUI 中加入 full-screen Run Hub。无显式 `--resume`、无初始输入且
当前 workspace 存在历史 root 时，冷启动先显示按 canonical `updated_at` 倒序的运行列表；
`/runs` 可随时打开同一投影。每行来自 `ListRoots + Get`，显示 exact terminal taxonomy、UTC
更新时间、TaskContract objective、continuation 标记和有界 Run ID，并提供键盘与鼠标等价的
选择、新建和关闭动作。

选择 active root 调用现有 `Resume`；选择可继续的 terminal root 从 sequence 1 重放 Store
事件，并把它设为下一次提交的 `Continue` source；`RecoveryRequired` 只允许检查，不能被误作
continuation source。`New run` 只清除进程内的 continuation 选择，下一次提交仍由
`AgentApplication` 创建独立 root。冷重开和同进程重选都重建 fresh
`CanonicalRunProjection`，不重发模型请求，也不创建 TUI history、Thread DB、第二 Store 或
第二生命周期。

确定性证据覆盖 Run Hub 的响应式中英文渲染、状态/时间、键盘/鼠标 parity、真实
`AgentApplication` terminal reopen/continue/new-root/same-process replay、SQLite 冷重开以及
真实中文 PTY 冷启动发现。PTY 重开使用不可达 loopback endpoint 仍完成 exact terminal replay，
证明历史查看没有再次访问模型。M42 没有 material model-visible treatment，不读取 Key、不做
付费 A/B 或 canary；Run API、RuntimeEvent 与 State schema 均未升级。`dse-tui` package、
focused、严格 workspace Clippy、完整 workspace tests、fmt、check 与 diff gate 全部通过。

### 40.3 M43 formal result

M43 在 `crates/tools` 的固定 catalog 中加入第 13 个 Host 工具 `load_skill(name)`。一次
production Start/Continue 只由 `crates/context` 发现一份不可变 `SkillRegistry`，同一快照同时
驱动 root/child prompt、root/read-only child/isolated Writer executor 和 production execution
fingerprint；没有第二 discovery owner、Skill session/store 或 permission ledger。prompt 不再
暴露磁盘路径或建议用 `read_file` 读取全局文件，只列出当前 actor catalog 真正提供
`load_skill` 时可加载的精确名称和说明。

Host 在发现时完整读取普通 UTF-8 `SKILL.md`，128 KiB 为 admission hard limit；不可读、无效、
过大以及同一 precedence cell 归一化后歧义的定义都不进入 prompt 或 loader。跨 discovery root
继续使用既有显式 precedence，但更高优先级 cell 的歧义会阻止同名低优先级 fallback。
`load_skill` schema 只有 `name`，拒绝大小写/空格别名、任意 path 和额外字段；成功返回完整未
截断 body、精确 source path、完整 source byte count/SHA-256、returned bytes 和
`trust=external_untrusted`。文件在 run 内变化或删除不改变已冻结结果；live resume 重新发现后的
snapshot hash 不同则由既有 execution fingerprint fail closed。

真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome -> RunStore`
loopback 中模型选择 `load_skill`，committed JSON outcome 原样进入下一次 model request。提交后
删除源 Skill，再以无凭据、零请求 application reopen，`Get/Events` 与原 SQLite events 完全
相同。actor catalog hash/parity、ToolPolicy 隐藏时 prompt 同步隐藏、typed preflight failure、
普通只读 authorization、不可变快照和 system-skill 安装升级均有确定性测试。

该切片属于 EVALUATION 已准入的 exact Skill grant，不是成功率/效率 treatment；Key 未读取，
official DeepSeek requests=0、actual canary cost `$0`，没有 Token/费用/效率声明，也没有产生需
provider usage 补全的 accounting observation。现有 `ToolOutcome` 已能无损表达结果，Run API、
RuntimeEvent、State schema 均未升级。MCP/plugin 继续不进入模型 catalog；marketplace、search、
browser、第二 Runtime/Store/permission owner 和 M44 以后能力均未引入。

### 40.4 M44 formal result

M44 结论为 `hold_wait_for_chat_surface`，没有把 `web_search` 加入 production catalog。DeepSeek
当前 ChatCompletions reference 明确只接受 function tools；官方 Anthropic compatibility 虽把
`server_tool_use`、`web_search_tool_result`、stream 与 thinking 标为 supported，却没有发布
Web Search 的 DeepSeek result 子字段、SSE/finish fixture、thinking signature/replay 规则或
search-specific usage/price 合同，并明确忽略 citations、拒绝 `search_result` input。

production audit 证明当前 `ApiSurface` 只有 Standard/Strict Chat，sender 只拥有
`/chat/completions` 与 `/beta/chat/completions`，parser 只接受 Chat `choices/message/delta`，而
canonical `ModelMessage/ModelOutput/TranscriptEntry` 只能无损保存 string content、单一
`reasoning_content` 和 Host client function calls。Anthropic server-tool call/result、content
block 顺序、来源、opaque thinking signature、`pause_turn` 与 Messages usage/finish 不能经这条
链无损表达。采用该路线需要新的 request/response/SSE wire 与 transcript/replay/accounting
映射，触发 ADR-0015 要求的新 ADR；M44 没有授权或预建这些结构。

本机没有可用 DeepSeek credential，因此可选 canary 未执行：official requests=0、Key 未读取、
actual cost `$0`、maximum reruns=0。费用仅作为状态记录，不是本次产品 hold 的核心理由；若
未来 usage 不完整，只停止精确费用声明与后续付费重试，不抹掉已闭合的 behavior evidence。
本次决定由 Chat surface 缺口、第二 wire 成本和无法冻结 DeepSeek 官方 replay fixture 共同产生。

M44 同时删除没有 executor 的 TUI/config search-provider 枚举、`[search]`、`DSE_SEARCH_*`
reader 以及文本/JSON Doctor projection；遗留配置明确 fail closed。它没有增加 provider
fallback chain、search HTML scraping、Anthropic production transport、Runtime/Store、protocol/
State version、ApplicationProbe、CDP/browser、视觉、MCP marketplace 或 M45 以后能力。

用户此前不能让 Agent 从未知问题发现公开来源；M44 后仍不能，不能把 decision slice 冒充能力
交付。实际改善是配置与 Doctor 不再显示一条不存在的搜索能力，且未来只在 DeepSeek Chat
提供可冻结、可重放的 search surface，或新 ADR 接受完整第二 wire 后重开。下一候选是 M45，
本切片不启动它。

离线门已通过：M44 config targeted tests、`cargo check -p dse-tui --locked`、
`cargo check -p dse-deepseek --locked`、focused gate、strict workspace clippy、workspace tests、
fmt check 与 diff check 均为绿色。

### 40.5 ADR-0016 首个实现 Goal（已完成）

真实问题是 M44 clean checkpoint 的默认 development bootstrap 仍需完整读取 Product Plan、
全部 ADR、Roadmap、Evaluation 和 Current Architecture，共 17,636 行，并在根 guide 与脚本间
重复写出 full gate。唯一 owner 是 repository guidance 与现有 product/architecture/decision
authority；机械检查只进入已有 `scripts/check-public-repository.py` 和 `scripts/dev-dse.sh`。

首个纵向切换是：根有界地图 -> `docs/README.md` owner route -> accepted ADR/current facts ->
根 Risk 0～4 contract -> 唯一 executable gate。真实 caller 切换后删除旧 unconditional
full-read 和根 guide 的重复 command list；不压缩历史正文，不创建第二 Roadmap/tracker。

最低证据为 M44 基线、六个预注册问题、十一条 owner route、actual read-set/line report、固定
边界 100% 可达、Risk 0 focused gate 和同一 revision 仅一次 pre-integration full gate。
production Rust delta=0、DeepSeek official requests=0；M45/M46 不启动。

正式结果为 `keep_bounded_authority_and_risk_tier_gate`。repository-guidance 实际 mandatory
bootstrap 为 1,024 行（基线的 5.81%），十一条 route 的 worst case 是 tools 1,755 行
（基线的 9.95%，低于 4,409 行 hard ceiling）；六个预注册问题全部有精确 authority，固定
边界 22/22 可达。M44 的 17,636 行 unconditional full-read 和根 guide 重复 full-gate
command list 已删除，历史结果中的命令只作为已发生证据保留，不是 active caller。

Risk 0 authority/public/diff gate 通过；canonical `./scripts/dev-dse.sh full` 对 final code/script
candidate 只执行一次，fmt、strict workspace clippy、workspace tests 和 diff check 全部通过。
full 后只增加本段结果投影并重新运行 Risk 0 gate。没有 Rust source、DeepSeek wire/Prompt、
Runtime/Event/Store 或产品 capability 变化，没有读取 Key 或启动 M45/M46。

在 M41–M46 期间继续停做：多 Writer/swarm、FIM、RepoGraph/LSP、视觉 placeholder、Firecrawl/
Playwright sidecar、MCP marketplace、独立大文件重构，以及不绑定正在交付能力的付费 loss
acquisition。每个里程碑必须写出“以前用户不能 X，现在可以 X”，并在 3–5 个工作日内产生
用户可见纵向结果，否则缩小或停止。

### 40.6 ADR-0016 continuation/Harness 离线复核（已完成）

真实问题是：current durable replay 是否仍在长任务 continuation 上重复丢失目标，以及 DSE
当前 Harness 的额外复杂度相对 same-DeepSeek minimal loop 到底承担哪些可复查保证。owner 是
现有 Evaluation/authority 与 `scripts/eval-*`；本切片不创建第二 Runtime、Store、planner 或
production benchmark owner。

continuation 准入门保持“同一 current loss 至少跨两个独立 admissible task”。历史证据复核为：

- M12 的 11 条 canonical trajectory 得到 current product loss 空集合；
- M13 三次采集由 evaluator contract instability 停止，不能重解释为 production loss；
- M36-A 的三个独立 long-horizon task 均在一次 SIGKILL/reopen 后完成，exact resume `3/3`，
  reopen 没有新增 physical request。

因此 observed/required 是 `0/2`，决定
`keep_current_harness_reject_verified_milestone_no_repeated_loss`。旧的 anecdote-driven candidate
admission 被离线 manifest 的机械门替代；`VerifiedMilestoneProjection` 没有 production
scaffold、selector、Store 字段或 caller 可保留，故不实施也不预建。

same-DeepSeek baseline 固定 `deepseek-v4-pro/high` 与 Standard streaming ChatCompletions，比较
面只改变 Harness。minimal contract 仅含 transport/parser、process-local transcript、function
dispatch 与 Bash/file edit 四个概念；DSE current 在同一比较面增加 TaskContract、canonical
RuntimeEvent、RunStore、typed actor authorization、latest-revision Host receipt/completion 与
Writer worktree lifecycle 六个概念。结构化结果为：

| cell | contract capabilities | comparison concepts | 证据边界 |
|---|---:|---:|---|
| same-DeepSeek minimal loop | 1/7 | 4 | comparator definition；未执行模型或质量测量 |
| DSE current | 7/7 | 10 | 六条 exact Rust owner tests + M36-A long-horizon `3/3` |

能力项是 basic tool loop、Host 拒绝未验证完成、committed outcome 无重执行 reopen、in-flight
model fail closed、durable typed authorization、Writer seal/integrate/verify/cleanup 与 interactive
SIGKILL/reopen completion。概念计数是这个 comparison surface 的有界 inventory，不是全项目
LOC，也不证明 DSE 在成功率、Token、费用或时间上优于 minimal loop。要形成质量或效率结论，
仍必须另行执行 fresh same-task/budget held-out comparison 和完整 accounting；本切片
`product_metric_eligible=false`。

冻结合同为 `eval/manifests/adr0016-harness-isolation-offline-v1.json`，唯一离线 evaluator 为
`scripts/eval-adr0016-harness-isolation.py`，它不实现 Agent loop、不读 credential、不访问网络。
manifest/source validation 与六条 exact offline Rust gate 均通过；canonical Risk 0 authority/diff
gate 通过。production Rust、DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、
RunStore 均无变化，official requests=0、actual cost `$0`、maximum reruns=0；M45/M46 未启动。

### 40.7 M46 read-only semantic browser admission audit（已完成）

真实问题不是“竞品有浏览器”，而是 M45-A 后 current `web_fetch + ApplicationProbe` 是否在至少
两个独立、真实 JS-only local application task 上重复无法产生 latest-revision 对应的 bounded
DOM/accessibility evidence。唯一审计 owner 是现有 Evaluation authority；control 直接调用
`crates/tools::ProductionToolExecutor`，Playwright 只作为 credential-free、loopback-confined
eval oracle，不进入 production dependency graph。

本切片替代 anecdote/feature-list-driven browser admission。预注册 manifest
`eval/manifests/m46-semantic-browser-admission-v1.json` 在执行前冻结 baseline
`997c67e20eb6`、两个 task id、独立性 key、同一 loss code、role/name/state、fixture SHA-256、
false-success=0 和 same-loss threshold=2：

| task | eval-only rendered observation | current `web_fetch` | current `ApplicationProbe` | loss |
|---|---|---|---|---|
| `m46_js_status_hydration` | `status / Deployment ready / data-state=ready` | script suppressed；rendered name absent | healthy；`application_probe_body_mismatch`；failed verdict；teardown settled | `tools:application_visibility` |
| `m46_js_switch_state` | `switch / Automatic retries enabled / aria-checked=true` | script suppressed；rendered name absent | healthy；`application_probe_body_mismatch`；failed verdict；teardown settled | `tools:application_visibility` |

oracle verified=`2/2`、control verified=`0/2`、control false-success=`0`，两项 failed
observation 都与 `ToolOutcome` 的 exact known workspace revision SHA 一致，因此
observed/required=`2/2`，决定为
`admit_next_goal_read_only_semantic_browser_w2_contract_only`。它只授权下一独立 Goal 在
`crates/tools` 完成 eval-only Rust CDP lifecycle/dependency spike 后，交付
`browser_navigate + bounded DOM/accessibility snapshot + Host-owned teardown`。该实现必须先冻结
pinned Chrome for Testing identity/SHA-256、profile/process cleanup，以及可强制的 public 或
exact local-origin egress guard；action、search、screenshot、vision、登录/user profile、
Node/Playwright production sidecar 与第二 Runtime/Store/session/accounting ledger 继续禁止。

第一次 evaluator implementation attempt 在第一个 ApplicationProbe control 后停止：production
已正确返回 body mismatch，但 test-only observer 错误要求 failed verifier observation 完全不存在。
current contract 实际会保留 typed `VerifierVerdict::Failed` 供 rework，且不能生成成功 receipt。
断言修正为 exact failed verdict 后，完整预注册矩阵一次闭合；首次停止未运行 browser oracle、
未形成准入结果，也没有被重解释为 product loss。

本阶段以前不能以可复查证据启动 M46；现在下一 read-only W2 Goal 获得有界准入，但用户仍不能
在 current production Agent 中浏览 JS 页面。production Rust/Cargo/DeepSeek wire/Prompt、13-tool
catalog、AgentRuntime、RuntimeEvent、RunStore delta 均为 0；browser tools/dependencies added=0。
DeepSeek Key 未读取、official/model requests=0、actual cost `$0`、`product_metric_eligible=false`；
费用只是状态事实，不是准入原因。canonical focused gate 全绿：authority/public、`dse-tools`
376/0/2 ignored、M46 integration 1/1、DeepSeek 61/0/1 ignored、Runtime conformance 88/88、app
64/0/3 ignored、app-server 23/23、exec 30/30、TUI run 20/20、PTY 7/7 与 owner check 全部通过。
没有 full gate，因为这是 production delta=0 的 eval-only admission/authority slice。

### 40.8 M46 W2 read-only semantic browser（已完成）

真实问题是 canonical `web_fetch` 不执行 script，而 ApplicationProbe 只验证 process/HTTP；root Agent
因此无法读取 JS hydration 后才存在的 role/name/text/state。唯一 production owner 是
`crates/tools`，旧路是依赖 eval-only Playwright oracle 或让模型从 raw script 猜 rendered state；
cutover 后 Playwright 仍只属于 frozen admission evaluator，不是 production caller。

W2 先在仓库外完成两条 Rust spike。`chromiumoxide 0.9.1` 因 standalone 161 dependency nodes、
149 packages、约 2.64 GiB first-build peak RSS 与约 60K generated CDP types 被删除；选择 direct
`tokio-tungstenite 0.30.0`，standalone dependency nodes 60、first-build peak RSS 约 237 MiB。
production 只新增该 pinned WebSocket dependency，不引入 Node/Playwright sidecar。

Chrome for Testing 固定为 mac-arm64 `151.0.7922.47`，archive SHA-256
`9529990b6afd9867a862c7a5bff2a4a8eef84614d910acac22e4c5fa5c24daee`，executable SHA-256
`e9e1c766953cf2ff5ea38c6cb63fa32b443a958c3fda7dcc3b60dd9b20436855`。Host 只验证预安装文件；
不下载、不自动更新。每次调用创建独立 profile、loopback egress proxy 与 process-tree owner，完成、
失败、取消和 deadline 后都回收。

固定生产 catalog 从 13 增至 14，只加入 `browser_navigate(url, max_nodes?, max_chars?)`。public target
限默认端口 HTTP(S)，exact local target 限 Host 注入的 literal-loopback origin；CDP interception +
connect-pinned proxy 每个 request/DNS/connect/redirect 重验 SSRF/origin/method，并剥离 Cookie、auth、
referer。跨 origin Document、非 GET/HEAD、下载、service worker、QUIC、非代理 WebRTC、private/
metadata target 均 fail closed；额外 page/worker/popup target 通过 auto-attach 在启动暂停态由 Host 关闭，
不能绕过 primary target interception；wire/decoded body、CDP frame、node、char、redirect、deadline 均有界。

真实 pinned-CfT local fixture 从不含目标 literal 的 script 产生并读取
`status / Deployment ready / data-state=ready` 与
`switch / Automatic retries enabled / checked=true`；取消 fixture 的 process/proxy/profile teardown
全为 true，fixture 的 `window.open` 也未产生任何 popup HTTP request。真实 `AgentApplication ->
AgentRuntime -> ProductionToolExecutor -> ToolOutcome ->
RuntimeEvent -> SQLite RunStore` loopback 中模型选择 `browser_navigate`，reopen 只重放且 navigate
call count 保持 1。protocol/state、DeepSeek wire/model-visible Prompt delta=0；official DeepSeek
requests=0、credential read=false、actual cost `$0`，不作效率或通用成功率声明。

本地确定性门闭合后只执行一次 credential-free public HTTPS canary：production path 成功读取
`https://example.com/` 并返回非空 bounded snapshot，process/proxy/profile teardown 全 settled；
maximum reruns=0。它没有调用 DeepSeek，也不构成多站点或产品效率评测。

W2 未加入 action、登录、Cookie/storage 持久化、截图、视觉、搜索、用户 Chrome profile、browser
session/store/accounting ledger、第二 Runtime/Store 或后台 daemon。下一阶段不能自动启动 W3；只有
W2 后新的重复 `browser_interaction` loss 才能另行准入 ref-based action Goal。

canonical focused gate 全绿：authority tools route 2,174/4,409、fixed boundary 23/23、tools
385/0/5 ignored、DeepSeek 61/0/1 ignored、Runtime 88/88、app 65/0/3 ignored、app-server 23/23、
exec 30/30、TUI 20/20、PTY 7/7。三条 ignored W2 tests 另行按合同执行为 pinned local 2/2、
credential-free public HTTPS 1/1；public canary 未重跑。

本切片按 security/catalog production delta 运行一次 pre-integration full gate 并全绿，覆盖 workspace
all-features、strict Clippy、crash/reopen、exec/TUI/PTY 与 doctests。其后人工审计补上 additional-target
start-paused/close guard；最终 revision 的 pinned local 2/2、owner strict checks、caller/reopen 与 focused
gate 全绿。遵守一次 full 上限，没有第二次 full。official DeepSeek requests 保持 0。

### 40.9 M46 post-W2 browser interaction admission（已完成）

真实问题是 W2 能读取 JavaScript 渲染后的语义状态，却只提供 one-shot observation：snapshot 没有
`element_ref`，catalog 没有 click/fill/press/wait。单个 demo 或混合 action family 不足以准入 W3，
因此本阶段只做 credential-free、eval-only repeated-loss audit，baseline 为 clean W2
`83b8bf455bffbe492fbbe32ff2fe88dbb5631878`。

预注册恰好两个独立真实 loopback application task，二者使用同一 `click` action family：

| task | current W2 target | required post-click state |
|---|---|---|
| `m46_interaction_deployment_approval` | `button / Reveal deployment approval` | `status / Deployment approved / data-state=approved` |
| `m46_interaction_retry_toggle` | `switch / Automatic retries disabled / aria-checked=false` | `switch / Automatic retries enabled / aria-checked=true` |

raw HTTP 不含 action target 或 post-action accessible name literal。exact production `web_fetch` 的
post-action observation 为 `0/2`；pinned-CfT production `browser_navigate` 对两个 target 均可见，但
element refs=`0`、action tools=`0`、post-action observation=`0/2`，process/proxy/profile teardown
全部 settled。现有 AgentApplication loopback/reopen regression 同时通过，committed outcome 不重导航。

eval-only oracle 使用 Node `v24.18.0`、Playwright `1.61.0` 和 pinned CfT `151.0.7922.47`，只允许
exact literal-loopback origin，每任务按 exact role/name click 一次；输出上限为 2 nodes / 4,096 bytes，
截图与坐标为 0，context/browser 均关闭。正式矩阵只运行一次、reruns=0：oracle=`2/2`、control
verified=`0/2`、同一 `tools:browser_interaction:click=2/2`、false-success=`0`、W2 teardown/replay
regression 均通过；result SHA-256 为
`490a8ca323ad1433c5680c89da84463fdd4f34ddcab800fe063e3e8c41fe17aa`。

决定为 `admit_next_goal_ref_based_browser_click_w3_contract_only`。下一 Goal 只允许
`crates/tools` owner 的 latest-snapshot/page-epoch opaque ref、单一 `browser_click` 和 mandatory
fresh post-action observation，并必须先闭合 stale/hidden/disabled/detached/ambiguous ref、origin/
side-effect、crash-after-start、authorization/catalog 与 reopen negative gates。本阶段没有实现 W3；
fill/press/wait、登录、Cookie/storage、public POST/upload/download/auth、用户 profile、截图/坐标/
视觉、搜索、Node/Playwright production sidecar、durable browser session truth 与第二
Runtime/Store/ledger 仍禁止。

production Rust、Cargo dependency、fixed catalog、DeepSeek wire/model-visible Prompt、RuntimeEvent、
RunStore、State schema 与 UI delta 全为 `0`；official DeepSeek requests=0、credential read=false、
actual cost=`$0`、`product_metric_eligible=false`。这是能力准入证据，不是 W3 实现、成功率、Token、
时间或费用提升声明。Risk 0 canonical focused gate 一次通过：authority baseline=`17,636` 行、
ceiling=`4,409` 行、最大 tools route=`2,212` 行、fixed boundary=`23/23`；tools=`385/0/5
ignored`、DeepSeek=`61/0/1 ignored`、Runtime=`88/88`、app=`65/0/3 ignored`、app-server=`23/23`、
exec=`30/30`、TUI run=`20/20`、PTY=`7/7`。本切片不运行 full。

### 40.10 M46 W3 ref-based browser click（已完成）

真实问题是 W2 已能看见两个 action target，但 Agent 仍不能取得 post-click state。唯一 owner 是
`crates/tools`；旧路是 exact-loopback navigate 后无条件 teardown，以及仅供 admission 使用的
Python + Node/Playwright role/name oracle。cutover 一次迁移为同一 direct-CDP lifecycle 的
latest-epoch opaque ref 和唯一 `browser_click(element_ref)`；旧 refs/action-tools=`0` Rust assertion、
Python evaluator 与 Node oracle 已删除，冻结 manifest/result/history 未改写。

public navigate 继续 one-shot/read-only/settled teardown。只有 Host 注入的 exact loopback application
origin 保留一个 same-run in-memory session；refs 绑定 random browser identity、snapshot 和 page epoch，
不接受 CSS/XPath/坐标/script。click 前重取 DOM/AX 并对 cross-run、stale、missing、hidden、disabled、
detached、ambiguous 和 identity drift fail closed；click 后强制 fresh bounded observation、旋转 epoch/
snapshot/refs，旧 ref 不可再用。action egress 继续拒绝 origin escape、非 GET/HEAD、popup/worker、
download、Cookie/auth 与 public side effect；不闭合的 post-dispatch path 使用既有
`Indeterminate/Unsafe + RecoveryRequired`，不会自动 click。

两个冻结 fixture 真实运行 `browser_navigate -> browser_click`，分别得到
`status / Deployment approved / data-state=approved` 和
`switch / Automatic retries enabled / aria-checked=true`：verified=`2/2`、false allow=`0`。真实
AgentApplication loopback 中模型依次选择 navigate/click，committed click SQLite reopen 后调用计数
保持 `1/1`；started-without-outcome reopen 为 RecoveryRequired、click replay=`0`。root 可见；
coordinator/read-only child 由既有 actor catalog 排除；isolated Writer 由既有 network sandbox 拒绝。

fixed Host catalog 从 14 增至 15；production dependency、DeepSeek wire/model-visible Prompt、
AgentRuntime、RuntimeEvent、RunStore 和 State schema delta=`0`。official DeepSeek requests=`0`、
credential read=`false`、actual cost=`$0`；费用仅记录状态，不作为准入理由。本切片没有加入
fill/press/wait、public action、POST/upload/download/auth、登录、Cookie/storage 持久化、用户 Chrome
profile、截图/视觉、搜索、Node sidecar、第二 Runtime/Store 或 session ledger。Risk 2 的 focused/full
最终数字以本 checkpoint 的 gate 结果为准。

### 40.11 M46 post-W3 browser interaction admission（已完成）

真实问题不是“click 后应补齐完整 browser API”，而是 current
`web_fetch + browser_navigate + browser_click` 是否在两个独立真实 local application task 上仍无法
产生同一 action-family 的 post-action evidence。本阶段唯一 owner 是 Evaluation authority；baseline
固定为 clean W3 `64b83a21b7859e38ca7ce43a4a035c6337702d36`，production control 使用 root
`Agent` permission 下的真实 `ProductionToolExecutor`，没有改变 production Rust。

预注册恰好两个彼此独立的 text-entry task：

| task | current target | required post-fill state |
|---|---|---|
| `m46_post_w3_release_channel_fill` | `textbox / Release channel` | `status / Release channel set to canary / data-channel=canary` |
| `m46_post_w3_test_filter_fill` | `searchbox / Test filter` | `status / Test filter applied: network / data-filter=network` |

target、fill value 与 post-fill name/state 均不以 literal 出现在 raw HTTP。production `web_fetch` 不执行
script；`browser_navigate` 能观察精确 target，但 current click-only ref policy 对两个 text-entry target
都返回 refs=`0`，15-tool catalog 没有 `browser_fill`，真实 Runtime preflight 为
`UnknownTool + NotApplied`。control verified=`0/2`、false-success=`0`。

eval-only oracle 使用 Node `v24.18.0`、Playwright `1.61.0` 和 pinned CfT `151.0.7922.47`，每个
task 只按 exact role/name fill 一次，只允许 Host-assigned literal-loopback origin 与 GET/HEAD，输出上限
2 nodes / 4,096 bytes，截图/坐标/external requests=`0`，context/browser 全关闭。oracle verified=`2/2`，
因此同一 `tools:browser_interaction:fill=2/2` 达到 repeated-loss threshold。真实 W3
AgentApplication committed navigate/click SQLite reopen 保持调用计数 `1/1`，pinned-CfT retained session
teardown regression 同样通过。

决定为 `admit_next_goal_ref_based_browser_fill_w3_1_contract_only`。它只允许下一独立 Goal 在
`crates/tools` 为 eligible latest-epoch text-entry target 扩展 opaque ref，并新增唯一
`browser_fill(element_ref, value)` 与 mandatory fresh observation；stale/missing/hidden/disabled/
readonly/detached/ambiguous ref、password/file/non-text input、过长/控制字符 value、origin escape、非
GET、external side effect、crash-after-start、authorization/catalog 和 committed reopen 必须先作为
负向门闭合。press/wait、click+fill macro、任意 submit、登录/secret、Cookie/storage、public POST/
upload/download/auth、用户 profile、截图/坐标/视觉、搜索、Node production sidecar、第二
Runtime/Store/session ledger 继续禁止。

完整正式 evaluator result 只形成一次、reruns=`0`，SHA-256 为
`71afb4a6ce866e2e47eb68ad4001c8c586549172c3b414221fedb40b4c76391f`。此前两次 harness
implementation attempt 均在 oracle 前停止：一次把 direct execute 误当真实 Runtime preflight，一次
使用系统 Python 3.9 不支持的 `zip(strict=True)`；它们没有形成准入结果、oracle action、外网或模型
请求。11/11 negative self-test、真实 control/oracle/reopen/teardown 与 targeted/authority gate 结果见
Evaluation 当前条目。本阶段 production Rust/Cargo/catalog/DeepSeek wire/Prompt/RuntimeEvent/RunStore/
State schema delta=`0`，official requests=`0`、credential read=`false`、actual cost=`$0`；不运行 full，
也不形成成功率、Token、时间或费用提升声明。

### 40.12 M46 W3.1 ref-based browser fill（已完成）

真实问题是 post-W3 audit 已证明两个独立 text-entry task 需要同一个 fill family，但 production 仍只有
click-only refs 与 15-tool catalog。唯一 owner 是 `crates/tools`；旧路是 eval-only Python +
Node/Playwright role/name fill oracle 和 test-only `UnknownTool` control。cutover 在同一个 Rust
direct-CDP、same-run in-memory exact-loopback lifecycle 中扩展 capability-bound refs，并只新增
`browser_fill(element_ref, value)`；上述三个旧 caller 路径已物理删除，冻结 manifest/summary/
fixtures/history 保留。

Host 只给 visible、enabled、非 readonly、非敏感 `input[type=text|search]` 返回 fill-only ref；ref
绑定 run、browser、backend DOM node、latest snapshot/page epoch 与 exact fill capability，click/fill
不可交叉消费。value 非空且最多 1,024 chars / 4,096 UTF-8 bytes，NUL、C0/C1、DEL、换行和 schema
外字段在 harness 前拒绝。password/file/date/color/number、textarea/contenteditable 与 login/secret/
token/API-key/credential/OTP target 均不获 ref。

fill 前 Host 重取 DOM/AX/layout identity 并拒绝 cross-run、stale、missing、hidden、disabled、readonly、
detached、ambiguous、identity/capability drift；内部 fixed CDP sequence 只有 focus、替换当前值与 insert
text，不开放 selector/CSS/XPath/坐标/JS/key/Enter/submit/blur。action 后必须取得 fresh bounded
`external_untrusted` observation 并旋转全部 refs/epoch；首次 focus 后的 crash/cancel/timeout/transport
ambiguity 进入既有 `Indeterminate/Unsafe + teardown`，started-without-outcome reopen 为
`RecoveryRequired` 且不自动 replay。

两个冻结 fixture 的 production `navigate -> fill -> fresh observation` 均通过：initial epoch=`1`、
post-fill epoch=`2`，分别观察到 `Release channel set to canary / data-channel=canary` 与
`Test filter applied: network / data-filter=network`；旧 ref reuse 为 `NotApplied` 并返回 epoch=`3`
fresh observation。真实 AgentApplication loopback 中模型选择 navigate/fill，committed SQLite reopen
的调用计数保持 `1/1`；started-without-outcome 的 model/fill replay=`0`。

fixed Host catalog 从 15 增至 16；root 可见，coordinator/read-only child 按既有 MayWrite actor policy
不可见，isolated Writer 由既有 network-denied sandbox/cleared local-origin grant 拒绝。production
dependency、DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore 与 State schema
delta=`0`。official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`；费用只是状态显示。
没有加入 press/wait、登录、public action、POST/upload/download、Cookie/storage persistence、用户
Chrome、截图/视觉、搜索、Node sidecar、第二 Runtime/Store 或 session ledger。Risk 2 最终 focused/full
证据以 Evaluation W3.1 条目为准。

### 40.13 ADR-0018 工程完全体方向审计与 capability-cluster cutover（已完成）

#### 问题、owner、旧路与结果

真实问题是：W1～W3.1 已证明 fetch/CDP/ref/replay 的底层安全机制，但 ADR-0015 又把每项初始
exclusion 固化为长期拒绝，并形成 `one action -> admission audit -> implementation -> next action`
循环。该路线可以持续增加负向测试，却不能在合理时间内形成完整工程 Agent。

本 audit 的 single owner 是 product/repository authority；production crate delta 必须为 0。旧 active
path 是 ADR-0015 的 loopback-only/per-action repeated-loss 后续规则、Roadmap 的下一个小 action
派生和 Evaluation 对所有 baseline capability 的 mandatory off-A/B。cutover 后：

- ADR-0018 是完整工程能力治理的 accepted decision；
- Product Plan 明确 task completeness 与 risk governance；
- ADR-0015 的 W1～W3.1 历史事实保留，但长期 blanket deny/one-action route 被部分 supersede；
- Roadmap 只排 capability cluster；Evaluation 用真实端到端 task、人工介入、返工、恢复、receipt
  和复杂度验收，不用测试数量冒充成果；
- accounting 继续按 ADR-0011 作为正交状态：不完整会阻止费用/Token/效率声明和下一付费请求，
  不删除已经闭合的 behavior evidence。

审计对“当前能否形成可替代完整工程产品”的答案是 **不能**。current catalog 已有 16 个 Host
tools 和强 replay/evidence 基础，但没有 canonical search、完整 semantic interaction、public action
scoped grant、managed login/session、upload/download、selective visual observation，也没有把这些能力
和 code/Writer/recovery 组成持续 production dogfood 的 release gate。费用显示不是 hold 原因。

#### 永久边界与阶段性缺口

永久保留：isolated profile/egress、opaque semantic ref、Host credential/redaction、typed
side-effect/recovery、unknown side effect 不自动重放、fresh observation、latest-revision completion、
resource bounds/teardown、high-risk confirmation 和 single Runtime/Event/Store。

阶段性缺口：loopback-only、click/fill-only、无 press/wait/scroll/select/textarea/contenteditable/
back/tab/multi-page、无 public action、managed session、login、upload/download、search 或 visual。
这些 capability 在执行边界未闭合时暂时 fail closed，但不得继续写成最终 deny。

继续拒绝的机制是 unrestricted eval/JS、无边界 shell/network、secret-to-model、默认个人 Chrome、
unknown side-effect replay、无确认 destructive/financial/publish、第二 Runtime/Store、production
Node/Playwright/Firecrawl sidecar、Provider marketplace 和空 Manager/Factory/Service/vision scaffold。

#### 后续唯一顺序

1. **Semantic Interaction + Public Action Governance**：首个 production cluster，owner=
   `crates/tools`。一个 Goal/一个 matrix 完成 press、typed wait、scroll、select、textarea/
   contenteditable、back/tab/multi-page、public reversible action，以及 exact preview/scoped approval/
   result receipt；不再拆成每个 action 的 admission Goal。
2. **Managed Browser Session**：复用同一 CDP/permission/outcome owner，加入 project-isolated
   login/session、Host credential、Cookie/storage clear、workspace-granted upload 和 isolated/scanned
   download；默认仍不读个人 Chrome。
3. **Canonical Search/Research**：一个 `web_search` surface，与 `web_fetch`/browser 分工，完成
   unknown question→source selection→read→cross-check→citation；不恢复 provider marketplace 或
   search-HTML scraping 假合同。
4. **Visual Re-entry**：仅在官方 DeepSeek production multimodal wire 同时闭合 tool/thinking/
   stream/usage/replay 后，做 selective screenshot/visual vertical refactor；现在不预建接口。
5. **Integration/Dogfood/Release**：不是最后才做的附录，而是每个 cluster 的水平门。持续运行跨
   code、app、Web、Writer 和 crash/recovery 的真实任务，优先清理分支、重复 evaluator、过时
   docs/tests 与无 consumer 路径。

首簇 frozen task families、permission ladder、negative/recovery matrix、old-path deletion 和五步
integration order 见 ADR-0018。若 3～5 天不能得到一个完整 vertical task family，缩小任务/matrix，
不能退回 one-button/one-Goal，也不能放松永久边界。

#### 本切片边界与验证

本 audit 只修改 stable guide、Product Plan、accepted decision/index、唯一 Roadmap/Evaluation 和
Current Architecture。production Rust/Cargo、DeepSeek wire/model-visible Prompt、AgentRuntime、
RuntimeEvent、RunStore、State schema、catalog、UI 与 frozen manifest/raw/summary/history delta 均为
`0`。official DeepSeek requests=`0`、credential read=`false`、actual cost=`$0`；没有启动首个
production cluster。Risk 0 authority gate 的 actual bootstrap 为 repository-guidance=`1,360` 行、
worst-case tools=`2,639/4,409` 行、fixed boundary=`24/24`；link/authority check 通过。最终
`git diff --check` 与 clean reviewable commit 由同一 checkpoint 闭合，不运行 full gate。

### 40.14 Semantic Interaction + Public Action Governance（已完成）

#### 问题、owner、旧路与 cutover

真实问题是：W3.1 之后 root Agent 仍只能在 exact loopback page 上 click/fill，不能编辑 textarea 或
contenteditable，不能 press/wait/scroll/select/back/tab，也不能在 public origin 上完成即使可撤销且经
用户授权的工程动作。验收不是新增按钮数量，而是一个 local SPA 与一个 disposable public workflow
都通过真实 production chain，external negative false allow=`0`，committed reopen 不访问网络，started-
without-outcome 不重放。

single owner 是 `crates/tools`；`crates/app` 只迁移 composition 与真实 caller。cutover 把 fixed catalog
从 16 收敛为 15：物理删除独立 `browser_click`/`browser_fill` catalog/schema/dispatch 和两个旧 integration
test path，由唯一 `browser_interact` tagged schema 接管。同一 Rust direct-CDP harness/ref registry/page
epoch/egress/ToolOutcome owner 被复用；没有 compatibility flag、第二 Runtime/Store、Provider、browser
Agent、Node/Playwright/Firecrawl sidecar、Manager/Factory/Service 或新依赖。

#### 纵向结果与权限治理

`browser_interact` 一次交付 click、fill、press、typed wait、bounded scroll、native select、back、
tab open/switch/close、最多三页状态与 Host-classified submit；textarea/contenteditable 进入既有 opaque
capability ref/fresh-observation 闭环。password/file/login/secret/token/credential/OTP 不获得敏感能力；
模型没有 selector、坐标、任意 key、JS/eval、header、Cookie、认证、proxy、Chrome flag 或路径输入。

public routine action 绑定 same-origin/current epoch：Ask 显示 exact origin/target/parameters/impact 后批准，
Agent 自动执行。只有 same-origin reversible draft POST 获得 submit ref；Ask/Agent 在执行前必须批准，
FullAccess 才可直接执行。publish/delete/purchase/buy/send/message、origin/scope escape、非 exact POST 与
敏感非空 successful control 均 fail closed。批准绑定一次 invocation、workspace revision、canonical
non-sensitive parameter hash 与 impact；成功保存 HTTP status、header receipt、semantic receipt、fresh
observation 和 observed time。

`crates/app` 删除 root Agent 的 broad full-access workaround：Ask/Agent 现均是 workspace-write，只有
Agent 的 sandbox network bit 为 true；受控 Web 工具仍由 Host URL/egress/authorization owner 执行，
isolated Writer 由 `actor_controlled_network_denied` 拒绝。RuntimeEvent、RunStore、State schema、
DeepSeek wire/model-visible Prompt delta=`0`。

#### 实际证据、返工与剩余边界

repository-pinned CfT `151.0.7922.47` 的 credential-free test 在 7.21 秒内完成 local text editing、
press/select/scroll/wait/back/tab/multi-page 与 mapped-public draft POST；task families=`2/2`、negative false
allow=`0`，transport/semantic receipt 均为 `draft-receipt-001`。真实 AgentApplication public submit 先产生
exact Runtime approval，再执行一次；committed SQLite reopen 的 submit count 不增加。click/fill 两个
started-without-outcome fixture 均 reopen 为 `RecoveryRequired`，model/tool replay=`0`。

development rework 实际关闭四类纵向缺陷：macOS native virtual key 误路由、backend/front-end DOM node
identity 混用、Host synthetic submit capability 导致的假 stale，以及 form successful-controls canonical
parameter drift。最终实现完全删除 native/windows virtual key code，并在 CDP async continuation error、
敏感空字段和双 receipt 上增加确定性证据。

official DeepSeek requests=`0`、credential read=`false`、actual model cost=`$0`；没有付费 A/B，也不声明
Token/费用/通用效率提升。accounting 只是正交状态，未阻止已闭合 behavior evidence。current remaining
gaps 是 managed login/session、Cookie/storage、workspace-granted upload、isolated/scanned download、
canonical search 与 selective visual；本切片没有启动后续 cluster。focused/full pre-integration gate 与
复杂度/删除 actual 已通过并记录在 Evaluation 同名条目；full gate 只调用一次且 exit=`0`，不改 frozen
manifest/raw/summary/history。

### 40.15 Managed Browser Session capability cluster（已完成）

#### 问题、owner、旧路与 cutover

第二个 cluster 解决的真实问题是：公开页面交互此前只有进程内 session，root Agent 不能安全登录工程
应用、跨 browser executor 复用项目会话、上传已授权 workspace artifact，或接收并显式纳入下载结果。
验收是 disposable authenticated engineering application 的完整 login → session reuse → authenticated
navigation → upload → isolated download → verified promotion → clear，而不是单独增加几个 action。

single owner 是 `crates/tools`；`crates/app` 只接入真实 caller/composition，credential 复用既有
`dse-secrets` Host owner。cutover 删除 public incognito/ephemeral-only profile、Cookie blanket strip、
managed action blanket deny 和 `same_run_in_memory_public_origin` active assertion。没有 compatibility flag、
BrowserManager/Factory/Service、BrowserSessionStore、第二 Runtime/Store、Provider 或 production sidecar。

#### 纵向能力与治理

public browser profile 由 canonical workspace SHA-256 派生 project identity，位于 DSE state root，使用独占
文件锁、0700/0600 权限、30 天上限、stale Chrome marker 清理和 bounded graceful teardown；exact-local
仍为 incognito TempDir。不同 workspace 即使访问同一 origin 也不能读取对方 Cookie。Host credential
grant 绑定 exact login URL、submit origin/target、字段与 secret key；模型只看到 opaque ref，secret 只由
Host 从既有 secret backend 解析，所有 durable outcome/event/store/error 均保持 redacted。

upload grant 绑定 workspace-relative ordinary leaf file、canonical containment、非 symlink、4 MiB 上限、
size/SHA-256 与 exact same-origin form POST；execution 再做 TOCTOU identity check。download 只进入 per-session
quarantine，最多 4 个/4 MiB，并验证 redirect/origin、Content-Type 与 magic；允许有界 UTF-8 text/PDF/PNG/
JPEG，拒绝 executable/shebang/archive/unknown binary。receipt 记录 requested/final URL、trajectory、media、
bytes、SHA-256、transport/plaintext provenance、retrieved_at，明确 `auto_opened=false`、`executed=false`；
promotion 只能 no-overwrite 原子创建到 workspace，随后删除 quarantine source。

Ask/Agent 对 login、upload、session clear 投影 exact target/impact 并确认；routine status/download/promotion
按既有 policy 执行，isolated Writer 的 network action 继续拒绝。每个外部 POST grant 只消费一次；登录、
上传、下载、promotion、clear 的 durable started-without-outcome 矩阵全部进入 `RecoveryRequired` 且 replay=0。
RuntimeEvent、RunStore、State schema、AgentRuntime、DeepSeek wire/model-visible Prompt delta=0。

#### 实际纵向证据与剩余边界

repository-pinned CfT `151.0.7922.47` 在 7.47 秒完成真实 managed vertical：in-memory Host secret 登录且
durable 输出零 secret、clean shutdown 后新 executor 复用认证 Cookie、第二 workspace 同 origin 被拒、
multipart upload bytes 匹配、download 隔离/扫描/provenance 闭合、promotion bytes 匹配、session status 与
profile clear 成功。verified task family=`1/1`、cross-project/origin/path/symlink/oversize/type/auto-execute/
replay false allow=`0`。

真实 `AgentApplication -> AgentRuntime` mock-model loop 执行 8 次 model request、3 次人工批准、6 个
committed managed outcomes；SQLite reopen 的 event prefix 完全相同且 network/filesystem counters 不增加。
official DeepSeek requests/tokens/cost=`0/0/$0`，没有读取用户 credential 或 DeepSeek key，不做付费 A/B 或
效率声明。实际返工关闭 HSTS auto-upgrade fixture、persistent DevTools marker、session cookie durability、
Chrome SQLite graceful flush、storage clear CDP session 与 multipart form 六类问题；安全门没有为绿测试放宽。

production complexity 是 `semantic_browser.rs` 约 11.2k 行；该簇保留单一 owner/执行链，后续只允许在不改
行为时按 observation、interaction、session-egress、authorization-receipt 拆普通 Rust module，不能借此
引入 Manager/Service 或平行路径。下一路线输入是 canonical search + semantic observation quality，随后
内部 Alpha dogfood；本 checkpoint 未启动它们，也未改 frozen manifest/raw/summary/history。

最终 bounded authority actual 是 bootstrap=`17,636` 行、tools owner route=`2,645/4,409` 行、fixed
boundary=`24/24`。focused gate 全绿：tools=`405 passed, 8 ignored`、DeepSeek=`61/1`、runtime
conformance=`88/88`、app=`72/3`、app-server=`23/23`、exec=`30/30`、canonical TUI=`20/20`、PTY=`7/7`。
首个 full candidate invocation 在测试前被两个 Clippy finding 拒绝（`unnecessary_sort_by` 与 test-only
`await_holding_lock`）；两项均已做最小修复，workspace/all-targets Clippy `-D warnings` 通过。第二个
candidate 的 workspace suite 又发现 exec 与 HTTP/stdio 未绑定同一 managed browser state-root，导致
execution fingerprint parity 失败；测试 caller 迁移到同一 Host root 后 targeted surface parity=`2/2`。
第三个 candidate 随后暴露既有 child-env test 临时把进程全局 `PATH` 改为不存在目录、与并行
application-probe fixture 竞争的问题；该测试改为使用真实 parent PATH，不再改变进程全局状态，targeted
child-env/application-probe=`11/11`。最终 revision 只调用一次 full 且 exit=`0`；总 invocation=`4`
（failed candidates=`3`、final revision=`1`），没有把失败冒充通过，也没有在同一 revision 重跑。
`cargo fmt --all -- --check`、
`cargo check -p dse-tools --locked`、`cargo test -p dse-tools --locked` 与 `git diff --check` 均通过。

### 40.16 ADR-0019 Canonical Web Search + Semantic Observation Quality（已完成）

#### 问题、owner、旧路与纵向 cutover

此前 root Agent 只能读取已知 URL，不能为未知工程问题发现来源；semantic browser 又按 eligible 顺序
机械取前 N 个节点，任务相关内容会被页面前部噪音挤出 bound。single owner=`crates/tools`，`crates/app`
只接 production caller。replacement 增加唯一 `web_search(query,max_results?)`，并在同一 AXTree +
DOMSnapshot extractor 中用 deterministic task-cue/interaction/role priority 与 safe dedupe 取代 first-N；
action 后返回 bounded ref-independent diff。

search adapter 固定为 Tavily Basic/general HTTPS endpoint；模型没有 provider/endpoint/header/Cookie/
credential/proxy/browser 参数。Host 托管 `TAVILY_API_KEY`，强制 endpoint public DNS/connect pin、no proxy/
redirect、12 秒 deadline 与 1 MiB response。结果保存 request/usage identity、有界 canonical public
HTTP(S) source metadata、received-response hash/bytes，并明确 snippet=`discovery_only/external_untrusted`。
稳定研究路由只允许 search→source selection→fetch/browser original→cross-check→URL citation。

production fixed catalog 15→16；Ask 显示 exact query，Agent/FullAccess root allow，isolated Writer deny。
committed SQLite reopen 只重放 search/fetch/citation，started search ambiguity 进入既有 RecoveryRequired。
ToolOutcome/RuntimeEvent/RunStore 已足够，protocol/state/DeepSeek wire/model-visible Prompt delta=0。

#### Pre-integration evidence 与 closure

deterministic production research fixture=`1/1`：search=`1`、两个独立 source fetch=`2/2`、cited=`2/2`、
unsupported claim=`0`；cold reopen model/search/DNS/HTTP reexecution=`0`。真实 pinned CfT complete interaction
vertical=`1/1 in 7.01s`，focus recall=`10,000 bps`、truncation focus loss=`false`、fresh fill diff=`3/3`
non-empty and `<16 KiB`。query/result/deadline/response/provider-status/content/JSON/catalog/authorization/
actor/recovery matrix false allow=`0`。

official DeepSeek requests=`0`，不做付费 A/B 或效率声明。环境与 existing Host secret store 的
`TAVILY_API_KEY` presence 均 unavailable，value 未读取；provider canary=`not_run`、requests/reruns=`0/0`、
actual charge unknown。M44 已删除的 search-provider config/Doctor 假能力没有恢复；first-N active loop 已
删除。没有 SearchManager/Factory/Service、fallback Provider、search HTML scraper、第二 Runtime/Store、
Node/Playwright sidecar、visual stub 或新 dependency。

Rust source/test delta=`+2,087/-53`，新 `web_search.rs`=`1,042` 行，`semantic_browser.rs`
`11,173→11,651`。实际返工关闭 identity schema v6 旧断言、六 actor catalog hash 与 cancellation/unknown-
charge safe-retry 三类问题。bounded authority=`17,636` bootstrap、tools=`2,814/4,409`、boundary=`25/25`；
focused exit=`0`：tools=`413/8`、DeepSeek=`61/1`、runtime=`88/0`、app=`74/3`、app-server=`23/0`、
exec=`30/0`、canonical TUI=`20/0`、PTY=`7/0`。最终 revision 的 canonical
`./scripts/dev-dse.sh full` 只调用一次且 exit=`0`，覆盖 authority/public、fmt、workspace all-features
check/strict Clippy、全 workspace tests 与 doctests；同一 revision 没有第二次 full。clean reviewable
checkpoint 随本条形成；没有启动内部 Alpha 或下一 capability cluster。

### 40.17 Canonical Web Search 后 Internal Alpha Integration Checkpoint（已完成）

#### 问题、owner、旧路与 cutover

此前 search、fetch、Writer、ApplicationProbe、integration 与 replay 各自有纵向测试，但没有一个真实 root
task family 证明它们能在同一 production run 中收敛；组件分别绿色不能冒充工程闭环。primary owner=
`crates/app`，唯一 evidence-proven production blocker 位于 `crates/tools`：isolated Writer 的普通 no-egress
sandbox flag 被重复当成 Host-only loopback verifier 的 deny，导致 Writer 已成功修改代码却必然在
`application_probe_network_denied` 结束，永远不能 seal/integrate。

cutover 保留 isolated Writer 的 worktree-only filesystem 与 external network deny，只为 Host 冻结 program/
argv/cwd/bounds、128-bit lease、随机 IPv4 loopback origin 的 `application_probe` 派生
`host_loopback_only` sandbox treatment。macOS Seatbelt 仅允许 `network-bind/network-inbound` 的
`localhost:*`，不出现 `network-outbound`；Linux 继续使用 isolated network namespace。普通 no-network
actor 仍在 spawn 前拒绝。旧的“external egress=false 等同 Host loopback verifier=false”重复门已删除，
没有放开 Writer 的 Web/shell network，也没有第二 Runtime/Store/Provider、UI、视觉、登录或新依赖。

#### Deterministic task family、recovery 与实际指标

三个独立 task 分别修复 constant/function/mapping 三种 `server.py` code shape。每个 root 都必须从唯一
`web_search` 发现两个来源、用 `web_fetch` 读取原文并引用 2/2 URL，再把唯一 `server.py` 写权限交给
isolated Writer。Host runner 先用 Python `compile()` 做确定性 build，再启动 local HTTP app；Writer
receipt 通过后依次提交 seal、CAS integration，root 只在 integrated latest revision 的第二张 receipt 后
完成，最后清理 writer branch/worktree。每个 terminal cold SQLite reopen 的 model/search/DNS/fetch/
build/probe/Writer reexecution 均为 0；既有 search started ambiguity、ApplicationProbe SIGKILL recovery 与
Writer checkpoint conformance 随 focused gate 一并保持绿色。

deterministic actual=`3/3 verified`、false success=`0`、citations=`6/6`、Writer/root receipts=`6/6`、
rework=`0`。current fixture 每项真实执行 child `read_file -> apply_patch`，恰为 8 model turns、
1,110 input/86 output tokens；加入 compile/build 后三项并行 suite wall=`11.12s`，单项观测约
`11.083–11.112s`。root 文件 byte-exact、HEAD 前进、Git clean、writer
refs/worktrees 清零；
terminal reopen event stream byte-exact。测试 fixture 不是官方模型或 live Tavily，不外推成功率/Token/时间。

首个 official DeepSeek dogfood 按 one run、maximum reruns=`0`、hard requests=`8`、runtime retries=`0`、
known ceiling `$0.10` 执行；实际在 4 个请求后因人为 512-token 单回合 cap 到达
`Failed(OutputLimit)`，wall=`23.636s`、usage complete=`true`、billing unknown=`false`、input/output=
`13,141/1,145`、cost=`3,730,821 nanousd`（约 `$0.00373`）；该 treatment 没有重跑。

用户显式授权的 fresh successor 使用 production-representative 2,048 output tokens、同一 hard requests=`8`、
runtime retries=`0`、new-treatment reruns=`0`、ceiling=`$0.10`；实际第 6 个 physical request 后到达
`Failed(ToolBudgetExceeded { limit: 6 })`，wall=`44.536s`、usage complete=`true`、billing unknown=`false`、
input/output=`21,015/2,212`、cost=`6,538,253 nanousd`（约 `$0.00654`）。没有再运行。它证明 output cap
不再是 failure，但人为 tool-call budget 仍未闭合 official Alpha vertical。`max_tool_calls=16` 只在下一份
fresh 显式授权后作为独立 treatment 执行，结果如下；前两次 closed accounting 合计约 `$0.01027`。

再次显式授权的 fresh successor 保持 2,048 output tokens、16 tools、hard requests=`8`、runtime retries=`0`、
new-treatment reruns=`0`、ceiling=`$0.10`。实际 terminal=`Blocked(Host application_probe deterministic
failure)`，physical requests=`8`、wall=`68.861s`、usage complete=`true`、billing unknown=`false`、
input/output=`26,349/4,473`、cost=`9,334,781 nanousd`（约 `$0.00933`）；没有再运行。它证明 output/tool
旧上限都已越过；当时 evidence 只支持“到 Host verifier failure 时已耗尽 recovery budget”。future ignored
harness 因而增加失败前 root tool/fixture/marker diagnostics，并把下一份 recovery candidate 调整为
12 model/API requests、24 tools、runtime retries=`0`；后续结果否定了 budget-only hypothesis。

12-request / 24-tool fresh treatment 仍使用 2,048 output tokens、runtime retries=`0`、new-treatment
reruns=`0`、ceiling=`$0.10`。实际 terminal=`Blocked(Host application_probe deterministic failure)`、
physical requests=`12`、wall=`119.601s`、usage complete=`true`、billing unknown=`false`、input/output=
`51,588/7,580`、cost=`15,838,756 nanousd`（约 `$0.01584`）；没有再运行。diagnostics 为 root tools=
`[web_search, web_fetch, web_fetch, agent, agent]`、search/fetch=`1/2`、root marker present=`false`。两次
Writer 后仍无有效 marker，Host 保持 fail closed。

审计定位原 Alpha fixture cheat：parent policy 与 scripted Agent launch 只给 `apply_patch`，真实 Writer
不能先读取 `server.py`，fixture 却凭空提交完整预制文件。现有 Writer catalog/Agent schema 已原生支持
`read_file`；最小修复只让 Alpha task/harness 执行 child `read_file -> apply_patch` 并断言两个 committed
outcome，不改 production Runtime/catalog/Prompt/Store/Host verifier。修正后 deterministic=`3/3`、false
success=`0`、每项 requests=`8`、input/output=`1,110/86`、parallel wall=`11.12s`。修正后的 fresh
official treatment 在同一 2,048 output、
12 request / 24 tool、0 retry、0 rerun、`$0.10` ceiling 下到达 Host-accepted latest-revision
`Completed`：physical requests=`10`、wall=`57.270s`、usage complete=`true`、billing unknown=`false`、
input/output=`46,281/2,981`、cost=`12,621,177 nanousd`（约 `$0.01262`）、root marker present=`true`、
search/fetch=`1/2`。root trajectory 为
`[web_search, web_fetch, web_fetch, read_file, agent, read_file]`。

test process 仅在 terminal 后因 exact-tool-list observer 把两次合法 root `read_file` 误判而 exit=`101`；
behavior/accounting truth 已闭合。observer 已离线改为核心 `search -> fetch -> fetch -> agent` 顺序 + 最多
两次 root read，并用 actual/unknown-write/excessive-read 正反例验证；official treatment 没有重跑。
post-fix official vertical=`1/1`、false success=`0`，五次 closed accounting 合计约 `$0.04806`。只证明
bounded deterministic Host-Web usability，不声明通用效率或 live Tavily success。`TAVILY_API_KEY`
unavailable，故 live provider canary 仍未执行。

production Rust 只增加 Host loopback sandbox distinction，app 的大部分 delta 是 task-family/canary test；
protocol/state、DeepSeek wire/model-visible Prompt、AgentRuntime、RuntimeEvent、RunStore、catalog 与依赖均为
delta 0。Rust total delta=`+938/-23`，其中 app `#[cfg(test)]` module=`+825/-6`；docs=`+127/-4`，无 Cargo
delta。bounded authority actual=`17,636` bootstrap、tools route=`2,825/4,409`、fixed boundary=`25/25`。
focused gate exit=`0`：tools=`410 passed, 6 ignored`、DeepSeek=`61/1`、runtime=`88/88`、
app=`77 passed, 4 ignored`、app-server=`23/23`、exec=`30/30`、canonical TUI=`20/20`、PTY=`7/7`。
production checkpoint revision 的 canonical full gate invocation=`1`、exit=`0`；后续只有 test/authority
truth 变化，没有第二次 full。current read-before-edit/observer revision 的四项 targeted Alpha、fmt、
authority 与 diff check 通过；clean reviewable checkpoint 随本条形成，不 push；没有启动
destructive/publish、visual 或下一 capability cluster。
