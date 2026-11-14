[English](PROCESS.md) | [中文](PROCESS.zh.md)

# 交付流程

说明一项变更如何从共享任务板走到 `main`，再进入发布。本文对仓库内所有参与者具有约束力；贡献规范见
[CONTRIBUTING.md](../CONTRIBUTING.md)，双语规则见 [LANGUAGE.md](LANGUAGE.md)。

## 1. 角色

| 角色 | 负责 | 决定 | 汇报对象 |
|---|---|---|---|
| 负责人（Lead） | 范围、任务板、评审门禁、发布批准 | 交付什么、何时交付 | 无 |
| 后端（Backend） | `crates/`：守护进程、命令行、协议 | 模块划分、错误模型 | Lead |
| 扩展（Extension） | `extension/`：MV3 扩展包 | 扩展内部实现 | Lead |
| 测试（QA） | 验证、验收执行、回归 | 是否满足验收标准 | Lead |
| 运维（DevOps） | `.github/`、`scripts/`、本文档 | CI 形态、发布产物 | Lead |

小团队可以兼任角色，但评审门禁不能自审：每一份合入 `main` 的变更都由 Lead 评审。

## 2. 任务板流程

1. 任务写明目标结果、验收标准与预计改动的写作用域；依赖其它任务的工作标注 `blocked_by`。
2. 负责人按任务当前版本认领。一个任务只有一个负责人。
3. 改动不得越出已声明的写作用域；确需跨域时，先与 Lead 确认。
4. 任务发生实质变化时，负责人更新描述；所有参与者以最新版本为准。
5. 满足下文「完成定义」后，负责人才用新版本关闭任务。

任务板是在途工作的唯一记录。聊天和笔记可以更快，但不能替代任务板。

## 3. 完成定义

同时满足以下条件才算完成：

- 在 Windows、Linux、macOS 的干净检出上可以构建并运行；
- `cargo fmt --all -- --check` 无差异；
- `cargo clippy --workspace --all-targets -- -D warnings` 零告警；
- `cargo test --workspace` 通过；新增公开行为有测试，协议变更带往返测试；
- 文档按 [LANGUAGE.md](LANGUAGE.md) 同步更新中英两版；
- 改动中没有令牌、本地绝对路径、代理配置或账号信息；
- 脚本保持幂等，并经过实际运行验证，而不是只读代码；
- 任务板条目以新版本关闭。

## 4. 评审门禁

1. 作者自查：运行上述三条命令并通读完整 diff。
2. Lead 门禁：Lead 重新运行格式化、clippy 与测试，通读完整 diff，确认写作用域未被越界、任务描述属实。
3. 门禁通过后由 Lead 提交；门禁未关闭期间任何人不得提交或推送。
4. `main` 使用 squash 合流，且必须保持全绿；CI 是每次推送与拉取请求的第二道门禁。
5. 门禁失败时，把失败的命令与输出附在任务上退回作者，而不是只给一句结论。

## 5. 提交信息

格式：

    <type>: <一句话结论>

    - 结论要点
    - 影响范围
    - 必要的数据（版本、阈值、错误码）

- `type` 取值：`feat` `fix` `perf` `refactor` `docs` `test` `build` `ci` `chore`。
- 标题行只写变更结果，不写得出该结果的过程。
- 正文只写结论、约束、影响面与验证方式；不得写入推理记录、调试日志、工具输出或「先试 A 再改 B」的经过，这些属于任务板，不属于历史。
- 一个提交只做一件事：重构与功能分开，便于单独回滚。

本规则与 [CONTRIBUTING.md](../CONTRIBUTING.md) 第 1 节一致，本文不放宽。

## 6. 发布节奏

- 里程碑版本在 [03-roadmap.zh.md](03-roadmap.zh.md) 的 M1-M4 退出标准达成时发布，不只按日历。
- 补丁版本在缺陷阻断用户时发布，没有固定班次。
- 每次发布都要求三平台 CI 全绿且 Lead 门禁完成。
- 发布步骤：更新 `Cargo.toml` 中的工作区版本、写变更记录、运行 `scripts/build.sh`、连同 `SHA256SUMS` 发布二进制、打 `vX.Y.Z` 标签。
- 发布说明与其它文档一样提供中英两版。

## 7. 本地开发

- `scripts/dev-setup.sh` 定位 Rust 工具链、打印所用版本，并用 `cargo metadata --offline` 验证工作区可以解析。
- `scripts/build.sh` 以 release 模式构建 `corvusd` 与 `corvus` 到 `dist/`，并生成 `dist/SHA256SUMS`。
- 本机使用 `--offline` 构建，因为依赖来自已预热的 cargo 缓存、且没有通用外网。CI 正常从网络安装；两种环境的差别只有这个参数，干净构建以 CI 为准。
- CI 在 `ubuntu-latest`、`windows-latest`、`macos-14` 上运行同样的三道门禁，见 [.github/workflows/ci.yml](../.github/workflows/ci.yml)。
