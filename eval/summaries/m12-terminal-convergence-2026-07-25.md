# M12 terminal convergence 跨任务复现

日期：2026-07-25

正式 acquisition：`complete`

trajectory 决定：`close_hypothesis_no_repeated_product_loss`

production candidate：无

## 结论

M12 证明 M11 `rust_cli` 的 `verified_workspace_without_terminal_receipt` 不是
CodeWhale Host 终止收敛损失。M11 app-server 使用的隔离 `HOME` 没有 rustup default
toolchain，canonical Host verifier 因而无法启动 Cargo；external verifier 继承了另一
环境并通过。原 analyzer 把这种评测环境不一致误投影为 product loss。

M12 没有修改 production crate、协议、State schema、模型路由、工具目录或完成权威。
唯一 corrected Harness 让 Host 与 external verifier 共享每个 arm 的隔离 `HOME`，并用
显式 `.rustup` link 和工具链 digest 冻结身份。两个独立任务各运行 3 次：

| task | verified | false success | physical requests | known cost | wall time |
|---|---:|---:|---:|---:|---:|
| `rust_endpoint` | 3/3 | 0 | 16 | USD 0.044280071 | 235,234 ms |
| `typescript_cache` | 3/3 | 0 | 16 | USD 0.025115595 | 127,015 ms |
| total | 6/6 | 0 | 32 | USD 0.069395666 | 362,249 ms |

每个 arm 都有 latest-revision Host receipt、external verifier pass、有效 fixed
Pro/high route、完整 request/usage/cache/cost accounting 和 exact credential-free
SQLite reopen。`typescript_cache` 有一次中间 `verifier_failed`，同一 canonical Run
按现有路径恢复并最终获得 receipt；没有重跑、补 mate 或额外 campaign retry。

结合 M11 与 M12 的 11 条 canonical Store trajectories 后：

- 8 verified success；
- 1 correct safety rejection；
- 0 false success；
- 1 `evaluation_environment_mismatch`（历史 M11 `rust_cli`）；
- 1 `measurement_incomplete`（历史 M11 `root_recovery`）；
- current product loss 集合为空。

因此“Host terminal controller 存在可重复 current loss”的假设关闭。没有证据授权新增
completion controller、request budget path、prompt、retry、Runtime/Store 状态或任何
production treatment。

## 冻结身份

- candidate revision：
  `5716713fe6f7083d8182c44b3ebf37bc3ad455aa`；
- candidate tree：
  `a07e8ffdb79b2e46bfdcdec98de7b0f681fd4281`；
- live admission revision：
  `0c05592980b7b92f23baa65fad678a06620e13b1`；
- binary：
  `/private/tmp/codewhale-m12-target/release/codewhale`；
- binary identity：
  `codewhale 0.8.68 (5716713fe6f7)`，
  `sha256:80a9210f6647aaba76945cb7464752dd17670329d1e3b46a5a5ef5d1db4e0f4e`，
  14,073,952 bytes；
- corrected Harness：
  `sha256:22a3823a59bd3176a2729c6e041f5750a09a5c7164393b32deb0f3ad769cf3cd`；
- analysis manifest：
  `sha256:c14aa1757ccd02be5e3f347012e5b983e76e138143db5f82a5540b89364dce45`；
- canonical report：
  `sha256:81015e01ffda4422f4348ee6a66a0b1a8f3cd9014648c4d8fe50b82d066cb0f6`。

正式 raw 为 ignored 0600：

```text
eval/results/m12-terminal-convergence-5716713fe6f7-v1.jsonl
sha256:a754e13feeaecbebaa20354106ce57dc77229213338cdf8d03f1e42e69cec043
15,751,036 bytes
39 complete hash-chained records
partial_tail_bytes = 0
```

raw 包含 1 个 plan、1 个 credential access fact、6 个 arm started、6 组
terminal/Store/reopen/verifier snapshots、6 个 arm result 和 1 个 summary。Key 未进入
protocol、Store、stderr、workspace、raw payload 或提交。

## 环境纠正

M11 的 Host verifier output 含稳定 rustup 签名：没有显式 toolchain 且隔离 `HOME`
没有 default。M12 每个 arm 使用：

```text
strategy = per_arm_isolated_home_with_explicit_rustup_link
host_and_external_share_home = true
rustc = 1.97.0
cargo = 1.97.0
```

Harness 在 credential access 前验证 symlink target、工具版本与 identity digest。Host
和 external verifier 使用同一环境，fixture Cargo target 仍位于仓库外。该纠正只修复
evaluation acquisition contract，不改变 CodeWhale production environment policy。

## Canonical trajectory 复算

同一 `scripts/eval-m9b-fixed-pro-regression.py --campaign m12
--trajectory-report` mode 同时读取冻结的 M11 与 M12 journal；没有第二 analyzer。它验证
0600 regular non-symlink file、schema、sequence、hash chain、file hash/size 与完整 tail，
并从 canonical Store facts 独立派生结果。

连续两次 report byte-identical。candidate result 为
`insufficient_repeated_current_loss`，`observed_losses=[]`，minimum independent
tasks=2。M12 summary 将产品决定表述为
`close_hypothesis_no_repeated_product_loss`：这是对 analyzer 空损失集合的产品解释，
不是新的状态真相。

## 正式协议复核

复核日期为 2026-07-25。唯一 production sender 继续使用官方 OpenAI-format
`https://api.deepseek.com` 与 `POST /chat/completions`，固定
`deepseek-v4-pro/high`。2026-07-24 退役的是 legacy
`deepseek-chat` / `deepseek-reasoner` alias，不是 ChatCompletions surface。

一手来源：

- [DeepSeek API Updates](https://api-docs.deepseek.com/updates/)
- [DeepSeek API](https://api-docs.deepseek.com/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)
- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/)

## 门禁

通过：

- M9-C、M11、M12 Harness self-test；
- M11 corrected trajectory report 与 M12 combined trajectory report 重复 byte identity；
- 两个 M12 fixture verifier fail-before、独立 pass-after 与 tree cleanliness；
- isolated shared toolchain identity probe；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- fixed route、root/read-only/Writer、RunStore exact reopen、process SIGKILL 与
  CLI/TUI/API production loopbacks；
- release binary credential-free dry-run；
- `git diff --check`。

所有 Cargo 命令均使用 `CARGO_INCREMENTAL=0` 和独立
`/private/tmp/codewhale-m12-target`。提交后精确删除 target、fixture proof 和临时 report；
ignored 0600 raw 作为正式证据保留。

## 非结论与下一步

M12 不证明：

- 所有长任务、语言或 verifier 环境都没有终止损失；
- M11 accounting-incomplete `root_recovery` 可以进入产品指标；
- 应新增 environment profile、终止 controller、预算、prompt 或 retry treatment；
- M10-A–G、Auto、Anthropic、FIM、swarm、multi-Writer、LLM Judge 或第二 Runtime/Store
  应重新进入产品。

后续不再围绕这个已关闭假设补样。新的 production vertical slice 必须先从新的
accounting-complete current trajectories 中证明同一 stable loss 至少跨两个独立 task
重复，并明确单一 owner、单变量 treatment 与 old-path deletion。
