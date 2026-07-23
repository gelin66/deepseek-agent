# M8-A DeepSeek-only production 配置与入口结论

- 日期：2026-07-23
- 分支：`deepseek-agent`
- production baseline：`8b65035696842ad67cac32f114820746ca64128b`
- baseline tree：`b0b57dc6385ea0fa57034224ddc3db9a9426ff24`
- CLI cutover：`e1a611ff`
- TUI cutover：`63246e72`
- config cutover：`00dcda0c`
- final validation：`48fe2ee0`
- final validation tree：`6ab95c2a572e8a49e4c1fba0108fd7f3fc2a3d21`
- 协议身份：Run API v10、RuntimeEvent v16、State schema v21、exec-stream v2
- 最终决策：`keep_deepseek_only_cutover / delete_generic_provider_paths`

## 1. 问题与切片合同

M7-I 关闭 request/Token 调优后，canonical Agent execution 已经只有
`AgentApplication -> AgentRuntime -> RunStore`，但入口仍携带两套不真实的产品语义：

- CLI 解析 34 个 Provider、generic auth/OAuth 和跨 Provider model registry；
- TUI 拥有另一套 Provider identity、catalog、pricing、route、OAuth、Doctor/onboarding；
- `crates/config` 仍序列化 Provider tables、fallback、models.dev、pricing 和 route resolver。

这些路径不能执行非 DeepSeek Agent，却持续增加首次启动、credential precedence、恢复和
帮助文案的分叉。

本切片合同是：

1. 真实问题：让三个保留入口使用一个 DeepSeek credential/endpoint/model owner，并删除
   无 production consumer 的 generic Provider 路径。
2. 验收条件：首启、无 Key、外来 Provider/模型、credential precedence、endpoint/TLS、
   resume/reopen、Doctor/onboarding、PTY、exec/HTTP/stdio 和 actor conformance 全部离线
   可复现。
3. 单一 owner：`crates/config` 解析根级 DeepSeek 配置；`crates/deepseek` 继续拥有 capability、
   planner、transport 和 accounting；应用执行仍由 `crates/app/runtime/state` 组成。
4. 替代旧路径：CLI/TUI/config 的 Provider selector、model-provider OAuth、catalog、pricing、
   alias、fallback 和 route resolver。
5. 证据：冻结 P01-P12、targeted crate tests/check、真实 PTY、loopback production path、
   SQLite reopen、surface parity、workspace Clippy/test 和源码/依赖 grep。
6. cutover 删除：不保留 compatibility reader、dual write、Provider mode、第二 model catalog
   或第二 route owner。

## 2. 冻结矩阵

`eval/manifests/m8-a-deepseek-only-entry-v1.json` 固定 clean `8b650356`、
`maximum_reruns=0`、外部 Cargo target、offline Cargo 和以下 12 项：

| ID | 范围 | 预期 |
|---|---|---|
| P01 | 隔离首启 | 只生成 DeepSeek-only template，不联网 |
| P02 | 无 Key | CLI/TUI/app-server 在请求前给出一致中文恢复 |
| P03 | 外来 Provider | flag/env/config 在 spawn、Store、network 前拒绝 |
| P04 | 外来模型 | 请求前拒绝，同时允许受约束的未来 `deepseek-*` |
| P05 | credential | CLI -> config -> keyring -> env，秘密不展示 |
| P06 | endpoint/TLS | official 默认；显式 loopback 只用于 fixture |
| P07 | resume/reopen | SQLite truth 不变，不重复请求或写入 |
| P08 | onboarding | DeepSeek-only 中文 Key/trust flow，无 Provider picker |
| P09 | Doctor | 只报告 DeepSeek route/auth/model |
| P10 | machine parity | Run API/RuntimeEvent/State/exec-stream 不变 |
| P11 | actor | root/read-only/Writer 共用 backend/Runtime/Store |
| P12 | source | generic Provider/OAuth/catalog/pricing/route 物理删除 |

Harness 只由现有 Rust/PTY/loopback gates 投影 canonical facts；没有第二配置 resolver、模型
循环、Store 或 accounting 实现。

## 3. 三个 production cutover

### S1 — CLI

`e1a611ff`：

- `auth set/status/clear`、model/config 和 runtime overrides 只接受 DeepSeek；
- credential precedence 固定为 explicit CLI、root config、CodeWhale keyring、
  `DEEPSEEK_API_KEY`；
- 非 DeepSeek flag/config 在 TUI spawn、RunStore 和网络前 fail closed；
- 删除 `ProviderArg`、generic Provider auth/OAuth/model registry 和整个 `crates/agent`。

### S2 — TUI

`63246e72`：

- interactive、exec、Doctor、onboarding 和 Fleet argv 使用同一 DeepSeek identity；
- config/model mismatch 在 terminal setup 前拒绝；
- 删除 `ApiProvider`、provider-specific OAuth、model catalog/provider lake、
  route runtime/billing/scorecard 和私有 capability route；
- 保留的 pricing 只展示 canonical DeepSeek usage/cost，不选择模型或 route。

### S3 — config

`00dcda0c`：

- 唯一模型配置变为根级 `api_key`、`base_url`、`default_text_model`；
- project config、named profile 和 Fleet profile 不能改变 Provider 或模型权威；
- 旧 `provider = "deepseek"`、`[providers.deepseek]`、camel-case keys 同样拒绝；
- 删除 `ProviderKind`、Provider tables、catalog、models.dev、model reference、generic
  pricing、fallback 和整棵 route resolver。

