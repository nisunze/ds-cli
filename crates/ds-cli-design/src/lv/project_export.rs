//! `ds design lv project-export` — one governed snapshot to one native request.

use std::path::PathBuf;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::lv_processing::{
    NativeLvError, encode_native_lv_request_from_layers,
    encode_native_lv_request_with_config,
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
    purpose: "Prepare a saved transformer for headless reprocessing. Read one exact signed-in project transformer with server version/digest fences. Use --project-config to include current project sheets, including pole material seeds, under the same identity; otherwise config is empty. Process settings remain owner defaults, so select intended settings before processing. design.lv.process accepts bounded parallel batches; design.lv.project-save publishes their verified results.",
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
    ],
    output: "Output path, request SHA-256/bytes, lane/project/transformer, server version/digest, layer/job counts, owner-default settings, and configuration inclusion/digest. project.printing_label is the canonical caption from the same sealed configuration receipt, or null when unavailable; project.printing_receipt references its bounded create-new local artifact beside --out. Raw process sheets remain unchanged; raw captions are not substitutes. No layer payload is printed.",
    examples: &[Example {
        command: "ds design lv project-export --project <id> --transformer T-1042 --out ./T-1042.fast-lv.json --output json",
        note: "Create a fenced one-transformer request for later native processing.",
        runnable: false,
    }],
    refusals: &[
        refusal!(
            "transformer_process_projection_unavailable",
            "the backend does not return exact saved layers and their same-read processing projection",
            "deploy matching ds-brain process projection support, then retry without changing local state"
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
            "--project is not one exact DS project id: blank, untrimmed, too long, or a path",
            "copy one exact ds_project value from ds auth project list"
        ),
        refusal!(
            "project_context_stale",
            "the account or project changed between the transformer and configuration reads",
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
            "confirm this lane still authenticates with `ds auth status`; if it does, the transformer-context route rejected a credential this call already verified, and no local change fixes that"
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
            "transformer_not_found",
            "the transformer does not exist in the named project",
            "pass one exact transformer name from that project"
        ),
        refusal!(
            "transformer_context_unfenced",
            "the server omitted either the snapshot version or content digest",
            "refresh or migrate the transformer until the context call returns both fences"
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
            "choose a writable absent path and retry the unchanged transformer export"
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

    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let headless =
        ds_cli_auth::processing_transformer_context_for_project(lane, project, transformer)?;
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
                let path = output_path.with_extension("printing-receipt.json");
                write_new(&path,&bytes,&PROJECT_REQUEST)?;
                printing_label = context["project"].as_str().map(str::to_owned);
                printing_receipt = Some(json!({"path":path,"sha256":sha256(&bytes),"bytes":bytes.len(),"sheets_sha256":receipt.sheets_sha256}));
            }
        }
        encode_native_lv_request_with_config(
            snapshot.transformer_name(),
            &process_layers,
            &sheets,
        )
    } else {
        encode_native_lv_request_from_layers(snapshot.transformer_name(), &process_layers)
    }
    .map_err(map_owner_error)?;
    let request_sha256 = sha256(&request);
    write_new(&output_path, &request, &PROJECT_REQUEST)?;

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
        "process_settings": "ds-network-owner-defaults",
        "project_config": if config_sha256.is_some() { "included" } else { "not-included" },
        "project_config_sha256": config_sha256,
    }))
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

