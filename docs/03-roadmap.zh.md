# Corvus 路线图

[English](03-roadmap.md) | [中文](03-roadmap.zh.md)

## 里程碑

| 阶段 | 目标 | 关键交付 | 出口标准 |
|---|---|---|---|
| M0 立项 | 把「做什么」写清楚 | 立项书、需求规格、架构、路线图、空仓库与 CI | 四份文档评审通过 |
| M1 骨架 | 打通一条真实链路 | corvusd + 扩展 + MCP(stdio/http) + navigate/snapshot/click/type | 在 DSH 里完成一个真实网页任务 |
| M2 全量 | 把能力铺满 | 60+ 工具、多浏览器、多标签并发、网络拦截、上传下载 | 需求规格第 6 节 10 个验收任务全过 |
| M3 观测 | 让一切可复盘 | 录制、trace、回放、站点记忆、Prometheus | 任意历史会话可逐步回放 |
| M4 生态 | 让别人用得起来 | 安装器、客户端配置生成、文档站、示例库、发布流水线 | 陌生用户 5 分钟跑通快速开始 |
| M6 混合 CDP | 浏览器暴露回环 CDP 端点时可直接挂接 | CDP 客户端、扩展探测、通道路由、十个 CDP 支撑工具 | 42 个工具，auto/cdp/extension 可切换，原有 32 个无回归 |
| M7 Token 效率 | 让工具面在长会话中足够便宜 | 紧凑页面快照、精简简版 schema、browser_batch、完整 CDP 对等 | 43 个工具，简版 tools/list 低于 30 KB，紧凑快照有实测 |

## v1.1.0 部署里程碑

v1.1.0 是 M4 的部署切片，32 个工具面保持不变。

| 项目 | 出口标准 |
|---|---|
| 单根目录 | `--home <dir>` 或 `CORVUS_HOME` 统一存放二进制、数据库、日志、配置与扩展；`doctor` 报告当前数据库路径，`--db` 仍可覆盖。 |
| Windows | 安装器默认使用 `%ProgramData%\Corvus`，迁移 1.0.0 的 `%USERPROFILE%\.corvus` 目录，并让服务只依赖一个根目录。 |
| Linux | x86_64 tar 包包含 `install-linux.sh`、systemd unit、升级/卸载脚本与 `SHA256SUMS`。 |
| 容器 | amd64 Docker 镜像与 Kubernetes 清单挂载单一 `/data` 根目录；CI 构建并校验。 |
| 发布 | 1.1.0 二进制、变更日志与双语发布说明。NFR-3 在 72 小时压测报告附上之前保持待定；Firefox 仍不受支持。 |

## v1.2.0 混合传输里程碑

v1.2.0 是工具面与传输切片。它新增十个 CDP 支撑工具，MCP 工具面从 32 增至 42，原有结果形状不变。

| 项目 | 出口标准 |
|---|---|
| 探测 | 扩展探测回环 9222、9223 与 9333，拒绝家族不匹配，只在 descriptor 有效时声明 cdp 能力。 |
| 直连客户端 | 守护进程持有一条浏览器级 WebSocket，使用 Target 会话，保留 1000 条事件环，以 250 毫秒至 10 秒退避重连，每 15 秒心跳。 |
| 通道 | auto、cdp 与 extension 可在启动时与运行时选择；auto 仅在 Unsupported 时回退，cdp 不回退，extension 忽略 CDP。 |
| 工具面 | 共 42 个工具；简版 tools/list 为 29,971 字节，完整详细为 35,552 字节；原有 32 个工具的参数与结果不变。 |
| 发布 | 1.2.0 二进制、变更日志与双语发布说明。CI 与 QA 证据只在运行存在时附上；NFR-3 仍待定，Firefox 仍不受支持。 |

本里程碑的实现与 CI 检查在发布标签前运行。72 小时压测仍未通过，Firefox 的 NFR-6 偏差保持不变。

## v1.3.0 Token 效率里程碑

v1.3.0 是上下文效率与对等性切片。它新增 `browser_batch`，MCP 工具面从 42 增至 43，并让默认发现与快照输出更小。

