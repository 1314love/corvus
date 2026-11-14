//! Mock Corvus extension.
//!
//! Connects to the daemon WebSocket endpoint, performs the hello handshake
//! described in docs/02-architecture.md section 4, answers command envelopes
//! with configurable canned responses, and records every received envelope to
//! a JSONL file. It is used by the conformance tests and by manual protocol
//! probing.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use clap::Parser;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const PROTOCOL_VERSION: u64 = 1;
/// Daemon capability names understood by protocol v1. Tool names are not capabilities.
const DEFAULT_CAPABILITIES: &str = "dom,scripting";

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;

#[derive(Debug, Parser)]
#[command(
    name = "mock-extension",
    version,
    about = "Mock browser extension for the Corvus daemon",
    long_about = "Connects to a running corvusd, performs the hello handshake, answers command \
envelopes with canned responses and records every received envelope to JSONL.\n\n\
Exit codes: 0 clean shutdown (daemon closed or --exit-after reached), 1 error, \
2 no hello_ack before --handshake-timeout-ms, 3 idle for --idle-timeout-ms."
)]
struct Args {
    /// WebSocket endpoint of the daemon.
    #[arg(long, default_value = "ws://127.0.0.1:7330")]
    url: String,

    /// Browser id announced in the hello envelope, e.g. mock:default.
    #[arg(long, default_value = "mock:default")]
    browser_id: String,

    /// Browser profile name announced in the hello envelope.
    #[arg(long, default_value = "default")]
    profile: String,

    /// Extension version announced in the hello envelope.
    #[arg(long = "extension-version", default_value = env!("CARGO_PKG_VERSION"))]
    extension_version: String,

    /// Comma separated daemon capability list announced in the hello envelope.
    #[arg(long, default_value = DEFAULT_CAPABILITIES)]
    capabilities: String,

    /// Raw JSON value for "args.capabilities" (overrides --capabilities).
    #[arg(long, value_name = "JSON")]
    capabilities_json: Option<String>,

    /// Tool name(s) to answer successfully: comma separated, "*" answers all.
    #[arg(long, default_value = "*")]
    answer_tool: String,

    /// Raw tool result returned as the response envelope "args".
    #[arg(long, default_value = r#"{"ok":true}"#)]
    answer_json: String,

    /// Per-tool raw results as a JSON object, e.g.
    /// {"browser_navigate":{"loaded":true,"url":"..."}}.
    /// Takes precedence over --answer-json; a "*" key is the fallback.
    #[arg(long, value_name = "JSON")]
    answer_map: Option<String>,

    /// Tool name(s) to fail with a structured error: comma separated.
    #[arg(long, value_name = "TOOLS")]
    fail_tool: Option<String>,

    /// ErrorCode used for --fail-tool, e.g. NOT_FOUND, TIMEOUT, BROWSER_GONE.
    #[arg(long, default_value = "NOT_FOUND")]
    fail_code: String,

    /// Error message returned with --fail-tool.
    #[arg(long, default_value = "mock failure")]
    fail_message: String,

    /// Delay before each response, in milliseconds.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,

    /// Append every received envelope to this JSONL file.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,

    /// How long to wait for hello_ack, in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    handshake_timeout_ms: u64,

    /// Exit after this many idle milliseconds; 0 disables the watchdog.
    #[arg(long, default_value_t = 0)]
    idle_timeout_ms: u64,

    /// Exit successfully after receiving this many command envelopes.
    #[arg(long, value_name = "N")]
    exit_after: Option<u64>,

    /// Leave commands that match neither --answer-tool nor --fail-tool unanswered.
    #[arg(long)]
    ignore_others: bool,

    /// Only print READY/CLOSED/DONE lines and errors, not every envelope.
    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Answered,
    Failed,
    Ignored,
}

/// Navigation state used to expand {{last_navigate_url}} and
/// {{last_navigate_title}} placeholders in canned snapshot answers.
#[derive(Debug, Default)]
struct MockState {
    last_navigate_url: Option<String>,
    last_navigate_title: Option<String>,
}

struct Recorder {
    file: Option<BufWriter<std::fs::File>>,
}

impl Recorder {
    fn open(path: &Option<PathBuf>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self { file: None });
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create output directory {}", parent.display()))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open --out {}", path.display()))?;
        Ok(Self {
            file: Some(BufWriter::new(file)),
        })
    }

    fn record(&mut self, text: &str) -> Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        writeln!(file, "{text}").context("write JSONL record")?;
        // Flush per line so a test can tail the file while the mock is running.
        file.flush().context("flush JSONL record")?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match run(args).await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("mock-extension: {err:#}");
            ExitCode::from(1)
        }
    }
}

