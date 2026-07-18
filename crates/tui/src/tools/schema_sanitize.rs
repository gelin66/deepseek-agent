//! Schema sanitizer for tool `input_schema` before sending to provider APIs.
//!
//! DeepSeek's `/beta/chat/completions` strict tool mode accepts a documented
//! subset of JSON Schema. General/MCP schemas often contain optional values,
//! composition, or keywords outside that subset. Strict preparation validates
//! the complete catalog without rewriting it; incompatible catalogs retain
//! ordinary tool calling instead of risking a 400 or changing tool semantics.
//!
//! The helpers below are called directly by retained generic TUI clients.
//! Canonical DeepSeek tool planning and whole-catalog strict compatibility are
//! owned by `crates/deepseek`; this module does not sit on that production path.

use serde_json::{Map, Value};

use crate::models::Tool;

/// Sanitize a JSON Schema in-place for DeepSeek strict-tool compatibility.
///
/// Applies a sequence of normalisations chosen to be semantics-preserving:
/// - Collapse `{"anyOf":[X, {"type":"null"}]}` → `X ∪ {"nullable": true}`
/// - Inject `"properties": {}` on bare-object schemas
/// - Prune dangling `required` entries
/// - Collapse single-element `oneOf` / `allOf`
/// - Walk recursively through all subschemas
pub fn sanitize(schema: &mut Value) {
    collapse_nullable_unions(schema);
    inject_properties_on_bare_objects(schema);
    prune_dangling_required(schema);
    collapse_single_element_unions(schema);
    // Recurse into all sub-schemas
    if let Some(obj) = schema.as_object_mut() {
        for (_, v) in obj.iter_mut() {
            sanitize(v);
        }
    } else if let Some(arr) = schema.as_array_mut() {
        for v in arr.iter_mut() {
            sanitize(v);
        }
    }
}

/// Prepare a complete active tool set for DeepSeek strict function-calling.
///
/// DeepSeek requires every function in one request to opt into strict mode.
/// Preflight the complete set without mutating schemas. An incompatible tool
/// leaves every schema untouched and disables strict mode for the set.
/// Returns `true` only when every tool in the set can use strict mode.
pub fn prepare_tools_for_strict_mode(tools: &mut [Tool]) -> bool {
    if tools
        .iter()
        .any(|tool| !codewhale_deepseek::strict_schema_supported(&tool.input_schema))
    {
        for tool in tools {
            tool.strict = None;
        }
        return false;
    }

    for tool in tools {
        tool.strict = Some(true);
    }
    true
}

/// Sanitize a schema for OpenAI Responses function tools.
///
/// The Responses API requires the top-level `parameters` schema to be an object
/// and rejects top-level `oneOf` / `anyOf` / `allOf` / `enum` / `not`. Keep the
/// schema permissive rather than changing tool semantics: merge any root
/// alternative properties we can see, then remove the root-only composition
/// keywords while preserving nested schemas.
///
/// Returns a short description note when root composition constraints with
/// meaningful `required` groups are dropped.
pub fn sanitize_for_responses(schema: &mut Value) -> Option<String> {
    let constraint_note = schema
        .as_object()
        .and_then(root_composition_constraint_note);

    sanitize(schema);

    if !schema.is_object() {
        *schema = Value::Object(Map::new());
    }

    let Some(obj) = schema.as_object_mut() else {
        return constraint_note;
    };

    merge_root_composition_properties(obj);
    obj.insert("type".into(), Value::String("object".to_string()));
    obj.remove("oneOf");
    obj.remove("anyOf");
    obj.remove("allOf");
    obj.remove("enum");
    obj.remove("not");
    ensure_properties_object(obj);
    prune_dangling_required(schema);
    constraint_note
}

/// Collapse `{"anyOf":[X, {"type":"null"}]}` → `X ∪ {"nullable": true}`.
///
/// Same treatment for `oneOf`. Only collapses when exactly one non-null
/// member and exactly one null-type member are present.
fn collapse_nullable_unions(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    for key in ["anyOf", "oneOf"] {
        let members: Vec<Value> = match obj.get(key).and_then(|v| v.as_array()) {
            Some(arr) => arr.clone(),
            None => continue,
        };
        let (nulls, nons): (Vec<_>, Vec<_>) = members.into_iter().partition(is_null_type);
        if nulls.len() == 1 && nons.len() == 1 {
            obj.remove(key);
            if let Value::Object(non_obj) = nons.into_iter().next().unwrap() {
                for (k, v) in non_obj {
                    if k != "type" || v != "null" {
                        obj.insert(k, v);
                    }
                }
            }
            obj.insert("nullable".into(), Value::Bool(true));
        }
    }
}

