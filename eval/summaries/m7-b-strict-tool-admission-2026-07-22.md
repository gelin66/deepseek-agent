# M7-B DeepSeek Strict 工具调用准入与失败恢复

- 冻结日期：2026-07-22
- 起始代码：`7ae5b26898e8f3576f224003dfb271fc28f7ca7d`
- production candidate：`e6b4b64d37cc496dd7bd8a20dbf500be0413241b`
- final admission Harness：`7e7eaadfabce356c4eccab62c8589e3d9fe488b7`
- post-decision shrink / SQLite reopen proof：`4e3536f140e6995c1f157f21ba55c6926c136845`
- 结论：`inadmissible_no_surface_delta`
- 产品指标资格：`false`
- live A/B：未开始，0 arms
- 官方 API 请求：0
- credential read：`false`

## 1. 问题与边界

M7-B 要判断的是：DeepSeek Beta Strict Function Calling 能否在不改变 production 工具合同
的前提下，为真实 root、read-only child 和 isolated Writer 提供可归因的工具调用 treatment，
并补齐工具失败后的 typed 恢复闭环。

本阶段没有把 Strict 当作默认正确答案。普通 Chat 和普通工具调用仍是 Standard Chat；FIM
仍是独立 Beta Completions surface，本阶段没有实现 FIM 编辑器。固定边界保持不变：一个
`AgentRuntime`、一个 `RuntimeEvent`、一个 `RunStore`、一个固定 Host production tool schema
owner；条件内建的 `agent`/`request_user_input` 仍由 Runtime 唯一拥有，
本阶段没有新增 Provider、模型可见工具、第二目录、第二 Runtime、兼容层或多 Writer。

## 2. 实现事实

### 2.1 Strict 与 exact replay

- `crates/tools` 继续唯一拥有 production tool schema 和参数语义。
- `crates/deepseek` 只对每次请求实际 advertised 的完整目录执行确定性 compatibility 判定。
- 整组兼容时才能生成官方 Beta Strict Chat；任一工具不兼容时整组原子回退 Standard Chat。
- fallback 前后的工具数量、名称、顺序、schema 和语义完全一致；Standard body 不泄漏
  `function.strict`。
- RuntimeEvent 持久化当次完整 `ModelRequestPrepared`，其中 `tools` 就是实际 advertised
  catalog；State 同时保留 catalog hash，execution fingerprint 绑定 `strict_tools` policy。
  `surface` 与 fallback reason 是唯一 DeepSeek planner 的确定性派生结果，不在 State 再保存
  一份可能漂移的第二真相。SQLite 重开测试证明同一 exact request 可逐字段重建同一完整
  `RequestPlan`（包括 surface、reason、endpoint 和 body），而 strict policy 变化会改变
  fingerprint 并在恢复边界 fail closed。
- malformed 或不完整 tool-call fragment fail closed，不把部分参数交给执行器。
- reasoning 与 tool-call history 绑定真实 actor turn 精确回放，root 与 child 不共享或错配
  pending tool history。

### 2.2 工具失败恢复

RuntimeEvent v16 / State schema v21 要求每个失败 `ToolOutcome` 都有稳定
`failure_code`，并独立保留：

- invocation；
- transport；
- operation；
- side effect；
- retry disposition；
- evidence；
- artifact；
- workspace revision。

当前稳定失败类别覆盖 malformed arguments、schema validation、invocation rejected、unknown
tool、missing/invalid field、workspace precondition、stale read、ambiguous edit、patch parse、
operation、transport、side-effect ambiguous 和 verifier failure。模型收到一份确定性的中文
摘要与恢复建议，英文 code/字段保持稳定；成功工具输出不改写。

Runtime 不自动重复可能有副作用的工具。root、read-only child 和 isolated Writer 使用同一
conformance；真实进程 `SIGKILL`/reopen 覆盖 `ToolPrepared` 与
`ToolOutcomeCommitted` 两侧，已提交工具或 outcome 不会再次执行。TUI、JSON summary 和
exec-stream v2 只投影 canonical 事实，不建立自己的失败分类。

