# ADR-0001：Rust + DeepSeek 专用产品

- 状态：已接受
- 日期：2026-07-15

## 决策

以 CodeWhale 的 Rust 源码为唯一底座，开发 DeepSeek 专用的本地编码 Agent 产品。
Cline 和其他顶级 Agent 只作为能力参考，不引入其运行时、源码拼接或永久桥接层。

## 原因

- CodeWhale 已有真实 Rust Agent、工具、上下文、多 Agent、状态和 TUI 能力；
- DeepSeek 专精可以删除通用 Provider 的路由、配置和测试复杂度；
- 单一语言和运行时更适合本地长期维护；
- 外部能力可以按问题重新实现，而不继承其完整产品负担。

## 后果

- 现有其他 Provider 在 DeepSeekBackend 接管并通过基准后删除；
- 不继续同步上游产品路线；
- 保留 MIT 许可和必要来源说明；
- 正式品牌、仓库 remote 和发布链在 V1 产品化阶段完成。
