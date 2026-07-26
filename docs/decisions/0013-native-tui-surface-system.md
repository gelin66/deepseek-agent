# ADR-0013：DSE 原生 TUI 表面系统

- 状态：已接受
- 日期：2026-07-26
- 实施里程碑：M28

## 真实问题

M26 已让 canonical Run 的任务、活动、变更、验证、权限和恢复事实可读，M27 又让权限
选择与 Host 强制语义闭合，但当前 TUI 仍不是一套完整产品界面：

- 主外壳继续使用从 CodeWhale 继承的 Underwater/Ocean、鱼群、气泡、渐变和空闲动画；
- onboarding 使用居中全边框卡片，`request_user_input` 使用大面积居中 modal，
  approval、permission、pager 和主工作区又各有不同的容器与留白语法；
- 主题、背景、海洋 treatment、动画、WorkSurface 位置、composer 密度/边框、
  transcript 间距和状态标识等显示参数把产品设计责任转嫁给用户；
- 可达组件有的读取 resolved `UiTheme`，有的直接读取全局 palette，导致同一状态在不同
  表面可能使用不同颜色真相；
- Doctor 和旧 feature fixture 仍可能出现已退役命令或硬编码英文，完整的中英文、
  窄屏、鼠标、恢复和终态视觉矩阵也没有一个统一 owner。

这不是单纯“换颜色”。长任务用户必须能稳定回答：

```text
当前目标是什么
Agent 正在做什么
已经改变了什么
Host 验证到哪里
现在是否需要我操作
任务最终以什么证据结束
```

旧表面之间的视觉和交互分裂会增加认知负担，也使后续每个功能继续发明自己的 panel、
modal、颜色和快捷键。

## 决策

DSE 采用一个 **Operate 模式、transcript-first、terminal-native** 的 TUI 表面系统。
它吸收 Codex、macOS Terminal 和成熟 CLI/TUI 的通用交互惯例，但不复制任何产品源码、
品牌、文案、布局资产或内部状态模型。DSE 的任务、证据、恢复、权限和 Agent 投影继续
完全来自自己的 canonical Run 链。

### 1. 一条信息闭环

主界面固定表达：

```text
task intent
  -> current activity
  -> observed changes
  -> Host verification
  -> terminal outcome / required user action
```

transcript 是主表面。稳定的 Run 摘要只投影
`CanonicalRunPresentation`，不得另建进度、plan、confidence、文件数或终态真相。
模型自报与 Host evidence 必须在视觉上可区分。

### 2. 三种容器与一种中断

所有可达界面只能由三种容器组成：

1. **主工作表面**：单行 header、transcript、canonical Run 摘要、composer 与状态行；
2. **底部 sheet**：权限、结构化用户问题和其他短选择；
3. **全屏 room**：onboarding、长帮助、日志、diff、证据和其他需要持续阅读的内容。

审批不是第四种 modal；它是 transcript/composer 之间的 **inline interruption band**，
保持上下文可见并只占完成当前决定所需的高度。

旧的通用居中 modal、任意矩形 panel 和每个功能自定义 chrome 不再是合法产品表面。
弹层中的选中对象必须有 render-time hitbox、可见焦点和键盘/鼠标同 action parity。

### 3. DSE 原生视觉语言

- 终端背景是默认 surface；不绘制海洋、水下渐变、鱼、气泡、ambient life、阴影卡片或
  纯装饰动画；
- 采用克制的中性层级与一个 DSE signal accent；绿色只表示确定性成功/验证，黄色只表示
  注意/审批，红色只表示失败/拒绝/危险；
- 状态不能只靠颜色表达，必须同时有稳定文案或符号；
- 边框只在分隔信息层级或焦点时使用；不以 `Borders::ALL` 包裹整页或普通内容；
- 终端拥有字体、字号、字距、原生复制和窗口管理。DSE 只使用 bold、dim、空白、对齐和
  Unicode-width-safe 截断建立层级，不伪造“字体系统”；
- idle 界面不持续 redraw。运行中只允许一个低频、可关闭的当前活动标记；streaming 速度
  始终来自真实 delta，不制造打字动画。

完整持久视觉合同见仓库根 [DESIGN.md](../../DESIGN.md)。

### 4. 固定响应式规则

布局由产品根据终端尺寸确定，不由用户选择 left/right/top：

- 宽终端保留 transcript 主列和右侧 canonical Run rail；
- 中等宽度把同一摘要压为 transcript 上方的短 strip；
- 窄/矮终端变为单列，只先去除重复标题、caption、边框和空白，不能先删任务、当前状态、
  待用户操作、终态证据或 composer；
