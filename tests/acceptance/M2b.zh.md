[English](M2b.md) | [中文](M2b.zh.md)

# Corvus M2b 验收手册

## 1. 目的

M2b 在 M2 wave 1 的 18 个工具之上新增八个网络、下载与上传工具，冻结表面为 26 个工具。
本手册是这些工具的手工真实浏览器验收路径。自动化一致性测试用有状态 mock 证明协议、参数与
路由行为；本手册证明带 Corvus 扩展的真实 Chromium 浏览器能够捕获真实请求、阻断与模拟请求、
下载真实文件，并把真实文件附加到文件输入框。

先在隔离端口上运行自动化测试：

```bash
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe \
CORVUSD_BIN=./target/debug/corvusd.exe \
CORVUSD_ARGS="serve --ws-port 29330 --http-port 29331" \
CORVUS_MCP_URL=http://127.0.0.1:29331/mcp \
CORVUS_WS_URL=ws://127.0.0.1:29330 \
bash tests/conformance/run.sh
```

预期结果为 108 passed、0 failed、0 skipped。然后在桌面会话中执行本手册，把每次请求、响应、
延迟和截图记录到证据目录。

## 2. 范围、路由与冻结的工具名

18 个 M2 wave 1 工具保持不变：

| 类别 | 工具 |
|---|---|
| 页面内容 | `page_text`、`page_html`、`page_find` |
| 指针与键盘 | `browser_hover`、`browser_press`、`browser_scroll`、`browser_select` |
| 页面执行 | `browser_eval`、`browser_wait` |
| 导航与标签页 | `browser_navigate`、`page_snapshot`、`browser_click`、`browser_type`、`browser_history`、`tab_list`、`tab_new`、`tab_close`、`tab_switch` |

M2b 新增八个工具：

| 类别 | 工具 |
|---|---|
| 浏览器注册表 | `browser_list` |
| 网络 | `net_capture`、`net_log`、`net_clear`、`net_intercept` |
| 下载 | `download_list`、`download_wait` |
| 上传 | `upload_file` |

每个工具的输入 schema 都有可选的字符串 `browser_id`。省略时，如果只连接一个浏览器就使用
该浏览器；如果没有浏览器则返回 TRANSPORT_DOWN；如果连接两个或更多浏览器则返回 BAD_ARGS，
并带 `details.reason="multiple_browsers"` 与 `details.browser_ids`。显式传入未知 id 会返回
TRANSPORT_DOWN，并带 `details.browser_id`。`browser_list` 由守护进程注册表直接回答，不需要
扩展往返。

`tab_id` 是 JSON 字符串；数字 JSON 值（例如 `999`）为 BAD_ARGS。未知字符串（例如
`"tab-999"`）会原样传递并返回 NOT_FOUND，错误文本携带 `details.tab_id`。`target` 可以是
`page_snapshot` 或 `page_find` 返回的 `@eN` ref，也可以是 CSS 选择器。

超时：所有 M2b 工具使用 60000 ms。`download_wait` 使用
`min(args.timeout_ms + 5000, 125000)`；其自身 `timeout_ms` 默认 30000 ms，因此默认预算为
35000 ms。不要自行发明工具名、参数键或错误码。

## 3. 前置条件

- 带 Microsoft Edge 的 Windows 10 桌面会话。参考运行使用品牌版 Edge 157。
- 仓库位于 `D:/DSH/browser_bridege/corvus`。
- Rust 工具链位于 `C:/Users/Administrator/.cargo/bin`，并使用本地离线 registry。
  始终传入 `--offline`。
- Node.js 用于 fixture 服务器与 JSON 编码，curl 用于 MCP 调用，MSYS/Git for Windows
  shell 中的 `base64` 与 `cygpath`。
- 一对空闲的 MCP 端口。默认 WebSocket 7330、HTTP 7331；本手册使用默认值，也可把任意
  空闲端口同时传给两侧。
