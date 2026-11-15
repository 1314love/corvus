# Corvus 项目现状

[English](10-project-state.md) | [中文](10-project-state.zh.md)

置信度：以下事实均读自仓库以及 1.5.0、1.4.0 与 1.6.0 验收报告。引用本页前请核对 `git log -1` 与发布页。1.5.0 是当前已发布版本，发布于 2026-11-15；1.4.0 为上一版本。v1.6.0 守护策略实现已进入工作树，并在 HEAD `61ca85f` 的工作树上通过正式 QA；不存在 1.6.0 发布，发布证据仍为 Pending。读者：维护者与评审者。

## 1. 当前版本：1.5.0

1.5.0 于 2026-11-15 由附注标签 `v1.5.0` 发布；标签对象 `a6ddc0f3e7b37f9cc29b0ff8c92eff839e05415f` 指向 `7d0e7c7126ea927a0d5f90df572c5aee31a9a4df`，产品提交为 `d4d01eab7ef87cd9c73ec2f7aef8d633eda3a49a`，元数据修复 `7d0e7c7` 仅对齐版本。GitHub 发布 `445967405` 发布了产物，主 CI 运行 `43702442120` 通过八个任务，[tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md) 记录冻结源树 HEAD `73092ea` 上的 QA PASS。1.4.0 为上一版本。

1.5.0 守护进程延续 1.4.0 的十 crate 端口与适配器后端：`corvus-domain`、`corvus-application`、`corvus-api`、`corvus-proto`、`corvus-mcp`、`corvus-transport`、`corvus-record`、`corvus-cdp`、`corvusd` 与 `corvus`；`corvus-runtime` 仍已移除。`crates/corvus-mcp/src/tools.rs` 声明 `const SPECS: [Spec; 43]`；1.5.0 QA 实测 43 个工具、九个类别，`tools/list` 简版正文 20,493 字节，`detail: "full"` 为 37,372 字节。发布证据见[发布说明](release-notes/v1.5.0.zh.md)与 [tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md)；[v1.4.0.md](../tests/acceptance/v1.4.0.md) 为上一基线。

1.5.0 发布内容包括：

- `crates/corvus-domain`、`crates/corvus-application` 与 `crates/corvus-api` 延续端口与适配器分层。application crate 定义 `BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，并在 `crates/corvus-application/src/spec.rs` 保留 `pub const TOOLS: [ToolSpec; 32]`。
- REST 面在 `/api/v1` 提供 health、browsers、sessions 与 tools，OpenAPI 位于 `/api-docs/openapi.json`，Swagger 位于 `/docs`。认证默认 `off`；`local` 启用 Argon2id 与 HS256 JWT；`corvusd` 挂载 `corvus_api::router`。
- `corvus-record` 使用 SeaORM 与 `sea-orm-migration`，schema 版本为 1；既有数据库在迁移前执行 `VACUUM INTO` 备份，并保持 WAL、单写入者与 5 秒 busy timeout。
- `extension/` 使用 TypeScript 与 Vue 3 编写，采用 Pinia、Naive UI、Vite、Vitest 与 Playwright；三次 Vite 构建产出 `extension/build/extension`。
- 已发布的 1.5.0 产物报告 1.5.0；当前工作树的 `Cargo.toml`、`extension/package.json` 与 `extension/manifest.json` 元数据报告 1.6.0（已通过 QA，尚未发布）。

MCP 面保持不变：43 个工具、九个类别、紧凑单文本信封与 `browser_batch`。[tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md) 记录了 25 条结果行上的 220 个 workspace 测试、136 项一致性检查、8 个文件中的 41 个 Vitest 测试与 Playwright 1/1 的 PASS。主 CI 运行 43702442120 与发布产物哈希已记录在 1.5.0 发布说明中。

## 2. 1.5.0 发布与证据

1.5.0 发布新增快照感知 v2、紧凑快照差异与扩展国际化；不新增 MCP 工具。

- **快照感知 v2。** `page_snapshot` 遍历开放 shadow root 与同源子 frame，增加文档顺序 frame 路径、带跳过原因的 `frames` 摘要、全局 `max_nodes` 预算与顶层视口包围盒。跨源 frame 的 DOM 永不读取。
- **紧凑差异。** `crates/corvus-application/src/snapshot_diff.rs` 维护按浏览器、会话与标签页为键的 64 项 LRU。应用层对紧凑输出只应用一次，因此扩展、CDP 直连与 `browser_batch` 路径共用同一语法；JSON 与 `diff` 同时使用返回类型化 `BAD_ARGS`。
- **扩展国际化。** 英文与 zh-CN 的 vue-i18n 目录、语言检测与 `corvus.locale` 持久化、响应式切换，以及供 MV3 名称与描述使用的 manifest `_locales`。
- **表面不变。** 43 个 MCP 工具、九个类别；1.5.0 不新增工具。

发布实测：14 个扩展构建文件、8 个文件中的 41 个 Vitest 测试、隔离 Playwright 1/1；Windows 安装包验收在独立的本地 ZIP 上 98 项检查与观察、0 失败（18,516,546 B，SHA-256 `2a36e28e42385d473edf058e8ed047a20ebf3cc4638f48fd3b469f80c29e5f5f`）。QA 记录 25 条结果行上的 220 个 workspace 测试、136 项一致性检查，以及 23,561 B 的 43 工具快照，SHA-256 `d761561f54112bf06305bde716ebb072d88afa0b57ea4a61d72105436471a0c7`。已发布产物是 CI 重建产物：Windows 18,534,237 B SHA-256 `e91e2ebce31d99d8753004849777e37277a93acd493cd320edfbda3987baabad`，Linux 19,849,047 B SHA-256 `c1a62be75c2895cd9eef27848b4a304551e2177bd66b9e1730d6150b9657c136`，合并 `SHA256SUMS` 194 B SHA-256 `845aae4ee442020a34de4d90661c8cb2e6f83377f226d7a9e6263bdb354f9fbf`。NFR-3 仍未通过，不宣称任何 1.5.0 实机部署。

## 3. v1.6.0 工作树内容（QA PASS；未发布）

工作树在 `crates/corvusd/src/guardrails.rs` 新增可选守护策略：只读模式、工具允许与拒绝清单、主机与私有网络导航门禁、cookie 脱敏与不可信内容边界。`RouterBackend::call`、每个 `browser_batch` 步骤与 `session_replay` 均执行该策略；拒绝使用 `POLICY_DENIED`，REST 返回 403。43 工具面与九个类别保持不变。源码元数据报告 1.6.0。正式 QA 验收报告 [tests/acceptance/v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 HEAD `61ca85f` 工作树 PASS：25 条结果行上的 237 个 workspace 测试、0 失败；137 项一致性检查、0 失败、0 跳过；10 个文件中的 55 个 Vitest 测试；14 个构建文件、490,393 B；Playwright 1/1。记录一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。不存在 1.6.0 标签、CI 运行、已发布安装包或 GitHub 发布。

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
| 1.6.0 QA 验收与发布证据 | `tests/acceptance/v1.6.0.md`、`docs/release-notes/v1.6.0.md`；尚无 1.6.0 标签、CI 运行、已发布安装包或 GitHub 发布 |

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
- 1.5.0 由标签 `v1.5.0` 发布；QA 与 Windows 安装包验收是在发布提交前、冻结源树上测得的，本地安装包 ZIP 与已发布的 CI 重建产物不同。不宣称任何 1.5.0 实机部署。
- 1.6.0 已在 HEAD `61ca85f` 的工作树上取得正式 QA PASS，但没有 1.6.0 标签、CI 运行、已发布安装包或已发布产物；独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败（18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`）；它不是已发布产物。1.5.0 仍是当前已发布版本。
- 快照差异只适用于紧凑输出；`format: "json"` 与 `diff` 同时使用返回类型化 `BAD_ARGS`。

