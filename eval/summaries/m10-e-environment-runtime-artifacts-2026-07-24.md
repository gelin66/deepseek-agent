# M10-E Environment / Runtime Artifacts（2026-07-24）

## 结论

M10-E 的正式决定是：

```text
reject_no_measured_environment_or_runtime_artifact_loss
keep exact verifier + execution fingerprint + revision-bound evidence
do not create ProjectEnvironmentProfile or a global runtime-artifact path
do not read a credential or call the official API
advance to M10-F read-only trajectory analysis
```

当前 fixed-Pro 正式轨迹没有出现环境命令猜测、setup、service 或 UI runtime 验证损失，
TaskContract 也没有可触发 service/browser artifact 收集的 typed acceptance。此时建立
`ProjectEnvironmentProfile` 会重复 `run_verifiers` 已有的确定性生态/命令解析；建立全局
service/browser 收集则必须从任务文本、manifest 或泛 `operation_failed` 猜测，既没有
真实 caller，也会产生新的 environment truth 和平台面。

所以 M10-E 没有 production candidate、Key、API、raw 或 live A/B。完整机器可读矩阵见
[m10-e-environment-runtime-artifact-audit-v1.json](../manifests/m10-e-environment-runtime-artifact-audit-v1.json)。

## 切片契约

- 真实问题：current fixed-Pro production 是否存在一个已测量的环境命令、初始化、
  service 或 UI-runtime false-success 损失，可由 deterministic profile 独立修复；
- 验收：profile 必须由确定性事实派生，runtime artifact 只能由 typed TaskContract
  需求触发并绑定 latest revision；环境失败/猜命令下降，verified success 非劣、
  false success 0、root/Writer 一致；
- owner（若准入）：高层 `crates/app`，执行 `crates/tools`，复用
  RunEnvironment/ToolArtifact/EvidenceReceipt；
- 禁止：第二 environment store、平台、全局 browser/MCP、任务文本 classifier 或复制
  `run_verifiers`；
- no-delta cutover：不创建 production branch，不制造稍后需要删除的 adapter。

## Current owner graph

当前已经具备的事实不是空白：

1. `RunEnvironment` 保存 canonical workspace、DeepSeek provider、tool catalog、
   execution fingerprint、write mode 与 sandbox controls；
2. `ProductionExecutionFingerprint` 绑定 build revision、fixed route、official endpoint、
   retry contract、tool identity 与完整 tool catalog；
3. `ProductionToolExecutionIdentity` 绑定 workspace、trust/authority、shell/sandbox policy、
   external paths 与 non-secret policy digests；
4. app 在 `RunCreated` 前把 TaskContract verifier 解析成 production exact plan，resume
   重新解析并要求字节语义一致；catalog/fingerprint mismatch 在任何 HTTP 前失败；
5. `run_verifiers` 的 advisory auto profile 已确定性识别 Rust/Node/Python/Go，但 Host
   acceptance 明确拒绝可随 workspace 漂移的 auto plan，只接受 frozen exact commands；
6. verifier 在执行前后捕获 workspace revision，只有 revision 不变时才产生
   hash-checked inline ToolArtifact，Runtime 再签发 matching EvidenceReceipt；
7. Writer 使用同一 AgentRuntime、fresh worktree tool binding，并在 root 集成后重新验证。

TUI 的 external dependency resolver 有真实 Doctor/保留 UI 工具消费者，但不参与
AgentRuntime 的 project command plan。把它迁移成 production profile 不会替代旧路，
只会新增第二事实；因此本切片不删除或复用它。

## Current trajectory loss audit

审计三个 current frozen 0600 raw：

| 数据 | canonical snapshots | 结论 |
| --- | ---: | --- |
| M9-C fixed-Pro | 9 | fixed-Pro regression，2 个 verifier recovery |
| M10-A scoped context | 22 | 已知 frozen observer mismatch 不是环境失败 |
| M10-B Working-Set | 6 | 质量 veto 是 child 参数契约，不是环境失败 |

聚合事实：

