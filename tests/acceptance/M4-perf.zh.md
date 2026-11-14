[English](M4-perf.md) | [中文](M4-perf.zh.md)

# M4 性能与内存证据：NFR-1 至 NFR-4

本手册是 Corvus NFR 的可重复性能与内存取证流程。英文原文以 [M4-perf.md](M4-perf.md) 为准，本文逐节镜像其章节与命令。

## 1. 目的

用一次实测回答四个问题：

- NFR-1：在 10,000 元素页面上，`browser_click`、`browser_type` 和 `page_snapshot` 是否足够快？
- NFR-2：10 个标签页和 10 个 MCP 客户端能否并发工作，而不是退化成单队列？
- NFR-3：持续混合负载下，守护进程内存增长是否在预算内，调用成功率是多少？
- NFR-4：release 守护进程二进制和扩展 bundle 是否在体积上限内？

默认路径可确定复现：mock 扩展提供固定延迟，便于同样条件对比。另有一条独立冒烟行只使用专用无头 Edge，确认真实扩展走同一 fixture 和快照链路。该冒烟不会使用用户浏览器，不会改停已安装的 Windows 服务，并会自行创建和关闭标签页。

用一条命令跑完整套。最终 summary 表是验收产物；本文记录方法与参考结果。

## 2. 范围与 NFR 目标

| NFR | 本套测量 | 目标 |
|-----|----------|------|
| NFR-1 | `browser_click` p95、`browser_type` p95 | 低于 50 ms |
| NFR-1 | 10,000 元素页面上 `page_snapshot` p95 | 低于 300 ms |
| NFR-2 | 10 个标签页并行发起 `browser_wait` 与 `browser_navigate` | 全部成功，墙钟接近单条命令 |
| NFR-2 | 10 个 MCP streamable-HTTP 会话 initialize 并调用 `browser_list` | 10/10 连接，10/10 调用成功 |
| NFR-3 | 900 秒混合负载加 120 秒空闲的守护进程 RSS 增长 | 低于 50 MB |
| NFR-3 | 混合负载成功率 | 至少 99.9% |
| NFR-4 | release `corvusd.exe` 体积 | 低于 25 MB |
| NFR-4 | `extension/` bundle 体积 | 低于 1 MB |

需求来源为 `docs/01-requirements.md` 第 195-198 行。需求中的 NFR-3 是 72 小时浸泡；本套用 15 分钟混合负载加 2 分钟空闲作为可重复代理，不宣称完成 72 小时浸泡，报告必须明确这一点。

10,000 元素 fixture 为 `tests/fixtures/perf-10k.html`，DOM 数量确定：`document.querySelectorAll('*').length === 10000`。

## 3. 前置条件

- Windows 10/11，MSYS bash、`curl`、`netstat.exe`。
- Node.js 20 或更新版本。
- Rust 与 Cargo；本仓库的 MSYS 路径为 `/c/Users/Administrator/.cargo/bin/cargo.exe`。
- 当前目录为仓库根：`D:/DSH/browser_bridege/corvus`。
- live 行需要 `127.0.0.1:9333` 上的专用无头 Edge，已加载 unpacked Corvus 扩展；同时已安装的 Windows Corvus 服务在 `127.0.0.1:7330`（WebSocket）和 `http://127.0.0.1:7331/mcp` 健康。预期 browser id 为 `edge:b5e09e58ecdf3caa`。
- live 行不得使用个人浏览器。任何行都不得重启或停止 Windows 服务，也不得复用其数据库给被测守护进程。

mock 行使用自己的守护进程、临时数据库、fixture 服务器和端口，不会干扰已安装服务。

## 4. 构建守护进程与模拟扩展

完整运行会把 release 守护进程构建到 `target/perf-release`，构建 mock 扩展，然后自行启动全部组件：

```bash
cd D:/DSH/browser_bridege/corvus
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh
```

`run.sh` 的默认值：

- 独立守护进程：WebSocket `31330`，HTTP `31331`；
- 临时数据库：`$TMP/corvus-perf.db`；
- fixture 服务器：由 `serve-fixture.mjs` 选择临时端口；
- mock 浏览器 id：`mock:perf`，`#delay` 延迟 250 ms；
- live 行：现有服务 `http://127.0.0.1:7331/mcp`，浏览器 `edge:b5e09e58ecdf3caa`；
- 证据目录：`$TMP/corvus-perf.XXXXXX/evidence`，默认保留。

