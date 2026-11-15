# CDP 与混合传输

[English](07-cdp.md) | [中文](07-cdp.zh.md)

状态：1.6.0 是当前已发布版本，于 2026-11-15 发布；1.5.0 为上一版本。页面快照遍历开放 shadow root 与同源子 frame，紧凑差异由守护进程计算。1.6.0 发布新增在 CDP 派发前运行的守护策略。QA PASS 记录于 [v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md)。读者：希望在扩展传输之外使用浏览器级 CDP 控制的运维与开发人员。

Corvus 1.6.0 可以由 Rust 守护进程通过 Chrome DevTools Protocol（CDP）直接驱动 Chromium 浏览器，也可以继续走 Manifest V3 扩展。扩展仍是零配置路径，也是用户浏览器的默认选择。CDP 是可选路径：它要求浏览器已经在回环调试端口上监听；在 1.6.0 中覆盖全部 33 个浏览器类工具，包括紧凑快照与 `@c` 元素引用。

## 1. 两个通道，一个工具面

MCP 工具面共 43 个。通道只决定浏览器类调用如何传输；原有 32 个结果形状仍可用，并在其旁保留 1.3.0 引入的紧凑快照与批量结果。

| 通道 | 传输 | 适用场景 |
|---|---|---|
| `extension` | 先连扩展 WebSocket，再经 `chrome.*` 与内容脚本 | 使用默认零配置路径、`@eN` DOM 引用或扩展的 25 个浏览器类工具。忽略 CDP。 |
| `cdp` | `corvusd` 到浏览器的单条长连接浏览器级 WebSocket | 浏览器已用 `--remote-debugging-port` 启动，需要全部 33 个浏览器类工具，包括使用 `@c` 引用的 `page_snapshot`。严格模式，不回退。 |
| `auto` | 已连接 CDP 时用 CDP，否则用扩展 | 默认值。优先 CDP，仅在 CDP 调用报告 `Unsupported` 时回退到扩展；传输与协议错误不会改走另一通道重试。 |

`auto` 刻意保守。如果 CDP 调用因为浏览器已关闭或 WebSocket 断开而失败，调用会返回结构化错误，而不是悄悄通过另一个浏览器重放动作。只有真正的「该操作没有等价 CDP 实现」才会触发回退。

## 2. 在 Chromium 浏览器中开启 CDP

普通的浏览器启动不会开启 CDP。请用独立 profile 与远程调试端口启动一个专用 Chromium 实例：

```bash
# Chrome
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile

# Edge
microsoft-edge --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile

# Brave
brave-browser --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile
```

Windows 上可执行文件为 `chrome.exe`、`msedge.exe` 或 `brave.exe`；macOS 上可使用应用内二进制，或用 `open -na "Google Chrome" --args ...`。较新的 Chrome 可能拒绝在默认 profile 上开启远程调试，因此专用 `--user-data-dir` 是可靠选择。

确认浏览器级 WebSocket 地址：

```bash
curl -s http://127.0.0.1:9222/json/version
```

响应中的 `webSocketDebuggerUrl` 即守护进程接受的端点，例如 `ws://127.0.0.1:9222/devtools/browser/6f...`。HTTP 发现地址本身不能作为守护进程端点。

如果浏览器以 origin 错误拒绝 WebSocket，只为被拒绝的 origin 添加 `--remote-allow-origins`，例如：

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile \
  --remote-allow-origins=http://127.0.0.1:9222
```

只有在隔离测试机上、且端口仅限回环时，才使用 `--remote-allow-origins=*`。不要在存有真实账号的机器上扩大端口或 origin 白名单。

## 3. 让扩展发现端点

扩展设置页把 CDP 端点与守护进程 WebSocket 地址分开保存，键为 `corvus.cdpEndpoint`。

| 值 | 行为 |
|---|---|
| `auto`（默认） | 探测 `127.0.0.1` 的 9222、9223、9333 端口，读取 `/json/version`，选择浏览器家族与本扩展匹配的可用端点。 |
| `http://127.0.0.1:9222` | 只探测该回环端点。必须使用 `http://`；主机必须是 `127.0.0.1`、`localhost` 或 `[::1]`；必须给出数字端口；路径、查询与凭据会被拒绝。 |

