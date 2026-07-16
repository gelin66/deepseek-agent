# M4-B 本地 API 单一 Runtime/RunStore 证据汇总

> 日期：2026-07-17
>
> 状态：严格完成
>
> 范围：`codewhale exec` 与 `codewhale app-server`；不代表交互 TUI 已迁移

## 1. 结论

M4-B 已让 Headless CLI 与本地 API 共用同一个生产 application service：

```text
exec / HTTP / SSE / stdio
  -> crates/app::AgentApplication
  -> one AgentRuntime
  -> one DeepSeekModelPort + fixed ProductionToolExecutor
  -> one SQLite RunStore
  -> canonical StoredRuntimeEvent
```

app-server 只负责认证、请求边界和 transport framing，不再拥有模型循环、工具实现、终态
判断、私有生命周期 Store 或 TUI 子进程。canonical start、get、events、resume、steer、
interrupt、cancel 已通过同 fixture parity、外部进程崩溃恢复、SSE 重连与无 Key 终态重放。

旧 `crates/core`、fake prompt、raw model proxy、direct tool route、`RuntimeBridge`、TUI sibling
process、私有 seq/thread map、事件翻译与旧 mobile/remote bridge 产品链已经物理删除，没有
保留 alias、双写、fallback 或长期兼容桥。

交互 TUI 仍使用旧 engine/session/task/runtime-thread 路径，因此整个 M4 尚未完成。下一切片
是 M4-C 交互 TUI 迁移与旧 loop/state 删除。

## 2. 候选身份

| 项目 | 值 |
|---|---|
| 被测代码提交 | `a534a824670b60c807c5abf399ea8674d4beb527` |
| Git tree | `72cc0895c14d7dedbd7b28c0ceab4f583a1518d8` |
| workspace version | `0.8.68` |
| 构建命令 | `cargo build --release --locked --offline -p codewhale-cli -p codewhale-tui` |
| 工具链 | Rustc `1.97.0`；Cargo `1.97.0` |
| launcher SHA-256 | `b1658eb0eba6ad539e19e2dfce7d80d4f6e81031ed0334efc33be7c9072ff767` |
| runtime SHA-256 | `1c0cf9ffccc1045db4b959529de58c59216050c559908bf2f3969137b867933c` |
| binary-pair SHA-256 | `sha256:2c73b402408eeff8835475dcabfefc676770c887c32edb68284d233a292b5fa6` |
| launcher size | 14,190,496 bytes |
| runtime size | 39,518,304 bytes |

该提交是全部 M4-B Rust 行为门禁和真实 canary 的被测源码。之后的文档提交只记录结果，
不改变上述源码或二进制身份；最终完成提交另由 clean detached checkout 的 locked/offline
release rebuild 核验。

## 3. 实现与删除证据

- `crates/app` 是 exec 与 app-server 唯一 production composition owner。
- `crates/runtime` 是唯一根/子 Agent loop；root/child 使用同一实现和 event contract。
- `crates/tools` 唯一持有 11 个固定 Host 工具及其 schema、identity 和 executor。
- `crates/deepseek` 唯一持有 official DeepSeek planner、transport、parser、请求预算和账本。
- `crates/state::StateStore` 是唯一 production SQLite `RunStore`；process-local active registry
  只保存 control handle，不保存可重放生命周期事实。
- `cargo tree -p codewhale-app-server --locked --offline` 不含 `core` 或 `tui`；app-server 的
  production dependency 只经 `app` 进入 runtime/deepseek/tools/state。
- `crates/core` 不存在；production 调用图不含 `handle_prompt`、`RuntimeBridge`、
  `spawn_engine`、`EngineEvent`、`monitor_turn` 或 `RuntimeThreadStore`。源码中的这些字面量只
  出现在防回归测试的 forbidden-symbol 列表。
- app-server 启动新增 TUI child process 为 0；新增第二个 Agent loop、持久 Store、工具实现
  或 compat adapter 均为 0。
