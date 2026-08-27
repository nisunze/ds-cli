//! `ds dsgrid project` — run one engine projection against a model.
//!
//! `ds dsgrid describe --kind projections` has always listed what the engine
//! can project; nothing could invoke one. That gap mattered most for the
//! projection that carries engineering evidence: `project_profile` solves
//! every span of one alignment — catenary geometry, the governing criterion
//! rule, required clearance, the calculated minimum and where it occurs — and
//! it was reachable only from inside the desktop application. A workflow that
//! needs a clearance answer from a `.dsgrid` had no command to call, so it
//! either did without or grew a private helper. Both are worse than a
//! command.
//!
//! This module owns nothing but the boundary. The catalog is the engine's,
//! the parameters are the ones its descriptor declares, and every value in
//! the result was computed by `ds-grid-engine`. What is added here is the
//! bounding: a projection over a real network is far larger than a terminal
//! or an agent's context, so stdout carries a bounded view and `--out` writes
//! the complete document to a file.
//!
//! The descriptor is the contract, in both directions. A parameter the
//! descriptor does not declare is refused rather than ignored, and a required
//! one that is absent is named. So a caller who read `describe` has read
//! everything this command accepts, and a projection whose parameters change
//! upstream changes here without an edit.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{
    ENGINE_VERSION, GridSession, ProfileAtlasOptions, ProfileAtlasVerticalScale, ProjectionRow,
    describe_projections,
};
use ds_grid_model::{AlignmentId, StructureTypeId, TableKind, TensionSectionId};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use crate::output::{sha256, validate_output_path, write_new};
use crate::package;

/// Above this, a non-row result is withheld from stdout and only written to
/// `--out`. A whole-model atlas scene is megabytes; printing one would cost
/// every caller more context than the answer is worth, and the bound has to
/// be on the serialized bytes because no row count describes a scene.
const MAX_INLINE_RESULT_BYTES: usize = 256 * 1024;

