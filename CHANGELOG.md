# Changelog

All notable changes to Corvus are recorded here. The format follows Keep a Changelog, and releases use semantic versioning.

[English](#english) | [中文](#中文)

## English

## 1.0.0 - 2026-11-14

First stable release. Scope: 32 MCP tools over stdio and streamable HTTP, per-tab scheduling, MCP request cancellation, session recording, replay and export, metrics and logs, the Windows installer, and the client configuration generator. Release validation covered Edge, Chrome for Testing 157.0.8103.78 and Brave Browser 157.1.99.57.

### Changed

- `scripts/package.sh` now also writes `dist/corvus-<version>-windows-x64.zip` alongside the `dist/` tree.
- `scripts/uninstall-windows.ps1 -PurgeData` now removes the install and data directories when they are empty.

### Known limitations

- The clean NFR-3 72-hour soak started 2026-11-14T14:41:37Z and completes 2026-11-17T14:41:37Z; the 72-hour claim is pending until it completes.
- Firefox 121+ is unsupported. The NFR-6 deviation remains open; option A (keep Firefox out of the supported matrix) is recommended. See [docs/06-firefox-assessment.md](docs/06-firefox-assessment.md).
- The installer is Windows-only.
- In headless Chrome and Brave, a synthetic click on an `<a download>` link does not start a download; navigate to the attachment URL instead, after which `download_wait` and `download_list` report `complete`.

## 0.1.0 - 2026-11-14

First public release.

### Added

- MCP server exposing 32 tools over stdio and streamable HTTP, covering navigation, snapshots, text and HTML extraction, find, click, type, press, hover, select, scroll, history, JavaScript evaluation, tabs, network capture and interception, downloads, uploads, sessions, metrics and logs.
- Manifest V3 browser extension that connects the browser the user already runs; release validation used Edge, Chrome for Testing 157 and Brave 157.
- Per-tab scheduling: calls on the same `browser_id` and `tab_id` are ordered, calls on different tabs run in parallel, and calls without a `tab_id` share the active-tab lane.
- MCP request cancellation through `notifications/cancelled` with `params.requestId`; the cancelled call returns JSON-RPC error `-32800`, and the session records `ok=false` with error code `CANCELLED`.
- Session recording, inspection, replay and JSONL export: `session_list`, `session_steps`, `session_replay` and `session_export`.
- `GET /health`, Prometheus text at `GET /metrics`, and the `metrics` and `logs_tail` tools.
- Windows service install and uninstall scripts with an idempotent upgrade path.
- Client configuration generator supporting `--transport stdio|http` and `--client claude|cursor|dsh|cline|generic`.
- Reference release evidence: click p95 2 ms and snapshot p95 1 ms against the mock, and snapshot 22 ms against a live 10,000-element page; daemon RSS growth 2.07 MB over a 900 s mixed-load run; release daemon 6.33 MB; extension bundle 0.23 MB; conformance suite 135 checks; 101 workspace tests.

### Changed

- The 32-tool surface is frozen for 0.1.0; per-tab scheduling and cancellation do not change tool arguments or result shapes.
- The release bundle now contains `corvusd.exe`, `corvus.exe`, the extension tree, `VERSION` and `SHA256SUMS`.

### Fixed

- The Windows installer treats an nssm start-pending response as a warning and polls the service until it reaches `Running`.
- Release packaging includes every extension file in `SHA256SUMS`, so each shipped file can be verified.

### Security

- The daemon binds only to `127.0.0.1`. It has no authentication layer, so the ports must not be exposed beyond loopback.
- `SHA256SUMS` covers binaries, `VERSION` and every extension file.
- The session database stores command arguments and results; treat it as sensitive data.

---

## 中文

## 1.0.0 - 2026-11-14

首个稳定版本。范围包括：通过 stdio 与 streamable HTTP 暴露的 32 个 MCP 工具、按标签调度、MCP 请求取消、会话录制、回放与导出、指标与日志、Windows 安装器，以及客户端配置生成器。发布验证覆盖 Edge、Chrome for Testing 157.0.8103.78 与 Brave Browser 157.1.99.57。

### 变更

- `scripts/package.sh` 现在还会生成 `dist/corvus-<version>-windows-x64.zip` 压缩包。
- `scripts/uninstall-windows.ps1 -PurgeData` 现在会删除已清空的安装目录与数据目录。

### 已知限制

- 干净的 NFR-3 72 小时浸泡测试于 2026-11-14T14:41:37Z 启动，将于 2026-11-17T14:41:37Z 完成；在完成前，72 小时结论仍为待定。
- 暂不支持 Firefox 121+。NFR-6 偏差仍未关闭；建议采用方案 A（将 Firefox 保持在支持矩阵之外）。参见 [docs/06-firefox-assessment.zh.md](docs/06-firefox-assessment.zh.md)。
- 安装器仅支持 Windows。
- 在 headless Chrome 与 Brave 中，对 `<a download>` 链接的合成点击不会启动下载；改为导航到附件 URL，之后 `download_wait` 与 `download_list` 会报告 `complete`。

## 0.1.0 - 2026-11-14

首个公开发布。

### 新增

- MCP 服务端通过 stdio 与 streamable HTTP 暴露 32 个工具，覆盖导航、快照、文本与 HTML 提取、查找、点击、输入、按键、悬停、下拉选择、滚动、前进后退、JavaScript 求值、标签管理、网络捕获与拦截、下载、上传、会话、指标与日志。
- Manifest V3 浏览器扩展，连接用户已有的浏览器；发布验证使用 Edge、Chrome for Testing 157 与 Brave 157。
- 按标签调度：同一 `browser_id` 与 `tab_id` 上的调用按顺序执行，不同标签的调用并行执行，未提供 `tab_id` 的调用共享活动标签通道。
- 通过 `notifications/cancelled` 与 `params.requestId` 取消 MCP 请求；被取消的调用返回 JSON-RPC 错误 `-32800`，会话以 `ok=false` 和错误码 `CANCELLED` 记录。
- 会话录制、查看、回放与 JSONL 导出：`session_list`、`session_steps`、`session_replay` 与 `session_export`。
- `GET /health`、`GET /metrics` 的 Prometheus 文本，以及 `metrics` 与 `logs_tail` 工具。
- Windows 服务安装与卸载脚本，升级路径幂等。
- 客户端配置生成器，支持 `--transport stdio|http` 与 `--client claude|cursor|dsh|cline|generic`。
- 参考发布证据：mock 下点击与快照 p95 分别为 2 ms 与 1 ms，10,000 元素实时页面快照 22 ms；900 秒混合负载守护进程 RSS 增长 2.07 MB；发布守护进程 6.33 MB；扩展包 0.23 MB；一致性套件 135 项检查；101 个工作区测试。

### 变更

- 0.1.0 冻结 32 个工具面；按标签调度与取消不改变工具参数或结果形状。
- 发布包现在包含 `corvusd.exe`、`corvus.exe`、扩展目录、`VERSION` 与 `SHA256SUMS`。

### 修复

- Windows 安装器把 nssm 的 start-pending 响应当作警告，并轮询服务直到状态为 `Running`。
- 发布打包把每个扩展文件纳入 `SHA256SUMS`，因此每个交付文件都可校验。

### 安全

- 守护进程只绑定 `127.0.0.1`，没有认证层，端口不得暴露到回环地址之外。
- `SHA256SUMS` 覆盖二进制、`VERSION` 与每个扩展文件。
- 会话数据库保存命令参数与结果，应视为敏感数据。
