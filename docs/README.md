# Documentation index / 文档索引

[English](README.md) | [中文](#中文)

Every document is maintained in two editions, English and Chinese, in the same folder, updated in the same
commit. 每份文档在同一目录下维护英文与中文两版，同一提交同步更新。

| Document | English | 中文 | Status |
|---|---|---|---|
| Language policy / 语言政策 | [LANGUAGE.md](LANGUAGE.md) | same file (bilingual) | done |
| Charter / 立项书 | [00-charter.md](00-charter.md) | [00-charter.zh.md](00-charter.zh.md) | done |
| Requirements / 需求规格 | [01-requirements.md](01-requirements.md) | [01-requirements.zh.md](01-requirements.zh.md) | done |
| Architecture / 架构 | [02-architecture.md](02-architecture.md) | [02-architecture.zh.md](02-architecture.zh.md) | done |
| Roadmap / 路线图 | [03-roadmap.md](03-roadmap.md) | [03-roadmap.zh.md](03-roadmap.zh.md) | done |
| Install / 安装 | [04-install.md](04-install.md) | [04-install.zh.md](04-install.zh.md) | done |
| Deployment / 部署 | [05-deployment.md](05-deployment.md) | [05-deployment.zh.md](05-deployment.zh.md) | done |
| Firefox assessment / Firefox 可行性评估 | [06-firefox-assessment.md](06-firefox-assessment.md) | [06-firefox-assessment.zh.md](06-firefox-assessment.zh.md) | done |
| CDP guide / CDP 指南 | [07-cdp.md](07-cdp.md) | [07-cdp.zh.md](07-cdp.zh.md) | done |
| Logging convention / 日志规范 | [08-logging.md](08-logging.md) | [08-logging.zh.md](08-logging.zh.md) | done |
| Tool reference / 工具参考 | [09-tools.md](09-tools.md) | [09-tools.zh.md](09-tools.zh.md) | done |
| Project state / 项目现状 | [10-project-state.md](10-project-state.md) | [10-project-state.zh.md](10-project-state.zh.md) | done |
| Release notes 1.5.0 / 发布说明 1.5.0 (unreleased / 未发布) | [release-notes/v1.5.0.md](release-notes/v1.5.0.md) | [release-notes/v1.5.0.zh.md](release-notes/v1.5.0.zh.md) | QA and installer PASS; CI and published-asset evidence Pending |
| Release notes 1.4.0 / 发布说明 1.4.0 | [release-notes/v1.4.0.md](release-notes/v1.4.0.md) | [release-notes/v1.4.0.zh.md](release-notes/v1.4.0.zh.md) | done; CI and published assets recorded |
| Release notes 1.3.0 / 发布说明 1.3.0 | [release-notes/v1.3.0.md](release-notes/v1.3.0.md) | [release-notes/v1.3.0.zh.md](release-notes/v1.3.0.zh.md) | done |
| Release notes 1.2.0 / 发布说明 1.2.0 | [release-notes/v1.2.0.md](release-notes/v1.2.0.md) | [release-notes/v1.2.0.zh.md](release-notes/v1.2.0.zh.md) | done |
| Release notes 1.1.0 / 发布说明 1.1.0 | [release-notes/v1.1.0.md](release-notes/v1.1.0.md) | [release-notes/v1.1.0.zh.md](release-notes/v1.1.0.zh.md) | done |
| Release notes 1.0.0 / 发布说明 1.0.0 | [release-notes/v1.0.0.md](release-notes/v1.0.0.md) | [release-notes/v1.0.0.zh.md](release-notes/v1.0.0.zh.md) | done |
| Release notes 0.1.0 / 发布说明 0.1.0 | [release-notes/v0.1.0.md](release-notes/v0.1.0.md) | [release-notes/v0.1.0.zh.md](release-notes/v0.1.0.zh.md) | done |
| Process / 交付流程 | [PROCESS.md](PROCESS.md) | [PROCESS.zh.md](PROCESS.zh.md) | done |

Reading order: charter -> requirements -> architecture -> roadmap. 阅读顺序：立项书 → 需求规格 → 架构 → 路线图。

Current release: 1.4.0. Acceptance evidence: [tests/acceptance/v1.4.0.md](../tests/acceptance/v1.4.0.md); CI and published-asset values are recorded in the release notes. 当前版本：1.4.0。验收证据见 [tests/acceptance/v1.4.0.md](../tests/acceptance/v1.4.0.md)；CI 与发布产物数值已记录在发布说明中。The tree also contains unreleased 1.5.0 work for frame and shadow perception, compact snapshot diffs and extension i18n; its QA acceptance PASS is recorded in tests/acceptance/v1.5.0.md and Windows installer acceptance passed 98 checks on the local ZIP; CI and published-asset evidence remains Pending. 工作树还包含未发布的 1.5.0 工作：frame 与 shadow 感知、紧凑快照差异与扩展 i18n；其 QA 验收 PASS 已记录于 tests/acceptance/v1.5.0.md，Windows 安装包验收在本地 ZIP 上通过 98 项检查；CI 与已发布产物证据仍为 Pending。

Superseded playbooks and reports live in [../tests/archive/](../tests/archive/README.md); they are kept for traceability and are not reading material. 已被取代的手册与报告放在 [../tests/archive/](../tests/archive/README.md)，只作追溯，不属于阅读材料。

## 中文

每份文档都有英文与中文两个版本，同一目录、同一提交。文件名规则：英文版 `<name>.md`，中文版 `<name>.zh.md`，
两版首行互相链接。缺一版视为未完成：要么两版一起交，要么把缺口登记到本表状态列标 `pending`，并在下一个
里程碑评审前补齐。本表文档均已按上述规则成对；`tests/archive/` 存放已被取代的材料，不代表现状。

维护要求见 [LANGUAGE.md](LANGUAGE.md) 与 [../CONTRIBUTING.md](../CONTRIBUTING.md) 第 4 节。
