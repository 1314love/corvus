[English](M2a.md) | [中文](M2a.zh.md)

# Corvus M2a 验收手册

## 1. 目的

M2 wave 1 在四个 M1 工具之外新增十四个工具。本手册是该 18 工具表面的手工真实浏览器验收路径，
与自动化一致性测试互补：自动化测试用有状态 mock 证明协议和参数处理，本手册证明带 Corvus 扩展的
真实 Chromium 浏览器会产生文档所描述的结果。

先运行自动化测试：

```bash
bash tests/conformance/run.sh
```

然后在桌面会话中执行本手册。把每次请求、响应、延迟和截图都记录到证据目录。

## 2. 范围与冻结的工具名

四个 M1 工具保持不变：`browser_navigate`、`page_snapshot`、`browser_click`、
`browser_type`。

M2 wave 1 新增：

| 类别 | 工具 |
|---|---|
| 页面内容 | `page_text`、`page_html`、`page_find` |
| 指针与键盘 | `browser_hover`、`browser_press`、`browser_scroll`、`browser_select` |
| 页面执行 | `browser_eval`、`browser_wait` |
| 导航 | `browser_history` |
| 标签页 | `tab_list`、`tab_new`、`tab_close`、`tab_switch` |

页面工具和操作工具上的 `tab_id` 可选，省略时表示活动标签页；`tab_close` 和
`tab_switch` 必须提供。`target` 可以是 `page_snapshot` 或 `page_find` 返回的
`@eN` ref，也可以是 CSS 选择器。不要自行发明工具名、参数键或错误码。

当前超时策略：默认 60000 ms，上限 125000 ms。`browser_eval` 使用 30000 ms。
`browser_wait` 使用 `args.timeout_ms + 5000`，封顶 125000 ms，其自身
`timeout_ms` 默认 10000 ms。

## 3. 前置条件

- 带 Microsoft Edge 的 Windows 10 桌面会话。参考运行使用品牌版 Edge 157。
- 仓库位于 `D:/DSH/browser_bridege/corvus`。
- Rust 工具链位于 `C:/Users/Administrator/.cargo/bin`，并使用本地离线 registry。
  始终传入 `--offline`。
- Node.js 用于 fixture 服务器，curl 用于 MCP 调用。
- 一对空闲的 MCP 端口。默认 WebSocket 7330、HTTP 7331；本手册使用默认值，也可把
  任意空闲端口同时传给守护进程和客户端。
- 扩展目录 `extension/`，其根目录包含 `manifest.json`。

## 4. 构建并启动守护进程

在仓库根的 MSYS bash 中：

```bash
export PATH="/c/Users/Administrator/.cargo/bin:$PATH"
mkdir -p /tmp/corvus-m2a
export EVID=/tmp/corvus-m2a/$(date +%Y%m%d-%H%M%S)
mkdir -p "$EVID"

cargo build --offline -p corvusd --bin corvusd 2>&1 | tee "$EVID/build.log"
./target/debug/corvusd.exe serve > "$EVID/daemon.log" 2>&1 &
echo $! > "$EVID/daemon.pid"
```

如果默认端口被占用，在两侧同时加参数，例如
`serve --ws-port 17330 --http-port 17331`，并使用匹配的 MCP URL。

等待 MCP 端点就绪并保存 health 文档：

```bash
for i in $(seq 1 50); do
  curl -fsS http://127.0.0.1:7331/health -o "$EVID/health-before.json" && break
  sleep 0.2
done
cat "$EVID/health-before.json"
```

`/health` 必须报告 `version`、`protocol` 为 `2025-06-18`、`ws_addr`、
`http_addr` 和 `browsers` 数组。

在第二个 shell 中启动 fixture 服务器：

```bash
node tests/acceptance/fixtures/serve.mjs > "$EVID/fixture.log" 2>&1 &
echo $! > "$EVID/fixture.pid"
curl -fsS -o /dev/null -w 'fixture status=%{http_code}\n' http://127.0.0.1:8808/m2
```

