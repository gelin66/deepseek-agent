# M17-A DSE 产品、binary 与 Cargo 身份硬切换

- 日期：2026-07-25
- 分支：`deepseek-agent`
- DSA 计划记录：`83d057751df7bb266d45f66bf1b88d7bdd7bede0`
- DSE 决策：`9fb100e84c1cb118dc5d15f86a9cb8e62a5784af`
- production cutover：`89f1bb9f5b422a58786ad34424399e43342211d8`
- candidate tree：`2754a79115182677869e0669d1f2587a90624939`
- 协议身份：Run API v12、RuntimeEvent v18、State schema v24、exec-stream v3
- 决策：`keep_dse_product_cargo_identity / continue_m17_b`

## 1. 问题与边界

公开前计划使用的 `DSA = DeepSeek Agent` 与 DeepSeek 官方已经公开使用的
`DSA = DeepSeek Sparse Attention` 冲突。ADR-0009 因此在任何 DSA production commit、
发布或远端改名发生前改为：

```text
DSE = DeepSeek Engineer
```

本切片只处理当前活动产品、binary、Cargo/import、User-Agent 和模型可见名称。它不改变
DeepSeek Chat transport、fixed actor route、工具目录、AgentRuntime、RunStore、完成权威、
配置/状态/协议 namespace 或 delivery lifecycle。后两组分别由 M17-B、M17-C 显式升版和
删除旧路径。

切片合同：

1. **真实问题**：`codewhale` binary、16 个 `codewhale-*` package、Rust import、帮助/
   版本、User-Agent 和 production constitution 仍产生旧活动身份；
2. **验收**：Cargo metadata 只有 `dse-*`；binary set 只有 `dse`、`dse-tui`；CLI/TUI
   help/version 无旧活动命令或 DSA；root/read-only/Writer、pending Start、SIGKILL/reopen、
   exact replay 和 accounting 不回归；
3. **唯一 owner**：workspace manifest 与 `crates/cli` 产品入口；
4. **旧路径**：`codewhale`、`codewhale-tui`、active `codewhale-*`/`codewhale_*`、
   `CodeWhale/<version>` User-Agent 和 `CodeWhale` prompt identity；
5. **证据**：同一 committed revision 的 Cargo identity、binary/hash、prompt provenance、
   focused、严格 Clippy 与全 workspace tests；
6. **cutover 删除**：旧 binary target、active package/import、旧帮助标题、旧 User-Agent
   与旧模型可见产品名；不改 frozen evidence。

## 2. DSE identity 结果

Cargo metadata 的 package set 精确为：

```text
dse-app
dse-app-server
dse-build-support
dse-cli
dse-config
dse-context
dse-deepseek
dse-execpolicy
dse-localization
dse-orchestrator
dse-protocol
dse-runtime
dse-secrets
dse-state
dse-tools
dse-tui
```

binary target 精确为：

```text
dse
dse-tui
```

candidate binary identity：

- `dse 0.8.68 (89f1bb9f5b42)`：
  `8a133fba70fd529d9dadb1d4367c459fccc3878c0adb38ee12189619c620fadb`
- `dse-tui 0.8.68 (89f1bb9f5b42)`：
  `e180f45e3dd8c5b7d55f4e93425380e04be8d7a3049f4493066ad2ff27db785e`
- `Cargo.lock`：
  `045bd5412e594139014add8d3b0a2968b42b96717c6d2d94cae5531f008d667e`

official DeepSeek sender 使用唯一 `DSE/<crate-version>` User-Agent；离线 loopback 测试断言
实际 request header。CLI/TUI help 不产生 `CodeWhale`、旧命令、`DSA` 或 `dsa-*`。

## 3. 模型可见 identity

production constitution 只做名称替换：

```text
## CodeWhale                         ## DSE
你是 CodeWhale，一个……       ->     你是 DSE，一个……
```

执行、权限、工具、验证、完成、多 Agent 和语言条款的内容与顺序不变。冻结 identity：

- constitution raw SHA-256：
  `1cdea5d9db04ebffc4e9e13bdc9685830adbb9ab75e93c1377bdaccdd87a7232`
- normalized stable first block：
  `4bc8bafe6fa99d5da2753c8362142f460d78582c5219543e4ba925590fc2580b`
- normalized whole production prompt：
  `7a10883783669fcb86676de79f86f871f0d6135a8c31c3e4c3600a0096ae1c5f`

这只是 M17-F 中英文 2×2 的共同品牌基线，不是 prompt 质量提升结论。

## 4. 历史事实与 allowlist

没有改写 frozen manifest、raw、summary、canonical JSON fixture、schema hash 或历史标题。
`DeepSeek Agent` 仍是通用产品类别和 M7-A/M7-A2 frozen 评测标题；`83d05775` 的 DSA
计划仍保留在 Git 历史。当前源码中的旧小写身份只允许属于：

- M17-B 尚待切换的 `.codewhale`、`CODEWHALE_*`、`codewhale.exec-stream` 和
  `<codewhale:runtime_event>`；
- frozen canonical JSON/exec-stream fixture；
- 明确拒绝已删除 crate、命令、部署资产或 compatibility path 的负向测试；
- MIT provenance、历史说明与旧 checkpoint。

当前 package、binary、import、help、User-Agent 和模型可见身份没有 DSA 或旧活动产品名。

## 5. 门禁

所有 Cargo 命令使用：

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/dse-m17-target
```

通过：

- `./scripts/dev-codewhale.sh focused`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo metadata --no-deps --locked`
- root/read-only child/Writer conformance
- pending Start、SQLite reopen、app-server SIGKILL/replay
- exec/HTTP/stdio 与 TUI/PTY parity
- prompt provenance 与 User-Agent loopback
- `git diff --check`

第一次 full workspace run 的 macOS Seatbelt 真实 `touch` 允许路径测试出现一次不可复现失败；
同一测试定向重跑通过，未修改测试或 production；随后完整 workspace run 全绿。该现象不
被计为产品收益，也没有用选择性结果替代最终全量门禁。

本切片未读取 Key、未调用官方 DeepSeek API、未访问外部网络、未产生费用，也未执行远端
改名、push、public visibility、tag 或 release。

## 6. 非结论与下一切片

M17-A 不证明：

- config、Secret、State、protocol/media type 已切到 DSE；
- delivery/CI、artifact、install/rollback/uninstall 已切到 DSE；
- `en`/`zh-Hans` localization、prompt 语言胜者或公开发布门禁已经完成；
- branding 提高 verified coding success、Token、时间或费用。

下一切片是 M17-B：由现有 config/state/protocol owner 完成 `~/.dse`、`DSE_*`、
`dse.*`、`application/vnd.dse.*` 的显式版本切换、一次性本地迁移、exact replay 与旧
reader 删除，不保留 dual write 或永久 compatibility。
