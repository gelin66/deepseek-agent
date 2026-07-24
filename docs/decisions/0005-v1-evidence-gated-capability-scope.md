# ADR-0005：V1 以可验证能力定义范围，优化实现按证据准入

- 状态：已接受
- 日期：2026-07-24

## 决策

V1 完成定义描述用户可验证的能力与安全边界，不把尚无归因证据的优化实现名称当作
发布门槛：

1. 多 Agent 的 V1 写入能力是一个显式准入的 isolated Writer。它与 root/read-only
   child 共用同一 `AgentRuntime`，并完成 worktree、diff、verify、fast-forward
   integration、最新 root verify 和 cleanup。并发多 Writer 不是 V1 要求。
2. DeepSeek 的 V1 production surface 是官方 Standard Chat，以及对整组工具做兼容性
   判定后选择 Beta Strict Chat 或无损回退 Standard Chat。FIM 不是 V1 要求。
3. 跨文件理解的 V1 能力由 canonical `file_search`、`grep_files`、`list_dir`、
   `read_file`、Git working-set facts、ContextBroker 和 deterministic verifier 共同验收。
   名为 RepoGraph 的索引实现不是 V1 要求。

多 Writer、FIM 和 RepoGraph 可以在后续重新进入，但必须分别先出现新的 current
production typed failure，并能把失败归因到缺少该能力；随后还要冻结同 revision、
immutable binary、deterministic verifier 和完整 accounting 的真实 treatment。没有这些
证据时继续保持 hold，不通过恢复旧代码、添加模式开关或制造无消费者抽象来获得准入。

## 原因

- ADR-0003 要求所有写 Agent 使用隔离 worktree，但没有要求 V1 同时调度多个 Writer。
  M6-B1 的 18 对 / 36 arms 正式 A/B 判定 `reject_and_rework`：单 Writer 到 Writer 的
  verified success 为 `2/18 -> 4/18`，但 Writer 仍有 7 次 false-success，Token、费用和
  时间分别增加 35.5%、52.5% 和 39.8%。后续 v3 又因 unknown billing 停止。现有单
  Writer lifecycle 保留为 explicit-only，没有证据支持扩展 Writer 数量。
- M7-C 已把冻结的 Host 编辑正确性矩阵从 3/12 修到 12/12。M8-H 复核此后没有新的
  current production 编辑生成失败，且旧 FIM planner 没有 sender、response parser、
  Host apply 或 reopen consumer，因此已删除该半分支。官方 FIM 仍是独立 Beta
  Completions surface；它不是 Strict Chat 的延伸。
- M5-B 没有把失败定位为结构检索缺失，M5-C 因而预注册为证据触发。当前 production
  已有唯一 ContextBroker、固定搜索/读取工具和 latest-revision verifier；仓库没有
  RepoGraph caller。仅为满足实现名称而加入 tree-sitter、LSP 或 embedding 会增加 owner
  和代码复杂度，却没有可归因的用户任务损失。
- `PRODUCT_PLAN` 第 12 节已把 RepoGraph 实现、Agent 数量和 Strict/FIM 路由列为可根据
  证据调整的实现。原 V08/V09/V10 的字面要求比这些边界更强，导致 V1 清单与正式评测
  结论互相冲突。

完整冻结输入见
[`m8-k-v1-scope-successor-v1.json`](../../eval/manifests/m8-k-v1-scope-successor-v1.json)。

## 后果

- ADR-0003 的核心架构保持不变：多 Agent 仍是
  `AgentRuntime x N + Orchestrator`，任何写 Agent 仍必须使用独立 worktree。
- `crates/deepseek` 继续只拥有当前可达的 Standard/Strict Chat
  planner/transport/parser/accounting；不恢复 `/beta/completions`、FIM parser 或第二
  model loop。
- `crates/context` 继续拥有 bounded projection 与 hard-limit compaction；RepoGraph 若
  重新准入，只能作为同一 ContextBroker 的候选事实来源，不拥有任务状态或完成判定。
- V08、V09、V10 按本 ADR 的能力定义关闭；current V1 matrix 从 10 pass / 6 blocked
  变为 13 pass / 3 blocked。V13、V15、V16 仍 blocked，V1 仍不可发布。
- 本决策不证明多 Writer、FIM 或 RepoGraph 质量较差，也不证明当前实现获得成功率、
  Token、时间或费用提升。

