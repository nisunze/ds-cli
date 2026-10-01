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
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::TaggedAlignmentLengthsRequest;
use ds_grid_engine::descriptor::operation_descriptors;
use ds_grid_engine::{
    EffectClass, EngineeringAttributeEvidence, GridSession, NetworkCalculationRequest,
    OperationDescriptor, ProfileAtlasOptions, ResultStore, SectionDemandsRequest,
    SpottingPlanError, SpottingPlanRequest, StructureAnalysisRequest,
    StructureCapacityTablesRequest, StructureUsageScreeningRequest, TerrainAnomalyOptions,
    analyze_network_topology, calculate_stringing_and_structures, structure_capacity_tables,
    structure_usage_screening,
};
use ds_grid_model::{
    AlignmentId, EntityId, StructureLabelPolicy, StructureTypeId, TableKind, TensionSectionId,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::package;

const MAX_PARAMS_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WholeModelSpottingParams {
    #[serde(default)]
    alignment_ids: Option<Vec<AlignmentId>>,
    #[serde(default)]
    settings: Option<ds_grid_model::SpottingSettings>,
    #[serde(default)]
    memory_budget_bytes: Option<u64>,
    #[serde(default)]
    max_workers: Option<usize>,
}

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
explicit truncation receipts. Profile index requests use the default native \
atlas; copy its revision and axis pin from project_profile_atlas with default options.",
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
        Arg::value(
            "limit",
            "<n>",
            "Cap every returned JSON collection outside a digest-sealed plan.",
        )
        .default(package::DEFAULT_LIMIT),
    ],
    output: "\
The exact source package identity and authored revision, engine and operation \
descriptor identity, a typed bounded result, staged:false and persisted:false. \
`more.truncated` names every collection shortened by --limit with exact totals. \
A plan sealed by plan_digest is never shortened, so the digest-verified apply \
path can check it; its request's max_reported_rejections bounds its rows.",
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
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation analyze_model_defaults --params ./analysis.json --output json",
            note: "Supply the native DefaultAnalysisRequest inside request; copy its revision and engineering root from project_criteria_workbench on this file.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation compute_structure_conductor_loads --params ./conductor-loads.json --output json",
            note: "Read conductor-only attachment and support forces for explicit structures at the pinned model head; upward demand is not anchored capacity or a strength failure.",
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
            remedy: "read detail.refusal when present, then use ids and authored values from this exact package revision",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..10000",
            remedy: "pass a limit inside the range, or omit it for the default of 50",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "conductor loads",
        "attachment forces",
        "upward demand",
        "model analysis",
    ],
    requires: Requires::Server,
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
struct ProfilePropertiesParams {
    entity_id: EntityId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainSamplingRegularityParams {
    alignment_id: AlignmentId,
    corridor_m: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceConsistencyParams {
    query: ds_grid_engine::SurfaceObservation,
    references: Vec<ds_grid_engine::SurfaceObservation>,
    #[serde(default)]
    options: ds_grid_engine::SurfaceConsistencyOptions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestParams<T> {
    request: T,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_path = inputs.require("model")?;
    let operation_id = inputs.require("operation")?;
    // Read-only projections may contain more than 5,000 rows even for one
    // ordinary MV model. Keep this ceiling local to run; mutation/list bounds
    // retain their smaller contract.
    let limit = match inputs.value("limit").unwrap_or("50").parse::<usize>() {
        Ok(value @ 1..=10_000) => value,
        _ => {
            return Err(Failure::invalid(
                "invalid_limit",
                "--limit must be 1..10000",
            ));
        }
    };
    let descriptor = operation_descriptor(operation_id)?;
    admit(&descriptor)?;
    let params = read_params(inputs.value("params"))?;
    validate_params(&descriptor, &params)?;

    let bytes = package::read_bytes(raw_path)?;
    let package = package::decode(raw_path, &bytes)?;
    let session = GridSession::open(package.snapshot);
    let authored_revision = session.current_revision().revision_id.clone();
    let evidence = if operation_id == "profile_properties" {
        package
            .assets
            .iter()
            .find(|asset| {
                asset.invariant_leaf == ds_grid_exchange::ENGINEERING_ATTRIBUTE_EVIDENCE_LEAF
            })
            .map(|asset| ds_grid_exchange::decode_engineering_attribute_evidence(&asset.bytes))
            .transpose()
            .map_err(|error| engine_error(operation_id, error))?
            .unwrap_or_default()
    } else {
        EngineeringAttributeEvidence::default()
    };
    let profile_labels = package
        .manifest
        .model
        .presentation
        .effective_profile_structure_labels();
    let result = dispatch(operation_id, &params, &session, &evidence, &profile_labels)?;
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

fn dispatch(
    operation_id: &str,
    params: &Value,
    session: &GridSession,
    evidence: &EngineeringAttributeEvidence,
    profile_labels: &StructureLabelPolicy,
) -> Result<Value, Failure> {
    match operation_id {
        "profile_properties" => {
            let request: ProfilePropertiesParams = parse(operation_id, params)?;
            ds_grid_engine::profile_properties(
                session.snapshot(),
                &session.current_revision().revision_id,
                request.entity_id.as_str(),
                evidence,
            )
            .ok_or_else(|| {
                engine_error(
                    operation_id,
                    format!("no Profile properties for entity {}", request.entity_id),
                )
            })
        }
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
            let mut scene = session
                .profile_atlas_scene(options)
                .map_err(|error| engine_error(operation_id, error))?;
            ds_grid_engine::profile_labels::compose_scene_structure_labels(
                &mut scene,
                profile_labels,
            );
            serialize(operation_id, scene)
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
        "structure_capacity_tables" => {
            let params: RequestParams<StructureCapacityTablesRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                structure_capacity_tables(session.snapshot(), &params.request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
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
        "screen_selected_structure_usage" => {
            let params: RequestParams<ds_grid_engine::SelectedStructureUsageRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                ds_grid_engine::selected_structure_usage_screening(
                    session.snapshot(),
                    &session.current_revision().revision_id,
                    &params.request,
                )
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
        "analyze_model_defaults" => {
            let params: RequestParams<ds_grid_engine::DefaultAnalysisRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .analyze_model_defaults(&params.request)
                    .map_err(|error| typed_engine_error(operation_id, error))?,
            )
        }
        "compute_structure_conductor_loads" => {
            let params: RequestParams<ds_grid_engine::StructureConductorLoadsRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .compute_structure_conductor_loads(&params.request)
                    .map_err(|error| typed_engine_error(operation_id, error))?,
            )
        }
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
        // The structure list with REG rule findings (contract 04 §5). The
        // typed `ds dsgrid report structures` adds the retained native line
        // angles beside it; through `run` the model's own geometry answers.
        "report_structures" => serialize(
            operation_id,
            ds_grid_engine::report_structures(session.snapshot(), None)
                .map_err(|error| engine_error(operation_id, error))?,
        ),
        // A structure-rules standard's design-policy facts (REG v7 bundled;
        // any ds.*-structure-rules/v1 document by path): what a design
        // policy row is authored from, so its values are the standard's.
        "design_policy_facts_from_standard" => {
            let request: ds_grid_engine::DesignPolicyFactsRequest = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .design_policy_facts_from_standard(&request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "clearance_report" => {
            let options: ds_grid_engine::ClearanceReportOptions = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .clearance_report(&options)
                    .map_err(|error| typed_engine_error(operation_id, error))?,
            )
        }
        "engineering_issue_layer" => {
            let params: RequestParams<ds_grid_engine::EngineeringIssueLayerRequest> =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .engineering_issue_layer(&params.request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "profile_geometry_create_plan" => {
            let params: RequestParams<
                ds_grid_engine::profile_geometry_create::ProfileGeometryCreateRequest,
            > = parse(operation_id, params)?;
            let index = native_profile_index(operation_id, session)?;
            serialize(
                operation_id,
                ds_grid_engine::profile_geometry_create::plan_profile_geometry_create(
                    session.snapshot(),
                    &session.current_revision().revision_id,
                    &index,
                    &params.request,
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "profile_selection_lasso" => {
            let params: RequestParams<ds_grid_engine::profile_lasso::ProfileSelectionLassoRequest> =
                parse(operation_id, params)?;
            let index = native_profile_index(operation_id, session)?;
            serialize(
                operation_id,
                index
                    .selection_lasso(&params.request)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "terrain_anomaly_analysis" => {
            let options: TerrainAnomalyOptions = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .terrain_anomaly_report(&options)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "profile_anomaly_layer" => {
            let options: TerrainAnomalyOptions = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .profile_anomaly_layer(&options)
                    .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "terrain_sampling_regularity" => {
            let request: TerrainSamplingRegularityParams = parse(operation_id, params)?;
            serialize(
                operation_id,
                ds_grid_engine::terrain_sampling_regularity(
                    session.snapshot(),
                    &request.alignment_id,
                    request
                        .corridor_m
                        .unwrap_or(ds_grid_engine::projection::DEFAULT_PROFILE_CORRIDOR_M),
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "surface_consistency" => {
            let request: SurfaceConsistencyParams = parse(operation_id, params)?;
            serialize(
                operation_id,
                ds_grid_engine::surface_consistency(
                    &request.query,
                    &request.references,
                    &request.options,
                ),
            )
        }
        "spotting_graph" => serialize(
            operation_id,
            ds_grid_engine::spotting_graph::spotting_graph(session.snapshot())
                .map_err(|error| engine_error(operation_id, error))?,
        ),
        "spotting_settings" => serialize(
            operation_id,
            ds_grid_engine::spotting::requests::spotting_settings_projection(session.snapshot()),
        ),
        "derive_spotting_requests" => {
            let request: ds_grid_engine::spotting::requests::DeriveSpottingRequestsParams =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                ds_grid_engine::spotting::requests::derive_spotting_requests_with(
                    session.snapshot(),
                    &request,
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "spotting_layout_application" => {
            let request: ds_grid_engine::spotting::apply::SpottingLayoutApplyRequest =
                parse(operation_id, params)?;
            serialize(
                operation_id,
                ds_grid_engine::spotting::apply::spotting_layout_application(
                    session.snapshot(),
                    session.current_revision(),
                    &request,
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "plan_whole_model_spotting" => {
            let WholeModelSpottingParams {
                alignment_ids,
                settings,
                memory_budget_bytes,
                max_workers,
            } = parse(operation_id, params)?;
            let settings = match settings {
                Some(settings) => settings,
                None => ds_grid_engine::spotting::requests::stored_spotting_settings(
                    session.snapshot(),
                )
                .cloned()
                .ok_or_else(|| {
                    engine_error(
                        operation_id,
                        ds_grid_engine::spotting::requests::SpottingRequestDerivationError::NoSpottingSettings,
                    )
                })?,
            };
            let memory_budget_bytes =
                memory_budget_bytes.or_else(crate::host_memory::spotting_memory_budget_bytes);
            let max_workers = if memory_budget_bytes.is_none() {
                max_workers.or(Some(1))
            } else {
                max_workers
            };
            serialize(
                operation_id,
                ds_grid_engine::spotting::batch::plan_whole_model_for_alignments(
                    session.snapshot(),
                    session.current_revision(),
                    &settings,
                    alignment_ids.as_deref(),
                    memory_budget_bytes,
                    max_workers,
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "plan_optimum_spotting_batch" => {
            let mut request: ds_grid_engine::spotting::batch::SpottingBatchRequest =
                parse(operation_id, params)?;
            // Parallel by default, within what this machine can spare. A host
            // that reports no memory figures runs one search at a time.
            if request.memory_budget_bytes.is_none() {
                match crate::host_memory::spotting_memory_budget_bytes() {
                    Some(budget) => request.memory_budget_bytes = Some(budget),
                    None => request.max_workers = request.max_workers.or(Some(1)),
                }
            }
            serialize(
                operation_id,
                ds_grid_engine::spotting::batch::plan_optimum_spotting_batch(
                    session.snapshot(),
                    session.current_revision(),
                    &request,
                )
                .map_err(|error| engine_error(operation_id, error))?,
            )
        }
        "plan_optimum_spotting" => {
            let request: SpottingPlanRequest = parse(operation_id, params)?;
            serialize(
                operation_id,
                session
                    .plan_optimum_spotting(&request)
                    .map_err(|error| spotting_error(operation_id, error))?,
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

fn native_profile_index(
    operation_id: &str,
    session: &GridSession,
) -> Result<ds_grid_engine::profile_hit::ProfilePickIndex, Failure> {
    let scene = session
        .profile_atlas_scene(ProfileAtlasOptions::default())
        .map_err(|error| engine_error(operation_id, error))?;
    Ok(ds_grid_engine::profile_hit::ProfilePickIndex::from_scene(
        &scene,
    ))
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

/// Preserve the planner's stable refusal variant and exact fields for a
/// headless caller. The human-readable engine message remains alongside it,
/// but automation never has to parse that prose to decide what authoring or
/// search input is missing.
fn spotting_error(operation_id: &str, error: SpottingPlanError) -> Failure {
    typed_engine_error(operation_id, error)
}

/// Carry an owner's serializable refusal without interpreting its fields.
fn typed_engine_error(
    operation_id: &str,
    error: impl std::fmt::Display + serde::Serialize,
) -> Failure {
    let message = error.to_string();
    let refusal = serde_json::to_value(&error).unwrap_or_else(|serialization_error| {
        json!({
            "code": "serialization_failed",
            "detail": serialization_error.to_string(),
        })
    });
    Failure::failed(
        "operation_failed",
        format!("the native engine refused `{operation_id}`"),
    )
    .remedy("read detail.refusal and author or adjust only the facts it identifies")
    .detail(json!({
        "engine": message,
        "refusal": refusal,
    }))
}

fn bound_result(mut result: Value, limit: usize) -> (Value, Vec<Value>) {
    let mut truncated = Vec::new();
    bound_value(&mut result, "result", limit, &mut truncated);
    (result, truncated)
}

pub(crate) fn bound_value(value: &mut Value, path: &str, limit: usize, truncated: &mut Vec<Value>) {
    match value {
        // A spotting plan's digest covers its whole content, diagnostic
        // `rejected.rows` and provisional `blocked_by` included: one shortened
        // collection leaves a plan no apply door can verify. The engine
        // already bounds those rows by the request's max_reported_rejections,
        // so a sealed plan crosses whole.
        Value::Object(object) if is_digest_sealed(object) => {}
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

fn is_digest_sealed(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("plan_digest")
        .and_then(Value::as_str)
        .is_some_and(|digest| digest.starts_with("sha256:"))
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
    fn clearance_bound_case_refusals_preserve_native_identity_and_model_revision() {
        use ds_grid_engine::{ClearanceCase, ClearanceReportOptions};
        use ds_grid_model::{
            AnalysisCaseId, AnalysisCaseRow, CableCondition, CaseBindingRole,
            CriterionCaseBindingId, CriterionCaseBindingRow, CriterionSetId, CriterionSetRow,
            GridModelSnapshot, SolverCapability, WeatherStateId, WindDirectionPolicy,
        };

        for (role, case, binding_role) in [
            (
                "vertical",
                ClearanceCase::Vertical,
                CaseBindingRole::SurveyPointVerticalClearance,
            ),
            (
                "horizontal",
                ClearanceCase::Horizontal,
                CaseBindingRole::SurveyPointHorizontalClearance,
            ),
        ] {
            for missing_weather in [false, true] {
                let set_id = CriterionSetId::new("set-clearance").unwrap();
                let case_id = AnalysisCaseId::new(format!("case-{role}-missing")).unwrap();
                let mut snapshot = GridModelSnapshot::default();
                snapshot.criterion_sets.push(CriterionSetRow {
                    id: set_id.clone(),
                    label: "Clearance".to_string(),
                });
                snapshot
                    .criterion_case_bindings
                    .push(CriterionCaseBindingRow {
                        id: CriterionCaseBindingId::new("binding-clearance").unwrap(),
                        set_id,
                        role: binding_role,
                        slot_ordinal: None,
                        analysis_case_id: case_id.clone(),
                    });
                if missing_weather {
                    snapshot.analysis_cases.push(AnalysisCaseRow {
                        id: case_id.clone(),
                        label: "Missing weather".to_string(),
                        weather_state_id: WeatherStateId::new("weather-missing").unwrap(),
                        condition: CableCondition::AfterCreep,
                        wind_direction: WindDirectionPolicy::TransverseBothSigns,
                        wire_load_factor: 1.0,
                        structure_load_factor: None,
                        solver: SolverCapability::RulingSpanCableState,
                    });
                }
                let session = GridSession::open(snapshot);
                let before = session.snapshot().clone();
                let revision = session.current_revision().clone();
                let options = ClearanceReportOptions {
                    case,
                    ..Default::default()
                };
                let native = session.clearance_report(&options).unwrap_err();
                let expected = json!({
                    "code": "bound_case_reference_unresolved",
                    "detail": {
                        "criterion_set": "set-clearance",
                        "role": role,
                        "analysis_case_id": case_id.as_str(),
                        "reason": if missing_weather {
                            "weather state weather-missing is not in the model"
                        } else {
                            "analysis case is not in the model"
                        },
                    },
                });
                assert_eq!(serde_json::to_value(&native).unwrap(), expected);
                let direct = crate::analyse::clearance::map_error(native.clone());
                assert_eq!(direct.code(), "bound_case_reference_unresolved");
                assert_eq!(
                    direct.class(),
                    ds_cli_contract::outcome::ExitClass::InvalidInput
                );
                assert_eq!(direct.message(), native.to_string());
                assert_eq!(direct.detail_value().unwrap()["refusal"], expected);
                let declared = crate::analyse::clearance::COMMAND
                    .refusals
                    .iter()
                    .find(|refusal| refusal.code == direct.code())
                    .unwrap();
                assert_eq!(direct.remedy_text(), Some(declared.remedy));

                let error = dispatch(
                    "clearance_report",
                    &serde_json::to_value(&options).unwrap(),
                    &session,
                    &EngineeringAttributeEvidence::default(),
                    &StructureLabelPolicy::default(),
                )
                .unwrap_err();
                assert_eq!(error.code(), "operation_failed");
                assert_eq!(error.class(), ds_cli_contract::outcome::ExitClass::Failed);
                assert!(
                    COMMAND
                        .refusals
                        .iter()
                        .any(|refusal| refusal.code == error.code())
                );
                assert_eq!(error.detail_value().unwrap()["refusal"], expected);
                assert_eq!(error.detail_value().unwrap()["engine"], native.to_string());
                assert_eq!(session.snapshot(), &before);
                assert_eq!(session.current_revision(), &revision);
            }
        }
    }

    #[test]
    fn default_analysis_admits_only_the_native_request_shape() {
        let operation = "analyze_model_defaults";
        let descriptor = operation_descriptor(operation).unwrap();
        admit(&descriptor).expect("non-journaled native solve");
        assert_eq!(descriptor.result_type, "DefaultAnalysisReport");
        assert_eq!(descriptor.params.len(), 1);
        assert_eq!(descriptor.params[0].name, "request");
        assert_eq!(descriptor.params[0].value_type, "DefaultAnalysisRequest");
        assert!(descriptor.params[0].required);

        let request = json!({
            "expected_revision": "rev:test",
            "expected_engineering_input_root": "test-root",
            "max_rows": 7,
        });
        let params = json!({ "request": request });
        validate_params(&descriptor, &params).unwrap();
        let parsed: RequestParams<ds_grid_engine::DefaultAnalysisRequest> =
            parse(operation, &params).unwrap();
        assert_eq!(serde_json::to_value(parsed.request).unwrap(), request);

        validate_params(&descriptor, &json!({})).unwrap_err();
        validate_params(&descriptor, &request).unwrap_err();
        validate_params(&descriptor, &json!({ "request": request, "max_rows": 1 })).unwrap_err();
        for invalid in [
            json!({ "request": {} }),
            json!({ "request": { "expected_revision": "rev:test" } }),
            json!({ "request": { "expected_engineering_input_root": "test-root" } }),
            json!({ "request": { "expected_revision": "rev:test",
                "expected_engineering_input_root": "test-root", "capacity": 100 } }),
            json!({ "request": { "expected_revision": "rev:test",
                "expected_engineering_input_root": "test-root", "max_rows": "7" } }),
        ] {
            assert_eq!(
                parse::<RequestParams<ds_grid_engine::DefaultAnalysisRequest>>(operation, &invalid)
                    .err()
                    .expect("malformed native request is refused")
                    .code(),
                "params_invalid"
            );
        }
        let mut defaulted = request;
        defaulted.as_object_mut().unwrap().remove("max_rows");
        let native: ds_grid_engine::DefaultAnalysisRequest =
            serde_json::from_value(defaulted.clone()).unwrap();
        let parsed: RequestParams<ds_grid_engine::DefaultAnalysisRequest> =
            parse(operation, &json!({ "request": defaulted })).unwrap();
        assert_eq!(parsed.request, native, "defaults belong to the native type");
    }

    #[test]
    fn default_analysis_dispatch_returns_the_exact_native_report_without_authoring() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let package = ds_grid_exchange::unpack(&std::fs::read(path).unwrap()).unwrap();
        let session = GridSession::open(package.snapshot);
        let before = session.snapshot().clone();
        let revision = session.current_revision().clone();
        let request = ds_grid_engine::DefaultAnalysisRequest {
            expected_revision: revision.revision_id.clone(),
            expected_engineering_input_root: revision.roots.engineering_input_root.clone(),
            max_rows: 1,
        };
        let native = session.analyze_model_defaults(&request).unwrap();
        let report = dispatch(
            "analyze_model_defaults",
            &json!({ "request": request }),
            &session,
            &EngineeringAttributeEvidence::default(),
            &StructureLabelPolicy::default(),
        )
        .unwrap();
        assert_eq!(report, serde_json::to_value(&native).unwrap());
        assert_eq!(report["model_revision"], revision.revision_id.as_str());
        assert_eq!(
            report["engineering_input_root"],
            revision.roots.engineering_input_root
        );
        assert!(native.cases.total_count > 1);
        assert!(native.cases.truncated);
        assert_eq!(native.cases.rows.len(), 1);
        assert_eq!(session.snapshot(), &before);
        assert_eq!(session.current_revision(), &revision);
    }

    #[test]
    fn default_analysis_dispatch_preserves_native_fence_and_default_refusals() {
        let session = GridSession::open(ds_grid_model::GridModelSnapshot::default());
        let request = json!({
            "expected_revision": session.current_revision().revision_id,
            "expected_engineering_input_root": session.current_revision().roots.engineering_input_root,
            "max_rows": 1,
        });
        for (field, value, code) in [
            ("expected_revision", json!("rev:stale"), "revision_mismatch"),
            (
                "expected_engineering_input_root",
                json!("stale-root"),
                "engineering_input_root_mismatch",
            ),
            ("max_rows", json!(0), "invalid_row_limit"),
            ("max_rows", json!(10_001), "invalid_row_limit"),
            ("max_rows", json!(1), "no_criterion_set"),
        ] {
            let mut request = request.clone();
            request[field] = value;
            let native_request = serde_json::from_value(request.clone()).unwrap();
            let native = session.analyze_model_defaults(&native_request).unwrap_err();
            let error = dispatch(
                "analyze_model_defaults",
                &json!({ "request": request }),
                &session,
                &EngineeringAttributeEvidence::default(),
                &StructureLabelPolicy::default(),
            )
            .unwrap_err();
            assert_eq!(error.code(), "operation_failed");
            let detail = error.detail_value().unwrap();
            assert_eq!(detail["refusal"], serde_json::to_value(&native).unwrap());
            assert_eq!(detail["refusal"]["code"], code);
            assert_eq!(detail["engine"], native.to_string());
        }
    }

    #[test]
    fn focused_conductor_analysis_admits_the_native_request_without_host_basis_defaults() {
        let operation = "compute_structure_conductor_loads";
        let descriptor = operation_descriptor(operation).unwrap();
        admit(&descriptor).expect("non-journaled focused native solve");
        assert_eq!(descriptor.result_type, "StructureConductorLoadsReport");
        assert_eq!(descriptor.params.len(), 1);
        assert_eq!(descriptor.params[0].name, "request");
        assert_eq!(
            descriptor.params[0].value_type,
            "StructureConductorLoadsRequest"
        );
        assert!(descriptor.params[0].required);
        let request = json!({
            "expected_revision": "rev:test",
            "expected_engineering_input_root": "test-root",
            "structure_ids": ["str-focus"],
        });
        let params = json!({ "request": request });
        validate_params(&descriptor, &params).unwrap();
        let parsed: RequestParams<ds_grid_engine::StructureConductorLoadsRequest> =
            parse(operation, &params).unwrap();
        let native: ds_grid_engine::StructureConductorLoadsRequest =
            serde_json::from_value(request.clone()).unwrap();
        assert_eq!(
            parsed.request, native,
            "default limits belong to the native request"
        );
        validate_params(&descriptor, &request).unwrap_err();
        validate_params(
            &descriptor,
            &json!({ "request": request, "structure_ids": [] }),
        )
        .unwrap_err();
        for invalid in [
            json!({ "expected_revision": "rev:test", "expected_engineering_input_root": "test-root" }),
            json!({ "expected_revision": "rev:test", "expected_engineering_input_root": "test-root",
                "structure_ids": ["str-focus"], "weight_span_basis": { "kind": "conventional" } }),
            json!({ "expected_revision": "rev:test", "expected_engineering_input_root": "test-root",
                "structure_ids": "str-focus" }),
        ] {
            assert_eq!(
                parse::<RequestParams<ds_grid_engine::StructureConductorLoadsRequest>>(
                    operation,
                    &json!({ "request": invalid }),
                )
                .err()
                .expect("malformed native focus is refused")
                .code(),
                "params_invalid"
            );
        }
    }

    #[test]
    fn focused_conductor_dispatch_preserves_native_vectors_and_never_authors() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let package = ds_grid_exchange::unpack(&std::fs::read(path).unwrap()).unwrap();
        let session = GridSession::open(package.snapshot);
        let before = session.snapshot().clone();
        let revision = session.current_revision().clone();
        let structure_id = before.tension_section_supports[0].structure_id.clone();
        let request = ds_grid_engine::StructureConductorLoadsRequest {
            expected_revision: revision.revision_id.clone(),
            expected_engineering_input_root: revision.roots.engineering_input_root.clone(),
            structure_ids: vec![structure_id.clone()],
            max_rows: 1,
        };
        let native = session.compute_structure_conductor_loads(&request).unwrap();
        let report = dispatch(
            "compute_structure_conductor_loads",
            &json!({ "request": request }),
            &session,
            &EngineeringAttributeEvidence::default(),
            &StructureLabelPolicy::default(),
        )
        .unwrap();
        assert_eq!(report, serde_json::to_value(&native).unwrap());
        assert_eq!(report["model_revision"], revision.revision_id.as_str());
        assert_eq!(
            report["engineering_input_root"],
            revision.roots.engineering_input_root
        );
        assert_eq!(report["structure_ids"], json!([structure_id.as_str()]));
        assert!(native.incident_section_count > 0);
        assert!(native.evaluated_section_count > 0);
        assert!(native.cases.total_count > 1 && native.cases.truncated);
        let whole = &native.whole_support_conductor_loads;
        assert!(whole.complete_case_state_count + whole.incomplete_case_state_count > 0);
        assert!(
            whole
                .loads
                .rows
                .iter()
                .all(|load| load.coverage.structure_id == structure_id)
        );
        assert!(
            whole
                .incomplete
                .rows
                .iter()
                .all(|load| load.coverage.structure_id == structure_id)
        );
        assert!(report.get("clearance").is_none() && report.get("structures").is_none());
        assert_eq!(session.snapshot(), &before);
        assert_eq!(session.current_revision(), &revision);
    }

    #[test]
    fn focused_conductor_dispatch_preserves_exact_native_refusals_and_pin_fields() {
        let session = GridSession::open(ds_grid_model::GridModelSnapshot::default());
        let request = json!({
            "expected_revision": session.current_revision().revision_id,
            "expected_engineering_input_root": session.current_revision().roots.engineering_input_root,
            "structure_ids": ["str-missing"],
            "max_rows": 1,
        });
        for (field, value, outer, nested) in [
            (
                "expected_revision",
                json!("rev:stale"),
                "analysis_unavailable",
                Some("revision_mismatch"),
            ),
            (
                "expected_engineering_input_root",
                json!("stale-root"),
                "analysis_unavailable",
                Some("engineering_input_root_mismatch"),
            ),
            (
                "max_rows",
                json!(0),
                "analysis_unavailable",
                Some("invalid_row_limit"),
            ),
            (
                "max_rows",
                json!(10_001),
                "analysis_unavailable",
                Some("invalid_row_limit"),
            ),
            (
                "structure_ids",
                json!([]),
                "invalid_structure_selection",
                None,
            ),
            (
                "structure_ids",
                json!(["str-missing", "str-missing"]),
                "invalid_structure_selection",
                None,
            ),
            (
                "structure_ids",
                json!(["str-missing"]),
                "structure_not_found",
                None,
            ),
        ] {
            let mut request = request.clone();
            request[field] = value;
            let native_request = serde_json::from_value(request.clone()).unwrap();
            let native = session
                .compute_structure_conductor_loads(&native_request)
                .unwrap_err();
            let error = dispatch(
                "compute_structure_conductor_loads",
                &json!({ "request": request }),
                &session,
                &EngineeringAttributeEvidence::default(),
                &StructureLabelPolicy::default(),
            )
            .unwrap_err();
            assert_eq!(error.code(), "operation_failed");
            let detail = error.detail_value().unwrap();
            assert_eq!(detail["refusal"], serde_json::to_value(&native).unwrap());
            assert_eq!(detail["refusal"]["code"], outer);
            if let Some(code) = nested {
                assert_eq!(detail["refusal"]["detail"]["refusal"]["code"], code);
            }
            assert_eq!(detail["engine"], native.to_string());
        }
    }

    #[test]
    fn engineering_issue_layer_admits_exact_typed_request_wrapper() {
        let descriptor = operation_descriptor("engineering_issue_layer").unwrap();
        assert!(is_admitted(&descriptor));
        let params = json!({ "request": { "clearance": {}, "structure_screening": null,
            "max_features_per_kind": 42,
            "filter": { "nature": "vertical_clearance", "minimum_vertical_deficit_m": 0.5 } } });
        validate_params(&descriptor, &params).expect("descriptor admits request wrapper");
        let parsed: RequestParams<ds_grid_engine::EngineeringIssueLayerRequest> =
            parse("engineering_issue_layer", &params).expect("typed native request");
        assert_eq!(parsed.request.max_features_per_kind, 42);
        assert!(parsed.request.structure_screening.is_none());
        assert_eq!(parsed.request.filter.minimum_vertical_deficit_m, Some(0.5));
        assert_eq!(
            parsed.request.filter.nature,
            Some(ds_grid_engine::EngineeringIssueNature::VerticalClearance)
        );

        validate_params(&descriptor, &json!({})).expect_err("request wrapper is required");
        validate_params(&descriptor, &json!({ "clearance": {} }))
            .expect_err("unwrapped request is refused");
    }

    #[test]
    fn whole_model_spotting_accepts_exact_selected_alignments_and_omitted_all() {
        let descriptor = operation_descriptor("plan_whole_model_spotting").unwrap();
        let selected = json!({ "alignment_ids": ["al-second", "al-first"] });
        validate_params(&descriptor, &selected).expect("native descriptor admits selected IDs");
        let parsed: WholeModelSpottingParams =
            parse("plan_whole_model_spotting", &selected).expect("typed selected IDs");
        assert_eq!(
            parsed.alignment_ids,
            Some(vec![
                AlignmentId::new("al-second").unwrap(),
                AlignmentId::new("al-first").unwrap(),
            ])
        );

        let all = json!({});
        validate_params(&descriptor, &all).expect("omitted IDs remain valid");
        let parsed: WholeModelSpottingParams =
            parse("plan_whole_model_spotting", &all).expect("omitted IDs");
        assert!(parsed.alignment_ids.is_none());
    }

    #[test]
    fn selected_structure_usage_dispatch_preserves_native_basis_and_revision_fence() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let mut snapshot = ds_grid_exchange::unpack(&std::fs::read(path).unwrap())
            .unwrap()
            .snapshot;
        for section in &mut snapshot.tension_sections {
            section.criterion_set_id = None;
        }
        let session = GridSession::open(snapshot);
        let before = session.snapshot().clone();
        let revision = session.current_revision().clone();
        let mut request = ds_grid_engine::SelectedStructureUsageRequest {
            structure_id: session.snapshot().structures[0].id.clone(),
            expected_revision: revision.revision_id.clone(),
        };
        let operation = "screen_selected_structure_usage";
        let descriptor = operation_descriptor(operation).unwrap();
        admit(&descriptor).unwrap();
        let params = json!({ "request": request });
        validate_params(&descriptor, &params).unwrap();
        let native = ds_grid_engine::selected_structure_usage_screening(
            session.snapshot(),
            &revision.revision_id,
            &request,
        )
        .unwrap();
        assert_eq!(
            native.unavailable.as_ref().unwrap().code,
            ds_grid_engine::SelectedStructureUsageUnavailableCode::NoCriterionSet
        );
        assert!(native.row.is_none());
        let report = dispatch(
            operation,
            &params,
            &session,
            &EngineeringAttributeEvidence::default(),
            &StructureLabelPolicy::default(),
        )
        .unwrap();
        assert_eq!(report, serde_json::to_value(&native).unwrap());
        assert_eq!(report["structure_id"], request.structure_id.as_str());
        assert_eq!(report["model_revision"], revision.revision_id.as_str());

        request.expected_revision = ds_grid_engine::RevisionId::from_content_root("stale");
        let native = ds_grid_engine::selected_structure_usage_screening(
            session.snapshot(),
            &revision.revision_id,
            &request,
        )
        .unwrap_err();
        let error = dispatch(
            operation,
            &json!({ "request": request }),
            &session,
            &EngineeringAttributeEvidence::default(),
            &StructureLabelPolicy::default(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "operation_failed");
        assert_eq!(error.detail_value().unwrap()["engine"], native.to_string());
        assert_eq!(session.snapshot(), &before);
        assert_eq!(session.current_revision(), &revision);
    }

    #[test]
    fn profile_index_dispatch_matches_native_selection_and_creation_without_authoring() {
        use ds_grid_engine::profile_geometry_create::{
            ProfileGeometryCreateOperation, ProfileGeometryCreateRequest,
            plan_profile_geometry_create,
        };
        use ds_grid_engine::profile_hit::{ProfilePickIndex, ProfilePickKind};
        use ds_grid_engine::profile_lasso::{
            ProfileLassoMode, ProfileLassoPoint, ProfileSelectionLassoRequest,
        };

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let snapshot = ds_grid_exchange::unpack(&std::fs::read(path).unwrap())
            .unwrap()
            .snapshot;
        let session = GridSession::open(snapshot);
        let before = session.snapshot().clone();
        let revision = session.current_revision().clone();
        let scene = session
            .profile_atlas_scene(ProfileAtlasOptions::default())
            .unwrap();
        let index = ProfilePickIndex::from_scene(&scene);
        let point = |x, y| ProfileLassoPoint {
            scene_x: x,
            scene_y: y,
        };
        let bounds = scene.bounds;
        let mut selection = ProfileSelectionLassoRequest {
            model_revision: revision.revision_id.clone(),
            axis_pin_digest: scene.axis_pin.digest.clone(),
            polygon: vec![
                point(bounds.min_x - 1.0, bounds.min_y - 1.0),
                point(bounds.max_x + 1.0, bounds.min_y - 1.0),
                point(bounds.max_x + 1.0, bounds.max_y + 1.0),
                point(bounds.min_x - 1.0, bounds.max_y + 1.0),
            ],
            mode: ProfileLassoMode::Intersects,
            families: vec![ProfilePickKind::Structure],
            candidate_ids: None,
        };
        let native = index.selection_lasso(&selection).unwrap();
        assert!(native.selected_count > 0);
        let ground = scene
            .bands
            .iter()
            .flat_map(|band| &band.ground)
            .flat_map(|ground| ground.points.windows(2))
            .next()
            .expect("native ground segment");
        let mut creation = ProfileGeometryCreateRequest {
            model_revision: revision.revision_id.clone(),
            axis_pin_digest: scene.axis_pin.digest.clone(),
            scene_x: (ground[0].scene_x + ground[1].scene_x) / 2.0,
            scene_y: (ground[0].scene_y + ground[1].scene_y) / 2.0,
            tolerance_scene_units: 0.001,
            operation: ProfileGeometryCreateOperation::GroundPoint {
                id: ds_grid_model::TerrainPointId::new("tp-proposed").unwrap(),
                feature_class: "GP".to_string(),
            },
        };
        let proposal = plan_profile_geometry_create(
            session.snapshot(),
            &revision.revision_id,
            &index,
            &creation,
        )
        .unwrap();
        assert!(proposal.command.is_some());
        for (operation, params, expected) in [
            (
                "profile_selection_lasso",
                json!({ "request": selection }),
                serde_json::to_value(&native).unwrap(),
            ),
            (
                "profile_geometry_create_plan",
                json!({ "request": creation }),
                serde_json::to_value(&proposal).unwrap(),
            ),
        ] {
            let descriptor = operation_descriptor(operation).unwrap();
            admit(&descriptor).unwrap();
            validate_params(&descriptor, &params).unwrap();
            let report = dispatch(
                operation,
                &params,
                &session,
                &EngineeringAttributeEvidence::default(),
                &StructureLabelPolicy::default(),
            )
            .unwrap();
            assert_eq!(report, expected);
            assert_eq!(report["model_revision"], revision.revision_id.as_str());
            assert_eq!(report["axis_pin_digest"], scene.axis_pin.digest);
        }
        selection.axis_pin_digest = "stale-axis".to_string();
        creation.axis_pin_digest = "stale-axis".to_string();
        for (operation, params, native) in [
            (
                "profile_selection_lasso",
                json!({ "request": selection }),
                index.selection_lasso(&selection).unwrap_err().to_string(),
            ),
            (
                "profile_geometry_create_plan",
                json!({ "request": creation }),
                plan_profile_geometry_create(
                    session.snapshot(),
                    &revision.revision_id,
                    &index,
                    &creation,
                )
                .unwrap_err()
                .to_string(),
            ),
        ] {
            let error = dispatch(
                operation,
                &params,
                &session,
                &EngineeringAttributeEvidence::default(),
                &StructureLabelPolicy::default(),
            )
            .unwrap_err();
            assert_eq!(error.code(), "operation_failed");
            assert_eq!(error.detail_value().unwrap()["engine"], native);
        }
        assert_eq!(session.snapshot(), &before);
        assert_eq!(session.current_revision(), &revision);
    }

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
    fn spotting_interval_fallback_is_admitted_and_parsed_by_dsgrid_run() {
        let descriptor = operation_descriptor("plan_optimum_spotting").unwrap();
        let params = json!({
            "alignment_id": "al-line",
            "stringing_basis_section_id": "ts-basis",
            "criterion_set_id": "cs-1",
            "analysis_case_ids": ["ac-1"],
            "catalog_resource_id": "res-catalog",
            "attachment_set_label": "conductor",
            "pass": "preliminary_awaiting_structural_analysis",
            "design_policy_id": "policy-spotting",
            "station_step_m": 7.0,
            "max_candidate_stations": 100,
            "max_search_states": 10000,
            "fixed_structures": ["str-before", "str-after"],
            "bounded_interval_fallback": {
                "from_fixed_structure_id": "str-before",
                "to_fixed_structure_id": "str-after",
                "maximum_span_m": 180.0
            }
        });
        validate_params(&descriptor, &params).expect("published descriptor admits opt-in");
        let parsed: SpottingPlanRequest = parse("plan_optimum_spotting", &params).unwrap();
        let fallback = parsed.bounded_interval_fallback.unwrap();
        assert_eq!(fallback.from_fixed_structure_id.as_str(), "str-before");
        assert_eq!(fallback.to_fixed_structure_id.as_str(), "str-after");
        assert_eq!(fallback.maximum_span_m, 180.0);

        let mut ordinary = params;
        ordinary
            .as_object_mut()
            .unwrap()
            .remove("bounded_interval_fallback");
        validate_params(&descriptor, &ordinary).unwrap();
        let parsed: SpottingPlanRequest = parse("plan_optimum_spotting", &ordinary).unwrap();
        assert!(parsed.bounded_interval_fallback.is_none());
    }

    #[test]
    fn native_profile_properties_uses_the_engine_sheet() {
        let mut snapshot = ds_grid_model::GridModelSnapshot::default();
        snapshot
            .terrain_points
            .push(ds_grid_model::TerrainPointRow {
                id: ds_grid_model::TerrainPointId::new("tp-1").unwrap(),
                x_m: 1.0,
                y_m: 2.0,
                z_m: 103.5,
                feature_class: "GP".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        let session = GridSession::open(snapshot);
        let sheet = dispatch(
            "profile_properties",
            &json!({ "entity_id": "tp-1" }),
            &session,
            &EngineeringAttributeEvidence::default(),
            &StructureLabelPolicy::default(),
        )
        .expect("native Profile sheet");
        assert_eq!(sheet["kind"], "terrain_point");
        assert_eq!(sheet["edit_layer"], "terrain");
        let elevation = sheet["flat_fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["id"] == "z_m")
            .expect("native elevation field");
        assert_eq!(elevation["value"], 103.5);
        assert_eq!(
            elevation["editor"]["command_kind"],
            "edit_profile_properties"
        );
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

    #[test]
    fn a_digest_sealed_plan_crosses_the_limit_whole() {
        // The whole-model receipt shape that lost 27 plans to --limit 10000:
        // cutting rejected.rows left plan_digest unverifiable.
        let plan = json!({
            "plan_digest": format!("sha256:{}", "a".repeat(64)),
            "commands": [1, 2, 3],
            "rejected": { "truncated": false, "rows": [1, 2, 3] },
            "provisional_intervals": [{ "blocked_by": [1, 2, 3] }],
        });
        let value = json!({
            "batch": { "items": [{ "plan": plan.clone() }] },
            "refused_derivations": [1, 2, 3],
        });
        let (bounded, truncated) = bound_result(value, 2);
        assert_eq!(bounded["batch"]["items"][0]["plan"], plan);
        assert_eq!(bounded["refused_derivations"], json!([1, 2]));
        let fields: Vec<&str> = truncated
            .iter()
            .map(|receipt| receipt["field"].as_str().unwrap())
            .collect();
        assert_eq!(fields, ["result.refused_derivations"]);

        // An unsealed (infeasible, empty-digest) plan is ordinary output.
        let (bounded, truncated) = bound_result(json!({ "plan_digest": "", "rows": [1, 2, 3] }), 2);
        assert_eq!(bounded["rows"], json!([1, 2]));
        assert_eq!(truncated.len(), 1);
    }

    #[test]
    fn spotting_refusals_keep_a_machine_readable_reason() {
        let failure = spotting_error(
            "plan_optimum_spotting",
            SpottingPlanError::ClearanceAuthorityMissing {
                section: "section-17".to_string(),
            },
        );
        let detail = failure.detail_value().expect("structured detail");
        assert_eq!(detail["refusal"]["code"], "clearance_authority_missing");
        assert_eq!(detail["refusal"]["detail"]["section"], "section-17");
        assert_eq!(
            detail["engine"],
            "section section-17 has no authored minimum-clearance rule; spotting has no clearance authority and will not assume one"
        );

        let failure = spotting_error(
            "plan_optimum_spotting",
            SpottingPlanError::NoEligibleCandidate {
                catalog: "catalog-4".to_string(),
                rejected: vec![ds_grid_engine::SpottingCandidateRejection {
                    catalog_sequence: 3,
                    structure_resource_id: ds_grid_model::ResourceId::new("resource-3")
                        .expect("test id is valid"),
                    ineligibility:
                        ds_grid_engine::SpottingCandidateIneligibility::NotFlaggedForAutomaticSpotting,
                }],
            },
        );
        let detail = failure.detail_value().expect("structured detail");
        assert_eq!(detail["refusal"]["code"], "no_eligible_candidate");
        assert_eq!(detail["refusal"]["detail"]["catalog"], "catalog-4");
        assert_eq!(
            detail["refusal"]["detail"]["rejected"][0]["reason"],
            "not_flagged_for_automatic_spotting"
        );
    }
}