- 扩展目录 `extension/`，其根目录包含 `manifest.json`。
- 仓库之外的可写临时目录，用于上传文件。

## 4. 构建并启动守护进程

在仓库根的 MSYS bash 中：

```bash
export PATH="/c/Users/Administrator/.cargo/bin:$PATH"
mkdir -p /tmp/corvus-m2b
export EVID=/tmp/corvus-m2b/$(date +%Y%m%d-%H%M%S)
mkdir -p "$EVID"

cargo build --offline -p corvusd --bin corvusd 2>&1 | tee "$EVID/build.log"
./target/debug/corvusd.exe serve > "$EVID/daemon.log" 2>&1 &
echo $! > "$EVID/daemon.pid"
```

如果默认端口被占用，在两侧同时加参数，例如
`serve --ws-port 29330 --http-port 29331`，并使用匹配的 MCP URL。

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
for path in /m2b /api/json /api/slow /download/report.txt; do
  curl -sS -o /dev/null -w "$path status=%{http_code}\n" "http://127.0.0.1:8808$path"
done
```

M2b 固定页面是 `http://127.0.0.1:8808/m2b`（文件
`tests/acceptance/fixtures/m2b-page.html`）。标题为 `Corvus M2b Page`，标记为
`M2b fixture marker 5b21`，包含按钮 `#fetch-slow`、`#fetch-json`，输出 `#net-out`，
下载链接 `#download`，以及接受 `.txt,text/plain` 的隐藏文件输入 `#file`。fixture 还提供
`/api/json`（立即返回 JSON `{"source":"json","value":7}`）与 `/api/slow`（600 ms 后
返回同形状结果，`source:"slow"`、`value:42`），以及以附件
`corvus-report.txt` 提供的 `/download/report.txt`。M2a 的 `/m2`、`/m2-b`、
`/m2-c`、`/m2-csp` 路由仍然可用。

## 5. 加载扩展

1. 打开 `edge://extensions`。
2. 启用**开发人员模式**。
3. 点击**加载解压缩的扩展**，选择仓库的 `extension/` 目录，即包含
   `manifest.json` 的那一层。
4. 确认 Corvus 卡片存在且没有错误标记。记录实际扩展 ID。参考环境生成的是
   `pdgemoghkehbjpklbmkmchgfmpfghbl`。
5. 打开 service worker 控制台，确认有 hello / hello_ack 日志且没有未捕获异常。
   工具栏弹窗仅显示状态，状态点必须显示已连接。debugger 与网络通道自动启用。
6. 重新读取 `/health` 并保存为 `health-after.json`，确认浏览器出现且
   `capabilities` 包含 `dom` 与 `scripting`。

## 6. MCP 会话与 helper

初始化一次并保留原始响应：

```bash
MCP=http://127.0.0.1:7331/mcp
PROTO=2025-06-18

curl -sS -D "$EVID/init.headers" -o "$EVID/init.json" \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"m2b-acceptance","version":"1.0"}}}'

SESSION=$(awk -F': ' 'tolower($1)=="mcp-session-id"{gsub("\r","",$2); print $2}' "$EVID/init.headers")
echo "session=$SESSION"
```

发送 `notifications/initialized` 并定义后续步骤使用的 helper：

```bash
curl -sS -o /dev/null -w 'initialized status=%{http_code}\n' \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H "MCP-Protocol-Version: $PROTO" \
  ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'

call() {
  local name="$1" body="$2"
  curl -sS -o "$EVID/$name.json" -w "$name %{time_total}\n" \
    -X POST "$MCP" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -H "MCP-Protocol-Version: $PROTO" \
    ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
    --data "$body"
}
```

## 7. 基线：26 个工具与 M2b 页面

```bash
call tools-list '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
call browser-list-one '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"browser_list","arguments":{}}}'
call m2b-navigate '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2b"}}}'
call m2b-snapshot '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"mode":"interactive"}}}'
```

