# 工具参考

[English](09-tools.md) | [中文](09-tools.zh.md)

状态：1.4.0。读者：MCP 客户端作者、REST 客户端作者与工具使用者。

Corvus 在九个类别中提供 43 个 MCP 工具。其中 33 个工具通过扩展或回环 CDP 直连操作浏览器；10 个工具由守护进程应答。本文中的工具名、输入 schema 与结果形状均描述 1.4.0。MCP 面与 1.3.0 相同：1.4.0 不新增工具。同一套 43 工具目录与执行器也通过 REST 面的 `/api/v1/tools` 与 `/api/v1/tools/call` 提供；第 1 节的信封仅适用于 MCP。

## 1. MCP 结果信封

成功的 `tools/call` 只返回一个内容块，不返回 `structuredContent`：

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"format\":\"compact\",\"tab_id\":\"1\",\"url\":\"https://example.com/\",\"node_count\":42,\"truncated\":false,\"text\":\"@e1 button \\\"Save\\\"\"}"
    }
  ],
  "isError": false
}
```

文本是没有内嵌换行的紧凑 JSON。客户端必须把 `content[0].text` 解析为 JSON。不要读取 `structuredContent`：Corvus 不发送该字段。工具错误同样返回一个文本块，`isError` 为 true，文本是包含类型化错误码、消息、可选 details 与可选 trace 的紧凑 JSON 对象。

该信封适用于所有工具，包括 `browser_batch`、`tool_help` 与 CDP 工具。文本内部的形状是下文描述的工具结果。

## 2. 两级发现

`tools/list` 接受两个可选参数：

| 参数 | 取值 | 默认 | 含义 |
|---|---|---|---|
| `detail` | `brief`、`full` | `brief` | `brief` 把每个长描述替换为一行摘要，删除 `title`，并移除输入 schema 中的每个属性描述。`full` 保留长描述、标题与 schema 描述。 |
| `category` | 一个类别名 | 全部 | 只返回该类别。未知类别返回类型化 `BAD_ARGS`。 |

默认简版返回全部 43 个工具，共 20,452 字节；`detail: "full"` 为 37,067 字节。简版低于 30,000 字节预算；完整结果需显式请求。

`tool_help` 是单个工具的补充入口：

```json
{ "name": "page_snapshot" }
```

返回包含 `name`、`title`、`category`、`summary`、`description`、`inputSchema` 与 `annotations` 的完整条目。`{ "category": "page" }` 返回该类别工具，`{}` 返回目录索引。`name` 与 `category` 互斥；同时给出时返回 `BAD_ARGS`，消息为 `tool_help accepts name or category, not both`。

每个定义还带有 `_meta["corvus.category"]`，客户端无需解析描述即可分组。每个工具 schema 都接受可选的 `browser_id` 路由字段；省略时使用唯一已连接的浏览器。

## 3. 类别

| 类别 | 数量 | 工具 |
|---|---:|---|
| `page` | 10 | `browser_navigate`、`page_snapshot`、`page_text`、`page_html`、`page_find`、`browser_wait`、`browser_history`、`browser_dialog`、`browser_emulate`、`browser_batch` |
| `input` | 7 | `browser_click`、`browser_type`、`browser_hover`、`browser_press`、`browser_scroll`、`browser_select`、`browser_eval` |
| `tabs` | 4 | `tab_list`、`tab_new`、`tab_close`、`tab_switch` |
| `network` | 5 | `net_capture`、`net_log`、`net_clear`、`net_intercept`、`browser_cookies` |
| `files` | 3 | `upload_file`、`download_list`、`download_wait` |
| `cdp` | 5 | `cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events`、`browser_channel` |
| `sessions` | 4 | `session_list`、`session_steps`、`session_replay`、`session_export` |
| `diagnostics` | 4 | `browser_list`、`metrics`、`logs_tail`、`browser_console` |
| `help` | 1 | `tool_help` |
| **合计** | **43** | |

`browser_batch` 属于 `page`，但由守护进程本地应答：它校验并调度步骤，每一步再走正常路由。`browser_channel` 属于 `cdp`，同样由守护进程本地应答。

## 4. 工具目录

通道列中，`both` 表示两种浏览器通道都可用；`cdp` 表示需要已连接的 CDP 端点；`daemon` 表示不经过浏览器通道。

| 工具 | 类别 | 通道 | 用途 |
|---|---|---|---|
| `browser_navigate` | page | both | 在标签页打开 URL 并等待文档。 |
| `page_snapshot` | page | both | 返回可点击元素引用的紧凑页面树。 |
| `page_text` | page | both | 返回可见页面文本，受 `max_chars` 限制。 |
| `page_html` | page | both | 返回文档或元素 HTML，受 `max_chars` 限制。 |
| `page_find` | page | both | 查找页面文本并返回元素引用。 |
| `browser_wait` | page | both | 等待选择器、文本或 URL 条件。 |
| `browser_history` | page | both | 后退、前进或刷新。 |
| `browser_dialog` | page | cdp | 接受或关闭 JavaScript 对话框。 |
| `browser_emulate` | page | cdp | 应用或重置设备与网络模拟。 |
| `browser_batch` | page | daemon | 按顺序运行最多 20 个浏览器工具调用。 |
| `browser_click` | input | both | 点击元素引用或 CSS 选择器。 |
| `browser_type` | input | both | 聚焦元素并替换其文本。 |
| `browser_hover` | input | both | 把指针移到元素上。 |
| `browser_press` | input | both | 发送一个按键或按键序列。 |
| `browser_scroll` | input | both | 滚动页面或元素。 |
| `browser_select` | input | both | 在 select 元素中选择值。 |
| `browser_eval` | input | both | 在页面中执行 JavaScript。 |
| `tab_list` | tabs | both | 列出打开的标签页。 |
| `tab_new` | tabs | both | 打开新标签页。 |
| `tab_close` | tabs | both | 关闭标签页。 |
| `tab_switch` | tabs | both | 激活标签页。 |
| `net_capture` | network | both | 开始、停止或查看网络捕获。 |
| `net_log` | network | both | 读取已捕获的网络请求。 |
| `net_clear` | network | both | 清空网络捕获条目。 |
| `net_intercept` | network | both | 设置或清除请求拦截规则。 |
| `browser_cookies` | network | cdp | 读取浏览器级或页面级 cookie。 |
| `upload_file` | files | both | 在文件输入上设置本地文件。 |
| `download_list` | files | both | 列出近期下载。 |
| `download_wait` | files | both | 等待下载完成。 |
| `cdp_status` | cdp | cdp | CDP 连接、descriptor 与事件环计数。 |
| `cdp_targets` | cdp | cdp | 列出 CDP 目标。 |
| `cdp_send` | cdp | cdp | 发送原始 CDP 命令。 |
| `cdp_events` | cdp | cdp | 读取有界 CDP 事件环。 |
| `browser_channel` | cdp | daemon | 查看或切换浏览器通道。 |
| `session_list` | sessions | daemon | 列出已录制的 MCP 会话。 |
| `session_steps` | sessions | daemon | 读取一个会话的步骤。 |
| `session_replay` | sessions | daemon | 计划或回放已录制会话。 |
| `session_export` | sessions | daemon | 把会话导出为 JSON Lines。 |
| `browser_list` | diagnostics | daemon | 列出已连接浏览器与通道状态。 |
| `metrics` | diagnostics | daemon | 运行时长、工具计数、会话与录制状态。 |
| `logs_tail` | diagnostics | daemon | 读取进程内日志缓冲。 |
| `browser_console` | diagnostics | cdp | 近期 console 与异常条目。 |
| `tool_help` | help | daemon | 目录索引、类别或单个工具的完整帮助。 |

## 5. page_snapshot

`page_snapshot` 返回用于引用的页面树。默认输出为紧凑文本；`format: "json"` 保留旧版节点对象。

| 输入 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `tab_id` | string | 活动标签页 | 要捕获的标签页；省略时使用活动标签页。 |
| `mode` | `interactive` 或 `full` | `interactive` | `interactive` 只返回可操作节点；`full` 返回整棵树。 |
| `max_nodes` | 40-3000 的整数 | 300 | 返回节点数上限。截断时 `truncated` 为 true。 |
| `format` | `compact` 或 `json` | `compact` | `compact` 使用下文的行语法；`json` 返回节点对象。 |
| `viewport_only` | boolean | false | 只保留与当前视口相交的节点。 |
| `include_bbox` | boolean | false | 加入视口相对包围盒。 |

紧凑结果如下：

```json
{
  "format": "compact",
  "tab_id": "1",
  "url": "https://example.com/",
  "title": "Example",
  "viewport": { "width": 1280, "height": 720, "scrollX": 0.0, "scrollY": 0.0 },
  "mode": "interactive",
  "node_count": 3,
  "truncated": false,
  "text": "@e1 button \"Save\"\n@e2 textbox \"Name\" value=\"Ada\"\n# \"Hello\""
}
```

JSON 结果保留同样的元数据，但把 `text` 换成 `nodes`。扩展通道的 JSON 额外包含 `node_cap`、`element_count`、`text_count` 与 `ref_count`。CDP 的 JSON 节点包含 `kind`、`ref`、`tag`、`role`、`name`、`depth`、`states`、`attributes`，以及可选的 `value` 与 `bbox`；CDP JSON 结果不增加扩展专属计数。

### 紧凑行语法

元素行为：

```text
@e12 button "Save" [disabled,focused] value="Save draft" bbox=40,120,96,32
```

```text
@c42 textbox "Name" value="Ada"
```

文本行为：

```text
# "Hello world"
```

各部分的顺序固定：

1. 扩展通道为 `@e<数字>`，CDP 为 `@c<backendNodeId>`。引用只在返回它的快照内有效。
2. 小写 ARIA role；没有 role 时使用小写标签名。
3. 双引号包裹的可访问名称。反斜杠与引号会转义；换行与制表符变为空格；名称最多 120 字符。
4. 可选 `[states]`，顺序固定为：`disabled`、`checked` 或 `mixed`、`focused`、`required`、`expanded`、`selected`。
5. 可选 `value="..."`，最多 80 字符。
6. 可选 `bbox=x,y,w,h`，仅在 `include_bbox: true` 时出现；坐标相对视口。

文本行使用 `# "..."`，文本最多 240 字符，并使用同样的转义。`max_nodes` 在达到请求节点数后截断，并把 `truncated` 置为 true。

