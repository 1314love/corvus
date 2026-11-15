# Corvus 需求规格说明书 v0.1

[English](01-requirements.md) | [中文](01-requirements.zh.md)

读者：产品负责人与实现者。工程内部细节见 02-architecture.md。

## 1. 用户与场景

**用户画像**：单机、单用户、本机常驻。用户把浏览器交给 AI 托管，期待 AI 像人一样操作浏览器，而不是像机器人一样在每次点击前请求许可。

**典型场景**

1. **长任务托管**：AI 连续操作十几个页面、跨越登录态、下载文件、填表单。中途不能有人工确认弹窗，也不能因为扩展休眠而中断。
2. **多浏览器并行**：同一个任务里，Chrome 查资料、Edge 登录企业系统、Firefox 验证前端兼容性。
3. **事后复盘**：任务失败后，回放整段会话，逐帧看 AI 看到了什么、点了什么、网络返回了什么。
4. **能力复用**：同一套工具被 Claude Code、Cursor、DSH、Cline 同时接入，换客户端不换肌肉记忆。

## 2. 术语

| 术语 | 含义 |
|---|---|
| daemon | corvusd 进程，MCP 服务端与命令路由中心 |
| extension | 浏览器侧 MV3 扩展，无页面，仅一个只读状态弹窗 |
| transport | daemon 与 extension 之间的通道：WS / Native Messaging / CDP 直连 |
| session | 一次可录制的操作序列，包含命令、响应、页面快照、网络事件 |
| site card | 某个域名下被验证过的选择器、流程与坑，写入 SQLite |
| tool | 暴露给 MCP 客户端的一个函数 |

## 3. 功能需求

### 3.1 连接与生命周期（FR-C）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-C1 | 扩展安装后自动发现本机 daemon，无需配对码、无需点击 | P0 | 冷启动到首个命令 < 60s，0 次人工点击 |
| FR-C2 | 三通道自动降级：WS -> Native Messaging -> 扩展主动长轮询 | P0 | 封掉 3001 端口后 3s 内自动切换并恢复命令 |
| FR-C3 | daemon 重启后扩展重连，未完成命令按幂等键重放 | P0 | kill daemon 后重启，进行中的命令最终成功且不重复执行 |
| FR-C4 | 看门狗：SW 休眠、扩展卸载、浏览器升级后自动恢复 | P0 | 连续 72h 压测无人工干预 |
| FR-C5 | 多浏览器、多 profile 同时在线，按 browserId 路由 | P0 | 3 个浏览器同时执行命令互不干扰 |
| FR-C6 | 每个浏览器可单独禁用 / 启用 | P1 | 禁用后该浏览器工具调用立即返回明确错误码 |

### 3.2 感知（FR-P）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-P1 | 混合快照：可访问性树 + 精简 DOM，带稳定引用 id | P0 | 复杂页面（GitHub 仓库页）快照 < 300ms，引用可直接用于 click |
| FR-P2 | 文本 / HTML / 链接 / 表单结构提取 | P0 | 与浏览器 DevTools 结果一致 |
| FR-P3 | 截图：视口、元素、整页；支持 DPR 与 WebP | P0 | 整页截图在 20000px 高页面成功 |
| FR-P4 | 控制台日志、未捕获异常、网络请求瀑布 | P0 | 时间戳与 DevTools 一致，误差 < 50ms |
| FR-P5 | 等待原语：元素出现/消失、文本出现、网络空闲、JS 条件成立 | P0 | 每种等待均可设超时并返回结构化失败原因 |
| FR-P6 | 页面变更订阅：DOM 变化、导航、下载开始 | P1 | 变更通过 MCP 通知推送给客户端 |

