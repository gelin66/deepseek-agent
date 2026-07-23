# M7-F canonical context-cache 前缀审计结论

## 1. 结论

- 起始 clean revision：
  `8285a883322fa9b0040fba26ef833ee9808001eb`（M7-E 结论文档）。
- 离线 contract / production-loopback checkpoint：
  `a1d68b05d1e252fe55aa4ea2f19b6dc910e74873`。
- 协议保持 Run API v10、RuntimeEvent v16、State v21、exec-stream v2。
- 产品决策：**hold**。当前 Host-facts suffix 确实使下一请求不能完整延伸上一请求的
  user-input / model-output cache-prefix unit，但没有候选同时满足真实 wire delta、freshness、
  evidence、权限、exact replay、crash/reopen 与模型可见语义不变。
- production 行为没有改变；没有新增产品模式、模型可见工具、Runtime、Store、模型循环、
  请求真相或工具目录。
- `live_api_admitted=false`；没有读取 Key、没有官方 DeepSeek API 请求、没有新 live raw
  或未知计费。

M7-F 没有把“缓存命中率越高”预设为正确方案。真实北极星仍是 verified task success /
tokens / wall time / API cost / code complexity。当前问题存在，但在能安全表达历史 Host
事实的 supersession 之前，直接为缓存重写 transcript 的复杂度和 stale-fact 风险高于已证明
收益。

## 2. 切片合同

| 项目 | 冻结合同 |
|---|---|
| 真实问题 | DeepSeek 只命中完整匹配的已持久化 cache-prefix unit；CodeWhale 每轮临时 Host facts 不进入下一轮 transcript |
| 验收条件 | exact `ModelRequestPrepared`、`RequestPlan` 与 raw loopback HTTP 一致；定位首个 break；fresh revision/evidence、reasoning/tool replay、actor 权限与 SQLite reopen 不退化 |
| 单一 owner | `crates/context` 拥有 prompt/Host-fact projection；`crates/deepseek` 拥有 wire planner/usage；Runtime/RunStore 拥有 exact request/event/replay |
| 替代旧路径 | 只有真实、安全、可归因 wire treatment 才替换当前 projection |
| 测试证据 | 真实三轮 production loopback、DeepSeek wire contract、ContextBroker、root/read-only/Writer、SIGKILL/reopen |
| cutover 删除 | 没有安全 treatment，因此无 production branch；只保留 contract tests、manifest、offline result 与结论 |

冻结 manifest：
[`m7-f-context-cache-prefix-v1.json`](../manifests/m7-f-context-cache-prefix-v1.json)。
离线结果：
[`m7-f-context-cache-prefix-offline-a1d68b05.json`](../results/m7-f-context-cache-prefix-offline-a1d68b05.json)。

## 3. 官方协议复核

2026-07-23 重新核对以下 DeepSeek 官方一手资料：

