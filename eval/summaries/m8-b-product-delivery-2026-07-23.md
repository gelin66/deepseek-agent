# M8-B CodeWhale 产品身份与本地交付结论

- 日期：2026-07-23
- 分支：`deepseek-agent`
- production baseline：`2ed3efe3ce56475549e2382f3d2c41ff39286e54`
- baseline tree：`7e5162411663de5ecb3232cdfe96bd00c6b918bc`
- manifest commit：`7e760e05`
- product identity cutover：`eddfd4bc`
- imported updater deletion：`ccc98245`
- CodeWhale state-path cutover：`d792113e`
- delivery owner cutover：`4aff11f6`
- offline toolchain fixes：`28f8a34c`、`e4232142`、`307f6c09`
- production candidate：`307f6c09d082d80c85daa43a5958b9e795c33d10`
- candidate tree：`247fc8c3499dfc1de5345c0c20e7a68443343d61`
- 协议身份：Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2
- 最终决策：
  `keep_single_codewhale_identity_and_delivery_owner / shrink_imported_delivery_paths`

## 1. 问题与切片合同

M8-A 已经让 CLI、TUI、app-server 只接受一个 DeepSeek 配置，但产品交付仍有多份互相冲突
的真相：

- package metadata、embedded build env、User-Agent 与 binary discovery 仍携带导入身份；
- 正式 binary set 还包含 `codew` compatibility executable；
- Doctor 查询 imported GitHub release，`crates/release` 同时混合 updater/CNB 与仍被使用的
  TLS helper；
- config/state/settings/secrets 会读取 `.deepseek` 或 DeepSeek-branded 产品 env；
- 没有一个可复现 package/install/upgrade/rollback/uninstall owner，CI 不运行 shipped
  artifact lifecycle，Rust 跟随 moving `stable`。

本切片合同是：

1. 真实问题：建立一个 CodeWhale identity、准确两项 binary set 和一个离线本地交付 owner。
2. 验收条件：clean source build、artifact identity/checksum、fresh install、upgrade、
   rollback、uninstall/data retention、macOS/Linux、offline 和 legacy rejection 全部可复现。
3. 单一 owner：product path 由 `crates/config` 拥有；artifact lifecycle 由
   `scripts/codewhale-delivery.sh` 拥有；CI 只调用同一脚本。
4. 替代旧路径：`codew`、DeepSeek-branded product env、`.deepseek` reader/migration、
   `crates/release`、Doctor updater、第二 metrics state 和 ad hoc install 说明。
5. 证据：D01-D12、targeted crates、real artifact、macOS/Linux lifecycle、focused、
   workspace Clippy/test、PTY、crash/reopen 和 source/dependency absence。
6. cutover 删除：不保留 compatibility binary、dual path、第二 updater/release owner 或
   自动联网 toolchain 安装。

## 2. 冻结矩阵

`eval/manifests/m8-b-product-delivery-v1.json` 固定 clean `2ed3efe3`、
`maximum_reruns=0`、外部 Cargo target、offline Cargo 和以下 12 项：

| ID | 范围 | 结果 |
|---|---|---|
| D01 | clean source build | exact Rust 1.97.0、Cargo.lock、locked/offline 通过 |
| D02 | artifact identity | version/target/revision/tree/lock/toolchain/two binaries 完整绑定 |
| D03 | checksum | archive corruption、inner tamper、wrong target 均在 activation 前拒绝 |
| D04 | fresh install | real artifact 两项 binary identity 正确，首次运行不创建用户状态 |
| D05 | upgrade | immutable release + atomic current/previous 通过 |
| D06 | rollback | 无重建、无网络恢复 previous 通过 |
| D07 | uninstall | program/delivery metadata 删除，用户 sentinel byte-identical |
| D08 | offline | real macOS lifecycle 在 OS 禁网下通过 |
| D09 | platform | macOS real artifact 与 Linux same-script fixture lifecycle 通过 |
| D10 | legacy | 旧 binary/env/config/state/release alias fail closed |
| D11 | runtime parity | v10/v16/v21/v2 与 canonical composition 不变 |
| D12 | physical cleanup | imported release/updater、重复 owner 和未接线部署资产删除 |

Harness 不复制 installer、checksum、config resolver、Runtime 或 Store；它直接运行 production
delivery script、installed binaries 与既有 canonical gates。

## 3. Production cutover

### S1 — 产品与 binary identity

`eddfd4bc`：

- workspace repository metadata 指向 owner origin
  `https://github.com/gelin66/deepseek-agent`；
- embedded build env 只接受 `CODEWHALE_BUILD_*`，TUI override 只接受
  `CODEWHALE_TUI_BIN`；
- DeepSeek transport User-Agent 为 `CodeWhale/<version>`；
- shipped binary set 只有 `codewhale`、`codewhale-tui`；
- `codew` target/shim 和 imported install hints 物理删除。

真正的 DeepSeek protocol env（例如 `DEEPSEEK_API_KEY`）没有改名。

