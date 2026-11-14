//! Tool catalogue: names, JSON schemas, descriptions and MCP annotations.

use corvus_proto::{ErrorBody, ErrorCode};
use serde_json::{json, Map, Value};

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

const SPECS: [Spec; 4] = [
    Spec {
        name: "browser_navigate",
        title: "Navigate",
        description: "Open an absolute URL in a tab of the connected browser and wait for the new document to finish loading. The result reports loaded (true when the load completed within the wait budget, false when the page was still busy) and url (the final URL after redirects). A page_snapshot taken after this call reflects the new document.",
        schema: navigate_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "page_snapshot",
        title: "Page snapshot",
        description: "Return a compact tree of the page. Element references from the snapshot are the values browser_click and browser_type accept as target.",
        schema: snapshot_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_click",
        title: "Click",
        description: "Click the element that target refers to. Fails with NOT_INTERACTABLE when the element is covered or disabled.",
        schema: click_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_type",
        title: "Type",
        description: "Focus the element that target refers to and replace its text with the given value.",
        schema: type_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
];

/// Names of every tool, in catalogue order.
pub fn names() -> Vec<&'static str> {
    SPECS.iter().map(|spec| spec.name).collect()
}

/// MCP tool definitions, ready to be embedded in a tools/list result.
pub fn definitions() -> Vec<Value> {
    SPECS
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "title": spec.title,
                "description": spec.description,
                "inputSchema": (spec.schema)(),
                "annotations": {
                    "readOnlyHint": spec.read_only,
                    "destructiveHint": spec.destructive,
                    "idempotentHint": spec.idempotent,
                    "openWorldHint": spec.open_world,
                },
            })
        })
        .collect()
}

fn navigate_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "Absolute URL to open.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to navigate; the active tab when omitted.",
            },
        },
        "required": ["url"],
        "additionalProperties": false,
    })
}

fn snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to capture; the active tab when omitted.",
            },
            "mode": {
                "type": "string",
                "enum": ["interactive", "full"],
                "default": "interactive",
                "description": "interactive returns only actionable nodes, full the whole tree.",
            },
            "max_nodes": {
                "type": "integer",
                "minimum": 40,
                "maximum": 3000,
                "description": "Upper bound on the nodes returned; defaults to the daemon limit.",
            },
        },
        "additionalProperties": false,
    })
}

fn click_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e12, or a CSS selector.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target"],
        "additionalProperties": false,
    })
}

fn type_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e7, or a CSS selector.",
            },
            "text": {
                "type": "string",
                "description": "Text to enter into the element.",
            },
            "clear": {
                "type": "boolean",
                "default": true,
                "description": "Replace the current value instead of appending to it.",
            },
            "blur": {
                "type": "boolean",
                "default": false,
                "description": "Blur the element after typing so change handlers run.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target", "text"],
        "additionalProperties": false,
    })
}

/// Check a tool call against the catalogue and normalise its arguments.
///
/// Everything this function rejects is a caller mistake, so the returned error
/// is always BAD_ARGS.
pub fn validate(name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
    let arguments = match arguments {
        Value::Null => json!({}),
        Value::Object(map) => Value::Object(map.clone()),
        _ => {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "tool arguments must be a JSON object",
            ))
        }
    };
    let mut object = arguments.as_object().cloned().unwrap_or_default();
    match name {
        "browser_navigate" => {
            required_string(&object, "url", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "page_snapshot" => {
            optional_string(&object, "tab_id", name)?;
            match object.get("mode") {
                None => {
                    object.insert("mode".to_string(), json!("interactive"));
                }
                Some(Value::String(mode)) if mode == "full" || mode == "interactive" => {}
                Some(_) => {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "page_snapshot mode must be interactive or full",
                    ))
                }
            }
            match object.get("max_nodes") {
                None | Some(Value::Null) => {}
                Some(value) => match value.as_u64() {
                    Some(nodes) if (40..=3000).contains(&nodes) => {}
                    _ => {
                        return Err(ErrorBody::new(
                            ErrorCode::BadArgs,
                            "page_snapshot max_nodes must be an integer from 40 to 3000",
                        ))
                    }
                },
            }
        }
        "browser_click" => {
            required_string(&object, "target", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "browser_type" => {
            required_string(&object, "target", name)?;
            if object.get("text").and_then(Value::as_str).is_none() {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_type requires a string text argument",
                ));
            }
            optional_string(&object, "tab_id", name)?;
            optional_bool(&object, "clear", name)?;
            optional_bool(&object, "blur", name)?;
            object.entry("clear".to_string()).or_insert(json!(true));
            object.entry("blur".to_string()).or_insert(json!(false));
        }
        other => {
            return Err(
                ErrorBody::new(ErrorCode::BadArgs, format!("unknown tool {other}"))
                    .with_details(json!({ "tool": other, "known": names() })),
            );
        }
    }
    Ok(Value::Object(object))
}

