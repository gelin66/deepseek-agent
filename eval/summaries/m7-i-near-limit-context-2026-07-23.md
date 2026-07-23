# M7-I near-limit context/request budget 结论

- 日期：2026-07-23
- 分支：`deepseek-agent`
- production baseline：`6e9e9b7b8bbbaff678994de64acec2328b93cc39`
- production tree：`d360072e4d2af26612061d6e47d620682a585853`
- baseline/manifest/Harness checkpoint：`f8d0b2423f3edf708b86bb140eb1556309d11ee5`
- test-only strict lint checkpoint：`b1894069`
- 协议身份：Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2
- 最终决策：`close_request_token_optimization_and_admit_m8_cleanup`

## 1. 问题与切片合同

M7-H 修复了 terminal `tools=[]` 请求在 hard-limit admission 前错误使用 ordinary catalog 的
确定性 Host 缺陷，但历史 raw 没有 current schema 的逐请求 advertised catalog，也没有
compaction 或 hard-budget exhaustion 样本。现有正确性证据分散在 Runtime、Context、
State、DeepSeek、app composition、Writer 和进程级 crash tests，不能作为一个可复现的
current-revision near-limit 基线。

本切片合同是：

1. 真实问题：证明每次实际请求使用的 catalog、context estimate、compaction ordering、
   request budget 与 reopen 后 RequestPlan 来自同一 canonical truth；先找反例，再决定是否
   修改 production。
2. 验收条件：ordinary/terminal、compaction success、mandatory facts over limit、逻辑/
   物理预算、partial failure、cancel、crash/reopen、root/read-only/Writer 和真实
   Git/verifier loopback 全部可复现。
3. 单一 owner：Runtime 决策在 `crates/runtime`，estimate 在 `crates/context`，持久 truth
   在 RuntimeEvent v16 + `crates/state`，wire plan 在 `crates/deepseek`。
4. 替代旧路径：不再从旧 aggregate raw 猜 terminal catalog，不把 historical zero sample
   当作 current boundary coverage，不为 evaluator 新增第二 estimator 或 permit 状态。
5. 证据：exact Rust regressions、SQLite reopen、production composition、process SIGKILL、
   deterministic verifier、Harness 和完整离线门禁。
6. cutover 删除：没有 material delta 时不保留 live/credential path；不保留 duplicate
   estimator/planner serializer；最终删除本 Goal 的 target、进程和 worktree。

## 2. 只读调用图审计

当前唯一事实链是：

```text
AgentRuntime
  -> 选择 ordinary catalog 或 reserved terminal tools=[]
  -> ContextBroker::effective_context(catalog)
  -> 可选 ContextCompactionCommitted
  -> ModelRequestPrepared { full ModelRequest }
  -> State v21 reducer / SQLite reopen
  -> DeepSeek planner(ModelRequest) -> RequestPlan
```

审计结果：

- root、read-only child 与 explicit Writer 都使用同一个 `AgentRuntime`；
- actual catalog 在 hard-limit/compaction 前确定，M7-H 的旧 full-catalog 分支已经删除；
- `ModelRequestPrepared` 持久化完整请求，不需要另存 advertised catalog、estimate 或 terminal
  permit；
- `ContextCompactionCommitted` 已包含 source digest、before/after estimate、projection、
  usage 和 matching tools；
- State v21 只 reducer/replay canonical events，SQLite reopen 不重新猜测请求；
- DeepSeek backend 只从 `ModelRequest` 确定 endpoint、body 和 response mode；
- Runtime logical request gate 与 DeepSeek physical admission budget 有不同 typed
  failure/accounting，不能合并成一个计数；
- partial output/side-effect ambiguity 不自动盲重试，cancel 和 crash recovery 使用既有
  typed terminal contract。

没有发现第二 Runtime、Store、planner、estimator、catalog 或 hidden model loop。

## 3. 冻结矩阵

manifest `eval/manifests/m7-i-near-limit-context-v1.json` 固定
`maximum_reruns=0`、同一 production composition、外部 Cargo target 和以下 13 项：

| ID | lane | boundary |
|---|---|---|
| N01 | root | ordinary request actual catalog/context |
| N02 | root | reserved terminal `tools=[]` |
| N03 | root | ordinary compaction success |
| N04 | root | terminal compaction success |
| N05 | root | mandatory facts over limit |
| N06 | root | logical model-request budget |
| N07 | root | physical API admission budget |
| N08 | root | actionable partial model failure |
| N09 | root | in-flight cancel |
| N10 | root | compaction commit SIGKILL/reopen |
| N11 | read-only child | shared broker/catalog/reopened plan |
| N12 | explicit Writer | catalog/budget/reopened plan |
| N13 | root | temporary Git workspace + deterministic verifier |

Harness `scripts/eval-m7i-near-limit.py` 只校验 immutable manifest、运行 exact Rust filters 并
记录进程 verdict；它不实现 estimator、reducer、planner、terminal classifier、工具或
Agent loop。

