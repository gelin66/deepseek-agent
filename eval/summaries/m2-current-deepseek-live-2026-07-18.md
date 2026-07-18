# M2 当前 DeepSeek 官方协议 Canary（2026-07-18）

> 状态：6/6 请求通过；这是当前官方线上协议的外部兼容证据，不是 Agent 或编码能力指标。

## 可复核身份

- 本地结果：`eval/results/m2-current-deepseek-live-4bd6563b-20260718T023218Z.jsonl`
  （Git 忽略、权限 `0600`，不提交原始记录）
- 被测 revision：`4bd6563b471292ad4bc632b233f0e85b9088df15`，记录为干净工作树
- Harness：`scripts/eval-deepseek-live.py`
- Harness SHA-256：
  `60072577a2e865d82b4a4c025c5756365bef778656cbeb689ff16de624c04cf1`
- JSONL SHA-256：
  `2f5a1956b9fd18ac2b202f4911faef71cf88823ebeeead10f166af4ca6351b10`
- 结果：计划 6 次、实际发送 6 次、全部 HTTP 200、6/6 断言通过、0 失败
- 整套墙钟耗时：5.465 秒
- 价格快照：2026-07-16

Harness 只保存请求体摘要、usage、finish reason、断言和费用等脱敏结构化字段，不保存 Key、
提示正文、模型正文、reasoning 正文、工具参数或完整请求/响应。

## 请求结果

| Surface / 步骤 | 模型 | Finish | Cache hit / miss | 输出 | Reasoning | 请求耗时 | 估算费用 |
|---|---|---|---:|---:|---:|---:|---:|
| Standard Chat / non-thinking | `deepseek-v4-flash` | `stop` | 0 / 13 | 1 | 未报告 | 0.876 s | $0.000002100 |
| Standard Chat / thinking tool call | `deepseek-v4-flash` | `tool_calls` | 256 / 50 | 72 | 25 | 0.941 s | $0.000027877 |
| Standard Chat / exact replay | `deepseek-v4-flash` | `stop` | 256 / 137 | 23 | 19 | 0.759 s | $0.000026337 |
| Beta Strict Chat / non-thinking tool call | `deepseek-v4-flash` | `tool_calls` | 256 / 55 | 38 | 未报告 | 1.061 s | $0.000019057 |
| Beta Strict Chat / reasoning omitted replay | `deepseek-v4-flash` | `stop` | 0 / 83 | 3 | 未报告 | 0.727 s | $0.000012460 |
| Beta FIM | `deepseek-v4-pro` | `stop` | 0 / 18 | 4 | 0 | 1.093 s | $0.000011310 |
| **合计** | — | **6/6** | **768 / 356** | **141** | **已报告 44** | **5.465 s（整套）** | **$0.00009914** |

总输入为 1,124 tokens，总 tokens 为 1,265；每条记录都满足
`prompt_tokens = cache_hit_tokens + cache_miss_tokens`。Reasoning 合计只累加 API 明确报告的
数值，不把缺失字段当成零。

## 协议结论

1. Standard Chat 显式关闭 thinking 后返回 `stop`，正文非空且 reasoning 为空。
2. Thinking 工具轮返回非空 reasoning 和单个合法工具调用；下一请求原样回放
   `reasoning_content` 与 tool call 后被 API 接受并正常 `stop`。
3. Beta Strict Chat 的整组函数均为 `strict: true`，non-thinking 工具轮返回符合 schema 的
   参数；工具结果轮省略 `reasoning_content` 后被 API 接受，响应 reasoning 仍为空。
4. Beta FIM 通过独立 `/beta/completions` surface 返回非空结果和 `finish_reason=stop`。
5. 六次响应的 usage/cache 与 finish reason 均完整，0 failed；观察到 cache hit 只证明本次
   请求事实，不能推导稳定命中率或性能收益。

费用按 2026-07-16 价格快照和逐请求 usage 估算。JSON summary 为
`USD 0.00009914`，低于 `USD 0.01` 硬停止线；估算值不替代官方账单。

## 证据边界

每条记录均明确标记：

```text
record_class=protocol_canary
product_metric_eligible=false
verified_success=null
```

因此，这组结果只重新确认当前 M2 候选所依据的官方 Standard Chat、thinking exact replay、
Beta Strict non-thinking replay 和 Beta FIM wire 契约在该时点可用：

- Harness 直接调用官方 API，不经过 CodeWhale 生产 Agent loop；本记录本身不证明 Rust
  `RequestPlan`、transport、SSE、工具执行、恢复或多 Agent 链路已经正确接管；
- 没有真实编码任务、确定性 verifier 或 baseline/candidate A/B，不能证明任务成功率、代码
  质量、Token 效率、时延或成本获得提升；
- 这是一次费用受限的小样本，不是稳定性、延迟、缓存收益或价格基准；
- 它未覆盖 streaming/SSE、Strict 部分不兼容回退、FIM 超限、rate limit、timeout 和错误
  分类；这些仍须由离线 fixture、生产 sender 测试及相应受限 canary 分层证明。

历史 M1-B 5/5 记录继续保留为旧 revision 的协议基线；本次 6/6 新增了 Beta Strict
non-thinking 工具结果轮省略 reasoning 字段的显式线上证据，但不把协议兼容升级成产品能力
结论。
