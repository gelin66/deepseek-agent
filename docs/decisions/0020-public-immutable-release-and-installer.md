# ADR-0020：公共 Immutable Release 与单一安装 owner

- 状态：已接受
- 日期：2026-07-29
- 细化：ADR-0006、ADR-0009、ADR-0016
- 部分取代：Roadmap M7 对所有 GitHub release discovery 的无差别删除结论

## 真实问题

DSE 已有可复核的本地 `dse.delivery.v1` artifact 与完整 install/verify/upgrade/
rollback/uninstall 生命周期，但用户仍必须先取得本地源码或 artifact。仓库没有公开 stable
Release、不可变 binary、版本化 installer 或一条公共安装命令。这个缺口属于发布与交付，
不是新的 Agent capability。

## 决策

### 1. 唯一发布与安装真相

- `scripts/dse-delivery.sh` 继续是唯一离线 package/install/verify/upgrade/rollback/
  uninstall owner；其 exact manifest、内外 SHA-256、原子 `current`/`previous`、foreign
  path 拒绝和 `DSE_HOME` preservation 不复制到第二套 installer。
- GitHub 仓库 `gelin66/deepseek-agent` 的已发布 **Immutable Release** 是唯一公共
  binary/release truth。Actions 临时 artifact、mutable default branch、raw URL、Sites、
  Homebrew bottle 或其他下载服务都不能成为并列二进制来源。
- release tag 必须等于 workspace SemVer 的 `vX.Y.Z`，并绑定通过当前 RC、full、公开 CI
  和 delivery 门的 default-branch exact revision。首次公共 stable release 为 `v0.8.68`。
- release workflow 必须先创建 Draft，直接把各 native runner 的资产加入 Draft，完整验证
  后才发布；Immutable Releases 必须在发布前启用。发布后 tag 和 asset 不得改写。

### 2. 稳定资产与 URL 合同

每个 stable release 精确发布：

```text
dse-installer.sh
dse-<version>-aarch64-apple-darwin.tar.gz
dse-<version>-x86_64-apple-darwin.tar.gz
dse-<version>-aarch64-unknown-linux-gnu.tar.gz
dse-<version>-x86_64-unknown-linux-gnu.tar.gz
SHA256SUMS
dist-manifest.json
SBOM.spdx.json
```

版本化资产根固定为：

```text
https://github.com/gelin66/deepseek-agent/releases/download/v<version>/
```

`dist-manifest.json` schema 为 `dse.release.v1`，绑定 product、repository、version、tag、
source revision/tree、installer/checksum/SBOM 名称，以及四个 target 的 exact asset、SHA-256、
byte size 与 source mode。公开 workflow 只接受四个 archive 都报告
`locked-offline-source`；顶层 `SHA256SUMS` 精确绑定 installer、manifest、SBOM 与四个 archive；archive
内部继续使用 `dse.delivery.v1` manifest 与 inner `SHA256SUMS`。

`source_sha256`、archive checksum 和 attestation 只证明 received bytes/release provenance，
不宣称软件无漏洞。GitHub release attestation 与 Actions artifact/SBOM attestation作为公共
provenance evidence；installer 的 cold path 不依赖 GitHub CLI。

### 3. 公共入口与版本化 installer

唯一主命令是：

```sh
curl -fsSL https://dse.run/install.sh | sh
```

`DSE.RUN` 是现有 Codex Sites 官网，不是新服务器或第二 release owner。Sites 的
`/install.sh` 只解析 GitHub latest stable 到 exact `vX.Y.Z`，下载并按 `SHA256SUMS` 验证
`dse-installer.sh` 与 `dist-manifest.json`，然后用 `/bin/sh` 调用版本化 installer；Sites
不保存平台二进制，也不读取 mutable branch。

版本化 `dse-installer.sh` 必须可由 POSIX `/bin/sh` 启动。它只负责：

1. 解析 exact stable version 与 Darwin/Linux、aarch64/x86_64；
2. 从固定 exact Release URL 下载 manifest/checksum/archive 到 `mktemp`；
3. 交叉校验 installer、manifest、target、archive SHA-256 与 release identity；
4. 提取同 revision 的 canonical `dse-delivery.sh` payload，并由系统 Bash 执行本地 owner。

它不 `source` 网络内容，不接受任意 URL、header、hook、proxy、credential 或后台更新。
默认 prefix 为 `$HOME/.local`、不使用 `sudo`，支持 `--version VERSION`、`--prefix DIR`、
`--no-modify-path`、`--help` 以及同一 delivery owner 的 verify/rollback/uninstall 投影。
首版不修改 shell profile；当 prefix/bin 不在 PATH 时只输出精确、可复制的 PATH 指令。

安装前拒绝 Homebrew、Cargo 或未知 owner 的已有 `dse`，失败不得改变 active release。
same-version 必须幂等；upgrade 保留 `previous`；rollback 可用；uninstall 只删除 installer-owned
路径并保持 `DSE_HOME`、config、RunStore 和 credential bytes。没有静默自更新；用户通过
重跑 installer 显式升级。

### 4. 支持边界与证明

只有 native build、fresh install、`dse --version`、`dse doctor --json` 与 lifecycle 全部
通过的 target 才列为支持。首版目标是 macOS/Linux 的 arm64/x86_64；Windows native 不
承诺，Windows 用户只通过 WSL2 Ubuntu 22.04 x86_64 安装；公开 support report 必须区分原生
Ubuntu runner 的 binary/install 证明与是否另有真实 Windows-hosted WSL canary，不得把二者混写。

离线/fixture 先覆盖 target、latest/exact、404/timeout/partial、checksum/manifest/
wrong-target、traversal/symlink、foreign manager、same-version、upgrade/rollback/uninstall 和
用户数据保持。冻结 revision 只跑一次 full gate，再执行 Draft release dry-run。不能用重复
tag/release 或放宽校验求绿。

## 与旧结论的关系

M7 删除的是失去 owner 的 Runtime/TUI updater、版本检查、`crates/release`、CNB/imported
release reader 和静默 release metadata discovery；这些继续保持删除。本 ADR 只准入用户显式
启动的 Host 外 delivery bootstrap。它不进入 AgentRuntime、RuntimeEvent、RunStore、DeepSeek
wire、Prompt、工具 catalog 或应用配置，也不恢复第二 installer、release Store 或后台 updater。

## 明确拒绝

- npm/Node wrapper、`cargo install` 主入口、Python cold-install dependency；
- mutable raw/default-branch installer、Actions artifact 下载入口、任意镜像 URL；
- 静默后台更新、自动重复未知副作用、sudo 默认安装；
- 新 server/CDN/object storage/API/sidecar、第二 Runtime/Store/Provider；
- 在程序仓线程直接修改 `/Users/gelin/Desktop/DSE站点`；
- 在首个公共命令闭合前用 Homebrew tap 冒充主入口完成。

## 完成真相

“脚本存在”不等于“一键安装可用”。只有 GitHub stable release 显示 Immutable、全部资产与
attestation 可核验，并且公网 `https://dse.run/install.sh` 在干净支持环境取得 shell 而非
HTML，完成下载、校验、安装、version/doctor 后，才可声明公共安装闭环完成。没有 stable
release 时官网必须非零退出并输出 `no stable DSE release is available yet`，HOME 零副作用；
失败不得返回 HTML 200 冒充 installer 成功。
