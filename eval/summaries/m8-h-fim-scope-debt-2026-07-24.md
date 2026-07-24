# M8-H FIM 产品范围与历史债收敛

> 复核日期：2026-07-24
> baseline revision：`dce858d0f8201e65bc01bcf79d400f20b121465d`
> code candidate：`7d9aa9a66f2df5b9f9bbadf15eb7844a532d1e74`
> candidate tree：`6c44194973f6f4b24d2119c544b83120da3ba7ff`
> 决策：`keep_standard_strict / reject_unreachable_fim_production_half_branch /
> hold_fim_reentry`

## 1. 真实问题与验收条件

M8-H 不以“必须接入 FIM”为目标。真实问题是：M7-C 以后，canonical
`apply_patch`/`edit_file` 是否出现新的 production 失败，且主要瓶颈是否可归因于编辑
生成；只有答案为是，并且存在可运行的同 binary FIM treatment，才允许读取 Key 或开展
live A/B。

单一 owner 必须保持：

- `crates/tools`：canonical deterministic 编辑、freshness、原子写入与 typed failure；
- `crates/deepseek`：官方 DeepSeek Chat planner/transport/parser/accounting；
- `AgentApplication -> AgentRuntime -> RunStore`：唯一 execution/reopen truth；
- Host：revision-bound verifier、terminal acceptance 与 Writer integrate/cleanup。

验收条件是先冻结调用图和 M7-C 基线；无 treatment 时删除只为未完成 FIM 表面存在的
production 历史债，不新增第二工具、Backend、Runtime、Store、模式或兼容 reader。

## 2. 冻结身份与事实

预注册 manifest：
`eval/manifests/m8-h-fim-scope-debt-v1.json`。

| 项目 | 结果 |
|---|---:|
| M7-C deterministic Host matrix | 12/12 |
| M7-C 后新的 current-v16 production 编辑失败样本 | 0 |
| canonical FIM caller | 0 |
| FIM full response parser | 0 |
| revision-bound Host apply lifecycle | 0 |
| SQLite reopen / retry-safe FIM lifecycle | 0 |

旧 `plan_fim` 能生成 `https://api.deepseek.com/beta/completions`，但 production sender
endpoint owner 只接受 Standard 与 Beta Strict Chat URL，所以该 RequestPlan 会在发送前
被拒绝。唯一完整 response parser 读取 `choices[0].message`，而官方 FIM
Completions response 使用 `choices[0].text`。Runtime/RunStore 也没有绑定 fresh read、
workspace revision、prefix/suffix digest、原子 Host apply 与 reopen 的 consumer。

因此旧代码不是隐藏的可运行 treatment，而是一条内部自相矛盾、无消费者的半分支。
live A/B 在 credential/API 之前判定
`inadmissible_no_surface_delta`；`key.txt` 未读取，官方 API 请求为 0。

## 3. 删除与保留

candidate `7d9aa9a6` 删除：

- `plan_fim`、`FimPlanError` 与 DeepSeek private/protocol FIM surface；
- FIM URL arm、usage bucket、accounting projection；
- 公共 terminal 永远为零的 `fim_response_count` 及 TUI/evaluator 投影；
- 当前 canonical M1 中无 caller 的 FIM planner row；
- evaluator 不在当前工具目录内的 `fim_edit`/`write_file` classifier；
- always-`None` 模型 alias deprecation 状态、Doctor warning 和重复 TUI alias switch。

config 现在是 V4 model id 的唯一规范化 owner，只接受
`deepseek-v4-pro`、`deepseek-v4-flash`、`auto` 及明确短写；
`deepseek-chat`、`deepseek-reasoner`、speculative `deepseek-*` 和外部模型 fail closed。

公共 terminal 字段删除使 exec-stream `v2 -> v3`，不保留兼容 reader。RuntimeEvent v16
与 State v21 不变：production sender 从未能产生 FIM `SurfaceUsage`，Standard/Strict
持久化形状也未改变。冻结的 M1/M7-C/M8-E manifest、result 和历史 summary 不改写；
它们保留审计价值，不是 production compatibility。

继续保留：

- 官方 `https://api.deepseek.com/chat/completions`；
- `deepseek-v4-pro` 与 `deepseek-v4-flash`；
- Standard Chat、thinking replay、lossless Beta Strict fallback；
- single canonical Runtime/RunStore/Backend/tool catalog；
- M7-C 12/12 deterministic 编辑与 process crash/reopen evidence；
- eval-only 官方协议 fixture 和冻结历史证据。

相对 baseline 的 code commit 为 26 files、`+185/-214`，净删除 29 行；新增 manifest
占主要插入，production 抽象和状态总量下降。

## 4. 官方协议复核

2026-07-24 复核的官方一手资料：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)：
  当前 Chat 模型为 `deepseek-v4-pro`、`deepseek-v4-flash`；
- [Tool Calls / Strict](https://api-docs.deepseek.com/guides/tool_calls/)：
  official base 为 `https://api.deepseek.com`，Strict 使用 Beta Chat 且整组 function
  schema 必须兼容；
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)：
  thinking tool call 后必须按协议回放 `reasoning_content`；
