# M36-A DeepSeek-native verified Harness fresh baseline 结论

日期：2026-07-27
产品决定：`keep_current_harness_no_repeated_loss`
production delta：0

## 决策

保留 current DeepSeek-native Harness，不进入 M36-B/C，不实现 Tool ACI、localization、
VerifiedMilestone、ApplicationProbe、context 或 effort treatment。

20 个 fresh position-1 tasks 全部形成 canonical Store、credential-free reopen、external
verifier、behavior truth 和 accounting truth。结果为 16 个正向 verified success、3 个
正确安全拒绝和 1 个 measurement-valid Writer product failure，false success 为 0。唯一
canonical loss 是：

```text
orchestrator:writer_integration
  task              writer_envelope
  trajectories      1
  independent tasks 1
```

合同要求同一 `owner_code:loss_code` 至少跨两个独立任务重复。第二个 Writer
`writer_policy_migration` 完整成功，因此当前没有 treatment admission；不能凭一个失败把
竞品能力清单变成 backlog。

## 可复现身份

- M35 clean anchor：`e84abb1ed`
- 权威 M36 合同：`b9221fd1b`
- frozen acquisition candidate：`03b40abe6dcced24318589999f8c101e46f2b7ad`
- candidate tree：`3ae866c50237b4540b1e3fc2d07f083c06ea3b75`
- live admission：`d023e43a7d53a7c3ace77805d8ab96ba5082c386`
- contract manifest：
  `eval/manifests/m36a-deepseek-native-baseline-v1.json`
- contract SHA-256：
  `9d64cbacd47191b8e9f48f62f1e67244b7e3949d936b731d2c5c40f9e2253e61`
- admission SHA-256：
  `67b942cef9db53d9095ae0c6b80b38021332b44339f37ad43bf5be8ea7eceb5c`
- acquisition Harness SHA-256：
  `2c56f9370f6b0082689182e3a0fc796da761817dfbaa8fa81ec7a6defbd56d3c`
- analysis correction：`d0b4b7954fe12eb383a74638fe230bb4f04f7bed`
- analysis Harness SHA-256：
  `a0797c89ec6ba3252977d2b921b2c036fca61a178a4619753f5bb4a596d32601`
- analysis manifest SHA-256：
  `8026e338ebecf348f8d3611bf7822a127b9d40bb63c392957984be0869053db4`
- immutable `dse`：
  `sha256:c869bfc9ede5c17a66c2f13de53ec6e565b367f07bb346d0990a4206c6c0d2f1`
  (`dse 0.8.68 (03b40abe6dcc)`, 14,371,344 bytes)
- surface：official DeepSeek Standard streaming ChatCompletions、
  `deepseek-v4-pro/high`
- protocol：Run API v15、RuntimeEvent v22、State v28、exec-stream v6
- task/schedule：
  `8282784b864453301b21aceee00aeb4e23c75e4fb9b6ea5c218872cb67cee68a` /
  `621efff3ea6f5cc2ea34dc946145044f754dc87b5bbe51c79af2d5477a7e72a1`
- formal raw：
  `eval/results/m36a-deepseek-native-baseline-03b40abe6dcc-v1.jsonl`
- raw SHA-256：
  `aa78b5641782a9fee5c5a0f8daefab3fe178f9c8e7cede687555d7d60f454b2f`
- raw mode/size：ignored `0600` / 35,414,849 bytes
- journal：132 records，no partial tail，sequence/previous/hash 全部有效
- Harness rerun：0；normal Runtime safe-retry limit：2
- Key：只从 ignored `0600` test-key file 读取；未打印、未提交、未写入 raw
- GitHub/push/release：0

## 离线合同与正式 acquisition

credential 前完成：

- 20 个新 `m36a-*` acceptance identity，逐项人工复核 objective、constraints、
  non-goals、gold scope 与 hidden verifier；
- 17/17 positive reference solutions 通过，3/3 safety counterexamples 保持失败；
- 六个 task family，每类至少两个独立任务；
- 10-code canonical taxonomy 的 11/11 conformance；
- first relevant/correct edit、localization recall、Tool ACI、stale/truncation、context/
  stable-prefix/raw-output、completion rework、receipt 与 accounting observer；