### 3.3 动作（FR-A）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-A1 | 导航、前进、后退、刷新，保留历史 | P0 | 单页应用路由变化可被识别 |
| FR-A2 | 点击（原生事件序列，含 pointer/mouse/触摸三态） | P0 | 点击 GitHub 按钮、菜单、SVG 图标均有效 |
| FR-A3 | 输入：受控组件原生 setter + InputEvent，支持清空、追加、逐字输入 | P0 | React / Vue / Angular 表单均不回滚 |
| FR-A4 | 键盘：组合键、快捷键、逐键序列 | P0 | Ctrl+A / Enter / Escape / Tab 顺序生效 |
| FR-A5 | 悬停、拖拽、下拉选择、滚动（含无限滚动） | P0 | HTML5 拖拽与 canvas 拖拽均可 |
| FR-A6 | 文件上传（input 与拖拽两种路径）与下载（含等待完成、获取落盘路径） | P0 | 单文件 100MB 上传成功，下载完成事件可等待 |
| FR-A7 | 执行 JS：页面世界与隔离世界两种作用域，可返回值与异常 | P0 | 可读取 window 变量、可调用页面函数 |
| FR-A8 | 语义定位：按角色、文本、标签、placeholder 查找元素，不依赖脆弱的 CSS | P1 | 页面改版后定位成功率 >= 90% |
| FR-A9 | 高亮与调试辅助：给元素加临时描边，便于截图取证 | P2 | 高亮不影响页面布局 |

### 3.4 标签与窗口（FR-T）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-T1 | 列出、新建、关闭、切换、复制标签 | P0 | 关闭当前标签后自动切到相邻标签并返回新 id |
| FR-T2 | 标签分组与批量操作 | P1 | 可把 5 个标签放进同一分组并关掉整组 |
| FR-T3 | 多标签并发执行同一批命令 | P0 | 10 个标签并行导航，无串话 |
| FR-T4 | 窗口级操作：新建窗口、全屏、置前 | P2 | 置前不影响用户正在输入的内容 |

### 3.5 存储与身份（FR-S）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-S1 | 读取/写入/清除 cookie，支持按域、路径、SameSite 过滤 | P0 | 写入的 cookie 在页面刷新后仍生效 |
| FR-S2 | localStorage / sessionStorage / IndexedDB 键级读写 | P1 | 大对象（>1MB）可读写 |
| FR-S3 | 自定义请求头（User-Agent、Referer、鉴权头）注入 | P1 | 注入后服务端可见 |
| FR-S4 | 登录态快照：导出/导入某站点的 cookie 集合 | P2 | 跨机器导入后可免登录 |

### 3.6 网络（FR-N）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-N1 | 请求/响应捕获，含 body（可配置上限）、耗时、状态 | P0 | 与 DevTools Network 面板条目一致 |
| FR-N2 | 请求拦截与改写（mock 响应、改请求体） | P1 | 拦截规则支持 URL 正则与优先级 |
| FR-N3 | 阻断与限速（离线、3G、慢网） | P1 | 限速后页面加载耗时符合预期区间 |
| FR-N4 | 等待某个请求完成（按 URL 匹配） | P0 | 作为 P5 等待原语的一种 |

### 3.7 调试与仿真（FR-D）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-D1 | debugger 通道：可读取 CDP 域能力（DOM、Network、Runtime、Emulation） | P1 | 始终声明能力，按需挂载；仅当浏览器拒绝挂载时返回 CAPABILITY_UNAVAILABLE |
| FR-D2 | 设备仿真：视口尺寸、DPR、触摸、UA | P1 | 仿真后页面 matchMedia 结果正确 |
| FR-D3 | 地理、时区、语言、暗色模式仿真 | P2 | navigator 相关字段同步变化 |
| FR-D4 | PDF 导出与整页截图分离 | P2 | 导出文件可被 pdf 阅读器打开 |

