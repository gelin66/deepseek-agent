# M32 current Hardness regression

- 日期：2026-07-27
- regression anchor：`bfa8ec84fea0253927dad74d16e1c7a1aa06a9b2`
- production candidate：`994e36d7bdfab57e47af4928b07f8c506ddd7b3a`
- live admission：`7b8e65b71c04a69283d4dcfa2bdef57c1b208570`
- acquisition：`stop_incomplete_accounting`
- loss decision：`insufficient_repeated_current_loss`
- treatment：`not_admitted`
- production delta：0

## 问题、合同与准入

M31 只在两个原 loss task 上证明 exact TaskContract verifier grant 接管有效。M32
因此从 M31 clean checkpoint 建立一轮新的 position-1 control-only regression：
仅通过 SHA-256 继承 M23 已冻结的 fixture、reference patches、tasks 和 tool policies，
不继承 M23/M30/M31 的 binary、raw、admission、停止位置或历史 label。

唯一 owner 是当时的 corrected
`scripts/eval-m9b-fixed-pro-regression.py`。正式 admission 在读取 credential 前绑定：

```text
candidate revision/tree     994e36d7bdfa / 0a5e0f20e4d9
release binary SHA-256      7ed974b3927e446468d5d248c1dd76dfb16cb4d2f8bbeb6f0f7b88e1b81d0e5f
acquisition Harness SHA     59b5b26bfc0c68d942224dd36a71adef9323ac55b11615b4d8683178425539a9
schedule/task SHA-256       621efff3ea6f / 7d71a8597adc
model/reasoning             root/Writer deepseek-v4-pro / high
ordinary read-only route    fixed actor profile
runs per task / reruns      1 / 0
per-arm / suite ceiling     USD 0.50 / USD 10.00
```

候选相对 M31 anchor 的唯一 `crates/` delta 是一个 hash-bound process acceptance
timing assertion：它改为读取 canonical terminal receipt 的 Runtime duration，不改变
release behavior。20-task materialization、17 个 reference solution、3 个 safety
counterexample、journal crash windows、真实 `request_user_input` SIGKILL/reopen、
M14/M16/M23 conformance、focused、fmt、strict workspace Clippy/test、中英文 PTY、
surface parity、public checker 与 `git diff --check` 均在 admission 前通过。

Key 当时只检查 ignored、regular、non-symlink、0600；内容尚未读取。prepared admission
还单独证明 `live_api_admitted=false` 时会在 credential read、raw creation 和 network
之前返回 `live_admission_invalid`。用户随后明确授权本次 M32、`$10` suite ceiling，
独立 admission commit 完成后 formal runner 才读取 Key。

## 正式采集与停止

campaign 按冻结顺序闭合 6 个 accounting-complete arm：

| position | task | behavior | cost USD |
|---:|---|---|---:|
| 1 | `rust_router_localization` | verified product failure | 0.018632326 |
| 2 | `typescript_route_localization` | verified success | 0.009923684 |
| 3 | `python_config_crossfile` | verified success | 0.008659197 |
| 4 | `rust_line_recovery_resume` | verified success | 0.010984446 |
| 5 | `python_jsonl_runtime` | verified success | 0.017143930 |
| 6 | `readonly_service_graph` | verified success | 0.010074919 |

第 4 个 long-horizon arm 实际经过
`request_user_input -> SIGKILL -> SQLite reopen -> resolve -> continue`，reopen 时物理
request 保持不变，最终 latest-revision verifier、terminal 和 accounting 均闭合。

第 7 个 `writer_envelope` 在 root 首请求得到 typed、retry-safe、pre-header
`deepseek_timeout`。没有 content、reasoning 或 tool fragment，没有 Runtime retry、
transport retry、child 或 workspace side effect。canonical terminal、Store snapshot、
credential-free SQLite reopen 与 external verifier snapshot 已保存；ModelAccounting 为：

```text
root physical attempts   1 started / 1 completed / 0 in flight
response headers         false
usage complete           true
billing unknown          true
complete                 false
sealed                   true
maximum_reruns           0
```

Harness 因 `accounting_incomplete` 在 position 8 前停止。没有重发 position 7、继续
后十三项、补 mate、选择性 rerun 或把旧 raw 拼入结果。

只对前 6 个 accounting-complete arm 记录测量总量：

```text
verified success             5
verified product failure     1
correct safety rejection     0（safety strata 尚未执行）
false success                0
physical model requests     52
input / output tokens        696,290 / 29,992
cache hit / miss tokens      595,712 / 100,578
known cost                   USD 0.075418502
wall time                    474,669 ms
```

这些数值不是 20-task pass@1、完整 regression、safety aggregate 或 suite cost。
position 7 与十三个未执行任务不进入质量、成本或 full-utility aggregate。