### 引用与 JSON 兼容

- 扩展引用为 `@eN`，CDP 引用为 `@c<backendNodeId>`。一种通道的引用不能用于另一种通道。跨通道使用时返回类型化 `BAD_ARGS`，`details.hint` 会提示改用匹配的引用，或省略 `tab_id` 对活动标签操作。
- 引用在产生它的页面快照仍有效时可用。导航或 DOM 变化后请重新快照，不要复用旧引用。
- 扩展标签 id 是数字（例如 `7`）；CDP 目标 id 是 `cdp_targets` 返回的 32 位十六进制字符串。v1.3 不在两个命名空间之间转换。任一通道都可省略 `tab_id` 使用活动标签页。
- `format: "json"` 是需要节点对象的客户端的兼容路径。`include_bbox` 在两种通道上都影响紧凑行；扩展路径的 JSON 节点本身已包含 `bbox`，`include_bbox` 不改变 JSON。
- `viewport_only` 只做节点包围盒与当前视口的相交判断，不会滚动页面。

## 6. browser_batch

`browser_batch` 按顺序运行最多 20 个浏览器工具调用：

```json
{
  "steps": [
    { "tool": "browser_navigate", "args": { "url": "https://example.com/" } },
    { "tool": "page_snapshot", "args": { "format": "compact" } },
    { "tool": "browser_click", "args": { "target": "@e1" } }
  ],
  "stop_on_error": true
}
```

