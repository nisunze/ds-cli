//! `ds dsgrid run` — execute one native non-mutating grid operation.
//!
//! The operation catalogue and all engineering behavior remain owned by
//! `ds-grid-engine`. This module is only the bounded file/JSON transport that
//! makes the already-compiled read, solve, and propose surface reachable to a
//! headless caller. Journaled mutations, imports, and exports are rejected;
//! `dsgrid apply` remains the sole file-revision path.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::TaggedAlignmentLengthsRequest;
use ds_grid_engine::descriptor::operation_descriptors;
use ds_grid_engine::{
    EffectClass, GridSession, NetworkCalculationRequest, OperationDescriptor, ProfileAtlasOptions,
    ResultStore, SectionDemandsRequest, SpottingPlanRequest, StructureAnalysisRequest,
    StructureUsageScreeningRequest, TerrainAnomalyOptions, analyze_network_topology,
    calculate_stringing_and_structures, structure_usage_screening,
};
use ds_grid_model::{AlignmentId, StructureTypeId, TableKind, TensionSectionId};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::package;

const MAX_PARAMS_BYTES: u64 = 16 * 1024 * 1024;

pub static COMMAND: Command = Command {
    id: "dsgrid.run",
    path: &["dsgrid", "run"],
    contract: 1,
    summary: "Run one native DS Grid read, solve, or proposal headlessly.",
    purpose: "\
Opens one verified .dsgrid package and executes an operation published by the \
native engine's live descriptor catalogue. Only non-journaled read, solve and \
propose operations are admitted. The source file is never changed, no command \
enters the model journal, and the typed result is recursively bounded with \
explicit truncation receipts.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The source .dsgrid package.").required(),
        Arg::value(
            "operation",
            "<id>",
            "One non-journaled read, solve, or propose operation id.",
        )
        .required(),
        Arg::value(
            "params",
            "<json-path>",
            "JSON object matching the live operation descriptor; omit for parameterless operations.",
        ),
        Arg::value("limit", "<n>", "Cap every returned JSON collection.")
            .default(package::DEFAULT_LIMIT),
    ],
    output: "\
The exact source package identity and authored revision, engine and operation \
descriptor identity, a typed bounded result, staged:false and persisted:false. \
`more.truncated` names every collection shortened by --limit with exact totals.",
    examples: &[
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation project_plan --output json",
            note: "Read stable plan entity ids from the authored revision.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation project_profile --params ./profile.json --limit 200 --output json",
            note: "Run a parameterized native projection with bounded output.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "model_not_found",
            when: "the source path does not exist or is not a file",
            remedy: "check the path; --model takes one .dsgrid file",
        },
        Refusal {
            code: "model_too_large",
            when: "the source is above the 512 MiB read bound",
            remedy: "confirm the file is a .dsgrid package and not a disk image",
        },
        Refusal {
            code: "model_unreadable",
            when: "the source exists but cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "not_a_dsgrid_package",
            when: "the source bytes are not a readable .dsgrid container",
            remedy: "convert the native source through dsgrid-exchange first",
        },
        Refusal {
            code: "package_decode_failed",
            when: "the source manifest or canonical tables do not verify",
            remedy: "run `ds dsgrid validate --model <path>` and repair the package",
        },
        Refusal {
            code: "unknown_operation",
            when: "the compiled engine publishes no operation with that id",
            remedy: "run `ds dsgrid describe --kind operations` and choose an exact id",
        },
        Refusal {
            code: "operation_not_read_only",
            when: "the operation journals, mutates, imports, or exports model state",
            remedy: "use `ds dsgrid apply` for a deliberate revision-gated mutation",
        },
        Refusal {
            code: "params_not_found",
            when: "--params does not name one regular file",
            remedy: "write one JSON object matching the operation descriptor",
        },
        Refusal {
            code: "params_too_large",
            when: "the params document exceeds 16 MiB",
            remedy: "use one bounded operation request",
        },
        Refusal {
            code: "params_unreadable",
            when: "the params file exists but cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "params_invalid",
            when: "the JSON is not an object matching the live descriptor",
            remedy: "read `ds dsgrid describe --kind operations --id <id>` and supply its exact fields",
        },
        Refusal {
            code: "operation_failed",
            when: "the native engine refuses the typed request against this authored revision",
            remedy: "read detail.engine and use ids from a projection of this exact package revision",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..5000",
            remedy: "pass a limit inside the range, or omit it for the default of 50",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlignmentParams {
    alignment_id: AlignmentId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableParams {
    table_kind: TableKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructureTypeParams {
    structure_type_id: StructureTypeId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionParams {
    section_ids: Vec<TensionSectionId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestParams<T> {
    request: T,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_path = inputs.require("model")?;
    let operation_id = inputs.require("operation")?;
    let limit = package::parse_limit(inputs.value("limit"))?;
    let descriptor = operation_descriptor(operation_id)?;
    admit(&descriptor)?;
    let params = read_params(inputs.value("params"))?;
    validate_params(&descriptor, &params)?;

    let bytes = package::read_bytes(raw_path)?;
    let package = package::decode(raw_path, &bytes)?;
    let session = GridSession::open(package.snapshot);
    let authored_revision = session.current_revision().revision_id.clone();
    let result = dispatch(operation_id, &params, &session)?;
    let (result, truncated) = bound_result(result, limit);

    let mut answer = json!({
        "source": {
            "path": raw_path,
            "model_id": package.manifest.model.model_id.as_str(),
            "package_revision": package.manifest.model.model_revision,
            "authored_revision": authored_revision.as_str(),
            "package_sha256": format!("sha256:{:x}", Sha256::digest(&bytes)),
        },
        "engine": ds_grid_engine::ENGINE_VERSION,
        "operation": {
            "id": descriptor.operation_id,
            "semantic_version": descriptor.semantic_version,
            "effect": descriptor.effect_class,
            "result_type": descriptor.result_type,
            "journaled": descriptor.journaled,
        },
        "staged": false,
        "persisted": false,
        "result": result,
    });
    if !truncated.is_empty() {
        answer["more"] = json!({ "truncated": truncated });
    }
    Ok(answer)
}

fn operation_descriptor(operation_id: &str) -> Result<OperationDescriptor, Failure> {
    let descriptors = operation_descriptors();
    if let Some(descriptor) = descriptors
        .iter()
        .find(|descriptor| descriptor.operation_id == operation_id)
    {
        return Ok(descriptor.clone());
    }
    let known: Vec<&str> = descriptors
        .iter()
        .filter(|descriptor| is_admitted(descriptor))
        .map(|descriptor| descriptor.operation_id.as_str())
        .collect();
    Err(Failure::invalid(
        "unknown_operation",
        format!("this engine publishes no operation named `{operation_id}`"),
    )
    .remedy("run `ds dsgrid describe --kind operations` for the exact ids")
    .detail(json!({ "admitted_operations": known })))
}

fn is_admitted(descriptor: &OperationDescriptor) -> bool {
    !descriptor.journaled
        && matches!(
            descriptor.effect_class,
            EffectClass::Read | EffectClass::Solve | EffectClass::Propose
        )
}

fn admit(descriptor: &OperationDescriptor) -> Result<(), Failure> {
    if is_admitted(descriptor) {
        return Ok(());
    }
    Err(Failure::invalid(
        "operation_not_read_only",
        format!(
            "`{}` is a {:?} operation and is not admitted by dsgrid run",
            descriptor.operation_id, descriptor.effect_class
        ),
    )
    .remedy("use `ds dsgrid apply` for deliberate revision-gated model mutation"))
}

fn read_params(raw_path: Option<&str>) -> Result<Value, Failure> {
    let Some(raw_path) = raw_path else {
        return Ok(json!({}));
    };
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("params_not_found", format!("cannot read `{raw_path}`"))
            .remedy("--params takes one JSON object file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("params_not_found", format!("`{raw_path}` is not a file"))
                .remedy("--params takes one JSON object file"),
        );
    }
    if metadata.len() > MAX_PARAMS_BYTES {
        return Err(Failure::invalid(
            "params_too_large",
            format!("`{raw_path}` exceeds the params bound"),
        )
        .remedy("use one bounded operation request")
        .detail(json!({ "byte_len": metadata.len(), "max_byte_len": MAX_PARAMS_BYTES })));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed("params_unreadable", format!("cannot read `{raw_path}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid("params_invalid", "the params document is not valid JSON")
            .remedy("supply one JSON object matching the live operation descriptor")
            .detail(json!({ "detail": error.to_string() }))
    })?;
    if !value.is_object() {
        return Err(Failure::invalid(
            "params_invalid",
            "the params document must be a JSON object",
        )
        .remedy("read the live operation descriptor and supply its named fields"));
    }
    Ok(value)
}

fn validate_params(descriptor: &OperationDescriptor, params: &Value) -> Result<(), Failure> {
    let object = params.as_object().expect("read_params returns an object");
    let known: Vec<&str> = descriptor
        .params
        .iter()
        .map(|spec| spec.name.as_str())
        .collect();
    let unknown: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|name| !known.contains(name))
        .collect();
    let missing: Vec<&str> = descriptor
        .params
        .iter()
        .filter(|spec| spec.required && !object.contains_key(&spec.name))
        .map(|spec| spec.name.as_str())
        .collect();
    if unknown.is_empty() && missing.is_empty() {
        return Ok(());
    }
    Err(Failure::invalid(
        "params_invalid",
        format!("params do not match `{}`", descriptor.operation_id),
    )
    .remedy(format!(
        "run `ds dsgrid describe --kind operations --id {}`",
        descriptor.operation_id
    ))
    .detail(json!({ "unknown": unknown, "missing": missing, "accepted": known })))
}

fn parse<T: DeserializeOwned>(operation_id: &str, params: &Value) -> Result<T, Failure> {
    serde_json::from_value(params.clone()).map_err(|error| {
        Failure::invalid(
            "params_invalid",
            format!("params are not valid for `{operation_id}`"),
        )
        .remedy(format!(
            "run `ds dsgrid describe --kind operations --id {operation_id}`"
        ))
        .detail(json!({ "detail": error.to_string() }))
    })
}

fn dispatch(operation_id: &str, params: &Value, session: &GridSession) -> Result<Value, Failure> {
    match operation_id {
        "project_plan" => serialize(operation_id, session.plan_projection()),
        "project_profile" => {
            let params: AlignmentParams = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .profile_projection(&params.alignment_id)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "project_table" => {
            let params: TableParams = parse(operation_id, params)?;
            serialize(operation_id, session.table_projection(params.table_kind))
        }
        "project_tagged_alignment_lengths" => {
            let params: TaggedAlignmentLengthsRequest = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .tagged_alignment_lengths(&params)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "project_profile_atlas" => {
            let options: ProfileAtlasOptions = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .profile_atlas_scene(options)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "project_structure_library" => {
            let params: StructureTypeParams = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .structure_library_scene(&params.structure_type_id)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "project_model_library" => serialize(operation_id, session.model_library_projection()),
        "project_criteria_workbench" => {
            serialize(operation_id, session.criteria_workbench_projection())
        }
        "project_profile_sag_criterion_options" => {
            let params: SectionParams = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .profile_sag_criterion_options(&params.section_ids)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "run_structure_analysis" => {
            let params: RequestParams<StructureAnalysisRequest> = parse(operation_id, params)?;
            let mut store = ResultStore::new();
            let result_id = store
                .run_structure_analysis(
                    session.snapshot(),
                    session.current_revision(),
                    &params.request,
                )
                .map_err(|error| engine_error(operation_id, error))?;
            let artifact = store.get(&result_id).ok_or_else(|| {
                engine_error(operation_id, "engine returned an unresolvable result id")
            })?;
            serialize(operation_id, artifact)
        }
        "screen_structure_usage" => {
            let params: RequestParams<StructureUsageScreeningRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                structure_usage_screening(session.snapshot(), &params.request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "calculate_stringing_and_structures" => {
            let params: RequestParams<NetworkCalculationRequest> = parse(operation_id, params)?;
            serialize(
                operation_id,
                calculate_stringing_and_structures(session.snapshot(), &params.request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "analyze_network_topology" => serialize(
            operation_id,
            analyze_network_topology(session.snapshot())
                .map_err(|error| engine_error(operation_id, error))?,
        ),
        "compute_support_demands" => {
            let request: SectionDemandsRequest = parse(operation_id, params)?;
            let mut store = ResultStore::new();
            let result_id = store
                .store_support_demands(session.snapshot(), session.current_revision(), &request)
                .map_err(|error| engine_error(operation_id, error))?;
            let artifact = store.get(&result_id).ok_or_else(|| {
                engine_error(operation_id, "engine returned an unresolvable result id")
            })?;
            serialize(operation_id, artifact)
        }
        "feature_code_report" => serialize(operation_id, session.feature_code_report()),
        "terrain_anomaly_analysis" => {
            let options: TerrainAnomalyOptions = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .terrain_anomaly_report(&options)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "plan_optimum_spotting" => {
            let request: SpottingPlanRequest = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .plan_optimum_spotting(&request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        // The descriptor admission check makes this unreachable. Keep the
        // branch typed so adding a descriptor without wiring it fails safely.
        _ => Err(Failure::failed(
            "operation_failed",
            format!("`{operation_id}` is admitted but has no CLI dispatcher"),
        )
        .remedy("report the missing dsgrid.run dispatcher")),
    }
}

fn serialize<T: serde::Serialize>(operation_id: &str, value: T) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| {
        Failure::failed(
            "operation_failed",
            format!("`{operation_id}` returned a result that could not be serialized"),
        )
        .remedy("report this engine/CLI contract defect")
        .detail(json!({ "detail": error.to_string() }))
    })
}

fn engine_error(operation_id: &str, error: impl std::fmt::Display) -> Failure {
    Failure::failed(
        "operation_failed",
        format!("the native engine refused `{operation_id}`"),
    )
    .remedy("use ids and authored values from this exact package revision")
    .detail(json!({ "engine": error.to_string() }))
}

fn bound_result(mut result: Value, limit: usize) -> (Value, Vec<Value>) {
    let mut truncated = Vec::new();
    bound_value(&mut result, "result", limit, &mut truncated);
    (result, truncated)
}

fn bound_value(value: &mut Value, path: &str, limit: usize, truncated: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            let total = items.len();
            if total > limit {
                items.truncate(limit);
                truncated.push(json!({
                    "field": path,
                    "total": total,
                    "shown": limit,
                    "withheld": total - limit,
                    "limit": limit,
                }));
            }
            for (index, item) in items.iter_mut().enumerate() {
                bound_value(item, &format!("{path}[{index}]"), limit, truncated);
            }
        }
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                bound_value(child, &format!("{path}.{key}"), limit, truncated);
            }
        }
        _ => {}
    }
}

pub fn render(data: &Value) -> String {
    let source = &data["source"];
    let operation = &data["operation"];
    let mut out = format!(
        "{}  {}\n  model {} · authored {}\n  {} · {} · persisted false\n",
        operation["id"].as_str().unwrap_or("?"),
        operation["effect"].as_str().unwrap_or("?"),
        source["model_id"].as_str().unwrap_or("?"),
        source["authored_revision"].as_str().unwrap_or("?"),
        data["engine"].as_str().unwrap_or("?"),
        operation["result_type"].as_str().unwrap_or("?"),
    );
    if let Some(rows) = data["more"]["truncated"].as_array() {
        out.push_str(&format!(
            "  {} bounded collection(s); use --limit to adjust\n",
            rows.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_admitted_engine_operation_has_a_dispatch_branch() {
        let source = include_str!("run.rs");
        for descriptor in operation_descriptors().iter().filter(|op| is_admitted(op)) {
            assert!(
                source.contains(&format!("\"{}\" =>", descriptor.operation_id)),
                "{} is admitted but not dispatched",
                descriptor.operation_id
            );
        }
    }

    #[test]
    fn recursive_bounding_is_explicit_at_every_shortened_path() {
        let value = json!({
            "rows": [1, 2, 3],
            "nested": { "items": [4, 5, 6] },
        });
        let (bounded, truncated) = bound_result(value, 2);
        assert_eq!(bounded["rows"], json!([1, 2]));
        assert_eq!(bounded["nested"]["items"], json!([4, 5]));
        assert_eq!(truncated.len(), 2);
        let rows = truncated
            .iter()
            .find(|receipt| receipt["field"] == "result.rows")
            .expect("rows truncation is explicit");
        assert_eq!(rows["total"], 3);
        assert_eq!(rows["withheld"], 1);
    }
}
