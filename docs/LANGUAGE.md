# Language policy / 语言政策

Corvus is developed bilingually: **every document exists in an English and a Chinese edition**, and both are
maintained at the same time. Corvus 采用双语开发：**每一份文档都有英文版与中文版**，两版同步维护。

## 1. Which language is canonical / 以哪种语言为准

| Artifact | Canonical | Counterpart |
|---|---|---|
| README, docs | English | Chinese |
| Code, comments, commit messages, logs, error strings | English | none required |
| User-facing release notes | English | Chinese |
| CLI `--help` and user-facing messages | English | Chinese (via locale, later) |
| Reader-facing product documents (this file, docs/) | English | Chinese |

Rationale: English removes ambiguity for contributors and for the MCP ecosystem; Chinese keeps the product
readable for the maintainer and for the primary users. 英文消除实现歧义，中文降低阅读成本，两者都要。

## 2. File naming / 文件命名

- English edition: `<name>.md` (`docs/00-charter.md`).
- Chinese edition: `<name>.zh.md` (`docs/00-charter.zh.md`).
- Both files carry `[English](...) | [中文](...)` switch links on line 1.
- A new document ships both editions in the same commit. 新文档必须在同一提交里带上两个版本。

## 3. Status of existing documents / 现有文档状态

Tracked in [docs/README.md](README.md). A document whose counterpart is missing is **not done**: it may be
merged, but the index must show it as `pending`, and the gap must be closed before the next milestone gate.
翻译状态在 [docs/README.md](README.md) 追踪；缺一版的文档视为未完成，索引里必须标 `pending`，且在下一个
里程碑评审前补齐。

## 4. Review rule / 评审规则

- A PR that changes content must change both editions, or state why the other edition is unaffected.
- Terminology is fixed by the glossary in [docs/02-architecture.md](02-architecture.md) and must be translated
  consistently: *tool* 工具, *snapshot* 快照, *session* 会话, *step* 步骤, *site card* 站点卡片,
  *connection* 连接, *channel* 通道, *approval* 审批.
- 修改内容的 PR 必须同时改两个版本，否则需说明另一版为何不受影响；术语按上表统一。
