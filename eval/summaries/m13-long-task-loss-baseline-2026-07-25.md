# M13 长任务恢复损失基线

日期：2026-07-25

当前阶段：`closed_inadmissible_observer_contract_instability`

production treatment：无

## 真实问题

M12 纠正评测环境后，current product loss 集合为空。M13 不从一次已经恢复的
`verifier_failed` 推导新 controller，而是先取得新的独立长任务轨迹，判断当前
owner-specific typed recovery 是否仍在跨文件调试、verifier failure、编辑冲突或显式
Writer 路径上产生可重复的 verified loss。

唯一 owner 仍是
`scripts/eval-m9b-fixed-pro-regression.py`。`--campaign m13` 只选择新的 manifest、
fixture、schedule 与观察契约；默认 M9-C、M11、M12 campaign 不变。production crate、
协议、State schema、工具目录、模型路由和完成权均无变化。

## 冻结 acquisition

任务集为 6 个独立临时 Git repository，每项 3 次，共 18 arms：

| task | lane | 观察层 |
|---|---|---|
| `rust_crossfile_proxy` | root | Rust 三文件配置调试 |
| `typescript_crossfile_cursor` | root | TypeScript 三文件 cursor/pagination 调试 |
| `python_verifier_recovery` | root | `verifier_failed -> write -> pass` |
| `python_ambiguous_edit` | root | 指定 `ambiguous_edit` 后缩窄恢复 |
| `typescript_patch_conflict` | root | stale-context `workspace_precondition` 后恢复 |
| `writer_config_migration` | explicit Writer | 四文件 worktree/verify/integrate/cleanup |

每个 fixture 在 Git init 前必须由 exact external verifier 证明 fail，且不得污染 fixture
tree。Host 与 external verifier 共享同一隔离 `HOME`、显式 `.rustup` identity 和 Rust
1.97.0。任务固定相同 immutable binary、`deepseek-v4-pro/high`、Standard Chat、
TaskContract、tool catalog、budget、verifier 与 schedule，`maximum_reruns=0`。

三类强制失败只用于测量 current typed feedback 后的恢复：

- `python_verifier_recovery` 必须先产生 `verifier_failed`；
- `python_ambiguous_edit` 的第一次写必须使用冻结的非唯一 search，产生
  `ambiguous_edit` 且 `side_effect=not_applied`；
- `typescript_patch_conflict` 的第一次写必须使用冻结 stale hunk，产生
  `workspace_precondition` 且 `side_effect=not_applied`。

观察器要求指定失败发生在第一次有效 mutation 和最终 Host receipt 之前。强制失败本身
不是“Host 制造错误”的产品结论。

## 停止与候选规则

每个 arm 必须先持久化 terminal、canonical Store、credential-free SQLite reopen、
external verifier 和 changed-file snapshot，再派生 label。accounting 必须
complete/usage-complete/sealed，physical started=completed、in-flight=0、
`billing_unknown=false`、unpriced=false。journal 为 ignored 0600、exclusive、fsynced、
hash-chained。

unknown billing、incomplete accounting、false success、identity/observer 歧义或成本门
触发时，在下一 arm 前停止；不得重跑、补 mate、续跑、重采样或拼接历史 raw。

只有一个相同 stable current product loss 至少跨两个独立 task ID 重复，且能明确：

1. 一个现有 owner；
2. 一个 treatment 变量；
3. 一个 deterministic fixture；
4. 一条 cutover 后可物理删除的旧路径；

才允许继续 production candidate audit。否则关闭假设，不写 production treatment。

## 官方协议复核

2026-07-25 复核官方一手资料后，唯一 production surface 仍是 OpenAI-format
`https://api.deepseek.com` + `POST /chat/completions`，
model=`deepseek-v4-pro`、reasoning=`high`、streaming=true。官方 change log 明确
2026-07-24 15:59 UTC 退役的是 `deepseek-chat` / `deepseek-reasoner` legacy alias，
不是 ChatCompletions surface。

冻结价格仍为 Pro 每百万 token：cache-hit input USD 0.003625、cache-miss input
USD 0.435、output USD 0.87；1M context、maximum output 384K。streaming 的
`include_usage` chunk 在 `[DONE]` 前到达；thinking tool-call turn 的
`reasoning_content` 必须完整回放。

一手来源：

