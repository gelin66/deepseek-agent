# M16 acceptance-equivalence observer cutover

日期：2026-07-25
决定：`keep_acceptance_equivalence_observer / close_m15_acquisition / no_production_treatment`

## 1. 真实问题与边界

M15 的冻结 external verifier、latest-revision Host receipt、fixed route、root lane 和
terminal 都接受了一项 TypeScript 两文件实现；旧 evaluator 却因为它没有修改参考 patch
预设的第三个 helper，把该 arm 标成 `false_success`。这证明
`expected_changed_files == actual changed_files` 把实现偏好提升成了第二完成权威。

M16 的唯一 owner 仍是
`scripts/eval-m9b-fixed-pro-regression.py`。本切片没有修改 `crates/`、Cargo identity、
Run API v12、RuntimeEvent v18、State v24、exec-stream v3、DeepSeek
ChatCompletions sender、fixed actor route、AgentRuntime、RunStore、工具目录或 production
verifier。

冻结权威分工为：

| 事实 | 权威 |
| --- | --- |
| 可修改范围 | `allowed_paths` |
| 行为验收 | 运行前冻结的 external verifier |
| 最新 revision 完成证据 | Host `EvidenceReceipt` + completed terminal decision |
| 实际修改 | Git `changed_files`；Writer seal + integration |
| 参考实现文件集 | 只读诊断，不参与完成权 |
| crash/reopen | canonical facts byte-equivalent |

## 2. Cutover 与删除

current Harness 现在：

- 把 frozen manifest 的旧 `expected_changed_files` 名称仅在历史读取边界转换为
  `reference_changed_files`；
- 输出 `scope_audit`，区分 `exact`、`implementation_subset`、
  `additional_within_scope`、`alternate_within_scope`、`no_change` 与
  `outside_scope`；
- 正向任务只有在修改非空且全部位于 allowed scope、external verifier 通过、route/lane
  有效、terminal completed、receipt 与 `workspace_state_after` 和 terminal decision
  完全绑定时才是 `verified_success`；
- Writer seal 与 root integrated diff 必须相等、非空且在 allowed scope，不再等于参考
  patch 的 exact file set；
- 用一个与 campaign 名无关的 frozen-reference projection 取代 M15 专属 changed-file
  特判；
- 在读取 Key 或联网前以 `m15_campaign_closed` 拒绝 M15 formal，物理删除旧的 M15
  false-success live abort 分支。

冻结 M9-C/M11/M12/M15 manifest、M15 raw、M15 acquisition labels 与历史 runner 没有修改。
M15 raw 保持 ignored、0600、42,745,897 bytes，SHA-256：

```text
f2466ef4c04a742b854a6e8ee367769ef0499a241e1160f5e60b0944fd8b41ac
```

## 3. M16 typed corpus

`eval/manifests/m16-acceptance-equivalence-observer-v1.json` 固定 baseline：

```text
revision 682e06ef889deb46c8d7aa9b80181a5adc176dc6
tree     e3b4b554efc259e5d103ed83d85e9cef1bb2822f
```

`eval/fixtures/m16-acceptance-equivalence-observer-v1.json` 固定 18 个 case：

- exact、M15 equivalent subset、alternate 与 additional in-scope 四类正例；
- out-of-scope、verifier failure、missing/stale receipt、receipt 后修改、无 terminal；
- exact reopen 与 reopen drift；
- no-change safety rejection、safety false completion 与 safety mutation；
- Writer subset、seal/integration mismatch；
- route 与 lane 反例。

连续两次：

```text
python3 scripts/eval-m9b-fixed-pro-regression.py --acceptance-conformance
```

输出 byte-identical：

- status：`pass`
- cases：`18/18`
- verified positive cases：5
- correct safety rejection cases：1
- planted false-success detections：9
- result SHA-256：
  `3ec9a44c2cf6e168625dea1dbbe1e107e29b7d8c684a8dff16c523b1c71704cd`
- report file SHA-256：
  `5df927c9b995880fafd8c9db8153c7289a967fbdb1583ce4dbcb8086282bb7f8`
- Harness SHA-256：
  `86ffa9f9ac4c14346ad3a2178f809c97d7d42c374bb0f565b6289f231a5afaa6`
- `historical_raw_read=false`
- `key_accessed=false`
- `network_accessed=false`

这里的 9 个 false-success 是 corpus 中刻意种植并被正确拒绝的 completed 负例，不是
production campaign 的 false success。

## 4. 历史兼容

- M14 observer 保持 12/12、6 positive / 6 negative，result SHA-256 仍为
  `09b840b8af0ee136d291a7ebf2203ebb05467fd24bc5adfa670dbacb093dd450`；
- M9-C、M11、M12、M15 Harness self-test 全部通过；
- M11、M12 trajectory report 继续为 `insufficient_repeated_current_loss`；
- M15 frozen labels 保持 11 verified / 2 correct rejection / 1 false-success；
- M15 corrected product projection 保持 12 / 2 / 0；
- `evaluation_scope_mismatches={"typescript_stacktrace":1}`；
- 剩余 current losses 仍只有不同 owner 的一次
  `deterministic_verifier_failed` 与一次 `writer_delegation_failed`，没有跨 task
  repeated owner。

没有重算、回写、补 mate、复用剩余 schedule 或读取 Key。

## 5. 离线与 production 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m16-target
```

通过：

- Python compile、M16 conformance 两次 byte identity、M14 conformance；
- M9-C/M11/M12/M15 self-test 与 M11/M12/M15 trajectory report；
- `./scripts/dev-codewhale.sh focused`；
- State stale-receipt、Writer lifecycle reopen、terminal exactly-once targeted tests；
- process crash recovery 38/38（1 个外部 helper ignored），覆盖 tool、Host verifier、
  read-only child、Writer、temporal receipt 与 creation reservation；
- `cargo fmt --all -- --check`；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`；
- `cargo test --workspace --locked`；
- root/read-only/Writer、production loopback、pending Start、CLI/TUI/API parity；
- `git diff --check`。

## 6. 决定、非结论与下一步

保留 M16 acceptance-equivalence observer，并永久关闭 M15 acquisition。参考实现文件集
仍可帮助解释结果，但不能决定 verified success 或 false success。

M16 不证明 fixed Pro 的完整回归基线已经完成，也不证明 Scoped Context Map、
Working-Set Selector、Acceptance Progress、Typed Recovery 或 Environment/Runtime
Artifacts 能改善产品；没有 production treatment、付费请求、Token/时间/费用收益或
DeepSeek 协议变化。

当前 M15 corrected trajectory 仍没有跨两个独立 task 的同 owner repeated loss，因此不
允许从它直接开发 A–E treatment。若未来需要新的 product-loss acquisition，必须另立全新
immutable task/binary/manifest identity，从 position 1 开始，保持 `maximum_reruns=0`，
并在 observer、latest-revision、accounting 或 billing 歧义时 fail closed。
