# M6-B1 rework single / isolated Writer 正式 DeepSeek A/B v3

> 日期：2026-07-22
>
> 记录类别：`formal_same_binary_treatment_ab_measurement_abort`
>
> 产品指标资格：`false`
>
> 决策：`hold_mechanism`
>
> M6-B2：不准入

## 1. 结论

M6-B1 rework 已完成并通过全部离线门禁；正式 v3 不是因 Harness、Runtime、RunStore、
Writer 安全或 Git cleanup 故障停止。第 3 个 arm 的第一个逻辑模型请求发生一次 retryable
`deepseek_transport`，Runtime 重试后继续完成。该 arm 的 10 个 physical attempts 中，
9 个有 provider response 与 usage，另 1 个是否到达 DeepSeek、是否计费无法证明。

预注册规则要求 unknown billing 保存当前 arm 后立即停止且不得重采样。Harness 因此未启动
T2 Writer mate，最终输出 `aborted_unknown_billing` / `hold_mechanism`。本结果证明计量门禁
正常工作，但只有 1 个完整 pair 和 1 个 invalid arm，不能用于比较 single/Writer 产品收益。

v2 的 18 对 / 36 arms `reject_and_rework` 结论不被本次小样本推翻。当前单 Writer 隔离
机制继续保留，但只允许显式 opt-in；M6-B2、双 Writer、通用 DAG 和第二 scheduler 仍不准入。

## 2. 冻结身份

| 轴 | 身份 |
|---|---|
| candidate revision | `3310aa73ef3bae45ce9296e65964c9b7531c22f0` |
| version | `codewhale 0.8.68 (3310aa73ef3b)` |
| binary SHA-256 | `4092546976820f0a7603ff6ed97faadcf5626e812c6107e90ded627004e6e3c8` |
| binary size | 14,735,664 bytes |
| result schema | `codewhale.eval.m6-writer-benefit.v3` |
| plan schema | `codewhale.eval.m6-writer-benefit-plan.v3` |
| Run API / RuntimeEvent / State | v9 / v13 / v18 |
| manifest SHA-256 | `96bc7030c19945501ab0d658a85325ba5a28ef9f50a1da9365f116eaa651087e` |
| manifest content SHA-256 | `5f3b173c43df9194aed199862146961b1f8c8c4bb6d4103a693d4a3c82e6021d` |
| Harness SHA-256 | `6c4b028c0e7afad56299c24d872a9ec99ee7f7b81ad7ca4fb486b3d5e2ef0187` |
| canary helper SHA-256 | `fbeea1ce4951f450acb0534297d3f6bd619a6060c4cb67cde3c00a8a717fe68f` |
| schedule SHA-256 | `f2c6e0c0eed0da65921abb45379423d7c2064a4570ec3391c0ad806348aa97ed` |
| app-server argv SHA-256 | `124b2acdf08a635e509808239e1cb8e702851da62cfa43b3d7a3816094d17c3f` |
| model / surface | `deepseek-v4-flash` / Standard Chat |
| raw result SHA-256 | `2bc41e464888e49c7894881f36b2ad8eed5b10e47191ddd2b2a5991148acb2c1` |

8 个冻结源码 owner 均唯一；三任务 fixture tree、task definition、verifier 文件、verifier
spec、start command 与全部 ordered tool definition SHA-256 均匹配 v3 manifest。candidate
revision 已编入二进制，`build_revision_bound=true`。

原始结果是本地 Git ignored 文件
`eval/results/m6-b1-writer-benefit-ab-v3.json`，权限 `0600`。它不保存 prompt、模型文本、
reasoning、工具参数/输出、stderr、临时路径或 Key；`.partial` 已删除。

正式 candidate 冻结前，以下离线证据全部通过：