- 37 个 canonical Store snapshots、34 个完整 arm results；
- 280 个 canonical `ToolOutcome` commits；
- 0 个 `exec_shell`，0 个 `run_tests`；
- 0 个 environment/setup/service/UI failure；
- 仅有 8 个 `verifier_failed` 与 1 个 `workspace_precondition`，均已在 M10-D 归因；
- 37/37 acceptance 都是单步 `run_verifiers` exact plan；
- 37/37 frozen program 都是 `/usr/bin/python3`；
- 0 个 service/UI runtime acceptance。

因此无法预注册一个“profile 相对 current 减少猜命令或 setup failure”的 treatment
delta。对没有出现的损失做 live A/B 只会消耗付费样本，不能证明产品收益。

raw identities：

- M9-C：
  `c73a01a4934b82b3f4a4042791aae89e5da56feaea1ffbea188a80d4e6e05b77`；
- M10-A：
  `0c81935b2e6c4afb2e80c76d0a3746689e068a90c17ee16644176b8d710a0d66`；
- M10-B：
  `69b44fa8ebcc8555fe102369bbf4fc25c66149aa713a8d4fa2157578f87da1d0`。

这些 raw 只用于 loss audit，不拼成新的 control/treatment 产品比较。

## 为什么不实现 profile 或 runtime artifacts

### ProjectEnvironmentProfile

未来若出现真实损失，profile 可能需要 language/build system/tool versions 和
build/test/lint commands；但 current Host verifier 已把完成所需命令解析成 exact
VerifierSpec，并持久化真实 plan。现在再持久化一份“推荐命令”：

- 不能替代 exact acceptance；
- 会与 workspace 修改后的 manifest/scripts 漂移；
- 会复制 `run_verifiers` 的 ecosystem/package-manager 解析；
- 没有 current model request 或 Host policy consumer；
- 没有 measured setup/guessing loss 可解释复杂度。

### Service / UI runtime artifacts

当前 `TaskAcceptance` 只有 Host 与 Verifier，没有 typed service/UI profile。没有这一
触发事实时，Host 无法安全判断是否应该启动服务、打开 browser、收集 network/console/
screenshot 或允许副作用。用关键词或 manifest 猜测会把后端任务变成全局平台任务，并
新增 tool/config/protocol surface。

未来只有一个具体 TaskContract stratum 证明静态 verifier 会 false-success，且能冻结
service start、health、interaction 与 revision-bound artifact contract，才允许作为新的
窄 vertical slice 重开；不能从本切片的“缺少类型”推导“现在应先造类型”。

## Deterministic conformance

在 current `a0cc817a`、Run API v12、RuntimeEvent v18、State v24、exec-stream v3 上通过：

- auto verifier plan 覆盖 Rust/Node/Python/Go 的现有确定性 resolver；
- exact resolver 与执行注入同一环境；
- Python verifier 成功/失败都不遗留 bytecode cache；
- production start 覆盖 caller 自写 verifier plan；
- exact Host verifier 产生 latest-revision receipt；
- catalog/fingerprint mismatch 在 HTTP 前失败；
- Writer 使用 fresh worktree tools，集成后由 root 重新验证。

这些是 existing mechanism conformance，不是候选收益证明。

## Cutover

M10-E 没有写 production 代码，因此：

- 不新增 ProjectEnvironmentProfile、environment event/schema/store/config/prompt；
- 不新增 service manager、browser mode、console/network/screenshot tool 或 artifact
  protocol；
- 不复制或移动 `run_verifiers` 的 deterministic ecosystem owner；
- 不删除 TUI Doctor、PDF/external dependency diagnostics；
- 不改变 exact verifier、revision evidence、Writer、fixed route 或 billing stop；
- 只保留 manifest、summary 与权威文档中的否决事实。

Key 未读取，official API requests=0，raw 未创建。

## 非结论

M10-E 不证明：

- UI/service 任务永远不需要 runtime artifacts；
- current execution fingerprint 已冻结未来所有 tool binary version；
- 静态测试可以代替明确要求真实运行的 TaskContract；
- generic `operation_failed` 已经可以驱动环境恢复；
- 应恢复 Auto、FIM、Anthropic Messages、第二 Provider/Runtime/Store 或 multi-Writer。

下一独立切片为 M10-F：从 current RunStore/raw 只读聚合 task stratum、failure code、
context source、tool repetition、evidence deficit 与 recovery outcome，先找出一个真实、
可复现且仍有 production headroom 的下一候选；analyzer 不进入 production，不自改提示词，
也不拥有完成权。
