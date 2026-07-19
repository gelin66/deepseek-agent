# M5-A canonical 完成门禁精确 A/B

> 日期：2026-07-19
>
> 状态：保留并关闭 M5-A；不外推为通用编码或 Token 优化收益
>
> 范围：`503d6294..ba25f836`、官方 DeepSeek、2 个场景、3/cell

## 1. 结论

`ba25f836` 把任务成功从“模型停止并给出非空文本”改为 Host 对冻结
`TaskContract`、最新 workspace generation/revision 和精确 verifier 证据的唯一裁决。
12-run 正式 A/B 得到：

- `coding_fix` 中 baseline/candidate 均为 `3/3` verified success，候选没有降低固定编码
  任务成功率；
- `forced_false_claim` 中 baseline 为 `3/3 completed + false-success`，candidate 为
  `3/3 blocked + correct-rejection`，false-success 从 `3/3` 降为 `0/3`；
- candidate 三次编码成功都生成了与当前 generation/revision、精确 verifier plan 和
  Available artifact 匹配的 `EvidenceReceipt`；
- candidate 三次反例都提交
  `CompletionProposed -> HostVerificationPrepared -> HostVerificationStarted ->
  HostVerificationCommitted -> CompletionRejected -> Terminal(Blocked)`，没有 receipt；
- 12/12 usage、费用、请求账本和终态计量完整，四个 cell 的 measurement、contract 与
  completion-gate 校验均有效，整个 A/B 为 `product_metric_eligible=true`；
- 每对 baseline/candidate 的首个真实模型请求投影 SHA 完全相同，任务、系统提示、
  模型可见工具、模型和预算没有被评测器暗中改变。

因此 M5-A 的保留理由是直接消除了已复现的完成权限漏洞，同时没有破坏固定编码任务。
编码 cell 中请求持平、Token/时间/费用小幅上升；样本很少，而且 treatment 不应直接改变
模型推理效率，不能把这些差异解释为普遍的 DeepSeek 性能变化。

## 2. 身份、预算与复杂度

| 项目 | 值 |
|---|---|
| evaluation id | `2eabda53cb8943b1875a20d2cf7a70f8` |
| baseline | `503d62948c2643a4a3b6a0828e93b57d866e97c0`；Run API v4 / RuntimeEvent v6 |
| candidate | `ba25f8368590818ca17198a9d59a8306ebb7a52a`；Run API v5 / RuntimeEvent v7 |
| baseline binary SHA-256 | `8a7504763dbd679c7e70a179849d5c1dd5f16fd0e0ed53989f52bbb62b966fd7` |
| candidate binary SHA-256 | `1ece00be6b4b4facd209d8d7043402c6126a601ddb84fe187bd38a9c10babeba` |
| model | `deepseek-v4-flash`；reasoning effort `high` |
| schedule | 12/12；2 场景 × 2 版本 × 3 次；成对顺序交替 |
| 每 run 预算 | 10 API 请求；32 turns；360 秒；8,192 output tokens |
| fixture SHA-256 | `5858e710f51400f580a048587bfbda7f0c63cb2176f335c2a1595330659fd270` |
| verifier SHA-256 | `c28818cc9134ec3f9ddb2468fe24460440aa8524079abd46f455cc0332a72642` |
| verifier spec SHA-256 | `cb09458748af0c9e228513331934b21aafa3c29cf8c610300470b9f52d10e2ee` |
| Harness SHA-256 | `8071c5fca023dbde4d91009f24b40adfae3ed830fa5db4b04e94ec841c706fbb` |
| raw result | `m5-completion-gate-ba25f836-vs-503d6294.json`（本地忽略，权限 `0600`） |
| raw SHA-256 | `42ac722d6f25abe6b066eee850ef5190b2e0367dfd91bed00472bad652e7b05c` |
| 总计量 | 40 API 请求；144,903 Token；USD `0.004110153` |
| treatment diff | 39 files；`+3,803/-461` |

按路径粗分，treatment 包含 production crates `+2,050/-288`、tests/fixtures
`+1,591/-122`、docs `+153/-44`、其他 `+9/-7`。这是明显的复杂度成本，不应把新增类型或
事件数量本身视为进步。保留依据是它替代了不可靠的直接完成路径，并由 Store reducer、
崩溃恢复和真实 false-success 反例共同约束；下一阶段不能再为同一问题增加第二套抽象。

Key 只从权限 `0600` 的本地文件读入 app-server 子进程环境，未进入 argv、请求 envelope、
结果或本摘要。

## 3. 分层结果

下表每个计量单元依次为“总量 / 均值 / 中位数”：

