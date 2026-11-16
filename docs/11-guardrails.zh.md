# 守护策略

[English](11-guardrails.md) | [中文](11-guardrails.zh.md)

状态：v1.7.0 是当前已发布版本，于 2026-11-16 从标签 `v1.7.0` 发布；1.6.0 为上一版本。1.7.0 发布将 MCP 工具面扩展为 10 个类别共 62 个工具并新增 18 个工具；`browser_screenshot` 与 `page_pdf` 对守护策略是只读的，但其像素与 PDF 字节不会被脱敏、边界包裹或检查。扩展声明 11 项权限（含 `tabGroups`）、42 个基于 debugger 的处理器与 `all_frames: false`；四个标签页与窗口工具仅扩展可用。QA 验收在发布提交 `f12930c` 之前的 HEAD `d804c33` 上通过；本地 Windows 打包单独通过，已发布产物由 CI 重新构建。标签 `v1.7.0` 的对象 `84d7c086147e96ff58531198b6ead55c65ff116f` 指向 `f12930c145c8cac05a50db9702ab3f1ad021c666`；主 CI 运行 43795994581 通过八个任务，发布工作流 43796443828 成功，GitHub 发布 446568768 发布了产物。守护策略实现在 1.6.0 工作树 HEAD `61ca85f` 上通过正式 QA（237 个 workspace 测试、137 项一致性检查、十个文件中的 55 个 Vitest 测试与 Playwright 1/1；一个低危 `browser_type` 通道发现，无高危或中危）。读者：运维与 MCP/REST 客户端作者。

## 1. 概览

Corvus 可在守护进程启动时加载一个可选的本地策略文件。该策略限制可运行的工具、把导航限制到选定主机、阻止字面私有网络目标、对 cookie 值进行脱敏，并把页面内容标记为不可信。它是显式启用的：没有策略文件时，守护进程保持原有行为。

策略由 `corvusd` 在工具调用分发前执行，并在导航结果返回时再次检查。它不能替代认证、网络出口控制或操作系统隔离。

## 2. 策略文件与 schema

文件必须是合法 JSON，最大 65,536 字节，且为不含未知字段的对象。`version` 必填，且必须正好为 1；其余字段均可选。

| 字段 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `version` | 整数 | 必填 | 必须为 1。 |
| `read_only` | 布尔 | `false` | 拒绝目录中所有非只读工具。 |
| `allow_tools` | 字符串数组 | `[]` | 非空时只允许这些工具名，除非只读规则或拒绝列表先作出决定。 |
| `deny_tools` | 字符串数组 | `[]` | 这些工具名始终被拒绝。 |
| `allowed_hosts` | 字符串数组 | `[]` | 非空时，导航必须指向匹配条目的 http 或 https 主机。 |
| `block_private_network` | 布尔 | `false` | 拒绝字面回环、私有、链路本地与本地名称主机。 |
| `redact_secrets` | 布尔 | `false` | 对 `browser_cookies` 与 `storage_get` 结果中的值脱敏。 |
| `content_boundary` | 布尔 | `false` | 用标记行包裹不可信页面文本与 HTML。 |

空的 `allow_tools` 表示不限制允许列表。空的 `allowed_hosts` 表示不限制主机。工具名必须存在于 1.7.0 发布的 62 工具、10 类别目录中；上一版本 1.6.0 为 43 个。未知名称会拒绝启动。同一名称不得同时出现在 `allow_tools` 与 `deny_tools` 中。

主机条目要么是精确的小写主机（例如 `example.com`），要么是前导通配后缀（例如 `*.example.com`）。模式拒绝空白、`/`、`:`、`?`、`#`、`@`、内嵌 `*`、前导或尾随点以及双点；字符仅限 ASCII 字母、数字、点、连字符与下划线。端口被忽略。`*.example.com` 匹配裸域 `example.com` 及其子域，但不匹配 `api.example.com.evil.test`。

## 3. 启动与校验

通过 `--guardrails <PATH>` 传入文件，或设置 `CORVUS_GUARDRAILS=<PATH>`。两者同时存在时，命令行选项优先。

```bash
corvusd serve --guardrails /etc/corvus/guardrails.json
```

任何加载或校验错误都是致命的：守护进程拒绝启动，并报告 invalid-input 错误。失败类别包括：

- 文件不可读或 I/O 错误；
- 文件超过 65,536 字节；
- JSON 无效、存在未知字段，或 `version` 错误；
- 工具名未知、同一工具同时出现在允许与拒绝列表，或主机模式无效。

启动时守护进程记录守护策略是否启用，以及列表计数与开关状态；它不会记录策略正文。

