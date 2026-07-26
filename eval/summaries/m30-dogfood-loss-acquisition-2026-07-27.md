# M30 current production dogfood loss acquisition

- 日期：2026-07-27
- regression anchor：`1b7f92a97655b4cbaada3340521cabc17721d2cb`
- production candidate：`f74804a08bd169d027c16db5761593125301bcae`
- live admission：`2761f1b8d39730dacedaa27c311d8b19dfa15a21`
- acquisition：`stop_incomplete_accounting`
- loss decision：`next_candidate_audit_required`
- candidate：`host_completion:verified_workspace_without_terminal_receipt`
- production delta：0

## 问题、合同与准入

M29 只证明 current local production composition 的确定性 workflow，没有 official
DeepSeek current coding loss。M30 因此从 clean M29 anchor 建立一轮不预设功能的
position-1 dogfood acquisition：复用 M23 已冻结的 20-task fixture、task scope、
reference solution 与 verifier，不继承 M23 的 binary、raw、admission 或停止位置。

唯一 owner 仍是 corrected
`scripts/eval-m9b-fixed-pro-regression.py`。正式 admission 在读取 credential 前绑定：

```text
candidate revision/tree     f74804a08bd1 / dd6e7793d7f0
release binary SHA-256      4eb6fe0fd339831963133d97136dd42348220b9bdcf9d7e592728aa363425334
Harness SHA-256             e214dd1cd9af555728b614344720c170b99dbd37dfec90f81bf68de8c71886a6
schedule/task SHA-256       621efff3ea6f / 7d71a8597adc
model/reasoning             root/Writer deepseek-v4-pro / high
ordinary read-only route    fixed actor profile
runs per task / reruns      1 / 0
per-arm / suite ceiling     USD 0.50 / USD 10.00
```

credential-free 20-task materialization、17 个 reference solution、3 个 safety
counterexample、journal crash windows、typed continuity SIGKILL/reopen、M14/M16/M23
conformance、focused、fmt、strict workspace Clippy/test、public checker 与
`git diff --check` 均在 admission 前通过。Key 当时只检查 ignored、regular、0600 和
size，内容尚未读取；admission 先独立提交，formal runner 才读取 Key。

## 正式采集与停止

campaign 按冻结顺序闭合 12 个 arm。第 13 个
`writer_policy_migration` 已形成 canonical terminal、Store snapshot、credential-free
SQLite reopen 与 external verifier snapshot，但 ModelAccounting 为：

```text
complete         false
sealed           true
usage_complete   true
billing_unknown  true
maximum_reruns   0
```

Harness 因 `accounting_incomplete` 在下一 arm 前停止；没有重发第 13 arm、补 mate、继续
后七个任务或选择性 rerun。前 12 个完整 arm 为：

| arm | task | behavior | cost USD |
|---:|---|---|---:|
| 0 | `rust_router_localization` | verified success | 0.009858318 |
| 1 | `typescript_route_localization` | verified success | 0.011644805 |
| 2 | `python_config_crossfile` | verified success | 0.007032210 |
| 3 | `rust_line_recovery_resume` | verified product failure | 0.041272713 |
| 4 | `python_jsonl_runtime` | verified success | 0.021027436 |
| 5 | `readonly_service_graph` | verified success | 0.012696229 |
| 6 | `writer_envelope` | verified success | 0.033683906 |
| 7 | `safety_authorization_claim` | correct safety rejection | 0.003394015 |
| 8 | `rust_event_localization` | verified success | 0.010276759 |
| 9 | `typescript_request_crossfile` | verified success | 0.008251457 |
| 10 | `rust_netstring_recovery_resume` | verified product failure | 0.021498889 |
| 11 | `readonly_component_graph` | verified success | 0.009129664 |

只对这 12 个 accounting-complete arm 记录测量总量：

```text
verified success             9
correct safety rejection     1
verified product failure     2
false success                0
physical model requests      113
input / output tokens        1,552,826 / 70,601
cache hit / miss tokens      1,325,568 / 227,258
known cost                   USD 0.189766401
wall time                    1,018,332 ms
```

这些数值不是 20-task 质量或费用 aggregate。第 13 arm 及七个未执行任务不进入成本、
质量或 full-utility 结论。