| 场景 | 版本 | 结果 | 请求 | Token | 时间（秒） | 费用（USD） |
|---|---|---:|---:|---:|---:|---:|
| coding_fix | baseline | 3/3 verified | 17 / 5.667 / 6 | 67,838 / 22,612.667 / 22,894 | 49.874 / 16.625 / 15.482 | 0.001939246 / 0.000646415 / 0.000658633 |
| coding_fix | candidate | 3/3 verified | 17 / 5.667 / 6 | 68,569 / 22,856.333 / 24,068 | 54.474 / 18.158 / 18.259 | 0.001999883 / 0.000666628 / 0.000695856 |
| forced_false_claim | baseline | 3/3 false-success | 3 / 1.000 / 1 | 4,264 / 1,421.333 / 1,410 | 5.653 / 1.884 / 1.891 | 0.000089992 / 0.000029997 / 0.000026824 |
| forced_false_claim | candidate | 3/3 correct-rejection | 3 / 1.000 / 1 | 4,232 / 1,410.667 / 1,410 | 6.351 / 2.117 / 2.122 | 0.000081032 / 0.000027011 / 0.000026824 |

编码 cell 的观测变化：

| verified success | 请求 | Token | 时间 | 费用 |
|---:|---:|---:|---:|---:|
| 0 pp | 0.00% | +1.08% | +9.22% | +3.13% |

反例 cell 中 candidate 为执行本地冻结 verifier 增加了约 `0.233` 秒/run；模型请求不变，
Token `-0.75%`、费用 `-9.95%`。编码 cell 的时间增加约 `1.533` 秒/run。时间是从夹具
Git 初始化开始，到 app-server、模型执行、Host verifier 和独立外部 verifier 全部结束的
端到端时间；两侧采用同一测量边界。Token/费用的小幅变化属于模型与缓存方差，不能解释为
完成门禁直接带来的效率收益。

## 4. 证据边界

Harness 不用普通 `codewhale exec` 冒充显式 acceptance，而是直接使用
`codewhale app-server --stdio`：

- baseline 通过 Run API v4 接收 candidate `TaskDefinition.model_message()` 的精确文本；
- candidate 通过 Run API v5 接收结构化 task 和同一 model-visible 文本；
- Host verifier 固定为 `run_verifiers(profile=exact)`，program、argv、cwd、env 和
  600 秒 timeout 全部进入 contract；
- verifier 的 outer/public/hidden Python 进程都使用 `-B`，Harness 自测还要求失败与成功
  verifier 前后的 Git tracked/untracked 状态精确相同，避免 `__pycache__` 改变 revision；
- 600 秒是生产 `run_verifiers` 冻结 plan 的单 gate 上限；Harness 对整次 run 使用
  390 秒 watchdog，触发时直接终止评测且不生成 eligible 结果，而不是把超时混为任务失败；
- 每个 run 使用独立 Git workspace、独立 `CODEWHALE_HOME` 和 SQLite State；
- 逐 run 保存终态、事件前缀哈希、精确 lifecycle identity、workspace transition、
  receipt/rejection、artifact SHA/状态、外部 verifier、请求、Token、时间和费用；
  不保存模型正文、reasoning 或工具内容；
- baseline/candidate 的 `--version` commit 前缀都必须能在当前仓库解析为不同的完整 commit，
  仅二进制 SHA 不同不足以让结果 eligible。

本结果只覆盖一个小型 Python 修复任务和一个刻意构造的完成反例。它不证明：

- RepoGraph、ContextBroker、compaction 或多 Agent 已得到收益；
- 所有真实项目的 false-success 已归零；
- 编码 cell 的 Token、时间或费用变化可以泛化；
- 当前较大的 M5-A treatment diff 可以继续无上限扩张。

## 5. 决策

1. 保留 `ba25f836` 的唯一 TaskContract、workspace generation、EvidenceReceipt 和 Host
   completion owner，M5-A 标记完成。
2. 不恢复旧 TUI Goal/Hunt/receipt prototype，不增加完成工具、第二 receipt store 或兼容层。
3. 普通 objective-only `exec` 仍是 Host acceptance；需要可证明成功的产品调用方应通过
   Run API v5 显式冻结 verifier contract。
4. 下一切片进入 M5-B：先做唯一 ContextBroker 的 evidence-aware compaction，确定性保留
   TaskContract、未决问题、当前 workspace/diff 和最新有效证据，再做 compaction
   on/off A/B。
5. RepoGraph、LSP 与 embedding 检索继续延后；只有 M5-B 真实任务暴露确定的检索瓶颈并能
   预定义收益指标时才引入，避免把框架做成概念堆栈。
