//! Closed CLI and protected-socket adapter for the retained native Profile owner.
use crate::{
    host::{App, ProjectQuery},
    profile_sessions::{Request, Scope, MAX_REQUEST, RECEIPT_SCHEMA},
};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use ds_cli_contract::{
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
    Context, Failure, Inputs,
};
use serde_json::Value;
use std::io::Read;

pub static COMMAND: Command = Command {
    id: "core.profile-session", path: &["core", "profile-session"], contract: 1,
    summary: "Open, read or close a retained native Grid Profile session.",
    purpose: "Keep one exact verified .dsgrid package's live Profile with the authenticated Core owner and its independently installed managed Network process. The closed request admits open, snapshot, state, scene, checkpoint or close only; handles are fenced to the current account/lane/deployment/install/project/model/history and expire at Core shutdown or bounded idle retirement. Scene executes only the existing native ProfileRuntimeScene protocol with exact history; cached mode never requests engineering calculation. No browser engine, arbitrary RPC, editing, publication or printing acceptance is granted.",
    chapter: Chapter::Design, effect: Effect::LocalFileWrite, authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::STATE_DIR_ARG, crate::LANE_ARG, crate::PROJECT_ARG.required(), Arg::value("request", "<json-file>", "Ordinary file at most 48 MiB containing ds.core.profile-session/v1; open binds at most 32 MiB exact package bytes and SHA-256, other actions bind the captured session/model/history.").required()],
    output: "ds.core.profile-receipt/v1 with session/model/history, exact package SHA/source revision, managed engine identity and opaque native artifact JSON. This first vertical slice does not expose unsupported operations.",
    examples: &[],
    refusals: &[
        Refusal {code:"core_profile_native_refused",when:"managed native Network refuses the exact Profile contract; detail retains native_code/remedy/detail",remedy:"retain the package and follow the named native refusal; do not substitute another engine or identity"},
        Refusal {code:"core_profile_request_invalid",when:"the closed schema, package bytes/digest or native package identity is invalid or exceeds bounds",remedy:"use the declared exact package/session request and preserve immutable source bytes"},
        Refusal {code:"core_profile_session_unavailable",when:"the handle is absent, expired, closed or belongs to another account/lane/project/model",remedy:"explicitly reopen the exact package through the same Core owner"},
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
    let path = inputs.require("request")?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| Failure::invalid("core_profile_request_invalid", e.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_REQUEST as u64
    {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile request must be a bounded ordinary file",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(crate::failure)?
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(crate::failure)?;
    Request::parse(&bytes)?;
    let project = crate::project(inputs)?;
    let response = crate::request_with_timeout(
        inputs,
        "POST",
        &crate::with_project("/v1/profile-session", &project),
        Some(&bytes),
        72 * 1024 * 1024,
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
    Ok(value)
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
        let authorized = || {
            app.auth
                .authorize(&app.connection.owner)
                .map_err(|e| Failure::unauthorized("server_owner_changed", e))
        };
        app.profiles
            .execute(scope, &scratch, request, &authorized)
            .map(Json)
    })
    .await
}