### 3.8 观测与记忆（FR-O）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-O1 | 会话录制：命令、响应、快照、截图、网络事件按时间轴落盘 | P0 | 录制开销 < 5% CPU |
| FR-O2 | 回放：按步前进/后退，可跳到任意命令 | P0 | 1000 步会话回放 UI 无卡顿 |
| FR-O3 | 导出：JSONL / HTML 报告 / Perfetto trace | P1 | 报告可离线打开 |
| FR-O4 | 站点记忆：成功的定位与流程写入 SQLite，下次优先复用 | P1 | 同站点二次任务步骤数下降 >= 20% |
| FR-O5 | 指标：Prometheus 端点暴露命令延迟、成功率、连接状态 | P2 | 可被 Grafana 直接抓取 |

### 3.9 系统与集成（FR-I）

| 编号 | 需求 | 优先级 | 验收 |
|---|---|---|---|
| FR-I1 | MCP 服务端：stdio 与 streamable-http 双栈，遵循 MCP 2025-06-18 | P0 | 至少 3 个客户端连接成功 |
| FR-I2 | 工具描述面向 LLM 优化：短、含示例、含错误码 | P0 | 模型首次调用成功率 >= 95% |
| FR-I3 | 客户端配置生成：一条命令打印 Claude/Cursor/DSH/Cline 的配置片段 | P1 | 复制粘贴即用 |
| FR-I4 | 健康检查与自检：doctor 命令输出环境、权限、端口、扩展状态 | P0 | 新机器上能定位 90% 的安装问题 |
| FR-I5 | 单二进制分发，无运行时依赖 | P0 | Windows/macOS/Linux 各一个可执行文件 |

## 4. MCP 工具矩阵（目标 >= 60）

感知：page_snapshot, page_text, page_html, page_screenshot, page_pdf, page_console, page_network, page_accessibility, page_links, page_forms, page_find, page_wait, page_observe

动作：browser_navigate, browser_back, browser_forward, browser_reload, browser_click, browser_type, browser_press, browser_hover, browser_drag, browser_select, browser_scroll, browser_focus, browser_upload, browser_download, browser_eval, browser_highlight

标签窗口：tab_list, tab_new, tab_close, tab_switch, tab_group, tab_duplicate, window_list, window_new

存储身份：cookie_get, cookie_set, cookie_clear, storage_get, storage_set, storage_clear, header_set

网络：net_capture_start, net_capture_stop, net_list, net_wait, net_mock, net_block, net_throttle

仿真调试：emulate_device, emulate_geo, emulate_timezone, emulate_media, debug_attach, debug_detach

会话记忆：session_start, session_stop, session_replay, session_export, trace_start, trace_stop, site_card_get, site_card_put

系统：browser_list, browser_attach, browser_detach, extension_status, health, doctor, version

### 4.1 M2 第一波需求覆盖

M2 第一波 18 个工具已全部落地。以下需求在该工具面中已有实现路径；只覆盖一部分的已明确标注，未列出的部分仍未完成。

- **FR-P1** —— `page_snapshot` 生成带稳定引用的混合树；`page_find` 返回同一引用空间的引用。
- **FR-P2** —— `page_text` 与 `page_html` 覆盖文本与 HTML 提取；链接与表单结构提取仍未完成。
- **FR-P5** —— `browser_wait` 覆盖选择器、文本与 URL 条件并支持超时；元素消失、网络空闲与 JS 条件等待仍未完成。
- **FR-A1** —— `browser_navigate` 与 `browser_history` 覆盖导航、后退、前进与刷新。
- **FR-A4** —— `browser_press` 覆盖组合键、命名键与逐键序列。
- **FR-A5** —— `browser_hover`、`browser_select` 与 `browser_scroll` 覆盖悬停、选择与滚动；拖拽仍未完成。
- **FR-A7** —— `browser_eval` 覆盖页面世界与隔离世界求值，并返回结构化异常。
- **FR-A8** —— `page_find` 覆盖文本与角色查找，并匹配表单控件的可访问标签与占位符。
- **FR-T1** —— `tab_list`、`tab_new`、`tab_close` 与 `tab_switch` 覆盖列出、新建、关闭与切换；复制标签仍未完成。

### 4.2 M2 第二波需求覆盖

