# Corvus 架构

[English](02-architecture.md) | [中文](02-architecture.zh.md)

状态：1.2.0 交付 M1 至 M4 的后端能力、采用 CDP 与扩展混合传输的 42 工具 MCP 面，并加入单根目录打包、Linux 安装包、Docker 与 Kubernetes 部署。读者：工程实现者。

## 1. 约束设计的硬条件

1. **默认必须驱动用户正在使用的那个浏览器。** 零配置路径使用已在运行浏览器中的扩展：不重启、不建独立 profile、不加远程调试开关。v1.2.0 为已开启远程调试的浏览器增加一条可选、显式配置的 CDP 路径，因此设计仍不是纯 CDP：扩展仍是唯一提供 DOM 引用与仅扩展工具的路径。
2. **零审批、零配对、无必需设置页。** 扩展开箱即用；可选设置页只用于覆盖守护进程 URL 与可选 CDP 端点。
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
    |  mcp-server  -> 工具注册表 -> 通道路由器 -> 会话运行时           |
    |  传输中枢（ws / native-messaging / long-poll）                  |
    |  CDP 客户端（可选）：浏览器级 WS、Target 会话                   |
    |  录制器（sqlite + jsonl + trace）                              |
    |  录制（sqlite）       指标（prometheus）   健康检查             |
    +--------------------------------------------------------------+
        |  分帧 JSON 信封，走 WS（主通道）
        v
    扩展（MV3）：Service Worker + 内容脚本 + offscreen 保活
        |  chrome.tabs / scripting / debugger / webRequest / downloads
        v
    浏览器  <--- corvusd 可选直连 CDP（仅回环）

进程模型：每个用户会话一个 corvusd（Windows 与 Linux 上注册为服务，macOS 以前台方式运行），每个浏览器 profile 一个扩展，另有一个 CLI 客户端 corvus。

## 3. crate 划分

| crate | 职责 |
|---|---|
| corvus-proto | 线格式：信封、命令、响应、错误分类、版本协商 |
| corvus-mcp | MCP 服务端：工具注册表、JSON-RPC、stdio 与 streamable-http 传输 |
| corvus-transport | WS 服务端、native-messaging 宿主、long-poll 兜底、浏览器注册表 |
| corvus-runtime | 命令路由、按标签的通道、超时、幂等、取消 |
| corvus-record | 会话录制、SQLite schema、步骤存储、回放支撑 |
| corvus-cdp | 浏览器级 CDP 客户端、端点校验、事件环、原始与高层 CDP 工具 |
| corvusd | 守护进程二进制：装配、服务生命周期、配置 |
| corvus | CLI：doctor、serve、attach、replay、bench、打印配置 |

关键依赖：tokio、axum、serde、serde_json、rusqlite（bundled）、tracing、tracing-subscriber、tracing-appender、thiserror、clap、uuid。

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
- 发现：`tools/list` 默认返回简版描述（42 个工具 29,971 字节）；`detail: "full"` 返回 35,552 字节的完整描述，`tool_help` 按需说明单个工具。
- 每个工具都声明注解：readOnlyHint、destructiveHint、idempotentHint、openWorldHint。
- 长输出按调用截断：`max_chars` 截断长字符串并置 `truncated: true`；M2 不做游标分页。
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

- `tab_id` 在参数和结果中都是字符串，页面与动作类工具可省略；省略时作用于当前活动标签。语法上不属于本扩展或不存在的 id 返回 `NOT_FOUND`，而不是 `BAD_ARGS`。`tab_close` 与 `tab_switch` 必须显式给出目标标签。
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
| `browser_eval` | `expression`；`world?`（`main` 或 `isolated` 默认；经 CDP `Runtime.evaluate` 执行）；`tab_id?` | `ok: true`、`value`；或 `ok: false`、`error`（含 `name`、`message`） |
| `browser_wait` | `tab_id?`；`selector?`、`text?`、`url_contains?` 至少一个；`timeout_ms?` 100..120000 默认 10000 | `matched: true`、`waited_ms`、`url` |
| `browser_history` | `action`（`back`、`forward`、`reload`；back/forward 移动一个条目）；`tab_id?` | `action`、最终 `url`、`loaded` |
| `tab_list` | 无 | `tabs[]`，元素含 `tab_id`、`index`、`active`、`url`、`title`、`status` |
| `tab_new` | `url?`；`active?` 默认 true | `tab_id`、`url`、`loaded`、`previous_tab_id` |
| `tab_close` | `tab_id` | `closed: true`、`tab_id`、`next_tab_id?` |
| `tab_switch` | `tab_id` | `tab_id`、`url`、`title`、`active: true` |