## 6. 文档置信度

| 领域 | 文档 | 置信度 |
|---|---|---|
| 需求 | [01-requirements.md](01-requirements.md) | 1.4.0 演进链可信；标题仍是 v0.1，因为基线从未改名 |
| 架构 | [02-architecture.md](02-architecture.md) | 1.5.0 发布面可信；5.9 节覆盖已发布的 v1.5.0，5.10 节覆盖已通过 QA、发布证据待补的 v1.6.0 工作树内容 |
| 工具面 | [09-tools.md](09-tools.md) | 高；43 个工具，frame 与 diff 在 1.5.0 中为当前能力，并记录 1.6.0 守护边界 |
| 路线图 | [03-roadmap.md](03-roadmap.md) | 中；1.5.0 已发布，v1.6.0 已在工作树中通过 QA 但发布证据待补，NFR-3 仍未关闭 |
| 发布证据 | `tests/acceptance/v1.4.0.md`、`tests/acceptance/v1.5.0.md`、`tests/acceptance/v1.6.0.md` | 高；1.5.0 在 HEAD 73092ea 上 PASS 且已发布；1.4.0 在 HEAD 65fbf52 上 PASS；1.6.0 在 HEAD 61ca85f 上 PASS 但尚未发布 |
| 归档 | `tests/archive/` | 低；仅供追溯，永不代表现状 |

## 7. 已被取代的材料

M1 至 M4 的验收手册与报告模板描述的是 JavaScript 扩展与 18 至 26 个工具的旧表面，不再代表现状，已移入 `tests/archive/`。0.1.0 到 1.5.0 的逐版本验证报告仍在 `tests/report/` 与 `tests/acceptance/`；1.5.0 的报告是当前证据，[v1.4.0.md](../tests/acceptance/v1.4.0.md) 为上一基线。当前验证使用 [v1.6.0.md](../tests/acceptance/v1.6.0.md)、`tests/conformance/run.sh` 与 `tests/perf/`；1.5.0 是最新已发布版本。v1.6.0 守护策略已在 HEAD `61ca85f` 上取得 PASS 验收报告但尚未发布，因此 1.5.0 报告仍是最新已发布基线。

## 8. 复核方式

- `cargo tree -p corvusd --depth 1` 显示守护进程的直接依赖。
- `pnpm --dir extension build` 重新构建可加载的扩展。
- `git log -1 --oneline` 显示本页读取时的提交。
- `cargo test -p corvusd guardrails` 覆盖策略解析、优先级与门禁。