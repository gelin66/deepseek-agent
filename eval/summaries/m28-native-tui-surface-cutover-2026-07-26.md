# M28 DSE native TUI surface cutover

- 日期：2026-07-26
- 结论：`keep_native_surface_and_delete_legacy`
- cutover checkpoint：`07214e2ef`（首次 PTY cutover `1e420d9d7`，parity closure
  `068c8e3a9`，旧术语清理 `60df6f8f7`，最终 PTY 观测闭合 `07214e2ef`）
- 范围：纯本地 presentation/interaction；无 Key、official API、GitHub、push 或 release

## 真实问题与唯一 owner

M26 已让 canonical Run 可读，M27 已闭合三档权限，但 production TUI 仍混有
Underwater/Ocean 动画外壳、居中 card/modal、并行 theme/palette reader、持久显示模式和
退役文案。它们让同一 Run 事实具有多套容器、颜色和焦点语法。

M28 保持原 owner 不变：

- `CanonicalRunPresentation`：task/activity/change/verification/Agent/permission/
  recovery/terminal；
- TUI presentation layer：surface topology、responsive layout、focus/hitbox；
- `crates/tui::palette`：唯一 semantic token 与 terminal depth adaptation；
- `crates/localization`：English/`zh-Hans` 人类文案；
- canonical Runtime/Store/tools/permission owner：业务、证据和终态事实。

没有第二 renderer、Runtime、Store、prompt、permission policy 或 presentation truth。

## Cutover

M28 的独立 checkpoints 为：

| slice | checkpoint | result |
|---|---|---|
| contract | `89d2b34dd` | ADR-0013 / DESIGN / frozen evaluation |
| A topology | `5ec379236` | 11 个可达 surface 合同与 legacy ban |
| B shell/tokens | `5ce6210cf` | terminal-native shell 接管，删除 Ocean/animation |
| C main layout | `16c5e7286` | wide rail / medium strip / narrow single-column |
| D secondary | `57d3e8343` | onboarding、sheet、room、inline approval 接管 |
| E deletion | `f25ffc906` | 删除 theme/display modes/retired copy/fixtures |
| gate hardening | `28577e80b` | 分离 transport 与 model-event stall 测试时限 |
| F PTY/reopen | `1e420d9d7` | 五尺寸双语真实 PTY与 exact visible-cell reopen |
| post-cutover lint | `8fc40ac6f` | 删除 E 切片遗留的等价分支与空 struct update |
| final parity closure | `068c8e3a9` | 双语剩余文案、固定 token 层级、onboarding/slash/mention 鼠标与双语 semantic-frame reopen |
| vocabulary deletion | `60df6f8f7` | 删除测试、局部变量和注释中的 active modal 旧术语 |
| PTY observation closure | `07214e2ef` | resize 断言绑定已满足 predicate 的同一 frame，删除额外 pump 的 clear/redraw 竞态 |

物理删除：

- `underwater.rs`、`ocean.rs`、fish/bubble/gradient/flee/ambient timer；
- `status_indicator.rs`、generic centered/shadow/modal helper；
- selectable theme/background owner、theme remap、display/layout/decorative setting reader；
- persistent calm/tool-detail variants；tool row 改用一个 compact rule 与 process-local
  expansion；
- Doctor 退役 command hints、硬编码用户英文；
- 孤儿 session/command Gherkin 与只证明旧视觉模式的 evidence/fixture。

保留的非视觉职责包括 bracketed paste、synchronized output、Unicode width、ANSI depth、
terminal reset background、SSH/low-motion safety、show-thinking、reasoning/cost/mention 和
tool/workspace compatibility。

整个 M27 checkpoint `79f92e92e` 到 M28 final cutover closure 的仓库 diff（包含合同文档）为
3,314 insertions / 5,321 deletions，净删除 2,007 行。production source 变更限定在
TUI/localization；protocol、runtime、state、deepseek、tools 没有 M28 semantic delta。

## Deterministic evidence

| gate | result |
|---|---|
| surface contracts | 11/11 complete；4 个合法 container |
| frozen size/language PTY | 2 languages × 5 sizes = 10/10 |
| PTY interaction | QA 15/15；keyboard/mouse/paste/resize/onboarding/slash/mention/approval/permission |
| canonical PTY/Store | 7/7；first-run、terminal、pending creation、reopen |
| live/reopen presentation | English/`zh-Hans` 的尺寸、非空 glyph/坐标、前后景、bold/italic/underline/inverse、cursor exact |
| idle | initial frame 后 5 秒无 PTY bytes |
| release runtime QA | 5/5，1 个预注册 heavy storm ignored |
| exec process acceptance | 25/25 |
| TUI unit/integration | 765/765，2 个预注册 ignored；删除了只数矩阵维度的空洞自测 |
| localization/color | key/placeholder parity；ANSI-16/256/truecolor/reset semantic gate |
| repository gates | focused、fmt、strict Clippy、workspace test、public checker、diff check |
| installed lifecycle | `8fc40ac6f` locked/offline package、install、verify、binary smoke、uninstall |

全量套件最初暴露一个测试自相干扰：raw SSE fixture 同时把 transport chunk/open guard 和
model-event idle guard 设为 1 秒；并发负载下前者会先失败，使测试进入错误的 transport
分支。`28577e80b` 只把 fixture 的 transport guard 放宽为 5 秒，仍以 1 秒验证 typed
`stream_stall`，production 配置和行为未变。

最终独立审查又发现四个不能留作历史债的缺口：`Draft` 与 tool-run summary 绕过
localization、onboarding/slash/mention 没有真实 mouse row action、reopen 只比较 glyph/
坐标、固定 token 把 secondary/hint/border 压成同一 terminal default。`068c8e3a9`
通过现有 localization/palette/TUI owner 修复，并删除 active `ModalKind`/`ModalView`/
`render_modal_*` 命名。reopen 合同会规范化 clear 与 overwrite 产生的等价空白编码；对其余
语义单元格及尺寸、样式、光标逐项精确比较，不宣称 raw PTY byte equality。

最终 workspace gate 还复现一次 resize predicate 通过后、断言前额外读取 PTY chunk
落在 clear/redraw 窗口的空帧。`07214e2ef` 让断言读取刚刚满足 predicate 的同一
observed frame；冻结矩阵随后独立连续 3/3、完整 QA 15/15 通过。

真实 macOS Terminal、iTerm2、Ghostty 宿主差异没有在缺少对应宿主的自动化进程中伪造
人工结论；标准 PTY/crossterm 路径、ANSI depth 和 terminal reset 已 deterministic 覆盖。

## 非结论

本结果证明 presentation correctness、交互 parity、idle stillness、RunStore reopen
一致性和历史债删除。它不证明 DeepSeek verified success、Token、cache、API cost 或模型
wall-time 改善，也不改变 prompt、model、Thinking、tools、permission、RuntimeEvent、
Run API、State schema 或 completion owner。

后续 TUI 功能必须进入现有三种 container 或 inline approval，并复用同一 token、
localization、focus/hitbox 与 canonical presentation owner；不得恢复 General Settings、
legacy toggle、第二 renderer 或兼容 reader。
