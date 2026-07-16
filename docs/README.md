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
- [reference/MCP.md](reference/MCP.md)
- [legacy/FLEET.md](legacy/FLEET.md)
- [legacy/WORKFLOW_AUTHORING.md](legacy/WORKFLOW_AUTHORING.md)

代码迁移完成后，应同步缩小或删除对应参考；禁止把 `legacy/` 中的概念重新写回
产品权威文档。

## 3. 当前开发说明

- `codewhale exec` 与 `codewhale app-server` 已共用
  `crates/app::AgentApplication`、`AgentRuntime` 和 SQLite `RunStore`。
- 交互 TUI 的旧 engine 与 `TaskManager` 状态路径仍待 M4-C 迁移；它不是
  app-server 的执行或状态来源。
- 本地 focused 检查脚本：`../scripts/dev-deepseek-agent.sh`。
- M1 离线能力基线：`../eval/README.md`。
- 当前配置样例：`../config.deepseek-agent.example.toml`。
- 当前二进制和状态目录仍保留 CodeWhale 名称，直到产品化里程碑统一修改。

## 4. 历史和待清理资料

网站、VS Code scaffold、npm 发布包装、上游社区自动化、版本 dogfood/release、
remote setup、腾讯云部署、Telegram/Feishu chat bridge 和未接入 Rust runtime 的
WeCom/Weixin bridge 已从活动开发树移除。通用 Provider、导入 skills 和少量旧 evidence
仍待后续 DeepSeek-only 切片按真实调用图清理。

新增文档时，应优先更新已有权威文件。只有新的长期架构决策才新增 ADR；不要创建
新的平行 Roadmap、计划、handoff 或版本 tracker。