通过标准：`tools-list` 恰好包含 26 个冻结名称，每个输入 schema 都有可选的字符串
`browser_id`。`browser-list-one` 返回 `browsers` 数组，其中一项包含 `browser_id`、
`profile`、`version`、`capabilities`、`connected_at_ms`，并按 `connected_at_ms`
升序排序。导航结果包含 `navigated:true`、`loaded:true` 与 `/m2b` URL。快照标题为
`Corvus M2b Page`，并包含 `#fetch-json`、`#fetch-slow`、`#download` 的 ref；隐藏的
`#file` 可能不在交互模式快照中，这是预期行为。

## 8. 网络捕获与日志

```bash
call capture-start '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"start"}}}'
call fetch-json '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"page_find","arguments":{"query":"Fetch JSON"}}}'
```

从 `fetch-json.json` 读取 ref 作为 `FETCH_REF`，将命令中的字面量 `FETCH_REF` 替换为该值，点击它以产生请求，然后检查捕获：

```bash
call fetch-click '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call fetch-wait '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"browser_wait","arguments":{"text":"json:7","timeout_ms":5000}}}'
call capture-status '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"status"}}}'
call net-log-api '{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"net_log","arguments":{"url_contains":"/api/","limit":20}}}'
call net-log-headers '{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"net_log","arguments":{"url_contains":"/api/json","include_headers":true,"limit":5}}}'
```

通过标准：`capture-start` 返回 `capturing:true`、活动 `tab_id`、`started_at` 与
`capacity:2000`。请求之后 `capture-status` 报告 `capturing:true` 与 `entries>0`。
`net-log-api` 返回按 `seq` 升序排序的条目，含 `seq`、`ts`、`tab_id`、
`request_id`、`method`、`url`、`status`、`mime`、`resource_type`、
`from_cache`、`duration_ms`、`error` 与 `intercepted`；`/api/json` 条目为 HTTP
200、方法 `GET`，且 `duration_ms` 为数字。只有 `net-log-headers` 带请求/响应头映射，
且 `next_seq` 等于最后返回的 `seq`。

然后清空并停止：

```bash
call net-clear '{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"net_clear","arguments":{}}}'
call net-log-empty '{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"net_log","arguments":{}}}'
call capture-stop '{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"stop"}}}'
```

`net-clear` 返回的 `cleared` 等于之前的条目数。下一次 `net_log` 的
`entries:[]` 且 `next_seq:0`。`capture-stop` 返回 `capturing:false`、tab id 与剩余
条目数。

## 9. 请求拦截

捕获必须对该标签页开启。设置标签页级 block 规则并点击 `#fetch-json`：

```bash
call intercept-block '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"set","rules":[{"id":"block-json","action":"block","url_regex":"/api/json"}]}}}'
call intercept-block-click '{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call intercept-block-text '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：`intercept-block` 返回 `ok:true`、`scope:"tab"`、回显的规则与
`applied:true`；id 为字符串。点击后页面 `#net-out` 显示 `network failed: ...`，其
`data-net` 为 `failed`。该请求的 `net_log` 条目 `status:null`、
`error:"BlockedByClient"`、`intercepted.action:"block"`；页面本身保持可响应。

现在把规则替换为 mock。响应体是 `{"ok":true,"source":"mock","value":99}` 的 base64：

```bash
BODY=$(printf '%s' '{"ok":true,"source":"mock","value":99}' | base64 -w0)
MOCK_RULE=$(node -e 'console.log(JSON.stringify({action:"set",rules:[{id:"mock-json",action:"mock",url_regex:"/api/json",status:200,headers:{"content-type":"application/json"},body_base64:process.argv[1]}]}))' "$BODY")
call intercept-mock "{\"jsonrpc\":\"2.0\",\"id\":23,\"method\":\"tools/call\",\"params\":{\"name\":\"net_intercept\",\"arguments\":$MOCK_RULE}}"
call intercept-mock-click '{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call intercept-mock-text '{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：set 回显规则 `mock-json`；点击后 `#net-out` 显示 `mock:99`，且
`data-net="mock"`。`net_log` 显示 200 响应，`intercepted.action:"mock"` 与
`rule_id:"mock-json"`。清除规则：

