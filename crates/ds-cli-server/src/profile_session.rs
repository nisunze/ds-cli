//! Closed CLI and protected-socket adapter for the retained native Profile owner.
use crate::{
    host::{App, ProjectQuery},
    profile_sessions::{MAX_REQUEST, Operation, RECEIPT_SCHEMA, Request, Scope},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use serde_json::Value;
use std::io::Read;
const MAX_RESPONSE: usize = 72 * 1024 * 1024;
const FILE_SCHEMA: &str = "ds.core.profile-artifact/v1";

pub static COMMAND: Command = Command {
    id: "core.profile-session", path: &["core", "profile-session"], contract: 1,
    summary: "Open, read or close a retained native Grid Profile session.",
    purpose: "Keep one exact verified .dsgrid package's live Profile with the authenticated Core owner and its independently installed managed Network process. The closed request admits open, snapshot, state, view, scene, feature_codes, engineering_attributes, gis_projection, properties, pick, selection_range, checkpoint or close only; handles are fenced to the current account/lane/deployment/install/project/model/history. Workers retire opportunistically after fifteen idle minutes; at most eight exact readonly initial package sources remain bounded in the same protected Core SQLite Store. An intentional request may rebind only the exact unchanged initial model/history/CRS/managed engine; missing, changed, invalidated or corrupt sources refuse. Close removes the exact known source without reopening an expired worker. Feature codes, strict structures/sections engineering attributes and GIS presentation are typed reads of the same retained managed worker, requiring its captured profile_read_projections_v1 capability before staging or request. GIS forwards only retained manifest CRS and bounded ordered StructureLabelField values to that worker; Core never opens a second Grid session to compute this read. Properties additionally requires captured profile_properties_v1 before staging or request and takes one canonical entity_id only (1..128 UTF-8 bytes, no controls or edge whitespace). It returns the existing complete ModelPropertySheet from the same retained worker/assets, fenced to exact entity/revision/schema; editable-field metadata grants no edit command. Readonly pick/range require captured profile_selection_v1 and the exact already-admitted native scene axis; these reads share the WASM geometry/ranking/range owner, never calculate or create another Grid session. Old capability refusals have no browser or in-process fallback. Scene executes only the existing native ProfileRuntimeScene protocol with exact history; cached mode never requests engineering calculation. View requires captured profile_view_v1 before style acquisition or staging and receives the complete scene/display projection from the same managed worker; Core retains authorization and display-state adoption without a linked Profile runtime. View style_document null alone demands the same project's fixed authored ds_grid/profile document through the existing authenticated Style Center owner; at most 64 KiB complete style metadata is retained only in the existing live session after unchanged physical-pen admission and late native-owner fences. A present malformed document is never replaced. Repeated same-owner Views reuse that captured document without catalogue fetch. No browser engine, arbitrary RPC, editing, publication or printing acceptance is granted.",
    chapter: Chapter::Design, effect: Effect::LocalFileWrite, authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::STATE_DIR_ARG, crate::LANE_ARG, crate::PROJECT_ARG.required(), Arg::value("request", "<json-file>", "Ordinary file at most 48 MiB containing ds.core.profile-session/v1; open binds at most 32 MiB exact package bytes and SHA-256, other actions bind the captured session/model/history.").required(), Arg::value("out", "<absolute-new-json-file>", "Optional fixed native result file, at most 72 MiB; output becomes a small path/size/SHA/scope-bound receipt. Existing files are never replaced.")],
    output: "ds.core.profile-receipt/v1 with session/model/history, exact package SHA/source revision, managed engine identity and opaque native artifact JSON plus additive rehydrated boolean (original-only worker recovery, never recovered authored edits/checkpoint). With --out, ds.core.profile-artifact/v1 binds the exact receipt bytes, size, SHA-256, native engine and full session scope. This first vertical slice does not expose unsupported operations.",
    examples: &[],
    refusals: &[
        Refusal {code:"core_profile_output_failed",when:"an admitted Profile action completed but the create-new output failed; detail names the completed action, exact session scope and whether its newly opened process was closed",remedy:"inspect completion and cleanup; reuse or close the exact retained session if cleanup was refused, never repeat an unknown effect blindly"},
        Refusal {code:"core_profile_native_refused",when:"managed native Network refuses the exact Profile contract or captured profile_read_projections_v1, profile_properties_v1 or profile_selection_v1 is absent before its corresponding readonly operation; detail retains native_code/remedy/detail",remedy:"retain the package and follow the named native refusal; do not substitute another engine or identity"},
        Refusal {code:"core_profile_request_invalid",when:"the closed schema, package bytes/digest or native package identity is invalid or exceeds bounds",remedy:"use the declared exact package/session request and preserve immutable source bytes"},
        Refusal {code:"core_profile_session_unavailable",when:"the handle has no exact eligible retained source, is closed or belongs to another account/lane/project/model",remedy:"explicitly reopen the exact package through the same Core owner"},
        Refusal {code:"core_profile_source_unavailable",when:"the protected SQLite original is missing, corrupt, incompatible, invalidated or cannot be retained within bounds",remedy:"preserve original model/history and follow the protected-store refusal; never substitute a package for authored or uncertain history"},
        Refusal {code:"core_profile_styles_unavailable",when:"a null View style demand cannot admit the same native UID/lane/audience/project authored ds_grid/profile document within 64 KiB, or its owner changes before adoption",remedy:"restore the intended project style through Style Center and retry intentionally; no default pens or alternate account are substituted"},
        Refusal {code:"core_profile_source_changed",when:"rehydration does not reproduce the exact original model/history/CRS/managed engine",remedy:"retain the original and explicitly resolve the changed source or component; no candidate worker is adopted"},
        Refusal {code:"core_profile_history_conflict",when:"the captured authored history differs from the retained head",remedy:"read the retained head before submitting the intended operation again"},
        Refusal {code:"core_profile_capacity",when:"Core already retains eight active Profile processes",remedy:"close an existing Profile session before opening another"},
        Refusal {code:"server_owner_changed",when:"the protected Core credential changed or no longer authorizes the owner",remedy:"sign in under the intended account and explicitly restart Core"},
        Refusal {code:"server_refused",when:"the same-lane protected Core cannot be reached or the native Network owner refuses",remedy:"start the same installed Core and read the native refusal; do not switch identity or weaken package fences"},
        Refusal {code:"module_removed",when:"Network was explicitly removed from this lane",remedy:"explicitly reinstall the admitted Network component in the same lane"},
        Refusal {code:"network_engine_missing",when:"no admitted managed Network executable is installed",remedy:"install the admitted Network component in the same lane"},
    ],
    reference: Some("docs/reference/core.md"), search: &["profile", "native", "dsgrid"],
    requires: Requires::Server, availability: crate::native_host_availability,
};

