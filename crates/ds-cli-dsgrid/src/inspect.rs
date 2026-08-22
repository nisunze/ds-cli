//! `ds dsgrid inspect` — read a `.dsgrid` package's identity and inventory.
//!
//! This is the reference implementation of the CLI's bounded-output rule, and
//! it is deliberately the first command in the product.
//!
//! A `.dsgrid` package holds a complete authored grid model. Printing all of
//! it would be tens of megabytes and would tell a caller nothing it could act
//! on. So the default answer is the model's *identity* — who it is, what
//! schema it speaks, and how big it is — and everything beyond that is an
//! explicit `--include` projection the caller asks for by name.
//!
//! The cost model is visible in the answer rather than hidden. Identity,
//! tables and members are read from the package manifest without decoding a
//! single Arrow table. `library` and `extent` need the real decode, so the
//! response says `decoded: true` when one happened. A caller that only wanted
//! to know which model a file is never pays for the tables.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_model::GridModelSummary;

use crate::package::{self, DEFAULT_LIMIT, parse_limit, table_token, take};
use serde_json::{Map, Value, json};

const INCLUDE_CHOICES: &[&str] = &["tables", "members", "library", "extent"];

pub static COMMAND: Command = Command {
    id: "dsgrid.inspect",
    path: &["dsgrid", "inspect"],
    contract: 1,
    summary: "Identify a .dsgrid model and inventory what is in it.",
    purpose: "\
Reads a .dsgrid package and answers who the model is: its stable id, revision, \
coordinate system, schema version and content fingerprint. By default it does \
not decode the model's tables, so it is fast on a large package and cheap to \
call before deciding what to do next. Ask for more with --include; each \
projection is named, bounded by --limit, and reports what it withheld.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The .dsgrid package to read.").required(),
        Arg::repeated(
            "include",
            "<projection>",
            "Add a projection to the answer; repeatable.",
        )
        .choices(INCLUDE_CHOICES),
        Arg::value("limit", "<n>", "Cap each listed collection.").default(DEFAULT_LIMIT),
    ],
    output: "\
A model identity object. Each --include adds one key. `decoded` reports \
whether the model's tables had to be decoded to answer. `more` lists the \
projections not requested and any collection that was truncated.",
    examples: &[
        Example {
            command: "ds dsgrid inspect --model ./model.dsgrid",
            note: "Identity only. No tables are decoded.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid inspect --model ./model.dsgrid --include tables --output json",
            note: "Row counts per canonical table, from the manifest.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid inspect --model ./model.dsgrid --include library --limit 10",
            note: "Structure types, cables and resources. Decodes the model.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "model_not_found",
            when: "the path does not exist or is not a file",
            remedy: "check the path; --model takes a file, not a directory",
        },
        Refusal {
            code: "model_too_large",
            when: "the file is above the 512 MiB read bound",
            remedy: "confirm the file is a .dsgrid package and not a disk image",
        },
        Refusal {
            code: "not_a_dsgrid_package",
            when: "the bytes are not a readable .dsgrid container",
            remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..5000",
            remedy: "pass a limit inside the range, or omit it for the default of 50",
        },
        Refusal {
            code: "model_unreadable",
            when: "the file exists but cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "manifest_unreadable",
            when: "the package manifest does not match this build's schema",
            remedy: "rebuild the package with a matching ds-network release",
        },
        Refusal {
            code: "package_decode_failed",
            when: "--include library or extent was asked for and the tables would not decode",
            remedy: "the package is damaged or predates this schema; re-export it",
        },
    ],
    reference: Some("docs/reference/dsgrid.inspect.md"),
    availability: available,
};

