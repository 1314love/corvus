# Contributing / 贡献指南

[English](#english) | [中文](#中文版)

---

## English

Corvus is source-available under the PolyForm Noncommercial License 1.0.0 (see `LICENSE`). Any contribution is
accepted under the same license.

### 1. Commit messages

    <type>: <one-line conclusion>

    - key conclusion
    - affected scope
    - relevant data (version, threshold, error code)

- `type`: `feat` `fix` `perf` `refactor` `docs` `test` `build` `ci` `chore`.
- The subject states **the result**, never the process. No "tried A, it failed, switched to B".
- The body carries conclusions, constraints, blast radius and how it was verified. No derivation, no debug
  logs, no conversation transcripts.
- One commit, one change. Keep refactors and features in separate commits.
- Never commit local paths, tokens, proxy settings or account details.

### 2. Branches and merge

- `main` is always green; direct pushes are reserved for maintainer release commits.
- Feature branches: `feat/<topic>`, `fix/<topic>`; one branch, one topic.
- Merge with squash so every commit on `main` is a complete conclusion.

### 3. Code

- Format with `cargo fmt --all`; CI enforces `--check`.
- Static analysis: `cargo clippy --workspace --all-targets -- -D warnings`, zero warnings.
- Tests: `cargo test --workspace`. New public behaviour needs tests; protocol changes need round-trip tests.
- Dependencies: justify each new dependency and the alternative in the PR. Prefer the standard library.
- Errors: `thiserror` in libraries, `anyhow` at process entry points; never swallow errors, never
  `unwrap` external input.
- Async: `tokio` everywhere; blocking or CPU-bound work must not occupy reactor threads.
- Logging: structured `tracing` fields; never log secrets, tokens, cookies or page content.

### 4. Documentation is bilingual

- Every document ships in English (`name.md`) and Chinese (`name.zh.md`) in the same commit; see
  [docs/LANGUAGE.md](docs/LANGUAGE.md). Section 5 fixes the writing rules: no reasoning traces, no filler.
- A document without its counterpart is not done: either deliver both, or add the gap to the index in
  [docs/README.md](docs/README.md) as `pending`.
- Write in the product's own voice: describe what Corvus is and does, never compare it to another product.
- Each document opens with its reader and purpose; terms and protocol fields follow
  `docs/02-architecture.md`.
- Protocol, configuration and error-code changes must update both the architecture and the requirements docs.

### 5. Review checklist

- [ ] commit message follows section 1
- [ ] `cargo fmt` / `clippy` / `test` green
- [ ] new or changed behaviour is covered by tests
- [ ] both language editions updated
- [ ] no tokens, no local absolute paths, no personal account data

### 6. Reporting

- Defect: symptom, reproduction steps, expected behaviour, environment (OS, browser version, Corvus version).
- Request: the scenario and the desired outcome; leave the implementation open.

---

## 中文版

Corvus 以公开源码、允许非商业使用的方式开放（见 `LICENSE`）。任何贡献都视为按同一许可提交。

### 1. 提交信息

格式：

    <type>: <一句话结论>

    - 结论要点
    - 影响范围
    - 必要的数据（版本、阈值、错误码）

- `type` 取值：`feat` `fix` `perf` `refactor` `docs` `test` `build` `ci` `chore`。
- 标题行只写**变更结果**，不写过程：不写「先试了 A，发现不行，改成 B」。
- 正文只写结论、约束、影响面与验证方式，不写推导过程、调试流水、对话记录。
- 一个提交只做一件事；重构与功能分开提交，便于回滚。
- 不提交任何本地路径、令牌、代理配置、账号信息。

### 2. 分支与合流

- `main` 始终可通过 CI；禁止直接推送到 `main`（维护者发布提交除外）。
- 功能分支命名：`feat/<主题>`、`fix/<主题>`；一个分支对应一个变更主题。
- 合流方式：squash merge，保持 `main` 每个提交都是一个完整结论。

### 3. 代码规范

- 格式化：`cargo fmt --all`，CI 以 `--check` 校验。
- 静态检查：`cargo clippy --workspace --all-targets -- -D warnings`，零告警。
- 测试：`cargo test --workspace`；新增公开行为必须带测试，协议变更必须带往返测试。
- 依赖：新增依赖需在 PR 描述里说明用途与替代方案；能用标准库就不引入依赖。
- 错误处理：库层用 `thiserror` 定义错误类型，进程入口用 `anyhow` 汇总；不吞错误，不用 `unwrap` 处理外部输入。
- 异步：统一 `tokio`；阻塞或 CPU 密集工作不得占用 reactor 线程。
- 日志：`tracing` 结构化字段；不打日志打印密钥、令牌、Cookie、页面原文。

### 4. 文档双语

- 每份文档必须在同一提交里同时提供英文版（`name.md`）与中文版（`name.zh.md`），规则见
  [docs/LANGUAGE.md](docs/LANGUAGE.md)。第 5 节规定写作规则：不写推理过程，不写填充词。
- 缺一版视为未完成：要么两版一起交，要么把缺口登记到 [docs/README.md](docs/README.md) 的状态表里标 `pending`。
- 文档以产品自身口吻书写：描述 Corvus 是什么、怎么做，不与他人产品做对照。
- 每份文档开头说明读者与目的；术语与协议字段以 `docs/02-architecture.md` 为准。
- 协议、配置、错误码的变更，必须同步更新架构文档与需求规格。

### 5. 评审清单

提交 PR 前自查：

- [ ] 提交信息符合第 1 节
- [ ] `cargo fmt` / `clippy` / `test` 全绿
- [ ] 新增或修改的行为有测试覆盖
- [ ] 中英两版都已更新
- [ ] 无令牌、无本地绝对路径、无个人账号信息

### 6. 问题反馈

- 缺陷：说明现象、复现步骤、期望行为、环境（操作系统、浏览器版本、Corvus 版本）。
- 需求：说明使用场景与期望结果，不限定实现方式。
