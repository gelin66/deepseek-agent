# ADR-0009：DSE 是唯一当前产品与发布身份

- 状态：已接受
- 日期：2026-07-25

## 决策

当前产品正式命名为 **DSE**，英文全称为 **DeepSeek Engineer**。DSE 是一个 Rust 原生、
本地优先、只接官方 DeepSeek API 的编码 Agent。当前产品、源码包、二进制、安装目录、
配置命名空间、活动协议和新评测统一使用以下身份：

| 范围 | 唯一当前身份 |
|---|---|
| 产品显示名 | `DSE` |
| 英文全称 | `DeepSeek Engineer` |
| 主二进制 | `dse` |
| TUI 二进制 | `dse-tui` |
| Cargo package/import 前缀 | `dse-*` / `dse_*` |
| 用户目录 | `~/.dse` |
| 产品配置环境变量 | `DSE_*` |
| release artifact | `dse-{version}-{target}-...` |
| 活动协议/评测命名空间 | `dse.*` |
| vendor media type | `application/vnd.dse.*` |

官方 DeepSeek 环境变量（例如 `DEEPSEEK_API_KEY`）保持官方名称，不改写为产品变量。

CodeWhale 只作为 MIT 上游来源、导入基线、Git 历史和冻结评测身份保留，不再作为当前
产品名称。不得改写已冻结 manifest、summary、raw、hash 或旧 schema 来伪造历史重命名；
新的代码、运行、协议、文档入口和发布物不得继续产生 CodeWhale 身份。

DSE V1 采用一次硬切换，不发布 `codewhale`、`codew`、旧 env/path reader、双写 schema
或永久兼容别名。因为旧身份尚未公开发布，现有本地配置、Secret 和可保留状态只允许使用
一个有明确删除点的一次性迁移切片：先复制、校验并保留原目录备份，DSE release candidate
形成前删除迁移代码和所有旧路径生产读取方。

模型可见身份先做仅名称替换：

```text
CodeWhale -> DSE
```

不得在该切片同时重写执行、验证、工具或多 Agent 语义。身份切换通过 current
conformance 后，才冻结为中英文 prompt A/B 的共同 DSE 基线。

## 原因

- DeepSeek 已把 `DSA` 公开定义为 V4 的 **DeepSeek Sparse Attention**；继续使用
  `DSA = DeepSeek Agent` 会造成官方技术名、搜索结果和用户认知冲突，因此在公开发布前
  改用 `DSE = DeepSeek Engineer`。依据：
  [DeepSeek V4 官方发布说明](https://api-docs.deepseek.com/zh-cn/news/news260424/)；
- 开源产品需要唯一、简短、可复述的产品、命令和发布身份；
- 当前 `CodeWhale/codewhale/codew` 同时出现在二进制、Cargo、路径、协议、脚本和
  model prompt，仅改 README 会留下真实产品债；
- DSE 尚未公开发布，当前是完成无别名硬切换、避免长期兼容成本的最低风险窗口；
- 上游来源与当前产品身份是不同事实：保留 MIT 归属不要求继续沿用上游品牌；
- branding、双语 UI 和 prompt 质量实验是不同变量，必须分切片接管和验证。

## 后果

- M8-B 的 CodeWhale 产品/交付身份作为历史 release-ready checkpoint 保留，但被本决策
  取代；DSE 二进制与 delivery lifecycle 必须重新形成 exact-current release evidence；
- `PRODUCT_PLAN.md`、`ROADMAP.md`、`EVALUATION.md` 和公共仓库入口改用 DSE；
- active Cargo package、binary、config/path/env、User-Agent、schema、media type、CI、
  install/rollback/uninstall 和 prompt identity 均进入有边界的 DSE cutover；
- 失去消费者的旧名称、别名、路径读取器、安装链接和临时迁移器在各自切片 cutover 时删除；
- 本决策不改变 DeepSeek-only、fixed model routing、AgentRuntime、RunStore、工具目录、
  completion ownership 或 Provider 范围；
- GitHub 仓库 slug 优先使用 `dse`；远端改名、公开和 release 仍是显式外部操作，只有本地
  DSE candidate、远端 CI 和治理门禁通过后才执行。
