[English](Brave-validation.md) | [中文](Brave-validation.zh.md)

# Brave 验证矩阵

## 1. 目的

本文件记录冻结的 Corvus 工具面在 Brave Browser 上的验证，并补充仓库中已有的 Edge 与 Chrome 证据。它是简洁索引，不替代完整套件报告。

## 2. 环境与证据根目录

| 项目 | 值 |
| --- | --- |
| 浏览器 | Brave Browser 157.1.99.57，官方 win32-x64 便携版，无头端点 `127.0.0.1:9335` |
| 浏览器 id | `brave:832eff2fef81596d` |
| Corvus 服务 | `corvusd` 0.1.0，MCP 2025-06-18，`http://127.0.0.1:7331/mcp` |
| 扩展 | 未打包 Corvus 扩展 0.1.0；能力 `dom`、`scripting`、`debugger` |
| 固定页面 | `http://127.0.0.1:8811/`、`/m2b`、`/blank`、`/api/json`、`/api/slow`、`/dl.txt` |
| Brave 证据根 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave` |
| M2b 证据日志 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m2b_run.txt` |
| M4 证据 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T14-25-24-147Z` |

使用现有 Corvus Windows 服务，未重启或停止。Brave 便携版只通过其自己的标签页使用。

## 3. 检查总数

| 套件 | 检查数 | 结果 |
| --- | ---: | --- |
| 冒烟 | 7 | 7 / 7 通过 |
| M3 录制、回放与可观测性 | 34 | 34 / 34 通过 |
| M4 取消与按标签页门控 | 16 | 16 / 16 通过 |
| M2b 网络、下载与上传 | 44 | 44 / 44 通过 |
| **合计** | **101** | **101 / 101 通过** |

## 4. 套件矩阵

| 套件 | Brave 检查数 | Brave 证据 | Chrome 对照 |
| --- | ---: | --- | --- |
| 冒烟：tab_new、page_text、type、click、eval、snapshot、tab_close | 7 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/smoke.log` 与 `smoke.json` | `Chrome-validation.zh.md`（7 / 7） |
| M3：会话、步骤、导出、干跑/实跑回放、metrics、日志、Prometheus | 34 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m3/checks.json` | `Chrome-validation.zh.md`（34 / 34） |
| M4：-32800 取消、取消空操作、不同标签页并行、同标签页串行 | 16 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T14-25-24-147Z/cancel.json` 与 `gates.json` | `Chrome-validation.zh.md`（16 / 16） |
| M2b：抓包、net_log、拦截、下载、上传、路由 | 44 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m2b_run.txt` | `Chrome-validation.zh.md`（44 / 44） |

101 是 Brave 实机矩阵的合计。Edge、Chrome 与 Brave 套件按各自运行规模计数，行数并不一一对应；对照列指向归档证据，不声称检查点完全相同。

## 5. Brave 观察

- `browser_list` 只返回一个浏览器 `brave:832eff2fef81596d`，具备 `dom`、`scripting`、`debugger`。
- M2b 44 检查驱动报告 `tool_count: 32`，因为它接受当前 32 工具守护进程（`>= 26`）；这不是 M2b 回归。
- 隐藏文件输入可用：`upload_file` 附加了 `up.txt`，`browser_eval` 读到了附件名。
- 首次 M2b 运行得分 42 / 44，因为新的便携配置没有下载目录，下载状态为 `interrupted`。在一次性配置中把 `download.default_directory` 设为 `D:\\BraveDownloadsTest` 并重启 Brave 后，复跑得分 44 / 44：id 2、状态 `complete`、13 / 13 字节、文件名 `D:\\BraveDownloadsTest\\dl.txt`。这是配置问题，不是产品缺陷。
- 取消证据：被取消响应为 JSON-RPC `-32800`；不同标签页并行耗时 412 ms，同标签页串行 821 ms，取消后门控在 415 ms 内释放。
- 未观察到 Brave 独有的成功形状、错误码或模式差异。

## 6. 复现

先启动便携无头 Brave 和实机服务，再从证据目录运行驱动：

```bash
node /tmp/corvus-chrome/smoke.mjs
node /tmp/corvus-m3/drive.mjs
node /tmp/corvus-m4/cancel_drive.mjs
node /tmp/corvus-m2b/drive.mjs
```

M2b 运行前先设置一次性配置的 `download.default_directory`。如果驱动不生成结果文件，请把标准输出保存为日志。

## 7. 通过标准

- 四个套件均报告零失败；合计 101 / 101。
- 每个检查都使用专用无头 Brave 浏览器 id，且不触碰客户端自己的标签页。
- 不记录产品缺陷；任何配置或测试框架修正都在本文件中说明。
- 上述 Edge 与 Chrome 证据仍是共享工具面的基线。

## 8. 限制与后续

- Brave 只有一个便携无头浏览器。多浏览器歧义与双 mock 路由仍由一致性套件覆盖，不在本实机矩阵内。
- 便携版不是安装器；Windows 服务与扩展安装路径由 Edge 证据和实机安装冒烟覆盖。
- Brave M2b 运行会把真实下载写入 `D:\\BraveDownloadsTest`；仅在磁盘占用重要时清理。
- 本文件只做证据索引，不改变冻结的 M2/M3/M4 契约。
