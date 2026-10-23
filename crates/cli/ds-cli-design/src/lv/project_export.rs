//! `ds design lv project-export` — one governed snapshot to one native request.

use std::path::PathBuf;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::lv_processing::{
    NativeLvError, encode_native_lv_request_from_layers, encode_native_lv_request_with_config,
};
use serde_json::{Value, json};

use super::artifact::{PROJECT_REQUEST, ensure_absent, sha256, write_new};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "One exact transformer in the project named by --project.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Absent path for one ds.lv.process.request/v1 document.",
)
.required();
const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");
pub(super) const PRESET: Arg = Arg::value(
    "preset",
    "<preset>",
    "Carry this preset's settings as the LV process dialog resolves them.",
)
.choices(&["drafting", "sketch"]);
pub(super) const OPERATOR: Arg = Arg::value(
    "operator",
    "<json-file>",
    "The operator's own toggles ({key: bool|number}) laid over --preset.",
);
pub(super) const PROCESS_SETTINGS_INVALID: Refusal = Refusal {
    code: "process_settings_invalid",
    when: "--operator is unreadable, lacks --preset, or holds a non-boolean, non-numeric value",
    remedy: "pass --preset, and settings as {key: bool|number}",
};

macro_rules! refusal {
    ($code:literal, $when:literal, $remedy:literal) => {
        Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        }
    };
}

