# 工具参考

[English](09-tools.md) | [中文](09-tools.zh.md)

状态：1.6.0 是当前已发布版本，于 2026-11-15 由标签 `v1.6.0` 发布；1.5.0 为上一版本。工作树包含未发布的 1.7.0 工作：MCP 面扩展为十个类别中的 62 个工具，包括新的 `storage` 类别与 18 个新工具。1.7.0 面有 46 个 CDP 浏览器侧工具、42 个扩展处理程序与 12 个守护进程本地工具；扩展为 `tabGroups` 增至 11 项权限，保持 `all_frames: false`，并有 42 个处理程序。QA 验收与本地 Windows 打包已在重新冻结的 62 工具树上通过；不存在 1.7.0 CI 运行、已发布包或已发布产物，因此所有 CI 与发布行仍为 Pending，工作树仍未发布。1.6.0 发布新增可选守护进程策略，不新增工具；标签对象 `c5daf98eb03b0fac310e53c3a59edfa77874f919` 指向 `b420c3787a8f240c60514ac8c745e67474f1c1fd`，主 CI 运行 43716832915 通过八个任务，发布工作流 43717236667 成功，GitHub 发布 446071476 发布了产物。QA PASS 记录于 [v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md)，基于 HEAD `61ca85f` 的工作树。读者：MCP 客户端作者、REST 客户端作者与工具使用者。

1.7.0 工作树在十个类别中提供 62 个 MCP 工具。46 个浏览器侧工具可通过回环 CDP 直连运行，42 个可通过扩展运行，12 个由守护进程应答；38 个工具在两种浏览器通道上都可用，8 个仅 CDP，4 个仅扩展。已发布的 1.6.0 面为 43 个工具。1.7.0 工作树新增 18 个工具：四个页面工具、三个输入工具、四个标签页与窗口工具、五个存储工具和两个诊断工具。本文中的工具名、输入 schema 与结果形状描述该工作树。MCP 面在 1.3.0 至 1.6.0 之间保持不变：这些版本均不新增工具；可选守护策略见 [11-guardrails.zh.md](11-guardrails.zh.md)。同一套 62 工具目录与执行器也通过 REST 面的 `/api/v1/tools` 与 `/api/v1/tools/call` 提供；第 1 节的信封仅适用于 MCP。

## 1. MCP 结果信封

成功的 `tools/call` 只返回一个内容块，不返回 `structuredContent`；例外是 `inline: true` 的 `browser_screenshot`，它返回一个文本块后跟一个图像块：

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

该信封适用于所有工具，包括 `browser_batch`、`tool_help` 与 CDP 工具。唯一例外是内联截图：`tool_success` 把 `content[0]` 设为摘要文本，把 `content[1]` 设为 `{ "type": "image", "data": "<base64>", "mimeType": "image/webp" }`，且不返回 `structuredContent`。不带 `inline` 的 `browser_screenshot` 返回通常的单个文本块。文本内部的形状是下文描述的工具结果。

## 2. 两级发现

`tools/list` 接受两个可选参数：

| 参数 | 取值 | 默认 | 含义 |
|---|---|---|---|
| `detail` | `brief`、`full` | `brief` | `brief` 把每个长描述替换为一行摘要，删除 `title`，并移除输入 schema 中的每个属性描述。`full` 保留长描述、标题与 schema 描述。 |
| `category` | 一个类别名 | 全部 | 只返回该类别。未知类别返回类型化 `BAD_ARGS`。 |

1.6.0 的 43 工具目录实测简版 20,493 字节；`detail: "full"` 为 37,496 字节。简版低于 30,000 字节预算；完整结果需显式请求。62 工具 1.7.0 目录实测简版原始正文 31,367 字节（含末尾换行 31,368 字节），低于 32,000 字节简版预算；完整版原始 52,328 字节（含换行 52,329 字节）。旧的 44 工具测量不适用于新面。

`tool_help` 是单个工具的补充入口：

```json
{ "name": "page_snapshot" }
```

返回包含 `name`、`title`、`category`、`summary`、`description`、`inputSchema` 与 `annotations` 的完整条目。`{ "category": "page" }` 返回该类别工具，`{}` 返回目录索引。`name` 与 `category` 互斥；同时给出时返回 `BAD_ARGS`，消息为 `tool_help accepts name or category, not both`。

每个定义还带有 `_meta["corvus.category"]`，客户端无需解析描述即可分组。每个工具 schema 都接受可选的 `browser_id` 路由字段；省略时使用唯一已连接的浏览器。

