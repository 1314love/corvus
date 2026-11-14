[English](06-firefox-assessment.md) | [中文](06-firefox-assessment.zh.md)

# Firefox 可行性评估

状态：评估完成；完整对等为 NO-GO，降级 DOM/标签页模式为有条件 GO。

范围：Firefox 121+ 桌面版，对照 v0.1.0 的浏览器侧工具面。本文只记录可行性决策，不改变产品代码或当前支持矩阵。

## 1. 决策

| 选项 | 决策 | 理由 |
|---|---|---|
| 完整 26 工具对等 | NO-GO | Firefox 121+ 没有 `chrome.debugger`/CDP attach，也没有 `chrome.offscreen`；平台缺口没有 ETA。 |
| 降级 18 工具 DOM/标签页模式 | 有条件 GO | 用内容脚本与 tabs API 可行，但会失去 8 个工具，需要 15-20 工程师日。 |
| 当前里程碑 | 暂不支持 Firefox | 客户尚未承诺 Firefox；记录 NFR-6 偏差，等待产品决策。 |

建议：不把 Firefox 加入当前里程碑。本文记录 NFR-6 偏差，客户决策待定；若 Firefox 成为承诺目标，或 Bugzilla 1316741 落地，再重新评估。

## 2. 完整对等为何是 NO-GO

对等目标是 26 个浏览器侧工具：32 个 MCP 工具减去 6 个守护进程级工具（`session_export`、`session_list`、`session_replay`、`session_steps`、`metrics`、`logs_tail`）。完整对等意味着 26 个工具都能在 Firefox 上工作。

Firefox 121+ 不提供 `chrome.debugger` 或等价的 CDP attach API。Bugzilla 1316741 状态为 NEW，无 ETA。扩展的 CDP 桥依赖该 API：

| 位置 | 用途 |
|---|---|
| `extension/background.js:2935` | `chrome.debugger.getTargets` |
| `extension/background.js:2950` | `chrome.debugger.attach` |
| `extension/background.js:2999` | `chrome.debugger.detach` |
| `extension/background.js:3044` | `chrome.debugger.sendCommand` |
| `extension/background.js:3198` | `chrome.debugger.onDetach` |
| `extension/background.js:3204` | `chrome.debugger.onEvent` |

Firefox 也不提供 `chrome.offscreen`。当前扩展是 MV3 service worker；它创建 offscreen 文档，在 service worker 之外持有本地回环 WebSocket 与心跳：

| 位置 | 用途 |
|---|---|
| `extension/manifest.json:8` | `"service_worker": "background.js"` |
| `extension/background.js:333,334` | 检查 `chrome.offscreen` 能力 |
| `extension/background.js:356` | `chrome.offscreen.createDocument` |
| `extension/background.js:359` | 明确用途：持有 WebSocket 与心跳 |
| `extension/offscreen.js:148` | 创建 WebSocket |
| `extension/offscreen.js:260` | 保活定时器 |
| `extension/offscreen.js:264` | 健康检查定时器 |
| `extension/offscreen.js:296` | 到 socket 的消息桥 |

没有 offscreen，完整对等需要另一套长连接设计。Firefox 事件页不是直接替代，因为空闲时会被挂起。

## 3. 降级 18 工具模式（有条件 GO）

降级模式保留 DOM 与标签页操作，去掉调试、网络、上传与下载类工具。

保留工具（18 个）：

| 组 | 工具 |
|---|---|
| DOM | `browser_click`、`browser_hover`、`browser_press`、`browser_scroll`、`browser_select`、`browser_type`、`browser_wait`、`page_find`、`page_html`、`page_snapshot`、`page_text` |
| 导航 | `browser_history`、`browser_navigate` |
| 浏览器与标签页 | `browser_list`、`tab_close`、`tab_list`、`tab_new`、`tab_switch` |

去掉工具（8 个）：`browser_eval`、`upload_file`、`net_capture`、`net_clear`、`net_intercept`、`net_log`、`download_list`、`download_wait`。

保留集走内容脚本、tabs/scripting API 与页面 DOM，不需要 `debugger`、`webRequest`、`downloads` 或 `offscreen`。

工作量：5-7 工程师日做出原型，证明 18 工具路径；15-20 工程师日达到生产质量，含 Firefox manifest 与打包、测试、文档与发布加固。72 小时浸泡测试另计日历时间，不计工程师日。

风险：

- 事件页 WebSocket 挂起。Firefox MV3 的后台事件页空闲时可能被挂起。没有 offscreen 文档，回环连接可能断开；alarms 保活与重连只能降低概率，不能消除。72 小时可靠性目标有风险。当前设计用 offscreen 文档正是为了避免这一点（`extension/background.js:359`）。
- 工具面缩小。没有网络检查、上传、下载与 eval；依赖这些能力的客户端行为会不同，能力列表与安装文档必须写明。
- 维护成本。第二套扩展代码路径、第二套 manifest 与打包流程、更大的 CI 矩阵，都是持续成本。
- 平台差异。WebExtension 行为、内容脚本时序与标签页生命周期都与 Chromium 不同，测试面更大。
- Bug 1316741 没有 ETA。即使未来落地，完整对等也必须重新评估，不能直接假定可用。

## 4. 条件与后续

只有客户明确把 Firefox 列为支持目标并接受 18 工具模式，才启动。

投产前，原型必须证明：

- Firefox 121+（或当前 ESR）上 72 小时 WebSocket 稳定。
- 18 个保留工具在验收页面上的对等。
- 能力列表公布缩减后的工具面。

若批准，Firefox 端口放在独立扩展目录，不改变 Chrome/Edge 交付路径。

若未批准，保持 Firefox 不支持，NFR-6 偏差继续开放，只在产品决策或 Bugzilla 1316741 变化时重评。

## 5. 证据

- `extension/manifest.json:7-8` 使用 MV3 `background.service_worker`；`:15` 申请 `debugger`；`:19` 申请 `offscreen`。
- `extension/background.js:2935,2950,2999,3044,3198,3204` 是 CDP 桥。
- `extension/background.js:333,334,356,359` 创建并说明 offscreen 文档。
- `extension/offscreen.js:148,260,264,296` 创建 WebSocket、运行定时器并转发消息。
- [Bugzilla 1316741](https://bugzilla.mozilla.org/show_bug.cgi?id=1316741) 状态为 NEW，无 ETA。
- 26/18 两个工具数由 v0.1.0 的 MCP 工具面和上述 8 个被去掉的工具推导。

本次评估未改动产品代码、扩展代码或测试。
