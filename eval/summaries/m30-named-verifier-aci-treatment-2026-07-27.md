# M30 named-verifier ACI treatment

- 日期：2026-07-27
- acquisition control candidate：`f74804a08bd169d027c16db5761593125301bcae`
- treatment candidate：`d0d509b6e3ddecf079c968c50a838e6516047010`
- treatment Harness checkpoint：`e8432a0c302a38665da353c84bf083418e0eb695`
- live admission：`a3fc81a6b`
- decision：`reject_and_delete_named_verifier_aci`
- production cutover：treatment 与临时 consumer 已删除

## 问题与单变量合同

M30 acquisition 在两个独立、accounting-complete 的 long-horizon task 上重复观察到：
latest workspace 通过 external verifier，但 canonical Run 缺少 Host terminal receipt。
acquisition audit 将九次 pre-execution named-verifier rejection 归因到模型可见的
TaskContract acceptance identity / tool identity 歧义。

候选只修改 `crates/runtime` 生成的 named-verifier description：

- `verifier_id` 必须逐字填写 frozen TaskContract acceptance ID；
- `verifier_id` 不是 advertised `run_verifiers` 工具名；
- schema enum、resolver、Host-expanded parameters、RuntimeEvent、State schema、
  RunStore replay 和 latest-revision completion gate 不变；
- 不增加 alias、raw parameter override、compatibility reader 或第二 verifier path。

预注册 keep 门要求两个原 loss task 各执行一次 fixed-Pro/high treatment，
`maximum_reruns=0`，并且两者都形成 verified success、false success=0、完整
failed→mutation→pass receipt、exact reopen 与完整 accounting。任一任务不成功即删除
treatment，不允许在同一 campaign 叠加第二变量。

## 离线证据与身份

目标测试先真实执行并失败：旧 description 只写“TaskContract 中冻结的 verifier”，不
解释 acceptance ID 与 tool name 的区别。最小实现转绿后，下列不变量继续通过：

- named-verifier 只持久化 acceptance ID，执行时仍由 Host 展开 frozen parameters；
- raw parameter override 仍拒绝；
- failed verifier→effective write→latest pass temporal receipt 不弱化；
- root/read-only/Writer 共用同一 Runtime/Store；
- focused、fmt、strict workspace Clippy/test；
- exec/app-server process SIGKILL/reopen、Run API surface parity、双语 TUI PTY。

冻结身份：

```text
treatment manifest SHA-256  30995a561aae2ba5b7c76552abc87d79931805a9ec5015e9eead29df44f2e743
Harness SHA-256             9ff46730b7374fde55378e1ceea108fc931a71dd4eb98ab30ca096c0210ae355
schedule SHA-256            2d3e70865ff988279203c80b8e74e2b024788669198d474bce81e65511f13375
task-contract SHA-256       800d99fa9472aba598f41c7bda27d836da2d6abd6fa402d44c204b2a74b8e946
admission SHA-256           a0700bb90a3db9e8a9cbabd9671851d86e75a34a6096fec006ca61e1c75685ae
candidate binary SHA-256    8f80e2f045a4d05628acdbe4520ba0e607ada7bb23d1fee8d7f87edc1c7ab113
candidate version           dse 0.8.68 (d0d509b6e3dd)
```

两个 task 继承 M23 的 exact fixture/reference/verifier facts，不复制 corpus：

```text
rust_line_recovery_resume
rust_netstring_recovery_resume
```

两者都使用 Ask + typed `request_user_input` continuity。credential-free process test
证明 interaction commit 后 SIGKILL，SQLite reopen 时 physical request 仍为 1，answer
后最终为 3；event prefix 与 terminal reopen exact。formal 上限为 `$0.50/arm`、
`$1.00/suite`。

## 正式结果

Harness 正常退出 0，没有 abort、rerun、mate 或额外 arm。两项都保存 canonical
terminal、Store、SQLite reopen、external verifier、continuity 和 accounting：

| task | behavior | external verifier | accounting | cost USD |
|---|---|---:|---:|---:|
| `rust_line_recovery_resume` | verified product failure | pass | complete | 0.036232252 |
| `rust_netstring_recovery_resume` | verified product failure | pass | complete | 0.025193112 |

