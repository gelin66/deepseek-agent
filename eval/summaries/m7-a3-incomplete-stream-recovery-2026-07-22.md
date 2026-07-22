# M7-A3 DeepSeek 不完整流式响应诊断与安全恢复

- 日期：2026-07-22
- 起始检查点：`0c3ef8121353c2f3c0e3dc195b870c4de7fc7265`
- 失败矩阵检查点：`afef610f688fb188a1cd1b1baebcf856d10fa301`
- 生产修复检查点：`e98ca5ae75fb258e5ff65e95a74b8966d180cded`
- Harness 检查点：`db381889b7fcc3c8a7eb649dce94bf8163f8a2a0`
- 决策：生产 correctness 机制保留；M7-A 产品收益结论继续 `hold`；本轮正式复评
  `inadmissible`，未读取 Key、未调用官方 API

## 1. 真实问题

M7-A2 正式 suite 在第 7/40 arm 停止。T4 treatment 的 4 个 physical attempts 只有 3 个
usage response，并留下 `model_request_failed=1`、`incomplete_responses=1`、
`usage_complete=false` 和 Failed terminal。原始事件没有足够的脱敏 typed response evidence，
无法证明失败发生在 headers、正文、reasoning、tool-call、finish、usage 或 `[DONE]` 的哪一
边界，也无法证明是否允许自动重放。

原始 raw 继续保持本地 Git ignored、`0600`、83,710 bytes，SHA-256 为
`ca3eeaaa7a2e46914d262549dab57c57fe23bfd7a94678002a81f405f80ffbbc`，同名 partial 不存在。
本切片没有覆盖、续跑、补 mate、追加或重写该结果。

## 2. 单一 owner 与实现

`crates/deepseek` 继续是 transport、SSE lifecycle 与 physical accounting 的唯一 owner：

- 只有受支持的 `finish_reason` 和 `[DONE]` 都已观察到，stream 才能提交完整响应；
- `[DONE]` 后出现任何新 data 均为 typed protocol failure；
- content、reasoning、tool-call ID/name/arguments、finish、usage 和 `[DONE]` 只投影为单调的
  `ModelResponseEvidence`，不持久化半截正文、工具参数、provider body 或 headers；
- usage frame 一经验证就立即进入 response accounting guard，之后 EOF、stall 或 consumer
  drop 仍保留已知 usage；
- `finish_reason + [DONE]` 已闭合但 usage 缺失时保留可信输出，同时明确记录 usage missing，
  不为补 usage 重发请求；
- DeepSeek production error message 使用固定中文诊断，不把网络细节、响应正文或本地路径
  写入 RunStore。

`crates/protocol + crates/runtime + crates/state` 只承载恢复需要的最小事实：

- `ModelAttemptFailure` 持久化 failure code/category、`retryable`、`retry_safe`、
  `actionable_output` 和完整布尔 response evidence；
- Runtime 合并 DeepSeek progress 与已落盘 content/reasoning delta；任意 partial tool-call 也
  属于 actionable output；
- 只有错误可重试且 response 可证明 replay-safe 时，才原子持久化下一次 prepared retry；
- partial content/reasoning/tool-call、已见 finish 或 `[DONE]` 均禁止自动重放；
- 完整响应提交前不会执行工具；crash 后 in-flight model request 仍进入
  `RecoveryRequired`，不会自动二次发送；
- root 与 read-only child 通过同一 Runtime conformance；Writer 没有获得第二套规则。

RuntimeEvent 直接切换到 v15，State 直接切换到 v20。v20 退役不兼容的 v19 run/finalized
creation，保留仍可安全恢复的 pending Run API creation；没有兼容 reader、双写或 schema
adapter。高频 content/reasoning delta 仍保持 append-only event，不复制到 snapshot。

## 3. Harness 修正

当前 Harness 只投影生产事实：

