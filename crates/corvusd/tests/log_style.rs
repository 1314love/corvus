//! Enforces the event schema from `docs/08-logging.md`.
//!
//! The scan walks `crates/**/*.rs`, masks comments and string bodies, and
//! inspects every `tracing::<level>!` call: the message must be a bare
//! lowercase literal and every field must be named.

use std::fs;
use std::path::{Path, PathBuf};

const MESSAGE_LIMIT: usize = 48;
const ID_NOUNS: &[&str] = &[
    "browser",
    "session",
    "command",
    "request",
    "envelope",
    "tab",
    "connection",
    "id",
];

struct Event {
    line: usize,
    message: String,
    fields: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits under crates/")
        .to_path_buf()
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn scan_workspace() -> (usize, Vec<String>) {
    let root = repo_root();
    let mut files = Vec::new();
    collect_sources(&root.join("crates"), &mut files);
    let mut total = 0;
    let mut problems = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).expect("read a crate source file");
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for event in scan_events(&text) {
            total += 1;
            for problem in check_event(&event) {
                problems.push(format!("{relative}:{} {problem}", event.line));
            }
        }
    }
    (total, problems)
}

/// Mask every byte that belongs to a comment or a string body.
fn mask_source(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut mask = vec![true; bytes.len()];
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    mask[index] = false;
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut depth = 0usize;
                while index < bytes.len() {
                    if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                        depth += 1;
                        mask[index] = false;
                        mask[index + 1] = false;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        mask[index] = false;
                        mask[index + 1] = false;
                        index += 2;
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        mask[index] = false;
                        index += 1;
                    }
                }
            }
            b'"' => {
                let end = string_end(bytes, index);
                for slot in &mut mask[index..end] {
                    *slot = false;
                }
                index = end;
            }
            b'\'' => {
                let end = char_end(bytes, index);
                for slot in &mut mask[index..end] {
                    *slot = false;
                }
                index = end;
            }
            _ => index += 1,
        }
    }
    mask
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut hashes = 0;
    let mut back = start;
    while back > 0 && bytes[back - 1] == b'#' {
        hashes += 1;
        back -= 1;
    }
    let raw = back > 0 && bytes[back - 1] == b'r';
    if raw {
        let mut cursor = start + 1;
        while cursor + hashes < bytes.len() {
            if bytes[cursor] == b'"'
                && bytes[cursor + 1..cursor + 1 + hashes] == vec![b'#'; hashes][..]
            {
                return cursor + 1 + hashes;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'"' => return cursor + 1,
            _ => cursor += 1,
        }
    }
    bytes.len()
}