pub static COMMAND: Command = Command {
    id: "dsgrid.project",
    path: &["dsgrid", "project"],
    contract: 1,
    summary: "Run one engine projection over a .dsgrid and bound its result.",
    purpose: "\
Opens one verified .dsgrid package, pins the authored revision, and runs one \
projection published by the compiled ds-grid engine — the plan and profile \
views, a canonical table, the profile atlas, the structure and model \
libraries, the criteria workbench, or the sag-criterion options for named \
sections. Parameters are validated against that projection's own descriptor: \
an undeclared one refuses, a required one that is absent is named. The \
profile projection is where span-by-span sag, governing condition, required \
clearance and calculated minimum come from. Nothing is computed here and \
nothing is written to the source package.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The .dsgrid package to project.").required(),
        Arg::value("id", "<projection-id>", "Which projection to run.").required(),
        Arg::repeated(
            "param",
            "<name=value>",
            "One projection parameter, as its descriptor declares it. Repeatable.",
        ),
        Arg::value("limit", "<n>", "How many rows to return on stdout.")
            .default(package::DEFAULT_LIMIT),
        Arg::value(
            "out",
            "<path>",
            "Write the complete, unbounded projection document here.",
        ),
    ],
    output: "\
The source package identity and authored revision, the projection id and the \
parameters as resolved, then the result: rows bounded by --limit with the \
withheld count, or the projected document when it fits the inline bound. With \
--out, the complete document is written there and reported with its byte \
length and SHA-256.",
    examples: &[
        Example {
            command: "ds dsgrid describe --kind projections --output json",
            note: "The projections this engine publishes, before choosing one.",
            runnable: true,
        },
        Example {
            command: "ds dsgrid project --model ./model.dsgrid --id project_profile --param alignment_id=<id> --out ./profile.json --output json",
            note: "Span sag and clearance evidence for one alignment.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid project --model ./model.dsgrid --id project_table --param table_kind=structures --limit 10 --output json",
            note: "One canonical table, bounded.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "model_not_found",
            when: "the model path does not exist or is not a file",
            remedy: "check the path; --model takes one .dsgrid file",
        },
        Refusal {
            code: "model_too_large",
            when: "the model is above the 512 MiB read bound",
            remedy: "confirm the file is a .dsgrid package and not a disk image",
        },
        Refusal {
            code: "model_unreadable",
            when: "the model exists but cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "not_a_dsgrid_package",
            when: "the model bytes are not a readable .dsgrid container",
            remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
        },
        Refusal {
            code: "package_decode_failed",
            when: "the manifest or canonical tables do not verify",
            remedy: "run `ds dsgrid validate --model <path>` and repair the package",
        },
        Refusal {
            code: "output_exists",
            when: "--out names a path that already exists",
            remedy: "choose a new output path; this domain never overwrites",
        },
        Refusal {
            code: "output_parent_missing",
            when: "the parent directory of --out does not exist",
            remedy: "create the intended directory, then retry",
        },
        Refusal {
            code: "output_unwritable",
            when: "the output file cannot be created or fully written",
            remedy: "check the parent path and permissions; a partial file is removed",
        },
        Refusal {
            code: "unknown_projection",
            when: "--id names a projection this engine does not publish",
            remedy: "run `ds dsgrid describe --kind projections` for the ids it does",
        },
        Refusal {
            code: "unsupported_projection",
            when: "the engine publishes the projection but this surface has no call for it",
            remedy: "report the gap with `ds feedback report`; do not approximate it elsewhere",
        },
        Refusal {
            code: "invalid_param",
            when: "a --param is not written as name=value",
            remedy: "pass `--param name=value`, once per parameter",
        },
        Refusal {
            code: "repeated_param",
            when: "the same parameter name is given more than once",
            remedy: "pass each parameter once; a list value is comma-separated",
        },
        Refusal {
            code: "unknown_param",
            when: "a --param name is not declared by the projection's descriptor",
            remedy: "read `ds dsgrid describe --kind projections --id <id>` for its parameters",
        },
        Refusal {
            code: "missing_param",
            when: "a parameter the descriptor marks required was not given",
            remedy: "pass it with --param; the descriptor names its value type",
        },
        Refusal {
            code: "invalid_param_value",
            when: "a parameter value does not parse as the type its descriptor declares",
            remedy: "read the declared value type; ids come from a projection of this same revision",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in range",
            remedy: "pass 1..5000, or use --out for the complete document",
        },
        Refusal {
            code: "projection_unavailable",
            when: "the engine cannot project this model with these parameters",
            remedy: "read the engine detail; it names the entity or state that is missing",
        },
        Refusal {
            code: "result_unserializable",
            when: "the engine result cannot be encoded as JSON",
            remedy: "report this engine failure with the model digest and projection id",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

/// What a projection returned: a row list this command bounds by count, or
/// one document it bounds by serialized size. The distinction is the engine's
/// — `result_type` in the descriptor — not a choice made here.
enum Projected {
    Rows(Vec<ProjectionRow>),
    Document(Value),
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let projection_id = inputs.require("id")?;
    let limit = package::parse_limit(inputs.value("limit"))?;
    let out_path = inputs.value("out");

    // Descriptor first, then parameters, then the expensive read. A caller
    // who misspelled a projection or a parameter should learn that in a
    // millisecond, not after a 512 MiB package is decoded.
    let descriptor = descriptor_for(projection_id)?;
    let params = parse_params(inputs.repeated("param"))?;
    check_params(projection_id, &descriptor, &params)?;
    if let Some(path) = out_path {
        validate_output_path(path)?;
    }

    let model_bytes = package::read_bytes(model_path)?;
    let package = package::decode(model_path, &model_bytes)?;
    let model_id = package.manifest.model.model_id.clone();
    let package_revision = package.manifest.model.model_revision;
    let session = GridSession::open(package.snapshot);
    let authored_revision = session.current_revision().revision_id.clone();

    let projected = project(&session, projection_id, &params)?;

    let resolved: Map<String, Value> = params
        .iter()
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect();

    // The identity every form of this answer carries: which model, which
    // authored revision, which projection, and with what parameters. Built
    // once so the bounded stdout view and the complete `--out` document
    // cannot describe themselves differently.
    let mut identity = Map::new();
    identity.insert("engine".into(), json!(ENGINE_VERSION));
    identity.insert(
        "source".into(),
        json!({
            "path": model_path,
            "model_id": model_id.as_str(),
            "package_revision": package_revision,
            "authored_revision": authored_revision.as_str(),
        }),
    );
    identity.insert("projection".into(), json!(projection_id));
    identity.insert("params".into(), Value::Object(resolved));
    identity.insert(
        "result_type".into(),
        json!(descriptor["result_type"].as_str().unwrap_or("")),
    );

    // The complete result is what `--out` receives and what the inline bound
    // is measured against, so it is built once either way.
    let complete = match &projected {
        Projected::Rows(rows) => serialize(rows)?,
        Projected::Document(value) => value.clone(),
    };

    let mut bounded = identity.clone();
    match &projected {
        Projected::Rows(rows) => {
            let all = complete.as_array().cloned().unwrap_or_default();
            let (shown, withheld) = package::take(all, limit);
            bounded.insert("row_count".into(), json!(rows.len()));
            bounded.insert("rows".into(), Value::Array(shown));
            bounded.insert(
                "more".into(),
                json!({ "limit": limit, "withheld": withheld }),
            );
        }
        Projected::Document(_) => {
            let byte_len =
                serde_json::to_vec(&complete).map_or(usize::MAX, |encoded| encoded.len());
            if byte_len <= MAX_INLINE_RESULT_BYTES {
                bounded.insert("result".into(), complete.clone());
                bounded.insert("more".into(), json!({ "result_byte_len": byte_len }));
            } else {
                bounded.insert(
                    "more".into(),
                    json!({
                        "result_byte_len": byte_len,
                        "result_withheld": true,
                        "max_inline_byte_len": MAX_INLINE_RESULT_BYTES,
                        "next": "pass --out <path> for the complete document",
                    }),
                );
            }
        }
    }

    if let Some(path) = out_path {
        let mut document = identity;
        match &projected {
            Projected::Rows(rows) => {
                document.insert("row_count".into(), json!(rows.len()));
                document.insert("rows".into(), complete);
            }
            Projected::Document(_) => {
                document.insert("result".into(), complete);
            }
        }
        let bytes = serde_json::to_vec_pretty(&Value::Object(document)).map_err(|error| {
            Failure::failed(
                "result_unserializable",
                "the projection document could not be encoded",
            )
            .remedy("report this engine failure with the model digest and projection id")
            .detail(json!({ "detail": error.to_string() }))
        })?;
        write_new(path, &bytes)?;
        bounded.insert(
            "artifact".into(),
            json!({
                "path": path,
                "byte_len": bytes.len(),
                "sha256": sha256(&bytes),
            }),
        );
    }

    Ok(Value::Object(bounded))
}

/// One projection's descriptor, straight from the engine's catalog.
fn descriptor_for(projection_id: &str) -> Result<Value, Failure> {
    let catalog = describe_projections();
    let entries = catalog.as_array().cloned().unwrap_or_default();
    let found = entries
        .iter()
        .find(|entry| entry["operation_id"].as_str() == Some(projection_id));
    if let Some(entry) = found {
        return Ok(entry.clone());
    }

    let known: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry["operation_id"].as_str())
        .collect();
    let mut failure = Failure::invalid(
        "unknown_projection",
        format!("this engine publishes no projection named `{projection_id}`"),
    );
    match ds_cli_contract::args::nearest(projection_id, known.iter().copied()) {
        Some(suggestion) => failure = failure.remedy(format!("did you mean `{suggestion}`?")),
        None => failure = failure.remedy("run `ds dsgrid describe --kind projections` for the ids"),
    }
    let (shown, withheld) = package::take(known, 40);
    Err(failure
        .next("ds dsgrid describe --kind projections")
        .detail(json!({ "ids": shown, "withheld": withheld })))
}

/// `name=value` pairs, in declaration-independent order.
fn parse_params(raw: &[String]) -> Result<BTreeMap<String, String>, Failure> {
    let mut params = BTreeMap::new();
    for entry in raw {
        let Some((name, value)) = entry.split_once('=') else {
            return Err(Failure::invalid(
                "invalid_param",
                format!("`{entry}` is not written as name=value"),
            )
            .remedy("pass `--param name=value`, once per parameter"));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(
                Failure::invalid("invalid_param", format!("`{entry}` names no parameter"))
                    .remedy("pass `--param name=value`, once per parameter"),
            );
        }
        if params.insert(name.to_string(), value.to_string()).is_some() {
            return Err(Failure::invalid(
                "repeated_param",
                format!("`{name}` was given more than once"),
            )
            .remedy("pass each parameter once; a list value is comma-separated"));
        }
    }
    Ok(params)
}

/// Every given parameter must be declared, and every required declared one
/// must be given. Both directions matter: an ignored parameter is a silently
/// different answer.
fn check_params(
    projection_id: &str,
    descriptor: &Value,
    params: &BTreeMap<String, String>,
) -> Result<(), Failure> {
    let declared = descriptor["params"].as_array().cloned().unwrap_or_default();
    let names: Vec<&str> = declared
        .iter()
        .filter_map(|param| param["name"].as_str())
        .collect();

    for given in params.keys() {
        if !names.contains(&given.as_str()) {
            let mut failure = Failure::invalid(
                "unknown_param",
                format!("`{projection_id}` declares no parameter `{given}`"),
            );
            match ds_cli_contract::args::nearest(given, names.iter().copied()) {
                Some(suggestion) => {
                    failure = failure.remedy(format!("did you mean `{suggestion}`?"))
                }
                None => {
                    failure = failure.remedy(format!(
                        "run `ds dsgrid describe --kind projections --id {projection_id}`"
                    ))
                }
            }
            return Err(failure
                .next(format!(
                    "ds dsgrid describe --kind projections --id {projection_id}"
                ))
                .detail(json!({ "declared": names })));
        }
    }

    for param in &declared {
        let Some(name) = param["name"].as_str() else {
            continue;
        };
        if param["required"].as_bool().unwrap_or(false) && !params.contains_key(name) {
            return Err(Failure::invalid(
                "missing_param",
                format!("`{projection_id}` requires the parameter `{name}`"),
            )
            .remedy(format!(
                "pass `--param {name}=<{}>`",
                param["value_type"].as_str().unwrap_or("value")
            ))
            .next(format!(
                "ds dsgrid describe --kind projections --id {projection_id}"
            )));
        }
    }
    Ok(())
}

/// Call the engine. One arm per published projection: the mapping from a
/// descriptor id to a session method is the whole of this module's knowledge,
/// and writing it out is what keeps the compiler checking that each one is
/// called with the types it actually takes.
fn project(
    session: &GridSession,
    projection_id: &str,
    params: &BTreeMap<String, String>,
) -> Result<Projected, Failure> {
    match projection_id {
        "project_plan" => Ok(Projected::Rows(session.plan_projection())),
        "project_profile" => {
            let alignment = alignment_id(params, "alignment_id")?;
            session
                .profile_projection(&alignment)
                .map(Projected::Rows)
                .map_err(|error| unavailable(projection_id, &error.to_string()))
        }
        "project_table" => {
            let kind = table_kind(params, "table_kind")?;
            Ok(Projected::Rows(session.table_projection(kind)))
        }
        "project_profile_atlas" => {
            let options = atlas_options(params)?;
            session
                .profile_atlas_scene(options)
                .map_err(|error| unavailable(projection_id, &error.to_string()))
                .and_then(|scene| serialize(&scene).map(Projected::Document))
        }
        "project_structure_library" => {
            let id = structure_type_id(params, "structure_type_id")?;
            session
                .structure_library_scene(&id)
                .map_err(|error| unavailable(projection_id, &error.to_string()))
                .and_then(|scene| serialize(&scene).map(Projected::Document))
        }
        "project_model_library" => {
            serialize(&session.model_library_projection()).map(Projected::Document)
        }
        "project_criteria_workbench" => {
            serialize(&session.criteria_workbench_projection()).map(Projected::Document)
        }
        "project_profile_sag_criterion_options" => {
            let sections = tension_section_ids(params, "section_ids")?;
            session
                .profile_sag_criterion_options(&sections)
                .map_err(|error| unavailable(projection_id, &error.to_string()))
                .and_then(|options| serialize(&options).map(Projected::Document))
        }
        other => Err(Failure::invalid(
            "unsupported_projection",
            format!("`{other}` is published by this engine but has no call on this surface"),
        )
        .remedy("report the gap with `ds feedback report`; do not approximate it elsewhere")),
    }
}

fn unavailable(projection_id: &str, detail: &str) -> Failure {
    Failure::failed(
        "projection_unavailable",
        format!("the engine could not project `{projection_id}` for this model"),
    )
    .remedy("read the engine detail; it names the entity or state that is missing")
    .detail(json!({ "engine": detail }))
}

fn serialize<T: serde::Serialize>(value: &T) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| {
        Failure::failed(
            "result_unserializable",
            "the engine result could not be encoded as JSON",
        )
        .remedy("report this engine failure with the model digest and projection id")
        .detail(json!({ "detail": error.to_string() }))
    })
}