| 输入 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `steps` | 1-20 个对象的数组 | 必填 | 每个对象有非空 `tool` 与可选对象 `args`；未知字段会被拒绝。 |
| `stop_on_error` | boolean | true | 第一个失败步骤后停止，并把 `stopped` 置为 true。 |

`browser_batch` 不能调用自身、`browser_channel`、`metrics`、`logs_tail`、任何会话工具或 `tool_help`。它可以调用 page、input、tabs、network、files、cdp 与 diagnostics 类别中的 34 个工具，但不包括 `browser_batch`、`browser_channel`、`metrics` 与 `logs_tail`；`browser_list` 与 `browser_console` 在允许范围内。允许列表在执行前校验；不允许的名称返回类型化 `BAD_ARGS`。

结果形状：

```json
{
  "ok": false,
  "count": 2,
  "stopped": true,
  "results": [
    { "step": 1, "tool": "browser_navigate", "ok": true, "data": { "url": "https://example.com/" } },
    { "step": 2, "tool": "page_snapshot", "ok": false, "error": { "code": "CAPABILITY_UNAVAILABLE", "message": "..." } }
  ]
}
```

`count` 是已执行步骤数。只有所有已执行步骤都成功时 `ok` 才为 true。`stop_on_error` 中止批次时 `stopped` 为 true；后续步骤不执行，也不出现在 `results` 中。每一步都走正常通道选择；批次本身由守护进程本地处理，不发送给浏览器。

