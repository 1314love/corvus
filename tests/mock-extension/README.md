# Mock extension

A small Rust test double for the Corvus browser extension. It connects to
`ws://127.0.0.1:7330`, performs the hello handshake from
`docs/02-architecture.md` section 4, answers command envelopes with canned
responses, and records every received envelope to a JSONL file.

It is standalone on purpose: `Cargo.toml` carries an empty `[workspace]`
table so this crate never joins the repository workspace.

## Build

```bash
cargo build --offline --manifest-path tests/mock-extension/Cargo.toml
```

Binary: `tests/mock-extension/target/debug/mock-extension.exe` (Windows) or
`tests/mock-extension/target/debug/mock-extension`.

## Run against a local daemon

```bash
tests/mock-extension/target/debug/mock-extension.exe \
  --url ws://127.0.0.1:7330 \
  --browser-id mock:default \
  --answer-tool browser_navigate,page_snapshot,browser_click,browser_type \
  --answer-json '{"url":"https://example.test/","title":"Mock page","nodes":[]}' \
  --out ./.tmp/mock-envelopes.jsonl
```

The process prints `READY browser=<id> daemon_version=<v> capabilities=...`
after the daemon acknowledges the handshake, then `IN <envelope>` and
`OUT <envelope>` for every frame. `DONE received=... commands=...` is printed
on shutdown. `--quiet` keeps only READY/CLOSED/DONE and errors.

For automated tests, pass `--exit-after <n>` so the process exits by itself
after N command envelopes, or `--idle-timeout-ms <ms>` as a watchdog.

## Flags

| flag | meaning |
|---|---|
| `--url` | daemon WebSocket endpoint (default `ws://127.0.0.1:7330`) |
| `--browser-id` | browser id announced in hello (default `mock:default`) |
| `--profile` | profile announced in hello (default `default`) |
| `--extension-version` | version announced in hello |
| `--capabilities` | comma separated capability list |
| `--capabilities-json` | raw JSON for `args.capabilities` (overrides above) |
| `--answer-tool` | tools to answer: comma separated, `*` for all (default `*`) |
| `--answer-json` | raw tool result returned as the response `args` |
| `--answer-map` | JSON object mapping tool names to raw results; overrides `--answer-json` |
| `--fail-tool` | tools to answer with a structured error |
| `--fail-code` | ErrorCode for `--fail-tool` (default `NOT_FOUND`) |
| `--fail-message` | error message for `--fail-tool` |
| `--delay-ms` | delay before each response |
| `--out` | append every received envelope to this JSONL file |
| `--handshake-timeout-ms` | wait for `hello_ack` (default 10000) |
| `--idle-timeout-ms` | exit after this much silence; 0 disables |
| `--exit-after` | exit after N command envelopes |
| `--ignore-others` | do not answer commands outside the configured tools |
| `--quiet` | suppress per-envelope output |

## Wire assumptions

The mock matches the protocol v1 contract in `docs/02-architecture.md` section 4:

- `hello` carries `browser` at the top level and
  `args: {"profile":"...","version":"...","capabilities":["dom","scripting"]}`.
  Capabilities are daemon capability names, not tool names.
- A successful command response echoes `id`, `tool`, `browser`, `session` and
  `meta`, and puts the raw tool result in `args`.
- A failed command response sets `args` to `null` and adds a top-level
  `error: {"code":"<ErrorCode>","message":"...","retryable":<bool>}`.

## Per-tool and stateful answers

`--answer-map` gives each tool its own canned result. The mock records the final
URL from a successful `browser_navigate` answer and expands it in later answers:

```bash
tests/mock-extension/target/debug/mock-extension.exe \
  --answer-map '{"browser_navigate":{"loaded":true,"url":"http://127.0.0.1:8808/b"},"page_snapshot":{"url":"{{last_navigate_url}}","title":"Page B","nodes":[]}}' \
  --out ./.tmp/mock-envelopes.jsonl
```

`{{request_url}}` is replaced with the URL carried by the current command, so a
single map can answer several navigate calls with their real target URLs.
`{{last_navigate_url}}` starts as `about:blank` and becomes the URL reported by
the most recent successful navigate answer. `{{last_navigate_title}}` works the
same way when the navigate answer carries a `title`. This lets a conformance
run assert that a snapshot issued immediately after navigate reflects the new
document, and that a `loaded:false` navigate answer is still a successful tool
call.

If the daemon contract changes, adjust `response_envelope` and `error_object`
in `src/main.rs`, plus the matching assertions in
`tests/conformance/mcp_assert.mjs`.

Exit codes: 0 clean shutdown, 1 error, 2 handshake timeout, 3 idle timeout.
