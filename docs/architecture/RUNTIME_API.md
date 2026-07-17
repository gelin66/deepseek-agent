# Canonical 本地 Run API

> 文档类别：当前生产接口。长期架构约束以
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md) 和 ADR 为准。

- 状态：M4-C C1 交互控制契约已冻结；交互 TUI 尚未切换
- 更新日期：2026-07-17
- schema：`Run API`（`schema_version = 2`）、`RuntimeEvent`（`schema_version = 4`）

`codewhale app-server` 是本地程序接入 Agent 的唯一 API 入口。它不拥有模型循环、
工具实现或运行状态，只把 HTTP/SSE/stdio 命令交给
`crates/app::AgentApplication`，并原样投影 SQLite `RunStore` 中的 canonical event。

```text
HTTP / SSE / stdio
        |
crates/app-server        认证、限流、framing
        |
AgentApplication         start/get/events/resume/steer/interrupt/cancel/resolve_interaction
        |
AgentRuntime             唯一根/子 Agent 执行内核
        |
SQLite RunStore          唯一持久事实
```

`exec` 与 app-server 使用同一个 production composition：官方 DeepSeek
`ModelPort`、固定工具目录、请求预算、提示词构建和 SQLite `RunStore` 都不在传输层重复。

## 1. 启动方式

### HTTP/SSE

```bash
codewhale app-server \
  --host 127.0.0.1 \
  --port 7878 \
  --auth-token "$TOKEN"
```

默认监听 `127.0.0.1:7878`。也可以通过
`CODEWHALE_APP_SERVER_TOKEN` 提供 token。

本机开发可显式关闭认证：

```bash
codewhale app-server --insecure-no-auth
```

无认证模式只能绑定 loopback 地址，不能与 `--auth-token` 同时使用。服务不会生成、打印
或持久化临时 token。`--cors-origin` 可重复提供；未配置时不开放跨域。

### stdio

```bash
codewhale app-server --stdio
```

stdio 每行接收一个 `RunCommandEnvelope`，每行返回一个 `RunCommandResponse`。它不是
JSON-RPC，也没有 method alias。`--stdio` 与 host、port、token、CORS、body-limit 等
HTTP 参数互斥。

已删除且不会兼容的旧入口包括：

- `codewhale serve --http` / `--mobile`；
- `codewhale app-server --http` / `--mobile`；
- mobile control page；
- legacy thread/job/session routes；
- raw `/v1/chat/completions` proxy；
- `/prompt` fake loop 和 `/tool` direct invoke；
- JSON-RPC app-server control surface。

MCP 与 ACP 是不同协议，仍由 `codewhale serve --mcp` 和
`codewhale serve --acp` 独立提供，不是 Run API alias。

## 2. HTTP surface

| Method | Path | 语义 |
|---|---|---|
| `GET` | `/healthz` | 公开进程健康检查 |
| `POST` | `/v1/runs` | start |
| `GET` | `/v1/runs/{run_id}` | get |
| `GET` | `/v1/runs/{run_id}/events?after_sequence=N` | events 或 SSE replay |
| `POST` | `/v1/runs/{run_id}/resume` | resume |
| `POST` | `/v1/runs/{run_id}/steer` | steer |
| `POST` | `/v1/runs/{run_id}/interrupt` | interrupt |
| `POST` | `/v1/runs/{run_id}/cancel` | cancel |
| `POST` | `/v1/runs/{run_id}/interactions/{interaction_id}/resolve` | resolve_interaction |

除 `/healthz` 外，HTTP route 都要求精确的 `Authorization: Bearer <token>`，除非服务以
loopback-only 的 `--insecure-no-auth` 启动。query token、备用 header 和浏览器页面注入均
不受支持。

POST body 必须是 canonical envelope，且 command kind 必须与 route 匹配。route 中的
`run_id` 必须与 envelope 中完全一致；交互响应 route 的 `interaction_id` 也必须与 envelope
一致。未知字段、错 route、空输入和 schema drift 都返回 typed error，而不是猜测意图。