/// Always available: the engine is linked into this binary, so there is
/// nothing to discover, install or reach. Resolving this must stay free — the
/// domain index calls it for every command.
fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_path = inputs.require("model")?;
    let limit = parse_limit(inputs.value("limit"))?;
    let requested: Vec<&str> = inputs
        .repeated("include")
        .iter()
        .map(String::as_str)
        .collect();

    let bytes = package::read_bytes(raw_path)?;
    // The cheap read: manifest and member inventory, no table decode.
    let manifest = package::read_manifest(raw_path, &bytes)?;

    let mut answer = Map::new();
    answer.insert("path".into(), json!(raw_path));
    answer.insert("byte_len".into(), json!(bytes.len()));
    answer.insert("format".into(), json!(ds_grid_exchange::dsgrid::FORMAT));
    answer.insert(
        "model".into(),
        json!({
            "id": manifest.model.model_id.as_str(),
            "revision": manifest.model.model_revision,
            "crs": manifest.model.coordinate_system.as_str(),
            "format_version": manifest.model.format_version,
            "schema_version": manifest.model.schema_version,
            "fingerprint": manifest.model.snapshot_fingerprint,
        }),
    );

    let populated: Vec<(String, u64)> = manifest
        .model
        .table_counts
        .iter()
        .filter(|(_, rows)| **rows > 0)
        .map(|(kind, rows)| (table_token(*kind), *rows))
        .collect();

    answer.insert(
        "inventory".into(),
        json!({
            "populated_tables": populated.len(),
            "total_rows": populated.iter().map(|(_, rows)| rows).sum::<u64>(),
            "package_members": manifest.members.len(),
        }),
    );

    let mut truncated: Vec<Value> = Vec::new();
    let mut decoded = false;

    if requested.contains(&"tables") {
        let (rows, withheld) = take(populated.clone(), limit);
        answer.insert(
            "tables".into(),
            Value::Object(
                rows.into_iter()
                    .map(|(name, count)| (name, json!(count)))
                    .collect(),
            ),
        );
        note_truncation(&mut truncated, "tables", withheld, limit);
    }

    if requested.contains(&"members") {
        let members: Vec<(String, u64)> = manifest
            .members
            .iter()
            .map(|(name, record)| (name.clone(), record.byte_len))
            .collect();
        let (rows, withheld) = take(members, limit);
        answer.insert(
            "members".into(),
            Value::Object(
                rows.into_iter()
                    .map(|(name, len)| (name, json!(len)))
                    .collect(),
            ),
        );
        note_truncation(&mut truncated, "members", withheld, limit);
    }

    if requested.contains(&"library") || requested.contains(&"extent") {
        // The expensive read. Only reached because the caller named a
        // projection that cannot be answered from the manifest.
        let package = package::decode(raw_path, &bytes)?;
        decoded = true;
        let summary = GridModelSummary::for_snapshot(&package.snapshot);

        if requested.contains(&"library") {
            let (structures, structures_withheld) = take(summary.structure_type_names, limit);
            let (cables, cables_withheld) = take(summary.cable_names, limit);
            let (resources, resources_withheld) = take(summary.resource_leaves, limit);
            answer.insert(
                "library".into(),
                json!({
                    "structure_types": structures,
                    "cables": cables,
                    "resources": resources,
                }),
            );
            note_truncation(
                &mut truncated,
                "library.structure_types",
                structures_withheld,
                limit,
            );
            note_truncation(&mut truncated, "library.cables", cables_withheld, limit);
            note_truncation(
                &mut truncated,
                "library.resources",
                resources_withheld,
                limit,
            );
        }

        if requested.contains(&"extent") {
            answer.insert(
                "extent".into(),
                json!({
                    "route_nodes": summary.route_node_extent.map(extent_json),
                    "structures": summary.structure_extent.map(extent_json),
                    "terrain": summary.terrain_extent.map(extent_json),
                }),
            );
        }
    }

    answer.insert("decoded".into(), json!(decoded));

    // The continuation mechanism: say what else exists rather than making the
    // caller re-read help to find out.
    let unrequested: Vec<&str> = INCLUDE_CHOICES
        .iter()
        .copied()
        .filter(|choice| !requested.contains(choice))
        .collect();
    if !unrequested.is_empty() || !truncated.is_empty() {
        let mut more = Map::new();
        if !unrequested.is_empty() {
            more.insert("available_projections".into(), json!(unrequested));
        }
        if !truncated.is_empty() {
            more.insert("truncated".into(), Value::Array(truncated));
        }
        answer.insert("more".into(), Value::Object(more));
    }

    Ok(Value::Object(answer))
}

/// Human presentation. A projection of the same value the JSON envelope
/// carries — never a second computation, so the two cannot disagree.
pub fn render(data: &Value) -> String {
    let mut out = String::new();
    let model = &data["model"];
    out.push_str(&format!(
        "{}  rev {}  {}\n",
        model["id"].as_str().unwrap_or("?"),
        model["revision"],
        model["crs"].as_str().unwrap_or("?"),
    ));
    out.push_str(&format!(
        "  schema v{}  format v{}  {} bytes\n",
        model["schema_version"], model["format_version"], data["byte_len"],
    ));
    out.push_str(&format!(
        "  {} populated tables · {} rows · {} members\n",
        data["inventory"]["populated_tables"],
        data["inventory"]["total_rows"],
        data["inventory"]["package_members"],
    ));
    out.push_str(&format!(
        "  {}\n",
        model["fingerprint"].as_str().unwrap_or("")
    ));

    for key in ["tables", "members"] {
        if let Some(Value::Object(entries)) = data.get(key) {
            out.push_str(&format!("\n{}:\n", key.to_uppercase()));
            for (name, value) in entries {
                out.push_str(&format!("  {name:<34}  {value}\n"));
            }
        }
    }
    if let Some(library) = data.get("library") {
        out.push_str("\nLIBRARY:\n");
        for key in ["structure_types", "cables", "resources"] {
            let count = library[key].as_array().map_or(0, Vec::len);
            out.push_str(&format!("  {key:<18}  {count}\n"));
            for entry in library[key].as_array().into_iter().flatten() {
                out.push_str(&format!("    {}\n", entry.as_str().unwrap_or("")));
            }
        }
    }
    if let Some(extent) = data.get("extent") {
        out.push_str("\nEXTENT:\n");
        for key in ["route_nodes", "structures", "terrain"] {
            match &extent[key] {
                Value::Null => out.push_str(&format!("  {key:<12}  —\n")),
                value => out.push_str(&format!(
                    "  {key:<12}  x {} … {}   y {} … {}\n",
                    value["min_x_m"], value["max_x_m"], value["min_y_m"], value["max_y_m"],
                )),
            }
        }
    }
    if let Some(more) = data
        .get("more")
        .and_then(|more| more.get("available_projections"))
    {
        let names: Vec<&str> = more
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push_str(&format!(
            "\nmore: --include {}\n",
            names.join(" --include ")
        ));
    }
    out
}

fn note_truncation(into: &mut Vec<Value>, field: &str, withheld: usize, limit: usize) {
    if withheld > 0 {
        into.push(json!({ "field": field, "withheld": withheld, "limit": limit }));
    }
}

fn extent_json(extent: ds_grid_model::Extent3) -> Value {
    json!({
        "min_x_m": extent.min_x_m,
        "min_y_m": extent.min_y_m,
        "min_z_m": extent.min_z_m,
        "max_x_m": extent.max_x_m,
        "max_y_m": extent.max_y_m,
        "max_z_m": extent.max_z_m,
    })
}