pub static COMMAND: Command = Command {
    id: "design.lv.project-export",
    path: &["design", "lv", "project-export"],
    contract: 1,
    summary: "Prepare one project transformer for headless LV processing.",
    purpose: "Prepare a saved transformer for headless reprocessing. Read one exact signed-in project transformer with server version/digest fences. Use --project-config to include current project sheets, including pole material seeds, under the same identity; otherwise config is empty, such a request fails every job in design.lv.process, and the result carries a project_config_not_included warning. Process settings are owner defaults unless --preset (with optional --operator toggles) carries what the LV process dialog would send. design.lv.process accepts bounded parallel batches; design.lv.project-save publishes their verified results.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER,
        OUT,
        crate::PROJECT_ARG,
        LANE,
        Arg::switch(
            "project-config",
            "Include freshly read project network configuration with its SHA-256.",
        ),
        PRESET,
        OPERATOR,
    ],
    output: "Output path, request SHA-256/bytes, lane/project/transformer, server version/digest, layer/job counts, process_settings (`ds-network-owner-defaults`, or {preset, operator, wire_settings}), and configuration inclusion/digest. project.printing_label is the canonical caption from the same sealed configuration receipt, or null when unavailable; project.printing_receipt references its bounded create-new local artifact beside --out. Raw process sheets remain unchanged; raw captions are not substitutes. No layer payload is printed.",
    examples: &[Example {
        command: "ds design lv project-export --project <id> --transformer T-1042 --out ./T-1042.fast-lv.json --output json",
        note: "Create a fenced one-transformer request for later native processing.",
        runnable: false,
    }],
    refusals: &[
        refusal!(
            "transformer_process_projection_unavailable",
            "the backend does not return saved layers with their processing projection",
            "deploy matching ds-brain projection support, then retry"
        ),
        refusal!(
            "native_profile_not_configured",
            "the exact packaged native profile is unavailable",
            "install one complete ds release"
        ),
        refusal!(
            "native_profile_digest_mismatch",
            "the packaged catalog differs from the build pin",
            "reinstall one complete ds release"
        ),
        refusal!(
            "native_profile_unsafe",
            "the packaged native catalog is unsafe or malformed",
            "reinstall one complete ds release"
        ),
        ds_cli_auth::SIGNED_OUT_REFUSAL,
        refusal!(
            "context_corrupt",
            "--project is not one exact project id (blank, untrimmed, too long or a path)",
            "copy one exact ds_project value from ds auth project list"
        ),
        refusal!(
            "project_context_stale",
            "the account or project changed between the two reads",
            "export the transformer again"
        ),
        refusal!(
            "native_state_unsafe",
            "protected native state is unsafe or unreadable",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_unavailable",
            "protected native state cannot be accessed",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_protection_unavailable",
            "this build has no protected-state adapter",
            "install a supported native ds build"
        ),
        refusal!(
            "native_state_root_invalid",
            "the configured state root is not absolute",
            "unset it or provide an absolute path"
        ),
        refusal!(
            "native_state_conflict",
            "another native operation holds the state lease",
            "retry after that operation finishes"
        ),
        refusal!(
            "native_cleanup_required",
            "revoked identity cleanup could not clear context",
            "repair protected state and run auth logout"
        ),
        refusal!(
            "auth_input_invalid",
            "the transformer or bound identity input is invalid",
            "pass one exact bounded transformer name"
        ),
        // Two different rejections mint this one code. The Firebase refresh
        // that runs before the call can be rejected, and then the credential
        // is not verified at all; the transformer-context route can also
        // reject one that this same call just verified. A declaration is read
        // without knowing which happened, so it must hold for both. The
        // route-only case is answered at runtime with
        // `ds_cli_auth::TRANSFORMER_CONTEXT_ROUTE_REMEDY`, which is more
        // specific than this and wins wherever it applies.
        refusal!(
            "auth_rejected",
            "the authentication service, or the transformer-context route, rejected this request",
            "confirm `ds auth status` authenticates this lane; if it does, the transformer-context route rejected a verified credential and no local change fixes it"
        ),
        refusal!(
            "auth_revoked",
            "Firebase permanently revoked the session",
            "sign in again interactively"
        ),
        refusal!(
            "auth_identity_mismatch",
            "Firebase returned another identity",
            "sign in again and report a repeated mismatch"
        ),
        refusal!(
            "auth_transient",
            "the fixed native service is temporarily unavailable",
            "retry without changing local state"
        ),
        refusal!(
            "auth_response_unreadable",
            "the transformer reply violates its closed contract",
            "retry once, then update ds if it persists"
        ),
        refusal!(
            "project_not_visible",
            "--project is not an exact id in the signed-in user's project list",
            "choose an exact id from auth project list"
        ),
        refusal!(
            "transformer_not_found",
            "the transformer does not exist in the named project",
            "pass one exact transformer name from that project"
        ),
        refusal!(
            "transformer_context_unfenced",
            "the server omitted either the snapshot version or content digest",
            "refresh or migrate the transformer until the context returns both fences"
        ),
        refusal!(
            "fast_lv_input_too_large",
            "the generated request exceeds 64 MiB",
            "use an owner-supported narrower project snapshot"
        ),
        refusal!(
            "fast_lv_input_invalid",
            "the governed layers cannot form the closed native request",
            "update ds and report the authoritative transformer context"
        ),
        refusal!(
            "fast_lv_bound_refused",
            "the transformer context exceeds a native name, layer, or feature bound",
            "narrow or repair the authoritative transformer context"
        ),
        refusal!(
            "fast_lv_request_output_exists",
            "--out already exists",
            "choose a new request path; this command never overwrites"
        ),
        refusal!(
            "fast_lv_request_output_write_failed",
            "the request cannot be durably written at --out",
            "choose a writable absent path and retry the export"
        ),
        PROCESS_SETTINGS_INVALID,
        super::catalogue::PRESET_NOT_OFFERED,
        super::catalogue::SETTING_NOT_OFFERED,
        super::catalogue::CATALOGUE_INVALID,
        refusal!(
            "network_engine_missing",
            "--preset needs the lane's Network engine catalogue and no Network engine is installed for the lane",
            "install the matching Network engine addon"
        ),
        refusal!(
            "network_engine_incompatible",
            "--preset needs the lane's Network engine catalogue and that engine publishes none (lv_processor_catalog_v1)",
            "install a Network engine that offers its LV processor catalogue"
        ),
    ],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let output_path = PathBuf::from(inputs.require("out")?);
    // Do not rotate credentials or call the project service when this local
    // precondition already makes the requested effect impossible.
    ensure_absent(&output_path, &PROJECT_REQUEST)?;
    let preset = inputs.value("preset");
    let operator = inputs.value("operator").map(read_operator).transpose()?;
    if preset.is_none() && operator.is_some() {
        return Err(process_settings_invalid("--operator needs --preset"));
    }

    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let headless =
        ds_cli_auth::processing_transformer_context_for_project(lane, project, transformer)
            .map_err(|failure| name_missing_project(lane, project, failure))?;
    let snapshot = headless.snapshot();
    let (Some(version), Some(content_digest)) = (
        snapshot.metadata().version(),
        snapshot.metadata().content_digest(),
    ) else {
        return Err(Failure::conflict(
            "transformer_context_unfenced",
            "The transformer context has no complete server version/content-digest fence.",
        )
        .remedy("Refresh or migrate the transformer until the context call returns both fences."));
    };

    // One response binds saved bytes and the separately governed transformer
    // projection to this principal, project and generation. No second read.
    let projected = snapshot.processing_layers().ok_or_else(|| Failure::unavailable(
        "transformer_process_projection_unavailable",
        "The backend did not return the coherent transformer processing projection.",
    ).remedy("Deploy matching ds-brain process projection support, then retry without changing local state."))?;
    let mut process_layers = snapshot.layers().clone();
    carry_projected_nature(&mut process_layers, projected)?;

    let mut config_sha256 = None;
    let mut printing_label = None;
    let mut printing_receipt = None;
    let mut sheets_for_preset = None;
    let request = if inputs.switch("project-config") {
        let config = ds_cli_auth::settings_configuration_receipt(
            lane,
            project,
            ds_client_core::ProjectConfigurationChange::ReadSettings,
        )?;
        if config.identity() != headless.identity() || config.project_id() != snapshot.ds_project()
        {
            return Err(Failure::conflict(
                "project_context_stale",
                "The account or project changed while preparing configuration.",
            )
            .remedy("Export the transformer again."));
        }
        let sheets = config.result().document["sheets"]
            .as_object()
            .ok_or_else(|| {
                Failure::failed(
                    "auth_response_unreadable",
                    "Project configuration has no sheets.",
                )
                .remedy("Read project configuration and report the invalid response.")
            })?;
        let sheets: std::collections::BTreeMap<String, Value> = sheets
            .iter().map(|(key,value)|(key.clone(),value.clone())).collect();
        config_sha256 = Some(sha256(&serde_json::to_vec(&sheets).map_err(|error| {
            Failure::internal("fast_lv_input_invalid", error.to_string())
        })?));
        // The save owner requires these raw sheets unchanged. Retain the
        // existing server seal separately; it is not process configuration.
        if let Some(receipt) = captured_printing_receipt(&config.result().document, project) {
            let context = receipt.printing_context_for_project(project).map_err(|e| Failure::internal("fast_lv_input_invalid", e))?;
            let bytes = serde_json::to_vec(&receipt).map_err(|e| Failure::internal("fast_lv_input_invalid",e.to_string()))?;
            if bytes.len() <= ds_network_contracts::lv_processing::MAX_NATIVE_LV_INPUT_BYTES {
                let path = super::artifact::printing_receipt_path(&output_path);
                write_new(&path,&bytes,&PROJECT_REQUEST)?;
                printing_label = context["project"].as_str().map(str::to_owned);
                printing_receipt = Some(json!({"path":path,"sha256":sha256(&bytes),"bytes":bytes.len(),"sheets_sha256":receipt.sheets_sha256}));
            }
        }
        let encoded = encode_native_lv_request_with_config(
            snapshot.transformer_name(),
            &process_layers,
            &sheets,
        );
        sheets_for_preset = Some(sheets);
        encoded
    } else {
        encode_native_lv_request_from_layers(snapshot.transformer_name(), &process_layers)
    }
    .map_err(map_owner_error)?;
    let (request, process_settings) = match preset {
        Some(preset) => {
            let (request, wire) = apply_process_preset(
                &request,
                preset,
                operator.as_ref(),
                sheets_for_preset.as_ref(),
                Some(lane),
            )?;
            let settings = json!({"preset": preset, "operator": operator, "wire_settings": wire});
            (request, settings)
        }
        None => (request, json!("ds-network-owner-defaults")),
    };
    let request_sha256 = sha256(&request);
    write_new(&output_path, &request, &PROJECT_REQUEST)?;

    // Without the project sheets the request holds an empty configuration, which
    // the engine refuses job by job (outdated network_config template).
    let warnings: Vec<Value> = if config_sha256.is_some() {
        Vec::new()
    } else {
        vec![json!({
            "code": "project_config_not_included",
            "message": "This request carries no project configuration; design lv process and design lv voltage-drop will fail every job with it.",
            "remedy": "export again with --project-config",
        })]
    };
    Ok(json!({
        "out": output_path,
        "request_sha256": request_sha256,
        "byte_count": request.len(),
        "lane": lane,
        "project": { "ds_project": snapshot.ds_project(), "printing_label": printing_label, "printing_receipt": printing_receipt },
        "transformer": snapshot.transformer_name(),
        "source": {
            "state": "fenced",
            "version": version,
            "content_digest": content_digest,
        },
        "jobs": 1,
        "layers": snapshot.layers().len(),
        "process_settings": process_settings,
        "project_config": if config_sha256.is_some() { "included" } else { "not-included" },
        "project_config_sha256": config_sha256,
        "warning_count": warnings.len(),
        "warnings": warnings,
    }))
}

