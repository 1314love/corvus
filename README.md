# Corvus

**The browser hub built for AI agents.** 让浏览器成为 AI 的器官，而不是它的沙箱。

[![CI](https://github.com/1314love/corvus/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/1314love/corvus/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/1314love/corvus)](https://github.com/1314love/corvus/releases)
[![License](https://img.shields.io/badge/license-PolyForm%20Noncommercial%201.0.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](rust-toolchain.toml)

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
| Minimal surface | a read-only status popup showing connection state, browser id, version and capabilities, plus an optional options page that stores the daemon URL and an optional CDP endpoint |
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
                                                       Edge / Chrome / Brave (one extension per connected browser)

- **corvusd** - the daemon: MCP server, transport hub, command router, session recorder and metrics.
- **corvus** - the CLI: doctor, serve, config and version.
- **extension** - MV3 with a read-only status popup and an optional options page for a custom `ws://` or `wss://` daemon URL and an optional loopback CDP endpoint. The popup shows connection, browser id, version and capabilities; the debugger channel is advertised up front and attached automatically when a command needs it, while the `cdp` capability appears only after endpoint detection succeeds.

### Feature surface (target)

- **Perception**: hybrid DOM + accessibility snapshot, screenshot (viewport / element / full page), text, HTML, console, network waterfall.
- **Action**: navigate, click, type, press, hover, drag, select, scroll, upload, download, multi-tab concurrency.
- **Power**: evaluate JS in page or isolated world, cookie and storage access, request interception, device and network emulation, PDF export.
- **Continuity**: idempotent commands, replayable sessions, resumable connection across daemon restarts, multi-browser fan-out.
- **Hybrid transport**: the extension by default, plus optional direct browser-level CDP over loopback with target sessions, a bounded event ring and explicit `auto`, `cdp` and `extension` channel selection.
- **Memory**: per-domain site cards learned from successful runs, stored in SQLite, exportable and shareable.

### Status

1.2.0 ships the full 42-tool surface over stdio and streamable HTTP: the original 32 tools keep their arguments and result shapes, and ten CDP and helper tools are added. Tool discovery has two levels: a brief default list and full detail, and `tool_help` returns one tool's full descriptor. The daemon is a hybrid transport: the extension remains the zero-setup default, while a browser explicitly launched with remote debugging can be driven directly over browser-level CDP on loopback. The daemon records every call, per-tab lanes let independent tabs run concurrently, and any in-flight MCP request can be cancelled. Windows has a service installer; Linux x86_64 has a tarball with systemd; an amd64 Docker image and Kubernetes manifests are release-gate checks covered by CI. All backend files live under one root selected by `--home` or `CORVUS_HOME`: binaries, `data/corvus.db`, logs, config and the extension. The clean 72-hour NFR-3 soak was stopped at the client's request after about eight hours, so NFR-3 is not passed and 1.2.0 is not 72-hour soak certified. CDP endpoints are loopback-only and unauthenticated; the shipped container image has no browser and cannot reach host loopback. See [docs/07-cdp.md](docs/07-cdp.md), [docs/03-roadmap.md](docs/03-roadmap.md), [docs/02-architecture.md](docs/02-architecture.md) and [docs/05-deployment.md](docs/05-deployment.md).

Network capture is opt-in per tab and keeps a 2000-entry ring. Interception supports block, mock and continue rules. Downloads can be listed and awaited, and uploads set file inputs through CDP, including hidden ones.

Every MCP session records each tools/call to SQLite, with arguments, result, error code and timing. session_list and session_steps read the timeline, session_replay plans or re-executes a range, and session_export writes JSONL. metrics and logs_tail expose counters and recent logs, and GET /metrics serves Prometheus text next to /health.

Independent tabs run concurrently: calls are ordered per browser tab and run in parallel across tabs, and any in-flight MCP request can be cancelled with notifications/cancelled.

### Development

`scripts/dev-setup.sh` checks the Rust toolchain and offline dependencies; `scripts/build.sh` builds release
binaries into `dist/` with checksums. Process, review gates and release cadence: [docs/PROCESS.md](docs/PROCESS.md).

### Install

Windows: `scripts/install-windows.ps1` registers the daemon as a service under `%ProgramData%\Corvus`, and
`scripts/uninstall-windows.ps1` removes it. Build the bundle with `scripts/package.sh`.

Linux x86_64: run `scripts/package-linux.sh`, verify `SHA256SUMS`, then run `sudo ./install-linux.sh`. The
package installs under `/opt/corvus` with a systemd unit.

Docker and Kubernetes: the amd64 image and manifests are described in
[docs/05-deployment.md](docs/05-deployment.md). The examples keep ports 7330 and 7331 on loopback. The daemon
has no authentication, so never expose them publicly.

Extension: load the unpacked `extension/` directory; the optional options page stores a custom `ws://` or
`wss://` daemon URL and an optional loopback CDP endpoint. A remote `wss://` endpoint still needs a trusted tunnel. Full install, extension loading,
upgrade and the five-minute walkthrough are in [docs/04-install.md](docs/04-install.md).

Supported platforms: Windows 10+ and Linux x86_64 packages; macOS runs from source with no packaged
installer. Supported browsers: Edge, Chrome and Brave (Chromium 120+). Firefox is not supported (recorded
NFR-6 deviation).

### Docs

- [Charter](docs/00-charter.md) - why this exists, scope, milestones, decisions
- [Requirements](docs/01-requirements.md) - functional and non-functional requirements
- [Architecture](docs/02-architecture.md) - components, protocols, data model
- [Roadmap](docs/03-roadmap.md) - milestones and acceptance tasks
- [Deployment](docs/05-deployment.md) - Windows service, Linux systemd, Docker and Kubernetes
- [CDP guide](docs/07-cdp.md) - optional browser-level channel, tools and troubleshooting
- [Release notes 1.2.0](docs/release-notes/v1.2.0.md) - what ships, evidence and limitations
- [Release notes 1.1.0](docs/release-notes/v1.1.0.md) - what ships, evidence and limitations
- [Language policy](docs/LANGUAGE.md) - how the bilingual editions are maintained

### Community

- [Contributing](CONTRIBUTING.md) - branch, commit and review rules
- [Security policy](SECURITY.md) - supported versions, private reporting, scope and safe harbour
- [Support](SUPPORT.md) - where to look first, what is supported, how to reach the maintainer
- [Code of conduct](CODE_OF_CONDUCT.md) - how we work together and how to report a problem
- [Documentation index](docs/README.md) - every document, both editions, and its status

Issues use the templates in `.github/ISSUE_TEMPLATE/`; a suspected vulnerability goes through the private
advisory flow and never through an issue. Release notes are generated from `.github/release.yml` categories,
dependency updates arrive as Dependabot pull requests, and `.github/CODEOWNERS` routes review.

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
| 最小界面 | 一个只读状态弹窗展示连接状态、浏览器 ID、版本与能力，另有一个可选设置页保存守护进程地址与可选 CDP 端点 |
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
                                                         Edge / Chrome / Brave (one extension per connected browser)

- **corvusd**：守护进程。MCP 服务端、传输中枢、命令路由、会话录制与指标。
- **corvus**：命令行。doctor、serve、config、version。
- **扩展**：MV3，包含只读状态弹窗与可选设置页，后者用于保存自定义 `ws://` 或 `wss://` 守护进程地址及可选回环 CDP 端点。弹窗展示连接状态、浏览器 ID、版本与能力；debugger 通道始终声明，命令需要时自动挂载，`cdp` 能力只有端点探测成功后才出现。

### 能力面（目标）

- **感知**：DOM + 可访问性混合快照、截图（视口 / 元素 / 整页）、文本、HTML、控制台、网络瀑布。
- **动作**：导航、点击、输入、按键、悬停、拖拽、选择、滚动、上传、下载、多标签并发。
- **强力**：页面或隔离世界执行 JS、Cookie 与存储读写、请求拦截、设备与网络模拟、PDF 导出。
- **连续性**：幂等命令、可回放会话、守护进程重启后连接自愈、多浏览器分发。
- **混合传输**：默认走扩展，另可选经回环直连浏览器级 CDP，支持 Target 会话、有界事件环与显式 `auto`、`cdp`、`extension` 通道选择。
- **记忆**：按域名从成功运行中学习站点卡片，存于 SQLite，可导出、可分享。

### 状态

1.2.0 已交付完整的 42 工具面，覆盖 stdio 与 streamable HTTP：原有 32 个工具的参数与结果结构保持不变，新增十个 CDP 与辅助工具。工具发现分为简版默认列表与完整详情两级，`tool_help` 可返回单个工具的完整描述。守护进程采用混合传输：扩展仍是零配置默认路径，显式以远程调试启动的浏览器则可经回环浏览器级 CDP 直连驱动。守护进程记录每次调用，按标签通道让独立标签并行执行，任何进行中的 MCP 请求都可取消。Windows 提供服务安装器；Linux x86_64 提供带 systemd 的 tar 包；amd64 Docker 镜像与 Kubernetes 清单属于由 CI 覆盖的发布门禁检查。所有后端文件都位于 `--home` 或 `CORVUS_HOME` 指定的单一根目录下：二进制、`data/corvus.db`、日志、配置与扩展。干净的 72 小时 NFR-3 压测在客户要求下于约八小时后停止，因此 1.2.0 未通过 NFR-3，也不能宣称通过 72 小时压测。CDP 端点仅限回环且没有认证；发布容器镜像不含浏览器，也无法访问宿主机回环。见 [docs/07-cdp.zh.md](docs/07-cdp.zh.md)、[docs/03-roadmap.zh.md](docs/03-roadmap.zh.md)、[docs/02-architecture.zh.md](docs/02-architecture.zh.md) 与 [docs/05-deployment.zh.md](docs/05-deployment.zh.md)。

网络捕获按标签显式开启，每个标签保留 2000 条环形缓冲。拦截支持 block、mock 与 continue 规则；下载可列出并等待完成，上传通过 CDP 设置文件 input，隐藏 input 也可用。

每个 MCP 会话都会把每次 tools/call 连同参数、结果、错误码与耗时记录到 SQLite。session_list 与 session_steps 读取该时间线，session_replay 生成计划或重新执行一段步骤，session_export 导出 JSONL。metrics 与 logs_tail 提供计数器与近期日志，GET /metrics 在 /health 旁输出 Prometheus 文本。

独立标签并发执行：调用在同一浏览器标签内有序、在不同标签间并行，任何进行中的 MCP 请求都可以通过 notifications/cancelled 取消。

### 开发

`scripts/dev-setup.sh` 检查 Rust 工具链与离线依赖；`scripts/build.sh` 构建发布二进制到 `dist/` 并生成校验和。开发流程、评审门禁与发布节奏见 [docs/PROCESS.md](docs/PROCESS.zh.md)。

### 安装

Windows：`scripts/install-windows.ps1` 把守护进程安装为 `%ProgramData%\Corvus` 下的服务，
`scripts/uninstall-windows.ps1` 负责卸载。用 `scripts/package.sh` 构建发布包。

Linux x86_64：运行 `scripts/package-linux.sh`，校验 `SHA256SUMS` 后执行 `sudo ./install-linux.sh`。
安装包默认放在 `/opt/corvus` 并注册 systemd unit。

Docker 与 Kubernetes：amd64 镜像与清单见 [docs/05-deployment.zh.md](docs/05-deployment.zh.md)。
示例中的 7330 与 7331 只绑定回环地址。守护进程没有认证层，绝不能把这些端口暴露到公网。

扩展：加载未打包的 `extension/` 目录；可选设置页可保存自定义 `ws://` 或 `wss://` 守护进程地址及可选回环 CDP 端点。
远程 `wss://` 端点仍需要可信隧道。完整安装、扩展加载、升级与五分钟流程见
[docs/04-install.zh.md](docs/04-install.zh.md)。

支持平台：Windows 10+ 与 Linux x86_64 安装包；macOS 仅支持源码运行，没有打包安装器。
支持浏览器：Edge、Chrome 与 Brave（Chromium 120+）。不支持 Firefox（已记录的 NFR-6 偏差）。

### 文档

- [立项书](docs/00-charter.zh.md) —— 为什么做、范围、里程碑、决策记录
- [需求规格](docs/01-requirements.zh.md) —— 功能与非功能需求
- [架构](docs/02-architecture.zh.md) —— 组件、协议、数据模型
- [路线图](docs/03-roadmap.zh.md) —— 里程碑与验收任务
- [部署](docs/05-deployment.zh.md) —— Windows 服务、Linux systemd、Docker 与 Kubernetes
- [CDP 指南](docs/07-cdp.zh.md) —— 可选浏览器级通道、工具与故障排查
- [1.2.0 发布说明](docs/release-notes/v1.2.0.zh.md) —— 交付内容、证据与限制
- [1.1.0 发布说明](docs/release-notes/v1.1.0.zh.md) —— 交付内容、证据与限制
- [语言政策](docs/LANGUAGE.md) —— 双语版本如何维护

### 社区

- [参与贡献](CONTRIBUTING.md) —— 分支、提交与评审规则
- [安全策略](SECURITY.md) —— 支持版本、私密报告、范围与安全港
- [支持](SUPPORT.md) —— 先看哪里、支持范围、如何联系维护者
- [行为准则](CODE_OF_CONDUCT.md) —— 协作方式与问题举报
- [文档索引](docs/README.md) —— 全部文档的中英两版与状态

问题通过 `.github/ISSUE_TEMPLATE/` 中的表单提交；怀疑存在漏洞时走私密公告流程，不要开 issue。发布说明按
`.github/release.yml` 的分类生成，依赖更新由 Dependabot 以 pull request 形式提交，评审路由见 `.github/CODEOWNERS`。

### 许可

**PolyForm Noncommercial License 1.0.0** —— 源码公开：可阅读、可运行、可修改、可在非商业用途下再分发。
未经单独授权不得商用；商用请联系 `cosher@163.com`。这刻意不是 OSI 认可的开源许可：源码公开，但商用受限。
以 [LICENSE](LICENSE) 条文为准。
