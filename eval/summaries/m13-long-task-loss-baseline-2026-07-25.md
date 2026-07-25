# M13 长任务恢复损失基线

日期：2026-07-25

当前阶段：`offline_contract_frozen`

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

## 当前离线证据

- 6/6 fixture verifier fail-before；
- 6/6 独立 expected-solution pass-after，证明冻结 contract 可满足；
- 6/6 deterministic Git base 与 fixture hash 可重建；
- M13 Harness self-test 通过 schedule、TaskContract、forced-failure ordering、
  isolated toolchain、journal SIGKILL/tamper 与 secret-redaction checks；
- credential read=false、official API requests=0。

正式 candidate、admission、live acquisition、trajectory report 和 keep/close/rework
决定将在离线全量门禁通过后追加；当前不得据此声称产品成功率或恢复率改善。