- [DeepSeek API Updates](https://api-docs.deepseek.com/updates/)
- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

## 离线证据

- 6/6 fixture verifier fail-before；
- 6/6 独立 expected-solution pass-after，证明冻结 contract 可满足；
- 6/6 deterministic Git base 与 fixture hash 可重建；
- M13 Harness self-test 通过 schedule、TaskContract、forced-failure ordering、
  isolated toolchain、journal SIGKILL/tamper 与 secret-redaction checks；
- focused、fmt、`crates/tools` patch characterization、workspace strict clippy/test、
  production fixed route、root/read-only/Writer、SQLite reopen、process SIGKILL 和
  CLI/TUI/API parity 全部通过；
- release candidate `fc0960d6102953d78d5611c433b2c6e5af87225d`、tree
  `a3450bdc8c8022cd5c981eed0adbf47821b25640`、binary
  `sha256:f537994251f3da9c916fe1fd174ff44b72db747b6d303e0c576721f7d3519f7d`
  已在 credential-free preflight 中与 manifest、Harness、schedule、tasks 和 verifier
  environment exact match。

## 三次采集事实

三次采集都遵守 `maximum_reruns=0`，并在触发停止条件后没有启动下一 arm。三份 raw
均为 ignored 0600、exclusive、hash-chained；它们互不续跑、互不补 mate，也不拼接为
产品指标。

| attempt | completed arm results | physical requests | known cost | stop | classification |
|---|---:|---:|---:|---|---|
| v1 / `c819d90b` | 3 | 35 | USD 0.077142813 | `false_success_observed` | evaluator 将 Writer `allowed_paths` 输入顺序与 Runtime canonical set 顺序直接比较 |
| v2 / `4710d8e7` | 6 | 52 | USD 0.122068830 | `false_success_observed` | stale patch header 声明 3 行但 body 只有 2 行，production 正确返回 `patch_parse` |
| v3 / `fc0960d6` | 5 | 39 | USD 0.075725235 | `false_success_observed` | evaluator 错把 `run_verifiers` 失败要求为 `not_applied/after_correction` |

raw identity：

- v1：`sha256:dd43a30232181713f4df8a326c4fcabef48c4b3c37fd5a476132aa9f7ea6cef0`，
  10,149,617 bytes，21 records；
- v2：`sha256:c5f85cc052126a35d89ec5e15a962f2bde1e6060e477ef3e314215952732eee9`，
  25,435,709 bytes，39 records；
- v3：`sha256:1c4de16478c7eb8012cb6b733872d9e985e1596c904d94f13d85224bdef01fcb`，
  14,135,359 bytes，33 records。

v3 前四个 arms 的 terminal、external verifier、latest-revision Host receipt、route、
lane 和 accounting 均闭合，描述性结果为 4/4 verified、false success 0；第 5 个
`python_verifier_recovery` 同样 terminal completed、external verifier passed、
Host receipt=true、changed files exact、5/5 request usage/cost closed。它被 Harness
标成 false success 的唯一原因是 `required_failure_disposition`：
production `run_verifiers` 正确把执行过外部命令后的失败表示为
`side_effect=indeterminate`、`retry=unsafe`，而冻结 observer 错误要求
`not_applied/after_correction`。因此这不是 CodeWhale Host 接受未验证结果的证据。

## 决定

决定为 `close_m13_inadmissible_observer_contract_instability`：

- v1、v2、v3 都不是可用于 current product loss 标签或成功率基线的完整 acquisition；
- 不生成以 v3 为产品标签输入的 trajectory report，不从部分 arms 推导 recovery
  candidate；
- 不再修改 observer 后发起第四次付费采集；截至关闭共 126 个 physical requests，
  known cost USD 0.274936878；
- stable current product loss 没有被证明跨两个独立 task 重复，因此 production
  treatment、Runtime/Store/protocol/schema/tool/config delta 均为零；
- 保留 fixed-Pro/high、typed ToolOutcome、Host completion 和现有 recovery 行为；
  不新增 controller、retry、prompt、Auto、FIM、swarm 或第二状态真相。

下一切片必须先从现有 accounting-complete、observer-stable 的 canonical acquisition
取得候选，或先建立更小的纯离线 observer conformance corpus；不得把 M13 三份部分 raw
重新解释、续跑或拼接。