## 4. 优先级

对工具调用，检查按以下顺序执行，第一个命中即返回拒绝：

1. **只读。** 若 `read_only` 为 true 且工具的目录条目不是只读，则以 `read_only` 原因拒绝。
2. **允许列表。** 若 `allow_tools` 非空且工具不在其中，则以 `allowlist` 原因拒绝。
3. **拒绝列表。** 若 `deny_tools` 包含该工具，则以 `deny_list` 原因拒绝。

由于启动时会拒绝允许/拒绝列表重叠，只有当非只读工具被加入允许列表且 `read_only` 同时为 true 时顺序才有影响：只读规则优先。

URL 门禁独立于工具优先级。它在分发前检查 `browser_navigate` 与 `tab_new` 的参数，并在 `browser_navigate`、`tab_new` 或 `browser_history` 成功后检查返回的 `url`。

## 5. 示例

### 5.1 只读

用于观察与测试的只读守护进程：

```json
{
  "version": 1,
  "read_only": true
}
```

每个动作工具都会在分发前以 `read_only` 原因拒绝；目录标记为只读的工具（例如快照、页面文本读取、`page_links`、`page_forms`、`page_accessibility`、`window_list`、`storage_get`、`health`、`version`、`page_pdf` 与 `browser_screenshot`）仍然可用。该策略不会让只读工具的结果自动可信，也不会检查截图像素或 PDF 字节。

### 5.2 主机允许列表与私有网络阻断

把导航限制到两个域并拒绝本地目标：

```json
{
  "version": 1,
  "allowed_hosts": ["example.com", "*.example.org"],
  "block_private_network": true
}
```

`https://example.com/a` 与 `https://docs.example.org/b` 允许。`https://api.example.com.evil.test/` 以 `host_not_allowed` 拒绝，因为后缀伪装不匹配 `example.com`。`http://127.0.0.1:8080/`、`http://localhost/`、`http://[::1]/` 以及私有 IPv4 或 IPv6 字面量以 `private_network` 拒绝。`about:blank` 始终允许。

`allowed_hosts` 非空时，非 http(s) URL 会被拒绝。重定向到被拒主机的导航会被分发后的 URL 检查捕获，并返回原因为 `redirect_not_allowed` 的 `POLICY_DENIED`；此时浏览器可能已经跟随了重定向。

### 5.3 不可信内容与脱敏

把页面输出标记为数据并隐藏 cookie 值：

```json
{
  "version": 1,
  "redact_secrets": true,
  "content_boundary": true
}
```

启用 `redact_secrets` 后，`browser_cookies` 结果顶层 `cookies` 数组中每个含 `value` 字段的对象都会得到 `value: "[redacted]"` 与 `redacted: true`，`storage_get` 结果的每个条目也会得到 `value: "[redacted]"` 与 `redacted: true`。不含 `value` 的对象保持不变。其他工具的值以及通用密钥扫描不在覆盖范围内。

启用 `content_boundary` 后，`page_text` 的 `text`、`page_html` 的 `html` 与紧凑 `page_snapshot` 的 `text` 会用标记行包裹。JSON 快照不受影响。标记在导航结果检查与 cookie 脱敏之后应用。

## 6. 执行与错误

`crates/corvusd/src/lib.rs` 的 `RouterBackend::call` 在分发前先检查工具策略，再检查导航参数。被拒调用会被记录，并返回错误而不触达浏览器。`browser_batch` 检查每个步骤、应用每个结果，默认在第一个被拒步骤停止；批次不能绕过嵌套步骤的策略。`session_replay` 检查每个计划或实况步骤，并把被拒的回放步骤记录为 reason 为 `policy_denied` 的跳过项。

拒绝使用第 11 个错误码 `POLICY_DENIED`，不可重试。MCP 返回一行错误文本，带 `isError: true` 且没有 `structuredContent`；details 为内嵌的紧凑 JSON：

```text
POLICY_DENIED: tool browser_navigate denied by guardrails policy (allowlist) {"reason":"allowlist","tool":"browser_navigate","policy":"/etc/corvus/guardrails.json"}
```

REST 返回 HTTP 403，错误体嵌套在 `error` 下，内容相同：

```json
{"error":{"code":"POLICY_DENIED","message":"navigation denied by guardrails policy (host_not_allowed)","retryable":false,"details":{"reason":"host_not_allowed","url":"https://example.net/","policy":"/etc/corvus/guardrails.json"}}}
```