/// The shared 404 mapping calls every missing project route a missing
/// transformer. When the project is not in the signed-in directory the project
/// is the missing thing: say that first, with the code the CLI already uses.
fn name_missing_project(lane: &str, project: &str, failure: Failure) -> Failure {
    if failure.code() != "transformer_not_found" {
        return failure;
    }
    let Ok(directory) = ds_cli_auth::project_directory(lane) else {
        return failure;
    };
    let visible = directory
        .projects()
        .iter()
        .any(|row| row["ds_project"].as_str() == Some(project));
    if visible {
        return failure;
    }
    Failure::invalid(
        "project_not_visible",
        format!("project {project} is not visible to the signed-in user on this lane"),
    )
    .remedy("choose an exact id from auth project list")
    .next("ds auth project list --output json")
}

fn captured_printing_receipt(
    configuration: &Value,
    project: &str,
) -> Option<ds_command_kernel::report_export::InputReceipt> {
    let receipt =
        ds_command_kernel::report_export::InputReceipt::from_config(configuration).ok()?;
    receipt.printing_context_for_project(project).ok()?;
    Some(receipt)
}

fn carry_projected_nature(
    saved: &mut std::collections::BTreeMap<String, Value>,
    projected: &std::collections::BTreeMap<String, Value>,
) -> Result<(), Failure> {
    let saved_tr = saved
        .get_mut("tr")
        .and_then(|layer| layer.get_mut("features"))
        .and_then(Value::as_array_mut)
        .and_then(|features| features.first_mut());
    let projected_tr = projected
        .get("tr")
        .and_then(|layer| layer.get("features"))
        .and_then(Value::as_array)
        .and_then(|features| features.first());
    let (Some(saved_tr), Some(projected_tr)) = (saved_tr, projected_tr) else {
        return Err(Failure::conflict(
            "transformer_context_unfenced",
            "The transformer point is absent from a saved or governed projection.",
        ));
    };
    if saved_tr.get("id") != projected_tr.get("id")
        || saved_tr.get("geometry") != projected_tr.get("geometry")
    {
        return Err(Failure::conflict(
            "project_context_stale",
            "The saved and governed transformer points differ.",
        ));
    }
    if let Some(nature) = projected_tr
        .get("properties")
        .and_then(|properties| properties.get("transformer_nature"))
    {
        saved_tr["properties"]["transformer_nature"] = nature.clone();
    } else if let Some(properties) = saved_tr
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        properties.remove("transformer_nature");
    }
    Ok(())
}

