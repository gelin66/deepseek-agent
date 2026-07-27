# M33 Runtime-owned model retry

- 日期：2026-07-27
- control：M32 clean checkpoint `e5df72e78`
- contract：`5bcaa7e45726ce06b2d3e4c63b4662163f206c68`
- 决策：`keep_runtime_owned_model_retry_loop`
- 唯一 retry owner：`crates/runtime::AgentRuntime`
- typed transport owner：`crates/deepseek`
- durable truth：canonical `RuntimeEvent` / `RunStore`

## 问题、边界与旧路径

M32 的 `maximum_reruns=0` 是正式 acquisition 合同，不是正常产品策略。正常
`RunLimits.max_model_retries=2` 已允许初次请求后最多两次安全重试，但 production 同时
存在两套漂移的控制面：DeepSeek transport 保留默认 3 次 retry 配置和 loop，
`DeepSeekModelPort` 又强制关闭它；真正起作用的 Runtime retry 在 durable decision 后
立即重发，没有可重开 not-before；CLI/config/fingerprint 仍公开无效 transport knobs。

M33 只允许一个控制器：

```text
DeepSeek typed failure + evidence + usage + optional Retry-After
  -> AgentRuntime replay-safety / actionable-output / budget / limit gate
  -> failed attempt + prepared retry + backoff/not-before atomic event
  -> RunStore exact replay
  -> remaining wait
  -> exactly one next physical request
```

DeepSeek transport 不作业务重试、不 sleep，也不判断 Host completion。app-server
sequence reconnect 只重放本地 event；same-Run resume 只恢复 canonical run；二者都不是
上游 SSE 断点续传。

## 外部研究与 DSE 取舍

复核日期：2026-07-27。

- DeepSeek 官方 rate-limit/error 文档把 429、500、503 作为可短暂等待后重试的
  transient failure；Chat 文档定义 data-only SSE、最终 usage chunk 与 `[DONE]`，
  但没有 event id、`Last-Event-ID` 或 partial stream continuation 合同：
  <https://api-docs.deepseek.com/quick_start/rate_limit/>、
  <https://api-docs.deepseek.com/quick_start/error_codes/>、
  <https://api-docs.deepseek.com/api/create-chat-completion>
- HTTP `Retry-After` 同时允许 delay-seconds 与 HTTP-date；实现两种格式：
  <https://www.rfc-editor.org/rfc/rfc9110.html>
- WHATWG SSE 的可恢复语义依赖 server event id 与 `Last-Event-ID`。DeepSeek 当前
  public contract 没有这些字段，因此 DSE 不伪造 token-level continuation：
  <https://html.spec.whatwg.org/multipage/server-sent-events.html>
- AWS 建议把 timeout 视为未知结果而不是“未执行”，只在单一层做 bounded
  retry/backoff，并用幂等性决定能否安全重放：
  <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/>、
  <https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/>
- Google 同样把 transient response 与 idempotency 作为两个独立 gate，建议对
  408/429/5xx/socket reset 使用 truncated exponential backoff：
  <https://docs.cloud.google.com/storage/docs/retry-strategy>、
  <https://docs.cloud.google.com/iam/docs/retry-strategy>
- Codex 与 Claude Code 的 `resume/continue` 文档描述的是保存会话恢复，不是上游模型
  stream continuation。这支持 DSE 把 session resume 与 physical request retry 分离：
  <https://developers.openai.com/codex/cli/reference/>、
  <https://docs.anthropic.com/en/docs/claude-code/cli-usage>

AWS 的 full-jitter 建议主要解决分布式客户端同步重试的惊群。DSE 当前是一个本地
Runtime、每个 logical request 最多两次 retry，并要求 exact replay；没有测得并发惊群
损失。因此 M33 保留确定、可持久重演的 1s/2s backoff，不为“看起来更高级”加入随机
jitter。若未来多客户端/多 actor 同时撞限流形成真实损失，应作为独立、可归因切片评测。

## 实现与 durable cutover

- Runtime 用唯一 `RuntimeClock` 生成
  `decision_unix_ms/backoff_ms/not_before_unix_ms/max_retries`；fake clock 测试不依赖
  flaky sleep。
- 第一次/第二次本地 backoff 为 1s/2s，上界 60s；实际 response header 的
  `Retry-After` 只能延长等待。
- timeout/network/429/可重试 5xx 只有在 replay-safe、没有
  content/reasoning/tool/usage/finish evidence、预算和次数允许时才 retry。
- 401/403/普通 4xx、partial/incomplete stream、usage 已出现、in-flight crash、
  unknown side effect 或 limit/deadline 耗尽都停止。
- prepared retry 在 send 前 crash，SQLite reopen 后等待剩余 not-before 并只发送一次；
  in-flight crash 保持 `RecoveryRequired(ModelRequest)`，不猜上游状态。
