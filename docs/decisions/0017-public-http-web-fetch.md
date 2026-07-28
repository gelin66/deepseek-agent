# ADR-0017：公开 HTTP 获取与传输信任分层

- 状态：已接受
- 日期：2026-07-28
- 部分取代：ADR-0015 `web_fetch`“只接受 HTTPS”条款
- 实施顺序：ADR-0016 首个 Goal clean checkpoint 后、M45 前
- owner：`crates/tools`

## 决策摘要

DSE `web_fetch` 应读取公开的 `http://` 与 `https://` URL。HTTPS 是传输认证与完整性保护，
SSRF 防护则由 URL、DNS、实际连接地址、重定向和 egress policy 强制；二者不是同一个安全
边界。把所有 HTTP 一律拒绝会丢失真实公开网页，但简单删除 scheme 检查又会引入静默降级、
信任误报和实现链不一致。

因此采用以下边界：

```text
public HTTP(S) URL
  -> scheme / userinfo / port gate
  -> hostname + literal IP gate
  -> resolve all addresses
  -> public-unicast validation
  -> connect pin
  -> manual redirect with monotonic transport security
  -> bounded text-only extraction
  -> explicit transport provenance
  -> ToolOutcome / RuntimeEvent / RunStore replay
```

本 ADR 不引入浏览器、搜索 Provider、Cookie、认证、第二 Runtime、第二 Store 或新协议面。

## 当前事实与问题清单

M41 提交 `943d9c500` 交付了安全、可重放的 HTTPS-only `web_fetch`。该最小切片正确建立了
SSRF、DNS pin、重定向、body、content type、deadline、ToolOutcome 和 reopen 边界，但 HTTPS
假设已经分布在完整垂直链中。

### 1. 产品覆盖不完整

`parse_and_validate_url` 直接拒绝 `http://`。公开的旧站点、地区性站点、设备文档镜像和部分
纯文本资料无法读取，用户只能退回 shell/curl 或等待更重的 browser，违背“HTTP 能解决时不
启动 Chrome”的既定顺序。

### 2. 传输安全与 SSRF 被混成一个概念

HTTPS 不能阻止请求访问 `127.0.0.1`、RFC1918、link-local 或 metadata；HTTP 也不必然意味
主机侧 SSRF。当前真正有效的安全门是：

- userinfo/credential deny；
- hostname 与 literal IP deny；
- DNS 全地址 public-unicast 校验；
- connect target pin；
- 每跳 redirect 重新校验；
- 无 proxy、Cookie、认证、Referer、任意 header 或主动内容。

这些门必须原样保留并覆盖 HTTP。不能用 scheme 白名单代替 egress policy。

### 3. 只改 URL 白名单会形成半切换

当前至少有以下 HTTPS-only implementation facts：

| 位置 | 当前假设 | 不完整修改的后果 |
|---|---|---|
| URL preflight | `scheme == https` | HTTP 在授权前被拒绝 |
| network client | `.https_only(true)` | preflight 放行后真实请求仍失败 |
| port/message | fallback 与错误文案写死 443/HTTPS | HTTP 诊断和连接 identity 错误 |
| redirect | 所有 HTTP target 被一律拒绝 | 没有明确 upgrade/downgrade contract |
| link extraction | 复用 HTTPS-only validator | HTTP 页面和 HTTPS 页面中的 HTTP 链接被静默丢弃 |
| authorization | `public_https_web_fetch` | Run audit 与实际能力不一致 |
| catalog | “已知公开 HTTPS URL” | DeepSeek 不知道 HTTP 已可用 |
| fingerprint | `rustls_pinned_public_https_v1` | replay/composition identity 漂移未显式切换 |
| fixtures/docs | HTTP 被固定为 scheme 反例 | 回归会继续要求错误行为 |

必须一次迁移真实 caller 并删除旧 identity；不能长期兼容双路。

### 4. 重定向降级没有合同

