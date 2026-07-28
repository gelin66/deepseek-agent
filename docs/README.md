# 文档与开发 authority 入口

本文件是从根 `AGENTS.md` 进入现有权威正文的机械地图，不是第二份 Roadmap、评测规范或
架构真相。产品总纲和 accepted ADR 仍高于旧文档；Roadmap 仍是唯一执行顺序，Evaluation
仍是唯一能力 keep/delete 合同，Current Architecture 仍只描述当前源码事实。

## 1. Mandatory bootstrap

每次修改只读取：根 `AGENTS.md`、完整
[Product Plan](product/PRODUCT_PLAN.md)、Roadmap 的
[当前执行窗口](product/ROADMAP.md#current-execution-window)，以及下表中唯一匹配 owner 的
accepted ADR 和 current-fact section。只有修改评测合同或判断 keep/delete 时才读取该行的
Evaluation 入口；发生冲突或跨固定架构时才沿 accepted decision index 扩大。

<a id="owner-routes"></a>
## 2. Owner routes

`Owner key` 是 fixture identity；一项切片只有一个主行。`Evaluation` 列是条件读取，不进入
普通 owner-scoped bootstrap 行数。

<!-- authority-routes:start -->
| Owner key | Code/document owner | Accepted decisions | Current facts | Evaluation when needed |
|---|---|---|---|---|
| `app` | `crates/app` composition/routing | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0008](decisions/0008-fixed-deepseek-routing-and-auto-retirement.md), [ADR-0012](decisions/0012-canonical-permission-policy.md) | [Application service](architecture/CURRENT_CODEWHALE.md#current-app) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `runtime` | `crates/runtime` loop/completion/replay/children | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0011](decisions/0011-orthogonal-behavior-and-accounting-truth.md), [ADR-0012](decisions/0012-canonical-permission-policy.md), [ADR-0014](decisions/0014-model-visible-contract-and-harness-control.md) | [Agent runtime](architecture/CURRENT_CODEWHALE.md#current-runtime) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `protocol` | `crates/protocol` commands/events/outcomes | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0011](decisions/0011-orthogonal-behavior-and-accounting-truth.md), [ADR-0012](decisions/0012-canonical-permission-policy.md) | [core protocol facts](architecture/CURRENT_CODEWHALE.md#current-core-facts) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `deepseek` | `crates/deepseek` plan/transport/parser/accounting | [ADR-0001](decisions/0001-rust-deepseek-product.md), [ADR-0008](decisions/0008-fixed-deepseek-routing-and-auto-retirement.md), [ADR-0011](decisions/0011-orthogonal-behavior-and-accounting-truth.md), [ADR-0014](decisions/0014-model-visible-contract-and-harness-control.md) | [DeepSeek backend](architecture/CURRENT_CODEWHALE.md#current-deepseek) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `context` | `crates/context` prompt/project context/compaction | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0014](decisions/0014-model-visible-contract-and-harness-control.md), [ADR-0016](decisions/0016-lean-cognitive-control-plane.md) | [core context facts](architecture/CURRENT_CODEWHALE.md#current-core-facts) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `tools` | `crates/tools` catalog/edit/shell/verifier | [ADR-0005](decisions/0005-v1-evidence-gated-capability-scope.md), [ADR-0012](decisions/0012-canonical-permission-policy.md), [ADR-0015](decisions/0015-native-web-retrieval-and-semantic-browser.md), [ADR-0017](decisions/0017-public-http-web-fetch.md) | [Tools](architecture/CURRENT_CODEWHALE.md#current-tools) | [W1.1 contract](product/EVALUATION.md#adr-0017-w11-evaluation), [M46 admission](product/EVALUATION.md#m46-semantic-browser-admission) |
| `state` | `crates/state` SQLite/events/snapshots/reopen | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0011](decisions/0011-orthogonal-behavior-and-accounting-truth.md), [ADR-0012](decisions/0012-canonical-permission-policy.md) | [State](architecture/CURRENT_CODEWHALE.md#current-state) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `orchestrator` | `crates/orchestrator` graph/Writer worktrees | [ADR-0002](decisions/0002-single-runtime-and-runstore.md), [ADR-0003](decisions/0003-multi-agent-worktrees.md), [ADR-0012](decisions/0012-canonical-permission-policy.md) | [Agent orchestrator](architecture/CURRENT_CODEWHALE.md#current-orchestrator) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `localization` | `crates/localization` human language | [ADR-0004](decisions/0004-fixed-simplified-chinese.md), [ADR-0010](decisions/0010-bilingual-product-and-prompt-admission.md) | [human clients](architecture/CURRENT_CODEWHALE.md#current-clients) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `clients` | `crates/cli`, `crates/tui`, `crates/app-server` | [ADR-0009](decisions/0009-dse-product-identity.md), [ADR-0010](decisions/0010-bilingual-product-and-prompt-admission.md), [ADR-0012](decisions/0012-canonical-permission-policy.md), [ADR-0013](decisions/0013-native-tui-surface-system.md) | [client entrypoints](architecture/CURRENT_CODEWHALE.md#current-clients), [TUI surface](architecture/CURRENT_CODEWHALE.md#current-tui) | [stable rules](product/EVALUATION.md#evaluation-stable-rules) |
| `repository-guidance` | root guidance + `docs/product`, `docs/architecture`, `docs/decisions`; mechanical checks in existing scripts | [ADR-0016](decisions/0016-lean-cognitive-control-plane.md) | [development authority](architecture/CURRENT_CODEWHALE.md#current-development-authority) | [ADR-0016 contract](product/EVALUATION.md#adr-0016-evaluation) |
<!-- authority-routes:end -->

## 3. Accepted decision index

- Product/backend/state: [ADR-0001](decisions/0001-rust-deepseek-product.md),
  [ADR-0002](decisions/0002-single-runtime-and-runstore.md),
  [ADR-0003](decisions/0003-multi-agent-worktrees.md).
- Product language/scope/release: [ADR-0004](decisions/0004-fixed-simplified-chinese.md),
  [ADR-0005](decisions/0005-v1-evidence-gated-capability-scope.md),
  [ADR-0006](decisions/0006-v1-release-benchmark-successor.md),
  [ADR-0007](decisions/0007-v1-fixed-chinese-prompt-release-evidence.md).
- Routing/identity/language/accounting: [ADR-0008](decisions/0008-fixed-deepseek-routing-and-auto-retirement.md),
  [ADR-0009](decisions/0009-dse-product-identity.md),
  [ADR-0010](decisions/0010-bilingual-product-and-prompt-admission.md),
  [ADR-0011](decisions/0011-orthogonal-behavior-and-accounting-truth.md).
- Permission/TUI/Harness/Web/control plane:
  [ADR-0012](decisions/0012-canonical-permission-policy.md),
  [ADR-0013](decisions/0013-native-tui-surface-system.md),
  [ADR-0014](decisions/0014-model-visible-contract-and-harness-control.md),
  [ADR-0015](decisions/0015-native-web-retrieval-and-semantic-browser.md),
  [ADR-0016](decisions/0016-lean-cognitive-control-plane.md),
  [ADR-0017](decisions/0017-public-http-web-fetch.md).

所有 ADR、里程碑历史和评测结果仍可查且不得改写；本索引只取消默认全量重放。

## 4. Pre-registered bootstrap questions

fresh Agent 必须能沿以下精确入口回答六个问题；checker 将问题 ID、链接和目标 anchor 作为
fixture 验证。

<!-- bootstrap-questions:start -->
| Question ID | Exact answer authority |
|---|---|
| `current_goal` | [Roadmap current execution window](product/ROADMAP.md#current-execution-window) |
| `owner` | [owner routes](#owner-routes) |
| `forbidden` | [product and architecture boundary](../AGENTS.md#product-boundary) |
| `focused_gate` | [risk-tier gate](../AGENTS.md#risk-tier-gate) |
| `full_gate` | [risk-tier gate](../AGENTS.md#risk-tier-gate) |
| `deletion` | [development method](../AGENTS.md#development-method) |
<!-- bootstrap-questions:end -->

## 5. Directory roles

- `product/`: product plan, one Roadmap and one Evaluation authority.
- `decisions/`: accepted long-lived decisions and supersession.
- `architecture/`: current implementation facts, not target completion claims.
- `reference/`: executable configuration and usage reference.
- `legacy/`: imported history that must not flow back into current architecture.
- `evidence/` and `eval/`: historical/frozen evidence; never a substitute for current results.

Useful current references remain [Tool surface](architecture/TOOL_SURFACE.md),
[Runtime API](architecture/RUNTIME_API.md), [Subagents](architecture/SUBAGENTS.md),
[Configuration](reference/CONFIGURATION.md), [MCP](reference/MCP.md),
[Operations](reference/OPERATIONS_RUNBOOK.md), and [Sandbox](reference/SANDBOX.md).

## 6. Current development fact

M44, ADR-0016, and ADR-0017 W1.1 are clean checkpoints. M45-A has completed its Host-only
ApplicationProbe implementation and deterministic acceptance. The M46 admission audit has now
proved the same `tools:application_visibility` loss on two independent JS-only local tasks, so only
the next read-only W2 contract is admitted; no production browser implementation is active yet.
`scripts/dev-dse.sh` owns executable gates, while `AGENTS.md` owns the single risk classification
contract. New parallel roadmaps, handoffs, trackers, or duplicated gate command lists are not
allowed.