说明：

- `browser_press` 接受 Enter、Tab、Escape、Backspace、Delete、Space、ArrowUp、ArrowDown、ArrowLeft、ArrowRight、PageUp、PageDown、Home、End、单字符键，以及 Control+、Shift+、Alt+、Meta+ 修饰键（例如 `Control+Shift+KeyK`）。
- `browser_select` 写入 `values` 中的每个值；`<select multiple>` 支持多个值。
- `browser_eval` 由 debugger 通道驱动：通过自动的按标签 debugger 通道调用 CDP `Runtime.evaluate` 执行表达式。`isolated`（默认）由该通道创建的专用隔离世界执行，`main` 在页面世界执行；页面 Content-Security-Policy 不会限制该表达式。debugger 挂载被拒绝时返回 `CAPABILITY_UNAVAILABLE`。页面异常属于成功的工具调用，返回 `{ "ok": false, "error": { "name", "message" } }`；只有工具级失败才置 `isError`。无法序列化为 JSON 的值返回 `null` 并附 `"value_note": "unserializable"`。
- `browser_history` 的每次 `back` 或 `forward` 都通过 CDP `Page.getNavigationHistory` 与 `Page.navigateToHistoryEntry` 精确移动一个条目，等待文档加载完成后返回最终 `url`；越界条目返回 `NOT_FOUND`。
- `browser_wait` 至少需要一个条件；超时为 `TIMEOUT`，`details.waited_ms` 给出已等待时长。 错误带有 `details` 时，MCP 工具结果的文本会以紧凑 JSON 追加这些内容（例如 `TIMEOUT: wait timed out after 100 ms {"waited_ms":4321}`）；错误结果不携带 `structuredContent`。
- `tab_close` 关闭活动标签并激活相邻标签时返回 `next_tab_id`。

### 5.3 M2 第二波工具面

M2 第二波新增 8 个工具。下表结果均为成功 MCP 工具结果中的 JSON 对象；失败使用第 8 节的错误分类并置 `isError`。

通用路由与超时：

- 每个面向浏览器的工具都接受可选的 `browser_id` 字符串输入，包括 5.2 节的 18 个工具；`browser_list` 是例外，因为它操作的是浏览器集合。省略 `browser_id` 且恰好连接一个浏览器时选择该浏览器；没有浏览器时调用失败并返回 `TRANSPORT_DOWN`；连接两个或更多浏览器时调用失败并返回 `BAD_ARGS`，`details` 为 `{"reason":"multiple_browsers","browser_ids":[...]}`。给出未知 id 时调用失败并返回 `TRANSPORT_DOWN`，`details` 为 `{"browser_id":"..."}`。现有 18 个工具的其他参数、行为和结果形状保持不变。
- `browser_list` 不接受参数，按 `connected_at_ms` 升序返回所有已连接浏览器；它由实时传输注册表直接应答，不经过扩展往返。
- 8 个新工具默认使用 60000 毫秒的 `meta.timeout_ms`；`download_wait` 改用 `min(args.timeout_ms + 5000, 125000)`。第 8 节的 9 个错误码保持不变。