- hash-chain crash windows和一次真实 process SIGKILL/reopen loopback；
- fresh workspace、HOME、RunStore、position 1；历史 raw/result/admission/label 均不是输入。

正式 acquisition 没有补跑、mate 或选择性重试。全部 20 arms 按 frozen schedule 执行，
known cost `$0.278444080`，低于 `$10` suite ceiling。

| task family | arms | verified | correct reject | product loss | requests | known cost |
|---|---:|---:|---:|---:|---:|---:|
| deterministic repair | 3 | 3 | 0 | 0 | 22 | $0.035594948 |
| large-repository localization | 3 | 3 | 0 | 0 | 29 | $0.040073302 |
| multi-module hard | 5 | 4 | 0 | 1 | 59 | $0.126471668 |
| long horizon | 3 | 3 | 0 | 0 | 21 | $0.038303925 |
| service/API/UI | 3 | 3 | 0 | 0 | 20 | $0.036374642 |
| false-completion adversarial | 3 | 0 | 3 | 0 | 3 | $0.001625595 |
| **total** | **20** | **16** | **3** | **1** | **154** | **$0.278444080** |

完整 canonical aggregate：

```text
positive pass@1                 16/17 (94.12%)
correct safety rejection         3/3
false success                      0
behavior product observations     20
accounting-complete observations  20
full-utility observations         20
input/output tokens       1,857,567 / 91,179
cache hit/miss tokens     1,547,008 / 310,559
wall time                  1,695,206 ms
Runtime retries                     0
compactions                         0
exact long-horizon resume          3/3
maximum Harness reruns               0
```

Token 数是 frozen Harness 的 canonical aggregate 字段；费用由完整 surface usage
accounting 计算。工具回合的多次 model request 不等于网络 retry。

service、HTTP API 和 DOM UI 三层都由外部 runtime assertion 验证通过。三个 long-horizon
任务均在确定性 SIGKILL/reopen 后只继续一次并完成。root positive、两个 read-only child
和第二个 explicit Writer 均完成。

`writer_envelope` 的 Writer child 因 agent invocation/cardinality 与 operation failure
未产生可集成 diff；Host 保持 blocked、无 latest receipt，isolated worktree cleanup 为
removed。这个结果是正确阻止 false completion 的同时真实未完成任务，因此记作
`verified_product_failure`，不是 Harness interruption。

## Acquisition summary defect 与只读修正

frozen raw 的 final summary 保留历史值：

```text
complete false
decision reject_incomplete_acquisition
```

原因不是缺 arm、计费或 verifier，而是 acquisition Harness 的 aggregate 额外要求每个
positive lane 都 valid；这把一个已闭合的 Writer product loss 误当成 acquisition
incomplete。raw、arm result、Store snapshot 和 admission 均未修改，也没有重新调用 API。

三个独立 correction commits：

1. `a74c3383e`：measurement completeness 只要求 closed behavior、complete accounting
   和 false success 0；verified product failure 是 loss，不是 interruption；
2. `c84c60f67`：read-only report 继续使用 M36 canonical loss taxonomy；
3. `d0b4b7954`：正式 report 使用
   `keep_current_harness_no_repeated_loss` 决策类。

analysis manifest 分别绑定 acquisition 与 analysis Harness hash。对同一 frozen raw 连续
运行两次 credential-free trajectory report，得到 byte-identical JSON：

```text
report SHA-256             05adc8fde512ad4bdb2332c0d5c8dc6d7a8dc4ba008b5856186d52406abf9517
canonical trajectories     20
completed arm results      20
behavior truth             16 success / 3 correct reject / 1 product loss
accounting truth           20 complete
full utility               20
false success               0
candidate                  null
decision                   keep_current_harness_no_repeated_loss
credential/network read    false / false
```

## 顶尖 Agent 机制审计

2026-07-27 复核的有效共同原则是少量高质量工具、按需上下文、可确定性验证、单一可恢复
轨迹和严格的 signal/noise 控制，而不是把竞品产品面拼装起来：

- DeepSeek V4 官方 Coding Agent 使用 Bash/编辑为核心的 minimal Harness；官方
  Thinking/Tool Calls 要求 reasoning、tool call 和 tool result 精确回放；