- `cargo fmt --all -- --check`；
- 27/27 Harness self-tests、standalone canary self-test 和精确 18 pairs / 36 arms dry-run；
- production Rust 实际 actor tool definition 与 manifest 冻结哈希一致性测试；
- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`，0 failure；
- verifier workspace 洁净、actor capability、时序 EvidenceReceipt、crash/reopen、accounting、
  cleanup/recovery 回归；
- `git diff --check`，candidate worktree clean，仓库内 `target/` 不存在。

构建使用空的仓库外 Cargo target 和 `CARGO_INCREMENTAL=0`。冻结后 candidate 源码、Harness、
manifest、schedule 和二进制均未改动。

## 3. 实际执行

| Arm | Terminal | Measurement | Requests | Usage responses | Runtime retry | Known cost USD |
|---|---|---|---:|---:|---:|---:|
| T1 single pair 1 | blocked | valid | 9 | 9 | 0 | 0.004897256 |
| T1 Writer pair 1 | blocked | valid | 10 | 10 | 0 | 0.003807518 |
| T2 single pair 1 | blocked | invalid | 10 | 9 | 1 | 0.003878678 lower bound |

总计 3 arms、29 physical starts，已知 usage 为 167,003 input、11,929 output，已知费用
下界为 USD `0.012583452` / CNY `0.089881800`。suite 耗时 141.168 秒。

T1 两个 arms 计量完整，但均未得到与当前任务和 workspace 精确匹配的 Host verifier receipt；
它们是可信 blocked 产品结果，不是假成功。T2 single 完成预期 fixture 修改且外部 verifier
通过，但 canonical Host 在同 revision 重复验证前拒绝完成；该产品结果也为 blocked。三个
arms 均 `verified_success=false`、`false_success=false`。

这些结果只供诊断。一个完整 T1 pair 不能满足预注册的 reliability、complex-task benefit、
dual-success efficiency 或 control 样本门槛。

## 4. unknown billing 归因

T2 single 的 canonical 关系为：

```text
10 ModelRequestInFlight
= 9 ModelResponseCommitted
+ 1 ModelRequestFailed(code=deepseek_transport, retryable=true)

1 ModelRequestFailed 触发 Runtime retry
=> runtime_retries=1

失败 attempt 没有 provider response/usage
=> billing_unknown_attempts=1
```

该 run 的 Terminal 唯一且是最后一个事件，sequence 1723 等于 RunView last sequence；Terminal
状态、Terminal accounting 与 RunView 全部精确一致。actor usage、surface usage 和 9 个
`ModelResponseCommitted` 的 token/cost 逐字段一致。root
`started=10/completed=10/in_flight=0`，没有 pending request、records-after-seal、unpriced
response 或 Harness cancel race。

`completed=10` 表示 10 个 physical transport lease 已结算，不表示 10 个都成功返回 usage。
DeepSeek accounting 对没有响应头的 inference lease 保守记录 unknown billing；Runtime 的
第 1 次 retry preparation 原子嵌入 `ModelRequestFailed`，因此 9 个独立
`ModelRequestPrepared` 加 1 个内嵌 retry attempt 也完全一致。

官方 DeepSeek 文档只规定成功 streaming 请求的 usage 在最终 usage chunk 中返回，并建议对
服务端故障重试；它没有提供可将本次 client transport failure 证明为免费请求的逐请求
账单回执。因此不能把缺失 usage 当成零费用：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [Error Codes](https://api-docs.deepseek.com/quick_start/error_codes/)

## 5. 预注册停止与决策

v3 manifest 在 live API 前已冻结：

- unknown billing 立即停止；可见费用只作下界；
- maximum reruns 为 0，任何 measurement-invalid arm 都不得替换；
- 停止前先原子保存当前 arm；停止后不启动 mate；
- 18 个 measurement-complete pairs 不可用时，决策为 `hold_mechanism`；
- hard mechanism failure 才判 `reject_and_rework`。

Harness 精确执行了这些规则。最终：

- `mode=aborted_unknown_billing`；
- `decision=hold_mechanism`；
- `product_metric_eligible=false`；
- `hard_mechanism_abort=false`；
- 0 false-success、0 Harness safety violation、0 Writer safety violation；
- M6-B2 不准入。

`hold_mechanism` 表示正式产品比较证据不足，不表示 Writer 获得净收益，也不表示 rework
回归。v2 的完整产品结论继续约束默认行为。

## 6. 防重采样边界

本次结果不可续跑或与未来结果拼接：

- 不重跑同一 `3310aa73` candidate；
- 不只补 T2 Writer mate；
- 不复用 v3 的 T1 pair；
- 不把 v3 arms 合并进未来 v4；
- 不通过放宽 unknown billing、修改任务/预算/阈值来跑满 36 arms。

新的 v4 只有在独立、实质性的产品代码变化与离线验收后才成立。no-op、注释、版本号或
Harness-only 换号不构成新 candidate。v4 必须在任何 API 请求前重新冻结，并从 schedule
position 1 全新运行 18 对 / 36 arms。若实现针对已观察的三个任务调优，结果只能标为
adaptive rework；默认启用仍需要未参与开发的确认性证据。

只有权威 transport phase 或 provider 计费证据才能把失败 attempt 标为
`known_unbilled`。当前没有这种证据，所以无需修改生产 accounting 或 Harness。

## 7. 下一阶段

Writer 继续作为显式、隔离、可恢复的单 Writer 机制保留；不开发 M6-B2。下一产品阶段优先
进入 M7：以 verified task success / tokens / time / code complexity 为目标，调优 DeepSeek
单 Agent、只读多 Agent、工具选择、上下文预算、stable prefix/cache 和失败恢复。任何新
能力继续以同任务离线 fixture、真实 DeepSeek A/B 和删除旧路径为准入条件。
