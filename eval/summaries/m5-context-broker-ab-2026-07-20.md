# M5-B evidence-aware ContextBroker 正式 A/B

> 日期：2026-07-20
>
> 状态：`shrink`；保留 ContextBroker 与硬上限安全压缩，删除主动压缩产品面
>
> 范围：`5fe7db82..da575da2`、官方 DeepSeek、2 个长任务、3/cell、12 对

## 1. 结论

正式评测没有证明主动 compaction 是一项有效的 DeepSeek 效率优化：

- candidate 的 compaction on/off 都是 `6/6` verified success，0 false-success；
- candidate on 相对 off 的请求数 6/6 对完全相同；
- Token 6/6 对下降，平均 `-8.743%`、中位数 `-9.083%`；
- 费用却 6/6 对上升，平均 `+7.995%`、中位数 `+5.768%`；
- 时间 3 对更快、3 对更慢，平均 `+2.547%`、中位数 `+0.189%`；
- verified success 增量为 0。

这说明主动裁剪虽然减少了发送 Token，却破坏了 DeepSeek 自动前缀缓存的经济性；不能把
“Token 更少”直接当成“更高效”。

同时，旧 baseline 的模型摘要路径不能恢复：baseline compaction on 在两个任务合计
`0/6`，off 为 `4/6`；稳定 marker 从 off 的 `9/9` 降为 on 的 `4/9`。candidate on 则
`6/6` 成功，并在 Task B 的三次有效运行中精确保留 inherited failure facts。正确结论不是
删除 ContextBroker，而是只保留：

```text
每次请求 -> evidence-aware ContextBroker
预计输入超过 Host 派生 hard input limit -> 同 run 确定性压缩
压缩后仍超 hard limit -> typed fail closed
```

提交 `e2c870b0` 因此删除手动 `/compact`、HTTP/stdio Compact、独立 compaction root、
90% 提前阈值及其特殊协议/状态/UI，只保留 root/child 共用 Runtime 内的硬上限 preflight
压缩。该收缩净删 `1,074` 行，不增加模型调用、工具、依赖、配置或持久真相。

## 2. 身份与可复现边界

| 项目 | 值 |
|---|---|
| schema | `codewhale.eval.m5-context-broker.v2` |
| mode | `formal_ab` |
| model | `deepseek-v4-flash` |
| baseline revision | `5fe7db829fdf35e6152db0604d906e01ce22ded8` |
| candidate revision | `da575da2d7f297f64a0cf1467e6d038c6dcddd84` |
| post-decision shrink checkpoint | `e2c870b0`（Run API v6 / RuntimeEvent v9 / State v13；不属于 live A/B 被测 candidate） |
| baseline binary SHA-256 | `947c7a76cd12aabe533b0c08b8c92e6d6a0f3a31a6ecfd4f27acb8999032b21c` |
| candidate binary SHA-256 | `d727dac4d425f98aa937ffd6894675db7f67a959c45b3c3ab36bffacb6bc1e58` |
| plan SHA-256 | `07f85703cd0c489dd61e80ede925bc20d6550667a28131f552958ecbbda22d48` |
| verifier SHA-256 | `8049e41f2847db8ff1ef2681d076c39ce776cdff0551f368354d58c75d6fa841` |
| schedule | 2 task × 2 revision × on/off × 3；12 对 / 24 treatment arms |
| accepted pairs | 12 |
| total pair attempts | 19 |
| invalid technical attempts | 7 |
| suite duration | 2,565.948 秒 |
| raw result | `m5-context-broker-5fe7db82-vs-da575da2-v2-20260720T122006Z.json`（本地忽略，`0600`） |
| raw SHA-256 | `ddc1f2395a7149b40dd406a10af0b0413a1fb9e3a2e709ba261fc2b9f43a1f63` |

两个冻结二进制均在正式评测前从精确 revision 使用 `CARGO_INCREMENTAL=0` 构建，再记录
版本与二进制 SHA。评测完成后的磁盘清理已删除这些可再生二进制和 `target/`，没有删除
raw result；以后若重跑正式评测，必须重新从相同 checkpoint 构建并记录新二进制 SHA，
不能假定字节与旧构建相同。Key 只从权限 `0600` 的本地文件传入子进程环境，未进入 argv、
结果、摘要或 Git。

## 3. Cell 结果

