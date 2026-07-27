# M35 官方 DeepSeek production reliability soak 结论

日期：2026-07-27
产品决定：`keep_current_retry_no_new_treatment`
正式 acquisition：`no_repeated_live_reliability_loss`

## 决策

保留 M33/M34 的现有生产行为，不新增 retry、jitter、circuit breaker、fallback、设置项或
第二控制器：

- DeepSeek transport 只做一次 physical attempt 和 typed 分类；
- `AgentRuntime` 仍是唯一 retry owner，正常上限为初次请求加最多两次安全重试；
- 只有 transient、replay-safe、无 actionable output 且预算允许的失败才重发；
- partial content/reasoning/tool/usage 或 in-flight crash 继续 fail closed；
- CLI/TUI/app-server 只投影同一 stored failure/retry/stop fact；
- app-server sequence reconnect、same-Run resume 与 upstream model retry 继续是三个
  独立概念，不把 SSE 断流包装成 token stream 续传。

24 个独立官方 Run 没有出现模型故障、Runtime retry、partial response、429 或 5xx，
因此不存在跨 profile/round 重复的 live loss。这个结果不授权调整次数或退避，也不把
“没有观察到故障”外推成官方服务永不失败。M33/M34 的 deterministic fault/reopen
matrix 仍是 replay safety 的主证据。

## 可复现身份

- 起点：`39da745f5dbd3e27fd4e2ab4a6159dfd0cfb20ca`
- contract：`9a47349ad`（`docs(m35): freeze official reliability soak contract`）
- corrected candidate：`6202bb0d70058af9a0fa535413694e2b1a5a4ffe`
- candidate tree：`6bf011e53c8f3475964ed74fe9beaf51cd990090`
- contract manifest：
  `eval/manifests/m35-official-reliability-soak-v1.json`
- live admission：
  `eval/manifests/m35-official-reliability-soak-live-admission-v2.json`
- admission SHA-256：
  `e450911aea22c4185998b51b2b394757abae16cc434f429aa0ba32a51e2d8aeb`
- `dse`：
  `sha256:e379e97235aa5cf4191f3e2ce402f15e68559a73832c9712c13bb24d3517bb01`
- `dse-tui`：
  `sha256:33b80d1ff4d17fae4c282d5e5e8e440abeb03fd83e8348511f9990b5563e0fb3`
- binary pair：
  `sha256:9f495c571b7a3abfe787dac1de9af0c14aa71b19e3f31a731f7a7d623d41bcf0`
- frozen raw：
  `eval/results/m35-official-reliability-soak-6202bb0d7005-v2.jsonl`
- raw SHA-256：
  `sha256:132138813afdaf4b2670e1ed411a1f6b709c01f94652591769ba31e76d536f64`
- raw mode/size：`0600` / `1,311,104` bytes
- journal chain：27 records，sequence/previous/hash 全部验证通过
- model/surface：`deepseek-v4-pro`、`high`、
  `https://api.deepseek.com/chat/completions`
- normal Runtime retry：2；Harness logical rerun：0
- credential：只从 ignored `0600` test-key file 读取；raw/Store/workspace 无 credential
- GitHub/push/release：0

## Observer 反馈与修正

第一次 formal v1 在任何模型请求前停止。临时 Harness 把全局 `--model` 放在 `exec`
之后，DSE CLI 正确 fail-fast；journal 只有 plan、credential access 和
`m35_stream_terminal_invalid` 三条记录，completed Run=0、official request=0。该
`0600` journal 不重写、不续跑：

- raw：
  `eval/results/m35-official-reliability-soak-ad14f0da7c71-v1.jsonl`
- SHA-256：
  `sha256:e2cf735713df896c755fe75d89a4eae9ec2d8f8b87eac8923a9df670fe064561`
- mode/size：`0600` / `2,256` bytes
- hash chain：3/3 valid

修正切片 `6202bb0d7` 做了两件事：

1. 让 Harness 构造真实 production 命令形状，并在 credential-free self-test 中冻结
   `--model` 必须位于 `exec` 前；
2. 无 stdout stream 时保存 `returncode/wall/output hashes` 并使用
   `m35_exec_no_stream`，不再把参数/启动失败误报成 SSE terminal 缺失。

修正后先运行一个不计入正式 24-Run aggregate 的官方 canary：marker 正确，1 个
physical attempt、0 retry、usage/cost complete、billing unknown 0，费用
`$0.000340315`。随后从新 revision、新 immutable binary、position 1 和新 v2 raw 开始
完整正式 acquisition；没有补跑或拼接 v1。

## 正式结果

| profile | Runs | verified | physical attempts | median TTFR | median wall | known cost |
|---|---:|---:|---:|---:|---:|---:|
| plain Chat marker | 6 | 6 | 6 | 1,507 ms | 1,733.5 ms | $0.001174529 |
| `read_file` | 6 | 6 | 12 | 1,774.5 ms | 3,521 ms | $0.002859980 |
| `grep_files -> read_file` | 6 | 6 | 18 | 2,173.5 ms | 6,292.5 ms | $0.005123372 |
| `read_file -> edit_file` | 6 | 6 | 18 | 2,243 ms | 6,217 ms | $0.004913528 |
| **total** | **24** | **24** | **54** | **2,020 ms** | **4,796 ms** | **$0.014071409** |

