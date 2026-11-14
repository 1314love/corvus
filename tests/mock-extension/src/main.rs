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

    /// Raw tool result returned as the response envelope "args". When this is
    /// omitted, known tools use the mock's stateful answers.
    #[arg(long, value_name = "JSON")]
    answer_json: Option<String>,

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

/// One tab inside the mock browser. History is a URL list with a cursor so
/// browser_history can move back and forward without a real browser.
#[derive(Debug, Clone)]
struct Tab {
    id: String,
    url: String,
    title: String,
    status: String,
    history: Vec<String>,
    history_index: usize,
    scroll_x: i64,
    scroll_y: i64,
}

impl Tab {
    fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            id: id.into(),
            title: title_for(&url),
            url: url.clone(),
            status: "complete".to_string(),
            history: vec![url],
            history_index: 0,
            scroll_x: 0,
            scroll_y: 0,
        }
    }

    fn navigate(&mut self, url: String, title: String) {
        if self.history.get(self.history_index) != Some(&url) {
            self.history.truncate(self.history_index + 1);
            self.history.push(url.clone());
            self.history_index = self.history.len() - 1;
        }
        self.url = url;
        self.title = title;
        self.status = "complete".to_string();
    }
}

fn title_for(url: &str) -> String {
    if url == "about:blank" {
        "New Tab".to_string()
    } else {
        "Mock page".to_string()
    }
}

/// Browser state behind the mock answers: tabs, active tab, navigation history,
/// scroll offsets and the placeholders used by canned page_snapshot answers.
#[derive(Debug)]
struct MockState {
    tabs: Vec<Tab>,
    active: String,
    next_tab: u64,
    last_navigate_url: Option<String>,
    last_navigate_title: Option<String>,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            tabs: vec![Tab::new("tab-1", "about:blank")],
            active: "tab-1".to_string(),
            next_tab: 2,
            last_navigate_url: None,
            last_navigate_title: None,
        }
    }
}

impl MockState {
    fn active_index(&self) -> usize {
        self.tabs
            .iter()
            .position(|tab| tab.id == self.active)
            .unwrap_or(0)
    }

    fn tab_index(&self, tab_id: Option<&str>) -> MockResult<usize> {
        let id = tab_id.unwrap_or(self.active.as_str());
        self.tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or_else(|| MockError::new("NOT_FOUND", format!("tab {id} does not exist")))
    }

    fn navigate(&mut self, url: String, title: Option<String>) {
        let index = self.active_index();
        let title = title.unwrap_or_else(|| title_for(&url));
        self.tabs[index].navigate(url.clone(), title.clone());
        self.last_navigate_url = Some(url);
        self.last_navigate_title = Some(title);
    }
}

/// Typed failure a stateful mock answer can return before the response envelope
/// is built. It carries the same code/message/retryable shape as a real extension.
#[derive(Debug, Clone)]
struct MockError {
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl MockError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

type MockResult<T> = std::result::Result<T, MockError>;

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
    let answer: Value = match &args.answer_json {
        Some(raw) => serde_json::from_str(raw)
            .with_context(|| format!("--answer-json is not valid JSON: {raw}"))?,
        None => json!({"ok": true}),
    };
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
            match timeout(Duration::from_millis(args.idle_timeout_ms), stream.next()).await {
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
            Ok(Some(Err(err))) => {
                return Err(err).context("websocket read failed during handshake")
            }
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
        return send_error(
            envelope,
            args,
            "BAD_ARGS",
            "command envelope has no tool",
            sink,
        )
        .await;
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

    let arguments = envelope.get("args").cloned().unwrap_or_else(|| json!({}));
    let request_url = arguments.get("url").and_then(Value::as_str);

    // An explicit --answer-map entry always wins. This keeps the navigate
    // loaded=false conformance case and any hand-written canned answer exact.
    if let Some(configured) = answer_map.get(tool).or_else(|| answer_map.get("*")) {
        if args.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
        let payload = expand_placeholders(configured.clone(), state, request_url);
        if tool == "browser_navigate" {
            let final_url = payload.get("url").and_then(Value::as_str).or(request_url);
            if let Some(url) = final_url {
                let title = payload
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                state.navigate(url.to_string(), title);
            }
        }
        let body = response_envelope(envelope, args, payload, None);
        send_json(sink, &body, args).await?;
        return Ok(Outcome::Answered);
    }

    if let Some(canned) = &args.answer_json {
        if tool_matches(&args.answer_tool, tool) {
            if args.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
            }
            let canned_value: Value = serde_json::from_str(canned)
                .with_context(|| format!("--answer-json is not valid JSON: {canned}"))?;
            let payload = expand_placeholders(canned_value, state, request_url);
            if tool == "browser_navigate" {
                let final_url = payload.get("url").and_then(Value::as_str).or(request_url);
                if let Some(url) = final_url {
                    let title = payload
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    state.navigate(url.to_string(), title);
                }
            }
            let body = response_envelope(envelope, args, payload, None);
            send_json(sink, &body, args).await?;
            return Ok(Outcome::Answered);
        }
    }