### 2.3 删除

- 删除没有真实执行差异的用户 Strict 配置/TUI 开关。
- 删除只为证明自身而存在的重复 actor/state 测试矩阵，保留 production owner、真实 actor
  conformance 与 OS 级 crash 反例。
- 准入结论冻结后删除 `ToolSurfaceDecision` 中两个可由 surface/reason 推导的冗余布尔值，
  以及零调用的 schema 包装函数；唯一诊断与 wire 行为不变。该收缩和 SQLite reopen 证明在
  `4e3536f1`，没有改写 frozen candidate、manifest 或 raw。
- 没有建立 schema transformer、第二 wire catalog、compatibility reader 或双写。

## 3. 冻结 actor catalog

正式 manifest 只覆盖六个默认可执行 actor catalog 和一个 terminal no-tools catalog，不外推
到任意自定义 `ToolPolicy` 子集。

| Actor | Catalog SHA-256 | 首个 blocker | Strict 候选实际 surface |
|---|---|---|---|
| `root_headless` | `b2fe1b3abfb72e4eebb28f2a3ced08225c4941861c0b9cb8f6fc7a4add6df9b3` | `agent $/required all_properties_required` | `standard_chat` |
| `root_interactive` | `90e9a19b10ad2997fec89ef96d2513ddc5b2682b2c8f13c88e3f4ff229200caa` | `agent $/required all_properties_required` | `standard_chat` |
| `coordinator` | `0b66bd94fcfd644782ba24a37bc642e28c73b18f986a5a73101c9d275da2f1e4` | `agent $/required all_properties_required` | `standard_chat` |
| `read_only_child` | `a9fdff5e75a6e1e833f2f7c6fb3bb84e1da0ffd1bcc2d2893cb975dbcee72cf9` | `agent $/required all_properties_required` | `standard_chat` |
| `read_only_depth_limit` | `9abb08ef262ae9851c61e7ba99e10bcb091b3d5a8ece8beb8bc669c022bc447b` | `file_search $/required all_properties_required` | `standard_chat` |
| `isolated_writer` | `0d10eda41159109d4e8cc70561c25dfc5685fa6bdc4653ef78c8e0c5ee6c61f5` | `apply_patch $/oneOf unsupported_keyword` | `standard_chat` |
| `terminal_empty` | `4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945` | 无工具 | `standard_chat_no_tools` |

阻断项是 production 语义，不是可以机械删除的装饰：DeepSeek Strict 无法用当前约束无损
表达 optional/nullable；`apply_patch.oneOf` 表达 `patch` 与 `changes` exactly-one 合同；把
缺省改为空数组、空字符串或 sentinel 不等价；删除 `minItems`、`minLength`、`default` 或
required 会弱化 Standard 工具合同。第二 wire schema 会制造第二 owner 并污染 A/B 归因。

因此六个可执行 actor 在 `strict_enabled=true` 时仍全部选择 Standard Chat。terminal
no-tools 没有 function call，不能成为 Strict treatment。

## 4. 准入与可复核身份

### 4.1 官方协议复核

2026-07-23 重新核对了 DeepSeek 官方文档：普通工具调用属于 Chat；Strict 仍是 Beta
能力，要求使用 Beta 入口并为请求中的全部 function 设置 `strict=true`，服务端会校验每份
JSON Schema；thinking 工具调用回合的 `reasoning_content` 必须在后续请求中完整回放；FIM
仍是独立的 Beta `/completions` surface。实现与本次准入判定继续以这些官方事实为准：

- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode)
- [FIM Completion (Beta)](https://api-docs.deepseek.com/guides/fim_completion/)

### 4.2 冻结身份

冻结文件：

- manifest：`eval/manifests/m7-b-strict-admission-v2.json`
  - SHA-256：`318dcf802603367cef5b9fc664483b83297ed43128bd665746bd25937fbdd4fb`
- Harness：`scripts/eval-m7b-strict-admission.py`
  - SHA-256：`b5f18cee787ac372e0dfda69a9d16c9df98f6558ea01704fa8dc0c018322d757`
- ignored preflight result：`eval/results/m7-b-strict-admission-v2.json`
  - mode：`0600`
  - SHA-256：`9f1780cdb556aec560ffedd753a1a2243f881dc072f9c325d729154c4e20a688`

Harness 在 release binary、credential 与官方 API 前核对 manifest、source owner hash、actor
目录、surface 和 offline production loopback。结果为：

```text
admitted=false
decision=inadmissible_no_surface_delta
product_metric_eligible=false
formal_ab.started=false
formal_ab.arms=0
maximum_reruns=0
official_api_requests=0
credential_read=false
local_loopback_only=true
binary_freeze_required=false
```

因为 control 与 treatment 都发送 Standard Chat 和相同完整目录，没有真实 surface delta，
继续构建 release binary、读取 `key.txt` 或运行 live A/B 都不能回答 Strict 收益问题，只会
产生不可归因费用。Harness 因此正确停止；没有补跑、重采样、拼接旧结果或猜测收益。

## 5. 离线证据

production candidate 通过：

- focused gate；
- `cargo fmt --all -- --check`；
- owning-crate 定向 test/check；
- workspace clippy `-D warnings`；
- 完整 workspace tests；
- production-path Standard/Strict candidate loopback；
- canonical TUI PTY、QA PTY 和 run-surface parity；
- root、read-only child、Writer recovery conformance；
- process-level tool crash/reopen；
- Harness self-test 5/5；
- final no-credential admission preflight。

完整 workspace test 在修复两份旧 thinking tool-call fixture 的 exact reasoning replay 后通过。
修复只补齐 fixture 中协议要求的 `reasoning_content`，没有放宽 production validator。

准入冻结后的收缩/重开证明 checkpoint `4e3536f1` 又重新通过 DeepSeek 与 app 定向测试、
`cargo fmt`、workspace Clippy `-D warnings`、完整 workspace tests、focused gate 和 Harness
self-test 5/5。新增的 production composition 测试覆盖 SQLite reopen 后 exact catalog/
RequestPlan 重建与 strict-policy fingerprint 分离；全程只使用本机回环 fixture，没有读取凭证
或调用官方 API。

## 6. 产品决策

### 保留

- 官方 Beta Strict planner 与 wire protocol；
- 整目录 deterministic compatibility diagnostics；
- 无损、原子的 Standard fallback；
- typed 工具失败恢复、exact reasoning/tool replay；
- root/read-only/Writer 与 crash/reopen conformance。

### 不准入

- Strict 作为当前默认 production path；
- 当前 checkpoint 的 Standard/Strict live A/B。

### 拒绝

- 为进入 Beta 弱化或删除工具 schema；
- 丢工具、改变工具顺序或语义；
- 第二 wire catalog 或 schema transformer；
- 用户 Strict 模式/开关；
- 用请求预算、提示词限制或自动重复执行掩盖失败。

### 非结论

本阶段没有证明 Strict 提高或降低 verified success、参数正确率、失败恢复率、模型轮次、
Token、时间或费用。没有真实 treatment surface 时，0 arms 是正确的停止结果，不是收益或
回归样本。

## 7. 下一独立切片

最大可行动缺口转向编辑策略。下一 Goal 应先测量 `apply_patch` 与 `edit_file` 的真实失败，
再决定是否建立最小 canonical FIM production caller，并执行同任务编辑 A/B。不得恢复已删除
的 `FimEditTool`，也不得把 FIM 与 Strict、Provider 清理、多 Writer或产品化混入同一切片。
