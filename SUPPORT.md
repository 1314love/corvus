# Support / 支持

[English](#english) | [中文](#中文)

---

## English

### Read first

| Question | Document |
|---|---|
| Install, upgrade, uninstall | [docs/04-install.md](docs/04-install.md) |
| Windows service, systemd, Docker, Kubernetes | [docs/05-deployment.md](docs/05-deployment.md) |
| CDP channel, endpoints, reconnect | [docs/07-cdp.md](docs/07-cdp.md) |
| Components, protocols, data model | [docs/02-architecture.md](docs/02-architecture.md) |
| Firefox | [docs/06-firefox-assessment.md](docs/06-firefox-assessment.md) |
| What changed in a release | [docs/release-notes](docs/release-notes), [CHANGELOG.md](CHANGELOG.md) |
| CLI flags | `corvus --help`, `corvusd --help` |

### Open an issue

| Request | Where |
|---|---|
| Bug | Bug report form |
| Wrong or missing documentation | Documentation form |
| Feature | Feature request form |
| Vulnerability | [SECURITY.md](SECURITY.md), never a public issue |

Blank issues are disabled; pick a form.

### Before posting

1. `corvus doctor` - it prints the checks that matter.
2. `corvusd --version`.
3. Channel and transport: `extension`, `cdp` or `auto`; stdio or streamable HTTP.
4. Logs: `logs_tail` over MCP, or the files under `<home>/logs`. Remove tokens, cookies and page text.

### Supported

- Windows 10+ and Linux x86_64 packages; macOS from source, no installer.
- Edge, Chrome, Brave 120+.
- Channels: `extension` (default), `cdp`, `auto`.
- Transports: stdio, streamable HTTP.

### Not supported

- Exposing `7330`/`7331` or the CDP ports beyond loopback.
- Firefox.
- A macOS installer.
- A browser started by the daemon.
- A 72-hour soak claim: NFR-3 did not pass, about 8 of 72 hours ran.

### Response

Best effort, no SLA. Commercial licensing: `cosher@163.com`.

---

## 中文

### 先看文档

| 问题 | 文档 |
|---|---|
| 安装、升级、卸载 | [docs/04-install.zh.md](docs/04-install.zh.md) |
| Windows 服务、systemd、Docker、Kubernetes | [docs/05-deployment.zh.md](docs/05-deployment.zh.md) |
| CDP 通道、端点、重连 | [docs/07-cdp.zh.md](docs/07-cdp.zh.md) |
| 组件、协议、数据模型 | [docs/02-architecture.zh.md](docs/02-architecture.zh.md) |
| Firefox | [docs/06-firefox-assessment.zh.md](docs/06-firefox-assessment.zh.md) |
| 每个版本改了什么 | [docs/release-notes](docs/release-notes)、[CHANGELOG.md](CHANGELOG.md) |
| 命令行参数 | `corvus --help`、`corvusd --help` |

### 提交 issue

| 诉求 | 去处 |
|---|---|
| 缺陷 | 缺陷表单 |
| 文档错误或缺失 | 文档表单 |
| 功能建议 | 功能表单 |
| 漏洞 | [SECURITY.md](SECURITY.md)，不要开公开 issue |

空白 issue 已关闭，请选表单。

### 提交前

1. `corvus doctor`，它会打印关键检查项。
2. `corvusd --version`。
3. 通道与传输：`extension`、`cdp` 或 `auto`；stdio 或 streamable HTTP。
4. 日志：MCP 的 `logs_tail`，或 `<home>/logs` 下的文件。去掉令牌、Cookie 与页面原文。

### 支持范围

- Windows 10+ 与 Linux x86_64 安装包；macOS 仅源码运行，无安装器。
- Edge、Chrome、Brave 120+。
- 通道：`extension`（默认）、`cdp`、`auto`。
- 传输：stdio、streamable HTTP。

### 不支持

- 把 `7330`/`7331` 或 CDP 端口暴露到回环之外。
- Firefox。
- macOS 安装器。
- 由守护进程启动浏览器。
- 72 小时压测结论：NFR-3 未通过，72 小时里只跑了约 8 小时。

### 响应

尽力而为，无 SLA。商业授权：`cosher@163.com`。
