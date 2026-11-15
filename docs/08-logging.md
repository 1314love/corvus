# Logging convention

[English](08-logging.md) | [中文](08-logging.zh.md)

Status: 1.4.0. Audience: operators reading daemon output and developers adding events.

Corvus logs through `tracing` only. One subscriber pipeline fans out to three sinks, so the console, the file and the `logs_tail` tool show the same records under the same field names.

## 1. Sinks

| Sink | Default | Control |
|---|---|---|
| stderr | on | `RUST_LOG` |
| daily rolling file | on, under `logs/` | `--log-file <PATH>`, `--no-log-file` |
| in-memory ring | on, 5,000 entries | `logs_tail` |

`RUST_LOG` defaults to `info`. The filter sits above all three sinks: a filtered event reaches none of them.

The file sink writes `<name>.<UTC date>`, for example `corvusd.log.2026-11-15`, never a bare `corvusd.log`. Rotation is daily and writes do not block request handling.

The ring keeps the latest 5,000 events with up to 16 structured fields each. `logs_tail` reads it with `limit` (default 200, maximum 1,000), `level` (`debug`, `info`, `warn`, `error`) and `since_ms`.

## 2. Levels

| Level | Use for | Example |
|---|---|---|
| `error` | data loss or a broken invariant | `could not serialize an envelope` |
| `warn` | a request, connection or record was rejected or degraded | `handshake rejected`, `tool call failed` |
| `info` | lifecycle of processes, connections and sessions | `corvusd listening`, `browser connected` |
| `debug` | per-message routing detail | `command dispatched` |

`trace` is unused: the ring holds four levels, and `debug` is the finest granularity.

## 3. Messages

The message is a bare string literal passed as the last argument.

- Lowercase ASCII, one sentence, at most 48 characters.
- No values inside the text; pass them as fields.
- No `{}` placeholders, no `format!`, no trailing `;`.
- Lifecycle events read `<subject> <verb>`: `browser connected`, `runtime ended`.
- Failures read `could not <verb> <object>`: `could not accept a websocket`.
- The text states what happened, not what a reader should do.

## 4. Fields

- `snake_case`; every argument is written `name = value`, never the positional `%error` shorthand.
- Identifiers end in `_id`: `browser_id`, `session_id`, `command_id`, `request_id`, `tab_id`.
- `error` carries the failure text and `code` the protocol error code.
- `message` belongs to the event text and is never used as a field name.
- Numbers use `count`, `duration_ms`, `attempt`; booleans use `ok`; enumerations stay short strings.

## 5. Examples

```rust
tracing::info!(browser_id = %browser, "browser connected");
tracing::warn!(code = %error.code, error = %error.message, "handshake rejected");
tracing::debug!(command_id = %command_id, tool = %request.tool, "command dispatched");
```

## 6. Enforcement

`crates/corvusd/tests/log_style.rs` scans every `crates/**/*.rs` file during `cargo test` and fails when a message or field breaks these rules. `cargo clippy --workspace --all-targets -- -D warnings` keeps the surrounding code clean.