使用已有守护进程二进制时设置 `CORVUSD_BIN`；修改端口用 `PERF_WS_PORT` 和 `PERF_HTTP_PORT`；修改样本数用 `PERF_LATENCY_SAMPLES` 和 `PERF_LIVE_SAMPLES`。验收运行必须执行 live 行。

编辑 harness 时可用短冒烟：

```bash
PERF_MEMORY_SECONDS=10 PERF_IDLE_SECONDS=5 PERF_LATENCY_SAMPLES=5 \
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh
```

## 5. 固定页面与服务器

`tests/fixtures/perf-10k.html` 为 70,226 字节、11 行。它包含 7 个结构元素（html、head、title、body、main、button、input）和 9,993 个空 `<i>` 叶子，共 10,000 个元素。没有脚本、图片、样式或外部请求，因此加载和节点遍历可确定复现。

单独诊断某一模式时可手工启动静态服务器：

```bash
PORT=0 HOST=127.0.0.1 node tests/perf/serve-fixture.mjs
```

它打印 `READY http://HOST:PORT`，并提供：

| 路径 | 内容 |
|------|------|
| `/`、`/perf-10k`、`/perf-10k.html` | 10,000 元素 fixture |
| `/load` | 内存混合负载用的 `#perf-click` 与 `#perf-type` 小页面 |

当前标签页 URL 含 `/perf-10k` 时，mock 扩展返回合成的 3,000 节点快照负载，即 NFR-1 负载行；其他 URL 返回普通小快照。真实扩展会遍历同一 10,000 元素 DOM，但其 full 快照模式只上报通过 `isInteresting` 判断的节点（可操作元素、landmark、标题、图片、显式 role 等）。空 `<i>` 叶子会被遍历但过滤掉，因此 live `node_count` 很小是正常的。live 的压力来自 10,000 元素 DOM；mock 行覆盖 3,000 节点的序列化响应。

## 6. 基准模式与证据文件

`tests/perf/bench.mjs` 有六个模式。`run.sh` 先跑五个测量模式，再跑 `summary`。

| 模式 | 输出文件 | 记录内容 |
|------|----------|----------|
| `latency` | `latency.json` | 每工具 60 样本、8 预热、p50/p95/max/mean |
| `concurrency` | `concurrency.json` | 10 标签页等待、10 标签页导航、10 MCP 客户端 |
| `memory` | `memory.json` | RSS 样本、调用成功数、增长、峰值 |
| `live` | `live.json` | 专用无头 Edge 的 fixture 冒烟 |
| `size` | `size.json` | release 二进制字节、扩展目录字节与文件数 |
| `summary` | `summary.txt` | NFR 表与最终 `RESULT` 行 |

单模式直接调用示例：

```bash
node tests/perf/bench.mjs latency \
  --url http://127.0.0.1:31331/mcp \
  --browser-id mock:perf \
  --fixture-url http://127.0.0.1:PORT/perf-10k.html \
  --samples 60 --out /tmp/latency.json
```

所有客户端调用都是 MCP streamable HTTP 上的 JSON-RPC `tools/call`。基准记录 HTTP POST 到响应解析完成的墙钟。优先使用 `structuredContent`，否则回退到 text JSON 部分。只有 HTTP 状态 200、无 JSON-RPC error 且 `isError` 为 false 才算成功。

## 7. 延迟测量方法

latency 行在 fixture URL 打开一个标签页并导航一次，然后在同一 MCP 会话、同一标签页中执行：

1. 每个工具 8 次预热；
2. 每个工具 60 次测量；
3. `browser_click` 点击 `#perf-click`；
4. `browser_type` 在 `#perf-type` 输入 `perf`，`clear: true`；
5. 默认参数 `page_snapshot`；
6. `page_snapshot`，`mode: "full"`，`max_nodes: 3000`。