| 工具 | 参数（除标注 ? 外必填） | 结果 |
|---|---|---|
| `browser_list` | 无 | `browsers[]`，元素含 `browser_id`、`profile`、`version`、`capabilities[]`、`connected_at_ms`，按 `connected_at_ms` 升序 |
| `net_capture` | `action`（`start`、`stop` 或 `status`）；`tab_id?`；`browser_id?` | start：`capturing: true`、`tab_id`、`started_at`、`capacity: 2000`；stop：`capturing: false`、`tab_id`、`entries`；status：`capturing`、`tab_id`、`started_at?`、`entries` |
| `net_log` | `tab_id?`；`browser_id?`；`since_seq?` 默认 0；`limit?` 1..1000 默认 100；`url_contains?`；`method?`；`resource_type?`；`status_min?`；`status_max?`；`include_headers?` 默认 false | `entries[]`，元素含 `seq`、`ts`、`tab_id`、`request_id`、`method`、`url`、`status`（数字或 null）、`mime`（字符串或 null）、`resource_type`、`from_cache`、`duration_ms`（数字或 null）、`error`（字符串或 null）、`intercepted`（`{action, rule_id}` 或 null），可选 `request_headers`、`response_headers`；`next_seq`；`capturing` |
| `net_clear` | `tab_id?`；`browser_id?` | `cleared` |
| `net_intercept` | `action`（`set` 或 `clear`）；`browser_id?`；`tab_id?`；`rules?` 1..50 | `ok: true`、`scope`（`tab` 或 `browser`）、`tab_id?`、`rules[]`（元素含 `id`、`action`、`url_regex`，可选 `method`、`resource_type`）；`applied` |
| `download_list` | `browser_id?`；`limit?` 1..200 默认 50 | `downloads[]`，元素含 `id`、`filename`、`url`、`state`、`bytes_received`、`total_bytes`、`danger`、`exists`、`started_at`，可选 `tab_id`；按开始时间从新到旧 |
| `download_wait` | `browser_id?`；`id?`；`timeout_ms?` 100..120000 默认 30000 | `id`、`state`、`filename`、`url`、`bytes_received`、`total_bytes` |
| `upload_file` | `target`；`files[]` 1..10 个绝对路径；`browser_id?`；`tab_id?` | `uploaded: true`、`files[]`、`target`（含可选 `ref`、`tag`、`text`）；`input`（含可选 `accept`、`multiple`） |

捕获生命周期：

- `net_capture` 按标签显式开启；省略 `tab_id` 时作用于当前活动标签。`start` 在需要时挂载 debugger 并启用 CDP Network 域；缓冲区是每个标签 2000 条的环形缓冲。挂载被拒绝时返回 `CAPABILITY_UNAVAILABLE`，`details` 为 `{"tab_id":"...","cause":"..."}`。捕获在同一标签导航后继续存在，标签关闭时静默结束。
- `net_log` 读取捕获数据。省略 `tab_id` 时读取所选浏览器的所有已捕获标签。条目按 `seq` 升序；过滤在 `limit` 之前执行，`limit` 在过滤之后执行。`next_seq` 为最后返回的 `seq`；结果为空时等于 `since_seq`。只有 `include_headers` 为 true 时才包含头信息。`net_clear` 清除一个标签；省略 `tab_id` 时清除该浏览器的所有已捕获标签。

拦截：

- `net_intercept` 的 `set` 替换该作用域的规则集，`clear` 删除规则集。规则需要唯一 `id`（1..64）、必填的 `url_regex`（按 JavaScript `RegExp` 编译）以及 `block`、`mock`、`continue` 之一作为 `action`；`method` 与 `resource_type` 用于收窄匹配。正则无效时返回 `BAD_ARGS`，`details` 为 `{"rule_id":"..."}`。`mock` 接受 `status` 100..599（默认 200）、`headers` 与 `body_base64`（解码后最多 1 MiB）；这三个字段在其他 action 上无效。
- 规则按数组顺序求值，首个匹配生效。带 `tab_id` 的规则集同时在该标签上启动捕获；不带 `tab_id` 的浏览器级规则集作用于任何正在捕获的标签。`block` 调用 CDP `Fetch.failRequest` 并传入 `BlockedByClient`；`mock` 调用 `Fetch.fulfillRequest` 并传入状态、头与 base64 body（有 body 且未给出 content type 时使用 `text/plain; charset=utf-8`）；`continue` 调用 `Fetch.continueRequest`。即使 CDP 调用失败，也必须继续被暂停的请求，并返回 `isError`。

下载与上传：