M2 第二波新增 8 个工具。以下需求在该工具面中已有实现路径；只覆盖一部分的已明确标注，未列出的部分仍未完成。

- **FR-C5** —— `browser_list` 返回实时浏览器集合，每个面向浏览器的工具都接受 `browser_id`，把一次命令路由到一个已连接浏览器。未给出 id 且连接多个浏览器时返回 `BAD_ARGS`，`details.reason` 为 `multiple_browsers`；没有浏览器或 id 未知时返回 `TRANSPORT_DOWN`。按浏览器禁用/启用（FR-C6）仍未完成。
- **FR-N1** —— `net_capture` 与 `net_log` 覆盖请求方法、URL、状态、MIME、资源类型、缓存标记、耗时、错误与可选请求/响应头，数据来自每标签 2000 条的环形缓冲；响应 body 仍未完成。
- **FR-N2** —— `net_intercept` 覆盖 URL 正则规则与 block、mock、continue 三种动作；mock 可设置状态、头与 base64 body，按数组顺序首个匹配生效。请求 body 改写仍未完成。
- **FR-N3** —— `block` 动作覆盖请求阻断；限速与离线仿真仍未完成。
- **FR-A6** —— `download_list` 与 `download_wait` 覆盖下载跟踪与完成等待；`upload_file` 覆盖经 CDP 的文件 input 上传，隐藏 input 也可用。拖拽路径上传与返回本地下载路径仍未完成。

### 4.3 M3 第一波需求覆盖

M3 第一波新增 6 个工具，工具面达到 32 个。以下需求在该工具面中已有实现路径；只覆盖一部分的已明确标注，未列出的部分仍未完成。

- **FR-O1** —— 一个 MCP 会话中的每次 `tools/call` 都记录工具、参数、结果、错误码、耗时与解析后的 `browser_id`/`tab_id`；`session_list` 与 `session_steps` 读取该时间线。快照、截图与网络事件作为独立时间线行仍未完成。
- **FR-O2** —— `session_replay` 在 `dry_run` 下为一段录制生成计划，在 `live` 下重新执行，跳过控制工具与标签工具，默认在首个错误停止（除非设置 `continue_on_error`），并把本次运行记录为回放会话。交互式逐步前进/后退控制仍未完成。
- **FR-O3（部分）** —— `session_export` 仅输出 JSONL；HTML 报告与 Perfetto trace 仍未完成。
- **FR-O5（部分）** —— `metrics`、`logs_tail` 与 `GET /metrics` 提供计数器、5000 条日志环形缓冲与 Prometheus 文本；Grafana 面板仍未完成。

仍未完成：站点记忆（FR-O4）、上述 FR-O1、FR-O2、FR-O3 与 FR-O5 的剩余范围，以及存储与身份族（FR-S1–FR-S4）。按请求等待与网络空闲等待（FR-N4、FR-P5）也仍未完成。

### 4.4 M4 第一波需求覆盖

M4 第一波不新增工具。它在现有 32 个工具面上加入作用域门控与 MCP 取消。

- **FR-T3** —— 使用不同显式 `tab_id` 的调用在不同通道并行，同一浏览器标签内的调用保持有序，因此 10 个标签级动作可以并发执行而无串话。并发由独立调用表达；本波不新增单独的批量原语。
- **FR-A1–FR-A8** —— 任何进行中的动作 `tools/call` 都可以通过 `notifications/cancelled` 取消：请求以 JSON-RPC 错误 `-32800` 结束，`session_steps` 记录 `ok: false` 与 `error_code` `"CANCELLED"`。等待、下载、求值与导航采用协作式中止；其他动作可能延迟完成，已经生效的效果不会回滚。
- **FR-O1** —— 录制时间线现在记录被取消的尝试，`error_code` 为 `"CANCELLED"`。快照、截图与网络事件行仍未完成。