| revision | task | compaction | verified | observed | false-success |
|---|---|---|---:|---:|---:|
| baseline | A | off | 2/3 | 2/3 | 0 |
| baseline | A | on | 0/3 | 0/3 | 0 |
| baseline | B | off | 2/3 | 2/3 | 0 |
| baseline | B | on | 0/3 | 0/3 | 0 |
| candidate | A | off | 3/3 | 3/3 | 0 |
| candidate | A | on | 3/3 | 3/3 | 0 |
| candidate | B | off | 3/3 accepted | 3/4 | 0 |
| candidate | B | on | 3/3 | 3/3 | 0 |

candidate Task B off 有一个真实失败 arm，其 retry 后 billing 不可知，因此没有进入三条
accepted measurement run，但进入 observed reliability；candidate 的 observed
non-regression 仍成立。评测器不会因任务失败重采样，只对 retry-backed accounting
observability gap 最多重跑整对。

candidate 合计六对的 on-minus-off：

| 指标 | mean | median | paired direction |
|---|---:|---:|---|
| API requests | 0 | 0 | 6 equal |
| input tokens | -9.592% | -9.628% | 6 lower |
| total tokens | -8.743% | -9.083% | 6 lower |
| cost USD | +7.995% | +5.768% | 6 higher |
| duration | +2.547% | +0.189% | 3 faster / 3 slower |
| verified success | 0 | — | on/off 均 6/6 |

baseline 合计六对的 on-minus-off 为 Token `-20.598%`、费用 `+21.431%`、时间
`+23.751%`、verified success `-4`。这进一步否定恢复旧摘要模型路径。

## 4. 计量边界

12 个 accepted pair 的正式产品聚合计量完整，`product_metric_eligible=true`：

- 265 个物理 API 请求；
- 2,144,293 Token；
- USD `0.111391817`；
- 149 个工具调用；
- 0 runtime retry。

若把七次因 retry 后账单不可知而作废的技术尝试也计入，整个执行只能给出已知下界：

- API 请求 `>=358`；
- Token `>=2,763,524`；
- USD `>=0.141682656`，CNY `>=1.01201884`；
- 7 runtime retry；
- 171 工具调用；
- 无未登记 pair attempt。

因此 accepted A/B 可以决定 treatment 的产品方向，但不能把整个物理执行成本伪装成精确
总额。

## 5. 复杂度与归因限制

被测 `da575da2` 相对 `5fe7db82` 的 production diff 为 `+1,435/-1,857`，净删 422 行：

- `AgentRuntime` owner 1；
- compaction owner 1；
- `RunStore` owner 1；
- 新依赖、模型调用、模型可见工具、配置、持久真相均为 0；
- 删除三个旧模型摘要 lifecycle event。

跨 revision 的可靠性变化只能归因给整个 M5-B vertical slice，不能单独归因给 compaction。
同一 candidate 内的 on/off 才能直接归因主动压缩的 Token/费用方向。任务数和样本数仍小，
这里只声称重复方向，不声称统计显著性或广泛任务泛化。

## 6. 决策

1. 保留唯一 evidence-aware ContextBroker、canonical transcript/request projection 分离、
   TaskContract/current workspace/evidence/verifier failure/child handoff 的确定性保留。
2. 保留超过基于官方 context/output capability 派生的 Host hard input limit 时的同
   Runtime 本地压缩、Store 重算验证和压缩后的 `ContextLimitExceeded` backstop。
3. 永久删除旧模型摘要请求及 prepared/in-flight/failed 协议，不添加摘要提示词或第二模型
   loop。
4. 删除手动与提前阈值压缩，不再把 compaction 宣称为 Token、成本或速度优化。
5. 不启动 RepoGraph：本评测没有把失败定位为缺少结构检索。下一阶段进入 M6
   Orchestrator/worktree 的最小垂直切片，并单独做 multi-agent A/B。

## 7. Post-decision shrink 门禁

`e2c870b0` 及上述文档同步使用仓库外 `CARGO_TARGET_DIR`、`CARGO_INCREMENTAL=0` 完成：

- `cargo fmt --all -- --check`；
- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`。

关键 focused/full 结果包括 Runtime conformance `60/60`、exec terminal `24/24`、TUI
canonical run `18/18`、真实 PTY `7/7`、State 进程级 crash/replay `15/15`（1 个 helper
按设计忽略）、TUI unit 1,346 passed（1 个按设计忽略），以及 release QA 5 passed
（1 个重型场景按设计忽略）。仓库内 `target/` 保持不存在；验收完成后仓库外构建目录也
删除。
