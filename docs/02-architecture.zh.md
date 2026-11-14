# Corvus 架构

[English](02-architecture.md) | [中文](02-architecture.zh.md)

状态：M1 已实现，M2 进行中。读者：工程实现者。

## 1. 约束设计的硬条件

1. **必须驱动用户正在使用的那个浏览器。** 不重启、不建独立 profile、不加远程调试开关。因此纯 CDP 方案被排除：扩展是唯一的入口。
2. **零审批、零配对、零页面。** 任何需要人点一下的流程都算缺陷。
3. **连接就是产品。** MV3 的 Service Worker 空闲约 30 秒即被回收，设计必须假定浏览器一侧对长连接是敌意的。
4. **多浏览器、多 profile、并发。** 一个守护进程、N 个扩展、互相独立的会话。
5. **一切可观测。** 每条命令及其页面上下文事后都必须能回放。
6. **守护进程是单个静态二进制。** 无运行时依赖、无安装前置条件。

## 2. 容器

    agent 客户端（Claude Code / Cursor / DSH / Cline / Copilot）
        |  MCP：stdio 或 streamable-http（JSON-RPC 2.0）
        v
    +--------------------------------------------------------------+
    | corvusd                                                      |
    |  mcp-server  -> 工具注册表 -> 路由器 -> 会话运行时              |
    |  传输中枢（ws / native-messaging / long-poll）                  |
    |  录制器（sqlite + jsonl + trace）                              |
    |  站点记忆（sqlite）   指标（prometheus）   健康检查             |
    +--------------------------------------------------------------+
        |  分帧 JSON 信封，走 WS（主通道）
        v
    扩展（MV3）：Service Worker + 内容脚本 + offscreen 保活
        |  chrome.tabs / scripting / debugger / webRequest / downloads
        v
    浏览器

进程模型：每个用户会话一个 corvusd（Windows/macOS/Linux 上注册为服务），每个浏览器 profile 一个扩展，另有一个 CLI 客户端 corvus。

## 3. crate 划分

| crate | 职责 |
|---|---|
| corvus-proto | 线格式：信封、命令、响应、错误分类、版本协商 |
| corvus-mcp | MCP 服务端：工具注册表、JSON-RPC、stdio 与 streamable-http 传输 |
| corvus-transport | WS 服务端、native-messaging 宿主、long-poll 兜底、浏览器注册表 |
| corvus-runtime | 命令路由、按浏览器的队列、超时、幂等、取消 |
| corvus-record | 会话录制、SQLite schema、trace 写出、回放引擎 |
| corvus-memory | 站点卡片、选择器启发式、导入导出 |
| corvusd | 守护进程二进制：装配、服务生命周期、配置 |
| corvus | CLI：doctor、serve、attach、replay、bench、打印配置 |

关键依赖：tokio、axum、tower、serde、serde_json、rusqlite（bundled）、tracing、tracing-subscriber、thiserror、clap、uuid、dashmap、arc-swap。

## 4. 线协议

所有传输共用一个信封，带版本号，向前兼容。

    {
      "v": 1,
      "id": "01J...ULID",
      "kind": "command" | "response" | "event" | "hello" | "ping" | "pong",
      "browser": "edge:default",
      "session": "s-...",
      "tool": "browser_click",
      "args": { "target": "@e12" },
      "meta": { "timeout_ms": 5000, "idempotency_key": "...", "trace_id": "..." }
    }

规则：

- id 由守护进程分配 ULID，扩展原样回传。
- 每条命令都带幂等键；重连后只重放「没有记录到响应」的键。
- 响应携带结构化错误对象（见第 8 节），绝不返回裸字符串。
- 事件（导航、下载开始、DOM 变更摘要）由扩展主动上行，不需要请求。
- 握手：hello(browser, profile, version, capabilities) -> hello_ack(daemon version, limits)。能力协商保证老扩展仍可用。

## 5. MCP 面