仍未完成：FR-T3 没有批量调度 API；取消是协作式的，且不回滚已生效效果；FR-O1 仍缺快照、截图与网络事件行。4.1 至 4.3 中其余 M2 与 M3 缺口保持不变。

### 4.5 v1.1.0 部署需求覆盖

v1.1.0 是部署发布切片，不新增 MCP 工具，32 个工具面保持不变。

- **单根目录布局** —— 所有后端文件都位于 `--home <dir>` 或 `CORVUS_HOME` 下：`bin/`、`data/corvus.db`、`logs/`、`config/` 与 `extension/`。数据库解析顺序为 `--db`、已配置根目录、`CORVUS_DB_PATH`、平台默认值。`doctor` 报告当前数据库路径、schema 版本、会话与步骤数量。这是 FR-S1–FR-S4 的运维基础，不代表它们已完成；存储与身份族仍未完成。
- **NFR-5（部分）** —— Windows 安装器使用单一 `%ProgramData%\Corvus` 根目录，Linux 安装包使用 `/opt/corvus` 与 systemd unit。Linux 打包仅支持 x86_64；macOS 在本版本仍无安装包，arm64 Linux 包仍未完成。
- **部署打包** —— Docker amd64 镜像与 Kubernetes 清单把同一个单根目录放在 `/data`。打包与容器检查在 CI 中运行，不构成托管服务。
- **NFR-3（未完成）** —— 干净的 72 小时压测于 2026-11-14T14:41:37Z 启动，在客户要求下于约八小时后停止（最后检查点 8：2026-11-14T22:41:37Z）。计划中的 72 小时运行未完成。较短的压测只能作为过程证据。NFR-3 未通过，1.1.0 也不能宣称通过 72 小时压测。
- **NFR-6（已记录偏差）** —— Firefox 仍不受支持。受支持的浏览器为 Edge 120+、Chrome 120+ 与最新 Brave；本版本不宣称支持 Firefox。

### 4.6 v1.2.0 混合传输需求覆盖

v1.2.0 新增十个工具，其中包括八个 CDP 后端工具，MCP 工具面从 32 个增至 42 个。原有 32 个工具的参数与结果形状保持不变。

- **扩展端 CDP 探测** —— 扩展探测回环端口 9222、9223 与 9333，读取 `/json/version`，并优先选择同一浏览器家族。只有找到有效端点时才声明 `cdp` 能力；握手携带端点、浏览器、协议版本与家族，由守护进程重新校验。
- **直接 CDP 客户端** —— 守护进程保持一条浏览器级长连接 WebSocket，页面操作使用扁平 `Target` 会话，保留 1000 条事件环，重连退避 250 毫秒至 10 秒，每 15 秒心跳，默认请求超时 10 秒、最大 60 秒。
- **通道选择** —— `auto`、`cdp` 与 `extension` 可在启动时（`--channel`、`CORVUS_CHANNEL`）和运行时（`browser_channel`）选择。`auto` 优先 CDP，仅在 CDP 返回 `Unsupported` 时回退扩展；`cdp` 严格不回退；`extension` 忽略 CDP。`--no-cdp` 完全移除 CDP 路径。
- **回退与工具可用性** —— 八个工具仅扩展、八个仅 CDP、十七个共享、九个守护进程本地。强制通道下无法执行的工具返回 `CAPABILITY_UNAVAILABLE`，不会静默切换。
- **原始与高层 CDP** —— `cdp_send` 与 `cdp_events` 直接暴露协议与事件环；`browser_console`、`browser_cookies`、`browser_dialog` 与 `browser_emulate` 覆盖常见场景，无需原始协议调用。`tool_help` 按需说明任意工具。
- **MCP 分类与 token 预算** —— `tools/list` 提供两级描述：42 个工具的默认简版为 29,971 字节，完整详细模式为 35,552 字节。长描述由 `tool_help` 承载，常规列表保持精简。
- **NFR-3（未完成）** —— 干净的 72 小时压测在客户要求下于约八小时后停止。NFR-3 未通过，1.2.0 也不能宣称通过 72 小时压测。
- **NFR-6（已记录偏差，未变化）** —— Firefox 仍不受支持。受支持的浏览器为 Edge 120+、Chrome 120+ 与最新 Brave；本版本不宣称支持 Firefox。

