# DeepSeek Agent 开发路线图

> 文档类别：产品权威。仅定义实施顺序、迁移和删除点。

- 状态：执行中
- 当前阶段：M4-C 收尾中。M4-B 被测代码提交为
  `a534a824670b60c807c5abf399ea8674d4beb527`；C1 实现提交 `1d127b78` 已建立并冻结
  canonical durable interaction/control contract。C2 的 Run API v3、RuntimeEvent writer
  v5/read v4-v5、State schema v8、continuation 和最小 context projection 已通过本机完整
  验收与费用受限的官方 DeepSeek sender canary，并冻结为提交 `4a3311ac`；后续
  `35fc3cc4` 已把 durable creation delivery 提升为 Run API v4 / State schema v9。交互 TUI
  foreground、canonical root/child Run 投影和旧前台状态删除已经完成；当前
  RuntimeEvent v6、State schema v10。`workflow`/`workflow-tool` 的第二模型循环和
  `serve --acp` 的独立模型/会话路径均已物理删除，未引入兼容桥；`83d9487f` 又删除了
  direct `review` completion、模型内工具和私有 receipt 真相，canonical reviewer Agent
  profile 保留。`528a72f2` 的 child eager join 已通过 24-run 精确 A/B 并保留。无生产
  构造入口但仍参与编译的旧 TUI SubAgent runtime/manager/registry 岛也已删除；M4 仍待
  完整集成门禁与其他旧编译岛复核，不提前标完成。M1 的导入基线 A/B 与 M2 的完整官方
  surface canary 仍是独立证据债务
- 上次更新：2026-07-18

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
| M1 | 建立原始 DeepSeek 能力基准 | 进行中（硬预算本地门禁已通过，导入基线真实编码 A/B 待完成） | 真实编码 A/B 在硬请求预算下可重复测量成功率、假成功、Token、时间和成本 |
| M2 | 独立 DeepSeekBackend 与领域协议 | 进行中（当前候选全仓/exec/QA 回归通过，official live 待完成） | Production RequestPlan 通过真实路径/live 门禁，旧 DeepSeek 决策分支删除 |
| M3 | 最小 Headless AgentRuntime 垂直切片 | 已完成（仅 `exec`） | `exec` 单一生产 loop，离线/全仓/真实 DeepSeek 证据通过 |
| M4 | 统一工具、事件、RunStore 和产品入口 | 收尾中（第二模型循环已删，最终集成门禁待通过） | CLI/TUI/API 同事件，所有生产模型循环统一 |
| M5 | RepoGraph、ContextBroker 和 canonical 证据链 | 待开始 | TaskContract/receipt 只由唯一 Runtime/RunStore 判定，成功率或 Token 优于基线且假成功下降 |
| M6 | 统一多 Agent 与 worktree 生命周期 | 部分开始（canonical 根/子同 Runtime 已完成） | 唯一 Orchestrator、writer worktree 和并行净收益 |
| M7 | DeepSeek 专项调优与产品清理 | 待开始 | 其他 Provider 和重复产品外壳被删除 |
| M8 | V1 本地产品化 | 待开始 | 自己的品牌、配置、CI、打包和开发流程完整 |

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

- 缺少覆盖 canonical Runtime 到官方 production sender 的 Standard/Strict/FIM surface 矩阵；
- 多轮 exact reasoning replay、transport retry、畸形/不完整响应和 FIM 事务性写入仍需在
  production path 形成垂直证据；
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

### M4 总体后续顺序

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
  `models.rs` 暂时只保留仍被 history、pricing、hooks、MCP/工具 schema 和配置状态真实消费的
  展示、计量与模型元数据，不再承担 DeepSeek transport 或请求规划职责。
- M4-C 已删除从未接入当前交互循环的 TUI `FrameRateLimiter`/`FrameRequester`/`MotionPolicy`
  编译岛及其只写不读的 `constrained_frame_rate` 设置。现有 24ms 事件轮询、80ms 动画重绘、
  `low_motion` 和实际 spinner/ocean 渲染保持不变。