pub fn render(value: &Value) -> String {
    format!(
        "LV request exported for {} ({} layer(s), owner defaults).\nRequest: {}\nSHA-256: {}",
        value["transformer"].as_str().unwrap_or(""),
        value["layers"].as_u64().unwrap_or(0),
        value["out"].as_str().unwrap_or(""),
        value["request_sha256"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn layers() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "tr".to_owned(),
                json!({ "type": "FeatureCollection", "features": [] }),
            ),
            (
                "customers".to_owned(),
                json!({ "type": "FeatureCollection", "features": [] }),
            ),
        ])
    }

    #[test]
    fn nature_is_carried_only_from_the_matching_governed_point() {
        let mut saved = layers();
        saved.get_mut("tr").unwrap()["features"] = json!([{
            "type":"Feature", "id":"tr-1", "properties":{}
        }]);
        let mut projected = saved.clone();
        projected.get_mut("tr").unwrap()["features"][0]["properties"]["transformer_nature"] =
            json!("fill_in");
        carry_projected_nature(&mut saved, &projected).unwrap();
        assert_eq!(
            saved["tr"]["features"][0]["properties"]["transformer_nature"],
            "fill_in"
        );
        projected.get_mut("tr").unwrap()["features"][0]["properties"]["transformer_nature"] =
            json!(["fill_in", "productive_use"]);
        carry_projected_nature(&mut saved, &projected).unwrap();
        assert_eq!(
            saved["tr"]["features"][0]["properties"]["transformer_nature"],
            json!(["fill_in", "productive_use"])
        );
        projected.get_mut("tr").unwrap()["features"][0]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("transformer_nature");
        carry_projected_nature(&mut saved, &projected).unwrap();
        assert!(
            saved["tr"]["features"][0]["properties"]
                .get("transformer_nature")
                .is_none()
        );
        projected.get_mut("tr").unwrap()["features"][0]["geometry"] =
            json!({"type":"Point", "coordinates":[1,2]});
        assert!(carry_projected_nature(&mut saved, &projected).is_err());
        projected.get_mut("tr").unwrap()["features"][0]
            .as_object_mut()
            .unwrap()
            .remove("geometry");
        projected.get_mut("tr").unwrap()["features"][0]["id"] = json!("another-tr");
        assert!(carry_projected_nature(&mut saved, &projected).is_err());
    }

    #[test]
    fn contract_is_one_fenced_project_read_and_local_file_write() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["design", "lv", "project-export"]);
        assert!(COMMAND.args.iter().any(|arg| arg.name == "project"));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.name != "desktop-descriptor")
        );
    }

    #[test]
    fn owner_builds_the_request_and_the_cli_does_not_shape_engine_fields() {
        let bytes = encode_native_lv_request_from_layers("T-1", &layers()).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["schema"], "ds.lv.process.request/v1");
        assert_eq!(request["jobs"][0]["config_dfs"], json!({}));
        assert_eq!(request["jobs"][0]["settings"]["delete_strange_cols"], true);
        assert!(request["jobs"][0].get("ds_project").is_none());
    }

    #[test]
    fn request_artifacts_are_create_new_and_renderer_is_bounded() {
        let root = std::env::temp_dir().join(format!(
            "ds-project-export-{}-{}",
            std::process::id(),
            sha256(b"project-export-test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("request.json");
        write_new(&path, b"{}", &PROJECT_REQUEST).unwrap();
        assert_eq!(
            ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
            "fast_lv_request_output_exists"
        );

        let text = render(&json!({
            "transformer": "T-1",
            "layers": 2,
            "out": path,
            "request_sha256": "abc",
            "secret_layers": { "customers": [1, 2, 3] },
        }));
        assert!(text.contains("owner defaults"));
        assert!(!text.contains("customers"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_output_symlink_is_existing_operator_state() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "ds-project-export-symlink-{}-{}",
            std::process::id(),
            sha256(b"project-export-symlink-test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("request.json");
        symlink(root.join("absent-target"), &path).unwrap();

        assert_eq!(
            ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
            "fast_lv_request_output_exists"
        );
        assert!(std::fs::symlink_metadata(&path).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn canonical_receipt_keeps_project_save_raw_configuration_equivalent() {
        let raw =
            json!({"printing_context":{"project_id":"project","project":"Raw sheet caption"}});
        let sealed =
            json!({"printing_context":{"project_id":"project","project":"Gisagara, CJIC"}})
                .to_string();
        let configuration = json!({"sheets":raw,"network_reporter_input_receipt":{"schema":1,"country":"Rwanda","sheets_json":sealed,"sheets_sha256":sha256(sealed.as_bytes()),"reference_semantic_sha256":"a".repeat(64)}});
        let receipt = captured_printing_receipt(&configuration, "project").unwrap();
        let sheets: BTreeMap<String, Value> =
            serde_json::from_value(configuration["sheets"].clone()).unwrap();
        let encoded = encode_native_lv_request_with_config("T-1", &layers(), &sheets).unwrap();
        let request: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(request["jobs"][0]["config_dfs"], configuration["sheets"]);
        for (key, expected) in sheets {
            assert_eq!(
                configuration["sheets"][key], expected,
                "fresh native project-save equality"
            );
        }
        assert_eq!(
            receipt.printing_context_for_project("project").unwrap()["project"],
            "Gisagara, CJIC"
        );
        assert_eq!(configuration["sheets"], raw);
        for config in [json!({}), json!({"network_reporter_input_receipt":{}}), {
            let mut c = configuration.clone();
            c["network_reporter_input_receipt"]["sheets_json"] = json!("{}");
            c
        }] {
            assert!(captured_printing_receipt(&config, "project").is_none());
        }
        assert!(captured_printing_receipt(&configuration, "other").is_none());
    }
}