    if tool_matches(&args.answer_tool, tool) {
        if args.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
        let outcome = match stateful_answer(tool, &arguments, state) {
            Some(result) => result,
            None => Ok(expand_placeholders(answer.clone(), state, request_url)),
        };
        match outcome {
            Ok(payload) => {
                let body = response_envelope(envelope, args, payload, None);
                send_json(sink, &body, args).await?;
                Ok(Outcome::Answered)
            }
            Err(error) => {
                let body = response_envelope(
                    envelope,
                    args,
                    Value::Null,
                    Some(error_object_full(error.code, &error.message, error.details)),
                );
                send_json(sink, &body, args).await?;
                Ok(Outcome::Failed)
            }
        }
    } else if args.ignore_others {
        Ok(Outcome::Ignored)
    } else {
        send_error(
            envelope,
            args,
            "BAD_ARGS",
            &format!("mock-extension has no canned response for tool {tool}"),
            sink,
        )
        .await
    }
}

/// Return a stateful answer for a tool, or None when the mock should fall
/// back to --answer-json. Explicit --answer-map entries never reach this path.
fn stateful_answer(tool: &str, args: &Value, state: &mut MockState) -> Option<MockResult<Value>> {
    let outcome = match tool {
        "browser_navigate" => navigate_answer(args, state),
        "page_snapshot" => snapshot_answer(args, state),
        "browser_click" => click_answer(args, state),
        "browser_type" => type_answer(args, state),
        "page_text" => page_text_answer(args, state),
        "page_html" => page_html_answer(args, state),
        "page_find" => page_find_answer(args, state),
        "browser_hover" => hover_answer(args, state),
        "browser_press" => press_answer(args, state),
        "browser_scroll" => scroll_answer(args, state),
        "browser_select" => select_answer(args, state),
        "browser_eval" => eval_answer(args, state),
        "browser_wait" => wait_answer(args, state),
        "browser_history" => history_answer(args, state),
        "tab_list" => tab_list_answer(state),
        "tab_new" => tab_new_answer(args, state),
        "tab_close" => tab_close_answer(args, state),
        "tab_switch" => tab_switch_answer(args, state),
        _ => return None,
    };
    Some(outcome)
}

fn resolve_tab(args: &Value, state: &MockState) -> MockResult<usize> {
    state.tab_index(args.get("tab_id").and_then(Value::as_str))
}

fn require_string(args: &Value, key: &str) -> MockResult<String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| MockError::new("BAD_ARGS", format!("mock requires a non-empty {key}")))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn optional_integer(args: &Value, key: &str, fallback: u64) -> MockResult<u64> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(fallback),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| MockError::new("BAD_ARGS", format!("mock requires integer {key}"))),
    }
}

fn optional_bool(args: &Value, key: &str, fallback: bool) -> MockResult<bool> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(fallback),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| MockError::new("BAD_ARGS", format!("mock requires boolean {key}"))),
    }
}