放开 HTTP 后若仍只做 `http|https` 白名单，初始 HTTPS 页面可把 Agent 静默重定向到 HTTP，
丢失调用者原本获得的 TLS 保护。安全合同必须单调：

```text
HTTP  -> HTTP   allow
HTTP  -> HTTPS  allow and latch secure transport
HTTPS -> HTTPS  allow
HTTPS -> HTTP   deny: web_transport_downgrade
HTTP -> HTTPS -> HTTP deny: web_transport_downgrade
```

不自动把用户的 HTTP URL 重写成 HTTPS，也不在失败后偷偷改 scheme 重试；精确 URL、响应和
redirect 才是可重放事实。

### 5. 当前结果不能表达传输完整性

`trust=external_untrusted` 正确表达网页没有指令权，但无法区分：

- 经服务器证书保护的 HTTPS 传输；
- 可被链路中间人修改的明文 HTTP 传输。

`source_sha256` 只证明 DSE 实际收到并提交了哪些 bytes，不能证明 HTTP 内容来自声明的发布者。
如果不增加显式字段，模型、人类 UI 和后续 evaluator 容易把“有哈希”误读成来源真实性。

### 6. 旧 HTTP 页面还可能受字符集边界影响

当前正文只接受 UTF-8、US-ASCII。部分 legacy HTTP 页面声明 GB2312/GBK、Big5 或
Windows-1252，即使 scheme 放开仍会在 decode 阶段失败。这是独立的兼容性问题，不能用
lossy UTF-8 或无限 charset detector 顺手掩盖。

第一切片先交付 public HTTP 和正确 provenance。只有 credential-free 小语料证明声明字符集
造成重复失败时，才启动后续 charset 切片：优先使用 WHATWG label 映射和确定性 declared
charset/BOM，不做无界猜测。

### 7. 缺失 Content-Type 与非标准端口仍是已知边界

部分旧站响应头不规范，当前会拒绝缺失 Content-Type；公开 HTTP 也可能位于非 80 端口。
这些不能与 scheme 修正混在一个切片：

- W1.1 保持当前 content-type fail-closed；只有真实重复 loss 才设计有界 sniffing；
- W1.1 默认只准 `http:80`，避免把新能力变成 public port scanner；
- HTTPS 保持 current port 行为，避免无证据回归；
- 若未来需要公开 HTTP 非标准端口，必须有用户/TaskContract exact host:port authority，不由模型
  自行扩大。

## 接受的合同

### 1. URL 与地址边界

- 只允许 `http`、`https`；继续拒绝 `file`、`ftp`、`data`、`javascript`、`ws` 等 scheme；
- HTTP 只允许默认端口 80；显式或隐式端口统一正规化后判定；
- URL 继续拒绝 username、password 和其他 userinfo；
- loopback、private、link-local、carrier-grade NAT、documentation、benchmark、multicast、
  reserved、IPv4-mapped IPv6 和云 metadata 地址继续 fail closed；
- 每一跳解析全部 DNS 地址；任意一个地址不合法则整跳拒绝；
- client 只连接已经校验并 pin 的地址，不读取系统 proxy；
- FullAccess 不绕过 SSRF/metadata/downgrade/port 门。

公开 HTTP 与 ApplicationProbe 的 loopback HTTP 是两个能力：`web_fetch` 永远不能借本 ADR
访问 localhost、局域网或 worktree 服务。

### 2. 请求边界

- 仍只执行 GET；
- 不携带 Cookie、Authorization、Referer、用户 Chrome session 或 caller 自定义 header；
- 不接受 proxy、certificate bypass、client certificate 或任意 method/body；
- 保持 manual redirect、5 跳、15 秒、raw/decompressed body 和正文/链接上限；
- 不执行 script、不加载 subresource、不保存 Cookie/storage。

### 3. 传输单调性

