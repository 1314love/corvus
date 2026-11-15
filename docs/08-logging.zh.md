# 日志规范

[English](08-logging.md) | [中文](08-logging.zh.md)

状态：1.3.0。读者：查看守护进程输出的运维人员，以及新增事件的开发者。

Corvus 只通过 `tracing` 记录日志。一条订阅管线分发到三个出口，因此控制台、文件与 `logs_tail` 工具看到的是同一批记录、同一套字段名。

## 1. 出口

| 出口 | 默认 | 控制方式 |
|---|---|---|
| stderr | 开启 | `RUST_LOG` |
| 每日滚动文件 | 开启，位于 `logs/` | `--log-file <PATH>`、`--no-log-file` |
| 内存环 | 开启，5,000 条 | `logs_tail` |

`RUST_LOG` 默认 `info`。过滤器位于三个出口之上：被过滤的事件不会进入任何一个出口。

文件出口写入 `<name>.<UTC 日期>`，例如 `corvusd.log.2026-11-15`，不会是裸的 `corvusd.log`。每天滚动一次，写入不阻塞请求处理。

内存环保留最近 5,000 条事件，每条最多 16 个结构化字段。`logs_tail` 支持 `limit`（默认 200，最大 1,000）、`level`（`debug`、`info`、`warn`、`error`）与 `since_ms`。

## 2. 级别

| 级别 | 用途 | 示例 |
|---|---|---|
| `error` | 数据丢失或不变式被破坏 | `could not serialize an envelope` |
| `warn` | 请求、连接或记录被拒绝、被降级 | `handshake rejected`、`tool call failed` |
| `info` | 进程、连接与会话的生命周期 | `corvusd listening`、`browser connected` |
| `debug` | 逐消息的路由细节 | `command dispatched` |

不使用 `trace`：内存环只保留四级，`debug` 已是本项目最细的粒度。

## 3. 消息

消息是作为最后一个参数传入的裸字符串字面量。

- 小写 ASCII、一句话、不超过 48 个字符。
- 不把值写进文本，值走字段。
- 不用 `{}` 占位符，不用 `format!`，结尾不加 `;`。
- 生命周期事件写作 `<主体> <动词>`：`browser connected`、`runtime ended`。
- 失败事件写作 `could not <动词> <对象>`：`could not accept a websocket`。
- 文本只陈述发生了什么，不写读者该做什么。

## 4. 字段

- `snake_case`；每个参数都写成 `name = value`，不使用 `%error` 这类位置简写。
- 标识符以 `_id` 结尾：`browser_id`、`session_id`、`command_id`、`request_id`、`tab_id`。
- `error` 放失败文本，`code` 放协议错误码。
- `message` 属于事件文本，永不作为字段名。
- 数量用 `count`、`duration_ms`、`attempt`；布尔用 `ok`；枚举用短字符串。

## 5. 示例

```rust
tracing::info!(browser_id = %browser, "browser connected");
tracing::warn!(code = %error.code, error = %error.message, "handshake rejected");
tracing::debug!(command_id = %command_id, tool = %request.tool, "command dispatched");
```

## 6. 执行

`crates/corvusd/tests/log_style.rs` 在 `cargo test` 时扫描 `crates/**/*.rs`，消息或字段不符合本规范即失败。周边代码由 `cargo clippy --workspace --all-targets -- -D warnings` 把关。