- `download_list` 按从新到旧返回已跟踪的下载，只暴露元数据，不返回本地文件路径。
- `download_wait` 在所选下载到达 `complete` 或 `interrupted` 时返回。其 `id` 是字符串：非负十进制字符串选择该下载；其他字符串返回 `BAD_ARGS`，`details` 为 `{"id":"..."}`；`id` 使用 JSON 数字时在 schema 层按 `BAD_ARGS` 拒绝。
- 未知的十进制 `id` 不会返回 `NOT_FOUND`，最终返回 `TIMEOUT`，`details` 为 `{"waited_ms":...,"id":"..."}`（未给出 `id` 时 `details` 只含 `waited_ms`）。省略 `id` 时，选择本次调用之前 5000 毫秒内新建或开始的下载，从新到旧查找，绝不选择更早的下载；已完成的候选立即返回。
- `upload_file` 将 `target`（该标签最新快照中的 `@eN` 引用或 CSS 选择器）经 CDP 解析并调用 `DOM.setFileInputFiles`，因此隐藏的文件 input 也能使用；随后派发 `input` 与 `change` 事件。它在设置文件前逐个检查所有本地路径，文件缺失或不可读时返回 `BAD_ARGS`，`details` 为 `{"path":"..."}`。
- 扩展没有文件访问权限时，浏览器仍可能拒绝 file URL 访问。`target` 不是 `input[type=file]` 时返回 `BAD_ARGS`，`details` 为 `{"reason":"not_a_file_input","tag":"...","input_type":"..."}`。

### 5.4 M3 第一波工具面

M3 第一波新增 6 个工具，工具面达到 32 个。录制始终开启；四个会话工具读取 SQLite 录制库，`metrics` 与 `logs_tail` 读取进程内的可观测性状态。下表结果均为成功 MCP 工具结果中的 JSON 对象；失败使用第 8 节的错误分类并置 `isError`。

| 工具 | 参数（除标注 ? 外必填） | 结果 |
|---|---|---|
| `session_list` | `limit?` 1..200 默认 50；`since_ms?` >= 0 | `sessions[]`，元素含 `session_id`、`started_at`、`ended_at`（数字或 null）、`status`、`transport`、`agent`（字符串或 null）、`steps`、`errors`；`total`；按 `started_at` 从新到旧 |
| `session_steps` | `session_id`；`from_seq?` >= 0 默认 0；`limit?` 1..500 默认 100 | `session_id`；`steps[]`，元素含 `seq`、`ts`、`tool`、`browser_id`（字符串或 null）、`tab_id`（字符串或 null）、`ok`、`error_code`（字符串或 null）、`duration_ms`、`args`、`args_truncated`、`result`、`result_truncated`；`total_steps`；`next_seq` 仅在还有步骤时返回 |
| `session_replay` | `session_id`；`from_seq?` 默认第一条；`to_seq?` 默认最后一条；`mode?` `dry_run` 或 `live` 默认 `dry_run`；`browser_id?`；`continue_on_error?` 默认 false | `session_id`、`mode`、`total`、`planned[]`（含 `seq`、`tool`、`args`）、`executed[]`（含 `seq`、`tool`、`ok`、`error_code`、`duration_ms`）、`skipped[]`（含 `seq`、`tool`、`reason`）、`stopped_at?`、`error_code?`、`replay_session_id?` |
| `session_export` | `session_id`；`format?` `jsonl` | `session_id`、`format`、`content`、`bytes`、`truncated` |
| `metrics` | 无 | `uptime_ms`、`started_at`、`protocol`、`browsers_connected`、`browsers[]`、`sessions_total`、`sessions_active`、`tool_calls_total`、`tool_errors_total`、`by_tool`（`tool` -> `calls`、`errors`、`avg_ms`）、`errors_by_code`、`inflight`、`recording`（`enabled`、`db_path`、`sessions`、`steps`）、`last_call_at_ms?` |
| `logs_tail` | `limit?` 1..1000 默认 200；`level?` `debug`、`info`、`warn` 或 `error`；`since_ms?` | `entries[]`，元素含 `ts`、`level`、`target`、`message`、`fields`；`dropped` |