| 项目 | 出口标准 |
|---|---|
| 紧凑快照 | `page_snapshot` 在两个通道上都默认紧凑文本；`format: "json"` 保留节点对象；`viewport_only` 与 `include_bbox` 负责过滤与标注。 |
| 精简发现 | 简版 `tools/list` 移除属性描述与标题；`detail: "full"` 恢复它们；`tool_help` 返回单个完整条目。 |
| 批量 | `browser_batch` 按允许列表运行最多 20 个浏览器步骤，支持 `stop_on_error`；由守护进程本地处理。 |
| CDP 对等 | 强制 CDP 提供全部 33 个浏览器类工具，包括此前仅扩展的八个工具与 `@c` 引用。 |
| 工具面 | 43 个工具、9 个类别；原有 32 个结果形状保留；简版 tools/list 为 20,452 字节，完整详细为 37,067 字节。 |
| 发布 | 1.3.0 二进制、变更日志与双语发布说明。QA 实机验收、CI 运行与资产哈希只在可用时附上；NFR-3 仍待定，Firefox 仍不受支持。 |

紧凑与 JSON 的基准记录候选均值：364.83 字节 / 91.21 tokens，对比 JSON 的 2,077.33 字节 / 519.33 tokens，缩小 82.44%。实测资产行在 CI 发布前保持待定标记。

## v1.4.0 结构里程碑

v1.4.0 把后端拆成端口与适配器 crate，在 MCP 之外增加 REST 面，并把扩展重写为 TypeScript 与 Vue 3。它不新增 MCP 工具：43 工具面及其紧凑信封保持 1.3.0 已发布状态。

| 项目 | 退出标准 |
|---|---|
| 端口与适配器 | `corvus-domain` 为纯领域层；`corvus-application` 承载用例、`BrowserRegistry`、`BrowserSession`、`StepRecorder`、`ToolCatalog` 四个端口，以及原有 32 条路由器规格；`corvus-cdp`、`corvus-transport`、`corvus-record`、`corvus-mcp`、`corvus-api` 为适配器；`corvusd` 是装配根；`corvus-runtime` 已移除。 |
| REST 与认证 | `/api/v1` 提供 health、browsers、sessions、tools 与工具调用，OpenAPI 位于 `/api-docs/openapi.json`，Swagger 位于 `/docs`。认证默认 `off`；`local` 启用 Argon2id 与 HS256 JWT；绑定非回环地址必须启用 `local`。账号与刷新令牌保存在内存中。 |
| 持久化 | SeaORM 与 `sea-orm-migration` 管理 schema 版本 1；既有 1.3.0 数据库无需数据迁移即可打开，迁移前的 `VACUUM INTO` 备份提供保护；适配器保持 WAL、单写入者、5 秒 busy timeout、30 天保留与 262144 字节截断。 |
| 扩展重写 | `extension/src` 下的 TypeScript 与 Vue 3 源码，使用 Pinia 与 Naive UI；三次 Vite 构建产出 `extension/build/extension`；21 个单元、契约与组件测试加隔离 Playwright 端到端套件覆盖构建。 |
| 版本对齐 | 发布打标签前，cargo 工作区、扩展 package 与 manifest 统一为 1.4.0。 |
| 发布 | 1.4.0 二进制、变更日志与双语发布说明。QA 验收记录在 `tests/acceptance/v1.4.0.md`；最终 CI 运行 43690906863 与已发布资产哈希已记录在发布说明中；NFR-3 与 Firefox 偏差仍未关闭。 |

现状：1.4.0 已交付，1.5.0 已于 2026-11-15 交付。1.4.0 发布时 cargo 工作区与扩展均为 1.4.0，[v1.4.0.md](../tests/acceptance/v1.4.0.md) 记录了 199 个 workspace 测试（25 个套件）、135 项一致性检查与扩展门禁的 PASS。NFR-3 仍未通过，不宣称已有 1.4.0 实机部署，最终 CI 运行 43690906863 与已发布资产哈希已记录在发布说明中。当前版本为 1.5.0。