## 3. 类别

| 类别 | 数量 | 工具 |
|---|---:|---|
| `page` | 15 | `browser_navigate`、`page_snapshot`、`page_text`、`page_html`、`page_find`、`browser_wait`、`browser_history`、`browser_screenshot`、`page_pdf`、`page_links`、`page_forms`、`page_accessibility`、`browser_dialog`、`browser_emulate`、`browser_batch` |
| `input` | 10 | `browser_click`、`browser_type`、`browser_hover`、`browser_press`、`browser_scroll`、`browser_select`、`browser_eval`、`browser_drag`、`browser_focus`、`browser_highlight` |
| `tabs` | 8 | `tab_list`、`tab_new`、`tab_close`、`tab_switch`、`tab_group`、`tab_duplicate`、`window_list`、`window_new` |
| `network` | 5 | `net_capture`、`net_log`、`net_clear`、`net_intercept`、`browser_cookies` |
| `storage` | 5 | `cookie_set`、`cookie_clear`、`storage_get`、`storage_set`、`storage_clear` |
| `files` | 3 | `upload_file`、`download_list`、`download_wait` |
| `cdp` | 5 | `cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events`、`browser_channel` |
| `sessions` | 4 | `session_list`、`session_steps`、`session_replay`、`session_export` |
| `diagnostics` | 6 | `browser_list`、`metrics`、`logs_tail`、`browser_console`、`health`、`version` |
| `help` | 1 | `tool_help` |
| **合计** | **62** | |

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
| `browser_screenshot` | page | both | 捕获视口、整页或单个元素。 |
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
| `browser_cookies` | network | cdp | 读取浏览器级或页面级 cookie。守护策略 `redact_secrets` 会把每个顶层 cookie 的 `value` 替换为 `[redacted]`。 |
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
| `health` | diagnostics | daemon | 已脱敏的守护进程健康、版本与连接摘要。 |
| `version` | diagnostics | daemon | 产品版本、工具数量与协议版本。 |
| `tool_help` | help | daemon | 目录索引、类别或单个工具的完整帮助。 |
| `page_pdf` | page | both | 将页面保存为 PDF 文件。 |
| `page_links` | page | both | 从文档与开放 shadow root 提取链接。 |
| `page_forms` | page | both | 检查表单及其字段。 |
| `page_accessibility` | page | both | 读取无障碍树。 |
| `browser_drag` | input | both | 从一个元素引用拖拽到另一个。 |
| `browser_focus` | input | both | 聚焦元素并滚动到可见区域。 |
| `browser_highlight` | input | both | 短暂高亮引用或选择器。 |
| `tab_group` | tabs | extension | 将标签页分组并设置标题与颜色。 |
| `tab_duplicate` | tabs | extension | 复制一个标签页。 |
| `window_list` | tabs | extension | 列出浏览器窗口及可选的标签页。 |
| `window_new` | tabs | extension | 创建浏览器窗口。 |
| `cookie_set` | storage | both | 设置一个 cookie，且不回显其值。 |
| `cookie_clear` | storage | both | 按 URL 或域清除 cookie。 |
| `storage_get` | storage | both | 读取页面 local 或 session storage。 |
| `storage_set` | storage | both | 写入页面 local 或 session storage。 |
| `storage_clear` | storage | both | 清除页面 local 或 session storage。 |

## 5. page_snapshot

`page_snapshot` 返回用于引用的页面树。默认输出为紧凑文本；`format: "json"` 保留旧版节点对象。遍历包含开放 shadow root 与同源子 frame；跨源 frame 永不读取。

| 输入 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `tab_id` | string | 活动标签页 | 要捕获的标签页；省略时使用活动标签页。 |
| `mode` | `interactive` 或 `full` | `interactive` | `interactive` 只返回可操作节点；`full` 返回整棵树。 |
| `max_nodes` | 40-3000 的整数 | 300 | 返回节点数上限。截断时 `truncated` 为 true。 |
| `format` | `compact` 或 `json` | `compact` | `compact` 使用下文的行语法；`json` 返回节点对象。 |
| `viewport_only` | boolean | false | 只保留与当前视口相交的节点。 |
| `include_bbox` | boolean | false | 加入视口相对包围盒。 |
| `diff` | boolean | false | 仅紧凑输出：对同一标签页的上一次紧凑快照标记行差异。与 `format: "json"` 同时使用返回类型化 `BAD_ARGS`。 |

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

