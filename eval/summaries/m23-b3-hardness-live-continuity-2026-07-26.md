# M23-B3 Hardness live continuity caller

- 日期：2026-07-26
- 起点：`ab992121a97ff3a51558f0855997f138debaac9e`
- 决策：`keep_live_continuity_caller_control_not_acquired`
- production delta：0
- official Key / API / external network / historical raw：0 / 0 / 0 / 0

## 真实问题、owner 与替代旧路

M23-B2 已定义真实 mid-run resume 的必要事实，但正式 M23B caller 仍只会在终态停止
app-server，再用无凭据进程读取 SQLite。该动作能证明 Store exactness，不能证明运行能在
进程丢失后继续，也不能证明 reopen 不会重复物理模型请求或文件副作用。

唯一 owner 继续是 `scripts/eval-m9b-fixed-pro-regression.py`。本切片不改
AgentRuntime、RuntimeEvent、RunStore、DeepSeek sender/accounting、route、prompt 或
canonical tools；它替代 `m23b_live_continuity_not_implemented` guard 和没有
Hardness/truth 字段的 M23B arm/aggregate。

## 唯一 lifecycle

只有 B1 冻结的 3 个 `required_continuity` task 获得 caller override：

```text
interactive=true
auto_approve=false
```

其他 57 个 arm 继续使用 manifest 的 `interactive=false`、`auto_approve=true`。长任务
在第一个 durable approval checkpoint：

1. 保存 canonical root/child event prefix 与 physical-request count；
2. SIGKILL app-server process group；
3. 用同一 workspace、state DB、binary 与 credential 启动新进程；
4. 要求 PID 改变、event prefix byte-exact、reopen request count 不增长；
5. resume 同一 root 后才解析已持久化 approval；
6. 后续 approval 只在新进程内解析，直到 canonical terminal；
7. 最后仍以无凭据进程证明 terminal Store exactness。

若模型在 checkpoint 前已终止，caller 不伪造 restart；B2 projection 会得到
`goal_constraint_loss=true`。user-input、多个并发 pending interaction、prefix/request
差异、重开失败或副作用重复均 fail closed。

## 离线 process 证据

自测调用现有 `dse-app-server` external-process integration child。该 child 运行真实
AgentApplication、AgentRuntime、production tool catalog、DeepSeek transport 与 SQLite
RunStore；DeepSeek endpoint 仅在 test composition 内替换为本机 loopback
ChatCompletions SSE。shipped `dse app-server` 仍只接受官方 endpoint，没有新增测试或
Provider 产品面。

连续两次输出 byte-identical：

```text
durable interaction requested/resolved  1 / 1
physical requests before restart        1
physical requests at reopen             1
physical requests at terminal           2
process restarts                        1
applied file side effects               1
terminal credential-free reopen         exact
official Key / API / external network   0 / 0 / 0
maximum_reruns                          0
```

M23B self-test 还对 60 个 schedule arm 投影 controls，并用合成 measurement-valid arms
执行正式 aggregate：17 个正向 task 的 `pass_at_1=1`、17 个 `pass_power_3` cell，
3 个 continuity task × 3 round 恰有 9 次 resume，其余 task 为 0；behavior 为
51 个 verified success + 9 个 correct safety rejection，accounting 为 60 个 complete，
goal constraint loss 为 0。该合成结果只证明 evaluator contract，不是产品成功率。

## Metrics、truth 与删除

measurement-valid live arm 现在写入：

- B2 定位、首改、repair、重复 read、compaction、continuity、service/runtime 指标；
- ADR-0011 的 `behavior.status` 与 `accounting.status`；
- cell/summary 的 `pass_at_1` 与严格连续三次 `pass_power_3`。

终态 `sqlite_reopen_snapshot` 保留为独立 Store audit，不进入 `resume_count`。正式
admission 必须同时绑定 B1 manifest、B2 observer 和本 B3 manifest；不保留旧 guard、
第二 evaluator 或 compatibility reader。

## 非结论与下一步

本切片没有创建 live admission、读取 Key、调用官方 API 或生成正式 results，因此：

- 没有 fixed-Pro/high control success、false-success、pass^3 或 owner/cause loss；
- 没有授权 M23-C high/max；
- 没有授权 ApplicationProbe、localization、VerifiedMilestone 或 Tool ACI。

下一独立切片只能在 clean commit 上冻结 current binary、Harness、authority、schedule、
费用上界与 0600 ignored output admission，再从 position 1 执行 control-only acquisition。
任何 identity、accounting、continuity 或安全歧义都在下一物理请求前停止，绝不补 mate。

## 门禁与身份

提交前要求：

```text
python3 -m py_compile scripts/eval-m9b-fixed-pro-regression.py
M23B --hardness-continuity-self-test x2 + byte comparison
M23B --hardness-conformance x2 + byte comparison
M23B --self-test / --freeze-report
M14 / M16 / M23-A conformance
focused + fmt + workspace strict clippy/test
git diff --check
```

最终冻结身份：

```text
baseline revision             ab992121a97ff3a51558f0855997f138debaac9e
shipped binary sha256         7eddbd306bb86ff1fd574a1537633f9cd8dca85c170929495c5c3cfc389be7ef
process test binary sha256    59f542f940b1dff7f5b051863b81ee85b164ee0203c515b6ec42831eefc0f860
Harness sha256                f91cbc688730eea016177167bf7c6840a24c33bdfdda04c046ef0faeb4687c74
B3 manifest sha256            4dd5909ba826672bd8ee8df95ae756c1d10787857c1de5f469da7900f7230079
continuity report sha256      dbf7ab155ae32f948a66cbedab297bd617e3b6ea8807a616ccd0b1a898f78074
Hardness observer sha256      b13e6724bd33ea5903735fb62020fc38b6a9a37e6d3531e5b68b43bd26d7b27c
M23B self-test sha256         2f59f31b9a7503479589332d0ec845fbdad09d4b3f39d53dcfa5198e19471846
M23B freeze report sha256     c8d17e8dd371530311cfd743bc9e9b92926056533077d96ecf75e4d8e889cac5
```

`python3 -m py_compile`、连续两次 byte-identical process/Hardness observer、
M14/M16/M23-A conformance、M23B self-test/freeze、`./scripts/dev-dse.sh focused`、
targeted approval SIGKILL recovery、fmt、workspace strict clippy、workspace test 与
`git diff --check` 全部通过。所有 Cargo 命令使用
`CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true` 和独立
`/private/tmp/dse-m23b3-target`；提交前精确删除该 target 与本切片临时报告。