## 7. tool_help

`tool_help` 有三种模式：

| 输入 | 结果 |
|---|---|
| `{}` | `{ "categories": [{ "category", "count", "tools" }], "usage": "..." }` |
| `{ "category": "page" }` | `{ "category": "page", "count": 10, "tools": [{ "name", "title", "summary" }] }` |
| `{ "name": "browser_batch" }` | 完整条目：`name`、`title`、`category`、`summary`、`description`、`inputSchema`、`annotations`。 |

未知名称与未知类别返回类型化 `BAD_ARGS`。`tool_help` 由守护进程本地应答，即使没有连接浏览器也始终可用。

## 8. 通道与可用性

`browser_channel` 查看并切换当前模式：

- `auto`（默认）优先使用已连接的 CDP 端点；只有 CDP 路由返回 `Unsupported` 时才回退到扩展。
- `cdp` 要求 CDP，并且从不回退。v1.3 在强制 CDP 下支持全部 33 个浏览器类工具，包括此前必须依赖扩展的八个工具。
- `extension` 忽略 CDP。它支持原有 25 个浏览器类工具，对八个仅 CDP 工具返回类型化 `CAPABILITY_UNAVAILABLE`。

十个守护进程本地工具在任何模式下都可用。`browser_channel` 只改变运行中的守护进程；下次启动仍使用 `--channel` 或 `CORVUS_CHANNEL`。

| 可用性分组 | 数量 | 工具 |
|---|---:|---|
| 两种通道 | 25 | page 7、input 7、tabs 4、network 4、files 3 |
| 仅 CDP | 8 | `browser_dialog`、`browser_emulate`、`browser_cookies`、`browser_console`、`cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events` |
| 仅扩展 | 0 | 1.4.0 中没有 |
| 守护进程本地 | 10 | `browser_list`、`browser_channel`、`tool_help`、`browser_batch`、四个会话工具、`metrics`、`logs_tail` |

扩展仍是零配置路径。强制 `cdp` 的进程无需扩展即可驱动所有浏览器类工具，但浏览器必须用远程调试与专用 profile 启动。端点设置、路由与安全见 [07-cdp.zh.md](07-cdp.zh.md)。

## 9. 错误与重试

每个失败都带类型化错误码。工具调用方最常见的错误：

| 错误码 | 含义 | 重试 |
|---|---|---|
| `BAD_ARGS` | 参数无效、未知工具或类别、跨通道引用、批次步骤不被允许。details 通常带 `hint` 或被拒绝字段。 | 否；修正请求。 |
| `NOT_FOUND` | 目标、会话、标签页或对话框不存在。`browser_dialog` 在没有对话框时返回 `no dialog is showing`。 | 状态变化后。 |
| `CAPABILITY_UNAVAILABLE` | 当前通道或浏览器无法提供该调用，例如强制 `extension` 下的仅 CDP 工具，或强制 `cdp` 但没有端点。 | 切换通道或重新连接。 |
| `BROWSER_GONE` | 调用挂起期间浏览器或其 CDP 连接消失。 | 重新列出标签页或目标后重试。 |
| `TIMEOUT` | 有界等待或命令超过期限。 | 仅幂等操作可重试。 |

`auto` 不会把失败的 CDP 动作通过扩展重放。只有 `Unsupported` 结果可以触发回退。其他传输或协议错误都会返回给调用方。

## 10. 实测体积与版本

候选发布基准比较了紧凑输出与 JSON 兼容路径：

| 指标 | 紧凑文本 | JSON | 缩减 |
|---|---:|---:|---:|
| 快照平均载荷 | 364.83 B / 91.21 tokens | 2,077.33 B / 519.33 tokens | 小 82.44% |
| 工具结果平均载荷 | 646.33 B | 2,077.33 B | 小 68.89% |

token 数使用基准的 tokenizer 估算。默认简版 `tools/list` 为 43 个工具 20,452 字节；完整详情为 37,067 字节。以上是候选发布测量；QA 附上实机验收证据后，发布说明再记录对应数据。

版本：1.4.0。本文的工具目录与 schema 已对照 1.4.0 工作区检查；MCP 目录与 1.3.0 相同。