# Security policy / 安全策略

[English](#english) | [中文](#中文)

---

## English

### Supported versions

| Version | Security fixes |
|---|---|
| 1.2.x | yes |
| 1.1.x | no; upgrade |
| 1.0.x and older | no; upgrade |

`corvusd --version` prints the running version; the extension popup shows it too.

### Report privately

Never a public issue, discussion or pull request.

- GitHub private advisory: https://github.com/1314love/corvus/security/advisories/new . Visible to the maintainer only; the fix and the disclosure run through the draft advisory.
- Email `cosher@163.com`, subject `Corvus security`.

Include:

- version, and how it was installed: release archive, source build or container;
- OS, browser, browser version, channel (`extension`, `cdp`, `auto`);
- impact, with steps or a minimal script;
- logs, MCP traffic or a session export with tokens, cookies and page text removed.

### Timeline

- 3 business days: acknowledgement.
- 10 business days: severity and scope.
- 90 days: disclosure embargo, shorter when a fix ships earlier.
- Credit in the advisory, under your name or handle, or anonymous.

No paid bounty.

### Scope

In: daemon and CLI (`crates/`), MCP server and transports, extension (`extension/`), packaging and installers (`scripts/`, `packaging/`), image and manifests (`deploy/`), workflows (`.github/`).

Out: Chromium and browser bugs; an attacker who already owns the local account, browser profile or daemon config; the design facts below; soak status.

### Design facts

Known and intentional. Report only a way to break them.

- No authentication on the loopback ports `7330`/ `7331`; exposing them is unsupported.
- CDP `9222`/`9223`/`9333` is unauthenticated and controls every profile of that browser.
- Extension permissions: `<all_urls>`, `debugger`, `webRequest`, `downloads`.
- `data/corvus.db` stores command arguments and results; treat it as personal data and delete it with the install.
- Container image ships no browser and cannot reach host loopback.

### Safe harbour

Local testing on your own installation, no third-party data, no disruption outside your machine: authorised research. No legal action; a public statement on request.

---

## 中文

### 支持的版本

| 版本 | 安全修复 |
|---|---|
| 1.2.x | 有 |
| 1.1.x | 无，请升级 |
| 1.0.x 及更早 | 无，请升级 |

`corvusd --version` 打印运行版本，扩展弹窗也会显示。

### 私密报告

不要开公开 issue、讨论或 pull request。

- GitHub 私密报告：https://github.com/1314love/corvus/security/advisories/new 。仅维护者可见；修复与披露走安全公告草稿。
- 邮件 `cosher@163.com`，主题 `Corvus security`。

报告请包含：

- 版本，以及安装方式：发布包、源码构建或容器；
- 操作系统、浏览器、浏览器版本、通道（`extension`、`cdp`、`auto`）；
- 可达的影响，附步骤或最小脚本；
- 日志、MCP 流量或会话导出，先去掉令牌、Cookie 与页面原文。

### 时间线

- 3 个工作日：确认收到。
- 10 个工作日：严重程度与范围。
- 90 天：披露禁期，修复提前发布则缩短。
- 安全公告署名，用你的名字或 ID，也可匿名。

没有赏金计划。

### 范围

在范围：守护进程与 CLI（`crates/`）、MCP 服务与传输、扩展（`extension/`）、打包与安装脚本（`scripts/`、`packaging/`）、镜像与清单（`deploy/`）、工作流（`.github/`）。

不在范围：Chromium 与浏览器自身的缺陷；已控制本机账户、浏览器配置档或守护进程配置的攻击者；下列设计事实；压测状态。

### 设计事实

已知且有意为之。只有找到打破它们的方法才需要报告。

- 回环端口 `7330`/ `7331` 没有认证，暴露到网络不受支持。
- CDP `9222`/`9223`/`9333` 无认证，可控制该浏览器的全部配置档。
- 扩展权限：`<all_urls>`、`debugger`、`webRequest`、`downloads`。
- `data/corvus.db` 记录命令参数与结果，按个人数据处理，卸载时删除。
- 容器镜像不含浏览器，也无法访问宿主回环。
