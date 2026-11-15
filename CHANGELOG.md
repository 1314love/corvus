# Changelog

All notable changes to Corvus are recorded here. The format follows Keep a Changelog, and releases use semantic versioning.

[English](#english) | [中文](#中文)

## English

## 1.5.0 - Unreleased

Unreleased work in the tree. The MCP surface is unchanged at 43 tools in nine categories. Version metadata now reports 1.5.0; QA acceptance and Windows installer acceptance PASS are recorded on a dirty tree, but no 1.5.0 tag or CI run exists and the local installer ZIP is not a published asset.

### Added

- `page_snapshot` perception v2: open shadow roots and same-origin child frames, document-order frame paths (`f0`, `f0.0`, `f1`), a `frames` summary with `cross_origin`, `unavailable` and `geometry_unavailable` skip reasons, one global `max_nodes` budget, and boxes projected into the top viewport. Cross-origin frame DOM is never read.
- Compact snapshot diffs: `diff` defaults to false and is compact-only; passing it with JSON is typed `BAD_ARGS`. A 64-entry application-layer LRU keyed by browser, session and tab adds `baseline`, `unchanged`, `changed`, `added` and `removed`, using two-space, `+ `, `~ ` and appended `- ` markers. A non-diff compact request refreshes the baseline; diff and JSON requests do not.
- Extension internationalization: vue-i18n with English and zh-CN catalogs, locale detection from `chrome.i18n.getUILanguage()`/`navigator.language`, persistence under `corvus.locale`, reactive switching without a reload, `document.documentElement.lang`, and MV3 `_locales`.

### Changed

- The extension build now contains 14 files including the two locale catalogs; the extension gate measures 41 Vitest tests in eight files and isolated Playwright 1/1.
- Windows installer acceptance passed 98 checks and observations with 0 failures on the local ZIP; that local artifact is not the CI rebuild.
- `browser_batch` shares the application-layer snapshot path, so a `page_snapshot` step receives the same diff treatment exactly once.
- The 43-tool MCP catalogue, nine categories, compact envelope and `browser_batch` surface are unchanged.
- The rule against reasoning text is now enforced in CI by `scripts/check-cot.mjs`, which scans tracked files and pull request commit messages and fails on a hit.

## 1.4.0 - 2026-11-15

Structural release. The MCP surface stays at 43 tools with the same compact envelope and `browser_batch`; the backend moves to ports and adapters, a REST `/api/v1` surface is added, recording moves to SeaORM migrations, and the extension is rebuilt from TypeScript and Vue 3 sources.

### Added

- `corvus-domain` for the pure domain model and `corvus-application` for use cases plus the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports; `corvusd` is the composition root.
- `corvus-api`: Axum REST on `/api/v1` for health, browsers, sessions, tools and auth, plus OpenAPI at `/api-docs/openapi.json` and Swagger UI at `/docs`. REST shares the 43-tool catalogue and executor with MCP.
- Optional local authentication: Argon2id passwords, HS256 JWTs, 900-second access tokens and rotating 30-day refresh tokens, with no default account. `--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH` is an explicit unsafe opt-out for a non-loopback bind without auth.
- SeaORM and `sea-orm-migration` recording with `schema_version` 1, a baseline migration, a pre-migration `VACUUM INTO` backup for existing databases, one writer connection, WAL, a 5-second busy timeout, 30-day retention and 262144-byte payload truncation.
- Extension rewrite: Vue 3, Pinia and Naive UI for the popup and options page; framework-free TypeScript for the service worker, content script and offscreen document; typed RPC; three Vite builds to `extension/build/extension`; ten MV3 permissions; 21 unit, contract and component tests plus an isolated Playwright suite.

### Changed

- `corvus-runtime` is removed. Routing now lives in `corvus-application` and `corvusd`; dependencies point inward to the domain.
- The extension is built with `pnpm install && pnpm build` and loaded from `extension/build/extension`, not from the source tree.
- Metadata and release assertions state 1.4.0. The MCP catalogue remains 43 tools in nine categories with the 1.3.0 schemas, compact single-text result and `browser_batch`.

### Fixed

- The recording store now has an explicit migration baseline instead of relying on ad-hoc table creation, and an existing 1.3.0 database opens without a data migration.
- Stale 1.3.0 and unpacked-source extension instructions were replaced across the README, install and architecture documents.

### Limitations

- REST auth accounts and refresh tokens are held in memory in 1.4.0 and do not survive a restart; persistent auth storage is future work.
- The bearer guard covers `/api/v1` only, with `/api/v1/health`, login and refresh exempt. `/mcp`, the extension WebSocket, legacy `/health`, `/metrics`, `/api-docs/openapi.json` and `/docs` remain unauthenticated even when local auth is on. Enabling REST auth does not make a non-loopback daemon remote-safe.
- The checked-in Kubernetes Deployment overrides the image entrypoint and does not yet pass `--allow-no-auth` or local auth, so it fails the 1.4.0 startup guard as checked in. The image entrypoint does pass the opt-out, but the resulting container is explicitly unauthenticated and is not turnkey secure.
- NFR-3 is not passed: the planned 72-hour soak remains incomplete after the earlier stop at about eight hours. 1.4.0 is not soak certified.
- Firefox remains unsupported (NFR-6 deviation unchanged). Supported browsers are Edge, Chrome and Brave (Chromium 120+).
- CDP is loopback-only and unauthenticated; the shipped Docker image and Kubernetes manifests contain no browser and cannot reach browser loopback on the host.
- Forced `cdp` has no fallback, macOS has no packaged installer, Linux packaging is x86_64 only, and extension tab ids are still not bridged to CDP target ids.

## 1.3.0 - 2026-11-15

Token-efficiency release. The MCP surface grows from 42 to 43 tools; the original 32 result shapes remain. Default snapshot and discovery output is leaner, and forced CDP now serves every browser-facing tool.

### Added

- `browser_batch`: up to 20 allowlisted browser steps in one call, with `stop_on_error` default true and a per-step result array; the tool itself is daemon-local.
- `page_snapshot` compact text on both extension and CDP, with `format: "compact"` as the default, `format: "json"` for node objects, `viewport_only`, `include_bbox`, and `@eN` or `@c<backendNodeId>` references.
- Lean `tools/list`: brief is the default and strips titles plus schema property descriptions; `detail: "full"` restores them. `tool_help` returns one full descriptor or a category index.
- Full CDP parity for all 33 browser-facing tools, including the eight former extension-only tools and `@c` references.
- Nine tool categories, 43 tools in total, and `_meta["corvus.category"]` on every definition.

### Changed

- A successful `tools/call` returns one compact JSON text block and no `structuredContent`; clients parse `content[0].text`.
- Brief `tools/list` is 20,452 bytes for 43 tools; full detail is 37,067 bytes. The original 32 result shapes remain available.
- Documentation, README, extension metadata and release assertions now state 1.3.0 and 43 tools.
- Logging events follow one schema: lowercase event messages, `snake_case` fields, identifiers suffixed with `_id`, and no `message` field that displaces the event text.
- A failed MCP tool call is logged at `warn` instead of `info`; the startup summary is one `corvusd listening` event and the duplicate `corvusd ready` line is gone.

### Fixed

- Removed stale 1.2.0 and 42-tool claims from the v1.3 documentation, roadmap and README while keeping the v1.2.0 release notes as historical records.

### Limitations

- NFR-3 is not passed: the clean 72-hour soak was stopped at the client's request after about eight hours. 1.3.0 is not 72-hour soak certified.
- Firefox remains unsupported (NFR-6 deviation unchanged). Supported browsers are Edge, Chrome and Brave (Chromium 120+).
- CDP is loopback-only and unauthenticated; a browser-level endpoint can control every profile in that browser instance. Never expose 9222, 9223 or 9333.
- The shipped Docker image and Kubernetes manifests contain no browser and cannot reach browser loopback on the host. The sidecar browser path is not supported out of the box; use the extension channel or `--no-cdp`.
- Forced `cdp` has no automatic fallback; if the endpoint is unavailable, browser-facing calls fail instead of moving to the extension.
- macOS has no packaged installer; Linux packaging is x86_64 only.
- Extension tab ids and CDP target ids remain separate namespaces in 1.3.0. `@c` and `@eN` references are channel-specific and cross-channel use returns typed `BAD_ARGS`; a full bridge is deferred.
- A configured extension CDP endpoint changed while the daemon is connected may need a daemon restart; auto-discovered endpoint changes are re-polled every 500 ms and reconnect automatically.
- The compact-versus-JSON benchmark records candidate means; individual pages vary.

## 1.2.0 - 2026-11-15

Hybrid transport release. The MCP surface grows from 32 to 42 tools. The original 32 keep their arguments and result shapes. A browser launched with remote debugging can now be driven over a loopback browser-level CDP connection, while the extension remains the zero-setup default.

### Added

- Direct browser-level CDP client in `corvus-cdp`: one long-lived WebSocket, flattened Target sessions, a 1000-event ring, reconnect with 250 ms to 10 s backoff, a 15 s heartbeat and a 10 s default / 60 s maximum command timeout.
- Ten tools: `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events`, `browser_channel`, `browser_console`, `browser_cookies`, `browser_dialog`, `browser_emulate` and `tool_help`.
- Channel selection at startup and at runtime: `auto` prefers CDP and falls back only when the route is Unsupported, `cdp` is strict with no fallback, and `extension` ignores CDP. Daemon flags `--cdp-endpoint`, `--no-cdp` and `--channel`; environment variables `CORVUS_CDP_ENDPOINT` and `CORVUS_CHANNEL`.
- Extension CDP detection on loopback ports 9222, 9223 and 9333 with same-browser-family selection and storage under `corvus.cdpEndpoint`; the `cdp` capability is advertised only after detection succeeds.
- Two-level tool categories in `tools/list` and a brief default / full detail switch; `tool_help` returns the full descriptor. The brief list is 29,971 bytes for 42 tools and the full list is 35,552 bytes.
- Repository materials: `SECURITY.md`, `SUPPORT.md`, `CODE_OF_CONDUCT.md`, `CITATION.cff`, three issue forms, a pull request template, `CODEOWNERS`, Dependabot configuration and release-note categories.
- Documentation gates: every crate root enables `missing_docs`, extension scripts start with `@fileoverview`, and CI builds the docs with `RUSTDOCFLAGS=-D warnings`.

### Changed

- `tools/list` now returns 42 tools instead of 32; the original result shapes are unchanged.
- The extension options page stores the daemon URL and the optional CDP endpoint; the popup shows both connection and CDP state.
- Packaging, Docker and CI release-gate assertions expect version 1.2.0 and 42 tools.

### Fixed

- Stale release metadata no longer asserts the previous 1.1.0 / 32-tool surface in the Docker and CI gates.

### Limitations

- NFR-3 is not passed: the clean 72-hour soak was stopped at the client's request after about eight hours. 1.2.0 is not 72-hour soak certified.
- Firefox remains unsupported (NFR-6 deviation unchanged). Supported browsers are Edge, Chrome and Brave (Chromium 120+).
- CDP is loopback-only and unauthenticated; a browser-level endpoint can control every profile in that browser instance. Never expose 9222, 9223 or 9333.
- The shipped Docker image and Kubernetes manifests contain no browser and cannot reach browser loopback on the host. The sidecar browser path is not supported out of the box; use the extension channel or `--no-cdp`.
- Forced `cdp` has no automatic fallback; extension-only tools require the extension.
- macOS has no packaged installer; Linux packaging is x86_64 only.
- Extension tab ids and CDP target ids are separate namespaces; v1.2.0 returns typed `BAD_ARGS` hints and extension-only tools work without `tab_id`, with full bridging deferred to 1.3.
- A configured extension CDP endpoint changed while the daemon is connected may need a daemon restart; auto-discovered endpoint changes are re-polled and reconnect automatically (reconnect verified).

## 1.1.0 - 2026-11-15

Deployment release. The 32-tool MCP surface is unchanged. Every backend file now lives under one root, and a Linux package, an amd64 Docker image and Kubernetes manifests join the Windows installer.

### Added

- `--home <dir>` on `corvusd serve`, `corvus serve` and both doctor commands; `CORVUS_HOME` selects the same root. The layout is `bin/`, `data/corvus.db`, `logs/`, `config/` and `extension/`; `--db` still overrides the database path.
- Daily rolling file logging at `logs/corvusd.log.YYYY-MM-DD` (UTC). `--log-file` overrides the path, `--no-log-file` disables it, and stderr plus the in-memory log ring are unchanged.
- A Linux x86_64 release tarball with `install-linux.sh`, `uninstall-linux.sh`, a systemd unit and `SHA256SUMS`.
- An amd64 Docker image and Kubernetes manifests, covered by the release-gate CI checks.
- `corvus config --home <dir>` pins stdio snippets to that root; HTTP snippets are unchanged.
- Doctor reports the database path, schema version, session and step counts, uptime and tool-call totals.
- The extension options page stores a custom `ws://` or `wss://` daemon URL.

### Changed

- The Windows installer defaults to `%ProgramData%\Corvus`. `-InstallDir`, `-DataDir` and `-ExtensionDir` remain as deprecated aliases that must resolve inside the home root.
- A 1.0.0 `%USERPROFILE%\.corvus` tree is migrated to `<HomeDir>\migration\userprofile-corvus`; set `-KeepLegacyData` to keep the old tree in place.
- `corvus serve` writes its startup notice to stderr so `--stdio` keeps stdout clean.

### Known limitations

- The clean NFR-3 72-hour soak started 2026-11-14T14:41:37Z and was stopped at the client's request after about eight hours (last checkpoint 8 at 2026-11-14T22:41:37Z). The planned 72-hour run will not be completed. NFR-3 is not passed in 1.1.0.
- Firefox 121+ is unsupported. The NFR-6 deviation remains open.
- Linux packaging is x86_64 only; macOS has no packaged installer in this release.
- The daemon has no authentication. Keep ports 7330 and 7331 on loopback; a remote `wss://` endpoint needs a trusted tunnel.
- The Docker image and Kubernetes manifests are release-gate CI checks only; no registry image or hosted deployment is part of this release.

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

## 1.5.0 - 未发布

工作树中的未发布工作。MCP 面保持 43 个工具、九个类别不变。版本元数据现为 1.5.0；QA 验收与 Windows 安装包验收已在脏工作树上 PASS，但不存在 1.5.0 标签或 CI 运行，本地安装包 ZIP 也不是已发布产物。

### 新增

- `page_snapshot` 感知 v2：开放 shadow root 与同源子 frame、文档顺序 frame 路径（`f0`、`f0.0`、`f1`）、带 `cross_origin`、`unavailable`、`geometry_unavailable` 跳过原因的 `frames` 摘要、全局 `max_nodes` 预算，以及投影到顶层视口的包围盒。跨源 frame 的 DOM 永不读取。
- 紧凑快照差异：`diff` 默认 false，仅用于紧凑输出；与 JSON 同时使用返回类型化 `BAD_ARGS`。应用层按浏览器、会话与标签页为键的 64 项 LRU 增加 `baseline`、`unchanged`、`changed`、`added`、`removed`，使用两个空格、`+ `、`~ ` 与追加的 `- ` 标记。非差异紧凑请求刷新基线；差异与 JSON 请求不刷新。
- 扩展国际化：vue-i18n、英文与 zh-CN 目录、从 `chrome.i18n.getUILanguage()`/`navigator.language` 检测语言、`corvus.locale` 持久化、无需刷新的响应式切换、`document.documentElement.lang`，以及 MV3 `_locales`。

### 变更

- 扩展构建现在包含 14 个文件，其中包括两份语言目录；扩展门禁实测 8 个文件中的 41 个 Vitest 测试与隔离 Playwright 1/1。
- Windows 安装包验收在本地 ZIP 上通过 98 项检查与观察、0 失败；该本地产物不是 CI 重建产物。
- `browser_batch` 共用应用层快照路径，因此 `page_snapshot` 步骤只按同一差异规则处理一次。
- 43 工具 MCP 目录、九个类别、紧凑信封与 `browser_batch` 面保持不变。
- 禁止推理文本的规则改为 CI 机械执行：`scripts/check-cot.mjs` 扫描仓库文件与拉取请求的提交信息，命中即失败。

## 1.4.0 - 2026-11-15

结构版本。MCP 工具面保持 43 个工具、紧凑信封与 `browser_batch` 不变；后端改为端口与适配器结构，新增 REST `/api/v1` 面，录制迁移到 SeaORM，扩展改为由 TypeScript 与 Vue 3 源码构建。

### 新增

- `corvus-domain` 承载纯领域模型，`corvus-application` 承载用例与 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口；`corvusd` 是装配根。
- `corvus-api`：基于 Axum 的 REST `/api/v1`，覆盖健康、浏览器、会话、工具与认证，OpenAPI 位于 `/api-docs/openapi.json`，Swagger UI 位于 `/docs`。REST 与 MCP 共用同一套 43 工具目录与执行器。
- 可选的本地认证：Argon2id 口令、HS256 JWT、900 秒访问令牌与轮换的 30 天刷新令牌，且没有默认账号。`--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH` 是非回环绑定且不启用认证时的显式不安全豁免。
- SeaORM 与 `sea-orm-migration` 录制：`schema_version` 为 1，提供基线迁移；既有数据库在迁移前执行 `VACUUM INTO` 备份；单写入连接、WAL、5 秒 busy timeout、30 天保留与 262144 字节载荷截断。
- 扩展重写：弹窗与设置页使用 Vue 3、Pinia 与 Naive UI；Service Worker、内容脚本与 offscreen 文档使用无框架 TypeScript；类型化 RPC；三次 Vite 构建输出到 `extension/build/extension`；MV3 十项权限；21 个单元、契约与组件测试，外加隔离的 Playwright 套件。

### 变更

- 删除 `corvus-runtime`。路由职责移入 `corvus-application` 与 `corvusd`，依赖只指向内部领域层。
- 扩展改用 `pnpm install && pnpm build` 构建，并从 `extension/build/extension` 加载，而不是源码目录。
- 元数据与发布断言改为 1.4.0。MCP 目录仍为九个类别、43 个工具，沿用 1.3.0 的 schema、紧凑单文本结果与 `browser_batch`。

### 修复

- 录制库改为显式的迁移基线，不再依赖临时建表；既有 1.3.0 数据库无需数据迁移即可打开。
- 替换 README、安装与架构文档中过时的 1.3.0 版本号与“未打包源码扩展”说明。

### 限制

- 1.4.0 的 REST 认证账号与刷新令牌保存在内存中，重启后不保留；持久化认证存储属于后续工作。
- bearer 守卫只覆盖 `/api/v1`，其中 `/api/v1/health`、login 与 refresh 豁免。即使启用本地认证，`/mcp`、扩展 WebSocket、旧版 `/health`、`/metrics`、`/api-docs/openapi.json` 与 `/docs` 仍无认证。启用 REST 认证不会让非回环守护进程对远程安全。
- 仓库内的 Kubernetes Deployment 覆盖了镜像入口，却没有传入 `--allow-no-auth` 或本地认证，因此按现状会在 1.4.0 启动守卫处失败。镜像入口确实传入了该豁免，但由此得到的容器明确无认证，不是开箱即用的安全配置。
- NFR-3 未通过：计划中的 72 小时压测在约八小时停止后仍未完成，1.4.0 不能宣称通过压测。
- Firefox 仍不支持（NFR-6 偏差不变）。支持浏览器为 Edge、Chrome 与 Brave（Chromium 120+）。
- CDP 仅限回环且没有认证；发布 Docker 镜像与 Kubernetes 清单不含浏览器，也无法访问宿主机浏览器回环。
- 强制 `cdp` 不回退，macOS 没有打包安装器，Linux 打包仅支持 x86_64，扩展标签 id 仍未与 CDP 目标 id 桥接。

## 1.3.0 - 2026-11-15

Token 效率版本。MCP 工具面从 42 个增至 43 个，原有 32 个结果形状保持不变。默认快照与工具发现输出更精简，强制 CDP 现在覆盖全部浏览器类工具。

### 新增

- `browser_batch`：一次调用最多顺序执行 20 个允许列表内的浏览器步骤，`stop_on_error` 默认 true，返回逐步结果数组；该工具由守护进程本地处理。
- `page_snapshot` 在扩展与 CDP 两个通道上都支持紧凑文本，默认 `format: "compact"`，`format: "json"` 返回节点对象，支持 `viewport_only`、`include_bbox`，引用为 `@eN` 或 `@c<backendNodeId>`。
- 精简 `tools/list`：默认简版，移除标题与 schema 属性描述；`detail: "full"` 恢复它们。`tool_help` 返回单个完整条目或类别索引。
- CDP 完整对等：全部 33 个浏览器类工具，包括此前仅扩展的八个工具与 `@c` 引用。
- 九个工具类别、共 43 个工具，每个定义带 `_meta["corvus.category"]`。

### 变更

- 成功的 `tools/call` 只返回一个紧凑 JSON 文本块，不返回 `structuredContent`；客户端解析 `content[0].text`。
- 43 个工具的简版 `tools/list` 为 20,452 字节，完整详细为 37,067 字节。原有 32 个结果形状仍可用。
- 文档、README、扩展元数据与发布断言改为 1.3.0 / 43 个工具。
- 日志事件统一结构：小写事件消息、`snake_case` 字段、标识符以 `_id` 结尾，不再有挤占事件文本的 `message` 字段。
- 失败的 MCP 工具调用记为 `warn`，不再用 `info`；启动摘要只保留一条 `corvusd listening` 事件，重复的 `corvusd ready` 已删除。

### 修复

- 从 v1.3 文档、路线图与 README 中移除了旧的 1.2.0 / 42 工具表述，同时保留 v1.2.0 发布说明作为历史记录。

### 限制

- NFR-3 未通过：干净的 72 小时压测在客户要求下于约八小时后停止，1.3.0 不能宣称通过 72 小时压测。
- Firefox 仍不支持（NFR-6 偏差不变）。支持浏览器为 Edge、Chrome 与 Brave（Chromium 120+）。
- CDP 仅限回环且没有认证；浏览器级端点可控制该浏览器实例中的所有 profile。绝不要暴露 9222、9223 或 9333。
- 发布 Docker 镜像与 Kubernetes 清单不含浏览器，也无法访问宿主机浏览器回环。sidecar 浏览器不是开箱即用路径；请使用扩展通道或 `--no-cdp`。
- 强制 `cdp` 不会自动回退；端点不可用时浏览器类调用直接失败，不会改走扩展。
- macOS 没有打包安装器；Linux 打包仅支持 x86_64。
- 1.3.0 中扩展标签 id 与 CDP 目标 id 仍属于不同命名空间。`@c` 与 `@eN` 引用按通道区分，跨通道使用返回类型化 `BAD_ARGS`；完整桥接延后。
- 守护进程已连接时修改显式配置的扩展 CDP 端点，可能需要重启守护进程才能生效；自动发现的端点每 500 毫秒重新轮询并自动重连。
- 紧凑与 JSON 基准记录的是候选均值；具体页面会有差异。

## 1.2.0 - 2026-11-15

混合传输版本。MCP 工具面从 32 个增至 42 个，原有 32 个工具的参数与结果结构保持不变。以远程调试启动的浏览器可经回环浏览器级 CDP 直连驱动，扩展仍是零配置默认路径。

### 新增

- `corvus-cdp` 中的浏览器级 CDP 直连客户端：单条长连接 WebSocket、展平的 Target 会话、1000 条事件环、250 毫秒至 10 秒退避重连、15 秒心跳，以及默认 10 秒、最大 60 秒的命令超时。
- 十个工具：`cdp_status`、`cdp_targets`、`cdp_send`、`cdp_events`、`browser_channel`、`browser_console`、`browser_cookies`、`browser_dialog`、`browser_emulate` 与 `tool_help`。
- 启动时与运行时均可选通道：`auto` 优先 CDP，仅在路由为 Unsupported 时回退；`cdp` 严格直连、绝不回退；`extension` 忽略 CDP。守护进程参数 `--cdp-endpoint`、`--no-cdp`、`--channel`；环境变量 `CORVUS_CDP_ENDPOINT` 与 `CORVUS_CHANNEL`。
- 扩展在回环端口 9222、9223、9333 上探测 CDP，并按同一浏览器家族选择端点，结果存于 `corvus.cdpEndpoint`；只有探测成功后才声明 `cdp` 能力。
- `tools/list` 支持两级分类与默认简版 / 完整详情切换；`tool_help` 返回完整描述。简版列表为 42 个工具 29,971 字节，完整列表 35,552 字节。
- 仓库材料：`SECURITY.md`、`SUPPORT.md`、`CODE_OF_CONDUCT.md`、`CITATION.cff`、三个 issue 表单、pull request 模板、`CODEOWNERS`、Dependabot 配置与发布说明分类。
- 文档门禁：每个 crate 根启用 `missing_docs`，扩展脚本以 `@fileoverview` 起始，CI 以 `RUSTDOCFLAGS=-D warnings` 构建文档。

### 变更

- `tools/list` 现在返回 42 个工具而非 32 个；原有结果结构不变。
- 扩展设置页保存守护进程地址与可选 CDP 端点；弹窗同时展示连接与 CDP 状态。
- 打包、Docker 与 CI 发布门禁断言版本为 1.2.0、工具数为 42。

### 修复

- 发布元数据不再在 Docker 与 CI 门禁中断言旧的 1.1.0 / 32 工具面。

### 限制

- NFR-3 未通过：干净的 72 小时压测在客户要求下于约八小时后停止，1.2.0 不能宣称通过 72 小时压测。
- Firefox 仍不支持（NFR-6 偏差不变）。支持浏览器为 Edge、Chrome 与 Brave（Chromium 120+）。
- CDP 仅限回环且没有认证；浏览器级端点可控制该浏览器实例中的所有 profile。绝不要暴露 9222、9223 或 9333。
- 发布 Docker 镜像与 Kubernetes 清单不含浏览器，也无法访问宿主机浏览器回环。sidecar 浏览器不是开箱即用路径；请使用扩展通道或 `--no-cdp`。
- 强制 `cdp` 不会自动回退；仅扩展工具需要扩展。
- macOS 没有打包安装器；Linux 打包仅支持 x86_64。
- 扩展标签 id 与 CDP 目标 id 属于不同命名空间；v1.2.0 返回类型化 `BAD_ARGS` 提示，仅扩展工具可省略 `tab_id` 使用，完整桥接推迟到 1.3。
- 守护进程已连接时修改显式配置的扩展 CDP 端点，可能需要重启守护进程才能生效；自动发现的端点变化会重新轮询并自动重连（重连已验证）。

## 1.1.0 - 2026-11-15

部署版本。32 个 MCP 工具面保持不变。所有后端文件归入单一根目录，Linux 安装包、amd64 Docker 镜像与 Kubernetes 清单同 Windows 安装器一起发布。

### 新增

- `corvusd serve`、`corvus serve` 与两个 doctor 命令支持 `--home <dir>`；`CORVUS_HOME` 选择同一个根目录。布局为 `bin/`、`data/corvus.db`、`logs/`、`config/` 与 `extension/`；`--db` 仍可覆盖数据库路径。
- 每日滚动文件日志位于 `logs/corvusd.log.YYYY-MM-DD`（UTC）。`--log-file` 覆盖路径，`--no-log-file` 关闭文件日志；stderr 与内存日志环保持不变。
- Linux x86_64 发布 tar 包，包含 `install-linux.sh`、`uninstall-linux.sh`、systemd unit 与 `SHA256SUMS`。
- amd64 Docker 镜像与 Kubernetes 清单，由发布门禁的 CI 检查覆盖。
- `corvus config --home <dir>` 让 stdio 片段固定使用该根目录；HTTP 片段不变。
- doctor 报告数据库路径、schema 版本、会话与步骤数量、运行时长与工具调用总量。
- 扩展设置页可保存自定义 `ws://` 或 `wss://` 守护进程地址。

### 变更

- Windows 安装器默认使用 `%ProgramData%\Corvus`。`-InstallDir`、`-DataDir` 与 `-ExtensionDir` 保留为已弃用别名，且必须解析到根目录内部。
- 1.0.0 的 `%USERPROFILE%\.corvus` 目录会迁移到 `<HomeDir>\migration\userprofile-corvus`；设置 `-KeepLegacyData` 可保留旧目录。
- `corvus serve` 的启动提示改写到 stderr，使 `--stdio` 的 stdout 保持干净。

### 已知限制

- 干净的 NFR-3 72 小时压测于 2026-11-14T14:41:37Z 启动，在客户要求下于约八小时后停止（最后检查点 8：2026-11-14T22:41:37Z）。计划中的 72 小时运行不会完成。1.1.0 未通过 NFR-3。
- 暂不支持 Firefox 121+。NFR-6 偏差仍未关闭。
- Linux 打包仅支持 x86_64；macOS 在本版本没有打包安装器。
- 守护进程没有认证层。7330 与 7331 必须保持在回环地址；远程 `wss://` 端点需要可信隧道。
- Docker 镜像与 Kubernetes 清单只属于发布门禁的 CI 检查；本版本不包含镜像仓库发布或托管部署。

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