- [FIM Completion](https://api-docs.deepseek.com/guides/fim_completion/)：
  FIM 是独立 Beta Completions surface，模型为 `deepseek-v4-pro`，response 使用
  completion text，不是 Chat message；
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) 与
  [Change Log](https://api-docs.deepseek.com/updates/)：冻结当前模型、价格与退役事实，
  不把旧模型 alias 下线误写成 ChatCompletions surface 下线。

这些事实支持保留成熟 Chat production 链，也证明未来 FIM 不能复用 Chat parser 或被
包装成 Strict/Function Calling。

## 5. 门禁与 A/B 准入

离线通过：

- M7-C Harness self-test；
- M8-E frozen contract 8/8；
- DeepSeek eval Harness self-test 25/25；
- config、DeepSeek、TUI、localization 定向测试；
- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`，覆盖 tools 347、DeepSeek 55、Runtime
  conformance 82、App 56、app-server 23、exec 25、TUI Run 18、PTY 7；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`，包括 root/read-only/Writer、
  SQLite/SIGKILL reopen、exec 25、TUI Run 18、PTY 7 和 CLI/TUI/API parity；
- exact baseline/current release binary 的本机 Chat loopback 与 terminal accounting
  对照；
- `git diff --check`。

本切片没有同 binary FIM treatment，因此不读取 Key、不调用 API，也不产生伪 A/B
产品指标。imported `352e86a6` 的 coding/workflow-step 对照仅在 immutable binary、
同 Chat/model/budget、deterministic verifier 与完整逐 arm accounting 全部离线可证后
才准入；否则按预注册规则保存 inadmissibility，而不是修改旧 baseline。

### Imported baseline A/B preflight

M8-H 在 FIM 决策后进入下一项离线准入检查，没有直接消费凭证。exact imported
`352e86a611fdf3cd8bd27c36d24d482c06a71117` 的 locked/offline release binary 成功构建：

| binary | SHA-256 |
|---|---|
| `codewhale` | `23fabcc72fdf1a97cefeec4a4cd15ff589ec1e05d8c5bd4c090680ca51117708` |
| `codewhale-tui` | `e2507e489bad96ffe54f56fa2378abdc466bedfda8bc328aca6cec0d31947d9f` |

exact code candidate release identity：

| binary | SHA-256 |
|---|---|
| `codewhale` | `243e532e59d22e525716229c9fe3d3cb2bda524d8d15cb71a0cecee235598df1` |
| `codewhale-tui` | `417d6070d2ade306c3d753409186f2b9aff7c3b4396d004afc7a78da3da1d50a` |

本机回环证明 baseline 可通过显式
`providers.deepseek.path_suffix=/chat/completions` 使用与 current 相同的
`/chat/completions` 路径和 `deepseek-v4-pro`，并投影 input/output/cache token 与 binary
identity。但它的 terminal schema v1 将 `retry_count` 固定为 `null`，没有
`api_request_count`、started request ledger、`cost_complete`、price/cost bucket 或
canonical RunStore child aggregate。它仍由旧 Engine/SessionManager 执行，不能从 reopen
truth 证明每个 root/child 物理请求、重试和费用全部入账。

同一 fixture 下 current exec-stream v3 报告 `api_request_count=1`、
`usage_complete=true`、`cost_complete=true`、root/child started/completed/in-flight、
transport retry、surface/model usage bucket 与完整费用。这一不对称是 baseline 缺失
事实，不是 Harness 可以在不猜测的情况下补齐的字段。

因此 imported-baseline live coding/workflow-step A/B 在 credential 前判定
`inadmissible_incomplete_baseline_accounting`。不修改旧 revision 来补兼容字段，不用外部
估算冒充 baseline 自身账本；Key 未读取，官方 API 请求仍为 0。这个结果关闭本 Goal 的
fallback 准入检查，但不声称 current 已优于 imported baseline。

## 6. 产品取舍

**Keep**

- Standard Chat、thinking replay 与 lossless Strict fallback；
- canonical tools correctness 和唯一 Agent execution/store chain；
- 具有审计价值的 frozen evidence。

**Delete**

- 无 caller、sender、parser/apply/reopen 闭环的 FIM production 半分支；
- always-zero/always-`None` 状态、重复 alias owner 和过期 classifier。

**Hold**

- FIM 产品接入。它不是因为质量差而 hold，而是没有新的编辑生成失败证据和完整
  treatment surface。

**Re-entry gate**

只有同时出现新的 current production typed edit failure、失败主因可归于生成、
Host-owned fresh read/revision/digest/atomic apply/reopen 候选、同任务离线身份与
accounting 证据时，才允许重新实现官方 Beta FIM 垂直路径并开展 live A/B。

## 7. 非结论与下一切片

M8-H 不证明：

- FIM 优于或劣于 `apply_patch`/`edit_file`；
- 12/12 deterministic Host matrix 等于真实模型编辑成功率提高；
- CodeWhale 已优于 imported baseline；
- V09、RepoGraph、multi-Writer、中文 prompt billing 或 V1 release 已关闭。

imported `352e86a6` 的 A/B 已因 baseline accounting 不完整在 credential 前 fail
closed。后续不得修改旧 revision 制造兼容性；应选择不依赖伪配对、能够由 current
RunStore 完整归因的最小产品缺口。