M2 固定页面是 `http://127.0.0.1:8808/m2`（文件
`tests/acceptance/fixtures/m2-page.html`）。标签页和历史检查还会用到
`/m2-b` 与 `/m2-c`。

## 5. 加载扩展

1. 打开 `edge://extensions`。
2. 启用**开发人员模式**。
3. 点击**加载解压缩的扩展**，选择仓库的 `extension/` 目录，即包含
   `manifest.json` 的那一层。
4. 确认 Corvus 卡片存在且没有错误标记。记录实际扩展 ID。参考环境生成的是
   `pdgemoghkehbjpklbmkmchgfmpfghbl`；不同的解压路径通常会生成不同 ID。
5. 打开 service worker 控制台，确认有 hello / hello_ack 日志且没有未捕获异常。
   工具栏弹窗仅显示状态，状态点必须显示已连接。debugger 通道自动启用，无需切换开关。
6. 重新读取 `/health` 并保存为 `health-after.json`：

```bash
curl -fsS http://127.0.0.1:7331/health -o "$EVID/health-after.json"
cat "$EVID/health-after.json"
```

`browsers` 数组中必须出现该浏览器，其 `capabilities` 包含 `dom` 和
`scripting`，例如 `["dom","scripting"]`。

## 6. MCP 会话与辅助函数

初始化一次并保留原始响应：

```bash
MCP=http://127.0.0.1:7331/mcp
PROTO=2025-06-18

curl -sS -D "$EVID/init.headers" -o "$EVID/init.json" \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"m2a-acceptance","version":"1.0"}}}'

SESSION=$(awk -F': ' 'tolower($1)=="mcp-session-id"{gsub("\r","",$2); print $2}' "$EVID/init.headers")
echo "session=$SESSION"
```

响应必须包含 `protocolVersion` `2025-06-18`。如果守护进程返回 session id，后续调用
继续携带；当前守护进程是无状态的，可能不返回。

定义调用辅助函数。它保存响应体并打印客户端延迟（秒）：

```bash
call() {
  local name="$1"
  local body="$2"
  curl -sS -o "$EVID/$name.json" -w "$name %{time_total}\n" \
    -X POST "$MCP" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -H "MCP-Protocol-Version: $PROTO" \
    ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
    --data "$body"
}
```

发送 initialized 通知：

```bash
curl -sS -o /dev/null -w 'initialized status=%{http_code}\n' \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'
```

## 7. 基线：18 个工具、导航、快照

列出工具并断言精确数量和名称：

```bash
call tools-list '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); const names=r.result.tools.map(t=>t.name); console.log("count="+names.length); console.log(names.sort().join(","));' "$EVID/tools-list.json"
```

预期数量为 18。每个工具都必须有 `inputSchema` 和全部四个布尔 annotations。缺少或
多出名称都是契约缺陷。

导航到 fixture 并快照：

```bash
call navigate-m2 '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2"}}}'
call snapshot-m2 '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"mode":"interactive"}}}'
node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); const s=JSON.parse(r.result.content[0].text); console.log(s.url, s.title, s.node_count); for (const n of s.nodes) if (n.ref && n.tag) console.log(n.ref, n.tag, n.name || "");' "$EVID/snapshot-m2.json"
```

预期：最终 URL `http://127.0.0.1:8808/m2`，标题 `Corvus M2 Page`，
`node_count` 为正，且 fixture 控件有 ref。下面步骤使用命令实际打印的 ref。
占位符 `Q_REF`、`GO_REF`、`HOVER_REF`、`KEY_REF`、`PICK_REF`、
`SINGLE_REF`、`FIND_REF` 指代这些实际 ref。

