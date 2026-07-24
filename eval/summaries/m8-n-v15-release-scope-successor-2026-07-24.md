# M8-N V15 fixed-Chinese baseline release-scope successor 结论

## 结论

- 产品决策：`keep_fixed_chinese_prompt / close_V15 / release_V1`。
- V1 matrix：`15 pass / 1 blocked -> 16 pass / 0 blocked`。
- 发布状态：`V1 可发布`，含义是 release-ready；本切片没有 push、publish 或远端变更。
- live admission：`inadmissible_no_material_treatment`。
- credential read=false、official DeepSeek API requests=0、external network=false。
- production constitution、默认 fixed Pro、official DeepSeek ChatCompletions、
  AgentRuntime、RunStore、工具目录和协议版本均未改变。

M8-N 关闭的是 fixed-Chinese baseline 的发布证据，不是 M8-D/M8-M candidate 的质量门。
`b088fd13` full-Chinese treatment 继续为 reject，M8-D/M8-M fact-gap treatment 继续为
hold；不得续跑、补 mate、拼样或把 unknown billing 当成零成本。

## 验收边界

`PRODUCT_PLAN` 6.1 与旧 V15 原本混合了两件事：

1. 未来改变 prompt 语义的候选只有在 current same-revision 同任务 A/B 无能力回归且有
   明确净收益时，才可以接管 production；
2. 已经 shipped 的固定中文 baseline 必须具备可追溯/回滚身份、合格 real-model
   coding/false-success evidence 和 exact-current production retention。

ADR-0007 保留第 1 条，不再把一个从未接管的可选 candidate 变成第 2 条的永久外部依赖。
未接管的候选不构成发布依赖；未来新候选仍不能借用本次 release evidence 绕过 A/B。

## 固定身份

| fact | value |
|---|---|
| clean baseline | `21200ccf0b4118793f1ad4e64acc973c25741e08` |
| contract/Harness checkpoint | `1a656bee` |
| accepted release candidate | `498599dd33b88f64a25107a004906112a8bc95b6` |
| candidate tree | `014434b4cb3dfaef68f0f7d3311aa19e6dbf5e2c` |
| Run API / RuntimeEvent / State / exec-stream | `v11 / v17 / v23 / v3` |
| constitution SHA-256 | `39f2eeb30519e143eed2d4c627fcb97d323c6b95ad9060816a93b72ea994d409` |
| manifest canonical SHA-256 | `05b89a119bc90cb2bde4f9e19e79271f876f0f264c50f9e71ee2513c2d51a41c` |
| manifest file SHA-256 | `eb1489139443c47440a5ae02780b22ca9a318847782ce6b8294962198db7d391` |
| one-shot Harness SHA-256 | `ffe8fc9b6718806e6ad961124ba76335ea9e073522926602d51a3fd880430de8` |
| Harness test SHA-256 | `bb46bc9445e880bbce1050c8cb6628f2aafd9d1d08aa89c829cad4c560a38dfb` |
| maximum reruns | `0` |

current constitution 在以下 revision 中 byte-identical：

- M5-A baseline `503d6294`；
- M5-A candidate `ba25f836`；
- M6-A official Writer canary `a982a9a8`；
- M8-M immutable live binary `d1d6ca5c`；
- M8-N clean baseline `21200ccf`。

`crates/context/src/prompts.rs::BASE_PROMPT` 继续在编译期 include 唯一 bundled
constitution。显式 local override 仍需用户 opt-in；它不是第二个 shipped default。
prompt rollback 由现有 immutable whole-release rollback负责，不增加 prompt selector、
version store、mode 或 compatibility branch。

## 合格 evidence chain

### 官方 DeepSeek coding 与 false-success

M5-A private raw 保持 `0600`，SHA-256
`42ac722d6f25abe6b066eee850ef5190b2e0367dfd91bed00472bad652e7b05c`。12/12 arms 均
`product_metric_eligible`：

- coding baseline/candidate 均为 3/3 verified；
- coding candidate false-success=0；
- forced-false-claim baseline 为 3/3 false-success；
- forced-false-claim candidate 为 3/3 correct rejection、false-success=0；
- 40 个 physical requests、144,903 tokens、`$0.004110153` accounting 完整。

两臂使用与 current byte-identical 的 constitution。该结果证明固定 baseline 出现在一份
窄范围、计费闭合的真实 coding/false-success evidence 中；不证明中文优于英文，也不证明
所有仓库的广泛质量。

### exact-current diagnostic 与 Writer