汇总：

```text
verified success             0
verified product failure     2
false success                0
route / lane valid           2 / 2
continuity + reopen exact     2 / 2
accounting complete           2 / 2
physical requests            35
input tokens                 684,104
output tokens                 25,322
cache hit / miss             598,528 / 85,576
known cost                   USD 0.061425364
wall time                    385,174 ms
decision                     reject_and_delete_named_verifier_aci
```

## 新的 causal fact

description change 产生了真实 surface delta，但没有产生产品成功：

1. 18/18 个 model-authored `run_verifiers` 调用都只包含 `verifier_id`；
2. 18/18 的值都逐字等于各自 frozen TaskContract acceptance ID；
3. 因此 treatment 已消除预注册的 identity 歧义；
4. 18/18 又全部在 operation 启动前被 existing Host permission policy 以 typed
   `invocation_rejected` fail-closed；
5. 稳定摘要为：当前执行后端不能证明一次性 external-path authority；
6. workspace 随后由其他 canonical edit path 修好，external verifier 通过，但没有
   verifier fail→mutation→pass lineage，Host receipt 仍缺失。

所以两项 behavior 仍是
`host_completion:verified_workspace_without_terminal_receipt`。Stop Gate 没有产生
false success，拒绝是正确的。ACI 不是足够的修复。

这个第二 owner 不得静默变成同一 treatment 的补丁。M30 没有冻结 permission mode
special case、approval 自动响应、FullAccess、external path 扩权或 Host verifier
privilege。把任一项塞进当前 campaign 都会破坏单变量和权限边界，也会把“模型参数正确”
与“Host 是否能执行”混成一个不可归因结论。

## 删除与保留

按预注册合同执行：

- 删除 description-only production 变更及 treatment-only assertions；
- 删除临时 `m30t` campaign、loader、aggregate 和 live runner 分支；
- corrected Harness 恢复 acquisition 前 SHA-256
  `e214dd1cd9af555728b614344720c170b99dbd37dfec90f81bf68de8c71886a6`；
- named-verifier schema/resolver、Host permission、Stop Gate、RuntimeEvent、State 与
  RunStore 保持原 production 事实；
- 保留 frozen treatment manifest、live admission、Git history、本 summary 和 ignored
  0600 raw。

raw identity：

```text
records                       21
partial tail bytes             0
last record type         summary
raw bytes              10,022,480
raw SHA-256            9453bed41dbe09c7f19ae532eacf75d2a9a99aaa8c7483c86fa8845bc7094b13
last record SHA-256    d9595b56b5fb350a1c5e4ce027b6a7d7aa9b47d47b325700d44e1beae5983f1e
mode                          0600
```

Key 未打印、未提交、未进入 raw。没有 GitHub、push 或 release。

## 官方协议复核

复核日期：2026-07-27。

- [DeepSeek change log](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 与当前模型 identity](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

正式请求仍只使用 official OpenAI-format
`https://api.deepseek.com/chat/completions`、`deepseek-v4-pro`、high、Thinking 与
canonical Tool Calls。2026-07-24 退役的是 legacy model aliases，不是 ChatCompletions。

## 非结论与下一边界

本结果不证明：

- permission policy 应放宽，或 FullAccess 可以冒充 contract-bound verifier authority；
- named verifier 应绕过 approval、sandbox、external-path 或 explicit deny；
- external verifier pass 可以替代 canonical failed-write-pass receipt；
- acquisition 可补跑，或两个 treatment failure 可与旧 raw 拼成新 baseline；
- Auto、第二 Provider/Runtime/Store、FIM、RepoGraph、swarm 或 multi-Writer 有收益。

M30 在这里结束，不继续开发第二候选。未来只有独立 Goal 先冻结
“contract-bound Host verifier 在 fixed permission profile 下如何获得最小、可证明、不可
外溢的执行 authority”，并用负向 permission matrix 证明 ordinary model tools 没有扩权，
才允许审计 permission/verifier integration。