- M4-C 已删除没有写入、弹出或清空调用方的持久 composer stash，以及 Doctor 对历史
  `composer_stash.jsonl` 的假诊断、不可达 Ctrl+S/`/stash` 帮助和提示；清空输入前用于撤销的
  进程内 recovery draft 仍由 `App` 保留。
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
  canonical Run 时 `ToolPolicy.allowed` 明确为 `None`；mode 权限基线只保留在真实调用方
  `App`。canonical Runtime terminal、RunStore、确定性 `run_verifiers` 和多 Agent/Fleet
  均未改变。
- M4-C 已将 WorkSurface 收敛为 task、同步 shell run、canonical child Agent 与 Todo 的
  只读投影：删除从未被生产事件循环调用的键盘/鼠标输入、焦点/选择/滚动、打开详情、
  停止确认、命中区和失效 `/task`/`/jobs` 动作。top/left/right 布局、状态排序、Todo 和
  canonical child 投影均保留；固定窗口不再显示实际不可操作的滚动条或动作控件。
- M4-C 已删除只有自测构造、没有生产打开入口的旧 Mode/Status picker modal、专属事件和
  状态行 picker 文案；`AppMode` 权限基线、`StatusItem` 配置与实际 footer 投影继续由原
  调用方保留，不恢复不可达的 `/mode` 或 `/statusline` 外壳。
- M4-C 已删除同样没有生产构造、canonical 命令或 Run 事件入口的旧 `ConfigView` 编译岛、
  专属测试和消息目录；`Config`、`Settings`、`ApprovalPolicyControl` 与启动时配置加载继续
  保留。Doctor 和当前参考文档改为真实 `~/.codewhale/config.toml` 路径，不再宣传不存在的
  `/config` 编辑器。
- M4-C 已删除只有自测构造的旧 `ThemePickerView` 及其专用 `settings_picker` 框架、
  `ConfigUpdated` 事件与主题 picker 消息；主题配置文件加载、`ThemeId`/`UiTheme`、Ocean
  渲染和主题对比测试继续保留，未把不可达 modal 当成真实主题能力。
- M4-C 已删除无生产入口的旧 `FeedbackPickerView`、其私有 command-palette 事件与失效的
  `Ctrl+K` 帮助项；真实 slash menu 仍由输入 `/` 打开并走 canonical 命令面。
- M4-C 已删除无人调用的旧 `FilePickerView`/`file_picker_relevance`、专属事件、消息与
  `Ctrl+P` 帮助项；真实 `@mention`、frecency、工作区补全和模糊路径解析继续保留并单独
  验证，避免把同名遗留 modal 与生产附件能力混淆。
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
  和失效的 `Ctrl+Shift+E` 声明。真实 `@mention`、frecency、模糊路径解析和 composer
  菜单保留，文件引用的 canonical context 接线继续作为独立能力缺口处理。
- M4-C 已物理删除仅由自身测试调用、从未进入 production prompt 的旧 TUI `memory.rs`
  push/inject 实现。canonical `crates/context::production_system_prompt`、instructions、skills、
  WorldState、handoff 与 compaction 保留；历史 memory 配置/Doctor/侧栏假投影另片清理。
- 该删除切片的 focused gate 已通过：Runtime conformance 53/53、DeepSeek 35/35、
  app 37 passed/1 ignored、app-server 23/23、exec production loopback 24/24、
  canonical TUI Run 20/20、PTY 5/5；State `run_store`、CLI canonical runs 与 TUI unit
  门禁也通过。exec 多 Agent fixture 同步锁定已接受的 eager join：单层 3 次请求、嵌套
  5 次请求，不再要求已经删除的无信息 wait 轮。
- focused 的 TUI 子集已迁移到 canonical command、Run client/projection/presenter、本地
  approval、Fleet 和 DeepSeek Doctor；旧 memory、旧 schema sanitizer、旧 model client、
  stream decoder 和 legacy route 测试不再冒充当前核心门禁。过滤器仍先按 `--list` 校验，
  任一零匹配继续 fail closed。
