# M8-D 中文原生 Agent prompt A/B 结论

- 日期：2026-07-24
- 分支：`deepseek-agent`
- 起始基线：`491c069c`（M8-C decision）
- candidate fixture：`77f5244a`
- production caller fix / immutable binary source：`8371b6dd9ac5d18570ff81a28bd94cc93372c372`
- source tree：`9fc34ee9dcd1837d361a37bbef174799c9241624`
- final evaluator checkpoint：`a51e145f`
- final live admission：`26841208`
- 协议身份：Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2
- 最终决策：`hold_prompt_candidate / keep_app_server_override_consistency`

> 2026-07-24 successor：M8-M 以 current `d1d6ca5c` fixed-Pro immutable binary、
> 全新 30-arm suite 和 fail-before-loss journal 取代旧 evaluator。它完成 5 个
> measurement-valid arms 后，第 6 arm 的首个请求无 response/usage，按
> `aborted_unknown_billing` 停止。旧 M8-D 与 M8-M 样本均不可续跑或拼接，bundled prompt
> 仍不切换。见
> [M8-M billing-provable 中文 Agent prompt successor](m8-m-billing-provable-zh-prompt-successor-2026-07-24.md)。

## 1. 结论

M8-D 没有产生可采纳的完整正式 A/B。production bundled prompt 保持原样，候选不接管：

- v1 证明真实 `app-server` 没有加载既有 context-owned prompt override，因此两臂 wire
  prompt 实际相同；该 caller 缺口已在 `8371b6dd` 修复并以外部进程 + State reopen 证明；
- v2-v4 逐次暴露 evaluator 对 current multi-Agent / Writer canonical facts 的旧投影。
  每个 raw 都保持不可覆盖，未补 mate、未拼接为产品指标；
- v5 在第一个 baseline arm 收到 1 次无 usage 的失败 response/attempt，canonical
  accounting 为 `billing_unknown=true`、`complete=false`、`surface_usage=[]`。Harness
  立即提交 `aborted_unknown_billing`，没有启动第二个 arm；
- 因 30 arms 未完成、计费不可证明，verified success、false success、Token、请求、
  wall time 和费用均不具备产品 A/B 资格。候选既不能 `keep`，也不能据不完整样本判定
  prompt 本身有害；结论是 `hold`。

不存在需要删除的 candidate production prompt branch：候选从未替换
`crates/context` 的 bundled constitution，只以 ignored eval fixture 和既有 config-home
override 进入冻结二进制。保留 fixture、manifest、Harness 与 0600 raw 以复核身份；保留
`app-server` override loader 修复，因为它收敛现有 TUI/exec/app-server caller，而不是
为候选新增模式、Provider、Runtime、Store 或工具 surface。

## 2. 当前官方 DeepSeek 接口事实