fn is_null_type(v: &Value) -> bool {
    v.as_object()
        .and_then(|o| o.get("type"))
        .and_then(|t| t.as_str())
        == Some("null")
}

/// Bare `{"type": "object"}` (no `properties`, no `additionalProperties`)
/// → inject `"properties": {}` so DeepSeek's strict validator doesn't 400.
fn inject_properties_on_bare_objects(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    if obj.get("type").and_then(|t| t.as_str()) != Some("object") {
        return;
    }
    if obj.contains_key("properties") || obj.contains_key("additionalProperties") {
        return;
    }
    obj.insert("properties".into(), Value::Object(Map::new()));
}

/// Remove entries from `required` that aren't keys in `properties`.
fn prune_dangling_required(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    // Collect known property names first (immutable borrow), then prune.
    let known_keys: Vec<String> = obj
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default();
    let Some(required) = obj.get_mut("required").and_then(|v| v.as_array_mut()) else {
        return;
    };
    required.retain(|entry| {
        entry
            .as_str()
            .is_some_and(|k| known_keys.iter().any(|known| known == k))
    });
    if required.is_empty() {
        obj.remove("required");
    }
}

/// Collapse `{"oneOf": [X]}` → X, same for `allOf`.
///
/// Single-element unions are semantically equivalent to the element itself;
/// DeepSeek's strict validator doesn't always flatten them.
fn collapse_single_element_unions(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    for key in ["oneOf", "allOf", "anyOf"] {
        let single = match obj.get(key).and_then(|v| v.as_array()) {
            Some(arr) if arr.len() == 1 => arr[0].clone(),
            _ => continue,
        };
        obj.remove(key);
        if let Value::Object(inner) = single {
            for (k, v) in inner {
                if !obj.contains_key(&k) {
                    obj.insert(k, v);
                }
            }
        }
    }
}

fn ensure_properties_object(obj: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let needs_replacement = !matches!(obj.get("properties"), Some(Value::Object(_)));
    if needs_replacement {
        obj.insert("properties".into(), Value::Object(Map::new()));
    }
    obj.get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("properties was just ensured as object")
}

fn merge_root_composition_properties(obj: &mut Map<String, Value>) {
    let mut merged = Map::new();
    for key in ["oneOf", "anyOf", "allOf"] {
        let Some(items) = obj.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(properties) = item.get("properties").and_then(Value::as_object) else {
                continue;
            };
            for (name, schema) in properties {
                merged.entry(name.clone()).or_insert_with(|| schema.clone());
            }
        }
    }

    if merged.is_empty() {
        return;
    }

    let properties = ensure_properties_object(obj);
    for (name, schema) in merged {
        properties.entry(name).or_insert(schema);
    }
}

fn root_composition_constraint_note(obj: &Map<String, Value>) -> Option<String> {
    for (key, prefix) in [
        ("oneOf", "Exactly one"),
        ("anyOf", "At least one"),
        ("allOf", "All"),
    ] {
        let Some(items) = obj.get(key).and_then(Value::as_array) else {
            continue;
        };
        let mut groups: Vec<String> = items.iter().filter_map(required_group_label).collect();
        groups.sort();
        groups.dedup();
        if groups.len() >= 2 {
            return Some(format!(
                "{prefix} of these parameter groups must be provided: {}.",
                groups.join(" | ")
            ));
        }
    }
    None
}