## 3. Command envelope

```json
{
  "schema_version": 2,
  "request_id": "client-request-42",
  "command": {
    "kind": "get",
    "run_id": "..."
  }
}
```

支持且只支持八种 command：

```text
start
get
events
resume
steer
interrupt
cancel
resolve_interaction
```

所有机器字段、command kind、error code、模型 ID 和工具名保持英文稳定。中文只用于人类
可读 message、文档与产品界面。

### start

客户端可控制：

- `input`、`workspace`；
- 官方模型或 auto route；
- reasoning、streaming、输出和请求预算；
- `ToolPolicy`、`RunLimits`；
- 本地 trust、approval 和 sandbox posture；
- `controls.interactive`：允许 Runtime 发布并等待 durable approval / user-input 交互。

`interactive` 默认是 `false`。`interactive=true` 表示客户端承诺持续消费并解决 durable
interaction。只有交互 root run 且 `ToolPolicy` 允许时，Runtime 才暴露内建
`request_user_input`；child run 始终非交互。非交互运行遇到需审批工具时提交 canonical
rejected outcome，不静默执行，也不永久等待。

provider、API endpoint、系统提示词、工具目录 hash、execution fingerprint、actor、
accounting baseline 等恢复事实由 Host 组合，不能从 transport 注入。

简化示例：

```json
{
  "schema_version": 2,
  "request_id": "start-1",
  "command": {
    "kind": "start",
    "input": "读取 src/lib.rs，解释当前入口。",
    "workspace": "/absolute/project",
    "model": "deepseek-v4-flash",
    "reasoning_effort": "high",
    "max_output_tokens": 8192,
    "max_api_requests": 8,
    "streaming": true,
    "tool_policy": {
      "enabled": true,
      "allowed": ["read_file", "grep_files", "request_user_input"],
      "denied": []
    },
    "controls": {
      "auto_approve": false,
      "trust_mode": false,
      "allow_sandbox_elevation": false,
      "interactive": true
    }
  }
}
```

### resolve_interaction

approval 与 `request_user_input` 都使用同一个 durable interaction 协议。客户端先从
`InteractionRequested` 读取 `interaction_id` 和 typed prompt，再提交
`resolve_interaction`。Runtime 会校验交互仍处于 pending、ID 匹配且 response 类型符合原始
prompt；过期、重复、错 ID 和错 response 均返回 typed error。

```json
{
  "schema_version": 2,
  "request_id": "approve-42",
  "command": {
    "kind": "resolve_interaction",
    "run_id": "...",
    "interaction_id": "...",
    "response": {
      "kind": "approved"
    }
  }
}
```

approval prompt 的合法 response 是 `approved`、`denied`（可带 `reason`）或 `cancelled`；
user-input prompt 的合法 response 是 `answered`（带按 question ID 索引的 `answers`）或
`cancelled`。两类 response 不能混用。

`InteractionRequested` 固定携带 `interaction_id`、`operation_id`、`call_id`、`tool_name` 和
typed prompt。approval 必须在任何 `ToolExecutionStarted` 前提交并解决；展示给用户的参数
必须与 `ToolPrepared` 的规范化调用参数完全一致。

## 4. Response 与错误

每个请求返回相同 schema 的 `RunCommandResponse`：

```json
{
  "schema_version": 2,
  "request_id": "client-request-42",
  "result": {
    "kind": "run",
    "run": {}
  }
}
```

result 只有四类：

- `run`：当前 Store projection；
- `events`：严格位于 cursor 之后的 `StoredRuntimeEvent`；
- `accepted`：control command 对应的 canonical event 已提交，`last_sequence` 是该 event 的
  Store sequence；
- `error`：typed `RunApiError`。

稳定 error code：

