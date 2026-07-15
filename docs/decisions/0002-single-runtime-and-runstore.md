# ADR-0002：唯一 AgentRuntime、事件协议和 RunStore

- 状态：已接受
- 日期：2026-07-15

## 决策

根 Agent、子 Agent、CLI、TUI 和 API 共用一个 `AgentRuntime`。运行状态通过版本化
`RuntimeEvent` 表达，并由一个 append-only `RunStore` 持久化。

## 原因

当前真实循环位于 TUI，`crates/core` 不是生产执行内核，根/子 Agent 又有不同语义；
同时 session、task、runtime、Fleet 和 Lane 存在多份状态。这使修复、恢复和单项升级
产生连锁修改。

## 后果

- UI 退化为命令和事件客户端；
- DeepSeek、Context、Tools、State 通过小端口接入 Runtime；
- 所有聚合状态由事件 reducer 投影；
- 新 Runtime 接管后删除旧 TUI loop、假 core 路径、bridge 和重复状态文件；
- 临时双路径最多跨一个里程碑。