时序陷阱：本地页面通常 `browser_navigate` 返回 `loaded=true`。如果紧接的
`page_snapshot` 仍显示旧文档，等待 500 ms 后重试一次。持续显示错误页面是缺陷；记录
navigate JSON 的 `loaded` 值和两次快照时间戳。

## 8. 页面内容工具

### page_text

```bash
call page-text '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：`url` 为 fixture URL，`title` 为 `Corvus M2 Page`，`text`
包含 `Corvus M2` 和 `M2 acceptance fixture`，`truncated` 为 false。
再用 `{"max_chars":20}` 调用一次：结果非空、`truncated=true`，且调用仍是成功。

### page_html

```bash
call page-html '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"page_html","arguments":{"target":"#main"}}}'
```

通过标准：`html` 以 `<main id="main">` 开头（或返回片段开头包含它），包含
`Corvus M2`，`truncated` 为 false。再用 `{"max_chars":50}` 调用，必须
`truncated=true`。不传 target 时返回整个 `documentElement`，该调用可以很大。

### page_find 与共享 ref

```bash
call page-find '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"page_find","arguments":{"query":"Find target","limit":5}}}'
```

通过标准：`count` 至少为 1 且不超过 `limit`；每个 match 都有 `ref`、
`tag`、`role`、`text`；至少一个 match 是 `Find target` 按钮。把它的
`ref` 填入 `FIND_REF` 并点击：

```bash
call find-click '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FIND_REF"}}}'
call find-result '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

点击必须返回 `clicked=true`，页面文本必须包含 `find target clicked`。
这证明 `page_find` ref 与 `page_snapshot` 位于同一文档 ref 空间，且
`browser_click` 接受它。

负向检查：`page_find` 缺少 `query`、`limit` 为 0 或 101、`regex:true`
模式非法，都必须返回结构化 BAD_ARGS。查询无匹配是成功且 `count=0`，不是错误。

## 9. 交互工具

### browser_hover

```bash
call hover '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"browser_hover","arguments":{"target":"#hover"}}}'
call hover-text '{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：`hovered=true`，`url` 为 fixture URL，`target` 包含
`tag:"button"`，最好有 `text:"Hover me"`。后续页面文本必须包含
`hover event received`，证明 mouseover 处理器已运行。缺少 `target` 为 BAD_ARGS。

### browser_press

```bash
call press '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"browser_press","arguments":{"keys":["Enter","Tab"],"target":"#keyinput"}}}'
call press-text '{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：`pressed=true`；`keys` 回显 `["Enter","Tab"]`；`url` 为
fixture URL。fixture 记录 keydown 事件，因此页面文本必须包含
`pressed:Enter,Tab`。再尝试 chord，例如
`{"keys":"Control+A","target":"#keyinput"}`：结果应报告规范化后的 chord。
空 `keys` 为 BAD_ARGS。

### browser_scroll

```bash
call scroll-down '{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"browser_scroll","arguments":{"direction":"down","pages":1}}}'
call scroll-up '{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"browser_scroll","arguments":{"direction":"up","pages":1}}}'
```

通过标准：`scrolled=true`；`x`、`y` 为数字；在这个高页面上下滚后 `y`
大于 0；上滚后 `y` 减小。`pages:0` 只报告当前偏移，不移动。方向不是 up、
down、left、right 时为 BAD_ARGS。

### browser_select

```bash
call select-multi '{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"browser_select","arguments":{"target":"#pick","values":["alpha","beta"]}}}'
call select-single '{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"browser_select","arguments":{"target":"#single","values":["two"]}}}'
```

通过标准：多选结果 `selected` 包含 `alpha` 和 `beta`，`value` 为
`alpha`（第一个选中项）；单选结果 `selected:["two"]`、`value:"two"`。
两者都返回 fixture `url`。空 `values` 为 BAD_ARGS；不存在的 option 值为
NOT_FOUND；单选元素传两个值为 BAD_ARGS。

## 10. browser_eval、browser_wait 与 browser_history

