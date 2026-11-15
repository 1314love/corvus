# Corvus 项目现状

[English](10-project-state.md) | [中文](10-project-state.zh.md)

置信度：下表事实均读自提交 `6b7efee` 及其未提交改动。本页与旧文档冲突时，以本页为准。读者：维护者与评审者。

## 1. 已发布与在研

已发布的 1.3.0 由 `corvus-proto`、`corvus-mcp`、`corvus-transport`、`corvus-runtime`、`corvus-record`、`corvus-cdp` 装配守护进程。`crates/corvus-mcp/src/tools.rs` 声明 `const SPECS: [Spec; 43]`；`tools/list` 精简版 20,452 字节，`detail: "full"` 为 37,067 字节，共九类。证据见 [v1.3.0-release-verification.md](../tests/report/v1.3.0-release-verification.md)、[v1.3.0-packaging.md](../tests/report/v1.3.0-packaging.md)、[v1.3.0-snapshot-benchmark.md](../tests/report/v1.3.0-snapshot-benchmark.md)。

未提交的工作树承载在研的 1.4.0：

- `crates/corvus-domain`、`crates/corvus-application`（`crates/corvus-application/src/spec.rs` 里 `pub const TOOLS: [ToolSpec; 32]`）与 `crates/corvus-api`（`/api/v1`、utoipa 生成 OpenAPI、JWT 与 argon2 认证）引入六边形分层与 REST 面。
- `extension/` 重写为 TypeScript 与 Vue 3，使用 Vite、pnpm、Vitest 与 Playwright；`extension/package.json` 与 `extension/manifest.json` 均为 1.4.0，原先的 `background.js`、`content.js`、`offscreen.js`、`popup.js`、`options.js`、`daemon_url.js` 已删除。

工作树中两侧已经接通：`crates/corvusd/Cargo.toml` 依赖 domain、application 与 api，`corvusd/src/lib.rs` 挂载 `corvus_api::router`，`crates/corvus-mcp` 依赖 application；`corvus-runtime` crate 已删除，其路由并入 `corvus-application`。缺的是发布：Rust 工作区仍为 1.3.0，扩展已是 1.4.0，且改动都未提交。

## 2. 事实来源

| 事实 | 读取位置 |
|---|---|
| MCP 面为 43 个工具 | `crates/corvus-mcp/src/tools.rs` |
| 浏览器侧应用规格为 32 个工具 | `crates/corvus-application/src/spec.rs` |
| 守护进程装配 | `crates/corvusd/Cargo.toml` |
| Rust 工作区版本 | `Cargo.toml` |
| 扩展版本 | `extension/package.json`、`extension/manifest.json` |
| 发布历史与限制 | `CHANGELOG.md`、`docs/release-notes/` |
| 1.3.0 实测证据 | `tests/report/v1.3.0-*.md` |

## 3. 仍然适用的限制

- NFR-3 未通过：72 小时浸泡在约 8 小时处因客户要求停止。见 [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md)。
- 不支持 Firefox；见 [06-firefox-assessment.md](06-firefox-assessment.md)。
- CDP 客户端只接受回环端点，且没有认证。
- Docker 与 Kubernetes 镜像不带浏览器。
- 强制走 CDP 时没有回退到扩展。
- macOS 没有安装包。
- 扩展的 tab id 与 CDP 的 tab id 仍是两套命名空间。

## 4. 文档置信度

| 领域 | 文档 | 置信度 |
|---|---|---|
| 需求 | [01-requirements.md](01-requirements.md) | 1.3.0 演进链可信；标题仍是 v0.1，因为基线从未改名 |
| 架构 | [02-architecture.md](02-architecture.md) | 已发布守护进程部分可信；crate 表与仓库结构已标注 1.4.0 为在研 |
| 工具面 | [09-tools.md](09-tools.md) | 高；与 43 工具注册表一致 |
| 路线图 | [03-roadmap.md](03-roadmap.md) | 中；表达意图，NFR-3 仍未关闭 |
| 发布证据 | `tests/report/v1.3.0-*.md` | 高；由 1.3.0 验证运行产出 |
| 归档 | `tests/archive/` | 低；仅供追溯，永不代表现状 |

## 5. 已被取代的材料

M1 至 M4 的验收手册、报告模板，以及 0.1.0 到 1.2.0 的发布报告，描述的是 JavaScript 扩展与 18 至 26 个工具的旧表面，不再代表现状，已移入 `tests/archive/`。当前验证使用 [v1.2.0.md](../tests/acceptance/v1.2.0.md)、[v1.3.0.md](../tests/acceptance/v1.3.0.md)、`tests/conformance/run.sh` 与 `tests/perf/`。

## 6. 复核方式

- `cargo tree -p corvusd --depth 1` 显示守护进程的直接依赖。
- `pnpm --dir extension build` 重新构建可加载的扩展。
- `git log -1 --oneline` 显示本页读取时的提交。