fn map_owner_error(error: NativeLvError) -> Failure {
    let code = error.code();
    match code {
        "fast_lv_input_too_large" => Failure::invalid(code, error.to_string())
            .remedy("Use an owner-supported narrower project snapshot."),
        "fast_lv_input_invalid" => Failure::failed(code, error.to_string())
            .remedy("Update ds and report the authoritative transformer context."),
        "fast_lv_bound_refused" => Failure::invalid(code, error.to_string())
            .remedy("Narrow or repair the authoritative transformer context."),
        _ => Failure::internal("fast_lv_input_invalid", error.to_string()),
    }
}

fn process_settings_invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(PROCESS_SETTINGS_INVALID.code, message.to_string())
        .remedy(PROCESS_SETTINGS_INVALID.remedy)
}

/// The operator's toggles: one bounded JSON object of setting values.
fn read_operator(path: &str) -> Result<Value, Failure> {
    const MAX_OPERATOR_BYTES: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            use std::io::Read;
            file.take(MAX_OPERATOR_BYTES + 1).read_to_end(&mut bytes)
        })
        .map_err(process_settings_invalid)?;
    if bytes.len() as u64 > MAX_OPERATOR_BYTES {
        return Err(process_settings_invalid("--operator exceeds 64 KiB"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(process_settings_invalid)?;
    if !value.is_object() {
        return Err(process_settings_invalid(
            "--operator must be one {key: bool|number} object",
        ));
    }
    Ok(value)
}

/// Carry the settings the LV process dialog sends for `preset` into every
/// job: the kernel resolves them once over the catalogue the engine of `lane`
/// (the packaged engine without one) answers and the project's preset rows;
/// a preset or setting that engine does not offer is refused here.
pub(crate) fn apply_process_preset(
    request: &[u8],
    preset: &str,
    operator: Option<&Value>,
    sheets: Option<&std::collections::BTreeMap<String, Value>>,
    lane: Option<&str>,
) -> Result<(Vec<u8>, Value), Failure> {
    let (catalog, offer) = super::catalogue::engine_catalogue(lane)?;
    super::catalogue::admit_preset(&offer, preset)?;
    let mut resolve = json!({
        "op": "resolve",
        "catalog": catalog,
        "preset": preset,
        "use_firestore_design_data": false,
    });
    if let Some(sheets) = sheets {
        resolve["project_config"] = json!({ "sheets": sheets });
    }
    if let Some(operator) = operator {
        resolve["operator"] = operator.clone();
    }
    let resolve = serde_json::to_vec(&resolve)
        .map_err(|error| Failure::internal("fast_lv_input_invalid", error.to_string()))?;
    let reply: Value = ds_command_kernel::process_settings::evaluate(&resolve)
        .and_then(|reply| serde_json::from_str(&reply).map_err(|error| error.to_string()))
        .map_err(process_settings_invalid)?;
    let wire = reply["wire_settings"].clone();
    if !wire.is_object() {
        return Err(process_settings_invalid(
            "the preset resolved to no wire settings",
        ));
    }
    let mut document: Value = serde_json::from_slice(request)
        .map_err(|error| Failure::internal("fast_lv_input_invalid", error.to_string()))?;
    for job in document["jobs"].as_array_mut().into_iter().flatten() {
        job["settings"] = wire.clone();
    }
    let bytes = serde_json::to_vec(&document)
        .map_err(|error| Failure::internal("fast_lv_input_invalid", error.to_string()))?;
    let admitted = ds_network_contracts::lv_processing::decode_native_lv_request(&bytes)
        .map_err(map_owner_error)?;
    offer.admit_request(&admitted).map_err(|error| {
        Failure::invalid(error.code(), error.to_string())
            .remedy(super::catalogue::SETTING_NOT_OFFERED.remedy)
    })?;
    Ok((bytes, wire))
}

pub fn render(value: &Value) -> String {
    let settings = match value["process_settings"]["preset"].as_str() {
        Some(preset) => format!("{preset} preset"),
        None => "owner defaults".to_string(),
    };
    let mut text = format!(
        "LV request exported for {} ({} layer(s), {settings}).\nRequest: {}\nSHA-256: {}",
        value["transformer"].as_str().unwrap_or(""),
        value["layers"].as_u64().unwrap_or(0),
        value["out"].as_str().unwrap_or(""),
        value["request_sha256"].as_str().unwrap_or(""),
    );
    for warning in value["warnings"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "\nWarning: {} Remedy: {}.",
            warning["message"].as_str().unwrap_or(""),
            warning["remedy"].as_str().unwrap_or(""),
        ));
    }
    text
}

#[cfg(test)]
#[path = "project_export_tests.rs"]
mod tests;
