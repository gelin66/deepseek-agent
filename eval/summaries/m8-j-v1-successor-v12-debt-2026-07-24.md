# M8-J V1 successor 与 V12 历史债删除

- 日期：2026-07-24
- 分支：`deepseek-agent`
- clean baseline：`bce36a53ea570ab7149f01b93fbf8c5b5eb1d354`
- baseline tree：`c374e2a237aabefa7f4f994ae7adf90647263a01`
- frozen manifest commit：`9e644add04faae6fe8dc1d33a7fe52d48211dd2b`
- code candidate：`bcbc1616c276edaa977f3352cc8cff9a754168b4`
- code candidate tree：`abbdfd930b1a3e05f528b3ba75ab2885bb3b584f`
- protocol：Run API v11 / RuntimeEvent v17 / State v23 / exec-stream v3
- credential read / official API requests：false / 0

## 1. 结论

M8-E 的 8 pass / 8 blocked 是 frozen 历史输入，不反向修改。M8-G 已关闭 V06；
M8-H 只删除不可达 FIM production 半分支，V09 仍 blocked；M8-I 不改变 V1 item。
因此 M8-J 候选前 current matrix 是 9 pass / 7 blocked。

M8-J 关闭 V12。code candidate 先保留并验证 canonical `runs` / `resume` caller，再物理
删除可见但断开 Run API 的 `codewhale thread`、第二套 `threads` metadata truth、
`session_index.jsonl` sidecar 和无 production consumer 的 protocol DTO 岛。current
matrix 变为 10 pass / 6 blocked。

产品决策：

- `keep_canonical_runstore`；
- `shrink_delete_legacy_thread_truth`；
- `close_V12`。

这不是模型 treatment，也不产生可付费产品指标。live A/B 不适用，没有读取 Key 或调用
官方 API。

## 2. 冻结边界

manifest
[`m8-j-v1-successor-v12-debt-v1.json`](../manifests/m8-j-v1-successor-v12-debt-v1.json)
在 production 代码变化前冻结：

- baseline revision/tree、Cargo.lock、Rust 1.97.0 与关键 source hashes；
- candidate 只允许 V12 `blocked -> pass`；
- visible CLI command `18 -> 17`、State `v22 -> v23`；
- Run API v11、RuntimeEvent v17 与 exec-stream v3 不变；
- `maximum_reruns=0`、offline Cargo 和独立 target 目录；
- material model treatment=false、live A/B=`not_applicable_deterministic_deletion`。

manifest 的 `current_v1_exit_audit.items` 与 `projected_remaining_blockers` 是本切片的
冻结 item truth。其 `non_conclusions[2]` 对 V08/V10/V13/V15 的英文标签发生转置；该字段
不参与 acceptance 或 candidate selection，frozen 文件不事后修改。本总结与权威
ROADMAP/EVALUATION 按 M8-E item 定义记录正确映射。

## 3. 真实调用图

候选前的旧链：

```text
codewhale thread
  -> list/read/resume/fork/archive/unarchive/set-name/clear-name
  -> StateStore legacy Thread metadata API
  -> SQLite threads table
  -> session_index.jsonl
```

审计事实：

- 唯一 production consumer 是 CLI `run_thread_command`；
- resume/fork 委托已退休的 TUI thread 语义，不进入 canonical Run command；
- `threads.model_provider` 是第二 metadata truth，不参与 DeepSeek production request；
- Thread/App/Prompt/EventFrame DTO 只有 crate-local parity test，没有 production consumer；
- canonical `codewhale runs`、`codewhale resume` 和
  `exec --resume/--continue` 已覆盖真实 Run list/read/continue；
- `RunEnvironment.provider="deepseek"` 参与 environment fingerprint / replay safety；
- `NetworkPolicyAmendment` 与 `NetworkPolicyRuleAction` 有 execpolicy caller。

最后两项是当前安全/重放事实，不按字段名误删，也没有 Provider 产品模式或第二 backend。

