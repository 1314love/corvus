# Corvus

**The browser hub built for AI agents.** 让浏览器成为 AI 的器官，而不是它的沙箱。

[English](#english) | [中文](#中文)

---

## English

Corvus is a Rust-powered local daemon plus a browser extension that gives any MCP-capable agent full,
persistent, zero-friction control over the browser you are actually using. No approval dialogs. No pairing
ritual. No separate browser. No disconnect.

### Why

Browser automation usually begins from the assumption that the agent is an intruder: every action needs a
permission, every permission needs a human, and every human decision is one more chance to be interrupted.
That trade-off makes sense when a stranger drives your machine. It is the wrong trade-off for an agent that
already works for you.

Corvus starts from the opposite premise: **the browser is an organ of the agent, not a sandbox it visits.**

| Principle | What it means in the product |
|---|---|
| No friction | zero approval dialogs, zero pairing steps, zero setup wizards |
| No surface | no side panel, no settings page; only a read-only status popup showing connection state, browser id, version and capabilities |
| No dropout | connection is a first-class subsystem: multi-transport, watchdog, resume across restarts |
| No ceiling | every operation the browser can perform becomes a tool, not a curated subset |
| No blind spots | every command, snapshot and request is recorded and replayable |

### Shape

    +-----------------+        MCP (stdio | streamable-http)        +------------------+
    |  agent client   | <-----------------------------------------> |  corvusd (Rust)  |
    +-----------------+                                             +---------+--------+
                                                                             |
                                                     WS  /  Native Messaging / CDP
                                                                             |
                                               +-----------------------------+----+
                                               |  Corvus extension (MV3, popup)   |
                                               +-----------------------------+----+
                                                                             |
                                                       Edge / Chrome (one extension per connected browser)

- **corvusd** - the daemon: MCP server, transport hub, command router, session recorder, site memory.
- **corvus** - the CLI: doctor, serve, attach, session replay, bench.
- **extension** - MV3, no UI pages beyond the status popup. The popup is a read-only status card (connection, browser id, version, capabilities); the debugger channel is advertised up front and attached automatically when a command needs it.

### Feature surface (target)

- **Perception**: hybrid DOM + accessibility snapshot, screenshot (viewport / element / full page), text, HTML, console, network waterfall.
- **Action**: navigate, click, type, press, hover, drag, select, scroll, upload, download, multi-tab concurrency.
- **Power**: evaluate JS in page or isolated world, cookie and storage access, request interception, device and network emulation, PDF export.
- **Continuity**: idempotent commands, replayable sessions, resumable connection across daemon restarts, multi-browser fan-out.
- **Memory**: per-domain site cards learned from successful runs, stored in SQLite, exportable and shareable.

### Status

M1 is implemented end to end: the daemon, the extension and the MCP server expose the four core tools on three-platform CI. M2 ships 26 tools in two waves: wave 1 added text and HTML extraction, find, hover, press, scroll, select, JavaScript evaluation, waits, history and tab management; wave 2 added browser listing and routing, network capture and interception, downloads and uploads. M3 wave 1 is in progress: recording, replay and observability bring the surface to 32 tools. See [docs/03-roadmap.md](docs/03-roadmap.md) and [docs/02-architecture.md](docs/02-architecture.md).

Network capture is opt-in per tab and keeps a 2000-entry ring. Interception supports block, mock and continue rules. Downloads can be listed and awaited, and uploads set file inputs through CDP, including hidden ones.

Every MCP session records each tools/call to SQLite, with arguments, result, error code and timing. session_list and session_steps read the timeline, session_replay plans or re-executes a range, and session_export writes JSONL. metrics and logs_tail expose counters and recent logs, and GET /metrics serves Prometheus text next to /health.

Independent tabs run concurrently: calls are ordered per browser tab and run in parallel across tabs, and any in-flight MCP request can be cancelled with notifications/cancelled.

### Development

`scripts/dev-setup.sh` checks the Rust toolchain and offline dependencies; `scripts/build.sh` builds release
binaries into `dist/` with checksums. Process, review gates and release cadence: [docs/PROCESS.md](docs/PROCESS.md).

### Install

`scripts/install-windows.ps1` registers the daemon as a Windows service, and `scripts/uninstall-windows.ps1`
removes it. Build the release bundle with `scripts/package.sh`; install, extension load and the five-minute
walkthrough are in [docs/04-install.md](docs/04-install.md).

Supported browsers: Edge and Chrome (Chromium 120+). Firefox is not supported.

### Docs

- [Charter](docs/00-charter.md) - why this exists, scope, milestones, decisions
- [Requirements](docs/01-requirements.md) - functional and non-functional requirements
- [Architecture](docs/02-architecture.md) - components, protocols, data model
- [Roadmap](docs/03-roadmap.md) - milestones and acceptance tasks
- [Language policy](docs/LANGUAGE.md) - how the bilingual editions are maintained

### License

**PolyForm Noncommercial License 1.0.0** - source available: read, run, modify and redistribute for any
noncommercial purpose. Commercial use is not allowed without a separate paid license; contact
`cosher@163.com` to arrange one. This is deliberately not an OSI-approved open source license: the source is
public, commercial use is not. See [LICENSE](LICENSE) for the binding text.

---

## 中文

Corvus 是一个 Rust 本地守护进程加一个浏览器扩展：它把「你正在使用的那个浏览器」完整、持续、零摩擦地交给任何支持
MCP 的 AI 代理。没有审批弹窗，没有配对仪式，不用另开浏览器，不会掉线。

### 为什么做

浏览器自动化长期被一个假设锁住：操作浏览器的是陌生人，所以每一次点击都要授权，每一个下载都要挂起，每一个敏感
字段都要人工放行。这套假设在「陌生脚本」场景里是必要的；但当操作者是已经在你机器上工作、持有你账号权限的 AI
代理时，它只剩下摩擦——人会疲劳，审批会被无脑点过，真正的风险信号被淹没在噪声里。

Corvus 换一个假设：**浏览器是 AI 的器官，不是它造访的沙箱。**

| 理念 | 在产品里的含义 |
|---|---|
| 零摩擦 | 没有审批弹窗、没有配对步骤、没有安装向导 |
| 零界面 | 没有侧边栏、没有设置页；只有一个只读状态弹窗，展示连接状态、浏览器 ID、版本与能力 |
| 永不掉线 | 连接是一等子系统：多传输、看门狗、重启后自动恢复 |
| 能力不设上限 | 浏览器能做的每一件事都成为工具，而不是精选子集 |
| 全可观测 | 每条命令、每次快照、每个请求都可记录、可回放 |

### 形态

    +-----------------+        MCP（stdio | streamable-http）        +------------------+
    |   AI 客户端      | <------------------------------------------> |  corvusd（Rust）  |
    +-----------------+                                              +---------+--------+
                                                                               |
                                                      WS / Native Messaging / CDP
                                                                               |
                                                 +-----------------------------+----+
                                                 |   Corvus 扩展（MV3，弹窗）      |
                                                 +-----------------------------+----+
                                                                               |
                                                         Edge / Chrome (one extension per connected browser)

- **corvusd**：守护进程。MCP 服务端、传输中枢、命令路由、会话录制、站点记忆。
- **corvus**：命令行。doctor、serve、attach、会话回放、bench。
- **扩展**：MV3，除状态弹窗外没有其他界面。弹窗只读展示连接状态、浏览器 ID、版本与能力；debugger 通道始终声明，命令需要时自动挂载。

### 能力面（目标）

- **感知**：DOM + 可访问性混合快照、截图（视口 / 元素 / 整页）、文本、HTML、控制台、网络瀑布。
- **动作**：导航、点击、输入、按键、悬停、拖拽、选择、滚动、上传、下载、多标签并发。
- **强力**：页面或隔离世界执行 JS、Cookie 与存储读写、请求拦截、设备与网络模拟、PDF 导出。
- **连续性**：幂等命令、可回放会话、守护进程重启后连接自愈、多浏览器分发。
- **记忆**：按域名从成功运行中学习站点卡片，存于 SQLite，可导出、可分享。

### 状态

M1 已端到端落地：守护进程、扩展与 MCP 服务端在三个平台的 CI 上提供四个核心工具。M2 分两波落地，工具面达到 26 个：第一波新增文本与 HTML 提取、查找、悬停、按键、滚动、下拉选择、JS 求值、等待、前进后退与标签管理；第二波新增浏览器列表与路由、网络捕获与拦截、下载与上传。M3 第一波正在进行：录制、回放与可观测性把工具面带到 32 个。见 [docs/03-roadmap.zh.md](docs/03-roadmap.zh.md) 与 [docs/02-architecture.zh.md](docs/02-architecture.zh.md)。

网络捕获按标签显式开启，每个标签保留 2000 条环形缓冲。拦截支持 block、mock 与 continue 规则；下载可列出并等待完成，上传通过 CDP 设置文件 input，隐藏 input 也可用。

每个 MCP 会话都会把每次 tools/call 连同参数、结果、错误码与耗时记录到 SQLite。session_list 与 session_steps 读取该时间线，session_replay 生成计划或重新执行一段步骤，session_export 导出 JSONL。metrics 与 logs_tail 提供计数器与近期日志，GET /metrics 在 /health 旁输出 Prometheus 文本。

独立标签并发执行：调用在同一浏览器标签内有序、在不同标签间并行，任何进行中的 MCP 请求都可以通过 notifications/cancelled 取消。

### 开发

`scripts/dev-setup.sh` 检查 Rust 工具链与离线依赖；`scripts/build.sh` 构建发布二进制到 `dist/` 并生成校验和。开发流程、评审门禁与发布节奏见 [docs/PROCESS.md](docs/PROCESS.zh.md)。

### 安装

`scripts/install-windows.ps1` 把守护进程注册为 Windows 服务，`scripts/uninstall-windows.ps1` 负责卸载。
用 `scripts/package.sh` 构建发布包；完整安装、扩展加载与五分钟流程见 [docs/04-install.zh.md](docs/04-install.zh.md)。

支持的浏览器：Edge 与 Chrome（Chromium 120+）。暂不支持 Firefox。

### 文档

- [立项书](docs/00-charter.zh.md) —— 为什么做、范围、里程碑、决策记录
- [需求规格](docs/01-requirements.zh.md) —— 功能与非功能需求
- [架构](docs/02-architecture.zh.md) —— 组件、协议、数据模型
- [路线图](docs/03-roadmap.zh.md) —— 里程碑与验收任务
- [语言政策](docs/LANGUAGE.md) —— 双语版本如何维护

### 许可

**PolyForm Noncommercial License 1.0.0** —— 源码公开：可阅读、可运行、可修改、可在非商业用途下再分发。
未经单独授权不得商用；商用请联系 `cosher@163.com`。这刻意不是 OSI 认可的开源许可：源码公开，但商用受限。
以 [LICENSE](LICENSE) 条文为准。
