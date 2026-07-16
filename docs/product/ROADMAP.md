# DeepSeek Agent 开发路线图

> 文档类别：产品权威。仅定义实施顺序、迁移和删除点。

- 状态：执行中
- 当前阶段：M4-A `ToolOutcome + SQLite RunStore + crash/resume` 已严格完成；被测代码提交为
  `0a5b76a8627fe8ae108a0a7688c0dd39ce5601a3`。下一阶段是 M4-B app-server 单一
  Runtime/RunStore 垂直迁移，尚未启动。M1 的导入基线 A/B 与 M2 的完整官方 surface
  canary 仍是独立证据债务
- 上次更新：2026-07-16

本文件是唯一执行路线。产品边界见 [PRODUCT_PLAN.md](PRODUCT_PLAN.md)，评测规则见
[EVALUATION.md](EVALUATION.md)。本文件可以根据开发证据调整顺序和实现细节，但不能
静默改变产品总纲中的固定架构决策。

## 1. 当前基线

- 导入基线：CodeWhale `352e86a611fdf3cd8bd27c36d24d482c06a71117`。
- 基线版本：workspace `0.8.68`。
- `codewhale exec` 已使用 `crates/runtime::AgentRuntime`；TUI 与 app-server 尚未迁移，
  它们的旧生产 loop 仍位于 `crates/tui`。
- `crates/core` 尚不是生产模型执行内核。
- 新 Runtime 内的根/子 Agent 使用同一实现；未迁移的旧 TUI 子 Agent 仍有不同循环。
- M1-A 离线契约证据与生产工具目录测量已经完成。
- M1-B 官方 DeepSeek live canary 已通过 5/5，但仅属于协议兼容证据。
- M1-C 的共享真实 HTTP 请求硬预算已经接入当前候选；提交 `0a5b76a` 的 M4-A production
  `exec` acceptance 22/22，TUI crate 6,888 passed、3 ignored，完整 workspace 回归 0 失败。
- 已完成冻结的 pre-M3 候选与 M3 候选之间的真实单 Agent 编码 A/B；这证明 M3 整体切片
  在该任务上成功率不退化且 Token/成本下降，但不是导入基线 A/B，不能据此关闭 M1。
- M2-A 已让官方 DeepSeek `RequestPlan` 接入现有 production Client 并通过单元回归；
  当前候选的 credentialed official live canary 尚未重跑，不能把本地门禁写成官方 API 验收通过。
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
3. 再用现有统一 locale 资源汉化保留的人类界面，不建立第二套 i18n；
4. 独立开发并评测中文原生模型提示词包，不把 UI 翻译混入 Runtime 语义。

人类界面默认 `zh-Hans`；命令和 flags、工具名、Schema/API 字段、模型 ID、路径、代码、
diff、stdout/stderr 和原始日志保持稳定。最终门禁至少覆盖：

- 隔离 HOME 下的首次启动、`--help`、Setup/Doctor、认证失败、限流、超时、SSE、上下文和
  请求预算耗尽都提供中文摘要与可执行建议；
- `exec` 文本输出默认中文，NDJSON/机器事件的字段与 golden fixture 不因 locale 改变；
- 单 Agent 和多 Agent 的成功、失败、取消、恢复链路均无非白名单英文，子 Agent 与恢复
  会话继承 locale；
- 80/120 列终端下 CJK 宽度、截断和换行正确；
- 英文泄漏门禁只扫描保留的用户渲染路径，并维护技术字面量白名单，不做全仓 ASCII 扫描；
- 中文提示词包与当前版本做同任务 A/B，记录 verified success、工具错误率、轮次、Token、
  时延和成本；无能力回归且关键指标有净提升后才默认启用，版本必须可追溯和回滚。

## 3. 里程碑总览