```bash
call intercept-clear '{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"clear"}}}'
call intercept-final-click '{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
```

清除后点击会再次到达真实端点，`#net-out` 回到 `json:7`。规则按 first-match-wins，
set 会替换整个作用域。

`action:"set"` 的响应只确认规则已安装，无法报告请求进行中才发生的 Fetch 动作失败。
若请求没有按预期被 block 或 mock，请检查 daemon 与扩展日志中的
`intercept action failed`；扩展会回退为继续该请求。将其记录为 S3 可观测性限制，
而不是错误结构缺陷。

## 10. 下载

```bash
call download-list-before '{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"download_list","arguments":{"limit":20}}}'
call download-click '{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"DOWNLOAD_REF"}}}'
```

从基线快照读取 `DOWNLOAD_REF`，将命令中的字面量 `DOWNLOAD_REF` 替换为该值。等待最新下载：

```bash
call download-wait '{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"download_wait","arguments":{"timeout_ms":20000}}}'
call download-list-after '{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"download_list","arguments":{"limit":20}}}'
```

通过标准：`download_list` 按最新优先返回，含 `id`、`filename`、`url`、
`state`、`bytes_received`、`total_bytes`、`danger`、`exists` 与
`started_at`。`download_wait` 返回新条目，`state:"complete"`、
`filename:"corvus-report.txt"`（链接的 download 属性）、`bytes_received>0` 与
`total_bytes>0`。`id` 是列表中的真实下载 id。MCP schema 要求 `id` 为字符串，因此
请把数字下载 id 转成十进制字符串传入（例如 `"42"`）。再用该显式 id 运行一次
`download_wait`，应立即解析。非十进制值（如 `"no-such-download"`）为 BAD_ARGS。
未知的十进制 id 不是 NOT_FOUND：它会等待直到超时，并返回 TIMEOUT，带
`details.waited_ms` 与 `details.id`。若下载在 `timeout_ms+5000` 内未完成，则为
TIMEOUT，并带 `details.waited_ms`。

## 11. 上传

在仓库之外创建一个小的文本文件，并读取其 Windows 路径：

```bash
printf 'corvus upload fixture\n' > "$EVID/upload.txt"
UPLOAD_WIN=$(cygpath -w "$EVID/upload.txt")
echo "$UPLOAD_WIN"
```

用 Node 构建 JSON 请求体，以正确转义反斜杠，然后通过 CSS 选择器对隐藏输入框调用工具：

