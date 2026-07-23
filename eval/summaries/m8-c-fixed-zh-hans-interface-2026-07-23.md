# M8-C 固定 zh-Hans 产品界面结论

- 日期：2026-07-23
- 分支：`deepseek-agent`
- production baseline：`e99bf6c7cf19608a4c0b772f98f6e3023560c68b`
- baseline tree：`e1b768c84f9ac37f39f83be267d41aedf5ebd9d8`
- manifest commit：`7689dd42`
- shared localization owner：`062a747d`
- retained help contract：`af8b7c3e`
- CLI contract fix：`413d4ae1`
- request-user-input/CJK cutover：`6d7ebe74`
- Clap value-label fix：`23e33980`
- Doctor/Headless cutover：`44b17940`
- production candidate：`44b17940846ceb5f946337623e7554768ee2e75e`
- candidate tree：`f44440985c27640f7388c0edc76768841851aa66`
- 协议身份：Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2
- 最终决策：
  `keep_single_fixed_zh_hans_owner / shrink_duplicate_product_text_paths`

## 1. 问题与切片合同

M8-B 已固定 CodeWhale 身份与两项 binary delivery，但真实产品界面仍有三类问题：

- 唯一 `zh-Hans` catalog 和 `MessageId` owner 私有于 TUI，CLI/Doctor/Headless 继续直接拥有
  英文 Host 文案；
- CLI、TUI、Doctor、错误恢复、`request_user_input` 和多 Agent 状态的翻译边界不一致，
  容易误改 machine schema 或 raw provider/tool 内容；
- 已有 CJK 单元覆盖没有冻结为 foreign locale、80/120 列和真实安装包的产品门禁。

本切片在写 production 代码前冻结三个纵向合同：

1. S1 由一个共享 compile-time message owner 替代 TUI 私有 owner，CLI/TUI 使用同一 catalog。
2. S2 只翻译保留的 Host chrome；Doctor JSON、NDJSON、HTTP/SSE/stdio、稳定 code/enum 和
   raw provider/tool/stdout/stderr 保持原样。
3. S3 用 L01-L14、真实 parser/renderer/PTY/Runtime/Store 和安装包验证固定中文、CJK 布局、
   raw 边界与身份；Harness 不复制产品实现。

不增加 locale 状态、语言选择、翻译模型请求、第二 Runtime/Store、第二 catalog、Provider、
模式系统或兼容层。

## 2. Production cutover

`062a747d` 新建 `crates/localization`，把 TUI 私有 catalog 与 `MessageId/tr` 移为 CLI/TUI
共享 owner；`scripts/test-fixed-zh-hans.sh` fail closed 检查：

- 恰好一个 `zh-Hans.json`；
- 恰好一个 `rust-i18n` Cargo owner；
- 旧 `crates/tui/src/localization.rs` 与 TUI catalog 不得残留；
- message owner 不得出现 locale 环境检测、切换或第二语言状态。

`af8b7c3e`、`413d4ae1`、`23e33980` 让真实 Clap parser 在 `LANG=C/LC_ALL=C` 下输出固定
中文帮助，同时保持命令、flags、布尔值和其他稳定身份不变。Clap 自动生成的英文
`possible values` 也被一个中文、值身份不变的 retained help contract 替代。

`6d7ebe74` 让 `request_user_input` 的 Host 标题、进度、快速选择、自由输入、确认和校验
使用共享 catalog；模型提供的问题、选项、description 和 canonical answer 仍保持 raw。
同时修复 multi-select 确认行索引，使其绑定真实最后一个 selectable row；测试 renderer
跳过双宽 CJK 的 continuation cell，80/120 列不再产生伪重复字符。

`44b17940` 收敛 Doctor 的 setup、MCP、Skills、Plugins、dependency、terminal 与 recovery
文案，并让直接 `codewhale-tui` 的 error chain 使用中文 Host 前缀。路径、命令、URL、
parser/provider detail 和 Doctor JSON 字段/值不被后处理翻译。