fn navigate_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let url = require_string(args, "url")?;
    let index = state.active_index();
    let previous = state.tabs[index].url.clone();
    let title = optional_string(args, "title");
    state.navigate(url.clone(), title);
    Ok(json!({
        "navigated": true,
        "loaded": true,
        "url": url,
        "previous": previous,
        "same_document": false,
        "started_at": 1_763_000_000_000u64,
    }))
}

fn snapshot_answer(args: &Value, state: &MockState) -> MockResult<Value> {
    let index = resolve_tab(args, state)?;
    let tab = &state.tabs[index];
    Ok(json!({
        "url": tab.url.clone(),
        "title": tab.title.clone(),
        "node_count": 2,
        "nodes": [
            {"kind": "element", "ref": "@e1", "tag": "input", "role": "textbox", "name": "Query"},
            {"kind": "element", "ref": "@e2", "tag": "button", "role": "button", "name": "Run"}
        ],
    }))
}

fn click_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let target = require_string(args, "target")?;
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let (reference, selector) = if target.starts_with('@') {
        (json!(target), Value::Null)
    } else {
        (Value::Null, json!(target))
    };
    Ok(json!({
        "clicked": true,
        "ref": reference,
        "selector": selector,
        "tag": "button",
        "role": "button",
        "name": "Run",
        "bbox": {"x": 10, "y": 20, "width": 80, "height": 30},
        "point": {"x": 50, "y": 35},
        "events": ["pointerover", "pointerenter", "pointermove", "pointerdown", "mousedown", "pointerup", "mouseup", "click"],
        "url": url,
    }))
}

fn type_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let target = require_string(args, "target")?;
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let clear = optional_bool(args, "clear", true)?;
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let value_length = text.chars().count();
    Ok(json!({
        "typed": true,
        "ref": if target.starts_with('@') { json!(target) } else { Value::Null },
        "selector": if target.starts_with('@') { Value::Null } else { json!(target) },
        "tag": "input",
        "role": "textbox",
        "cleared": clear,
        "events": ["beforeinput", "input", "change"],
        "value_length": value_length,
        "url": url,
        "value": text,
    }))
}

fn page_text_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let index = resolve_tab(args, state)?;
    let limit = optional_integer(args, "max_chars", 100_000)? as usize;
    let tab = &state.tabs[index];
    let full = format!(
        "Mock page text for {}. Corvus M2 acceptance sample with select, hover, scroll and history controls.",
        tab.url
    );
    let truncated = full.chars().count() > limit;
    let text: String = full.chars().take(limit).collect();
    Ok(json!({
        "url": tab.url.clone(),
        "title": tab.title.clone(),
        "text": text,
        "truncated": truncated,
    }))
}

fn page_html_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let index = resolve_tab(args, state)?;
    let limit = optional_integer(args, "max_chars", 200_000)? as usize;
    let tab = &state.tabs[index];
    let full = match args.get("target").and_then(Value::as_str) {
        Some(target) => format!("<div data-target=\"{target}\">Mock element</div>"),
        None => format!(
            "<html><head><title>{}</title></head><body><h1>Mock page</h1></body></html>",
            tab.title
        ),
    };
    let truncated = full.chars().count() > limit;
    let html: String = full.chars().take(limit).collect();
    Ok(json!({
        "url": tab.url.clone(),
        "html": html,
        "truncated": truncated,
    }))
}

fn page_find_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let query = require_string(args, "query")?;
    let limit = optional_integer(args, "limit", 20)? as usize;
    let role = optional_string(args, "role").unwrap_or_else(|| "textbox".to_string());
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let mut matches = Vec::new();
    if query != "nothing" && limit > 0 {
        matches.push(json!({
            "ref": "@e1",
            "tag": "input",
            "role": role,
            "text": format!("{query} match"),
        }));
    }
    let count = matches.len();
    Ok(json!({
        "url": url,
        "count": count,
        "matches": matches,
    }))
}