```bash
UPLOAD_BODY=$(node -e 'console.log(JSON.stringify({target:"#file",files:[process.argv[1]]}))' "$UPLOAD_WIN")
call upload-file "{\"jsonrpc\":\"2.0\",\"id\":40,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_file\",\"arguments\":$UPLOAD_BODY}}"
call upload-files '{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"Array.from(document.querySelector(\"#file\").files).map(f=>f.name)"}}}'
call upload-text '{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

通过标准：`upload-file` 返回 `uploaded:true`、`files` 中的绝对路径，以及
`target.tag:"input"`，`input.accept` 含 `.txt`、`input.multiple:false`。
`upload-files` 返回 `{"ok":true,"value":["upload.txt"]}`，证明 `input.files` 已填充。
`page_text` 包含 `files:upload.txt`。非文件输入框目标返回 BAD_ARGS，并带
`details.reason:"not_a_file_input"`；缺失路径返回 BAD_ARGS，并带 `details.path`。
相对路径为 BAD_ARGS；API 要求绝对路径。

## 12. 多浏览器路由

`browser_list` 证明注册表。只有一个浏览器时，单条结果即可；要验证路由，在第二个浏览器
配置文件或窗口中加载同一个解压扩展，使 `/health` 列出两个浏览器。记录两个
`browser_id`。

- 省略 `browser_id`：BAD_ARGS，`details.reason:"multiple_browsers"`，
  `details.browser_ids` 列出两个 id。
- 传入第一个 `browser_id`：调用针对该浏览器成功。
- 传入伪造 id：TRANSPORT_DOWN，并带 `details.browser_id`。
- `browser_list` 仍然本地回答，并按 `connected_at_ms` 升序列出两个浏览器。

除非环境确实有两个浏览器，否则不要测试双浏览器；把单浏览器情况记为注册表 PASS，
把歧义情况记为 SKIP。

## 13. 负向与错误路径矩阵

每个失败必须是 HTTP 200 且 `result.isError=true`，`content[0].text` 以
`CODE: message` 开头；details 作为尾随 JSON 对象附加。工具级失败若变成 JSON-RPC
`error`，即为缺陷。

| 工具 | 参数 | 预期 |
|---|---|---|
| 未知工具 `corvus_missing_tool` | 任意 | BAD_ARGS |
| `net_capture` | `{"action":"pause"}` | BAD_ARGS |
| `net_log` | `{"limit":0}` | BAD_ARGS |
| `net_log` | `{"limit":1001}` | BAD_ARGS |
| `net_intercept` | `{"action":"set"}` | BAD_ARGS |
| `net_intercept` | 规则 `id` 为数字 `1` | BAD_ARGS |
| `net_intercept` | 两条规则 id 相同 | BAD_ARGS，`details.rule_id` |
| `net_intercept` | 规则 `url_regex:"["` | BAD_ARGS，`details.rule_id` |
| `download_list` | `{"limit":201}` | BAD_ARGS |
| `download_wait` | `{"id":"no-such-download"}` | BAD_ARGS（id 必须是十进制字符串） |
| `download_wait` | `{"id":"999999999","timeout_ms":100}` | TIMEOUT，`details.waited_ms` 与 `details.id` |
| `download_wait` | `{"timeout_ms":100}` 且没有新下载 | TIMEOUT，`details.waited_ms` |
| `upload_file` | `{"target":"#file","files":[]}` | BAD_ARGS |
| `upload_file` | 11 个路径 | BAD_ARGS |
| `upload_file` | `{"target":"#net-out","files":["C:/Windows/win.ini"]}` | BAD_ARGS，`not_a_file_input` |
| `upload_file` | 不存在的路径 | BAD_ARGS，`details.path` |
| `upload_file` | 相对路径 | BAD_ARGS |
| `page_text` | 未知 `browser_id` | TRANSPORT_DOWN，`details.browser_id` |
| `page_text` | 两个浏览器时省略 `browser_id` | BAD_ARGS，`multiple_browsers` |
| `tab_switch` | `{"tab_id":"tab-999"}` | NOT_FOUND，`details.tab_id` |
| `tab_switch` | `{"tab_id":999}` | BAD_ARGS（必须是字符串） |
| 扩展被禁用 | 任意页面工具 | TRANSPORT_DOWN |

禁用扩展不得使守护进程崩溃。重新启用后确认 `/health` 再次列出浏览器，然后继续。

## 14. 证据记录

- `build.log`、`daemon.log`、`fixture.log`
- `health-before.json`、`health-after.json`、`init.headers`、`init.json`、
  `tools-list.json`
- `browser-list-one.json`；存在两个浏览器时还要
  `browser-list-two.json`、`route-ambiguous.json`、`route-unknown.json`
- `capture-start.json`、`capture-status.json`、`capture-stop.json`
- `net-log-api.json`、`net-log-headers.json`、`net-log-empty.json`、
  `net-clear.json`
- `intercept-block.json`、`intercept-block-click.json`、
  `intercept-block-text.json`、`intercept-mock.json`、
  `intercept-mock-text.json`、`intercept-clear.json`
- `download-list-before.json`、`download-click.json`、
  `download-wait.json`、`download-list-after.json`
- `upload-file.json`、`upload-files.json`、`upload-text.json`
- 每个负向调用一个 JSON
- 截图：扩展卡片、弹窗已连接、交互前 `/m2b`、`network failed` 状态、
  `mock:99` 状态、上传 `files:` 状态、浏览器下载页中的下载条目
- 每次调用的客户端 `curl %{time_total}` 行
- 实际扩展 ID、浏览器 ID、守护进程版本与协议版本

仅有结果 JSON 不够：必须能追溯到工具名、参数、延迟以及证明效果的文字或截图。

## 15. 各工具通过标准

延迟是本地页面上单次运行的客户端 `curl %{time_total}`。本 fixture 上任何 M2b 调用超过
3000 ms 为硬失败，`download_wait` 与 `net_capture` 按其自身预算判断。

| 工具 | 通过标准 | 预算 |
|---|---|---|
| `browser_list` | 数组形状，按 `connected_at_ms` 排序；零、一、两个浏览器均正确 | <= 1000 ms |
| `net_capture` | start/status/stop 形状；条目数增长；capacity 2000 | <= 1000 ms |
| `net_log` | 过滤、排序、limit、headers 开关、`next_seq` 正确 | <= 1000 ms |
| `net_clear` | 返回清除数；后续日志为空 | <= 1000 ms |
| `net_intercept` | set/clear 形状；block 产生页面级失败；mock 改变页面文本；非法规则返回 BAD_ARGS 并带 rule id | <= 1000 ms |
| `download_list` | 最新优先、形状、limit、数字 id | <= 2000 ms |
| `download_wait` | 解析完成的下载；十进制字符串 id；非十进制 id 为 BAD_ARGS；未知十进制 id 为 TIMEOUT 并带 waited_ms 与 id | <= timeout + 5000 ms |
| `upload_file` | 通过 eval 证明 `input.files` 已填充；返回 accept/multiple；非文件与缺失路径为 BAD_ARGS | <= 2000 ms |
| `browser_id` 路由 | 歧义 BAD_ARGS；未知 TRANSPORT_DOWN；显式 id 正确路由 | <= 1000 ms |
| 错误路径 | 结构化 code 与 message；守护进程健康 | <= 60000 ms 默认 |

以下任一情况为硬失败：工具返回成功但页面、捕获或下载没有变化；非结构化错误；九码分类之外的
错误码；缺少或多余的工具；结果类型错误；守护进程崩溃。

## 16. 清理与故障排查

```bash
call intercept-final-clear '{"jsonrpc":"2.0","id":99,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"clear"}}}'
call capture-final-stop '{"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"stop"}}}'
```

然后停止守护进程与 fixture 服务器。保留证据目录并附到报告。

| 症状 | 检查 |
|---|---|
| MCP 连接被拒绝 | 守护进程日志；正确的 `--http-port`；`/health` |
| `browsers` 为空 | 扩展已加载、service worker 运行、控制台有 hello |
| `browser_list` 有两个条目 | 后续调用使用显式 `browser_id` |
| `net_capture` start 失败 | 浏览器连接且具有 `dom`；重新加载扩展 |
| `net_log` 为空 | 导航或点击前先 start capture；检查 `tab_id` |
| block 无效果 | 规则正则必须匹配完整 URL；first-match-wins；捕获已开启 |
| mock 响应未被使用 | `status` 在 100..599；body 是合法 base64；带 content-type |
| `download_wait` 超时 | 检查浏览器下载页与 `download_list`；文件名可能被去重 |
| `upload_file` BAD_ARGS | 绝对 Windows 路径；目标是文件输入框；1..10 个文件 |
| `upload_file` 成功但 files 为空 | 页面可能重置了输入框；立即重读 `input.files` |
| tab 或 browser id 错误 | id 是 JSON 字符串；不要强制转数字；未知字符串为 NOT_FOUND |

通过这些检查仍失败即为缺陷。记录精确的请求与响应体、工具、参数、时间戳、延迟和截图；
验收期间不要修补产品代码。