**会话与录制。** 一个会话就是一个 MCP 会话：HTTP 使用 `Mcp-Session-Id`，stdio 每个进程生成一个 `s-<uuid v4>` id。每次 `tools/call` 都会被录制，失败调用也不例外，记录解析后的 `browser_id` 与 `tab_id`、耗时、`ok` 与 `error_code`；`args` 与 `result` 以 JSON 文本存储，各自上限 262144 字节，并用 `args_truncated`/`result_truncated` 标记。录制异步执行，数据库错误只记录警告，绝不让工具调用失败。会话在关闭或守护进程停止时以 `closed` 结束；上一进程遗留的活动会话在下次启动时标记为 `abandoned`。

**会话读取。** `session_steps` 按 `seq` 升序返回从 `from_seq` 开始的步骤。存储文本完整且可解析时，`args` 与 `result` 为解析后的 JSON，否则为已存储的前缀字符串；序列化后超过 65536 字节的值返回 null，并带上对应的 `*_truncated` 标记与 `*_bytes` 大小。只有还有步骤时才有 `next_seq`。对 `session_steps`、`session_replay` 与 `session_export`，未知会话 id 返回 `NOT_FOUND`，`details` 为 `{"session_id":"..."}`。

**回放。** `session_replay` 默认 `dry_run`，只生成计划；`live` 通过正常路由器重发步骤并沿用正常工具超时，结果记录到新会话，`agent` 为 `corvus-replay`、`transport` 为 `internal`，id 由 `replay_session_id` 返回。`session_list`、`session_steps`、`session_replay`、`session_export`、`metrics`、`logs_tail` 与 `browser_list` 以 `control_tool` 原因跳过；live 模式下 `tab_close` 与 `tab_switch` 以 `tab_tool` 原因跳过，录制中的 `tab_id` 会被移除，因此步骤作用于活动标签，除非用 `browser_id` 选择浏览器。回放默认在第一个失败步骤停止；`continue_on_error` 为 true 时继续。被回放工具失败属于数据，不是 `session_replay` 的错误（`isError` 为 false）。没有可回放步骤时返回 `BAD_ARGS`，`details.reason` 为 `no_replayable_steps`；live 模式没有已连接浏览器时返回 `TRANSPORT_DOWN`。

**导出与可观测性。** `session_export` 写 JSONL：第一行 `{"type":"session",...}` 含会话全部列，随后每个步骤一行 `{"type":"step",...}`，其中 `args_json` 与 `result_json` 为字符串。内容上限 2097152 字节，被截断时 `truncated` 为 true；`format` 只支持 `jsonl`。`metrics` 计数器是进程生命周期内的原子计数，不重置，`avg_ms` 为截断后的整数毫秒。`logs_tail` 读取 5000 条的内存环形缓冲；`ts` 为 epoch 毫秒，`fields` 最多含 16 个一级原始事件字段，值转为字符串，`dropped` 为启动以来被覆盖的事件数，非法 `level` 返回 `BAD_ARGS`，`details.level` 给出原因。

`GET /metrics` 位于 MCP HTTP 端口、与 `/health` 并列，输出 Prometheus 文本：`corvus_uptime_seconds`、`corvus_browsers_connected`、`corvus_tool_calls_total{tool="..."}`、`corvus_tool_errors_total{code="..."}`、`corvus_sessions_total` 与 `corvus_steps_total`。`/health` 保持现有形状。

### 5.5 并发与取消

M4 第一波不新增工具。它改变路由器调度调用的方式，以及进行中的 `tools/call` 如何被取消；32 个工具的参数与结果形状保持不变。

**作用域门控。** 每条浏览器命令在派发前都会被分配到一条通道，键为 `(browser_id, tab_id)`。同一通道内的调用逐条执行并保持有序；不同通道的调用并行执行，彼此没有顺序关系。因此显式 `tab_id` 与其他标签并行，并在同一标签内与其他调用有序。省略 `tab_id` 时，在派发时解析为活动标签并加入该标签的通道，所以它与同一标签的显式调用有序，而不会另开一条通道。`tab_new`、`tab_close`、`tab_switch`、`download_wait` 以及未给 `tab_id` 的 `net_intercept` 使用按浏览器划分的通道，该通道只以 `browser_id` 为键；这些调用彼此有序，且不串行化独立的标签工作。不执行浏览器命令的调用（`browser_list`、`metrics`、`logs_tail` 与只读会话工具）不进入通道。`page_text`、`page_html`、`page_find`、`net_log` 与 `download_list` 是不受门控的只读调用：它们读取既有状态，不排在同一标签的写操作之后，也不阻塞写操作。

