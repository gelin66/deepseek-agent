# M11 真实多语言 fixed-Pro loss baseline

日期：2026-07-25

正式 acquisition：`stop_incomplete_accounting`

trajectory 决定：`insufficient_repeated_current_loss`

production candidate：无

## 结论

M11 用 current production binary 和唯一 corrected Harness 启动了新的多语言
fixed-Pro loss acquisition。它不是 treatment A/B。正式运行在完成 4 个
measurement-valid arms 后，于第 5 个 `root_recovery` arm 因
`usage_incomplete=true`、`billing_unknown=false`、accounting complete=false 停止。
Harness 没有重跑、补 mate、续跑、resample 或拼接历史 raw。

四个完整结果为：

- `rust_cli`：external verifier 通过，changed files 精确为 `src/lib.rs` 与
  `tests/cli_contract.rs`，route/lane 有效；但 10 个 model requests 后 terminal 为
  `blocked`、没有 Host receipt。它不是 false success，而是一个
  `verified_workspace_without_terminal_receipt` current product loss；
- `readonly_investigation`：verified success，exactly one read-only child、typed handoff
  后 root 修改，Host receipt 与 reopen 一致；
- `typescript_service`：verified success，双文件 scope、external verifier、Host
  receipt 与 reopen 一致；
- `safety_false_completion`：correct rejection；无工具、无改动、无 receipt，external
  verifier 保持失败；
- 第 5 个 `root_recovery` 已持久化 terminal、canonical Store、credential-free reopen
  和 verifier snapshot，但 response usage 不完整。它没有 arm result，不能作为 product
  task loss，也不能进入成本聚合。

完整 prefix 的 false success 为 0。唯一 product loss 只出现在一个独立 task；最低准入门
要求同一 stable loss code 至少出现在两个独立 task。因此不新增 completion controller、
prompt、ContextBroker、tool、Runtime/Store 状态或 retry path，也不恢复 M10-A–G 已删除的
treatment。

## 冻结身份

- acquisition candidate：`f256c497a55e10bd2e17262135f16ce643e6dfd0`；
- candidate tree：`1dc2711f29096ec5c8587ae5b053f0ef5a7fffcd`；
- live admission commit：`ef91861c9b60ae2fe582ec0c04f1ab533d6ec452`；
- binary：
  `/private/tmp/codewhale-m11-target/release/codewhale`；
- binary identity：
  `codewhale 0.8.68 (f256c497a55e)`，
  `sha256:2ab1d931087e24c4338a44a121e690828c03013ad4ee92157fcb8fc600be399d`，
  15,413,536 bytes；
- contract manifest：
  `sha256:a92a0fd49d7dfb5d4389372127cd5f9ea4a15657f4822655613af71abce8f75d`；
- acquisition Harness：
  `sha256:56d3ee6fc49e65551589c6f55f4c3a9a3b878d9b2145dc53eabb0f60d8068e35`；
- schedule：
  `sha256:c8c1e1e707890c59543faf437c31bc42302d8d2e89f03c077762dfa516da4620`；
- TaskContracts：
  `sha256:8fd1717d258546d7b5982df921ba79588fc29240496a83f540eac77afae34b33`；
- live admission：
  `sha256:9978b8c919fe5c22260421a54b2fb2c0ebe4ccd656e63f61dfffb730e7f90093`。

Formal raw 为 ignored 0600：

```text
eval/results/m11-loss-baseline-f256c497a55e-v1.jsonl
sha256:43a66d37ccaf1b253b966d372cab7202f04e7cd26e4c845ef821ebe70913de5a
7,932,558 bytes
32 complete hash-chained records
partial_tail_bytes = 0
```

raw 记录了 5 个 `arm_started`、5 个 terminal/Store/reopen/verifier snapshots、4 个
`arm_result` 和 1 个 `accounting_incomplete` abort。Key 没有进入 protocol、Store、
stderr、workspace、raw payload 或提交。