自动探测并行请求三个端口，超时 800 毫秒。它优先精确家族匹配，其次 Chrome/Chromium 交叉匹配，最后按端口顺序。显式配置的端点即使家族不匹配也不致命，但 `auto` 会拒绝家族不匹配的 descriptor。Chrome 与 Chromium 属于同一家族；Edge、Brave、Opera 与 Vivaldi 各自独立。

握手中的 CDP descriptor 携带端点、WebSocket 调试地址、浏览器、协议版本、来源与家族。扩展只有在探测确实找到可用端点时才声明 `cdp` 能力。守护进程会重新校验每个 descriptor，只接受回环的 `ws://` 或 `wss://` 调试地址。

## 4. 守护进程参数与环境变量

`corvusd serve` 与 `corvus serve` 接受：

| 参数 | 环境变量 | 含义 |
|---|---|---|
| `--cdp-endpoint <ws-url>` | `CORVUS_CDP_ENDPOINT` | 启动时连接该浏览器级 WebSocket 地址。必须是回环 `ws://` 或 `wss://`。 |
| `--no-cdp` | — | 完全关闭 CDP。`auto` 改用扩展；强制 `cdp` 会失败。 |
| `--channel auto|cdp|extension` | `CORVUS_CHANNEL` | 设置启动路由模式，默认 `auto`。 |

示例：

```bash
# 由扩展的 descriptor 选择端点。
corvusd serve --home /opt/corvus --channel auto

# 直接连接；CDP 能力工具不需要扩展。
corvusd serve --home /opt/corvus \
  --cdp-endpoint "ws://127.0.0.1:9222/devtools/browser/6f3a..."

# 提供 CDP 能力工具，但始终走扩展。
corvusd serve --home /opt/corvus --channel extension
```

`corvusd doctor` 为命令兼容也接受这三个 CDP 参数，但它打印的本地检查不会连接浏览器。要检查运行中的守护进程，请通过 MCP 调用 `browser_channel` 与 `cdp_status`。

## 5. 查看与切换通道

不带参数的 `browser_channel` 返回通道状态文档：

```json
{ "action": "status" }
```

结果包含 `channel`、`cdp_enabled`、CDP 状态、扩展连接与浏览器 id、CDP 工具名、最近的扩展 descriptor，以及适用时的 `descriptor_error` 或 `browser_mismatch`。

运行时切换：

```json
{ "action": "set", "channel": "cdp" }
```

`browser_channel set` 只改变运行中的守护进程；下次启动仍使用 `--channel` 或 `CORVUS_CHANNEL`。在 `--no-cdp` 启动的进程上设置 `cdp` 会返回结构化错误。切换模式不会重启浏览器，也不会清空 CDP 事件环。

## 6. 状态与目标

`cdp_status` 报告连接与计数：

```json
{}
```

结果在核心状态之外增加 `event_capacity`（1000）、`event_count` 与 CDP 工具名列表。未连接时仍会应答；在假定端点健康之前，先看 `last_error` 与 `reconnects`。

`cdp_targets` 列出客户端已知的页面目标：

```json
{}
```

每个目标包含 `target_id`、`target_type`、`title`、`url`、`attached`、可选 `session_id` 与可选 `status`。目标与会话 id 属于当前连接；重连后请重新列出目标，不要复用旧 id。

## 7. 原始 CDP 工具

`cdp_send` 发送一条协议命令：

```json
{ "method": "Runtime.evaluate", "params": { "expression": "document.title" }, "target_id": "<target-id>" }
```

| 参数 | 含义 |
|---|---|
| `method` | 必填协议方法，例如 `Runtime.evaluate` 或 `Page.navigate`。 |
| `params` | 可选对象，默认 `{}`。 |
| `session_id` | 可选扁平会话 id，优先于 `target_id`。 |
| `target_id` | 可选页面目标；未给会话时按需挂载。 |
| `timeout_ms` | 1..60000，默认 10000。 |

