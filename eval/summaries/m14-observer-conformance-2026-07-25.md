# M14 typed observer conformance

日期：2026-07-25

实现 checkpoint：`7a9e2278`

起始 revision：`e1aa0fe9e75dfaeabdd0a3455bcb6b10db522015`

决定：`keep_offline_observer_conformance / retire_m13_live_acquisition`

## 问题与边界

M13 三次 fresh position-1 acquisition 分别暴露 Writer scope order、malformed stale
patch 与 verifier disposition 三个 evaluator-contract mismatch。第三次的 production
terminal、external verifier、latest-revision Host receipt、changed scope 和 accounting
实际闭合；错误来自 observer 把已经执行外部命令的 `verifier_failed` 强制解释为
`not_applied/after_correction`，而 canonical `ToolOutcome` 正确保守表示为
`indeterminate/unsafe`。

M14 的问题不是模型能力，而是未来 acquisition 前必须先证明 observer 只读取 canonical
事实。本切片不读取 Key，不调用 DeepSeek，不生成或读取 raw，不重开、续跑或拼接 M13，
也不改变 production Runtime、Store、protocol、sender、tool catalog 或 route。

## 单一 owner 与 cutover

唯一 evaluator owner 仍是 `scripts/eval-m9b-fixed-pro-regression.py`。Rust owner 只提供其
已有稳定事实：

- `crates/protocol`：`ToolOutcome` invariant 与 Writer assignment contract；
- `crates/tools`：patch preflight/execution、verifier outcome；
- `crates/runtime` / `crates/orchestrator`：canonical Writer scope/worktree；
- `crates/state`：exact SQLite reopen。

Cutover 物理删除：

- closed `--campaign m13` live execution；
- M13 successor loader、M13-only environment/profile/admission/self-test；
- 不可达的 M13 trajectory branch；
- 把所有 required failure 隐式写成
  `side_effect=not_applied/retry=after_correction` 的 generic assertion。

M9-C、M11、M12 仍是 Harness 的真实消费者。M13 frozen manifest、fixtures、summary 与
ignored raw 保持历史不可变，不作为 M14 输入。

## 冻结 corpus

Manifest：
`eval/manifests/m14-observer-conformance-v1.json`

- SHA-256：
  `32386e1ad04c1d28a07723f19fe2f0f408eb2910153802071167a6140bcc012d`

Corpus：
`eval/fixtures/m14-observer-conformance-v1.json`

- SHA-256：
  `40e58f5a14008fa98fe1a347e3e5f2e30ebec9780aaee0cfaa977cffb15b705b`
- 12 cases：6 positive / 6 negative；
- semantic set canonicalization：3；
- patch parse / workspace precondition：3；
- verifier disposition：2；
- Writer assignment：2；
- exact reopen / crash drift：2。

稳定 ToolOutcome projection 只含：

`failure_code / invocation / transport / operation / side_effect / retry`

不从错误文本、任务名称、关键词或 evaluator 自定义 recovery 状态猜测标签。

## 结果

连续执行两次：

```text
python3 scripts/eval-m9b-fixed-pro-regression.py --observer-conformance
```

两次 report byte-identical：

- status：`pass`
- cases：`12/12`
- positive / negative：`6/6`
- result SHA-256：
  `09b840b8af0ee136d291a7ebf2203ebb05467fd24bc5adfa670dbacb093dd450`
- `historical_raw_read=false`
- `key_accessed=false`
- `network_accessed=false`

退役入口 `--campaign m13` 由 argparse 稳定拒绝；M9-C、M11、M12 self-test 全部通过。

## 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/codewhale-m14-target
```

通过：

- `cargo fmt --all -- --check`
- `./scripts/dev-codewhale.sh focused`
- protocol/runtime/tools 定向 test 与 check
- ToolOutcome commit/replay 与 Writer lifecycle SQLite reopen
- tool prepared / side-effect / outcome committed SIGKILL reopen
- Writer create side-effect SIGKILL reopen
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- CLI/TUI/API 与 root/read-only/Writer conformance
- `git diff --check`

Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3 未变化。

## 结论与非结论

保留离线 observer conformance，退役 M13 live acquisition。M14 只证明未来 evaluator
能精确区分 canonical facts；它不证明 CodeWhale 的 verified success、恢复率、Token、
时间或费用改善，也不能补算 M13。

新的 product-effect acquisition 只有在另立 immutable manifest、全新 identity 和
position 1 后才能开始。任何 observer、accounting、latest-revision evidence 或安全歧义
仍必须在下一 arm 前 fail closed；不得复用 M13 raw。
