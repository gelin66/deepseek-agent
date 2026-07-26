# M22 canonical streaming-delta convergence

日期：2026-07-26

结论：`keep_minimal_streaming_delta_convergence`

执行边界：纯本地、无 Key、无官方 API、无外部网络、无 GitHub/push/release

## 1. 问题、owner 与删除点

M20B 的两个 canonical Store snapshot 分别达到 sequence 2,993 和 5,298。只读派生得到：

| profile | total Store event | reasoning delta | content delta | delta share | delta UTF-8 bytes |
|---|---:|---:|---:|---:|---:|
| TypeScript | 2,993 | 2,743 | 209 | 98.6% | 10,097 |
| Python | 5,298 | 4,769 | 462 | 98.7% | 17,927 |

M20B journal 的 5,875,259 bytes 不能直接代表 production SQLite：corrected Harness 在
`canonical_store_snapshot` 和 credential-free reopen observation 中各嵌入了一份相同
Store facts。M22 因此只把事件数和 delta byte totals 写入 redacted fixture；prompt、
reasoning/content 原文、tool arguments、evaluation id、workspace、timestamp 和 credential
均不进入 M22 tracked evidence。

写放大的唯一 production owner 是 `crates/deepseek/src/transport.rs` 的官方 SSE sender/
parser boundary。旧路径对每个非空 reasoning/content SSE frame 立即产生一个
`ModelStreamEvent`，随后 `AgentRuntime` 为每个 event append 一次 canonical
`RuntimeEvent`。`StateStore` 已正确避免为 projection-neutral delta 重写 snapshot JSON，
但仍为每个 delta 执行一次 event insert、sequence advance 和 transaction commit。

cutover 删除点是 sender 中逐 frame `yield` 文本 delta 的分支。Store、Runtime、Run API、
TUI 和 headless client 不是修复 owner，不增加第二 reducer、Store 或投影。

## 2. 冻结身份

基线合同先独立提交：

- commit：`c7a77045746970a48c31314a09efdf98bb3fd86d`
- tree：`d88132a17f9bd545f52a968f99a8dc3a70c215c7`
- baseline manifest SHA-256：
  `357e7336090869d25f4efb92ef574e2f44ee60273ac067d1b5faa15268efbf34`
- fixture SHA-256：
  `ea257357ec5924506a340eded29b77237c289d61ea52ddb6fe188ff207ca9c31`
- Harness SHA-256：
  `dff1e7580ca1a67d2d5deb6fbed765e5079e5071f59bf4f2cd375f9bb79a5df7`
- ignored `0600` baseline result：
  `eval/results/m22-streaming-delta-baseline-c7a770457469-v1.json`
- result SHA-256：
  `340f2f51401e80bde12d5d43af834417fe03187289cfac0ac6a287e561eca7b4`

candidate：

- commit：`9f59d4a44fbcace49d5c8fb150b91742c2035fb7`
- tree：`5d69d96d5a615913e76ec6541a31f199eb442367`
- candidate manifest SHA-256：
  `f752eecc83e3723fce868c87f9358e48153051ba8fa34f1de72c5ab855c03e87`
- Harness SHA-256：
  `fd166795547cea7a5592798f6347bae34f3077cd6b57c72a2c714cf09d9c3c95`
- ignored `0600` candidate result：
  `eval/results/m22-streaming-delta-candidate-9f59d4a44fbc-v1.json`
- result SHA-256：
  `591d983f2e418066968dd03740da97d9eb46ff4af540b45ae19ff1a6d52b80d3`

两个合同均为 1 warmup + 5 measured repetitions、`maximum_reruns=0`。没有读取
`key.txt`，没有 official model request、known API cost 或 external network；candidate
只访问本机 loopback fixture。

## 3. 基线 gate

基线使用真实 `StateStore`、`AgentApplication RunCommand::Events`、canonical JSON、
`CanonicalRunProjection` 和 headless exec serializer。5,234 synthetic event profile
的正式中位数为：