fn required_group_label(item: &Value) -> Option<String> {
    let mut names: Vec<String> = item
        .get("required")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(|name| format!("`{name}`"))
        .collect();
    if names.is_empty() {
        None
    } else {
        names.sort();
        names.dedup();
        Some(names.join(" + "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_tool(name: &str, input_schema: Value) -> Tool {
        Tool {
            tool_type: None,
            name: name.to_string(),
            description: name.to_string(),
            input_schema,
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }
    }

    #[test]
    fn collapses_nullable_anyof() {
        let mut schema = json!({
            "anyOf": [
                {"type": "string"},
                {"type": "null"}
            ]
        });
        sanitize(&mut schema);
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["nullable"], true);
        assert!(schema.get("anyOf").is_none());
    }

    #[test]
    fn collapses_nullable_oneof() {
        let mut schema = json!({
            "oneOf": [
                {"type": "null"},
                {"type": "integer", "minimum": 0}
            ]
        });
        sanitize(&mut schema);
        assert_eq!(schema["type"], "integer");
        assert_eq!(schema["minimum"], 0);
        assert_eq!(schema["nullable"], true);
    }

    #[test]
    fn preserves_non_null_anyof() {
        let original = json!({
            "anyOf": [
                {"type": "string"},
                {"type": "integer"}
            ]
        });
        let mut schema = original.clone();
        sanitize(&mut schema);
        // Multi-typed anyOf should collapse to single element after
        // recursive walk — but here neither is null so the collapse
        // doesn't trigger. The anyOf array itself remains.
        assert!(schema.get("anyOf").is_some());
    }

    #[test]
    fn injects_properties_on_bare_object() {
        let mut schema = json!({"type": "object"});
        sanitize(&mut schema);
        assert!(schema.get("properties").is_some());
        assert_eq!(schema["properties"], json!({}));
    }

    #[test]
    fn does_not_inject_properties_when_present() {
        let mut schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let expected = schema.clone();
        sanitize(&mut schema);
        assert_eq!(schema, expected);
    }

    #[test]
    fn prunes_dangling_required() {
        let mut schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name", "email"]
        });
        sanitize(&mut schema);
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
    }

    #[test]
    fn removes_required_when_all_pruned() {
        let mut schema = json!({
            "type": "object",
            "properties": {},
            "required": ["ghost"]
        });
        sanitize(&mut schema);
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn collapses_single_element_oneof() {
        let mut schema = json!({
            "oneOf": [{"type": "string", "minLength": 1}]
        });
        sanitize(&mut schema);
        assert!(schema.get("oneOf").is_none());
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 1);
    }

    #[test]
    fn collapses_single_element_anyof() {
        let mut schema = json!({
            "anyOf": [{"type": "boolean"}]
        });
        sanitize(&mut schema);
        assert!(schema.get("anyOf").is_none());
        assert_eq!(schema["type"], "boolean");
    }

    #[test]
    fn recursive_walk_into_properties() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "opt_name": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ]
                }
            }
        });
        sanitize(&mut schema);
        let prop = &schema["properties"]["opt_name"];
        assert_eq!(prop["type"], "string");
        assert_eq!(prop["nullable"], true);
    }

    #[test]
    fn recursive_walk_into_items() {
        let mut schema = json!({
            "type": "array",
            "items": {
                "anyOf": [
                    {"type": "integer"},
                    {"type": "null"}
                ]
            }
        });
        sanitize(&mut schema);
        let items = &schema["items"];
        assert_eq!(items["type"], "integer");
        assert_eq!(items["nullable"], true);
    }

    #[test]
    fn nested_anyof_in_anyof_collapses() {
        // Pydantic can nest unions: Optional[Union[str, int]].
        let mut schema = json!({
            "anyOf": [
                {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "integer"}
                    ]
                },
                {"type": "null"}
            ]
        });
        sanitize(&mut schema);
        // Outer anyOf is single non-null → collapsed. Inner anyOf is
        // multi-typed → preserved, but the outer null is handled.
        assert_eq!(schema["nullable"], true);
        assert!(schema.get("anyOf").is_some());
    }

    #[test]
    fn idempotent() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "maybe": {
                    "anyOf": [{"type": "integer"}, {"type": "null"}]
                }
            },
            "required": ["name", "missing_field"]
        });
        sanitize(&mut schema);
        let after_first = schema.clone();
        sanitize(&mut schema);
        assert_eq!(schema, after_first, "sanitize must be idempotent");
    }

    #[test]
    fn strict_mode_is_all_or_nothing_for_mixed_catalog() {
        let mut tools = vec![
            test_tool(
                "lookup",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    },
                    "required": []
                }),
            ),
            test_tool(
                "either",
                json!({
                    "type": "object",
                    "properties": {
                        "a": {"type": "string"},
                        "b": {"type": "string"}
                    },
                    "anyOf": [
                        {"required": ["a"]},
                        {"required": ["b"]}
                    ]
                }),
            ),
            test_tool(
                "nested",
                json!({
                    "type": "object",
                    "properties": {
                        "value": {
                            "oneOf": [
                                {"type": "string"},
                                {"type": "integer"}
                            ]
                        }
                    }
                }),
            ),
        ];
        tools[0].strict = Some(true);
        tools[1].strict = Some(false);
        let original_schemas: Vec<Value> =
            tools.iter().map(|tool| tool.input_schema.clone()).collect();

        assert!(!prepare_tools_for_strict_mode(&mut tools));
        assert!(tools.iter().all(|tool| tool.strict.is_none()));
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.input_schema.clone())
                .collect::<Vec<_>>(),
            original_schemas,
            "incompatible catalogs must not partially sanitize schemas"
        );
    }

    #[test]
    fn strict_mode_rejects_nested_unsupported_composition() {
        let mut tools = vec![Tool {
            tool_type: None,
            name: "nested".to_string(),
            description: "Nested oneOf".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "integer"}
                        ]
                    }
                }
            }),
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }];

        let original_schema = tools[0].input_schema.clone();
        assert!(!prepare_tools_for_strict_mode(&mut tools));
        assert_eq!(tools[0].strict, None);
        assert_eq!(tools[0].input_schema, original_schema);
    }

    #[test]
    fn strict_mode_marks_compatible_tools_strict() {
        let mut tools = vec![Tool {
            tool_type: None,
            name: "lookup".to_string(),
            description: "Lookup".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }];

        assert!(prepare_tools_for_strict_mode(&mut tools));
        assert_eq!(tools[0].strict, Some(true));
        assert_eq!(tools[0].input_schema["required"], json!(["query"]));
        assert_eq!(tools[0].input_schema["additionalProperties"], false);
    }

    #[test]
    fn strict_mode_preserves_deepseek_supported_nested_any_of() {
        let mut tools = vec![test_tool(
            "lookup_account",
            json!({
                "type": "object",
                "properties": {
                    "account": {
                        "anyOf": [
                            {"type": "string", "format": "email"},
                            {"type": "string", "pattern": "^\\d{11}$"}
                        ]
                    }
                },
                "required": ["account"],
                "additionalProperties": false
            }),
        )];
        let original_any_of = tools[0].input_schema["properties"]["account"]["anyOf"].clone();

        assert!(prepare_tools_for_strict_mode(&mut tools));
        assert_eq!(tools[0].strict, Some(true));
        assert_eq!(
            tools[0].input_schema["properties"]["account"]["anyOf"], original_any_of,
            "DeepSeek beta documents nested anyOf as a supported strict type"
        );
    }

    #[test]
    fn strict_mode_preserves_deepseek_supported_ref_and_definitions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "author": {"$ref": "#/$def/author"}
            },
            "required": ["author"],
            "additionalProperties": false,
            "$def": {
                "author": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "email": {"type": "string", "format": "email"}
                    },
                    "required": ["name", "email"],
                    "additionalProperties": false
                }
            }
        });
        let mut tools = vec![test_tool("save_author", schema)];

        assert!(prepare_tools_for_strict_mode(&mut tools));
        assert_eq!(tools[0].strict, Some(true));
        assert_eq!(
            tools[0].input_schema["properties"]["author"]["$ref"],
            "#/$def/author"
        );
        assert_eq!(
            tools[0].input_schema["$def"]["author"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn strict_mode_rejects_optional_or_implicitly_open_objects_without_mutation() {
        for schema in [
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": [],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }),
        ] {
            let original = schema.clone();
            let mut tools = vec![test_tool("lookup", schema)];
            assert!(!prepare_tools_for_strict_mode(&mut tools));
            assert_eq!(tools[0].strict, None);
            assert_eq!(tools[0].input_schema, original);
        }
    }

    #[test]
    fn strict_mode_rejects_unsupported_bounds_and_nullable_keyword() {
        for property in [
            json!({"type": "string", "minLength": 1}),
            json!({"type": "array", "items": {"type": "string"}, "maxItems": 5}),
            json!({"type": "string", "nullable": true}),
        ] {
            let schema = json!({
                "type": "object",
                "properties": {"value": property},
                "required": ["value"],
                "additionalProperties": false
            });
            let original = schema.clone();
            let mut tools = vec![test_tool("bounded", schema)];
            assert!(!prepare_tools_for_strict_mode(&mut tools));
            assert_eq!(tools[0].input_schema, original);
        }
    }

    #[test]
    fn strict_mode_rejects_undocumented_composition_and_type_forms() {
        for property in [
            json!({"type": ["string", "null"]}),
            json!({"type": "string", "format": "date-time"}),
            json!({"type": "string", "const": "fixed"}),
            json!({"type": "string", "default": "fallback"}),
            json!({"type": "array", "items": {"type": "string"}, "uniqueItems": true}),
            json!({"type": "array", "items": {"type": "string"}, "pattern": "x"}),
            json!({"not": {"type": "string"}}),
        ] {
            let schema = json!({
                "type": "object",
                "properties": {"value": property},
                "required": ["value"],
                "additionalProperties": false
            });
            let original = schema.clone();
            let mut tools = vec![test_tool("strict_subset", schema)];
            assert!(!prepare_tools_for_strict_mode(&mut tools));
            assert_eq!(tools[0].strict, None);
            assert_eq!(tools[0].input_schema, original);
        }
    }

    #[test]
    fn strict_mode_accepts_documented_numeric_keywords() {
        let schema = json!({
            "type": "object",
            "properties": {
                "score": {
                    "type": "integer",
                    "const": 5,
                    "default": 5,
                    "minimum": 1,
                    "maximum": 5,
                    "multipleOf": 1
                }
            },
            "required": ["score"],
            "additionalProperties": false
        });
        let original = schema.clone();
        let mut tools = vec![test_tool("numeric_subset", schema)];

        assert!(prepare_tools_for_strict_mode(&mut tools));
        assert_eq!(tools[0].strict, Some(true));
        assert_eq!(tools[0].input_schema, original);
    }

    #[test]
    fn responses_sanitize_removes_root_composition_from_apply_patch_shape() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "patch": {"type": "string"},
                "changes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "content": {"type": "string"}
                        },
                        "required": ["path", "content"]
                    }
                }
            },
            "oneOf": [
                {"required": ["patch"]},
                {"required": ["changes"]}
            ]
        });

        let note = sanitize_for_responses(&mut schema);

        assert_eq!(schema["type"], "object");
        assert!(schema.get("oneOf").is_none());
        assert!(schema.get("anyOf").is_none());
        assert!(schema.get("allOf").is_none());
        assert!(schema.get("enum").is_none());
        assert!(schema.get("not").is_none());
        assert!(schema["properties"].get("patch").is_some());
        assert!(schema["properties"].get("changes").is_some());
        assert_eq!(
            note.as_deref(),
            Some("Exactly one of these parameter groups must be provided: `changes` | `patch`.")
        );
    }

    #[test]
    fn responses_sanitize_merges_root_alternative_properties() {
        let mut schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                },
                {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string"}
                    },
                    "required": ["url"]
                }
            ]
        });

        let note = sanitize_for_responses(&mut schema);

        assert_eq!(schema["type"], "object");
        assert!(schema.get("anyOf").is_none());
        assert!(schema["properties"].get("path").is_some());
        assert!(schema["properties"].get("url").is_some());
        assert!(schema.get("required").is_none());
        assert_eq!(
            note.as_deref(),
            Some("At least one of these parameter groups must be provided: `path` | `url`.")
        );
    }

    #[test]
    fn responses_sanitize_preserves_nested_alternatives() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "integer"}
                    ]
                }
            }
        });

        let note = sanitize_for_responses(&mut schema);

        assert_eq!(schema["type"], "object");
        assert!(schema.get("anyOf").is_none());
        assert!(schema["properties"]["value"].get("anyOf").is_some());
        assert_eq!(note, None);
    }

    #[test]
    fn responses_sanitize_plain_object_has_no_constraint_note() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        });

        let note = sanitize_for_responses(&mut schema);

        assert_eq!(schema["type"], "object");
        assert_eq!(note, None);
    }

    #[test]
    fn responses_constraint_note_is_sorted_and_deduped() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "a": {"type": "string"},
                "b": {"type": "string"},
                "c": {"type": "string"}
            },
            "oneOf": [
                {"required": ["b", "a", "a"]},
                {"required": ["c"]},
                {"required": ["a", "b"]}
            ]
        });

        let note = sanitize_for_responses(&mut schema);

        assert_eq!(
            note.as_deref(),
            Some("Exactly one of these parameter groups must be provided: `a` + `b` | `c`.")
        );
    }
}