- `RuntimeThreadStore` 仍有交互 TUI 的真实消费者，按 M4-C 删除，未为了目录整洁提前破坏。

## 4. 离线与本机验收

被测代码通过：

| 门禁 | 结果 |
|---|---:|
| `cargo fmt --all -- --check` | passed |
| `git diff --check` | passed |
| `./scripts/dev-deepseek-agent.sh focused` | passed |
| `codewhale-app` | 20 passed |
| `codewhale-app-server` | 21 passed |
| production `exec` acceptance | 23 passed |
| run surface parity | 1 passed |
| app-server external crash recovery | 3 passed；1 helper ignored |
| M4-A process crash matrix | 5 passed；1 helper ignored |
| 完整 TUI test，第 1 次 | 6,232 passed；3 ignored |
| 完整 TUI test，第 2 次 | 6,232 passed；3 ignored |
| workspace all-target Clippy `-D warnings` | passed |
| `cargo test --workspace --locked --offline --quiet` | passed |
| locked/offline release build | passed |

关键契约证据：

- exec、HTTP 与 stdio 对同一个 read-only fixture 产生相同的 normalized canonical events、
  terminal、usage 和 accounting；frame 可逐个映射到 Store event，0 丢失、0 虚构。
- SSE `id` 等于 Store sequence；按 `after_sequence` 重连只返回后缀，直到唯一 terminal。
- 外部 app-server 被 `SIGKILL` 后，第二个进程恢复同一 run；前缀保持、dead owner 回收、
  live owner 并发 resume 被拒绝，最终只有一个 terminal。
- 第三个无 Key 进程可读取 run/events 并重放 terminal，0 model request、0 tool call、0 new
  event；M4-A 的 exec crash/reopen contract 没有退化。

完整 workspace 首次暴露 Homebrew Python `3.14.6` 可启动但在原生模块导入期间挂起。最终
修复位于解释器选择边界：对每个绝对 PATH 候选执行 3 秒隔离 capability probe
`import json, math`，超时则 kill/reap 并继续选择后续健康 Python。它不是版本黑名单或增加
全局超时；resolver 测试、RLM 26/26 串行和 26/26 并行、两次完整 TUI 回归均通过。

## 5. 真实 DeepSeek release canary

真实 Key 只通过进程环境注入，未写入仓库、命令参数、脱敏摘要或测试产物；`key.txt` 由
`.gitignore` 明确忽略。运行前使用 offline fixture 覆盖协议，live 只作费用受限链路 canary。

| 项目 | 值 |
|---|---|
| 模型 | `deepseek-v4-pro` |
| API surface | `standard_chat` |
| 任务 | 必须且只调用一次 `read_file` 读取只读 marker |
| hard request limit | 4 |
| 实际模型请求 | root 2/2 completed；child 0；retry 0；in-flight 0 |
| 工具调用 | 1 |
| Token | input 5,516；output 159；总计 5,675 |
| cache | hit 2,688；miss 2,828；write 0 |
| reasoning | 41；replay 28 |
| 持久事件时长 | 3,908 ms |
| 费用 | USD `$0.001378254`；CNY `0.0095052` |
| accounting | complete；usage complete；billing known；priced |
| run event | 89；sequence 1..89；89 unique event id；1 terminal |
| tool catalog | `sha256:124b51b32f23fb1e2259f150e2384407e32e8f8609c2b4e486c6f8da78ef7dd6` |
| execution fingerprint | `sha256:5d22ff3ef605ea71b5a8d4e08301dc64c672b3b435daa6d84b92bb0c68e54617` |
| HTTP/SSE canonical digest | `2a412294585c16eb25aec08d3851b615920ee8454e85de490aaba8bdb354b1f7` |
| final Run API digest | `e9a767087dcb66dba4af5ce6d9f6088fc755085405bf046ea524ab864fc5649a` |
| final events digest | `2346b4d2a9d6791249f8cbedc5490a2cc29abdfaf566adfeac2740a27523aace` |
| workspace marker digest | `f9ac6c9c98bf00c70aafce3c3c2ab7d56280ebd7ba5954c670f7dd740620e71f` |
| product metric eligible | `false` |

