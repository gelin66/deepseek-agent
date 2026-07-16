# Canonical 本地 Run API

> 文档类别：当前生产接口。长期架构约束以
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md) 和 ADR 为准。

- 状态：M4-B 生产接口
- 更新日期：2026-07-17
- schema：`Run API`（`schema_version = 1`）

`codewhale app-server` 是本地程序接入 Agent 的唯一 API 入口。它不拥有模型循环、
工具实现或运行状态，只把 HTTP/SSE/stdio 命令交给
`crates/app::AgentApplication`，并原样投影 SQLite `RunStore` 中的 canonical event。

```text
HTTP / SSE / stdio
        |
crates/app-server        认证、限流、framing
        |
AgentApplication         start/get/events/resume/steer/interrupt/cancel
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

除 `/healthz` 外，HTTP route 都要求精确的 `Authorization: Bearer <token>`，除非服务以
loopback-only 的 `--insecure-no-auth` 启动。query token、备用 header 和浏览器页面注入均
不受支持。

POST body 必须是 canonical envelope，且 command kind 必须与 route 匹配。route 中的
`run_id` 必须与 envelope 中完全一致；未知字段、错 route、空输入和 schema drift 都返回
typed error，而不是猜测意图。

## 3. Command envelope

```json
{
  "schema_version": 1,
  "request_id": "client-request-42",
  "command": {
    "kind": "get",
    "run_id": "..."
  }
}
```

支持且只支持七种 command：

```text
start
get
events
resume
steer
interrupt
cancel
```

所有机器字段、command kind、error code、模型 ID 和工具名保持英文稳定。中文只用于人类
可读 message、文档与产品界面。

### start

客户端可控制：

- `input`、`workspace`；
- 官方模型或 auto route；
- reasoning、streaming、输出和请求预算；
- `ToolPolicy`、`RunLimits`；
- 本地 trust、approval 和 sandbox posture。

provider、API endpoint、系统提示词、工具目录 hash、execution fingerprint、actor、
accounting baseline 等恢复事实由 Host 组合，不能从 transport 注入。

简化示例：

```json
{
  "schema_version": 1,
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
      "allowed": ["read_file", "grep_files"],
      "denied": []
    },
    "controls": {
      "auto_approve": false,
      "trust_mode": false,
      "allow_sandbox_elevation": false
    }
  }
}
```

## 4. Response 与错误

每个请求返回相同 schema 的 `RunCommandResponse`：

```json
{
  "schema_version": 1,
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
- `accepted`：control command 已被当前进程接收；
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

## 6. 并发、控制与恢复

- `start`/`resume` 只有在 canonical Store 已持久创建或取得 lease 后才确认。
- 同进程 active run 通过一个 process-local control registry 接收 steer、interrupt、cancel；
  registry 不是持久事实。
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
