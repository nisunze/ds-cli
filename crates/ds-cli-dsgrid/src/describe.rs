//! `ds dsgrid describe` — the engine's own catalog of what it can do.
//!
//! `ds-grid-engine` describes itself: every journaled command, every
//! operation, every projection, each with its parameters and effect class,
//! and every named type those parameters declare, with its exact shape.
//! That catalog is the authoritative answer to "what can this engine be asked
//! to do", and it lives in the engine rather than here.
//!
//! It is also large. So the same tiering `ds` applies to its own help applies
//! to the engine's catalog: an index by default, one entry's full descriptor
//! when named. The alternative — printing the whole catalog — would be the
//! single most expensive call in the product and would undo the reason `ds`
//! exists.
//!
//! Nothing here is copied. Intentional discovery reads the captured Network
//! executable; compatible addon updates do not require rebuilding this CLI.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_catalog::{
    GRID_CATALOG_REQUEST_SCHEMA_V1, GridCatalogKind, NativeGridCatalogRequestV1,
};
use serde_json::{Value, json};

const KINDS: &[&str] = &["commands", "operations", "projections", "types"];

pub static COMMAND: Command = Command {
    id: "dsgrid.describe",
    path: &["dsgrid", "describe"],
    contract: 1,
    summary: "List the grid engine's commands, operations, projections and types.",
    purpose: "Read the captured Network engine's catalog. Omit --id for an index; name one entry for parameters, effects and bounded derived type_schemas (fields, required marks, enum tokens, units). more_type_schemas names deferred shapes. Use --kind types --id <type> for fully documented JSON Schema.",
    chapter: Chapter::GridModel,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("kind", "<kind>", "Which catalog to read.")
            .default("operations")
            .choices(KINDS),
        Arg::value("id", "<id>", "Return this one entry's full descriptor."),
    ],
    output: "Engine version and catalog index; with --id, one descriptor and its derived type_schemas. Deferred shapes appear in more_type_schemas. Types return fully documented JSON Schema.",
    examples: &[
        Example {
            command: "ds dsgrid describe --output json",
            note: "The operation index.",
            runnable: true,
        },
        Example {
            command: "ds dsgrid describe --kind commands --output json",
            note: "Journaled mutations only.",
            runnable: true,
        },
        Example {
            command: "ds dsgrid describe --kind types --id StructureDutyProfileRow --output json",
            note: "One parameter row's shape.",
            runnable: true,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["json schema", "row fields"],
    requires: Requires::Server,
    availability: available,
};

const OWN_REFUSALS: &[Refusal] = &[Refusal {
    code: "unknown_descriptor",
    when: "--id names an entry this engine does not publish",
    remedy: "run `ds dsgrid describe --kind <kind>` for the ids it does",
}];
const REFUSALS: &[Refusal] = &crate::native::extend_refusals::<
    { OWN_REFUSALS.len() + crate::native::REFUSALS.len() },
>(OWN_REFUSALS);

/// Metadata-only discovery never starts an addon process.
fn available() -> Availability {
    ds_network_client::available()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let kind = match inputs.value("kind").unwrap_or("operations") {
        "commands" => GridCatalogKind::Commands,
        "projections" => GridCatalogKind::Projections,
        "types" => GridCatalogKind::Types,
        _ => GridCatalogKind::Operations,
    };
    let captured = ds_network_client::CapturedNetworkClient::capture(1, &|| false)
        .map_err(crate::native::map_failure)?;
    let scratch = crate::native::scratch()?;
    let result = captured
        .grid_catalog(
            &NativeGridCatalogRequestV1 {
                schema: GRID_CATALOG_REQUEST_SCHEMA_V1.into(),
                kind,
                id: inputs.value("id").map(str::to_string),
            },
            scratch.path(),
            &|| false,
        )
        .map_err(crate::native::map_failure)?;
    Ok(result.result.data)
}

/// Read the already-admitted descriptor's display identifier.
fn identifier(entry: &Value) -> Option<&str> {
    entry["operation_id"]
        .as_str()
        .or_else(|| entry["command_id"].as_str())
        .or_else(|| entry["projection_id"].as_str())
        .or_else(|| entry["id"].as_str())
}

/// The string values a schema admits, when it is an enumeration: an `enum`,
/// a `const`, or a choice among those.
fn enumeration(schema: &Value) -> Option<Vec<String>> {
    if let Some(value) = schema["const"].as_str() {
        return Some(vec![value.to_string()]);
    }
    if let Some(values) = schema["enum"].as_array() {
        return values
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect();
    }
    let options = schema["oneOf"].as_array().or(schema["anyOf"].as_array())?;
    let mut values = Vec::new();
    for option in options {
        values.extend(enumeration(option)?);
    }
    Some(values)
}

/// A tagged union's tag field and the value each variant carries in it.
fn tagged_union(schema: &Value) -> Option<(String, Vec<String>)> {
    let options = schema["oneOf"].as_array().or(schema["anyOf"].as_array())?;
    let first = options.first()?["properties"].as_object()?;
    let tag = first
        .iter()
        .find(|(_, field)| field["const"].is_string())
        .map(|(name, _)| name.clone())?;
    let values = options
        .iter()
        .map(|option| {
            option["properties"][tag.as_str()]["const"]
                .as_str()
                .map(str::to_string)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((tag, values))
}

/// The short name of a schema's type: a referenced type's name, an enum's
/// values, a tagged union's tag values, an array of its item, or the JSON
/// primitive.
fn schema_type(schema: &Value) -> String {
    if let Some(reference) = schema["$ref"].as_str() {
        return reference
            .rsplit('/')
            .next()
            .unwrap_or(reference)
            .to_string();
    }
    if let Some(values) = enumeration(schema) {
        return values.join("|");
    }
    if let Some((tag, values)) = tagged_union(schema) {
        return format!("{tag}: {}", values.join("|"));
    }
    if let Some(options) = schema["anyOf"].as_array().or(schema["oneOf"].as_array()) {
        return options
            .iter()
            .map(schema_type)
            .filter(|name| name != "null")
            .collect::<Vec<_>>()
            .join("|");
    }
    match &schema["type"] {
        Value::String(kind) if kind == "array" => format!("[{}]", schema_type(&schema["items"])),
        Value::String(kind) => kind.clone(),
        Value::Array(kinds) => {
            let named: Vec<&str> = kinds
                .iter()
                .filter_map(Value::as_str)
                .filter(|kind| *kind != "null")
                .collect();
            if named == ["array"] {
                format!("[{}]", schema_type(&schema["items"]))
            } else {
                named.join("|")
            }
        }
        _ => String::new(),
    }
}

/// A type descriptor for a person: its fields, then the enums it reaches.
fn render_type(descriptor: &Value) -> String {
    let schema = &descriptor["schema"];
    let mut out = format!(
        "{}\n{}\n",
        identifier(descriptor).unwrap_or(""),
        descriptor["summary"].as_str().unwrap_or(""),
    );
    if let Some(operations) = descriptor["declared_by"].as_array() {
        let names: Vec<&str> = operations.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            out.push_str(&format!("\n  declared by  {}\n", names.join(", ")));
        }
    }
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if let Some(properties) = schema["properties"].as_object() {
        out.push_str("\nFIELDS\n");
        for (name, field) in properties {
            let mark = if required.contains(&name.as_str()) {
                "*"
            } else {
                " "
            };
            let unit = field["unit"]
                .as_str()
                .map(|unit| format!(" [{unit}]"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {mark} {name:<34} {}{unit}\n",
                schema_type(field)
            ));
        }
        out.push_str("\n  * required\n");
    } else {
        out.push_str(&format!("\n  {}\n", schema_type(schema)));
    }
    // Every enumeration and tagged union the type reaches, with the exact
    // spellings the engine reads.
    let choices: Vec<(&String, String)> = schema["$defs"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, definition)| {
            enumeration(definition)
                .map(|values| values.join("|"))
                .or_else(|| {
                    tagged_union(definition)
                        .map(|(tag, values)| format!("{tag}: {}", values.join("|")))
                })
                .map(|spelled| (name, spelled))
        })
        .collect();
    if !choices.is_empty() {
        out.push_str("\nVALUES\n");
        for (name, spelled) in choices {
            out.push_str(&format!("  {name:<36} {spelled}\n"));
        }
    }
    out.push_str("\nfull JSON Schema: --output json\n");
    out
}

pub fn render(data: &Value) -> String {
    if let Some(descriptor) = data.get("descriptor") {
        if data["kind"] == "types" {
            return render_type(descriptor);
        }
        let mut out = format!(
            "{}\n{}\n\n",
            identifier(descriptor).unwrap_or(""),
            descriptor["summary"].as_str().unwrap_or(""),
        );
        out.push_str(&format!(
            "  effect     {}\n  journaled  {}\n  result     {}\n",
            descriptor["effect_class"].as_str().unwrap_or("?"),
            descriptor["journaled"].as_bool().unwrap_or(false),
            descriptor["result_type"].as_str().unwrap_or("?"),
        ));
        if let Some(params) = descriptor["params"].as_array() {
            out.push_str("\nPARAMS\n");
            for param in params {
                let mark = if param["required"].as_bool().unwrap_or(false) {
                    "*"
                } else {
                    " "
                };
                out.push_str(&format!(
                    "  {mark} {:<26} {:<22} {}\n",
                    param["name"].as_str().unwrap_or(""),
                    param["value_type"].as_str().unwrap_or(""),
                    param["description"].as_str().unwrap_or(""),
                ));
            }
            out.push_str("\n  * required\n");
            for param in params {
                if let Some(schemas) = param["type_schemas"].as_object() {
                    for (name, schema) in schemas {
                        out.push_str(&render_type(&json!({"id":name,"schema":schema})));
                    }
                }
                for name in param["more_type_schemas"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    out.push_str(&format!(
                        "\nmore: ds dsgrid describe --kind types --id {name}\n"
                    ));
                }
            }
        }
        return out;
    }

    let mut out = format!(
        "{}  ·  {}\n\n",
        data["engine"].as_str().unwrap_or(""),
        data["kind"].as_str().unwrap_or(""),
    );
    for entry in data["entries"].as_array().into_iter().flatten() {
        if data["kind"] == "types" {
            let declared: Vec<&str> = entry["declared_by"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            out.push_str(&format!(
                "  {:<44} {}\n",
                entry["id"].as_str().unwrap_or(""),
                declared.join(", "),
            ));
            continue;
        }
        out.push_str(&format!(
            "  {:<40} {:<10} {}\n",
            entry["id"].as_str().unwrap_or(""),
            entry["effect"].as_str().unwrap_or(""),
            if entry["journaled"].as_bool().unwrap_or(false) {
                "journaled"
            } else {
                ""
            },
        ));
    }
    out.push_str("\nnext: ds dsgrid describe --kind <kind> --id <id>\n");
    out
}