pub fn command(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    crate::require_native_host()?;
    let output = inputs.value("out").map(std::path::PathBuf::from);
    if let Some(path) = &output {
        validate_output(path)?;
    }
    let path = inputs.require("request")?;
    let mut file = read_request_file(std::path::Path::new(path))?;
    let metadata = file.metadata().map_err(crate::failure)?;
    if !metadata.is_file() || metadata.len() > MAX_REQUEST as u64 {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile request must be a bounded ordinary opened file",
        ));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(crate::failure)?;
    let parsed = Request::parse(&bytes)?;
    let action = match parsed.operation {
        Operation::Open { .. } => "open",
        Operation::Snapshot { .. } => "snapshot",
        Operation::State { .. } => "state",
        Operation::Checkpoint { .. } => "checkpoint",
        Operation::Close { .. } => "close",
        Operation::View { .. } => "view",
        Operation::Scene { .. } => "scene",
        Operation::GisProjection { .. } => "gis_projection",
        Operation::FeatureCodes { .. } => "feature_codes",
        Operation::Properties { .. } => "properties",
        Operation::Pick { .. } => "pick",
        Operation::SelectionRange { .. } => "selection_range",
        Operation::EngineeringAttributes { .. } => "engineering_attributes",
    };
    let project = crate::project(inputs)?;
    let response = crate::request_with_timeout(
        inputs,
        "POST",
        &crate::with_project("/v1/profile-session", &project),
        Some(&bytes),
        MAX_RESPONSE as u64,
        40,
    )?;
    let value: Value = serde_json::from_slice(&response).map_err(crate::failure)?;
    if value["schema"] != RECEIPT_SCHEMA
        || value["project"] != project
        || value["lane"] != inputs.require("lane")?
    {
        return Err(Failure::failed(
            "core_profile_request_invalid",
            "Core returned a crossed Profile receipt",
        ));
    }
    match output {
        Some(path) => finish_output(&path, &response, &value, action, &|| {
            let close = serde_json::to_vec(&serde_json::json!({"schema":crate::profile_sessions::REQUEST_SCHEMA,"operation":{"action":"close","session_id":value["session_id"],"model_id":value["model_id"],"expected_history":value["history"]}})).map_err(crate::failure)?;
            let bytes = crate::request_with_timeout(
                inputs,
                "POST",
                &crate::with_project("/v1/profile-session", &project),
                Some(&close),
                MAX_RESPONSE as u64,
                40,
            )?;
            let closed: Value = serde_json::from_slice(&bytes).map_err(crate::failure)?;
            Ok(closed["closed"] == true
                && closed["session_id"] == value["session_id"]
                && closed["project"] == project)
        }),
        None => Ok(value),
    }
}

