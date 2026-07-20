# M6-A isolated Writer 机制 canary

> 日期：2026-07-21
>
> 状态：`passed`；生产机制可用，不具备产品指标资格
>
> 范围：唯一 Orchestrator 下的单 Writer isolated worktree 完整闭环

## 1. 结论

M6-A 已证明当前生产链可以让根 Agent 通过唯一 `agent` 工具启动一个真正可写、物理隔离的
Writer，并完成：

```text
root
-> AgentTask
-> isolated worktree
-> Writer AgentRuntime
-> Host-observed diff/outcome
-> worktree exact verifier
-> guarded fast-forward integration
-> latest-root verifier
-> root completion
-> cleanup
```

根、只读 child 与 Writer 使用同一个 `AgentRuntime`、`RuntimeEvent`、固定工具目录和
SQLite `RunStore`。唯一 `ProductionAgentOrchestrator` 只拥有任务编排与 Git lifecycle，
没有第二模型循环、DeepSeek transport、工具实现、终态或私有 ledger。

本结果是一次 `mechanism_canary`，`product_metric_eligible=false`。它不能证明 Writer
相对单 Agent 提高成功率、降低 Token/费用或缩短时间，也不支持直接扩张为通用 DAG 或
多 Writer swarm。

## 2. 身份

| 项目 | 值 |
|---|---|
| schema | `codewhale.eval.m6-writer-canary.v1` |
| candidate revision | `a982a9a878587450aab57e87f4ec9df7751eb146` |
| workspace version | `0.8.68` |
| binary SHA-256 | `ccba6647be7e07b7f117bf1a28d888ffc4578215f7defcb2e40aa510e425566c` |
| model | `deepseek-v4-flash` |
| surface | Standard Chat |
| Run API | v7 |
| RuntimeEvent | v10 |
| raw result | `m6-a-isolated-writer-canary-2026-07-21.json`（本地忽略，`0600`） |

二进制从精确 revision 使用仓库外 `CARGO_TARGET_DIR` 和 `CARGO_INCREMENTAL=0` 构建后记录
hash。Key 只从本地权限 `0600` 文件注入子进程环境；不进入 argv、stdio frame、临时
State、fixture Git、摘要或提交。

## 3. 真实链路结果

| 指标 | 结果 |
|---|---:|
| status | passed |
| physical requests | 7 started / 7 completed / 0 in flight |
| transport retries | 0 |
| wall time | 29.047 秒 |
| input tokens | 21,611 |
| output tokens | 1,790 |
| cache hit / miss | 14,080 / 7,531 |
| reasoning / reasoning replay | 726 / 598 |
| cost | USD 0.001594964 / CNY 0.0113926 |

Provider surface ledger 的 7 个 response 都带 usage。按 actor 聚合的根级摘要只覆盖其归属
请求，因此本表使用完整 Standard Chat surface ledger，不把局部 actor 计数冒充总量。

Git 与证据结果：

- Writer base 精确绑定 fixture base commit；
- Writer 只允许并只修改 `answer.txt`；
- Host 封存 changed files、Writer commit 与 diff SHA-256；
- integration 为精确 fast-forward；
- 根仓最终 clean，临时 worktree 与 branch 均删除；
- 根 EvidenceReceipt 绑定 integration 后 workspace generation 3 和最新已知 revision；
- child receipt 不能直接满足 root TaskContract。

## 4. Canary 暴露并修复的生产问题

前三次真实运行证明离线 mock 没覆盖一个官方协议顺序缺口：canonical child handoff 可能
出现在 assistant tool call 和对应 tool result 之间。若直接按 transcript 顺序投影，
DeepSeek 会以 HTTP 400 拒绝该请求。

最终修复位于唯一 DeepSeek request projection：

1. 暂存夹在 tool call/result 之间的 user handoff；
2. 先精确回放所有对应 tool result；
3. 再恢复暂存 handoff；
4. 若 tool result 缺失，在 HTTP 前返回 typed `MissingToolResults`。

该修复没有把 Strict Function Calling 当成普通 Chat，也没有改坏 Beta Strict 的整目录
兼容性判断、原子 fallback 或独立 Beta FIM surface。

真实运行还暴露了 Writer 偶发只声明完成、不实际写文件的问题。处理方式不是放宽完成门禁：
TaskContract 继续拒绝空 diff；Writer 的最小中文 profile 明确要求先读取、实际写入、复读
并取得工具证据。根 Agent 仍可做只读侦察和 review，但 harness 拒绝根直接写入，并要求
恰好一次 Writer delegation。

## 5. 复杂度与删除

从 M5-B 文档检查点 `4f368ab5` 到 canary 检查点 `a982a9a8` 的完整代码、测试和评测
harness diff 为 `+19,035/-1,080`，净增 17,955 行。该数字包含：

- 4,377 行净增的独立测试文件；
- 1,413 行费用、身份、Git lifecycle、计量和敏感信息验收 harness；
- 大量与生产实现同文件的 `#[cfg(test)]` 反例和 crash tests。

按生产实现与测试/评测代码拆分，生产净增约 5.6k 行；主要复杂度集中在唯一 canonical
contract、Git CAS/lease/ownership/recovery 与三平台 sandbox，而不是新增产品模式。
新增模型可见工具、模型循环、Store、事件总线、JSON ledger、配置开关和兼容路径均为 0。

切换后物理删除 Lane 的第二份 worktree 实现及重复 worktree runtime/registry 字段；仍有
真实消费者的 Lane/Fleet 生命周期没有在本切片无证据删除。M6-B 必须把这份复杂度纳入
收益判定，不能因为代码已经存在就默认保留或继续扩张。

## 6. 已知边界与下一决策

当前只支持一个 root Integrator、最多一个 Writer、clean Git workspace、精确 base、
冻结 allowed paths 和唯一 fast-forward integration。dirty/non-Git/unborn/不受支持仓库、
base/branch 变化、越界 diff、空 diff、verifier 失败与恢复歧义都会 fail closed。

M6-A 没有实现多 Writer、通用 DAG、脏工作区快照、自动冲突修复或远程 worker。M6-B 应先
执行 current single-agent 与 writer-agent 的冻结任务正式 A/B；只有预注册净收益门槛
通过，才值得以独立切片实现最多两个、范围不重叠的 Writer。
