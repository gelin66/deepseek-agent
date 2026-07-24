# M8-E V1 退出证据总审计

> 复核日期：2026-07-24  
> 冻结 production revision：`433a871b9e26a09009d99575594c29557ebc7484`  
> 审计 contract / artifact revision：`a12bea45f20668e7ba58aebee4a6267ff4f70233`  
> 结论：`not_releasable / keep_canonical_chain_and_delivery / hold_release`

> 2026-07-24 纠正：frozen manifest/result 中的
> `user_selected_release_target=https://api.deepseek.com/anthropic` 是未经授权的错误
> 前提，用户已明确否认，PRODUCT_PLAN/ADR 也从未接受。冻结文件保持不变以保留审计
> 历史；下文产品结论已 supersede：Anthropic 不计入 V09 或 release gap，M8-F
> Messages cutover 为 `canceled_invalid_premise`。

## 1. 产品结论

M8-E 逐项审计 `PRODUCT_PLAN.md` 的 16 项 V1 完成定义。8 项有当前源码、离线
conformance 和制品证据，8 项仍被明确反例或缺失证据阻塞。因此当前 CodeWhale **不是
V1 release candidate**，即使完整 workspace 门禁和两项正式 binary 都通过，也不能把
“机制可运行”扩张为“产品完成”。

本切片不修改模型、Runtime、Store、工具、prompt 或 production sender。它保留：

- 唯一 `DeepSeek -> AgentApplication -> AgentRuntime -> RunStore` 链；
- root、read-only child 和 explicit Writer 共用的 Runtime/Store contract；
- latest-revision `EvidenceReceipt` completion gate；
- fixed `zh-Hans` owner；
- `codewhale` + `codewhale-tui` 的 locked/offline delivery owner。

本切片继续 hold：

- M8-D 中文 prompt candidate；
- canonical FIM production 接入；
- automatic read-only fan-out；
- broader request/reasoning/cache 优化。

M8-D v5 的 unknown-billing raw 不续跑、不补 mate、不拼样。M8-E 没有新模型
treatment，没有读取 `key.txt`，官方 DeepSeek 请求为 0。

## 2. DeepSeek 官方接口复核与发布目标

2026-07-24 重新核对：