HTTP events 与 SSE canonical 数组逐字节相同。app-server 重启后的 Get/Events 与 live 终态
逐字节相同，工作区 marker 未改变。移除所有常见 API Key 环境变量后：

1. app-server Get/Events/Resume 只重放现有 Store 内容；
2. 顶层 release `codewhale --model deepseek-v4-pro ... exec --resume <run_id>` 退出 0，输出原
   tool/result/content/terminal receipt，metadata 中的 runtime binary digest 与冻结 runtime
   一致；
3. SQLite 仍为 89 events、89 unique sequence、89 unique event id、1 terminal，lease 已释放、
   pending model attempt 为空、模型请求和工具调用没有增加。

普通工具调用走 Standard Chat 是正确行为，不应误判为缺少 Beta：官方 DeepSeek 的普通
[Tool Calls](https://api-docs.deepseek.com/guides/tool_calls) 属于 Chat；只有显式 Strict
Function Calling 使用 Beta Chat 且要求整组工具 schema strict-compatible。FIM 则是独立
Beta Completions surface。本 canary 因此验证 ordinary tool-call production path，不冒充
Strict 或 FIM surface canary。

## 6. 监督脚本诊断

两次脚本错误均被保留为诊断，不伪装成产品失败或成功，也没有产生额外 DeepSeek 请求：

- 首次无 Key app-server probe 使用了 macOS 非 canonical `/var/...` workspace，而 run 持久
  保存的是 `/private/var/...`。API 正确返回 HTTP 409 `run_environment_mismatch`，事件增量 0；
  改用 RunView 返回的 canonical workspace 后通过。
- 首次顶层 launcher 命令把全局 `--model` 放到 `exec` 后，CLI 在执行前正确拒绝。使用帮助
  中规定的参数顺序补跑后退出 0；补跑前后数据库不变。

这些问题属于一次性 supervisor 命令编排，不在产品代码中增加兼容分支。canary 实际只执行
一个 live run、两次 API 请求；所有后续诊断均为无 Key 本地读取。

## 7. 复杂度与非结论

从严格完成的 M4-A checkpoint `0a5b76a` 到 M4-B 被测代码 `a534a824`：

| 指标 | 值 |
|---|---:|
| changed files | 299 |
| additions | 33,892 |
| deletions | 55,761 |
| net | -21,869 |
| production app-server Agent loop | 0 |
| production app-server writable Store truth | 0 |
| production fixed Host tools | 11，共用 |

该 diff 包含 M4-B application/DeepSeek/tools/context 依赖纵切、旧控制面删除和测试，不是某个
crate 的纯净 LOC 归因。净删除只说明旧路径确实收敛，不能替代行为门禁。

本 canary 还暴露出真实效率债务：一个极小只读任务消耗 5,516 input token，production
prompt 中仍有大段英文 constitution、project context 与 compaction relay。这不属于 M4-B
transport/state 迁移，不能在本切片凭直觉删改。它应在 M7 用中文原生、stable prefix、上下文
预算和 compact prompt 做同任务同预算重复 A/B，以 verified success、工具错误、Token、时间
和费用决定保留或删除。

明确不能从本证据推出：

- 交互 TUI 已统一；
- M2 的 Strict Chat、FIM 和全部官方 surface canary 已完成；
- Provider 清理、全面汉化或中文 prompt 调优已完成；
- RepoGraph/EvidenceReceipt 或 writer-worktree 多 Agent 已完成；
- 一次 read-only canary 提升了真实编码成功率、Token 效率或恢复率统计值。

M4-B 的有效结论只有：本地 API 纵切、旧路径删除、协议投影、崩溃恢复、无 Key 重放和完整
回归门禁均已通过。下一目标应是 M4-C，而不是在 app-server 上叠加新兼容层。
