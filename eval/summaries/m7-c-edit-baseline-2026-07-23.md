# M7-C canonical 编辑能力基线与 FIM 准入结论

- 复核日期：2026-07-23
- 起始 clean revision：`afb9b0abcc4a`
- 冻结基线：`c4972130`
- canonical tools 修复：`7613073c`
- 非 canonical CLI/TUI 路径删除：`639f1e8b`、`9cba8b53`
- 当前 production actor catalog 冻结：`6a27fb85`
- 最终离线 Harness revision：`9cba8b53f02d9d457575711ecfe6a08342334d83`
- 结论后只读复核与残余 correctness 修复：`1cb65b82`
- manifest：`eval/manifests/m7-c-edit-baseline-v1.json`
- manifest canonical-content SHA-256：`4d5457db2e7fc075fbecf925f89d45ea412a9aace515da1b4cfb0b3a03755c55`
- manifest raw-file SHA-256：`24ecf8252c192136edfd122919a0a5aab5a0c42ae9a9799f808febc26e825277`
- 结论：**keep canonical tool correctness；hold FIM；live A/B 为
  `inadmissible_no_surface_delta`**

## 1. 本切片回答的问题

M7-C 没有把“接入 FIM”作为成功条件。真实问题是：当前 production
`apply_patch`/`edit_file` 的失败究竟来自模型难以生成编辑，还是 Host 自身会错误接受、
错误定位或不完整回滚编辑；只有前者且存在可归因 treatment surface 时，才值得增加
canonical FIM 生命周期。

只读调用图确认唯一生产链仍是：

```text
exec / app-server / interactive TUI
  -> AgentApplication
  -> AgentRuntime
  -> ProductionToolExecutor (crates/tools)
  -> RunStore
```

root、read-only child 与显式 isolated Writer 使用同一个 `AgentRuntime`。固定 Host schema
和执行 owner 在 `crates/tools`；DeepSeek planner、transport 与 accounting owner 在
`crates/deepseek`。没有第二个编辑器、Runtime、Store、工具目录或模型循环进入 production。

## 2. 官方 DeepSeek 协议复核

2026-07-23 重新核对了以下官方一手资料：