结果为 `{ "method": "...", "result": ... }`。未给会话或目标时，Corvus 使用活动页面目标；没有页面目标时调用返回 `CAPABILITY_UNAVAILABLE`。

`cdp_events` 读取有界事件环：

```json
{ "since_seq": 0, "limit": 100, "method": "Runtime.consoleAPICalled", "wait_ms": 1000 }
```

| 参数 | 含义 |
|---|---|
| `since_seq` | 返回该序号之后的事件，默认 0。 |
| `limit` | 1..500，默认 100。 |
| `method` | 可选精确方法过滤。 |
| `wait_ms` | 0..30000，默认 0。大于零时等待事件后再应答。 |
| `clear` | 读取后丢弃已保留事件，默认 false。 |

事件环保留 1000 条。`next_seq` 是下次读取游标，`dropped` 统计读者看到之前已被淘汰的事件。事件序号在守护进程生命周期内单调递增，而不是单条连接内。

### CDP 上的快照与引用

`page_snapshot` 在两个通道上都可用。走 CDP 时，紧凑输出使用 `@c<backendNodeId>` 元素引用，行语法与扩展通道相同：

```text
@c42 button "保存" [focused] bbox=40,120,96,32
@c43 textbox "姓名" value="Ada"
# "Hello world"
```

`format: "json"` 返回节点对象而不是文本；`viewport_only`、`include_bbox` 与 `max_nodes` 的行为见 [09-tools.zh.md](09-tools.zh.md)。引用只在产生它的快照内有效；导航或 DOM 变化后请重新快照。

在 CDP 通道上，快照同样遍历开放 shadow root 与同源子 frame。顶层文档没有 frame 路径；子 frame 使用文档顺序路径，例如 `f0`、`f0.0` 与 `f1`。子 frame 内的内容会在紧凑行末尾追加 `frame="<path>"`，JSON 节点仅在路径非空时带 `frame`。页面有子 frame 时，结果添加 `frames: {total, included, skipped: [{frame, url, reason}]}`；`reason` 为 `cross_origin`、`unavailable` 或 `geometry_unavailable`。跨源 frame 的 DOM 永不读取，`max_nodes` 是全部已包含文档共用的一个预算，包围盒投影到顶层视口。紧凑差异由守护进程应用层计算，而不是 CDP；差异契约见 [09-tools.zh.md](09-tools.zh.md)。

`@c` 引用仅属于 CDP，`@eN` 引用仅属于扩展。交给另一通道会返回类型化 `BAD_ARGS`；省略 `tab_id` 可对活动目标操作。此前仅扩展的八个工具现在在强制 `cdp` 下也接受 `@c` 引用；1.4.0 中仅扩展可用性为零。

### 跨通道的标签 id

1.4.0 中不同通道的标签 id 不能互换。扩展使用数字标签 id（数字或数字字符串，例如 `7`）；CDP 使用 `cdp_targets` 返回的 32 位十六进制 `target_id`。守护进程不会在两种命名空间之间做转换。

数字扩展 id 交给路由到 CDP 的工具时，调用返回类型化 `BAD_ARGS`；`details.hint` 为 `omit tab_id to use the active tab; extension tab ids are not CDP target ids`。32 位十六进制 CDP 目标 id 交给路由到扩展的工具时同样失败；`details.hint` 为 `omit tab_id to use the active tab; CDP target ids are not extension tab ids`。`details.tab_id` 回显被拒绝的值。请使用当前通道返回的 id，或省略 `tab_id` 对活动标签操作。两个命名空间在 1.4.0 中仍相互独立；完整桥接推迟到后续版本。

`cdp_targets` 是 CDP 目标 id 的权威列表；`tab_list` 返回实际服务该调用的通道所用的 id。

## 8. CDP 支撑的高层工具

