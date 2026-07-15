# DeepSeek Agent 评测规范

> 文档类别：产品权威。仅定义能力的验证与保留门槛。

- 状态：V1 评测契约
- 上次更新：2026-07-15

本文件决定一项能力是否真正提升产品。它不是排行榜，也不以“模型回答看起来不错”
作为结论。

## 1. 评测目标

北极星指标：

```text
已验证任务成功率 / Token / 时间 / 代码复杂度
```

评测回答四个问题：

1. 任务是否真实完成？
2. Agent 是否错误地宣称完成？
3. 成功所需成本和时间是否合理？
4. 新能力带来的维护复杂度是否值得？

## 2. 基线

必须保留至少三条可重复基线：

1. 导入时的 CodeWhale `352e86a6` + 官方 DeepSeek。
2. 当前稳定单 Agent Runtime。
3. 当前候选实现。

涉及多 Agent、RepoGraph、FIM、Strict 或 compaction 的改动还必须有关闭该能力的
A/B 对照。

## 3. 任务集

固定任务按难度和能力分组，仓库 fixture 必须可重置且不依赖未保存的本地状态。

### A. 协议正确性

- 普通 Chat；
- thinking + tool calls；
- reasoning/tool history replay；
- Strict 全兼容；
- Strict 部分不兼容后的普通工具回退；
- FIM 正常和超限；
- SSE 半帧、多帧、畸形帧和结束帧；
- retry、rate limit、timeout；
- `length`、过滤和资源不足终止；
- cache usage 和 cost。

### B. 基础编码

- 单文件缺陷；
- 小型功能；
- 精确搜索与读取；
- patch 失败恢复；
- 编译错误修复；
- 单元测试修复；
- 文档和配置修改。

### C. 仓库理解

- 跨文件调用定位；
- 配置到运行时影响追踪；
- 接口实现定位；
- 测试影响分析；
- 大仓库固定 Token 预算；
- RepoGraph 与浅层 project map 对照。

### D. 长任务和恢复

- context compaction 后保持目标；
- 多次工具循环；
- 中途 steer；
- cancel/interrupt；
- 进程被终止后的 resume；
- snapshot/replay 一致性；
- 不重复提交终态和写工具。

### E. 验证与假成功

- 测试命令失败；
- 工具函数返回但业务操作失败；
- 修改后使用旧证据；
- reasoning-only/空响应；
- step budget 耗尽；
- 项目没有标准测试命令；
- 分析/文档任务不应被机械测试门卡死。

### F. 多 Agent

- 并行只读调查；
- Explorer + Implementer；
- Implementer + Verifier；
- 两个独立 worktree writer；
- 文件范围冲突；
- Agent 失败、超时、取消和恢复；
- Integrator review/merge；
- 单 Agent 与多 Agent 净收益对照。

## 4. 运行变体

根据能力阶段选择，不要求每次运行全部组合：

```text
single-agent
single-agent + RepoGraph
single-agent + candidate edit protocol
explorers + implementer
implementer + verifier
worktree writers + integrator
strict on/off
FIM on/off
compaction strategy A/B
```

模型、API surface、reasoning 参数、上下文预算、工具目录摘要和代码提交必须记录，
确保结果可解释。

## 5. 指标

每次运行至少记录：

```text
run_id
task_id
git_commit
workspace_revision
model
api_surface
verified_success
terminal_state
false_success
wall_time_ms
input_tokens
output_tokens
cache_hit_tokens
api_cost
model_turns
tool_calls
tool_failures
patch_failures
verification_runs
test_regressions
resume_success
worktree_conflicts
changed_files
evidence
```

复杂度另行记录：

- 新增生产代码；
- 删除生产代码；
- 新增状态类型；
- 新增持久化真相；
- 新增模型可见工具；
- 新增长期依赖；
- 新增运行路径。

新增第二个状态真相或第二个 Agent loop 默认视为架构失败，不由成功率小幅提升抵消。

## 6. 真实性判定

`verified_success` 只能来自任务定义的验收器，例如：

- 测试、编译、lint；
- 确定性输出检查；
- API/CLI 行为检查；
- 结构化 diff 断言；
- 人工预先定义的 code-review rubric。

模型自评、额外 critic 模型或自然语言结论只能作为 review 信号，不能单独成为成功证据。

所有证据必须绑定生成时的 workspace revision；后续写入使旧证据失效。

## 7. 能力保留门槛

能力进入稳定核心必须同时满足：

1. 在目标任务组中提升 verified success，或在成功率不退化时显著降低成本/时间；
2. 不增加 false-success；
3. 默认用户流程不变复杂；
4. 失败模式明确且可恢复；
5. 没有产生重复 Runtime、Store、Task 或产品概念；
6. 生产复杂度与收益成比例；
7. 有关闭能力的 A/B 对照；
8. 有回归测试和删除方案。

若收益只存在于极少任务，应作为按需策略，而不是默认全局行为。

## 8. 当前 `verify` 实验的判定

当前 WIP 中的独立 `verify` 工具使用额外 DeepSeek 调用评审 bounded diff/file evidence。
它不等于 test/verifier receipt，必须单独评估：

- 是否提高真实缺陷发现率；
- 是否降低 false-success；
- 是否只是重复主 Agent 的判断；
- 成本和延迟；
- 是否应默认关闭、按需启用、缩小或删除。

在该实验通过本规范前，不得让它成为所有任务的强制完成阶段。

## 9. DeepSeek live canary

默认 CI 使用离线 fixture。真实 API 测试必须显式启用，并设置：

- 最大运行数；
- 最大输入/输出 Token；
- 最大费用；
- 单请求和整套超时；
- retry 上限；
- 日志脱敏；
- 结果归档。

live canary 重点验证官方协议可能变化的部分，而不是替代离线测试。

## 10. 结果与决策记录

建议结果格式：

```text
eval/
  manifests/
  fixtures/
  tasks/
  results/       # 默认不提交原始大日志
  summaries/     # 提交可复核汇总
```

每个里程碑在 [ROADMAP.md](ROADMAP.md) 中记录：

- 基线；
- 候选版本；
- 结果摘要；
- 保留、重做/缩小、删除/推迟结论；
- 下一步。