| 里程碑 | 目标 | 状态 | 主要退出门槛 |
|---|---|---|---|
| M0 | 保护基线、整理仓库、建立唯一文档真相 | 已完成 | 工作区可追溯，产品方案落库，现有 WIP 被隔离说明 |
| M1 | 建立原始 DeepSeek 能力基准 | 进行中（硬预算本地门禁已通过，导入基线真实编码 A/B 待完成） | 真实编码 A/B 在硬请求预算下可重复测量成功率、假成功、Token、时间和成本 |
| M2 | 独立 DeepSeekBackend 与领域协议 | 进行中（当前候选全仓/exec/QA 回归通过，official live 待完成） | Production RequestPlan 通过真实路径/live 门禁，旧 DeepSeek 决策分支删除 |
| M3 | 最小 Headless AgentRuntime 垂直切片 | 已完成（仅 `exec`） | `exec` 单一生产 loop，离线/全仓/真实 DeepSeek 证据通过 |
| M4 | 统一工具、事件、RunStore 和产品入口 | 进行中（M4-A 严格完成；M4-B/M4-C 待完成） | CLI/TUI/API 同事件，旧 core/bridge 路径删除 |
| M5 | RepoGraph、ContextBroker 和现有 WIP 证据链迁移 | 待开始 | 现有 TaskContract/receipt 只由唯一 Runtime/RunStore 判定，成功率或 Token 优于基线且假成功下降 |
| M6 | 统一多 Agent 与 worktree 生命周期 | 待开始 | 根/子 Agent 同内核，并行任务产生净收益 |
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

- 当前提交在同一 Harness/manifest 下通过 41/41 离线用例。
- 导入提交在同一 Harness/manifest 下通过 12/12 个 `cross_revision` 可比较用例。
- 12 个跨提交用例包含生产 Engine 离线链路与多 Agent 契约，作为重构防回归底线。
- 另外 29 个 `candidate_only` 用例只证明候选契约通过，不证明能力提升。
- 完整证据、哈希和解释边界见
  [M1-A 离线契约基线](../../eval/summaries/m1-offline-baseline-2026-07-15.md)。
- [`scripts/measure-tool-catalog.py`](../../scripts/measure-tool-catalog.py) 已通过真实生产 Engine
  turn 测量完整目录、模型可见目录和确定性字节估算；该测量描述工具面规模，不是
  Provider Token、真实模型调用或能力提升证据。

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
   Agent 可靠性、`verify` 和本地配置逐项给出保留、
   重做、缩小或删除结论。

`verify` 仍是额外模型评审实验，不等同于测试证据；未经真实缺陷检出率和误报率评测，
不得默认成为完成门禁。

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

- 未新建 crate；最小 `ApiSurface::{StandardChat, StrictChat, Fim}` 与 `RequestPlan` 位于现有
  `crates/tui/src/client/`。
- 官方 DeepSeek streaming/non-streaming Client 已消费同一 planner；`RequestPlan` 一次性决定
  surface、endpoint、wire model、streaming、reasoning replay、工具/strict 状态和 body。
- Strict 在整组 schema 兼容时走 Beta；任一不兼容时整组原子回退 Standard，并保留全部工具。
- FIM 保持独立 Beta Completions 语义；现有 transport、retry、parser 和 Agent loop 被复用。
- planner 与相关 Client 单元回归已经通过，未增加第二个 Runtime 或第二套 Client 主循环。

尚未完成的验收事实：

- 缺少覆盖 Engine 到官方 production sender 的 Standard/Strict/FIM surface 矩阵；
- 多轮 exact reasoning replay、transport retry、畸形/不完整响应和 FIM 事务性写入仍需在
  production path 形成垂直证据；
- 切换后的官方 DeepSeek live canary 尚未重跑。

因此 M2-A 当前只能标记为“代码接入完成、production-path/live gate pending”，不能标记
为验收完成。下一步先补齐上述门禁，再删除已被 planner 替代的旧决策分支。

### 同切片删除/替代

- 生产路径切换时删除它原有的 DeepSeek URL、Beta route、strict flag 保留/剥离和 FIM
  endpoint 决策分支；不能让新旧 planner 并存。
- 保留仍被未迁移调用方或其他 Provider 使用的通用 transport；不得为了目录纯度扩大删除范围。
- 不新增独立 `deepseek` crate。只有当生产调用方已经迁移、旧分支已经删除且依赖方向明确后，
  才评估物理抽取。

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

### M4-B：本地 API 单一 Runtime/RunStore 纵向切换（下一阶段）

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

