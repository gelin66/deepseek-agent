# M8-M billing-provable 中文 Agent prompt successor 结论

## 结论

- 产品决策：`hold_prompt_candidate / keep_bundled_prompt / do_not_resume_or_splice`。
- V15：仍为 `blocked`；current V1 matrix 保持 `15 pass / 1 blocked`，V1 不可发布。
- production：`crates/context/src/prompts/constitution.md` 保持不变；没有 prompt
  selector、模式、第二 owner 或 compatibility branch。
- 正式 suite：在 6/30 arm 以 `aborted_unknown_billing` fail closed；不得续跑、补 mate、
  拼接 M8-D/M8-M 样本或从 position 7 继续。
- raw：`eval/raw/m8-m-prompt-successor-formal-d1d6ca5c-v1.jsonl`，mode `0600`，
  7,427,653 bytes，SHA-256
  `641d7d80129b4673bd93eb4d4a0af57f278b326357af71c04188b8eae74acec7`。

这不是 candidate 质量结论。前 5 个 arm 全部 verified success、false success 为 0，
但只形成两个完整 pair 和一个未配对 baseline；第 6 个 candidate arm 的首个物理请求没有
收到 response headers、content、finish reason 或 usage。canonical accounting 正确记录
`billing_unknown=true`，因此没有完整、计费可证明的正式矩阵，也不能计算产品收益。

## 固定身份

M8-M 从 M8-L clean checkpoint `d1d6ca5c8aed06ffcd4be063f14a27666c2a65b8`
开始，保持 Run API v11、RuntimeEvent v17、State v23、exec-stream v3 和唯一
`AgentApplication -> AgentRuntime -> DeepSeekModelPort -> RunStore` 链。

immutable release identity：

| fact | value |
|---|---|
| source tree | `b76a85f889388d5e38c579672a935c22e41c2177` |
| archive SHA-256 | `18dcbcab95e03918b143df954267308ffe33364b08b51b650352645d7fa7c010` |
| `codewhale` SHA-256 | `94e3a3839e983179a36e7a4dfc7fdffb7e0f8b49a949764605f09abebfc99456` |
| `codewhale-tui` SHA-256 | `064c015985ec6021bd3ac0a06272120b8d5d63f983cc75d040fd98e59a4886bf` |
| binary pair SHA-256 | `2540375f81a92e96b633d83aa6102ba778e131b96668818b7547d6e7f8769c99` |
| Harness revision | `d723d0b3cc01f19d78fd0e2807a96919bafe6e05` |
| Harness SHA-256 | `fb054706bcf79da6e254da71973d99fcddfec5011818da555d2a36e01730d876` |
| schedule SHA-256 | `4c299076985e433d0294efcf64fefb9bd1ca3402da70fa66d43901d791a399ae` |
| candidate prompt SHA-256 | `2c3b8018a11c49ca1e0ae9b331ef12621aec77a3d56c631c01e535a74ec50ebf` |

两臂都固定 `deepseek-v4-pro`、ordinary Standard Chat、`reasoning_effort=high`、
streaming、8,192 output-token 实验上限、10 physical-request 上限、0 transport/runtime
retry 和 `maximum_reruns=0`。5 个冻结任务覆盖 single、read-only child 和 explicit
isolated Writer；正式计划为 5 tasks × 2 variants × 3 repetitions = 30 arms。

candidate 只通过既有显式 constitution override，把 bundled 五步 checklist 替换为两行
fact-gap loop。preflight 由同一 immutable binary 在禁外网 loopback 中证明两臂除 stable
constitution text 外，actor、workspace authority、TaskContract、catalog、budget、
reasoning、cache controls、remaining prompt blocks 与 verifier identity 相同；SQLite
重开不需要 credential，也不产生模型请求。

## M8-D 历史审计与替代

M8-D v1-v5 不能继续使用：

| suite | 不可采纳原因 |
|---|---|
| v1 | app-server 未加载 override，两臂没有真实 prompt surface delta |
| v2 | child/root accounting 投影错误 |
| v3 | Writer fixture identity 错误，raw 留在 running |
| v4 | Writer lifecycle/scope 被错误扁平化 |
| v5 | 首个 arm 有 1 次无 usage 的 physical request，billing unknown |

v1-v4 已知费用下界为 `$0.052454002`，v5 费用未知。M8-M 不复用这些 arm，使用全新
suite/output 和 current fixed-Pro production binary。旧 M8-D frozen manifests、raw 与
summary 保持历史可审计；失去消费者的 M8-D Python evaluator/test 已由 M8-M Harness
替代并删除。

## 正式执行事实

正式 runner 先 durable 写入 suite plan，随后才读取 credential；每个 arm 依次 durable
写入 exact terminal snapshot、无 credential SQLite reopen snapshot、external verifier
snapshot，最后才派生 observation。日志使用 `O_EXCL|O_APPEND|O_NOFOLLOW`、目录 fsync、
逐 record fsync、单调 sequence 和 previous-record SHA-256 chain。

