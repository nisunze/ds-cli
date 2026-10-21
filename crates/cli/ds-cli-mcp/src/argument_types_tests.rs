//! An MCP argument must be the type its tool declares: the input schema says
//! `string`, so a number or boolean is refused rather than stringified into
//! some other request.

use serde_json::{Value, json};

use crate::tools::{argv_for_call, tool_from_descriptor};

fn inspect_like() -> Value {
    json!({
        "id": "dsgrid.inspect",
        "chapter": "design",
        "path": ["dsgrid", "inspect"],
        "contract": 1,
        "summary": "Identify a model.",
        "purpose": "Reads a package.",
        "output": "Identity.",
        "effect": "read_only",
        "authority": "none",
        "confirmation_required": false,
        "inputs": [
            { "name": "model", "kind": "value", "value": "<path>", "required": true, "summary": "The package." },
            { "name": "include", "kind": "repeated", "value": "<projection>", "required": false, "summary": "Projection." },
            { "name": "limit", "kind": "value", "value": "<n>", "required": false, "summary": "Cap." },
            { "name": "verbose", "kind": "switch", "required": false, "summary": "More." }
        ],
        "refusals": []
    })
}

#[test]
fn a_non_string_value_is_a_wrong_type_not_a_stringified_request() {
    let tool = tool_from_descriptor(&inspect_like()).expect("tool");
    for (arguments, key) in [
        (json!({ "model": 123 }), "model"),
        (json!({ "model": true }), "model"),
        (json!({ "model": ["a.dsgrid"] }), "model"),
        (json!({ "model": "a.dsgrid", "limit": 10 }), "limit"),
        (json!({ "model": "a.dsgrid", "include": [1] }), "include"),
    ] {
        let message = argv_for_call(&tool, &arguments).expect_err(&arguments.to_string());
        assert!(message.contains(&format!("`{key}` ")), "{message}");
        assert!(message.contains("string"), "{message}");
    }
}

#[test]
fn declared_types_still_pass() {
    let tool = tool_from_descriptor(&inspect_like()).expect("tool");
    let argv = argv_for_call(
        &tool,
        &json!({ "model": "a.dsgrid", "include": ["tables", "members"], "limit": "10", "verbose": true }),
    )
    .expect("argv");
    assert!(argv.contains(&"--model=a.dsgrid".to_owned()), "{argv:?}");
    assert!(argv.contains(&"--include=tables".to_owned()), "{argv:?}");
    assert!(argv.contains(&"--limit=10".to_owned()), "{argv:?}");
    assert!(argv.contains(&"--verbose".to_owned()), "{argv:?}");
    // A single string for a repeated input remains one item.
    argv_for_call(&tool, &json!({ "model": "a.dsgrid", "include": "tables" })).expect("argv");
}
