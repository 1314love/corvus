## Summary / 摘要

One paragraph: what changed and why. State the conclusion, not the reasoning. 一段话：改了什么、为什么。写结论，不写推导。

## Type / 类型

- [ ] feat
- [ ] fix
- [ ] perf
- [ ] refactor
- [ ] docs
- [ ] build
- [ ] ci
- [ ] chore

## Scope / 影响范围

Crates, directories and user-visible behaviour touched. Breaking changes and their migration. 涉及的 crate、目录与用户可见行为；破坏性变更及迁移方式。

## Gates / 门禁

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Paste the summary lines and any failure. 贴汇总行与失败信息，不要贴整份日志。

## Documentation / 文档

- [ ] English and Chinese editions both updated ([docs/LANGUAGE.md](../docs/LANGUAGE.md)); 中英两版同步更新
- [ ] `CHANGELOG.md` entry, one sentence; `CHANGELOG.md` 一句话条目
- [ ] Release notes updated if user-visible; 用户可见变更更新发布说明

## Checklist / 检查

- [ ] Commit message states the change only; 提交信息只写变更结论
- [ ] No tokens, cookies, personal paths or customer names; 无令牌、Cookie、个人路径、客户名
- [ ] New dependencies justified in the summary; 新依赖在摘要里说明理由
- [ ] Scripts and commands run end to end; 脚本与命令实际跑通