当页面包含子 frame 时，紧凑结果还带有 `frames`：`{ "total": 2, "included": 1, "skipped": [{ "frame": "f1", "url": "https://other.example/", "reason": "cross_origin" }] }`。`total` 统计所有子 frame，`included` 统计已读取的文档，`skipped` 给出 `cross_origin`、`unavailable` 或 `geometry_unavailable` 原因。被跳过的 frame 绝不会被静默丢弃。

JSON 结果保留同样的元数据，但把 `text` 换成 `nodes`。扩展通道的 JSON 额外包含 `node_cap`、`element_count`、`text_count` 与 `ref_count`。CDP 的 JSON 节点包含 `kind`、`ref`、`tag`、`role`、`name`、`depth`、`states`、`attributes`，以及可选的 `value` 与 `bbox`；CDP JSON 结果不增加扩展专属计数。子 frame 内的 JSON 节点仅在 frame 路径非空时带 `frame`；顶层文档节点省略该字段。

快照中的密码值始终渲染为 `***`。`browser_type` 因通道而异：扩展对密码字段返回 `masked: true`，其他输入返回截断的 `value`；CDP 不返回 `value`，也不返回 `masked`，因此不会返回输入值。启用守护进程 `content_boundary` 策略时，紧凑 `page_snapshot` 文本会用不可信内容标记包裹；JSON 节点不受影响。见 [11-guardrails.zh.md](11-guardrails.zh.md)。

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
7. 节点位于子 frame 时，可选 `frame="<path>"` 作为最后一个字段。顶层文档没有后缀。路径按文档顺序：第一个子 frame 为 `f0`，其第一个子 frame 为 `f0.0`，下一个兄弟为 `f1`。

文本行使用 `# "..."`，文本最多 240 字符，并使用同样的转义；子 frame 内的文本带同样的 `frame` 后缀。`max_nodes` 是顶层文档、开放 shadow root 与已包含 frame 共用的一个预算；截断时把 `truncated` 置为 true。

### 引用与 JSON 兼容

- 扩展引用为 `@eN`，CDP 引用为 `@c<backendNodeId>`。一种通道的引用不能用于另一种通道。跨通道使用时返回类型化 `BAD_ARGS`，`details.hint` 会提示改用匹配的引用，或省略 `tab_id` 对活动标签操作。
- 引用在产生它的页面快照仍有效时可用。导航或 DOM 变化后请重新快照，不要复用旧引用。
- 扩展标签 id 是数字（例如 `7`）；CDP 目标 id 是 `cdp_targets` 返回的 32 位十六进制字符串。当前版本不在两个命名空间之间转换。任一通道都可省略 `tab_id` 使用活动标签页。
- `format: "json"` 是需要节点对象的客户端的兼容路径。`include_bbox` 在两种通道上都影响紧凑行；扩展路径的 JSON 节点本身已包含 `bbox`，`include_bbox` 不改变 JSON。
- `viewport_only` 只做节点包围盒与当前视口的相交判断，不会滚动页面。

### 紧凑差异

`diff: true` 只适用于紧凑输出；与 `format: "json"` 同时使用返回类型化 `BAD_ARGS`。守护进程应用层用最近 64 个快照的基线缓存，按浏览器、会话与标签页为键，在每次调用中计算一次差异。浏览器通道不计算差异，因此扩展与 CDP 路径共用同一语法，`browser_batch` 中的步骤也只计算一次。

`diff: false` 的紧凑请求会刷新基线；差异请求从不刷新，JSON 请求也不触碰基线。若同一 URL 存在基线，结果添加 `diff: { baseline: true, unchanged, changed, added, removed }`，当前每一行都带前缀：未变为两个空格，新增为 `+ `，变化为 `~ `。基线中无当前匹配的行按旧顺序以 `- ` 追加。没有基线，或标签页已切换到不同 URL 时，`diff` 为 `{ baseline: false, unchanged: 0, changed: 0, added: 0, removed: 0 }`，文本不带标记。

### browser_screenshot

`browser_screenshot` 捕获可见视口、整页或单个元素，两种通道都可用。守护进程负责最终处理：图像始终写入 `<home>/screenshots/`，文件名为 `screenshot-<unix-millis>-<uuid8>.<ext>`；结果用 `width`、`height`、`bytes`、`sha256`、`path`、`summary` 与 `inline` 替换 base64 载荷。

