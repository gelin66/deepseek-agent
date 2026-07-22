# M7-A DeepSeek Agent 收敛正式 A/B v1

日期：2026-07-22

阶段结论：**hold（评测口径失效，不是产品安全 reject）**。

## 冻结身份

| 项目 | 值 |
|---|---|
| suite ID | `m7-a-agent-convergence-ab-v1-3351213b-vs-24c8a530` |
| result schema | `codewhale.eval.m7-agent-convergence.v2` |
| baseline | `3351213b07a821954ac06200012f21e1b1f2b3d5` |
| baseline tree / protocol | `10e2f40fc4b4e087d06e85f75badf4dabb90b1b7` / Run API v9、RuntimeEvent v13、State v18 |
| baseline binary | `sha256:4a93608ff5c3d409c0a1d05457aa87f30e7daac754557c5c7ecb368324dfb57d` |
| candidate | `24c8a530fd7ae200d823e07cce5d9c75ff2cc5ea` |
| candidate tree / protocol | `050691b10662bc31ba0405342a9e154c38a8e5a8` / Run API v9、RuntimeEvent v14、State v19 |
| candidate binary | `sha256:4173cc14aaf853674023cdfc53e331f42195486d5e0e2beb6f2aac6dd5e7d278` |
| model / surface | `deepseek-v4-flash` / Standard Chat |
| Harness | `sha256:3ca0ac151dd105205a1ed93e92d47bef42034e0aee1e29afe36d385611d69ab6` |
| manifest content | `sha256:7e9bd0c6bd086501b5c9bcaecfc1a08f9399568322fcb74814e78b3f8a2ba512` |
| manifest file | `sha256:1c50abf66fdca60eab275cdc87df77028b748d84f1bf2592ca753b85f95ef79a` |
| schedule | `sha256:6cc1ec05469aa874e43bb4989e13b35a2eb753dcabe2506c289fdeaadc90ec37` |
| canary helper | `sha256:fbeea1ce4951f450acb0534297d3f6bd619a6060c4cb67cde3c00a8a717fe68f` |
| raw result | `eval/results/m7-a-agent-convergence-formal-3351213b-vs-24c8a530.json` |
| raw result SHA | `sha256:27e38db2f33280528a1552a57c1bb705ef3ab9bd15bffdaa9552ba9958825af2` |

Raw 保持 `0600` 且 Git ignored。正式 output 没有覆盖、续跑或重采样。

## 实现、替代与复杂度

M7-A production candidate 由两个独立提交组成：

- `13b5c3eb`：真实问题是 caller、Host 与 recovery 分别推断 verifier plan；
  `crates/tools` 现在是 named verifier 解析和 exact spec 的唯一 owner，Start/Continue 在
  durable creation 前冻结 tool-resolved contract，旧的调用方手写/恢复重建路径被替代。
- `24c8a530`：真实问题是 completion rejection 只有非结构化文本，模型和恢复路径无法区分
  必须修改 workspace 还是补齐证据；`protocol/runtime/state` 现在以 exhaustive `cause` 和
  `required_transition` 绑定当前 task generation、workspace revision 和 canonical replay，
  旧的文本猜测路径被删除。

从起始 checkpoint 到 candidate 的 `crates/` 路径 diff 为 19 files、`+2,045/-428`；其中
独立 tests 为 5 files、`+633/-41`，其余源码路径上界为 `+1,412/-387`，且仍包含源码内联
测试。候选没有新增 Cargo 依赖、模型可见工具、Runtime、RunStore、scheduler、prompt
treatment 或兼容开关。这个复杂度是待完整复评的真实成本；当前 `hold` 不把新增代码量
解释为产品收益。

## 实际执行与停止

预注册计划为 20 对、40 arms、5 个任务、每个 variant/task 4 次。v1 只执行首对
`t1` 后，Harness 因 candidate 被计算为 `false_success` 按安全门禁停止：

| 指标 | baseline | candidate |
|---|---:|---:|
| canonical terminal | blocked | completed |
| 外部冻结 verifier | passed | passed |
| canonical receipt | 0 | 1 |
| completion rejection | 2 | 0 |
| physical API attempts | 10 | 5 |
| input + output tokens | 84,598 | 27,970 |
| known USD cost | 0.007048597 | 0.002141535 |
| wall time | 87.485 s | 31.994 s |

两 arm accounting 均 complete、sealed、billing known；总已知费用为 USD `0.009190132`。
candidate 只修改预期的 `slugify.py`，外部 verifier 在最终 workspace 通过且执行期间
workspace 未变化；路径、工具、child、时序、Standard Chat/model、请求预算和 retry
归因门禁均通过。

这一个不完整 pair 中 candidate 请求、Token、费用和时间分别下降 50.00%、66.94%、
69.62% 和 63.43%。由于只完成 2/40 arms 且测量口径失效，这些数值只能描述已执行事实，
不能作为产品收益结论。

## 为什么 raw `reject` 不是产品结论

candidate 的唯一直接验证失败是 `artifact_closure_mismatch`；
`final_evidence_chain_mismatch` 和 `false_success` 都由它派生。独立复核确认这是 Harness 与
生产协议的 canonical JSON 口径不一致：

- Python Harness 使用 `json.dumps(..., sort_keys=True)` 重算 artifact payload SHA；
- Rust `ToolArtifact::inline_verification` 使用 `canonical_json` 后的
  `serde_json::to_vec`；
- exact binary 的 `serde_json` 因 workspace feature unification 启用了
  `preserve_order`；
- 当前 Rust `canonical_json` 只递归 `map.iter().collect()`，没有显式排序，因此保留
  `IndexMap` 插入顺序；
- `VerificationArtifactPayload`、`VerifierSpec` 和 `VerifierStep` 的结构体字段顺序与字典序
  不同，所以两端重算 SHA/Artifact ID 必然不同，字节长度通常相同。

同一个候选 arm 已有唯一 Host-sealed receipt，说明 Rust 生产路径先按自身协议通过了 inline
artifact、observation、revision、spec 和 lineage 校验。正式结果同时证明：

- 1 次 proposal 对应唯一 Host prepared → started → committed；
- candidate、verification、receipt ID 闭合，0 rejection；
- contract、resolved、observed verifier spec 完全一致；
- `ledger_valid=true`、`lineage_valid=true`、Terminal/RunView projection 一致；
- 外部 exact verifier、修改范围、权限和 accounting 全部通过。

baseline 与 candidate 都出现相同 artifact closure mismatch，也证明它不是 candidate
独有的安全回归。因而 raw 聚合中的 `false_success=true / reject` 是评测假阳性，不能用来
声称 Host 接受了错误代码。

## 最终处置

- 不 `keep`：正式 schedule 只完成 2/40，无法证明总体成功率或效率收益。
- 不按产品安全问题 `reject`：candidate 的确定性行为、Host receipt 和权限链均成立。
- 不 `shrink`：本候选是全局完成不变量，manifest 已预注册禁止结果后挑任务子集。
- 结论为 `hold`：暂不删除 candidate，但不标记为 `keep` 或已证明保留；原始结果继续保存，
  不宣布收益，也不回滚成已证明失败。

下一独立切片必须先让 Rust `canonical_json` 真正递归排序，增加不同 key 插入顺序与
Rust/Python 固定向量测试，再从新的 clean candidate、suite ID、output path 和 position 1
完整 refreeze。v1 不得覆盖、续跑或与新结果拼样。