## v1.5.0 已交付（2026-11-15）

1.5.0 于 2026-11-15 由附注标签 `v1.5.0` 发布；标签对象 `a6ddc0f3e7b37f9cc29b0ff8c92eff839e05415f` 指向 `7d0e7c7126ea927a0d5f90df572c5aee31a9a4df`（产品提交 `d4d01eab7ef87cd9c73ec2f7aef8d633eda3a49a`，元数据修复 `7d0e7c7`）。它新增快照感知 v2、紧凑快照差异与扩展国际化，不新增 MCP 工具；43 工具面与九个类别保持不变。[v1.5.0.md](../tests/acceptance/v1.5.0.md) 记录了冻结源树 HEAD `73092ea` 上的 QA PASS；主 CI 运行 `43702442120` 通过八个任务，GitHub 发布 `445967405` 发布了产物。

| 项目 | 已交付 | 证据 |
|---|---|---|
| 感知 v2 | `page_snapshot` 遍历开放 shadow root 与同源子 frame，增加 frame 路径与 `frames` 元数据，跳过跨源 frame，全局预算 `max_nodes`，并把包围盒投影到顶层视口。 | `corvus-cdp` 与扩展运行时代码；QA PASS。 |
| 紧凑差异 | `diff` 仅用于紧凑输出，由应用层按浏览器、会话与标签页为键的 64 项缓存计算一次；前缀与基线重置见 [09-tools.zh.md](09-tools.zh.md)。 | `snapshot_diff.rs` 中的代码；`browser_batch` 共用该路径。 |
| 扩展国际化 | vue-i18n、英文与 zh-CN 目录、检测与持久化、响应式切换，以及 manifest `_locales`。 | 已实测 14 个构建文件与 8 个文件中的 41 个 Vitest 测试。 |
| 表面 | 43 个 MCP 工具、九个类别；无新增工具。 | 目录未变。 |
| 版本对齐 | cargo 工作区、扩展 package 与 manifest 均为 1.5.0。 | 发布时对齐；元数据修复 `7d0e7c7`。 |
| QA 验收 | 220 个 workspace 测试与 136 项一致性检查通过；43 工具快照 23,561 B。 | 冻结 HEAD `73092ea` 上 PASS；安装包 PASS 98/0。 |
| 发布 | 标签 `v1.5.0`、CI `43702442120`、GitHub 发布 `445967405`；Windows ZIP 18,534,237 B，Linux tar 包 19,849,047 B。 | 已发布产物已验证；合并 `SHA256SUMS` 194 B。 |

发布实测：25 条结果行上 220 个 workspace 测试通过、136 项一致性检查通过、扩展构建 14 个文件、8 个文件中的 41 个 Vitest 测试、隔离 Playwright 1/1；Windows 安装包验收在独立的本地 ZIP 上 98 项检查与观察、0 失败（18,516,546 B，SHA-256 `2a36e28e42385d473edf058e8ed047a20ebf3cc4638f48fd3b469f80c29e5f5f`）。已发布产物是 CI 重建产物：Windows 18,534,237 B SHA-256 `e91e2ebce31d99d8753004849777e37277a93acd493cd320edfbda3987baabad`，Linux 19,849,047 B SHA-256 `c1a62be75c2895cd9eef27848b4a304551e2177bd66b9e1730d6150b9657c136`，合并 `SHA256SUMS` 194 B SHA-256 `845aae4ee442020a34de4d90661c8cb2e6f83377f226d7a9e6263bdb354f9fbf`。QA 与安装包验收在发布提交前、冻结树上测得。NFR-3 仍未通过，不宣称任何 1.5.0 实机部署，也不宣称任何 Docker 镜像仓库发布或 Kubernetes 运行时测试。

## v1.6.0 已进入工作树（QA PASS；未发布）

