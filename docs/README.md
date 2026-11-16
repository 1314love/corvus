# Documentation index / 文档索引

[English](README.md) | [中文](#中文)

Every document is maintained in two editions, English and Chinese, in the same folder, updated in the same
commit. 每份文档在同一目录下维护英文与中文两版，同一提交同步更新。

| Document | English | 中文 | Status |
|---|---|---|---|
| Language policy / 语言政策 | [LANGUAGE.md](LANGUAGE.md) | same file (bilingual) | done |
| Charter / 立项书 | [00-charter.md](00-charter.md) | [00-charter.zh.md](00-charter.zh.md) | done |
| Requirements / 需求规格 | [01-requirements.md](01-requirements.md) | [01-requirements.zh.md](01-requirements.zh.md) | done |
| Architecture / 架构 | [02-architecture.md](02-architecture.md) | [02-architecture.zh.md](02-architecture.zh.md) | done; includes the unreleased v2.0 P2 milestone |
| Roadmap / 路线图 | [03-roadmap.md](03-roadmap.md) | [03-roadmap.zh.md](03-roadmap.zh.md) | done; includes the unreleased v2.0 P2 milestone |
| Install / 安装 | [04-install.md](04-install.md) | [04-install.zh.md](04-install.zh.md) | done |
| Deployment / 部署 | [05-deployment.md](05-deployment.md) | [05-deployment.zh.md](05-deployment.zh.md) | done |
| Firefox assessment / Firefox 可行性评估 | [06-firefox-assessment.md](06-firefox-assessment.md) | [06-firefox-assessment.zh.md](06-firefox-assessment.zh.md) | done; decision current, code and tool baseline refreshed to 1.7.0 on 2026-11-16 |
| CDP guide / CDP 指南 | [07-cdp.md](07-cdp.md) | [07-cdp.zh.md](07-cdp.zh.md) | done |
| Logging convention / 日志规范 | [08-logging.md](08-logging.md) | [08-logging.zh.md](08-logging.zh.md) | done |
| Tool reference / 工具参考 | [09-tools.md](09-tools.md) | [09-tools.zh.md](09-tools.zh.md) | done; 1.7.0 catalogue plus the unreleased v2.0 P2 surface |
| Project state / 项目现状 | [10-project-state.md](10-project-state.md) | [10-project-state.zh.md](10-project-state.zh.md) | done; includes the unreleased v2.0 P2 milestone |
| Guardrails policy / 守护策略 | [11-guardrails.md](11-guardrails.md) | [11-guardrails.zh.md](11-guardrails.zh.md) | done; v1.7.0 released 2026-11-16; 1.6.0 previous |
| Release notes 1.7.0 / 发布说明 1.7.0 | [release-notes/v1.7.0.md](release-notes/v1.7.0.md) | [release-notes/v1.7.0.zh.md](release-notes/v1.7.0.zh.md) | done; released 2026-11-16; CI and published assets recorded |
| Release notes 1.6.0 / 发布说明 1.6.0 | [release-notes/v1.6.0.md](release-notes/v1.6.0.md) | [release-notes/v1.6.0.zh.md](release-notes/v1.6.0.zh.md) | done; CI and published assets recorded |
| Release notes 1.5.0 / 发布说明 1.5.0 | [release-notes/v1.5.0.md](release-notes/v1.5.0.md) | [release-notes/v1.5.0.zh.md](release-notes/v1.5.0.zh.md) | done; previous release |
| Release notes 1.4.0 / 发布说明 1.4.0 | [release-notes/v1.4.0.md](release-notes/v1.4.0.md) | [release-notes/v1.4.0.zh.md](release-notes/v1.4.0.zh.md) | done; CI and published assets recorded |
| Release notes 1.3.0 / 发布说明 1.3.0 | [release-notes/v1.3.0.md](release-notes/v1.3.0.md) | [release-notes/v1.3.0.zh.md](release-notes/v1.3.0.zh.md) | historical; superseded by 1.4.0 |
| Release notes 1.2.0 / 发布说明 1.2.0 | [release-notes/v1.2.0.md](release-notes/v1.2.0.md) | [release-notes/v1.2.0.zh.md](release-notes/v1.2.0.zh.md) | historical |
| Release notes 1.1.0 / 发布说明 1.1.0 | [release-notes/v1.1.0.md](release-notes/v1.1.0.md) | [release-notes/v1.1.0.zh.md](release-notes/v1.1.0.zh.md) | historical |
| Release notes 1.0.0 / 发布说明 1.0.0 | [release-notes/v1.0.0.md](release-notes/v1.0.0.md) | [release-notes/v1.0.0.zh.md](release-notes/v1.0.0.zh.md) | historical |
| Release notes 0.1.0 / 发布说明 0.1.0 | [release-notes/v0.1.0.md](release-notes/v0.1.0.md) | [release-notes/v0.1.0.zh.md](release-notes/v0.1.0.zh.md) | historical |
| Process / 交付流程 | [PROCESS.md](PROCESS.md) | [PROCESS.zh.md](PROCESS.zh.md) | done |

Reading order: charter -> requirements -> architecture -> roadmap. 阅读顺序：立项书 → 需求规格 → 架构 → 路线图。

Current release: 1.7.0, released on 2026-11-16 from annotated tag `v1.7.0` at commit `f12930c`; 1.6.0 is the previous release. Acceptance evidence: [tests/acceptance/v1.7.0.md](../tests/acceptance/v1.7.0.md); local Windows packaging passed 160/160 checks; main CI run 43795994581 passed eight jobs, release workflow 43796443828 succeeded and GitHub Release 446568768 published the Windows ZIP, Linux tarball and combined `SHA256SUMS`. The release has 62 tools in ten categories; the schema snapshot is complete at 62 entries (36,993 B, 1,853 lines), and conformance passed 137 checks with 0 failed and 0 skipped. QA acceptance passed with 263 Rust tests, 137 conformance checks, 91 Vitest tests in 13 files, 3 Playwright E2E tests and a 14-file/537,844-byte build. 当前版本：1.7.0，于 2026-11-16 由附注标签 `v1.7.0` 在提交 `f12930c` 发布；1.6.0 为上一版本。验收证据见 [tests/acceptance/v1.7.0.md](../tests/acceptance/v1.7.0.md)；本地 Windows 打包通过 160/160 项检查；主 CI 运行 43795994581 通过八个任务，发布工作流 43796443828 成功，GitHub 发布 446568768 发布了 Windows ZIP、Linux tar 包与合并 `SHA256SUMS`。该发布为十类 62 个工具；schema 快照已完成为 62 条目（36,993 B、1,853 行），一致性检查通过 137 项、0 失败、0 跳过。QA 验收通过：263 个 Rust 测试、137 项一致性检查、13 个文件中的 91 个 Vitest 测试、3 个 Playwright 端到端测试，以及 14 个文件/537,844 B 的构建。

Internal work: v2.0 P2 is an unreleased internal milestone on branch `v2.0`. It exposes ten tools through a Rust gateway and encrypted provider actions, has QA PASS (303/303 checks), and is not tagged, released or deployed; 1.7.0 remains the current published release. 内部工作：v2.0 P2 是 `v2.0` 分支上的未发布内部里程碑。它通过 Rust 网关与加密 provider 动作暴露十个工具，QA 通过（303/303 项检查），未打标签、未发布、未部署；1.7.0 仍是当前已发布版本。

Document confidence was reviewed on 2026-11-16; the graded table is in [10-project-state.md](10-project-state.md#7-document-confidence). Superseded playbooks and reports live in [../tests/archive/](../tests/archive/README.md); they are kept for traceability and are not reading material. 文档置信度已于 2026-11-16 审查，分级表见 [10-project-state.zh.md](10-project-state.zh.md#7-文档置信度)。已被取代的手册与报告放在 [../tests/archive/](../tests/archive/README.md)，只作追溯，不属于阅读材料。

## 中文

每份文档都有英文与中文两个版本，同一目录、同一提交。文件名规则：英文版 `<name>.md`，中文版 `<name>.zh.md`，
两版首行互相链接。缺一版视为未完成：要么两版一起交，要么把缺口登记到本表状态列标 `pending`，并在下一个
里程碑评审前补齐。本表文档均已按上述规则成对；`tests/archive/` 存放已被取代的材料，不代表现状。

维护要求见 [LANGUAGE.md](LANGUAGE.md) 与 [../CONTRIBUTING.md](../CONTRIBUTING.md) 第 4 节。
