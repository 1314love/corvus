//! Accessibility-tree to compact snapshot conversion.
//!
//! The CDP path reads `Accessibility.getFullAXTree` and
//! `DOMSnapshot.captureSnapshot` in one pass. This module turns those two
//! protocol payloads into the shared Corvus compact line grammar.

use std::collections::{HashMap, HashSet};

use corvus_domain::SnapshotPolicy;
use serde_json::{json, Value};

use crate::CdpError;

/// Snapshot granularity requested by `page_snapshot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Interactive,
    Full,
}

impl Mode {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, CdpError> {
        match value {
            None | Some("interactive") => Ok(Self::Interactive),
            Some("full") => Ok(Self::Full),
            Some(other) => Err(CdpError::InvalidArgument(format!(
                "page_snapshot mode must be interactive or full, got {other}"
            ))),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Full => "full",
        }
    }
}

/// Snapshot output encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Compact,
    Json,
}

impl Format {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, CdpError> {
        match value {
            None | Some("compact") => Ok(Self::Compact),
            Some("json") => Ok(Self::Json),
            Some(other) => Err(CdpError::InvalidArgument(format!(
                "page_snapshot format must be compact or json, got {other}"
            ))),
        }
    }
}

/// Page-level metadata read from the live document.
#[derive(Clone, Debug)]
pub(crate) struct PageInfo {
    pub url: String,
    pub title: String,
    pub viewport: Viewport,
}

/// Viewport size and scroll offset.
#[derive(Clone, Debug, Default)]
pub(crate) struct Viewport {
    pub width: i64,
    pub height: i64,
    pub scroll_x: f64,
    pub scroll_y: f64,
}

impl Viewport {
    /// Convert a document-coordinate box to a viewport-coordinate box.
    fn to_viewport(&self, bounds: [f64; 4]) -> BBox {
        BBox {
            x: (bounds[0] - self.scroll_x).round() as i64,
            y: (bounds[1] - self.scroll_y).round() as i64,
            width: bounds[2].round().max(0.0) as i64,
            height: bounds[3].round().max(0.0) as i64,
        }
    }

    fn intersects(&self, bbox: &BBox) -> bool {
        bbox.width > 0
            && bbox.height > 0
            && bbox.x < self.width
            && bbox.y < self.height
            && bbox.x + bbox.width > 0
            && bbox.y + bbox.height > 0
    }

    pub(crate) fn as_json(&self) -> Value {
        json!({
            "width": self.width,
            "height": self.height,
            "scrollX": self.scroll_x,
            "scrollY": self.scroll_y,
        })
    }
}

/// Options accepted by the compact renderer.
#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub mode: Mode,
    pub format: Format,
    pub max_nodes: usize,
    pub viewport_only: bool,
    pub include_bbox: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Interactive,
            format: Format::Compact,
            max_nodes: SnapshotPolicy::DEFAULT_MAX_NODES as usize,
            viewport_only: false,
            include_bbox: false,
        }
    }
}

/// Box in viewport coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BBox {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl BBox {
    fn as_json(self) -> Value {
        json!({
            "x": self.x,
            "y": self.y,
            "width": self.width,
            "height": self.height,
        })
    }
}

/// Rendered snapshot plus internal counts used by tests.
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub text: String,
    pub node_count: usize,
    pub truncated: bool,
    pub nodes: Vec<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Element,
    Text,
}

#[derive(Clone, Debug)]
struct Entry {
    kind: Kind,
    backend_id: i64,
    tag: String,
    role: String,
    name: String,
    value: Option<String>,
    states: Vec<&'static str>,
    bbox: Option<BBox>,
    depth: usize,
    interactive: bool,
}

const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "checkbox",
    "radio",
    "textbox",
    "combobox",
    "listbox",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "treeitem",
    "searchbox",
];

const SKIPPED_FULL_ROLES: &[&str] = &[
    "generic",
    "none",
    "presentation",
    "inlinetextbox",
    "linebreak",
    "rootwebarea",
    "statictext",
];