1.6.0 在 `crates/corvusd/src/guardrails.rs` 新增可选守护策略：只读模式、工具允许与拒绝清单、主机与私有网络导航门禁、cookie 脱敏以及不可信内容边界。执行发生在派发前：`RouterBackend::call`、每个 `browser_batch` 步骤与 `session_replay`。拒绝使用 `POLICY_DENIED`，REST 返回 403。43 工具面与九个类别保持不变。HEAD `61ca85f` 的工作树已取得正式 QA 验收 PASS：25 条结果行上的 237 个 workspace 测试、137 项一致性检查、10 个文件中的 55 个 Vitest 测试与 Playwright 1/1；记录一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。1.6.0 尚无标签、CI 运行、已发布安装包或 GitHub 发布。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败（18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`）；它不是已发布产物。

| 项目 | 已进入工作树 | 证据 |
|---|---|---|
| 守护策略 | 严格 version 1 JSON、65,536 字节上限、CLI `--guardrails` 与 `CORVUS_GUARDRAILS`；非法策略中止启动。 | `crates/corvusd/src/guardrails.rs` 及其单元测试；QA PASS。 |
| 执行 | 派发前检查工具与 URL；批次与回放受同一策略约束；`POLICY_DENIED` 为第 11 个错误码，REST 返回 403。 | `crates/corvusd/src/lib.rs`；237 个 workspace 测试、0 失败。 |
| 脱敏与边界 | cookie 值脱敏；紧凑快照、页面文本与 HTML 用不可信内容标记包裹；密码输入掩码。 | `corvusd` 与 `corvus-cdp` 中的代码；QA PASS，一项 `browser_type` Low 发现。 |
| 扩展体验 | 本地化的 `POLICY_DENIED` 消息，带脱敏原因。 | 扩展代码与单元测试；10 个文件中的 55 个 Vitest 测试、14 个构建文件、Playwright 1/1。 |
| 表面与版本 | 43 个 MCP 工具、九个类别；源码元数据报告 1.6.0。 | 目录未变；源码元数据为 1.6.0；发布元数据待补。 |

QA 证据：[v1.6.0.zh.md](../tests/acceptance/v1.6.0.zh.md) 记录 25 条结果行上的 237 个 workspace 测试、137 项一致性检查、10 个文件中的 55 个 Vitest 测试、14 个文件/490,393 B 的扩展构建与 Playwright 1/1 通过；报告记录一项 `browser_type` 按通道的 Low 发现，无 High 或 Medium 发现。发布证据仍为 Pending：不存在 1.6.0 标签、CI 运行、已发布安装包、已发布的 Windows 或 Linux 产物、合并校验和或 GitHub 发布。独立的本地安装包 ZIP 已通过本地验收：84 项检查与观察、0 失败（18,801,258 B，SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`）；它不是已发布产物。NFR-3 仍未通过，不宣称任何 1.6.0 实机部署。

**v1.7.0 下一步（尚未定义范围）。** 未设定范围或日期。

## 建议排期（以 1 名主力 + AI 结对估算）

| 周 | 内容 |
|---|---|
| W1 | workspace 骨架、MCP 双栈、扩展最小连接、navigate/snapshot |
| W2 | click/type/press/scroll、错误码与超时体系、M1 验收 |
| W3-W4 | 感知全家桶、动作全家桶、标签并发 |
| W5-W6 | 网络拦截、cookie/storage、下载上传、多浏览器 |
| W7-W8 | 录制、回放、站点记忆、指标、M2 验收 |
| W9-W10 | 安装器、客户端配置、文档站、M3/M4 |

## 质量门禁

- 每个 PR：cargo fmt、clippy -D warnings、cargo test 全绿、三平台 CI。
- 每个工具：至少一个集成测试 + 一个真实页面冒烟测试。
- 每周：72h 短跑一次稳定性回归（可并行在后台）。
- 每次发布：版本号、变更日志、二进制签名、校验和。

## 风险缓冲

- MV3 生命周期问题优先在 M1 解决，不要拖到 M2。
- debugger 通道与纯 DOM 通道必须实现为可切换的两条路径，避免单点依赖。
- Firefox 兼容性单独排期，不与 Chromium 主线混做。