### browser_eval

```bash
call eval-title '{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"document.title"}}}'
call eval-sum '{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"1+1"}}}'
call eval-throw '{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"(() => { throw new Error(\"m2 boom\") })()"}}}'
call eval-cyclic '{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"(() => { const x = {}; x.self = x; return x; })()"}}}'
```

通过标准：

- title 结果为 `{"ok":true,"value":"Corvus M2 Page"}`。
- 求和结果为 `{"ok":true,"value":2}`；`value` 保持数字类型。
- 页面抛错是成功的工具调用，结果为
  `{"ok":false,"error":{"name":"Error","message":"m2 boom"}}`；`isError`
  必须为 false，envelope 不得有 `error` 字段。
- 循环对象返回 `{"ok":true,"value":null,"value_note":"unserializable"}`。
- `world` 默认 `isolated`；`world:"main"` 也应返回标题。`world:"server"`
  为 BAD_ARGS。该工具超时为 30000 ms。

### browser_wait

```bash
call wait-reload '{"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"reload"}}}'
call wait-late '{"jsonrpc":"2.0","id":35,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#late","timeout_ms":5000}}}'
call wait-selector '{"jsonrpc":"2.0","id":36,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#h","timeout_ms":5000}}}'
call wait-text '{"jsonrpc":"2.0","id":37,"method":"tools/call","params":{"name":"browser_wait","arguments":{"text":"M2 acceptance fixture","timeout_ms":5000}}}'
call wait-url '{"jsonrpc":"2.0","id":38,"method":"tools/call","params":{"name":"browser_wait","arguments":{"url_contains":"/m2","timeout_ms":5000}}}'
call wait-timeout '{"jsonrpc":"2.0","id":39,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#never","timeout_ms":100}}}'
```

通过标准：`wait-late` 在 reload 后立即调用，此时 `#late` 仍隐藏；必须返回
`matched=true` 且 `waited_ms > 0`（fixture 约 1200 ms 后显示）。若返回 0 ms，
说明 reload 或轮询可疑，记录并重试一次。其余正常调用返回 `matched=true`、非负 `waited_ms` 和 fixture
`url`；`waited_ms` 小于请求的超时。超时调用返回结构化 TIMEOUT 错误，耗时约为
100 ms 加传输开销；MCP 错误文本只含 `CODE: message`，`ErrorBody.details.waited_ms`
保留在信封上，不要在 MCP 结果中强制要求。没有条件为 BAD_ARGS。守护进程
有效超时为 `args.timeout_ms + 5000` 且封顶 125000 ms，因此 5000 ms 的等待上限为
10000 ms。

### browser_history

```bash
call navigate-b '{"jsonrpc":"2.0","id":40,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2-b"}}}'
call history-back '{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"back"}}}'
call history-forward '{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"forward"}}}'
call history-reload '{"jsonrpc":"2.0","id":43,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"reload"}}}'
```

通过标准：back 返回 `action:"back"`、URL `/m2`、`loaded:true`；forward
返回 URL `/m2-b`、`loaded:true`；reload 保持当前 URL 且 `loaded:true`。
动作不是 back、forward、reload 时为 BAD_ARGS。新标签页没有历史时 back 为 NOT_FOUND。

## 11. 标签页

```bash
call tabs-before '{"jsonrpc":"2.0","id":50,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
call tab-new '{"jsonrpc":"2.0","id":51,"method":"tools/call","params":{"name":"tab_new","arguments":{"url":"http://127.0.0.1:8808/m2-c"}}}'
```

从 `tab-new.json` 读取 `tab_id` 记为 `NEW_TAB`，从 `tabs-before.json`
读取一个之前的 tab id 记为 `OLD_TAB`。