fetch trajectory 维护 `secure_transport_seen`。初始 HTTPS 设为 true；HTTP 链一旦进入 HTTPS 也
设为 true。`secure_transport_seen=true` 后任何 HTTP target 在 DNS 或 connect 前以 typed
`web_transport_downgrade` 拒绝。

HTTP -> HTTPS upgrade 不是证书绕过：TLS 错误继续是 transport failure，不回退 HTTP。

### 4. 输出与 replay

成功结果在现有字段外增加：

```text
final_transport_scheme = http | https
final_transport_security = plaintext | tls_authenticated
transport_trajectory = plaintext_exposed | tls_only
transport_integrity = unprotected | tls_protected
redirect_count
transport_upgraded = true | false
trust = external_untrusted
```

字段语义：

- `trust` 表示网页永远没有系统/用户指令权；
- `final_transport_security` 只描述 final response 的链路；
- 只要初始请求或任一 redirect hop 是 HTTP，`transport_trajectory` 就是
  `plaintext_exposed`、`transport_integrity` 就是 `unprotected`；HTTP→HTTPS 的最终响应虽然
  有 TLS，也不能抹掉初始明文跳可能被改写的事实；
- `transport_integrity` 不声称发布者、事实或语义可信；
- `source_sha256` 是 received-content/replay identity，不是 publisher signature；
- HTTP 失败 outcome 也投影 requested/final URL、scheme、failure stage/code 和
  `external_untrusted`，不得把原始页面文本塞进错误。

committed ToolOutcome 继续由 RunStore 原样重放，reopen/continue 不重新访问 HTTP(S)。不升级
RuntimeEvent 或 State schema，除非现有 JSON ToolOutcome 无法无损表达上述字段；按 current
事实它可以表达，因此预期 protocol/state delta=0。

### 5. 模型与权限可见合同

工具 schema 不增加 `allow_insecure`、header、Cookie 或 policy 参数。`url` 本身决定 scheme，
Host 决定能否执行。catalog description 改为“已知公开 HTTP(S) URL”，并明确 HTTP 明文、所有
内容 external-untrusted。

authorization audit 使用两个稳定 reason：

```text
public_https_web_fetch
public_plaintext_http_web_fetch
```

两者都复用现有网络 permission；Ask 和 sandbox-no-network 仍 fail closed。HTTP 可以在 Agent
mode 读取，但必须以 plaintext provenance 进入 outcome，不能伪装成 HTTPS。

## 实现切片 W1.1

### 真实用户变化

以前用户给出公开 HTTP URL 时 DSE 固定拒绝；完成后 root Agent 能安全读取默认端口 80 的公开
HTTP HTML/UTF-8 文本，知道它是明文、不可信来源，并在 redirect、SSRF 与 reopen 上保持
canonical 行为。

### 单一 owner 与最小改动

owner 为 `crates/tools`。按以下顺序完成一条纵向切换：

1. 在 `web_fetch.rs` 将 URL scheme gate 改为 public HTTP(S)，引入 scheme-specific port 和
   monotonic redirect contract；
2. 将受限 reqwest client 从 HTTPS-only 改为 HTTP(S)，保留 no-proxy、no-cookie、no-referer、
   manual redirect、DNS pin、size/deadline；
3. 增加 transport provenance 字段，澄清 hash 语义；
4. 让 canonical link extraction 保留语法合法的 public HTTP(S) links；真正 fetch 时仍重新
   执行 DNS/connect gate；
5. 更新 authorization reason、catalog description 和 network fingerprint；
6. 迁移真实 `AgentApplication -> AgentRuntime -> ProductionToolExecutor -> ToolOutcome ->
   RunStore` caller fixture；
7. 更新 ADR-0015 被取代条款、Current Architecture、Roadmap 当前顺序和 Evaluation 合同；
8. 调用方和 fixture 切换后删除 `rustls_pinned_public_https_v1`、只允许 HTTPS 的错误文案和
   “HTTP 必须 web_scheme_denied”旧断言。

