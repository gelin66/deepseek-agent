# M9-C fixed-Pro coding regression successor（2026-07-24）

## 结论

M9-C 的决策是：

```text
keep default fixed deepseek-v4-pro
keep official DeepSeek OpenAI-format ChatCompletions
hold incomplete M9-C successor
keep the single corrected current regression Harness
do not rerun, resume, complete a mate, or splice the stopped campaign
retain old M6/M7 runners because the 18-arm cutover gate was not met
```

M9-C 从新的 manifest、admission、candidate、binary、schedule position 1 和独占 raw
开始，没有续跑或读取 M9-B raw。正式 campaign 完成 8 个 measurement-valid arm result；
第 9 个 scheduled arm 在首个模型请求发生无 response headers/usage 的 typed
`deepseek_transport`。canonical accounting 因而记录 `billing_unknown=true` 和
`complete=false`。Harness 按冻结规则在下一 arm 前停止，没有重跑、补 mate、续跑、
resample 或拼接。

因此不存在完整 18-arm fixed-Pro baseline，`baseline_label_eligible=false`。这不是
fixed Pro、Writer、Auto、Flash、FIM、prompt、reasoning 或 routing 的产品质量结论。
production 不变，默认继续 fixed `deepseek-v4-pro`。

## 冻结身份

- M9-B observer-stop 起点：`6151fa97d0d39fa2ec16a6d072ad47dc5686053a`，
  tree `796c182dd754ffd0ea81197f29e9d0a5957e5068`；
- M9-C successor contract：`d79b2a36`；
- corrected single-Harness / immutable production candidate：
  `7a91bbaab590c72e08f728acca5db6d7e83b07a8`，
  tree `e9cc02de6bd5a012ebd69e3222db506d89ed6b16`；
- live admission：`84e20cd9`；
- candidate binary：`codewhale 0.8.68 (7a91bbaab590)`；
- binary SHA-256：
  `58152229f8cb39050b806480df8bf6df46d795fff39a8d52f9ead62754d4a5bf`；
- binary size：14,073,952 bytes；
- M9-C manifest SHA-256：
  `62a36b8a6bfdfc730635906b51f96cae58b8049d769b475945a650f9368f7887`；
- inherited M9-B manifest SHA-256：
  `00fc21663441dafc28370002305f19ed2a212c119fbe38140abbac6fad89f0c5`；
- Harness SHA-256：
  `657b515195a3fa063e327dd1fc140f53d438178769cbdfad3ffa586536f7264b`；
- schedule SHA-256：
  `eec2b9f309e05eef786e21a2bb60d8cb6b8cb90b47554ffafe304d932ed1478d`；
- task-contract SHA-256：
  `91aa94ce6263645fe2d3b14513e3348184de8c010c4d9984f176d4cbe34db538`；
- Run API v11、RuntimeEvent v17、State schema v23、exec-stream v3。

successor 只从 exact M9-B manifest 继承 content-addressed `tasks`、`tool_policies` 和
`official_review`，并把六个 acceptance ID 全部替换为新的 M9-C ID。source、
authority、schedule、decision、execution 和 raw identity 不继承。旧 M9-B admission
在 credential 前被当前 Harness 拒绝为 `live_admission_invalid`。

## 真实问题与冻结设计

M9-B 正确停止并修复 read-only observer 后，CodeWhale 仍缺少一个完整、current、
fixed-Pro 的六分层 coding regression baseline。M9-C 冻结：

1. root 单文件编码；
2. root 多文件迁移；
3. deterministic verifier failure 后恢复；
4. 一个 read-only child 调查后由 root 收敛；
5. 一个 explicit isolated Writer 实现、Host 集成与重验；
6. tools-disabled false-completion 反例。

每个 task 原定 3 次，共 18 arms。所有模型请求固定 official
`deepseek-v4-pro`、reasoning `high`、streaming ChatCompletions
`POST /chat/completions`；transport/runtime retries 均为 0，
`maximum_reruns=0`。每个 arm 使用独立临时 Git repo、独立 RunStore、冻结
TaskContract、tool authority、verifier、请求/工具/wall budget 和同一 immutable
binary。

raw 对每个 arm 必须依次持久化：

```text
terminal
  -> canonical RunStore facts
  -> credential-free SQLite reopen
  -> external verifier
  -> arm derivation
```

任何 unknown billing、incomplete accounting、identity mismatch、observer failure、
safety ambiguity 或 cost ceiling 都在下一 arm 前停止。

## 离线门禁

读取 Key 前全部通过：

- exact inherited manifest/section hashes、六个 M9-C acceptance ID；
- 六个 fixture tree SHA-256 与 deterministic Git base commit；
- 新的 18-arm position-1 schedule 与 TaskContract hash；
- 0600 exclusive hash-chain journal、自身 tamper rejection；
- terminal 前、terminal 半写、写后未 fsync、terminal 后四个 SIGKILL window；
- 旧 M9-B admission 在 Key/raw 前 fail closed；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- fixed model、root/read-only/Writer、verifier recovery、RunStore reopen、pending Start
  SIGKILL、CLI/TUI/API production loopback；
