# M40-A fresh engineering loss acquisition 结论

日期：2026-07-28

产品决定：`reject_incomplete_acquisition`

production delta：0

## 决策

不准入 production treatment，不选择 owner，也不继续八任务 baseline。正式 acquisition 在
第一个 `rust_feature_matrix` arm 触发冻结停止门：workspace 修改通过 deterministic verifier，
但第七个 physical DeepSeek request 在已有 response headers 和 actionable reasoning 后断流，
没有返回 content、trusted finish、usage 或 stream `[DONE]`。Runtime 正确拒绝 unsafe replay，
Host 没有签发 latest-revision completion，`false_success=0`。

这条 trajectory 的 behavior truth 已闭合为 `verified_product_failure`；accounting truth 则为
`usage_incomplete`。两者不能互相覆盖。缺少的 provider usage/费用不会用 tokenizer 估算、
余额差、历史均值或选择性重跑补造，所以完整 baseline、repeated-loss candidate 和 utility
aggregate 均不成立。

## 可复现身份

- 起始 clean checkpoint：`f7b54fc4e`
- contract commit：`9d22cd770`
- fixture/manifest commit：`e2fef519d`
- current-protocol continuity commit：`669a66a52`
- fixture identity correction：`e2ed02c3a`
- live admission commit：`2b1169dbe`
- read-only incomplete-acquisition analysis：`5ea14c63b`
- temporary consumer deletion：`f97e21661`
- acquisition candidate/tree：`e2ed02c3a805118d06399eb2b01d15f36789f033` /
  `e4b67767b396013982af23e44f37ea7f9f4cd49d`
- immutable `dse`：
  `sha256:5e81b58721eca6109545422fd1fb06af0804b756de502f894169a5b3193b8821`
  (`dse 0.8.68 (e2ed02c3a805)`, 14,371,344 bytes)
- process-test binary：
  `sha256:674b653556bf21f821426a2609cce3750388bf70f7e6f10f6a5b0750918ae6bc`
- protocol：Run API v15、RuntimeEvent v22、State v28、exec-stream v6
- surface：official DeepSeek Standard streaming ChatCompletions、
  `deepseek-v4-pro/high`
- contract manifest SHA-256：
  `63a6432e0644e72a9ecb30ce587daa7ddc653617da0b349c9da4d33e038ca0ce`
- live admission SHA-256：
  `2ad47fa94ba0fe7c83c573dea55d6d6d3709d4b58f5cef195b6771b0018f1289`
- acquisition Harness SHA-256：
  `73e2773b5d561ea10307375da7cf40c7424fbb9608158ccbce1774d3bccd2bdd`
- analysis manifest SHA-256：
  `eea72aa188f476957a60cfd9c3886cbe635adee74d313f196e7f6034c499591f`
- analysis Harness SHA-256：
  `19ff2f69650ec17bb1ffa54c6a42ff3f7785bd3e94f552c5492ca4f2ecaecb66`
- read-only canonical report SHA-256：
  `410b9a13db8d1d7bab0969a6c67e492d9c36351cef31c794b6653ddbf4b290c3`
- raw：`eval/results/m40a-engineering-loss-e2ed02c3a805-v1.jsonl`
- raw SHA-256：
  `3dda3bc717694a732e6a9a956e188f62373b13d87e5caa440071e41f5c9aae9c`
- raw mode/size：ignored `0600` / 1,006,365 bytes；8-record hash chain，无 partial tail
- fixture tree SHA-256：
  `37ef46b6bbad3116976891cac415f0305ca3accbeb3a6ccee40c6a8f1a150d9f`
- reference patch SHA-256：
  `169a6eb8460b9cf52d0824ae52ec6d116c35cefdd3bcdee2382d5e207bd85314`
- Key：只由 committed admission 后的正式 Harness 从 ignored `0600` test-key file 读取；
  未打印、未提交
- GitHub/push/release：0

## 正式结果

| task | terminal / verifier | behavior | canonical loss | physical requests | usage responses | cost |
|---|---|---|---|---:|---:|---:|
| `rust_feature_matrix` | failed / pass | verified product failure | `deepseek:transport_or_accounting` | 7 | 6 | `$0.010289171` known partial；total unknown |
| remaining 7 scheduled arms | not started | no observation | — | 0 | 0 | `$0` |

canonical facts：

```text
changed files                                      2
tool calls/outcomes                               11/11
model physical requests started/completed          7/7
model responses with usage                           6
incomplete responses                                 1
runtime retries                                      0
behavior observations closed                       1/1
verified product failure                             1
false success                                        0
accounting complete                                0/1
full-utility observations                          0/1
completed arm_result                                 0
maximum Harness reruns                               0
decision                 reject_incomplete_acquisition
```

