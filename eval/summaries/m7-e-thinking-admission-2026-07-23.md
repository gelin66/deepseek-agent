# M7-E 默认 Thinking 准入结论

## 1. 结论

- 起始 clean revision：
  `7b8b485154d55b3e6a3d1a78599efaaff850b932`（M7-D 结论文档）。
- production candidate：
  `b9b83cdf267dd386a62d7a005308c7638c8e2490`。
- evaluator v2/v3/v4 checkpoints：`30ce183f`、`53475dc4`、`ee73e761`。
- final fail-closed Harness checkpoint：
  `458c3d7d12c673f2b4fd2b79247b883399d6566c`。
- 协议保持 Run API v10、RuntimeEvent v16、State v21、exec-stream v2。
- 产品决策：**hold**。production 默认继续使用当前 `Auto` / thinking-enabled
  规划；没有产品模式、模型可见工具、第二 Runtime、第二 Store、第二模型循环或第二工具
  目录。
- M7-E 没有获得可采纳的 15 pair / 30 arm 正式结果，因而不声明 verified success、
  Token、时间或费用收益。v5 在 output reservation、Key read 和官方 API 之前固定返回
  `live_api_not_admitted`。

当前正式样本中 reasoning 的成本具有足够强的方向性信号：M7-A2 的成功候选 17 个请求包含
3,042 reasoning tokens 和 7,263 reasoning replay tokens；较早完整 eager-join suite 的
154 个请求包含 21,349 reasoning tokens 和 27,629 replay tokens。仓库又已经拥有同一
Standard Chat production 链上的 `high` / `off` 官方字段，因此它是审计后唯一能形成
同 binary 非编辑 treatment 的候选。该事实只支持开展准入实验，不支持默认关闭 thinking。

## 2. 切片合同

| 项目 | 冻结合同 |
|---|---|
| 真实问题 | 默认 thinking 会产生 reasoning output 和后续 tool-call replay input，但没有同 binary 产品证据证明它带来相称 verified success |
| 验收条件 | 5 个任务、每 variant × task 3 次、15 pair / 30 arms、`maximum_reruns=0`；success/false-success、accounting、身份、权限、最新 revision、external verifier 全闭合 |
| 单一 owner | `crates/deepseek` 拥有 thinking planner/parser/accounting；`AgentRuntime` 与 `RunStore` 拥有 exact request、event、receipt 和 terminal |
| treatment | 同一 immutable binary 只把 `StartRunCommand.reasoning_effort` 从 `high` 改为 `off`；模型、工具目录、任务、预算、Runtime、Store、completion 与 verifier 不变 |
| 替代旧路径 | 只有全新正式 A/B 同时通过可靠性与收益门槛，才替代当前 `Auto` 默认；本次未达门槛，因此无 cutover |
| 删除点 | 未新增 production treatment branch 或模式，无生产旧路径可删；v5 删除继续 live 的权限，并冻结 successor-only re-admission |

## 3. 官方协议复核

2026-07-23 重新核对以下 DeepSeek 官方一手资料：

- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Change Log](https://api-docs.deepseek.com/updates/)

官方资料确认 thinking 是 Standard Chat 上的开关与 effort 控制；涉及 tool call 的
`reasoning_content` 必须在后续请求中完整回放。M7-E 不按泛 OpenAI 经验推断字段。易变的
模型、wire body 与价格只由
[`m7-e-thinking-admission-v5.json`](../manifests/m7-e-thinking-admission-v5.json)
fixture 固定：`deepseek-v4-flash`、enabled/high 对 disabled/off，以及当时的
cache-hit/cache-miss/output price。后续若重新准入，必须重新复核官方资料并新建 successor
manifest，不得原地改写 v5。

## 4. 冻结 treatment 与公平性

candidate 在现有 production composition 上补齐两项可复核事实：

1. `crates/deepseek` 的 fixture 证明 `high` 与 `off` 都走 Standard Chat，且 wire delta
   只落在 `thinking` / `reasoning_effort` 及其诱发的 reasoning replay。
2. `crates/app` 的 production test 证明 high/off 首个 exact `ModelRequest` 和完整
   `RequestPlan` 可以在 SQLite reopen 后逐字段重建；只归一化唯一 Host-owned
   `task_generation` 行后，同一稳定 workspace 的首请求语义身份相同。

v5 Harness 进一步冻结：

- 每个 pair 使用同一 suite-owned absolute workspace slot；两个 arm 分别从同一 fixture
  重新物化，State、RunStore、run ID 和 binary copy 保持隔离。
- pair 一闭合就比较真实 semantic messages、actor、system prompt、tools、模型、surface、
  streaming、max output、revision、binary、fixture 和 schedule，不等 30 arm 才发现偏差。
- 只允许归一化恰好一条 `task_generation` Host fact；不得归一化
  `workspace_revision` 或其他 wire 事实。
- `verifier_failed` 只有满足“失败 → 后续有效 edit → 后续 verifier success”，且 Host
  temporal receipt、external verifier、scope、measurement、authority 全部通过时才算恢复。
- 任何 unresolved transport/operation/side-effect ambiguity、billing unknown、
  unpriced/incomplete accounting、身份偏差或 credential 泄漏都立即保存证据并停止。

v5 frozen identities：

- manifest SHA-256：
  `20e9c7b9294805293023a028977b4b3f7fdc166b0e7b19432ab8f9faeec83602`
- manifest canonical content（不含 frozen hashes）：
  `36963f25e86d8f514e2b8e9b977df90ca2880a50dd6d0919d0c8e20cf50aad10`
- Harness：
  `c2dd0197795bc74fb217df24545cf9c65cae16a3e16b5751bf84324a87b9e3c0`
- Harness regression：
  `ae9e989867a6a442766876eddc030d6421057aa8902f6b01a2f789a184660f47`
- derived 30-arm schedule：
  `47cc8f79905caff0dcf3b3968dca2748fae68df3446166bee3601ee5268367bc`
- frozen task source：
  `ec30735bc68428c00135ecfaa44b31f6ae60a83a9aa775dfb9d635a6da253d94`

## 5. 失败尝试与停止边界

四次尝试都暴露 evaluator/fairness 缺陷，全部排除出产品指标；它们不是可拼接的 partial
formal：

| 版本 | 完成 arms / requests | 已知费用（nanousd） | 失效原因 |
|---|---:|---:|---|
| v1 `b9b83cdf` | 1 / 5 | 1,720,426 | 把成功 `run_verifiers` 的保守 `side_effect=indeterminate` 错判为失败歧义 |
| v2 `30ce183f` | 5 / 26 | 7,147,206 | 把任务明确要求且已闭环的 verifier failure recovery 错判为 unresolved ambiguity |
| v3 `53475dc4` | 7 / 36 | 9,522,149 | T3-only allowlist 仍错误拒绝 T4 的真实 edit → verify recovery |
| v4 `ee73e761` | 5 / 27（仅已完成 arms） | 6,346,379（下界） | pair 两臂使用不同随机 workspace；真实 `workspace_revision` 因 absolute path 不同而改变 prompt |

v1–v4 共 18 个已完成 arm，全部 verified、false success 0，共 94 个已完成-arm requests，
已知费用 24,736,160 nanousd（USD 0.024736160）只是下界。任何局部 high/off 差值都只是
evaluator-invalid 诊断，不是产品指标。

v4 在发现第二个同类 pair identity mismatch 后由外部 SIGINT 精确停止；Harness 的
`finally` 回收 app-server。停止时 `t3/reasoning_off/run_1` 可能已有 in-flight API request，
而该 arm 的临时 State/RunStore 已按 Harness 清理，无法重建最终 billing。原 raw 保持
`status=running`，不得事后改写成 aborted 或 complete：

- exact binary commit：`ee73e7612f20af21b8d01cb323690e722650fc5d`
- binary SHA-256：
  `be086bee06675cc91b0c09cb54c6599c1a4d07ecaf319f51a6b6c402145a3a66`
- binary size：14,901,088 bytes
- v4 raw SHA-256：
  `e6ae9602aff62d38f06a8d7b322f4e9115eab26f4105ec6c35f7fa8bba8ae70c`

四份 raw 都保留为 Git ignored、`0600`、脱敏文件。v5 没有 raw；它在 preflight、output
reservation、Key read 之前因 `live_api_admitted=false` fail closed。M7-E 不再读取 Key，
不再发出官方 API 请求。若未来要复评，必须显式解决或接受上述 unknown-billing 边界，并以
新的 clean revision、successor manifest、suite ID、binary 和 output 从 position 1 开始。

## 6. 离线与生产门禁

final `458c3d7d` 已通过：

- 15/15 Harness regressions 和 freeze-report identity；
- DeepSeek high/off endpoint/body/planner/parser/accounting targeted tests；
- app production high/off exact RequestPlan SQLite reopen；
- `cargo fmt --all -- --check`；
- owning crates targeted test/check；
- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- production loopback；
- Tool/Runtime process crash/reopen；
- root/read-only child/explicit Writer conformance 与 Writer integrate/cleanup；
- CLI/TUI 简体中文 canonical projection；
- `git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0` 与
`CARGO_TARGET_DIR=/private/tmp/codewhale-m7e-target`。最终交付会精确删除该 target；没有
M7-E 临时 worktree、评测 binary 或残留进程。

## 7. 产品取舍与复杂度

- **keep**：现有官方 high/off planner/parser/accounting contract、exact replay/reopen
  production test、v5 公平性与 fail-closed gate。
- **hold**：默认关闭 thinking、任何自动 thinking admission，以及新的 credentialed A/B。
- **reject as evidence**：v1–v4 局部 paired 指标、把 v4 raw 补写成终态、把 unknown billing
  当零、只归一化 prompt hash 而不修复真实 workspace identity。
- **shrink/delete**：没有新增 production surface 可删；v5 把 live 执行权限收窄为 false，
  后续只能由新 successor 显式重新准入。

从起始 revision 到 final Harness 没有改变 production 默认、工具目录、Runtime、Store、
completion owner 或用户配置。新增内容集中在 protocol/production contract tests 与
eval-only Harness/manifest；每一项都由真实 evaluator 反例解释。没有临时 adapter 留到下一
里程碑。

## 8. 非结论与下一切片

M7-E 不证明：

- reasoning-off 提高、保持或降低完整 5-task suite 的 verified success；
- reasoning-off 稳定降低 Token、请求、wall time 或费用；
- v1–v4 的 18 个 verified arms 可代替 30 个公平正式 arms；
- 当前 `Auto` 默认应该改变；
- unknown active-arm billing 可以从已完成-arm accounting 推断。

下一切片不应立即重做付费 thinking A/B。现有 raw 暴露了更直接的非编辑事实：production
workspace revision 和 Host 动态事实会进入首请求并改变前缀。建议 M7-F 先只读审计 exact
`ModelRequest` 的 stable-prefix/cache break position，确认哪些变化是必要的 freshness /
evidence，哪些只是可安全后移的 volatile projection。只有在不隐藏 Host 事实、不改变
语义或 replay、且能形成同 binary wire delta 时，才建立最小 prefix treatment；否则继续
hold，并且不读取 Key、不调用官方 API。