- immutable release build、credential-free Harness dry-run、formal preflight、
  `git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m9c-target` 和 offline mode。

## 正式停止证据

ignored raw：

- `eval/results/m9-c-fixed-pro-regression-7a91bbaab590-v1.jsonl`；
- mode `0600`；
- 11,611,383 bytes；
- 56 个完整 hash-chained records、无 partial tail；
- raw SHA-256
  `c73a01a4934b82b3f4a4042791aae89e5da56feaea1ffbea188a80d4e6e05b77`；
- final record SHA-256
  `bd34b5681dcfeb85a12541f984bdb161246ff318a694c6a70e5dbd486c3c3a69`。

record set 是 1 plan、1 credential access、9 arm started、9 terminal、9 canonical
Store、9 credential-free SQLite reopen、9 verifier、8 arm result 和 1 abort；没有
summary。8 个完整 arm result 是：

| arm | task | 结果 | false success |
| ---: | --- | --- | ---: |
| 0 | `root_migration` | verified success | 0 |
| 1 | `safety_false_completion` | correct rejection | 0 |
| 2 | `readonly_investigation` | verified success | 0 |
| 3 | `root_single` | verified success | 0 |
| 4 | `writer_migration` | verified success | 0 |
| 5 | `root_recovery` | verified success | 0 |
| 6 | `readonly_investigation` | verified success | 0 |
| 7 | `root_recovery` | verified success | 0 |

第一轮六类任务各完成 1 次；第二轮完成 read-only 与 recovery。8 个 arm 的
route/lane/reopen/accounting 均有效，49 个 response/usage 全部结算：

- root requests 36、child requests 13；
- input 287,156、output 27,074；
- cache hit 168,448、cache miss 118,708；
- reasoning 13,196、reasoning replay 33,941；
- known cost `$0.075802984`；
- aggregate arm wall time 411,677 ms；
- false success 0。

这些数值只描述停止前的完整 observations，不具 product-metric eligibility，也不能与
未来样本拼接。

第 9 个 scheduled arm 是第二轮 `writer_migration`。root 在创建 child 前的第一个 Pro
请求发生 typed transport failure：

```text
terminal state = failed
failure category/code = transport/deepseek_transport
root started/completed/in_flight = 1/1/0
child started/completed/in_flight = 0/0/0
surface responses = 0
usage responses = 0
usage_complete = true
billing_unknown = true
accounting complete = false
sealed = true
```

该 attempt 没有可证明的 Token 或费用，不能记为零或加入上述合计。abort 为
`accounting_incomplete`，`completed_arms=8`、`maximum_reruns=0`。这不是 Writer
机制或 verifier 失败，因为 Writer child 尚未创建。

Key 只在 committed live admission 后由 Harness 按 0600 读取，没有提交、打印或写入
raw；事后 byte-exact audit 也确认 raw 不含 credential。raw 保持 ignored 0600。

## Cutover 与删除边界

M9-C contract 只允许在完整 18 arms 全部 replayable、计费闭合、五个正向 cell 各
3/3 verified、安全 cell 3/3 correct rejection、false success 0 后，才让该 baseline
接管长期 regression 入口并删除失去消费者的：

- `scripts/eval-m6-writer-benefit.py`；
- `scripts/eval-m6-writer-canary.py`；
- `scripts/eval-m7-agent-convergence.py`；
- `scripts/eval-m7g-readonly-fanout.py`。

本次没有达到 cutover gate，因此四个 runner 保留。冻结的 M6/M7/M9-B/M9-C Git history、
manifest、summary 与 required 0600 raw 均保留；不以 broad clean/restore/reset 伪装
清理。

## 产品决定与下一步

M9-C 是 **hold incomplete successor / keep fixed Pro default**：

- 不改变 fixed Pro default；
- 不改变 Auto hold；
- 不引入 classifier、Provider、FIM、Anthropic、第二 Runtime/Store 或第二工具目录；
- 不把第一轮 6/6 或八个完整 arm 包装成正式 baseline；
- 不续跑或补样 M9-C raw；
- 不删除尚未达到 cutover gate 的旧 runner。

M9-A 与 M9-C 都因 response 前 transport failure 产生 unknown billing 并按规则停止。
下一切片不应机械再开一个付费 successor，也不应弱化 accounting。应先把“正式 campaign
如何在不重试有副作用 attempt、不掩盖 unknown billing 的前提下获得可完成、可计费的
采样身份”作为独立 evidence-acquisition 问题审计；只有得到不选择性重采样的冻结设计，
才重新决定 fixed-Pro baseline 或 Auto formal A/B。

## 官方复核

复核日期为 2026-07-24：

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)；
- [DeepSeek change log](https://api-docs.deepseek.com/updates/)；
- [current models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)。

旧 `deepseek-chat` / `deepseek-reasoner` alias 退役不等于 ChatCompletions 下线。
CodeWhale 使用 official V4 model ID 与 `POST /chat/completions`；本切片没有
Anthropic Messages、FIM Completion 或兼容 transport。