- 到 M4 退出前，三个入口必须使用同一 `AgentRuntime`、`RuntimeEvent` 和 `RunStore`，并统一
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
  Runtime 的 TUI `auto_compact` 假设置和阈值状态。M5 再以 A/B 决定
  evidence-aware compaction/ContextBroker 的保留设计，不堆叠第二套摘要器。

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
- 持久协议：RuntimeEvent 保持 v6；State schema v10 持久化并重建最近模型请求实际
  advertised tool catalog。
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
- 隐藏 workflow 的 Workflow/SubAgent JSON/JSONL 写入链、专属 adapter 和 UI 已随第二
  Runtime 物理删除；未把旧状态迁成 canonical 双写。
- ACP 独立 session/stream/direct completion 已删除；不保留 editor 协议兼容桥。

### 整个 M4 的退出门槛

- CLI/API/交互 TUI foreground 对同一 canonical command/event contract 工作。
- 内存 Store 与 SQLite Store 重放一致。（M4-A 行为门禁已通过）
- crash/resume 和 exactly-once completion 通过。（`exec` 与 app-server 已通过）
- exec/app-server/交互 foreground 不存在第二个生产 loop、可写 Store 或入口私有 completion
  语义；已删除的 hidden workflow/ACP/direct `review` 不再构成生产例外。仍须通过 M4 完整
  workspace 门禁、调用图复核并删除剩余旧编译岛后才能关闭里程碑。

## 9. M5：RepoGraph、ContextBroker 与 canonical 证据链

### 工作

- 用 tree-sitter、ripgrep、LSP、包依赖和 git diff 建立增量 RepoGraph。
- 在统一 Runtime 上实现 `ContextBroker`，按任务相关性、证据新鲜度和 Token 预算选择上下文。
- 基于 M4-C C2 的唯一 projection/event 状态机增强 compaction，使其可确定性保留
  TaskContract、未决问题、当前 diff 和最新证据；不得另建第二套 compaction runtime。
- 用同任务、同预算的 compaction on/off A/B 测量 verified success、false-success、Token、
  时间和费用；M4-C 的协议/恢复通过不能替代该收益证据。
- 在 canonical `protocol/runtime/state` 中新实现唯一的 `TaskContract`、
  `EvidenceReceipt` 和 Host completion owner，并让 `workspace_revision` 与确定性 verifier
  结果进入同一个 `AgentRuntime`/`RunStore`。M4 已删除无生产消费者的历史 TUI prototype；
  可以参考其测试反例，但不得为旧类型建立 adapter、双写或兼容状态机。
- 每个 generation 冻结 objective、constraints、non-goals 和 acceptance；显式 verifier
  只接受匹配 generation、精确参数和最新 workspace revision 的 receipt，objective-only
  任务只能由 Host 验收。普通 `run_tests`、参数不匹配的 verifier 和模型自评只记录为
  artifact，不能自行升级为完成证据。
- 提供可执行任意项目命令的显式 verify 入口，不写命令字符串猜测器。
- 让 `ToolOutcome`、证据失效和 completion 判定都经过同一 Runtime/RunStore；先完成这条
  单 Agent 证据链，再让多 Agent 复用，避免把不可靠终态并行放大。

### 删除/替代

- 历史 TUI/Goal 本地判定、receipt 记账和重复状态已在 M4 物理删除；M5 不恢复镜像同步、
  adapter 或旧语义 fallback。
- 最终只保留一套 `TaskContract`、`EvidenceReceipt`、`TerminalState` 和 Host 接受流程。

### 退出门槛

- RepoGraph 相比当前 project map 提高成功率或减少 Token。
- evidence-aware compaction 相比关闭 compaction 的变体产生可重复净收益；无收益则缩小或
  删除对应增强，不能用实现复杂度冒充能力。
- 写操作会使旧证据失效。
- false-success 显著下降。
- 根/子 Agent 与 Headless/TUI/API 对同一 TaskContract 共享同一验收判定和 RunStore 真相。
- 不存在第二套 Goal 状态机、receipt store、completion 判定器或兼容桥。

## 10. M6：统一多 Agent

多 Agent 是必须保留的产品能力。本阶段不是删除智能体，而是删除多套重复内核和产品外壳。

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

### 调优

- `apply_patch/search-replace/FIM` A/B；
- thinking、上下文预算和压缩策略；
- stable prefix/cache；
- 并行只读工具；
- Agent 数量和预算；
- 开发中文原生 Agent 提示词组合，分别调优规划、工具策略、失败恢复、压缩和子 Agent 协作；
  以当前生产提示和归档基线做同任务 A/B，候选按版本评测并可回滚；
