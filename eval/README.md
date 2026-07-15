# M1 评测入口

本目录只保存能够复核产品能力的任务清单和小型汇总。原始日志与逐次结果分别写入
`eval/raw/` 和 `eval/results/`，默认不提交。

## 当前离线基线

`manifests/m1-offline.tsv` 将证据明确分为：

- `full-runtime-offline`：用注入模型或本地 WireMock 驱动真实 Engine、真实工具注册表，
  或真实 `codewhale-tui exec`；
- `protocol-fixture`：验证 DeepSeek 路由、Strict、FIM、SSE、reasoning replay 和 usage；
- `runtime-contract`：验证终态、错误恢复、上下文、多 Agent 预算与 worktree 等确定性契约；
- `critic-plumbing`：只验证 `verify` 模型评审工具的接线和结果归一化；
- `config-contract`：验证本地 DeepSeek 配置约定。

离线用例证明确定性协议和运行时行为，不证明真实 DeepSeek 的编码智能。尤其是
`critic-plumbing` 通过，不代表 critic 能发现真实缺陷，也不能作为任务完成证据。

现有 `codewhale eval` / `crates/tui/src/eval.rs` 会直接调用一套重复实现的简化文件与
Shell 函数，绕过生产 Agent loop 和生产工具注册表，因此它只算 smoke，不纳入 M1
生产能力结论。

## 运行

当前提交的全部离线用例：

```bash
./scripts/eval-m1.sh
```

只运行可与导入基线 `352e86a6` 比较的用例：

```bash
./scripts/eval-m1.sh --scope cross-revision
```

使用当前评测器测试另一个干净 worktree：

```bash
./scripts/eval-m1.sh \
  --repo /absolute/path/to/worktree \
  --scope cross-revision \
  --output eval/results/m1-offline-imported.jsonl
```

脚本逐项使用 Cargo 的精确测试名，并检查确实运行且通过了一个测试，避免“过滤器匹配
零项但 Cargo 返回成功”的假绿。目标仓库必须是干净提交；清单、结果 Schema 或证据
等级不合法时也会直接失败。结果同时记录被测提交、评测器提交和 manifest blob，避免
以后用不同清单生成同名“基线”。

## 结果边界

- `cross_revision`：测试在导入提交和当前提交都存在，可以做同口径比较；
- `candidate_only`：只说明当前候选实现没有退化，不能宣称相对导入基线有提升；
- 真实 API 任务必须另建显式启用、费用/Token/超时受限的 live suite；
- live suite 必须用预定义验收器判定 `verified_success`，不能用模型自评；
- 任何能力只有在成功率、假成功、Token、耗时和复杂度的净收益成立后才能进入稳定核心。