百分位使用排序后最近秩：60 样本时 p50 为第 30 个，p95 为第 57 个。报告 p50 与 p95 而不只报告均值，单次慢调用才可见。测量是客户端到 daemon、mock 扩展和响应解析的端到端时间，不是扩展内部计时。

mock 的 250 ms 等待延迟不用于 latency 行，只用于 concurrency 行，并且必须写在该结果旁边。

## 8. 延迟结果

参考完整运行：

| 工具 | 样本 | p50 | p95 | 最大 | 目标 | 结论 |
|------|------|-----|-----|------|------|------|
| `browser_click` | 60 | 1 ms | 2 ms | 2 ms | 低于 50 ms | PASS |
| `browser_type` | 60 | 1 ms | 2 ms | 2 ms | 低于 50 ms | PASS |
| `page_snapshot` | 60 | 1 ms | 2 ms | 12 ms | 低于 300 ms | PASS |
| `page_snapshot` full，3,000 节点 | 60 | 1 ms | 2 ms | 2 ms | 低于 300 ms | PASS |

这是 mock 数字：把协议、daemon、调度和响应序列化从浏览器渲染中隔离出来。第 11 节的 live 冒烟补充真实扩展测量。mock 通过是必要但不充分条件；summary 表同时要求 mock 的 NFR-1 行和 live 快照行。

## 9. 并发与 MCP 客户端

concurrency 行在 fixture URL 创建 10 个标签页。它并行发送 10 个 `browser_wait`，每条带自己的 `tab_id`、选择器 `#delay` 和 8 秒超时。mock 配成 250 ms 时，正确的按标签页调度应在略高于 250 ms 完成，而不是约 2.5 秒。随后并行发送 10 个 `browser_navigate`。最后打开 10 个独立 MCP 会话，逐个 initialize，并各发一个 `browser_list`。

参考结果：

| 行 | 观察值 | 目标 | 结论 |
|----|--------|------|------|
| 10 个标签页上的 10 个 `browser_wait` | 10/10，273 ms | 全部成功，接近 250 ms | PASS |
| 10 个标签页上的 10 个 `browser_navigate` | 10/10，7 ms | 全部成功 | PASS |
| 10 个并发 MCP 会话 | 10/10 连接 | 10/10 | PASS |
| 10 个 `browser_list` | 10/10，5 ms | 10/10 | PASS |
| 会话打开墙钟 | 11 ms | 无重连风暴 | PASS |

273 ms 与 10 个并发的 250 ms 等待一致。若接近 2,500 ms，说明调度把不同标签页串行了，这是产品缺陷，不是 harness 波动。

## 10. 内存测量

memory 行用 `netstat -ano` 从监听 HTTP 端口解析守护进程的 Windows PID，每 2 秒通过 PowerShell 读取 `(Get-Process -Id PID).WorkingSet64`，以字节记录。第一个样本在 worker 启动前采集，空闲结束后一定再采最后一个样本。

负载配置：

- 900 秒混合负载；
- 3 个并发 worker；
- 每个 worker 在无在途调用时每 150 ms 发一次工具调用；
- 工具组合：`browser_list`、`tab_list`、`page_text`、`page_find`、`browser_click`、`browser_type`、`browser_wait`、`net_log`、`download_list`、`logs_tail`；
- worker 停止后 120 秒空闲。

组合覆盖 daemon 本地工具、页面感知、页面动作、网络读取、下载和日志读取，刻意不是单一热工具。调用成功与错误分别计数；工具错误降低成功率，不会中止运行。

参考结果：

| 项目 | 观察值 | 目标 | 结论 |
|------|--------|------|------|
| 负载时长 | 900 s | 900 s | PASS |
| 空闲尾段 | 120 s | 120 s | PASS |
| RSS 样本数 | 510 | 至少 500 | PASS |
| 尝试调用数 | 16,143 | 有代表性 | PASS |
| 调用错误 | 0 | 0 | PASS |
| 成功率 | 100% | 至少 99.9% | PASS |
| RSS 起始 | 9,428,992 | 仅参考 | PASS |
| RSS 结束 | 11,599,872 | 仅参考 | PASS |
| RSS 最大 | 11,796,480 | 仅参考 | PASS |
| 增长（结束减起始） | 2.07 MB | 低于 50 MB | PASS |
| 峰值增长（最大减起始） | 2.26 MB | 低于 50 MB | PASS |