fn finish_output(
    path: &std::path::Path,
    bytes: &[u8],
    value: &Value,
    action: &str,
    close_new: &dyn Fn() -> Result<bool, Failure>,
) -> Result<Value, Failure> {
    write_output(path,bytes,value).map_err(|cause| {
        let cleanup = if action == "open" { Some(close_new()) } else { None };
        let closed = cleanup.as_ref().is_some_and(|result| matches!(result,Ok(true)));
        let detail = serde_json::json!({"completed_action":action,"output_error":cause.message(),"new_open_closed":closed,
            "cleanup_error":cleanup.as_ref().and_then(|r|r.as_ref().err()).map(Failure::code),
            "profile":{"session_id":value["session_id"],"model_id":value["model_id"],"history":value["history"],"package_sha256":value["package_sha256"],"project":value["project"],"lane":value["lane"],"engine":value["engine"]}});
        Failure::failed("core_profile_output_failed","Profile action completed but its output file could not be admitted; inspect the scoped completion and cleanup receipt before retrying").detail(detail)
    })
}

fn validate_output(path: &std::path::Path) -> Result<(), Failure> {
    if !path.is_absolute() || std::fs::symlink_metadata(path).is_ok() {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile output must be a new absolute file; existing files are never replaced",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        Failure::invalid(
            "core_profile_request_invalid",
            "Profile output has no parent",
        )
    })?;
    let metadata = std::fs::symlink_metadata(parent).map_err(crate::failure)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile output parent must be an existing ordinary directory",
        ));
    }
    #[cfg(windows)]
    ds_cli_auth::windows_host::validate_directory(parent).map_err(crate::failure)?;
    Ok(())
}
fn write_output(path: &std::path::Path, bytes: &[u8], value: &Value) -> Result<Value, Failure> {
    validate_output(path)?;
    if bytes.len() > MAX_RESPONSE {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile output exceeds 72 MiB",
        ));
    }
    let mut metadata = value.clone();
    metadata
        .as_object_mut()
        .ok_or_else(|| {
            Failure::invalid(
                "core_profile_request_invalid",
                "Profile output has no receipt",
            )
        })?
        .remove("payload");
    let receipt = serde_json::json!({"schema":FILE_SCHEMA,"out":path,"byte_count":bytes.len(),"sha256":ds_compute_runtime::digest(bytes),"profile":metadata});
    if serde_json::to_vec(&receipt).map_err(crate::failure)?.len() > 64 * 1024 {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile metadata receipt exceeds 64 KiB",
        ));
    }
    // Same atomic create-new/no-clobber owner as core.result. Partial staging
    // stays with that owner; the caller's protected temporary room owns cleanup.
    ds_design_workspace::write_new(path, bytes).map_err(crate::failure)?;
    #[cfg(windows)]
    drop(ds_cli_auth::windows_host::read_file(path).map_err(crate::failure)?);
    Ok(receipt)
}