- [FIM Completion 指南](https://api-docs.deepseek.com/guides/fim_completion/)
- [Completions / FIM API reference](https://api-docs.deepseek.com/api/create-completion/)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)
- [Tool Calls 与 Strict](https://api-docs.deepseek.com/guides/tool_calls/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/)

复核边界：FIM 是 `https://api.deepseek.com/beta` 下独立的 `/completions` Beta surface，
请求是 `prompt` 加可选 `suffix`，响应正文在 `choices[].text`；它不是 Chat Function Calling，
也不是 Strict 的延伸。指南当前声明 FIM 最大输出 4K。Thinking tool-call turn 的
`reasoning_content` 必须在后续请求中完整回放。Strict 则要求请求内每个 function 都设为
strict，且服务器会校验整份 schema。

易变事实不能由常识猜测：当前 Completions reference 只列 `deepseek-v4-pro`，但同日 pricing
feature table 把 FIM 标为 `deepseek-v4-flash` 和 `deepseek-v4-pro` 均支持且仅限
non-thinking。模型名、limit、Beta 行为和价格若未来进入 production，必须重新复核并冻结
fixture；本切片没有用该不一致强行构造 treatment。

## 3. 失败矩阵与根因

审计覆盖了用户要求的完整矩阵。结论不是“patch 已经足够好”，而是现有 Host 存在可由
确定性反例证明的 correctness 缺陷：

| 失败类别 | 起始事实 | M7-C 处理 |
|---|---|---|
| malformed/missing/extra arguments | Runtime 已有 typed invocation rejection | 保持唯一 `ToolOutcome`，补 production 回归 |
| patch parse / hunk count | parser 会接受声明行数与正文不一致 | 写前拒绝 |
| context mismatch / drift | fuzzy 搜索取第一个候选，重复块可错位 | 多候选时 `ambiguous_edit`，不写盘 |
| stale prior read | `edit_file` 只比较长度与 mtime | 绑定读取字节 SHA-256，publish 前再次校验原字节，再原子替换；不宣称线性化 CAS |
| search not found | 已 fail closed | 保持 |
| ambiguous/non-unique search | 重叠匹配会被误判唯一 | 重叠感知计数并拒绝 |
| identical/no-op | `apply_patch` 可接受无变化结果 | 写前拒绝 |
| path escape/permission/scope | 已有 workspace 边界 | 保持并纳入矩阵 |
| create/delete/rename/duplicate | rename 未实现却可能被解析；重复目标可顺序覆盖 | rename 与重复 resolved target 均写前拒绝 |
| multi-file rollback | 普通失败 best-effort rollback，rollback error 被吞 | 普通失败恢复原字节；rollback 不完整显式失败 |
| CRLF/Unicode/multibyte | patch 有现有支持，需冻结 | byte-exact 回归；编辑仍只支持 UTF-8 |
| operation/transport/side-effect ambiguous | canonical typed axes 已存在 | 保持，不自动盲重试 |
| verifier/terminal rejection | Host 已要求 latest revision evidence | production loopback 冻结失败后修正与 SQLite reopen |
| crash windows | Runtime 能区分 Prepared/Started/Outcome | 进程级 SIGKILL 门禁；单文件原子替换只出现 old/new |

另有一个 metadata 缺陷：原子替换会丢失原文件 mode。M7-C 现在保留原 permissions。

历史 `eval/results`/raw 的 30 个 JSONL 文件只提供方向性事实：157 个 run manifest、158 次
patch 调用、1 次记录的 patch failure、138 次 verified success、4 次 false success。它们
跨 schema 和 revision，不能作为正式成功率或 FIM 收益基线。低表面失败率尤其不能否定上述
确定性 false-accept / wrong-target 反例。

## 4. 冻结任务与可复现结果

manifest 冻结 12 个任务目标：唯一替换、ambiguous 恢复、stale 重读、多 hunk、失败时
多文件回滚、create/delete、CRLF/Unicode、context drift、verifier 失败后修正、negative
no-change、read-only child handoff、显式 Writer isolate/verify/integrate/cleanup。
`maximum_reruns=0`，全部在真实临时 Git repository、deterministic verifier 或 production
loopback 上运行；Harness 只投影 Rust process verdict，不复制 patch parser、编辑器或失败分类。

在起始 production 逻辑上，12 个冻结 tools contract assertion 只有 3 个通过，9 个失败。
失败项分别是 same-length/same-mtime stale、重叠 ambiguous search、mode 保留、重复目标、
rename、hunk count、no-op、ambiguous fuzzy placement 和多文件失败全量回滚。该 3/12 ->
12/12 是确定性 Host correctness 变化，不是模型任务成功率 A/B。

最终 Harness 在 clean `9cba8b53` 上执行 8/8 gates：

1. `codewhale-tools m7c_`：14 passed、1 external crash helper ignored；
2. `codewhale-app m7c_`：真实 `AgentApplication -> AgentRuntime -> tools -> SQLite` loopback，
   verifier 先失败、修正后只以 latest revision receipt 完成，并可重开；
3. read-only child exact lifecycle；
4. isolated Writer verify/integrate/cleanup；
5. `ToolPrepared` 后 SIGKILL/reopen；
6. side-effect started 后 SIGKILL 不重放；
7. `ToolOutcomeCommitted` 后 SIGKILL 不重放；
8. app-server 进程 SIGKILL/replay，无 credential。

Harness 事实：source before/after 都是
`9cba8b53f02d9d457575711ecfe6a08342334d83`，tree
`41b0a6baa400b7192af5a1f8962c7b81de008c04`，dirty false，API requests 0，credential read
false。临时 0600 result SHA-256 为
`ae6897d77270133593553db76640696178f8c61cfda3fd2dcfea37d6c830bfb0`；按清理合同不提交。

完整离线门禁通过：Harness self-test、owning crate tests/check、production loopback、
process crash/reopen、`focused`、`cargo fmt --check`、workspace clippy `-D warnings`、workspace
tests 和 `git diff --check`。

## 5. 实现、替代与删除

唯一 owning module 是 `crates/tools`：

- `edit_file` 的 read freshness 改为 exact-byte digest，发布前再次校验原字节，再原子替换；
- 精确搜索使用重叠感知唯一性，原子替换保留 permissions；
- `apply_patch` 在任何写入前拒绝 duplicate target、rename、hunk count mismatch、no-op 和
  ambiguous fuzzy placement；
- multi-file 普通错误回滚不再吞 rollback failure，创建的空父目录也按精确记录清理；
- schema 增加实际参数边界，中文成功/失败投影仍来自 canonical outcome；
- 当前 writable actor catalog hash 已重冻，历史 M7-B manifest 保持不可变。

cutover 物理删除了：len+mtime freshness、first-match fuzzy placement、未使用的工具 capability
抽象、CLI `codewhale apply` 的直接 `git apply` 绕行、TUI-local `codewhale eval` 简化编辑器及
其剩余 acceptance/说明。保留的 TUI tool-lifecycle acceptance 走 canonical `exec`。

从 `afb9b0ab` 到代码 checkpoint `9cba8b53` 为 25 files、`+1,424/-2,048`；没有新增 Cargo
依赖、模型可见工具、Runtime、Store、Provider、用户模式或 compatibility reader。

### 5.1 结论后复核与勘误

在不改写上述 frozen manifest、历史 Harness result 或 3/12 -> 12/12 结论的前提下，六个
只读审计再次检查了 tools、DeepSeek、Runtime/State、真实 caller、Harness 与复杂度。
`1cb65b82` 关闭了四个残余的写前 correctness 反例：

- `changes` 不再静默忽略仅属于 patch 的 `path`、`fuzz` 或 `create_if_missing`；
- `path` 不得覆盖 `/dev/null` create/delete header，避免把创建/删除语义降成普通替换；
- delete-to-null 必须实际移除完整内容，create-from-null 遇到已有目标必须拒绝；
- checked create 使用 no-clobber publish，竞态中的外部创建不会被覆盖；canonical preflight
  保留 `changes` 语义错误的 `invalid_field`，而不是误报 `patch_parse`。

模型可见 `apply_patch` 描述现在明确：单文件逐个原子 publish，多文件普通失败回滚，但跨
文件 crash window 不是事务；`path/fuzz/create_if_missing` 只属于 `patch`。当前 root
headless、root interactive 与 isolated Writer 目录 hash 随这一真实 wire identity 更新，历史
M7-B manifest 仍保持不可变。定向门禁为 13 个 M7-C apply-patch 用例、2 个 checked-publish
用例、canonical preflight 与 actor catalog identity 全部通过；完整 tools crate 为 347 passed、
1 ignored external helper，集成测试另 1 passed。

这里同时纠正两处证据口径：

- 对已有文件，当前跨平台原语是“读取 expected bytes -> 原子 rename replacement”，不是把
  内容 predicate 与 rename 合并为同一线性化 compare-and-swap。未遵守 CodeWhale 单 Writer
  契约的外部进程仍可能在两步之间竞争；Runtime 在 Started 后继续 fail closed，不盲重放。
  删除同样是 byte precondition 后 remove。真正消除该窗口需要新的 durable operation 设计，
  不能靠改名为 CAS 或 advisory lock 声称完成。
- frozen manifest 的 E10 把 no-op 的 verifier 写成 tree 和 generation 均不变；该断言只由
  direct tools mechanism 覆盖。canonical Runtime 中任何已经 Started 的 `MayWrite` 都会令
  workspace generation 前进，即使 outcome 是 `NotApplied` 且 revision 未变。历史 manifest
  为保持可复核身份不重写；后续 Runtime 级任务必须按“revision 不变、generation 前进”判定。

## 6. FIM 与 live A/B 决策

`crates/deepseek` 现有 `plan_fim` 只有 request-planning 和 accounting 基础。production
transport 仍只拥有 Chat URL，现有完整响应 parser 读取 Chat `choices[].message` 而不是 FIM
`choices[].text`；RuntimeEvent/RunStore 也没有 Host-owned、revision-bound FIM edit
lifecycle。仓库没有真实 canonical FIM caller。

因此 control 与所谓 treatment 在同一 immutable binary 上没有 surface delta。按照预注册
准入规则，live A/B 在 credential/API 前判定 `inadmissible_no_surface_delta`：0 arms、
maximum reruns 0、official API requests 0、credential read false、product metric
ineligible。`key.txt` 未读取，也没有价格或成功率 claim。

这不是“FIM 无效”的结论。它只说明现有证据把主要可修复瓶颈定位到 Host correctness，且
本切片没有足以公平归因的 FIM production treatment。为制造 A/B 而加入半条 parser 或
模型可见同义工具会违反 canonical lifecycle 和复杂度门槛。

## 7. 保留、推迟与非结论

- **keep**：canonical tools correctness 修复、冻结 manifest/Harness、typed failure/recovery、
  current catalog identity。
- **hold**：现有低复杂度 FIM planner/accounting 基础；不接入 production caller，不默认
  启用，不执行无 surface delta live A/B。
- **reject/delete**：直接 `git apply` CLI、TUI-local eval/edit loop、旧 acceptance、第二编辑
  路径、schema 弱化、第二目录/Runtime/Store、无消费者 FIM production 分支。

本切片不证明真实 DeepSeek 的 verified success、Token、wall time 或 API cost 得到改善；
也不证明 FIM 相对 patch 更好或更差。12/12 只证明已列出的确定性 Host 反例关闭，历史统计
只作方向性证据。

已知边界：

- 单文件 rename-based replacement 是原子的，但 expected-byte check 与 rename 不是一个
  线性化 CAS；多文件 publish 也不是跨文件 crash-atomic。普通错误
  会回滚，Started/Outcome crash 窗口由 Runtime fail closed 为 recovery required 而不重放；
- 只保留 permissions/mode，不声称保留 ownership、xattr 或 ACL；
- freshness digest 是单次 Runtime context 内的 read fact；Prepared 前后进程丢失 read cache
  会安全拒绝并要求重读，可能牺牲恢复率；
- `apply_patch`/`edit_file` 仍是 UTF-8 文本编辑器。

下一切片应先从新的 production failure 样本判断剩余瓶颈。只有当 patch 生成失败或恢复轮次
成为主要损失，且能在同一 binary 冻结 Host-owned FIM treatment 时，才实现完整垂直路径：
fresh read + revision/prefix/suffix digest、官方 `/beta/completions` parser/accounting、边界校验、
atomic apply、typed crash/retry、RunStore replay 和同任务 A/B。若真实频率指向多文件 crash
歧义，则优先建立 operation-specific durable transaction facts，而不是引入 FIM。