- append：891.748ms；
- credential-free reopen：40.047ms；
- Run API events + canonical JSON：122.883ms；
- TUI projection：19.131ms；
- headless projection：3.936ms；
- SQLite+WAL：2,437,120 bytes；
- canonical Run API JSON：1,348,985 bytes。

Run API+JSON 稳定超过预注册 100ms material threshold，授权一个最小 candidate audit。
该基线本身不授权 schema、Store 或 client 改写。

## 4. 最小 candidate

sender 在每个 reqwest body chunk 已经到达后：

1. 解析该 chunk 中当前可用的完整 SSE frame；
2. 只合并相邻、同类的 reasoning 或 content delta；
3. evidence、tool fragment、finish、usage、`[DONE]` 和 error 作为 flush barrier；
4. malformed frame 前先投影已收到的文本，再返回既有 typed failure；
5. 不等待下一 chunk，不增加 timer、配置、模式或 schema。

`AgentRuntime` 仍分配唯一 canonical delta index，`StateStore` 仍 append-only。旧 SQLite
逐 frame event 可继续按 RuntimeEvent v19 精确 replay；新 run 只是产生更少、payload
更大的同 schema event。这不是 compatibility reader 或双写。

## 5. 同源码 A/B

transport candidate 的五次 sample 完全一致：

- TypeScript：6 reasoning + 2 content event；
- Python：7 reasoning + 2 content event。

下游 A/B：

| profile | baseline delta | candidate delta | event reduction | SQLite reduction | Run API+JSON improvement |
|---|---:|---:|---:|---:|---:|
| TypeScript | 2,952 | 8 | 99.73% | 94.33% | 97.97% |
| Python | 5,231 | 9 | 99.83% | 96.05% | 98.62% |

candidate 中位数：

| profile | append | reopen | Run API+JSON | TUI | headless | SQLite+WAL | canonical JSON |
|---|---:|---:|---:|---:|---:|---:|---:|
| TypeScript | 2.023ms | 1.266ms | 1.475ms | 0.075ms | 0.050ms | 90,112 | 15,885 |
| Python | 2.367ms | 1.289ms | 1.758ms | 0.086ms | 0.076ms | 106,496 | 23,874 |

两个 profile 的 event/SQLite 均远高于 50% reduction gate；原 material wall metric
远高于 20% improvement gate；所有测量 wall metric 无 10% regression。

## 6. 等价性与恢复门

以下本地门通过：

- complete response 的 reasoning/content 拼接与 atomic completed output byte-identical；
- finish、`[DONE]`、usage 与 accounting contract 不变；
- malformed frame 先 flush actionable delta，再 typed `invalid_json`；
- M21 truncated response 保持 actionable partial、no blind retry、no false completion；
- production loopback 通过真实
  `AgentApplication -> DeepSeekModelPort -> AgentRuntime -> RunStore -> Run API`；
- State delta 继续只 advance durability，不重写 snapshot JSON；
- credential-free SQLite reopen exact；
- app-server SIGKILL/reopen、pending Start、root/read-only/Writer conformance；
- CLI/TUI/app-server surface parity 和 exact canonical events；
- `./scripts/dev-dse.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- Harness self-test 与 `git diff --check`。

所有 Cargo 命令均使用：

```text
CARGO_INCREMENTAL=0
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/private/tmp/dse-m22-target
```

## 7. 决策与非结论

决策为 `keep_minimal_streaming_delta_convergence`。旧逐 frame production emission 已被
同 chunk convergence 物理替代；没有 compatibility branch、第二 sender、第二 Runtime/
Store、schema migration、用户模式或配置开关。

本结论不证明：

- 真实 DeepSeek、代理和 OS 的每个网络 chunk 都与本机 loopback 有相同分布；
- coding verified success、Token 或 API cost 已改善；
- M20B 可以补算为完整 fixed-Pro baseline；
- 5.87MB historical evaluator journal 应被改写或删除；
- 可以弱化 M21 partial-response fail-closed 或 `billing_unknown -> stop`。

下一步只应从新产生的 canonical trajectory 选择重复、可归因的 current loss；不应继续
围绕 streaming event 形状堆叠第二轮压缩、timer batching 或 Store compaction。