/// Normalize a tool's function schema for Kimi / Moonshot API compatibility.
///
/// Kimi's API enforces stricter JSON Schema validation: when a schema uses
/// `anyOf` / `oneOf`, the `type` field must be placed inside each item rather
/// than on the parent object.  This function walks the schema root and any
/// nested objects, pushing `"type": "object"` down into `anyOf` / `oneOf`
/// items when present.
///
/// Invariant: only mutates objects that carry a top-level `type` + an
/// `anyOf` or `oneOf` array — pure schemas without conditional alternatives
/// are left untouched.
pub fn sanitize_for_kimi(schema: &mut serde_json::Value) {
    if let Some(obj) = schema.as_object_mut() {
        // Recurse first so a type injected into this object's alternatives is
        // not immediately removed again by processing that freshly-mutated item.
        for (_, v) in obj.iter_mut() {
            sanitize_for_kimi(v);
        }

        // If this object has `type` + `anyOf`/`oneOf`, push `type` into
        // each item and remove it from the parent. Otherwise leave it alone.
        let should_push =
            obj.contains_key("type") && (obj.contains_key("anyOf") || obj.contains_key("oneOf"));
        if should_push && let Some(type_val) = obj.remove("type") {
            for key in ["anyOf", "oneOf"] {
                if let Some(items) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                    for item in items {
                        if let Some(item_obj) = item.as_object_mut()
                            && !item_obj.contains_key("type")
                        {
                            item_obj.insert("type".to_string(), type_val.clone());
                        }
                    }
                }
            }
        }
    } else if let Some(arr) = schema.as_array_mut() {
        for v in arr.iter_mut() {
            sanitize_for_kimi(v);
        }
    }
}

