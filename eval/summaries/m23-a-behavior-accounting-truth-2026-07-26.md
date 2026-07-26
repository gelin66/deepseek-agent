# M23-A behavior/accounting truth orthogonality

- 日期：2026-07-26
- 起点：`83c1c99ba5f86c0146a0cd19862c98080a3fe7f2`
- 决策：`keep_orthogonal_behavior_accounting_truth`
- production delta：0
- Key / official API / external network：0 / 0 / 0

## 真实问题

corrected fixed-Pro Harness 正确地在 `billing_unknown` 或 usage 不完整时停止下一付费请求，
但只读 analyzer 还把所有缺少完整 `arm_result` 的 trajectory 统一投影为
`measurement_incomplete`。这会把两个不同事实混在一起：

- production 已形成 typed failed/blocked terminal、Store reopen、workspace/verifier
  outcome，只是 usage/cost 不完整；
- Harness deadline、机器或基础设施中断，production outcome 根本没有形成。

前者是可独立学习的产品行为，后者才是测量中断。两者都不能伪造费用。

## Contract 与唯一 owner

ADR-0011 接受两个互不推导的状态轴：

```text
behavior_status:
  verified_success
  correct_safety_rejection
  verified_product_failure
  measurement_interruption
  invalid

accounting_status:
  complete
  usage_incomplete
  billing_unknown
  unpriced
```

规范 owner 仍是 `docs/product/EVALUATION.md`；执行 owner 仍是
`scripts/eval-m9b-fixed-pro-regression.py`。production 的 AgentRuntime、RuntimeEvent、
RunStore、DeepSeek sender/accounting、TaskContract/EvidenceReceipt、route 与工具目录没有
变化。

行为标签只使用冻结 identity/task、canonical Store、credential-free SQLite reopen、
external verifier、latest-revision Host receipt、terminal 与 actor contract。accounting
标签只使用 physical request/usage/pricing/seal ledger。非 complete accounting 继续停止
下一付费 request，并禁止 Token、费用和 full-utility aggregate。

## 实现与旧路径删除

- 新增 10-case credential-free corpus 与冻结 manifest；
- analyzer 现在连接 `canonical_store_snapshot`、`sqlite_reopen_snapshot` 和
  `verifier_snapshot`，要求 reopen facts exact；
- behavior 与 accounting 分别聚合，owner/cause 共同构成重复损失 key；
- 删除 `trajectory_loss_projection(arm_result=None) -> measurement_incomplete` 及其混合
  product-loss 分支；
- 不增加第二 evaluator、journal writer、compatibility reader 或双写；
- `eval/results`、ignored raw、旧 manifest/summary 和历史决策均未修改。

冻结身份：

```text
harness sha256
  8da1576d1e9013e88625279c57beac52fe69ffb4cce01e2f32e66fea9d2f96c9
manifest sha256
  b1b587ef1a0b8eb77ad3b777dfc45a2ac167d6cd088eb3dd610ace22a6e83803
corpus sha256
  a942c74e77494ab9ce3412e6f9daf82d7a747ea17369a6a2fb7cd95529a3b753
```

## 离线证据

10/10 case 通过，报告在同一进程内重复派生 byte-identical：

| axis | status | count |
|---|---|---:|
| behavior | verified success | 3 |
| behavior | correct safety rejection | 1 |
| behavior | verified product failure | 4 |
| behavior | measurement interruption | 1 |
| behavior | invalid | 1 |
| accounting | complete | 5 |
| accounting | usage incomplete | 2 |
| accounting | billing unknown | 2 |
| accounting | unpriced | 1 |

corpus 故意包含一个 false-success 反例，投影精确为 1；这不是产品任务样本，而是证明
zero-tolerance label 没有被新状态轴吞掉。

现有 frozen journal 的只读重算还证明：

- M19 typed `deepseek_transport` terminal 是 behavior product failure，同时 accounting
  为 billing unknown；
- M20B 是一个 verified success + 一个 typed transport product failure，accounting
  分别为 complete + usage incomplete；
- M15 的 historical reference-file 假阳性在当前 behavior truth 中为 12 verified、
  2 correct rejection、2真实 failure、false success 0；root 与 Writer failure 因 owner
  不同，不会被合并为重复损失；
- M11、M12、M18 的 current journal 也能在 exact reopen 约束下重算。

这些是 ADR-0011 下的新派生报告，不回写或改名历史 evidence，也不改变当时的 admission
decision。

## 门禁

本切片要求并通过：

```text
python3 -m py_compile scripts/eval-m9b-fixed-pro-regression.py
--truth-conformance（连续两次 byte-identical）
--observer-conformance
--acceptance-conformance
M11/M12/M15/M18/M19/M20B --trajectory-report
M20B --self-test（四个 journal SIGKILL window）
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、`CARGO_NET_OFFLINE=true` 和独立
`/private/tmp/dse-m23a-target`。结束后精确删除该 target。

## 非结论与下一步

M23-A 不建立 18–24 task Hardness baseline，不证明 fixed Pro/high 的困难任务能力，不比较
high/max，也不授权 ApplicationProbe、symbol/reference localization、VerifiedMilestone
或 Tool ACI。下一独立切片是 M23-B：先冻结、自证并离线门禁新的私有 Hardness task set，
然后只允许 current fixed Pro/high control acquisition；没有重复 stable owner/cause 时停止
生产功能开发。
