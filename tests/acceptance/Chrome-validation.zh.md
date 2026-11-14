[English](Chrome-validation.md) | [中文](Chrome-validation.zh.md)

# Chrome 验证矩阵

## 1. 目的

本文件记录冻结的 Corvus 工具面在 Chrome for Testing 上的验证，并与仓库中已有的 Edge 证据做对照。它是简洁索引，不替代完整套件报告。

## 2. 环境与证据根目录

| 项目 | 值 |
| --- | --- |
| 浏览器 | Chrome for Testing 157.0.8103.78，无头端点 `127.0.0.1:9333` |
| 浏览器 id | `chromium:7abc8aafb8098216` |
| Corvus 服务 | `corvusd` 0.1.0，MCP 2025-06-18，`http://127.0.0.1:7331/mcp` |
| 扩展 | 未打包 Corvus 扩展 0.1.0；能力 `dom`、`scripting`、`debugger` |
| 固定页面 | `http://127.0.0.1:8811/`、`/m2b`、`/blank`、`/api/json`、`/api/slow`、`/dl.txt` |
| Chrome 证据根 | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome` |
| M2b 证据日志 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive-chrome-20261114T2036.log` |

使用现有 Corvus Windows 服务，未重启或停止。专用无头 Chrome 只通过其自己的标签页使用。

## 3. 检查总数

| 套件 | 检查数 | 结果 |
| --- | ---: | --- |
| 冒烟 | 7 | 7 / 7 通过 |
| M3 录制、回放与可观测性 | 34 | 34 / 34 通过 |
| M4 取消与按标签页门控 | 16 | 16 / 16 通过 |
| M2b 网络、下载与上传 | 44 | 44 / 44 通过 |
| **合计** | **101** | **101 / 101 通过** |

## 4. 套件矩阵

| 套件 | Chrome 检查数 | Chrome 证据 | Edge 对照 |
| --- | ---: | --- | --- |
| 冒烟：tab_new、page_text、type、click、eval、snapshot、tab_close | 7 | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/smoke.json` | `tests/report/M2b-live-report.md`（Edge 实机冒烟及完整 105 检查） |
| M3：会话、步骤、导出、干跑/实跑回放、metrics、日志、Prometheus | 34 | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/m3d/checks.json` | `tests/report/M3-report-template.md`（M3 一致性 RESULT 127/0/0） |
| M4：-32800 取消、取消空操作、不同标签页并行、同标签页串行 | 16 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T12-44-30-592Z/cancel.json` 与 `gates.json`；摘要日志 `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/cancel-chrome-20261114T2044.log`；驱动 `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/cancel_drive.mjs` | `tests/report/M4-report-template.md`（M4 一致性 RESULT 135/0/0，取消参考 56 ms） |
| M2b：抓包、net_log、拦截、下载、上传、路由 | 44 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive-chrome-20261114T2036.log`；驱动 `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive.mjs` | `tests/report/M2b-live-report.md`（Edge 实机 RESULT passed=105 failed=0） |

101 是 Chrome 实机矩阵的合计。Edge 套件按各自运行规模计数（M2b 105 个实机检查点、M3 127 个一致性检查、M4 135 个检查）；两边行数并不相同。Edge 列用于对照共享工具面，不假装计数一致。

## 5. Chrome 观察

- `browser_list` 只返回一个浏览器 `chromium:7abc8aafb8098216`，具备 `dom`、`scripting`、`debugger`。
- M2b 44 检查驱动报告 `tool_count: 32`，因为它接受当前 32 工具守护进程（`>= 26`）；这不是 M2b 回归。
- 隐藏文件输入可用：`upload_file` 附加了 `up.txt`，`browser_eval` 看到 `input.files[0].name === "up.txt"`。
- M2b 下载证据：id 5，状态 `complete`，13 / 13 字节，文件名 `D:\ChromeDownloadsTest\dl (3).txt`。
- 取消证据：HTTP 取消返回 202，被取消响应为 JSON-RPC `-32800`，耗时 716 ms；不同标签页与同标签页门控耗时分别为 418 ms 和 843 ms。
- 未观察到 Chrome 独有的成功形状、错误码或模式差异。

## 6. 复现

先启动专用无头 Chrome 和实机服务，再从证据目录运行驱动。精确脚本与 JSON 证据在同一目录：

```bash
node /tmp/corvus-chrome/smoke.mjs
node /tmp/corvus-m2b/drive.mjs
```

M3 与 M4 驱动位于 `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/m3d` 和 `C:/Users/Administrator/AppData/Local/Temp/corvus-m4`。如果驱动不生成结果文件，请把标准输出保存为日志。

## 7. 通过标准

- 四个套件均报告零失败；合计 101 / 101。
- 每个检查都使用专用无头 Chrome 浏览器 id，且不触碰客户端自己的标签页。
- 不记录产品缺陷；任何测试框架修正都在套件日志中说明。
- 上述 Edge 证据链接仍是共享工具面的基线。

## 8. 限制与后续

- Chrome 只有一个无头浏览器。多浏览器歧义与双 mock 路由仍由一致性套件覆盖，不在本实机矩阵内。
- Chrome M2b 运行会把真实下载写入专用测试下载目录；仅在磁盘占用重要时清理。
- 本文件只做证据索引，不改变冻结的 M2/M3/M4 契约。