- 传输：stdio（IDE 类客户端默认）与 streamable-http，监听 127.0.0.1:7331/mcp（DSH 及 Web 类客户端默认）。
- 工具命名：<域>_<动词>，一经发布永久稳定；别名只废弃不删除。
- 每个工具都声明注解：readOnlyHint、destructiveHint、idempotentHint、openWorldHint。
- 长输出按调用截断：`max_chars` 截断长字符串并置 `truncated: true`；M2 第一波不做游标分页。
- Resource：corvus://session/<id>/step/<n> 返回该步骤记录下来的页面快照，代理无需重跑即可查看历史。
- Prompt：M1 不提供（代理应当直接用工具），M3 再评估。

### 5.1 browser_navigate 载荷

请求：`{ "url": "<绝对 http、https 或 file URL>", "tab_id": "<可选标签页 id>" }`。

结果：

| 字段 | 类型 | 含义 |
|---|---|---|
| navigated | boolean | 导航已发起时为 true |
| loaded | boolean | 新文档在等待预算内到达 readyState complete（或触发 load 事件）时为 true；预算耗尽时为 false |
| url | string | 重定向后的最终 URL |
| previous | string 或 null | 导航前文档的 URL |
| same_document | boolean | 仅 URL 片段发生变化、不需要加载新文档时为 true |
| started_at | number | 导航开始时的 Unix 毫秒时间戳 |

扩展会等新文档就绪后才应答，因此 `browser_navigate` 之后的 `page_snapshot` 反映的是新页面。等待预算为 `max(0, min(meta.timeout_ms - 2000, 15000))` 毫秒；预算耗尽时调用仍然成功，返回 `loaded: false` 和当前 `url`，绝不会变成 `TIMEOUT`。守护进程原样转发该结果。

### 5.2 M2 第一波工具面

M2 第一波冻结的工具面共 18 个。`browser_navigate`、`page_snapshot`、`browser_click`、`browser_type` 保持 M1 行为不变；其余 14 个在本波加入。下表结果均为成功 MCP 工具结果中的 JSON 对象；失败使用第 8 节的错误分类并置 `isError`。

通用约定：

- 页面与动作类工具的 `tab_id` 可省略，省略时作用于当前活动标签；`tab_close` 与 `tab_switch` 必须显式给出目标标签。
- `target` 接受 `@eN` 引用或 CSS 选择器。引用按文档分配，`page_snapshot` 与 `page_find` 共用一个引用空间，因此两者产出的引用都可直接用于 `browser_click`、`browser_type`、`browser_hover`、`browser_press`、`browser_scroll` 或 `browser_select`。
- `max_chars` 上限会截断返回字符串并置 `truncated: true`，不会把成功调用变成错误。

