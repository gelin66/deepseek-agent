# M10-C Acceptance Progress（2026-07-24）

## 结论

M10-C 的正式决定是：

```text
reject_offline_viability_and_delete
do not admit the derived Acceptance Progress projection
do not read a credential or call the official API
restore the single canonical Host-facts production path
preserve the frozen offline decision evidence
advance to M10-D Failure-Directed Recovery
```

候选完成了 contract、最小实现、真实 production caller、root/read-only/Writer、
ContextBroker、compaction、SQLite reopen 与 verifier recovery 的离线机制闭环，但在
credential 边界前已经不能满足准入条件：

- pending request 的 estimated context tokens 为 `235 -> 356`，增加 121（51.5%）；
- verifier rejection request 为 `538 -> 547`，增加 9（1.7%）；
- satisfied state 为 `492 -> 386`，减少 106（21.5%），但 canonical Host 在 receipt
  sealing 后立即接受 terminal completion，不会再发出一条能消费该下降的模型请求；
- 候选改动 20 files、`+1,587/-86`，净增加 1,501 行；
- current fixed-Pro frozen baseline 已有两个 accounting-complete 的
  `root_recovery` 成功样本，均以最短确定性
  `run_verifiers -> edit_file -> run_verifiers` 完成，verified success 为真且
  false success 为零。

所有实测的 model-request-visible 状态都没有效率收益，且现有路径没有出现该候选所要
修复的验收进度失败。继续做付费 A/B 既不能解释新增复杂度，也会无必要地暴露于 M9-E
已经证明无法逐物理请求对账的 pre-header failure 窗口。因此 M10-C 在 offline
viability gate 被否决，不读取 Key，不调用 DeepSeek API。

## 切片契约

- 真实问题：TaskContract、verifier observation、completion rejection 与
  EvidenceReceipt 已是 canonical facts，但执行中没有统一的逐 acceptance
  `satisfied/pending/invalidated/evidence_needed` 视图；
- 验收：只从 canonical facts 和 latest workspace revision 派生，compaction/reopen
  一致；降低重复工具轮次或漏验收；verified success 非劣、false success 0，并取得
  可归因效率或可靠性净收益；
- owner：`crates/runtime`，现有 `crates/context` ContextBroker 只消费 request-local
  派生结果；
- 被替代旧路：只有准入后才替代重复 Host-facts prose 与模型自行重建剩余验收；
- cutover：任一 gate 失败即删除 projection、prompt marker、配置/用户接线及
  treatment-only tests；
- 禁止：第二 plan truth、plan store、Goal/Hunt、update-plan 工具或第二完成权威。

## 冻结身份

- 起点 commit：
  `11528a990a6f0b46fa7adeeaaf47295d841fbeb1`；
- 起点 tree：
  `4c2a825844d30599e1641aaa84438b1e465fc1c7`；
- WIP candidate commit：
  `9fd1ff8db91199d6b37d615b333f86a28ed7fcef`；
- WIP candidate tree：
  `c0d70f22e873e8b6356fbd3c19c878e0ab92af46`；
- offline evidence checkpoint：
  `4723db67`；
- Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3；
- Cargo.lock SHA-256：
  `f7fe42f3cd6c64bf01b3ca089b2a2767378ad49b14769d2454a0727ea998c8e0`；
- rust-toolchain.toml SHA-256：
  `d5de56d1101f9e3ba0d694a777665be841781229b16408c262294e4de9d2ca85`。

机器可读冻结契约见
[m10-c-acceptance-progress-offline-v1.json](../manifests/m10-c-acceptance-progress-offline-v1.json)。

## 候选机制与边界

已删除候选曾：

- 从 TaskContract、receipt/rejection、Host verifier observation、workspace generation/
  revision 与 temporal evidence request-locally 派生逐 acceptance 状态；
- 通过同一个 `RuntimeContextInput` 供 live Runtime 与 RunStore validation 使用；
- 只在 ContextBroker 渲染 model-visible progress，不新增 RuntimeEvent、State schema、
  store、tool、plan 或完成 owner；
- 用 persisted system-prompt 中的 exact volatile marker 让 SQLite reopen 重建同一
  treatment，而不依赖 mutable process config；
- 通过一个临时 global-only typed config 让同一 binary 构造 control/treatment；
- 覆盖 pending、current/stale receipt、Host rejection、temporal failure/mutation、
  compaction、root/read-only/Writer 与 production loopback。