/// Normalize a complete Kimi / Moonshot `function.parameters` object.
///
/// Kimi / Moonshot requires `"type": "object"` on the parameters root
/// regardless of whether the schema uses `properties`, `$ref`, `anyOf`,
/// `allOf`, or `oneOf`.  We run `sanitize_for_kimi` first so nested
/// `anyOf` / `oneOf` handling is correct, then unconditionally ensure
/// `type: object` is present at the root (#3281).
///
/// This is root-only because recursively injecting `type: object` into
/// every empty object would corrupt JSON Schema maps such as
/// `"properties": {}`.
pub fn sanitize_for_kimi_parameters(parameters: &mut serde_json::Value) {
    if !parameters.is_object() {
        *parameters = serde_json::Value::Object(Map::new());
    }

    // Run the generic Kimi pass first so nested `anyOf` / `oneOf` receive
    // their `type` from the parent *before* we re-add it at the root.
    sanitize_for_kimi(parameters);

    // Always ensure `type: object` at the parameters root.  Kimi/Moonshot
    // rejects any parameters schema missing it (#3265, #3281).
    //
    // For bare `$ref` schemas (e.g. `{"$ref": "#/definitions/FileArgs"}`),
    // we cannot add a sibling `type` because JSON Schema forbids sibling
    // keywords alongside `$ref`.  Instead we wrap the $ref in an `allOf`
    // array and inject `type: object` at the root — a standard JSON Schema
    // pattern that preserves the $ref semantics.
    if let Some(obj) = parameters.as_object_mut()
        && !obj.contains_key("type")
    {
        if let Some(ref_val) = obj.remove("$ref") {
            let mut new_root = serde_json::Map::new();
            new_root.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
            new_root.insert(
                "allOf".to_string(),
                serde_json::Value::Array(vec![serde_json::json!({"$ref": ref_val})]),
            );
            // Preserve any other keys the original object may have had
            // (e.g. "description") in the new root.
            for (k, v) in obj.iter() {
                if k != "$ref" {
                    new_root.insert(k.clone(), v.clone());
                }
            }
            *obj = new_root;
        } else {
            obj.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
        }
    }
}