## 4. 新增缺口覆盖

`f8d0b242` 只增加测试和评测资产：

- `m7i_actor_request_plans_rebuild_from_v16_sqlite_events`
  - 使用真实临时 Git workspace；
  - 由 production composition 建立 root、read-only child、explicit Writer 请求；
  - 逐 actor 检查实际 catalog 与权限；
  - 将完整 `ModelRequestPrepared` 写入 StateStore；
  - SQLite reopen 后请求逐字段相等；
  - ContextBroker 重算 estimate 与原事件一致；
  - DeepSeek `RequestPlan` 在 reopen 前后逐字段一致。
- `mandatory_facts_over_limit_fail_before_any_model_request`
  - mandatory task constraints 自身超过 `hard_limit=1000`；
  - terminal 为 typed `ContextLimitExceeded`；
  - ModelPort 调用、logical/physical request、compaction 和 prepared request 全部为 0；
  - reducer replay 后 terminal 与 accounting 精确。

`b1894069` 只把新增测试中的布尔表达式改为 strict Clippy 接受的显式 `if/else`，没有改变
production Rust 或测试语义。整个 M7-I 没有 schema/version bump。

## 5. Formal offline 结果

formal 在 clean `f8d0b242` 前后保持同一 revision
`f8d0b2423f3edf708b86bb140eb1556309d11ee5` 与 tree
`5cd343ec423d29adbba66ce6d85d68375705e93f`：

- 13/13 matrix cases；
- 16/16 exact gates；
- `maximum_reruns=0`；
- `credential_read=false`；
- `official_api_requests=0`；
- `network_accessed=false`；
- `material_production_delta=false`；
- `product_metric_eligible=false`。

ignored raw 保留为：

- path：`eval/results/m7-i-near-limit-context-6e9e9b7b-v1.json`；
- mode：`0600`；
- size：3,616 bytes；
- SHA-256：`aac12ec7c16b8ce28d5acf0cd2b5753297a838b12d68fbd46382ab3edfd5dc2c`；
- manifest SHA-256：`ed54c9921ffeac57e4da05b4a192acf652ec7666011eb40d6c4cd460b4850a04`。

raw 不覆盖、不重跑、不进入 Git。由于没有模型、prompt、reasoning、catalog、预算、
context policy 或 Provider treatment，读取 Key 和发送付费请求不能增加归因力，credential
gate 在 Key 前停止。

## 6. 门禁

以下门禁使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m7i-target
```

结果：

- Harness self-test：通过；
- formal Harness：16/16 通过；
- targeted runtime：40 unit + 82 conformance + 25 Writer，0 failure；
- targeted app：54 passed、1 ignored helper；
- State process SIGKILL/reopen：通过；
- `./scripts/dev-deepseek-agent.sh focused`：通过；
- `cargo fmt --all -- --check`：通过；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过；
- `cargo test --workspace --locked`：通过；
- production Git/verifier loopback：通过；
- real PTY 与 exec/http/stdio parity：通过；
- `git diff --check`：通过。

## 7. 产品决策

决策是：

**关闭 M7 request/Token optimization，准入 M8 cleanup。**

理由：

- current v16/v21 的 actual catalog、context estimate、compaction/request ordering、
  terminal permit、预算失败和 reopen plan 已有可复现闭环；
- mandatory facts over limit、partial failure、cancel 与 compaction crash 都 fail closed；
- root、read-only child、explicit Writer 没有 conformance 分叉；
- 没有新的 production counterexample，不能解释任何 production 代码或状态新增；
- live API 没有可比较的 material treatment。

保留 M7-H deterministic Runtime correctness fix；M7-I 只保留低复杂度 test/manifest/Harness
证据。没有旧 production path 需要替换或删除。

## 8. 非结论

M7-I 不证明：

- near-limit、compaction 或 hard-budget exhaustion 在一般编码任务中的发生频率；
- 当前 Agent 已降低 Token、模型请求、wall time 或费用；
- verified task success 或首次完成率得到提升；
- 更改 reasoning、prompt、budget、context policy 或 provider cache 会有净收益；
- FIM、thinking-off、cache-prefix rewrite 或自动 fan-out 应被准入；
- historical raw 的 zero sample 能代表 current production 分布。

## 9. 下一切片

M8 先做一个独立的 DeepSeek-only 产品清理切片：

- 只读审计三个保留入口的 credential/config/model/Doctor/onboarding/help caller；
- 冻结首次启动、无 Key、非法 Provider/模型、resume 与 CLI/TUI/app-server parity；
- caller 切到唯一 DeepSeek owner 后，物理删除无 production consumer 的 generic Provider、
  model catalog、pricing、alias、route 和 UI/config 旧路径；
- 不在同一切片混入品牌改名、发布、RepoGraph、多 Writer、通用 DAG 或新模型选择系统。
