# M7-A2 DeepSeek Agent 收敛正式 A/B

日期：2026-07-22

阶段结论：**hold（真实 DeepSeek stream/accounting 不完整，不是 Harness 假阳性）**。

## 冻结身份

| 项目 | 值 |
|---|---|
| suite ID | `m7-a2-agent-convergence-ab-v1-18de2ad2-vs-c6a74304` |
| result schema | `codewhale.eval.m7-agent-convergence.v3` |
| control | `18de2ad29a9db5780fcbe90e8ba0eef39225399f`，parent `3351213b07a821954ac06200012f21e1b1f2b3d5` |
| control tree / protocol | `99abedbcb9b2371a8bfd19645291823f4b7e9b2b` / Run API v9、RuntimeEvent v13、State v18 |
| control binary | `sha256:9c8d5b095222ac879ad5cf85f83bfe53b9e91dda88c4775f213fd02d3bdc0b3d`，16,058,768 B |
| treatment | `c6a743040eae9548837849c6f58e5c638cc073cc`，parent `24c8a530fd7ae200d823e07cce5d9c75ff2cc5ea` |
| treatment tree / protocol | `8615fd994a63ee50106b415cd43f1a10de814307` / Run API v9、RuntimeEvent v14、State v19 |
| treatment binary | `sha256:dd5becce5fb9ab50b1cf53de70918adb26c8da7957672817943c51f1d774445a`，16,091,840 B |
| model / surface | `deepseek-v4-flash` / Standard Chat |
| evaluator | `1077fe93c2cfffb15a0b8d866ecb3fc729079342`，tag `eval/m7-a2-evaluator` |
| Harness | `sha256:8ecda93109cfd9f9a9f37dc5b1e2cf2b652a70b5b11b4e434ab747858e51ccd8` |
| manifest content / file | `sha256:ce379e0b4c050f5f9e45931b14db478ba6c210706819024354ed0c5cd71bbb19` / `sha256:ec30735bc68428c00135ecfaa44b31f6ae60a83a9aa775dfb9d635a6da253d94` |
| schedule / suite schedule | `sha256:6cc1ec05469aa874e43bb4989e13b35a2eb753dcabe2506c289fdeaadc90ec37` / `sha256:d71a9509c0ad9b520ccea0b5fddecf636f073a39dfa2fbc5b85d0155b895a6ff` |
| canonical vector file | `sha256:15f8cafc890293ab991893dd72fb279edea6e2387794f98cbdfc6c1d7ca9e87d` |
| shared fix patch / patch-id | `sha256:688bf9100c6bf4d42227098a1d9a8140bd6ab7c3ffcb46d4b930452897323b13` / `1625ec95e2068bd6b71c53434e655c34a4dc40cb` |
| raw result | `eval/results/m7-a2-agent-convergence-formal-18de2ad2-vs-c6a74304.json` |
| raw result SHA | `sha256:ca3eeaaa7a2e46914d262549dab57c57fe23bfd7a94678002a81f405f80ffbbc` |

control 与 treatment 都是指定 parent 的单一 direct-child commit，应用相同 changed paths、
numstat 和 stable patch-id；原始 parent delta 与修复后 delta 的 stable patch-id 都是
`57f8b1f7ecc022c89bb77f77b330a072e6cadfd1`。本地 tag
`eval/m7-a2-control`、`eval/m7-a2-treatment` 和 `eval/m7-a2-evaluator` 保留可重建身份。

## 修复与离线门禁

`crates/protocol` 现在是唯一 canonical JSON owner：每层 object 显式按 UTF-8 key bytes
排序，array 保持原顺序，artifact 构造和 replay 校验共用同一 canonical-byte helper；不再
依赖 `serde_json::Map` 后端或 workspace feature unification。固定向量覆盖嵌套对象、数组、
Unicode、转义、M7-A v1 T1 artifact/receipt，以及 payload、digest、byte length、artifact ID
篡改。关闭或开启 `serde_json/preserve_order` 的生产依赖图得到相同 bytes/SHA/ID。

Python Harness 显式使用同一排序、UTF-8、无 NaN 口径，并冻结 Rust/Python 共同向量。它还
绑定实际 RunCreated/RunView 的 model、临时 workspace、root actor、parent/continuation，
校验完整 event/RunView digest、TaskContract、Host verification、typed rejection 实因、
artifact/receipt/lineage 和工具 Prepared → Started → Committed 生命周期。同步双边篡改不能再
靠“彼此一致”通过。