M8-M private raw 保持 `0600`，SHA-256
`641d7d80129b4673bd93eb4d4a0af57f278b326357af71c04188b8eae74acec7`。其 t1、t3、t5
current baseline observations 均 measurement-valid、verified、false-success=false；
t5 覆盖 read-only child。第 6 arm 的 candidate unknown billing 继续使 suite
product-metric ineligible，不能计算 candidate benefit。

M6-A Writer raw 保持 `0600`，SHA-256
`5e7ae080c6beeffff254ab286ff65e714453ac7af9d327e9c7a472e3d0d72a52`。它使用同一
constitution，通过 7/7 usage-bearing responses、Writer integration、latest-root receipt
和 cleanup；只作为机制证据，不作为 Writer 收益结论。

## 正式离线结果

private result：

- path：`eval/results/m8-n-v15-release-scope-21200ccf-v1.json`；
- mode：`0600`；
- bytes：5,324；
- SHA-256：`e690b28d2304646145fd136f3bef0b043d680d3fe7ea496cf40af33964ea4bfc`；
- decision：`keep_fixed_chinese_prompt_close_V15_release_V1`；
- source before/after 均为 clean `498599dd` / tree `014434b4`；
- 7/7 exact-current gates exit 0。

七项 gate 覆盖：

1. prompt structure、固定中文、cache block 和 source provenance；
2. exec/app-server 同一 context-owned override loader；
3. root/read-only child/Writer RequestPlan 与 accounting SQLite reopen；
4. root/read-only Host route audit reopen；
5. production Git edit、verifier failure/recovery、latest receipt 和 physical accounting；
6. false completion 被唯一 Host completion owner拒绝；
7. explicit Writer worktree、integration、latest-root verify 和 cleanup。

此外以下门禁通过：

- 11/11 M8-N contract tests 和 Harness self-test；
- `cargo fmt --all -- --check`；
- `./scripts/dev-codewhale.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- process crash/reopen、RunStore、Writer、exec/HTTP/stdio/TUI parity；
- delivery fixture install/upgrade/rollback/uninstall self-test；
- `git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8n-target` 和 offline dependency resolution。

## release artifact

locked/offline candidate artifact：

| fact | value |
|---|---|
| artifact | `codewhale-0.8.68-aarch64-apple-darwin-498599dd33b8.tar.gz` |
| artifact SHA-256 | `52ee0b55314df5a726b414809a2d2937a0c881f41b4706592bcccf19fc23c766` |
| checksum sidecar SHA-256 | `dad360392a984b4ed23506e788adcaf269846958755078e840148d47b626528b` |
| `codewhale` SHA-256 | `25aa0d69428911dc4b20904130c4099c8ed326d24d4aaaf835b6462f1add9be5` |
| `codewhale-tui` SHA-256 | `c73c7c3ac7501aa68a6796147a94546c192a7c81f8bc198107e188bc07c08630` |
| Rust | `1.97.0` |
| target | `aarch64-apple-darwin` |

artifact manifest精确绑定 candidate revision/tree、Cargo.lock 和两个 binary hashes。
install/verify/uninstall 通过，用户数据 sentinel hash 保持不变。artifact 与 external
target/dist 在记录哈希后删除；本次不发布 artifact。

## 删除与保留

删除：

- `crates/app` 中 M8-D candidate-only subprocess probe 和 prompt-only compare/reopen
  test；
- current-tree `eval/fixtures/m8-d-prompt/v1/constitution.md`；
- 通用 app-server override regression 中失去产品意义的 M8-D 私有 marker/request id；
- formal 后的一次性 M8-N Python evaluator/test。

保留：

- `crates/context` 唯一 production prompt owner；
- app-server/exec/TUI 共用 override loader 及通用进程级一致性测试；
- official DeepSeek Chat planner/sender/parser/accounting；
- 唯一 AgentRuntime、RunStore、tool catalog 和 delivery owner；
- frozen Git history、M8-D/M8-M manifests/summaries 与 ignored `0600` raw；
- 未来 prompt 语义候选的 current same-revision A/B 接管门槛。

accepted candidate 相对本次 authority/debt切片为 223 insertions、322 deletions；
production behavior 只减少无消费者测试资产，不改变模型可见 prompt 或发送链。

## 非结论

M8-N 不证明：

- current 中文 prompt 优于英文或任何 rejected/hold candidate；
- M8-D/M8-M candidate 更好、更差或等价；
- M5-A 覆盖所有仓库、语言、actor 或动态 prompt block；
- M6-A 证明 Writer 有产品净收益；
- Auto 已达到 fixed-Pro 非劣和效率门槛或应成为默认；
- FIM、Anthropic、Provider、多 Writer、RepoGraph、第二 Runtime/Store 应重新进入；
- release-ready 等于已经发布。