## 4. 垂直切换与删除

single owners：

- persistent cutover：`crates/state`；
- product command catalog：`crates/cli`；
- wire contract deletion：`crates/protocol`。

candidate 完成：

1. 删除 `Commands::Thread`、八个 subcommand、dispatch、args 和专属简体中文文案；
2. 九种旧 spelling 在 config、TUI、Store、credential/model 前 fail closed；
3. 删除 Thread metadata/filter/source、CRUD、archive、memory/rollout lookup 与 session
   index reader/writer/compactor；
4. fresh State v23 不创建 `threads`；
5. v23 migration 在同一 `IMMEDIATE` transaction 中
   `DROP TABLE IF EXISTS threads`；
6. 删除无 consumer 的 Thread/App/Prompt/EventFrame protocol 岛和自证 parity test，
   保留真实 network policy types；
7. 保持 canonical `runs`、`resume`、app-server Run API、root/read-only/Writer 和唯一
   DeepSeek ChatCompletions backend。

code candidate 统计为 11 files、`+232/-1,797`，净删除 1,565 行；连同 frozen manifest
相对 baseline 为 12 files、`+398/-1,797`。

## 5. 恢复与失败证据

定向测试证明：

- exact v22 debt fixture 升级到 v23 后，current canonical run 原样 replay，旧表消失；
- pending Start、route audit、request ledger 与 accounting 不被 v23 改写；
- 注入同名 view 使 drop 失败时，user_version 和旧对象在同一事务回滚；
- fresh v23 schema 不包含 `threads`；
- canonical Run API/CLI 继续 list、read、resume 与 continue；
- process crash/reopen 与 SIGKILL window 不恢复第二状态真相。

v23 没有 compatibility reader、dual write 或旧命令 adapter。旧表删除是当前 migration 的
唯一作用，不借 schema bump 改写 RuntimeEvent。

## 6. 门禁

所有 Cargo 命令均使用：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8j-target
```

通过：

- targeted protocol/state/CLI check 与 tests；
- state unit/parity/run-store/process crash tests；
- old command pre-initialization fail-closed process test；
- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- frozen M8-E V1 exit contract：8/8；
- M7-C canonical edit Harness self-test：pass；
- `git diff --check`。

focused gate 覆盖 tools、DeepSeek、Runtime、app/app-server、exec、TUI canonical Run 与
PTY；workspace gates 覆盖 root/read-only/explicit Writer、SQLite reopen 和 process
recovery。没有联网协议变化或同 binary model treatment，因此没有 Key/API 阶段。

## 7. Current V1 matrix

| Item | Current |
|---|---|
| V01–V07 | pass |
| V08 multi-Writer | blocked |
| V09 FIM product scope | blocked |
| V10 RepoGraph | blocked |
| V11 | pass |
| V12 legacy provider/state vocabulary | pass |
| V13 imported-baseline coding A/B | blocked |
| V14 | pass |
| V15 billing-provable 中文 prompt A/B | blocked |
| V16 imported-baseline workflow-step A/B | blocked |

总计 10 pass / 6 blocked，V1 仍不可发布。

## 8. 非结论与下一切片

M8-J 不证明：

- verified coding success、false success、Token、cache、wall time 或费用改善；
- FIM、RepoGraph 或 multi-Writer 已完成或应直接实现；
- imported baseline 的缺失 accounting 可以补猜；
- 中文 prompt unknown billing 可以事后拼接；
- Auto 已获得 fixed Pro 非劣或约 20% 成本/时间收益。

下一切片应把 V08/V09/V10 作为 V1 产品范围 successor 独立审计：先要求真实 production
失败样本、consumer 和同 identity treatment，再只选择一个 blocker；若证据仍不成立，
应在现有 authority 中作明确 keep/shrink/hold/reject 范围决策，而不是恢复已删除的
FIM 半分支、增加第二 Runtime/Store 或堆无消费者架构。
