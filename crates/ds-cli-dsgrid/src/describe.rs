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
//! Nothing here is copied. The descriptors are read from the engine compiled
//! into this binary, so they cannot be stale relative to what it will do.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{
    ENGINE_VERSION, describe_commands, describe_operations, describe_projections, describe_types,
};
use serde_json::{Value, json};

const KINDS: &[&str] = &["commands", "operations", "projections", "types"];

pub static COMMAND: Command = Command {
    id: "dsgrid.describe",
    path: &["dsgrid", "describe"],
    contract: 1,
    summary: "List the grid engine's commands, operations, projections and types.",
    purpose: "\
Reads the descriptor catalog published by the engine compiled into this binary \
— every journaled command, every read operation, every projection, with its \
parameters and effect class. By default it lists identifiers and effects only; \
name one with --id for its complete descriptor. The full catalog is large, \
which is why it is never printed whole. --kind types --id <type> gives a \
parameter row's exact shape as JSON Schema: fields, required, enum values.",
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
    output: "\
The engine version, the catalog kind, and one line per entry: its id and \
effect class. With --id, that entry's complete descriptor including its \
parameter list; for a type, its JSON Schema.",
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
    refusals: &[Refusal {
        code: "unknown_descriptor",
        when: "--id names an entry this engine does not publish",
        remedy: "run `ds dsgrid describe --kind <kind>` for the ids it does",
    }],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["json schema", "row fields"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

/// The engine's catalog for one kind. Named here so the mapping from a `ds`
/// word to an engine function is in one place.
fn catalog(kind: &str) -> Value {
    match kind {
        "commands" => describe_commands(),
        "projections" => describe_projections(),
        "types" => describe_types(),
        _ => describe_operations(),
    }
}

/// A descriptor's identifier, whichever of the engine's id fields it carries.
/// The three catalogs do not spell it the same way, and a caller should not
/// have to know that.
fn identifier(entry: &Value) -> Option<&str> {
    entry["operation_id"]
        .as_str()
        .or_else(|| entry["command_id"].as_str())
        .or_else(|| entry["projection_id"].as_str())
        .or_else(|| entry["id"].as_str())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let kind = inputs.value("kind").unwrap_or("operations");
    let catalog = catalog(kind);
    let entries = catalog.as_array().cloned().unwrap_or_default();

    let Some(wanted) = inputs.value("id") else {
        if kind == "types" {
            // A type has no effect and is never journaled; what locates it is
            // the operations whose parameters declare it. The index lists the
            // declared types only: every type they reach is inside their
            // schema's `$defs` and answers --id too, and listing all of them
            // would double the index with names nobody starts from.
            let declared: Vec<&Value> = entries
                .iter()
                .filter(|entry| {
                    entry["declared_by"]
                        .as_array()
                        .is_some_and(|operations| !operations.is_empty())
                })
                .collect();
            return Ok(json!({
                "engine": ENGINE_VERSION,
                "kind": kind,
                "entries": declared.iter().map(|entry| json!({
                    "id": identifier(entry),
                    "declared_by": entry["declared_by"],
                    "summary": entry["summary"],
                })).collect::<Vec<_>>(),
                "more": {
                    "next": "ds dsgrid describe --kind types --id <type>",
                    "nested_types": entries.len() - declared.len(),
                },
            }));
        }
        return Ok(json!({
            "engine": ENGINE_VERSION,
            "kind": kind,
            "entries": entries.iter().map(|entry| json!({
                "id": identifier(entry),
                // The engine spells it `effect_class`; `ds` spells effects
                // `effect` everywhere else, and a caller reading a `ds`
                // response should not have to learn a second word for the
                // same idea at one command.
                "effect": entry["effect_class"],
                "journaled": entry["journaled"],
                "summary": entry["summary"],
            })).collect::<Vec<_>>(),
            "more": { "next": "ds dsgrid describe --kind <kind> --id <id>" },
        }));
    };

    let found = entries
        .iter()
        .find(|entry| identifier(entry) == Some(wanted));
    let Some(entry) = found else {
        let known: Vec<&str> = entries.iter().filter_map(identifier).collect();
        let mut failure = Failure::invalid(
            "unknown_descriptor",
            format!("this engine publishes no `{kind}` entry named `{wanted}`"),
        );
        match ds_cli_contract::args::nearest(wanted, known.iter().copied()) {
            Some(suggestion) => failure = failure.remedy(format!("did you mean `{suggestion}`?")),
            None => {
                failure = failure.remedy(format!(
                    "run `ds dsgrid describe --kind {kind}` for the ids it publishes"
                ))
            }
        }
        // The id list can be long; bound it rather than making a refusal the
        // largest response in the domain.
        let (shown, withheld) = crate::package::take(known, 40);
        return Err(failure
            .next(format!("ds dsgrid describe --kind {kind}"))
            .detail(json!({ "ids": shown, "withheld": withheld })));
    };

    Ok(json!({
        "engine": ENGINE_VERSION,
        "kind": kind,
        "descriptor": entry,
    }))
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
            out.push_str(&format!("  {mark} {:<34} {}\n", name, schema_type(field)));
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
