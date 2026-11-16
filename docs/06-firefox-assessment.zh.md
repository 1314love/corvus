[English](06-firefox-assessment.md) | [中文](06-firefox-assessment.zh.md)

# Firefox 可行性评估

状态：评估完成于 v0.1.0 表面，并于 2026-11-16 对照 1.7.0 发布复核；完整对等为 NO-GO；降级 DOM/标签页模式为有条件 GO。决策仍然成立，下文工具数均标明基线。

范围：Firefox 121+ 桌面版。原始评估对照 v0.1.0 的浏览器侧表面，即 32 个 MCP 工具中的 26 个。1.7.0 发布有 62 个 MCP 工具，其中 50 个面向浏览器、12 个由守护进程本地应答，46 个可经 CDP 使用、42 个由扩展提供，因此本文的 26 与 18 两个数字属于历史基线。保留它们是因为平台缺口与降级模式的形态没有变化，不是因为它们描述当前目录。本文只记录可行性决策，不改变产品代码或当前支持矩阵。

## 1. 决策

| 选项 | 决策 | 理由 |
|---|---|---|
| 完整浏览器侧对等（v0.1.0 为 26 个工具；1.7.0 发布扩展侧为 42 个） | NO-GO | Firefox 121+ 没有 `chrome.debugger`/CDP attach，也没有 `chrome.offscreen`；平台缺口没有 ETA。 |
| 降级 18 工具 DOM/标签页模式（v0.1.0 范围） | 有条件 GO | 用内容脚本与 tabs API 可行，但会失去 8 个工具，需要 15-20 工程师日。若现在启动移植，需要按 1.7.0 发布的 42 个扩展侧浏览器工具重新划定范围。 |
| 当前里程碑 | 暂不支持 Firefox | 客户尚未承诺 Firefox；记录 NFR-6 偏差，等待产品决策。 |

建议：不把 Firefox 加入当前里程碑。本文记录 NFR-6 偏差，客户决策待定；若 Firefox 成为承诺目标，或 Bugzilla 1316741 落地，再重新评估。

## 2. 完整对等为何是 NO-GO

原始对等目标是 26 个浏览器侧工具：32 个 MCP 工具减去 6 个守护进程级工具（`session_export`、`session_list`、`session_replay`、`session_steps`、`metrics`、`logs_tail`）。当前树有 50 个浏览器侧工具：62 个工具减去 12 个守护进程级工具，其中 CDP 提供 46 个、扩展提供 42 个。绝对数量变了，平台缺口没有变。

Firefox 121+ 不提供 `chrome.debugger` 或等价的 CDP attach API。Bugzilla 1316741 状态为 NEW，无 ETA。扩展的 CDP 桥依赖该 API：

| 位置 | 用途 |
|---|---|
| `extension/src/background/runtime.ts:3277` | `chrome.debugger.getTargets` |
| `extension/src/background/runtime.ts:3294` | `chrome.debugger.attach` |
| `extension/src/background/runtime.ts:3343` | `chrome.debugger.detach` |
| `extension/src/background/runtime.ts:3388` | `chrome.debugger.sendCommand` |
| `extension/src/background/runtime.ts:3642` | `chrome.debugger.onDetach` |
| `extension/src/background/runtime.ts:3648` | `chrome.debugger.onEvent` |

Firefox 也不提供 `chrome.offscreen`。当前扩展是 MV3 service worker；它创建 offscreen 文档，在 service worker 之外持有本地回环 WebSocket 与心跳：

| 位置 | 用途 |
|---|---|
| `extension/manifest.json:8-10` | MV3 `background.service_worker` |
| `extension/manifest.json:17` | `debugger` 权限 |
| `extension/manifest.json:21` | `offscreen` 权限 |
| `extension/src/background/runtime.ts:467-503` | 能力检查与 `chrome.offscreen.createDocument` |
| `extension/src/background/runtime.ts:495` | 明确用途：持有回环 WebSocket 与心跳 |
| `extension/src/offscreen/index.ts:160` | 创建 WebSocket |
| `extension/src/offscreen/index.ts:274` | 保活定时器 |
| `extension/src/offscreen/index.ts:278` | 健康检查定时器 |

