# M34 模型故障反馈与恢复体验结论

日期：2026-07-27
结论：`keep_minimal_model_failure_feedback`

## 决策

M33 的单一 Runtime-owned retry、安全重放、1s/2s durable backoff、
`Retry-After`、accounting 和 crash/reopen 语义保持不变。M34 只保留一个客户端
投影切片：

- plain `dse exec` 把 transient retry progress 写入 stderr，stdout 仍只承载模型输出；
- TUI 从同一个 stored `ModelRequestFailed` 显示双语 category、ordinal/total 和 wait，
  窄终端仍保留 typed warning，成功后回到正常状态；
- 最终 stop reason 与下一既有操作写入 TUI history，不再被 terminal 状态覆盖；
- exec-stream v6 增加 bounded `model_request_failed` 事件，携带 event/attempt identity、
  failure、retry/stop decision 与紧凑 accounting，但不复制完整 request、system prompt、
  transcript 或 tool catalog；
- app-server 继续原样投影 canonical stored event，没有第二 retry truth、Store 或控制器。

## 可复现身份

- control：M33 clean checkpoint `408611fb1928b76dcf657288bc7df8deae24cf77`
- contract：`cbcd8ec0d`（`docs(m34): freeze model failure feedback contract`）
- manifest：
  `eval/manifests/m34-model-failure-feedback-v1.json`
- fixture：
  `eval/fixtures/m34-model-failure-feedback-v1.json`
- Cargo target：`/private/tmp/dse-m34-target`
- `maximum_reruns=0`
- credential read：false
- official DeepSeek API request：0

没有提交 raw terminal、SQLite、binary、target、credential 或本地运行状态。

## 第一手依据与取舍

2026-07-27 复核：

- DeepSeek 官方把 429、500、503 作为 transient failure，把 400、401、422 作为应先
  修正请求或凭据的错误；Chat SSE 以 `[DONE]` 闭合，但没有 partial stream continuation
  合同：
  <https://api-docs.deepseek.com/quick_start/error_codes/>、
  <https://api-docs.deepseek.com/quick_start/rate_limit/>、
  <https://api-docs.deepseek.com/api/create-chat-completion>。
- AWS 与 Google 的重试设计要求单一 bounded retry layer、backoff、幂等/replay safety
  gate，以及 error/attempt/delay/final outcome 可观察：
  <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/>、
  <https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/>、
  <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/retry-strategy>。
- Codex 把进度与最终输出分离，saved-session resume 不等于 transport retry；Claude
  Code 的 `--resume` / `--continue` 同样是会话恢复：
  <https://developers.openai.com/codex/codex-manual.md>、
  <https://docs.anthropic.com/en/docs/claude-code/cli-usage>。

DSE 只吸收这些控制机制。当前是一个本地 Runtime，没有测得并发惊群，所以不凭通用云端
建议把 M33 的可重放确定性 1s/2s 改成 jitter。也不新增设置页、modal、Provider 或伪
partial SSE continuation。

## Control 失败与两轮反馈

真实 production binary + loopback control 重复暴露同一信息损失：

1. 429 后 Runtime 正确等待 2 秒并恢复，server physical attempts 为 2，但 plain exec
   stderr 只有最终 usage；
2. 连续 503 的 exec-stream accounting 正确为 physical=3、runtime retry=2，但逐次
   failure/retry decision 不可见；
3. partial content 后连接关闭只发送 1 次并正确 fail closed，但 plain exec 只有泛化
   stream incomplete；
4. TUI terminal 会用最终 phase 覆盖 stop reason；
5. 首个 140 列 treatment 把完整 raw failure 放入 status，ordinal/wait 位于尾部而被
   截断；改为紧凑 category/ordinal/wait 后，12×48 compact tier 又隐藏普通 toast；
   最终只让 typed Warning/Error 在 compact tier 可见，普通 Info 仍隐藏；
6. response-header timeout 的第一次 treatment 正确恢复，但英文 CLI 混入 DeepSeek
   层中文自由文本；最终 transient progress 只使用 localization owner 的稳定 category，
   raw diagnostic 仅在终止时保留。

这些缺失跨 429、503、partial SSE、timeout 和 TUI terminal 多个独立 profile 重复，
满足预注册 treatment admission；没有为了单个截图扩大 Runtime 或协议状态。

## 冻结故障矩阵结果

| profile | production observation | 结果 |
|---|---|---|
| response-header timeout → success | 45s header deadline、1s backoff、2 physical | 英文显示 provider/category/1 of 2/wait；成功 |
| reset → reset → success | 1s/2s、3 physical、2 retry | 两个 machine failure event；成功 |
| 429 + `Retry-After: 2` → success | 等待不少于 2s、2 physical | en/zh-Hans PTY 与中文 exec 显示限流/1 of 2/2s；成功 |
| 503 exhausted | 3 physical、2 retry、Stop | stop reason 与下一操作在 terminal 后保留 |
| 401 | 1 physical、0 retry | authentication + `not_retryable` |
| partial content close | 1 physical、0 retry | stdout 保留已收 content；说明 actionable output，禁止盲发 |
| prepared retry crash/reopen | decision 后 SIGKILL | 等剩余 not-before，只发送一次 |
| in-flight crash/reopen | attempt 已发送后 SIGKILL | `RecoveryRequired`，零盲发 |

适用 information rubric 为 100%；server attempts、Runtime physical accounting、
runtime retry 和 terminal metadata 一致；partial/in-flight duplicate request=0，
false progress=0，false success=0。root、普通 read-only child 与 Writer 继续共用 M33
同一 conformance。

## Cutover 与删除

- 删除 plain exec 对 `ModelRequestFailed` 的静默分支；
- 删除 presenter 内重复的 failure/category/retry formatter，由单一 human projection
  模块服务 CLI 与 TUI；
- transient status 替代会把 raw error、完整 prompt 或 tool catalog 扩散到状态面的
  旧做法；
- exec-stream 的第一次候选曾直接嵌入完整 stored event，实测发现会重复 system prompt、
  messages 与 tool schema；该候选在接管前删除，最终 bounded event 每条小于 5KB；
- test-only proxy 只产生 wire fault 和计数，不复制 retry classification、ledger 或 reducer。

没有新增或保留 transport retry loop、retry config、compatibility reader、第二
Runtime/Store、partial continuation 或用户模式。

## 门禁

已通过：

- frozen controlled fault profiles；
- `exec_terminal_acceptance`：30/30；
- 双语/窄终端真实 PTY：16/16；
- Runtime retry/reopen conformance；
- app-server prepared-retry SIGKILL/reopen；
- State in-flight crash/reopen；
- `./scripts/dev-dse.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- public repository checker；
- `git diff --check`。

## 非结论

M34 不证明官方 DeepSeek 故障频率、真实 429/503 的服务端行为、随机 jitter 收益、模型质量
或远端 CI。它没有读取测试 Key，也没有请求官方 API；controlled loopback 才能精确冻结
故障与 physical attempt。M34 也不声称 partial stream 可以续传：app-server sequence
reconnect、same-Run resume 与 upstream request retry 仍是三个独立概念。
