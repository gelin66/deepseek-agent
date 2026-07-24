# M8-K V1 产品范围 successor

- 日期：2026-07-24
- 分支：`deepseek-agent`
- clean baseline：`49a465811e96036bc985a23fcd4a69a6391861fe`
- baseline tree：`245a2ef233922026351a6484954fd0bcee05e231`
- frozen manifest commit：`b44d7ff919291091ed022f3f806bc8d5839dfa0e`
- frozen manifest tree：`b44affe3c5a675b0d1f01eec8ed7519619e342d4`
- accepted scope candidate：`6a99cb79ee1a63c57215456b5f0f040659b9426d`
- candidate tree：`60e80a06c8d0a0f4b097a6af6d22a5df39205d25`
- protocol：Run API v11 / RuntimeEvent v17 / State v23 / exec-stream v3
- credential read / official API requests：false / 0

## 1. 结论

M8-K 没有选择 production 实现。V08 multi-Writer、V09 FIM 与 V10 RepoGraph 都没有
latest owning decision 之后的可归因 production failure、canonical treatment 或完整
accounting；为满足旧清单恢复已拒绝/已删除路径会增加无消费者复杂度，而不是能力。

ADR-0005 接受如下 V1 capability scope：

- V08 由现有一个 explicit isolated Writer 的完整 Host lifecycle 验收；并发
  multi-Writer 不是 V1 requirement；
- V09 由 official Standard Chat 与 Strict whole-catalog admission/lossless Standard
  fallback 验收；FIM 不是 V1 requirement；
- V10 由 canonical search/read/diff、bounded ContextBroker 与 deterministic verifier
  的跨文件结果验收；RepoGraph 实现名不是 V1 requirement。

产品决策是 `keep current verified capabilities / reject implementation literals as V1
requirements / hold evidence-gated re-entry`。V08/V09/V10 关闭后 current matrix 从
10 pass / 6 blocked 变为 13 pass / 3 blocked；V13、V15、V16 仍 blocked，V1 仍不可发布。

## 2. 冻结选择合同

manifest
[`m8-k-v1-scope-successor-v1.json`](../manifests/m8-k-v1-scope-successor-v1.json)
在 authority 变化前冻结：

- baseline revision/tree、Cargo.lock、Rust 1.97.0 与 authority/source hashes；
- current 10 pass / 6 blocked matrix；
- candidate 只有同时具备 current attributable failure、canonical caller、
  deterministic verifier、同 revision immutable treatment、完整 request/usage/retry/
  time/cost accounting、单一 owner 与 cutover deletion 才能实施；
- 至多选择一个 blocker；没有合格 blocker 时只能作可审计 scope decision；
- `maximum_reruns=0`、credential read=false、official API requests allowed=false。

因此本切片不存在 paid arm，也没有用离线诊断结果拼接模型指标。

## 3. V08/V09/V10 证据

| Item | Existing evidence | Current caller/failure | Decision |
|---|---|---|---|
| V08 | M6-B1 v2：2/18 -> 4/18 verified，但 7 个 Writer false success，Token +35.5%、费用 +52.5%、时间 +39.8%；v3 unknown billing | 一个 explicit Writer lifecycle 已完整；second Writer typed-rejected；multi-Writer failure/treatment=0 | keep single / reject V1 literal / hold re-entry |
| V09 | M7-C Host edit matrix 3/12 -> 12/12；M8-H 删除不可达 FIM 半分支 | current Chat sender/parser/replay/accounting 完整；FIM caller/parser/apply/reopen=0；新 edit-generation failure=0 | keep Chat / reject V1 literal / hold re-entry |
| V10 | M5-B ContextBroker on/off 均 6/6 verified，失败未定位为结构检索 | canonical file search/grep/list/read/diff/status + ContextBroker + verifier；RepoGraph caller/failure=0 | keep bounded cross-file outcome / reject V1 literal / hold re-entry |

这些事实不证明三种候选能力质量较差或永远无价值；只证明它们目前不具备 V1 实施准入
证据。未来重开必须从新的 attributable failure 与 frozen task position 1 开始。

## 4. Authority cutover 与删除边界