没有 offscreen，完整对等需要另一套长连接设计。Firefox 事件页不是直接替代，因为空闲时会被挂起。

## 3. 降级 18 工具模式（有条件 GO）

降级模式保留 DOM 与标签页操作，去掉调试、网络、上传与下载类工具。下表的保留集属于 v0.1.0 范围。当前树若启动，需要按 42 个扩展侧浏览器工具重新划定范围，失去的工具会多于这里列出的 8 个。

保留工具（18 个）：

| 组 | 工具 |
|---|---|
| DOM | `browser_click`、`browser_hover`、`browser_press`、`browser_scroll`、`browser_select`、`browser_type`、`browser_wait`、`page_find`、`page_html`、`page_snapshot`、`page_text` |
| 导航 | `browser_history`、`browser_navigate` |
| 浏览器与标签页 | `browser_list`、`tab_close`、`tab_list`、`tab_new`、`tab_switch` |

去掉工具（8 个，v0.1.0 范围）：`browser_eval`、`upload_file`、`net_capture`、`net_clear`、`net_intercept`、`net_log`、`download_list`、`download_wait`。

保留集走内容脚本、tabs/scripting API 与页面 DOM，不需要 `debugger`、`webRequest`、`downloads` 或 `offscreen`。

工作量：5-7 工程师日做出原型，证明 18 工具路径；15-20 工程师日达到生产质量，含 Firefox manifest 与打包、测试、文档与发布加固。这些估算属于 v0.1.0 范围，任何承诺前都需要按当前树重估。72 小时浸泡测试另计日历时间，不计工程师日。

风险：

- 事件页 WebSocket 挂起。Firefox MV3 的后台事件页空闲时可能被挂起。没有 offscreen 文档，回环连接可能断开；alarms 保活与重连只能降低概率，不能消除。72 小时可靠性目标有风险。当前设计用 offscreen 文档正是为了避免这一点（`extension/src/background/runtime.ts:495`）。
- 工具面缩小。没有网络检查、上传、下载与 eval；依赖这些能力的客户端行为会不同，能力列表与安装文档必须写明。
- 维护成本。第二套扩展代码路径、第二套 manifest 与打包流程、更大的 CI 矩阵，都是持续成本。
- 平台差异。WebExtension 行为、内容脚本时序与标签页生命周期都与 Chromium 不同，测试面更大。
- Bug 1316741 没有 ETA。即使未来落地，完整对等也必须重新评估，不能直接假定可用。

## 4. 条件与后续

只有客户明确把 Firefox 列为支持目标并接受重新划定范围的降级模式，才启动。

投产前，原型必须证明：

- Firefox 121+（或当前 ESR）上 72 小时 WebSocket 稳定。
- 重新划定范围后保留的每个工具在验收页面上的对等。
- 能力列表公布缩减后的工具面。

若批准，Firefox 端口放在独立扩展目录，不改变 Chrome/Edge 交付路径。

若未批准，保持 Firefox 不支持，NFR-6 偏差继续开放，只在产品决策或 Bugzilla 1316741 变化时重评。

## 5. 证据与置信度

- `extension/manifest.json:8-10` 使用 MV3 `background.service_worker`；`:17` 申请 `debugger`；`:21` 申请 `offscreen`。
- `extension/src/background/runtime.ts:3277,3294,3343,3388,3642,3648` 是 CDP 桥。
- `extension/src/background/runtime.ts:467-503` 创建并说明 offscreen 文档。
- `extension/src/offscreen/index.ts:160,274,278` 创建 WebSocket 并运行定时器。
- [Bugzilla 1316741](https://bugzilla.mozilla.org/show_bug.cgi?id=1316741) 状态为 NEW，无 ETA。
- 26/18 两个工具数由 v0.1.0 的 MCP 工具面和上述 8 个被去掉的工具推导。当前 34/10 的划分记录在 [10-project-state.zh.md](10-project-state.zh.md)。
- v0.1.0 的 `extension/background.js` 与 `extension/offscreen.js` 已在 1.4.0 被上述 TypeScript 源码取代。旧行号对当前树无效。
- 置信度：平台缺口与 NO-GO 决策为高；降级模式设计为中；针对当前树的工期估算为低，尚未在 Firefox 上重新测量。

本次评估未改动产品代码、扩展代码或测试。