### S2 — imported updater/release 删除

`ccc98245`：

- 仍有真实 caller 的 TLS helper 移到 TUI owner；
- Doctor 成为确定性本地诊断，不查询 latest release；
- 删除 `crates/release`、CNB/imported GitHub discovery 和 updater-only dependency；
- 已删除的 `codewhale update` 继续在任何配置、Store 或模型副作用前 fail closed。

### S3 — CodeWhale state namespace

`d792113e`：

- config/state/settings/secrets 只使用 `CODEWHALE_HOME`、
  `CODEWHALE_CONFIG_PATH` 与 `.codewhale`；
- `.deepseek` 不读取、不迁移、不回写、不由 uninstall 删除；
- OS keychain service 固定为 `codewhale`；
- 删除 CLI `metrics` 第二状态真相和 legacy path/env compatibility reader。

### S4 — 可复现本地 delivery

`4aff11f6` 建立 `scripts/codewhale-delivery.sh`：

- `package`、`install`、`verify`、`rollback`、`uninstall`、`host-target` 共用一个 owner；
- source package 只执行 `cargo build --release --locked --offline`；
- manifest 绑定完整 revision/tree、Cargo.lock SHA、rustc、target、source mode 和两项
  binary set；
- archive outer SHA-256 与四个 canonical inner file SHA-256 都先验证；
- install/upgrade 写入 immutable release dir，再原子切换 current/previous；
- uninstall 只删除程序和 delivery metadata，保留 `CODEWHALE_HOME`。

`28f8a34c`、`e4232142`、`307f6c09` 进一步要求只复用已安装、实际版本精确等于
1.97.0 的 rustup toolchain；不会因为 pinned channel 缺失而自动联网下载。CI 的
Ubuntu/macOS matrix 调用同一 lifecycle owner 并上传 checksum-bound artifact。

## 4. 真实 artifact 与 lifecycle

candidate `307f6c09` 在外部 target、Cargo offline 和 macOS network-denied sandbox 中构建：

| 字段 | 值 |
|---|---|
| artifact | `codewhale-0.8.68-aarch64-apple-darwin-307f6c09d082.tar.gz` |
| bytes | 17,660,358 |
| SHA-256 | `af3cae6ae254f0331162aa475ab2201b659dc9ef68264822ea5321d3c6c48115` |
| source revision | `307f6c09d082d80c85daa43a5958b9e795c33d10` |
| source tree | `247fc8c3499dfc1de5345c0c20e7a68443343d61` |
| Cargo.lock | `0699519a54e34db65457a56c143be8bfd89fdb96bdae7dde01a7a2540788f4ba` |
| rustc | `1.97.0 (2d8144b78 2026-07-07)` |
| `codewhale` | 15,695,008 bytes；SHA `b15556b7…0b410ce3` |
| `codewhale-tui` | 22,522,400 bytes；SHA `6dea47e9…d1e7e24` |

真实安装后的两个 `--version` 都报告 `0.8.68 (307f6c09d082)`；verify、Doctor 和 uninstall
通过，用户 sentinel 前后 byte-identical。fixture 的相同 identity/binaries 产生相同 archive
SHA；tamper 和 target mismatch 没有改变 active release。

平台证据分层：

- macOS aarch64：真实 source release build + real artifact lifecycle；
- Linux aarch64 GNU：缓存 `golang:1.26-bookworm`，`docker --network none`，同一
  fixture lifecycle 通过；
- Linux source release build：由 CI matrix 拥有，本机没有观察，因此不把它写成已执行。

Linux 前两次只读 container preflight 因临时文件系统 `noexec` 正确拒绝 fixture executable；
调整为 writable ephemeral container 后一次通过。这不是模型/task rerun，也没有持久产品
状态。

## 5. 复杂度与删除

从 `2ed3efe3` 到 `307f6c09`：

- 59 files changed；
- 1,574 insertions；
- 3,276 deletions；
- net -1,702 lines。

新增主体是唯一 delivery owner、lifecycle self-test 和 CI contract；删除量来自 updater/
release、第二 metrics state、legacy state path 与兼容 binary。没有第二 Runtime、Store、
tool catalog、model loop、Provider 或用户模式。

## 6. 结果与门禁

ignored result：

- path：`eval/results/m8-b-product-delivery-2ed3efe3-v1.json`；
- mode/size：`0600` / 8,474 bytes；
- SHA-256：`8c9f818674e1d739a707f538510e7db8c6c99e69c3de13a78fa75a2fecb065a4`；
- manifest SHA-256：`b960d2ba7620c560b82bda729385993e6a44b2554cc855990820a9d76e034239`；
- 12/12 cases passed；
- `credential_read=false`、`official_api_requests=0`；
- `material_model_treatment=false`、`product_metric_eligible=false`。

最终门禁：