| 输入 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `target` | string | `viewport` | `viewport`、`full_page` 或 `element`。 |
| `ref` | string | 无 | 仅 `element` 必填；CDP 用 `@c<backendNodeId>`，扩展用 `@e<number>`。来自另一通道的引用返回类型化 `BAD_ARGS`。 |
| `format` | string | `webp` | `webp`、`png` 或 `jpeg`。 |
| `quality` | integer | `80` | `webp` 与 `jpeg` 取 `1` 到 `100`；`png` 忽略。 |
| `inline` | boolean | `false` | 同时以 MCP 图像内容返回字节；文件始终保存。 |
| `tab_id` | string | 活动标签页 | 要捕获的标签页。 |

`full_page` 使用页面布局尺寸并捕获视口之外的内容；`element` 从快照引用解析节点包围盒并捕获该区域。扩展使用现有调试通道，活动标签页 PNG 视口可回退到 `chrome.tabs.captureVisibleTab`；1.7.0 将扩展提升为 11 项权限、42 个处理程序与 `all_frames: false`。

`inline: true` 仅适用于 MCP：`content[0]` 为摘要文本，`content[1]` 为图像，不返回 `structuredContent`。`browser_batch` 的截图步骤必须设 `inline: false`；`inline: true` 在派发前返回类型化 `BAD_ARGS`。截图对守护策略是只读操作，但像素不会被脱敏或包裹边界，因此图像可能包含秘密或不可信页面内容。

### 1.7.0 新增工具

1.7.0 工作树新增 18 个工具。每个浏览器工具都接受可选 `browser_id`；页面与输入工具还接受 `tab_id`，省略时使用活动标签页。CDP 引用为 `@c<backendNodeId>`，扩展引用为 `@e<number>`；错误类型返回 `BAD_ARGS`。四个仅扩展工具在 CDP 下返回 `CAPABILITY_UNAVAILABLE`。五个存储工具不可用于批量执行。

| 工具 | 类别 | 通道 | 输入 | 结果与说明 |
|---|---|---|---|---|
| `page_pdf` | page | both | `browser_id?`、`tab_id?`、`paper_width` 1-100（8.5 英寸）、`paper_height` 1-100（11 英寸）、`landscape` false、`print_background` true、`scale` 0.1-2（1） | PDF 文件：`format`、`mime_type`、`tab_id`、`url`、`path`、`bytes`、`sha256`、纸张字段与 `summary`；没有内联或 base64 字段。 |
| `page_links` | page | both | `selector?`（512 字符）、`max_results` 1-1000（200）、`include_hidden` false | `url`、`count`、`truncated`、`links[{text,url,title?,rel?,target?}]`；扫描顶层文档与开放 shadow root，不扫描同源 iframe。 |
| `page_forms` | page | both | `selector?`（512 字符）、`include_values` false、`max_forms` 1-100（20） | `url`、`count`、`truncated`、`forms[{index,action,method,name?,id?,fields}]`；永不返回密码值。 |
| `page_accessibility` | page | both | `root?`（512 字符）、`max_nodes` 1-2000（500）、`max_depth` 1-50（12） | `url`、`count`、`truncated`、`nodes[{role,name,value?,description?,disabled?,checked?,expanded?,selected?,level?,depth}]`；没有调试器时返回 `CAPABILITY_UNAVAILABLE`。 |
| `browser_drag` | input | both | `from_ref`、`to_ref`、`steps` 1-50（10） | 指针从一个引用拖到另一个：`dragged`、`from`、`to`、`tab_id`、`url`；不保证 HTML5 拖放。 |
| `browser_focus` | input | both | `ref` | 聚焦并滚动到可见区域：`focused`、`ref`、`tab_id`、`url`。 |
| `browser_highlight` | input | both | `ref` 或 `selector` 恰好一个；`duration_ms` 0-10000（1200）；`color` `#rrggbb`（`#ffd400`） | 短暂高亮：`highlighted`、引用或选择器、`duration_ms`、`tab_id`、`url`。 |
| `tab_group` | tabs | extension | `tab_ids` 1-50 且互不相同、`title`（64 字符）、`color` grey-blue-red-yellow-green-pink-purple-cyan-orange（blue） | `group_id`、`tab_ids`、`title`、`color`、`window_id`；需要 `tabGroups` 权限。 |
| `tab_duplicate` | tabs | extension | `tab_id` | `duplicated`、`source_tab_id`、`tab`。 |
| `window_list` | tabs | extension | `include_tabs` true、`max_windows` 1-50（20） | `count`、`truncated`、`windows`；每个窗口的标签页上限为 200。 |
| `window_new` | tabs | extension | `url?`（4096 字符）、`incognito` false、`focused` true、`width`/`height` 200-10000 | `created`、`window_id`、`tab_id`、`url`、`incognito`、`width`、`height`；仅允许 http、https 与 `about:blank`。 |
| `cookie_set` | storage | both | `name` 1-256、`value` 0-4096、可选 `url`、`domain`、`path`、`secure`、`http_only`、`same_site`、`expires` | `set`、cookie 属性、`tab_id?`、`url`；永不回显值。 |
| `cookie_clear` | storage | both | `url?` 或 `domain?`（恰好一个）、`name?` | `cleared`、选择器与 `names`（上限 100）。 |
| `storage_get` | storage | both | `area` local 或 session（local）、`keys` 1-100 | `area`、`origin`、`count`、`truncated`、`entries[{key,value}]`；录制步骤与守护策略脱敏会替换每个值。 |
| `storage_set` | storage | both | `area`、`entries` 1-100 个字符串值 | `area`、`origin`、`set`、`keys`；永不回显值；总计 64 KiB。 |
| `storage_clear` | storage | both | `area`、`keys` 最多 100 | `area`、`origin`、`cleared`。 |
| `health` | diagnostics | daemon | 无 | `status`、`version`、`uptime_ms`、`started_at`、浏览器计数、`channel`、`database`、`guardrails` 与 `tools:{count:62,categories:10}`；不含路径、URL 或秘密。 |
| `version` | diagnostics | daemon | 无 | `name`、`version`、`tools:62`、`categories:10`、`protocol_version:1`、`schema_snapshot:"v1.7.0"`。 |

