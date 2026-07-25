# ADR-0010：DSA 双语产品与单一 prompt 语言准入

- 状态：已接受
- 日期：2026-07-25
- 取代：ADR-0004 的固定简体中文产品方向

## 决策

DSA 面向人的产品界面正式支持且只支持：

```text
en
zh-Hans
```

公开仓库以英文为主入口并提供完整中文入口。UI locale 与模型可见 prompt 是两个独立
层次：`crates/localization` 是人类消息目录与语言解析的唯一 owner；`crates/context`
继续是 production prompt 的唯一 owner。不得把 locale 传播到第二 Runtime、第二 Store、
第二事件协议或每 Agent 私有状态。

产品语言按以下确定性顺序解析一次：

```text
显式进程级 --language
  -> 持久化 ui.language
  -> 首次交互启动的双语选择
  -> 非交互新环境默认 en
```

旧本地身份迁移到 DSA 时默认保留 `zh-Hans`。不增加模型语言分类请求、关键词检测、
按 Run/Agent 的自动路由、输出后处理翻译或在线翻译服务。

命令、flags、工具名、JSON/API/schema 字段、稳定错误码、模型 ID、路径、代码、diff、
stdout/stderr、原始日志和机器协议保持稳定，不随 UI locale 翻译。CLI、TUI、Doctor、
onboarding、审批、错误恢复、root/read-only child/Writer 状态及其他人类文本使用同一
`en`/`zh-Hans` catalog contract。

Agent 默认使用用户当前任务的语言回答，除非用户显式指定其他语言；代码、协议、命令和
技术标识保持原样。该行为由单一 production prompt 表达，不增加 Host classifier。

当前中文 production prompt 保留为可回滚基线，但不再被视为未经比较的永久语言方向。
品牌身份改为 DSA 后，建立语义、顺序、强度和工具/验证规则等价的中文与英文 system
prompt，执行 current fixed-Pro 的 2×2 配对实验：

| production system prompt | 用户任务语言 |
|---|---|
| English | English |
| English | 简体中文 |
| 简体中文 | English |
| 简体中文 | 简体中文 |

正式实验第一块为 8 个独立任务族 × 2 个任务语言 × 2 个 prompt，共 32 runs；只有第一块
无质量否决且预注册判定仍需更多置信度时，才执行完整的第二块，最多 64 runs。不得选择性
补跑、补 mate 或拼接旧中文 treatment。

最终 production 只保留一个 prompt：

1. 英文在两个任务语言层都质量不回归且相同或更好时，保留单一英文 prompt；
2. 中文在两个任务语言层都相同或更好时，保留单一中文 prompt；
3. 两者只在同语言任务占优时，不增加 Auto 路由；最多再评测一个单一紧凑双语候选；
4. 评测无效或证据不足时保持当前中文 DSA 基线，不声明语言优劣。

失败候选、eval-only selector/asset 和临时产品开关在 cutover 时删除。生产不提供 prompt
语言模式、per-run selector 或两套行为分支。

## 原因

- ADR-0004 的前提是所有者自用且没有真实多语言需求；公开开源已经使该前提失效；
- 英文公共入口和完整英文产品面是国际采用与外部审查的真实需求，中文仍是核心用户体验；
- 旧 2026-07-18 实验同时改变语言、内容、结构和长度，只能拒绝该中文 treatment，
  不能证明英文或中文天然更优；
- UI 翻译不等于 Agent 能力；prompt 语言必须以 verified task success 和 false-success
  证据决定；
- 单一 prompt、单一 runtime 和无分类请求比永久双 prompt/Auto 路由更简单、可重放。

## 后果

- ADR-0004 标记为被本决策取代；其 fixed-Chinese 实现与历史证据保留到双语切片接管；
- ADR-0007 继续证明旧中文基线的 immutable identity、rollback 和历史 release evidence，
  但其“固定中文是永久产品语言”部分被本决策取代；
- `zh-Hans.json` 不再是 sole catalog；新增 exact-key/placeholder-compatible `en.json`；
- 新增最小 `ProductLanguage { English, SimplifiedChinese }` 和一个进程级解析结果；
- 不恢复 CodeWhale 的旧多语言包、locale 生态、`/translate`、模型翻译或第二消息 owner；
- `README.md` 为英文权威入口，`README.zh-CN.md` 为完整中文入口；Roadmap、评测和架构
  真相仍只有一套，不创建双语平行路线图；
- DSA 公开发布门槛新增双语 UI、回答语言、机器协议稳定和 prompt admission evidence。