fn char_end(bytes: &[u8], start: usize) -> usize {
    if bytes.get(start + 1) == Some(&b'\\') {
        let mut cursor = start + 2;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\'' {
                return cursor + 1;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    if bytes.get(start + 2) == Some(&b'\'') {
        return start + 3;
    }
    start + 1
}

fn call_end(bytes: &[u8], mask: &[bool], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut index = open;
    while index < bytes.len() {
        if mask[index] {
            match bytes[index] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn split_arguments(bytes: &[u8], mask: &[bool], start: usize, end: usize) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut from = start;
    let mut index = start;
    while index < end {
        if mask[index] {
            match bytes[index] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    args.push(String::from_utf8_lossy(&bytes[from..index]).to_string());
                    from = index + 1;
                }
                _ => {}
            }
        }
        index += 1;
    }
    args.push(String::from_utf8_lossy(&bytes[from..end]).to_string());
    args
}

fn scan_events(text: &str) -> Vec<Event> {
    let bytes = text.as_bytes();
    let mask = mask_source(text);
    let mut events = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some(offset) = text[index..].find("tracing::") else {
            break;
        };
        let start = index + offset;
        index = start + 1;
        if !mask[start] {
            continue;
        }
        let mut cursor = start + "tracing::".len();
        let Some(level) = ["error", "warn", "info", "debug", "trace"]
            .into_iter()
            .find(|name| bytes[cursor..].starts_with(name.as_bytes()))
        else {
            continue;
        };
        cursor += level.len();
        if bytes.get(cursor) != Some(&b'!') {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && (bytes[cursor].is_ascii_whitespace() || !mask[cursor]) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'(') {
            continue;
        }
        let Some(end) = call_end(bytes, &mask, cursor) else {
            continue;
        };
        let mut args = split_arguments(bytes, &mask, cursor + 1, end);
        let message = args.pop().unwrap_or_default();
        let line = text[..start].matches('\n').count() + 1;
        events.push(Event {
            line,
            message,
            fields: args,
        });
    }
    events
}

fn check_event(event: &Event) -> Vec<String> {
    let mut problems = check_message(&event.message);
    for field in &event.fields {
        problems.extend(check_field(field));
    }
    problems
}

fn check_message(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return vec![format!("message is not a bare literal: {trimmed}")];
    }
    let message = &trimmed[1..trimmed.len() - 1];
    let mut problems = Vec::new();
    if message.contains('\\') {
        problems.push(format!("message uses escapes: {message}"));
    }
    if message.len() > MESSAGE_LIMIT {
        problems.push(format!(
            "message is {} characters, limit is {MESSAGE_LIMIT}: {message}",
            message.len()
        ));
    }
    if !message.is_ascii() {
        problems.push(format!("message is not ASCII: {message}"));
    }
    if message.contains('{') || message.contains('}') {
        problems.push(format!("message carries a placeholder: {message}"));
    }
    if message.contains(';') {
        problems.push(format!("message carries a semicolon: {message}"));
    }
    if !message
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
    {
        problems.push(format!("message does not start lowercase: {message}"));
    }
    if message.ends_with(['.', ',', ':', ';', '!', '?']) {
        problems.push(format!("message ends with punctuation: {message}"));
    }
    if message.trim() != message || message.contains("  ") {
        problems.push(format!("message has stray spaces: {message}"));
    }
    problems
}

fn check_field(raw: &str) -> Vec<String> {
    let text = raw.trim();
    if text.is_empty() {
        return vec!["empty tracing argument".to_string()];
    }
    let Some((name, _)) = text.split_once('=') else {
        return vec![format!("field is not named: {text}")];
    };
    let name = name.trim();
    let mut problems = Vec::new();
    if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        problems.push(format!("field name is not snake case: {name}"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        problems.push(format!("field name is not snake case: {name}"));
    }
    if name == "message" {
        problems.push("field shadows the event text: message".to_string());
    }
    for noun in ID_NOUNS {
        if (name == *noun || name.ends_with(&format!("_{noun}"))) && !name.ends_with("_id") {
            problems.push(format!("field holds an identifier but lacks _id: {name}"));
        }
    }
    problems
}

fn check_source(source: &str) -> (usize, Vec<String>) {
    let events = scan_events(source);
    let problems = events.iter().flat_map(check_event).collect();
    (events.len(), problems)
}

#[test]
fn every_tracing_event_follows_the_schema() {
    let (total, problems) = scan_workspace();
    assert!(total > 0, "scan found no tracing events");
    assert!(
        problems.is_empty(),
        "log schema violations:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_scan_covers_the_workspace() {
    let (total, _) = scan_workspace();
    assert!(total >= 30, "scan found only {total} tracing events");
}

#[test]
fn the_checkers_reject_violations() {
    let source = r#"
fn demo() {
    tracing::info!(browser = %browser, "Browser connected.");
    tracing::warn!(error = %error, "could not open a session while the database lock was held by another writer");
    tracing::debug!(%error, "positional field");
    tracing::info!(message = %text, "field shadows the text");
    tracing::info!(request = %id, "lifecycle without a suffix");
}
"#;
    let (total, problems) = check_source(source);
    assert_eq!(total, 5, "the scan should find five events");
    assert_eq!(problems.len(), 7, "problems: {problems:#?}");
}
