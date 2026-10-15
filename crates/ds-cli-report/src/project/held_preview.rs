//! Exact caller-held transformer preview. No report input or room is fetched.
use super::{CliEngine, host_failure, preview_failure};
use ds_cli_contract::{Failure, Inputs};
use ds_command_kernel::{
    printing::{
        self,
        context::RetainedContext,
        held_context::HeldCapture,
        preview::{RoomFacts, room_admission},
    },
    report_export::{INPUT_RECEIPT_MEMBER, InputReceipt},
};
use ds_report_host::{
    HeldContext, HeldRoom, HeldState, Holdings, PreviewRequest, PreviewSettings, preview_artifact,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, io::Read, path::PathBuf};
const SCHEMA: &str = "ds.report-held-preview/v1";
const MAX_BYTES: u64 = 128 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: String,
    uid: String,
    project: String,
    lane: String,
    credential_audience_sha256: String,
    args: Map<String, Value>,
    held: HeldRows,
    media_grant_document: Option<Vec<u8>>,
    project_crs: Option<printing::project_crs::Capture>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeldRows {
    style_capture: Option<Value>,
    receipt: Value,
    room: Option<RoomRow>,
    media_grant: Option<Value>,
    captures: Vec<HeldCapture>,
    retained: BTreeMap<String, RetainedContext>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomRow {
    source: String,
    transformer_name: String,
    updated_at: Option<f64>,
    layers: BTreeMap<String, Value>,
    local_dirty: Option<bool>,
    server_version: Option<f64>,
}
struct Admitted {
    payload: Payload,
    request: PreviewRequest,
    receipt: InputReceipt,
    room: HeldRoom,
    style: Option<Value>,
    retention: preview_artifact::Retention,
    output: Option<PathBuf>,
}
fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid("report_held_preview_invalid",message.to_string()).remedy("Preserve the exact held receipt, room, styles and context; correct the named field without reacquiring inputs")
}
fn conflict(message: impl Into<String>) -> Failure {
    Failure::conflict("project_context_changed", message)
}
fn load(path: &str) -> Result<Payload, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("held preview exceeds 128 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))
}
fn admit(payload: Payload, project: &str, lane: &str) -> Result<Admitted, Failure> {
    if payload.schema != SCHEMA
        || payload.project != project
        || payload.lane != lane
        || payload.uid.is_empty()
        || payload.credential_audience_sha256.is_empty()
        || !ds_command_kernel::execution_context::valid_project(project)
    {
        return Err(conflict(
            "Held preview scope differs from the explicit native project/lane",
        ));
    }
    if payload.args.keys().any(|k| {
        !matches!(
            k.as_str(),
            "transformer" | "preview-layout" | "preview-retention" | "preview-out"
        )
    }) {
        return Err(invalid("held preview has undeclared controls"));
    }
    if let Some(crs) = &payload.project_crs {
        crs.validate_scope(
            project,
            lane,
            &payload.uid,
            &payload.credential_audience_sha256,
            None,
        )
        .map_err(conflict)?;
    }
    let names = payload.args["transformer"]
        .as_array()
        .ok_or_else(|| invalid("name exactly one held transformer"))?;
    let transformer = match names.as_slice() {
        [Value::String(s)] if !s.trim().is_empty() => s.clone(),
        _ => return Err(invalid("name exactly one held transformer")),
    };
    let raw = &payload.args["preview-layout"];
    if !raw.is_object()
        || serde_json::to_vec(raw)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > printing::MAX_LAYOUT_BYTES
    {
        return Err(invalid("bounded inline preview-layout required"));
    }
    let layout: printing::Layout =
        serde_json::from_value(raw.clone()).map_err(|e| invalid(e.to_string()))?;
    let receipt = InputReceipt::from_config(&json!({INPUT_RECEIPT_MEMBER:payload.held.receipt}))
        .map_err(invalid)?;
    let style = payload
        .held
        .style_capture
        .as_ref()
        .map(|v| {
            ds_command_kernel::style_resolution::admit_print_capture(project, &layout.style_refs, v)
                .map_err(|e| Failure::invalid(e.code, e.message))
        })
        .transpose()?;
    let row = payload.held.room.as_ref();
    let facts = row
        .map(|r| RoomFacts {
            present: true,
            source: Some(r.source.clone()),
            transformer_name: Some(r.transformer_name.clone()),
            layer_count: r.layers.len(),
            local_dirty: r.local_dirty.unwrap_or(false),
            server_version: r.server_version,
            updated_at: r.updated_at,
        })
        .unwrap_or_default();
    let admitted = room_admission(&transformer, &facts)
        .map_err(|r| preview_failure(ds_report_host::PreviewRefusal::Kernel(r)))?;
    // The Desktop's existing pure grant encoder supplies exactly one engine
    // document; the engine remains the owner of grant semantics/validation.
    match (&payload.held.media_grant, &payload.media_grant_document) {
        (None, None) => {}
        (Some(raw), Some(bytes)) => {
            if bytes.len() as u64 > ds_report_host::MAX_MEDIA_GRANT_BYTES {
                return Err(invalid("media grant exceeds owner bound"));
            }
            let keys = [
                ("resolverOrigin", "resolver_origin"),
                ("bucket", "bucket"),
                ("prefix", "prefix"),
                ("token", "token"),
                ("expiresAt", "expires_at"),
            ];
            if raw.as_object().is_none_or(|m| m.len() != keys.len()) {
                return Err(invalid("closed original media grant required"));
            }
            let mut grant = Map::new();
            for (from, to) in keys {
                let v = raw[from]
                    .as_str()
                    .ok_or_else(|| invalid("media grant must retain its original string fields"))?;
                grant.insert(to.into(), json!(v.trim()));
            }
            let encoded: Value = serde_json::from_slice(bytes)
                .map_err(|_| invalid("malformed engine media grant document"))?;
            if encoded != json!({"grants":[grant]}) {
                return Err(invalid("media grant document differs from held grant"));
            }
        }
        _ => {
            return Err(invalid(
                "media grant and encoded document must be retained together",
            ));
        }
    }
    let retention = payload
        .args
        .get("preview-retention")
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|_| invalid("preview-retention must be 24h or 7d"))
        })
        .transpose()?
        .unwrap_or_default();
    let output = payload
        .args
        .get("preview-out")
        .map(|v| {
            preview_artifact::admit_output(
                v.as_str()
                    .ok_or_else(|| invalid("preview-out must be a fresh absolute SVG path"))?,
            )
            .map_err(host_failure)
        })
        .transpose()?;
    let room = HeldRoom {
        transformer: transformer.clone(),
        server_version: admitted.server_version,
        layers: row.map(|r| r.layers.clone()).unwrap_or_default(),
        content_digest: None,
        media_grant: payload.media_grant_document.clone(),
        sheet: None,
    };
    Ok(Admitted {
        payload,
        request: PreviewRequest {
            transformer,
            layout,
        },
        receipt,
        room,
        style,
        retention,
        output,
    })
}
fn scratch_path(inputs: &Inputs) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(inputs.require("out-dir")?);
    if !path.is_absolute() || std::fs::symlink_metadata(&path).is_ok() {
        return Err(invalid(
            "held-preview out-dir must be a fresh absolute private scratch directory",
        ));
    }
    Ok(path)
}
pub(super) fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    scratch_path(inputs)?;
    for arg in super::COMMAND.args {
        if !matches!(arg.name, "held-preview" | "project" | "lane" | "out-dir")
            && (inputs.switch(arg.name)
                || !inputs.repeated(arg.name).is_empty()
                || inputs
                    .value(arg.name)
                    .is_some_and(|value| Some(value) != arg.default))
        {
            return Err(invalid(format!(
                "--{} cannot be combined with --held-preview",
                arg.name
            )));
        }
    }
    admit(
        load(inputs.require("held-preview")?)?,
        inputs.require("project")?,
        inputs.require("lane")?,
    )?;
    Ok(())
}
fn authority(
    payload: &Payload,
    uid: &str,
    lane: &str,
    audience: &str,
    visible: bool,
) -> Result<(), Failure> {
    if payload.uid != uid
        || payload.lane != lane
        || payload.credential_audience_sha256 != audience
        || !visible
    {
        return Err(Failure::unauthorized(
            "auth_context_mismatch",
            "Core account, lane, audience or visible project differs from the exact held owner-window scope",
        ));
    }
    Ok(())
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}
pub(super) fn run(inputs: &Inputs) -> Result<Value, Failure> {
    let a = admit(
        load(inputs.require("held-preview")?)?,
        inputs.require("project")?,
        inputs.require("lane")?,
    )?;
    // One intentional native authority read, never a config/room/style refresh.
    let directory = ds_cli_auth::project_directory(&a.payload.lane)?;
    let identity = directory.identity();
    authority(
        &a.payload,
        identity.uid(),
        identity.lane(),
        identity.credential_audience_sha256(),
        directory
            .projects()
            .iter()
            .any(|p| p["ds_project"] == a.payload.project),
    )?;
    let mut crs = a.payload.project_crs.clone();
    let config: Value =
        serde_json::from_str(&a.receipt.sheets_json).map_err(|e| invalid(e.to_string()))?;
    let policy = printing::map_policy::resolve(&config).map_err(invalid)?;
    if policy.metric_grid_enabled
        && a.request
            .layout
            .elements
            .iter()
            .any(|e| e.kind == printing::ElementKind::Map)
        || crs.is_some()
    {
        let params = directory
            .project_params(&a.payload.project)
            .ok_or_else(|| {
                Failure::unavailable(
                    "print_project_crs_missing",
                    "Core has no declared projection for the exact project",
                )
            })?;
        let authority = printing::project_crs::Capture::new(
            &a.payload.project,
            &a.payload.lane,
            identity.uid(),
            identity.credential_audience_sha256(),
            params.clone(),
        )
        .map_err(invalid)?;
        if let Some(held) = &crs {
            held.validate_scope(
                &a.payload.project,
                &a.payload.lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                Some(&authority.source_sha256),
            )
            .map_err(conflict)?;
        } else {
            crs = Some(authority);
        }
    }
    let current = || {
        let now = ds_cli_auth::headless_identity_for_named_project(&a.payload.lane)?;
        super::require_same_context(identity, &a.payload.project, &now, &a.payload.project)
            .map_err(host_failure)
    };
    current()?;
    let root = ds_layer_store::default_root().map_err(invalid)?;
    let scope = ds_command_kernel::project_dataset_cache::Scope {
        principal: identity.uid().into(),
        project: a.payload.project.clone(),
    };
    let result_room = ds_project_data::preview_room::Room {
        root,
        scope: scope.clone(),
        lane: a.payload.lane.clone(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    let staging = scratch_path(inputs)?;
    let admin = if a.receipt.requires_admin_bounds() {
        let root = ds_report_host::shared_root().map_err(invalid)?;
        let path = ds_report_host::installed_admin_bounds_path(
            &root,
            &a.receipt.reference_semantic_sha256,
        );
        Some(
            ds_report_host::verify_admin_bounds_asset(&path, &a.receipt.reference_semantic_sha256)
                .map_err(host_failure)?,
        )
    } else {
        None
    };
    let holdings = ds_layer_store::project_dataset_cache::default_root()
        .ok()
        .map(|root| Holdings {
            catalog: ds_project_data::held::held_catalog_rooms(&root, &scope),
            room_root: Some(result_room.root.clone()),
            scope,
            root,
        });
    let engine = CliEngine::capture(&a.payload.lane).map_err(host_failure)?;
    std::fs::create_dir(&staging).map_err(|e| invalid(e.to_string()))?;
    let _scratch = Scratch(staging.clone());
    ds_layer_store::private::create_dir_all(&staging).map_err(invalid)?;
    let settings = PreviewSettings {
        project_id: &a.payload.project,
        project_crs: crs.as_ref(),
        receipt: &a.receipt,
        admin_bounds: admin.as_ref(),
        staging_root: &staging,
    };
    let held = HeldState {
        room: a.room,
        context: HeldContext {
            captures: a.payload.held.captures,
            retained: a.payload.held.retained,
            holdings,
        },
    };
    current()?;
    let rendered = ds_report_host::execute_artifact_retained(
        &engine,
        &settings,
        &a.request,
        &held,
        a.style.as_ref(),
        &result_room,
        a.retention,
    )
    .map_err(preview_failure);
    let (page, artifact) = rendered?;
    current().map_err(|failure| failure.detail(json!({"retained":true,"artifact":artifact})))?;
    let mut answer =
        ds_report_host::answer_artifact(&a.payload.project, &a.payload.lane, &page, &artifact)
            .map_err(host_failure)?;
    if let Some(out) = a.output {
        current()?;
        let path = preview_artifact::copy(
            &result_room,
            &artifact,
            out.to_str()
                .ok_or_else(|| invalid("output path is not UTF-8"))?,
        )
        .map_err(host_failure)?;
        current().map_err(|failure| {
            failure.detail(json!({"retained":true,"out":path,"artifact":artifact}))
        })?;
        answer["results"][0]["out"] = json!(path);
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn payload() -> Value {
        let sheets = "{\"project_settings\":[],\"held_marker\":{\"spaces\":\"preserve  these\"}}";
        json!({"schema":SCHEMA,"uid":"owner","project":"project-a","lane":"canary","credential_audience_sha256":"audience",
            "args":{"transformer":["held-tr"],"preview-layout":printing::default_layout(),"preview-retention":"7d"},
            "held":{"style_capture":null,"receipt":{"schema":1,"country":"Burundi","sheets_json":sheets,"sheets_sha256":format!("{:x}",Sha256::digest(sheets.as_bytes())),"reference_semantic_sha256":"7".repeat(64)},"room":{"source":"lv","transformer_name":"held-tr","updated_at":1234.0,"layers":{"held-layer":{"type":"FeatureCollection","features":[],"marker":"held-only"}},"local_dirty":false,"server_version":17.0},"media_grant":null,"captures":[],"retained":{}},
            "media_grant_document":null,"project_crs":null})
    }
    fn admission(value: Value) -> Result<Admitted, Failure> {
        admit(
            serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?,
            "project-a",
            "canary",
        )
    }
    #[test]
    fn retained_input_bytes_room_revision_and_layout_are_not_reacquired_or_substituted() {
        let raw = payload();
        let admitted = admission(raw.clone()).unwrap();
        assert_eq!(
            admitted.receipt.sheets_json,
            raw["held"]["receipt"]["sheets_json"].as_str().unwrap()
        );
        assert_eq!(
            admitted.receipt.sheets_sha256,
            raw["held"]["receipt"]["sheets_sha256"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::to_value(admitted.room.layers).unwrap(),
            raw["held"]["room"]["layers"]
        );
        assert_eq!(admitted.room.server_version, 17);
        assert_eq!(admitted.request.transformer, "held-tr");
        assert_eq!(
            serde_json::to_value(admitted.request.layout).unwrap(),
            raw["args"]["preview-layout"]
        );
        assert_eq!(serde_json::to_value(admitted.retention).unwrap(), "7d");
    }
    #[test]
    fn crossed_scope_changed_receipt_dirty_room_and_undeclared_controls_refuse() {
        for mutation in [
            "project", "lane", "receipt", "dirty", "room", "controls", "style",
        ] {
            let mut p = payload();
            match mutation {
                "project" => p["project"] = json!("other"),
                "lane" => p["lane"] = json!("stable"),
                "receipt" => p["held"]["receipt"]["sheets_json"] = json!("{}"),
                "dirty" => {
                    p["held"]["room"]["local_dirty"] = json!(true);
                    p["held"]["room"]["layers"] = json!({});
                }
                "room" => p["held"]["room"]["transformer_name"] = json!("other"),
                "controls" => p["args"]["seed"] = json!(true),
                "style" => p["held"]["style_capture"] = json!({"borrowed":"style"}),
                _ => unreachable!(),
            }
            assert!(admission(p).is_err(), "{mutation}");
        }
    }
    #[test]
    fn media_document_must_match_exact_held_grant_and_is_never_logged() {
        let mut p = payload();
        let raw = json!({"resolverOrigin":"https://example.test","bucket":"fixture","prefix":"projects/project-a/","token":"fixture.signature","expiresAt":"2030-01-01"});
        let bytes=serde_json::to_vec(&json!({"grants":[{"resolver_origin":raw["resolverOrigin"],"bucket":raw["bucket"],"prefix":raw["prefix"],"token":raw["token"],"expires_at":raw["expiresAt"]}]})).unwrap();
        p["held"]["media_grant"] = raw;
        p["media_grant_document"] = json!(bytes);
        assert_eq!(admission(p.clone()).unwrap().room.media_grant, Some(bytes));
        p["held"]["media_grant"]["prefix"] = json!("other/");
        assert!(admission(p).is_err());
    }
    #[test]
    fn mixed_controls_and_existing_scratch_refuse_before_native_authority() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("held.json");
        std::fs::write(&input, serde_json::to_vec(&payload()).unwrap()).unwrap();
        let args = vec![
            "--held-preview".into(),
            input.to_str().unwrap().into(),
            "--out-dir".into(),
            root.path().join("fresh").to_str().unwrap().into(),
            "--project".into(),
            "project-a".into(),
            "--lane".into(),
            "canary".into(),
        ];
        let parse = |tokens: &[String]| {
            ds_cli_contract::args::parse(&super::super::COMMAND, tokens).unwrap()
        };
        preflight(&parse(&args)).unwrap();
        for flag in ["--seed", "--force", "--dry-run"] {
            let mut mixed = args.clone();
            mixed.push(flag.into());
            assert_eq!(
                preflight(&parse(&mixed)).unwrap_err().code(),
                "report_held_preview_invalid"
            );
        }
        std::fs::create_dir(root.path().join("fresh")).unwrap();
        assert_eq!(
            preflight(&parse(&args)).unwrap_err().code(),
            "report_held_preview_invalid"
        );
    }

    #[test]
    fn native_account_scope_must_match_before_any_engine_is_selected() {
        let p: Payload = serde_json::from_value(payload()).unwrap();
        authority(&p, "owner", "canary", "audience", true).unwrap();
        for (uid, lane, audience, visible) in [
            ("other", "canary", "audience", true),
            ("owner", "stable", "audience", true),
            ("owner", "canary", "other", true),
            ("owner", "canary", "audience", false),
        ] {
            assert_eq!(
                authority(&p, uid, lane, audience, visible)
                    .unwrap_err()
                    .code(),
                "auth_context_mismatch"
            );
        }
    }

    #[test]
    fn explicit_crs_capture_retains_exact_fields_for_native_scope_validation() {
        let mut p = payload();
        let capture = printing::project_crs::Capture::new(
            "project-a",
            "canary",
            "owner",
            "audience",
            json!({"crs":{"mode":"utm","utm_zone":36,"utm_hemisphere":"south"}}),
        )
        .unwrap();
        p["project_crs"] = serde_json::to_value(&capture).unwrap();
        assert_eq!(admission(p).unwrap().payload.project_crs, Some(capture));
    }
}