完整 aggregate：

```text
verified success          24/24
false success              0
model failure events       0
Runtime retries            0
physical started          54
physical completed        54
physical in-flight         0
input tokens          106,121
cache-hit tokens       82,688
cache-miss tokens      23,433
output tokens           4,113
reasoning tokens        2,199
TTFR min/median/p95/max   1,174 / 2,020 / 2,380 / 2,380 ms
wall min/median/p95/max   1,424 / 4,796 / 7,428 / 7,721 ms
known cost             $0.014071409
usage/cost complete        24/24
billing unknown             0
SQLite quick-check          24/24
credential-free reopen      24/24 exact
replayed stored events       1,185
```

工具任务中的 2–3 个 physical attempts 是正常的 tool-call → ToolOutcome → next model
turn，不是网络重试。Harness 分别记录 physical request 和 `runtime_retry_count`，避免用
总请求数虚构重连率。

## 顶级 Agent 与可靠性依据

2026-07-27 重新复核：

- DeepSeek 官方 Standard Chat 流以 `[DONE]` 结束，`include_usage` 在结束前给出 usage；
  streaming 可能收到 `: keep-alive`，推理十分钟未开始时服务端会关闭连接。官方明确
  429/500/503 可短暂等待后重试，400/401/402/422 应先修正请求、凭据或余额；没有
  partial continuation 或逐物理请求账单追溯承诺：
  <https://api-docs.deepseek.com/api/create-chat-completion>、
  <https://api-docs.deepseek.com/quick_start/rate_limit/>、
  <https://api-docs.deepseek.com/quick_start/error_codes/>。
- AWS Builders Library 要求在单一层做有界 retry/backoff，并把 timeout 与“请求一定未
  执行”分开；有 side effect 的操作没有幂等合同就不能盲发：
  <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/>、
  <https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/>。
- Google 的 Agent/API retry guidance 同样把 transient response 与 idempotency 分成两个
  gate，并要求记录 attempt、delay 和 final outcome：
  <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/retry-strategy>。
- Codex 的公开 Rust client 把通用 HTTP/SSE transport、request policy 和上层 API 语义
  分层；app-server overload/reconnect 也是客户端控制面策略，不等于模型 stream
  continuation：
  <https://github.com/openai/codex/blob/main/codex-rs/codex-client/README.md>、
  <https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md>。
- Claude Code 的 `--resume` / `--continue` 是保存会话恢复，不是上游 token stream
  断点续传：
  <https://docs.anthropic.com/en/docs/claude-code/cli-usage>。
- Codex 公开故障反馈中，含糊的多次 “Reconnecting” 曾掩盖长时间等待或不可恢复错误；
  DSE 因而保留 M34 的 typed category、ordinal/total、wait 与 stop reason，而不是复制
  一个无语义 spinner：
  <https://github.com/openai/codex/issues/23015>、
  <https://github.com/openai/codex/issues/24419>。

DSE 吸收的是控制论机制，不复制 Provider、WebSocket fallback、五次 reconnect、随机
jitter 或第二 transport。当前单进程顺序 workload 没有 concurrent-herd loss，正式 live
数据也没有 transient failure，所以继续保留可重放的 deterministic 1s/2s backoff。

## Cutover 与删除

- production `crates/`、protocol、State、config、CLI/TUI/app-server 均无 M35 delta；
- temporary M35 runner、parser、schedule、admission loader 和 CLI flags 已从唯一
  corrected Harness 物理删除；
- Harness 恢复到 M35 前 exact Git blob
  `5f3f613cd68d57d14852cfbb51504088b24ad456`；
- 不保留 compatibility reader、第二 evaluator、retry setting、fallback 或隐藏模式；
- contract manifest、live admission、ignored `0600` raw、summary 与 Git 历史保留审计。

## 门禁

最终切断临时 consumer 后已通过：

- corrected Harness self-test、Python compile 和 exact pre-M35 blob identity；
- `cargo fmt --all -- --check`；
- `./scripts/dev-dse.sh focused`；
- DeepSeek 60/60（1 ignored）、Runtime conformance 88/88；
- app 57/57（1 ignored）、app-server 23/23；
- exec process acceptance 30/30；
- canonical TUI run 18/18、双语 PTY 7/7、QA PTY 16/16；
- app-server retry-decision SIGKILL/reopen 与 State 39/39 process crash tests；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- public repository checker；
- `git diff --check`。

## 非结论

M35 不证明官方 DeepSeek 没有 timeout/429/5xx，也没有真实观察到一次可计算的 live
recovery rate；它只证明 2026-07-27 的 24 个独立 production Runs 全部成功，当前
latency/accounting/reopen 闭合，且没有重复损失授权新 treatment。不能把 24/24 marker
任务外推为广泛编码能力，也不能把没有触发 retry 解释成 retry 功能无用。真实故障发生
时仍应从 canonical RunStore trajectory 归因；只有同一 stable owner/cause 跨独立任务
重复，才重开一个最小候选。