`crates/protocol`、`crates/runtime`、`crates/state`、`crates/app-server` 相对 baseline 的
Git diff 为 0；canonical execution 仍只有：

```text
codewhale / codewhale-tui
  -> AgentApplication
  -> AgentRuntime
  -> RunStore
```

## 3. 冻结矩阵与可理解性

`eval/manifests/m8-c-fixed-zh-hans-interface-v1.json` 固定 clean baseline、外部 Cargo
target、offline Cargo、`maximum_reruns=0` 和 L01-L14。14/14 均通过：

- foreign locale 下 CLI、TUI、exec、app-server help 与 Doctor human report 使用中文；
- missing Key、invalid config、retired command、Headless startup 和 typed recovery 提供
  中文 Host 摘要，raw detail/secret redaction 保持；
- Doctor JSON、exec stream-json、HTTP/SSE/stdio 和 stored event JSON 保持 machine contract；
- root、read-only child、explicit Writer 使用同一状态投影和 crash/reopen conformance；
- 80/120 列 help 与 `request_user_input` frame、真实中文 PTY、cursor/hit target 均通过；
- 冻结 representative outputs 的非白名单英文产品文案泄漏为 0。

英文白名单只包含命令/flag/enum、API/HTTP/SSE/stdio/TUI/MCP/Skills/Plugins、模型/工具 ID、
路径、代码、diff、canonical machine field/status，以及 raw provider/parser/tool/stdout/stderr。
这不是全仓 ASCII 扫描，也不把模型生成英文算成 Host 产品泄漏。

## 4. 真实安装包

clean candidate `44b17940` 使用 Rust 1.97.0、Cargo `--locked --offline` 和外部 target 生成：

| 字段 | 值 |
|---|---|
| artifact | `codewhale-0.8.68-aarch64-apple-darwin-44b17940846c.tar.gz` |
| bytes | 17,733,507 |
| SHA-256 | `0578404370cf07af86d73e2d8f6a5c15cefa62e888319fbc6382b705678b660c` |
| source revision | `44b17940846ceb5f946337623e7554768ee2e75e` |
| source tree | `f44440985c27640f7388c0edc76768841851aa66` |
| Cargo.lock | `be75e11ee7b1a879908cd5a4ae8a615a45d965d63822115c8de2db7e1294eeac` |
| `codewhale` | 15,827,216 bytes；SHA `9d50320e…e55220c` |
| `codewhale-tui` | 22,588,464 bytes；SHA `e9f8554f…e1efb4b` |

真实安装、verify、两个 `--version`、CLI/TUI/app-server help、Doctor text/JSON、missing Key、
retired command 和 uninstall 均通过。`LANG=C/LC_ALL=C` 不改变中文界面；manifest 的完整
revision/tree 与二进制 embedded identity 一致；uninstall 删除程序链接和 delivery metadata，
保留隔离的 `CODEWHALE_HOME`。

## 5. 复杂度与删除

从 `e99bf6c7` 到 `44b17940`：

- 31 files changed；
- 2,741 insertions；
- 963 deletions；
- net +1,778 lines；
- catalog 从 TUI 私有 167 keys 收敛为共享 427 keys。

正增量主要来自一个 exhaustive `MessageId` owner、共享 catalog、真实 help/Doctor/
`request_user_input` contract 和回归测试。它不是能力提升指标；保留理由仅是 14 个冻结
产品反例都需要这些文案和边界。旧 TUI localization wrapper/catalog 已物理删除，没有第二
Runtime、Store、协议、model loop、用户模式或 translation call。

## 6. 门禁与结果

最终通过：

- shared localization/CLI/TUI targeted tests 与 fixed-zh-Hans static gate；
- `./scripts/dev-codewhale.sh focused`；
- `cargo fmt --all -- --check`；
- workspace all-targets Clippy `-D warnings`；
- `cargo test --workspace --locked`；
- TUI hermetic full suite 两次：每次 889 passed、1 ignored；
- canonical PTY 7/7、QA PTY 9/9、release runtime 5/5（1 heavy ignored）；
- exec acceptance 25/25、run-surface parity 2/2、app-server 23/23；
- State process crash windows 38/38、RunStore reopen 33/33；
- root/read-only/Writer conformance、delivery self-test 与真实 artifact lifecycle。