**MCP 取消。** 客户端用 `notifications/cancelled` 取消进行中的请求，`params.requestId` 为对应 `tools/call` 的 JSON-RPC id。守护进程用 JSON-RPC 错误 `-32800` 应答该请求，并在 `session_steps` 中把这次尝试记录为 `ok: false`、`error_code` 为 `"CANCELLED"`。为停止浏览器工作，守护进程向扩展发送 `event` 为 `"cancel"` 的事件帧；扩展协作式中止。取消只作用于所指的请求。错误消息为 `"request cancelled"`，`retryable` 为 false。

**取消不做什么。** 守护进程立即停止等待并返回 `-32800`，不等待浏览器完成。浏览器工作对 `browser_wait`、`download_wait`、`browser_eval` 与 `browser_navigate` 采用协作式中止，其他工具可能延迟完成。已经作用于页面、下载或标签的效果不会回滚。

### 5.6 混合 CDP 传输

v1.2.0 新增十个工具，工具面达到 42 个。原有 32 个工具的参数与结果形状保持不变。浏览器类调用现在走两个通道之一。

**探测。** 扩展设置页可以把 CDP 端点保存在 `corvus.cdpEndpoint`。`auto` 以 800 毫秒超时探测回环端口 9222、9223 与 9333，读取 `/json/version`，选择与本扩展浏览器家族匹配的可用端点。Chrome 与 Chromium 同族；Edge、Brave、Opera 与 Vivaldi 各自独立。握手携带包含端点、WebSocket 调试地址、浏览器、协议版本、来源与家族的 descriptor。只有探测成功时才声明 `cdp` 能力。

**客户端。** `corvus-cdp` 校验 descriptor 端点与调试地址必须是回环 `ws://` 或 `wss://`，并保持一条浏览器级长连接 WebSocket。页面操作使用扁平 `Target.attachToTarget` 会话；请求按数字 id 关联。客户端保留 1000 条事件环，序号单调递增，并有 `dropped` 计数。重连退避从 250 毫秒到 10 秒，每 15 秒心跳，请求超时默认 10 秒、最大 60 秒。套接字断开会让未完成请求失败，绝不重放。

**路由。** `auto` 优先 CDP，仅在 CDP 调用返回 `Unsupported` 时回退扩展。`cdp` 严格不回退。`extension` 忽略 CDP。`--no-cdp` 移除 CDP 路径，此时请求 `cdp` 模式会失败。`browser_channel` 在运行中的守护进程上切换模式；启动模式来自 `--channel` 或 `CORVUS_CHANNEL`。

**可用性。** 八个工具仅扩展（快照与引用动作），八个仅 CDP，十七个共享，九个守护进程本地。强制模式对无法执行的工具返回 `CAPABILITY_UNAVAILABLE`，不会静默切换通道。只有 `auto` 可能在 CDP 返回 `Unsupported` 后移动调用。

**分类。** 发现采用两级：`tools/list` 默认返回简版描述（42 个工具 29,971 字节），`detail: "full"` 返回完整描述（35,552 字节）。`tool_help` 按需说明单个工具。

| 分类 | 工具数 |
|---|---|
| 页面 | 9 |
| 输入 | 7 |
| 标签 | 4 |
| 网络 | 5 |
| 文件 | 3 |
| CDP | 5 |
| 会话 | 4 |
| 诊断 | 4 |
| 帮助 | 1 |
| **合计** | **42** |

**安全。** 浏览器级 CDP 端点可以读取 cookie、注入脚本、拦截流量，并驱动该浏览器实例中的所有 profile。Corvus 不在其前面增加认证。校验只接受回环，但任何能访问调试端口的本地进程都拥有同等能力。绝不要把 9222、9223、9333、7330 或 7331 暴露到回环之外。

## 6. 扩展设计

三个运行时上下文与两个小型界面：