- [DeepSeek Change Log](https://api-docs.deepseek.com/updates) 和
  [DeepSeek V4 发布说明](https://api-docs.deepseek.com/news/news260424/) 指明
  `deepseek-chat`、`deepseek-reasoner` 是在 2026-07-24 15:59 UTC 退役的旧模型别名；
  保留模型是 `deepseek-v4-pro`、`deepseek-v4-flash`。
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) 与
  [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
  仍把 OpenAI 格式列为受支持接口。
- [Anthropic API](https://api-docs.deepseek.com/guides/anthropic_api/) 同时定义官方
  Anthropic 格式 base URL `https://api.deepseek.com/anthropic` 和 Messages API。

因此“旧 alias 下线”和“ChatCompletions 接口下线”不是同一事实。CodeWhale 是只接
官方 DeepSeek 的独立本地编码 Agent，不需要兼容 Claude Code/Anthropic 生态。
PRODUCT_PLAN 固定 Standard Chat、Strict fallback 与独立 FIM；当前
`crates/deepseek` `/chat/completions` sender/parser/replay/accounting 是应保留的
production 路径。Anthropic 只是官方提供的另一种兼容格式，不是发布要求。

## 3. V1 gap matrix

| ID | V1 完成定义 | 状态 | 当前证据 |
|---|---|---|---|
| V01 | 只有 DeepSeekBackend | pass | M8-A 已删除 generic model Provider/router；三入口共用 `crates/deepseek` |
| V02 | 只有一个 AgentRuntime | pass | root、read-only child、Writer 都由 `crates/runtime::AgentRuntime` 执行 |
| V03 | 根/子 Agent 同一 conformance | pass | focused、workspace、Writer 和 process recovery suite 共用 v16/v21 contract |
| V04 | 一个 canonical RuntimeEvent | pass | `crates/protocol` 唯一拥有 RuntimeEvent v16 |
| V05 | 一个 RunStore | pass | `StateStore` 是唯一 production SQLite `RunStore` |
| V06 | 一个 TaskGraph 产品概念 | blocked | canonical Orchestrator 与用户可见 Fleet、Lane protocol/config/state 并存 |
| V07 | CLI/TUI/API 是薄客户端 | pass | 都进入 `AgentApplication -> AgentRuntime -> RunStore` |
| V08 | 多写 Agent 完整闭环 | blocked | 单 explicit Writer 完整；multi-Writer 经 M6-B1 后未准入 |
| V09 | Standard/Strict/FIM 路由准确 | blocked | Standard/Strict fallback 已证；FIM 无 caller/parser/apply |
| V10 | RepoGraph 跨文件理解 | blocked | M5-C 仍未实现，也没有等价 canonical owner |
| V11 | latest EvidenceReceipt completion | pass | M5-A owner 与 v16/v21 reopen 保留 latest-revision 绑定 |
| V12 | 旧 Provider/updater/重复路径清除 | blocked | model/updater 已删；Fleet/Lane 与部分 generic provider vocabulary 尚存 |
| V13 | 优于导入 CodeWhale 基线 | blocked | 尚无对 `352e86a6` 的固定真实 coding A/B |
| V14 | 固定 zh-Hans | pass | M8-C L01-L14、CJK、machine/raw 与 installed artifact 均通过 |
| V15 | 中文 Agent prompt A/B 通过 | blocked | M8-D v5 unknown billing，bundled prompt 未切换 |
| V16 | 用户步骤没有变复杂 | blocked | 没有对 imported baseline 的同任务 workflow-step 比较 |

冻结 contract：

- `eval/manifests/m8-e-v1-exit-audit-v1.json`；
- `scripts/test-eval-m8e-v1-exit.py`；
- ignored、0600、单次写入
  `eval/results/m8-e-v1-exit-audit-433a871b-v1.json`。

manifest 绑定 frozen `433a871b` 的四份权威文档内容；测试通过 `git show` 从该 revision
解析 blob，后续权威文档更新不能反向改写审计输入。

## 4. 可复现制品

从 clean `a12bea45` 构建：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8e-target
scripts/codewhale-delivery.sh package --output-dir <external-temp>
```

身份：

| 字段 | 值 |
|---|---|
| revision | `a12bea45f20668e7ba58aebee4a6267ff4f70233` |
| tree | `966afe2fde6e4af788409c992fc78e872a96acc9` |
| Cargo.lock SHA-256 | `ff53b4985b18a05847bd617fec2aa2b69e93d579cbe3f3d64f8f874acfaf0ef2` |
| rustc | `1.97.0 (2d8144b78 2026-07-07)` |
| artifact | `codewhale-0.8.68-aarch64-apple-darwin-a12bea45f206.tar.gz` |
| artifact bytes | `17,734,562` |
| artifact SHA-256 | `6104e45023ff8c8594f60645da28f57c18913ec4e9249c0900569c3020a86ffc` |
| `codewhale` | `15,827,232` bytes / `bee2414a…afba9` |
| `codewhale-tui` | `22,588,464` bytes / `f8266dd1…9262f` |

归档目录只有 manifest、LICENSE、inner checksums 和这两个 binary。真实临时 prefix
完成 install/verify，两者分别报告：

```text
codewhale 0.8.68 (a12bea45f206)
codewhale-tui 0.8.68 (a12bea45f206)
```

## 5. 门禁

全部 Cargo 命令使用 external target、`CARGO_INCREMENTAL=0` 与 offline 模式。通过：

- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- M8-E contract 8/8；
- exec acceptance 25/25；
- canonical TUI PTY 7/7、Run 18/18；
- exec/HTTP/stdio parity 2/2；
- root/read-only/Writer conformance；
- app-server process crash/reopen 与 SIGKILL recovery；
- delivery self-test；
- real locked/offline package install/verify；
- `git diff --check`。

这些门禁证明当前 baseline 的一致性和可交付性，不补足 V06/V08/V09/V10/V12/V13/V15/V16。

## 6. Keep / shrink / hold / release

**Keep**

- canonical DeepSeek application/runtime/store chain；
- latest-revision evidence 与 typed failure/recovery；
- explicit single Writer mechanism；
- fixed `zh-Hans` owner；
- reproducible two-binary delivery。

**Shrink/delete in successor cutovers**

- canonical TaskGraph replacement 运行后删除 Fleet/Lane 重复产品与持久状态路径；
- consumer 迁移后删除余下 generic provider vocabulary。

**Hold**

- prompt candidate、FIM、automatic fan-out、broader request/cache 调优；
- 任何依赖 M8-D unknown-billing raw 的续跑或产品结论。

**Release**

- 当前结论：`not_releasable`。
- M8-F Anthropic Messages production cutover：`canceled_invalid_premise`；冻结 commit
  `066e15cb` 保留历史，manifest/专属测试与未提交 WIP 已删除，Key 未读取、官方请求 0。
- 首个下一切片：把 Orchestrator/Fleet/Lane 收敛为单一 TaskGraph 产品概念。
- 后续仍需 FIM 产品范围/证据、V1 multi-Writer/RepoGraph scope 决策、
  imported-baseline coding/workflow-step A/B，以及 billing-provable 中文 prompt
  successor。

### 2026-07-24 successor note

M8-G code candidate `64f6bc16` 已物理删除 Fleet/Lane 的 protocol、config、持久状态、
command、UI 与 process shell，保留现有 `AgentRuntime`、`RunStore` 和 explicit Writer
Orchestrator。因此本文件冻结的 V06 `blocked` 是 M8-E 历史输入，当前 V06 已变为
`pass`；V12 收窄但未关闭，V16 仍需 imported-baseline workflow-step A/B。M8-E 的
8 pass / 8 blocked frozen result、manifest 与 raw 不反向修改。successor 见
[M8-G 单一 TaskGraph 产品概念收敛](m8-g-taskgraph-convergence-2026-07-24.md)。

M8-H code candidate `7d9aa9a6` 又 supersede 本文件“保留 FIM planner/accounting
基础”的历史快照。调用图证明旧 `plan_fim` 产生 sender endpoint owner 不接受的
`/beta/completions` URL，完整 parser 只读取 Chat `choices[].message`，Runtime/RunStore
也没有 revision-bound Host apply/reopen consumer；它不是可运行 treatment。当前 production
已物理删除这条无消费者半分支及 always-zero accounting/terminal 字段，保留官方
Standard Chat 与 lossless Beta Strict fallback。V09 的 frozen `blocked` 不反向改写；
当前结论为 `keep Standard/Strict / reject unreachable FIM production half-branch /
hold FIM re-entry`，详见
[M8-H FIM 产品范围与历史债收敛](m8-h-fim-scope-debt-2026-07-24.md)。

M8-J successor contract `9e644add` 从 M8-G 后的 current 9 pass / 7 blocked 重新审计
V12。code candidate `bcbc1616` 删除旧 `codewhale thread`、SQLite `threads` metadata
表、`session_index.jsonl` 第二真相和无 production consumer 的
Thread/App/Prompt/EventFrame protocol 岛；State v23 保留 current canonical run、
pending Start、route audit 与 accounting。V12 当前为 pass，successor matrix 为
10 pass / 6 blocked；本文件 frozen 8/8 结果不反向改写。详见
[M8-J V1 successor 与 V12 历史债删除](m8-j-v1-successor-v12-debt-2026-07-24.md)。

## 7. 非结论

M8-E 不证明：

- offline 全绿等于 V1 完成；
- FIM、RepoGraph 或 multi-Writer 已实现；
- CodeWhale 已优于导入基线；
- verified coding success、false success、Token、cache、时间或 API 成本已有改善；
- M8-D 的 unknown billing 可以事后补算。
