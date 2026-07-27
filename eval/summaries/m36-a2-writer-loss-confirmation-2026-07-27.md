# M36-A2 fresh Writer repeated-loss confirmation 结论

日期：2026-07-27

产品决定：`keep_current_harness_no_repeated_loss`

production delta：0

## 决策

不准入 Orchestrator-owned verify -> repair -> reverify treatment，也不进入 treatment A/B。
三个 fresh explicit Writer task 中只有 `writer_header_policy` 出现
`orchestrator:writer_integration`；另外两个任务 verified success。同一
`owner_code:loss_code` 只覆盖一个独立 task_id，低于冻结的两个独立 task 门槛。

M36-A 的历史 `writer_envelope` 只提供待复核假设，没有进入本次输入或 threshold。不能把
两个不同 campaign 的单例拼成 repeated current loss。

## 可复现身份

- M38 clean anchor：`3b43a41a356f45fc4f1ca13100361b286695c154`
- M36-A2 contract commit：`7f7e510259f52ca7d21395f15f8fee7c8ab7a013`
- current permission projection corrections：`42fd2c58ccaffe9e66873368bedd6398a14fc774`、
  `a0847c5c1c0428bd79ff8bd7a553a48441d7115d`
- live admission commit：`2ffae6134351379acf45c2cf3ec1403a1776f297`
- acquisition candidate/tree：`a0847c5c1c0428bd79ff8bd7a553a48441d7115d` /
  `29d83999be582b3e49b1b8913ace32608a61de13`
- immutable `dse`：
  `sha256:bfa52d994870ff1141887df2143936f775908c0cb0fbf0309d5454cd1e244f28`
  (`dse 0.8.68 (a0847c5c1c04)`, 14,371,344 bytes)
- contract manifest：`eval/manifests/m36a2-writer-loss-confirmation-v1.json`
- contract SHA-256：
  `b27179750924d33ca942c886213174a4db25a693c08abf984fc794ebf3c219cd`
- live admission：
  `eval/manifests/m36a2-writer-loss-confirmation-live-admission-v1.json`
- live admission SHA-256：
  `fb802d832e0efa7936810708d99215afdbbb4c80a59440022aa59ecd483ce349`
- acquisition Harness SHA-256：
  `e6ee6de428967e224b27540f1d0d8ceaa34d32b8a0083414cdf57a28d90c7283`
- task/schedule SHA-256：
  `d7ae57d3de9e64adcab9880966628aa3a2e0547bd055867f6d02c08442fcfc26` /
  `bd5a39716eb56671f58d6432fe53c02d7b40335da7ba0c11edea0595c80634e5`
- fixture tree SHA-256：
  `b13990beefb69c5bfe99156296cb6cf876896e564ebc890c609f8548323bc4fc`
- protocol：Run API v15、RuntimeEvent v22、State v28、exec-stream v6
- surface：official DeepSeek Standard streaming ChatCompletions、
  `deepseek-v4-pro/high`
- raw：
  `eval/results/m36a2-writer-loss-confirmation-a0847c5c1c04-v1.jsonl`
- raw SHA-256：
  `5a43e4a0ccb5b5b8bba5a5907ae0affbd65c3c8c00f85354ac37fda9c724d4b6`
- raw mode/size：ignored `0600` / 8,025,420 bytes
- journal：21 records，hash chained，maximum reruns=0
- Key：只由正式 Harness 从 ignored `0600` test-key file 读取；未打印、未提交
- GitHub/push/release：0

## 正式结果

| task | terminal | verified | canonical loss | requests | known cost | wall time |
|---|---|---:|---|---:|---:|---:|
| `writer_retry_ledger` | completed | 1 | — | 12 | $0.034049016 | 137,872 ms |
| `writer_header_policy` | blocked | 0 | `orchestrator:writer_integration` | 14 | $0.039988506 | 212,011 ms |
| `writer_route_contract` | completed | 1 | — | 12 | $0.039689110 | 191,759 ms |
| **total** | — | **2/3** | **1 independent task** | **38** | **$0.113726632** | **541,642 ms** |

完整 aggregate：