2026-07-24 重新复核以下官方一手资料：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)
- [Change Log](https://api-docs.deepseek.com/updates/)
- [FIM Completion](https://api-docs.deepseek.com/api/create-completion/)

冻结事实是：

- OpenAI-compatible base URL 仍为 `https://api.deepseek.com`；
- Standard Chat 当前请求路径仍为 `POST /chat/completions`；
- M8-D 只使用当前模型 `deepseek-v4-flash`，未使用 `deepseek-chat` 或
  `deepseek-reasoner`；
- 两个旧 model alias 于 2026-07-24 15:59 UTC 退役；这不等于
  `/chat/completions` API surface 退役；
- FIM 是独立 Beta `POST /beta/completions` surface，M8-D 没有调用、恢复或新增 FIM
  production caller。

manifest 同时冻结 model、endpoint、价格、thinking/tool-call 合同和 legacy alias rejection。
v1-v4 的成功请求与 v5 首请求都来自同一 immutable binary 和该 endpoint/model 身份。
v5 的具体 provider failure 原因没有足够 raw 事实可安全归因；不能把
`aborted_unknown_billing` 猜成 endpoint、余额、限流或模型下线。

## 3. 唯一 treatment 与 production caller

candidate 只替换 constitution 中的固定五步 checklist：

```text
先确定当前事实与目标之间的最小缺口，再行动。
每次工具结果返回后只根据新事实选择下一步；证据足够时立即完成。
```

以下事实在两臂保持相同：

- immutable `codewhale` / `codewhale-tui` binary pair；
- `deepseek-v4-flash`、`reasoning_effort=high`、streaming 与 output limit；
- task contract、tool schema、root/read-only child/explicit Writer 权限；
- request/tool/turn/wall budgets、retry policy、pricing 和 accounting；
- stable prompt suffix、其余 system blocks、cache controls 和 task definitions；
- AgentApplication、AgentRuntime、RunStore、RuntimeEvent 与 State schema。

真实 caller 审计发现 TUI 与 exec 已调用
`codewhale_context::prompts::load_prompt_overrides_from_config_home()`，而
`codewhale app-server` 没有。`8371b6dd` 在构造 AgentApplication 前调用同一 loader，
并增加外部 `codewhale app-server --stdio` 进程测试：网络由 loopback 阻断，Start 后
SIGKILL，再从 State DB 的 `RunCreated` request 证明 override 已 durable 生效。

没有新增模型可见同义工具、用户 prompt 模式、第二 prompt owner、第二 model loop 或
compatibility reader。

## 4. 冻结身份

正式 suite 共用：

| 字段 | 值 |
|---|---|
| source revision | `8371b6dd9ac5d18570ff81a28bd94cc93372c372` |
| source tree | `9fc34ee9dcd1837d361a37bbef174799c9241624` |
| `codewhale` | 15,827,232 bytes；SHA `d7d6afbe…98f823b` |
| `codewhale-tui` | 22,588,464 bytes；SHA `f0239151…5661c4f` |
| binary pair SHA | `30288ee1…f87939c` |
| source artifact | `codewhale-0.8.68-aarch64-apple-darwin-8371b6dd9ac5.tar.gz` |
| artifact bytes / SHA | 17,734,574 / `e343fcbc…e8dec8e` |
| candidate prompt SHA | `2c3b8018…ec50ebf` |
| schedule | 5 tasks × 2 variants × 3 runs = 30 arms |
| maximum reruns | 0 |

五个任务为：

- `t1`、`t3`：single；
- `t5`：exactly one read-only child；
- `w1`、`w3`：explicit-only isolated Writer。

每次 preflight 都在禁用外部网络时由真实 app-server 证明 prompt-only delta，并物化全部
fixture 的 frozen tree/base commit。v5 另用真实 clean integrated commit 证明 Writer
scope 必须取 `base..HEAD`，而不是集成后为空的 `git status`。

## 5. 不可覆盖 raw 与停止原因

| suite | raw status / 完成 arms | 已知费用 | 事实与停止原因 |
|---|---:|---:|---|
| v1 | aborted / 2 | $0.004789003 | app-server 未加载 override，两臂无真实 prompt delta |
| v2 | aborted / 5 | $0.011115720 | t5 Host/verifier 成功；Harness 读取旧 child 字段与 root-only usage |
| v3 | process exited，raw 保持 running / 6 | $0.013158683 | 6/6 valid/verified；M6 Writer fixture 被按 M7 Git identity 物化，shared error 未收口 |
| v4 | aborted / 7 | $0.023390596 | 6/6 single/read-only valid；首个 Writer 被旧扁平 task/status scope 投影错判 |
| v5 | aborted / 1 | unknown，raw lower bound $0 | 首请求无可证明 billing，立即 `aborted_unknown_billing` |

raw identities：

- v1：31,506 bytes，SHA `9963d324…6a2f27`；
- v2：70,547 bytes，SHA `67ec3410…a5cc84`；
- v3：85,183 bytes，SHA `6823fcec…d1392c`；
- v4：109,133 bytes，SHA `dd612d6c…289d78`；
- v5：19,583 bytes，SHA `d129a372…c7a3a0`。

五个文件均为 ignored `0600`。v1-v4 可证明费用合计下界为 `$0.052454002`；v5 费用未知，
因此不能写成总费用。五次 suite 共观察到 126 个 physical request attempts，其中 v5
最后一次没有 usage；该计数也不等于可证明账单。

v3/v4 的三个完整 single/read-only pairs 方向并不稳定：v3 paired median 显示 Token
改善 23.50%，v4 则退化 12.33%；两者都只有 3 pairs 且 suite measurement contract
不完整。这些数值只用于诊断，不能支持 keep/reject。

## 6. 离线门禁

同一 immutable binary pair 在 v2 已通过：

- `cargo fmt --all -- --check`；
- owning crate targeted test/check；
- `./scripts/dev-codewhale.sh focused`；
- workspace all-target strict Clippy 与完整 tests；
- exec、HTTP、stdio production loopback；
- app-server / State process SIGKILL + SQLite reopen；
- root、read-only child、explicit Writer conformance；
- TUI/PTTY 和 raw/machine projection。

v5 在 Key 前又通过：

- 16 项 Python Harness contract tests；
- frozen Harness/test/manifest/schedule hashes；
- 全五任务 fixture Git identity；
- current nested AgentTask / aggregate ModelAccounting；
- real integrated Writer `base..HEAD` scope 与 clean cleanup；
- network-blocked process prompt activation；
- `git diff --check`。

最终 decision tree 又通过 `cargo fmt`、两项 M8-D targeted Rust tests、owning crate check、
`./scripts/dev-codewhale.sh focused`、workspace all-target strict Clippy
`-D warnings` 和 `cargo test --workspace --locked`。完整 workspace run 包含 exec 25/25、
canonical/QA PTY 7/7 + 9/9、TUI 889 passed/1 ignored、State process crash
38 passed/1 helper ignored、RunStore 33/33、Writer 25/25 和 run-surface parity 2/2。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0` 和
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8d-target`。

## 7. 复杂度与取舍

相对 M8-C decision `491c069c`：

- 15 files changed；
- 4,016 insertions、13 deletions，净 +4,003；
- 正增量主要是 2,078 行 evaluator、523 行 evaluator tests、五个 lineage manifests、
  fixture 和 production-path regression；
- production 行为增量只是一处 app-server 启动 loader 调用及其依赖；bundled prompt、
  protocol、Runtime、Store、tool catalog 和默认模型行为未改变。

该增量不是能力收益。保留理由仅是五次不可拼接 raw 的身份、fail-closed accounting、
current root/read-only/Writer projection 和可复核 prompt-only contract需要它们。没有
把 evaluator 变成 Runtime、Provider、模式或第二状态真相。

## 8. 产品决策与非结论

决策为：

**hold prompt candidate；keep app-server override consistency；production default 不变。**

M8-D 不证明：

- candidate 提高或降低总体 verified success；
- false success、首次工具正确率或失败恢复率有稳定变化；
- Token、模型轮次、cache hit、wall time 或费用有净改善；
- `chat/completions` endpoint 已退役；
- v5 unknown billing 的具体外部原因；
- FIM、thinking-off、fan-out 或多 Writer 应重新准入。

任何未来 prompt 复评必须有新的 production delta、全新 successor manifest、从
position 1 开始的完整 schedule，以及在首 arm 后仍可证明的 billing；不得续跑 v5、
补 mate 或合并 v1-v4 样本。

## 9. 下一阶段

下一阶段不应继续堆 prompt 版本，而应执行 M8-E V1 退出证据总审计：把
PRODUCT_PLAN 的完成定义与 M1/M2/M5-C/M6/M8 已有证据逐项对齐，先决定真实发布阻塞，
再只修复有 current production 反例的缺口。

建议 Goal objective：

> M8-E：以 M8-D 的 hold 结论、未切换的 production prompt、唯一
> DeepSeek/AgentApplication/AgentRuntime/RunStore 链和 M8-A～M8-C 的可复现交付为基线，
> 执行 V1 退出证据总审计与发布候选收敛：逐项对照 PRODUCT_PLAN V1 完成定义和当前
> M1/M2/M5-C/M6/M8 证据债务，冻结唯一 gap matrix、clean revision、locked/offline
> 两二进制 artifact、CLI/TUI/API/root/read-only/explicit Writer/crash-reopen/
> machine-zh-Hans conformance；只修复能由当前 production 反例证明的确定性缺口并删除
> 被替代路径，不恢复 Provider、第二 Runtime/Store、模式系统、prompt 实验分支或多 Writer；
> M8-D unknown-billing raw 保持不可续跑，除非有新的 production delta、全新 successor
> manifest 且计费可证明，否则不读取 Key；最终明确 V1 可发布/不可发布清单、
> keep/shrink/hold、权威文档、分离提交与精确清理。