| 工具 | 用途 |
|---|---|
| `browser_console` | 启用 Runtime，短暂等待后返回近期 console 或异常条目，最新在最后。参数：`tab_id?`、`limit?` 1..500 默认 100。 |
| `browser_cookies` | 不给 `urls` 时通过 `Storage.getCookies` 读取浏览器级 cookie；给最多 20 个 `urls` 时通过 `Network.getCookies` 读取页面 cookie。 |
| `browser_dialog` | 通过 `Page.handleJavaScriptDialog` 应答 JavaScript 对话框。参数：`tab_id?`、`accept?` 默认 true、`prompt_text?`。 |
| `browser_emulate` | 应用或重置设备指标、网络条件、user agent 与时区。`reset: true` 清除这些设置。 |

`browser_emulate` 设置设备指标时必须同时给出 `width` 与 `height`。网络字段为 `offline`、`latency_ms`、`download_throughput` 与 `upload_throughput`；吞吐量 `-1` 表示关闭对应限制。结果列出已应用的项目。

### 事件、下载与对话框

每次建立 CDP 连接时，Corvus 都会在开始服务命令前调用 `Browser.setDownloadBehavior`，参数为 `behavior: default` 与 `eventsEnabled: true`，使下载事件可供 `download_list` 与 `download_wait` 使用。如果浏览器拒绝该命令，Corvus 记录警告，该连接上的下载工具无法观察新事件；其他 CDP 工具仍可用。

`Target.attachToTarget` 返回会话后，守护进程按会话启用 `Page`、`Runtime` 与 `Log`，超时上限五秒且失败不致命。因此事件环只包含该次挂载之后产生的事件，不会回填更早的历史。重连后请重新挂载；守护进程会为新会话再次启用这三个域。

没有打开的对话框时，`browser_dialog` 返回类型化 `NOT_FOUND`，消息为 `no dialog is showing`。把它当作状态检查，不要当作传输故障。

## 9. 重连、心跳与请求生命周期

守护进程只保留一条浏览器级 WebSocket。请求按数字 id 关联；页面操作使用扁平的 `Target.attachToTarget` 会话。重连退避从 250 毫秒到 10 秒，心跳每 15 秒一次，默认请求超时 10 秒、最大 60 秒。套接字断开时未完成请求会失败，不会自动重放。每次成功重连都会增加 `cdp_status.reconnects`。

连接是有状态的，不要在重连后缓存 `session_id` 或 `target_id`。重新读取 `cdp_targets` 并再次挂载。事件环在守护进程进程内跨重连保留，但旧的挂载会话不会保留。重新挂载页面后，每次新挂载都会为该会话启用 `Page`、`Runtime` 与 `Log`，因此只可见新挂载之后的事件。

## 10. 按通道划分的工具可用性

| 分组 | 数量 | 工具 |
|---|---|---|
| 两种通道 | 25 | `browser_navigate`、`page_snapshot`、`page_text`、`page_html`、`page_find`、`browser_wait`、`browser_history`、`browser_click`、`browser_type`、`browser_hover`、`browser_press`、`browser_scroll`、`browser_select`、`browser_eval`、`tab_list`、`tab_new`、`tab_close`、`tab_switch`、`net_capture`、`net_log`、`net_clear`、`net_intercept`、`upload_file`、`download_list`、`download_wait` |
| 仅 CDP | 8 | `cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events`、`browser_console`、`browser_cookies`、`browser_dialog`、`browser_emulate` |
| 仅扩展 | 0 | 1.4.0 中没有。 |
| 守护进程本地 | 10 | `browser_list`、`browser_channel`、`tool_help`、`browser_batch`、`session_list`、`session_steps`、`session_replay`、`session_export`、`metrics`、`logs_tail` |

合计 43 个。守护进程本地工具不经过任何浏览器通道。CDP 覆盖全部 33 个浏览器类工具；八个仅 CDP 工具仍不对扩展开放。强制 `cdp` 时所有浏览器类工具都接受；强制 `extension` 时仅 CDP 工具返回 `CAPABILITY_UNAVAILABLE`。`auto` 是唯一可能在通道间移动调用的模式，且只针对 `Unsupported` 的 CDP 结果。

## 11. 故障排查

