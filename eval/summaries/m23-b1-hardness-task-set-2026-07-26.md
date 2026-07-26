# M23-B1 Hardness task-set conformance

- 日期：2026-07-26
- 起点：`59fc8a65f596f3ff6f008bea6cb5d875f80a9e6e`
- 决策：`keep_offline_hardness_task_set_control_not_acquired`
- production delta：0
- Key / official API / external network / model request：0 / 0 / 0 / 0

## 真实问题

M23-A 已把 behavior 与 accounting truth 解耦，但现有 current loss acquisition 主要由
小到中等 fixture 构成，不能公平测量 large-repo localization、跨文件行为、长时恢复、
service/API/UI 或 explicit Writer。直接开发四个 production candidate 会把“代码存在”
误当成能力收益。

本切片只解决可复现 task-set/control contract。它不读取凭据、不运行模型，也不把
reference solution 的通过率包装成 DSE 成功率。

## Contract 与唯一 owner

唯一 owner 仍是 `scripts/eval-m9b-fixed-pro-regression.py` 的 corrected Harness。新增
`m23b` campaign，复用同一个 manifest loader、fresh Git materializer、external verifier、
hash-chained journal crash self-test 和 canonical report writer；没有第二 evaluator、
Runtime、Store、工具目录或 production caller。

冻结矩阵：

| 维度 | 覆盖 |
|---|---:|
| task / round / future arm | 20 / 3 / 60 |
| positive / safety | 17 / 3 |
| root / read-only / Writer / safety lane | 13 / 2 / 2 / 3 |
| Rust / TypeScript / Python / Go | 5 / 7 / 7 / 1 |
| large-repo localization | 4 |
| cross-file behavior | 7 |
| failure/recovery/safety | 5 |
| long-horizon resume | 3 |
| service/API/UI | 3 |
| explicit Writer | 2 |

每个 task 冻结 5–20 个相关文件、project scope、task text、human-estimated minutes、
allowed/reference changed files、external verifier、actor profile、tool policy、
continuity/runtime assertion 与 reference patch identity。每个 future arm 使用 fresh
independent Git repository、独立 RunStore/verifier HOME、fixed Pro/high、
`maximum_reruns=0`；不补 mate、不选择性重跑。

## Fixture 与 reference 自证

共享 136-file monorepo fixture 的 canonical identity：

```text
tree sha256
  0a1e61e35e722a071f606a9a02d6c2009c52ed7fad15029a09685495d13824b4
deterministic base commit
  eea71679b89373fe869280dea8c6e1b27a770afe
commit profile
  m23b-2026-07-26
```

参考补丁只存在于 model workspace 外。17 个正向任务均满足初始 verifier 失败、只应用
该 project 的 scoped reference patch 后通过；changed scope 与 manifest 完全一致。3 个
安全反例没有 reference patch，并继续失败。

两个新 runtime fixture 补齐旧 task set 的实际运行缺口：

- Go health service 通过真实 loopback HTTP 验证 GET/HEAD、405/Allow、JSON content type、
  region/version/deep payload；
- TypeScript DOM dashboard 通过真实 loopback server、固定本机 Chrome 与 Playwright
  验证可访问性状态、空列表行为与交互。

它们证明 verifier 可执行且 reference contract 正确，不证明模型已经完成任务。

## 离线结果

freeze report 与 self-test 各运行两次并 byte-identical：

```text
positive reference tasks passed       17 / 17
safety tasks still failed              3 / 3
materialized base repositories        20 / 20
journal crash windows                  4 / 4
maximum_reruns                              0
control_baseline_acquired               false
credential required for conformance     false
Key / API / network / model request     0 / 0 / 0 / 0
```

冻结 identity：

```text
harness sha256
  91f6b485bb0467961df8a41cb96d8caf107a9fe8a6ac7c64d2d230ef6ac29fb7
manifest sha256
  af8bc143a74cec18564cb450d833ea5de278cb4b74c355e6632f4e840f642d04
schedule sha256
  e8f67f51e2d692e0423b545c16f9f8624a87ac7fb3bdcea53ac40b58397ea3a6
task contracts sha256
  363426bdc5a952222f839fd2242c4c040120e0556c51f0e28fb0f49cbf86e12b
reference changed scopes sha256
  7e6b139d8534ec16d4a0b0e2c4f2f574a662d32ee512b4dbaef5ecf54e0fea09
freeze report sha256
  b66241e435acad801f11d1ef84926b1c8c98238ab84dc567f734fdf9111e27b3
self-test report sha256
  313d2721befbad88da62e72d3231da721f0d7789a417a369be1ae1c8d5283db4
```

M15/M20B self-test、M14 observer conformance、M16 acceptance-equivalence
conformance 与 M23-A behavior/accounting truth conformance 继续通过。

## 替代、删除与复杂度

本切片替代“继续用历史小任务推断 Hardness 能力”的不充分评测前提。它没有替换
production 路径，因此不删除任何 Runtime、Store、tool 或 frozen evidence；新的 task
set 直接接入唯一 corrected Harness，不建立兼容 reader、dual writer 或平行 runner。
reference patch 不进入 model-visible workspace。

未来 control acquisition 一旦作出 keep/reject 决策，只保留可复核 manifest、fixture、
summary 与必要 ignored raw；任何无消费者 live admission/analysis 分支必须物理删除。

## 门禁

本切片通过：

```text
python3 -m py_compile scripts/eval-m9b-fixed-pro-regression.py
M23B --freeze-report x2 + byte comparison
M23B --self-test x2 + byte comparison
M20B --self-test
M15 --self-test
--truth-conformance
--observer-conformance
--acceptance-conformance
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

Cargo 使用 `CARGO_INCREMENTAL=0`、`CARGO_NET_OFFLINE=true` 和独立
`/private/tmp/dse-m23b-target`；提交前精确删除 target、临时报告和新生成 pycache。

第一次全仓 test 在并行负载下出现一次既有
`established_sse_without_events_hits_typed_stream_stall` timing failure；该用例在本切片
focused 门中已通过，随后单独精确复现通过，完整 workspace test 从头重跑也全绿。没有为
该无代码交集的瞬时窗口修改 production 或测试断言。

## 非结论与停止门

本切片没有 fixed-Pro trajectory，因此没有 pass@1、pass^3、false success、首次相关文件、
repair loop、Token、费用或 stable owner/cause loss matrix。reference solution 的
17/17 不是产品成功率。

M23-B 的 control acquisition 退出门尚未满足。M23-C high/max 和 M23-D 的
ApplicationProbe、deterministic symbol/reference localization、Host-derived
VerifiedMilestone、Tool ACI 继续禁止。下一步只能是独立、明确授权且 accounting 边界
完整的 fixed-Pro/high control acquisition；在它闭合前停止生产功能开发。
