# M8-G 单一 TaskGraph 产品概念收敛结论

- 日期：2026-07-24
- baseline：`650df581c825100c70a621da214c6c3d9ccb39d0`
- code candidate：`64f6bc1610787993fb0ad2d709aa232161c83453`
- candidate tree：`9c965668460b54080921b88f37ef5610c71f0b13`
- Cargo.lock SHA-256：
  `1d8b3e08412b2e0a5b2d7dd4e414dd5c86ee5a573559684b755b44d411dd9d77`
- 决策：`keep_canonical_taskgraph / delete_fleet_lane / close_V06`
- Key：未读取
- 官方 API 请求：0

## 1. 真实问题与审计结论

M8-E 的 V06 阻塞不是缺少另一套 TaskGraph 框架，而是同一产品同时暴露三组概念：

1. canonical `AgentTask` / `AgentOutcome` / child lifecycle；
2. TUI Fleet 的协议、配置、ledger、lease、scheduler、SSH、alerts 和 worker shell；
3. Lane crate 的 registry、tmux/inline process runtime 和独立 receipt。

只读调用图证明，production 的 root、read-only child 和 explicit Writer 已经统一进入：

```text
AgentApplication
  -> AgentRuntime
  -> RuntimeEvent / RunStore
  -> parent/child lifecycle
```

唯一特殊 side effect 是 explicit Writer 的 Git worktree/diff/verify/integrate/cleanup，
其 owner 已是 `ProductionAgentOrchestrator`。Fleet worker 最终只是再次启动
`codewhale exec`，Lane 也不拥有 canonical model loop、terminal 或 RunStore。因此新增
TaskGraph DTO、trait、scheduler 或 Store 只会制造第四个事实源。

## 2. 冻结切片

| 项目 | 冻结内容 |
|---|---|
| 真实问题 | Fleet/Lane 重复产品 shell 与 canonical child facts 并存 |
| 验收 | root/read-only/Writer、reopen、CLI/TUI/API parity 与 crash recovery 不退化 |
| 单一 owner | Runtime 执行、RunStore 持久、Orchestrator 只拥有 Writer Git side effect |
| 替代旧路径 | Fleet protocol/config/ledger/UI/skill 与 Lane registry/process shell |
| 证据 | focused、全 workspace clippy/test、process recovery、静态 absence gate |
| cutover 删除 | 见 manifest 的 `removed_paths` 和 `removed_persistent_concepts` |

没有临时 adapter、compatibility reader、dual write、远程 Fleet、通用 DAG 或多 Writer。

## 3. Cutover 结果

- 删除 `crates/protocol/src/fleet.rs`、`crates/config/src/fleet.rs` 和完整
  `crates/tui/src/fleet/`；
- 删除 `crates/lane`、`codewhale lane` 与 hidden `lane-log-proxy`；
- 删除 `codewhale fleet`、`/fleet`、bundled `fleet-manager` skill 和 Fleet locale；
- 删除 setup-state 的 `OperateFleet`、`operate_receipts_verified` 与 Doctor roster；
- setup-state schema 从 v1 升到 v2，旧 Fleet-bearing record fail closed 后从现有配置
  重新派生，不保留兼容 reader；
- Doctor JSON 改为投影 canonical `task_graph` owner/actor facts；
- 保留 `AgentRuntime`、`RunStore`、`ProductionAgentOrchestrator`、单模型可见 `agent`
  工具和 explicit-only Writer。

相对 baseline：

| 指标 | baseline | candidate | 变化 |
|---|---:|---:|---:|
| visible `codewhale` commands | 20 | 18 | -2 |
| setup steps | 7 | 6 | -1 |
| source lines | — | `+150/-15,260` | net `-15,110` |
| Run API / RuntimeEvent / State / exec-stream | 10 / 16 / 21 / 2 | 10 / 16 / 21 / 2 | 0 |

V06 因而从 blocked 变为 pass。V12 明显收窄，但 generic provider vocabulary 是否仍有真实
consumer 必须由独立删除审计决定。V16 只得到本切片的局部步骤下降；相对 imported
`352e86a6` 的同任务 workflow-step A/B 仍未完成。

## 4. 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8g-target
```

通过：

- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- CLI retired Fleet/Lane 在 config、TUI、Store、model 前 fail closed；
- root/read-only/explicit Writer conformance；
- exec/HTTP/stdio parity；
- app-server process crash/reopen、SIGKILL recovery 和 SQLite reopen；
- setup-state v1 retirement/v2 reopen；
- static caller/配置/协议 absence matrix；
- `git diff --check`。

完整 contract 见
[`m8-g-taskgraph-convergence-v1.json`](../manifests/m8-g-taskgraph-convergence-v1.json)。

## 5. Keep / shrink / hold / reject

**Keep**

- 现有 `AgentApplication -> AgentRuntime -> RunStore` TaskGraph facts；
- `ProductionAgentOrchestrator` 作为 explicit Writer Git side-effect 唯一 owner；
- 单一 `agent` tool、root/read-only/explicit Writer conformance 与 recovery。

**Delete**

- Fleet 与 Lane 的全部可调用 production shell、持久概念、配置和 UI；
- 为它们服务的 blocking HTTP feature、skill、locale 与 setup card。

**Hold**

- FIM 默认接管、RepoGraph、multi-Writer、automatic fan-out；
- M8-D unknown-billing prompt successor；
- imported-baseline coding/workflow-step 产品结论。

**Reject**

- 新 TaskGraph crate/DTO/Store/scheduler；
- Fleet/Lane compatibility layer 或模式开关。

## 6. 非结论与下一切片

本切片没有模型 treatment，不证明 verified coding success、false success、Token、cache、
wall time 或 API cost 改善；15,110 行净删除是复杂度证据，不是能力指标。它也不把单
explicit Writer 解释为 multi-Writer 完成。

下一切片应冻结 PRODUCT_PLAN 的 Standard/Strict/FIM V1 范围与真实 caller 缺口。M7-C
已经证明 deterministic Host 编辑基线从 3/12 修到 12/12，但没有真实 FIM product
metric；因此只有新证据证明编辑生成仍是主要瓶颈且 FIM 存在可归因 surface delta 时，
才允许最小 Host-owned FIM 候选。否则保持 hold/reject，并转入 imported-baseline
coding/workflow-step A/B。
