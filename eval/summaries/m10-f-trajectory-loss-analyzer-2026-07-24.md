# M10-F Trajectory Loss Analyzer（2026-07-24）

## 结论

M10-F 的正式决定是：

```text
keep_read_only_analyzer
insufficient_current_loss_evidence_for_a_new_product_candidate
do not restore Working-Set, Acceptance Progress, Recovery Controller or Environment Profile
do not read a credential or call the official API
advance only to the final read-only parallel evidence audit
```

corrected Harness 现在有一个只读 `--trajectory-report` mode。它验证三份 frozen raw 的
0600 regular-file、schema、sequence、hash chain、file hash、size 与完整 tail，然后只从
canonical Store snapshots 和 Runtime events 派生 task stratum、context/tool source、
typed failure、evidence deficit、recovery outcome 与重复 observation。输出不包含 prompt、
tool arguments/content、credential、evaluation id，也不进入 production。

报告的最终结论是 `insufficient_current_loss_evidence`。当前 controls 没有一个满足
“重复出现、单一 owner、可离线复现、能改变一个变量并删除旧路”的新产品损失，所以不能
为了继续开发而强行指定 candidate。

预注册契约见
[m10-f-trajectory-loss-analyzer-v1.json](../manifests/m10-f-trajectory-loss-analyzer-v1.json)。

## 切片契约

- 真实问题：M10-D/E 仍依赖 one-off `jq` 和 evaluator-specific 人工解释，候选选择不能
  从同一 canonical evidence 独立重算；
- 验收：验证每个 frozen journal；按 actor 分离 observation；workspace mutation 后重置
  epoch；只有上一 observation 仍在当前 ModelRequest 时才算 model-visible duplicate；
  区分 canonical evidence 与 frozen label；重复运行 canonical JSON 字节一致；零 Key/
  network/raw disclosure；
- owner：现有 `scripts/eval-m9b-fixed-pro-regression.py` corrected Harness；
- 替代：M10-D/E 的一次性 jq loss aggregation；
- cutover：保留一个 analyzer mode，不新增 runner/script/store/protocol；
- 删除条件：如果不能复算已知结论或只输出无消费者统计，则删除 mode。

## 输入身份

只读输入：

| campaign | journal sha256 | canonical snapshots |
| --- | --- | ---: |
| M9-C fixed-Pro | `c73a01a...e05b77` | 9 |
| M10-A context pack | `0c81935b...0a0d66` | 22 |
| M10-B Working-Set | `69b44fa8...da1d0` | 6 |

完整 hash/size/schema 在 manifest 中冻结。报告没有拼接这些 raw 为新的 product A/B；
它只做 loss identification。

最终 identity：

- Harness：
  `sha256:2c61b54abed036c20578a4e945e24cfd2f85ebda869aee697f7f95e15cd59a0f`；
- manifest：
  `sha256:12a5a3d890d4b79f6306bda2244074460b2d9d540358cdac303715125687974a`；
- canonical report：
  `sha256:765629c3a889bc44aabd9d6b34c42bb93ea32ed5aa2b909bf90a15b36203483e`。

复算命令：

```bash
python3 -I -B scripts/eval-m9b-fixed-pro-regression.py --trajectory-report
```

## 聚合结果

### Strata 与 acquisition

- 37 个 canonical trajectories；
- 34 个 arm results；
- 3 个 `accounting_incomplete` acquisition stops；
- 210 个 physical model requests；
- root single/migration/recovery 各 6，read-only 7，Writer 7，safety 5。

3 个缺失 label 恰好对应 M9-C、M10-A、M10-B 的既有 accounting stop，不是新的产品
失败，也不能由 analyzer 补样或改写为 success/cost。

### Typed failures 与 recovery

- `verifier_failed=8`；
- `workspace_precondition=1`；
- 带 typed failure 的 9 个 trajectory 全部以 canonical completed terminal +
  Host evidence 恢复；
- 其余 28 个没有 typed failure；
- 没有 `typed_failure_not_recovered`。

这独立复算 M10-D 的结论：current raw 没有一个安全 typed action 已知但未执行的恢复
样本。

### Evidence deficits

frozen labels 有两个 false success：

1. M10-A 的一个 root recovery：canonical failed→mutation→Host pass、external verifier
   与 receipt 都成立；旧 lane observer 给出
   `failure_mutation_pass_order_missing`，属于已纠正的 observer contradiction；
2. M10-B 的一个 read-only treatment：Host/external verifier 通过，但 frozen child-call
   arguments 不匹配，属于 lane contract failure。它只发生在已删除 Working-Set
   treatment，没有 current-control 重复。

另有 3 个 `arm_result_missing`，正是上述 accounting stops。没有 completed positive task
缺 Host receipt。

### Context / tool observations

280 个 tool outcomes：

- workspace read 171；
- workspace mutation 38；
- verifier observation 38；
- git observation 21；
- child handoff 12。

`read_file=166` 是最高频工具，但“频率高”本身不是损失。一个粗糙的全 trajectory
`path` 分组会把 root/child、修改前后和 compaction 后重读混在一起，错误地制造大量
duplicate。analyzer 使用更严格的三重条件：

1. 同一 actor/run；
2. 同一个 workspace observation epoch（applied mutation/revision change 会清空）；
3. 上一次相同调用的 Tool message 仍在下一次 ModelRequest 中。

最终：

- current controls 的 visible exact duplicate `read_file=0`；
- current controls 的任何 visible exact duplicate tool=0；
- 全部数据只有 1 次 visible exact duplicate `run_verifiers`，发生在 treatment；
- 因此没有 revision-bound read-cache/compact-result candidate。

这也是 analyzer 的实际消费者价值：它阻止用 naive repetition count 恢复已被质量否决的
Working-Set 或新增无证据缓存层。

## 保留决定

analyzer 保留，因为它：

- 在一个命令中复算 M10-D 的 9/9 recovery、M10-E 的无 environment tool loss、
  M10-A observer contradiction、M10-B treatment lane failure 与三次 accounting stop；
- 替代手工 jq，同时复用现有 journal hash contract，没有第二 evaluator；
- 输出 canonical bytes，双跑 report hash 一致；
- 将 raw repetition 缩小到 actor/revision/request-visible 的真实观察；
- 明确输出 `insufficient_current_loss_evidence`，而不是为了“有下一项”制造 candidate。

它不决定产品完成、不改 prompt、不执行模型、不读取 Key，不把 summary 当 RunStore。

## 门禁

通过：

- Python isolated bytecode compilation；
- corrected Harness full self-test（含 SIGKILL journal windows 与 tamper rejection）；
- synthetic same-actor duplicate、model-visible、mutation-epoch reset；
- synthetic raw argument non-disclosure；
- 三份 real 0600 raw schema/hash/size/chain/tail verification；
- real report 连续两次 byte-identical；
- manifest JSON 与 `git diff --check`。

Key 未读取，network=false，official API requests=0，没有新 raw。

## 非结论与下一步

M10-F 不证明：

- `read_file` 调用数已经最优；
- 一次 treatment verifier 重复值得做 production 机制；
- M10-A/M10-B 候选应恢复；
- accounting stop 可以忽略或补样；
- 当前小型 Python fixtures 已覆盖 broad real coding quality。

当前没有 admissible product candidate。长期 Goal 的 A–E 均已形成 keep/delete 决定后，
只剩用户预注册顺序中的最后条件项：复核现有 read-only fan-out 是否有新的 current
fixed-Pro、quality-first 证据。该复核不得因“多 Agent 看起来更强”重开默认并行；没有
independent-task wall-time 净收益就保持 explicit-only/现有 fixed read-only profile。