不新建 crate、HTTP Provider、session、cache、policy Store 或 transport adapter。

### 必须覆盖的确定性矩阵

| 类别 | 用例 |
|---|---|
| scheme | HTTP 80 成功、HTTPS 成功、其他 scheme 拒绝 |
| port | HTTP 80 成功、HTTP 非 80 typed deny |
| redirect | HTTP→HTTP、HTTP→HTTPS、HTTPS→HTTPS 成功；HTTPS→HTTP 和升级后降级拒绝 |
| SSRF | HTTP/HTTPS literal、DNS、mixed answers、redirect、IPv4/IPv6 metadata 全部拒绝 |
| connection | scheme-specific port、all-address validation、connect pin、no proxy |
| authority | Ask deny、Agent/FullAccess public allow、Writer sandbox-no-network deny |
| request | 无 Cookie/auth/referer/custom header/body；GET only |
| bounds | redirect、deadline、raw/decompressed bytes、content type、charset、links/text truncation |
| provenance | HTTP/HTTPS 字段正确，hash 不宣称 publisher authenticity |
| replay | committed HTTP outcome reopen 不二次请求，failure 也 typed/replayable |
| catalog | root 可见、actor parity、tool description 与真实 scheme 一致 |

### 真实 transport canary

确定性门通过后最多执行一次 credential-free、maximum-reruns=0 的已知公开 HTTP URL canary，
只验证 `SystemWebFetchNetwork` 实际能发出 HTTP 并提交 provenance。它不读取 DeepSeek Key、不
调用模型、不形成成功率或效率声明。外部站不可用时记录 transport state，不为让 canary 变绿
放宽 SSRF、端口、content-type 或 downgrade 门。

### 验证门

```text
cargo fmt --all -- --check
cargo test -p dse-tools --locked web_fetch
cargo check -p dse-tools --locked
targeted dse-app production caller/reopen fixture
./scripts/dev-dse.sh focused
git diff --check
```

只在 pre-integration revision 运行一次 workspace strict Clippy/full tests。W1.1 是确定性工具
能力修正，不做多 cell Prompt A/B、不读取官方模型 Key；计费字段与本切片无关。

### Keep/delete 门

仅在以下条件全部成立时保留：

- public HTTP 真实可读；
- private/metadata/redirect escape 与 HTTPS downgrade false allow=0；
- HTTP 结果从模型、人类与 replay 都明确标为 plaintext/external-untrusted；
- production caller/reopen 通过且没有第二 HTTP truth；
- 旧 HTTPS-only identity、文案、断言和 authority reason 已删除；
- production delta 集中在 owning module 与必要 caller/docs，没有 browser/search/Runtime 重构。

任一安全门不能闭合时，回滚整个 HTTP candidate，保留已证明的 HTTPS `web_fetch`；不留下
disabled flag、双 client 或长期 adapter。

## 后续但不在 W1.1 实施

只有真实 corpus 证明 W1.1 仍反复失败时，按顺序考虑：

1. **W1.2 declared charset**：BOM、Content-Type 和有界 HTML meta 的 WHATWG label 解码；
2. **W1.3 bounded content sniffing**：只为缺失/错误 Content-Type 的文本证据设计，默认仍
   fail closed；
3. **exact non-default port grant**：由用户/TaskContract 限定 host:port，不能由模型参数授权。

每项都是独立 keep/delete 候选。它们不阻塞 W1.1，也不授权 M45/M46、浏览器动作、视觉或
Firecrawl。

## 执行节奏

当前正在执行 ADR-0016 的同一开发任务先完成并提交当前 Goal。随后在同一任务创建 W1.1 Goal；
不得把 public HTTP 改动混入认知控制面 commit，不得并行启动新任务。W1.1 clean checkpoint 后
才恢复 Roadmap 的 M45 ApplicationProbe 顺序。
