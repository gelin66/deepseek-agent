# M9-E fixed-Pro billing evidence boundary

- 日期：2026-07-24
- 起点：`62745894`（M9-D Auto retirement）
- owner：现有 `scripts/eval-m9b-fixed-pro-regression.py`、canonical accounting 与
  RunStore；没有第二 evaluator
- 结论：
  `infeasible_exact_pre-header_request_reconciliation_under_current_official_contract`
- credential：未读取
- official DeepSeek API requests：0
- official docs network：只读复核

## 问题与审计上限

M9-A 与 M9-C 都在服务端是否已开始执行未知、客户端尚未收到 response headers/usage
时发生 typed transport failure。CodeWhale 正确保存
`billing_unknown=true`、`complete=false` 并停止 formal campaign，但因此无法完成
fixed-Pro regression baseline。

本切片只回答一个问题：当前官方契约能否把这样的一个物理 attempt 追溯为 billed 或
unbilled。预注册的审计上限是：

1. 复核官方 Chat response/usage、价格扣费、`/user/balance`、Usage export、
   concurrency/`user_id`/keep-alive 与错误语义；
2. 审计现有 production transport、accounting、RunStore 和 corrected Harness；
3. 只有官方契约同时提供 request-level identity、账单 reconciliation 和有界结算语义，
   才设计一次隔离 Key 的最小付费 canary；
4. 任一必要能力没有官方保证时，在 credential 前结束，不用轮询、猜测余额差或自建账单
   系统弥补 Provider 真相。

## 官方事实

截至复核日，官方契约提供：

- 成功的 ChatCompletions response/chunk 有 completion `id`；完整 response 的 `usage`
  给出 input/output/cache/reasoning Token。streaming 的完整 usage 位于终止前的 usage
  chunk；
- 价格按实际 input/output Token 计算并从账户余额扣除；
- `GET /user/balance` 只返回账户级币种余额、赠金和充值余额；
- FAQ 的 Usage export 是按月下载两个 CSV，其中 `amount` 提供按 API Key 分解的用量；
- `user_id` 只声明用于内容安全、KV cache 和调度隔离；concurrency 仍按 account
  计算，与 API Key 无关；
- 排队时可能返回空行或 SSE `: keep-alive`，未开始 inference 达十分钟后服务端会关闭
  连接。

当前公开官方文档没有声明：

- request body 可提交、或 response headers 可取得一个可用于账单追溯的 idempotency/
  request identity；
- `/user/balance` 的更新时限、强一致性、最小精度或某次扣费与物理 request 的映射；
- Usage export 的 request-level 行、request/completion ID、当前月刷新时限或结算完成
  水位；
- 根据 `user_id` 查询费用，或由官方接口把一个 pre-header transport failure 判为
  billed/unbilled；
- 客户端断开或 response-before-headers failure 的取消、继续执行和计费契约。

因此 completion `id` 和 response usage 只能证明已经收到的 response；它们在 M9-A/M9-C
的失败窗口中都不存在。独占 Key、前后余额快照和月度 Key aggregate 能减少其他流量干扰，
但没有 request-level link 或 settlement bound，不能把“某次观察到余额变化/暂未变化”
提升为该物理 attempt 的可重放账单真相。一次成功 canary 也不能证明 pre-header
failure 的一般语义。

## 本地闭环

当前唯一 production sender 已做正确的可证明边界：

- 请求开始即进入 physical request ledger；
- 收到 headers、usage、finish 和重试事实分别 typed；
- pre-header transport failure 不伪造 Token/费用，不自动盲重发；
- RunStore/SQLite reopen 保留 exact `billing_unknown`、started/completed/in-flight、
  surface usage 和 retry disposition；
- corrected Harness 在下一 arm 前停止，`maximum_reruns=0`，不补 mate、不续跑、不拼接。

本切片不修改 sender、accounting 或 Harness。给 transport 增加客户端自造 request ID、
把 `user_id` 当账单 ID、用余额差推算 usage、或自动等待/重试，都不会产生官方账单真相，
反而会引入第二状态或选择性采样。

## 决策

P0 得到明确的不可行结论，而不是外部 blocker：

1. 在当前官方公开契约下，CodeWhale 不能精确 reconciliation 一个无 headers/usage 的
   physical attempt；
2. 保留现有 `billing_unknown -> formal campaign stop`，不读取 Key、不调用 API；
3. 未来 fixed-Pro 效果实验只有在每个 physical response 都有完整 usage/accounting 时，
   才能形成 Token/费用结论；出现 unknown billing 的 campaign 继续 fail closed；
4. A–E 产品能力切片可以先做离线证据，并在完整 accounting 的正式 arms 上评估质量和
   效率；不完整 campaign 不作成本结论；
5. 若未来要放宽“任何 unknown billing 使整个 formal aggregate 不可准入”的规则，必须
   单独提出 ADR 和保守统计契约，不能在候选评测中临时改门。

这关闭的是“当前是否值得继续自造 billing acquisition”的问题，不关闭未来官方增加
request-level billing export/support 后的重新复核。

## 官方来源

复核日期均为 2026-07-24：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Get User Balance](https://api-docs.deepseek.com/api/get-user-balance/)
- [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit/)
- [DeepSeek FAQ / Usage export](https://static.deepseek.com/faq/index.html?lang=en)
- [Error Codes](https://api-docs.deepseek.com/quick_start/error_codes/)
