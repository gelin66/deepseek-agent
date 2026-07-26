# M23-B4 Hardness fixed-Pro/high control

- 日期：2026-07-26
- production candidate：`e68d215c300f4b5eda8ab4f8bd96c61bf7cff11d`
- live admission：`8090adce37186af92322086c74c6092a90227bef`
- acquisition：`stop_incomplete_accounting`
- loss decision：`insufficient_repeated_current_loss`
- production delta：0

## 真实问题、owner 与准入

M23-B1–B3 已冻结 20 个 current Hardness task、3 个平衡 round、Hardness observer 与真实
durable-approval process continuity caller。B4 的唯一问题是从 position 1 取得
fixed `deepseek-v4-pro/high` control-only loss evidence；它不比较 treatment，也不允许
在 accounting、安全、identity 或 observer 歧义后补跑。

唯一 owner 仍是 `scripts/eval-m9b-fixed-pro-regression.py`。正式 admission 在读取
credential 前绑定：

```text
candidate revision/tree     e68d215c300f / 9443a2029c
binary SHA-256              f38dc426223b99035ba0bb0015aaa1d08f5826a3209fee08b8b59cf78f717ab8
Harness SHA-256             f91cbc688730eea016177167bf7c6840a24c33bdfdda04c046ef0faeb4687c74
schedule/task SHA-256       e8f67f51e2d / 363426bdc5a9
model/reasoning             deepseek-v4-pro / high
maximum_reruns              0
per-arm / suite ceiling     USD 0.50 / USD 30.00
```

offline freeze/self-test、B2 observer、B3 process continuity、M14/M16/M23-A
conformance、focused、targeted SIGKILL recovery、fmt、workspace strict clippy/test 与
`git diff --check` 均在 admission 前通过。Key 当时只检查 ignored regular file、0600 和
size，内容尚未读取。admission 先作为独立 clean commit 落地，之后 formal runner 才按
显式参数读取 Key。

## 正式采集

campaign 按冻结 schedule 执行到第 6 个 task 后自行停止：

| completed arm | task | behavior | accounting | cost USD |
|---:|---|---|---|---:|
| 0 | `rust_router_localization` | verified success | complete | 0.019893449 |
| 1 | `typescript_route_localization` | verified success | complete | 0.013402553 |
| 2 | `python_config_crossfile` | verified success | complete | 0.006870593 |
| 3 | `rust_line_recovery_resume` | verified success | complete | 0.014789797 |
| 4 | `python_jsonl_runtime` | verified success | complete | 0.014667649 |
| 5 | `readonly_service_graph` | verified product failure | usage incomplete | 不作成本结论 |

前 5 个完整 arm 合计：

```text
verified success          5 / 5
false success             0
physical model requests   39
input / output tokens     535,669 / 24,743
cache hit / miss tokens   428,672 / 106,997
cost                      USD 0.069624041
wall time                 380,131 ms
```

`rust_line_recovery_resume` 完成一次 B3 定义的真实 process resume。第 6 个 task 已经
形成 canonical terminal、Store snapshot、credential-free SQLite reopen 与 external
verifier snapshot，但一个 response 以
`complete=false, usage_complete=false, usage_incomplete=true,
billing_unknown=false` 封口。runner 因 `accounting_incomplete` 在下一物理 request 前
停止；没有重发、补 mate、继续 schedule 或选择性 rerun。

因此只存在 5 个 full-utility observation。54 个未执行 arm 不是产品样本，不能计算完整
60-arm pass@1、pass^3、费用或 Token aggregate。

## 正交只读结论

新增 manifest 只让现有 trajectory-report owner 校验这一份 ignored 0600 journal；
没有第二 evaluator、credential read 或 network access。两次 canonical report
byte-identical，SHA-256 为
`b0f1fd1787c9c622146e86a121bcce87d16fee4d66c1b5ad873b557242579f0b`：

```text
canonical trajectories       6
behavior observations        6
  verified success           5
  verified product failure   1
  false success              0
accounting observations
  complete                   5
  usage incomplete           1
full utility observations    5
model requests               46
```

唯一 loss key 为
`deepseek_transport:deepseek_transport`，只出现在
`readonly_service_graph` 一个独立 task。冻结门要求同一 stable owner/cause 至少跨两个
独立 task；所以结果是 `insufficient_repeated_current_loss`，candidate 为 `null`。
accounting stop 没有被伪装成行为失败，而 independently closed 的产品失败也没有因
usage 不完整被抹掉。

## 决策与删除

M23-B control baseline 没有闭合，且当前行为观察没有重复损失。因此：

- 不运行 M23-C `high`/`max` 配对 A/B；
- 不开发 ApplicationProbe、deterministic symbol/reference localization、
  Host-derived VerifiedMilestone 或 Tool ACI；
- 不补跑第 6 个 task、不完成 mate、不从第 7 个 task 续跑；
- 保留 B1 task contract、B2 observer、B3 continuity caller、ADR-0011 truth 和只读
  analyzer，因为它们仍是可复现 Harness 能力；
- 保留必要 manifest、summary 与 ignored 0600 raw 作为不可变审计证据；
- production 保持 fixed root/Writer Pro-high、read-only Flash-high、typed recheck
  Pro-max、唯一 AgentRuntime/RuntimeEvent/RunStore、canonical tools 与 Host
  latest-revision completion。

这是 M23 的 evidence-driven stop，不是 transport、reasoning 或工具 treatment 的收益
结论。没有 production branch 需要 cutover 或删除；实验从未创建 production candidate。

## 身份、安全与本地边界

```text
admission manifest SHA-256   e0f7b730efd1895ad685aaf870f752340255d08258b2eb358e7578588a8ff11e
analysis manifest SHA-256    918599a204cd1d24a9bc4093e9a43bc0ee98313171ba6016bae0662e1297189c
raw bytes                    9,747,100
raw SHA-256                  9c988b5079b9d6a634afc45ea03104d9af059d8405657fa80734b816a70c80a7
raw mode                     ignored 0600
raw records                  41, no partial tail
```

Key 未打印、未提交、未写入 raw；只读 analyzer 明确报告 credential/network/raw
prompt/reasoning/tool content/evaluation id 输出均为 false。没有访问 GitHub，没有 push
或 release。release binary 和临时报告在决策提交后精确删除；ignored 0600 raw 保留。

## 官方协议复核

复核日期：2026-07-26。

- [DeepSeek change log](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 与当前模型 identity](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

当前 production 继续使用 `https://api.deepseek.com/chat/completions`、
`deepseek-v4-pro/high` 与 official OpenAI-format ChatCompletions。2026-07-24 退役的是
legacy `deepseek-chat` / `deepseek-reasoner` aliases，不是 ChatCompletions 接口。

## 非结论

本结果不证明：

- 20-task/60-arm Hardness control baseline 已完成；
- 5 个成功样本足以形成总体 pass@1、pass^3 或质量/费用基线；
- 单个 read-only transport failure 应由 DeepSeek transport、工具、定位、milestone、
  reasoning `max` 或新生产机制修复；
- `max` 优于、劣于或等同 `high`；
- production 需要 Auto、FIM、RepoGraph、第二 Provider/Runtime/Store、默认 swarm 或
  multi-Writer；
- 任何费用、Token 或 wall-time 改进已被证明。
