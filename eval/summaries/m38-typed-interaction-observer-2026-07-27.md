# M38 typed interaction observer 与 durable abort accounting

日期：2026-07-27

基线：`b91d10780bbd714d845dbbf28dada27791509f5a`

基线 tree：`23c8a201d955e3786da135e84c3a0011d51e357e`

决定：`keep_typed_interaction_observer_and_durable_abort_snapshot`

## 真实问题

M37-C position 1 到达 canonical `request_user_input` checkpoint 后，唯一 corrected Harness
仍按 campaign 名称猜 interaction 类型。它把合法 user input 当成 approval-only 历史路径，
在 terminal/accounting snapshot 之前中止。RunStore 已经持久化的 behavior、usage 和 known
cost 因此没有进入 eval journal。

这是 Harness observer defect。它不证明 Prompt、Runtime、DeepSeek、工具或产品任务失败，
也不能通过续跑 M37-C 修正 frozen evidence。

## 最小实现与删除

唯一 owner 仍是 `scripts/eval-m9b-fixed-pro-regression.py`：

- 从 canonical `UserInteractionPrompt::{approval,user_input}` payload 派生 admission；
- 按 prompt kind 构造并验证 `approved` 或 `answered` response；
- root interactive profile 接受两类 typed request；read-only/Writer child 保持非交互并
  fail closed；
- observer 在 durable checkpoint 后失败时，先写安全、hash-chained
  `observer_abort_snapshot`，再写 abort；
- snapshot 保存 event prefix、terminal、physical attempts、runtime retries、known usage/cost
  与 accounting truth，不保存 raw model content/reasoning/tool arguments/credential。

物理删除了 campaign-driven interaction admission、campaign-driven response construction、
`hardness_user_input_not_admitted` 和“已有 durable accounting 却只写 abort”的路径。没有
新增第二 evaluator、Store、protocol 或 production compatibility branch。

## 冻结证据

- fixture：`eval/fixtures/m38-typed-interaction-observer-v1.json`
- fixture SHA-256：`8f24adaecd6252b353fa903a0eafb63304cfdaee0f47ca3b66c8b26f5a11ac17`
- manifest：`eval/manifests/m38-typed-interaction-observer-v1.json`
- manifest SHA-256：`e9bcc7ad6e28e36edc2a52c818d5bbf0ae732722b090509c400d7b92edd31eaf`
- Harness SHA-256：`04cdac789ecca8e235053b22c75a7492d40fe0c4bf54b427c74d453034ecef1d`
- conformance stdout SHA-256：`371b30ffe38d1478066379d6826dbb405cc43a3be2a9fdf132ed607662bc087e`
- case result SHA-256：`6bae0f255edfb18425df15561046cf6a9e08f31feb69de9801f6b8d645059049`
- abort journal projection SHA-256：`282709a17e2e9a0d214ef0f368ed9f4e3a5b86fa01cee708d2e851276184a84c`

14 个 case 中 8 个 typed interaction/profile 被正确接受，6 个 malformed、mismatched、
multiple、non-interactive 或 reopen-drift case 被 fail closed。known input/output tokens
`120/24` 与 cost `38400 nanousd` 在 observer abort projection 中保留；provider
`billing_unknown` case 仍为 unknown。

M9-C 与 M30 的 `--interaction-conformance` stdout byte-identical。journal conformance
覆盖 before/mid/unfsynced/after snapshot 四个 SIGKILL 窗口、partial-tail 拒绝、record
顺序和 tamper rejection。M9-C/M30 self-test、M14 observer、M16 acceptance、M23
behavior/accounting truth 和 Hardness conformance 均通过。

## 官方计费边界

复核日 2026-07-27：

- [Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/) 定义 response/
  chunk completion id；stream usage 由结束前 usage chunk 提供；
- [Get User Balance](https://api-docs.deepseek.com/api/get-user-balance/) 是账户余额聚合；
- [FAQ usage export](https://api-docs.deepseek.com/faq/) 是 monthly CSV，并可按 API Key 分解；
- [Pricing](https://api-docs.deepseek.com/quick_start/pricing/) 按 token 计价。

官方公开合同没有为 response-before-headers failure 提供可用于单个物理请求对账的 request
identity 或有界 settlement guarantee。因此 M38 彻底修复的是“本地已经存在的 accounting
事实被 observer 丢失”；真正 provider pre-header ambiguity 仍必须 fail closed，不能用余额
差或月度导出拍脑袋分摊。

## 非结论

- 没有读取 Key、调用官方 API、访问网络或修改 M37-C frozen evidence；
- 没有证明 M37-C treatment 有益、无害或等价，也没有形成 continuation 权限；
- 没有修改 production Prompt、Runtime、RunStore、tools、permission、Writer 或 DeepSeek
  accounting；
- 没有声称 account aggregate 已经能逐请求闭合 response-before-headers billing。