```bash
call tabs-after-new '{"jsonrpc":"2.0","id":52,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
call tab-switch '{"jsonrpc":"2.0","id":53,"method":"tools/call","params":{"name":"tab_switch","arguments":{"tab_id":"NEW_TAB"}}}'
call tab-close '{"jsonrpc":"2.0","id":54,"method":"tools/call","params":{"name":"tab_close","arguments":{"tab_id":"NEW_TAB"}}}'
call tabs-after-close '{"jsonrpc":"2.0","id":55,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
```

通过标准：`tab_list` 返回 `tabs`，每项含字符串 `tab_id`、数字 `index`、
布尔 `active`、`url`、`title`、`status`；恰好一个 tab 为 active。
`tab_new` 返回 `tab_id`、`url`、`loaded`、`previous_tab_id`；新标签页
出现在 `tab_list`，当 `active` 为 true 时成为活动页。`tab_switch` 返回请求的
`tab_id`、其 `url`、`title` 和 `active:true`。`tab_close` 返回
`closed:true` 和 `tab_id`；仅当关闭的是活动标签页且相邻标签页被激活时才有
`next_tab_id`。关闭后 `tab_list` 不再包含该标签页。

负向检查：`tab_close` 或 `tab_switch` 缺少 `tab_id` 为 BAD_ARGS；未知 id
为 NOT_FOUND；`tab_new` 的 `active` 不是布尔值为 BAD_ARGS。之后使用已关闭的
tab id 是 NOT_FOUND 或 BROWSER_GONE，不能是裸字符串。

## 12. 负向与错误路径矩阵

每个失败都必须是结构化的 MCP 工具结果：HTTP 200，`result.isError=true`，
`content[0].text` 以 `CODE: message` 开头；或者传输失败使用文档规定的 MCP
错误形状。JSON-RPC 层不得把工具失败变成 `error` 对象。

| 工具 | 参数 | 预期 |
|---|---|---|
| 未知工具 `corvus_missing_tool` | 任意 | BAD_ARGS |
| `page_text` | `{"max_chars":0}` | BAD_ARGS |
| `page_text` | `{"max_chars":200001}` | BAD_ARGS |
| `page_html` | `{"max_chars":500001}` | BAD_ARGS |
| `page_find` | `{}` | BAD_ARGS |
| `page_find` | `{"query":"x","limit":101}` | BAD_ARGS |
| `page_find` | `{"query":"[","regex":true}` | BAD_ARGS |
| `browser_hover` | `{}` | BAD_ARGS |
| `browser_press` | `{"keys":[]}` | BAD_ARGS |
| `browser_press` | `{"keys":"Hyper+Q"}` | BAD_ARGS |
| `browser_scroll` | `{"direction":"diagonal"}` | BAD_ARGS |
| `browser_select` | `{"target":"#pick","values":[]}` | BAD_ARGS |
| `browser_select` | `{"target":"#pick","values":["missing"]}` | NOT_FOUND |
| `browser_eval` | `{"expression":"1","world":"server"}` | BAD_ARGS |
| `browser_wait` | `{}` | BAD_ARGS |
| `browser_wait` | `{"selector":"#never","timeout_ms":100}` | TIMEOUT；记录耗时，details 保留在信封 |
| `browser_history` | `{"action":"refresh"}` | BAD_ARGS |
| `tab_new` | `{"active":"yes"}` | BAD_ARGS |
| `tab_close` | `{}` | BAD_ARGS |
| `tab_close` | `{"tab_id":"tab-999"}` | NOT_FOUND |
| `tab_switch` | `{}` | BAD_ARGS |
| 禁用扩展 | 任意页面工具 | TRANSPORT_DOWN |

停用扩展后调用 `page_text` 必须返回 TRANSPORT_DOWN，且守护进程不得崩溃。重新启用扩展，
确认 /health 再次列出该浏览器后再继续。

## 13. 证据记录

每次运行创建一个目录。保留原始响应体，不要只保留摘要。