/// Build a compact or JSON snapshot result.
pub(crate) fn render(
    ax: &Value,
    dom: &Value,
    page: &PageInfo,
    options: &Options,
) -> Result<Snapshot, CdpError> {
    let entries = collect_entries(ax, dom, &page.viewport);
    let mut selected = Vec::new();
    let mut truncated = false;
    for entry in entries {
        if !include_entry(&entry, options) {
            continue;
        }
        if options.viewport_only {
            let Some(bbox) = entry.bbox else { continue };
            if !page.viewport.intersects(&bbox) {
                continue;
            }
        }
        selected.push(entry);
    }
    if selected.len() > options.max_nodes {
        selected.truncate(options.max_nodes);
        truncated = true;
    }
    let nodes: Vec<Value> = selected
        .iter()
        .map(|entry| json_node(entry, options.include_bbox))
        .collect();
    let text = if options.format == Format::Compact {
        selected
            .iter()
            .map(|entry| compact_line(entry, options.include_bbox))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    Ok(Snapshot {
        text,
        node_count: selected.len(),
        truncated,
        nodes,
    })
}

fn include_entry(entry: &Entry, options: &Options) -> bool {
    match options.mode {
        Mode::Interactive => entry.interactive,
        Mode::Full => match entry.kind {
            Kind::Text => true,
            Kind::Element => {
                entry.interactive
                    || (!entry.role.is_empty()
                        && !SKIPPED_FULL_ROLES.contains(&entry.role.to_ascii_lowercase().as_str()))
            }
        },
    }
}

fn compact_line(entry: &Entry, include_bbox: bool) -> String {
    match entry.kind {
        Kind::Text => {
            let text = truncate_chars(&clean_text(&entry.name), 240);
            format!("# \"{}\"", escape(&text))
        }
        Kind::Element => {
            let role = if entry.role.is_empty() {
                entry.tag.clone()
            } else {
                entry.role.to_ascii_lowercase()
            };
            let name = truncate_chars(&clean_text(&entry.name), 120);
            let mut line = format!("@c{} {} \"{}\"", entry.backend_id, role, escape(&name));
            if !entry.states.is_empty() {
                line.push_str(&format!(" [{}]", entry.states.join(",")));
            }
            if let Some(value) = entry.value.as_deref() {
                let value = truncate_chars(&clean_text(value), 80);
                if !value.is_empty() {
                    line.push_str(&format!(" value=\"{}\"", escape(&value)));
                }
            }
            if include_bbox {
                if let Some(bbox) = entry.bbox {
                    line.push_str(&format!(
                        " bbox={},{},{},{}",
                        bbox.x, bbox.y, bbox.width, bbox.height
                    ));
                }
            }
            line
        }
    }
}

fn json_node(entry: &Entry, include_bbox: bool) -> Value {
    let mut node = json!({
        "kind": match entry.kind { Kind::Element => "element", Kind::Text => "text" },
        "ref": format!("@c{}", entry.backend_id),
        "tag": entry.tag,
        "role": entry.role.to_ascii_lowercase(),
        "name": clean_text(&entry.name),
        "depth": entry.depth,
        "states": entry.states,
        "attributes": {},
    });
    if let Some(value) = entry.value.as_deref() {
        node["value"] = json!(clean_text(value));
    }
    if include_bbox {
        if let Some(bbox) = entry.bbox {
            node["bbox"] = bbox.as_json();
        }
    }
    node
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn clean_text(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn collect_entries(ax: &Value, dom: &Value, viewport: &Viewport) -> Vec<Entry> {
    let document = dom.pointer("/documents/0");
    let layout = document.and_then(|document| {
        let backend = document.pointer("/nodes/backendNodeId")?.as_array()?;
        let names = document.pointer("/nodes/nodeName")?.as_array()?;
        let strings = document.pointer("/strings").and_then(Value::as_array);
        let node_index = document.pointer("/layout/nodeIndex")?.as_array()?;
        let bounds = document.pointer("/layout/bounds")?.as_array()?;
        let mut by_node_index: HashMap<i64, [f64; 4]> = HashMap::new();
        for (position, index) in node_index.iter().enumerate() {
            let Some(index) = index.as_i64() else {
                continue;
            };
            let Some(bounds) = bounds.get(position).and_then(parse_bounds) else {
                continue;
            };
            by_node_index.insert(index, bounds);
        }
        let mut map: HashMap<i64, (String, Option<[f64; 4]>)> = HashMap::new();
        for (position, backend) in backend.iter().enumerate() {
            let Some(backend) = backend.as_i64() else {
                continue;
            };
            let tag = names
                .get(position)
                .and_then(Value::as_u64)
                .and_then(|index| strings.and_then(|strings| strings.get(index as usize)))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let bbox = by_node_index.get(&(position as i64)).copied();
            map.insert(backend, (tag, bbox));
        }
        Some(map)
    });
    let empty = HashMap::new();
    let layout = layout.as_ref().unwrap_or(&empty);

    let Some(nodes) = ax.get("nodes").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut by_id: HashMap<&str, &Value> = HashMap::new();
    let mut parent_of: HashMap<&str, &str> = HashMap::new();
    for node in nodes {
        let Some(id) = node.get("nodeId").and_then(Value::as_str) else {
            continue;
        };
        by_id.insert(id, node);
        if let Some(parent) = node.get("parentId").and_then(Value::as_str) {
            parent_of.insert(id, parent);
        }
    }
    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    // Roots are nodes whose parent is absent or outside this payload. Keeping
    // the original order preserves document order.
    for node in nodes {
        let Some(id) = node.get("nodeId").and_then(Value::as_str) else {
            continue;
        };
        if parent_of.contains_key(id) {
            continue;
        }
        walk(id, 0, &by_id, layout, viewport, &mut visited, &mut entries);
    }
    // Defensive: malformed payloads may leave cycles or detached nodes.
    for node in nodes {
        let Some(id) = node.get("nodeId").and_then(Value::as_str) else {
            continue;
        };
        if !visited.contains(id) {
            walk(id, 0, &by_id, layout, viewport, &mut visited, &mut entries);
        }
    }
    entries
}

fn walk<'a>(
    id: &'a str,
    depth: usize,
    by_id: &HashMap<&'a str, &'a Value>,
    layout: &HashMap<i64, (String, Option<[f64; 4]>)>,
    viewport: &Viewport,
    visited: &mut HashSet<&'a str>,
    entries: &mut Vec<Entry>,
) {
    if depth > 128 || !visited.insert(id) {
        return;
    }
    let Some(node) = by_id.get(id) else { return };
    let ignored = node
        .get("ignored")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let role = node
        .pointer("/role/value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let name = node
        .pointer("/name/value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let value = node
        .pointer("/value/value")
        .and_then(Value::as_str)
        .map(str::to_string);
    let backend_id = node
        .get("backendDOMNodeId")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let (tag, bounds) = layout
        .get(&backend_id)
        .cloned()
        .unwrap_or_else(|| (String::new(), None));
    let bbox = bounds.map(|bounds| viewport.to_viewport(bounds));
    let states = read_states(node);
    let focusable = read_boolean_property(node, "focusable");
    let role_lower = role.to_ascii_lowercase();
    let interactive = role_lower != "rootwebarea"
        && (INTERACTIVE_ROLES.contains(&role_lower.as_str()) || focusable);
    let tag = if tag.is_empty() {
        match role_lower.as_str() {
            "statictext" => "text".to_string(),
            "rootwebarea" => "html".to_string(),
            other => other.to_string(),
        }
    } else {
        tag
    };
    if !ignored && backend_id > 0 {
        if role_lower == "statictext" || tag == "#text" {
            if !name.is_empty() {
                entries.push(Entry {
                    kind: Kind::Text,
                    backend_id,
                    tag: "text".to_string(),
                    role: "text".to_string(),
                    name,
                    value: None,
                    states: Vec::new(),
                    bbox,
                    depth,
                    interactive: false,
                });
            }
        } else {
            entries.push(Entry {
                kind: Kind::Element,
                backend_id,
                tag,
                role,
                name,
                value,
                states,
                bbox,
                depth,
                interactive,
            });
        }
    }
    let children = node.get("childIds").and_then(Value::as_array);
    if let Some(children) = children {
        let child_depth = if ignored { depth } else { depth + 1 };
        for child in children {
            if let Some(child) = child.as_str() {
                walk(
                    child,
                    child_depth,
                    by_id,
                    layout,
                    viewport,
                    visited,
                    entries,
                );
            }
        }
    }
}

fn parse_bounds(value: &Value) -> Option<[f64; 4]> {
    let values = value.as_array()?;
    if values.len() < 4 {
        return None;
    }
    Some([
        values[0].as_f64()?,
        values[1].as_f64()?,
        values[2].as_f64()?,
        values[3].as_f64()?,
    ])
}

fn read_boolean_property(node: &Value, wanted: &str) -> bool {
    let Some(properties) = node.get("properties").and_then(Value::as_array) else {
        return false;
    };
    properties.iter().any(|property| {
        property
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
            && property
                .pointer("/value/value")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    })
}

fn read_states(node: &Value) -> Vec<&'static str> {
    let mut states = Vec::new();
    if read_boolean_property(node, "disabled") {
        states.push("disabled");
    }
    match node
        .get("properties")
        .and_then(Value::as_array)
        .and_then(|properties| {
            properties.iter().find(|property| {
                property
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name == "checked")
            })
        })
        .and_then(|property| property.pointer("/value/value"))
        .and_then(Value::as_str)
    {
        Some("mixed") => states.push("mixed"),
        Some("true") => states.push("checked"),
        _ => {}
    }
    if read_boolean_property(node, "focused") {
        states.push("focused");
    }
    if read_boolean_property(node, "required") {
        states.push("required");
    }
    if read_boolean_property(node, "expanded") {
        states.push("expanded");
    }
    if read_boolean_property(node, "selected") {
        states.push("selected");
    }
    states
}

/// A single page_find match.
#[derive(Clone, Debug)]
pub(crate) struct FindMatch {
    pub reference: String,
    pub tag: String,
    pub role: String,
    pub text: String,
}

/// Match accessible names and values against a literal or regular expression.
pub(crate) fn find(
    ax: &Value,
    dom: &Value,
    query: &str,
    role: Option<&str>,
    regex: bool,
    limit: usize,
) -> Result<(Vec<FindMatch>, bool), CdpError> {
    let viewport = Viewport::default();
    let entries = collect_entries(ax, dom, &viewport);
    let matcher = if regex {
        let compiled = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()
            .map_err(|error| {
                CdpError::InvalidArgument(format!(
                    "page_find query is not a valid regular expression: {error}"
                ))
            })?;
        Some(compiled)
    } else {
        None
    };
    let wanted_role = role.map(str::to_ascii_lowercase);
    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    let mut truncated = false;
    for entry in entries {
        if !seen.insert(entry.backend_id) {
            continue;
        }
        if let Some(wanted_role) = wanted_role.as_deref() {
            if entry.role.to_ascii_lowercase() != wanted_role
                && entry.tag.to_ascii_lowercase() != wanted_role
            {
                continue;
            }
        }
        let haystack = match entry.kind {
            Kind::Text => entry.name.clone(),
            Kind::Element => format!("{}\n{}", entry.name, entry.value.as_deref().unwrap_or("")),
        };
        let matched = match &matcher {
            Some(matcher) => matcher.is_match(&haystack),
            None => haystack
                .to_ascii_lowercase()
                .contains(&query.to_ascii_lowercase()),
        };
        if !matched {
            continue;
        }
        if matches.len() >= limit {
            truncated = true;
            break;
        }
        let text = if entry.kind == Kind::Text || entry.name.is_empty() {
            entry
                .value
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or(entry.name.clone())
        } else {
            entry.name.clone()
        };
        matches.push(FindMatch {
            reference: format!("@c{}", entry.backend_id),
            tag: entry.tag,
            role: entry.role.to_ascii_lowercase(),
            text: clean_text(&text),
        });
    }
    Ok((matches, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Value) {
        let ax = json!({
            "nodes": [
                { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "Fixture" },
                  "childIds": ["2", "3", "4", "5", "6"] },
                { "nodeId": "2", "parentId": "1", "role": { "value": "heading" }, "name": { "value": "Hello\nWorld" },
                  "backendDOMNodeId": 11, "childIds": ["7"] },
                { "nodeId": "7", "parentId": "2", "role": { "value": "StaticText" }, "name": { "value": "Hello World" },
                  "backendDOMNodeId": 12 },
                { "nodeId": "3", "parentId": "1", "role": { "value": "button" }, "name": { "value": "Go" },
                  "backendDOMNodeId": 13,
                  "properties": [
                      { "name": "disabled", "value": { "value": true } },
                      { "name": "focusable", "value": { "value": true } }
                  ] },
                { "nodeId": "4", "parentId": "1", "role": { "value": "textbox" }, "name": { "value": "Name" },
                  "value": { "value": "Ada" }, "backendDOMNodeId": 14,
                  "properties": [
                      { "name": "focusable", "value": { "value": true } }
                  ] },
                { "nodeId": "5", "parentId": "1", "role": { "value": "checkbox" }, "name": { "value": "Agree" },
                  "backendDOMNodeId": 15,
                  "properties": [
                      { "name": "checked", "value": { "value": "mixed" } },
                      { "name": "focusable", "value": { "value": true } }
                  ] },
                { "nodeId": "6", "parentId": "1", "ignored": true, "childIds": ["8"] },
                { "nodeId": "8", "parentId": "6", "role": { "value": "link" }, "name": { "value": "Docs" },
                  "backendDOMNodeId": 16,
                  "properties": [
                      { "name": "focusable", "value": { "value": true } }
                  ] }
            ]
        });
        let dom = json!({
            "documents": [{
                "nodes": {
                    "backendNodeId": [11, 12, 13, 14, 15, 16],
                    "nodeName": [0, 0, 1, 2, 3, 4],
                },
                "strings": ["#document", "h1", "button", "input", "a"],
                "layout": {
                    "nodeIndex": [0, 1, 2, 3, 4, 5],
                    "bounds": [
                        [0.0, 0.0, 800.0, 600.0],
                        [10.5, 21.4, 200.0, 30.0],
                        [40.0, 1200.0, 100.0, 40.0],
                        [40.0, 300.0, 200.0, 30.0],
                        [40.0, 340.0, 20.0, 20.0],
                        [40.0, 700.0, 80.0, 20.0]
                    ]
                }
            }]
        });
        (ax, dom)
    }

    fn page() -> PageInfo {
        PageInfo {
            url: "https://example.test/".into(),
            title: "Fixture".into(),
            viewport: Viewport {
                width: 800,
                height: 600,
                scroll_x: 0.0,
                scroll_y: 500.0,
            },
        }
    }

    #[test]
    fn compact_interactive_uses_backend_refs_and_viewport_coordinates() {
        let (ax, dom) = fixture();
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                include_bbox: true,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(!snapshot.truncated);
        assert_eq!(snapshot.node_count, 4);
        assert_eq!(
            snapshot.text,
            "@c13 button \"Go\" [disabled] bbox=40,700,100,40\n\
             @c14 textbox \"Name\" value=\"Ada\" bbox=40,-200,200,30\n\
             @c15 checkbox \"Agree\" [mixed] bbox=40,-160,20,20\n\
             @c16 link \"Docs\" bbox=40,200,80,20"
        );
        assert!(snapshot.nodes.iter().all(|node| node["ref"]
            .as_str()
            .is_some_and(|reference| reference.starts_with("@c"))));
    }

    #[test]
    fn interactive_mode_excludes_the_root_web_area() {
        let ax = json!({
            "nodes": [
                { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "Page" },
                  "backendDOMNodeId": 5,
                  "properties": [
                      { "name": "focusable", "value": { "value": true } }
                  ],
                  "childIds": ["2"] },
                { "nodeId": "2", "parentId": "1", "role": { "value": "button" }, "name": { "value": "Go" },
                  "backendDOMNodeId": 6,
                  "properties": [
                      { "name": "focusable", "value": { "value": true } }
                  ] }
            ]
        });
        let dom = json!({
            "documents": [{
                "nodes": { "backendNodeId": [5, 6], "nodeName": [0, 1] },
                "strings": ["html", "button"],
                "layout": {
                    "nodeIndex": [0, 1],
                    "bounds": [[0.0, 0.0, 800.0, 600.0], [10.0, 10.0, 20.0, 20.0]]
                }
            }]
        });
        let snapshot = render(&ax, &dom, &page(), &Options::default()).expect("snapshot");
        assert_eq!(snapshot.node_count, 1);
        assert_eq!(snapshot.text, "@c6 button \"Go\"");
    }

    #[test]
    fn full_mode_emits_text_lines_and_honours_max_nodes() {
        let (ax, dom) = fixture();
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                mode: Mode::Full,
                max_nodes: 2,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(snapshot.truncated);
        assert_eq!(snapshot.node_count, 2);
        assert_eq!(
            snapshot.text,
            "@c11 heading \"Hello World\"\n# \"Hello World\""
        );
    }

    #[test]
    fn viewport_only_filters_offscreen_nodes() {
        let (ax, dom) = fixture();
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                mode: Mode::Full,
                viewport_only: true,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(!snapshot.text.contains("@c13"));
        assert!(!snapshot.text.contains("@c14"));
        assert!(snapshot.text.contains("@c16"));
        assert!(!snapshot.text.contains("# \"Hello World\""));
    }

    #[test]
    fn viewport_only_keeps_visible_nodes_and_emits_bboxes() {
        let (ax, dom) = fixture();
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                mode: Mode::Full,
                viewport_only: true,
                include_bbox: true,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(snapshot
            .text
            .contains("@c16 link \"Docs\" bbox=40,200,80,20"));
        assert!(!snapshot.text.contains("@c13"));
        assert!(!snapshot.text.contains("@c14"));
    }

    #[test]
    fn json_mode_keeps_the_legacy_node_shape() {
        let (ax, dom) = fixture();
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                format: Format::Json,
                include_bbox: true,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(snapshot.text.is_empty());
        let first = &snapshot.nodes[0];
        for key in ["kind", "ref", "tag", "role", "name", "bbox", "depth"] {
            assert!(first.get(key).is_some(), "missing {key}");
        }
        assert_eq!(first["ref"], "@c13");
        assert_eq!(first["kind"], "element");
    }

    #[test]
    fn names_and_values_are_escaped_and_truncated() {
        let ax = json!({
            "nodes": [
                { "nodeId": "1", "role": { "value": "button" },
                  "name": { "value": format!("{}{}", "a\"b\\c\n", "x".repeat(200)) },
                  "backendDOMNodeId": 7,
                  "properties": [
                      { "name": "focusable", "value": { "value": true } }
                  ] }
            ]
        });
        let dom = json!({
            "documents": [{
                "nodes": { "backendNodeId": [7], "nodeName": [0] },
                "strings": ["button"],
                "layout": { "nodeIndex": [0], "bounds": [[0.0, 0.0, 10.0, 10.0]] }
            }]
        });
        let snapshot = render(
            &ax,
            &dom,
            &page(),
            &Options {
                include_bbox: false,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(snapshot.text.starts_with("@c7 button \"a\\\"b\\\\c x"));
        let name = snapshot.nodes[0]["name"].as_str().expect("name");
        assert_eq!(name.chars().count(), 206);
        assert!(!snapshot.text.contains('\n'));
    }

    #[test]
    fn include_bbox_option_adds_or_omits_box_coordinates() {
        let (ax, dom) = fixture();
        let without = render(&ax, &dom, &page(), &Options::default()).expect("snapshot");
        assert!(without.text.contains("@c16 link \"Docs\""));
        assert!(!without.text.contains("bbox="));

        let with = render(
            &ax,
            &dom,
            &page(),
            &Options {
                include_bbox: true,
                ..Options::default()
            },
        )
        .expect("snapshot");
        assert!(with.text.contains("@c16 link \"Docs\" bbox=40,200,80,20"));
    }

    #[test]
    fn page_find_supports_literal_regex_and_role_filters() {
        let (ax, dom) = fixture();
        let (literal, _) = find(&ax, &dom, "ada", None, false, 20).expect("literal");
        assert_eq!(literal.len(), 1);
        assert_eq!(literal[0].reference, "@c14");
        let (roles, _) = find(&ax, &dom, "ag", Some("checkbox"), false, 20).expect("role");
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].text, "Agree");
        let (regex, _) = find(&ax, &dom, "Do", None, true, 20).expect("regex");
        assert_eq!(regex[0].reference, "@c16");
        assert!(find(&ax, &dom, "(", None, true, 20).is_err());
    }
}