async fn run(args: Args) -> Result<ExitCode> {
    let answer: Value = serde_json::from_str(&args.answer_json)
        .with_context(|| format!("--answer-json is not valid JSON: {}", args.answer_json))?;
    let answer_map = parse_answer_map(args.answer_map.as_deref())?;
    let mut state = MockState::default();
    let capabilities = build_capabilities(&args)?;
    let mut recorder = Recorder::open(&args.out)?;

    let (ws, _) = connect_async(args.url.as_str())
        .await
        .with_context(|| format!("connect to {} (is corvusd running?)", args.url))?;
    let (mut sink, mut stream) = ws.split();

    let hello = json!({
        "v": PROTOCOL_VERSION,
        "id": next_id("hello"),
        "kind": "hello",
        "browser": args.browser_id,
        "args": {
            "profile": args.profile,
            "version": args.extension_version,
            "capabilities": capabilities,
        }
    });
    send_json(&mut sink, &hello, &args).await?;

    let ack = match wait_for_hello_ack(&mut sink, &mut stream, &args, &mut recorder).await? {
        Some(ack) => ack,
        None => return Ok(ExitCode::from(2)),
    };
    let daemon_version = ack
        .pointer("/args/version")
        .or_else(|| ack.pointer("/args/daemon_version"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!(
        "READY browser={} daemon_version={} capabilities={}",
        args.browser_id,
        daemon_version,
        args.capabilities_json
            .as_deref()
            .unwrap_or(args.capabilities.as_str())
    );

    let mut received: u64 = 0;
    let mut commands: u64 = 0;
    let mut answered: u64 = 0;
    let mut failed: u64 = 0;

    loop {
        let next = if args.idle_timeout_ms > 0 {
            match timeout(
                Duration::from_millis(args.idle_timeout_ms),
                stream.next(),
            )
            .await
            {
                Ok(next) => next,
                Err(_) => {
                    eprintln!(
                        "mock-extension: no envelope for {} ms",
                        args.idle_timeout_ms
                    );
                    recorder.file.as_mut().map(Write::flush);
                    return Ok(ExitCode::from(3));
                }
            }
        } else {
            stream.next().await
        };

        let Some(message) = next else {
            println!("CLOSED by daemon after {received} envelopes");
            return Ok(ExitCode::SUCCESS);
        };
        let message = message.context("websocket read failed")?;

        match &message {
            Message::Ping(payload) => {
                sink.send(Message::Pong(payload.clone()))
                    .await
                    .context("send websocket pong")?;
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(frame) => {
                println!(
                    "CLOSED by daemon after {received} envelopes{}",
                    frame
                        .as_ref()
                        .map(|f| format!(" (code {} {})", u16::from(f.code), f.reason))
                        .unwrap_or_default()
                );
                return Ok(ExitCode::SUCCESS);
            }
            _ => {}
        }

        let Some(text) = text_payload(&message)? else {
            continue;
        };
        let envelope: Value = serde_json::from_str(&text)
            .with_context(|| format!("received invalid JSON envelope: {text}"))?;
        recorder.record(&text)?;
        received += 1;
        if !args.quiet {
            println!("IN {text}");
        }

        let kind = envelope.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "command" => {
                commands += 1;
                let tool = envelope.get("tool").and_then(Value::as_str).unwrap_or("");
                match handle_command(
                    &envelope,
                    tool,
                    &args,
                    &answer,
                    &answer_map,
                    &mut state,
                    &mut sink,
                )
                .await?
                {
                    Outcome::Answered => {
                        answered += 1;
                        if !args.quiet {
                            println!("CMD tool={tool} id={} -> ok", string_field(&envelope, "id"));
                        }
                    }
                    Outcome::Failed => {
                        failed += 1;
                        if !args.quiet {
                            println!(
                                "CMD tool={tool} id={} -> error {}",
                                string_field(&envelope, "id"),
                                args.fail_code.to_ascii_uppercase()
                            );
                        }
                    }
                    Outcome::Ignored => {
                        if !args.quiet {
                            println!(
                                "CMD tool={tool} id={} -> ignored",
                                string_field(&envelope, "id")
                            );
                        }
                    }
                }
                if args.exit_after.is_some_and(|limit| commands >= limit) {
                    break;
                }
            }
            "ping" => {
                let pong = json!({
                    "v": PROTOCOL_VERSION,
                    "id": envelope.get("id").cloned().unwrap_or(Value::Null),
                    "kind": "pong",
                    "browser": args.browser_id,
                });
                send_json(&mut sink, &pong, &args).await?;
            }
            "hello_ack" | "event" | "response" => {}
            other => eprintln!("mock-extension: ignoring envelope kind {other:?}"),
        }
    }

    sink.send(Message::Close(None)).await.ok();
    println!("DONE received={received} commands={commands} answered={answered} failed={failed}");
    Ok(ExitCode::SUCCESS)
}

async fn wait_for_hello_ack(
    sink: &mut WsSink,
    stream: &mut futures_util::stream::SplitStream<WsStream>,
    args: &Args,
    recorder: &mut Recorder,
) -> Result<Option<Value>> {
    let started = Instant::now();
    let budget = Duration::from_millis(args.handshake_timeout_ms);

    loop {
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            eprintln!(
                "mock-extension: no hello_ack within {} ms",
                args.handshake_timeout_ms
            );
            return Ok(None);
        }
        match timeout(remaining, stream.next()).await {
            Err(_) => {
                eprintln!(
                    "mock-extension: no hello_ack within {} ms",
                    args.handshake_timeout_ms
                );
                return Ok(None);
            }
            Ok(None) => bail!("daemon closed the connection before hello_ack"),
            Ok(Some(Err(err))) => return Err(err).context("websocket read failed during handshake"),
            Ok(Some(Ok(message))) => {
                match &message {
                    Message::Ping(payload) => {
                        sink.send(Message::Pong(payload.clone()))
                            .await
                            .context("send websocket pong")?;
                        continue;
                    }
                    Message::Close(_) => bail!("daemon closed the connection before hello_ack"),
                    _ => {}
                }
                let Some(text) = text_payload(&message)? else {
                    continue;
                };
                let envelope: Value = serde_json::from_str(&text)
                    .with_context(|| format!("received invalid JSON envelope: {text}"))?;
                recorder.record(&text)?;
                if !args.quiet {
                    println!("IN {text}");
                }
                match envelope.get("kind").and_then(Value::as_str) {
                    Some("hello_ack") => return Ok(Some(envelope)),
                    Some("ping") => {
                        let pong = json!({
                            "v": PROTOCOL_VERSION,
                            "id": envelope.get("id").cloned().unwrap_or(Value::Null),
                            "kind": "pong",
                            "browser": args.browser_id,
                        });
                        send_json(sink, &pong, args).await?;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_command(
    envelope: &Value,
    tool: &str,
    args: &Args,
    answer: &Value,
    answer_map: &HashMap<String, Value>,
    state: &mut MockState,
    sink: &mut WsSink,
) -> Result<Outcome> {
    if tool.is_empty() {
        return send_error(envelope, args, "BAD_ARGS", "command envelope has no tool", sink).await;
    }
    if args
        .fail_tool
        .as_deref()
        .is_some_and(|list| tool_matches(list, tool))
    {
        if args.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
        let code = args.fail_code.to_ascii_uppercase();
        let body = response_envelope(
            envelope,
            args,
            Value::Null,
            Some(error_object(&code, &args.fail_message)),
        );
        send_json(sink, &body, args).await?;
        return Ok(Outcome::Failed);
    }
    if tool_matches(&args.answer_tool, tool) {
        if args.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
        let configured = answer_map
            .get(tool)
            .or_else(|| answer_map.get("*"))
            .unwrap_or(answer)
            .clone();
        let request_url = envelope.pointer("/args/url").and_then(Value::as_str);
        let payload = expand_placeholders(configured, state, request_url);
        if tool == "browser_navigate" {
            let final_url = payload
                .get("url")
                .and_then(Value::as_str)
                .or_else(|| envelope.pointer("/args/url").and_then(Value::as_str));
            if let Some(url) = final_url {
                state.last_navigate_url = Some(url.to_string());
            }
            if let Some(title) = payload.get("title").and_then(Value::as_str) {
                state.last_navigate_title = Some(title.to_string());
            }
        }
        let body = response_envelope(envelope, args, payload, None);
        send_json(sink, &body, args).await?;
        return Ok(Outcome::Answered);
    }
    if args.ignore_others {
        return Ok(Outcome::Ignored);
    }
    send_error(
        envelope,
        args,
        "BAD_ARGS",
        &format!("mock-extension has no canned response for tool {tool}"),
        sink,
    )
    .await
}

async fn send_error(
    envelope: &Value,
    args: &Args,
    code: &str,
    message: &str,
    sink: &mut WsSink,
) -> Result<Outcome> {
    if args.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
    }
    let body = response_envelope(
        envelope,
        args,
        Value::Null,
        Some(error_object(code, message)),
    );
    send_json(sink, &body, args).await?;
    Ok(Outcome::Failed)
}

fn response_envelope(
    command: &Value,
    args: &Args,
    payload: Value,
    error: Option<Value>,
) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("v".to_string(), json!(PROTOCOL_VERSION));
    body.insert(
        "id".to_string(),
        command.get("id").cloned().unwrap_or(Value::Null),
    );
    body.insert("kind".to_string(), json!("response"));
    body.insert(
        "browser".to_string(),
        command
            .get("browser")
            .cloned()
            .unwrap_or_else(|| json!(args.browser_id)),
    );
    if let Some(session) = command.get("session") {
        body.insert("session".to_string(), session.clone());
    }
    if let Some(tool) = command.get("tool") {
        body.insert("tool".to_string(), tool.clone());
    }
    body.insert("args".to_string(), payload);
    if let Some(error) = error {
        body.insert("error".to_string(), error);
    }
    body.insert(
        "meta".to_string(),
        command.get("meta").cloned().unwrap_or_else(|| json!({})),
    );
    Value::Object(body)
}

/// Typed error object returned at the top level of a response envelope.
fn error_object(code: &str, message: &str) -> Value {
    json!({
        "code": code,
        "message": message,
        "retryable": retryable(code),
    })
}

/// Retryability per docs/02-architecture.md section 8.
fn retryable(code: &str) -> bool {
    matches!(
        code,
        "NOT_FOUND" | "NOT_INTERACTABLE" | "TIMEOUT" | "NAVIGATION_LOST" | "TRANSPORT_DOWN"
    )
}

async fn send_json(sink: &mut WsSink, value: &Value, args: &Args) -> Result<()> {
    let text = serde_json::to_string(value).context("serialize envelope")?;
    if !args.quiet {
        println!("OUT {text}");
    }
    sink.send(Message::Text(text.into()))
        .await
        .context("websocket send failed")
}

fn text_payload(message: &Message) -> Result<Option<String>> {
    match message {
        Message::Text(text) => Ok(Some(text.to_string())),
        Message::Binary(bytes) => Ok(Some(
            String::from_utf8(bytes.to_vec()).context("binary frame is not UTF-8")?,
        )),
        _ => Ok(None),
    }
}

fn build_capabilities(args: &Args) -> Result<Value> {
    if let Some(raw) = &args.capabilities_json {
        return serde_json::from_str(raw)
            .with_context(|| format!("--capabilities-json is not valid JSON: {raw}"));
    }
    Ok(Value::Array(
        args.capabilities
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| json!(item))
            .collect(),
    ))
}

fn parse_answer_map(raw: Option<&str>) -> Result<HashMap<String, Value>> {
    let Some(raw) = raw else {
        return Ok(HashMap::new());
    };
    let value: Value = serde_json::from_str(raw)
        .with_context(|| format!("--answer-map is not valid JSON: {raw}"))?;
    let object = value
        .as_object()
        .with_context(|| "--answer-map must be a JSON object mapping tool names to results")?;
    Ok(object
        .iter()
        .map(|(tool, result)| (tool.clone(), result.clone()))
        .collect())
}

/// Replace navigation placeholders recursively in a canned answer.
fn expand_placeholders(value: Value, state: &MockState, request_url: Option<&str>) -> Value {
    match value {
        Value::String(text) => Value::String(
            text.replace(
                "{{last_navigate_url}}",
                state.last_navigate_url.as_deref().unwrap_or("about:blank"),
            )
            .replace(
                "{{last_navigate_title}}",
                state.last_navigate_title.as_deref().unwrap_or(""),
            )
            .replace("{{request_url}}", request_url.unwrap_or("about:blank")),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| expand_placeholders(item, state, request_url))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, expand_placeholders(item, state, request_url)))
                .collect(),
        ),
        other => other,
    }
}

fn tool_matches(list: &str, tool: &str) -> bool {
    list.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .any(|item| item == "*" || item == tool)
}

fn string_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn next_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}-{millis:x}-{sequence:x}")
}