修改文件仅为 `rust-feature-matrix/feature-cli/Cargo.toml` 和
`rust-feature-matrix/feature-core/src/lib.rs`，均在冻结 scope 内；external verifier 返回 0。
但是 Run terminal 为 `deepseek_transport` failed 且没有 Host completion receipt，因此 verifier
pass 没有被错误升级为产品成功。

最后请求的 response evidence 为：headers/reasoning observed；content/tool fragment/finish/usage/
DONE 未 observed。failure 标记 `retryable=true`，但同时
`actionable_output=true`、`retry_safe=false`；Runtime decision 为
`stop(actionable_output)`。这避免重复推理、重复计费和 crash 后重复发送。

已返回六份 usage 的局部计数为 input 38,825、output 3,140、cache hit 21,632、cache miss
17,193，known partial cost `$0.010289171`。第七个响应没有 usage，因此这些数不能称为 arm
total；`billing_unknown=false` 只表示没有未决 in-flight physical attempt，不表示费用完整。

## 为什么客户端不能补造最后一次费用

DeepSeek streaming contract 只在 `[DONE]` 前的附加 chunk 返回整次请求 usage；本次连接在该
chunk 前终止。官方 `/user/balance` 只返回账户总余额，Usage 页面导出只承诺按 API Key/月度
汇总；截至复核日，没有官方文档提供按 completion ID/request ID 查询单次已结算 usage 的 API。
余额差还会受到同 Key 其他请求、赠金/充值余额选择、缓存价格和结算时序影响，不能作为这次
physical request 的精确证据。

因此可彻底保证的是：canonical attempt 计数、partial-response 分类、安全重试门、Store
重开一致性、已知 usage/cost 的不丢失，以及“不完整绝不冒充完整”。在 provider 没有返回
usage 且没有 per-request reconciliation surface 时，客户端无法无损恢复真实 billed tokens。
未来只有官方新增可按 immutable request identity 查询的账单事实，才值得独立准入
reconciliation slice；不能在 M40 自造第二账单真相。

## Cutover 与删除

- production `crates/`、Prompt、DeepSeek request、RuntimeEvent、RunStore、tools、permission、
  actor route、Writer behavior、CLI/TUI/app-server delta=0；
- 没有实现 DeepSeek retry、billing estimator、balance-delta attribution、context treatment、
  browser/search/vision adapter、第二 Runtime/Store 或兼容分支；
- M40 campaign selector、manifest loader、fixture/live caller、aggregate/self-test 和 trajectory
  consumer 已从 corrected Harness 物理删除；
- Harness 恢复 exact Git blob
  `d3916654f74699ddec188e80cc5ac1edda0f2126`；
- frozen fixture/reference、contract/live admission/analysis、ignored raw、本 summary 与 Git 历史
  保留审计。

## 门禁

credential 前通过：8/8 initial verifier fail、7/7 reference pass、安全反例仍失败、exact
scope/tree/base identity、journal crash windows、M38 typed interaction、truth/acceptance/
hardness observer、immutable binary dry-run、current-protocol process SIGKILL/reopen、
root/read-only/Writer、CLI/TUI/app-server parity、双语 PTY、focused、fmt、strict workspace
Clippy/test、public repository checker 和 `git diff --check`。

正式 acquisition 停止后，exact raw 以 credential-free mode 两次生成 byte-identical report；
M40 self-test及历史 M9-C/M23-B/M30 report回归通过。临时 consumer 删除后再次执行最终
focused、fmt、strict Clippy、workspace tests、public checker 和 diff check；最终结果记录在
本 Goal clean checkpoint。

## 官方协议复核

复核日 2026-07-28：

- <https://api-docs.deepseek.com/updates/>
- <https://api-docs.deepseek.com/news/news260424/>
- <https://api-docs.deepseek.com/quick_start/pricing/>
- <https://api-docs.deepseek.com/api/create-chat-completion/>
- <https://api-docs.deepseek.com/guides/thinking_mode/>
- <https://api-docs.deepseek.com/guides/tool_calls/>
- <https://api-docs.deepseek.com/api/get-user-balance/>
- <https://api-docs.deepseek.com/faq>

## 非结论

本结果不是完整 M40 coding baseline，不证明其余七项任务成功或失败，也不能把单例
`deepseek:transport_or_accounting` 与历史失败拼成 repeated loss。它不证明模型能力、Prompt、
ContextBroker、tools 或 Runtime treatment 需要修改；partial known cost 也不是全 arm 费用。

ADR-0015 仍是 architecture accepted、implementation not admitted。本 Goal 没有开发或评测
browser、search、vision、Firecrawl、Playwright MCP、sidecar、remote browser 或 CDP
candidate；未来仍须由 ROADMAP 基于至少两个独立任务的同一 repeated loss另行准入。