fn hover_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let target = require_string(args, "target")?;
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let element = if target.starts_with('@') {
        json!({"ref": target, "tag": "button", "text": "Hover target"})
    } else {
        json!({"tag": "button", "text": "Hover target"})
    };
    Ok(json!({
        "hovered": true,
        "url": url,
        "target": element,
    }))
}

fn press_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let keys = match args.get("keys") {
        Some(Value::String(key)) if !key.trim().is_empty() => vec![key.clone()],
        Some(Value::Array(items)) => {
            let mut keys = Vec::new();
            for item in items {
                let key = item
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| MockError::new("BAD_ARGS", "mock keys must be strings"))?;
                keys.push(key.to_string());
            }
            if keys.is_empty() {
                return Err(MockError::new("BAD_ARGS", "mock keys must not be empty"));
            }
            keys
        }
        _ => return Err(MockError::new("BAD_ARGS", "mock requires keys")),
    };
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    Ok(json!({
        "pressed": true,
        "keys": keys,
        "url": url,
    }))
}

fn scroll_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let direction = require_string(args, "direction")?;
    if !["up", "down", "left", "right"].contains(&direction.as_str()) {
        return Err(MockError::new(
            "BAD_ARGS",
            "mock direction is not supported",
        ));
    }
    let pages = match args.get("pages") {
        None | Some(Value::Null) => 1.0,
        Some(value) => value
            .as_f64()
            .ok_or_else(|| MockError::new("BAD_ARGS", "mock pages must be a number"))?,
    };
    let delta = (pages * 100.0).round() as i64;
    let index = resolve_tab(args, state)?;
    let tab = &mut state.tabs[index];
    match direction.as_str() {
        "up" => tab.scroll_y = (tab.scroll_y - delta).max(0),
        "down" => tab.scroll_y += delta,
        "left" => tab.scroll_x = (tab.scroll_x - delta).max(0),
        "right" => tab.scroll_x += delta,
        _ => {}
    }
    Ok(json!({
        "scrolled": true,
        "x": tab.scroll_x,
        "y": tab.scroll_y,
        "url": tab.url.clone(),
    }))
}

fn select_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let target = require_string(args, "target")?;
    let values = match args.get("values") {
        Some(Value::Array(items)) if !items.is_empty() => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| MockError::new("BAD_ARGS", "mock values must be strings"))
            })
            .collect::<MockResult<Vec<String>>>()?,
        _ => {
            return Err(MockError::new(
                "BAD_ARGS",
                "mock values must be a non-empty array",
            ))
        }
    };
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let value = values.first().cloned().unwrap_or_default();
    Ok(json!({
        "selected": values,
        "value": value,
        "url": url,
        "target": target,
    }))
}

fn eval_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let expression = require_string(args, "expression")?;
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    let title = state.tabs[index].title.clone();
    let trimmed = expression.trim();
    let result = if trimmed == "1 + 1" || trimmed == "1+1" {
        json!({"ok": true, "value": 2})
    } else if trimmed == "document.title" {
        json!({"ok": true, "value": title})
    } else if trimmed == "location.href" {
        json!({"ok": true, "value": url})
    } else if matches!(trimmed, "window" | "globalThis" | "document") {
        json!({"ok": true, "value": null, "value_note": "unserializable"})
    } else if trimmed.starts_with("throw") {
        json!({
            "ok": false,
            "error": {"name": "Error", "message": "mock page error"},
        })
    } else if let Some(value) = trimmed
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    {
        json!({"ok": true, "value": value})
    } else {
        json!({"ok": true, "value": trimmed})
    };
    Ok(result)
}

fn wait_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let timeout = optional_integer(args, "timeout_ms", 10_000)?;
    let selector = optional_string(args, "selector");
    let text = optional_string(args, "text");
    let url_contains = optional_string(args, "url_contains");
    if selector.is_none() && text.is_none() && url_contains.is_none() {
        return Err(MockError::new("BAD_ARGS", "browser_wait needs a condition"));
    }
    let never = selector.as_deref() == Some("#never")
        || text.as_deref() == Some("never")
        || url_contains.as_deref() == Some("never");
    if never {
        return Err(
            MockError::new("TIMEOUT", format!("mock wait timed out after {timeout} ms"))
                .with_details(json!({"waited_ms": timeout})),
        );
    }
    let index = resolve_tab(args, state)?;
    let url = state.tabs[index].url.clone();
    Ok(json!({
        "matched": true,
        "waited_ms": 5,
        "url": url,
    }))
}