1. **Service Worker**：命令分发、tabs API、scripting、debugger 挂载/卸载、下载。
2. **内容脚本**：DOM 快照、点击与输入合成、等待、变更观察。
3. **offscreen 文档**：WebSocket 持有者（Service Worker 无法可靠持有长连接）、保活、重连循环。

弹窗只读：一个连接状态点加一行状态文字，没有任何控件。设置页是可选的，把守护进程 WebSocket URL（`ws://` 或 `wss://`，默认 `ws://127.0.0.1:7330`）保存在 `corvus.daemonUrl`，并把可选 CDP 端点保存在 `corvus.cdpEndpoint`；两者都不参与命令链路。只有端点探测成功时才声明 `cdp` 能力。debugger 作为基础能力声明，需要 CDP 的工具按需挂载；只有浏览器拒绝挂载时该 debugger 通道才不可用。

重连策略：指数退避加抖动（250ms 起，顶到 8s），网络恢复事件立即重试，30 秒一次 alarm 兜底，守护进程重启时主动下发 resume 消息。

## 7. 可靠性模型

    扩展状态：IDLE -> CONNECTING -> READY -> DEGRADED -> IDLE

- DEGRADED 表示已连上但某项能力不可用（例如浏览器拒绝 debugger 挂载）。依赖该能力的工具直接返回 CAPABILITY_UNAVAILABLE，而不是挂住。
- 每条命令都有截止时间；超时后守护进程向扩展发起取消，并返回带可重放 id 的 TIMEOUT。
- 路由器按 `(browser_id, tab_id)` 通道串行执行命令，只读感知不受门控，详见 5.5 节。
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
| CANCELLED | 浏览器应答前命令已被取消 | 否 |

## 9. 存储与文件布局

守护进程把所有后端文件放在一个根目录下，由 `--home <dir>` 或 `CORVUS_HOME` 选择。平台默认值：Windows 为 `%LOCALAPPDATA%\Corvus`，Linux 为 `$XDG_DATA_HOME/corvus`（回退到 `~/.local/share/corvus`），macOS 为 `~/Library/Application Support/Corvus`。Windows 安装器把服务根目录设为 `%ProgramData%\Corvus`，Linux 安装包使用 `/opt/corvus`，Docker 镜像与 Kubernetes 清单使用 `/data`。

    <root>/
      bin/            corvusd、corvus
      data/corvus.db  SQLite 录制库
      logs/           corvusd.log.YYYY-MM-DD（UTC）及可选服务输出文件
      config/         预留给文件配置
      extension/      已解包的扩展

解析顺序：`--db <path>` 优先；否则已配置的 `--home`/`CORVUS_HOME` 选择 `<root>/data/corvus.db`；两者都没有时仍读取 `CORVUS_DB_PATH`，最后才是平台默认数据库。相对根目录按进程工作目录转为绝对路径。`corvusd serve` 创建 `data/`、`logs/`、`config/`，不创建 `bin/` 或 `extension/`。文件日志默认写入 `<root>/logs/corvusd.log.YYYY-MM-DD`，按 UTC 每日轮转；`--log-file` 覆盖路径，`--no-log-file` 关闭文件输出。服务与容器布局见 [05-deployment.zh.md](05-deployment.zh.md)。

录制库为 SQLite，WAL 模式，schema 版本 1 记录在 `schema_meta`。

| 表 | 列 |
|---|---|
| sessions | `id`、`started_at`、`ended_at`、`browser`、`profile`、`agent`、`transport`、`status`、`steps`、`errors` |
| steps | `id`、`session_id`、`seq`、`ts`、`tool`、`args_json`、`result_json`、`error_code`、`duration_ms`、`browser_id`、`tab_id`、`ok`、`args_truncated`、`result_truncated`；`UNIQUE(session_id, seq)`，并在 `(session_id, seq)` 上建索引 |

启动时，上一进程遗留的活动会话把 `ended_at` 设为当前时间、`status` 设为 `abandoned`；`started_at` 早于 30 天的会话会被清理。

`snapshots`、`network` 与 `site_cards` 不在 schema v1 中创建；它们仍是后续工作，本里程碑不写 blob 目录。