这些机制测试证明候选可以正确实现，但不能证明其比现有路径更有效。机制正确不覆盖
产品净收益门。

## Offline viability

同一 deterministic `RunSnapshot`、相同 canonical ContextBroker token estimator 的冻结
结果是：

| 状态 | control | treatment | delta | 模型请求可见 |
| --- | ---: | ---: | ---: | --- |
| pending | 235 | 356 | +121 / +51.5% | 是 |
| verifier rejected | 538 | 547 | +9 / +1.7% | 是 |
| receipt satisfied | 492 | 386 | -106 / -21.5% | 否 |

精确断言在 evidence checkpoint 中通过后随 treatment 一并删除；结果和身份已冻结在
manifest 与 Git 历史。这里不把估算 Token 冒充 API usage，而只把它用作 credential
前的同输入 viability gate。

## Current fixed-Pro 恢复证据

ignored frozen raw：

- `eval/results/m9-c-fixed-pro-regression-7a91bbaab590-v1.jsonl`；
- 11,611,383 bytes；
- SHA-256：
  `c73a01a4934b82b3f4a4042791aae89e5da56feaea1ffbea188a80d4e6e05b77`。

两个独立的 `root_recovery` arm：

| record | verified | false success | requests | direct writes | cache-miss input | wall ms |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| `b8ce3a7a...af0` | 1 | 0 | 5 | verifier, edit, verifier | 10,029 | 29,637 |
| `ced6cb4d...143` | 1 | 0 | 5 | verifier, edit, verifier | 4,316 | 22,449 |

两者 response usage、cost、canonical Store/reopen 与 external verifier 都完整。它们只
证明 current fixed-Pro 已能可靠完成这类恢复，不证明其已经最优；但足以说明 M10-C
不能在“未观察到的失败 + request-visible Token 增长 + 1,501 净新增行”下越过付费门。

## Cutover 与删除

M10-C 不接管：

- 删除 `crates/runtime/src/acceptance_progress.rs` 及所有 derived status/reason/need
  类型和 mapping；
- 删除 ContextBroker treatment renderer、validation error、Host-failure visibility
  branch 和 treatment-only tests；
- 删除 immutable prompt marker、ledger layer 与 detector；
- 删除 `[context].acceptance_progress` config、CLI/TUI/app production wiring 与 tests；
- 删除 root/read-only/Writer prompt parity、compaction、production recovery 的
  treatment-only assertions；
- Runtime 与 Store 恢复同一个原有 `ContextInput` 构造路径；
- production 保持原来的 canonical TaskContract/EvidenceReceipt/latest-revision
  completion gate 与 fixed actor routing。

删除后的整个 `crates/` tree 与起点 `11528a99` 字节级一致。没有 compatibility reader、
dual path、第二 store、第二 completion truth 或失去消费者的生产代码。

保留 offline manifest、Git 历史与本 summary；它们是为什么没有继续付费实验和为什么
删除候选的审计证据，不是 current production consumer。

## 验证

删除 cutover 通过：

- `cargo fmt --all -- --check`；
- `codewhale-context` 18/18；
- runtime conformance 82/82；
- canonical production verifier-failure loopback；
- `codewhale-config` 74/74；
- `cargo check -p codewhale-app --locked`；
- `./scripts/dev-codewhale.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- process-level crash/reopen 与 root/read-only/Writer conformance；
- CLI/TUI/app-server projection gates；
- `git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0` 与独立
`CARGO_TARGET_DIR=/private/tmp/codewhale-m10c-target`；完成后精确删除该 target。

## 非结论

M10-C 不证明：

- 所有 derived acceptance projection 都必然有害；
- treatment 与 control 的 live fixed-Pro success、Token、时间或费用差异；
- current recovery 已无任何改进空间；
- 应放宽 `billing_unknown -> formal campaign stop`；
- 应恢复 Auto、FIM、Anthropic Messages、第二 Provider、第二 Runtime/Store、plan store
  或 LLM completion authority。

下一独立切片是 M10-D Failure-Directed Recovery Controller：先从 canonical trajectory
冻结真实 failure→action 损失矩阵，再判断是否有一个无额外分类请求、无第二 loop 的
最小 Host policy 候选；不得把 M10-C 的未运行 live treatment 当作其基线样本。