这是 NFR-3 的 15 分钟代理，不是 72 小时浸泡，报告必须保留该区别。成功率覆盖所有尝试调用，包括空闲尾段的错误。约 18,000 次调用中的 1 次错误仍会四舍五入为 99.99%，但必须报告精确错误数，而不只是百分比。

## 11. 无头 Edge 冒烟

live 行调用已安装服务 `http://127.0.0.1:7331/mcp` 和专用无头 Edge 浏览器 `edge:b5e09e58ecdf3caa`。它在 fixture URL 创建一个活动标签页，验证 DOM 数量，并测量默认快照、full 快照、click 和 type 各 60 样本。标签页必须创建为 active：扩展的输入路径会等待渲染帧，而后台无头标签页可能暂停 `requestAnimationFrame`。这是无头后台标签页限制，不是产品 NFR 缺陷；用户的 active 标签页不受影响。

参考结果：

| 项目 | 观察值 | 目标 | 结论 |
|------|--------|------|------|
| DOM 元素 | 10,000 | 正好 10,000 | PASS |
| 默认快照节点数 | 2 | 仅供参考 | PASS |
| Full 快照节点数 | 3 | 仅供参考 | PASS |
| 默认快照 p95 | 22 ms | 低于 300 ms | PASS |
| Full 快照 p95 | 20 ms | 低于 300 ms | PASS |
| Click p95 | 4 ms | 低于 50 ms | PASS |
| Type p95 | 19 ms | 低于 50 ms | PASS |

live 节点数低于 3,000，是因为扩展 full 模式会经过 `isInteresting` 过滤。DOM 数量证明页面规模；第 8 节 mock 行证明 3,000 节点响应路径。两个事实都要报告，不要把 live 节点数当作失败或负载大小。

## 12. 体积测量

size 行测量本次运行使用的 release 守护进程二进制以及整个 `extension/` 目录树。体积使用十进制 MB（1,000,000 字节），与 NFR 预算单位一致。

参考结果：

| 产物 | 字节 | MB | 目标 | 结论 |
|------|------|----|------|------|
| release `corvusd.exe` | 6,334,464 | 6.33 | 低于 25 MB | PASS |
| `extension/` bundle | 232,885 | 0.23 | 低于 1 MB | PASS |

扩展统计 `extension/` 下所有文件，包括 README 和 popup 资源。debug 守护进程二进制不是 NFR-4 产物，不得写在这里。

## 13. 负向与边界矩阵

以下行证明 harness 会失败关闭，而不是误报通过。它们是测试 harness 检查，不是产品 API 错误。

| 场景 | 预期行为 | 证据 |
|------|----------|------|
| daemon 构建失败 | runner 打印构建日志并以非零退出 | `build-daemon.log` |
| fixture 服务器未打印 READY | runner 打印 fixture 日志并以非零退出 | `fixture.log` |
| 独立 daemon 未健康 | runner 打印 `daemon.log` 并以非零退出 | `daemon.log` |
| mock 未注册 `mock:perf` | runner 打印 `mock.log` 并以非零退出 | `mock.log` |
| 无法解析 daemon PID | 在内存采样前非零退出 | 运行输出 |
| fixture DOM 数量不是 10,000 | live 行显式断言失败 | `live.json` 缺失或失败 |
| live 服务或无头 Edge 不可用 | live 行非零退出，不产生部分 summary | 运行输出 |
| 任一 NFR 行未达标 | `summary.txt` 打印 MISS 并以 1 退出 | `summary.txt` |
| latency、concurrency 或 live 调用收到工具错误 | 该模式非零退出并停止后续模式；memory 模式改为记录错误并报告成功率 | 运行输出 |
| 端口 `31330`/`31331` 被占用 | daemon 启动或健康检查失败 | `daemon.log` |

summary 缺失、任一行 MISS，或最后一行不是 `RESULT: PASS 13 checks`，该次运行都不验收。不要用 sleep 或重试把瞬时失败变成通过；修复环境后重跑完整套。

## 14. 需要记录的证据

参考运行应保留整个临时运行根目录。其中 `evidence/` 目录包含：