```text
invalid_request
run_already_exists
run_not_found
run_already_running
run_not_active
run_recovery_required
run_terminal
run_environment_mismatch
event_cursor_ahead
interaction_not_pending
interaction_mismatch
interaction_already_resolved
invalid_interaction_response
run_store_failed
```

HTTP status 只是 transport 映射，程序判断必须以 `result.kind` 和 `error.code` 为准。

## 5. Event replay 与 SSE

普通 GET 返回 events result。请求头包含 `Accept: text/event-stream` 时返回 SSE：

```text
id: <StoredRuntimeEvent.sequence>
data: <完整 StoredRuntimeEvent JSON>
```

传输层不重命名、不聚合、不补造 event。`after_sequence=N` 只返回 `sequence > N` 的事件；
cursor 超过 Store 最新 sequence 时返回 `event_cursor_ahead`。客户端断线后用最后成功处理的
sequence 重连即可。

终态 event 是最后一个 event。终态 run 的 get/events/resume 只读 SQLite：不要求
DeepSeek Key，不调用模型或工具，也不追加新事件。

RuntimeEvent v4 的控制事实为：

- `InteractionRequested / InteractionResolved`：交互请求与 Host 响应；
- `SteerQueued / SteerApplied`：持久受理与安全边界应用，按 FIFO 执行；
- `ControlRequested`：interrupt/cancel 的持久意图；
- v3 的单一 `Steered` 已删除，不提供兼容 alias。

## 6. 并发、控制与恢复

- `start`/`resume` 只有在 canonical Store 已持久创建或取得 lease 后才确认。
- 同进程 active run 通过一个 process-local control registry 投递 steer、interrupt、cancel 和
  interaction response；registry 不是持久事实，命令回执和结果事件才是持久事实。
- steer 先提交 `SteerQueued`，只在完整模型响应与整组 tool/child outcome 之后的安全边界提交
  `SteerApplied` 并进入下一次模型请求；运行中 steer 不再制造恢复故障。
- interrupt/cancel 先提交 `ControlRequested` 再进入 typed terminal；steer、interrupt、
  cancel 和 resolve_interaction 使用持久 command receipt：同一 `request_id`、同一 payload
  重试返回原 sequence，不同 payload 复用同一 `request_id` 会被拒绝。该承诺不适用于
  `start`。
- command receipt 持久保存命令类型与规范化 payload；应用层事前检查和 Runtime 原子受理点
  都执行一致性校验，因此并发复用 `request_id` 也不能让两个不同命令同时成功。
- tool approval 和 `request_user_input` 先提交 `InteractionRequested`，Host 只有在
  `InteractionResolved` 已提交后才能继续。reducer/conformance 契约保证未解决交互可从
  Store 原样重放；对应 OS 进程崩溃窗口仍按 EVALUATION 的矩阵独立验收。
- 第二进程不能接管仍存活的 lease owner，返回 `run_already_running`。
- owner 进程死亡后，resume 重开同一个 `run_id`，保留已提交前缀并提升 execution epoch。
- 模型请求在途而账单未知时 fail closed 为 `RecoveryRequired`，不会盲目重发请求。
- Host/Store 保证 canonical terminal 至多一个；模型只提出完成候选。

## 7. 边界与门禁

app-server crate 只依赖 application/protocol 与 transport 库，不依赖 `crates/core` 或
`crates/tui`，也不启动 TUI 子进程。生产调用图不得重新出现：

```text
handle_prompt
RuntimeBridge
spawn_engine
EngineEvent
monitor_turn
RuntimeThreadStore
```

关键门禁：

```bash
cargo test -p codewhale-app-server --lib --locked
cargo test -p codewhale-app-server --test process_crash_recovery --locked
cargo test -p codewhale-tui --test run_surface_parity --locked
cargo tree -p codewhale-app-server --locked
```

跨入口 parity fixture 必须同时证明：exec/HTTP/SSE/stdio 的 normalized canonical event
kind/payload/order、terminal 和 accounting 一致，且 transport event 与 Store event
0 丢失、0 虚构。