- [Context Caching](https://api-docs.deepseek.com/guides/kv_cache/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Change Log](https://api-docs.deepseek.com/updates/)

当前官方规则不是“任意最长公共前缀立即命中”。后续请求必须完整匹配一个已经持久化的
cache-prefix unit；unit 可在用户输入末尾、模型输出末尾、重复请求检测到的公共前缀及长输入/
输出的固定 token 间隔形成。缓存是 best-effort，通常保留数小时到数天，不能保证每次命中。
官方响应用 `prompt_cache_hit_tokens` 与 `prompt_cache_miss_tokens` 报告结果。

manifest 固定了复核当日的易变事实：

- `deepseek-v4-flash`：cache-hit input `$0.0028`、cache-miss input `$0.14`、output
  `$0.28` / 1M tokens；
- `deepseek-v4-pro`：对应 `$0.003625`、`$0.435`、`$0.87`；
- `deepseek-chat` / `deepseek-reasoner` alias 当时公告将在
  `2026-07-24 15:59 UTC` deprecated。

M7-F 没有按泛 OpenAI 经验猜测多 system message、tool schema 或 JSON key order如何参与
provider cache key。

## 4. 当前 production 调用图与真实 break

```text
production_system_prompt
  -> SystemPrompt[stable constitution, volatile world-state blocks...]
  -> ContextBroker canonical transcript
  -> final Host-facts User message
  -> ModelRequestPrepared (RunStore)
  -> plan_runtime_chat
  -> one flattened system message + ordered transcript/Host facts
  -> /chat/completions
```

### 4.1 system 与工具目录

- `crates/context` 把 stable constitution 放在 blocks[0]，随后按固定 identity 排列 workspace、
  permissions、route 和可选 topology/skills-tools/token-budget volatile blocks。
- `crates/deepseek` 不发送 `cache_control`。它把全部 blocks 用固定分隔符拼成一个 system
  message；因此 typed stable/volatile boundary 是 Host metadata，不是 provider wire boundary。
- `crates/tools` 的 production tool definitions 已按固定字母顺序生成；root、read-only child、
  Writer 与 terminal catalog 的差异来自权限/生命周期，不是随机漂移。

### 4.2 Host facts

ContextBroker 每个请求最后追加一个非 transcript Host user message，包含：

- task generation 与 acceptance IDs；
- workspace generation/revision；
- 当前有效 EvidenceReceipt 或 none；
- 未解决的 completion rejection、最近 verifier failure 及其 workspace。

这些事实在当前位置是正确的：它们不破坏 system、任务和已提交历史的更早公共前缀；写入或
验证后还会正确刷新。但是该 Host tail 没有 canonical transcript index。模型响应提交后，
下一请求的历史包含 assistant/tool output，却不含触发该输出的上一轮 Host facts。

三轮 production loopback 的 exact `ModelRequest.messages` 为：

| request | message count | 与前一请求公共完整 message 数 | Host facts |
|---:|---:|---:|---|
| 1 | 2 | — | 初始 revision |
| 2 | 4 | 1 | 与 request 1 完全相同（上一轮只读） |
| 3 | 6 | 3 | edit 后 revision 正确变化 |

公共 message 数分别等于上一请求 `message_count - 1`。换言之，即使 Host facts 未变化，
request 2 仍不是 request 1 input + output 的完整延伸；break 后第一个 message 是已提交的
assistant output。编辑后 freshness 仍正确，但同样不保留旧 request-boundary unit。

官方“重复公共前缀检测”可以在后续请求持久化较早公共前缀，所以这通常是一轮滞后，而不是
完全禁用缓存。loopback 还证明每个 raw HTTP body 等于从 persisted `ModelRequestPrepared`
重建的 `RequestPlan`，且 SQLite reopen 后 event/snapshot 精确一致；这不是 Harness 猜测。

## 5. 既有 cache 观测基线

四份 M7-E ignored `0600` raw 中，18 个已完成且 accounting 闭合的 arms 合计：

- 94 requests；
- 395,812 input tokens；
- 281,344 cache-hit；
- 114,468 cache-miss；
- aggregate hit ratio `71.0802%`；
- 28,296 output tokens；
- 已知费用 24,736,160 nanousd。

这些 usage/accounting 是有效的非因果 production 诊断，但 M7-E 的 evaluator/fairness
缺陷使其不能成为 M7-F treatment 指标。raw 没有逐请求 usage，也没有 M7-F candidate；不能
从 aggregate 71.08% 推断某个 break 的 token 数或潜在净收益。

## 6. 候选取舍

| 候选 | wire delta | 决策 | 原因 |
|---|---:|---|---|
| 只重标 stable/volatile blocks | 否 | reject | planner 仍发送一个相同 system message |
| 拆成多个 system messages | 是 | reject | 官方未定义其 cache 语义；会改变 tokenization/模型语义 |
| Host facts 前移 | 是 | reject | revision/evidence 一变会更早破坏 prefix，并改变指令优先级 |
| 删除或粗化 generation/revision/receipt/failure | 是 | reject | 破坏 freshness、evidence 与 latest-revision completion |
| tool catalog 重排/复制 | 是 | reject | 当前顺序已稳定；官方未定义 tool cache key，且会增加 token |
| 把每轮旧 Host facts 按时间顺序写回下一轮 | 是 | hold | 可恢复 input/output boundary 延伸，但会把已过期 revision/receipt 加入后续模型输入，并需要 typed supersession、projection/reducer 与 reopen 状态 |

最后一个候选是真实的潜在 treatment，不是本阶段可安全接管的最小改动。仅仅把字符串复制进
transcript 会制造“旧事实是否仍有效”的第二语义；也会影响 compaction、child fork、reasoning
replay、completion rejection 与 crash recovery。没有这些合同前，不为 cache 命中率弱化
canonical truth。

## 7. 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m7f-target
```

通过的 targeted 证据包括：

- DeepSeek deterministic planner/raw reasoning arguments；
- cache-control metadata flattening；
- multi-tool result adjacency 与 child handoff；
- `codewhale-context` 17/17；
- 三轮 AgentApplication production loopback + SQLite reopen；
- M7-E high/off 与 M7-B RequestPlan SQLite reopen；
- 六类 actor catalog identity；
- root/read-only child/Writer ToolOutcome conformance；
- child ContextBroker compaction、latest workspace evidence boundary；
- model in-flight、committed response 与 compaction SIGKILL/reopen。

最终还通过 `cargo fmt --all -- --check`、owning crate checks、
`./scripts/dev-deepseek-agent.sh focused`、workspace Clippy/test 与 `git diff --check`。
没有 credential、release binary、评测 worktree、后台进程或 live output。

## 8. 产品取舍与下一切片

- **keep**：唯一 production chain、当前 latest Host-facts tail、deterministic tool order、
  exact reasoning/tool replay，以及新 wire/reopen regression。
- **shrink**：文档不再把 `PromptCacheControl` 描述成 provider cache boundary；aggregate
  cache ratio 只作诊断，不作 treatment 证据。
- **hold**：typed chronological Host-fact snapshot + explicit supersession；任何 credentialed
  cache A/B。
- **reject**：事实删除/粗化、多 system message 猜测、tool duplication、新 cache 模式、
  第二 Runtime/Store/request truth。

若未来重开 cache 优化，第一步不是 live A/B，而是定义一个 derived、typed、可 compaction /
crash/reopen 的历史 Host-fact snapshot：旧 snapshot 明确为历史，latest snapshot 才能控制
freshness/evidence。只有该合同通过同 binary production wire identity 后，successor manifest
才可读取 Key。否则应转向另一个有更强损失证据的瓶颈。

M7-F 不证明：

- 保留旧 Host facts 会提高 cache hit、降低费用或保持 verified success；
- 71.08% aggregate hit ratio 能定位任一逐请求 break；
- 多 system message、body key order或 tool schema顺序能改善 DeepSeek cache；
- 当前 cache 行为导致了 M7-E 任一任务失败；
- 单次 provider hit/miss 能覆盖 best-effort 缓存方差。