#[cfg(test)]
mod artifact_tests {
    use super::*;
    #[test]
    fn null_style_immediate_owner_guard_binds_connection_uid_lane_and_audience() {
        let owner = crate::auth::fence("fixture", "canary", &"a".repeat(64)).unwrap();
        require_style_owner(&owner, Ok(owner.clone())).unwrap();
        for changed in [
            crate::auth::fence("fixture", "canary", &"b".repeat(64)).unwrap(),
            crate::auth::fence("foreign", "canary", &"a".repeat(64)).unwrap(),
            crate::auth::fence("fixture", "stable", &"a".repeat(64)).unwrap(),
        ] {
            assert_eq!(
                require_style_owner(&owner, Ok(changed)).unwrap_err().code(),
                "server_owner_changed"
            );
        }
        assert_eq!(
            require_style_owner(&owner, Err("No native credential".into()))
                .unwrap_err()
                .code(),
            "server_owner_changed"
        );
    }
    #[test]
    fn file_handoff_keeps_exact_large_bytes_and_bounded_scope_receipt() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("profile.json");
        let value = serde_json::json!({"schema":RECEIPT_SCHEMA,"session_id":"one","project":"fixture-project","lane":"canary","history":{"model_revision":"rev:fixture"},"payload":"x".repeat(5*1024*1024)});
        let bytes = serde_json::to_vec(&value).unwrap();
        let receipt = write_output(&path, &bytes, &value).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(receipt["byte_count"], bytes.len());
        assert_eq!(receipt["sha256"], ds_compute_runtime::digest(&bytes));
        assert_eq!(receipt["profile"]["project"], "fixture-project");
        assert!(receipt["profile"].get("payload").is_none());
        assert!(serde_json::to_vec(&receipt).unwrap().len() < 4096);
        assert!(write_output(&path, b"replacement", &value).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn occupied_output_refuses_before_request_read_or_core_effect() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("occupied.json");
        std::fs::write(&path, b"original").unwrap();
        let inputs = ds_cli_contract::args::parse(
            &COMMAND,
            &[
                "--lane".into(),
                "canary".into(),
                "--project".into(),
                "fixture-project".into(),
                "--request".into(),
                root.path()
                    .join("nonexistent-request.json")
                    .display()
                    .to_string(),
                "--out".into(),
                path.display().to_string(),
            ],
        )
        .unwrap();
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::output::Output::resolve(
                ds_cli_contract::output::Format::Json,
                false,
                true,
            ),
        };
        let refusal = command(&inputs, &context).unwrap_err();
        assert!(
            refusal
                .message()
                .contains("output must be a new absolute file")
        );
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
    #[test]
    fn late_output_failure_closes_only_its_new_open_and_reports_completed_scope() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("occupied.json");
        std::fs::write(&path, b"original").unwrap();
        let value = serde_json::json!({"session_id":"captured","model_id":"fixture-model","project":"fixture-project","history":{"model_revision":"head"}});
        let called = std::cell::Cell::new(0);
        let error = finish_output(&path, b"bytes", &value, "open", &|| {
            called.set(called.get() + 1);
            Ok(true)
        })
        .unwrap_err();
        assert_eq!(called.get(), 1);
        assert_eq!(error.code(), "core_profile_output_failed");
        assert_eq!(error.detail_value().unwrap()["completed_action"], "open");
        assert_eq!(error.detail_value().unwrap()["new_open_closed"], true);
        let error = finish_output(&path, b"bytes", &value, "view", &|| {
            panic!("An existing session must not be closed by a failed view output")
        })
        .unwrap_err();
        assert_eq!(error.detail_value().unwrap()["completed_action"], "view");
        assert_eq!(
            error.detail_value().unwrap()["profile"]["session_id"],
            "captured"
        );
    }
    #[test]
    fn relative_or_linked_output_is_not_a_new_native_artifact() {
        assert!(validate_output(std::path::Path::new("relative.json")).is_err());
        #[cfg(unix)]
        {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("missing");
            let link = root.path().join("link");
            std::os::unix::fs::symlink(target, &link).unwrap();
            assert!(validate_output(&link).is_err());
        }
    }
}

fn read_request_file(path: &std::path::Path) -> Result<std::fs::File, Failure> {
    #[cfg(windows)]
    {
        return ds_cli_auth::windows_host::read_file(path)
            .map_err(|e| Failure::invalid("core_profile_request_invalid", e));
    }
    #[cfg(not(windows))]
    {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        options
            .open(path)
            .map_err(|e| Failure::invalid("core_profile_request_invalid", e.to_string()))
    }
}

fn require_style_owner(expected: &str, actual: Result<String, String>) -> Result<(), Failure> {
    let actual = actual.map_err(|error| Failure::unauthorized("server_owner_changed", error))?;
    if actual != expected {
        return Err(Failure::unauthorized(
            "server_owner_changed",
            "Profile style demand no longer belongs to this Core connection",
        ));
    }
    Ok(())
}

pub(crate) async fn invoke(
    State(app): State<App>,
    query: Option<Query<ProjectQuery>>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = crate::host::project_query(query)?;
    crate::host::admitting(move || {
        let request = Request::parse(&body)?;
        let context = app.sessions.admit_read(
            ds_command_kernel::execution_context::PROFILE_SESSION,
            project.as_deref(),
            &body,
            ds_compute_runtime::now_ms(),
        )?;
        let scope = Scope {
            principal: context.principal_uid,
            lane: context.lane,
            deployment: context.deployment,
            install: context.install_id,
            project: context.project,
        };
        let parent = app.database.parent().ok_or_else(|| {
            Failure::invalid(
                "core_profile_request_invalid",
                "Core has no protected state parent",
            )
        })?;
        let scratch = parent.join("profile-sessions");
        crate::host::prepare_directory(&scratch).map_err(crate::failure)?;
        let style_demand = matches!(&request.operation, Operation::View { style_document, .. } if style_document.is_null());
        let authorized = || {
            app.auth.authorize(&app.connection.owner)
                .map_err(|e| Failure::unauthorized("server_owner_changed", e))?;
            if style_demand {
                // Null style acquisition crosses the existing authenticated
                // metadata owner. Bind it immediately to this Core connection,
                // rather than its ordinary fifteen-second local observation.
                require_style_owner(&app.connection.owner, crate::auth::owner_fence(&app.connection.lane))?;
            }
            Ok(())
        };
        app.profiles
            .execute(scope, &scratch, &app.database, request, &authorized)
            .map(Json)
    })
    .await
}
