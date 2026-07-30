# Canonical 本地 Run API

> 文档类别：当前生产接口。长期架构约束以
> [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md) 和 ADR 为准。

- 状态：M8-J legacy Thread truth 已删除，canonical RunStore 是唯一持久状态
- 更新日期：2026-07-30
- schema：`Run API`（`schema_version = 16`）、`RuntimeEvent`（writer/reader v23）、
  `State`（schema v29）、`dse.exec-stream`（v7）

`dse app-server` 是本地程序接入 Agent 的唯一 API 入口。它不拥有模型循环、
工具实现或运行状态，只把 HTTP/SSE/stdio 命令交给
`crates/app::AgentApplication`，并原样投影 SQLite `RunStore` 中的 canonical event。

```text
HTTP / SSE / stdio
        |
crates/app-server        认证、限流、framing
        |
AgentApplication         start/continue/list/recover/get/events/resume/control/accept
        |
        +------> AgentRuntime                  唯一根/只读子/Writer 执行内核
        |
        +------> ProductionAgentOrchestrator   Writer worktree/verify/integrate/cleanup
                         |
                         +----> AgentRuntime    同一实现，不是第二模型循环
        |
SQLite RunStore          唯一持久事实
```

`exec` 与 app-server 使用同一个 production composition：官方 DeepSeek
`ModelPort`、固定工具目录、请求预算、提示词构建和 SQLite `RunStore` 都不在传输层重复。

## 1. 启动方式

### HTTP/SSE

```bash
dse app-server \
  --host 127.0.0.1 \
  --port 7878 \
  --auth-token "$TOKEN"
```

默认监听 `127.0.0.1:7878`。也可以通过
`DSE_APP_SERVER_TOKEN` 提供 token。

本机开发可显式关闭认证：

```bash
dse app-server --insecure-no-auth
```

无认证模式只能绑定 loopback 地址，不能与 `--auth-token` 同时使用。服务不会生成、打印
或持久化临时 token。`--cors-origin` 可重复提供；未配置时不开放跨域。

### stdio

```bash
dse app-server --stdio
```

stdio 每行接收一个 `RunCommandEnvelope`，每行返回一个 `RunCommandResponse`。它不是
JSON-RPC，也没有 method alias。`--stdio` 与 host、port、token、CORS、body-limit 等
HTTP 参数互斥。

已删除且不会兼容的旧入口包括：

- `codewhale serve --http` / `--mobile`；
- `codewhale app-server --http` / `--mobile`；
- mobile control page；
- legacy thread/job/session routes；
- `codewhale thread` 及其 list/read/resume/fork/archive/name metadata 命令；
- raw `/v1/chat/completions` proxy；
- `/prompt` fake loop 和 `/tool` direct invoke；
- `codewhale serve --acp` / `--mcp`、`codewhale mcp-server` 和
  `codewhale-tui mcp add-self`；
- JSON-RPC app-server control surface。

DSE 仍可作为 MCP client 消费外部工具服务，但不再提供自托管 MCP server。
本地 Agent 控制面只使用 canonical app-server；不再提供绕过
`AgentApplication`、`AgentRuntime` 和 `RunStore` 的 ACP 模型/会话服务。

## 2. HTTP surface

| Method | Path | 语义 |
|---|---|---|
| `GET` | `/healthz` | 公开进程健康检查 |
| `POST` | `/v1/runs` | start |
| `GET` | `/v1/runs?workspace=...&limit=...` | list_roots；精确 workspace，默认 50、范围 1-200 |
| `GET` | `/v1/runs/pending-creations?workspace=...&limit=...` | list_pending_creations |
| `POST` | `/v1/runs/pending-creations/{creation_request_id}/recover` | recover_creation |
| `GET` | `/v1/runs/{run_id}` | get |
| `GET` | `/v1/runs/{run_id}/events?after_sequence=N` | events 或 SSE replay |
| `POST` | `/v1/runs/{run_id}/continue` | 从终态 root 创建新的 root run |
| `POST` | `/v1/runs/{run_id}/resume` | resume |
| `POST` | `/v1/runs/{run_id}/steer` | steer |
| `POST` | `/v1/runs/{run_id}/interrupt` | interrupt |
| `POST` | `/v1/runs/{run_id}/cancel` | cancel |
| `POST` | `/v1/runs/{run_id}/interactions/{interaction_id}/resolve` | resolve_interaction |
| `POST` | `/v1/runs/{run_id}/accept-completion` | accept_completion |