- `build.log`、`daemon.log`、`fixture.log`
- `health-before.json`、`health-after.json`
- `init.headers`、`init.json`、`tools-list.json`
- 每次正常调用和负向调用各一个 `<tool>.json`
- 辅助函数输出的 `curl` `time_total` 行，或带时间戳的延迟文件
- 截图：扩展页、已连接的弹窗、交互前的 fixture、type/click 后、scroll 后、
  tab switch 后
- 实际扩展 ID、浏览器 ID、浏览器版本、守护进程版本和协议版本

只有结果 JSON 不够：必须能追溯到工具名、参数、延迟，以及证明效果的截图或页面文本。

## 14. 各工具通过标准

延迟是本地页面上客户端 `curl %{time_total}` 的单次值，浏览器在前台。此 fixture 上
任何交互工具超过 3000 ms 即硬失败；重复运行时记录分布。

| 工具 | 通过标准 | 预算 |
|---|---|---|
| `page_text` | URL/标题/文本正确；截断如实 | <= 1000 ms |
| `page_html` | target 片段正确；截断如实 | <= 1000 ms |
| `page_find` | count/matches 正确；返回的 ref 可点击该元素 | <= 1000 ms |
| `browser_hover` | `hovered=true`；target 的 tag/text 正确；mouseover 生效 | <= 1000 ms |
| `browser_press` | `pressed=true`；keys 回显；keydown 生效 | <= 1000 ms |
| `browser_scroll` | `scrolled=true`；偏移按请求方向变化 | <= 1000 ms |
| `browser_select` | selected 与 value 正确 | <= 1000 ms |
| `browser_eval` | value 类型正确；页面抛错为 `ok=false`；循环对象不可序列化 | <= 1000 ms |
| `browser_wait` | 条件匹配；超时为结构化 TIMEOUT（记录耗时） | <= waited + 500 ms |
| `browser_history` | action、URL、loaded 正确 | <= 2000 ms |
| `tab_list` | 列出全部标签页，恰好一个 active，形状正确 | <= 2000 ms |
| `tab_new` | 新 tab id、URL、previous id、loaded 正确 | <= 2000 ms |
| `tab_switch` | 请求的标签页成为 active，URL/title 正确 | <= 2000 ms |
| `tab_close` | closed 为 true、id 正确、列表反映移除 | <= 2000 ms |
| 错误路径 | 结构化 code 与 message；守护进程健康 | <= 60000 ms 默认 |

出现以下任一情况即硬失败：工具返回成功但页面没有变化；错误未结构化或是裸字符串；错误码不在
九码分类内；工具名缺失或多出；结果类型错误；守护进程崩溃。

## 15. 清理与故障排查

```bash
kill "$(cat "$EVID/daemon.pid")" 2>/dev/null || true
kill "$(cat "$EVID/fixture.pid")" 2>/dev/null || true
```

保留证据目录并随报告附上。

| 症状 | 检查 |
|---|---|
| MCP 连接被拒绝 | 守护进程日志；`--http-port` 是否正确；`/health` |
| `browsers` 为空 | 扩展是否加载、service worker 是否运行、控制台是否有 hello |
| 所有工具 CAPABILITY_UNAVAILABLE | /health capabilities 缺少 `dom`；重新加载扩展 |
| page_snapshot refs 变化 | 文档过期或刷新；重新取快照 |
| `page_find` 没有 ref | 查询大小写或可见性；换更简单的子串 |
| `browser_press` 无效果 | 焦点 target；检查按键语法和修饰键顺序 |
| `browser_wait` 提前超时 | fixture 是否在 8808；selector/text 是否可见 |
| `browser_eval` 抛 TypeError | 使用完整表达式或 async IIFE |
| 标签页工具失败 | 扩展 tab 权限；活动窗口；browser id 是否仍连接 |

通过上述检查仍失败即为缺陷。记录精确请求体、响应体、工具名、参数、时间戳、延迟和截图；
验收期间不要修改产品代码。