#[cfg(test)]
mod kimi_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kimi_sanitize_pushes_type_into_anyof_items() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "handle": {
                    "type": "object",
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ]
                }
            }
        });
        sanitize_for_kimi(&mut schema);
        let handle = &schema["properties"]["handle"];
        assert!(
            !handle.as_object().unwrap().contains_key("type"),
            "root type should be removed"
        );
        let any_of = handle["anyOf"].as_array().unwrap();
        assert_eq!(any_of[0]["type"], "string");
        assert_eq!(any_of[1]["type"], "null");
    }

    #[test]
    fn kimi_sanitize_injects_missing_anyof_item_types() {
        let mut schema = json!({
            "type": "object",
            "anyOf": [
                {"properties": {"path": {"type": "string"}}},
                {"required": ["url"], "properties": {"url": {"type": "string"}}}
            ]
        });

        sanitize_for_kimi(&mut schema);

        assert!(
            !schema.as_object().unwrap().contains_key("type"),
            "parent type should be removed"
        );
        let any_of = schema["anyOf"].as_array().unwrap();
        assert_eq!(any_of[0]["type"], "object");
        assert_eq!(any_of[1]["type"], "object");
    }

    #[test]
    fn kimi_sanitize_preserves_type_injected_into_nested_anyof_item() {
        let mut schema = json!({
            "type": "object",
            "anyOf": [
                {
                    "anyOf": [
                        {"properties": {"path": {"type": "string"}}}
                    ]
                }
            ]
        });

        sanitize_for_kimi(&mut schema);

        let outer_item = &schema["anyOf"][0];
        assert_eq!(outer_item["type"], "object");
        assert!(
            !schema.as_object().unwrap().contains_key("type"),
            "outer parent type should be removed"
        );
    }

    #[test]
    fn kimi_sanitize_leaves_pure_object_untouched() {
        let original = json!({
            "type": "object",
            "properties": {"x": {"type": "string"}},
            "required": ["x"]
        });
        let mut schema = original.clone();
        sanitize_for_kimi(&mut schema);
        assert_eq!(schema, original);
    }

    #[test]
    fn kimi_parameters_add_type_to_empty_root() {
        let mut schema = json!({});
        sanitize_for_kimi_parameters(&mut schema);
        assert_eq!(schema, json!({"type": "object"}));
    }

    #[test]
    fn kimi_parameters_add_type_to_properties_root_without_corrupting_properties_map() {
        let mut schema = json!({
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        });

        sanitize_for_kimi_parameters(&mut schema);

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert!(schema["properties"].get("type").is_none());
    }

    // #3281: root schemas using $ref, allOf, anyOf, oneOf must also
    // receive type: object so Kimi/Moonshot does not reject them.

    #[test]
    fn kimi_parameters_add_type_to_anyof_root() {
        let mut schema = json!({
            "anyOf": [
                {"type": "object", "properties": {"path": {"type": "string"}}},
                {"type": "null"}
            ]
        });
        sanitize_for_kimi_parameters(&mut schema);
        assert_eq!(schema["type"], "object");
        assert!(schema["anyOf"].is_array());
    }

    #[test]
    fn kimi_parameters_add_type_to_allof_root() {
        let mut schema = json!({
            "allOf": [
                {"type": "object", "properties": {"name": {"type": "string"}}}
            ]
        });
        sanitize_for_kimi_parameters(&mut schema);
        assert_eq!(schema["type"], "object");
        assert!(schema["allOf"].is_array());
    }

    #[test]
    fn kimi_parameters_add_type_to_oneof_root() {
        let mut schema = json!({
            "oneOf": [
                {"type": "object", "properties": {"id": {"type": "integer"}}},
                {"type": "object", "properties": {"name": {"type": "string"}}}
            ]
        });
        sanitize_for_kimi_parameters(&mut schema);
        assert_eq!(schema["type"], "object");
        assert!(schema["oneOf"].is_array());
    }

    #[test]
    fn kimi_parameters_wraps_ref_in_allof_with_type_object() {
        let mut schema = json!({"$ref": "#/definitions/FileArgs"});
        sanitize_for_kimi_parameters(&mut schema);
        // $ref cannot have sibling keywords per JSON Schema, so we wrap
        // it in allOf and inject type: object at the root (#3281).
        assert_eq!(schema["type"], "object");
        assert!(schema["allOf"].is_array());
        assert_eq!(schema["allOf"][0]["$ref"], "#/definitions/FileArgs");
        assert!(schema.get("$ref").is_none());
    }
}