### 4.7 v1.3.0 token 效率需求覆盖

v1.3.0 新增 `browser_batch`，工具面达到 43 个、覆盖九个分类，并降低默认上下文开销，同时不改变原有 32 个工具的结果形状。

- **精简快照** —— `page_snapshot` 在扩展与 CDP 通道上都默认返回紧凑文本；`format: "json"` 保留对象形式。紧凑引用在扩展通道为 `@eN`，在 CDP 通道为 `@c<backendNodeId>`。`max_nodes`、`viewport_only`、`include_bbox` 与行语法见 [09-tools.zh.md](09-tools.zh.md)。
- **批量调用** —— `browser_batch` 在守护进程本地一次执行最多 20 个 `{tool,args}` 步骤，默认在首个失败处停止，并排除递归与守护进程控制类工具。
- **精简发现** —— 43 个工具的简版 `tools/list` 为 20,452 字节；`detail: "full"` 为 37,067 字节；`tool_help` 返回单个完整条目。成功的 `tools/call` 只返回一个紧凑 JSON 文本块，不含 `structuredContent`。
- **CDP 对等** —— 强制 CDP 可执行全部 33 个浏览器类工具，包括此前仅扩展的八个工具；仅扩展工具已不再单独成组。跨通道引用返回类型化 `BAD_ARGS`。
- **NFR-3（未完成）** —— 1.3.0 不能宣称通过 72 小时压测；此前在约八小时停止后，72 小时运行仍未完成。**NFR-6（未变化）** —— Firefox 仍不受支持；受支持浏览器为 Edge 120+、Chrome 120+ 与最新 Brave。

### 4.8 v1.4.0 结构与 REST 需求覆盖

v1.4.0 不新增 MCP 工具：1.3.0 的 43 个工具、九个类别、紧凑单文本信封与 `browser_batch` 均保持不变。本版本重构后端，并新增独立的 REST 面。

- **端口与适配器。** `corvus-domain` 承载无 I/O 的纯领域模型。`corvus-application` 承载用例与 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，以及原有 32 条路由器规格。`corvus-cdp`、`corvus-transport`、`corvus-record`、`corvus-mcp` 与 `corvus-api` 是适配器；`corvusd` 是装配根。`corvus-runtime` 已删除，其路由职责并入 application crate 与 `corvusd`。依赖只向内：domain 不依赖任何适配器。
- **REST API。** 守护进程提供 `/api/v1/health`、`/api/v1/browsers`、`/api/v1/browsers/{id}`、`/api/v1/sessions`、`/api/v1/sessions/{id}`、`/api/v1/sessions/{id}/steps`、`/api/v1/tools` 与 `/api/v1/tools/call`，以及认证路由。OpenAPI 位于 `/api-docs/openapi.json`，Swagger UI 位于 `/docs`。REST 与 MCP 共用同一套 43 工具 `ToolCatalog` 与 `ToolBackend`。每个请求带 `x-request-id`、链路追踪、120 秒超时并返回 408、gzip 压缩，CORS 仅允许回环来源；错误为类型化结构。
- **认证。** 回环地址上认证默认关闭。绑定非回环地址且未启用认证时启动失败，除非运维显式传入 `--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH` 豁免；豁免会记录一条不安全配置警告，且不存在默认账号。`--auth local` 启用 Argon2id 口令校验与 HS256 JWT：访问令牌有效期 900 秒，刷新令牌 30 天并在使用时轮换。`/api/v1/auth/login`、`/refresh`、`/logout` 与 `/me` 实现该流程；`/api/v1/health`、login 与 refresh 不需要令牌。REST 认证中间件只包裹 `/api/v1`，不覆盖既有 `/mcp` 端点、扩展 WebSocket、旧版 `/health`、`/metrics`、`/api-docs/openapi.json` 与 `/docs`；这些端点保持 1.3.0 的无认证行为。1.4.0 的账号与刷新令牌保存在内存中，重启后不保留；持久化认证存储属于后续工作。
- **持久化。** `corvus-record` 使用 SeaORM 与 `sea-orm-migration`，`schema_version` 为 1。1.3.0 数据库无需数据迁移即可打开；在既有文件上执行基线迁移前，守护进程先写 `VACUUM INTO` 备份，备份失败则中止启动。存储保持 WAL、单写入连接、5 秒 busy timeout、30 天会话保留与 262144 字节载荷截断。
- **扩展。** 扩展由 TypeScript 与 Vue 3 源码经三次 Vite 构建，产出可加载的 `extension/build/extension` 目录。弹窗与设置页使用 Vue 3、Pinia 与 Naive UI；Service Worker、内容脚本与 offscreen 文档保持无框架 TypeScript，并使用类型化 RPC。MV3 manifest 声明十项权限与 ESM Service Worker，构建会检查预期文件与 manifest 结构。
- **NFR-3（未完成）与 NFR-6（未变化）。** 1.4.0 不能宣称通过 72 小时压测；此前在约八小时停止后，计划中的运行仍未完成。Firefox 仍不受支持；受支持浏览器为 Edge 120+、Chrome 120+ 与最新 Brave。

