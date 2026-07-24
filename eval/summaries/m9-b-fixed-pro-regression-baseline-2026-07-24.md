# M9-B fixed-Pro coding regression baseline（2026-07-24）

## 结论

M9-B 的决策是：

```text
keep default fixed deepseek-v4-pro
keep official DeepSeek OpenAI-format ChatCompletions
hold incomplete M9-B baseline
fix and keep the single current regression Harness
do not rerun, resume, or splice the stopped v1 campaign
defer old M6/M7 runner deletion until a fresh successor proves all six lanes
```

这不是模型质量失败，也不是 Auto、Flash、FIM、Anthropic 或新路由的评测。正式 v1 在第
2 个完整 arm 派生结果时发现 Harness 把 read-only child 共用的
`agent_result_collected` 错当成 Writer-only event。该 observer failure 会把真实有效的
read-only 生命周期错误标为 `lane_valid=false`。按冻结的 observer-failure stop rule，
第 3 arm 运行中立即中止；没有继续花费、重跑、补样、续跑或拼接。

因此没有 18-arm fixed-Pro baseline，也没有可发布的新 regression label aggregate。
production 不变，默认继续 `deepseek-v4-pro`。

## 冻结身份

- 起始 release-ready checkpoint：`a09ca2eb`；
- M9-B contract：`7ba7552b`；
- immutable production/evaluator candidate：`983fa9cee7111dd8b2b6b19ec21264fe39819f0f`，
  tree `a0ebfc83bffdf905d21b584828bf92d32871865d`；
- live admission：`37c4cc96`；
- observer correction：`9342fb03`；
- candidate binary：`codewhale 0.8.68 (983fa9cee711)`；
- binary SHA-256：
  `e16da6234d6a51ed311314f3ff1fda159b6fd05bb660739715a9612ff85da594`；
- pre-correction Harness SHA-256：
  `bbf1ec48510efa9867927e9d7effad535cb0c4746b27b148480034abd1b82d9d`；
- corrected Harness SHA-256：
  `2324eb2623bb27e743b214967f07d1a4142a85632c629dca37742f3d5ecad6a9`；
- Run API v11、RuntimeEvent v17、State schema v23、exec-stream v3。

live admission 精确绑定 pre-correction Harness 和 candidate binary。observer correction
改变 Harness hash 后，原 admission 会 fail closed，不能被当前 runner 静默复用。

## 真实问题与冻结设计

V1 已 release-ready，M9-A 也正确保持 fixed Pro default，但 current exact-source
retention 主要来自 deterministic loopback 与分散的历史 paid suites。M9-B 试图建立一个
同 binary、跨六类真实临时 Git repository 的 fixed-Pro regression baseline：

1. root 单文件编码；
2. root 多文件迁移与回归测试；
3. deterministic verifier 失败后恢复；
4. 一个 read-only child 调查后由 Pro root 收敛；
5. 一个 explicit isolated Writer 实现、Host 集成与重验；
6. tools disabled 的 false-completion 反例。

每个 task 原定 3 次，共 18 arms。所有请求固定 official
`deepseek-v4-pro`、reasoning `high`、streaming ChatCompletions
`POST /chat/completions`；transport/runtime retries 均为 0，
`maximum_reruns=0`。每个 arm 使用独立临时 Git repo、独立 RunStore、冻结 verifier、
路径范围、请求/工具/wall budget 和同一 immutable binary。

raw 必须依次持久化：

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

- 六个 fixture tree SHA-256 与 deterministic Git base commit 精确重建；
- 18-arm 三轮日程和 TaskContract hash 冻结；
- 0600 exclusive hash-chain journal 自检；
- terminal 前、terminal 半写、写后未 fsync、terminal 后四个 SIGKILL window；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`；
- `cargo test --workspace --locked --offline`；
- fixed-model、root/read-only/Writer、verifier recovery、RunStore reopen、pending Start
  SIGKILL、CLI/TUI/API production loopback；
- immutable release build、credential-free Harness dry-run、`git diff --check`。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m9b-target` 和 offline mode。

## 正式停止证据

ignored raw：

