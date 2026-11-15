# 测试

Corvus 工作区的测试代码、夹具与验收证据。产品源码在 `crates/` 与
`extension/`；本目录不随任何包发布。

## 产物位置

测试运行不写仓库，也不写命令启动时所在的目录。一次运行产生的全部产物（MCP
响应转储、JSONL 轨迹、截图、证据 JSON、临时 SQLite 文件）统一放在
`<tmp>/corvus-tests` 之下。

`<tmp>` 的解析顺序：

| 场景 | 顺序 |
| --- | --- |
| shell | `CORVUS_TEST_TMP`、`TMPDIR`、`TEMP`/`TMP`、`/tmp` |
| Node | `CORVUS_TEST_TMP`、`os.tmpdir()`（Windows 上是 `%TEMP%`） |

每次运行在 `<tmp>/corvus-tests` 下独占一个目录，例如
`corvus-perf.XXXXXX` 或 `corvus-conformance.XXXXXX`；证据不再需要时直接删除该
目录。只有经过评审、属于仓库的证据才复制到 `tests/report/`。

## 运行入口

| 入口 | 范围 |
| --- | --- |
| `tests/conformance/run.sh` | MCP 契约：43 个工具、路由、抓包、录制 |
| `tests/perf/run.sh` | 延迟、并发、内存与二进制体积证据 |
| `tests/perf/soak.sh` | NFR-3 长时间与实机运行 |
| `extension`（`pnpm test`、`pnpm test:e2e`） | Vitest 单元与 Playwright 端到端 |

## 新测试写产物

Shell：

```sh
. "$(dirname "$0")/../lib/tmp.sh"
TMPROOT=$(corvus_tmp_root) || exit 1
TMP=$(mktemp -d "$TMPROOT/corvus-example.XXXXXX") || exit 1
```

Node：

```js
import { testTmpRoot } from "../lib/tmp.mjs";
const dir = testTmpRoot();
```

命令行，用于单次转储：

```sh
node tests/lib/tmp.mjs path snapshot.json   # 打印 %TEMP% 下的绝对路径
```