- 首个合并候选 `b088fd13` 及后续 v2/v3 收敛 canary 均因 multi 可靠性或计量门槛被拒绝；
  v3 已证明 fixed checklist 影响 root 收敛，也证明 child 最终结果轮不能靠提示词保证；
  后续先改 Runtime 机制，不恢复已经删除的模式、人格、Provider 或兼容提示层；
- 只有基准证明需要时才加入 embedding。

### 剩余清理

- 其他 Provider、模型目录、定价和别名；
- release 和品牌耦合；
- 其余腾讯云/CNB 等未接线云部署资产；
- 遗留 evidence 和最终不再需要的导入资产；
- 无接线 stub、兼容别名、旧语义适配层和新旧双路径。

Telegram、Feishu、bridge-core、remote-setup 调用面和 Tencent Lighthouse 部署链已在 M4-B
因 app-server 旧控制面删除而同步物理删除，不再列为 M7 待办。

顶层 `codewhale update`、CLI 自更新实现及其专用直接依赖已删除；TUI 启动时版本检查和
仍有真实消费者的 `release` crate 保留，后续只能按各自消费者单独处置。

清理必须先通过依赖盘点；Cargo 核心能力不得因外围删除而退化。Provider 专用的人类界面
随对应旧路径一起删除，不投入翻译；每个切片只汉化已经确认保留的 DeepSeek 配置、Agent
运行和多 Agent 链路。

## 12. M8：V1 产品化

- 正式产品名、二进制名、配置目录和 User-Agent；
- 自己的 origin/upstream 远程策略；
- DeepSeek-only 配置向导；
- 固定 `zh-Hans` 的 CLI/TUI/Headless 文本界面与中文帮助、Doctor、错误恢复和多 Agent 状态；
- 保持 NDJSON/API 字段、命令参数、工具名、模型 ID、路径、代码和原始输出稳定；
- 中文原生 Agent 提示词包通过同任务 A/B 后默认启用，提示词版本可追溯并可回滚；
- 本地开发、安装、卸载和数据迁移；
- 精确 Rust toolchain；
- 自己的 CI、版本、changelog 和发布流程；
- 架构依赖门禁和长期 benchmark。

M8 退出前必须通过第 2.1 节的中文端到端、机器协议稳定性、CJK 终端布局、英文泄漏和
提示词 A/B 门禁；只增加翻译字符串但保留英文主流程，不计为完成。

## 13. 当前源码迁移表

| 当前实现 | 目标归属 | 替代后删除 |
|---|---|---|
| `client.rs`、`client/chat.rs` | `deepseek` | 通用 Provider/DeepSeek 混合 client |
| 退役 `tui/compaction`、`seam_manager`（已删除） | `context` | canonical 实现位于 `context + runtime + app` |
| `project_context`、`working_set` 遗留半区 | `context` | 浅层 project map 和重复投影 |
| `tui/src/tools/*` | `tools` | TUI 工具业务逻辑 |
| legacy thread tables、Fleet ledger | `state` | 多状态真相 |
| Fleet、Lane | `orchestrator` | 重复产品外壳；hidden Workflow 第二循环已删除 |
| 交互 foreground/child projection（M4-C C3 已迁移） | `app + runtime + tui` | 旧 Engine、runtime-thread、SessionManager、child cache 已删除 |
| `app-server` canonical projection（M4-B 已迁移） | `app + app-server` | TUI 子进程桥已删除 |
| `crates/core` 脚手架（M4-B 已删除） | `app + runtime` | fake `handle_prompt` 已删除 |

## 14. 调整机制

里程碑结束时只允许三种结论：

- **保留**：真实评测有净收益，复杂度合理；
- **重做/缩小**：方向有价值，实现或默认策略不合理；
- **删除/推迟**：没有收益、明显负优化或不属于产品范围。

改变固定架构边界必须新增 ADR，说明问题、证据、替代方案、迁移和删除影响。
不能用临时开发困难作为恢复多 Provider、多 Runtime 或多状态真相的理由。