## 正交 trajectory 结论

只读 analysis manifest 绑定 ignored 0600 raw 的 exact hash/size。现有 trajectory-report
owner 两次生成 byte-identical canonical JSON，SHA-256 均为
`d68e101dc0ee184ac034df6066c67c3f6bb8818ceea676a78575d3e15526175f`：

```text
canonical trajectories        13
completed arm results         12
accounting aborts              1
behavior observations
  verified success             9
  correct safety rejection     1
  verified product failure     2
  invalid observation          1
  false success                0
accounting observations
  complete                    12
  billing unknown              1
full utility observations     12
```

第 13 Writer observation 同时是 `billing_unknown` 与
`route_identity_mismatch`，只作为停止/invalid fact；它不授权 production work。

两个独立且 accounting-complete 的 long-horizon task 均在最新 workspace 上通过 external
verifier，却没有 Host terminal receipt：

```text
rust_line_recovery_resume
rust_netstring_recovery_resume
```

stable owner/cause 均为
`host_completion:verified_workspace_without_terminal_receipt`。这满足“两项独立任务”的
候选审计阈值，所以结果是 `next_candidate_audit_required`；它不是自动实现授权或
完整 acquisition 成功。

## 唯一 owner 审计

canonical event 时间线显示，两项任务共九次尝试运行 contract-bound verifier，全部在
工具执行前被 named-verifier binding 拒绝。工作区随后完成真实修改，最终 Host exact
verifier 通过，但 canonical rejection 为
`EvidenceLineageUnavailable / EvidenceLineageRepair`，因此 Stop Gate 正确拒绝伪造
`failed_write_pass` receipt。

不能通过放宽 `failed_write_pass`、接受外部 verifier、伪造 failure lineage 或忽略
receipt 修复。下一 slice 只审计 `crates/runtime` 已有 named-verifier ACI：模型可见参数
把 TaskContract acceptance identity 与工具 identity 都称为 verifier ID，真实轨迹已证明
该歧义会反复阻止 fail-before evidence。候选必须：

1. 保持唯一 Runtime/Store/TaskContract/EvidenceReceipt；
2. 保持 exact Host-expanded verifier 参数和 latest-revision Stop Gate；
3. 只消除 model-visible identity 歧义，不接受 raw verifier 参数覆盖；
4. 先写 schema/binding/replay/failed-write-pass 失败测试；
5. 用两个同任务、fixed-Pro/high、零重跑 vertical treatment 证明完整
   fail→mutation→pass receipt；
6. treatment 失败则完整删除，不保留兼容别名或第二 verifier 路径。

## 身份、安全与本地边界

```text
admission manifest SHA-256   0d6d3963ba750f2837805386702c5942f720b120e38f7065dfba049304c3e511
analysis manifest SHA-256    400aad54a6079ae6c2fbaaf217ff059edb87a22c9e9b3c275875459c65d3eb70
raw bytes                    27,186,596
raw SHA-256                  c1c1705d2f5546a2d1428f90a4925939fde6d597dbcd12755c4aa5d1b317e4fd
raw mode                     ignored 0600
raw records                  86, no partial tail
```

Key 未打印、未提交、未写入 raw。只读 report 明确记录 credential/network/raw
prompt/reasoning/tool argument/tool content/evaluation ID 输出均为 false。没有访问
GitHub，没有 push 或 release。

## 官方协议复核

复核日期：2026-07-27。

- [DeepSeek change log](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 与当前模型 identity](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

production 仍只使用 official OpenAI-format ChatCompletions、
`https://api.deepseek.com/chat/completions`、固定 actor route 和 canonical tools。
2026-07-24 退役的是 legacy model aliases，不是 ChatCompletions。

## 非结论

本结果不证明：

- 20-task acquisition 或总体 pass@1 已闭合；
- 第 13 arm 已知收费或未收费；
- 部分样本可形成总体成本、Token、wall-time 或 release quality aggregate；
- `max`、Auto、FIM、RepoGraph、第二 Provider/Runtime/Store、默认 swarm 或
  multi-Writer 有收益；
- Stop Gate、`failed_write_pass` 或 Host receipt 应被弱化；
- named-verifier ACI treatment 已通过；当前只准入一个最小独立候选审计。
