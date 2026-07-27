# M31 contract-bound verifier permission treatment

- 日期：2026-07-27
- baseline：M30 clean checkpoint `401545971`
- production candidate：`dbfbb8a58899817a1ac7f7d896808fbea68c66bb`
- live admission：`b2d0c21fd`
- decision：`keep_contract_verifier_grant`
- production cutover：保留 typed exact grant；删除临时 M31 Harness consumer

## 问题与单变量合同

M30 的两个独立 Ask/interactive continuity task 已证明：模型提交的
`run_verifiers` acceptance ID 与 Runtime 展开的冻结参数都正确，但 generic
external-path classifier 在 operation 前拒绝 verifier executable；同一 Run 的终态 Host
verifier 随后能在相同 workspace-write、no-network sandbox 中执行同一 spec。稳定 owner
是 permission/verifier integration，不是模型 ACI。

M31 只引入一个不可由模型请求的 typed execution grant：

- Runtime 从 exact acceptance-ID handle 与 frozen `VerifierSpec` 派生
  `ToolExecutionGrant::TaskContractVerifier`；
- grant 绑定 canonical spec SHA-256、workspace revision 与 prepared invocation；
- tools 重新解析 spec，只让其 `commands[].program` 跳过 generic external-path
  classification；
- external cwd、普通 read/edit/shell path、network、explicit deny、hard invariant、
  read-only child 与 isolated Writer sandbox 不变；
- Host completion 仍要求 latest-revision failed→write→pass receipt。

预注册 keep 门要求两个原 task 各执行一次 fixed-Pro/high treatment，
`maximum_reruns=0`，2/2 verified success、false success=0、exact reopen 与完整
accounting。任一失败都要求完整删除 grant，不允许追加 FullAccess、approval 特例、
raw verifier 参数或第二执行路径。

## 离线证据与冻结身份

candidate 先通过：

- canonical digest、forged grant、wrong digest 与 spec drift 负向测试；
- 16-case permission/scope matrix；
- Ask interactive/headless failed→write→pass production loopback；
- root、read-only child、isolated Writer 与 SQLite exact reopen；
- prepared、authorization committed、execution started、outcome committed 四个
  SIGKILL/reopen window；
- CLI/TUI/app-server parity、双语 PTY、focused、fmt、strict workspace Clippy/test、
  public checker 与 diff check。

冻结身份：

```text
contract manifest SHA-256   89191576e8789f65423d6b7e4b5fe4c9a9894b978e24bdd49f5e11c6848ffaa0
permission fixture SHA-256  74780eafeaa66b1c61b0c9b09f82c4edde104d6941c27957e657c8ca13c5109e
treatment manifest SHA-256  824083dd461612543ad67bdcc7364713c3e144495853489c4fbc2e49370cbde3
live admission SHA-256      037325a724bfa7bd7609491485c2e52c1245e6cca78ed7d4f66f0780652f7a12
Harness SHA-256             b8ddb85875ae0b2dd2f5ea89b6c2b1a9ed9c34c1fc90849cdc28e9369c14170e
schedule SHA-256            2d3e70865ff988279203c80b8e74e2b024788669198d474bce81e65511f13375
task-contract SHA-256       800d99fa9472aba598f41c7bda27d836da2d6abd6fa402d44c204b2a74b8e946
candidate binary SHA-256    3c4f6087504e5a511bfcee89f68746c288fe03b8cab54f65c4df7eb639616847
candidate version           dse 0.8.68 (dbfbb8a58899)
```

Run API / RuntimeEvent / State identity 为 v14 / v21 / v27。admission commit 前只检查
Key 是 ignored、regular、non-symlink、owner-only `0600`；未读取内容，official 请求为
0。用户随后只授权本 Goal 两条 arm，`$0.50/arm`、`$1.00/suite`。

## 正式结果

Harness 正常退出 0，没有 abort、rerun、mate 或额外 arm：

| task | behavior | receipt | reopen | accounting | requests | cost USD |
|---|---|---:|---:|---:|---:|---:|
| `rust_line_recovery_resume` | verified success | latest pass | exact | complete | 11 | 0.018534248 |
| `rust_netstring_recovery_resume` | verified success | latest pass | exact | complete | 6 | 0.012027576 |

汇总：

```text
verified success             2 / 2
false success                0
route / lane valid           2 / 2
continuity SIGKILL/reopen     2 / 2
SQLite terminal reopen exact 2 / 2
additional approvals         0
accounting complete           2 / 2
physical requests            17
input tokens                 254,746
output tokens                 14,443
cache hit / miss             215,168 / 39,578
known cost                   USD 0.030561824
wall time                    218,252 ms
decision                     keep_contract_verifier_grant
```

两个 task 都真实经历 verifier failure、workspace mutation 和最新 revision pass。
`rust_line_recovery_resume` 还从一次 `stale_read` 与两次 `verifier_failed` 恢复；
`rust_netstring_recovery_resume` 从一次 `verifier_failed` 恢复。Host receipt、external
verifier、allowed-path diff、route、accounting 和 credential-free SQLite reopen 均通过。

raw identity：

```text
records                       21
partial tail bytes             0
last record type         summary
raw bytes               4,626,788
raw SHA-256            efcfe2846952d13416910a9980b7b37a88e7ec1480587db69f1960fb1b0c5570
last record SHA-256    6f777b3989f23aa027c10ac058489bffdf415a602d5060a3140931ebe4dd0481
mode                          0600
```

Key 未打印、未提交、未进入 raw。没有 GitHub、push 或 release。

## 保留与删除

保留：

- typed `ToolExecutionGrant::{Ordinary, TaskContractVerifier}`；
- exact `VerifierSpec` digest、Runtime derivation、authorization/replay/store validation；
- tools 对 exact canonical verifier program 的最小 classification exemption；
- State v27 对缺少 grant 的旧 materialized Run 的 fail-closed retirement；
- frozen manifest、fixture、live admission、ignored `0600` raw、Git history 与本 summary。

删除：

- 临时 `--campaign m31` loader、aggregate、preflight、continuity 与 live runner 分支；
- M31-only Harness schema/decision consumer；
- 所有没有 post-decision consumer 的 temporary adapter。

corrected Harness 恢复到 pre-M31 blob
`90ffb72bbd830ec7e1e66c685768bea37b37e0c3`。生产链没有第二 Runtime、Store、
permission mode、verifier executor 或 external-path compatibility path。

## 官方协议复核

复核日期：2026-07-27。

- [DeepSeek change log](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 与当前模型 identity](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

正式请求只使用 official OpenAI-format
`https://api.deepseek.com/chat/completions`、`deepseek-v4-pro`、high、Thinking 与
canonical Tool Calls。未使用 Anthropic Messages、legacy model alias、Auto、FIM 或第二
Provider。

## 非结论

本结果不证明：

- ordinary model tool 获得通用 external-path、network 或 host authority；
- Ask 等价于 FullAccess，或 verifier 可以绕过 explicit deny/hard invariant；
- external verifier pass 可替代 canonical latest-revision temporal receipt；
- read-only child 可写，或 isolated Writer 可以脱离 worktree；
- 两任务结果可外推为任意模型质量、Auto、swarm、multi-Writer、RepoGraph 或 FIM 收益。

M31 只接管 exact TaskContract verifier 的 Host-owned typed grant，并以物理删除临时
Harness consumer 完成 cutover。
