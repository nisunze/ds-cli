//! `ds network describe` — the engine's own catalog of what it can do.
//!
//! `ds-grid-engine` describes itself: every journaled command, every
//! operation, every projection, each with its parameters and effect class.
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
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{
    ENGINE_VERSION, describe_commands, describe_operations, describe_projections,
};
use serde_json::{Value, json};

const KINDS: &[&str] = &["commands", "operations", "projections"];

pub static COMMAND: Command = Command {
    id: "network.describe",
    path: &["network", "describe"],
    contract: 1,
    summary: "List the grid engine's commands, operations and projections.",
    purpose: "\
Reads the descriptor catalog published by the engine compiled into this binary \
— every journaled command, every read operation, every projection, with its \
parameters and effect class. By default it lists identifiers and effects only; \
name one with --id for its complete descriptor. The full catalog is large, \
which is why it is never printed whole.",
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
parameter list.",
    examples: &[
        Example {
            command: "ds network describe --output json",
            note: "The operation index.",
            runnable: true,
        },
        Example {
            command: "ds network describe --kind commands --output json",
            note: "Journaled mutations only.",
            runnable: true,
        },
    ],
    refusals: &[Refusal {
        code: "unknown_descriptor",
        when: "--id names an entry this engine does not publish",
        remedy: "run `ds network describe --kind <kind>` for the ids it does",
    }],
    reference: Some("docs/reference/network.md"),
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
            "more": { "next": "ds network describe --kind <kind> --id <id>" },
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
                    "run `ds network describe --kind {kind}` for the ids it publishes"
                ))
            }
        }
        // The id list can be long; bound it rather than making a refusal the
        // largest response in the domain.
        let (shown, withheld) = crate::package::take(known, 40);
        return Err(failure
            .next(format!("ds network describe --kind {kind}"))
            .detail(json!({ "ids": shown, "withheld": withheld })));
    };

    Ok(json!({
        "engine": ENGINE_VERSION,
        "kind": kind,
        "descriptor": entry,
    }))
}

pub fn render(data: &Value) -> String {
    if let Some(descriptor) = data.get("descriptor") {
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
    out.push_str("\nnext: ds network describe --kind <kind> --id <id>\n");
    out
}