```text
verified success               2/3
measurement-valid product loss 1/3
false success                    0
behavior truth closed           3/3
accounting complete             3/3
input/output tokens       196,296 / 19,773
cache hit/miss tokens     116,864 / 79,432
maximum Harness reruns            0
decision          keep_current_harness_no_repeated_loss
candidate                                      null
```

两个成功任务均有 exact Writer lifecycle、sealed diff、root integration、latest-revision
receipt、external verifier 和 `cleanup_status=removed`。失败任务没有修改 root、没有 sealed
diff、没有 Host receipt，worktree 仍被精确清理；Host blocked 避免了 false completion。
它是 measurement-valid product failure，不是 Harness、环境、transport 或 accounting
interruption。

三臂 usage 均为 complete、priced、sealed，`billing_unknown=false`；本次没有出现用户此前
多次遇到的 pre-header provider billing ambiguity。费用闭合来自每个 response 的 canonical
usage，不是用余额差或月度账单推测单请求成本。

## Cutover 与删除

- production-compiled `crates/`、protocol、State、config、CLI/TUI/app-server delta=0；
- 不实现、也不保留 bounded repair controller、selector、alternate Runtime 或兼容分支；
- M36-A2 campaign selector、manifest loader、Writer observer/aggregate、自测与 CLI consumer
  已从唯一 corrected Harness 物理删除；
- Harness 恢复为 M36-A2 前 exact Git blob `d3916654f74699ddec188e80cc5ac1edda0f2126`；
- restored Harness SHA-256：
  `04cdac789ecca8e235053b22c75a7492d40fe0c4bf54b427c74d453034ecef1d`；
- frozen manifest、live admission、fixture/reference、ignored raw、本 summary 与 Git 历史保留
  审计；没有创建无消费者 analysis manifest。

## 门禁

credential 前通过：3/3 initial-fail/reference-pass proof、fixture/base-commit identity、
current permission controls、Writer lifecycle/cleanup observer、四个 journal SIGKILL 窗口、
exact SQLite reopen、synthetic 0/1 与 2-task threshold、immutable binary dry-run、focused、
fmt、strict workspace Clippy、workspace tests、public repository checker 和 diff check。

workspace tests 的第一次运行只有
`bounded_git_success_terminates_a_descendant_that_keeps_stdout_open` 在全仓并行负载下超过
2 秒；同一未修改测试随后隔离 10/10 约 0.49–0.52 秒通过，完整 workspace gate 复跑也
通过。没有降低阈值或修改 production 进程回收。

删除后的第一次 M30 historical self-test 仅在 `go_health_api` reference verifier 失败；
同一未修改 fixture/reference 在隔离 evaluation HOME 中 8.57 秒通过，随后完整 M30
self-test、interaction/truth conformance 均通过。没有改 M30 frozen evidence、fixture、
verifier 或 Harness 语义。

最终删除后重新执行 Harness Python compile/self-tests、focused、fmt、strict Clippy、
workspace tests、public checker 与 `git diff --check`，并核对 exact pre-M36-A2 Harness
blob，全部通过。

## 官方协议复核

复核日 2026-07-27：official V4 model、Standard ChatCompletions、Thinking、Tool Calls、
stream usage 与价格均与 frozen manifest 一致：

- <https://api-docs.deepseek.com/updates/>
- <https://api-docs.deepseek.com/news/news260424/>
- <https://api-docs.deepseek.com/quick_start/pricing/>
- <https://api-docs.deepseek.com/api/create-chat-completion/>
- <https://api-docs.deepseek.com/guides/thinking_mode/>
- <https://api-docs.deepseek.com/guides/tool_calls/>

## 非结论

2/3 pass@1 不能外推到所有 Writer 任务；一个 Rust failure 也不能证明 Orchestrator treatment
无效。结论只是：当前 fresh evidence 没有达到开发该 treatment 的门槛。费用/Token/时间是
control observation，不是优化收益。

ADR-0015 仍是 architecture accepted、implementation not admitted。本 Goal 没有开发或
评测 browser、search、vision、Firecrawl、Playwright MCP、sidecar 或 CDP candidate；只有
未来 ROADMAP 基于至少两个独立任务的同一 repeated loss 明确准入后，才可开始最小 slice。