除 `/healthz` 外，HTTP route 都要求精确的 `Authorization: Bearer <token>`，除非服务以
loopback-only 的 `--insecure-no-auth` 启动。query token、备用 header 和浏览器页面注入均
不受支持。

POST body 必须是 canonical envelope，且 command kind 必须与 route 匹配。route 中的
`run_id` 必须与 envelope 中完全一致；交互响应 route 的 `interaction_id` 也必须与 envelope
一致。未知字段、错 route、空 task objective 和 schema drift 都返回 typed error，而不是
猜测意图。

## 3. Command envelope

```json
{
  "schema_version": 16,
  "request_id": "client-request-42",
  "command": {
    "kind": "get",
    "run_id": "..."
  }
}
```

支持且只支持十三种 command：

```text
start
continue
list_roots
list_pending_creations
recover_creation
get
events
resume
steer
interrupt
cancel
resolve_interaction
accept_completion
```

所有机器字段、command kind、error code、模型 ID 和工具名保持英文稳定。中文只用于人类
可读 message、文档与产品界面。

### start

客户端可控制：

- 结构化 `task`、`workspace`；
- 可省略模型以使用 fixed actor profile，或显式指定官方 Pro/Flash；不存在 Auto 模式；
- reasoning、streaming、输出和请求预算；
- `ToolPolicy`、`RunLimits`；
- 唯一 `controls.permission_mode` 与 write execution mode；
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
  "schema_version": 16,
  "request_id": "start-1",
  "command": {
    "kind": "start",
    "task": {
      "objective": "读取 src/lib.rs，解释当前入口。",
      "constraints": [],
      "non_goals": [],
      "acceptance": [
        {
          "kind": "host",
          "id": "host",
          "description": "由 Host 明确接受完成候选"
        }
      ]
    },
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
      "permission_mode": "ask",
      "write_execution_mode": "root",
      "interactive": true
    }
  }
}
```

`task` 在创建 run 时被 Host 冻结为带 generation ID 的 `TaskContract`。acceptance 可要求
显式 Host 接受，也可以要求一个精确的 deterministic verifier plan；同一任务至多有一个
verifier acceptance，多项命令门禁放进该 plan 的多个 step。verifier 的参数、program、
argv、workspace 内 cwd、environment 和 timeout 都是契约的一部分，不能由模型在验收时
改写。非默认结构化 task 的 objective、constraints、non-goals 和人类可读 acceptance
description 会以确定性的中文 user turn 进入 canonical transcript；精确 verifier 参数仍是
Host typed fact，不复制进 prompt。模型 Stop 只能产生可展示的 `Answered` candidate；默认
Host acceptance 必须由客户端/父 Host 另行提交 exact durable command，形成的
`HostAccepted` 不等于 deterministic `VerifiedCompleted` 或评测意义上的
`verified_success`。

`permission_mode` 只接受 `ask`、`agent`、`full_access`。它在 Run 创建时冻结，并进入
execution fingerprint；每次工具调用的 Host authorization decision 还绑定 exact
tool name、arguments digest 和 workspace revision。旧 bool/trust/sandbox/elevation
字段不是兼容输入。

### continue 与 list

`continue` 只接受终态 root run 和有效的新 `TaskDefinition`，并可用
`expected_workspace` 做精确 workspace 校验。它创建独立的新 root：新 run 的
`parent_run_id` 为空，
`continued_from_run_id` 指向 source；source transcript、event、terminal 和 accounting
保持不变。新 run 继承并由 Host 重新校验 model、提示词、工具策略、执行姿态和
model-visible context projection，同时重新开始本 run 的请求与用量记账。它不是 child
Agent，也不是同 run 的 `resume`。

```json
{
  "schema_version": 16,
  "request_id": "continue-42",
  "command": {
    "kind": "continue",
    "run_id": "...",
    "task": {
      "objective": "继续实现下一项验收条件。",
      "constraints": [],
      "non_goals": [],
      "acceptance": [
        {
          "kind": "host",
          "id": "host",
          "description": "由 Host 明确接受完成候选"
        }
      ]
    },
    "expected_workspace": "/absolute/project"
  }
}
```

`list_roots` 按精确 workspace 返回最近更新优先的 Agent root 摘要，不返回 child run。
`list_pending_creations` 返回创建事实尚未送达 `RunCreated` 的 durable intent；
`recover_creation` 只按原 `creation_request_id` 恢复该 intent，不接受客户端重建或修改
原命令。

完整 canonical transcript 始终 append-only；每次模型请求的 projection 由唯一
`crates/context` ContextBroker 确定。Runtime 在安全边界估算下一次输入，只有预计超过
基于官方 context/output capability 和 safety headroom 派生的 Host hard input limit 时，
才在同一个 root/child 内本地确定性压缩，并提交
`ContextCompactionCommitted`；压缩不调用模型、不消耗请求许可，也不创建新 run。Store
按 canonical transcript、当次真实 tool catalog、source digest 和 before/after Token
重算该事件；压缩后仍超限则 typed fail closed。Run API、HTTP/stdio 和 TUI 都没有手动
compact 命令，也没有提前百分比阈值或用户配置。

正式 M5-B A/B 只支持把该机制保留为 hard-limit 可靠性边界；主动压缩没有通过费用、时间
或成功率收益门槛，不得把它描述为效率优化。完整证据见
[M5-B ContextBroker 正式 A/B](../../eval/summaries/m5-context-broker-ab-2026-07-20.md)。

### resolve_interaction

approval 与 `request_user_input` 都使用同一个 durable interaction 协议。客户端先从
`InteractionRequested` 读取 `interaction_id` 和 typed prompt，再提交
`resolve_interaction`。Runtime 会校验交互仍处于 pending、ID 匹配且 response 类型符合原始
prompt；过期、重复、错 ID 和错 response 均返回 typed error。

```json
{
  "schema_version": 16,
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

### accept_completion

Host 只能接受当前 RunStore 中唯一 pending candidate。客户端必须从 `RunCompletion::Answered`
读取 exact `candidate_id`、`generation_id` 与 `workspace_state`，再提交：

```json
{
  "schema_version": 16,
  "request_id": "accept-answer-42",
  "command": {
    "kind": "accept_completion",
    "run_id": "...",
    "acceptance": {
      "candidate_id": "completion-17",
      "generation_id": "...",
      "workspace_state": {
        "generation": 3,
        "revision": { "status": "known", "sha256": "..." }
      }
    }
  }
}
```

Runtime 在提交前重新观察 workspace；只有 candidate、task generation 与最新 Known revision
完全匹配时才持久化 `HostCompletionAccepted` 和 `HostAcceptanceReceipt`。同一 request ID、
同一 payload 重试返回原 sequence，并可在 receipt 已提交但 terminal 尚未提交的 crash prefix
继续闭合同一 terminal；同 ID 不同 payload、foreign/stale candidate、错误 generation 或
workspace drift 分别 typed fail closed。该 receipt 只产生 `HostAccepted`，不能冒充
`EvidenceReceipt` 或 `VerifiedCompleted`。

## 4. Response 与错误

每个请求返回相同 schema 的 `RunCommandResponse`：

```json
{
  "schema_version": 16,
  "request_id": "client-request-42",
  "result": {
    "kind": "run",
    "run": {}
  }
}
```

result 只有六类：

- `run`：当前 Store projection；
- `events`：严格位于 cursor 之后的 `StoredRuntimeEvent`；
- `runs`：精确 workspace 下的 root-run 轻量列表；
- `pending_creations`：精确 workspace 下尚未完成投递的创建 intent；
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
run_continuation_invalid
run_environment_mismatch
event_cursor_ahead
interaction_not_pending
interaction_mismatch
interaction_already_resolved
invalid_interaction_response
completion_not_pending
completion_mismatch
completion_stale
run_store_failed
```

共享 broad code 的失败还可携带稳定 `error.reason`：

```text
deepseek_credential_missing
workspace_mismatch
provider_mismatch
tool_catalog_mismatch
execution_fingerprint_missing
execution_fingerprint_mismatch
```

启动、继续和恢复时的 DeepSeek credential、持久路由或执行环境失败不得只藏在人类
message 中。
HTTP status 只是 transport 映射，程序判断必须以 `result.kind`、`error.code` 和可选
`error.reason` 为准，不能解析中文消息前缀。

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

app-server 的 SSE/stdio 始终原样投影完整 stored event。`dse exec --output-format
stream-json` 的 `dse.exec-stream` v7 另提供 bounded `model_request_failed` 客户端事件：
它从同一 `ModelRequestFailed` 确定性投影 run/event/attempt identity、typed failure、
retry/stop decision 与紧凑 accounting，但不复制完整 request、system prompt、transcript
或 tool catalog。plain exec 把 transient retry progress 写到 stderr，模型内容继续只写
stdout；交互 TUI 以同一 stored fact 投影双语状态。客户端 sequence reconnect、终止后的
same-Run resume 与上游 DeepSeek request retry 是三种机制；当前协议不声称 partial SSE
续传。

RuntimeEvent v4 的控制事实为：

- `InteractionRequested / InteractionResolved`：交互请求与 Host 响应；
- `SteerQueued / SteerApplied`：持久受理与安全边界应用，按 FIFO 执行；
- `ControlRequested`：interrupt/cancel 的持久意图；
- v3 的单一 `Steered` 已删除，不提供兼容 alias。

RuntimeEvent v5 曾在 v4 基础上增加：

- `ContextCompactionPrepared`：摘要请求及其 source projection 已持久准备；
- `ContextCompactionInFlight`：物理摘要请求可能已经发出；
- `ContextCompactionAttemptFailed`：失败、重试决定和已知 accounting；
- `ContextCompactionCommitted`：新的 model-visible projection 与前后 Token 估算已提交。

RuntimeEvent v6 删除泛化的 `request_budget_exceeded` failure kind，改为两个互斥事实：

- `model_request_budget_exceeded`：`RuntimeBudget` 在进入 ModelPort 前拒绝第 N+1 个逻辑
  请求；不会故意发送超额物理请求，物理 accounting 的
  `api_request_budget_exhausted=false`、`api_request_rejected_exhausted=0`；
- `api_request_budget_exceeded`：DeepSeek transport 的物理 admission 确实观察到预算拒绝，
  必须有 `api_request_rejected_exhausted > 0`，并投影
  `api_request_budget_exhausted=true`。

达到物理 `started == limit` 本身不代表耗尽。RunStore 原样持久化这两个 kind，不根据计数
重新猜测终态。

RuntimeEvent v7 建立唯一任务完成与证据链：

- `RunCreated` 冻结带 generation ID 的 `TaskContract`；
- `ToolOutcomeCommitted.workspace_state` 与 `WorkspaceObserved` 持久化 Host 观测的单调
  workspace generation 和 revision；
- 模型 `Stop` 只产生 `CompletionProposed`，不能直接制造 terminal；
- 显式 verifier 依次提交 `HostVerificationPrepared / Started / Committed`；
- 只有 verifier spec、task generation、acceptance、当前 workspace generation/revision
  和实际可用 artifact 全部精确匹配时，Runtime 才签发 `EvidenceReceipt`；
- 不满足契约时提交 `CompletionRejected`；满足全部 acceptance 时，`Completed` terminal
  携带 Host 的 `CompletionDecision`。

任何 `MayWrite` 操作一旦可能开始就推进 workspace generation，即使最终内容 hash 与旧
revision 相同，旧 receipt 也不会复活。verifier Started 后进程死亡无法证明副作用边界时
fail closed 为 `RecoveryRequired`；Committed 后恢复只重放已提交事实，不重复 verifier 或
terminal。

RuntimeEvent v8 用唯一的本地确定性 `ContextCompactionCommitted` 替换旧摘要模型的
prepared/in-flight/failed 生命周期。事件持久化精确 source projection、selected entry、
真实 tool catalog、before/after Token 和未变化的 accounting；Store 独立重算后才接受。
因此 production 调用图只剩普通 Agent request 的一个 `ModelPort::stream` 调用点。

RuntimeEvent v9 继续收缩协议：删除手动/threshold trigger、特殊 compaction ID、独立
compaction purpose 和 compaction terminal。v10 当时直接替代 v9；当前 writer 与
reducer/Store reader 只接受 v22，不保留旧 event schema 兼容路径。

RuntimeEvent v10 建立唯一 Writer lifecycle：

- `AgentTaskPrepared` 冻结 parent/root、role profile、workspace access、精确 base、
  allowed paths、tool policy、预算与 verifier；
- workspace create/execute/collect 的 canonical 事件绑定唯一 task、child run、worktree、
  branch/base/final revision；
- `AgentOutcome` 同时携带模型 handoff 与 Host 观测的 changed files、diff digest、checks、
  artifacts、usage、terminal 和 integration 状态；
- integration prepared/started/failed/committed、post-integration verifier 与 cleanup/recovery
  都写入同一个 RunStore reducer；
- Writer receipt 不能完成 root；只有集成后最新根 generation/revision 上的新
  EvidenceReceipt 可以满足 root TaskContract。

RuntimeEvent v11-v14 依次把显式 Writer admission、verifier evidence policy/temporal
progress、精确 cleanup disposition 和 completion rejection 原因/required transition 变成
canonical facts；旧 event shape 直接退役，不保留兼容 reader。

RuntimeEvent v15 记录完整 DeepSeek response closure、脱敏 partial-output evidence、usage
观察和 replay-safe retry。RuntimeEvent v16 进一步要求每个失败 `ToolOutcome` 都携带稳定
`failure_code`，并保留 invocation、transport、operation、side effect、retry、evidence、
artifact 和 workspace revision。模型可见失败反馈只由该 outcome 确定性生成；成功输出保持
原样。`ToolPrepared` 后 crash 不能重复不安全调用，`ToolOutcomeCommitted` 后恢复只回放
已提交结果。

RuntimeEvent v17 曾让每个 `RunRequest` 和 child `AgentTask` 绑定 requested mode/reasoning
与 Host policy audit。RuntimeEvent v18 删除已退休的 Auto caller intent，只保留中性
`ModelRouteProfile::{Explicit, FixedActor}`、policy version 和稳定 reason code。
actual model/reasoning 仍只存于既有 canonical 字段，actor/workspace authority 仍只存于
`RunRequest`/`AgentTask`，不建立第二份派生真相。未显式指定模型的 root 为 Pro/high；
fixed-profile 普通 read-only child 为 Flash/high，Writer 为 Pro/high，typed recovery/
recheck/rework 为 Pro/max；显式 Pro/Flash/reasoning 由 child 精确继承，一个 Run 内
selection immutable。

RuntimeEvent v19 完成 DSE active identity cutover。RuntimeEvent v20 新增
`ToolAuthorizationCommitted`：每个可执行工具在 start 前必须先持久化 Host 决策；决策
绑定冻结的 `RunPermissionMode`、exact tool/arguments digest、workspace revision、
matched rule、risk 和 allow/ask/deny disposition。Ask 只有 exact durable interaction
resolve 后才能 start；deny 永不 start；reopen 复用已提交 decision，不能重新匹配当前
配置或重复副作用。

RuntimeEvent v21 让每个 `ToolAuthorizationCommitted` 必须绑定一个 typed
`ToolExecutionGrant`。普通模型工具只有 `Ordinary`；只有 Runtime 从 frozen
TaskContract 的 exact acceptance ID 与 canonical `VerifierSpec` 派生的
`TaskContractVerifier { acceptance_id, verifier_sha256 }` 才能请求 contract verifier
授权。tools 重新解析同一 spec，Store 在 reopen 时从 prepared abbreviated invocation
和 frozen contract 重建 exact invocation/grant；模型不能提供、扩张或伪造该事实。该
grant 不是第四个 permission 档位，也不授予普通 external path、cwd、network、write root
或 explicit-deny 绕过。

RuntimeEvent v22 把上游模型请求的安全重试收敛为唯一 durable Host 事实：
`ModelRequestFailed` 原子携带 typed response evidence、accounting 与
`Retry { prepared }` / `Stop { reason }`；prepared retry 冻结下一 attempt、1s/2s
backoff、`not_before_unix_ms` 和 limit。DeepSeek transport 不再拥有 retry loop。
prepared retry 重开只在 not-before 后发送一次；in-flight 重开因上游结果未知而
`RecoveryRequired`，不会盲发。

M7-B 的 `ModelRequestPrepared.request.tools` 是当次 actual advertised catalog 的唯一完整
持久事实，Run environment 另存 catalog hash，execution fingerprint 绑定当时的
`strict_tools` policy。DeepSeek surface 与 fallback reason 不重复写入 State；SQLite 重开后
由唯一 planner 从 exact request 确定性重建完整 `RequestPlan`。生产回环测试同时证明重开前后
request/plan 相等，并证明 strict policy 变化会改变 fingerprint 而在恢复边界 fail closed。

RuntimeEvent v23 在 v22 上增加 exact `HostCompletionAccepted`，并把模型 Stop、Host 接受与
verifier 证据分别投影为 `Answered`、`HostAccepted`、`VerifiedCompleted`；三者不能由客户端
互相升级。

Run API v16 只把上述 canonical facts 投影到 exec、TUI、HTTP/SSE/stdio，并为 DeepSeek
startup/environment 失败增加稳定 `reason`；没有 presentation-local worktree command、第二
事件总线或兼容 alias。State schema v29 复用 canonical
event/snapshot/lease/creation intent；v21 已退役无法补齐 typed tool failure 的旧 run，
v22 再退役缺少 route audit 的 materialized run，v23 物理删除旧 `threads` metadata 表。
v24 退休无法无损映射 old Auto/omitted-reasoning exact wire 的 v23 materialized run，只
保留能按 v18 command 直接反序列化的 pending Start；v25 完成 DSE identity retirement；
v26 不猜旧 bool/trust/sandbox/elevation tuple，直接退休无法无损映射为三档 permission
contract 的旧 materialized Run 与 pending Start；v27 再退休缺少 typed execution grant
的旧 materialized Run；v28 退休无法无损重建 durable retry schedule 的旧 materialized
Run，只保留能按当前 Start command 无损反序列化的 pending intent；v29 再退休无法证明
独立 Host acceptance、且旧 candidate 未绑定 workspace 的 materialized Run，只保留可按
v16 command 无损恢复的 pending Start intent。
旧 `session_index.jsonl`
writer/reader 已删除；没有 compatibility reader 或 dual write。

## 6. 并发、控制与恢复

- `start` 和 `continue` 在创建 run 前先把
  `request_id + normalized command digest -> reserved run_id` 及可恢复 creation intent
  持久写入 State schema v27（该 creation intent 表由 State schema v9 引入并保留；v13
  迁移会删除旧 `creation_kind = compact` 的 pending intent）；
  同 ID 同 payload 重试复用同一 reserved/created run，不同 payload 复用同一 ID 被拒绝。
  若 reservation 已存在但 continuation run 尚未创建，重试沿用同一 reserved
  run ID，不能再生成第二个 run。fixed route 在创建前没有 classifier 网络请求或未知
  计费窗口；v20 创建的新 pending Start 可恢复 exact reserved run。
- `start`/`resume` 只有在 canonical Store 已持久创建或取得 lease 后才确认。
- 同进程 active run 通过一个 process-local control registry 投递 steer、interrupt、cancel 和
  interaction response；registry 不是持久事实，命令回执和结果事件才是持久事实。
- steer 先提交 `SteerQueued`，只在完整模型响应与整组 tool/child outcome 之后的安全边界提交
  `SteerApplied` 并进入下一次模型请求；运行中 steer 不再制造恢复故障。
- interrupt/cancel 先提交 `ControlRequested` 再进入 typed terminal；steer、interrupt、
  cancel、resolve_interaction 和 accept_completion 使用持久 command receipt：同一
  `request_id`、同一 payload
  重试返回原 sequence，不同 payload 复用同一 `request_id` 会被拒绝。creation command
  使用上一条独立的 durable creation reservation，而不是 control receipt。
- command receipt 持久保存命令类型与规范化 payload；`accept_completion` 只有在
  `HostCompletionAccepted` 已提交后才返回 accepted sequence。应用层事前检查和 Runtime 原子受理点
  都执行一致性校验，因此并发复用 `request_id` 也不能让两个不同命令同时成功。
- tool approval 和 `request_user_input` 先提交 `InteractionRequested`，Host 只有在
  `InteractionResolved` 已提交后才能继续。reducer/conformance 契约保证未解决交互可从
  Store 原样重放；对应 OS 进程崩溃窗口仍按 EVALUATION 的矩阵独立验收。
- 第二进程不能接管仍存活的 lease owner，返回 `run_already_running`。
- owner 进程死亡后，resume 重开同一个 `run_id`，保留已提交前缀并提升 execution epoch。
- 模型请求在途而账单未知时 fail closed 为 `RecoveryRequired`，不会盲目重发请求。
- Host/Store 保证 canonical terminal 至多一个；模型只提出完成候选。
- M6-A Writer 只接纳 clean Git、精确 base、唯一 Writer 和冻结 allowed paths；dirty、
  base/branch 漂移、越界 diff、verifier 失败或恢复歧义 typed fail closed。
- Writer integration 使用唯一 fast-forward 策略、跨进程 lease、精确 Git `HEAD.lock` 与
  compare-and-swap；成功、失败、取消和恢复后的 cleanup 都按 canonical ownership 幂等执行。

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
cargo test -p dse-app-server --lib --locked
cargo test -p dse-app-server --test process_crash_recovery --locked
cargo test -p dse-tui --test run_surface_parity --locked
cargo tree -p dse-app-server --locked
```

跨入口 parity fixture 必须同时证明：exec/HTTP/SSE/stdio 的 normalized canonical event
kind/payload/order、terminal 和 accounting 一致，且 transport event 与 Store event
0 丢失、0 虚构。