- `surface.response_count` 表示收到 HTTP response 的次数；
- `surface.usage_response_count` 必须等于 canonical `usage_responses`；
- 允许 `response_count > usage_response_count`，差额只能落在 missing/incomplete 的可解释
  上界内；`incomplete_responses > 0` 仍独立使 measurement 无效；
- token/cost 真相来自 `accounting.usage`，RunView 顶层已提交响应 usage 单独记录为
  `committed_usage`，并要求逐字段不超过 accounting usage；
- typed failure 只输出 code/category、布尔 evidence、retry 决策和 ID/message 哈希；不输出
  raw message、attempt ID、prepared request 或路径；
- retry tagged union、stop reason、prepared attempt 和 replay-safe 关系按 canonical schema
  精确校验；root 与全部直接 child 都进入同一 shape gate。

对 M7-A2 的 `4 response / 3 usage / 1 incomplete` 重新投影后，旧
`surface_totals_valid` 假 mismatch 消失，但 `complete=false`、`usage_complete=false` 和
`incomplete_responses=1` 仍保持原有停止与费用下界结论。Harness 自测 39/39 通过。

当前 Harness 不冒充新 formal evaluator。`run_suite` 在访问参数、创建 output、读取 Key、
复制 binary 或调用 API 前无条件返回
`m7_a3_formal_reevaluation_inadmissible`；M7-A2 的原 evaluator 身份仍由其历史 checkpoint 和
manifest 固定。

## 4. 为什么没有正式复评

Goal 要求同一个完整 stream/accounting correctness patch 字节等价地应用到 M7-A2 control
与 treatment，两个 checkpoint 都必须是 direct child，并保持 changed paths、numstat、
stable patch-id 和原 M7-A production delta 不变。当前实现无法同时满足这些条件：

- M7-A3 production diff `0c3ef812..e98ca5ae` 修改 12 个路径，stable patch-id 为
  `51109cf1cfca6755b5adbead6c151838c7aac8f0`；
- 这 12 个路径在 `0c3ef812` 与旧 treatment `c6a74304` 上 12/12 blob 相同，因此该 patch
  天然是 treatment-side correctness patch；
- 旧 control `18de2ad2` 在其中 8/12 路径不同；只读 `git merge-tree` 得到 8 个
  `changed in both` 文件和 20 个冲突块；
- 旧 control 为 RuntimeEvent v13 / State v18，旧 treatment 为 v14 / v19，而本修复要求
  v15 / v20 直接切换。

若把 control 直接提升到 v15/v20，会把 treatment 原有的 v14/v19 production 能力混入
control；若按 control 自身递增到 v14/v19，两边 correctness patch 的 patch-id/numstat 和
修复后的 production delta 都会不同；若不升级 schema，又不能持久化本切片要求的 typed
facts。排除 schema hunks、只共享 transport 子集或增加 arm-specific adapter 都不能证明
完整修复公平。

因此正式复评在读取 Key 前判定不可采信。本轮没有创建 M7-A3 formal manifest、release
binary、suite output 或 live raw，也没有消耗 API。M7-A 的产品收益结论继续保持 `hold`，
不能用 M7-A2 前 6 arms 的方向性结果宣布总体收益。

如果未来改变实验设计，应从共同的 post-M7-A3 corrected base 建立新 control，再把 M7-A
production treatment 重新实现为唯一 direct-child delta；那将是一个新的 treatment，不能
继续声称旧 production patch-id 被保持。

## 5. 离线验收

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m7a3-target
```

通过：

- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- Harness self-test 39/39；
- 独立只读 Harness、shared-fix fairness 与最终 evidence 复核。

从起始检查点到 production checkpoint 的 crates diff 为 12 files、`+1,216/-74`，没有新增
crate、Cargo dependency、Provider、模型可见工具、Runtime、RunStore 或兼容开关。新增
复杂度用于 typed protocol、离线反例和 crash/reopen 证明，不把代码行数本身视为能力收益。