fn required<'a>(params: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    // `check_params` already proved every required parameter is present, so
    // an absent one here would be a defect in that check rather than a
    // caller's mistake. An empty string reaches the type parser below and
    // refuses with the value type the caller needs to see.
    params.get(name).map_or("", String::as_str)
}

fn bad_value(name: &str, value: &str, value_type: &str, detail: &str) -> Failure {
    Failure::invalid(
        "invalid_param_value",
        format!("`{name}={value}` is not a {value_type}"),
    )
    .remedy("read the declared value type; ids come from a projection of this same revision")
    .detail(json!({ "param": name, "value_type": value_type, "detail": detail }))
}

fn alignment_id(params: &BTreeMap<String, String>, name: &str) -> Result<AlignmentId, Failure> {
    let raw = required(params, name);
    AlignmentId::new(raw).map_err(|error| bad_value(name, raw, "AlignmentId", &error.to_string()))
}

fn structure_type_id(
    params: &BTreeMap<String, String>,
    name: &str,
) -> Result<StructureTypeId, Failure> {
    let raw = required(params, name);
    StructureTypeId::new(raw)
        .map_err(|error| bad_value(name, raw, "StructureTypeId", &error.to_string()))
}

fn tension_section_ids(
    params: &BTreeMap<String, String>,
    name: &str,
) -> Result<Vec<TensionSectionId>, Failure> {
    let raw = required(params, name);
    let mut ids = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        ids.push(
            TensionSectionId::new(part).map_err(|error| {
                bad_value(name, raw, "Vec<TensionSectionId>", &error.to_string())
            })?,
        );
    }
    if ids.is_empty() {
        return Err(bad_value(
            name,
            raw,
            "Vec<TensionSectionId>",
            "no section id in a comma-separated list",
        ));
    }
    Ok(ids)
}

