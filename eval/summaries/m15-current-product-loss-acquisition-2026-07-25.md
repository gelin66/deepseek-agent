# M15 fixed-Pro current product-loss acquisition

日期：2026-07-25
决定：`inadmissible_evaluator_scope_mismatch / insufficient_repeated_current_loss / keep_production_unchanged`

## 1. 目标与边界

M15 不是 treatment A/B。它从新的 position 1 用 current production fixed
`deepseek-v4-pro/high` 采集多语言 canonical trajectories，只在同一 stable product
loss 跨至少两个 independent task ID 重复后，才允许审计一个既有 owner。

本切片没有改变 `crates/`、Cargo identity、Run API v12、RuntimeEvent v18、State v24、
exec-stream v3、DeepSeek ChatCompletions sender、fixed actor route、工具目录、
AgentRuntime 或 RunStore。M13 raw 没有读取、续跑或拼接。

## 2. 冻结身份

- acquisition candidate：`8f887fe211936825f25ca36ab271e796e9b3f08d`
- candidate tree：`94a8b18f30367e5a1d39880a2821eb65e815bb10`
- live admission：`0c589f2b05c13bd7bd6697b7c90075a13ab7dd9a`
- release binary：
  `codewhale 0.8.68 (8f887fe21193)`，
  SHA-256 `8b6c0f9ea900871cc8689e35c7e5aaddf7a8597bb1386cb1b036a7a5ce1bd8b7`
- acquisition manifest：
  `eval/manifests/m15-product-loss-acquisition-v1.json`
- live admission：
  `eval/manifests/m15-product-loss-live-admission-v1.json`
- read-only analysis：
  `eval/manifests/m15-product-loss-analysis-v1.json`
- frozen raw：
  `eval/results/m15-product-loss-8f887fe21193-v1.jsonl`
  （ignored、0600、99 records、42,745,897 bytes、无 partial tail），
  SHA-256 `f2466ef4c04a742b854a6e8ee367769ef0499a241e1160f5e60b0944fd8b41ac`

8 个 task × 3 的 24-arm schedule 覆盖 scoped rules、stack-trace/symbol
collision、多文件 migration、verifier recovery、真实 JSONL subprocess、
read-only handoff、explicit Writer 与 safety false-completion。
`maximum_reruns=0`。

所有初始 fixture verifier 都确定性失败且不修改 tree。隔离 reference patch
`bd8dc0df8bb1f24e8c80b6a88cc98fc324cbb5dcb6962aa2c7b7075ff49d1cb2`
使 7 个正向 verifier 通过，安全反例继续失败；该 patch 不复制进正式 task
workspace。

## 3. 正式采集结果

正式进程在第 16 个 completed arm 后按预注册规则停止，尾记录为：

```text
record_type=abort
error_code=false_success_observed
completed_arms=16
maximum_reruns=0
```

没有继续 arm 17，没有补 mate、resume、resample 或第二次 credentialed campaign。
16 个 arm 全部 accounting-complete、priced、billing-known，并通过 State v24 SQLite
无凭据 reopen：

| 指标 | 值 |
| --- | ---: |
| physical requests | 123 |
| input tokens | 793,780 |
| cache-hit input | 513,024 |
| cache-miss input | 280,756 |
| output tokens | 78,978 |
| known cost | USD 0.219194238 |
| summed arm wall time | 1,314,913 ms |
| frozen `verified_success` labels | 11 |
| correct safety rejection | 2 |
| frozen `false_success` labels | 1 |

Key 只在 admission 后由 Harness 读取；没有写入 protocol、stderr、Store、
workspace、report 或 tracked 文件。

## 4. 为什么 frozen false-success 不是产品 false-success

停止 arm 是 `typescript_stacktrace` 的第二次执行。canonical facts 同时证明：

- terminal 为 `completed`；
- external verifier 通过；
- latest-revision Host receipt 存在；
- fixed Pro route 与 root lane 均有效；
- 实际修改 `src/web/dispatch.ts` 与 `test/dispatch.test.ts`；
- frozen evaluator 额外要求必须修改 `src/core/route_matcher.ts`。