- `eval/results/m9-b-fixed-pro-regression-983fa9cee711-v1.jsonl`；
- mode `0600`；
- 4,058,364 bytes；
- 15 个完整 hash-chained records、无 partial tail；
- raw SHA-256
  `67df6a7056edefbf680113536e9e85b42df46bdd86ce08541865d8962f798fcc`；
- final record hash
  `703a487b65855d06db794d03ec078d3509718883bf45d2baf12a39c5e02fb932`。

raw 包含 2 个完整 arm result 和第 3 arm 的 `arm_started`。前两个 arm 都完整保存
terminal、Store、无凭据 reopen 与 verifier：

- `root_single`：terminal completed、verified=true、false success=false、route/lane
  valid，5 requests，known cost `$0.007344308`；
- `readonly_investigation`：terminal completed、external verifier pass、exact path、
  Host receipt、route valid；原 Harness 记录 lane invalid/false success，唯一 reason 是
  错误的 `readonly_writer_lifecycle_present`。

对同一个 durable Store snapshot 使用修正后的纯观察逻辑重放，read-only arm 得到：

```text
agent arguments valid = true
child terminal = completed
child writes = []
children = 1
root mutation after handoff = true
reasons = []
lane valid = true
```

这证明 observer defect，但不能把已经停止的 v1 重写为完整 baseline。两个已结算 arm
合计 12 requests、62,606 input tokens、6,834 output tokens、35,072 cache-hit tokens、
27,534 cache-miss tokens、known cost `$0.021395127`。第 3 arm 是
`writer_migration`，在 observer failure 被发现时仍在运行；它没有 terminal/Store/
accounting snapshot，可能存在未证明计费的 attempt，因此不加入 request、Token 或费用
合计，也不估价为零。

Key 只在 live admission 后由 runner 按 0600 读取，没有提交、打印、写入 raw 或 stderr。
中止后没有残留 app-server、临时 arm、worktree 或 frozen binary。

## Observer 修复与删除边界

read-only 与 Writer 共用：

- `agent_task_prepared`；
- `child_started`；
- `agent_result_collected`；
- `child_finished`。

只有 `agent_workspace_created`、seal、integration 和 cleanup 事件是 Writer-only。修复把
这个边界固化进 Harness self-test；未修改 Runtime、Store、工具目录或 production route。

冻结 contract 只允许在新 runner 实际证明全部六类 lane 后，删除当前树中失去消费者的
M6/M7 one-off live runners。v1 没有达到该 cutover 条件，因此本切片不伪装清理：
`eval-m6-writer-benefit.py`、`eval-m6-writer-canary.py`、
`eval-m7-agent-convergence.py` 和 `eval-m7g-readonly-fanout.py` 暂时保留。它们的删除
只能由 fresh position-1 successor 的完整证据触发；历史 Git、manifest、summary 与
required 0600 raw 始终保留。

本切片另行精确删除六份没有任何 current caller/reference、可由冻结 Git identity 重建的
ignored prompt A/B binary copies；没有删除任何 raw JSONL、manifest 或 summary。

## 产品决定与下一步

M9-B 是 **hold incomplete baseline / keep corrected Harness**：

- 不改变 fixed Pro default；
- 不改变 Auto hold；
- 不引入 classifier、Provider、FIM、Anthropic、第二 Runtime/Store 或第二工具目录；
- 不把两个完整 arm 或离线重放包装成 product metric；
- 不续跑 v1 raw。

下一次只能以新的 manifest、admission、raw path、candidate/harness hash 从 position 1
启动。只有完整 18 arms 的 route/lane/reopen/accounting 全部有效，才允许保留 baseline
并删除被替代的 current-tree milestone runner。

## 官方复核

复核日期为 2026-07-24：

- [DeepSeek V4 release](https://api-docs.deepseek.com/news/news260424/)；
- [DeepSeek change log](https://api-docs.deepseek.com/updates/)；
- [current models and pricing](https://api-docs.deepseek.com/quick_start/pricing/)；
- [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)。

旧 `deepseek-chat` / `deepseek-reasoner` alias 退役不等于 ChatCompletions 下线。
CodeWhale 继续使用最新 official V4 model ID 与 `POST /chat/completions`；本切片没有
Anthropic Messages、FIM Completion 或兼容 transport。