fn required_string(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} requires a non-empty string {field} argument"),
        )),
    }
}

fn optional_bool(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Bool(_)) => Ok(()),
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument {field} must be a boolean"),
        )),
    }
}

fn optional_string(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument {field} must be a string"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_carry_names_schemas_and_annotations() {
        let definitions = definitions();
        assert_eq!(definitions.len(), 4);
        for definition in &definitions {
            assert!(definition.get("name").and_then(Value::as_str).is_some());
            let schema = definition.get("inputSchema").expect("schema");
            assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
            let annotations = definition.get("annotations").expect("annotations");
            for hint in [
                "readOnlyHint",
                "destructiveHint",
                "idempotentHint",
                "openWorldHint",
            ] {
                assert!(
                    annotations.get(hint).and_then(Value::as_bool).is_some(),
                    "missing {hint}"
                );
            }
        }
        assert_eq!(
            names(),
            vec![
                "browser_navigate",
                "page_snapshot",
                "browser_click",
                "browser_type"
            ]
        );
    }

    #[test]
    fn validation_accepts_good_arguments_and_fills_defaults() {
        assert!(validate(
            "browser_navigate",
            &json!({ "url": "https://example.test" })
        )
        .is_ok());
        let snapshot = validate("page_snapshot", &json!({})).expect("snapshot");
        assert_eq!(
            snapshot.get("mode").and_then(Value::as_str),
            Some("interactive")
        );
        let full = validate(
            "page_snapshot",
            &json!({ "mode": "full", "max_nodes": 500 }),
        )
        .expect("full snapshot");
        assert_eq!(full.get("mode").and_then(Value::as_str), Some("full"));
        assert_eq!(full.get("max_nodes").and_then(Value::as_u64), Some(500));
        let click =
            validate("browser_click", &json!({ "target": "@e1", "tab_id": "7" })).expect("click");
        assert_eq!(click.get("tab_id").and_then(Value::as_str), Some("7"));
        let typed =
            validate("browser_type", &json!({ "target": "@e1", "text": "" })).expect("type");
        assert_eq!(typed.get("clear").and_then(Value::as_bool), Some(true));
        assert_eq!(typed.get("blur").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn validation_rejects_bad_arguments_with_bad_args() {
        let cases = [
            ("browser_navigate", json!({})),
            ("browser_navigate", json!({ "url": "" })),
            ("browser_navigate", json!({ "url": 7 })),
            ("page_snapshot", json!({ "mode": "everything" })),
            ("page_snapshot", json!({ "max_nodes": 10 })),
            ("page_snapshot", json!({ "max_nodes": "many" })),
            ("browser_click", json!({})),
            ("browser_click", json!({ "target": "@e1", "tab_id": 3 })),
            ("browser_type", json!({ "target": "@e1" })),
            (
                "browser_type",
                json!({ "target": "@e1", "text": "x", "clear": "yes" }),
            ),
            ("browser_wiggle", json!({})),
        ];
        for (tool, arguments) in cases {
            let error = validate(tool, &arguments).expect_err("must reject");
            assert_eq!(error.code, ErrorCode::BadArgs);
        }
    }
}