- `latency.json`、`concurrency.json`、`memory.json`、`live.json`、`size.json`；
- `summary.txt`，含 NFR 表与最终结果。

构建与进程日志（`build-daemon.log`、`build-mock.log`、`daemon.log`、`fixture.log`、`mock.log`、`mock.jsonl`）位于同一运行根目录下、`evidence/` 旁边。runner stdout 会打印 `EVIDENCE <path>` 指针。

报告必须记录精确运行命令、UTC+8 起止时间、仓库提交、`corvusd` 版本与协议、Node 与 Rust 版本、Edge 版本、browser id、fixture URL 和证据路径。本文对应参考运行使用：

- 开始：2026-11-14 19:20:46 UTC+8；结束：2026-11-14 19:38:54 UTC+8；时长：18 分 8 秒；
- 仓库：HEAD 195f8fe5a16c1a28acde34534508f2dbca91d5f8，运行时工作区含未提交的产品与 QA 改动；
- daemon：corvusd 0.1.0，MCP 协议 2025-06-18，release 构建；
- 工具链：Node v24.21.0，cargo 1.99.1；
- live 浏览器：Edge 157.0.4341.46，browser id edge:b5e09e58ecdf3caa，无头端点 127.0.0.1:9333；
- fixture：http://127.0.0.1:56722/perf-10k.html；
- 证据：`C:/Users/Administrator/AppData/Local/Temp/corvus-perf.57cjxl/evidence`；
- 命令：`CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh`；
- summary：`RESULT: PASS 13 checks`。

原始证据包含绝对路径和本机浏览器标识，应作为内部 QA 证据，不要提交进仓库。

## 15. 通过标准与解读

性能通过要求全部满足：

1. summary 最后一行为 `RESULT: PASS 13 checks`；
2. mock latency 行 click 与 type p95 低于 50 ms；
3. mock latency 行 snapshot 与 snapshot-full p95 低于 300 ms；
4. 10 个并行等待和 10 个并行导航全部成功，等待墙钟接近一个 250 ms 延迟，而不是十倍；
5. 10 个 MCP 会话连接成功，10 个调用全部成功；
6. daemon RSS 增长低于 50 MB，混合负载成功率至少 99.9%；
7. release 二进制低于 25 MB，扩展 bundle 低于 1 MB；
8. live 行 DOM 正好 10,000，live full 快照 p95 低于 300 ms。

mock 数字不能替代真实浏览器交互质量。live 行是冒烟，不是完整浏览器基准：它只测一个页面、一个无头 Edge 构建和一个扩展构建。15 分钟内存行不是 72 小时浸泡。发布报告必须写明这两个限制。

若目标未达标，记录观察值、精确工具与参数、样本数和证据文件。看到结果后不得调整 fixture 或样本集。NFR 未达标是带数字的缺陷报告，不是放宽测试的理由。

## 16. 清理与故障排查

runner 的退出 trap 会杀掉 mock、fixture 服务器和独立 daemon，删除临时数据库及其 WAL/SHM，并打印证据路径。它绝不会停止或重启已安装的 Windows 服务。live 行会关闭自己的标签页。

常见修复：

| 现象 | 修复 |
|------|------|
| `cargo: command not found` | 设置 `CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe` |
| daemon 健康检查一直不通过 | 释放 `31330`/`31331`，或设置 `PERF_WS_PORT`/`PERF_HTTP_PORT` |
| fixture 服务器未启动 | 直接运行 `node tests/perf/serve-fixture.mjs` 并检查 `READY` 行 |
| live 行找不到浏览器 | 确认 7331 服务显示 `edge:b5e09e58ecdf3caa`，且 9333 的无头 Edge 正在运行 |
| live type 挂起 | 保持 live 标签页 active；不要在后台无头标签页测 `browser_type` |
| 内存 RSS 读取失败 | 确认 `netstat -ano` 解析出的 PID 是 `corvusd.exe` |
| summary 报 MISS | 检查对应 JSON 行，修复环境或产品后重跑全部模式 |

失败运行需保留诊断时，不要删除打印出的证据目录。要主动丢弃证据可设置 `PERF_KEEP_TMP=0`；验收运行必须保留。