single authority owner 是 PRODUCT_PLAN + accepted ADR：

1. ADR-0005 明确 capability outcome 与 implementation choice 的边界；
2. PRODUCT_PLAN 仍保持 16 项 V1 completion definition，只把 V08/V09/V10 从实现名收敛
   为 current verified outcome；
3. ROADMAP、EVALUATION、SUBAGENTS、CURRENT_CODEWHALE 和 repository guidance 使用同一
   13 pass / 3 blocked successor truth；
4. 没有 production caller 迁移，因为没有 implementation candidate 获准；
5. 没有恢复 M8-H 已物理删除的 FIM half-branch，没有增加第二 Writer scheduler、
   RepoGraph owner、Provider、模式、Runtime、RunStore 或工具目录；
6. stale V1 literals 已从 current authority 中删除；frozen historical evidence 保持
   immutable，不改写历史。

相对 baseline 到 accepted candidate 为 7 files、`+592/-43`，其中新增 frozen manifest、
accepted ADR 与离线 scope contract；production/crates delta 为 0。

## 5. 官方 DeepSeek 复核

复核日期：2026-07-24。只采用官方一手资料：

- [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)：
  ordinary production surface 是 `POST /chat/completions`；
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls)：Strict 是 Beta Chat
  capability，且 request 中全部 functions 都必须 strict-compatible；
- [FIM Completion](https://api-docs.deepseek.com/guides/fim_completion/) 与
  [Create FIM Completion](https://api-docs.deepseek.com/api/create-completion)：
  FIM 是独立 Beta `POST /completions`，返回 `choices[].text`，当前 reference 列出
  `deepseek-v4-pro` 与 4K output limit；
- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)：official base URL
  不变，current models 是 `deepseek-v4-pro` / `deepseek-v4-flash`；2026-07-24 退役的是
  旧 `deepseek-chat` / `deepseek-reasoner` aliases，不是 ChatCompletions API。

因此 CodeWhale 继续使用已验证的 official DeepSeek OpenAI ChatCompletions；没有
Anthropic Messages、第二 transport 或旧 model alias fallback。

## 6. 离线门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/codewhale-m8k-target
```

通过：

- M8-K scope contract：8/8；
- frozen M8-E V1 exit contract：8/8；
- M7-C canonical edit Harness self-test；
- targeted Runtime second-Writer rejection；
- `codewhale-deepseek` 51 tests、`codewhale-context` 17 tests 与 tools catalog tests；
- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- `git diff --check`。

首次 cold focused run 只有
`established_sse_without_events_hits_typed_stream_stall` 失败：cold compile load 下先锁存
network error，随后 stall 被投影为 generic `deepseek_model_error`，而 fixture 预期
`stream_stall`。同一 exact targeted diagnostic 随即通过，完整 warmed focused 与 workspace
test 也通过同一 case；该问题未复现且与本次 doc-only scope candidate 无关，所以没有制造
无证据 source fix。这个观察保留为 future fixture timing/recovery audit 输入，不计作
formal evaluation rerun。

## 7. Current V1 matrix

| Item | Current |
|---|---|
| V01–V12 | pass |
| V13 imported-baseline coding comparison | blocked |
| V14 | pass |
| V15 billing-provable Simplified Chinese prompt comparison | blocked |
| V16 imported-baseline workflow-step comparison | blocked |

总计 13 pass / 3 blocked。

## 8. 非结论与下一切片

M8-K 不证明：

- multi-Writer、FIM 或 RepoGraph 已实现、应默认启用或没有未来价值；
- verified coding success、false success、Token、cache、wall time 或费用改善；
- imported baseline 缺失的 accounting 可以事后估算；
- current Auto 已获得 fixed Pro 非劣或约 20% 的成本/时间收益；
- V1 已可发布。

下一切片应先合并审计 V13/V16 的 imported-baseline 可比性。若旧 revision 无法原生提供
同口径完整 accounting 与 immutable replay identity，不得修改旧 revision 或外部补猜
账单；应冻结可接受的 release benchmark successor，再只实施能形成 current-production
真实 coding 与 workflow-step 证据的最小 benchmark。
