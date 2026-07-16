# M3 Headless AgentRuntime 真实编码 A/B（2026-07-16）

> 状态：M3 `codewhale exec` 垂直切片通过；这是一个冻结 Python 任务的 single-lane
> 最小真实 smoke，不是任意仓库或多 Agent 成功率结论。

## 1. 可复核身份

- Evaluation ID：`deepseek-exec-ab-f8ac6974c20f403282adeaca57563f3c`
- 任务：`python-coalesce-ranges-v1`
- 模型：`deepseek-v4-flash`，reasoning effort `high`
- 本地原始结果：
  `eval/results/m3-runtime-single-ab-3274be473031-20260716T033637Z.jsonl`
- JSONL SHA-256：`938d042d7ccaeed486896bade102012f924bb25fdc020cdae2c92f1827da8d11`
- Harness commit：`54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3`，运行时工作树有已记录改动；
  Harness SHA-256：`sha256:ef97a5e87b7ee76bfb76fc4321785317979de4526f660833c735c6c7a1cdbc97`
- fixture SHA-256：`sha256:c97ccfbcdb2d7f56ae97a751520e570249b2e8f5be7eb60685a7eeb0df4f6b48`
- verifier SHA-256：`sha256:4c12392f0b71533415feeb40a309bbdf134c85ac92e92882bd20df2b21c75886`

Baseline：

- revision：
  `worktree-head-54fb7cb9bcd0-tree-78fa12559218fcf899b52409dcee8d1ddfe27073`
- dispatcher SHA-256：`sha256:a71c4ab303e467219942ce2d99c9bf3dd50b421ac1a4889360e6eea1de24df1b`
- runtime SHA-256：`sha256:bc8fc334d9ef1ff69fb80bea9bbce98acc76b26d32542b5fc230fa3a3c11b088`
- binary-pair SHA-256：`sha256:132e98984c2954ba72648c11ebe6f6c7ac6136915d689273e66dd6a7ad47a931`

Candidate：

- revision：
  `worktree-head-54fb7cb9bcd0fd613cf417b683b0b3bdbe190bd3-source-3274be47303193dd81b0ea0b8d26801a80ca1780abbaacd62c606cd3a3f2a54c`
- dispatcher SHA-256：`sha256:85a5dacdcfdfe72f6cdcfef78a712826e0ab70903e095e1a4a92451b605d646b`
- runtime SHA-256：`sha256:1f7023eb980df9933fc63325f3efe89f33e2ce07fa2e2b891d244a01b39bc996`
- binary-pair SHA-256：`sha256:610cafea04551a215efaa14a4589e1422bc4ff816eebf1d69ef3c3fbdc0ea35f`

原始 JSONL、模型流和本地运行状态按仓库规则保持忽略；本汇总只保存脱敏身份、逐 run
度量、断言结果和解释边界，不保存 Key、模型正文、reasoning、工具参数或工具输出。

## 2. 逐 run 结果

| Variant | 重复 | verified | false-success | 请求 | 总 Token | Wall time | 费用 | 模型侧精确命令计数 | Host verifier |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 1 | true | false | 5 | 43,245 | 14,079 ms | `$0.0012144440` | 1 | pass/stable |
| candidate | 1 | true | false | 5 | 25,913 | 13,096 ms | `$0.0007536032` | 1 | pass/stable |
| candidate | 2 | true | false | 5 | 24,707 | 9,393 ms | `$0.0004224136` | 0 | pass/stable |
| baseline | 2 | true | false | 5 | 43,464 | 11,938 ms | `$0.0007873824` | 0 | pass/stable |
| baseline | 3 | true | false | 4 | 33,690 | 10,785 ms | `$0.0006266344` | 0 | pass/stable |
| candidate | 3 | true | false | 5 | 26,639 | 12,228 ms | `$0.0005867176` | 0 | pass/stable |

六次运行均满足：

- Runtime terminal 为 `completed/resolved`，process exit 0，无 timeout/spawn error；
- stream、single-lane、执行预算、测量和 claim acceptance 契约通过；
- 冻结 Host verifier 在最终 `workspace_revision` 上通过，验证期间工作区保持稳定；
- 唯一变更文件为 `ranges.py`，fixture、verifier 与冻结 binary pair 摘要匹配；
- usage 与费用完整，Runtime 报告和 Harness 按官方价格快照复算一致；
- 请求全部结算、in-flight 为 0；candidate 的 root/child actor 请求账本有效且 child 为 0。

