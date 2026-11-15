# Corvus 项目现状

[English](10-project-state.md) | [中文](10-project-state.zh.md)

置信度：以下事实均读自仓库与 1.4.0 验收报告。引用本页前请核对 `git log -1` 与发布页。cargo 工作区与扩展均为 1.4.0，v1.4.0 验收矩阵已通过。读者：维护者与评审者。

## 1. 当前版本：1.4.0

1.4.0 守护进程由十个 crate 装配：`corvus-domain`、`corvus-application`、`corvus-api`、`corvus-proto`、`corvus-mcp`、`corvus-transport`、`corvus-record`、`corvus-cdp`、`corvusd` 与 `corvus`；`corvus-runtime` 已移除，其路由职责位于 `corvus-application` 与 `corvusd`。`crates/corvus-mcp/src/tools.rs` 声明 `const SPECS: [Spec; 43]`；1.4.0 验收实测 43 个工具、九个类别，`tools/list` 简版正文 20,451 字节，`detail: "full"` 为 37,066 字节。发布证据见 [v1.4.0.md](../tests/acceptance/v1.4.0.md)；已被取代的 1.3.0 报告仍保存在 `tests/report/` 供追溯。

1.4.0 交付内容包括：

- `crates/corvus-domain`、`crates/corvus-application`、`crates/corvus-api` 提供端口与适配器分层。application crate 定义 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，并在 `crates/corvus-application/src/spec.rs` 保留 `pub const TOOLS: [ToolSpec; 32]`；`corvus-runtime` 已移除。
- REST 面在 `/api/v1` 提供 health、browsers、sessions 与 tools，OpenAPI 位于 `/api-docs/openapi.json`，Swagger 位于 `/docs`。认证默认 `off`；`local` 启用 Argon2id 与 HS256 JWT；`corvusd` 挂载 `corvus_api::router`。
- `corvus-record` 使用 SeaORM 与 `sea-orm-migration`，schema 版本为 1；既有数据库在迁移前执行 `VACUUM INTO` 备份，并保持 WAL、单写入者与 5 秒 busy timeout。
- `extension/` 重写为 TypeScript 与 Vue 3，使用 Pinia、Naive UI、Vite、Vitest 与 Playwright；三次 Vite 构建产出 `extension/build/extension`。
- `Cargo.toml`、`extension/package.json`、`extension/manifest.json` 均为 1.4.0。

MCP 面保持不变：43 个工具、九个类别、紧凑单文本信封与 `browser_batch`。[tests/acceptance/v1.4.0.md](../tests/acceptance/v1.4.0.md) 记录了必需矩阵的 PASS：199 个 workspace 测试（25 个套件）、135 项一致性检查，以及扩展 21 个测试与 Playwright 门禁。最终 CI 运行 43690906863 与发布产物哈希已记录在 1.4.0 发布说明中。

## 2. 事实来源

| 事实 | 读取位置 |
|---|---|
| MCP 面为 43 个工具 | `crates/corvus-mcp/src/tools.rs` |
| 浏览器侧应用规格为 32 个工具 | `crates/corvus-application/src/spec.rs` |
| 应用层端口与用例 | `crates/corvus-application/src/` |
| 守护进程装配 | `crates/corvusd/Cargo.toml`、`crates/corvusd/src/lib.rs` |
| REST 路由与认证 | `crates/corvus-api/src/` |
| 录制 schema 与迁移 | `crates/corvus-record/src/` |
| Rust 工作区版本 | `Cargo.toml` |
| 扩展版本与构建 | `extension/package.json`、`extension/manifest.json`、`extension/vite.*.config.ts` |
| 发布历史与限制 | `CHANGELOG.md`、`docs/release-notes/` |
| 1.4.0 验收证据 | `tests/acceptance/v1.4.0.md` |

## 3. 仍然适用的限制

- NFR-3 未通过：72 小时浸泡在约 8 小时处因客户要求停止。见 [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md)。
- 不支持 Firefox；见 [06-firefox-assessment.md](06-firefox-assessment.md)。
- CDP 客户端只接受回环端点，且没有认证。
- Docker 与 Kubernetes 镜像不带浏览器。
- 强制走 CDP 时没有回退到扩展。
- macOS 没有安装包。
- 扩展的 tab id 与 CDP 的 tab id 仍是两套命名空间。
- 1.4.0 的认证账号与刷新令牌保存在内存中，重启后失效；系统没有默认账号。
- REST 认证不覆盖 `/mcp`、扩展 WebSocket 与 Swagger，非回环监听仍需可信认证代理。
- REST 的 `session_id` 字段可接受，但尚未用于标注录制。

## 4. 文档置信度

| 领域 | 文档 | 置信度 |
|---|---|---|
| 需求 | [01-requirements.md](01-requirements.md) | 1.4.0 演进链可信；标题仍是 v0.1，因为基线从未改名 |
| 架构 | [02-architecture.md](02-architecture.md) | 1.4.0 发布面可信；端口、REST、认证与持久化章节与代码一致 |
| 工具面 | [09-tools.md](09-tools.md) | 高；仍为 43 个工具，与注册表一致 |
| 路线图 | [03-roadmap.md](03-roadmap.md) | 中；1.4.0 已交付，下一版为 1.5.0，NFR-3 仍未关闭 |
| 发布证据 | `tests/acceptance/v1.4.0.md` | 高；HEAD 65fbf52 上的 1.4.0 验收运行 |
| 归档 | `tests/archive/` | 低；仅供追溯，永不代表现状 |

## 5. 已被取代的材料

M1 至 M4 的验收手册与报告模板描述的是 JavaScript 扩展与 18 至 26 个工具的旧表面，不再代表现状，已移入 `tests/archive/`。0.1.0 到 1.4.0 的逐版本验证报告仍在 `tests/report/` 与 `tests/acceptance/`；1.4.0 的报告是当前证据，更早的仅作历史。当前验证使用 [v1.4.0.md](../tests/acceptance/v1.4.0.md)、`tests/conformance/run.sh` 与 `tests/perf/`；1.4.0 验收已完成。

## 6. 复核方式

- `cargo tree -p corvusd --depth 1` 显示守护进程的直接依赖。
- `pnpm --dir extension build` 重新构建可加载的扩展。
- `git log -1 --oneline` 显示本页读取时的提交。
