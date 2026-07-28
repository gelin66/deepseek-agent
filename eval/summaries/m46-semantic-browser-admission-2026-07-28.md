# M46 read-only semantic browser admission audit

- 日期：2026-07-28
- clean baseline：`997c67e20eb67b663eaf424e334b3ee80e914bee`
- 决定：`admit_next_goal_read_only_semantic_browser_w2_contract_only`
- production Rust / Cargo / browser tool delta：`0 / 0 / 0`
- DeepSeek Key / official request / actual cost：`not read / 0 / $0`
- product metric：`product_metric_eligible=false`

## 问题、owner 与旧路

M45-A 已能启动 worktree-local HTTP app、等待 health、读取 bounded status/body/log、绑定最新
workspace revision 并 teardown/reopen，但这不证明 current `web_fetch + ApplicationProbe` 能观察
JavaScript 执行后才存在的 DOM/accessibility state。M46 在至少两个独立任务出现同一
`javascript_rendering` 或 `application_visibility` loss 前不得实现。

本 audit 的 owner 是现有 Evaluation authority；control 直接使用当前
`crates/tools::ProductionToolExecutor`，没有复制 fetch/probe 逻辑。它替代凭 feature list、竞品
存在或单个演示直接准入 browser 的旧推断方式。Playwright 只作为 external evaluator oracle，
不是 production dependency、sidecar、Agent loop 或用户能力。

## 预注册合同

执行前冻结的
[`m46-semantic-browser-admission-v1.json`](../manifests/m46-semantic-browser-admission-v1.json)
包含：

- baseline、两个 exact task id 与不同 independence key；
- 同一 canonical `tools:application_visibility`；
- 每个任务的 title、role、accessible name、state attribute/value；
- fixture 与 server SHA-256；
- same-loss threshold `2`，mixed loss 不可拼接，false-success 必须为 `0`；
- official requests `0`、credential read `false`、external network `false`；
- W2 若准入也只能进入下一独立 Goal，本 audit production implementation=false。

evaluator self-test 为 `5/5`：credential read、duplicate task、rendered literal 出现在 raw body、
mixed loss code 与 false-success 五种 false allow 均被拒绝。

## 任务与实际观察

两份 fixture 都启动真实 Python loopback HTTP application。raw HTML 只包含编码后的字符值；
预注册 accessible name 不以 literal 出现在 response 中。`web_fetch` 通过受控 public HTTPS
network seam 运行 exact production extraction，ApplicationProbe 则真实 spawn、health、GET、lease
identity 和 teardown。oracle 使用 Playwright `1.61.0`、Node `v24.18.0`、Chrome
`150.0.7871.187`，page route 只允许 exact Host-assigned loopback origin；输出最多一个 node、
4,096 bytes，截图为 0。

| task | oracle bounded DOM/accessibility | `web_fetch` | `ApplicationProbe` | false success |
|---|---|---|---|---:|
| `m46_js_status_hydration` | `status / Deployment ready / data-state=ready` | rendered name absent | healthy；`application_probe_body_mismatch`；failed verdict；exact latest revision；teardown settled | 0 |
| `m46_js_switch_state` | `switch / Automatic retries enabled / aria-checked=true` | rendered name absent | healthy；`application_probe_body_mismatch`；failed verdict；exact latest revision；teardown settled | 0 |

聚合：oracle `2/2`、current control verified `0/2`、同一
`tools:application_visibility=2/2`、control false-success `0`；两项 failed observation 的 known
workspace revision SHA 都与对应 `ToolOutcome` 精确一致。两次 final-source result byte-identical：

```text
sha256:1b03f8047d032f35654f6481f506de7390393df0ff0a10c268305d2955fa2939
```

## Evaluator correction

第一次 evaluator implementation attempt 在第一个 ApplicationProbe control 后停止。production
已经正确返回 `application_probe_body_mismatch`；错误在 test-only observer 把 existing typed
failed verifier observation 预期为 `None`。M45-A 合同本来就会保留
`VerifierVerdict::Failed` 供同一 Agent rework，它不能生成成功 receipt。

修正为 exact failed verdict 后，完整矩阵闭合；第一次停止没有运行 browser oracle、没有形成
准入结果，也不计入 product loss。最终 revision 又做一次 credential-free replay，与正式成功
结果 byte-identical。没有 DeepSeek/model 请求或付费 rerun。

## 决定与下一合同

repeated-loss gate 通过，所以只准入下一独立 W2 Goal：

```text
crates/tools
  -> eval-only Rust CDP lifecycle/dependency spike
  -> pinned Chrome for Testing identity + SHA-256
  -> enforceable public or exact-local origin egress guard
  -> browser_navigate
  -> bounded DOM/accessibility snapshot
  -> Host-owned teardown
```

action、search、screenshot、vision、登录/user profile、Cookie/storage、Node/Playwright production
sidecar、第二 Runtime/Store/session/accounting ledger 继续禁止。current 用户仍不能让 production
Agent 浏览 JS 页面；变化只是 browser implementation 已从“无证据禁止”进入“下一 Goal 可做窄
read-only W2”。

## 身份与验证

```text
manifest sha256  0975824c9f288c689daa9c8e3986a167f8fd0f48fcb242f1f685b6fa80914b10
evaluator sha256 baec48a6ea6e6d7508bf865fb96c10bc1dd98734bd2f510f38668c0ea4e2bac8
oracle sha256    9f24021003c132efc939989e6b1b5d56e3d7215065ef301c0875ac735421989f
control sha256   3e0060be4faf31cd21248b945b8c38a3d4055d6952b81d4eb1d8a385ee9b7fa3
server sha256    f11185ab32b5e46a739a9cab692aa7870d1613fdf421dec60ba3fbc4a3c7fbd7
task A sha256    624bc1fda0117630c945cde514708cb96517776daeec90cda5c62666870167fe
task B sha256    ee58f71dcde2832e79ef7779be4901e4315e05dc5ba183eed01dcf289fbf56f9
```

已完成的 evidence：manifest validate、5-case evaluator self-test、Rust control compile preflight、
正式矩阵、final-source byte replay、authority 23/23（tools route 2,131 / ceiling 4,409）、
`dse-tools --tests` check、M46 test strict Clippy、fmt 与 diff check。canonical focused gate 全绿：
`dse-tools` 376/0/2 ignored + integration 1/1、DeepSeek 61/0/1 ignored、Runtime 88/88、app
64/0/3 ignored、app-server 23/23、exec 30/30、TUI run 20/20、PTY 7/7。production delta=0，
所以按 Risk-tier 与 Goal 边界未运行 full。
