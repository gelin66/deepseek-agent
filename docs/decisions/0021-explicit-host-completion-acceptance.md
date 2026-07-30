# ADR-0021：显式 Host completion acceptance

- 状态：已接受
- 日期：2026-07-30
- 细化：ADR-0002、ADR-0011、ADR-0012、ADR-0014

Primary owner 是 `crates/runtime` completion gate；`crates/protocol` 与 `crates/state` 只承担
必要的 durable expression，`crates/app`、`crates/localization` 与 clients 只发送 canonical command
或投影同一事实。

## 真实问题

默认 `TaskDefinition::host` 没有 exact verifier。当前 Runtime 在模型以非空
`finish_reason=stop` 提交 `CompletionProposed` 后，会在同一控制流里无条件构造
`AcceptanceSatisfaction::Host`，继而写入 `Completed`。因此模型自己的完成声明会被持久化为
Host 已接受；RunStore 只能检查 Host criterion 与 Host satisfaction 的类型配对，不能证明存在
独立的 Host 决定。这与“模型只提出候选、Host 拥有完成裁决”和 latest-revision evidence 合同
冲突。

## 决策

### 1. 三种事实

- **Answered / Proposal**：非空 Stop 只提交一个绑定 task generation 与当时
  `WorkspaceState` 的 `CompletionCandidate`。该答案可立即展示，但 Run 仍没有成功终态；
  Run API canonical projection 为 `RunCompletion::Answered`，exec 投影为
  `status=answered` / `termination_reason=answered_unverified`。Runtime 返回的
  `AwaitingHostAcceptance` 只是可重开 quiescent boundary，RunStore 此时没有 `Terminal`。
  Store 还必须把 proposal 的 ID、message 与当前已提交的无 tool-call Model Stop 逐字绑定；
  orphan、替换 candidate 或客户端伪造 proposal 都 fail closed。
- **Host accepted**：Host 通过 canonical Run command 显式提交 exact candidate、task
  generation 与 workspace state。Runtime 在重新观察当前 workspace 后生成一个 durable
  Host acceptance receipt；重复同一 request id 幂等，任何 payload 复用、foreign/stale
  candidate、错误 generation 或 revision 都 fail closed。Host receipt 不是 verifier receipt，
  不投影为 deterministic verified。若 Host 无法取得已知 workspace revision，答案仍可作为
  `answered_unverified` 展示，但 acceptance fail closed；不能用相同的 `Unknown` 原因字符串
  冒充“工作区未漂移”。
- **Verified completed**：冻结的 exact verifier 为最新 workspace revision 生成有效
  `EvidenceReceipt`。只有该 receipt 能形成 verified-completed projection。

`CompletionDecision` 的每个 Host satisfaction 必须引用一个 replay 可验证的 Host acceptance
receipt；所属 Run 由同一 event stream 中的 `HostCompletionAccepted` 固定，receipt 继续精确绑定
candidate、generation 与 workspace。只写 satisfaction 或 terminal 不能伪造 acceptance。包含
verifier criterion 的完成决定仍必须引用有效 EvidenceReceipt。模型 Stop、Prompt 文案、客户端
显示与模型自评都不能生成这两类 receipt。

### 2. 单一状态机与恢复

`CompletionProposed` 是一个可重开的 quiescent boundary，不是 `Blocked`、`Failed` 或成功
terminal。Runtime 在没有匹配 Host receipt 时释放执行 lease，不继续发模型请求；普通问答因此可
立即显示 `answered_unverified`。显式 acceptance 由同一个 `AgentRuntime` 重新取得该 Run，提交
Host receipt，再在同一 completion gate 形成 Host-accepted terminal；verifier task 则形成
verified-completed terminal。

proposal 后 workspace 漂移会使旧 candidate/receipt 失效。proposal、Host receipt、verifier
receipt 与 terminal 都进入同一 append-only RuntimeEvent/RunStore；crash/reopen 只恢复已提交
前缀，不重发模型、工具、已开始的未知 verifier 或 Host action。root、read-only child 与 Writer
使用同一 receipt/decision 验证；内部 child 的 Host 接受也必须通过同一 durable event，不得恢复
旧的无条件 satisfaction。父 Runtime 收集 read-only child 时还会重验 candidate、decision 与
receipt；Host receipt ID 必须来自该 parent run/task/candidate 的确定性 acceptance command，不能
由一个自洽但伪造的 child outcome 代替。

### 3. 客户端边界

Run API 提供一个 exact `accept_completion` command 和一个 canonical completion projection。
TUI、exec 与 app-server 只发送命令或显示 projection：

- TUI 通过明确的 `/accept` 接受当前 proposal；
- plain/json/stream exec 在 proposal 处返回 `answered_unverified`，不自动接受；
- app-server 原样承载同一个 versioned Run command/view/event。

客户端不得把 Answered 自动升级为 Completed，也不得根据文案、退出码或本地缓存构造 Host/
verifier receipt。

## Cutover 与拒绝

本决定直接替换并删除 Runtime 内无条件 `AcceptanceSatisfaction::Host` 路径和冻结该行为的测试。
Run API v16、RuntimeEvent v23、State schema v29 与 exec-stream v7 进行一次直接 cutover；不保留
旧 completion compatibility reader 或第二状态机。v29 退役缺少不可推导 Host receipt、且候选未
绑定 workspace state 的旧 materialized Run，只保留仍可安全重建的 pending Start intent。

不新增第二 Runtime、Store、Provider、Goal/Todo/Memory、completion service 或客户端旁路；不改
DeepSeek wire、model-visible Prompt、工具 catalog、浏览器、MCP、视觉与发布链。