URL 拒绝消息为 `navigation denied by guardrails policy (<reason>)`，携带 `details.reason`、`details.url` 与 `details.policy`。工具拒绝消息为 `tool <name> denied by guardrails policy (<reason>)`，携带 `details.reason`、`details.tool` 与 `details.policy`。分发后的拒绝仍返回 `POLICY_DENIED`，但导航已经发生。

## 7. 脱敏与内容边界

Cookie 与存储脱敏刻意保持狭窄：`browser_cookies` 只处理顶层 `cookies` 数组；`storage_get` 只处理顶层条目；只修改已含 `value` 的对象。其他工具、嵌套值、请求头与页面文本都不会被脱敏。

内容边界使用以下精确标记：

```text
<<<CORVUS UNTRUSTED PAGE CONTENT>>>
<<<END CORVUS UNTRUSTED PAGE CONTENT>>>
```

若载荷已包含任一标记，会先转义为 `<<<CORVUS_UNTRUSTED_PAGE_CONTENT_ESCAPED>>>` 或 `<<<END_CORVUS_UNTRUSTED_PAGE_CONTENT_ESCAPED>>>`。最终值恰好有一个前导起始标记与一个尾随结束标记，载荷位于两者之间。

密码掩码独立于守护策略，且对快照始终启用：密码输入值渲染为 `***`。`browser_type` 因通道而异：扩展对密码字段返回 `masked: true`，其他输入返回截断的 `value`；CDP 不返回 `value`，也不返回 `masked`，因此不会返回输入值。JSON 快照按设计仍不会被内容边界包裹。

## 8. 健康检查

启用时，守护进程 `/health` 文档在 `health` 下包含 `guardrails` 对象。REST `/api/v1/health` 返回 `{ok, version, health: {..., guardrails: {...}}}`。该对象使用固定键与计数，绝不包含工具名或页面数据：

```json
"guardrails": {
  "enabled": true,
  "read_only": false,
  "allow_tools": 0,
  "deny_tools": 3,
  "allowed_hosts": 2,
  "block_private_network": true,
  "redact_secrets": true,
  "content_boundary": true
}
```

未配置策略时，`enabled` 为 false，三个列表计数字段为 0，四个开关为 false。该健康对象可安全暴露给本地监控系统。

## 9. 限制

- 导航门禁不是网络沙箱。允许的页面加载后，页面脚本仍可请求被拒来源，子资源也不会被 `allowed_hosts` 过滤。网络隔离应使用出口控制。
- 门禁从不解析 DNS。通过模式的主机名仍可解析到私有地址，DNS 重绑定不会被检测。
- 即使开启 `redact_secrets`，`browser_eval`、`page_html` 与 `browser_history` 仍可能暴露密钥。脱敏只覆盖上述 cookie 值。
- `page_html` 按原样返回粘贴属性；粘贴的 HTML 属性不会被清洗。
- JSON 快照不会被内容边界包裹；只有紧凑快照文本、`page_text` 的文本与 `page_html` 的 HTML 会被包裹。
- REST 认证账号与刷新令牌保存在内存中；守护策略不增加持久身份或认证。
- `details.policy` 会在守护进程响应中暴露本地策略路径。扩展界面会隐藏它，但直连 MCP/REST 客户端会收到。
- 被拒的 `browser_batch` 步骤是错误，默认会停止批次；`session_replay` 会把被拒步骤记录为跳过而非执行。
- 只读是目录分类。依赖某工具前请查阅 [09-tools.zh.md](09-tools.zh.md) 中该工具的当前分类。
- 不宣称任何 1.7.0 实机部署，NFR-3 仍未通过。1.7.0 发布有 10 个类别共 62 个工具；`browser_screenshot` 与 `page_pdf` 对守护策略是只读的，但截图像素与 PDF 字节不会被脱敏、边界包裹或检查。四个标签页与窗口工具仅扩展可用。QA 验收在发布提交 `f12930c` 之前的 HEAD `d804c33` 上通过；本地 Windows 打包单独通过，所有已发布产物与 CI 结果记录在发布说明中。`page_pdf` 强制 32 MiB 解码上限；实机上限未触及，最大实机 PDF 为 55,891 B。

## 10. 相关文档

- [09-tools.zh.md](09-tools.zh.md)：工具输入、结果与错误码。
- [02-architecture.zh.md](02-architecture.zh.md)：执行点与守护进程结构。
- [05-deployment.zh.md](05-deployment.zh.md)：服务配置与健康检查。
- [release-notes/v1.6.0.md](release-notes/v1.6.0.md) 记录 1.6.0 发布证据，[release-notes/v1.7.0.md](release-notes/v1.7.0.md) 记录 1.7.0 发布。
