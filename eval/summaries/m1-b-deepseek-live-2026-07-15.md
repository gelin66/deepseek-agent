# M1-B DeepSeek live 协议 Canary（2026-07-15）

> 状态：5/5 请求通过；这是线上协议证据，不是 Agent 或编码能力指标。

## 可复核结果

- 本地结果：`eval/results/m1-b-deepseek-live-366e8b5b.jsonl`
- 被测提交：`366e8b5b37bedbbf3b1ebb326b14a70e885a57a2`，记录为干净工作树
- JSONL SHA-256：`93895014ca7dce03fa6e809a2639dc3f3b4c25ea4cf87dcc2db03f51ef70b7c5`
- 结果：5 次计划请求全部实际发送、全部 HTTP 200、5/5 断言通过；无自动重试
- 整套墙钟耗时：5.094 秒

| Surface / 步骤 | 模型 | Finish | Cache hit / miss | 输出 | Reasoning | 请求耗时 | 估算费用 |
|---|---|---|---:|---:|---:|---:|---:|
| Standard Chat / non-thinking | `deepseek-v4-flash` | `stop` | 0 / 13 | 3 | 未报告 | 0.908 s | $0.000002660 |
| Standard Chat / thinking tool call | `deepseek-v4-flash` | `tool_calls` | 256 / 49 | 71 | 24 | 1.162 s | $0.000027457 |
| Standard Chat / exact replay | `deepseek-v4-flash` | `stop` | 256 / 135 | 23 | 19 | 0.935 s | $0.000026057 |
| Beta Strict Chat / tool call | `deepseek-v4-flash` | `tool_calls` | 256 / 44 | 38 | 未报告 | 0.939 s | $0.000017517 |
| Beta FIM | `deepseek-v4-pro` | `stop` | 0 / 18 | 4 | 0 | 1.147 s | $0.000011310 |
| **合计** | — | **5/5** | **768 / 259** | **139** | **已报告 43** | **5.094 s（整套）** | **$0.0000850004** |

总输入为 1,027 tokens，总 tokens 为 1,166；每条记录都满足
`prompt_tokens = cache_hit_tokens + cache_miss_tokens`。`Reasoning` 合计只累加 API 明确报告的
数值，不把缺失字段当成零。

## 费用口径

价格快照取自 2026-07-15 的 [DeepSeek 官方模型与价格](https://api-docs.deepseek.com/quick_start/pricing)：

- `deepseek-v4-flash`：每百万 tokens，cache hit `$0.0028`、cache miss `$0.14`、输出 `$0.28`；
- `deepseek-v4-pro`：每百万 tokens，cache hit `$0.003625`、cache miss `$0.435`、输出 `$0.87`。

逐请求 usage 按上述单价直接计算得到 `$0.0000850004`；表内各请求费用单独显示到 9 位小数，
因此相加会有末位展示误差。JSON summary 四舍五入为 `$0.000085`。运行前的
保守计划上界为 `$0.00413424`，硬停止线为 `$0.01`。这些都是价格快照下的估算，不替代
账单；官方价格变化后必须重新计算。

## 协议结论

1. Standard Chat 显式关闭 thinking 后返回 `stop`，正文非空且 reasoning 为空。
2. Thinking 工具轮返回非空 reasoning、单个合法工具调用；下一请求原样回放
   `reasoning_content` 与 tool call 后被 API 接受并正常 `stop`。这符合官方
   [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode) 对工具轮完整回放的要求。
3. `/beta` Strict Chat 中所有函数均为 `strict: true`，服务端接受 catalog，并返回符合 schema
   的工具参数；约束依据见官方 [Tool Calls / Strict Mode](https://api-docs.deepseek.com/guides/tool_calls)。
4. `/beta/completions` 使用 `deepseek-v4-pro` 完成一次非空 FIM，`finish_reason=stop`；入口与模型
   约束见官方 [FIM Completion](https://api-docs.deepseek.com/guides/fim_completion/)。
5. 五次响应都返回完整 usage/cache 字段；replay 相关请求观察到 cache hit，但单次小样本不能
   推导稳定命中率或性能收益。

## 明确边界

结果记录被标为 `record_class=protocol_canary`、`product_metric_eligible=false`、
`verified_success=null`。因此 5/5 只证明这组固定小请求在该时点与官方线上协议兼容：

- 它绕过 CodeWhale 生产 Agent loop，不证明规划、工具执行、上下文压缩、多 Agent、恢复或
  证据化完成能力；
- 它没有真实编码任务、确定性验收器或候选/基线 A/B，不能证明代码质量、任务成功率或能力提升；
- 它是单次小样本，不是延迟、稳定性、缓存收益或成本基准；
- 它未覆盖 streaming/SSE、错误分类、限流/超时、strict 不兼容回退等失败路径。

下一步应把这些协议契约迁入生产 DeepSeek Backend 的离线 fixture，再用固定真实编码任务和
确定性验收器建立 `verified_success` 基线。