## 10. 配置

本版本不加载 `config.toml`，所有设置来自命令行参数与环境变量：

| 设置 | 控制项 | 默认值 |
|---|---|---|
| 监听地址 | `corvusd serve --bind`、`corvus serve --bind` | `127.0.0.1` |
| WebSocket 端口 | `--ws-port` | 7330 |
| MCP HTTP 端口 | `--http-port` | 7331 |
| 根目录 | `--home`、`CORVUS_HOME` | 平台默认值 |
| 数据库 | `--db`，其次根目录，再次 `CORVUS_DB_PATH` | `<root>/data/corvus.db` |
| 文件日志 | `--log-file`、`--no-log-file` | `<root>/logs/corvusd.log` |
| 日志过滤 | `RUST_LOG` | `info` |
| MCP 传输 | `--stdio`，或 HTTP 的 `/mcp` | HTTP |
| CDP 端点 | `--cdp-endpoint`、`CORVUS_CDP_ENDPOINT` | 无；使用扩展 descriptor |
| 通道 | `--channel`、`CORVUS_CHANNEL` | `auto` |
| 关闭 CDP | `--no-cdp` | 启用 CDP |

录制始终开启，保留期固定 30 天。`corvus config --client <name> --transport stdio|http [--home <dir>]` 在 stdout 输出可直接粘贴的 `mcpServers` JSON，位置提示写到 stderr；stdio 形式在给出 `--home` 时把它写入参数，HTTP 形式指向 `http://127.0.0.1:7331/mcp`。

按工具覆盖超时：`browser_wait` 为 `args.timeout_ms + 5000` 毫秒、上限 125000 毫秒，`browser_eval` 为 30000 毫秒，`download_wait` 为 `args.timeout_ms + 5000` 毫秒、上限 125000 毫秒，其余工具使用 60000 毫秒默认值；路由器把所有截止时间统一限制在 125000 毫秒以内。

## 11. 构建与 CI

- cargo workspace，edition 2021，`Cargo.toml` 与 `rust-toolchain.toml` 固定 Rust 1.90 为 MSRV。
- CI 矩阵：windows-latest、macos-latest、ubuntu-latest，各自运行 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo test --workspace`。
- 打包任务：`scripts/package-linux.sh` 构建 x86_64 tar.gz 并校验 systemd 单元；Docker 任务构建镜像、检查版本与 `/health`，并断言工具面为 42 个。
- 扩展：手写 MV3，无打包器；manifest key 固定，保证扩展 id 跨机器稳定。
- 发布：平台安装包附 `SHA256SUMS`；1.2.0 不提供 macOS 安装包。

## 12. 安全姿态

刻意为之：没有权限弹窗、没有白名单、没有逐动作确认，也没有任何认证或配对密钥。信任边界是操作系统用户账号：浏览器传输与 MCP HTTP 服务只监听回环地址，根目录文件权限保护录制库。任何能访问 7330 或 7331 端口的本机进程都能驱动已连接的浏览器，因此绝不要把这两个端口暴露到回环之外。可选 CDP 客户端具有同样的边界：浏览器级调试端点可以读取 cookie、注入脚本并控制该浏览器实例中的所有 profile，它没有认证，守护进程只接受回环 `ws://` 或 `wss://` 端点。也绝不要暴露 9222、9223 或 9333。

已考虑的威胁：

- 同机其他用户：回环绑定 + 每用户根目录权限。
- 恶意网页：内容脚本绝不把页面可控字符串当代码执行；所有选择器与脚本都来自守护进程。
- 扩展供应链：固定 manifest key，发布产物签名。

非目标：防「用户自己的代理」。这在设计上就不在范围内。

## 13. 仓库结构

    corvus/
      crates/corvus-proto, corvus-mcp, corvus-transport, corvus-runtime,
             corvus-record, corvus-cdp, corvusd, corvus
      extension/
        manifest.json, background.js, content.js, offscreen.html,
        popup.html, popup.js, options.html, options.js, daemon_url.js
      deploy/docker/, deploy/k8s/
      packaging/windows/, packaging/systemd/
      docs/
      tests/
      scripts/
      .github/workflows/
