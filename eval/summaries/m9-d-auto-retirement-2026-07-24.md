# M9-D Auto retirement / fixed actor route cutover

- 日期：2026-07-24
- 起点：`deepseek-agent` clean `7459f74f010343ecdf92fc47e89406187a722cbd`
- 类型：用户接受的产品范围删除；无模型 treatment
- 结论：`retire_and_delete_auto / keep neutral actual-route audit / keep fixed-Pro effect-first`
- credential：未读取
- official DeepSeek API requests：0
- 外部付费 A/B：未执行

## 问题与切片契约

M8-I 已删除 pre-run Flash prompt classifier，M9-A 又因 formal matrix 不完整而从未准入
Auto 默认。但 current tree 仍让 model/reasoning Auto 穿过 config、CLI/TUI/API、
protocol、Host policy、route audit、显示和 evaluator 投影。用户明确决定 fixed-Pro
effect-first，不再开发、评测或保留 Auto。

本切片的唯一产品 route owner 是 `crates/app::ProductionFixedRoutePolicy`。验收为：

1. root 默认 Pro/high；
2. 显式 Pro/Flash/reasoning 保持原值并由 child 精确继承；
3. fixed-profile 普通 read-only child 为 Flash/high；
4. fixed-profile isolated Writer 为 Pro/high；
5. typed recovery/recheck/rework 为 Pro/max；
6. actual route 可在 RunStore 中精确重放；
7. current 用户面、配置和协议不存在 Auto；
8. 不增加 classifier request、关键词 heuristic、dynamic router、兼容 reader 或双写。

## 消费者审计与删除

审计把命中项分成三类：

- 已删除的 Auto 产品语义：`ReasoningEffort::Auto`、
  `ModelRouteRequestedMode::{Explicit, Auto}`、requested Auto/reasoning audit 字段、
  config/model normalizer、CLI/TUI model/reasoning input、TUI `auto_model` 状态与显示、
  exec Host-route分支、app Auto reason codes、current Harness 旧字段；
- 保留的通用事实：actual model、actual reasoning、actor/workspace authority、
  `ModelRouteProfile::{Explicit, FixedActor}`、policy version、稳定 reason code、physical
  request/usage/cost accounting；
- 保留的非模型 `auto`：`codewhale exec --auto` 工具/自动批准、approval policy、
  alternate screen、verifier profile、synchronized output 和其他无关产品语义。

旧 classifier 文件、prompt/parser/provider DTO 和未知计费 pre-route 分支已在 M8-I
删除，本切片没有恢复。M9-A-only runner 已不存在；active corrected fixed-Pro Harness
只迁移到中性 `profile=explicit` audit。无消费者
`core_command_surfaces.feature` 中的 `/model auto` scenario 被物理删除。

M8-I/M9-A frozen manifest、summary 和 ignored raw 未修改。它们记录发生时的历史事实，
但不再承载 current 产品承诺或未来 admission。

## 固定路由与协议切换

`RunRequest.model`、`reasoning_effort` 和 actor 仍是 canonical truth。
`ModelRouteAudit` 只保留 profile/policy/reason，不复制 selected model/reasoning。
explicit child 的 request 必须与 parent 持久化的 canonical `AgentTask`
model/reasoning/route 完全一致；fixed actor child 还必须匹配 workspace authority 对应的
固定 model/reasoning。

版本直接切换为：

```text
Run API v12
RuntimeEvent v18
State schema v24
exec-stream v3（不变）
```

旧 `ReasoningEffort::Auto` 在 DeepSeek wire 上表示省略 `reasoning_effort`。它不能无损
反推为 high 或 max，所以 State v24 不猜测：v23 materialized runs 全部退休；pending
Start 只有在能直接按 v18 command 反序列化时保留。迁移与 version bump 位于同一个
SQLite `IMMEDIATE` transaction；没有 compatibility reader 或 dual write。

## 离线证据

已通过：

- protocol/deepseek/runtime/state/orchestrator/app 测试：
  app 55、DeepSeek 51、Orchestrator 44、protocol 39、runtime store 40、
  runtime conformance 82、Writer 25、State unit/parity 8；
- State v24 migration：37/37，包括旧 materialized run 退休、safe pending Start 保留、
  incompatible Auto pending Start 删除和 future schema fail closed；
- process crash/reopen：38/38，1 个 process helper ignored；
- TUI unit：797/797，1 个独立 live-listener fixture ignored；
- production root/read-only/Writer/fixed-Pro inheritance/recovery/reopen loopback；
- corrected fixed-Pro Harness self-test与四个 journal fault window：
  `passed=true`、`key_accessed=false`、`network_accessed=false`；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- exec/HTTP/stdio surface parity 2/2、app-server 23/23、app-server process
  crash/reopen 3/3（1 个 external helper ignored）；
- `git diff --check`。

所有 Cargo 门禁均固定
`CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/codewhale-auto-retirement-target`。

## 取舍、非结论与下一步

保留 explicit Pro/Flash 是真实用户控制和 fixed-Pro formal baseline 所需 surface，不是
Auto。普通 fixed-profile read-only child 仍可使用 Flash，但没有新质量/成本收益结论；
它只能提交 handoff，由 Pro root 与 Host evidence gate 验收。

本切片不把 M9-A 的不完整 Auto/Pro observations 改写为 Auto 更好、更差或等价，也不把
范围删除包装成模型能力提升。删除 Auto 不读取 Key、不调用 API、不重开 M9-A。

下一步按更新后的长期 Goal 进入 billing-provable acquisition contract。该工作只服务
fixed-Pro 的真实效果/上下文/恢复实验；若第三方账单仍无法闭合，继续 fail closed，但不再
影响本切片的 Auto 删除。