`page_pdf` 由守护进程完成文件收尾。它解码 PDF，执行 32 MiB 解码上限（33,554,432 字节），在 `<home>/pdf/` 下以 create-new 语义写入唯一文件 `page-<unix-millis>-<uuid8>.pdf`，只返回元数据。它对守护策略为只读，并被排除在 `browser_batch` 之外；录制会移除原始 `data` 字段，回放以 `page_pdf_not_replayable` 跳过该步骤。

`cookie_set` 与 `storage_set` 永不回显秘密值。录制将 `cookie_set` 的 `value`、每个 `storage_set` 条目值与每个 `storage_get` 结果值替换为 `[redacted]`；守护策略的 `redact_secrets` 独立脱敏 `storage_get` 的值并添加 `redacted: true`。存储类别故意不参与批量执行。

扩展处理全部 16 个新浏览器工具，包括四个仅扩展的标签页与窗口工具，使用现有调试通道并新增 `tabGroups` 权限。`health` 与 `version` 仅由守护进程提供：它们不是扩展处理程序，也不是 CDP 浏览器侧工具。

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

`browser_batch` 不能调用自身、`browser_channel`、`metrics`、`logs_tail`、`page_pdf`、`tab_group`、`tab_duplicate`、`window_new`、`health`、`version`、任何会话工具或 `tool_help`。它可以调用 page、input、tabs、network、files、cdp 与 diagnostics 类别中的 42 个工具，但不包括上述十个；`page_links`、`page_forms`、`page_accessibility`、`browser_drag`、`browser_focus`、`browser_highlight`、`window_list`、`browser_list`、`browser_console` 与 `browser_screenshot` 在允许范围内。storage 类别不参与批量执行，sessions 与 `help` 原本就已排除。`browser_screenshot` 步骤必须设 `inline: false`；`inline: true` 在派发前返回类型化 `BAD_ARGS`。允许列表在执行前校验；不允许的名称返回类型化 `BAD_ARGS`。

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
| `{ "category": "page" }` | `{ "category": "page", "count": 15, "tools": [{ "name", "title", "summary" }] }` |
| `{ "name": "browser_batch" }` | 完整条目：`name`、`title`、`category`、`summary`、`description`、`inputSchema`、`annotations`。 |

未知名称与未知类别返回类型化 `BAD_ARGS`。`tool_help` 由守护进程本地应答，即使没有连接浏览器也始终可用。

## 8. 通道与可用性

`browser_channel` 查看并切换当前模式：

- `auto`（默认）优先使用已连接的 CDP 端点；只有 CDP 路由返回 `Unsupported` 时才回退到扩展。
- `cdp` 要求 CDP，并且从不回退。1.7.0 在强制 CDP 下支持全部 46 个 CDP 浏览器侧工具，包括八个仅 CDP 工具与 CDP 实现的 12 个新浏览器工具。
- `extension` 忽略 CDP。它支持 42 个浏览器侧工具，对八个仅 CDP 工具返回类型化 `CAPABILITY_UNAVAILABLE`。

