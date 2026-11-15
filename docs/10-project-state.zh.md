# Corvus 项目现状

[English](10-project-state.md) | [中文](10-project-state.zh.md)

置信度：以下事实均读自仓库以及 1.6.0、1.5.0 与 1.4.0 验收报告。引用本页前请核对 `git log -1` 与发布页。1.6.0 是当前已发布版本，发布于 2026-11-15；1.5.0 为上一版本。正式 QA 验收报告记录 HEAD `61ca85f` 工作树 PASS。读者：维护者与评审者。

## 1. 当前版本：1.6.0

1.6.0 于 2026-11-15 由附注标签 `v1.6.0` 发布；标签对象 `c5daf98eb03b0fac310e53c3a59edfa77874f919` 指向发布提交 `b420c3787a8f240c60514ac8c745e67474f1c1fd`。GitHub 发布 `446071476` 发布了产物，主 CI 运行 `43716832915` 通过八个任务，发布工作流 `43717236667` 成功，[tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 HEAD `61ca85f` 工作树上的 QA PASS。1.5.0 为上一版本。

1.6.0 守护进程延续 1.5.0 的十 crate 端口与适配器后端：`corvus-domain`、`corvus-application`、`corvus-api`、`corvus-proto`、`corvus-mcp`、`corvus-transport`、`corvus-record`、`corvus-cdp`、`corvusd` 与 `corvus`；`corvus-runtime` 仍已移除。它新增 `crates/corvusd/src/guardrails.rs` 中的可选守护策略。`crates/corvus-mcp/src/tools.rs` 声明 `const SPECS: [Spec; 43]`；1.6.0 QA 实测 43 个工具、九个类别，`tools/list` 简版正文 20,493 字节，`detail: "full"` 为 37,496 字节。发布证据见[发布说明](release-notes/v1.6.0.zh.md)与 [tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md)；[v1.5.0.md](../tests/acceptance/v1.5.0.md) 为上一基线。

1.6.0 发布内容包括：

- `crates/corvus-domain`、`crates/corvus-application` 与 `crates/corvus-api` 延续端口与适配器分层。application crate 定义 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，并在 `crates/corvus-application/src/spec.rs` 保留 `pub const TOOLS: [ToolSpec; 32]`。
- REST 面在 `/api/v1` 提供 health、browsers、sessions 与 tools，OpenAPI 位于 `/api-docs/openapi.json`，Swagger 位于 `/docs`。认证默认 `off`；`local` 启用 Argon2id 与 HS256 JWT；`corvusd` 挂载 `corvus_api::router`。
- `corvus-record` 使用 SeaORM 与 `sea-orm-migration`，schema 版本为 1；既有数据库在迁移前执行 `VACUUM INTO` 备份，并保持 WAL、单写入者与 5 秒 busy timeout。
- `extension/` 使用 TypeScript 与 Vue 3 编写，采用 Pinia、Naive UI、Vite、Vitest 与 Playwright；三次 Vite 构建产出 `extension/build/extension`。
- 已发布的 1.6.0 产物报告 1.6.0；`Cargo.toml`、`extension/package.json` 与 `extension/manifest.json` 元数据也报告 1.6.0。

MCP 面保持不变：43 个工具、九个类别、紧凑单文本信封与 `browser_batch`。[tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录了 25 条结果行上的 237 个 workspace 测试、137 项一致性检查、10 个文件中的 55 个 Vitest 测试、14 个文件/490,393 B 的扩展构建与 Playwright 1/1 的 PASS。主 CI 运行 43716832915 与发布产物哈希已记录在 1.6.0 发布说明中。

## 2. 1.6.0 发布与证据

1.6.0 发布新增可选守护进程策略；不新增 MCP 工具。

- **守护策略。** `crates/corvusd/src/guardrails.rs` 接受严格 version 1 JSON：只读模式、工具允许与拒绝清单、主机与私有网络导航门禁、cookie 脱敏与不可信内容边界。非法策略中止启动。
- **执行。** 检查在派发前运行于 `RouterBackend::call`、每个 `browser_batch` 步骤与 `session_replay`。拒绝使用第 11 个错误码 `POLICY_DENIED`，REST 返回 403。
- **脱敏与边界。** `browser_cookies` 脱敏 cookie 值；`page_text`、`page_html` 与紧凑 `page_snapshot` 文本用不可信内容边界包裹。密码输入仍掩码。
- **表面不变。** 43 个 MCP 工具、九个类别；1.6.0 不新增工具。

发布实测：14 个扩展构建文件、490,393 B，10 个文件中的 55 个 Vitest 测试、隔离 Playwright 1/1；QA 记录 25 条结果行上的 237 个 workspace 测试、137 项一致性检查，以及一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。已发布产物是 CI 重建产物：Windows 18,822,522 B SHA-256 `bdec40f2f26012f2aeb5f977d6eef4fe80c18a662683c4fa9004ea99f59451db`，Linux 20,124,260 B SHA-256 `e7a21d3996e52f8a0c5725d959987aa8c96eacfb044a928341575af19d4ce738`，合并 `SHA256SUMS` 194 B SHA-256 `4ee452ce947c4b47cbe9cca4eabc64d8eac1345177f27ea3e2b0acca2e49f5bf`。独立的本地安装包 ZIP 为 18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`，84 项检查与观察、0 失败。NFR-3 仍未通过，不宣称任何 1.6.0 实机部署。

## 3. 验证与打包证据

正式 QA 验收报告 [tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 HEAD `61ca85f` 工作树 PASS：25 条结果行上的 237 个 workspace 测试、0 失败；137 项一致性检查、0 失败、0 跳过；10 个文件中的 55 个 Vitest 测试；14 个构建文件、490,393 B；Playwright 1/1。记录一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。已发布的 Windows、Linux 与合并校验和产物是标签 `v1.6.0` 的 CI 重建产物。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败；它不是已发布产物。

## 4. 事实来源

| 事实 | 读取位置 |
|---|---|
| MCP 面为 43 个工具 | `crates/corvus-mcp/src/tools.rs` |
| 浏览器侧应用规格为 32 个工具 | `crates/corvus-application/src/spec.rs` |
| 应用层端口与用例 | `crates/corvus-application/src/` |
| 快照基线与差异语义 | `crates/corvus-application/src/snapshot_diff.rs`、`crates/corvus-application/src/router.rs` |
| 守护进程装配 | `crates/corvusd/Cargo.toml`、`crates/corvusd/src/lib.rs` |
| 守护策略与执行 | `crates/corvusd/src/guardrails.rs`、`crates/corvusd/src/lib.rs` |
| REST 路由与认证 | `crates/corvus-api/src/` |
| 录制 schema 与迁移 | `crates/corvus-record/src/` |
| Rust 工作区版本 | `Cargo.toml` |
| 扩展版本与构建 | `extension/package.json`、`extension/manifest.json`、`extension/vite.*.config.ts` |
| 扩展 i18n 目录 | `extension/src/ui/shared/i18n/`、`extension/_locales/` |
| 发布历史与限制 | `CHANGELOG.md`、`docs/release-notes/` |
| 1.4.0 验收证据 | `tests/acceptance/v1.4.0.md` |
| 1.5.0 发布与 QA 验收证据 | `tests/acceptance/v1.5.0.md`、`docs/release-notes/v1.5.0.md` |
| 1.6.0 发布、QA 验收与打包证据 | `tests/acceptance/v1.6.0.md`、`docs/release-notes/v1.6.0.md`、`tests/report/v1.6.0-packaging.md` |

## 5. 仍然适用的限制

- NFR-3 未通过：72 小时浸泡在约 8 小时处因客户要求停止。见 [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md)。
- 不支持 Firefox；见 [06-firefox-assessment.md](06-firefox-assessment.md)。
- CDP 客户端只接受回环端点，且没有认证。
- Docker 与 Kubernetes 镜像不带浏览器。
- 强制走 CDP 时没有回退到扩展。
- macOS 没有安装包。
- 扩展的 tab id 与 CDP 的 tab id 仍是两套命名空间。
- 1.5.0 的认证账号与刷新令牌保存在内存中，重启后失效；系统没有默认账号。
- REST 认证不覆盖 `/mcp`、扩展 WebSocket 与 Swagger，非回环监听仍需可信认证代理。
- 守护策略限制工具与导航参数，但不是网络沙箱：不解析 DNS，被允许的页面仍可请求其他源。
- 内容边界只包裹页面文本、页面 HTML 与紧凑快照文本；JSON 快照不包裹，`browser_eval`、`page_html` 与 `browser_history` 仍可能暴露页面或机密数据。
- 策略拒绝会在错误详情中带策略文件路径，不要把原始守护进程错误暴露给不可信客户端。
- REST 的 `session_id` 字段可接受，但尚未用于标注录制。
- 1.6.0 由标签 `v1.6.0` 发布；QA 是在发布提交前、HEAD `61ca85f` 工作树上测得的，本地安装包 ZIP 与已发布的 CI 重建产物不同。不宣称任何 1.6.0 实机部署。
- 1.6.0 已在 HEAD `61ca85f` 的工作树上取得正式 QA PASS，并且是当前已发布版本。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败（18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`）；它不是已发布产物。
- 快照差异只适用于紧凑输出；`format: "json"` 与 `diff` 同时使用返回类型化 `BAD_ARGS`。

## 6. 文档置信度

| 领域 | 文档 | 置信度 |
|---|---|---|
| 需求 | [01-requirements.md](01-requirements.md) | 1.4.0 演进链可信；标题仍是 v0.1，因为基线从未改名 |
| 架构 | [02-architecture.md](02-architecture.md) | 1.6.0 发布面可信；5.9 节覆盖已发布的 v1.5.0，5.10 节覆盖已发布的 v1.6.0 守护策略 |
| 工具面 | [09-tools.md](09-tools.md) | 高；43 个工具，frame 与 diff 在 1.5.0 中为当前能力，守护边界在 1.6.0 中为当前能力 |
| 路线图 | [03-roadmap.md](03-roadmap.md) | 中；1.6.0 已发布，1.5.0 为上一版本，NFR-3 仍未关闭 |
| 发布证据 | `tests/acceptance/v1.4.0.md`、`tests/acceptance/v1.5.0.md`、`tests/acceptance/v1.6.0.md` | 高；1.6.0 在 HEAD 61ca85f 上 PASS 且已发布；1.5.0 在 HEAD 73092ea 上 PASS 且为上一版本；1.4.0 在 HEAD 65fbf52 上 PASS |
| 归档 | `tests/archive/` | 低；仅供追溯，永不代表现状 |

## 7. 已被取代的材料

M1 至 M4 的验收手册与报告模板描述的是 JavaScript 扩展与 18 至 26 个工具的旧表面，不再代表现状，已移入 `tests/archive/`。0.1.0 到 1.6.0 的逐版本验证报告仍在 `tests/report/` 与 `tests/acceptance/`；1.6.0 的报告是当前证据，[v1.5.0.md](../tests/acceptance/v1.5.0.md) 为上一基线。当前验证使用 [v1.6.0.md](../tests/acceptance/v1.6.0.md)、`tests/conformance/run.sh` 与 `tests/perf/`；1.6.0 是最新已发布版本。

## 8. 复核方式

- `cargo tree -p corvusd --depth 1` 显示守护进程的直接依赖。
- `pnpm --dir extension build` 重新构建可加载的扩展。
- `git log -1 --oneline` 显示本页读取时的提交。
- `cargo test -p corvusd guardrails` 覆盖策略解析、优先级与门禁。