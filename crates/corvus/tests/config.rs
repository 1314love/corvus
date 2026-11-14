use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const CLIENTS: [&str; 5] = ["claude", "cursor", "dsh", "cline", "generic"];
const TRANSPORTS: [&str; 2] = ["stdio", "http"];

struct Captured {
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Captured {
    let output = Command::new(env!("CARGO_BIN_EXE_corvus"))
        .args(args)
        .output()
        .expect("the corvus binary starts");
    assert!(
        output.status.success(),
        "arguments {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Captured {
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    }
}

fn json_output(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim()).expect("stdout is a single JSON document")
}

fn sorted_keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("value is a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

fn canonical(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|error| panic!("{path} is not readable: {error}"))
}

fn expected_location(client: &str) -> Option<&'static str> {
    match client {
        "claude" => Some(".claude.json"),
        "cursor" => Some(".cursor/mcp.json"),
        "dsh" => Some("DSH MCP server settings"),
        "cline" => Some("cline_mcp_settings.json"),
        _ => None,
    }
}

fn assert_snippet(captured: &Captured, transport: &str) -> Value {
    let value = json_output(&captured.stdout);
    assert_eq!(sorted_keys(&value), vec!["mcpServers"]);
    let servers = &value["mcpServers"];
    assert_eq!(sorted_keys(servers), vec!["corvus"]);
    let entry = &servers["corvus"];
    if transport == "stdio" {
        assert_eq!(sorted_keys(entry), vec!["args", "command"]);
        assert_eq!(entry["args"], json!(["serve", "--stdio"]));
        let command = entry["command"]
            .as_str()
            .expect("the stdio command is a string");
        assert_eq!(canonical(command), canonical(env!("CARGO_BIN_EXE_corvus")));
    } else {
        assert_eq!(sorted_keys(entry), vec!["url"]);
        assert_eq!(entry["url"], "http://127.0.0.1:7331/mcp");
    }
    assert!(
        captured.stdout.trim_start().starts_with('{'),
        "stdout must begin with the JSON document, got {:?}",
        captured.stdout
    );
    value
}

#[test]
fn every_client_and_transport_prints_json_on_stdout_and_hints_on_stderr() {
    for client in CLIENTS {
        for transport in TRANSPORTS {
            let captured = run(&["config", "--client", client, "--transport", transport]);
            assert_snippet(&captured, transport);
            match expected_location(client) {
                Some(location) => {
                    assert!(
                        !captured.stderr.trim().is_empty(),
                        "{client}/{transport} must print a location hint on stderr"
                    );
                    assert!(
                        captured.stderr.contains(location),
                        "{client}/{transport} stderr hint should mention {location}: {:?}",
                        captured.stderr
                    );
                    assert!(
                        captured.stderr.trim_start().starts_with("// "),
                        "{client}/{transport} stderr hint should keep the comment form: {:?}",
                        captured.stderr
                    );
                }
                None => assert!(
                    captured.stderr.trim().is_empty(),
                    "generic output must not print a hint: {:?}",
                    captured.stderr
                ),
            }
        }
    }
}

#[test]
fn defaults_are_generic_stdio_with_the_current_executable() {
    let captured = run(&["config"]);
    assert!(
        captured.stderr.trim().is_empty(),
        "generic output must not print a hint: {:?}",
        captured.stderr
    );
    let value = assert_snippet(&captured, "stdio");
    let entry = &value["mcpServers"]["corvus"];
    assert_eq!(sorted_keys(entry), vec!["args", "command"]);
    assert_eq!(entry["args"], json!(["serve", "--stdio"]));
    let command = entry["command"]
        .as_str()
        .expect("the stdio command is a string");
    assert_eq!(canonical(command), canonical(env!("CARGO_BIN_EXE_corvus")));
}

#[test]
fn unknown_clients_and_transports_are_rejected() {
    for args in [
        ["config", "--client", "vim"].as_slice(),
        ["config", "--transport", "sse"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_corvus"))
            .args(args)
            .output()
            .expect("the corvus binary starts");
        assert!(!output.status.success(), "{args:?} should fail");
    }
}