`verification_runs` 是模型侧 `exec_shell` 命令形状遥测，不是 Host receipt。Candidate 第 2、
3 次虽有成功 shell 调用，但不满足 Harness 对精确命令字符串的识别，因此记录为 0；不能据此
声称模型执行了指定命令，也不能用它否定随后在最新修订上通过的冻结 Host verifier。

## 3. Cell 与差值

两个 cell、single lane comparison 和 evaluation summary 均为
`product_metric_eligible=true`、`evidence_complete=true` 且 eligibility reasons 为空。
逐 run 按规则固定为 `product_metric_eligible=false`，因为单次样本永远不是产品指标。

| single lane（每 cell 3 次） | pre-M3 baseline | M3 candidate | candidate - baseline |
|---|---:|---:|---:|
| verified success | 3/3 | 3/3 | 0 |
| false success | 0 | 0 | 0 |
| 输入 Token 总量 / 均值 | 117,030 / 39,010 | 74,064 / 24,688 | 均值下降 |
| 输出 Token 总量 / 均值 | 3,369 / 1,123 | 3,195 / 1,065 | 均值下降 |
| 总 Token 总量 / 均值 | 120,399 / 40,133 | 77,259 / 25,753 | `-35.83%` |
| 请求总量 / 均值 | 14 / 4.67 | 15 / 5.00 | `+7.14%` |
| Wall time 总量 / 均值 | 36,802 / 12,267 ms | 34,717 / 11,572 ms | `-5.67%` |
| 费用总量 / 均值 | `$0.002628461` / `$0.000876154` | `$0.001762734` / `$0.000587578` | `-32.94%` |

整套 6 次运行使用 29/60 个允许请求，墙钟 73,334 ms，费用 `$0.004391195`，低于
`$0.15` suite 上限。严格离线断言重新读取 JSONL，并要求 6 个 run 的终态、Host evidence、
stream/lane contract、usage/cost、请求结算和 frozen binary identity 全部成立；断言退出 0。

## 4. Treatment 与可归因范围

这是 bundled vertical-slice treatment。评测任务、模型和预算相同，但 production tool
catalog hash 发生有意变化；因此成功率不退化和 Token/成本下降只能归因给整个 M3 候选，
不能拆成“Runtime 单独带来 35.83% Token 降幅”。

本次结果只能证明：

1. 新 `AgentRuntime` 的 production `exec` 能在受控预算内稳定完成这个基础编码任务；
2. Host 没有接受错误终态或不稳定工作区，三次 candidate false-success 均为 0；
3. 对这个任务，M3 候选没有降低成功率，并以更多 1/3 个请求换取较少 Token、时间和费用。

不能证明：

- 任意仓库、语言或任务类型的成功率；
- TUI、app-server、crash/resume 或多 Agent 已经迁移；
- Token 降幅由 Runtime、提示词或工具目录中的哪一项单独造成；
- M1 导入基线 A/B、M2 全 surface production canary 或后续 M5/M6 退出门槛已经完成。

## 5. 复杂度与本地门禁

相对冻结 pre-M3 source，排除忽略的本地运行状态：

- Rust/Cargo 按文件路径 `numstat`：非 test 路径 `+6,583/-3,595`，test 路径
  `+1,924/-194`；新文件另含 812 行内嵌 `#[cfg(test)]`，因此生产新增上界为 5,771 行，
  测试新增下界为 2,736 行；
- 新增 34 个 protocol 与 11 个 runtime 公共 struct/enum；新增持久化真相 0；
- 新增第三方长期依赖 0，只增加 `tui -> codewhale-runtime` 工作区依赖；
- `exec` 从一条旧 Agent loop 替换为一条新 loop，净新增并行生产路径 0。

当前工作树重新通过：

- `./scripts/dev-deepseek-agent.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- `cargo test -p codewhale-runtime --locked`：14/14 conformance；
- `cargo test -p codewhale-tui --locked --test exec_terminal_acceptance`：18/18；
- `git diff --check`。

macOS linker 的 `__eh_frame section too large` 仍是已知 warning，不是测试失败。

## 6. 决策

接受 M3 `codewhale exec` 最小垂直切片，下一 Goal 进入 M4-A Headless 状态真相。广泛任务集、
多 Agent A/B 和产品级成功率继续作为后续里程碑证据，不能从这个 single-lane smoke 外推。