fn history_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let action = require_string(args, "action")?;
    if !["back", "forward", "reload"].contains(&action.as_str()) {
        return Err(MockError::new(
            "BAD_ARGS",
            "mock history action is not supported",
        ));
    }
    let index = resolve_tab(args, state)?;
    let tab = &mut state.tabs[index];
    match action.as_str() {
        "back" => {
            if tab.history_index > 0 {
                tab.history_index -= 1;
            }
        }
        "forward" => {
            if tab.history_index + 1 < tab.history.len() {
                tab.history_index += 1;
            }
        }
        "reload" => {}
        _ => {}
    }
    let url = tab.history[tab.history_index].clone();
    tab.url = url.clone();
    tab.title = title_for(&url);
    Ok(json!({
        "action": action,
        "url": url,
        "loaded": true,
    }))
}

fn tab_list_answer(state: &MockState) -> MockResult<Value> {
    let tabs: Vec<Value> = state
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            json!({
                "tab_id": tab.id,
                "index": index,
                "active": tab.id == state.active,
                "url": tab.url.clone(),
                "title": tab.title.clone(),
                "status": tab.status.clone(),
            })
        })
        .collect();
    Ok(json!({"tabs": tabs}))
}

fn tab_new_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let url = optional_string(args, "url").unwrap_or_else(|| "about:blank".to_string());
    let active = optional_bool(args, "active", true)?;
    let previous = state.active.clone();
    let id = format!("tab-{}", state.next_tab);
    state.next_tab += 1;
    state.tabs.push(Tab::new(id.clone(), url.clone()));
    if active {
        state.active = id.clone();
    }
    Ok(json!({
        "tab_id": id,
        "url": url,
        "loaded": true,
        "previous_tab_id": previous,
    }))
}

fn tab_close_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let tab_id = require_string(args, "tab_id")?;
    let index = state.tab_index(Some(&tab_id))?;
    let was_active = state.tabs[index].id == state.active;
    state.tabs.remove(index);
    let mut body = json!({"closed": true, "tab_id": tab_id});
    if was_active {
        let next_id = if state.tabs.is_empty() {
            let id = format!("tab-{}", state.next_tab);
            state.next_tab += 1;
            state.tabs.push(Tab::new(id.clone(), "about:blank"));
            state.active = id.clone();
            id
        } else {
            let next_index = index.min(state.tabs.len().saturating_sub(1));
            let id = state.tabs[next_index].id.clone();
            state.active = id.clone();
            id
        };
        if let Some(object) = body.as_object_mut() {
            object.insert("next_tab_id".to_string(), json!(next_id));
        }
    }
    Ok(body)
}

fn tab_switch_answer(args: &Value, state: &mut MockState) -> MockResult<Value> {
    let tab_id = require_string(args, "tab_id")?;
    let index = state.tab_index(Some(&tab_id))?;
    state.active = tab_id.clone();
    let tab = &state.tabs[index];
    Ok(json!({
        "tab_id": tab.id.clone(),
        "url": tab.url.clone(),
        "title": tab.title.clone(),
        "active": true,
    }))
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
        Some(error_object_full(code, message, None)),
    );
    send_json(sink, &body, args).await?;
    Ok(Outcome::Failed)
}

fn response_envelope(command: &Value, args: &Args, payload: Value, error: Option<Value>) -> Value {
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
    error_object_full(code, message, None)
}

fn error_object_full(code: &str, message: &str, details: Option<Value>) -> Value {
    let mut body = json!({
        "code": code,
        "message": message,
        "retryable": retryable(code),
    });
    if let Some(details) = details {
        if let Some(object) = body.as_object_mut() {
            object.insert("details".to_string(), details);
        }
    }
    body
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
