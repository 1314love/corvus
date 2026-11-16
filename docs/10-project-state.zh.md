# Corvus 项目现状

[English](10-project-state.md) | [中文](10-project-state.zh.md)

置信度：以下事实均读自仓库以及 1.6.0、1.5.0 与 1.4.0 验收报告；1.7.0 的 QA 验收与本地 Windows 打包已在冻结树上通过，报告已包含在工作树中。引用本页前请核对 `git log -1` 与发布页。1.6.0 是当前已发布版本，发布于 2026-11-15；1.5.0 为上一版本。正式 QA 验收报告记录 HEAD `61ca85f` 工作树 PASS。1.7.0 工作树把 MCP 工具面重划为十类 62 个工具；此前的 44 工具 QA 与本地打包结果早于本次重划；62 条目 schema 快照与一致性测量描述冻结工作树，因此 CI 与发布证据仍为 Pending。读者：维护者与评审者。

## 1. 当前版本：1.6.0

1.6.0 于 2026-11-15 由附注标签 `v1.6.0` 发布；标签对象 `c5daf98eb03b0fac310e53c3a59edfa77874f919` 指向发布提交 `b420c3787a8f240c60514ac8c745e67474f1c1fd`。GitHub 发布 `446071476` 发布了产物，主 CI 运行 `43716832915` 通过八个任务，发布工作流 `43717236667` 成功，[tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 HEAD `61ca85f` 工作树上的 QA PASS。1.5.0 为上一版本。

1.6.0 守护进程延续 1.5.0 的十 crate 端口与适配器后端：`corvus-domain`、`corvus-application`、`corvus-api`、`corvus-proto`、`corvus-mcp`、`corvus-transport`、`corvus-record`、`corvus-cdp`、`corvusd` 与 `corvus`；`corvus-runtime` 仍已移除。它新增 `crates/corvusd/src/guardrails.rs` 中的可选守护策略。1.6.0 发布在 `crates/corvus-mcp/src/tools.rs` 声明 `const SPECS: [Spec; 43]`；1.7.0 工作树在十类中声明 62 个规格并新增 18 个工具，`browser_screenshot` 为第 44 个。1.6.0 QA 实测 43 个工具、九个类别，`tools/list` 简版正文 20,493 字节，`detail: "full"` 为 37,496 字节。此前的 44 工具工作树实测简版/默认 21,164 字节、完整 38,964 字节，但这些数值早于 62 工具重划；62 工具目录实测简版/默认原始 31,367 字节（含末尾换行 31,368 字节）、完整原始 52,328 字节（含换行 52,329 字节）。发布证据见[发布说明](release-notes/v1.6.0.zh.md)与 [tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md)；[v1.5.0.md](../tests/acceptance/v1.5.0.md) 为上一基线。

1.6.0 发布内容包括：

- `crates/corvus-domain`、`crates/corvus-application` 与 `crates/corvus-api` 延续端口与适配器分层。application crate 定义 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，并在 `crates/corvus-application/src/spec.rs` 保留 `pub const TOOLS: [ToolSpec; 32]`。
- REST 面在 `/api/v1` 提供 health、browsers、sessions 与 tools，OpenAPI 位于 `/api-docs/openapi.json`，Swagger 位于 `/docs`。认证默认 `off`；`local` 启用 Argon2id 与 HS256 JWT；`corvusd` 挂载 `corvus_api::router`。
- `corvus-record` 使用 SeaORM 与 `sea-orm-migration`，schema 版本为 1；既有数据库在迁移前执行 `VACUUM INTO` 备份，并保持 WAL、单写入者与 5 秒 busy timeout。
- `extension/` 使用 TypeScript 与 Vue 3 编写，采用 Pinia、Naive UI、Vite、Vitest 与 Playwright；三次 Vite 构建产出 `extension/build/extension`。
- 已发布的 1.6.0 产物报告 1.6.0；`Cargo.toml`、`extension/package.json` 与 `extension/manifest.json` 元数据现在为未发布的 1.7.0 工作树报告 1.7.0。

MCP 面保持不变：43 个工具、九个类别、紧凑单文本信封与 `browser_batch`。[tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录了 25 条结果行上的 237 个 workspace 测试、137 项一致性检查、10 个文件中的 55 个 Vitest 测试、14 个文件/490,393 B 的扩展构建与 Playwright 1/1 的 PASS。主 CI 运行 43716832915 与发布产物哈希已记录在 1.6.0 发布说明中。

## 2. 1.6.0 发布与证据

1.6.0 发布新增可选守护进程策略；不新增 MCP 工具。

- **守护策略。** `crates/corvusd/src/guardrails.rs` 接受严格 version 1 JSON：只读模式、工具允许与拒绝清单、主机与私有网络导航门禁、cookie 脱敏与不可信内容边界。非法策略中止启动。
- **执行。** 检查在派发前运行于 `RouterBackend::call`、每个 `browser_batch` 步骤与 `session_replay`。拒绝使用第 11 个错误码 `POLICY_DENIED`，REST 返回 403。
- **脱敏与边界。** `browser_cookies` 脱敏 cookie 值；`page_text`、`page_html` 与紧凑 `page_snapshot` 文本用不可信内容边界包裹。密码输入仍掩码。
- **表面不变。** 43 个 MCP 工具、九个类别；1.6.0 不新增工具。

发布实测：14 个扩展构建文件、490,393 B，10 个文件中的 55 个 Vitest 测试、隔离 Playwright 1/1；QA 记录 25 条结果行上的 237 个 workspace 测试、137 项一致性检查，以及一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。已发布产物是 CI 重建产物：Windows 18,822,522 B SHA-256 `bdec40f2f26012f2aeb5f977d6eef4fe80c18a662683c4fa9004ea99f59451db`，Linux 20,124,260 B SHA-256 `e7a21d3996e52f8a0c5725d959987aa8c96eacfb044a928341575af19d4ce738`，合并 `SHA256SUMS` 194 B SHA-256 `4ee452ce947c4b47cbe9cca4eabc64d8eac1345177f27ea3e2b0acca2e49f5bf`。独立的本地安装包 ZIP 为 18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`，84 项检查与观察、0 失败。NFR-3 仍未通过，不宣称任何 1.6.0 实机部署。

## 3. v1.7.0 工作树（未发布）

v1.7.0 工作树重划 MCP 工具面：新增 18 个工具，达到十类 62 个，`browser_screenshot` 为第 44 个，page 类别为 15 个。它把 `viewport`、`full_page` 或 `element` 捕获为 `webp`、`png` 或 `jpeg`；`quality` 为 1..100，png 忽略；`inline` 默认 false，`inline: true` 追加一个 MCP 图像内容块。文件由 `crates/corvusd/src/screenshot.rs` 写入 `<home>/screenshots/`，上限为解码后 8 MiB，并由 `crates/corvusd/src/lib.rs` 的 `finalize_screenshot_result` 只最终处理一次。`page_pdf` 在 `<daemon home>/pdf` 下写入唯一 PDF，解码上限 32 MiB，不返回内联 PDF。

CDP 提供 46 个浏览器类工具，扩展提供 42 个，12 个为守护进程本地。四个标签页与窗口工具（`tab_group`、`tab_duplicate`、`window_list`、`window_new`）仅扩展可用，强制 CDP 返回 `CAPABILITY_UNAVAILABLE`；CDP 接受 `@cN` 引用，扩展接受 `@eN` 引用，错误通道引用返回 `BAD_ARGS`。扩展声明 11 项权限（含 `tabGroups`）、42 个基于 debugger 的处理器与 `all_frames: false`。`browser_batch` 在十个具名排除项后保留 42 个可批量工具；`storage`、`sessions` 与 `help` 不可批量。录制与守护策略脱敏 `cookie_set` 与存储值；截图与 PDF 像素不脱敏、不包裹边界。

不存在 1.7.0 标签、CI 运行、已发布包或已发布产物。[tests/conformance/schema-snapshot.v1.7.0.json](../tests/conformance/schema-snapshot.v1.7.0.json) 的 schema 快照是 62 条目文件（36,993 B、1,853 行、SHA-256 `0763eee9256bae1942c1480317fc6a80375257d44b3674261b689ae1f3844114`）；一致性运行器为 52,902 B、1,046 行（SHA-256 `43df9ea1e2366eff9db6171b369b9b56af2a75fbc412c04b589b52bb7e3d691f`）。冻结的 62 工具二进制上一致性检查通过 137 项、0 失败、0 跳过。此前的 44 工具 v1.7.0 结果已被取代。QA 验收与本地 Windows 打包已在重新冻结的 62 工具树上通过：25 条结果行上的 263 个 workspace 测试、13 个文件中的 91 个 Vitest 测试、14 文件/537,844 B 的扩展构建、3 个 Playwright 端到端测试，以及在 ZIP 19,129,611 B SHA-256 `6b216bc251bfbbcd444823eed9ccebac97711ca5f9c66984a5f986735d86833e` 上的 160/160 项本地打包检查；该 ZIP 仅本地可用。1.7.0 的所有 CI 与发布行均为 Pending，1.6.0 仍是当前已发布版本。

## 4. 验证与打包证据

正式 QA 验收报告 [tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 HEAD `61ca85f` 工作树 PASS：25 条结果行上的 237 个 workspace 测试、0 失败；137 项一致性检查、0 失败、0 跳过；10 个文件中的 55 个 Vitest 测试；14 个构建文件、490,393 B；Playwright 1/1。记录一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。已发布的 Windows、Linux 与合并校验和产物是标签 `v1.6.0` 的 CI 重建产物。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败；它不是已发布产物。

62 工具 v1.7.0 的 QA 验收与本地 Windows 打包报告作为工作树证据存在于重新冻结的树上：25 条结果行上的 263 个 workspace 测试、137 项一致性检查、13 个文件中的 91 个 Vitest 测试、14 文件/537,844 B 的构建、3 个 Playwright 端到端测试与 160/160 项打包检查。此前的 44 工具报告已被取代，不能作为该树的证据。报告已包含在工作树中，外部证据目录未提交，1.7.0 的所有 CI 与发布行仍为 Pending。

## 5. 事实来源

| 事实 | 读取位置 |
|---|---|
| 1.7.0 工作树的 MCP 面为十类 62 个工具；已发布 1.6.0 为 43 个 | `crates/corvus-mcp/src/tools.rs` |
| 浏览器侧应用规格为 49 个工具 | `crates/corvus-application/src/spec.rs` |
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
| 1.7.0 截图代码、工作树验收与本地打包 | `tests/acceptance/v1.7.0.zh.md`、`tests/report/v1.7.0-packaging.md`、`crates/corvus-mcp/src/tools.rs`、`crates/corvus-cdp/src/tools.rs`、`extension/src/background/screenshot.ts`、`crates/corvusd/src/screenshot.rs`、`crates/corvusd/src/lib.rs` |

## 6. 仍然适用的限制

- NFR-3 未通过：72 小时浸泡在约 8 小时处因客户要求停止。见 [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md)。
- 不支持 Firefox；见 [06-firefox-assessment.md](06-firefox-assessment.md)。
- CDP 客户端只接受回环端点，且没有认证。
- Docker 与 Kubernetes 镜像不带浏览器。
- 强制走 CDP 时没有回退到扩展。
- macOS 没有安装包。
- 扩展的 tab id 与 CDP 的 tab id 仍是两套命名空间。
- 认证账号与刷新令牌保存在内存中，重启后失效；系统没有默认账号。
- REST 认证不覆盖 `/mcp`、扩展 WebSocket 与 Swagger，非回环监听仍需可信认证代理。
- 守护策略限制工具与导航参数，但不是网络沙箱：不解析 DNS，被允许的页面仍可请求其他源。
- 内容边界只包裹页面文本、页面 HTML 与紧凑快照文本；JSON 快照不包裹，`browser_eval`、`page_html` 与 `browser_history` 仍可能暴露页面或机密数据。
- 策略拒绝会在错误详情中带策略文件路径，不要把原始守护进程错误暴露给不可信客户端。
- REST 的 `session_id` 字段可接受，但尚未用于标注录制。
- 1.6.0 由标签 `v1.6.0` 发布；QA 是在发布提交前、HEAD `61ca85f` 工作树上测得的，本地安装包 ZIP 与已发布的 CI 重建产物不同。不宣称任何 1.6.0 实机部署。
- 1.6.0 已在 HEAD `61ca85f` 的工作树上取得正式 QA PASS，并且是当前已发布版本。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败（18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`）；它不是已发布产物。
- 快照差异只适用于紧凑输出；`format: "json"` 与 `diff` 同时使用返回类型化 `BAD_ARGS`。
- 1.7.0 工作树重划为十类 62 个工具；不存在 1.7.0 标签、CI 运行、已发布包或已发布产物。62 条目 schema 快照以及一致性与测试测量描述冻结的 62 工具树；QA 验收与本地 Windows 打包已在该树上通过，CI 与发布证据仍为 Pending。截图与 PDF 像素不脱敏，也不包裹边界，四个标签页与窗口工具仅扩展可用。

## 7. 文档置信度

置信度分级：高表示本轮审查已对照带标签代码、工作树或已发布产物核实；中表示已核实，但依赖未提交的 1.7.0 工作或仓库外证据；低表示历史材料，不得当作现状。审查日期：2026-11-16。

| 领域 | 文档 | 置信度 |
|---|---|---|
| 需求 | [01-requirements.md](01-requirements.md) | 1.4.0 至 1.6.0 演进链与 1.7.0 的 62 工具树可信；标题仍是 v0.1，因为基线从未改名；第 4 节名称是原始目标矩阵，不是已交付目录 |
| 架构 | [02-architecture.md](02-architecture.md) | 1.6.0 与 1.7.0 工作树可信；crate 列表、REST 面与扩展构建路径已在本轮核实 |
| 路线图 | [03-roadmap.md](03-roadmap.md) | 中；1.6.0 已发布，1.7.0 为 62 工具工作树内容，QA 与本地打包 PASS 但未发布，NFR-3 仍未关闭 |
| 安装 | [04-install.md](04-install.md) | 中；版本戳与 Linux 资产名为 1.6.0，macOS 没有打包安装器，数据库备份行为自 1.4.0 起已记录 |
| 部署 | [05-deployment.md](05-deployment.md) | 中；迁移与守护策略注意事项已核实，Docker 与 Kubernetes 镜像不带浏览器 |
| Firefox 评估 | [06-firefox-assessment.md](06-firefox-assessment.md) | 中；NO-GO 决策与平台缺口为高，26/18 数字是 v0.1.0 基线，当前树在 CDP 上有 46 个、扩展上有 42 个浏览器侧工具，工期估算为低 |
| CDP 指南 | [07-cdp.md](07-cdp.md) | 1.6.0 为高；1.7.0 的 46 工具树仍未发布 |
| 日志 | [08-logging.md](08-logging.md) | 高；日志行为自 1.4.0 未变 |
| 工具面 | [09-tools.md](09-tools.md) | 1.6.0 目录为高；1.7.0 工作树为 62 个工具，简版/完整发现原始正文实测为 31,367/52,328 B |
| 守护策略 | [11-guardrails.md](11-guardrails.md) | 1.6.0 为高；截图与 PDF 像素不脱敏，也不包裹边界 |
| 项目现状 | 本页 | 高；下文来源表逐项对应；1.7.0 证据未提交且在仓库外 |
| 发布历史 | `docs/release-notes/` 0.1.0 至 1.4.0 | 中；对其所属版本正确，不代表现状 |
| 测试与归档 | `tests/acceptance/`、`tests/report/`、`tests/archive/` | 1.4.0 至 1.6.0 报告为中；1.4 以前验证、M1 至 M4 归档与仅英文的 v1.1.0 记录为低 |
| 入口文档 | `README.md`、`extension/README.md` | 高；1.6.0 已发布，1.7.0 明确为未发布的 62 工具工作树 |

## 8. 已被取代的材料

M1 至 M4 的验收手册与报告模板描述的是 JavaScript 扩展与 0.1.0 时期 18 至 26 个工具的旧表面，不再代表现状，已移入 `tests/archive/`。0.1.0 至 1.3.0 的验收与发布记录，包括 Brave 与 Chrome 验证矩阵，也已移入 `tests/archive/`，只作历史证据，不是当前验证。当前验证使用 [v1.6.0.md](../tests/acceptance/v1.6.0.md)、[v1.7.0 草案](../tests/acceptance/v1.7.0.zh.md)、`tests/conformance/run.sh` 与 `tests/perf/`。1.6.0 是最新已发布版本。1.7.0 工作树有未提交的 QA PASS 报告与本地 Windows 打包 PASS 报告，证据均在仓库外，但没有标签、CI 运行、已发布包或发布；其发布证据仍为 Pending。

## 9. 复核方式

- `cargo tree -p corvusd --depth 1` 显示守护进程的直接依赖。
- `pnpm --dir extension build` 重新构建可加载的扩展。
- `git log -1 --oneline` 显示本页读取时的提交。
- `cargo test -p corvusd guardrails` 覆盖策略解析、优先级与门禁。