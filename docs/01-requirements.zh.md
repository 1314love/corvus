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
| extension | 浏览器侧 MV3 扩展，无页面，仅一个开关弹窗 |
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
| FR-D1 | debugger 通道：可读取 CDP 域能力（DOM、Network、Runtime、Emulation） | P1 | 弹窗开关打开后可用，关闭后退化为纯 DOM 通道 |
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
| NFR-9 | 安全姿态 | 无审批、无白名单、无沙箱；以操作系统用户身份运行，端点只监听回环地址 |
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