| 工具 | 参数（除标注 ? 外必填） | 结果 |
|---|---|---|
| `browser_navigate` | `url`；`tab_id?` | `navigated`、`loaded`、`url`、`previous`、`same_document`、`started_at` |
| `page_snapshot` | `tab_id?`；`mode?`（`interactive` 默认或 `full`）；`max_nodes?` 40..3000 | `url`、`title`、`viewport`、`mode`、`node_cap`、`node_count`、`element_count`、`text_count`、`ref_count`、`truncated`、`nodes[]` |
| `browser_click` | `target`；`tab_id?` | `clicked`、`ref`、`selector`、`tag`、`role`、`name`、`bbox`、`point`、`events[]`、`url` |
| `browser_type` | `target`；`text`；`clear?` 默认 true；`blur?` 默认 false；`tab_id?` | `typed`、`ref`、`selector`、`tag`、`role`、`cleared`、`events[]`、`value_length`、`url`、`value`（密码字段改回 `masked`） |
| `page_text` | `tab_id?`；`max_chars?` 1..200000 默认 100000 | `url`、`title`、`text`、`truncated` |
| `page_html` | `tab_id?`；`target?` 默认整个文档；`max_chars?` 1..500000 默认 200000 | `url`、`html`、`truncated` |
| `page_find` | `query`；`tab_id?`；`role?`；`limit?` 1..100 默认 20；`regex?` 默认 false | `url`、`count`、`matches[]`，元素含 `ref`、`tag`、`role`、`text` |
| `browser_hover` | `target`；`tab_id?` | `hovered: true`、`url`、`target`（含 `ref?`、`tag`、`text?`） |
| `browser_press` | `keys`，字符串或字符串数组；`target?`；`tab_id?` | `pressed: true`、`keys[]`、`url` |
| `browser_scroll` | `direction`（`up`、`down`、`left`、`right`）；`pages?` 0..50 默认 1；`target?`；`tab_id?` | `scrolled: true`、`x`、`y`、`url` |
| `browser_select` | `target`；`values[]`；`tab_id?` | `selected[]`、`value`、`url` |
| `browser_eval` | `expression`；`world?`（`main` 或 `isolated` 默认）；`tab_id?` | `ok: true`、`value`；或 `ok: false`、`error`（含 `name`、`message`） |
| `browser_wait` | `tab_id?`；`selector?`、`text?`、`url_contains?` 至少一个；`timeout_ms?` 100..120000 默认 10000 | `matched: true`、`waited_ms`、`url` |
| `browser_history` | `action`（`back`、`forward`、`reload`）；`tab_id?` | `action`、`url`、`loaded` |
| `tab_list` | 无 | `tabs[]`，元素含 `tab_id`、`index`、`active`、`url`、`title`、`status` |
| `tab_new` | `url?`；`active?` 默认 true | `tab_id`、`url`、`loaded`、`previous_tab_id` |
| `tab_close` | `tab_id` | `closed: true`、`tab_id`、`next_tab_id?` |
| `tab_switch` | `tab_id` | `tab_id`、`url`、`title`、`active: true` |

说明：

- `browser_press` 接受 Enter、Tab、Escape、Backspace、Delete、Space、ArrowUp、ArrowDown、ArrowLeft、ArrowRight、PageUp、PageDown、Home、End、单字符键，以及 Control+、Shift+、Alt+、Meta+ 修饰键（例如 `Control+Shift+KeyK`）。
- `browser_select` 写入 `values` 中的每个值；`<select multiple>` 支持多个值。
- `browser_eval` 默认在隔离世界执行，可按需切到页面世界。页面异常属于成功的工具调用，返回 `{ "ok": false, "error": { "name", "message" } }`；只有工具级失败才置 `isError`。无法序列化为 JSON 的值返回 `null` 并附 `"value_note": "unserializable"`。
- `browser_wait` 至少需要一个条件；超时为 `TIMEOUT`，`details.waited_ms` 给出已等待时长。 错误带有 `details` 时，MCP 工具结果的文本会以紧凑 JSON 追加这些内容（例如 `TIMEOUT: wait timed out after 100 ms {"waited_ms":4321}`）；错误结果不携带 `structuredContent`。
- `tab_close` 关闭活动标签并激活相邻标签时返回 `next_tab_id`。

## 6. 扩展设计

三个上下文，零页面：

1. **Service Worker**：命令分发、tabs API、scripting、debugger 挂载/卸载、下载。
2. **内容脚本**：DOM 快照、点击与输入合成、等待、变更观察。
3. **offscreen 文档**：WebSocket 持有者（Service Worker 无法可靠持有长连接）、保活、重连循环。

弹窗只读：一个连接状态点加一行状态文字，没有任何控件。debugger 能力在握手中始终声明，需要 CDP 的工具按需挂载；只有浏览器拒绝挂载时该通道才不可用。

重连策略：指数退避加抖动（250ms 起，顶到 8s），网络恢复事件立即重试，30 秒一次 alarm 兜底，守护进程重启时主动下发 resume 消息。

## 7. 可靠性模型

    扩展状态：IDLE -> CONNECTING -> READY -> DEGRADED -> IDLE

- DEGRADED 表示已连上但某项能力不可用（例如浏览器拒绝 debugger 挂载）。依赖该能力的工具直接返回 CAPABILITY_UNAVAILABLE，而不是挂住。
- 每条命令都有截止时间；超时后守护进程向扩展发起取消，并返回带可重放 id 的 TIMEOUT。
- 路由器为「不可交错」的动作（click/type）保留按浏览器的串行队列，为只读感知保留并行池。
- 守护进程重启不丢会话：会话状态在 SQLite 里，扩展通过重放未确认命令续上。