### M4 总体后续顺序

- M4-A 已完成 Headless CLI 的 canonical `ToolOutcome`、SQLite RunStore 和恢复闭环。
- M4-B 按上面的垂直切片迁移 app-server，并删除该入口的旧 core/bridge/私有状态路径。
- M4-C 最后迁移交互 TUI，只保留命令输入与 `RuntimeEvent` 投影，删除 TUI 生产 turn loop。
- 到 M4 退出前，三个入口必须使用同一 `AgentRuntime`、`RuntimeEvent` 和 `RunStore`，并统一
  steer、resume、request-user-input、现有 compaction 与 completion 的 canonical
  command/event 投影；compaction 的能力重构仍属于 M5。真实工具只有在生产 consumer 同步
  迁移时才物理收敛到 `crates/tools`，不做空目录式模块搬家。

### 删除/替代

- Headless 已删除 `exec` 的旧 TUI Engine/spawn 路径；`crates/core::Runtime::handle_prompt`
  不再是 `exec` 调用方，但仍被 app-server 使用，归入 M4-B 的明确删除点。
- app-server 切换时删除启动 TUI 子进程的 bridge、`RuntimeBridge`、`monitor_turn` 和
  `RuntimeThreadStore` 等事件/状态翻译层。
- TUI 切换时删除 TUI 内生产 turn loop，只保留交互和 `RuntimeEvent` 投影。
- 每个入口切换时同步删除对应 runtime/session/task/fleet/lane 重复 JSON/JSONL 写入。

### 整个 M4 的退出门槛

- CLI/TUI/API 对同一 fixture 产生相同事件和终态。（未完成：TUI/API 尚未迁移）
- 内存 Store 与 SQLite Store 重放一致。（M4-A 行为门禁已通过）
- crash/resume 和 exactly-once completion 通过。（M4-A `exec` 已通过）
- 不存在第二个生产 loop、可写 Store 或入口私有 completion 语义。（仅 `exec` 已满足；
  TUI/app-server 待 M4-B/M4-C 删除旧路径）

## 9. M5：RepoGraph、ContextBroker 与现有 WIP 证据链迁移

### 工作

- 用 tree-sitter、ripgrep、LSP、包依赖和 git diff 建立增量 RepoGraph。
- 在统一 Runtime 上实现 `ContextBroker`，按任务相关性、证据新鲜度和 Token 预算选择上下文。
- 重构 compaction，保留 TaskContract、未决问题、当前 diff 和最新证据。
- 将当前生产 WIP 中已经接线的 `TaskContract`、`workspace_revision`、`EvidenceReceipt`、
  verifier receipt 和 Goal 终态约束拆分评测后迁入唯一 `AgentRuntime`/`RunStore`；这是迁移
  和收敛，不是再实现一套 Task、Goal、receipt 或 completion 状态机。
- 每个 generation 冻结 objective、constraints、non-goals 和 acceptance；显式 verifier
  只接受匹配 generation、精确参数和最新 workspace revision 的 receipt，objective-only
  任务只能由 Host 验收。普通 `run_tests`、参数不匹配的 verifier 和模型自评只记录为
  artifact，不能自行升级为完成证据。
- 提供可执行任意项目命令的显式 verify 入口，不写命令字符串猜测器。
- 让 `ToolOutcome`、证据失效和 completion 判定都经过同一 Runtime/RunStore；先完成这条
  单 Agent 证据链，再让多 Agent 复用，避免把不可靠终态并行放大。

### 删除/替代

- 每迁入一段 WIP 证据链，同一切片删除 TUI/Goal 工具中的对应本地判定、receipt 记账和
  重复状态写入，不保留镜像同步或旧语义 fallback。
- 最终只保留一套 `TaskContract`、`EvidenceReceipt`、`TerminalState` 和 Host 接受流程。

### 退出门槛

- RepoGraph 相比当前 project map 提高成功率或减少 Token。
- 写操作会使旧证据失效。
- false-success 显著下降。
- 根/子 Agent 与 Headless/TUI/API 对同一 TaskContract 共享同一验收判定和 RunStore 真相。
- 不存在第二套 Goal 状态机、receipt store、completion 判定器或兼容桥。

