# M23-B2 Hardness metric / continuity observer

- 日期：2026-07-26
- 起点：`bcbb2a91ebfb248a9ff6081feb6f254cd0ab9441`
- 决策：`keep_offline_hardness_metric_observer_live_continuity_pending`
- production delta：0
- Key / official API / external network / historical raw：0 / 0 / 0 / 0

## 真实问题与唯一 owner

M23-B1 冻结了 20 个 Hardness task 和指标名称，但 corrected Harness 还不能从 canonical
trajectory 执行这些指标，也没有独立的中途 continuity truth。原有每 arm 终态后的
credential-free SQLite reopen 只能证明 Store exactness；如果把它算成
`resume_count=1`，会把没有继续执行的审计动作包装成长任务恢复能力。

唯一 owner 继续是 `scripts/eval-m9b-fixed-pro-regression.py`。本切片不修改
AgentRuntime、RuntimeEvent、RunStore、DeepSeek sender/accounting、prompt、route、
completion 或 canonical tools。

## 可执行指标契约

observer 从 canonical RuntimeEvent envelope 的持久时间戳和 typed event 派生：

| 指标 | 唯一事实 |
|---|---|
| first relevant file / files before edit | successful read/search outcomes + frozen project file inventory |
| first-edit verified | first applied edit 后、下一次 edit 前的 deterministic verifier success |
| repair loops | failed verifier 后的下一次 applied edit |
| repeated reads | 同 actor、同 mutation epoch、同 tool+argument identity |
| compaction count | `context_compaction_committed` |
| runtime assertion / service start | task 冻结的 exact external verifier |
| goal constraint loss | actor/route/scope、temporal evidence 与 required continuity |

报告只保留计数、时延和稳定布尔值，不输出 prompt、reasoning、tool argument/content 或文件
路径明细。reference changed files 仍只是诊断；`allowed_paths` 才是修改安全边界。

## Continuity 真相

一次 resume 必须同时满足：

1. 运行在中途 `interaction_requested` durable checkpoint 停住；
2. 新进程重开所得 event prefix 与停止前 byte-exact；
3. 前后 process identity 不同；
4. 重开时 physical request count 不增加；
5. interaction 只在重开后解析，随后才继续；
6. 该记录明确不是 terminal snapshot audit。

缺少或伪造任何一个条件都不计 resume；task 要求 continuity 时同时产生
`goal_constraint_loss=true`。这保留“crash/reopen 不重复请求或副作用”的产品边界。

## 离线证据

4-case corpus 覆盖：

- fail-before、重复 read、compaction、首改通过和真实中途 resume；
- 首改失败、verifier failure 后第二次 edit 恢复；
- 把终态 SQLite reopen 冒充 resume 的负向反例；
- 由冻结 loopback verifier 证明 runtime assertion 与 service start。

结果：

```text
cases                                  4 / 4
real mid-run resume                    1
expected goal-constraint loss          1
runtime assertion case                 1
terminal SQLite reopen counted resume  0
report regeneration                    byte-identical
Key / API / network / historical raw   0 / 0 / 0 / 0
```

M23-B1 self-test/freeze report、M14 observer、M16 acceptance-equivalence 和 M23-A
truth conformance 均继续通过。

## 替代、删除与停止门

本切片替代了“只冻结指标名字”和“可以从终态 reopen 猜 resume”的空白/错误前提。
M23B formal caller 目前在 binary、credential 和 output claim 前以稳定
`m23b_live_continuity_not_implemented` fail closed。

live Harness 尚未实现 interaction checkpoint 的 stop/reopen/resume caller，也尚未把
这些 metric 接入 arm result 与 aggregate。因此本切片没有 fixed-Pro trajectory、pass@1、
pass^3、false-success 或 stable owner/cause loss matrix；它不授权读 Key、发 API 请求、
进入 M23-C high/max 或开发 M23-D 四个 candidate。

下一独立切片必须先完成 live continuity lifecycle、process-level crash/reopen、metric
arm/aggregate 接线和离线门禁。失败时删除该 live candidate，保留本 observer 的
credential-free truth；不得退回 terminal-reopen-as-resume。

## 门禁与身份

提交前要求：

```text
python3 -m py_compile scripts/eval-m9b-fixed-pro-regression.py
M23B --hardness-conformance x2 + byte comparison
M23B --self-test
M23B --freeze-report
M14 / M16 / M23-A conformance
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

最终身份：

```text
Harness             sha256:792808e6b880c68d9611ae176a88553512516e1a85f1cefca6f234f1153aa77f
observer manifest   sha256:dd5c29d556e61a7fad7511c4cd6f592955b16c63d94049be70abaf61889548ec
observer corpus     sha256:0cb5372c93a3751864a7d94ab4fdd3f6a5a3117053da2922147d40d556302ed0
canonical results  sha256:0f661d77ae67c587c1338d13eb2f824f1fc2bc8ce64d561eb33ce963e9892f95
serialized report   sha256:f22dca61a8a657546176c1d43d5fb440a5f952ea7ca0542a8325ac9b547acd19
```

全部上述门禁通过。第一次 workspace test 中一个无 Rust diff 的 stream-stall 时序用例瞬时
投影为 retry-open transport error；该用例立即单独复跑通过，随后完整 workspace test
再次通过（包括 25/25 exec terminal acceptance）。formal M23B 入口也在 credential、
binary 和 output claim 前以 `m23b_live_continuity_not_implemented`、`key_accessed=false`、
`network_accessed=false` 精确停止。