- macOS delivery self-test（OS 禁网）：通过；
- Linux delivery self-test（Docker `--network none`）：通过；
- real source package/install/verify/Doctor/uninstall：通过；
- owning crate targeted tests/check：通过；
- `./scripts/dev-codewhale.sh focused`：通过；
- TUI hermetic two-run：每次 886 passed、1 ignored；
- canonical PTY 7/7、QA PTY 9/9、release runtime 5/5（1 heavy ignored）；
- exec acceptance 25/25、exec/HTTP/stdio parity 2/2；
- process SIGKILL/SQLite reopen、root/read-only/Writer conformance：通过；
- `cargo fmt --all -- --check`：通过；
- workspace strict Clippy：通过；
- `cargo test --workspace --locked`：通过。

所有 Cargo gate 使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8b-target`、`CARGO_NET_OFFLINE=true` 和已安装
stable 1.97.0。

## 7. 网络与 credential 边界

M8-B 没有 DeepSeek request、response、model capability、reasoning、prompt、tool catalog、
budget 或 accounting treatment。付费请求不能为本地 identity/delivery 增加归因力，所以
没有读取 `key.txt`，官方 DeepSeek API 请求为 0。

冻结的 real delivery 命令在 `sandbox-exec` 禁网下通过，Linux container 使用
`--network none`。但 candidate 后的一次只读 binary-list gate 漏设
`RUSTUP_TOOLCHAIN=stable`，rustup 尝试同步/下载 pinned 1.97.0 channel；命令被立即中断，
没有 Key、DeepSeek request、package/install/state/result 或 repo mutation。结果明确披露
这一点，因此这里只声明 delivery path 的 no-network 可复现性，不声称整个 Agent session
从未尝试外网。

本切片没有改变易变 DeepSeek protocol 事实，也没有重新声明 model limits、Beta behavior
或价格；M8-A 复核的官方一手资料和 `crates/deepseek` fixtures 继续有效，无需为了本地
delivery 再制造一次 web/API 依赖。

## 8. 产品决策

决策是：

**保留唯一 CodeWhale identity 与本地 delivery owner；收缩/删除 imported delivery 路径。**

理由：

- artifact 身份覆盖 source、lock、toolchain、target 和准确 binary set；
- checksum、immutable release 和 atomic activation 让失败在切换前关闭；
- upgrade/rollback/uninstall 不需要 release metadata 或网络，用户数据有明确保留边界；
- macOS real artifact 和 Linux same-script lifecycle 都有行为证据；
- 代码净删除 1,702 行，没有增加 Runtime/Store/协议或产品模式。

## 9. 非结论

M8-B 不证明：

- verified coding success、Token、wall time 或 API cost 得到提升；
- Linux source release binary 已在本机生成或执行；该职责目前由 CI matrix 拥有；
- GitHub release 已发布或 remote CI 已运行；本 Goal 按边界没有 push/release；
- fixed `zh-Hans` CLI/TUI/Headless 产品界面或中文 Agent prompt A/B 已完成；
- MCP、RepoGraph、多 Writer、通用 DAG 或新模型选择系统应准入。

## 10. 下一切片

建议下一阶段为 M8-C，单独建立 fixed `zh-Hans` 产品界面：

- 审计 CLI、TUI、Headless/Doctor、错误恢复和多 Agent 状态的真实 user-facing caller；
- 冻结中文端到端、英文泄漏、CJK 宽度/窄终端、raw 原样保留和 machine schema 稳定矩阵；
- 只翻译 Host 生成且确认保留的产品文本，保持 JSON/NDJSON/API 字段、命令参数、工具名、
  model ID、路径、代码和原始 provider/tool output 稳定；
- 删除重复或不可达文案 owner，不改变 Runtime/Store/协议；
- 中文 Agent prompt A/B 作为后续独立 model-treatment slice，不与产品界面翻译混合。

建议 Goal objective：

> M8-C：以 M8-B 已完成的 CodeWhale V1 身份与可复现本地交付链、唯一
> AgentApplication -> AgentRuntime -> RunStore、Run API v10 / RuntimeEvent v16 /
> State v21 / exec-stream v2 为基线，建立固定 zh-Hans 的 CLI、TUI、
> Headless/Doctor/错误恢复与多 Agent 状态产品界面：先只读审计所有真实 user-facing caller
> 与英文泄漏，冻结中文端到端、机器协议不变、CJK 宽度/窄终端、raw 原样保留和安装包身份
> 矩阵；只翻译 Host 生成且已保留的产品文本，保持 JSON/NDJSON/API 字段、命令参数、工具名、
> 模型 ID、路径、代码与原始 provider/tool 输出稳定，删除重复/不可达文案 owner，不改变
> canonical Runtime/Store/协议，不混入 Agent prompt A/B、MCP、RepoGraph、多 Writer、
> 通用 DAG 或模型选择；完成 offline gates、真实安装包回归、权威文档、分离提交和精确清理，
> 以任务可理解性、错误恢复成功率、英文泄漏率、CJK 布局回归和代码复杂度决定 keep、
> shrink 或 delete。