fn table_kind(params: &BTreeMap<String, String>, name: &str) -> Result<TableKind, Failure> {
    let raw = required(params, name);
    // The accepted tokens are the model crate's own serde spelling, so a
    // table renamed upstream is renamed here without an edit — and the
    // refusal lists them rather than making the caller guess.
    serde_json::from_value(Value::String(raw.to_string())).map_err(|_| {
        let tokens: Vec<String> = TableKind::ALL
            .iter()
            .map(|kind| package::table_token(*kind))
            .collect();
        bad_value(name, raw, "TableKind", "not a canonical table token")
            .detail(json!({ "param": name, "value_type": "TableKind", "tokens": tokens }))
    })
}

fn atlas_options(params: &BTreeMap<String, String>) -> Result<ProfileAtlasOptions, Failure> {
    let mut options = ProfileAtlasOptions::default();
    if let Some(raw) = params.get("vertical_scale") {
        options.vertical_scale =
            serde_json::from_value::<ProfileAtlasVerticalScale>(Value::String(raw.to_string()))
                .map_err(|_| {
                    bad_value(
                        "vertical_scale",
                        raw,
                        "ProfileAtlasVerticalScale",
                        "expected `1:5` or `1:10`",
                    )
                })?;
    }
    if let Some(raw) = params.get("vertical_exaggeration") {
        options.vertical_exaggeration = Some(finite(raw, "vertical_exaggeration", "f64_ratio")?);
    }
    if let Some(raw) = params.get("terrain_corridor_half_width_m") {
        options.terrain_corridor_half_width_m =
            Some(finite(raw, "terrain_corridor_half_width_m", "f64_metre")?);
    }
    Ok(options)
}