以下 live 前门禁全部通过：

- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- Harness 36/36 self-tests；
- 两个 direct-child checkpoint 的 protocol/tools 固定向量与独立 exact release build；
- `git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0` 和仓库外 target。live 前旧 M7-A v1 raw 仍为
`0600`、Git ignored，SHA-256 保持
`27e38db2f33280528a1552a57c1bb705ef3ab9bd15bffdaa9552ba9958825af2`。

## 正式执行

预注册计划为 20 对 / 40 arms。实际完成冻结 schedule 的前 7 arms：

```text
T1 control -> treatment
T2 treatment -> control
T3 control -> treatment
T4 treatment -> stop
```

前 6 arms 全部 measurement valid、false-success 为 0：

| T1–T3 三个完整 pair | control | treatment |
|---|---:|---:|
| verified success | 0/3 | 3/3 |
| physical attempts | 29 | 17 |
| input + output tokens | 191,586 | 77,089 |
| known USD cost | 0.014296946 | 0.004452426 |
| Harness arm wall time | 170.779 s | 64.249 s |

treatment 的三个成功 arm 均为 canonical Completed、唯一 Host receipt、外部 exact verifier
通过、修改范围与权限正确、accounting 完整；control 三个 arm 都是 canonical Blocked。上述
方向性差异只能描述已执行前缀，不能代替完整 40-arm 产品结论。

第 7 arm `T4 treatment` 修改了预期的 `runtime_banner.py`，外部 verifier 通过，但 canonical
terminal 为 Failed，没有 Host receipt。production 事实为：

- `model_request_prepared=4`、`model_request_in_flight=4`；
- `model_response_committed=3`、`model_request_failed=1`；
- physical request `started=completed=4`、`in_flight=0`、`sealed=true`；
- 只有 3 个 usage response，`incomplete_responses=1`；
- `usage_complete=false`、`complete=false`；
- `billing_unknown=false`、`unpriced=false`、0 transport/runtime retry。

accounting 语义证明 HTTP response 已建立，但 response body/stream 的 terminal/usage 生命周期
没有闭合。raw 只保留 `model_request_failed=1` 和摘要，没有可把具体原因归因到网络、provider、
输出上限或其他产品故障的 typed cause，因此不得进一步猜测。`billing_unknown=false` 只表示
不是“请求是否到达 provider 未知”；它不把缺失的 usage 变成已知零费用。7 arms 共 50 个
physical attempts；Harness 按冻结门禁停止，并将总已知费用 USD `0.019383566`（CNY
`0.138454040`）标为下界。

Harness 的 `surface_totals_valid` 还把 bucket `response_count=4` 与
`usage_response_count=3` 视为不相等；这对 incomplete response 的字段表述偏严，也是当前
`accounting.valid=false` 的组成条件，但不是必要条件。即使以后单独修正该投影，
`incomplete_responses=1`、`usage_complete=false` 和 `complete=false` 仍独立要求
measurement invalid/hold。冻结 evaluator 和本 raw 不做结果后修改。

## 决策与后续边界

- 不 `keep`：只完成 7/40 arms，正式 schedule 不完整，费用不是完整值。
- 不 `reject`：安全门禁全部通过，0 false-success、0 treatment identity failure；正式计量
  不完整，预注册规则要求 hold，不能据此判定或排除候选能力回归。
- 不 `shrink`：manifest 已冻结 `shrink_enabled=false`，不能结果后挑 T1–T3 子集。
- 决策为 `hold`：`product_metric_eligible=false`。观察到的 `+3` 只作机制证据，不能宣称
  M7-A 已获得正式净收益。

raw 为 `0600`、Git ignored、无 `.partial`、`active_arm=null`。不得删除、覆盖、重命名、
续跑、补 T4 control、替换第 7 arm、追加第 8 arm或从头重采样。

下一独立生产切片应先解决 DeepSeek stream terminal/usage 的可诊断与恢复边界：在不重复可能
已有 actionable output、不伪造 usage、不中和 fail-closed accounting 的前提下，区分完整
响应、可安全重试的零输出中断和带部分输出的不可重放失败；正式 evidence 还应持久化脱敏的
model failure code/category/actionable/retry decision，避免只剩事件计数。只有实质 production
变化和新的离线 stream fixture/crash/replay 证据成立后，才能建立新 candidate、suite ID、
output path，从 position 1 重新冻结。不得用 Harness-only 换号重跑，也不据此开放多 Writer。
