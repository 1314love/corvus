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

## 5. Writing rules / 写作规则

Applies to documents, comments, commit messages and release notes. 适用于文档、注释、提交信息与发布说明。

- No chain of thought. State the decision, the result and the evidence; never the deliberation behind them. <!-- cot-gate: allow -->
  不写思考过程：只写结论、结果与证据。 <!-- cot-gate: allow -->
- Enforced mechanically, not by review alone: `node scripts/check-cot.mjs` scans every tracked file and the commits of a
  change, and CI fails on a hit. A line that quotes this rule ends with `cot-gate: allow`, which exempts it.
  由脚本强制执行：`node scripts/check-cot.mjs` 扫描全部仓库文件与本次变更的提交，命中即失败；引用本条规则的行以
  `cot-gate: allow` 结尾即豁免。
- One idea per sentence. Delete a sentence whose content the table above already states.
  一句一个意思；表格已说明的内容不再复述。
- Cut lead-ins and filler: "This section covers", "Note that", "simply", "basically",
  "值得注意的是", "需要说明的是", "事实上". 删掉开场白与填充词。
- Fewer stop words: "the daemon that the user runs" becomes "the user's daemon"; drop generic articles and
  connectives. 少用虚词：可省的冠词与连接词省掉。
- Comments explain a non-obvious mechanism or constraint. Never restate the identifier, never narrate the
  edit. 注释只解释不直观的机制或约束，不复述标识符，不叙述编辑过程。
- Code: `//!` for the module, `///` for the item, `//` for implementation notes. Public items carry docs and
  every crate root enables `missing_docs`. 代码：模块用 `//!`，条目用 `///`，实现说明用 `//`；公共条目
  不缺文档，每个 crate 根启用 `missing_docs`。
- A plain JavaScript file starts with `@fileoverview` and at most three lines of purpose.
  JS 文件头以 `@fileoverview` 起始，用途不超过三行。
- No milestone or sprint codes in comments (`M2b`, `v1.2:`); state the behavior and its limits.
  注释不写里程碑编号（`M2b`、`v1.2:`），直接写行为与边界。
- Doc blocks: summary line, blank `///`, then detail; keep `# Panics` where a public function can panic.
  文档块：摘要行、空 `///`、再写细节；可能 panic 的公共函数保留 `# Panics`。
- No em dashes for rhetorical pause; use a colon or a full stop. 不用破折号做语气停顿，改用冒号或句号。
- Commit messages: change only, no reasoning, no conversation. 提交信息只写变更，不写推理与对话。