所有 Cargo 命令使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8c-target`、`CARGO_NET_OFFLINE=true` 和
Rust 1.97.0。

ignored result：

- path：`eval/results/m8-c-fixed-zh-hans-interface-e99bf6c7-v1.json`；
- mode/size：`0600` / 8,986 bytes；
- SHA-256：`a07596ffb0d7ca18398fa0a90a53946199712fa4ad6ffe9bc02ff08091df7ee2`；
- manifest SHA-256：`d92792d8e5de7b869c0def6a003a3421f74edf85fd8e3c8539e0cdeaf4ad48e9`；
- 14/14 cases passed；
- `credential_read=false`、`official_api_requests=0`；
- `material_model_treatment=false`、`product_metric_eligible=false`。

## 7. 产品决策

决策是：

**保留唯一 fixed zh-Hans owner；收缩/删除重复产品文案路径。**

理由：

- CLI/TUI/Doctor/Headless 使用同一 compile-time message truth；
- 固定中文不依赖 locale，也没有第二语言包或翻译请求；
- raw 与 machine contract 的非翻译边界由 paired text/JSON、protocol parity 和 sentinel
  fixtures 证明；
- CJK、错误恢复、多 Agent、crash/reopen 和 installed binary 都有 production-path 证据；
- TUI 私有 catalog/wrapper 和覆盖到的 direct English owner 已删除。

## 8. 非结论

M8-C 不证明：

- verified coding success、Token、wall time 或 API cost 得到提升；
- Agent system prompt、模型 reasoning/output 或工具策略已中文化；
- 技术身份、machine enum 或 raw 外部内容应该翻译；
- MCP、RepoGraph、多 Writer、通用 DAG 或模型选择系统应准入。

本切片没有 material model treatment，付费请求不能增加归因力，因此未读取
`/Users/gelin/Desktop/codewhale/key.txt`，官方 DeepSeek API 请求为 0。

## 9. 下一切片

下一阶段建议为 M8-D，独立评测版本化中文原生 Agent prompt；历史 v1-v3 已因 multi
可靠性或计量门槛被拒绝，不能直接恢复为新候选。

建议 Goal objective：

> M8-D：以 M8-C 已完成的固定 zh-Hans 产品界面、唯一 AgentApplication ->
> AgentRuntime -> RunStore、Run API v10 / RuntimeEvent v16 / State v21 /
> exec-stream v2 和可复现两二进制交付身份为基线，建立版本化、可回滚的中文原生
> DeepSeek Agent prompt 候选与同任务可归因 A/B：先只读审计当前 production prompt、
> 历史被拒绝候选、canonical context projection 和 root/read-only child/显式 Writer
> 真实 caller，冻结同 revision、同 immutable binary、同任务/模型/预算/缓存前缀、
> maximum_reruns=0、计费与 verifier 身份；只改变 DeepSeek 可见的 system prompt 语言和
> 必要的中文任务表达，不改 Host UI、工具 schema、Runtime/RunStore、协议、模型路由、
> 权限、请求预算或 accounting；先离线证明 reasoning/tool replay、raw、crash/reopen、
> evidence completion 和 machine surface 不变，仅在存在真实 prompt surface delta、
> 费用可证明且无 unknown billing 时读取 Key，并按 single、read-only multi 和显式 Writer
> 分层执行每个 variant × task/lane 至少 3 次的正式 A/B，以 verified success、
> false success、首次工具调用正确率、失败恢复率、模型请求、input/output/cache Token、
> wall time、费用和代码复杂度决定 keep、shrink、hold 或 reject；无净收益则删除候选
> production prompt 分支，完成权威文档、分离提交与精确清理。
