//! Fixed Core lifecycle adapters. Core fences its durable captures; the
//! updater alone owns generation admission and atomic component selection.
use crate::core_components::{CHANNEL, COMPONENT, REFUSALS, RUNTIME, owner_failure, selected};
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::module_install::{ModuleSelection, validate_selection};
use ds_command_kernel::module_release::ModuleComponent;
use ds_update_runtime::modules::{ModuleStore, current_platform};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};

const ACTIVATE_ROUTE: &str = "/v1/core/component/activate";
const ROLLBACK_ROUTE: &str = "/v1/core/component/rollback";
const MAX_REQUEST_BYTES: usize = 512;
const GENERATION: Arg = Arg::value(
    "expected-generation",
    "<u64>",
    "Exact generation read through core component selection; zero when absent.",
)
.required();

pub static ACTIVATE: Command = Command {
    id: "core.component.activate",
    path: &["core", "component", "activate"],
    contract: 2,
    summary: "Activate one verified staged component through Core.",
    purpose: "Explicit runtime wasm activates an existing UI staging receipt locally under this OS account without login or a running Core, rechecking its captured generation and complete bytes. Native remains the default: confirm an existing native staging receipt at an explicitly held generation. The selected lane's running Core fences durable job admission and refuses uncaptured pending work. The shared updater recovers the exact receipt, verifies retained executable bytes and atomically advances selection under its component lock. Captured jobs retain their immutable executable. No download, release lookup, engine process, Desktop session, caller idle assertion or project calculation is involved.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        COMPONENT,
        CHANNEL,
        RUNTIME,
        Arg::value(
            "receipt-id",
            "<id>",
            "Exact ready receipt returned by core module-stage.",
        )
        .required(),
        GENERATION,
    ],
    output: "UI runtime wasm returns committed ds.ui-module-selection/v1. Native returns committed ds.module-selection/v1 installed receipt with incremented generation, active unit and retained previous unit when present.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["component", "addon", "activate", "staged", "install"],
    requires: Requires::Server,
    availability: crate::native_host_availability,
};
pub static ROLLBACK: Command = Command {
    id: "core.component.rollback",
    path: &["core", "component", "rollback"],
    contract: 2,
    summary: "Restore a verified retained previous component through Core.",
    purpose: "Explicit runtime wasm restores a retained verified UI unit locally under this OS account without login or a running Core. Native remains the default: confirm rollback of one native component at an explicitly held generation through the selected lane's running Core. The same durable job admission fence and shared updater selection lock apply. The updater verifies the retained previous bytes without network or process launch, advances the generation and preserves already captured jobs. An absent previous unit explicitly refuses; it never downloads a substitute.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[COMPONENT, CHANNEL, RUNTIME, GENERATION],
    output: "UI runtime wasm returns committed ds.ui-module-selection/v1. Native returns committed ds.module-selection/v1 receipt naming the verified previous unit as active and the incremented generation.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["component", "addon", "rollback", "recover"],
    requires: Requires::Server,
    availability: crate::native_host_availability,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActivateRequest {
    component: ModuleComponent,
    channel: String,
    expected_generation: u64,
    receipt_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RollbackRequest {
    component: ModuleComponent,
    channel: String,
    expected_generation: u64,
}

enum Request {
    Activate(ActivateRequest),
    Rollback(RollbackRequest),
}
impl Request {
    fn component(&self) -> ModuleComponent {
        match self {
            Self::Activate(r) => r.component,
            Self::Rollback(r) => r.component,
        }
    }
    fn channel(&self) -> &str {
        match self {
            Self::Activate(r) => &r.channel,
            Self::Rollback(r) => &r.channel,
        }
    }
    fn generation(&self) -> u64 {
        match self {
            Self::Activate(r) => r.expected_generation,
            Self::Rollback(r) => r.expected_generation,
        }
    }
    fn route(&self) -> &'static str {
        match self {
            Self::Activate(_) => ACTIVATE_ROUTE,
            Self::Rollback(_) => ROLLBACK_ROUTE,
        }
    }
    fn body(&self) -> Result<Vec<u8>, Failure> {
        match self {
            Self::Activate(r) => serde_json::to_vec(r),
            Self::Rollback(r) => serde_json::to_vec(r),
        }
        .map_err(|e| Failure::internal("module_result_invalid", e.to_string()))
    }
}
fn invalid(message: &str) -> Failure {
    owner_failure(format!("component_request_invalid: {message}"))
}
fn generation(inputs: &Inputs) -> Result<u64, Failure> {
    let raw = inputs.require("expected-generation")?;
    raw.parse::<u64>()
        .ok()
        .filter(|_| !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            owner_failure("component_generation_invalid: use an unsigned decimal generation".into())
        })
}
fn validate_request(request: &Request) -> Result<(), Failure> {
    if !matches!(request.channel(), "stable" | "canary") {
        return Err(invalid("use an exact stable or canary lane"));
    }
    if let Request::Activate(r) = request
        && (r.receipt_id.len() != 32
            || !r
                .receipt_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    {
        return Err(invalid(
            "name the exact 32-character lowercase native staging receipt",
        ));
    }
    Ok(())
}
fn from_inputs(inputs: &Inputs, activate: bool) -> Result<Request, Failure> {
    let (component, channel) = selected(inputs)?;
    let channel = channel.to_owned();
    let expected_generation = generation(inputs)?;
    let request = if activate {
        Request::Activate(ActivateRequest {
            component,
            channel,
            expected_generation,
            receipt_id: inputs.require("receipt-id")?.to_owned(),
        })
    } else {
        Request::Rollback(RollbackRequest {
            component,
            channel,
            expected_generation,
        })
    };
    validate_request(&request)?;
    Ok(request)
}
pub fn activate(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::ui_components::requested(inputs)? {
        return crate::ui_components::activate(inputs);
    }
    crate::require_native_host()?;
    request_runtime(&from_inputs(inputs, true)?)
}
pub fn rollback(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::ui_components::requested(inputs)? {
        return crate::ui_components::rollback(inputs);
    }
    crate::require_native_host()?;
    request_runtime(&from_inputs(inputs, false)?)
}
fn unavailable(message: impl std::fmt::Display) -> Failure {
    owner_failure(format!("component_runtime_unavailable: {message}"))
}
fn request_runtime(request: &Request) -> Result<Value, Failure> {
    let state =
        ds_compute_runtime::server_state_directory(request.channel(), None).map_err(unavailable)?;
    let connection = crate::host::load_connection(&state).map_err(unavailable)?;
    if connection.lane != request.channel() {
        return Err(owner_failure(
            "component_runtime_identity_mismatch: protected Core lane differs".into(),
        ));
    }
    if connection.legacy_address.is_some() {
        return Err(unavailable("Core connection predates owner-only transport"));
    }
    let reply = crate::transport::call(
        &connection.socket,
        "POST",
        request.route(),
        &[("content-type", "application/json")],
        Some(&request.body()?),
        ds_command_kernel::module_install::MAX_SELECTION_BYTES as u64,
        Duration::from_secs(10),
    )
    .map_err(unavailable)?;
    decode_reply(reply, request)
}
fn decode_reply(reply: crate::transport::Reply, request: &Request) -> Result<Value, Failure> {
    let value: Value = serde_json::from_slice(&reply.body).map_err(unavailable)?;
    if reply.status >= 400 {
        let code = value["code"].as_str().unwrap_or_default();
        return if REFUSALS.iter().any(|r| r.code == code) {
            Err(owner_failure(format!(
                "{code}: {}",
                value["error"]
                    .as_str()
                    .unwrap_or("Core refused component lifecycle")
            )))
        } else {
            Err(unavailable("Core does not support this lifecycle request"))
        };
    }
    let selection: ModuleSelection = serde_json::from_value(value).map_err(|_| {
        owner_failure("module_receipt_invalid: Core returned malformed installed selection".into())
    })?;
    validate_selection(
        &selection,
        request.component(),
        request.channel(),
        current_platform().map_err(owner_failure)?,
    )
    .map_err(owner_failure)?;
    if Some(selection.generation) != request.generation().checked_add(1) {
        return Err(owner_failure(
            "module_receipt_invalid: Core returned a crossed selection generation".into(),
        ));
    }
    serde_json::to_value(selection)
        .map_err(|e| Failure::internal("module_result_invalid", e.to_string()))
}
fn decode_request(body: &[u8], activate: bool) -> Result<Request, Failure> {
    if body.len() > MAX_REQUEST_BYTES {
        return Err(invalid("lifecycle request exceeds 512 bytes"));
    }
    let request = if activate {
        Request::Activate(
            serde_json::from_slice(body)
                .map_err(|_| invalid("use the closed activation request"))?,
        )
    } else {
        Request::Rollback(
            serde_json::from_slice(body).map_err(|_| invalid("use the closed rollback request"))?,
        )
    };
    validate_request(&request)?;
    Ok(request)
}
pub(crate) async fn invoke_activate(
    app: axum::extract::State<crate::host::App>,
    request: axum::extract::Request,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    invoke(app, request, true).await
}
pub(crate) async fn invoke_rollback(
    app: axum::extract::State<crate::host::App>,
    request: axum::extract::Request,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    invoke(app, request, false).await
}
async fn invoke(
    axum::extract::State(app): axum::extract::State<crate::host::App>,
    request: axum::extract::Request,
    activate: bool,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    let body = axum::body::to_bytes(request.into_body(), MAX_REQUEST_BYTES)
        .await
        .map_err(|_| crate::host::typed(&invalid("lifecycle request exceeds 512 bytes")))?;
    let request = decode_request(&body, activate).map_err(|e| crate::host::typed(&e))?;
    crate::host::admitting(move || {
        let identity = app.sessions.identity();
        if identity.owner != app.connection.owner || identity.lane() != app.connection.lane { return Err(owner_failure("component_runtime_identity_mismatch: Core identity differs from protected connection".into())); }
        execute(&app.database, identity, request, ModuleStore::managed).map(axum::Json)
    }).await
}
fn execute(
    database: &Path,
    identity: &ds_compute_runtime::HostIdentity,
    request: Request,
    open: impl FnOnce(ModuleComponent, &str) -> Result<ModuleStore, String>,
) -> Result<Value, Failure> {
    if request.channel() != identity.lane() {
        return Err(owner_failure(
            "component_runtime_identity_mismatch: requested component lane differs from Core lane"
                .into(),
        ));
    }
    let selection = ds_compute_runtime::with_component_removal_admission(
        database,
        identity,
        request.component(),
        |guard| {
            let store = open(request.component(), request.channel())?;
            match &request {
                Request::Activate(r) => store.activate_at_generation(
                    &store.recover_stage(&r.receipt_id)?,
                    r.expected_generation,
                    guard,
                    &|| false,
                ),
                Request::Rollback(r) => store.rollback(r.expected_generation, guard, &|| false),
            }
        },
    )
    .map_err(owner_failure)?;
    serde_json::to_value(selection)
        .map_err(|e| Failure::internal("module_result_invalid", e.to_string()))
}

#[cfg(test)]
mod tests;