## 正交 trajectory 结论

只读 analysis manifest 绑定 ignored 0600 raw 的 exact hash/size。M32 临时 consumer
删除前，trajectory-report 两次生成 byte-identical canonical JSON，SHA-256 均为
`b6e780949dd9d88c233e6bfee8a07a8f003f065986d63d75012c4f90bbada6d8`：

```text
canonical trajectories        7
completed arm results         6
accounting aborts             1
behavior observations
  verified success            5
  verified product failure    1
  invalid                     1
  false success               0
accounting observations
  complete                    6
  billing unknown             1
full utility observations     6
```

position 7 同时为 `billing_unknown` 与 `route_identity_mismatch`，只作为停止/invalid
fact，不授权 production work。唯一 accounting-complete loss 是：

```text
rust_router_localization
root_task_outcome:deterministic_verifier_failed
```

它只出现在一个 task id；没有达到两个独立任务的候选阈值。结果因此是
`insufficient_repeated_current_loss`，不是 `next_candidate_audit_required`。

M31 原 loss task `rust_line_recovery_resume` 在 current binary 上已经 verified success，
说明 exact grant 在这一个新 observation 上没有回退；另一个原 loss task
`rust_netstring_recovery_resume` 未执行，因此不能把 partial M32 外推为完整 M31 broad
regression safety。

## 决策与删除

M32 以 `stop_incomplete_accounting` 收口：

- 不把 partial acquisition 宣称为 `no_repeated_current_loss`；
- 不从一个独立 loss 或 billing-unknown Writer observation 准入 treatment；
- fixed actor route、exact TaskContract verifier grant、唯一 AgentRuntime/RunStore、
  canonical tools 与 Host latest-revision completion 均不改变；
- 不开发 Auto、第二 Provider/Runtime/Store、默认 swarm/multi-Writer、FIM 或 RepoGraph；
- 若未来仍需完整 current Hardness baseline，必须由新 Goal 从新 identity 重新冻结，
  不能续跑本 journal。

临时 `--campaign m32` manifest loader、aggregate、preflight、continuity/live runner 与
gate-only adapter 已物理删除。corrected Harness 精确恢复到 M32 前 blob
`90ffb72bbd830ec7e1e66c685768bea37b37e0c3`；M30 self-test 仍得到冻结输出 SHA-256
`849446b67c7ed072ae83145a0fabe9362f9b4e9ead72a0bd628ccc03a0cbb512`。
frozen contract/admission/analysis、ignored 0600 raw 与本 summary 保留作审计证据。

## 身份、安全与本地边界

```text
contract manifest SHA-256    3210430a0832e8f9c7e5028e625ed2a2795847e8328370ff4a27a40bfcf898cd
admission manifest SHA-256   bb46cfae0491cb9d7c36273e9deef4f3f483688eba4ba642b325d21360e90687
analysis manifest SHA-256    6167201f24f81fa5effa0230d94b69dbf9bfc70e330d20ed318798829c30f5dc
raw bytes                    11,530,869
raw SHA-256                  4f7ed8b09122ab613b31e2b1b1b4b108df85df1f69a257aec2c478f57b914521
raw mode                     ignored 0600
raw records                  47, no partial tail
```

Key 未打印、未提交、未写入 raw。只读 report 明确记录 credential/network/raw
prompt/reasoning/tool argument/tool content/evaluation ID 输出均为 false。全程没有访问
GitHub，没有 push 或 release。

## 官方协议复核

复核日期：2026-07-27。

- <https://api-docs.deepseek.com/updates/>
- <https://api-docs.deepseek.com/news/news260424/>
- <https://api-docs.deepseek.com/quick_start/pricing/>
- <https://api-docs.deepseek.com/api/create-chat-completion/>
- <https://api-docs.deepseek.com/guides/thinking_mode/>
- <https://api-docs.deepseek.com/guides/tool_calls/>

production 仍只使用 official OpenAI-format ChatCompletions、
`https://api.deepseek.com/chat/completions`、固定 actor route 和 canonical tools。
2026-07-24 退役的是 legacy model aliases，不是 ChatCompletions。

## 非结论

本结果不证明：

- 20-task current Hardness regression、总体 pass@1 或 safety strata 已闭合；
- M31 grant 在所有 Hardness strata 上无回归；
- position 7 已知收费或未收费；
- partial 六项可形成总体成本、Token、wall-time 或 release quality aggregate；
- `rust_router_localization` 的单项 loss 已跨独立任务重复；
- 新 production treatment、reasoning `max`、Auto、FIM、RepoGraph、第二
  Provider/Runtime/Store、默认 swarm 或 multi-Writer 有收益。