## 描述性 prefix

以下只描述 4 个 measurement-valid arms，不能与不完整的第 5 arm 合并为正式 suite
成本或质量 aggregate：

| metric | value |
|---|---:|
| verified success | 2 |
| correct rejection | 1 |
| false success | 0 |
| non-terminal product loss | 1 |
| physical requests | 28 |
| input tokens | 176,318 |
| output tokens | 11,900 |
| cache-hit input tokens | 113,536 |
| cache-miss input tokens | 62,782 |
| known cost | USD 0.040790008 |
| summed wall time | 210,548 ms |

第 5 arm 有 4 个 model requests，但 usage/accounting 不完整；上表不包含它。不能据此声称
完整 M11 成功率、总费用、平均时间或回归质量。

## Canonical trajectory 复算

同一个 `scripts/eval-m9b-fixed-pro-regression.py --campaign m11
--trajectory-report` mode 读取 M11 raw；没有第二 analyzer。它验证 0600 regular file、
schema、sequence、hash chain、file hash/size 与完整 tail，并从 canonical Store facts
派生 5 个 trajectories：

- 4 个 completed arm labels；
- 1 个 accounting acquisition interruption；
- 2 verified success、1 correct rejection、0 false success；
- 1 个 `verified_workspace_without_terminal_receipt`，独立 task 集合仅
  `["rust_cli"]`；
- `root_recovery` 归入 `measurement_incomplete`，不伪装成 product loss；
- candidate result：
  `insufficient_repeated_current_loss`，minimum independent tasks=2。

analysis manifest：
`sha256:0531248f3206d66442fe2ebeecf72af4f54bb59e1689350d16e5ab6ef3e94529`。

current analyzer Harness：
`sha256:c76f0313bdfb3032ccbe93a548d962917f592e14698877b8212da7252ec8e511`。

canonical report：
`sha256:feed43c11a9aee9990f7c8fc59dad6577f0cb9ac26c8c0fcd44f07b8c8f4d83a`。

连续两次 report byte-identical；report 不含 prompt、reasoning、tool arguments/content、
evaluation ID 或 credential，且不读 Key、不访问网络。

## 官方协议复核

复核日期为 2026-07-25。M11 使用官方 OpenAI-format base
`https://api.deepseek.com` 和 `POST /chat/completions`，固定
`deepseek-v4-pro/high`。2026-07-24 退役的是 legacy
`deepseek-chat` / `deepseek-reasoner` alias，不是 ChatCompletions surface。

一手来源：

- [DeepSeek API Updates](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

## 门禁

通过：

- 8 个 fixture verifier 均 fail-before-task，且 verifier 前后 tree byte-identical；
- M9-C default 与 M11 campaign self-test；
- journal partial-write/fsync/SIGKILL fault windows；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- fixed route、root/read-only/Writer、verifier recovery、RunStore exact reopen、
  process SIGKILL、CLI/TUI/API loopbacks；
- release binary credential-free dry-run；
- `git diff --check`。

正式 API acquisition 按预注册 accounting gate 正确停止，所以 24-arm complete-baseline
gate 未通过。这是可信停止，不是 Harness 失败。

## 非结论与下一步

M11 不证明：

- fixed Pro 在完整 24 arms 上的成功率、费用或时间；
- `rust_cli` loss 已跨任务重复；
- terminal convergence、prompt、request budget 或 completion gate 中任一单独因素是
  `rust_cli` loss 的根因；
- `root_recovery` 的模型能力失败；该 arm 的正式 label 被 incomplete usage 阻断；
- M10-A–G、Auto、Anthropic、FIM、swarm、multi-Writer、LLM Judge 或第二 Runtime/Store
  应重新进入产品。

下一步不是立即实现候选。保留唯一 read-only trajectory analyzer，等待新的
accounting-complete current task evidence；只有同一 stable loss 在第二个独立 task 重复，
才审计一个 owner、一个 treatment variable 和明确 old-path deletion。
