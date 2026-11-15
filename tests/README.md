# Tests

Test code, fixtures and acceptance evidence for the Corvus workspace. Product
sources live in `crates/` and `extension/`; nothing here ships in a package.

## Artifact location

A test run never writes into the repository and never into the directory the
command was started from. Every artifact it produces (MCP response dumps, JSONL
traces, screenshots, evidence JSON, temporary SQLite files) goes below
`<tmp>/corvus-tests`.

`<tmp>` resolves in this order:

| context | order |
| --- | --- |
| shell | `CORVUS_TEST_TMP`, `TMPDIR`, `TEMP`/`TMP`, `/tmp` |
| Node | `CORVUS_TEST_TMP`, `os.tmpdir()` (`%TEMP%` on Windows) |

Each run owns one directory below `<tmp>/corvus-tests`, for example
`corvus-perf.XXXXXX` or `corvus-conformance.XXXXXX`; delete it once the evidence
is no longer needed. Only reviewed evidence that belongs in the repository is
copied to `tests/report/`.

## Runners

| entry | scope |
| --- | --- |
| `tests/conformance/run.sh` | MCP contract: 43 tools, routing, capture, recording |
| `tests/perf/run.sh` | latency, concurrency, memory and binary-size evidence |
| `tests/perf/soak.sh` | NFR-3 soak and live runs |
| `extension` (`pnpm test`, `pnpm test:e2e`) | Vitest units and Playwright end-to-end |

## Writing artifacts from a new test

Shell:

```sh
. "$(dirname "$0")/../lib/tmp.sh"
TMPROOT=$(corvus_tmp_root) || exit 1
TMP=$(mktemp -d "$TMPROOT/corvus-example.XXXXXX") || exit 1
```

Node:

```js
import { testTmpRoot } from "../lib/tmp.mjs";
const dir = testTmpRoot();
```

Command line, for a single dump:

```sh
node tests/lib/tmp.mjs path snapshot.json   # prints an absolute path below %TEMP%
```