最终日志有 33 个完整 records、0 partial-tail bytes：

| record | count |
|---|---:|
| suite plan / credential access | 1 / 1 |
| arm plan | 6 |
| terminal / SQLite reopen / verifier snapshot | 6 / 6 / 6 |
| arm observation | 6 |
| suite abort | 1 |
| suite result | 0 |

前 5 个 measurement-valid arms：

| task / variant | verified | false success | requests | input / output tokens | cache hit / miss | known cost | wall time |
|---|---:|---:|---:|---:|---:|---:|---:|
| t1 baseline | true | false | 5 | 22,434 / 2,618 | 10,496 / 11,938 | `$0.007508738` | 42.561 s |
| t1 candidate | true | false | 6 | 28,304 / 2,745 | 15,360 / 12,944 | `$0.008074470` | 46.238 s |
| t3 candidate | true | false | 6 | 22,683 / 1,377 | 15,360 / 7,323 | `$0.004439175` | 25.775 s |
| t3 baseline | true | false | 4 | 14,645 / 1,252 | 6,272 / 8,373 | `$0.004754231` | 24.100 s |
| t5 baseline | true | false | 10 | 56,910 / 5,645 | 27,264 / 29,646 | `$0.017905992` | 85.553 s |

这 5 个 arm 合计 31 个 usage-bearing responses、144,976 input tokens、13,637 output
tokens、74,752 cache-hit tokens、70,224 cache-miss tokens，known-cost lower bound 为
`$0.042682606`。这些是停止证据，不是 product-metric eligible aggregate。

第 6 个 arm 是 t5 candidate。其首个 root request 产生 typed
`deepseek_transport`：category `transport`、retryable/retry-safe true，但 response headers、
content、reasoning、tool call、finish reason、stream done 和 usage 全部未观察到。冻结的
retry contract 为 0，因此 retry decision 是 `stop/retry_limit_reached`。RunStore 在
terminal 后 sealed，SQLite reopen exact；workspace 没有副作用，但该 request 是否计费
无法由 provider response 证明：

```text
started = 1
surface responses = 0
usage responses = 0
billing_unknown_attempts = 1
billing_unknown = true
complete = false
```

Harness 随即写入 `aborted_unknown_billing`，记录
`completed_measurement_valid_arms=5`、known-cost lower bound `$0.042682606`，退出码 2。
没有执行第 7 arm，也没有自动重试。

## 离线与安全门禁

Key 只在 clean-tree live admission 提交和最终 immutable preflight 之后由 formal runner
读取；没有提交、打印或写入 raw。raw 为 ignored `0600`。正式执行前通过：

- 19/19 successor Harness tests；
- 6/6 journal SIGKILL fault windows和 hash-chain self-test；
- production prompt-only activation、no-credential SQLite reopen 和 exact binary identity；
- targeted context/app/CLI checks；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- workspace `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- workspace `cargo test --workspace --locked`；
- exec/HTTP/stdio/TUI parity、production loopback、process-level SIGKILL/reopen；
- `git diff --check`。

全部 Cargo 命令使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8m-target` 和 offline dependency resolution。

## 官方协议复核

2026-07-24 重新核验：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)；
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)；
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)；
- [Change Log](https://api-docs.deepseek.com/updates/)。

CodeWhale production 继续使用官方
`POST https://api.deepseek.com/chat/completions`。当前模型为
`deepseek-v4-pro`/`deepseek-v4-flash`；2026-07-24 退役的是
`deepseek-chat`/`deepseek-reasoner` legacy model aliases，不是 ChatCompletions
interface。M8-M 没有 Anthropic Messages、FIM、Provider、第二 transport 或模型路由
treatment。

## 保留、删除与非结论

保留：

- bundled production prompt 和 `crates/context` 单一 owner；
- app-server/exec/TUI 共用的 prompt override loader consistency；
- canonical Chat planner/sender/parser/accounting、AgentRuntime 与 RunStore；
- frozen M8-M manifest、本 summary 和 ignored `0600` raw。

删除：

- M8-M candidate-only runner/test；其冻结 hash 和实现仍由 Harness revision
  `d723d0b3` 可复核；
- 任何从 stopped suite 续跑、补 mate 或拼接历史样本的路径。

本结果不证明 candidate 更好、更差或等价，也不证明当前 bundled prompt 已通过 V15。
两个完整 diagnostic pair 不满足 15-pair、strata、false-success、质量非劣和稳定效率门槛；
第 6 arm 也不能按 0 cost/0 tokens 计入 candidate。

V15 只有在全新 successor 从 position 1 完成全部 30 arms，且每个 active arm 的
physical request、usage、cache、retry、cost、RunStore、verifier 与 reopen 身份闭合后才
能关闭。没有新的完整证据前，production 默认不变。