| 现象 | 检查 | 处理 |
|---|---|---|
| 9222、9223、9333 都找不到端点 | `curl -s http://127.0.0.1:9222/json/version` | 用 `--remote-debugging-port` 启动浏览器；确认端口只在回环监听。 |
| 找到端点但浏览器家族不匹配 | `browser_channel` 状态出现 `browser_mismatch` | 所选端点属于其他浏览器家族。改用匹配的浏览器；只有确实需要时才显式设置扩展端点。 |
| 远程调试被拒绝 | 浏览器 stderr 出现 origin 拒绝 | 用 `--remote-allow-origins` 为精确 origin 重启浏览器。不要扩大监听地址。 |
| WebSocket 地址返回 404 | 地址取自 `/json/list` 而不是 `/json/version` | 使用 `/json/version` 返回的浏览器级 `webSocketDebuggerUrl`。 |
| DevTools 与 Corvus 在标签上冲突 | DevTools 打开时出现挂载错误或 `CAPABILITY_UNAVAILABLE` | 关闭该标签的 DevTools，或改用专用浏览器 profile。标签级 debugger 挂载是独占的。 |
| `browser_channel set cdp` 失败 | `cdp_enabled` 为 false | 去掉 `--no-cdp` 重启后再设置通道。 |
| 强制 `extension` 下仅 CDP 工具失败 | 强制扩展模式无法访问 CDP | 切换到 `auto` 或 `cdp`，或使用扩展的 25 个浏览器类工具。1.4.0 中 `page_snapshot` 不再是仅扩展工具。 |
| 起初正常，浏览器重启后停止 | `cdp_status.connected` 为 false，`reconnects` 不再增加 | 浏览器重启改变了 WebSocket 地址。端点来自扩展 descriptor 时，守护进程每 500 毫秒重新轮询，选择替换端点并自动重连；随后 `cdp_status.reconnects` 增加，无需重启守护进程。用 `--cdp-endpoint` 固定的地址不会被重新发现；只有浏览器打印的 `webSocketDebuggerUrl` 确实变化时才更新该参数。 |
| 运行时修改了已配置的 CDP 端点 | 守护进程已连接时在扩展中修改端点 | 自动发现的端点每 500 毫秒重新轮询并自动重连；显式配置的端点变化可能需要重启守护进程才能生效。更新地址后按需重启，并用 `cdp_status` 确认新端点。 |

## 12. 安全与限制

CDP 不是沙箱。浏览器级端点可以读取 cookie、注入脚本、导航、拦截流量，并控制该浏览器实例中的所有 profile。Corvus 不在其前面增加认证。端点校验只接受回环主机与 `ws://` 或 `wss://`，但任何能访问调试端口的本地进程都拥有同等能力。

- 绝不要把 9222、9223、9333、7330 或 7331 暴露到回环之外。
- CDP 工作使用专用 profile；不要把测试工具挂到存有真实账号的日常浏览器 profile。
- 把 CDP 事件环、console 输出与 cookie 结果视为敏感数据。
- 守护进程不启动也不安装浏览器。Docker 与 Kubernetes 默认无法访问宿主机回环，随包镜像也不含浏览器；该限制见 [05-deployment.zh.md](05-deployment.zh.md#7-端口暴露面与认证边界)。
- 扩展仍是零配置路径，以 `@eN` 引用提供 25 个浏览器类工具。CDP 现在以 `@c` 引用提供全部 33 个浏览器类工具；任一通道都不是任意失败下的回退路径。

## 13. 测试策略

不要用自动化测试驱动用户日常的 Edge 或 Chrome 窗口。请用独立 `--user-data-dir`、临时端口且未登录账号的专用浏览器。这是测试策略，不是产品限制：守护进程会驱动交给它的任何回环 CDP 端点，选择该端点是运维责任。

安装与升级步骤见 [04-install.zh.md](04-install.zh.md)。容器与 Kubernetes 限制见 [05-deployment.zh.md](05-deployment.zh.md)。通道路由架构见 [02-architecture.zh.md](02-architecture.zh.md#56-混合-cdp-传输)。完整工具目录与紧凑语法见 [09-tools.zh.md](09-tools.zh.md)。