fn finite(raw: &str, name: &str, value_type: &str) -> Result<f64, Failure> {
    let parsed: f64 = raw
        .parse()
        .map_err(|_| bad_value(name, raw, value_type, "not a decimal number"))?;
    if !parsed.is_finite() {
        return Err(bad_value(name, raw, value_type, "not a finite number"));
    }
    Ok(parsed)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}\nmodel     {}\nrevision  {}\nresult    {}\n",
        data["projection"].as_str().unwrap_or("?"),
        data["source"]["model_id"].as_str().unwrap_or("?"),
        data["source"]["authored_revision"].as_str().unwrap_or("?"),
        data["result_type"].as_str().unwrap_or("?"),
    );

    if let Some(rows) = data["rows"].as_array() {
        out.push_str(&format!(
            "rows      {} of {}\n",
            rows.len(),
            data["row_count"],
        ));
        for row in rows.iter().take(10) {
            out.push_str(&format!(
                "  {:<28} {}\n",
                row["entity_id"].as_str().unwrap_or(""),
                row["projection_kind"].as_str().unwrap_or(""),
            ));
        }
        let withheld = data["more"]["withheld"].as_u64().unwrap_or(0);
        if withheld > 0 {
            out.push_str(&format!(
                "  … {withheld} withheld; raise --limit or use --out\n"
            ));
        }
    } else if data["more"]["result_withheld"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "document  {} bytes; over the inline bound\n  → pass --out <path> for the complete document\n",
            data["more"]["result_byte_len"],
        ));
    } else if data.get("result").is_some() {
        out.push_str(&format!(
            "document  {} bytes\n",
            data["more"]["result_byte_len"],
        ));
    }

    if let Some(artifact) = data.get("artifact") {
        out.push_str(&format!(
            "written   {}\nsha256    {}\n",
            artifact["path"].as_str().unwrap_or("?"),
            artifact["sha256"].as_str().unwrap_or("?"),
        ));
    }
    out
}