## 8. 错误分类

| 错误码 | 含义 | 可重试 |
|---|---|---|
| BAD_ARGS | 参数不符合 schema | 否 |
| NOT_FOUND | 选择器 / 页面 / 标签不存在 | 是，页面变化后 |
| NOT_INTERACTABLE | 元素被遮挡、禁用或不在视口 | 是，滚动后 |
| TIMEOUT | 超过截止时间 | 是 |
| NAVIGATION_LOST | 命令执行中页面发生导航 | 是，需重新定位 |
| CAPABILITY_UNAVAILABLE | 浏览器拒绝 debugger 挂载，或页面不允许脚本注入 | 否，直到页面或权限变化 |
| BROWSER_GONE | 标签或浏览器已关闭 | 否 |
| TRANSPORT_DOWN | 扩展断连 | 是，重连后 |
| INTERNAL | 缺陷；携带 trace id | 否 |

## 9. 存储

SQLite 位于 ~/.corvus/corvus.db（WAL 模式）。

| 表 | 列 |
|---|---|
| sessions | id, started_at, ended_at, browser, profile, agent, status |
| steps | id, session_id, seq, ts, tool, args_json, result_json, error_code, duration_ms |
| snapshots | id, step_id, url, title, nodes, blob_path |
| network | id, step_id, url, method, status, ms, req_headers, res_headers, body_path |
| site_cards | domain, selector, role, success_count, fail_count, last_used, payload |

大对象（截图、响应体）落在数据库旁边的 ~/.corvus/blobs/<yyyy-mm-dd>/，按保留策略清理（默认 30 天，可配置）。

## 10. 配置

~/.corvus/config.toml：

    [daemon]
    http_port = 7331
    ws_port = 7330
    bind = "127.0.0.1"

    [recording]
    enabled = true
    mode = "structured"      # structured | full
    retention_days = 30

    [tools]
    paginate_over_bytes = 65536
    default_timeout_ms = 60000

    [memory]
    enabled = true
    export_format = "jsonl"

按工具覆盖超时：`browser_wait` 为 `args.timeout_ms + 5000` 毫秒、上限 125000 毫秒，`browser_eval` 为 30000 毫秒，其余工具使用 60000 毫秒默认值；路由器把所有截止时间统一限制在 125000 毫秒以内。

## 11. 构建与 CI

- cargo workspace，edition 2021，MSRV 由 rust-toolchain.toml 固定。
- CI 矩阵：windows-latest、macos-14、ubuntu-22.04，各自跑 fmt、clippy -D warnings、nextest。
- 扩展：esbuild 打包，无框架；体积预算 1MB；manifest key 固定，保证扩展 id 跨机器稳定。
- 发布：交叉编译产物加校验和，Release 说明由规范化提交信息生成。

## 12. 安全姿态

刻意为之：没有权限弹窗、没有白名单、没有逐动作确认。信任边界是操作系统用户账号：守护进程只监听回环地址，并用每机唯一的密钥认证扩展，密钥存放在系统钥匙串（Windows DPAPI、macOS Keychain、libsecret）。

已考虑的威胁：

- 同机其他用户：回环绑定 + 每用户密钥 + 文件权限。
- 恶意网页：内容脚本绝不把页面可控字符串当代码执行；所有选择器与脚本都来自守护进程。
- 扩展供应链：固定 manifest key，发布产物签名。

非目标：防「用户自己的代理」。这在设计上就不在范围内。

## 13. 仓库结构

    corvus/
      crates/corvus-proto, corvus-mcp, corvus-transport, corvus-runtime,
             corvus-record, corvus-memory, corvusd, corvus
      extension/
        manifest.json, background.js, content.js, offscreen.html, popup.html
      docs/
      tests/e2e/
      scripts/
      .github/workflows/