## 5. 非功能需求

| 编号 | 类别 | 要求 |
|---|---|---|
| NFR-1 | 延迟 | 本机 click / type p95 < 50ms；snapshot p95 < 300ms（1 万节点页面） |
| NFR-2 | 并发 | 10 个标签并行命令，10 个 MCP 客户端并发连接 |
| NFR-3 | 稳定性 | 72h 压测，命令成功率 >= 99.9%，内存增长 < 50MB |
| NFR-4 | 体积 | 单二进制 < 25MB；扩展包 < 1MB |
| NFR-5 | 平台 | Windows 10+、macOS 13+、Linux x64/arm64 |
| NFR-6 | 浏览器 | Edge 120+、Chrome 120+、Brave 最新、Firefox 121+ |
| NFR-7 | 可维护 | Rust workspace 分 crate，clippy 零警告，CI 三平台 |
| NFR-8 | 可观测 | 结构化 JSON 日志、可调级别、默认不打印用户页面正文 |
| NFR-9 | 安全姿态 | 无审批、无白名单、无沙箱；以操作系统用户身份运行；MCP、扩展 WebSocket 与 CDP 只监听回环地址；REST 仅在启用本地认证或显式传入不安全豁免时才可绑定非回环地址 |
| NFR-10 | 兼容 | MCP 2025-06-18 规范，工具注解完整（readOnly / destructive / idempotent） |

## 6. 验收任务（真实场景，全部通过才算 M2 完成）

1. 在 GitHub 网页新建一个仓库并添加 README。
2. 在必应搜索「浏览器自动化 MCP」并总结前 5 条结果。
3. 登录企业系统，填一张 20 字段的表单并提交。
4. 从邮箱页面下载附件并报告落盘路径。
5. 同时打开 3 个浏览器，各自执行一次导航并截图。
6. 用 browser_eval 把一个表格抽成 JSON。
7. 拦截一个接口并 mock 返回，验证页面渲染变化。
8. 录制一次 50 步的任务，导出报告并在另一台机器回放。
9. 拔网线 10 秒后恢复，进行中的命令最终成功。
10. 在同一台机器上用 Claude Code 与 DSH 同时连接，交替下达命令互不干扰。

## 7. 未决问题

1. Firefox 是否进 M1（WebExtension API 差异较大，建议 M2）。
2. 站点记忆是否需要云端同步（建议先本地，保留导出格式）。
3. 录制是否默认开启（建议默认开启，30 天保留，可关闭）。