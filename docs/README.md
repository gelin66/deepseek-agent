# 文档入口

文档按职责分目录，避免产品目标、当前实现和导入资料混成一份事实。权威顺序和
目录边界固定如下。

## 1. 产品权威文档

1. [product/PRODUCT_PLAN.md](product/PRODUCT_PLAN.md) — 唯一产品范围与目标架构。
2. [decisions/](decisions/) — 已接受的长期架构决策。
3. [product/ROADMAP.md](product/ROADMAP.md) — 唯一开发顺序、迁移与删除计划。
4. [product/EVALUATION.md](product/EVALUATION.md) — 唯一能力评测和保留门槛。

如果其他文档与以上内容冲突，以产品总纲和 ADR 为准。

## 2. 目录职责

- `product/`：产品总纲、路线与评测门槛；只有这三份文件。
- `decisions/`：已接受 ADR；只有长期架构决定才新增。
- `architecture/`：当前架构、协议和能力迁移输入，不代表目标已经完成。
- `reference/`：与当前可执行代码对应的配置和使用参考。
- `legacy/`：仍可能被旧代码引用、但禁止继续扩展的导入表面积。
- `evidence/`：历史验证材料；不能替代当前评测结果。

关键迁移入口：

- [architecture/CURRENT_CODEWHALE.md](architecture/CURRENT_CODEWHALE.md)
- [architecture/TOOL_SURFACE.md](architecture/TOOL_SURFACE.md)
- [architecture/RUNTIME_API.md](architecture/RUNTIME_API.md)
- [architecture/SUBAGENTS.md](architecture/SUBAGENTS.md)
- [reference/CONFIGURATION.md](reference/CONFIGURATION.md)
- [reference/ACCESSIBILITY.md](reference/ACCESSIBILITY.md)
- [reference/MCP.md](reference/MCP.md)
- [reference/OPERATIONS_RUNBOOK.md](reference/OPERATIONS_RUNBOOK.md)
- [reference/SANDBOX.md](reference/SANDBOX.md)
- [legacy/FLEET.md](legacy/FLEET.md)

代码迁移完成后，应同步缩小或删除对应参考；禁止把 `legacy/` 中的概念重新写回
产品权威文档。

## 3. 当前开发说明

- `dse exec`、`dse app-server` 与交互 TUI 已共用
  `crates/app::AgentApplication`、`AgentRuntime` 和 SQLite `RunStore`。
- M4 已关闭：旧 TUI engine/Classic shell、私有状态路径和第二模型循环均已删除；
  Underwater 是唯一交互外壳。当前 Run API v12、RuntimeEvent v19、State schema v25、
  exec-stream v4。
- M5-A 已在 canonical protocol/runtime/state 中建立唯一 TaskContract、EvidenceReceipt
  与 Host completion owner；代码、本地门禁和正式 DeepSeek 显式 verifier A/B 已完成。
  M5-B evidence-aware ContextBroker 也已完成正式 A/B 并 shrink 为仅 hard-limit safety；
  M6-A 单 Writer isolated worktree 已在唯一 Orchestrator 下完成真实 DeepSeek 闭环；M6-B
  证据不准入双 Writer。M7-A 保持 `hold`；M7-B 已完成 Strict 目录准入与 typed 工具失败
  恢复，因六个默认可执行 actor 均无 Strict treatment surface 而未执行 live A/B。
  M8-H 已删除不可达 FIM production 半分支并保持 FIM re-entry 为 hold；M8-J 又删除旧
  `codewhale thread`、SQLite `threads`/session index 第二真相和无消费者 protocol DTO，
  V12 关闭。M17-A–F 已把当前 binary/config/protocol/delivery/CI/model identity 切为
  DSE，建立 `en`/`zh-Hans` 唯一 localization owner 与完整人类投影，并完成 fixed-Pro
  prompt 2×2 评测。production 只保留中文表达 prompt，按用户任务语言回答；当前进入
  M17-G 公开仓库文档与治理收敛。
- 本地 focused 检查脚本：`../scripts/dev-dse.sh`。
- M1 离线能力基线：`../eval/README.md`。
- 当前配置样例：`../config.example.toml`。
- 本地包与安装生命周期：`../scripts/dse-delivery.sh`；
  macOS/Linux 自测：`../scripts/test-dse-delivery.sh`。

## 4. 历史和待清理资料

网站、VS Code scaffold、npm 发布包装、上游社区自动化、版本 dogfood/release、
remote setup、腾讯云部署、Telegram/Feishu chat bridge 和未接入 Rust runtime 的
WeCom/Weixin bridge 已从活动开发树移除。generic Provider 与 legacy Thread active path
也已按真实调用图删除；导入 skills 和少量旧 evidence 仅作为受约束的能力输入/历史材料
保留。canonical `provider="deepseek"` environment fact 仍用于 replay safety，不是产品
模式。

新增文档时，应优先更新已有权威文件。只有新的长期架构决策才新增 ADR；不要创建
新的平行 Roadmap、计划、handoff 或版本 tracker。