12 个守护进程本地工具在任何模式下都可用。`browser_channel` 只改变运行中的守护进程；下次启动仍使用 `--channel` 或 `CORVUS_CHANNEL`。

| 可用性分组 | 数量 | 工具 |
|---|---:|---|
| 两种通道 | 38 | page 12、input 10、tabs 4、network 4、files 3，另加 `browser_screenshot` |
| 仅 CDP | 8 | `browser_dialog`、`browser_emulate`、`browser_cookies`、`browser_console`、`cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events` |
| 仅扩展 | 4 | `tab_group`、`tab_duplicate`、`window_list`、`window_new` |
| 守护进程本地 | 12 | `browser_list`、`browser_channel`、`tool_help`、`browser_batch`、四个会话工具、`metrics`、`logs_tail`、`health`、`version` |

扩展仍是零配置路径。强制 `cdp` 的进程无需扩展即可驱动所有浏览器类工具，但浏览器必须用远程调试与专用 profile 启动。端点设置、路由与安全见 [07-cdp.zh.md](07-cdp.zh.md)。

## 9. 错误与重试

每个失败都带类型化错误码。工具调用方最常见的错误：

| 错误码 | 含义 | 重试 |
|---|---|---|
| `BAD_ARGS` | 参数无效、未知工具或类别、跨通道引用、批次步骤不被允许。details 通常带 `hint` 或被拒绝字段。 | 否；修正请求。 |
| `NOT_FOUND` | 目标、会话、标签页或对话框不存在。`browser_dialog` 在没有对话框时返回 `no dialog is showing`。 | 状态变化后。 |
| `CAPABILITY_UNAVAILABLE` | 当前通道或浏览器无法提供该调用，例如强制 `extension` 下的仅 CDP 工具，或强制 `cdp` 但没有端点。 | 切换通道或重新连接。 |
| `POLICY_DENIED` | 守护策略拒绝了工具调用或导航。详情携带 `reason`、工具或 URL，以及 `policy`。 | 否；修改请求或策略。 |
| `BROWSER_GONE` | 调用挂起期间浏览器或其 CDP 连接消失。 | 重新列出标签页或目标后重试。 |
| `TIMEOUT` | 有界等待或命令超过期限。 | 仅幂等操作可重试。 |

`auto` 不会把失败的 CDP 动作通过扩展重放。只有 `Unsupported` 结果可以触发回退。其他传输或协议错误都会返回给调用方。

## 10. 实测体积与版本

候选发布基准比较了紧凑输出与 JSON 兼容路径：

| 指标 | 紧凑文本 | JSON | 缩减 |
|---|---:|---:|---:|
| 快照平均载荷 | 364.83 B / 91.21 tokens | 2,077.33 B / 519.33 tokens | 小 82.44% |
| 工具结果平均载荷 | 646.33 B | 2,077.33 B | 小 68.89% |

token 数使用基准的 tokenizer 估算。1.5.0 QA 报告实测：43 个工具的默认简版 `tools/list` 为 20,493 B（数组 20,449 B），完整详情为 37,372 B（数组 37,328 B）；发布说明引用该报告。1.6.0 报告实测 43 工具简版 20,493 B、完整 37,496 B。62 工具 1.7.0 目录实测简版原始正文 31,367 B（含末尾换行 31,368 B）与完整原始 52,328 B（含换行 52,329 B）；简版正文低于 32,000 字节简版预算。

版本：1.6.0 是当前已发布版本，于 2026-11-15 由标签 `v1.6.0` 发布；1.5.0 为上一版本。QA 验收 PASS 记录于 [v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md)，发布为 GitHub 发布 446071476，主 CI 运行为 43716832915，发布工作流为 43717236667。工作树提供 62 个工具、十个类别，新增 `storage` 类别与 18 个工具；1.7.0 的 QA 验收与本地 Windows 打包已在重新冻结的 62 工具树上通过（25 条结果行上的 263 个 Rust 测试、137 项一致性检查、13 个文件中的 91 个 Vitest 测试、3 个 Playwright 端到端测试与 160/160 项本地打包检查）；不存在 1.7.0 CI 运行、已发布包或已发布产物，因此所有 CI 与发布行仍为 Pending，工作树仍未发布。