- hard cutover 为 Run API v15、RuntimeEvent v22、State v28、exec-stream v5。旧
  materialized Run 不通过 compatibility reader 猜新 schedule；仅保留可安全重建的
  pending Start。
- TUI/CLI/app-server 只投影 stored event。中英文同时显示失败、retry ordinal/total、
  等待秒数和停止原因，不新增 modal、设置页或第二状态机。

物理删除：

- `TransportRetryPolicy`、transport retry loop/sleep；
- `with_retries_disabled`；
- `[retry]` reader、example 与 active reference；
- `--transport-max-retries`；
- production retry fingerprint 字段；
- `transport_retry_count` / `transport_retries` active accounting；
- corrected current Harness 对失效 flag/字段的依赖。

历史 evaluator、frozen manifest/summary/raw 中的旧字段保留为当时事实，不是 production
consumer，也不被改写。

## deterministic 证据

冻结矩阵结果：

```text
pre-header timeout -> 1s -> success             pass
network -> 1s/2s -> success                     pass
429 + Retry-After -> success                    pass
500/503 bounded retry                           pass
401/403/ordinary 4xx no retry                   pass
partial content/reasoning/tool/usage no replay  pass
prepared retry SIGKILL/reopen                   exactly one send
in-flight SIGKILL/reopen                        RecoveryRequired, zero blind send
limit/budget/deadline                           explicit terminal
root/read-only/Writer                           one conformance
CLI/TUI/app-server + en/zh-Hans                 one stored fact
false success / false progress                  0 / 0
```

门禁：

- `./scripts/dev-dse.sh focused`：tools 347 passed/1 ignored；DeepSeek 60/1；
  Runtime conformance 88；app 57/1；app-server 23；exec terminal 25；
  canonical TUI 18；real PTY 7；
- process retry SIGKILL/reopen、State v28 migration、surface parity 2/2、CLI canonical
  6/6：通过；
- corrected Harness self-test：通过，credential 未读取；
- `cargo fmt --all -- --check`：通过；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过；
- `cargo test --workspace --locked`：通过；主 TUI/CLI unit group 765 passed/2 ignored，
  PTY 15、release runtime 5/1、canonical TUI PTY 7、exec terminal 25、surface parity
  2，全部 doctest 通过；
- public repository checker 与 `git diff --check`：通过。

## official DeepSeek canary

离线门禁闭合后，用户明确授权直接使用 repository ignored test Key。Key 未打印、未写入
日志、raw 或 Git。

冻结 identity：

```text
HEAD / contract                   5bcaa7e45726ce06b2d3e4c63b4662163f206c68
dirty source diff SHA-256         534361738f62f21aa5efab39640664b2533bc720fcefdd0765383756ed191955
release dse SHA-256               d1116715b1b05c8b518b713b881120df73f9912649a70ac7ef4f9fb85c10f1d3
release dse-tui SHA-256           54df2a0142a456380033d3060faf56ed1dbd21d0b672c8f538ac063587f7a95a
model / reasoning                 deepseek-v4-pro / high
surface                           official Standard Chat
max physical requests             1
```

第一次 local delivery preflight 因 release dispatcher 找不到 sibling `dse-tui` 在网络前
失败，官方请求数为 0；补齐同源 companion 后唯一 canary 返回 `M33_LIVE_OK`：

```text
physical started/completed/in-flight   1 / 1 / 0
runtime retries                        0
usage complete / cost complete         true / true
billing unknown                        0
input / output / total tokens          2811 / 38 / 2849
cache hit / miss                       640 / 2171
reasoning tokens                       32
cost                                   USD 0.000979765 / CNY 0.006757
duration                               1611 ms
```

没有为触发真实 timeout、429 或 partial output 而攻击官方服务。该 canary 只证明同源
release 的普通成功路径与 accounting；failure/retry safety 由 deterministic
loopback、fake clock 和 process crash evidence 负责。

## 决策与非结论

M33 接管 production。它同时提高正常 transient failure 的可恢复性、保留 partial
output/in-flight crash 的 fail-closed 行为，并删除双 controller 与撒谎配置。

本结果不证明：

- DeepSeek 提供 partial SSE continuation 或 exactly-once request identity；
- timeout 一定未计费、429/5xx 一定未执行，或任意 partial output 可安全重发；
- 真实远端 failure retry 的成功率、延迟或费用已经通过故障注入测量；
- random jitter、更多 retry、用户可调 retry、Auto/router、第二 Provider/Runtime/Store、
  默认 swarm/multi-Writer、FIM 或 RepoGraph 有收益；
- app-server reconnect 或 same-Run resume 等于上游 token stream continuation。