- resize 只能改变投影位置和密度，不能改变 Run 状态、焦点对象或已输入内容。

### 5. 成熟交互，不增加学习成本

- `↑/↓` 移动，`Enter` 提交或确认，`Esc` 关闭当前 sheet/room 或取消当前选择；
- `Ctrl-C` 保持现有 canonical 语义：活动 Run 时 interrupt；空闲且有输入时清空；
  空闲空输入时退出；
- `Ctrl-D` 保持现有明确 cancel/退出语义；
- `Shift/Alt+Enter` 保持 composer 换行；
- `/permissions` 与可点击 permission chip 打开同一个底部 sheet；
- mouse click、scroll 和键盘必须落到同一 action；不得增加只对鼠标可见的能力；
- 不拦截 macOS Terminal/iTerm2/Ghostty 的 `Cmd-C`、`Cmd-V`、窗口、标签页和系统级惯例；
- 不增加隐藏的 Shift-Tab 模式轮换、全局 command palette 或新奇快捷键。

### 6. 不做 General Settings

TUI 不增加 General、Appearance、Theme、Layout 或 Advanced 设置中心，也不提供 Custom
权限。用户不负责组合一套“好看且可用”的 DSE。

- 权限继续使用 ADR-0012 的独立三档 selector；
- 语言继续由 ADR-0010 的进程级 owner 解析；
- theme/background/layout/motion 使用产品默认和终端能力自动适配；
- 只有真实终端兼容、无障碍或工具执行需要的技术开关可以保留在配置文件，且不得成为
  第二视觉系统；
- 没有 production necessity 的装饰、密度和位置键在 M28 cutover 物理删除，不保留
  alias、兼容 reader、隐藏 UI 或第四种模式。

### 7. 唯一 owner

| 事实 | 唯一 owner |
|---|---|
| Run task/activity/change/verification/terminal projection | `CanonicalRunPresentation` |
| surface topology、responsive placement、focus/hitbox | `crates/tui` presentation layer |
| color/spacing/state roles | 一个 resolved DSE UI theme/token owner |
| human copy | `crates/localization` 的 `en` / `zh-Hans` catalog |
| permission semantics | ADR-0012 的 protocol/tools/runtime/state 链 |
| task、tool、evidence、recovery truth | canonical RuntimeEvent / RunStore |

所有可达 renderer 必须显式获得同一个 resolved theme/tokens；不得一部分读取
`app.ui_theme`、另一部分读取全局 palette 常量。TUI 不持久化业务状态，也不从显示文本
反推 protocol value。

## 实施与切换

M28 必须在 M27 clean checkpoint 后独立实施，按以下垂直顺序：

1. 冻结可达表面、状态、尺寸、语言、键鼠和终端矩阵；
2. 建立单一 presentation token/surface primitive，并先迁移真实主 caller；
3. 删除 Underwater/Ocean 和 idle animation，再完成主 shell；
4. 把 onboarding、user input、approval、permission 和 pager 迁到固定表面语法；
5. 删除失去必要性的显示配置、旧 Doctor 文案、孤儿 feature 和 direct palette path；
6. 通过真实 PTY、crash/reopen、全仓门禁后，更新 current facts 并物理删除所有旧 renderer。

临时 adapter 只能存在于 M28 工作分支内，最终提交不得保留双 renderer、legacy theme
selector 或兼容 reader。

## 被拒绝的替代方案

- **只给 Underwater 换一套颜色**：旧容器、动画和设置债仍然存在，不能形成统一系统。
- **逐页慢慢美化并长期保留两套表面**：同一状态继续有不同布局、token 和交互语法。
- **增加 General Settings 让用户自行选择**：把产品决策变成配置复杂度，无法保证默认体验。
- **照搬 Codex 界面或源码**：会带入不属于 DSE 的产品事实、兼容负担和品牌混淆。
- **为了极简隐藏活动/变更/验证摘要**：重新变成不可解释的盲盒。
- **让 TUI 维护自己的 plan/progress/evidence**：破坏唯一 RuntimeEvent/RunStore 真相。

## 后果

- “彻底重构完成”意味着全部可达 surface 已切换、旧路径已删除，而不是默认页看起来更新；
- M28 是纯 presentation/interaction 切片，不改变 DeepSeek prompt、model、Thinking、
  tools、permission policy、Run API 语义、RuntimeEvent 语义或 RunStore 事实；
- 用户获得稳定、安静、可解释的长期工作界面，代码侧减少装饰 renderer、显示配置和并行
  theme path；
- 任何新的 TUI 功能必须先选择现有三种容器之一，并复用同一 token、focus、hitbox 和
  localization contract；若确实需要第四种容器，必须以新的可用性证据修改本 ADR。