## 10. M6：统一多 Agent

多 Agent 是必须保留的产品能力。本阶段不是删除智能体，而是删除多套重复内核和产品外壳。

### 工作

- 让根 Agent 与每个子 Agent 都运行相同 `AgentRuntime`、发出相同 `RuntimeEvent`、写入同一
  `RunStore` 契约；差异只来自 TaskContract、预算、权限和 workspace。
- 建立唯一 `Orchestrator`，只负责 TaskGraph、预算、并发、mailbox、follow-up、wait、
  interrupt 和结果汇聚，不拥有第二套模型/工具循环。
- 建立 `AgentTask/AgentOutcome`。
- 建立 worktree create、diff、review、verify、merge、conflict、cleanup。
- 模型侧只保留一个 `agent` 工具入口；生命周期与调度语义是其参数/事件，不扩张为多组工具。
- 将现有 SubAgent、Workflow DAG、Fleet ledger/lease 和 Lane worktree 中经过测试的能力逐项
  迁入 `Orchestrator`，每迁完一个生产调用方就删除对应旧入口和重复状态写入。

### 删除/替代

- 子 Agent 切换到 `AgentRuntime` 时删除第二套 `run_subagent` 循环。
- 能力迁入并有回归测试后，删除 Workflow/Fleet/Lane 重复用户概念、scheduler 和状态真相。
- 删除不再承载独有能力的 `workflow-js`，不删除已迁入统一 Orchestrator 的智能体能力。

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
- 开发中文原生 Agent 提示词包，分别调优规划、工具策略、失败恢复、压缩和子 Agent 协作；
  保留英文/当前提示词为同任务 A/B 基线，按版本发布并可回滚；
- 只有基准证明需要时才加入 embedding。

### 剩余清理

- 其他 Provider、模型目录、定价和别名；
- 旧 updater、release 和品牌耦合；
- 当前仍被 remote-setup 引用的 Telegram、Feishu 和 bridge-core；
- 腾讯云/CNB 等云部署；
- 当前 Rust remote-setup 调用面；
- 遗留 evidence 和最终不再需要的导入资产；
- 无接线 stub、兼容别名、旧语义适配层和新旧双路径。

清理必须先通过依赖盘点；Cargo 核心能力不得因外围删除而退化。Provider 专用的人类界面
随对应旧路径一起删除，不投入翻译；每个切片只汉化已经确认保留的 DeepSeek 配置、Agent
运行和多 Agent 链路。

## 12. M8：V1 产品化

- 正式产品名、二进制名、配置目录和 User-Agent；
- 自己的 origin/upstream 远程策略；
- DeepSeek-only 配置向导；
- 默认 `zh-Hans` 的 CLI/TUI/Headless 文本界面与中文帮助、Doctor、错误恢复和多 Agent 状态；
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
| `crates/tui/src/core/engine/*` | `runtime` | TUI 生产循环 |
| `client.rs`、`client/chat.rs` | `deepseek` | 通用 Provider/DeepSeek 混合 client |
| `project_context`、`working_set`、`compaction` | `context` | 浅层 project map 和重复投影 |
| `tui/src/tools/*` | `tools` | TUI 工具业务逻辑 |
| `runtime_threads`、各 JSON store、Fleet ledger | `state` | 多状态真相 |
| `subagent`、Workflow、Fleet、Lane | `orchestrator` | 第二循环和重复产品外壳 |
| `app-server` bridge | `app + app-server` | TUI 子进程桥 |
| `crates/core` 脚手架 | 真实 runtime/application 职责 | 假 `handle_prompt` 路径 |

## 14. 调整机制

里程碑结束时只允许三种结论：

- **保留**：真实评测有净收益，复杂度合理；
- **重做/缩小**：方向有价值，实现或默认策略不合理；
- **删除/推迟**：没有收益、明显负优化或不属于产品范围。

改变固定架构边界必须新增 ADR，说明问题、证据、替代方案、迁移和删除影响。
不能用临时开发困难作为恢复多 Provider、多 Runtime 或多状态真相的理由。