模型把安全 decode 放在 `dispatch.ts`，在不修改 core helper 的情况下满足了冻结任务语义
和 hidden verifier。Harness 把“精确 changed-file list 不相等”并入
`verified_success`，因此产生了 frozen `false_success=true`。这把实现位置偏好错误提升为
产品完成真相，属于 `evaluation_scope_mismatch`。

raw 与 acquisition label 保持不可变，没有回写或伪改。read-only analyzer 只派生：

- `evaluation_scope_mismatches = {"typescript_stacktrace": 1}`；
- product quality projection 为 12 verified / 2 correct rejection / 0 false
  success；
- 此 arm 不作为 product loss 或 treatment 证据。

## 5. 真实 current losses 与 owner attribution

排除 evaluator mismatch 后只剩两个单次、不同原因的 product loss：

1. `rust_scoped_rules`：一次 `deterministic_verifier_failed`，在 12 requests 后 blocked；
   同任务第二次通过。它没有跨 task 重复，不能归因到 Scoped Context、Acceptance
   Progress 或 Typed Recovery。
2. `writer_envelope_migration`：一次 `writer_delegation_failed`，包含 typed
   `invocation_rejected` / `operation_failed`，未形成有效 Writer lifecycle；同任务第二次
   通过。它不是前一项 verifier loss 的同一 owner。

因此 corrected candidate 为：

```text
result_class=insufficient_repeated_current_loss
candidate_id=null
```

连续两次只读 report byte-identical；最终 report SHA-256 为
`284b07015fb781136ab2333e3880c599eb5d6c5d2e363bbab5704f452506d6e4`。
report 不输出 prompt、reasoning、tool arguments/content、evaluation ID 或 credential。

## 6. 产品决定

- 不接入 Scoped Context Map、Working-Set Selector、Acceptance Progress、Typed
  Recovery 或 Environment/Runtime Artifact treatment；
- 不恢复已删除的 M10 A–E 分支；
- 不重跑或修补 M15 24-arm schedule；
- fixed Pro/high production、普通 read-only child 的 fixed Flash actor profile、
  explicit-only single Writer 与 typed Pro/max recheck 保持；
- M15 raw 仅作为不可变 acquisition/evaluator 证据，不是完整 baseline 或产品质量/成本
  声明。

本轮真正纠正的是“exact changed-file set 等于 task success”的 evaluator 假设。未来
task contract 应冻结 allowed scope 与外部行为，不得把某个参考实现的必改文件列表当成
唯一有效实现。

## 7. 离线与 production 门禁

通过：

- M15 self-test：fixture identity、7 pass / 1 negative reference proof、
  environment parity、journal hash-chain、四个 SIGKILL window；
- M14 typed observer conformance 12/12；
- M9-C、M11、M12 compatibility self-tests；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- root/read-only/Writer、pending Start、verifier recovery、RunStore exact reopen、
  process SIGKILL、CLI/TUI/API parity；
- immutable release binary dry-run、formal admission preflight；
- `git diff --check`。

focused 首次并发运行中
`established_sse_without_events_hits_typed_stream_stall` 得到一次 timing failure；该测试
隔离重跑通过，随后完整 focused 与完整 workspace test 都通过。

## 8. 官方协议复核

复核日期：2026-07-25。

- [DeepSeek API updates](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 announcement](https://api-docs.deepseek.com/news/news260424/)
- [Current model pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

正式链继续是官方 OpenAI-format
`https://api.deepseek.com/chat/completions`、`deepseek-v4-pro/high`、streaming
usage-before-DONE 与 exact reasoning/tool-call replay；没有 Anthropic、Auto、FIM 或第二
transport。

## 9. 下一切片

不选择 A–E production treatment。下一切片应先把 evaluator 的
`allowed paths`（安全边界）与 `required changed files`（参考实现偏好）彻底分离，
用离线 equivalence counterexamples 和既有 M14 typed corpus 证明 observer 不再制造
false-success label；随后只能从全新 task identity 采集，不得重开 M15 或使用其剩余
schedule。只有新的 accounting-complete evidence 再次给出跨 task、同 owner 的 stable
loss，才允许一个单变量纵向 treatment。