- OpenAI 的 coding-eval 经验强调任务可解性、deterministic grader 和污染审计，否则
  Harness 排名会被噪声主导；
- Anthropic 的 long-running harness 依靠清晰环境、增量进度和 session continuity，不等于
  增加独立 planner/critic/evaluator；
- SWE-agent 把 ACI 视为模型与计算机之间的关键接口；Agentless 证明简单 localization →
  repair → validation 也可有竞争力；
- Aider repo map 只支持 Token 预算内的符号/引用候选，不授权默认 RepoGraph/vector memory；
- OpenHands 的 Agent/Application/State 分层支持单一 durable truth，不授权第二 Runtime
  或多 Provider SDK。

这些依据只定义候选来源。M36-A 没有 repeated production loss，因此没有一个外部机制获得
production admission。

来源：

- <https://huggingface.co/deepseek-ai/DeepSeek-V4-Pro/blob/89d501aed998d33fa4f4702102ec1bb2331e10f6/DeepSeek_V4.pdf>
- <https://api-docs.deepseek.com/guides/thinking_mode/>
- <https://api-docs.deepseek.com/guides/tool_calls/>
- <https://openai.com/index/separating-signal-from-noise-coding-evaluations/>
- <https://www.anthropic.com/engineering/harness-design-long-running-apps>
- <https://swe-agent.com/0.7/background/aci/>
- <https://arxiv.org/abs/2407.01489>
- <https://aider.chat/docs/repomap.html>
- <https://docs.openhands.dev/sdk/arch/overview>

## 最终门禁暴露的 test-only 债

第一次、第二次 post-cutover focused 都在同一个 M31 app loopback test 的 5 秒 terminal
等待处超时；该 test 单独运行两轮约 9.68 秒。它每个 Run 固定执行五个 model/tool 回合，
test helper 的 timeout 与正常并行门禁开销没有余量。`8657a5b5c` 只把
`#[cfg(test)] wait_terminal` 的 bounded timeout 从 5 秒改为 15 秒，仍低于 task 的
30 秒 wall contract。修正后 targeted test、focused 和 workspace test 均通过。没有改
production Runtime deadline、retry、Store、permission 或任何用户行为。

## Cutover 与删除

- production-compiled `crates/`、protocol、State、config、CLI/TUI/app-server delta=0；
- 唯一 crate source delta 是上述一行 `#[cfg(test)]` timeout 修正；
- M36 temporary campaign、task overlay、observer、parser、admission loader 和 CLI flags
  已从唯一 corrected Harness 物理删除；
- Harness 恢复到 M36 前 exact Git blob
  `5f3f613cd68d57d14852cfbb51504088b24ad456`；
- 不保留 second evaluator、selector、compatibility reader 或 treatment；
- contract、loss fixture、live admission、analysis manifest、ignored `0600` raw、本 summary
  与 Git 历史保留审计。

## 门禁

admission 前及最终删除后通过：

- M36 reference proof、loss taxonomy、metric observer、journal crash windows；
- process SIGKILL/reopen loopback；
- corrected Harness self-test、credential-free report determinism、Python compile；
- M31 targeted multi-turn loopback（修正后 10.42 秒内完成）；
- `cargo fmt --all -- --check`；
- `./scripts/dev-dse.sh focused`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- public repository checker；
- `git diff --check`；
- exact pre-M36 Harness blob identity。

## 非结论与下一门

M36-A 不证明 DSE 在所有仓库、语言或公开 benchmark 上达到 94.12% pass@1，也不证明
`writer_envelope` 的失败一定来自通用 Writer owner 而不是这一个任务/模型轨迹。154 个
model requests 没有 Runtime retry，不代表 timeout/429/partial 永远不会发生。0 次
compaction 不评价 compaction 收益。Token、cache、cost 和 wall time 是 current baseline，
不是 treatment improvement。

因此不执行 M36-B/C。若产品仍要调查 Writer，下一 Goal 只能冻结新的独立 Writer tasks，
确认同一 `orchestrator:writer_integration` 是否跨至少两个 task_id 重复；未重复仍保持
production delta=0。不得用这个单点失败恢复 Auto、RepoGraph、FIM、planner/critic、
swarm、多 Writer、第二 Provider/Runtime/Store 或动态工具市场。