没有旧 schema reader 或双写。`[search].provider` 只选择 retrieval adapter；MCP OAuth
只认证 MCP transport。

## 4. 最终 gate 发现与闭合

最终 workspace gate 额外发现两个不应被隐藏的问题：

1. raw SSE fixture 在全套负载下可能尚未 ready 就启动 exec，使目标 `stream_stall` 退化为
   普通 network error。`48fe2ee0` 要求本机 `/v1/models` readiness round-trip 成功后才启动
   子进程；精确反例和完整 workspace 都通过。
2. 5 个 release PTY scenarios 仍注入已经删除的 `CODEWHALE_PROVIDER=deepseek`。同一提交
   删除 7 处注入；retained release suite 为 5 passed、1 heavy ignored。

同一提交还让 tracked `config.example.toml` 同时经过 `crates/config::ConfigToml` 和交互
TUI loader，并删除一个 strict Clippy 识别出的重复 `must_use` 属性。没有 production 行为
新增。

## 5. 复杂度与删除

从 `8b650356` 到 production cutover `00dcda0c`：

- 71 files changed；
- 3,689 insertions；
- 42,712 deletions；
- net -39,023 lines。

加入最终 gate tests 后，`48fe2ee0` 相对 baseline 为 72 files、`+3,732/-42,713`，
net -38,981 lines。新增代码由单 owner 配置、明确失败边界和可复现测试解释；没有第二
Provider、Runtime、Store、model loop 或用户模式。

## 6. 离线结果与门禁

ignored result：

- path：`eval/results/m8-a-deepseek-only-entry-8b650356-v1.json`；
- mode/size：`0600` / 5,513 bytes；
- SHA-256：`13a66e0a0173a2620b47b01c7a0739182c12ca0e05ec6991d772f0ba06e590c6`；
- manifest SHA-256：`68d099798ae0c7c61b86e89de6fe8a0aaeccae994ec112c907ec9e2723b22dd2`；
- 12/12 matrix cases passed；
- `credential_read=false`、`official_api_requests=0`；
- `external_network_accessed=false`、loopback only；
- `material_model_treatment=false`、`product_metric_eligible=false`。

最终门禁：

- `./scripts/dev-deepseek-agent.sh focused`：通过；
- owning config：76 passed；
- TUI unit：886 passed、1 ignored；
- canonical PTY：7/7；
- QA PTY：9/9；
- release PTY：5 passed、1 heavy ignored；
- exec acceptance：25/25；
- exec/HTTP/stdio parity：2/2；
- process SIGKILL/SQLite reopen：通过；
- root/read-only/Writer conformance：通过；
- `cargo fmt --all -- --check`：通过；
- workspace strict Clippy：通过；
- `cargo test --workspace --locked`：通过；
- `git diff --check`：通过。

所有 Cargo gate 使用 `CARGO_INCREMENTAL=0`、
`CARGO_TARGET_DIR=/private/tmp/codewhale-m8a-target` 和 `CARGO_NET_OFFLINE=true`。

## 7. Credential 与官方资料边界

M8-A 没有改变 DeepSeek request、response、model capability、reasoning、prompt、tool
catalog、budget 或 accounting surface。付费请求不能为本地配置/入口切换增加归因力，
因此 credential gate 在 `key.txt` 前停止。

2026-07-23 保留并复核的官方一手资料入口：

- [API introduction](https://api-docs.deepseek.com/)
- [Models and pricing](https://api-docs.deepseek.com/quick_start/pricing)
- [Thinking mode](https://api-docs.deepseek.com/guides/thinking_mode)
- [Tool calls](https://api-docs.deepseek.com/guides/tool_calls)
- [FIM completion](https://api-docs.deepseek.com/guides/fim_completion)
- [Beta API](https://api-docs.deepseek.com/guides/beta_version)

本切片只离线复核 ownership 和入口；没有重新声明易变模型 limits、Beta 行为或价格，也
没有用 credential 请求来伪装产品证据。易变事实继续由 `crates/deepseek` fixtures 管理。

## 8. 产品决策

决策是：

**保留 DeepSeek-only production 配置与入口，删除 generic Provider 路径。**

理由：

- 三个入口现在只有一个 credential/endpoint/model truth；
- 外来 Provider/模型和旧兼容配置在副作用前 fail closed；
- reopen、machine protocol 和 actor conformance 未回归；
- 38,981 行净删除降低了真实维护复杂度；
- 没有新增模式、抽象层或重复 owner。

## 9. 非结论

M8-A 不证明：

- verified coding success、Token、wall time 或 API cost 得到提升；
- DeepSeek v4 model ids、limits、Beta 或价格在未来保持不变；
- 品牌改名、release、安装、卸载、打包、CI 或远程策略已经完成；
- fixed zh-Hans 全产品端到端和中文 Agent prompt A/B 已完成；
- MCP 已进入 canonical model-visible tool catalog；
- RepoGraph、多 Writer 或通用 DAG 应准入。

## 10. 下一切片

建议下一阶段为 M8-B，单独处理产品身份与本地交付生命周期：

- 只读审计 binary/package/config/state/User-Agent、origin/upstream、version check、
  `crates/release`、安装/卸载脚本、CI、GitHub/CNB 和未接线云资产；
- 冻结 source build、install、first run、upgrade、rollback/uninstall、data retention、
  offline/no-network、checksum 和旧资产拒绝矩阵；
- 只在真实 caller 迁到唯一 product/release owner 后删除旧品牌、重复更新器、兼容路径和
  未接线部署资产；
- 不混入全面汉化、prompt A/B、MCP、RepoGraph、多 Writer 或新模型系统。
